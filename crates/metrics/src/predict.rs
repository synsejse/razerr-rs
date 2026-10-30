//! Empirical prediction rules ported from Chromaven's numeric test vectors.
//! Missing evidence yields `None`, never a fabricated default rate.

#![allow(
    clippy::float_cmp,
    reason = "Exact 100% is the protocol's full-battery endpoint"
)]

use serde::Serialize;

use crate::Sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Prediction {
    pub charging: bool,
    pub exact_context: bool,
    pub confidence: Confidence,
    pub percent_per_hour: f64,
    pub hours_remaining: f64,
    pub hours_min: f64,
    pub hours_max: f64,
    pub observed_seconds: f64,
    pub intervals: usize,
}

/// A learned interval already filtered by serial, USB identity, charging and age.
#[derive(Clone, Copy, Debug)]
pub struct RateInterval {
    pub seconds: f64,
    pub first: f64,
    pub last: f64,
    pub poll_rate: u16,
}

#[derive(Clone, Copy)]
struct Interval {
    source: RateInterval,
    delta: f64,
    rate: f64,
}

struct Model {
    rate: f64,
    low: f64,
    high: f64,
    seconds: f64,
    count: usize,
    exact: bool,
    confidence: Confidence,
}

fn band(battery: f64) -> usize {
    if battery < 80.0 {
        0
    } else if battery < 95.0 {
        1
    } else if battery < 99.0 {
        2
    } else {
        3
    }
}

fn model(
    source: &[Interval],
    poll_rate: u16,
    charging: bool,
    target_band: usize,
    exact: bool,
) -> Option<Model> {
    let mut selected: Vec<_> = source
        .iter()
        .copied()
        .filter(|row| {
            let row = row.source;
            // Quantization may jump over 99%; reaching full still observes the tail.
            let observed_band = if row.last == 100.0 {
                3
            } else {
                band(f64::midpoint(row.first, row.last))
            };
            if charging && observed_band != target_band {
                return false;
            }
            if exact {
                row.poll_rate == poll_rate
            } else {
                poll_rate > 0
                    && row.poll_rate > 0
                    && (0.5..=2.0).contains(&(f64::from(row.poll_rate) / f64::from(poll_rate)))
            }
        })
        .collect();
    let terminal = charging && target_band == 3;
    let required = if exact {
        if terminal {
            1
        } else if charging {
            2
        } else {
            3
        }
    } else {
        5
    };
    if selected.len() < required {
        return None;
    }
    selected.sort_by(|a, b| a.rate.total_cmp(&b.rate));
    let median = selected[selected.len() / 2].rate;
    // Slow charging tails are real evidence; only discharge excludes outliers.
    if !charging {
        selected.retain(|row| (median / 3.0..=median * 3.0).contains(&row.rate));
    }
    let seconds: f64 = selected.iter().map(|row| row.source.seconds).sum();
    let delta: f64 = selected.iter().map(|row| row.delta).sum();
    let maximum = selected
        .iter()
        .map(|row| row.source.last)
        .fold(0.0, f64::max);
    let minimum_seconds = if terminal {
        300.0
    } else if charging {
        600.0
    } else {
        1800.0
    } * if exact { 1.0 } else { 2.0 };
    let required_delta = match required {
        1 => 1.0,
        2 => 2.0,
        3 => 3.0,
        _ => 5.0,
    } * if terminal { 0.25 } else { 2.0 };
    if selected.len() < required
        || seconds < minimum_seconds
        || delta < required_delta
        || (terminal && maximum < 100.0)
    {
        return None;
    }
    let rate = delta * 3600.0 / seconds;
    let mut low = selected[(selected.len() - 1) / 5].rate.min(rate) - 3600.0 / seconds;
    let mut high = selected[((selected.len() - 1) * 4).div_ceil(5)]
        .rate
        .max(rate)
        + 3600.0 / seconds;
    if !exact {
        low *= 0.75;
        high *= 1.25;
    }
    low = low.max(rate / 4.0);
    let confidence = confidence(exact, seconds, delta, high / low, selected.len(), poll_rate);
    Some(Model {
        rate,
        low,
        high,
        seconds,
        count: selected.len(),
        exact,
        confidence,
    })
}

fn confidence(
    exact: bool,
    seconds: f64,
    delta: f64,
    spread: f64,
    count: usize,
    poll_rate: u16,
) -> Confidence {
    if exact && seconds >= 21600.0 && delta >= 30.0 && count >= 10 && spread < 1.5 && poll_rate > 0
    {
        Confidence::High
    } else if exact && seconds >= 7200.0 && delta >= 12.0 && spread < 2.0 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

pub fn predict(current: &Sample, source: &[RateInterval]) -> Option<Prediction> {
    let battery = current.battery.get();
    if current.charging && battery >= 100.0 {
        return None;
    }
    let source: Vec<_> = source
        .iter()
        .take(1024)
        .filter_map(|row| {
            let delta = if current.charging {
                row.last - row.first
            } else {
                row.first - row.last
            };
            let minimum_change = if current.charging && row.last == 100.0 {
                0.25
            } else {
                2.0
            };
            if !row.seconds.is_finite()
                || row.seconds <= 0.0
                || !row.first.is_finite()
                || !row.last.is_finite()
                || !(0.0..=100.0).contains(&row.first)
                || !(0.0..=100.0).contains(&row.last)
                || !(minimum_change..=100.0).contains(&delta)
            {
                return None;
            }
            let rate = delta * 3600.0 / row.seconds;
            if !rate.is_finite() || rate <= 0.0 {
                return None;
            }
            Some(Interval {
                source: *row,
                delta,
                rate,
            })
        })
        .collect();
    let mut prediction = Prediction {
        charging: current.charging,
        exact_context: true,
        confidence: Confidence::High,
        percent_per_hour: 0.0,
        hours_remaining: 0.0,
        hours_min: 0.0,
        hours_max: 0.0,
        observed_seconds: 0.0,
        intervals: 0,
    };
    let first = if current.charging { band(battery) } else { 0 };
    let last = if current.charging { 3 } else { 0 };
    let edges: [f64; 5] = [0.0, 80.0, 95.0, 99.0, 100.0];
    for target in first..=last {
        let model = model(&source, current.poll_rate, current.charging, target, true)
            .or_else(|| model(&source, current.poll_rate, current.charging, target, false))?;
        let remaining = if current.charging {
            edges[target + 1] - battery.max(edges[target])
        } else {
            battery
        };
        prediction.hours_remaining += remaining / model.rate;
        prediction.hours_min += remaining / model.high;
        prediction.hours_max += remaining / model.low;
        prediction.observed_seconds += model.seconds;
        prediction.intervals += model.count;
        prediction.exact_context &= model.exact;
        prediction.confidence = prediction.confidence.min(model.confidence);
        if target == first {
            prediction.percent_per_hour = model.rate;
        }
    }
    if [
        prediction.hours_remaining,
        prediction.hours_min,
        prediction.hours_max,
        prediction.observed_seconds,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(prediction)
}
