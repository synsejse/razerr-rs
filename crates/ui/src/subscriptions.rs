use std::{
    hash::{Hash, Hasher},
    num::NonZeroUsize,
    sync::Arc,
    time::Duration,
};

use iced::{
    Subscription,
    futures::{SinkExt, Stream, StreamExt, channel::mpsc as mpsc_futures},
    stream,
};
use razerr_ipc::{Error, SERVICE_NAME};
use razerr_model::Snapshot;
use tokio::sync::{Mutex, mpsc};
use zbus::{MatchRule, MessageStream, message::Type};

use crate::{Activation, app::Message};

#[derive(Clone)]
pub struct Activations(Arc<Mutex<mpsc::Receiver<Activation>>>);

impl Activations {
    pub fn new(receiver: mpsc::Receiver<Activation>) -> Self {
        Self(Arc::new(Mutex::new(receiver)))
    }
}

impl Hash for Activations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

pub fn activation_subscription(channel: Activations) -> Subscription<Message> {
    Subscription::run_with(channel, activations)
}

fn activations(channel: &Activations) -> impl Stream<Item = Message> + use<> {
    let channel = channel.clone();
    stream::channel(16, async move |mut output| {
        let mut receiver = channel.0.lock().await;
        while let Some(activation) = receiver.recv().await {
            if output.send(Message::Activation(activation)).await.is_err() {
                return;
            }
        }
        std::future::pending::<()>().await;
    })
}

const DISCONNECTED: &str = "Service disconnected";

pub fn service() -> Subscription<Message> {
    Subscription::run(service_stream)
}

fn service_stream() -> impl Stream<Item = Message> {
    stream::channel(32, async move |mut output| {
        loop {
            let error = match follow_service(&mut output).await {
                Ok(reason) => reason.to_owned(),
                Err(error) => error.to_string(),
            };
            if output.send(Message::Disconnected(error)).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

/// Forwards one service connection's state and errors, and says why it ended.
async fn follow_service(output: &mut mpsc_futures::Sender<Message>) -> Result<&'static str, Error> {
    let client = razerr_ipc::Client::connect().await?;
    let proxy = client.devices().await?;
    // Subscribe first, then obtain the initial property to avoid a lost update.
    let mut updates = proxy.receive_state_changed().await;
    let mut errors = proxy.receive_error().await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .path("/org/freedesktop/DBus")
        .member("NameOwnerChanged")
        .arg(0, SERVICE_NAME)
        .build()?;
    let mut owners =
        MessageStream::for_match_rule(rule, client.connection(), NonZeroUsize::new(8)).await?;
    let snapshot = client.snapshot().await?;
    forward(output, Message::Connected(client.clone())).await?;
    forward(output, Message::Snapshot(Arc::new(snapshot))).await?;
    loop {
        tokio::select! {
            update = updates.next() => {
                let Some(update) = update else { return Ok(DISCONNECTED) };
                let snapshot: Snapshot = razerr_ipc::decode(update.get().await?)?;
                snapshot.validate().map_err(|error| Error::Invalid(error.to_string()))?;
                forward(output, Message::Snapshot(Arc::new(snapshot))).await?;
            }
            error = errors.next() => {
                let Some(error) = error else { return Ok(DISCONNECTED) };
                let message = error.args()?.message.to_owned();
                forward(output, Message::Error(message)).await?;
            }
            _ = owners.next() => return Ok("Service connection changed; reconnecting…"),
        }
    }
}

async fn forward(
    output: &mut mpsc_futures::Sender<Message>,
    message: Message,
) -> Result<(), Error> {
    output
        .send(message)
        .await
        .map_err(|error| Error::Invalid(error.to_string()))
}
