use iced::Task;
use razerr_model::{
    Command, Device, DpiRange, IdleTime, LowBatteryThreshold, Percent, PollRate, Serial, ZoneId,
};

use crate::{
    app::{App, Edit, Message},
    dpi_pad,
    editor::{Editor, Target, ZoneEditor},
    state::{Section, State},
};

impl App {
    pub(crate) fn edit(&mut self, edit: Edit) -> Task<Message> {
        if !self.state.controls_enabled() {
            self.state.error = Some(
                "The device connection is unavailable. Refresh before changing settings.".into(),
            );
            return Task::none();
        }
        let section = match edit {
            Edit::Apply(section) => Some(section),
            _ => None,
        };
        match command(&mut self.state, edit) {
            Ok(commands) => match section {
                Some(section) => self.apply(section, commands),
                None => Task::none(),
            },
            Err(error) => {
                self.state.error = Some(error);
                Task::none()
            }
        }
    }
}

/// Edits only change drafts; `Edit::Apply` turns a section's drafts into
/// validated commands. Nothing is sent if any of them is invalid.
fn command(state: &mut State, edit: Edit) -> Result<Vec<Command>, String> {
    if let Edit::Apply(section) = edit {
        return commands(state, section);
    }
    let device = state.selected().ok_or("Device was removed")?.clone();
    draft(&mut state.editor, &device, edit)?;
    Ok(Vec::new())
}

/// The validated commands for `section`'s drafts, or why they can't be sent.
pub(crate) fn commands(state: &State, section: Section) -> Result<Vec<Command>, String> {
    let serial = state.selected.clone().ok_or("No device selected")?;
    let device = state.selected().ok_or("Device was removed")?;
    let commands = apply(&state.editor, &serial, device, section)?;
    for command in &commands {
        command
            .validate(device)
            .map_err(|error| error.to_string())?;
    }
    Ok(commands)
}

fn draft(editor: &mut Editor, device: &Device, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::DpiX(value) => {
            editor.x.clone_from(&value);
            editor.y = value;
            editor.dpi_dirty = true;
        }
        Edit::PollRate(rate) => {
            let rate = PollRate::new(rate).map_err(|error| error.to_string())?;
            if !device
                .polling
                .as_ref()
                .is_some_and(|polling| polling.supported.contains(&rate))
            {
                return Err("That polling rate is unavailable".into());
            }
            editor.poll_rate = Some(rate.get());
            editor.poll_dirty = true;
        }
        Edit::Target(target) => editor.select_target(target),
        Edit::Axis(axis, value) => editor.set_axis(axis, value)?,
        Edit::AxisPosition(axis, position) => {
            let maximum = continuous_maximum(device).ok_or("The DPI range is unavailable")?;
            editor.set_axis(axis, dpi_pad::dpi_at(position, maximum).to_string())?;
        }
        Edit::Vector(x, y) => editor.set_vector(x, y)?,
        Edit::Linked(value) => editor.set_linked(value)?,
        Edit::ActiveStage(active) => {
            if active == 0 || usize::from(active) > editor.stages.len() {
                return Err("That DPI stage is unavailable".into());
            }
            editor.active = active;
            editor.stages_dirty = true;
        }
        Edit::AddStage => {
            if editor.stages.len() < 5
                && let Some(last) = editor.stages.last().cloned()
            {
                editor.stages.push(last);
                editor.stages_dirty = true;
                editor.select_target(Target::Stage(editor.stages.len() - 1));
            }
        }
        Edit::RemoveStage(index) => editor.remove_stage(index),
        Edit::Idle(value) => {
            editor.idle = value;
            editor.idle_dirty = true;
        }
        Edit::Threshold(value) => {
            editor.threshold = value;
            editor.threshold_dirty = true;
        }
        Edit::Kind(zone, kind) => zone_effect(editor, zone)?.kind = Some(kind),
        Edit::Color(zone, second, value) => {
            let draft = zone_effect(editor, zone)?;
            if second {
                draft.second = value;
            } else {
                draft.first = value;
            }
        }
        Edit::Speed(zone, speed) => zone_effect(editor, zone)?.speed = speed,
        Edit::Direction(zone, direction) => zone_effect(editor, zone)?.direction = direction,
        Edit::Brightness(zone, brightness) => {
            let draft = zone_draft(editor, zone)?;
            draft.brightness = brightness;
            draft.brightness_known = true;
            draft.brightness_dirty = true;
        }
        Edit::Apply(_) => {}
    }
    Ok(())
}

fn zone_draft(editor: &mut Editor, zone: ZoneId) -> Result<&mut ZoneEditor, String> {
    editor
        .zones
        .get_mut(&zone)
        .ok_or_else(|| "Zone is unavailable".to_owned())
}

/// A zone's draft, for a change to its effect.
fn zone_effect(editor: &mut Editor, zone: ZoneId) -> Result<&mut ZoneEditor, String> {
    let draft = zone_draft(editor, zone)?;
    draft.effect_dirty = true;
    Ok(draft)
}

