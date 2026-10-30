use super::{Kind, action, caption, divider, heading, icon, icon_button, segmented, setting, tip};
use crate::{
    app::{App, Edit, Message},
    dpi_pad::{self, Pad},
    keyboard_button::{self, cancellable},
    state::{Axis, Target},
    style::{self, Icon},
};
use iced::{
    Alignment, Element, Fill, Length,
    widget::{Column, Space, button, canvas, column, container, row, slider, text, text_input},
};
use razerr_model::{Device, DpiRange};

impl App {
    pub(super) fn sensitivity(&self, device: &Device) -> Element<'_, Message> {
        let mut contents = Column::new().spacing(22);
        if let Some(dpi) = &device.dpi {
            match &dpi.range {
                DpiRange::Continuous { maximum } => {
                    if let Some(maximum) = maximum.displayed() {
                        contents = contents
                            .push(self.vector_editor(*maximum, device.capabilities.dpi_stages));
                    } else {
                        contents = contents.push(self.numeric_dpi(device.capabilities.dpi_stages));
                    }
                }
                DpiRange::Discrete { values } => {
                    contents = contents.push(
                        self.discrete_dpi(values.displayed(), device.capabilities.dpi_stages),
                    );
                }
            }
        } else if device.capabilities.dpi_stages {
            contents = contents.push(self.numeric_dpi(true));
        }
        if let Some(polling) = &device.polling {
            contents = contents.push(divider()).push(self.polling(polling));
        }
        contents.into()
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
            .style(style::well);

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
        .align_y(Alignment::Center);

