use iced::Task;
use razerr_model::{
    Command, Device, DpiRange, IdleTime, LowBatteryThreshold, Percent, PollRate, Serial,
};

use crate::{
    app::{App, Edit, Message},
    dpi_pad,
    state::{Editor, Section, State, Target, color_hex},
};

impl App {
    pub(crate) fn edit(&mut self, edit: Edit) -> Task<Message> {
        if !self.state.controls_enabled() {
            self.state.error = Some(
                "The device connection is unavailable. Refresh before changing settings.".into(),
            );
            return Task::none();
        }
        match command(&mut self.state, edit) {
            Ok(commands) => self.send_all(commands),
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
    let serial = state.selected.clone().ok_or("No device selected")?;
    let device = state.selected().ok_or("Device was removed")?.clone();
    if let Edit::Apply(section) = edit {
        let commands = apply(&state.editor, &serial, &device, section)?;
        for command in &commands {
            command
                .validate(&device)
                .map_err(|error| error.to_string())?;
        }
        return Ok(commands);
    }
    draft(&mut state.editor, &device, edit)?;
    Ok(Vec::new())
}

fn draft(editor: &mut Editor, device: &Device, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::DpiX(value) => {
            editor.x.clone_from(&value);
            editor.y = value;
            editor.dpi_dirty = true;
        }
        Edit::Target(target) => editor.select_target(target),
        Edit::Axis(axis, value) => editor.set_axis(axis, value)?,
        Edit::AxisGesture(axis, gesture) => {
            if let Some(normal) = gesture.new_normal() {
                let maximum = continuous_maximum(device).ok_or("The DPI range is unavailable")?;
                editor.set_axis(axis, dpi_pad::dpi_at(normal.as_f32(), maximum).to_string())?;
            }
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
        Edit::Poll(rate) => {
            editor.poll = Some(rate);
            editor.poll_dirty = true;
        }
        Edit::Idle(value) => {
            editor.idle = value;
            editor.idle_dirty = true;
        }
        Edit::Threshold(value) => {
            editor.threshold = value;
            editor.threshold_dirty = true;
        }
        Edit::Kind(zone, kind) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            draft.kind = Some(kind);
            draft.effect_dirty = true;
        }
        Edit::Color(zone, second, value) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            if second {
                draft.second = value;
            } else {
                draft.first = value;
            }
            draft.effect_dirty = true;
        }
        Edit::Swatch(zone, second, color) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            if second {
                draft.second = color_hex(color);
            } else {
                draft.first = color_hex(color);
            }
            draft.effect_dirty = true;
        }
        Edit::Speed(zone, speed) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            draft.speed = speed;
            draft.effect_dirty = true;
        }
        Edit::Direction(zone, direction) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            draft.direction = direction;
            draft.effect_dirty = true;
        }
        Edit::Brightness(zone, brightness) => {
            let draft = editor.zones.get_mut(&zone).ok_or("Zone is unavailable")?;
            draft.brightness = brightness;
            draft.brightness_known = true;
            draft.brightness_dirty = true;
        }
        Edit::Apply(_) => {}
    }
    Ok(())
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
            if editor.poll_dirty
                && let Some(rate) = editor.poll
            {
                commands.push(Command::SetPollRate {
                    serial: id(),
                    rate: PollRate::new(rate).map_err(|error| error.to_string())?,
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
        Section::Insights => {}
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
    use crate::state::Axis;
    use razerr_model::Snapshot;
    use razerr_storage::Settings;
    use std::sync::Arc;

    fn wired() -> State {
        let snapshot: Snapshot = serde_json::from_str(include_str!("../tests/captures/wired.json"))
            .expect("actual wired snapshot");
        let mut state = State::new(Settings::default(), false, false);
        state.receive(Arc::new(snapshot));
        state
    }

    #[test]
    fn edits_stay_local_until_their_section_is_applied() {
        let mut state = wired();
        for edit in [
            Edit::Vector(800, 800),
            Edit::AxisGesture(Axis::X, iced_audio::Gesture::GestureStart),
            Edit::AxisGesture(Axis::X, iced_audio::Gesture::Gesturing(0.5.into())),
            Edit::AxisGesture(Axis::X, iced_audio::Gesture::GestureEnd),
            Edit::Axis(Axis::Y, "1200".into()),
            Edit::ActiveStage(1),
            Edit::Poll(500),
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
                Command::SetDpiStages { .. },
                Command::SetDpi { .. },
                Command::SetPollRate { .. }
            ]
        ));
        let power = command(&mut state, Edit::Apply(Section::Power)).expect("apply");
        assert!(matches!(power.as_slice(), [Command::SetIdleTime { .. }]));
    }

    #[test]
    fn an_invalid_draft_blocks_the_whole_section() {
        let mut state = wired();
        command(&mut state, Edit::Poll(500)).expect("draft");
        command(&mut state, Edit::Axis(Axis::X, "fast".into())).expect("draft");
        assert!(command(&mut state, Edit::Apply(Section::Sensitivity)).is_err());
    }

    #[test]
    fn reverting_a_section_restores_reported_values_and_keeps_others() {
        let mut state = wired();
        let reported = state.editor.poll;
        command(&mut state, Edit::Poll(125)).expect("draft");
        command(&mut state, Edit::Threshold("20".into())).expect("draft");
        state.revert(Some(Section::Sensitivity));
        assert_eq!(state.editor.poll, reported);
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
}
