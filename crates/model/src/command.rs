use serde::{Deserialize, Serialize};

use crate::{
    Availability, Device, Dpi, DpiStages, Effect, IdleTime, LowBatteryThreshold, Percent, PollRate,
    Serial, ValidationError, ZoneId,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Refresh,
    SetDpi {
        serial: Serial,
        dpi: Dpi,
    },
    SetDpiStages {
        serial: Serial,
        stages: DpiStages,
    },
    SetPollRate {
        serial: Serial,
        rate: PollRate,
    },
    SetIdleTime {
        serial: Serial,
        seconds: IdleTime,
    },
    SetLowBattery {
        serial: Serial,
        percent: LowBatteryThreshold,
    },
    SetBrightness {
        serial: Serial,
        zone: ZoneId,
        brightness: Percent,
    },
    SetEffect {
        serial: Serial,
        zone: ZoneId,
        effect: Effect,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CommandKey {
    Refresh,
    Dpi(Serial),
    DpiStages(Serial),
    PollRate(Serial),
    IdleTime(Serial),
    LowBattery(Serial),
    Brightness(Serial, ZoneId),
    Effect(Serial, ZoneId),
}

impl Command {
    pub fn serial(&self) -> Option<&Serial> {
        match self {
            Self::Refresh => None,
            Self::SetDpi { serial, .. }
            | Self::SetDpiStages { serial, .. }
            | Self::SetPollRate { serial, .. }
            | Self::SetIdleTime { serial, .. }
            | Self::SetLowBattery { serial, .. }
            | Self::SetBrightness { serial, .. }
            | Self::SetEffect { serial, .. } => Some(serial),
        }
    }

    pub fn key(&self) -> CommandKey {
        match self {
            Self::Refresh => CommandKey::Refresh,
            Self::SetDpi { serial, .. } => CommandKey::Dpi(serial.clone()),
            Self::SetDpiStages { serial, .. } => CommandKey::DpiStages(serial.clone()),
            Self::SetPollRate { serial, .. } => CommandKey::PollRate(serial.clone()),
            Self::SetIdleTime { serial, .. } => CommandKey::IdleTime(serial.clone()),
            Self::SetLowBattery { serial, .. } => CommandKey::LowBattery(serial.clone()),
            Self::SetBrightness { serial, zone, .. } => {
                CommandKey::Brightness(serial.clone(), *zone)
            }
            Self::SetEffect { serial, zone, .. } => CommandKey::Effect(serial.clone(), *zone),
        }
    }

    /// Whether `device`'s fresh readings show this write took effect, or `None`
    /// when it has no fresh reading of that setting to compare.
    pub fn reflected_by(&self, device: &Device) -> Option<bool> {
        match self {
            Self::Refresh => Some(true),
            Self::SetDpi { dpi, .. } => {
                let actual = device.dpi.as_ref()?.current.fresh()?;
                Some(dpi.same_setting(*actual))
            }
            Self::SetDpiStages { stages, .. } => Some(device.stages.as_ref()?.fresh()? == stages),
            Self::SetPollRate { rate, .. } => {
                Some(device.polling.as_ref()?.current.fresh()? == rate)
            }
            Self::SetIdleTime { seconds, .. } => {
                Some(device.idle_time.as_ref()?.fresh()? == seconds)
            }
            Self::SetLowBattery { percent, .. } => {
                Some(device.low_battery.as_ref()?.fresh()? == percent)
            }
            Self::SetBrightness {
                zone, brightness, ..
            } => {
                let zone = device
                    .zones
                    .iter()
                    .find(|candidate| candidate.id == *zone)?;
                let actual = zone.brightness.as_ref()?.fresh()?;
                // Firmware keeps brightness in 0–255, so percentages round.
                Some((actual.get() - brightness.get()).abs() < 0.5)
            }
            Self::SetEffect { zone, effect, .. } => {
                let zone = device
                    .zones
                    .iter()
                    .find(|candidate| candidate.id == *zone)?;
                Some(zone.effect.fresh()? == effect)
            }
        }
    }

    pub fn validate(&self, device: &Device) -> Result<(), ValidationError> {
        if matches!(self, Self::Refresh) {
            return Ok(());
        }
        if self.serial() != Some(&device.serial) {
            return Err(ValidationError("command targets a different device"));
        }
        if device.availability == Availability::Unavailable {
            return Err(ValidationError("device connection is unavailable"));
        }
        match self {
            Self::Refresh => Ok(()),
            Self::SetDpi { dpi, .. } => device
                .dpi
                .as_ref()
                .ok_or(ValidationError("DPI is unsupported"))?
                .range
                .validate(*dpi),
            Self::SetDpiStages { stages, .. } => {
                if !device.capabilities.dpi_stages {
                    return Err(ValidationError("DPI stages are unsupported"));
                }
                let dpi = device
                    .dpi
                    .as_ref()
                    .ok_or(ValidationError("DPI range is unavailable"))?;
                for stage in stages.values() {
                    dpi.range.validate(*stage)?;
                }
                Ok(())
            }
            Self::SetPollRate { rate, .. } => {
                if !device
                    .polling
                    .as_ref()
                    .is_some_and(|polling| polling.supported.contains(rate))
                {
                    return Err(ValidationError(
                        "polling rate is not advertised by the device",
                    ));
                }
                Ok(())
            }
            Self::SetIdleTime { .. } if device.capabilities.idle_time => Ok(()),
            Self::SetLowBattery { .. } if device.capabilities.low_battery_threshold => Ok(()),
            Self::SetBrightness { zone, .. } => {
                if device
                    .capabilities
                    .zones
                    .iter()
                    .any(|cap| cap.id == *zone && cap.brightness)
                {
                    Ok(())
                } else {
                    Err(ValidationError("zone brightness is unsupported"))
                }
            }
            Self::SetEffect { zone, effect, .. } => {
                if device
                    .capabilities
                    .zones
                    .iter()
                    .any(|cap| cap.id == *zone && cap.effects.contains(&effect.kind()))
                {
                    Ok(())
                } else {
                    Err(ValidationError("effect is unsupported on this zone"))
                }
            }
            _ => Err(ValidationError("setting is unsupported")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, DpiControl, DpiRange, Polling, Reading};

    fn device() -> Device {
        Device {
            serial: Serial::new("command_target").expect("serial"),
            name: None,
            kind: None,
            firmware: None,
            driver: None,
            image: None,
            usb: None,
            availability: Availability::Responding,
            capabilities: Capabilities::default(),
            dpi: Some(DpiControl {
                current: Reading::Fresh(Dpi::new(800, 800).expect("DPI")),
                range: DpiRange::Continuous {
                    maximum: Reading::Fresh(30000),
                },
            }),
            stages: Some(Reading::Unavailable),
            polling: Some(Polling {
                current: Reading::Stale {
                    value: PollRate::new(1000).expect("rate"),
                    age: std::time::Duration::from_secs(1),
                },
                supported: Vec::new(),
            }),
            battery: None,
            idle_time: None,
            low_battery: None,
            zones: Vec::new(),
        }
    }

    #[test]
    fn readback_compares_only_fresh_readings() {
        let device = device();
        let serial = device.serial.clone();
        let dpi = |x, y| Command::SetDpi {
            serial: serial.clone(),
            dpi: Dpi::new(x, y).expect("DPI"),
        };
        assert_eq!(dpi(800, 800).reflected_by(&device), Some(true));
        assert_eq!(dpi(800, 0).reflected_by(&device), Some(true));
        assert_eq!(dpi(900, 900).reflected_by(&device), Some(false));
        // A stale value can't confirm or contradict anything.
        let rate = Command::SetPollRate {
            serial: serial.clone(),
            rate: PollRate::new(1000).expect("rate"),
        };
        assert_eq!(rate.reflected_by(&device), None);
        let stages = Command::SetDpiStages {
            serial,
            stages: DpiStages::new(1, vec![Dpi::new(800, 800).expect("DPI")]).expect("stages"),
        };
        assert_eq!(stages.reflected_by(&device), None);
    }
}