        column![
            heading("DPI"),
            row![
                pad,
                column![self.dpi_targets(stages), axes, self.stage_actions(stages)]
                    .spacing(22)
                    .width(Fill)
            ]
            .spacing(24)
            .align_y(Alignment::Start)
        ]
        .spacing(14)
        .into()
    }

    fn discrete_dpi(&self, values: Option<&Vec<u16>>, stages: bool) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let editor = &self.state.editor;
        let mut contents = column![heading("DPI"), self.dpi_targets(stages)].spacing(14);
        if let Some(values) = values {
            let options = values
                .iter()
                .map(|value| {
                    (
                        value.to_string(),
                        Message::Edit(match editor.target {
                            Target::Current => Edit::DpiX(value.to_string()),
                            Target::Stage(_) => Edit::Vector(*value, *value),
                        }),
                        editor
                            .target_values()
                            .is_some_and(|(x, _)| x == value.to_string()),
                    )
                })
                .collect();
            contents = contents.push(row![segmented("dpi-choice", options, enabled)].wrap());
        } else {
            contents = contents.push(caption("DPI values unavailable"));
        }
        contents.push(self.stage_actions(stages)).into()
    }

    fn dpi_targets(&self, stages: bool) -> Element<'_, Message> {
        if !stages {
            return Space::new().into();
        }
        let editor = &self.state.editor;
        let mut targets = vec![(
            "Current".to_owned(),
            Message::Edit(Edit::Target(Target::Current)),
            editor.target == Target::Current,
        )];
        for index in 0..editor.stages.len() {
            targets.push((
                (index + 1).to_string(),
                Message::Edit(Edit::Target(Target::Stage(index))),
                editor.target == Target::Stage(index),
            ));
        }

        row![
            segmented("dpi-target", targets, self.state.controls_enabled()),
            icon_button(
                "stage-add",
                Icon::Plus,
                "Add stage",
                (self.state.controls_enabled()
                    && !editor.stages.is_empty()
                    && editor.stages.len() < 5)
                    .then_some(Message::Edit(Edit::AddStage)),
                Kind::Ghost,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .wrap()
        .into()
    }

    fn stage_actions(&self, stages: bool) -> Element<'_, Message> {
        if !stages {
            return Space::new().into();
        }
        let editor = &self.state.editor;
        let Target::Stage(index) = editor.target else {
            return caption(if editor.stages.is_empty() {
                "Stages unavailable".to_owned()
            } else {
                format!("Active stage: {}", editor.active)
            })
            .into();
        };
        let number = u8::try_from(index + 1).unwrap_or(1);
        let active = number == editor.active;
        let enabled = self.state.controls_enabled();
        row![
            action(
                "stage-activate",
                Some(Icon::Crosshair),
                if active {
                    "Active stage"
                } else {
                    "Make active"
                },
                (enabled && !active).then_some(Message::Edit(Edit::ActiveStage(number))),
                Kind::Secondary,
            ),
            Space::new().width(Fill),
            icon_button(
                "stage-remove",
                Icon::Trash,
                "Remove stage",
                (enabled && editor.stages.len() > 1)
                    .then_some(Message::Edit(Edit::RemoveStage(index))),
                Kind::GhostDanger,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    fn numeric_dpi(&self, stages: bool) -> Element<'_, Message> {
        let editor = &self.state.editor;
        let (x, y) = editor.target_values().unwrap_or(("", ""));
        let input = |axis, value| {
            text_input("DPI", value)
                .width(100)
                .padding([6, 10])
                .style(style::input)
                .on_input_maybe(
                    self.state
                        .controls_enabled()
                        .then_some(move |value| Message::Edit(Edit::Axis(axis, value))),
                )
        };
        column![
            heading("DPI"),
            self.dpi_targets(stages),
            row![
                text("X"),
                input(Axis::X, x),
                lock(editor.linked, self.state.controls_enabled()),
                text("Y"),
                input(Axis::Y, y)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            self.stage_actions(stages),
        ]
        .spacing(14)
        .into()
    }

    fn polling(&self, polling: &razerr_model::Polling) -> Element<'_, Message> {
        let enabled = self.state.controls_enabled();
        let selected = self.state.editor.poll;
        let contents = column![heading("Polling rate")].spacing(10);
        if polling.supported.is_empty() {
            return contents.push(caption("Polling rates unavailable")).into();
        }
        let options = polling
            .supported
            .iter()
            .map(|rate| {
                let value = rate.get();
                (
                    format_rate(value),
                    Message::Edit(Edit::Poll(value)),
                    selected == Some(value),
                )
            })
            .collect();
        let mut line = row![segmented("poll", options, enabled)]
            .spacing(14)
            .align_y(Alignment::Center);
        if selected.is_some_and(|rate| rate >= 4000) {
            line = line.push(tip(
                container(
                    row![
                        icon(Icon::BatteryWarning).size(13),
                        text("Higher power use").size(12)
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([3, 10])
                .style(style::warning_chip),
                "4000 Hz and above uses more CPU and drains a wireless battery faster",
            ));
        }
        contents.push(line.wrap()).into()
    }

    pub(super) fn power(&self, device: &Device) -> Element<'_, Message> {
        let editor = &self.state.editor;
        let mut contents = Column::new().spacing(18);
        if device.idle_time.is_some() {
            let seconds = editor.idle.parse::<u16>().unwrap_or(300).clamp(60, 900);
            contents = contents.push(setting(
                "Sleep after",
                None,
                row![
                    cancellable(
                        slider(60_u16..=900, seconds, |value| {
                            Message::Edit(Edit::Idle(value.to_string()))
                        })
                        .step(15_u16)
                        .width(240)
                        .height(22)
                        .style(style::slider)
                    ),
                    text(format_duration(seconds)).size(14).width(72)
                ]
                .spacing(14)
                .align_y(Alignment::Center),
            ));
        }
        if device.idle_time.is_some() && device.low_battery.is_some() {
            contents = contents.push(divider());
        }
        if device.low_battery.is_some() {
            let percent = editor.threshold.parse::<u8>().unwrap_or(15).clamp(5, 25);
            contents = contents.push(setting(
                "Low battery warning",
                None,
                row![
                    cancellable(
                        slider(5_u8..=25, percent, |value| {
                            Message::Edit(Edit::Threshold(value.to_string()))
                        })
                        .width(240)
                        .height(22)
                        .style(style::slider)
                    ),
                    text(format!("{percent}%")).size(14).width(72)
                ]
                .spacing(14)
                .align_y(Alignment::Center),
            ));
        }
        contents.into()
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
    row![
        text(match axis {
            Axis::X => "X",
            Axis::Y => "Y",
        })
        .font(style::SEMIBOLD)
        .size(13)
        .width(14),
        container(cancellable(
            iced_audio::HSlider::new(iced_audio::NormalParam::new(
                dpi_pad::fraction(current, maximum),
                dpi_pad::fraction(1600, maximum),
            ))
            .on_gesture(move |gesture| Message::Edit(Edit::AxisGesture(axis, gesture)))
            .enabled(enabled)
            .config(&iced_audio::virtual_slider::Config {
                wheel_scalar: 0.0,
                ..Default::default()
            })
            .height(Length::Fixed(12.0))
            .style(style::DpiSlider)
        ))
        .width(Fill),
        iced_aw::NumberInput::new(&current, 1..=maximum, move |value: u16| {
            Message::Edit(Edit::Axis(axis, value.to_string()))
        })
        .on_input_maybe(
            enabled.then_some(move |value: u16| Message::Edit(Edit::Axis(axis, value.to_string())))
        )
        .step(50)
        .ignore_scroll(true)
        .width(106)
        .padding([6, 8])
        .set_size(14)
        .style(style::number_input)
        .input_style(style::input),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// The lock between the X and Y rows: locked axes move together.
fn lock(linked: bool, enabled: bool) -> Element<'static, Message> {
    let line = || {
        container(Space::new())
            .width(1)
            .height(10)
            .style(if linked {
                style::accent_line
            } else {
                style::divider
            })
    };
    let message = enabled.then_some(Message::Edit(Edit::Linked(!linked)));
    let control = keyboard_button::wrap(
        "dpi-linked",
        button(
            container(icon(if linked { Icon::Lock } else { Icon::LockOpen }))
                .center_x(Length::Fixed(18.0))
                .center_y(Length::Fixed(18.0)),
        )
        .padding(7)
        .style(style::lock(linked))
        .on_press_maybe(message.clone()),
        message,
    );
    column![
        line(),
        tip(
            control,
            if linked {
                "X and Y locked"
            } else {
                "Lock X and Y"
            }
        ),
        line()
    ]
    .align_x(Alignment::Center)
    .into()
}

fn format_rate(value: u16) -> String {
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
