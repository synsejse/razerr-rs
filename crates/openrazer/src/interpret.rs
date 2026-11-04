//! Pure validation and cache handling, independent of D-Bus and any runtime.

use std::time::Duration;

use razerr_model::{
    Availability, Battery, Direction, Dpi, DpiStages, Effect, EffectKind, Percent, PollRate,
    Reading, Rgb, Speed, UsbId, ValidationError,
};

pub const BATTERY_CACHE_TTL: Duration = Duration::from_secs(120);

/// A transport error cannot wake a known inactive receiver. A successful but
/// malformed/zero DPI reply is evidence of an inactive endpoint.
pub fn availability(previous: Availability, reply: Option<&[i32]>) -> Availability {
    match reply {
        Some(values) if dpi(values).is_ok() => Availability::Responding,
        None if previous == Availability::Responding => previous,
        _ => Availability::Unavailable,
    }
}

/// A missing battery or the daemon's -1 sentinel is an unavailable reading,
/// not malformed data. A charging reply is meaningful only with a valid level.
pub fn battery(level: Option<f64>, charging: Option<bool>) -> Result<Battery, ValidationError> {
    let level = match level {
        None | Some(-1.0) => Reading::Unavailable,
        Some(value) => Reading::Fresh(Percent::new(value)?),
    };
    Ok(Battery {
        charging: level.fresh().and(charging),
        level,
    })
}

pub fn dpi(values: &[i32]) -> Result<Dpi, ValidationError> {
    let (x, y) = match values {
        [x] => (*x, *x),
        [x, y] => (*x, *y),
        _ => return Err(ValidationError("DPI reply must contain one or two values")),
    };
    let x = u16::try_from(x).map_err(|_| ValidationError("horizontal DPI is outside u16 range"))?;
    let y = u16::try_from(y).map_err(|_| ValidationError("vertical DPI is outside u16 range"))?;
    Dpi::new(x, y)
}

