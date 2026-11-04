use std::sync::Arc;

use razerr_model::{Command, Snapshot};
use razerr_worker::Request as WorkerRequest;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Open,
    Settings,
    ClearHistory,
    Stop,
    Quit,
}

pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<(), String>>,
}

pub struct Control {
    pub requests: mpsc::Sender<Request>,
    pub commands: mpsc::Sender<WorkerRequest>,
    pub snapshots: watch::Receiver<Arc<Snapshot>>,
}

impl Control {
    async fn request(&self, action: Action) -> zbus::fdo::Result<()> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request { action, reply })
            .await
            .map_err(|_| zbus::fdo::Error::Failed("service is shutting down".into()))?;
        response
            .await
            .map_err(|_| {
                zbus::fdo::Error::Failed("service stopped before completing the request".into())
            })?
            .map_err(zbus::fdo::Error::Failed)
    }
}

#[zbus::interface(name = "org.razerr_rs.Control1")]
impl Control {
    async fn open(&self) -> zbus::fdo::Result<()> {
        self.request(Action::Open).await
    }
    async fn settings(&self) -> zbus::fdo::Result<()> {
        self.request(Action::Settings).await
    }
    async fn clear_history(&self) -> zbus::fdo::Result<()> {
        self.request(Action::ClearHistory).await
    }
    async fn stop(&self) -> zbus::fdo::Result<()> {
        self.request(Action::Stop).await
    }
    async fn quit(&self) -> zbus::fdo::Result<()> {
        self.request(Action::Quit).await
    }
    async fn refresh(&self) -> zbus::fdo::Result<()> {
        self.commands
            .send(Command::Refresh.into())
            .await
            .map_err(|_| zbus::fdo::Error::Failed("device worker is shutting down".into()))
    }
    fn status(&self) -> zbus::fdo::Result<String> {
        serde_json::to_string_pretty(self.snapshots.borrow().as_ref())
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}
