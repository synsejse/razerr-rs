use razerr_model::{
    Availability, Battery, Device, DpiControl, DpiRange, Effect, EffectKind, IdleTime,
    LowBatteryThreshold, Percent, PollRate, Polling, Reading, Serial, ValidationError, Zone,
    ZoneCapabilities,
};
use serde::Serialize;
use zbus::wire::DynamicDeserialize;

use crate::{Client, Error, caps, client::device_path, interpret, lighting};
use openrazer::{
    Introspection, Method,
    methods::{device_dpi as dpi, device_misc as misc, device_power as power},
};

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

impl Client {
    /// Enumerate actual daemon objects. A failed optional getter adds a warning;
    /// it never fills a field with a made-up default. Discovery failure is fatal.
    pub async fn probe(&self) -> Result<ProbeReport, Error> {
        let serials = self.enumerate().await?;
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
            let path = device_path(&serial);
            report.observed_serials.push(serial.clone());
            let endpoint = match self.inner.device(serial.as_str()).await {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    let transient = !matches!(
                        error,
                        openrazer::Error::Introspection(_) | openrazer::Error::InvalidSerial
                    );
                    report.warnings.push(ReadWarning {
                        serial: raw,
                        operation: "Introspect".into(),
                        message: error.to_string(),
                        transient,
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
                introspection: endpoint.capabilities(),
            };
            report.devices.push(reader.device().await);
        }
        Ok(report)
    }

    /// The periodic pass reads only live sensitivity, polling and battery state.
    /// Lighting persistence is read by full refreshes, never by this method.
    pub async fn live(&self, previous: &[Device]) -> ProbeReport {
        let mut report = ProbeReport::default();
        for known in previous {
            report.observed_serials.push(known.serial.clone());
            let path = device_path(&known.serial);
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
            let identity: Option<Vec<i32>> = reader.get(&misc::GET_VID_PID).await;
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
                let values: Option<Vec<i32>> = reader.get(&dpi::GET_DPI).await;
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
                let rate = reader.get(&misc::GET_POLL_RATE).await;
                polling.current = reader.validate(
                    "getPollRate",
                    rate.map(|rate| PollRate::new(interpret::positive_u16(rate)?)),
                );
            }
            if device.battery.is_some() {
                let level = if device.availability == Availability::Unavailable {
                    None
                } else {
                    reader.get(&power::GET_BATTERY).await
                };
                let charging = if device.capabilities.charging && level.is_some() {
                    reader.get(&power::IS_CHARGING).await
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
                let reply: Option<(u8, Vec<(u16, u16)>)> = reader.get(&dpi::GET_DPI_STAGES).await;
                *stages = reader.validate("getDPIStages", reply.map(interpret::stages));
            }
            if let Some(threshold) = &mut device.low_battery {
                let reply = reader.get(&power::GET_LOW_BATTERY_THRESHOLD).await;
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

    async fn get<T>(&mut self, method: &Method<(), (T,)>) -> Option<T>
    where
        (T,): for<'de> DynamicDeserialize<'de>,
    {
        match self.client.get(self.path, method).await {
            Ok(value) => Some(value),
            Err(error) => {
                self.push_warning(method.info().name, error, true);
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
        method: &Method<(), (T,)>,
        validate: impl FnOnce(T) -> Result<U, ValidationError>,
    ) -> Reading<U>
    where
        (T,): for<'de> DynamicDeserialize<'de>,
    {
        let value = self.get(method).await.map(validate);
        self.validate(method.info().name, value)
    }
}

pub async fn probe() -> Result<ProbeReport, Error> {
    Client::connect().await?.probe().await
}

/// A full read of a device, guided by what it advertises.
struct Reader<'a> {
    reads: Reads<'a>,
    introspection: &'a Introspection,
}

impl Reader<'_> {
    async fn image(&mut self) -> Option<String> {
        if self.introspection.supports(&misc::GET_DEVICE_IMAGE) {
            let image: Option<String> = self.reads.get(&misc::GET_DEVICE_IMAGE).await;
            if let Some(image) = image.filter(|image| !image.is_empty()) {
                return Some(image);
            }
        }
        if !self.introspection.supports(&misc::GET_RAZER_URLS) {
            return None;
        }
        let legacy: String = self.reads.get(&misc::GET_RAZER_URLS).await?;
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
        let capabilities = caps::capabilities(self.introspection);
        let name = self.reads.get(&misc::GET_DEVICE_NAME).await;
        let kind = self.reads.get(&misc::GET_DEVICE_TYPE).await;
        let firmware = self.reads.get(&misc::GET_FIRMWARE).await;
        let driver = self.reads.get(&misc::GET_DRIVER_VERSION).await;
        let image = self.image().await;
        let usb = self
            .reads
            .checked(&misc::GET_VID_PID, |values: Vec<i32>| {
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
                    .checked(&dpi::GET_DPI_STAGES, interpret::stages)
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
                    .checked(&power::GET_IDLE_TIME, IdleTime::new)
                    .await,
            )
        } else {
            None
        };
        let low_battery = if capabilities.low_battery_threshold {
            Some(
                self.reads
                    .checked(&power::GET_LOW_BATTERY_THRESHOLD, LowBatteryThreshold::new)
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
        let level = self.reads.get(&power::GET_BATTERY).await;
        let charging = if reports_charging {
            self.reads.get(&power::IS_CHARGING).await
        } else {
            None
        };
        interpret::battery(level, charging).unwrap_or_else(|error| {
            self.reads.warn("getBattery", error);
            unavailable
        })
    }

    async fn dpi(&mut self, discrete: bool) -> (DpiControl, Availability) {
        let values: Option<Vec<i32>> = self.reads.get(&dpi::GET_DPI).await;
        let availability = interpret::availability(Availability::Unknown, values.as_deref());
        let current = self
            .reads
            .validate("getDPI", values.map(|values| interpret::dpi(&values)));
        let range = if discrete {
            DpiRange::Discrete {
                values: self
                    .reads
                    .checked(&dpi::AVAILABLE_DPI, |values: Vec<i32>| {
                        interpret::dpi_values(&values)
                    })
                    .await,
            }
        } else {
            let maximum = if self.introspection.supports(&dpi::MAX_DPI) {
                self.reads
                    .checked(&dpi::MAX_DPI, interpret::positive_u16)
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
            .checked(&misc::GET_POLL_RATE, |rate| {
                PollRate::new(interpret::positive_u16(rate)?)
            })
            .await;
        let supported = if advertised_rates {
            self.reads
                .checked(&misc::GET_SUPPORTED_POLL_RATES, |values: Vec<u16>| {
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
            let brightness = if zone.brightness {
                Some(
                    self.reads
                        .checked(lighting::brightness(zone.id), Percent::new)
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
        let method = lighting::effect(zone.id);
        let Some(name): Option<String> = self.reads.get(method).await else {
            return Reading::Unavailable;
        };
        let Some(kind) = zone
            .effects
            .iter()
            .copied()
            .find(|&kind| lighting::effect_suffix(kind).eq_ignore_ascii_case(&name))
        else {
            self.reads.warn(
                method.info().name,
                format!("unsupported remembered effect {name:?}"),
            );
            return Reading::Unavailable;
        };
        let colors: Vec<u8> = if kind.color_count() > 0 {
            if !zone.effect_colors {
                return Reading::Unavailable;
            }
            let Some(colors) = self.reads.get(lighting::colors(zone.id)).await else {
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
            self.reads.get(lighting::speed(zone.id)).await
        } else {
            None
        };
        let direction = if kind == EffectKind::Wave {
            if !zone.wave_direction {
                return Reading::Unavailable;
            }
            self.reads.get(lighting::direction(zone.id)).await
        } else {
            None
        };
        self.reads.validate(
            method.info().name,
            Some(interpret::effect(kind, &colors, speed, direction)),
        )
    }
}
