use std::time::Duration;

use razerr_model::{Availability, Device, DpiRange, Reading};
use razerr_openrazer::interpret::{BATTERY_CACHE_TTL, merge_reading};

fn retain<T: Clone>(
    old: &Reading<T>,
    new: &mut Reading<T>,
    elapsed: Duration,
    limit: Option<Duration>,
) {
    if !matches!(new, Reading::Fresh(_)) {
        *new = merge_reading(old, None, elapsed, limit);
    }
}

/// Retain values only when the USB transport identity is known and unchanged.
/// Changed/unknown transport means old settings cannot describe this endpoint.
pub fn merge(old: &Device, new: &mut Device, elapsed: Duration, full: bool) {
    if old.usb.is_none() || old.usb != new.usb {
        return;
    }
    if let (Some(old), Some(new)) = (&old.dpi, &mut new.dpi) {
        retain(&old.current, &mut new.current, elapsed, None);
        if full {
            match (&old.range, &mut new.range) {
                (DpiRange::Continuous { maximum: old }, DpiRange::Continuous { maximum: new }) => {
                    retain(old, new, elapsed, None);
                }
                (DpiRange::Discrete { values: old }, DpiRange::Discrete { values: new }) => {
                    retain(old, new, elapsed, None);
                }
                _ => {}
            }
        }
    }
    if let (Some(old), Some(new)) = (&old.polling, &mut new.polling) {
        retain(&old.current, &mut new.current, elapsed, None);
        if new.supported.is_empty() {
            new.supported.clone_from(&old.supported);
        }
    }
    if let (Some(old), Some(new_battery)) = (&old.battery, &mut new.battery) {
        if new.availability == Availability::Unavailable {
            new_battery.level = Reading::Unavailable;
        } else {
            retain(
                &old.level,
                &mut new_battery.level,
                elapsed,
                Some(BATTERY_CACHE_TTL),
            );
        }
        if new_battery.level.fresh().is_none() {
            new_battery.charging = None;
        }
    }
    if let (Some(old), Some(new)) = (&old.stages, &mut new.stages) {
        retain(old, new, elapsed, None);
    }
    if let (Some(old), Some(new)) = (&old.low_battery, &mut new.low_battery) {
        retain(old, new, elapsed, None);
    }
    if !full {
        return;
    }
    if let (Some(old), Some(new)) = (&old.idle_time, &mut new.idle_time) {
        retain(old, new, elapsed, None);
    }
    for new_zone in &mut new.zones {
        if let Some(old_zone) = old.zones.iter().find(|zone| zone.id == new_zone.id)
            && let (Some(old), Some(new)) = (&old_zone.brightness, &mut new_zone.brightness)
        {
            retain(old, new, elapsed, None);
        }
        // Unknown persisted effects must stay unselected; retain only on a
        // failed getter, handled by the caller using its read diagnostics.
    }
}

pub fn stale(device: &mut Device, elapsed: Duration) {
    if let Some(dpi) = &mut device.dpi {
        dpi.current = merge_reading(&dpi.current, None, elapsed, None);
    }
    if let Some(polling) = &mut device.polling {
        polling.current = merge_reading(&polling.current, None, elapsed, None);
    }
    if let Some(battery) = &mut device.battery {
        battery.level = merge_reading(&battery.level, None, elapsed, Some(BATTERY_CACHE_TTL));
        battery.charging = None;
    }
}