/// Commands for every unapplied draft in `section`.
fn apply(
    editor: &Editor,
    serial: &Serial,
    device: &Device,
    section: Section,
) -> Result<Vec<Command>, String> {
    let id = || serial.clone();
    let mut commands = Vec::new();
    match section {
        Section::Sensitivity => {
            if editor.poll_dirty {
                commands.push(Command::SetPollRate {
                    serial: id(),
                    rate: PollRate::new(editor.poll_rate.ok_or("Polling rate is unavailable")?)
                        .map_err(|error| error.to_string())?,
                });
            }

            // Stages first: activating a stage changes the current DPI, which an
            // explicit current-DPI edit should then override.
            if editor.stages_dirty {
                commands.push(Command::SetDpiStages {
                    serial: id(),
                    stages: editor.stages()?,
                });
            }
            if editor.dpi_dirty {
                commands.push(Command::SetDpi {
                    serial: id(),
                    dpi: editor.dpi(device.capabilities.available_dpi)?,
                });
            }
        }
        Section::Lighting => {
            let mut zones: Vec<_> = editor.zones.iter().collect();
            zones.sort_by_key(|(id, _)| id.as_str());
            for (zone, draft) in zones {
                if draft.effect_dirty {
                    commands.push(Command::SetEffect {
                        serial: id(),
                        zone: *zone,
                        effect: draft.effect()?,
                    });
                }
                if draft.brightness_dirty {
                    commands.push(Command::SetBrightness {
                        serial: id(),
                        zone: *zone,
                        brightness: Percent::new(draft.brightness)
                            .map_err(|error| error.to_string())?,
                    });
                }
            }
        }
        Section::Power => {
            if editor.idle_dirty {
                commands.push(Command::SetIdleTime {
                    serial: id(),
                    seconds: IdleTime::new(
                        editor
                            .idle
                            .parse()
                            .map_err(|_| "Idle time must be a whole number")?,
                    )
                    .map_err(|error| error.to_string())?,
                });
            }
            if editor.threshold_dirty {
                commands.push(Command::SetLowBattery {
                    serial: id(),
                    percent: LowBatteryThreshold::new(
                        editor
                            .threshold
                            .parse()
                            .map_err(|_| "Threshold must be a whole number")?,
                    )
                    .map_err(|error| error.to_string())?,
                });
            }
        }
    }
    Ok(commands)
}

fn continuous_maximum(device: &Device) -> Option<u16> {
    device.dpi.as_ref().and_then(|dpi| match &dpi.range {
        DpiRange::Continuous { maximum } => maximum.displayed().copied(),
        DpiRange::Discrete { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Axis;
    use razerr_model::Snapshot;
    use razerr_storage::Settings;
    use std::sync::Arc;

    fn wired() -> State {
        let snapshot: Snapshot = serde_json::from_str(include_str!("../tests/captures/wired.json"))
            .expect("actual wired snapshot");
        let mut state = State::new(Settings::default(), false);
        state.receive(Arc::new(snapshot));
        state
    }

    #[test]
    fn edits_stay_local_until_their_section_is_applied() {
        let mut state = wired();
        for edit in [
            Edit::Vector(800, 800),
            Edit::AxisPosition(Axis::X, 0.5),
            Edit::Axis(Axis::Y, "1200".into()),
            Edit::ActiveStage(1),
            Edit::PollRate(500),
            Edit::Idle("600".into()),
        ] {
            assert_eq!(
                command(&mut state, edit).expect("draft"),
                Vec::<Command>::new()
            );
        }
        assert!(state.editor.dirty(Section::Sensitivity));
        assert!(state.editor.dirty(Section::Power));

        let commands = command(&mut state, Edit::Apply(Section::Sensitivity)).expect("apply");
        assert!(matches!(
            commands.as_slice(),
            [
                Command::SetPollRate { .. },
                Command::SetDpiStages { .. },
                Command::SetDpi { .. }
            ]
        ));
        let power = command(&mut state, Edit::Apply(Section::Power)).expect("apply");
        assert!(matches!(power.as_slice(), [Command::SetIdleTime { .. }]));
    }

    #[test]
    fn an_invalid_draft_blocks_the_whole_section() {
        let mut state = wired();
        command(&mut state, Edit::ActiveStage(1)).expect("draft");
        command(&mut state, Edit::Axis(Axis::X, "fast".into())).expect("draft");
        assert!(command(&mut state, Edit::Apply(Section::Sensitivity)).is_err());
    }

    #[test]
    fn reverting_a_section_restores_reported_values_and_keeps_others() {
        let mut state = wired();
        let reported = state.editor.active;
        command(&mut state, Edit::ActiveStage(1)).expect("draft");
        command(&mut state, Edit::Threshold("20".into())).expect("draft");
        state.revert(Some(Section::Sensitivity));
        assert_eq!(state.editor.active, reported);
        assert!(!state.editor.dirty(Section::Sensitivity));
        assert!(
            state.editor.dirty(Section::Power),
            "other sections keep drafts"
        );
        assert_eq!(
            command(&mut state, Edit::Apply(Section::Sensitivity)).expect("nothing to apply"),
            Vec::<Command>::new()
        );
    }

    #[test]
    fn polling_drafts_survive_snapshots_and_revert_without_a_write() {
        let mut state = wired();
        let original = state.editor.poll_rate;
        command(&mut state, Edit::PollRate(500)).expect("draft");
        let snapshot = state.snapshot.clone();
        state.receive(snapshot);
        assert_eq!(state.editor.poll_rate, Some(500));
        assert!(state.editor.poll_dirty);
        state.revert(Some(Section::Sensitivity));
        assert_eq!(state.editor.poll_rate, original);
        assert!(!state.editor.poll_dirty);
        assert_eq!(
            commands(&state, Section::Sensitivity).expect("clean"),
            Vec::<Command>::new()
        );
    }

    #[test]
    fn unsupported_polling_choices_leave_the_draft_unchanged() {
        let mut state = wired();
        let original = state.editor.poll_rate;
        assert!(command(&mut state, Edit::PollRate(123)).is_err());
        assert_eq!(state.editor.poll_rate, original);
        assert!(!state.editor.poll_dirty);
    }
}
