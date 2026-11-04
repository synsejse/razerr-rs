//! Map application lighting zones to the library's typed methods.

use openrazer::{Method, MethodInfo, methods};
use razerr_model::{EffectKind, ZoneId};

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

pub fn brightness(zone: ZoneId) -> &'static Method<(), (f64,)> {
    match zone {
        ZoneId::Main => &methods::device_lighting_brightness::GET_BRIGHTNESS,
        ZoneId::Logo => &methods::device_lighting_logo::GET_LOGO_BRIGHTNESS,
        ZoneId::Scroll => &methods::device_lighting_scroll::GET_SCROLL_BRIGHTNESS,
        ZoneId::Backlight => &methods::device_lighting_backlight::GET_BACKLIGHT_BRIGHTNESS,
        ZoneId::Left => &methods::device_lighting_left::GET_LEFT_BRIGHTNESS,
        ZoneId::Right => &methods::device_lighting_right::GET_RIGHT_BRIGHTNESS,
    }
}

pub fn set_brightness(zone: ZoneId) -> &'static Method<(f64,), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_brightness::SET_BRIGHTNESS,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_BRIGHTNESS,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_BRIGHTNESS,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_BRIGHTNESS,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_BRIGHTNESS,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_BRIGHTNESS,
    }
}

pub fn effect(zone: ZoneId) -> &'static Method<(), (String,)> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::GET_EFFECT,
        ZoneId::Logo => &methods::device_lighting_logo::GET_LOGO_EFFECT,
        ZoneId::Scroll => &methods::device_lighting_scroll::GET_SCROLL_EFFECT,
        ZoneId::Backlight => &methods::device_lighting_backlight::GET_BACKLIGHT_EFFECT,
        ZoneId::Left => &methods::device_lighting_left::GET_LEFT_EFFECT,
        ZoneId::Right => &methods::device_lighting_right::GET_RIGHT_EFFECT,
    }
}

pub fn colors(zone: ZoneId) -> &'static Method<(), (Vec<u8>,)> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::GET_EFFECT_COLORS,
        ZoneId::Logo => &methods::device_lighting_logo::GET_LOGO_EFFECT_COLORS,
        ZoneId::Scroll => &methods::device_lighting_scroll::GET_SCROLL_EFFECT_COLORS,
        ZoneId::Backlight => &methods::device_lighting_backlight::GET_BACKLIGHT_EFFECT_COLORS,
        ZoneId::Left => &methods::device_lighting_left::GET_LEFT_EFFECT_COLORS,
        ZoneId::Right => &methods::device_lighting_right::GET_RIGHT_EFFECT_COLORS,
    }
}

pub fn speed(zone: ZoneId) -> &'static Method<(), (i32,)> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::GET_EFFECT_SPEED,
        ZoneId::Logo => &methods::device_lighting_logo::GET_LOGO_EFFECT_SPEED,
        ZoneId::Scroll => &methods::device_lighting_scroll::GET_SCROLL_EFFECT_SPEED,
        ZoneId::Backlight => &methods::device_lighting_backlight::GET_BACKLIGHT_EFFECT_SPEED,
        ZoneId::Left => &methods::device_lighting_left::GET_LEFT_EFFECT_SPEED,
        ZoneId::Right => &methods::device_lighting_right::GET_RIGHT_EFFECT_SPEED,
    }
}

pub fn direction(zone: ZoneId) -> &'static Method<(), (i32,)> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::GET_WAVE_DIR,
        ZoneId::Logo => &methods::device_lighting_logo::GET_LOGO_WAVE_DIR,
        ZoneId::Scroll => &methods::device_lighting_scroll::GET_SCROLL_WAVE_DIR,
        ZoneId::Backlight => &methods::device_lighting_backlight::GET_BACKLIGHT_WAVE_DIR,
        ZoneId::Left => &methods::device_lighting_left::GET_LEFT_WAVE_DIR,
        ZoneId::Right => &methods::device_lighting_right::GET_RIGHT_WAVE_DIR,
    }
}

