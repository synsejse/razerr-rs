use super::number_input;
use crate::{
    activity::Control,
    app::{App, Edit, Message},
    dpi_pad::{self, Pad},
    state::{Axis, Target},
};
use iced::{
    Element, Fill,
    widget::{Column, Space, canvas, column, container, row, slider, text_input},
};
use iced_t3::{cancellable, font, icon, style, widget as t3};
use razerr_model::{Device, DpiRange};

/// Polling rates from here up cost CPU time and battery.
const HIGH_POLLING: u16 = 4000;
/// Wide enough for the axis rows, and small enough to sit beside the pad.
const AXES_WIDTH: f32 = 400.0;

impl App {
    pub(super) fn sensitivity(&self, device: &Device) -> Element<'_, Message> {
        let stages = device.capabilities.dpi_stages;
        let mut page = Column::new().spacing(24);
        if device.dpi.is_some() || stages {
            let editor = match device.dpi.as_ref().map(|dpi| &dpi.range) {
                Some(DpiRange::Continuous { maximum }) => maximum.displayed().map_or_else(
                    || self.numeric_dpi(stages),
                    |maximum| self.vector_editor(*maximum, stages),
                ),
                Some(DpiRange::Discrete { values }) => {
                    self.discrete_dpi(values.displayed(), stages)
                }
                None => self.numeric_dpi(stages),
            };
            let mut rows: Vec<_> = self.stage_row(device).into_iter().collect();
            rows.push(editor);
            page = page.push(t3::section(t3::section_title("DPI"), None, rows));
        }
        if let Some(row) = self.polling_row(device) {
            page = page.push(t3::section(t3::section_title("Polling"), None, [row]));
        }
        page.into()
    }

    /// Switches stage on the device. It shows the stage the device reports and
    /// writes its reported values, so it stays out of the way of stage drafts.
    fn stage_row(&self, device: &Device) -> Option<Element<'_, Message>> {
        let reading = device.stages.as_ref()?;
        let stages = reading.displayed()?;
        let dirty = self.state.editor.stages_dirty;
        let usable = self.state.controls_enabled() && reading.fresh().is_some();
        let segments = (1..).zip(stages.values()).map(|(stage, dpi)| {
            let selected = stages.active() == stage;
            t3::segment(
                format!("stage-{stage}"),
                if dpi.x() == dpi.y() {
                    dpi.x().to_string()
                } else {
                    format!("{} × {}", dpi.x(), dpi.y())
                },
                selected,
                (usable && !dirty).then_some(if selected {
                    Message::Idle
                } else {
                    Message::ActivateStage(stage)
                }),
            )
        });
        let mut control = t3::segmented(segments);
        if usable && dirty {
            control = t3::tooltip(
                control,
                "Apply or revert your stage edits first.",
                t3::Position::Bottom,
            );
        }
        let progress = (1..=5).find_map(|stage| self.progress(Control::Stage(stage)));
        Some(t3::setting_titled(
            "Active stage",
            Self::with_progress(progress, control),
        ))
    }

    fn polling_row(&self, device: &Device) -> Option<Element<'_, Message>> {
        let polling = device.polling.as_ref()?;
        if polling.supported.is_empty() {
            return Some(t3::setting_titled(
                "Rate",
                t3::muted("Polling rates unavailable"),
            ));
        }
        let current = polling.current.displayed().map(|rate| rate.get());
        let enabled = self.state.controls_enabled();
        let segments = polling.supported.iter().map(|rate| {
            let rate = rate.get();
            let selected = current == Some(rate);
            t3::segment(
                format!("poll-{rate}"),
                format_rate(rate),
                selected,
                enabled.then_some(if selected {
                    Message::Idle
                } else {
                    Message::PollRate(rate)
                }),
            )
        });
        let control = t3::segmented(segments);
        let progress = polling
            .supported
            .iter()
            .find_map(|rate| self.progress(Control::Poll(rate.get())));
        let control = Self::with_progress(progress, control);
        Some(match current.filter(|rate| *rate >= HIGH_POLLING) {
            Some(_) => t3::setting(
                "Rate",
                t3::warning("Uses more CPU and drains a wireless battery faster."),
                control,
            ),
            None => t3::setting_titled("Rate", control),
        })
    }

    /// The pad and the two axis sliders edit one vector: the current DPI or a stage.
    fn vector_editor(&self, maximum: u16, stages: bool) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let editor = &self.state.editor;
        let (draft_x, draft_y) = editor.target_values().unwrap_or(("", ""));
        let parse = |value: &str| value.parse::<u16>().ok().filter(|value| *value > 0);
        let current = |value: &str| parse(value).unwrap_or(dpi_pad::MIN_DPI).min(maximum);
        let pad = Pad {
            maximum,
            target: (current(draft_x), current(draft_y)),
            stages: editor
                .stages
                .iter()
                .map(|(x, y)| parse(x).zip(parse(y)))
                .collect(),
            active: usize::from(editor.active).checked_sub(1),
            editing: match editor.target {
                Target::Current => None,
                Target::Stage(index) => Some(index),
            },
            linked: editor.linked,
            enabled,
        };
        let pad = container(canvas(pad).width(300).height(270))
            .padding(4)
            .style(style::inset);
        let axes = row![
            column![
                axis_row(Axis::X, draft_x, maximum, enabled),
                axis_row(Axis::Y, draft_y, maximum, enabled),
            ]
            .spacing(14)
            .width(Fill),
            lock(editor.linked, enabled),
        ]
        .spacing(6)
        .align_y(iced::Center);
        let mut controls = Column::new().spacing(22).width(AXES_WIDTH);
        if stages {
            controls = controls.push(self.dpi_targets());
        }
        controls = controls.push(axes);
        if stages && let Some(actions) = self.stage_actions() {
            controls = controls.push(actions);
        }
        container(row![pad, controls].spacing(24).wrap().vertical_spacing(24))
            .padding([14, 16])
            .into()
    }

    fn discrete_dpi(&self, values: Option<&Vec<u16>>, stages: bool) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let editor = &self.state.editor;
        let mut contents = Column::new().spacing(14);
        if stages {
            contents = contents.push(self.dpi_targets());
        }
        if let Some(values) = values {
            let options = values.iter().map(|value| {
                let label = value.to_string();
                let selected = editor.target_values().is_some_and(|(x, _)| x == label);
                let message = Message::Edit(match editor.target {
                    Target::Current => Edit::DpiX(label.clone()),
                    Target::Stage(_) => Edit::Vector(*value, *value),
                });
                t3::segment(
                    format!("dpi-{value}"),
                    label,
                    selected,
                    enabled.then_some(message),
                )
            });
            contents = contents.push(t3::segmented(options));
        } else {
            contents = contents.push(t3::muted("DPI values unavailable"));
        }
        if stages && let Some(actions) = self.stage_actions() {
            contents = contents.push(actions);
        }
        container(contents).padding([14, 16]).into()
    }

    fn dpi_targets(&self) -> Element<'_, Message> {
        let editor = &self.state.editor;
        let enabled = self.state.controls_enabled();
        let target = |id: String, label: String, target| {
            t3::segment(
                id,
                label,
                editor.target == target,
                enabled.then_some(Message::Edit(Edit::Target(target))),
            )
        };
        let mut targets = vec![target(
            "target-current".into(),
            "Current".into(),
            Target::Current,
        )];
        for index in 0..editor.stages.len() {
            targets.push(target(
                format!("target-{index}"),
                (index + 1).to_string(),
                Target::Stage(index),
            ));
        }
        row![
            t3::segmented(targets),
            Space::new().width(Fill),
            t3::ghost(
                icon::PLUS,
                "Add stage",
                (enabled && !editor.stages.is_empty() && editor.stages.len() < 5)
                    .then_some(Message::Edit(Edit::AddStage)),
            ),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    }

    fn stage_actions(&self) -> Option<Element<'_, Message>> {
        let editor = &self.state.editor;
        let Target::Stage(index) = editor.target else {
            return editor
                .stages
                .is_empty()
                .then(|| t3::muted("Stages unavailable").into());
        };
        let number = u8::try_from(index + 1).unwrap_or(1);
        let active = number == editor.active;
        let enabled = self.state.controls_enabled();
        let actions = row![
            t3::outline(
                icon::CROSSHAIR,
                if active {
                    "Active stage"
                } else {
                    "Make active"
                },
                (enabled && !active).then_some(Message::Edit(Edit::ActiveStage(number))),
            ),
            Space::new().width(Fill),
            t3::ghost_danger(
                icon::TRASH_2,
                "Remove stage",
                (enabled && editor.stages.len() > 1)
                    .then_some(Message::Edit(Edit::RemoveStage(index))),
            ),
        ]
        .spacing(8)
        .align_y(iced::Center);
        Some(actions.into())
    }

    fn numeric_dpi(&self, stages: bool) -> Element<'_, Message> {
        let editor = &self.state.editor;
        let enabled = self.state.controls_enabled();
        let (x, y) = editor.target_values().unwrap_or(("", ""));
        let input = |axis, value| {
            text_input("DPI", value)
                .width(100)
                .padding([6, 10])
                .size(13)
                .line_height(t3::line_height(13))
                .style(style::input)
                .on_input_maybe(
                    enabled.then_some(move |value| Message::Edit(Edit::Axis(axis, value))),
                )
        };
        let mut contents = Column::new().spacing(14);
        if stages {
            contents = contents.push(self.dpi_targets());
        }
        contents = contents.push(
            row![
                t3::text("X", 13),
                input(Axis::X, x),
                lock(editor.linked, enabled),
                t3::text("Y", 13),
                input(Axis::Y, y),
            ]
            .spacing(12)
            .align_y(iced::Center),
        );
        if stages && let Some(actions) = self.stage_actions() {
            contents = contents.push(actions);
        }
        container(contents).padding([14, 16]).into()
    }
}

