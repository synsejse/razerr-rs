use std::{
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use iced::{
    Subscription,
    futures::{SinkExt, Stream, StreamExt},
    stream,
};
use razerr_ipc::SERVICE_NAME;
use tokio::sync::{Mutex, mpsc};
use zbus::{MatchRule, MessageStream, message::Type};

use crate::{
    Activation,
    app::{Message, Service},
};

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

pub fn service() -> Subscription<Message> {
    Subscription::run(service_stream)
}

fn service_stream() -> impl Stream<Item = Message> {
    stream::channel(32, async move |mut output| {
        loop {
            let result: Result<(),String> = async {
                let client = Arc::new(razerr_ipc::Client::connect().await.map_err(|error| error.to_string())?);
                let proxy = client.devices().await.map_err(|error| error.to_string())?;
                // Subscribe first, then obtain the initial property to avoid a lost update.
                let mut updates = proxy.receive_state_changed().await;
                let mut errors = proxy.receive_error().await.map_err(|error| error.to_string())?;
                let rule = MatchRule::builder().msg_type(Type::Signal)
                    .sender("org.freedesktop.DBus").interface("org.freedesktop.DBus")
                    .path("/org/freedesktop/DBus").member("NameOwnerChanged").arg(0,SERVICE_NAME)
                    .build().map_err(|error| error.to_string())?;
                let mut owners = MessageStream::for_match_rule(rule, client.connection(), std::num::NonZeroUsize::new(8))
                    .await.map_err(|error| error.to_string())?;
                let snapshot = client.snapshot().await.map_err(|error| error.to_string())?;
                output.send(Message::Connected(Service(client.clone()))).await.map_err(|error| error.to_string())?;
                output.send(Message::Snapshot(Arc::new(snapshot))).await.map_err(|error| error.to_string())?;
                loop {
                    tokio::select! {
                        update = updates.next() => {
                            let Some(update) = update else { return Err("Service disconnected".to_owned()); };
                            let state = update.get().await.map_err(|error| error.to_string())?;
                            let snapshot: razerr_model::Snapshot = razerr_ipc::decode(state).map_err(|error| error.to_string())?;
                            snapshot.validate().map_err(|error| error.to_string())?;
                            output.send(Message::Snapshot(Arc::new(snapshot))).await.map_err(|error| error.to_string())?;
                        }
                        error = errors.next() => {
                            let Some(error) = error else { return Err("Service disconnected".to_owned()); };
                            let args = error.args().map_err(|error| error.to_string())?;
                            output.send(Message::Error(args.message.to_owned())).await.map_err(|error| error.to_string())?;
                        }
                        _ = owners.next() => return Err("Service connection changed; reconnecting…".to_owned()),
                    }
                }
            }.await;
            let error = match result {
                Err(error) => error,
                Ok(()) => "Service disconnected".to_owned(),
            };
            if output.send(Message::Disconnected(error)).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}
