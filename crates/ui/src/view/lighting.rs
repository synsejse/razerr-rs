use crate::{
    app::{App, Edit, Message},
    editor::{ZoneEditor, color_hex, parse_color},
};
use iced::{
    Color, Element,
    widget::{Column, Row, Space, container, pick_list, row, slider, text_input},
};
use iced_t3::{cancellable, font, style, widget as t3};
use razerr_model::{Device, Direction, EffectKind, Rgb, ZoneId};

const fn preset(red: u8, green: u8, blue: u8) -> Rgb {
    Rgb { red, green, blue }
}

/// Quick colors; a hex field covers everything else.
const PRESETS: [Rgb; 9] = [
    preset(0x44, 0xD6, 0x2C),
    preset(0xFF, 0xFF, 0xFF),
    preset(0xFF, 0x3B, 0x30),
    preset(0xFF, 0x95, 0x00),
    preset(0xFF, 0xD6, 0x0A),
    preset(0x00, 0xC7, 0xBE),
    preset(0x2F, 0x7B, 0xFF),
    preset(0xAF, 0x52, 0xDE),
    preset(0xFF, 0x2D, 0x92),
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
        let mut page = Column::new().spacing(24);
        for (cap, editor) in zones {
            page = page.push(t3::section(
                t3::section_title(zone_label(cap.id)),
                None,
                self.zone(cap, editor),
            ));
        }
        page.into()
    }

    fn zone<'a>(
        &'a self,
        cap: &'a razerr_model::ZoneCapabilities,
        editor: &'a ZoneEditor,
    ) -> Vec<Element<'a, Message>> {
        let enabled = self.state.controls_enabled();
        let id = cap.id;
        let choices: Vec<Choice> = cap.effects.iter().copied().map(Choice).collect();
        let effect = pick_list(choices, editor.kind.map(Choice), move |choice: Choice| {
            if enabled {
                Message::Edit(Edit::Kind(id, choice.0))
            } else {
                Message::Idle
            }
        })
        .placeholder("Choose an effect")
        .width(220)
        .padding([6, 10])
        .text_size(13)
        .text_line_height(t3::line_height(13))
        .font(font::REGULAR)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu);
        let mut rows = vec![t3::setting_titled("Effect", effect)];
        if let Some(kind) = editor.kind {
            if kind.color_count() > 0 {
                rows.push(color_row(id, false, "Color", &editor.first, enabled));
            }
            if kind.color_count() > 1 {
                rows.push(color_row(id, true, "Second color", &editor.second, enabled));
            }
            if kind.uses_speed() {
                let options = [(1, "Fast"), (2, "Medium"), (3, "Slow")].map(|(value, label)| {
                    t3::segment(
                        format!("speed-{id:?}-{value}"),
                        label,
                        editor.speed == value,
                        enabled.then_some(Message::Edit(Edit::Speed(id, value))),
                    )
                });
                rows.push(t3::setting_titled("Speed", t3::segmented(options)));
            }
            if kind == EffectKind::Wave {
                let options =
                    [(Direction::Left, "Left"), (Direction::Right, "Right")].map(|(way, label)| {
                        t3::segment(
                            format!("direction-{id:?}-{label}"),
                            label,
                            editor.direction == way,
                            enabled.then_some(Message::Edit(Edit::Direction(id, way))),
                        )
                    });
                rows.push(t3::setting_titled("Direction", t3::segmented(options)));
            }
        }
        if cap.brightness {
            rows.push(brightness_row(id, editor, enabled));
        }
        rows
    }
}

/// An unread brightness stays unread: no value appears until there is one.
fn brightness_row(id: ZoneId, editor: &ZoneEditor, enabled: bool) -> Element<'_, Message> {
    let known = editor.brightness_known;
    t3::setting_titled(
        "Brightness",
        row![
            cancellable(
                slider(0.0..=100.0, editor.brightness, move |value| {
                    if enabled {
                        Message::Edit(Edit::Brightness(id, value))
                    } else {
                        Message::Idle
                    }
                })
                .step(1.0)
                .width(220)
                .height(22)
                .style(style::slider(enabled && known))
            ),
            container(t3::text(
                if known {
                    format!("{:.0}%", editor.brightness)
                } else {
                    "—".to_owned()
                },
                12
            ))
            .align_right(48),
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
}

fn color_row<'a>(
    id: ZoneId,
    second: bool,
    title: &'static str,
    value: &'a str,
    enabled: bool,
) -> Element<'a, Message> {
    let parsed = parse_color(value).ok();
    let mut presets = Row::new().spacing(6).align_y(iced::Center);
    for (index, preset) in PRESETS.iter().copied().enumerate() {
        presets = presets.push(t3::swatch(
            format!("swatch-{id:?}-{second}-{index}"),
            rgb(preset),
            parsed == Some(preset),
            enabled.then(|| Message::Edit(Edit::Color(id, second, color_hex(preset)))),
        ));
    }
    let input = text_input("#RRGGBB", value)
        .width(92)
        .padding([6, 10])
        .size(12)
        .line_height(t3::line_height(12))
        .style(style::input)
        .on_input_maybe(
            enabled.then_some(move |value| Message::Edit(Edit::Color(id, second, value))),
        );
    let control = row![presets, Space::new().width(4), input]
        .spacing(8)
        .align_y(iced::Center);
    if parsed.is_none() {
        return t3::setting(title, t3::warning("Invalid color"), control);
    }
    t3::setting_titled(title, control)
}

fn rgb(color: Rgb) -> Color {
    Color::from_rgb8(color.red, color.green, color.blue)
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
