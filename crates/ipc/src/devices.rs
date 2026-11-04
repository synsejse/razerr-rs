use std::sync::Arc;

use razerr_model::{
    Command, Dpi, DpiStages, Effect, IdleTime, LowBatteryThreshold, Percent, PollRate, Serial,
    Snapshot,
};
use razerr_worker::Request;
use tokio::sync::{mpsc, watch};
use zbus::{Connection, fdo, object_server::SignalEmitter};

use crate::{State, decode, encode};

pub const DEVICES_PATH: &str = "/org/razerr_rs/Devices";

pub struct Devices {
    commands: mpsc::Sender<Request>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
}

fn invalid(error: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::InvalidArgs(error.to_string())
}

impl Devices {
    pub fn new(commands: mpsc::Sender<Request>, snapshots: watch::Receiver<Arc<Snapshot>>) -> Self {
        Self {
            commands,
            snapshots,
        }
    }
    fn check(&self, command: &Command) -> fdo::Result<()> {
        if let Some(serial) = command.serial() {
            let snapshot = self.snapshots.borrow();
            if !snapshot.reliable || !snapshot.daemon_available {
                return Err(fdo::Error::Failed(
                    "device connection is unavailable".into(),
                ));
            }
            let device = snapshot
                .devices
                .iter()
                .find(|device| &device.serial == serial)
                .ok_or_else(|| invalid("unknown device serial"))?;
            command.validate(device).map_err(invalid)?;
        }
        Ok(())
    }
    /// Replies only once the worker has finished, so a success means the device
    /// was read back, not just that the command was queued.
    async fn run(&self, command: Command) -> fdo::Result<()> {
        self.check(&command)?;
        let (request, outcome) = Request::new(command);
        self.commands
            .try_send(request)
            .map_err(|error| fdo::Error::Failed(format!("worker queue: {error}")))?;
        outcome
            .await
            .map_err(|_| fdo::Error::Failed("device worker stopped".into()))?
            .map_err(fdo::Error::Failed)
    }
}

#[zbus::interface(name = "org.razerr_rs.Devices1")]
impl Devices {
    #[zbus(property)]
    fn state(&self) -> fdo::Result<State> {
        encode(self.snapshots.borrow().as_ref())
            .map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    #[zbus(property)]
    fn refreshing(&self) -> bool {
        self.snapshots.borrow().refreshing
    }

    async fn refresh(&self) -> fdo::Result<()> {
        self.run(Command::Refresh).await
    }
    async fn set_dpi(&self, serial: &str, x: u16, y: u16) -> fdo::Result<()> {
        self.run(Command::SetDpi {
            serial: Serial::new(serial).map_err(invalid)?,
            dpi: Dpi::new(x, y).map_err(invalid)?,
        })
        .await
    }
    async fn set_dpi_stages(
        &self,
        serial: &str,
        active: u8,
        stages: Vec<(u16, u16)>,
    ) -> fdo::Result<()> {
        let values = stages
            .into_iter()
            .map(|(x, y)| Dpi::new(x, y))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        self.run(Command::SetDpiStages {
            serial: Serial::new(serial).map_err(invalid)?,
            stages: DpiStages::new(active, values).map_err(invalid)?,
        })
        .await
    }
    async fn set_poll_rate(&self, serial: &str, rate: u16) -> fdo::Result<()> {
        self.run(Command::SetPollRate {
            serial: Serial::new(serial).map_err(invalid)?,
            rate: PollRate::new(rate).map_err(invalid)?,
        })
        .await
    }
    async fn set_idle_time(&self, serial: &str, seconds: u16) -> fdo::Result<()> {
        self.run(Command::SetIdleTime {
            serial: Serial::new(serial).map_err(invalid)?,
            seconds: IdleTime::new(seconds).map_err(invalid)?,
        })
        .await
    }
    async fn set_low_battery(&self, serial: &str, percent: u8) -> fdo::Result<()> {
        self.run(Command::SetLowBattery {
            serial: Serial::new(serial).map_err(invalid)?,
            percent: LowBatteryThreshold::new(percent).map_err(invalid)?,
        })
        .await
    }
    async fn set_brightness(&self, serial: &str, zone: &str, value: f64) -> fdo::Result<()> {
        self.run(Command::SetBrightness {
            serial: Serial::new(serial).map_err(invalid)?,
            zone: zone.parse().map_err(invalid)?,
            brightness: Percent::new(value).map_err(invalid)?,
        })
        .await
    }
    async fn set_effect(&self, serial: &str, zone: &str, effect: State) -> fdo::Result<()> {
        let effect: Effect = decode(effect).map_err(invalid)?;
        self.run(Command::SetEffect {
            serial: Serial::new(serial).map_err(invalid)?,
            zone: zone.parse().map_err(invalid)?,
            effect,
        })
        .await
    }

    #[zbus(signal)]
    pub async fn error(emitter: &SignalEmitter<'_>, message: &str) -> zbus::Result<()>;
}

pub async fn publish(connection: &Connection) -> zbus::Result<()> {
    let interface = connection
        .object_server()
        .interface::<_, Devices>(DEVICES_PATH)
        .await?;
    let devices = interface.get().await;
    devices.state_changed(interface.signal_emitter()).await?;
    devices.refreshing_changed(interface.signal_emitter()).await
}

pub async fn emit_error(connection: &Connection, error: &str) -> zbus::Result<()> {
    let interface = connection
        .object_server()
        .interface::<_, Devices>(DEVICES_PATH)
        .await?;
    Devices::error(interface.signal_emitter(), error).await
}

#[zbus::proxy(
    interface = "org.razerr_rs.Devices1",
    default_service = "org.razerr_rs.Service",
    default_path = "/org/razerr_rs/Devices"
)]
pub trait DeviceService {
    #[zbus(property)]
    fn state(&self) -> zbus::Result<State>;
    #[zbus(property)]
    fn refreshing(&self) -> zbus::Result<bool>;
    fn refresh(&self) -> zbus::Result<()>;
    fn set_dpi(&self, serial: &str, x: u16, y: u16) -> zbus::Result<()>;
    fn set_dpi_stages(&self, serial: &str, active: u8, stages: Vec<(u16, u16)>)
    -> zbus::Result<()>;
    fn set_poll_rate(&self, serial: &str, rate: u16) -> zbus::Result<()>;
    fn set_idle_time(&self, serial: &str, seconds: u16) -> zbus::Result<()>;
    fn set_low_battery(&self, serial: &str, percent: u8) -> zbus::Result<()>;
    fn set_brightness(&self, serial: &str, zone: &str, value: f64) -> zbus::Result<()>;
    fn set_effect(&self, serial: &str, zone: &str, effect: State) -> zbus::Result<()>;
    #[zbus(signal)]
    fn error(&self, message: &str) -> zbus::Result<()>;
}

pub use DeviceServiceProxy as DevicesProxy;
