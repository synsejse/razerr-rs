use std::time::Duration;

use razerr_model::{
    Availability, Battery, Device, DpiControl, DpiRange, Effect, EffectKind, IdleTime,
    LowBatteryThreshold, Percent, PollRate, Polling, Reading, Serial, ValidationError, Zone,
    ZoneCapabilities,
};
use serde::Serialize;
use thiserror::Error;
use zbus::{
    Connection,
    wire::{DynamicDeserialize, DynamicType},
};

use crate::{
    caps::Introspection,
    interpret,
    protocol::{self, DESTINATION, DPI, MISC, POWER, ROOT},
};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
// One D-Bus output argument containing the active-index/stages struct.
type DpiStagesReply = ((u8, Vec<(u16, u16)>),);

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not connect to the session D-Bus: {0}")]
    Session(#[source] zbus::Error),
    #[error("{operation}: {detail}")]
    Call { operation: String, detail: String },
    #[error("{0} timed out after 10 seconds")]
    Timeout(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadWarning {
    pub serial: String,
    pub operation: String,
    pub message: String,
    #[serde(skip)]
    pub transient: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct ProbeReport {
    pub devices: Vec<Device>,
    pub warnings: Vec<ReadWarning>,
    /// Successful enumeration even when a subsequent device introspection fails.
    #[serde(skip)]
    pub observed_serials: Vec<Serial>,
}

/// A connection for reads and validated writes, with no device polling loop.
pub struct Client {
    pub(crate) connection: Connection,
}

impl Client {
    pub async fn connect() -> Result<Self, Error> {
        let connection = tokio::time::timeout(CALL_TIMEOUT, Connection::session())
            .await
            .map_err(|_| Error::Timeout("session bus connection".into()))?
            .map_err(Error::Session)?;
        Ok(Self { connection })
    }

    /// A method call that can't outlast [`CALL_TIMEOUT`].
    pub(crate) async fn call<B: Serialize + DynamicType>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<zbus::Message, Error> {
        let operation = format!("{interface}.{method}");
        tokio::time::timeout(
            CALL_TIMEOUT,
            self.connection
                .call_method(Some(DESTINATION), path, Some(interface), method, body),
        )
        .await
        .map_err(|_| Error::Timeout(operation.clone()))?
        .map_err(|error| Error::Call {
            operation,
            detail: error.to_string(),
        })
    }

    pub(crate) async fn get<T>(&self, path: &str, interface: &str, method: &str) -> Result<T, Error>
    where
        T: for<'de> DynamicDeserialize<'de>,
    {
        let reply = self.call(path, interface, method, &()).await?;
        reply.body().deserialize().map_err(|error| Error::Call {
            operation: format!("{interface}.{method}"),
            detail: error.to_string(),
        })
    }

    /// Enumerate actual daemon objects. A failed optional getter adds a warning;
    /// it never fills a field with a made-up default. Discovery failure is fatal.
    pub async fn probe(&self) -> Result<ProbeReport, Error> {
        let serials: Vec<String> = self.get(ROOT, "razer.devices", "getDevices").await?;
        let mut report = ProbeReport::default();
        for raw in serials {
            let serial = match Serial::new(raw.clone()) {
                Ok(serial) => serial,
                Err(error) => {
                    report.warnings.push(ReadWarning {
                        serial: raw,
                        operation: "getDevices".into(),
                        message: error.to_string(),
                        transient: false,
                    });
                    continue;
                }
            };
            let path = format!("/org/razer/device/{serial}");
            report.observed_serials.push(serial.clone());
            let xml: String = match self
                .get(&path, "org.freedesktop.DBus.Introspectable", "Introspect")
                .await
            {
                Ok(xml) => xml,
                Err(error) => {
                    report.warnings.push(ReadWarning {
                        serial: raw,
                        operation: "Introspect".into(),
                        message: error.to_string(),
                        transient: true,
                    });
                    continue;
                }
            };
            let introspection = match Introspection::parse(&xml) {
                Ok(value) => value,
                Err(error) => {
                    report.warnings.push(ReadWarning {
                        serial: raw,
                        operation: "Introspect".into(),
                        message: error.to_string(),
                        transient: false,
                    });
                    continue;
                }
            };
            let reader = Reader {
                reads: Reads {
                    client: self,
                    path: &path,
                    serial: &serial,
                    warnings: &mut report.warnings,
                },
                introspection,
            };
            report.devices.push(reader.device().await);
        }
        Ok(report)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub async fn serials(&self) -> Result<Vec<Serial>, Error> {
        let values: Vec<String> = self.get(ROOT, "razer.devices", "getDevices").await?;
        values
            .into_iter()
            .map(|value| {
                Serial::new(value).map_err(|error| Error::Call {
                    operation: "getDevices".into(),
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// The periodic pass reads only live sensitivity, polling and battery state.
    /// Lighting persistence is read by full refreshes, never by this method.
    pub async fn live(&self, previous: &[Device]) -> ProbeReport {
        let mut report = ProbeReport::default();
        for known in previous {
            report.observed_serials.push(known.serial.clone());
            let path = format!("/org/razer/device/{}", known.serial);
            let mut device = known.clone();
            // The reader uses only get/checked helpers here, not introspection.
            let mut reader = Reads {
                client: self,
                path: &path,
                serial: &known.serial,
                warnings: &mut report.warnings,
            };
            // A serial can survive a wired/wireless handoff. Recheck transport
            // even if a hotplug signal was missed, before accepting observations.
            let identity: Option<Vec<i32>> = reader.get(MISC, "getVidPid").await;
            device.usb = reader
                .validate(
                    "getVidPid",
                    identity.map(|values| interpret::usb_id(&values)),
                )
                .fresh()
                .copied();
            if device.usb.is_none() {
                continue;
            }
            if let Some(dpi) = &mut device.dpi {
                let values: Option<Vec<i32>> = reader.get(DPI, "getDPI").await;
                device.availability =
                    interpret::availability(known.availability, values.as_deref());
                dpi.current =
                    reader.validate("getDPI", values.map(|values| interpret::dpi(&values)));
                if dpi.current.fresh().is_none() {
                    // Keep controls through cache merging, but do not sample
                    // cached daemon battery/polling values after a failed DPI read.
                    if let Some(polling) = &mut device.polling {
                        polling.current = Reading::Unavailable;
                    }
                    if let Some(battery) = &mut device.battery {
                        battery.level = Reading::Unavailable;
                        battery.charging = None;
                    }
                    report.devices.push(device);
                    continue;
                }
            }
            if let Some(polling) = &mut device.polling {
                let rate = reader.get(MISC, "getPollRate").await;
                polling.current = reader.validate(
                    "getPollRate",
                    rate.map(|rate| PollRate::new(interpret::positive_u16(rate)?)),
                );
            }
            if device.battery.is_some() {
                let level = if device.availability == Availability::Unavailable {
                    None
                } else {
                    reader.get(POWER, "getBattery").await
                };
                let charging = if device.capabilities.charging && level.is_some() {
                    reader.get(POWER, "isCharging").await
                } else {
                    None
                };
                device.battery = Some(match interpret::battery(level, charging) {
                    Ok(battery) => battery,
                    Err(error) => {
                        reader.warn("getBattery", error);
                        Battery {
                            level: Reading::Unavailable,
                            charging: None,
                        }
                    }
                });
            }
            if let Some(stages) = &mut device.stages {
                let reply: Option<DpiStagesReply> = reader.get(DPI, "getDPIStages").await;
                *stages = reader.validate(
                    "getDPIStages",
                    reply.map(|(value,)| interpret::stages(value)),
                );
            }
            if let Some(threshold) = &mut device.low_battery {
                let reply = reader.get(POWER, "getLowBatteryThreshold").await;
                *threshold = reader.validate(
                    "getLowBatteryThreshold",
                    reply.map(LowBatteryThreshold::new),
                );
            }
            report.devices.push(device);
        }
        report
    }
}

/// Reads of one device. A failed read becomes a warning, never a made-up value.
struct Reads<'a> {
    client: &'a Client,
    path: &'a str,
    serial: &'a Serial,
    warnings: &'a mut Vec<ReadWarning>,
}

impl Reads<'_> {
    fn push_warning(&mut self, operation: &str, error: impl std::fmt::Display, transient: bool) {
        self.warnings.push(ReadWarning {
            serial: self.serial.to_string(),
            operation: operation.into(),
            message: error.to_string(),
            transient,
        });
    }
    fn warn(&mut self, operation: &str, error: impl std::fmt::Display) {
        self.push_warning(operation, error, false);
    }
    async fn get<T: for<'de> DynamicDeserialize<'de>>(
        &mut self,
        interface: &str,
        method: &str,
    ) -> Option<T> {
        match self.client.get(self.path, interface, method).await {
            Ok(value) => Some(value),
            Err(error) => {
                self.push_warning(method, error, true);
                None
            }
        }
    }
    fn validate<T>(
        &mut self,
        operation: &str,
        value: Option<Result<T, ValidationError>>,
    ) -> Reading<T> {
        match value {
            Some(Ok(value)) => Reading::Fresh(value),
            Some(Err(error)) => {
                self.warn(operation, error);
                Reading::Unavailable
            }
            None => Reading::Unavailable,
        }
    }
    async fn checked<T, U>(
        &mut self,
        interface: &str,
        method: &str,
        validate: impl FnOnce(T) -> Result<U, ValidationError>,
    ) -> Reading<U>
    where
        T: for<'de> DynamicDeserialize<'de>,
    {
        let value = self.get(interface, method).await.map(validate);
        self.validate(method, value)
    }
}

pub async fn probe() -> Result<ProbeReport, Error> {
    Client::connect().await?.probe().await
}

/// A full read of a device, guided by what it advertises.
struct Reader<'a> {
    reads: Reads<'a>,
    introspection: Introspection,
}

impl Reader<'_> {
    async fn image(&mut self) -> Option<String> {
        if self
            .introspection
            .has_method(MISC, "getDeviceImage", "", "s")
        {
            let image: Option<String> = self.reads.get(MISC, "getDeviceImage").await;
            if let Some(image) = image.filter(|image| !image.is_empty()) {
                return Some(image);
            }
        }
        if !self.introspection.has_method(MISC, "getRazerUrls", "", "s") {
            return None;
        }
        let legacy: String = self.reads.get(MISC, "getRazerUrls").await?;
        match serde_json::from_str::<serde_json::Value>(&legacy) {
            Ok(value) => value
                .get("top_img")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            Err(error) => {
                self.reads.warn("getRazerUrls", error);
                None
            }
        }
    }

    async fn device(mut self) -> Device {
        let capabilities = self.introspection.capabilities();
        let name = self.reads.get(MISC, "getDeviceName").await;
        let kind = self.reads.get(MISC, "getDeviceType").await;
        let firmware = self.reads.get(MISC, "getFirmware").await;
        let driver = self.reads.get(MISC, "getDriverVersion").await;
        let image = self.image().await;
        let usb = self
            .reads
            .checked(MISC, "getVidPid", |values: Vec<i32>| {
                interpret::usb_id(&values)
            })
            .await;
        let usb = usb.fresh().copied();
        let (dpi, availability) = if capabilities.dpi {
            let (dpi, availability) = self.dpi(capabilities.available_dpi).await;
            (Some(dpi), availability)
        } else {
            (None, Availability::Unknown)
        };
        let stages = if capabilities.dpi_stages {
            // One struct output: the outer one-tuple is the D-Bus argument list.
            Some(
                self.reads
                    .checked(DPI, "getDPIStages", |(value,): ((u8, Vec<(u16, u16)>),)| {
                        interpret::stages(value)
                    })
                    .await,
            )
        } else {
            None
        };
        let polling = if capabilities.poll_rate {
            Some(self.polling(capabilities.supported_poll_rates).await)
        } else {
            None
        };
        let battery = if capabilities.battery {
            Some(self.battery(capabilities.charging, availability).await)
        } else {
            None
        };
        let idle_time = if capabilities.idle_time {
            Some(
                self.reads
                    .checked(POWER, "getIdleTime", IdleTime::new)
                    .await,
            )
        } else {
            None
        };
        let low_battery = if capabilities.low_battery_threshold {
            Some(
                self.reads
                    .checked(POWER, "getLowBatteryThreshold", LowBatteryThreshold::new)
                    .await,
            )
        } else {
            None
        };
        let zones = self.zones(&capabilities.zones).await;
        Device {
            serial: self.reads.serial.clone(),
            name,
            kind,
            firmware,
            driver,
            image,
            usb,
            availability,
            capabilities,
            dpi,
            stages,
            polling,
            battery,
            idle_time,
            low_battery,
            zones,
        }
    }

    async fn battery(&mut self, reports_charging: bool, availability: Availability) -> Battery {
        let unavailable = Battery {
            level: Reading::Unavailable,
            charging: None,
        };
        if availability == Availability::Unavailable {
            return unavailable;
        }
        let level = self.reads.get(POWER, "getBattery").await;
        let charging = if reports_charging {
            self.reads.get(POWER, "isCharging").await
        } else {
            None
        };
        interpret::battery(level, charging).unwrap_or_else(|error| {
            self.reads.warn("getBattery", error);
            unavailable
        })
    }

    async fn dpi(&mut self, discrete: bool) -> (DpiControl, Availability) {
        let values: Option<Vec<i32>> = self.reads.get(DPI, "getDPI").await;
        let availability = interpret::availability(Availability::Unknown, values.as_deref());
        let current = self
            .reads
            .validate("getDPI", values.map(|values| interpret::dpi(&values)));
        let range = if discrete {
            DpiRange::Discrete {
                values: self
                    .reads
                    .checked(DPI, "availableDPI", |values: Vec<i32>| {
                        interpret::dpi_values(&values)
                    })
                    .await,
            }
        } else {
            let maximum = if self.introspection.has_method(DPI, "maxDPI", "", "i") {
                self.reads
                    .checked(DPI, "maxDPI", interpret::positive_u16)
                    .await
            } else {
                Reading::Unavailable
            };
            DpiRange::Continuous { maximum }
        };
        (DpiControl { current, range }, availability)
    }

    async fn polling(&mut self, advertised_rates: bool) -> Polling {
        let current = self
            .reads
            .checked(MISC, "getPollRate", |rate| {
                PollRate::new(interpret::positive_u16(rate)?)
            })
            .await;
        let supported = if advertised_rates {
            self.reads
                .checked(MISC, "getSupportedPollRates", |values: Vec<u16>| {
                    interpret::poll_rates(&values)
                })
                .await
                .displayed()
                .cloned()
                .unwrap_or_default()
        } else {
            interpret::legacy_poll_rates()
        };
        Polling { current, supported }
    }

    async fn zones(&mut self, capabilities: &[ZoneCapabilities]) -> Vec<Zone> {
        let mut zones = Vec::new();
        for zone in capabilities {
            let prefix = protocol::zone_prefix(zone.id);
            let brightness = if zone.brightness {
                Some(
                    self.reads
                        .checked(
                            protocol::brightness_interface(zone.id),
                            &format!("get{prefix}Brightness"),
                            Percent::new,
                        )
                        .await,
                )
            } else {
                None
            };
            let effect = self.effect(zone).await;
            zones.push(Zone {
                id: zone.id,
                brightness,
                effect,
            });
        }
        zones
    }

    async fn effect(&mut self, zone: &ZoneCapabilities) -> Reading<Effect> {
        if !zone.effect_name {
            return Reading::Unavailable;
        }
        let interface = protocol::zone_interface(zone.id);
        let prefix = protocol::zone_prefix(zone.id);
        let method = format!("get{prefix}Effect");
        let Some(name): Option<String> = self.reads.get(interface, &method).await else {
            return Reading::Unavailable;
        };
        let Some(kind) = zone
            .effects
            .iter()
            .copied()
            .find(|&kind| protocol::effect_suffix(kind).eq_ignore_ascii_case(&name))
        else {
            self.reads
                .warn(&method, format!("unsupported remembered effect {name:?}"));
            return Reading::Unavailable;
        };
        let colors: Vec<u8> = if kind.color_count() > 0 {
            if !zone.effect_colors {
                return Reading::Unavailable;
            }
            let Some(colors) = self
                .reads
                .get(interface, &format!("get{prefix}EffectColors"))
                .await
            else {
                return Reading::Unavailable;
            };
            colors
        } else {
            Vec::new()
        };
        let speed = if kind.uses_speed() {
            if !zone.effect_speed {
                return Reading::Unavailable;
            }
            self.reads
                .get(interface, &format!("get{prefix}EffectSpeed"))
                .await
        } else {
            None
        };
        let direction = if kind == EffectKind::Wave {
            if !zone.wave_direction {
                return Reading::Unavailable;
            }
            self.reads
                .get(interface, &format!("get{prefix}WaveDir"))
                .await
        } else {
            None
        };
        self.reads.validate(
            &method,
            Some(interpret::effect(kind, &colors, speed, direction)),
        )
    }
}