pub fn dpi_values(values: &[i32]) -> Result<Vec<u16>, ValidationError> {
    if values.is_empty() {
        return Err(ValidationError("advertised DPI list is empty"));
    }
    let mut result = Vec::new();
    for &value in values {
        let value = positive_u16(value)?;
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result.sort_unstable();
    Ok(result)
}

pub fn positive_u16(value: i32) -> Result<u16, ValidationError> {
    let value = u16::try_from(value).map_err(|_| ValidationError("value is outside u16 range"))?;
    if value == 0 {
        return Err(ValidationError("value must be positive"));
    }
    Ok(value)
}

pub fn stages((active, values): (u8, Vec<(u16, u16)>)) -> Result<DpiStages, ValidationError> {
    DpiStages::new(
        active,
        values
            .into_iter()
            .map(|(x, y)| Dpi::new(x, y))
            .collect::<Result<_, _>>()?,
    )
}

pub fn usb_id(values: &[i32]) -> Result<UsbId, ValidationError> {
    let [vendor, product] = values else {
        return Err(ValidationError("USB identity must contain two values"));
    };
    Ok(UsbId {
        vendor: u16::try_from(*vendor).map_err(|_| ValidationError("invalid USB vendor ID"))?,
        product: u16::try_from(*product).map_err(|_| ValidationError("invalid USB product ID"))?,
    })
}

pub fn poll_rates(values: &[u16]) -> Result<Vec<PollRate>, ValidationError> {
    if values.is_empty() {
        return Err(ValidationError("supported polling-rate list is empty"));
    }
    let mut rates = values.to_vec();
    rates.sort_unstable();
    rates.dedup();
    rates.into_iter().map(PollRate::new).collect()
}

/// The legacy contract predates getSupportedPollRates; this is not device data.
pub fn legacy_poll_rates() -> Vec<PollRate> {
    [125, 500, 1000]
        .into_iter()
        .filter_map(|value| PollRate::new(value).ok())
        .collect()
}

/// An unsuccessful read preserves the last value only as stale. The elapsed
/// duration must refer to the previous observation, not to the last success.
pub fn merge_reading<T: Clone>(
    previous: &Reading<T>,
    fresh: Option<T>,
    elapsed: Duration,
    max_age: Option<Duration>,
) -> Reading<T> {
    if let Some(value) = fresh {
        return Reading::Fresh(value);
    }
    let (value, age) = match previous {
        Reading::Fresh(value) => (value, elapsed),
        Reading::Stale { value, age } => (value, age.saturating_add(elapsed)),
        Reading::Unavailable => return Reading::Unavailable,
    };
    if max_age.is_some_and(|limit| age > limit) {
        return Reading::Unavailable;
    }
    Reading::Stale {
        value: value.clone(),
        age,
    }
}

pub fn effect(
    kind: EffectKind,
    colors: &[u8],
    speed: Option<i32>,
    direction: Option<i32>,
) -> Result<Effect, ValidationError> {
    if colors.len() < kind.color_count() * 3 || colors.len() > 9 || !colors.len().is_multiple_of(3)
    {
        return Err(ValidationError(
            "remembered effect colors are incomplete or oversized",
        ));
    }
    let color = |offset: usize| Rgb {
        red: colors[offset],
        green: colors[offset + 1],
        blue: colors[offset + 2],
    };
    let speed = || -> Result<Speed, ValidationError> {
        let speed = speed.ok_or(ValidationError("remembered effect speed is unavailable"))?;
        Speed::new(
            u8::try_from(speed).map_err(|_| ValidationError("invalid remembered effect speed"))?,
        )
    };
    Ok(match kind {
        EffectKind::Off => Effect::Off,
        EffectKind::Static => Effect::Static { color: color(0) },
        EffectKind::Spectrum => Effect::Spectrum,
        EffectKind::Wave => Effect::Wave {
            direction: Direction::from_wire(
                direction.ok_or(ValidationError("remembered wave direction is unavailable"))?,
            )?,
        },
        EffectKind::Breath => Effect::Breath { color: color(0) },
        EffectKind::BreathDual => Effect::BreathDual {
            colors: [color(0), color(3)],
        },
        EffectKind::BreathRandom => Effect::BreathRandom,
        EffectKind::Reactive => Effect::Reactive {
            color: color(0),
            speed: speed()?,
        },
        EffectKind::Starlight => Effect::Starlight {
            color: color(0),
            speed: speed()?,
        },
        EffectKind::StarlightDual => Effect::StarlightDual {
            colors: [color(0), color(3)],
            speed: speed()?,
        },
        EffectKind::StarlightRandom => Effect::StarlightRandom { speed: speed()? },
        EffectKind::Ripple => Effect::Ripple { color: color(0) },
        EffectKind::RippleRandom => Effect::RippleRandom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_dpi_reads_never_wake_a_receiver() {
        assert_eq!(
            availability(Availability::Unavailable, None),
            Availability::Unavailable
        );
        assert_eq!(
            availability(Availability::Responding, None),
            Availability::Responding
        );
        assert_eq!(
            availability(Availability::Unknown, None),
            Availability::Unavailable
        );
        for invalid in [&[0, 0][..], &[][..], &[-1, 800][..], &[70000, 800][..]] {
            assert_eq!(
                availability(Availability::Responding, Some(invalid)),
                Availability::Unavailable
            );
        }
        assert_eq!(
            availability(Availability::Unavailable, Some(&[800, 800])),
            Availability::Responding
        );
    }

    #[test]
    fn unavailable_battery_cannot_imply_charging() {
        for level in [None, Some(-1.0)] {
            let battery = battery(level, Some(true)).expect("valid unavailable reading");
            assert!(matches!(battery.level, Reading::Unavailable));
            assert_eq!(battery.charging, None);
        }
        for level in [f64::NAN, f64::INFINITY, -2.0, 100.1] {
            assert!(battery(Some(level), Some(true)).is_err());
        }
        // Unknown charging does not hide a useful, valid battery percentage.
        let battery = battery(Some(0.0), None).expect("empty battery");
        assert_eq!(battery.level.fresh().map(|value| value.get()), Some(0.0));
        assert_eq!(battery.charging, None);
    }

    #[test]
    fn battery_cache_expires_without_becoming_fresh() {
        let previous = Reading::Fresh(Percent::new(40.0).expect("percentage"));
        let limit = Some(BATTERY_CACHE_TTL);
        let stale = merge_reading(&previous, None, Duration::from_secs(20), limit);
        assert!(matches!(stale, Reading::Stale { .. }));
        assert!(stale.fresh().is_none());
        assert!(matches!(
            merge_reading(&stale, None, Duration::from_secs(101), limit),
            Reading::Unavailable
        ));
    }

    #[test]
    fn failed_or_empty_stages_preserve_last_success_as_stale() {
        let previous = Reading::Fresh(stages((1, vec![(800, 800)])).expect("stages"));
        let invalid = stages((1, vec![])).ok();
        let retained = merge_reading(&previous, invalid, Duration::from_secs(20), None);
        assert_eq!(retained.displayed(), previous.displayed());
        assert!(retained.fresh().is_none());
    }

    #[test]
    fn invalid_dpi_and_polling_are_rejected() {
        for values in [
            vec![],
            vec![0, 0],
            vec![-1, 800],
            vec![70000, 800],
            vec![800, 800, 800],
        ] {
            assert!(dpi(&values).is_err());
        }
        assert_eq!(dpi(&[900, 0]).expect("legacy DPI").y(), 0);
        assert!(dpi_values(&[450, 0]).is_err());
        assert!(poll_rates(&[0, 1000]).is_err());
        assert!(poll_rates(&[]).is_err());
    }

    #[test]
    fn incomplete_or_unknown_lighting_parameters_are_not_substituted() {
        assert!(effect(EffectKind::BreathDual, &[1, 2, 3], None, None).is_err());
        assert!(effect(EffectKind::Reactive, &[1, 2, 3], Some(4), None).is_err());
        assert!(effect(EffectKind::Wave, &[], None, Some(3)).is_err());
        assert!(
            effect(
                EffectKind::StarlightDual,
                &[1, 2, 3, 4, 5, 6],
                Some(2),
                None
            )
            .is_ok()
        );
    }
}
