use serde::{Deserialize, Serialize};

use crate::ValidationError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneId {
    Main,
    Logo,
    Scroll,
    Backlight,
    Left,
    Right,
}

impl ZoneId {
    pub const ALL: [Self; 6] = [
        Self::Main,
        Self::Logo,
        Self::Scroll,
        Self::Backlight,
        Self::Left,
        Self::Right,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Off,
    Static,
    Spectrum,
    Wave,
    Breath,
    BreathDual,
    BreathRandom,
    Reactive,
    Starlight,
    StarlightDual,
    StarlightRandom,
    Ripple,
    RippleRandom,
}

impl EffectKind {
    pub const ALL: [Self; 13] = [
        Self::Off,
        Self::Static,
        Self::Spectrum,
        Self::Wave,
        Self::Breath,
        Self::BreathDual,
        Self::BreathRandom,
        Self::Reactive,
        Self::Starlight,
        Self::StarlightDual,
        Self::StarlightRandom,
        Self::Ripple,
        Self::RippleRandom,
    ];

    pub fn color_count(self) -> usize {
        match self {
            Self::Static | Self::Breath | Self::Reactive | Self::Starlight | Self::Ripple => 1,
            Self::BreathDual | Self::StarlightDual => 2,
            _ => 0,
        }
    }

    pub fn uses_speed(self) -> bool {
        matches!(
            self,
            Self::Reactive | Self::Starlight | Self::StarlightDual | Self::StarlightRandom
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Speed(u8);

impl Speed {
    pub fn new(value: u8) -> Result<Self, ValidationError> {
        if !(1..=3).contains(&value) {
            return Err(ValidationError("effect speed must be 1–3"));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Speed {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl ZoneId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Logo => "logo",
            Self::Scroll => "scroll",
            Self::Backlight => "backlight",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl std::str::FromStr for ZoneId {
    type Err = ValidationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|zone| zone.as_str() == value)
            .ok_or(ValidationError("unknown lighting zone"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
}

impl Direction {
    pub fn from_wire(value: i32) -> Result<Self, ValidationError> {
        match value {
            1 => Ok(Self::Left),
            2 => Ok(Self::Right),
            _ => Err(ValidationError("wave direction must be 1 or 2")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Off,
    Static { color: Rgb },
    Spectrum,
    Wave { direction: Direction },
    Breath { color: Rgb },
    BreathDual { colors: [Rgb; 2] },
    BreathRandom,
    Reactive { color: Rgb, speed: Speed },
    Starlight { color: Rgb, speed: Speed },
    StarlightDual { colors: [Rgb; 2], speed: Speed },
    StarlightRandom { speed: Speed },
    Ripple { color: Rgb },
    RippleRandom,
}

impl Effect {
    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Off => EffectKind::Off,
            Self::Static { .. } => EffectKind::Static,
            Self::Spectrum => EffectKind::Spectrum,
            Self::Wave { .. } => EffectKind::Wave,
            Self::Breath { .. } => EffectKind::Breath,
            Self::BreathDual { .. } => EffectKind::BreathDual,
            Self::BreathRandom => EffectKind::BreathRandom,
            Self::Reactive { .. } => EffectKind::Reactive,
            Self::Starlight { .. } => EffectKind::Starlight,
            Self::StarlightDual { .. } => EffectKind::StarlightDual,
            Self::StarlightRandom { .. } => EffectKind::StarlightRandom,
            Self::Ripple { .. } => EffectKind::Ripple,
            Self::RippleRandom => EffectKind::RippleRandom,
        }
    }
}