pub fn off(zone: ZoneId) -> &'static Method<(), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_NONE,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_NONE,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_NONE,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_NONE,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_NONE,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_NONE,
    }
}

pub fn static_color(zone: ZoneId) -> &'static Method<(u8, u8, u8), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_STATIC,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_STATIC,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_STATIC,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_STATIC,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_STATIC,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_STATIC,
    }
}

pub fn spectrum(zone: ZoneId) -> &'static Method<(), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_SPECTRUM,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_SPECTRUM,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_SPECTRUM,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_SPECTRUM,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_SPECTRUM,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_SPECTRUM,
    }
}

pub fn wave(zone: ZoneId) -> &'static Method<(i32,), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_WAVE,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_WAVE,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_WAVE,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_WAVE,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_WAVE,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_WAVE,
    }
}

pub fn breath(zone: ZoneId) -> &'static Method<(u8, u8, u8), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_BREATH_SINGLE,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_BREATH_SINGLE,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_BREATH_SINGLE,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_BREATH_SINGLE,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_BREATH_SINGLE,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_BREATH_SINGLE,
    }
}

pub fn breath_dual(zone: ZoneId) -> &'static Method<(u8, u8, u8, u8, u8, u8), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_BREATH_DUAL,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_BREATH_DUAL,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_BREATH_DUAL,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_BREATH_DUAL,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_BREATH_DUAL,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_BREATH_DUAL,
    }
}

pub fn breath_random(zone: ZoneId) -> &'static Method<(), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_BREATH_RANDOM,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_BREATH_RANDOM,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_BREATH_RANDOM,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_BREATH_RANDOM,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_BREATH_RANDOM,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_BREATH_RANDOM,
    }
}

pub fn reactive(zone: ZoneId) -> &'static Method<(u8, u8, u8, u8), ()> {
    match zone {
        ZoneId::Main => &methods::device_lighting_chroma::SET_REACTIVE,
        ZoneId::Logo => &methods::device_lighting_logo::SET_LOGO_REACTIVE,
        ZoneId::Scroll => &methods::device_lighting_scroll::SET_SCROLL_REACTIVE,
        ZoneId::Backlight => &methods::device_lighting_backlight::SET_BACKLIGHT_REACTIVE,
        ZoneId::Left => &methods::device_lighting_left::SET_LEFT_REACTIVE,
        ZoneId::Right => &methods::device_lighting_right::SET_RIGHT_REACTIVE,
    }
}

pub fn effect_method(zone: ZoneId, kind: EffectKind) -> Option<MethodInfo> {
    Some(match kind {
        EffectKind::Off => off(zone).info(),
        EffectKind::Static => static_color(zone).info(),
        EffectKind::Spectrum => spectrum(zone).info(),
        EffectKind::Wave => wave(zone).info(),
        EffectKind::Breath => breath(zone).info(),
        EffectKind::BreathDual => breath_dual(zone).info(),
        EffectKind::BreathRandom => breath_random(zone).info(),
        EffectKind::Reactive => reactive(zone).info(),
        EffectKind::Starlight if zone == ZoneId::Main => {
            methods::device_lighting_chroma::SET_STARLIGHT_SINGLE.info()
        }
        EffectKind::StarlightDual if zone == ZoneId::Main => {
            methods::device_lighting_chroma::SET_STARLIGHT_DUAL.info()
        }
        EffectKind::StarlightRandom if zone == ZoneId::Main => {
            methods::device_lighting_chroma::SET_STARLIGHT_RANDOM.info()
        }
        EffectKind::Ripple if zone == ZoneId::Main => {
            methods::device_lighting_custom::SET_RIPPLE.info()
        }
        EffectKind::RippleRandom if zone == ZoneId::Main => {
            methods::device_lighting_custom::SET_RIPPLE_RANDOM_COLOUR.info()
        }
        _ => return None,
    })
}

