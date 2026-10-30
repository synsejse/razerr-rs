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
