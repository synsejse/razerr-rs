//! Translate library capabilities into the controls this app supports.

use crate::lighting;
use openrazer::{
    Introspection,
    methods::{device_dpi as dpi, device_misc as misc, device_power as power},
};
use razerr_model::{Capabilities, EffectKind, ZoneCapabilities, ZoneId};

pub fn capabilities(introspection: &Introspection) -> Capabilities {
    Capabilities {
        dpi: introspection.supports(&dpi::GET_DPI) && introspection.supports(&dpi::SET_DPI),
        dpi_stages: introspection.supports(&dpi::GET_DPI_STAGES)
            && introspection.supports(&dpi::SET_DPI_STAGES),
        available_dpi: introspection.supports(&dpi::AVAILABLE_DPI),
        poll_rate: introspection.supports(&misc::GET_POLL_RATE)
            && introspection.supports(&misc::SET_POLL_RATE),
        supported_poll_rates: introspection.supports(&misc::GET_SUPPORTED_POLL_RATES),
        battery: introspection.supports(&power::GET_BATTERY),
        charging: introspection.supports(&power::IS_CHARGING),
        idle_time: introspection.supports(&power::GET_IDLE_TIME)
            && introspection.supports(&power::SET_IDLE_TIME),
        low_battery_threshold: introspection.supports(&power::GET_LOW_BATTERY_THRESHOLD)
            && introspection.supports(&power::SET_LOW_BATTERY_THRESHOLD),
        zones: ZoneId::ALL
            .into_iter()
            .filter_map(|id| {
                let effects: Vec<_> = EffectKind::ALL
                    .into_iter()
                    .filter(|&kind| {
                        lighting::effect_method(id, kind).is_some_and(|m| {
                            introspection.has_method(m.interface, m.name, m.input, m.output)
                        })
                    })
                    .collect();
                let brightness = introspection.supports(lighting::brightness(id))
                    && introspection.supports(lighting::set_brightness(id));
                if effects.is_empty() && !brightness {
                    return None;
                }
                Some(ZoneCapabilities {
                    id,
                    effects,
                    brightness,
                    effect_name: introspection.supports(lighting::effect(id)),
                    effect_colors: introspection.supports(lighting::colors(id)),
                    effect_speed: introspection.supports(lighting::speed(id)),
                    wave_direction: introspection.supports(lighting::direction(id)),
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPTURE: &str = include_str!("../tests/introspection/deathadder_v3_pro.xml");
    const DPI: &str = "razer.device.dpi";
    const POWER: &str = "razer.device.power";

    #[test]
    fn real_device_capabilities() {
        let caps = capabilities(&Introspection::parse(CAPTURE).expect("real capture"));
        assert!(caps.dpi && caps.dpi_stages && caps.poll_rate && caps.battery && caps.charging);
        assert!(caps.idle_time && caps.low_battery_threshold);
        assert!(!caps.available_dpi && !caps.supported_poll_rates);
        assert_eq!(caps.zones, []);
    }

    #[test]
    fn wireless_transport_advertises_its_own_polling_rates() {
        let xml = include_str!("../tests/introspection/deathadder_v3_pro_wireless.xml");
        let caps = capabilities(&Introspection::parse(xml).expect("real wireless capture"));
        assert!(caps.dpi && caps.dpi_stages && caps.poll_rate);
        assert!(caps.supported_poll_rates);
        assert!(caps.battery && caps.charging && caps.idle_time && caps.low_battery_threshold);
        assert!(!caps.available_dpi);
        assert_eq!(caps.zones, []);
    }

    #[test]
    fn output_struct_is_not_flattened() {
        let xml = Introspection::parse(CAPTURE).expect("real capture");
        assert!(xml.has_method(DPI, "getDPIStages", "", "(ya(qq))"));
        assert!(!xml.has_method(DPI, "getDPIStages", "", "ya(qq)"));
        assert!(xml.has_method(DPI, "setDPIStages", "ya(qq)", ""));
        assert!(!xml.has_method(DPI, "setDPIStages", "(ya(qq))", ""));
        assert!(!xml.has_method(POWER, "getBattery", "", "i"));
        assert!(!xml.has_method(DPI, "getDPI", "q", "ai"));
    }
}