/// Whether an optional lighting read failed, for stale-value retention.
pub fn is_effect_read(zone: ZoneId, operation: &str) -> bool {
    [
        effect(zone).info(),
        colors(zone).info(),
        speed(zone).info(),
        direction(zone).info(),
    ]
    .iter()
    .any(|method| method.name == operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(zone: ZoneId) -> &'static str {
        match zone {
            ZoneId::Main => "razer.device.lighting.chroma",
            ZoneId::Logo => "razer.device.lighting.logo",
            ZoneId::Scroll => "razer.device.lighting.scroll",
            ZoneId::Backlight => "razer.device.lighting.backlight",
            ZoneId::Left => "razer.device.lighting.left",
            ZoneId::Right => "razer.device.lighting.right",
        }
    }

    fn prefix(zone: ZoneId) -> &'static str {
        match zone {
            ZoneId::Main => "",
            ZoneId::Logo => "Logo",
            ZoneId::Scroll => "Scroll",
            ZoneId::Backlight => "Backlight",
            ZoneId::Left => "Left",
            ZoneId::Right => "Right",
        }
    }

    /// The naming rule the hand-written tables above must agree with, so a pasted
    /// row can't send one zone's write to another.
    #[test]
    fn effect_setters_follow_each_zones_naming() {
        for zone in ZoneId::ALL {
            for kind in EffectKind::ALL {
                let main_only = matches!(
                    kind,
                    EffectKind::Starlight
                        | EffectKind::StarlightDual
                        | EffectKind::StarlightRandom
                        | EffectKind::Ripple
                        | EffectKind::RippleRandom
                );
                let Some(method) = effect_method(zone, kind) else {
                    assert!(main_only && zone != ZoneId::Main, "{zone:?} {kind:?}");
                    continue;
                };
                assert!(!main_only || zone == ZoneId::Main, "{zone:?} {kind:?}");
                let ripple = matches!(kind, EffectKind::Ripple | EffectKind::RippleRandom);
                let expected = if ripple {
                    "razer.device.lighting.custom"
                } else {
                    interface(zone)
                };
                assert_eq!(method.interface, expected, "{zone:?} {kind:?}");
                let name = format!("set{}{}", prefix(zone), effect_suffix(kind));
                assert_eq!(method.name, name, "{zone:?} {kind:?}");
            }
        }
    }

    #[test]
    fn readers_and_brightness_follow_each_zones_naming() {
        for zone in ZoneId::ALL {
            let prefix = prefix(zone);
            let brightness_interface = if zone == ZoneId::Main {
                "razer.device.lighting.brightness"
            } else {
                interface(zone)
            };
            for (method, expected_interface, name) in [
                (
                    brightness(zone).info(),
                    brightness_interface,
                    format!("get{prefix}Brightness"),
                ),
                (
                    set_brightness(zone).info(),
                    brightness_interface,
                    format!("set{prefix}Brightness"),
                ),
                (
                    effect(zone).info(),
                    interface(zone),
                    format!("get{prefix}Effect"),
                ),
                (
                    colors(zone).info(),
                    interface(zone),
                    format!("get{prefix}EffectColors"),
                ),
                (
                    speed(zone).info(),
                    interface(zone),
                    format!("get{prefix}EffectSpeed"),
                ),
                (
                    direction(zone).info(),
                    interface(zone),
                    format!("get{prefix}WaveDir"),
                ),
            ] {
                assert_eq!(method.interface, expected_interface, "{zone:?}");
                assert_eq!(method.name, name, "{zone:?}");
            }
        }
    }

    #[test]
    fn only_a_zones_own_effect_reads_count_as_effect_reads() {
        assert!(is_effect_read(ZoneId::Main, "getEffectColors"));
        assert!(is_effect_read(ZoneId::Logo, "getLogoWaveDir"));
        assert!(!is_effect_read(ZoneId::Main, "getLogoEffect"));
        assert!(!is_effect_read(ZoneId::Logo, "getEffect"));
        assert!(!is_effect_read(ZoneId::Logo, "getLogoBrightness"));
    }
}
