use razerr_model::Serial;
use serde::Serialize;
use thiserror::Error;
use zbus::wire::{DynamicDeserialize, DynamicType};

use openrazer::Method;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not connect to the session D-Bus: {0}")]
    Session(#[source] zbus::Error),
    #[error("{operation}: {detail}")]
    Call { operation: String, detail: String },
    #[error("{0} timed out after 10 seconds")]
    Timeout(String),
}

/// A connection for reads and validated writes, with no device polling loop.
pub struct Client {
    pub(crate) inner: openrazer::Client,
}

impl Client {
    pub async fn connect() -> Result<Self, Error> {
        let inner = openrazer::Client::connect()
            .await
            .map_err(|error| match error {
                openrazer::Error::Bus(error) => Error::Session(error),
                error => map_error("session bus connection", error),
            })?;
        // Preserve the previous client's support for D-Bus daemon activation.
        Ok(Self {
            inner: inner.with_auto_start(true),
        })
    }

    // Use generated descriptors; periodic reads intentionally avoid repeated
    // introspection. The worker invalidates model capabilities on owner/USB changes.
    pub(crate) async fn call<I: Serialize + DynamicType, O: for<'de> DynamicDeserialize<'de>>(
        &self,
        path: &str,
        method: &Method<I, O>,
        body: &I,
    ) -> Result<O, Error> {
        let info = method.info();
        self.inner
            .call(path, info.interface, info.name, body)
            .await
            .map_err(|error| map_error(&format!("{}.{}", info.interface, info.name), error))
    }

    pub(crate) async fn get<T>(&self, path: &str, method: &Method<(), (T,)>) -> Result<T, Error>
    where
        (T,): for<'de> DynamicDeserialize<'de>,
    {
        self.call(path, method, &()).await.map(|(value,)| value)
    }

    pub(crate) async fn enumerate(&self) -> Result<Vec<String>, Error> {
        self.inner
            .serials()
            .await
            .map_err(|error| map_error("getDevices", error))
    }

    pub async fn device_events(&self) -> Result<zbus::MessageStream, openrazer::Error> {
        self.inner.device_events().await
    }

    pub async fn owner_events(&self) -> Result<zbus::MessageStream, openrazer::Error> {
        self.inner.owner_events().await
    }

    pub async fn serials(&self) -> Result<Vec<Serial>, Error> {
        self.enumerate()
            .await?
            .into_iter()
            .map(|value| {
                Serial::new(value).map_err(|error| Error::Call {
                    operation: "getDevices".into(),
                    detail: error.to_string(),
                })
            })
            .collect()
    }
}

pub(crate) fn device_path(serial: &Serial) -> String {
    format!("{}/device/{serial}", openrazer::ROOT)
}

fn map_error(operation: &str, error: openrazer::Error) -> Error {
    match error {
        openrazer::Error::Timeout => Error::Timeout(operation.into()),
        error => Error::Call {
            operation: operation.into(),
            detail: error.to_string(),
        },
    }
}
