use std::time::Duration;

use razerr_model::{Command, Device, Direction, Effect, ZoneId};
use serde::Serialize;
use zbus::wire::DynamicType;

use crate::{
    Client, Error,
    protocol::{self, DESTINATION, DPI, MISC, POWER},
};

impl Client {
    /// Validate again immediately before execution; a queued command may have
    /// crossed a transport handoff since the service originally accepted it.
    pub async fn execute(&self, device: &Device, command: &Command) -> Result<(), Error> {
        command.validate(device).map_err(|error| Error::Call {
            operation: "validate command".into(),
            detail: error.to_string(),
        })?;
        let path = format!("/org/razer/device/{}", device.serial);
        if !matches!(command, Command::Refresh) {
            let usb: Vec<i32> = self.get(&path, MISC, "getVidPid").await?;
            if device.usb.is_none() || crate::interpret::usb_id(&usb).ok() != device.usb {
                return Err(Error::Call {
                    operation: "validate transport".into(),
                    detail: "USB identity changed; refresh before applying this setting".into(),
                });
            }
        }
        match command {
            Command::Refresh => Ok(()),
            Command::SetDpi { dpi, .. } => {
                self.set(&path, DPI, "setDPI", &(dpi.x(), dpi.y())).await
            }
            Command::SetDpiStages { stages, .. } => {
                let values: Vec<_> = stages
                    .values()
                    .iter()
                    .map(|value| (value.x(), value.y()))
                    .collect();
                self.set(&path, DPI, "setDPIStages", &(stages.active(), values))
                    .await
            }
            Command::SetPollRate { rate, .. } => {
                self.set(&path, MISC, "setPollRate", &(rate.get(),)).await
            }
            Command::SetIdleTime { seconds, .. } => {
                self.set(&path, POWER, "setIdleTime", &(seconds.get(),))
                    .await
            }
            Command::SetLowBattery { percent, .. } => {
                self.set(&path, POWER, "setLowBatteryThreshold", &(percent.get(),))
                    .await
            }
            Command::SetBrightness {
                zone, brightness, ..
            } => {
                self.set(
                    &path,
                    protocol::brightness_interface(*zone),
                    &format!("set{}Brightness", protocol::zone_prefix(*zone)),
                    &(brightness.get(),),
                )
                .await
            }
            Command::SetEffect { zone, effect, .. } => {
                self.write_effect(&path, *zone, effect).await
            }
        }
    }

    async fn write_effect(&self, path: &str, zone: ZoneId, effect: &Effect) -> Result<(), Error> {
        let (interface, method) =
            protocol::effect_endpoint(zone, effect.kind()).ok_or_else(|| Error::Call {
                operation: "effect".into(),
                detail: "unsupported effect endpoint".into(),
            })?;
        match effect {
            Effect::Off | Effect::Spectrum | Effect::BreathRandom => {
                self.set(path, interface, &method, &()).await
            }
            Effect::Static { color } | Effect::Breath { color } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(color.red, color.green, color.blue),
                )
                .await
            }
            Effect::BreathDual { colors: [a, b] } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(a.red, a.green, a.blue, b.red, b.green, b.blue),
                )
                .await
            }
            Effect::Reactive { color, speed } | Effect::Starlight { color, speed } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(color.red, color.green, color.blue, speed.get()),
                )
                .await
            }
            Effect::StarlightDual {
                colors: [a, b],
                speed,
            } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(a.red, a.green, a.blue, b.red, b.green, b.blue, speed.get()),
                )
                .await
            }
            Effect::StarlightRandom { speed } => {
                self.set(path, interface, &method, &(speed.get(),)).await
            }
            Effect::Wave { direction } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(match direction {
                        Direction::Left => 1_i32,
                        Direction::Right => 2,
                    },),
                )
                .await
            }
            Effect::Ripple { color } => {
                self.set(
                    path,
                    interface,
                    &method,
                    &(color.red, color.green, color.blue, 0.05_f64),
                )
                .await
            }
            Effect::RippleRandom => self.set(path, interface, &method, &(0.05_f64,)).await,
        }
    }

    async fn set<B: Serialize + DynamicType>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<(), Error> {
        let operation = format!("{interface}.{method}");
        tokio::time::timeout(
            Duration::from_secs(10),
            self.connection
                .call_method(Some(DESTINATION), path, Some(interface), method, body),
        )
        .await
        .map_err(|_| Error::Timeout(operation.clone()))?
        .map_err(|error| Error::Call {
            operation,
            detail: error.to_string(),
        })?;
        Ok(())
    }
}
