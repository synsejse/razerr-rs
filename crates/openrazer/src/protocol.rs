//! Names and signatures from the upstream `OpenRazer` API.
use razerr_model::{EffectKind, ZoneId};

pub const DESTINATION: &str = "org.razer";
pub const ROOT: &str = "/org/razer";
pub const MISC: &str = "razer.device.misc";
pub const DPI: &str = "razer.device.dpi";
pub const POWER: &str = "razer.device.power";

pub fn zone_interface(zone: ZoneId) -> &'static str {
    match zone {
        ZoneId::Main => "razer.device.lighting.chroma",
        ZoneId::Logo => "razer.device.lighting.logo",
        ZoneId::Scroll => "razer.device.lighting.scroll",
        ZoneId::Backlight => "razer.device.lighting.backlight",
        ZoneId::Left => "razer.device.lighting.left",
        ZoneId::Right => "razer.device.lighting.right",
    }
}

pub fn zone_prefix(zone: ZoneId) -> &'static str {
    match zone {
        ZoneId::Main => "",
        ZoneId::Logo => "Logo",
        ZoneId::Scroll => "Scroll",
        ZoneId::Backlight => "Backlight",
        ZoneId::Left => "Left",
        ZoneId::Right => "Right",
    }
}

pub fn brightness_interface(zone: ZoneId) -> &'static str {
    if zone == ZoneId::Main {
        "razer.device.lighting.brightness"
    } else {
        zone_interface(zone)
    }
}

pub fn effect_suffix(effect: EffectKind) -> &'static str {
    match effect {
        EffectKind::Off => "None",
        EffectKind::Static => "Static",
        EffectKind::Spectrum => "Spectrum",
        EffectKind::Wave => "Wave",
        EffectKind::Breath => "BreathSingle",
        EffectKind::BreathDual => "BreathDual",
        EffectKind::BreathRandom => "BreathRandom",
        EffectKind::Reactive => "Reactive",
        EffectKind::Starlight => "StarlightSingle",
        EffectKind::StarlightDual => "StarlightDual",
        EffectKind::StarlightRandom => "StarlightRandom",
        EffectKind::Ripple => "Ripple",
        EffectKind::RippleRandom => "RippleRandomColour",
    }
}

pub fn effect_signature(effect: EffectKind) -> &'static str {
    match effect {
        EffectKind::Off | EffectKind::Spectrum | EffectKind::BreathRandom => "",
        EffectKind::Static | EffectKind::Breath => "yyy",
        EffectKind::BreathDual => "yyyyyy",
        EffectKind::Wave => "i",
        EffectKind::Reactive | EffectKind::Starlight => "yyyy",
        EffectKind::StarlightDual => "yyyyyyy",
        EffectKind::StarlightRandom => "y",
        EffectKind::Ripple => "yyyd",
        EffectKind::RippleRandom => "d",
    }
}

pub fn effect_endpoint(zone: ZoneId, effect: EffectKind) -> Option<(&'static str, String)> {
    let main_only = matches!(
        effect,
        EffectKind::Starlight
            | EffectKind::StarlightDual
            | EffectKind::StarlightRandom
            | EffectKind::Ripple
            | EffectKind::RippleRandom
    );
    if zone != ZoneId::Main && main_only {
        return None;
    }
    if matches!(effect, EffectKind::Ripple | EffectKind::RippleRandom) {
        Some((
            "razer.device.lighting.custom",
            format!("set{}", effect_suffix(effect)),
        ))
    } else {
        Some((
            zone_interface(zone),
            format!("set{}{}", zone_prefix(zone), effect_suffix(effect)),
        ))
    }
}
