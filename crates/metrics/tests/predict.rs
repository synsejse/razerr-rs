use std::time::Duration;

use proptest::prelude::*;
use razerr_metrics::{Confidence, RateInterval, Sample, predict::predict};
use razerr_model::{Percent, Serial, UsbId};

fn current(battery: f64, poll_rate: u16) -> Sample {
    Sample {
        serial: Serial::new("numeric").expect("label"),
        timestamp: 1_700_000_000,
        monotonic: Duration::ZERO,
        battery: Percent::new(battery).expect("percentage"),
        charging: false,
        poll_rate,
        usb: UsbId {
            vendor: 0x1532,
            product: 0x00c3,
        },
    }
}

#[test]
fn outliers_do_not_decide_discharge_rate_or_high_confidence() {
    let regular = RateInterval {
        seconds: 2160.0,
        first: 80.0,
        last: 76.0,
        poll_rate: 1000,
    };
    let mut observations = vec![regular; 12];
    let before = predict(&current(50.0, 1000), &observations).expect("sufficient observations");
    observations.extend([
        RateInterval {
            seconds: 10.0,
            first: 80.0,
            last: 76.0,
            poll_rate: 1000,
        },
        RateInterval {
            seconds: 500_000.0,
            first: 80.0,
            last: 76.0,
            poll_rate: 1000,
        },
    ]);
    let after = predict(&current(50.0, 1000), &observations).expect("filtered observations");
    assert_eq!(before, after);
    assert_eq!(after.confidence, Confidence::High);
}

#[test]
fn unknown_poll_rate_never_claims_high_confidence() {
    let observations = vec![
        RateInterval {
            seconds: 2160.0,
            first: 80.0,
            last: 76.0,
            poll_rate: 0
        };
        12
    ];
    let prediction = predict(&current(50.0, 0), &observations).expect("unknown exact context");
    assert_eq!(prediction.confidence, Confidence::Medium);
}

proptest! {
    #[test]
    fn predictions_require_evidence_and_keep_finite_ordered_ranges(
        battery in 0.0_f64..=100.0,
        seconds in 600.0_f64..=100_000.0,
        delta in 2.0_f64..=10.0,
        count in 0_usize..32,
    ) {
        let sample = current(battery, 1000);
        prop_assert!(predict(&sample, &[]).is_none());
        let source = vec![RateInterval { seconds, first: 80.0, last: 80.0-delta, poll_rate: 1000 }; count];
        if let Some(prediction) = predict(&sample, &source) {
            prop_assert!(count >= 3);
            prop_assert!(prediction.observed_seconds >= 1800.0);
            prop_assert!(prediction.percent_per_hour.is_finite() && prediction.percent_per_hour > 0.0);
            prop_assert!(prediction.hours_min.is_finite() && prediction.hours_min >= 0.0);
            prop_assert!(prediction.hours_max.is_finite());
            prop_assert!(prediction.hours_min <= prediction.hours_remaining);
            prop_assert!(prediction.hours_remaining <= prediction.hours_max);
        }
    }
}
