use std::time::Duration;

use indexmap::IndexMap;
use razerr_model::{Availability, Command, CommandKey, Snapshot};

/// Execute surviving commands in last-request order. This matters for controls
/// that interact: activating stages can also change the current DPI.
#[derive(Default)]
pub struct Pending {
    commands: IndexMap<CommandKey, Command>,
}

impl Pending {
    pub fn push(&mut self, command: Command) {
        self.commands.shift_remove(&command.key());
        self.commands.insert(command.key(), command);
    }
    pub fn pop(&mut self) -> Option<Command> {
        self.commands
            .shift_remove_index(0)
            .map(|(_, command)| command)
    }
}

#[derive(Default)]
pub struct RetrySchedule {
    read_retries: u8,
}

impl RetrySchedule {
    pub fn next(&mut self, snapshot: &Snapshot) -> Duration {
        if !snapshot.reliable || !snapshot.daemon_available {
            return Duration::from_secs(2);
        }
        let unavailable = snapshot
            .devices
            .iter()
            .any(|device| device.availability == Availability::Unavailable);
        let missing = snapshot.devices.iter().any(|device| {
            device.availability != Availability::Unavailable
                && (device
                    .stages
                    .as_ref()
                    .is_some_and(|stages| stages.fresh().is_none())
                    || device.battery.as_ref().is_some_and(|battery| {
                        missing_battery_sample(
                            battery.level.fresh().is_some(),
                            device.capabilities.charging,
                            battery.charging,
                        )
                    }))
        });
        self.live_interval(missing, unavailable)
    }

    fn live_interval(&mut self, missing: bool, unavailable: bool) -> Duration {
        if !missing {
            self.read_retries = 0;
        }
        if missing && self.read_retries < 2 {
            self.read_retries += 1;
            return Duration::from_secs(2);
        }
        Duration::from_secs(if unavailable { 2 } else { 20 })
    }
}

fn missing_battery_sample(
    fresh_level: bool,
    requires_charging: bool,
    charging: Option<bool>,
) -> bool {
    !fresh_level || (requires_charging && charging.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use razerr_model::{Dpi, DpiStages, Serial};

    #[test]
    fn invalid_stage_read_retries_even_with_healthy_battery() {
        let mut snapshot: Snapshot =
            serde_json::from_str(include_str!("../../ui/tests/captures/wired.json"))
                .expect("captured hardware snapshot");
        snapshot.devices[0].stages = Some(razerr_model::Reading::Unavailable);
        let mut schedule = RetrySchedule::default();
        assert_eq!(schedule.next(&snapshot), Duration::from_secs(2));
        assert_eq!(schedule.next(&snapshot), Duration::from_secs(2));
        assert_eq!(schedule.next(&snapshot), Duration::from_secs(20));
    }

    #[test]
    fn a_failed_required_charging_read_needs_a_quick_retry() {
        assert!(missing_battery_sample(true, true, None));
        assert!(missing_battery_sample(false, true, Some(false)));
        assert!(!missing_battery_sample(true, true, Some(false)));
        assert!(!missing_battery_sample(true, false, None));
    }

    #[test]
    fn coalescing_keeps_last_setting_and_order() {
        // These are command identifiers and numeric inputs, not invented devices.
        let serial = Serial::new("command_target").expect("serial");
        let command = |x| Command::SetDpi {
            serial: serial.clone(),
            dpi: Dpi::new(x, x).expect("DPI"),
        };
        let mut queue = Pending::default();
        queue.push(command(800));
        queue.push(Command::Refresh);
        queue.push(command(1600));
        assert_eq!(queue.pop(), Some(Command::Refresh));
        assert_eq!(queue.pop(), Some(command(1600)));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn final_dpi_request_follows_an_intervening_stage_request() {
        let serial = Serial::new("command_target").expect("serial");
        let dpi = |x| Command::SetDpi {
            serial: serial.clone(),
            dpi: Dpi::new(x, x).expect("DPI"),
        };
        let stages = Command::SetDpiStages {
            serial: serial.clone(),
            stages: DpiStages::new(1, vec![Dpi::new(1200, 1200).expect("DPI")]).expect("stages"),
        };
        let mut queue = Pending::default();
        queue.push(dpi(800));
        queue.push(stages.clone());
        queue.push(dpi(1600));
        assert_eq!(queue.pop(), Some(stages));
        assert_eq!(queue.pop(), Some(dpi(1600)));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn discovery_failure_retries_quickly_without_inventing_devices() {
        let mut schedule = RetrySchedule::default();
        let mut snapshot = Snapshot::default();
        assert_eq!(schedule.next(&snapshot), Duration::from_secs(2));
        snapshot.reliable = true;
        snapshot.daemon_available = true;
        assert_eq!(schedule.next(&snapshot), Duration::from_secs(20));
    }

    #[test]
    fn transient_battery_failures_have_two_quick_retries_then_back_off() {
        let mut schedule = RetrySchedule::default();
        assert_eq!(schedule.live_interval(true, false), Duration::from_secs(2));
        assert_eq!(schedule.live_interval(true, false), Duration::from_secs(2));
        assert_eq!(schedule.live_interval(true, false), Duration::from_secs(20));
        assert_eq!(
            schedule.live_interval(false, false),
            Duration::from_secs(20)
        );
        assert_eq!(schedule.live_interval(true, false), Duration::from_secs(2));
        assert_eq!(schedule.live_interval(true, true), Duration::from_secs(2));
        assert_eq!(schedule.live_interval(true, true), Duration::from_secs(2));
    }
}
