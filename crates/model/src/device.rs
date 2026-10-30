use serde::{Deserialize, Serialize};

use crate::{
    Dpi, DpiRange, DpiStages, Effect, EffectKind, IdleTime, LowBatteryThreshold, Percent, PollRate,
    Reading, Serial, ZoneId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
}

/// A successful DPI read is not proof that a wireless device is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Unknown,
    Responding,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent advertised protocol features, not mutually exclusive states"
)]
pub struct Capabilities {
    pub dpi: bool,
    pub dpi_stages: bool,
    pub available_dpi: bool,
    pub poll_rate: bool,
    pub supported_poll_rates: bool,
    pub battery: bool,
    pub charging: bool,
    pub idle_time: bool,
    pub low_battery_threshold: bool,
    pub zones: Vec<ZoneCapabilities>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent optional getters in the upstream protocol"
)]
pub struct ZoneCapabilities {
    pub id: ZoneId,
    pub effects: Vec<EffectKind>,
    pub brightness: bool,
    pub effect_name: bool,
    pub effect_colors: bool,
    pub effect_speed: bool,
    pub wave_direction: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DpiControl {
    pub current: Reading<Dpi>,
    pub range: DpiRange,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Polling {
    pub current: Reading<PollRate>,
    pub supported: Vec<PollRate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Battery {
    pub level: Reading<Percent>,
    /// None means unknown/unsupported; never infer false from a failed call.
    pub charging: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub id: ZoneId,
    pub brightness: Option<Reading<Percent>>,
    /// Unknown/unsupported parameters leave the selection unavailable.
    pub effect: Reading<Effect>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub serial: Serial,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub firmware: Option<String>,
    pub driver: Option<String>,
    pub image: Option<String>,
    pub usb: Option<UsbId>,
    pub availability: Availability,
    pub capabilities: Capabilities,
    pub dpi: Option<DpiControl>,
    pub stages: Option<Reading<DpiStages>>,
    pub polling: Option<Polling>,
    pub battery: Option<Battery>,
    pub idle_time: Option<Reading<IdleTime>>,
    pub low_battery: Option<Reading<LowBatteryThreshold>>,
    pub zones: Vec<Zone>,
}
