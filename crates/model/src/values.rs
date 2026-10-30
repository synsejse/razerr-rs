use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ValidationError(pub &'static str);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Serial(String);

impl Serial {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(ValidationError(
                "serial must be a nonempty D-Bus object-path component",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Serial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Percent(f64);

impl Percent {
    pub fn new(value: f64) -> Result<Self, ValidationError> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(ValidationError(
                "percentage must be finite and between 0 and 100",
            ));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct PollRate(u16);

impl PollRate {
    pub fn new(value: u16) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError("poll rate must be positive"));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct IdleTime(u16);

impl IdleTime {
    pub fn new(value: u16) -> Result<Self, ValidationError> {
        if !(60..=900).contains(&value) {
            return Err(ValidationError("idle time must be 60–900 seconds"));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct LowBatteryThreshold(u8);

impl LowBatteryThreshold {
    pub fn new(value: u8) -> Result<Self, ValidationError> {
        if !(5..=25).contains(&value) {
            return Err(ValidationError(
                "low-battery threshold must be 5–25 percent",
            ));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u8 {
        self.0
    }
}

/// A missing read never becomes a plausible zero or a fresh cached value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case")]
pub enum Reading<T> {
    Fresh(T),
    Stale { value: T, age: Duration },
    Unavailable,
}

impl<T> Reading<T> {
    pub fn fresh(&self) -> Option<&T> {
        match self {
            Self::Fresh(value) => Some(value),
            _ => None,
        }
    }

    pub fn displayed(&self) -> Option<&T> {
        match self {
            Self::Fresh(value) | Self::Stale { value, .. } => Some(value),
            Self::Unavailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Dpi {
    x: u16,
    y: u16,
}

impl Dpi {
    /// Zero Y is a valid wire value only for legacy single-axis devices.
    /// Call `DpiRange::validate` before applying a value to a device.
    pub fn new(x: u16, y: u16) -> Result<Self, ValidationError> {
        if x == 0 {
            return Err(ValidationError("horizontal DPI must be positive"));
        }
        Ok(Self { x, y })
    }
    pub fn x(self) -> u16 {
        self.x
    }
    pub fn y(self) -> u16 {
        self.y
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DpiRange {
    Continuous { maximum: Reading<u16> },
    Discrete { values: Reading<Vec<u16>> },
}

impl DpiRange {
    pub fn validate(&self, dpi: Dpi) -> Result<(), ValidationError> {
        match self {
            Self::Continuous { maximum } => {
                let maximum = maximum
                    .displayed()
                    .ok_or(ValidationError("DPI maximum is unknown"))?;
                if dpi.y == 0 || dpi.x > *maximum || dpi.y > *maximum {
                    return Err(ValidationError(
                        "DPI is outside the device's supported range",
                    ));
                }
            }
            Self::Discrete { values } => {
                let values = values
                    .displayed()
                    .ok_or(ValidationError("supported DPI values are unknown"))?;
                if dpi.y != 0 || !values.contains(&dpi.x) {
                    return Err(ValidationError(
                        "single-axis DPI requires an advertised X and Y=0",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DpiStages {
    active: u8,
    values: Vec<Dpi>,
}

impl DpiStages {
    /// The active index is one-based, matching `OpenRazer`'s public API.
    pub fn new(active: u8, values: Vec<Dpi>) -> Result<Self, ValidationError> {
        if !(1..=5).contains(&values.len()) || active == 0 || usize::from(active) > values.len() {
            return Err(ValidationError(
                "DPI stages require 1–5 entries and a one-based active index",
            ));
        }
        if values.iter().any(|dpi| dpi.y == 0) {
            return Err(ValidationError(
                "DPI stages require positive X and Y values",
            ));
        }
        Ok(Self { active, values })
    }
    pub fn active(&self) -> u8 {
        self.active
    }
    pub fn values(&self) -> &[Dpi] {
        &self.values
    }
}

macro_rules! validated_deserialize {
    ($name:ty, $raw:ty) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <$raw>::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

validated_deserialize!(Serial, String);
validated_deserialize!(Percent, f64);
validated_deserialize!(PollRate, u16);
validated_deserialize!(IdleTime, u16);
validated_deserialize!(LowBatteryThreshold, u8);

impl<'de> Deserialize<'de> for Dpi {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            x: u16,
            y: u16,
        }
        let value = Raw::deserialize(deserializer)?;
        Self::new(value.x, value.y).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for DpiStages {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            active: u8,
            values: Vec<Dpi>,
        }
        let value = Raw::deserialize(deserializer)?;
        Self::new(value.active, value.values).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_percentages_are_not_zero_readings() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 100.1] {
            assert!(Percent::new(value).is_err());
        }
        assert!(Percent::new(0.0).is_ok());
        assert!(Percent::new(100.0).is_ok());
    }

    #[test]
    fn discrete_dpi_requires_supported_x_and_zero_y() {
        let range = DpiRange::Discrete {
            values: Reading::Fresh(vec![450, 900, 1800]),
        };
        assert!(range.validate(Dpi::new(900, 0).expect("DPI")).is_ok());
        assert!(range.validate(Dpi::new(900, 900).expect("DPI")).is_err());
        assert!(range.validate(Dpi::new(1000, 0).expect("DPI")).is_err());
        let missing = DpiRange::Discrete {
            values: Reading::Unavailable,
        };
        assert!(missing.validate(Dpi::new(900, 0).expect("DPI")).is_err());
    }

    #[test]
    fn stages_require_both_axes_and_valid_active_index() {
        let dpi = Dpi::new(800, 800).expect("DPI");
        assert!(DpiStages::new(0, vec![dpi]).is_err());
        assert!(DpiStages::new(2, vec![dpi]).is_err());
        assert!(DpiStages::new(1, vec![dpi; 6]).is_err());
        assert!(DpiStages::new(1, vec![]).is_err());
        assert!(DpiStages::new(1, vec![Dpi::new(800, 0).expect("DPI")]).is_err());
        assert!(DpiStages::new(5, vec![dpi; 5]).is_ok());
    }

    #[test]
    fn stale_values_are_display_only() {
        let reading = Reading::Stale {
            value: 42,
            age: Duration::from_secs(20),
        };
        assert_eq!(reading.displayed(), Some(&42));
        assert_eq!(reading.fresh(), None);
    }

    #[test]
    fn serial_cannot_escape_the_device_object_path() {
        for serial in ["", "../", "a/b", "a b", "é"] {
            assert!(Serial::new(serial).is_err());
        }
        assert!(Serial::new("ABC_123").is_ok());
    }

    #[test]
    fn power_settings_validate_hardware_ranges() {
        assert!(IdleTime::new(59).is_err());
        assert!(IdleTime::new(60).is_ok());
        assert!(IdleTime::new(900).is_ok());
        assert!(IdleTime::new(901).is_err());
        assert!(LowBatteryThreshold::new(4).is_err());
        assert!(LowBatteryThreshold::new(25).is_ok());
        assert!(LowBatteryThreshold::new(26).is_err());
        assert!(PollRate::new(0).is_err());
    }
}
