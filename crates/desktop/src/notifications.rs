use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use futures_util::StreamExt;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use zbus::{Connection, wire::Variant};

use crate::{DesktopAction, Error, tray::escape_markup};

const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ACTIVE: usize = 32;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait NotificationServer {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;
    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

#[derive(Clone, Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub category: String,
    pub critical: bool,
}

/// Notification transport only. The service owns alert policy and hysteresis.
pub struct Notifications {
    connection: Connection,
    active: Arc<Mutex<VecDeque<u32>>>,
    listener: JoinHandle<()>,
}

impl Notifications {
    pub async fn connect(actions: mpsc::Sender<DesktopAction>) -> Result<Self, Error> {
        tokio::time::timeout(TIMEOUT, Self::connect_inner(actions))
            .await
            .map_err(|_| Error::Timeout("notification connection"))?
    }

    async fn connect_inner(actions: mpsc::Sender<DesktopAction>) -> Result<Self, Error> {
        let connection = zbus::connection::Builder::session()
            .method_timeout(TIMEOUT)
            .build()
            .await?;
        let proxy = NotificationServerProxy::new(&connection).await?;
        proxy.get_server_information().await?;
        // Subscribe before the caller can show anything, avoiding an action race.
        let mut invoked = proxy.receive_action_invoked().await?;
        let mut closed = proxy.receive_notification_closed().await?;
        let bus = zbus::fdo::DBusProxy::new(&connection).await?;
        let mut owners = bus.receive_name_owner_changed().await?;
        let active = Arc::new(Mutex::new(VecDeque::new()));
        let tracked = active.clone();
        let listener = tokio::spawn(async move {
            loop {
                tokio::select! {
                    signal = invoked.next() => {
                        let Some(signal) = signal else { break };
                        let Ok(args) = signal.args() else { continue };
                        let mut active = tracked.lock().await;
                        if take_action(&mut active, args.id, args.action_key)
                            && let Err(error) = actions.try_send(DesktopAction::Open) {
                            tracing::warn!(%error, "notification activation could not be queued");
                        }
                    }
                    signal = closed.next() => {
                        let Some(signal) = signal else { break };
                        if let Ok(args) = signal.args() {
                            tracked.lock().await.retain(|id| *id != args.id);
                        }
                    }
                    signal = owners.next() => {
                        let Some(signal) = signal else { break };
                        if let Ok(args) = signal.args()
                            && args.name.as_str() == "org.freedesktop.Notifications" {
                            // IDs belong to an owner, and may be reused after a restart.
                            tracked.lock().await.clear();
                        }
                    }
                }
            }
        });
        Ok(Self {
            connection,
            active,
            listener,
        })
    }

    pub async fn show(&self, notification: &Notification) -> Result<u32, Error> {
        tokio::time::timeout(TIMEOUT, self.show_inner(notification))
            .await
            .map_err(|_| Error::Timeout("notification delivery"))?
    }

    async fn show_inner(&self, notification: &Notification) -> Result<u32, Error> {
        let proxy = NotificationServerProxy::new(&self.connection).await?;
        let hints = HashMap::from([
            ("category", Variant::from(notification.category.as_str())),
            (
                "urgency",
                Variant::from(if notification.critical { 2u8 } else { 1u8 }),
            ),
            ("desktop-entry", Variant::from("org.razerr_rs.App")),
        ]);
        // Serialize delivery and registration with signal handling. A very quick
        // click cannot arrive before the returned ID has entered our own set.
        let mut active = self.active.lock().await;
        let id = proxy
            .notify(
                "razerr-rs",
                0,
                "input-mouse",
                &notification.title,
                &escape_markup(&notification.body),
                &["default", "Open razerr-rs"],
                hints,
                if notification.critical { -1 } else { 6000 },
            )
            .await?;
        active.push_back(id);
        let oldest = if active.len() > MAX_ACTIVE {
            active.pop_front()
        } else {
            None
        };
        drop(active);
        if let Some(oldest) = oldest
            && let Err(error) = proxy.close_notification(oldest).await
        {
            tracing::debug!(%error, "old notification already closed");
        }
        Ok(id)
    }

    pub async fn shutdown(&self) {
        self.listener.abort();
        let active: Vec<_> = self.active.lock().await.drain(..).collect();
        let close = async {
            if let Ok(proxy) = NotificationServerProxy::new(&self.connection).await {
                for id in active {
                    let _ = proxy.close_notification(id).await;
                }
            }
        };
        let _ = tokio::time::timeout(TIMEOUT, close).await;
    }
}

impl Drop for Notifications {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

fn take_action(active: &mut VecDeque<u32>, id: u32, action_key: &str) -> bool {
    if action_key != "default" || !active.contains(&id) {
        return false;
    }
    active.retain(|candidate| *candidate != id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_our_default_action_opens_once() {
        let mut active = VecDeque::from([1, 2]);
        assert!(!take_action(&mut active, 3, "default"));
        assert!(!take_action(&mut active, 1, "other"));
        assert!(take_action(&mut active, 1, "default"));
        assert!(!take_action(&mut active, 1, "default"));
        assert_eq!(active, VecDeque::from([2]));
    }
}
