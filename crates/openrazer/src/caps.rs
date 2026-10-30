//! Exact signature matching prevents controls for incompatible daemon methods.

use razerr_model::{Capabilities, EffectKind, ZoneCapabilities, ZoneId};
use zbus_xml::{ArgDirection, Node};

use crate::protocol::{self, DPI, MISC, POWER};

pub struct Introspection {
    node: Node<'static>,
}

impl Introspection {
    pub fn parse(xml: &str) -> Result<Self, zbus_xml::Error> {
        Node::try_from(xml).map(|node| Self {
            node: node.into_owned(),
        })
    }

    pub fn has_method(&self, interface: &str, method: &str, input: &str, output: &str) -> bool {
        self.node
            .interfaces()
            .iter()
            .filter(|iface| iface.name().as_str() == interface)
            .any(|iface| {
                iface
                    .methods()
                    .filter(|candidate| candidate.name().as_str() == method)
                    .any(|candidate| {
                        let mut actual_input = String::new();
                        let mut actual_output = String::new();
                        for arg in candidate.args() {
                            let target = if arg.direction() == Some(ArgDirection::Out) {
                                &mut actual_output
                            } else {
                                &mut actual_input
                            };
                            target.push_str(&arg.ty().to_string());
                        }
                        actual_input == input && actual_output == output
                    })
            })
    }

    pub fn capabilities(&self) -> Capabilities {
        let has = |interface: &str, method: &str, input: &str, output: &str| {
            self.has_method(interface, method, input, output)
        };
        Capabilities {
            dpi: has(DPI, "getDPI", "", "ai") && has(DPI, "setDPI", "qq", ""),
            dpi_stages: has(DPI, "getDPIStages", "", "(ya(qq))")
                && has(DPI, "setDPIStages", "ya(qq)", ""),
            available_dpi: has(DPI, "availableDPI", "", "ai"),
            poll_rate: has(MISC, "getPollRate", "", "i") && has(MISC, "setPollRate", "q", ""),
            supported_poll_rates: has(MISC, "getSupportedPollRates", "", "aq"),
            battery: has(POWER, "getBattery", "", "d"),
            charging: has(POWER, "isCharging", "", "b"),
            idle_time: has(POWER, "getIdleTime", "", "q") && has(POWER, "setIdleTime", "q", ""),
            low_battery_threshold: has(POWER, "getLowBatteryThreshold", "", "y")
                && has(POWER, "setLowBatteryThreshold", "y", ""),
            zones: ZoneId::ALL
                .into_iter()
                .filter_map(|id| {
                    let effects: Vec<_> = EffectKind::ALL
                        .into_iter()
                        .filter(|&kind| {
                            protocol::effect_endpoint(id, kind).is_some_and(|(iface, method)| {
                                has(iface, &method, protocol::effect_signature(kind), "")
                            })
                        })
                        .collect();
                    let prefix = protocol::zone_prefix(id);
                    let brightness = has(
                        protocol::brightness_interface(id),
                        &format!("get{prefix}Brightness"),
                        "",
                        "d",
                    ) && has(
                        protocol::brightness_interface(id),
                        &format!("set{prefix}Brightness"),
                        "d",
                        "",
                    );
                    if effects.is_empty() && !brightness {
                        return None;
                    }
                    let iface = protocol::zone_interface(id);
                    Some(ZoneCapabilities {
                        id,
                        effects,
                        brightness,
                        effect_name: has(iface, &format!("get{prefix}Effect"), "", "s"),
                        effect_colors: has(iface, &format!("get{prefix}EffectColors"), "", "ay"),
                        effect_speed: has(iface, &format!("get{prefix}EffectSpeed"), "", "i"),
                        wave_direction: has(iface, &format!("get{prefix}WaveDir"), "", "i"),
                    })
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CAPTURE: &str = include_str!("../tests/introspection/deathadder_v3_pro.xml");

    #[test]
    fn real_device_capabilities() {
        let caps = Introspection::parse(CAPTURE)
            .expect("real capture")
            .capabilities();
        assert!(caps.dpi && caps.dpi_stages && caps.poll_rate && caps.battery && caps.charging);
        assert!(caps.idle_time && caps.low_battery_threshold);
        assert!(!caps.available_dpi && !caps.supported_poll_rates);
        assert_eq!(caps.zones, []);
    }

    #[test]
    fn wireless_transport_advertises_its_own_polling_rates() {
        let xml = include_str!("../tests/introspection/deathadder_v3_pro_wireless.xml");
        let caps = Introspection::parse(xml)
            .expect("real wireless capture")
            .capabilities();
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
