use std::time::Duration;

use ksni::{MenuItem, TrayMethods, menu::StandardItem};
use tokio::sync::mpsc;

use crate::{DesktopAction, Error};

const TRAY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Tray {
    handle: ksni::Handle<TrayState>,
}

impl Tray {
    /// Keep the SNI service alive even if the panel has not started yet.
    /// ksni tracks the watcher owner and automatically registers again.
    pub async fn start(actions: mpsc::Sender<DesktopAction>) -> Result<Self, Error> {
        let state = TrayState {
            actions,
            statuses: vec!["Connecting to OpenRazer…".into()],
        };
        let handle = tokio::time::timeout(TRAY_TIMEOUT, state.assume_sni_available(true).spawn())
            .await
            .map_err(|_| Error::Timeout("tray startup"))??;
        Ok(Self { handle })
    }

    pub async fn update(&self, statuses: Vec<String>) -> Result<(), Error> {
        let updated = tokio::time::timeout(
            TRAY_TIMEOUT,
            self.handle.update(move |state| state.statuses = statuses),
        )
        .await
        .map_err(|_| Error::Timeout("tray update"))?;
        updated.ok_or(Error::TrayStopped)
    }

    pub async fn shutdown(&self) {
        let _ = tokio::time::timeout(TRAY_TIMEOUT, self.handle.shutdown()).await;
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Sending Shutdown is immediate; graceful callers await shutdown first.
        drop(self.handle.shutdown());
    }
}

struct TrayState {
    actions: mpsc::Sender<DesktopAction>,
    statuses: Vec<String>,
}

impl TrayState {
    fn send(&self, action: DesktopAction) {
        if let Err(error) = self.actions.try_send(action) {
            tracing::warn!(%error, ?action, "tray action could not be queued");
        }
    }

    fn item(label: &str, action: DesktopAction) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |state: &mut Self| state.send(action)),
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for TrayState {
    fn id(&self) -> String {
        "razerr-rs".into()
    }

    fn title(&self) -> String {
        "razerr-rs".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn icon_name(&self) -> String {
        // A standard theme icon also works when running from the source tree.
        "input-mouse".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: self.icon_name(),
            title: self.title(),
            description: self
                .statuses
                .iter()
                .map(|text| escape_markup(text))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(DesktopAction::Open);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.send(DesktopAction::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut menu = vec![
            Self::item("Open razerr-rs", DesktopAction::Open),
            Self::item("Settings…", DesktopAction::Settings),
            MenuItem::Separator,
        ];
        menu.extend(self.statuses.iter().map(|label| {
            StandardItem {
                label: label.clone(),
                enabled: false,
                ..Default::default()
            }
            .into()
        }));
        menu.extend([
            MenuItem::Separator,
            Self::item("Refresh devices", DesktopAction::Refresh),
            Self::item("Stop background service", DesktopAction::Stop),
            Self::item("Quit razerr-rs", DesktopAction::Quit),
        ]);
        menu
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        tracing::debug!(?reason, "tray panel offline; waiting for its return");
        true
    }

    fn watcher_online(&self) {
        tracing::debug!("tray panel available again");
    }
}

pub(crate) fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

    #[test]
    fn status_labels_are_not_markup_and_activation_is_queued() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut state = TrayState {
            actions: sender,
            statuses: vec!["Mouse <USB> & charging".into()],
        };
        assert_eq!(
            state.tool_tip().description,
            "Mouse &lt;USB&gt; &amp; charging"
        );
        state.activate(0, 0);
        assert_eq!(receiver.try_recv(), Ok(DesktopAction::Open));
        assert!(state.watcher_offline(ksni::OfflineReason::No));
    }
}