/// One logarithmic axis of the edited vector: slider plus exact input.
fn axis_row(axis: Axis, value: &str, maximum: u16, enabled: bool) -> Element<'_, Message> {
    let current = value
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(dpi_pad::MIN_DPI)
        .min(maximum);
    let change = move |value: u16| Message::Edit(Edit::Axis(axis, value.to_string()));
    row![
        t3::text(
            match axis {
                Axis::X => "X",
                Axis::Y => "Y",
            },
            13
        )
        .font(font::SEMIBOLD)
        .width(14),
        cancellable(
            slider(
                0.0..=1.0_f32,
                dpi_pad::fraction(current, maximum),
                move |position| {
                    if enabled {
                        Message::Edit(Edit::AxisPosition(axis, position))
                    } else {
                        Message::Idle
                    }
                },
            )
            .step(0.001_f32)
            .default(dpi_pad::fraction(1600, maximum))
            .height(22)
            .style(style::slider(enabled)),
        ),
        iced_aw::NumberInput::new(&current, 1..=maximum, change)
            .on_input_maybe(enabled.then_some(change))
            .ignore_buttons(true)
            .ignore_scroll(true)
            .step(50)
            .width(72)
            .padding([6, 8])
            .set_size(12)
            .line_height(t3::line_height(12))
            .font(font::REGULAR)
            .style(number_input)
            .input_style(style::input),
    ]
    .spacing(12)
    .align_y(iced::Center)
    .into()
}

/// Whether the X and Y rows move together.
fn lock(linked: bool, enabled: bool) -> Element<'static, Message> {
    t3::icon_choice(
        "dpi-linked",
        if linked { icon::LOCK } else { icon::LOCK_OPEN },
        linked,
        enabled.then_some(Message::Edit(Edit::Linked(!linked))),
    )
}

pub fn format_rate(value: u16) -> String {
    if value >= 1000 && value.is_multiple_of(1000) {
        format!("{} kHz", value / 1000)
    } else {
        format!("{value} Hz")
    }
}

pub fn format_duration(seconds: u16) -> String {
    match (seconds / 60, seconds % 60) {
        (minutes, 0) => format!("{minutes} min"),
        (0, rest) => format!("{rest} s"),
        (minutes, rest) => format!("{minutes} min {rest} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_rates_and_durations() {
        assert_eq!(format_rate(125), "125 Hz");
        assert_eq!(format_rate(1000), "1 kHz");
        assert_eq!(format_rate(8000), "8 kHz");
        assert_eq!(format_duration(300), "5 min");
        assert_eq!(format_duration(75), "1 min 15 s");
        assert_eq!(format_duration(45), "45 s");
    }
}
