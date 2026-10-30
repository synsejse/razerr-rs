use super::{divider, heading, segmented, setting};
use crate::keyboard_button::{self, cancellable};
use crate::{
    app::{App, Edit, Message},
    state::{ZoneEditor, parse_color},
    style,
};
use iced::{
    Alignment, Color, Element,
    widget::{
        Column, Row, Space, button, column, container, pick_list, row, slider, text, text_input,
    },
};
use razerr_model::{Device, Direction, EffectKind, Rgb, ZoneId};

/// Quick colors; a hex field covers everything else.
const PRESETS: [Rgb; 9] = [
    Rgb {
        red: 0x44,
        green: 0xD6,
        blue: 0x2C,
    },
    Rgb {
        red: 0xFF,
        green: 0xFF,
        blue: 0xFF,
    },
    Rgb {
        red: 0xFF,
        green: 0x3B,
        blue: 0x30,
    },
    Rgb {
        red: 0xFF,
        green: 0x95,
        blue: 0x00,
    },
    Rgb {
        red: 0xFF,
        green: 0xD6,
        blue: 0x0A,
    },
    Rgb {
        red: 0x00,
        green: 0xC7,
        blue: 0xBE,
    },
    Rgb {
        red: 0x2F,
        green: 0x7B,
        blue: 0xFF,
    },
    Rgb {
        red: 0xAF,
        green: 0x52,
        blue: 0xDE,
    },
    Rgb {
        red: 0xFF,
        green: 0x2D,
        blue: 0x92,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Choice(EffectKind);

impl std::fmt::Display for Choice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(effect_label(self.0))
    }
}

impl App {
    pub(super) fn lighting<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        let mut contents = Column::new().spacing(18);
        let zones: Vec<_> = device
            .capabilities
            .zones
            .iter()
            .filter_map(|cap| {
                self.state
                    .editor
                    .zones
                    .get(&cap.id)
                    .map(|editor| (cap, editor))
            })
            .collect();
        for (position, (cap, editor)) in zones.iter().enumerate() {
            if position > 0 {
                contents = contents.push(divider());
            }
            contents = contents.push(self.zone(cap, editor, zones.len() > 1));
        }
        contents.into()
    }

    fn zone<'a>(
        &'a self,
        cap: &'a razerr_model::ZoneCapabilities,
        editor: &'a ZoneEditor,
        titled: bool,
    ) -> Element<'a, Message> {
        let enabled = self.state.controls_enabled();
        let id = cap.id;
        let mut zone = column![].spacing(16);
        if titled {
            zone = zone.push(heading(zone_label(id)));
        }
        let choices: Vec<Choice> = cap.effects.iter().copied().map(Choice).collect();
        let effect = pick_list(choices, editor.kind.map(Choice), move |choice: Choice| {
            Message::Edit(Edit::Kind(id, choice.0))
        })
        .placeholder("Choose an effect")
        .width(240)
        .padding([7, 12])
        .text_size(14)
        .style(style::select)
        .menu_style(style::select_menu);
        zone = zone.push(setting("Effect", None, effect));
        if let Some(kind) = editor.kind {
            if kind.color_count() > 0 {
                zone = zone.push(color_row(id, false, "Color", &editor.first, enabled));
            }
            if kind.color_count() > 1 {
                zone = zone.push(color_row(id, true, "Second color", &editor.second, enabled));
            }
            if kind.uses_speed() {
                let options = [(1, "Fast"), (2, "Medium"), (3, "Slow")]
                    .into_iter()
                    .map(|(value, label)| {
                        (
                            label.to_owned(),
                            Message::Edit(Edit::Speed(id, value)),
                            editor.speed == value,
                        )
                    })
                    .collect();
                zone = zone.push(setting(
                    "Speed",
                    None,
                    segmented(&format!("speed-{id:?}"), options, enabled),
                ));
            }
            if kind == EffectKind::Wave {
                let options = [(Direction::Left, "Left"), (Direction::Right, "Right")]
                    .into_iter()
                    .map(|(direction, label)| {
                        (
                            label.to_owned(),
                            Message::Edit(Edit::Direction(id, direction)),
                            editor.direction == direction,
                        )
                    })
                    .collect();
                zone = zone.push(setting(
                    "Direction",
                    None,
                    segmented(&format!("direction-{id:?}"), options, enabled),
                ));
            }
        }
        if cap.brightness {
            zone = zone.push(setting(
                "Brightness",
                None,
                row![
                    cancellable(
                        slider(0.0..=100.0, editor.brightness, move |value| {
                            Message::Edit(Edit::Brightness(id, value))
                        })
                        .step(1.0)
                        .width(240)
                        .height(22)
                        .style(style::slider)
                    ),
                    text(format!("{:.0}%", editor.brightness))
                        .size(14)
                        .width(72)
                ]
                .spacing(14)
                .align_y(Alignment::Center),
            ));
        }
        zone.into()
    }
}

fn color_row<'a>(
    id: ZoneId,
    second: bool,
    title: &'static str,
    value: &'a str,
    enabled: bool,
) -> Element<'a, Message> {
    let parsed = parse_color(value).ok();
    let preview = container(Space::new().width(30).height(30)).style(style::swatch_fill(
        parsed.map_or(Color::TRANSPARENT, style::rgb),
    ));
    let mut presets = Row::new().spacing(6).align_y(Alignment::Center);
    for (index, preset) in PRESETS.iter().copied().enumerate() {
        let message = enabled.then_some(Message::Edit(Edit::Swatch(id, second, preset)));
        presets = presets.push(keyboard_button::wrap(
            format!("swatch-{id:?}-{second}-{index}"),
            button(Space::new().width(18).height(18))
                .padding(0)
                .style(style::swatch(style::rgb(preset), parsed == Some(preset)))
                .on_press_maybe(message.clone()),
            message,
        ));
    }
    let input = text_input("#RRGGBB", value)
        .width(104)
        .padding([6, 10])
        .size(14)
        .style(style::input)
        .on_input_maybe(
            enabled.then_some(move |value| Message::Edit(Edit::Color(id, second, value))),
        );
    let mut control = column![
        row![presets, Space::new().width(6), preview, input]
            .spacing(8)
            .align_y(Alignment::Center)
    ]
    .spacing(4)
    .align_x(Alignment::End);
    if parsed.is_none() {
        control = control.push(text("Invalid color").size(12).style(style::danger_text));
    }
    setting(title, None, control)
}

fn zone_label(id: ZoneId) -> &'static str {
    match id {
        ZoneId::Main => "Main",
        ZoneId::Logo => "Logo",
        ZoneId::Scroll => "Scroll wheel",
        ZoneId::Backlight => "Backlight",
        ZoneId::Left => "Left side",
        ZoneId::Right => "Right side",
    }
}

fn effect_label(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Off => "Off",
        EffectKind::Static => "Static",
        EffectKind::Spectrum => "Spectrum",
        EffectKind::Wave => "Wave",
        EffectKind::Breath => "Breathing",
        EffectKind::BreathDual => "Breathing · two colors",
        EffectKind::BreathRandom => "Breathing · random",
        EffectKind::Reactive => "Reactive",
        EffectKind::Starlight => "Starlight",
        EffectKind::StarlightDual => "Starlight · two colors",
        EffectKind::StarlightRandom => "Starlight · random",
        EffectKind::Ripple => "Ripple",
        EffectKind::RippleRandom => "Ripple · random",
    }
}
