use razerr_model::{Command, Snapshot};
use zbus::Connection;

use crate::{DevicesProxy, Error, decode, encode};

#[derive(Clone)]
pub struct Client {
    connection: Connection,
    destination: String,
}

impl Client {
    pub async fn connect() -> Result<Self, Error> {
        let connection = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            zbus::connection::Builder::session()
                .method_timeout(std::time::Duration::from_secs(10))
                .build(),
        )
        .await
        .map_err(|_| Error::Invalid("session bus connection timed out".into()))??;
        // Resolve an existing unique owner so connecting never auto-activates
        // the service; its lifecycle belongs to packaging.
        let reply = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetNameOwner",
                &(crate::SERVICE_NAME,),
            )
            .await?;
        let destination = reply.body().deserialize()?;
        Ok(Self {
            connection,
            destination,
        })
    }
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn destination(&self) -> &str {
        &self.destination
    }
    pub async fn devices(&self) -> Result<DevicesProxy<'_>, Error> {
        Ok(DevicesProxy::builder(&self.connection)
            .destination(self.destination.as_str())
            .build()
            .await?)
    }
    pub async fn snapshot(&self) -> Result<Snapshot, Error> {
        let proxy = self.devices().await?;
        let snapshot: Snapshot = decode(proxy.state().await?)?;
        snapshot
            .validate()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        Ok(snapshot)
    }
    pub async fn send(&self, command: Command) -> Result<(), Error> {
        let proxy = self.devices().await?;
        match command {
            Command::Refresh => proxy.refresh().await?,
            Command::SetDpi { serial, dpi } => {
                proxy.set_dpi(serial.as_str(), dpi.x(), dpi.y()).await?;
            }
            Command::SetDpiStages { serial, stages } => {
                proxy
                    .set_dpi_stages(
                        serial.as_str(),
                        stages.active(),
                        stages
                            .values()
                            .iter()
                            .map(|value| (value.x(), value.y()))
                            .collect(),
                    )
                    .await?;
            }
            Command::SetPollRate { serial, rate } => {
                proxy.set_poll_rate(serial.as_str(), rate.get()).await?;
            }
            Command::SetIdleTime { serial, seconds } => {
                proxy.set_idle_time(serial.as_str(), seconds.get()).await?;
            }
            Command::SetLowBattery { serial, percent } => {
                proxy
                    .set_low_battery(serial.as_str(), percent.get())
                    .await?;
            }
            Command::SetBrightness {
                serial,
                zone,
                brightness,
            } => {
                proxy
                    .set_brightness(serial.as_str(), zone.as_str(), brightness.get())
                    .await?;
            }
            Command::SetEffect {
                serial,
                zone,
                effect,
            } => {
                proxy
                    .set_effect(serial.as_str(), zone.as_str(), encode(&effect)?)
                    .await?;
            }
        }
        Ok(())
    }
}
