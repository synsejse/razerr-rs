use razerr_model::{Command, Device, Direction, Effect, ZoneId};
use serde::Serialize;
use zbus::wire::DynamicType;

use crate::{Client, Error, client::device_path, lighting};
use openrazer::{
    Method,
    methods::{
        device_dpi as dpi, device_lighting_chroma as chroma, device_lighting_custom as custom,
        device_misc as misc, device_power as power,
    },
};

impl Client {
    /// Validate again immediately before execution; a queued command may have
    /// crossed a transport handoff since the service originally accepted it.
    pub async fn execute(&self, device: &Device, command: &Command) -> Result<(), Error> {
        command.validate(device).map_err(|error| Error::Call {
            operation: "validate command".into(),
            detail: error.to_string(),
        })?;
        let path = device_path(&device.serial);
        if !matches!(command, Command::Refresh) {
            let usb: Vec<i32> = self.get(&path, &misc::GET_VID_PID).await?;
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
                self.set(&path, &dpi::SET_DPI, &(dpi.x(), dpi.y())).await
            }
            Command::SetDpiStages { stages, .. } => {
                let values: Vec<_> = stages
                    .values()
                    .iter()
                    .map(|value| (value.x(), value.y()))
                    .collect();
                self.set(&path, &dpi::SET_DPI_STAGES, &(stages.active(), values))
                    .await
            }
            Command::SetPollRate { rate, .. } => {
                self.set(&path, &misc::SET_POLL_RATE, &(rate.get(),)).await
            }
            Command::SetIdleTime { seconds, .. } => {
                self.set(&path, &power::SET_IDLE_TIME, &(seconds.get(),))
                    .await
            }
            Command::SetLowBattery { percent, .. } => {
                self.set(&path, &power::SET_LOW_BATTERY_THRESHOLD, &(percent.get(),))
                    .await
            }
            Command::SetBrightness {
                zone, brightness, ..
            } => {
                self.set(&path, lighting::set_brightness(*zone), &(brightness.get(),))
                    .await
            }
            Command::SetEffect { zone, effect, .. } => {
                self.write_effect(&path, *zone, effect).await
            }
        }
    }

    async fn write_effect(&self, path: &str, zone: ZoneId, effect: &Effect) -> Result<(), Error> {
        // Starlight and ripple exist only on the main zone, so the arms below name
        // its methods directly. Never let them land on a zone that wasn't asked for.
        if lighting::effect_method(zone, effect.kind()).is_none() {
            return Err(Error::Call {
                operation: "effect".into(),
                detail: "unsupported effect endpoint".into(),
            });
        }
        match effect {
            Effect::Off | Effect::Spectrum | Effect::BreathRandom => {
                let method = match effect {
                    Effect::Off => lighting::off(zone),
                    Effect::Spectrum => lighting::spectrum(zone),
                    _ => lighting::breath_random(zone),
                };
                self.set(path, method, &()).await
            }
            Effect::Static { color } | Effect::Breath { color } => {
                self.set(
                    path,
                    if matches!(effect, Effect::Static { .. }) {
                        lighting::static_color(zone)
                    } else {
                        lighting::breath(zone)
                    },
                    &(color.red, color.green, color.blue),
                )
                .await
            }
            Effect::BreathDual { colors: [a, b] } => {
                self.set(
                    path,
                    lighting::breath_dual(zone),
                    &(a.red, a.green, a.blue, b.red, b.green, b.blue),
                )
                .await
            }
            Effect::Reactive { color, speed } | Effect::Starlight { color, speed } => {
                self.set(
                    path,
                    if matches!(effect, Effect::Reactive { .. }) {
                        lighting::reactive(zone)
                    } else {
                        &chroma::SET_STARLIGHT_SINGLE
                    },
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
                    &chroma::SET_STARLIGHT_DUAL,
                    &(a.red, a.green, a.blue, b.red, b.green, b.blue, speed.get()),
                )
                .await
            }
            Effect::StarlightRandom { speed } => {
                self.set(path, &chroma::SET_STARLIGHT_RANDOM, &(speed.get(),))
                    .await
            }
            Effect::Wave { direction } => {
                self.set(
                    path,
                    lighting::wave(zone),
                    // Preserve the app's existing mapping during the binding migration.
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
                    &custom::SET_RIPPLE,
                    &(color.red, color.green, color.blue, 0.05_f64),
                )
                .await
            }
            Effect::RippleRandom => {
                self.set(path, &custom::SET_RIPPLE_RANDOM_COLOUR, &(0.05_f64,))
                    .await
            }
        }
    }

    async fn set<B: Serialize + DynamicType>(
        &self,
        path: &str,
        method: &Method<B, ()>,
        body: &B,
    ) -> Result<(), Error> {
        self.call(path, method, body).await
    }
}
