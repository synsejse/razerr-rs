use super::{
    Kind, action, caption, card, chip, divider, empty_state, icon, sidebar::kind_label, tip,
};
use crate::{
    app::{App, Edit, Message},
    chart::Chart,
    dpi_pad::Ring,
    history, keyboard_button,
    state::Section,
    style::{self, Icon},
};
use iced::{
    Alignment, Element, Fill, Length,
    widget::{Row, button, canvas, column, container, image, row, text},
};
use razerr_model::{Availability, Device};

impl Section {
    fn icon(self) -> Icon {
        match self {
            Self::Lighting => Icon::Lightbulb,
            Self::Sensitivity => Icon::Crosshair,
            Self::Power => Icon::Power,
            Self::Insights => Icon::History,
        }
    }
}

impl App {
    pub(super) fn device_page(&self) -> Element<'_, Message> {
        if !self.state.received {
            return empty_state(
                Icon::Refresh,
                "Looking for devices…",
                "Connecting to the razerr-rs service.",
            );
        }
        if !self.state.connected {
            return empty_state(
                Icon::Plug,
                "Service unavailable",
                "Start razerr-rs-service in your desktop session. This window reconnects automatically.",
            );
        }
        if !self.state.snapshot.daemon_available {
            return empty_state(
                Icon::Alert,
                "OpenRazer is unavailable",
                "Start the OpenRazer daemon, then refresh.",
            );
        }
        let Some(device) = self.state.selected() else {
            return empty_state(
                Icon::Mouse,
                "No supported devices connected",
                "Connect a Razer device, then refresh.",
            );
        };
        let mut content = column![self.hero(device)].spacing(16);
        if !self.state.controls_enabled() {
            content = content.push(
                container(
                    row![
                        icon(Icon::Alert).size(18).style(style::warning_text),
                        text("Connection unavailable · showing last known values").size(13)
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .style(style::notice),
            );
        }
        for section in Section::ALL {
            if !self.state.controls_enabled() && section != Section::Insights {
                continue;
            }
            let available = match section {
                Section::Lighting => !device.zones.is_empty(),
                Section::Sensitivity => device.dpi.is_some() || device.polling.is_some(),
                Section::Power => device.idle_time.is_some() || device.low_battery.is_some(),
                Section::Insights => device.battery.is_some(),
            };
            if available {
                content = content.push(self.section(section, device));
            }
        }
        content.into()
    }

    fn section<'a>(&'a self, section: Section, device: &'a Device) -> Element<'a, Message> {
        let collapsed = self.state.collapsed.contains(&section);
        let header = row![
            container(icon(section.icon()).size(18).style(style::accent_text))
                .center_x(Length::Fixed(34.0))
                .center_y(Length::Fixed(34.0))
                .style(style::chip),
            text(section.title())
                .font(style::SEMIBOLD)
                .size(16)
                .width(Fill),
            icon(if collapsed {
                Icon::ChevronRight
            } else {
                Icon::ChevronDown
            })
            .size(18)
            .style(style::muted)
        ]
        .spacing(14)
        .align_y(Alignment::Center);
        let message = Message::Toggle(section);
        let toggle = keyboard_button::wrap(
            format!("section-{section:?}"),
            button(header)
                .width(Fill)
                .padding([14, 20])
                .style(style::ghost)
                .on_press(message.clone()),
            Some(message),
        );
        let mut body = column![toggle];
        if !collapsed {
            let contents = match section {
                Section::Lighting => self.lighting(device),
                Section::Sensitivity => self.sensitivity(device),
                Section::Power => self.power(device),
                Section::Insights => self.insights(),
            };
            body = body
                .push(divider())
                .push(container(contents).padding([18, 20]));
        }
        if self.state.editor.dirty(section) {
            body = body
                .push(divider())
                .push(pending(section, self.state.controls_enabled()));
        }
        container(body)
            .width(Fill)
            .padding(1)
            .style(style::card)
            .into()
    }

    fn hero<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        let picture: Element<'_, Message> = if let Some(handle) = device
            .image
            .as_ref()
            .and_then(|source| self.images.get(source))
        {
            image(handle.clone())
                .width(88)
                .height(88)
                .content_fit(iced::ContentFit::Contain)
                .into()
        } else {
            icon(Icon::for_kind(device.kind.as_deref()))
                .size(40)
                .style(style::muted)
                .into()
        };
        let picture = container(picture)
            .center_x(Length::Fixed(104.0))
            .center_y(Length::Fixed(104.0))
            .style(style::well);

        let mut chips = Row::new()
            .spacing(6)
            .push(chip(kind_label(device.kind.as_deref())));
        if let Some(firmware) = &device.firmware {
            chips = chips.push(chip(format!("Firmware {firmware}")));
        }
        if let Some(driver) = &device.driver {
            chips = chips.push(chip(format!("Driver {driver}")));
        }
        if let Some(usb) = device.usb {
            chips = chips.push(chip(format!("{:04x}:{:04x}", usb.vendor, usb.product)));
        }
        let mut info = column![
            text(device.name.as_deref().unwrap_or(device.serial.as_str()))
                .font(style::SEMIBOLD)
                .size(22),
            chips.wrap(),
            caption(format!("Serial {}", device.serial)),
        ]
        .spacing(8)
        .width(Fill);
        if device.battery.is_some()
            && let Some(estimate) = self.estimate(device)
        {
            info = info.push(estimate);
        }
        let mut layout = row![picture, info].spacing(20).align_y(Alignment::Center);
        if let Some(battery) = &device.battery {
            let stale = self.battery_stale(device);
            layout = layout.push(
                canvas(Ring {
                    level: battery.level.displayed().map(|level| level.get()),
                    charging: battery.charging == Some(true) && !stale,
                    stale,
                    low: device
                        .low_battery
                        .as_ref()
                        .and_then(|reading| reading.displayed())
                        .map_or(15.0, |value| f64::from(value.get())),
                })
                .width(112)
                .height(112),
            );
        }
        card(layout)
    }

    fn battery_stale(&self, device: &Device) -> bool {
        device
            .battery
            .as_ref()
            .is_none_or(|battery| battery.level.fresh().is_none())
            || !self.state.snapshot.reliable
            || device.availability != Availability::Responding
    }

    fn estimate<'a>(&'a self, device: &'a Device) -> Option<Element<'a, Message>> {
        let stale = self.battery_stale(device);
        let caption_text = if device
            .battery
            .as_ref()
            .is_some_and(|battery| battery.level.displayed().is_none())
        {
            "Battery unavailable".to_owned()
        } else if stale {
            "Cached reading".to_owned()
        } else if self.history.prediction.is_some() {
            history::estimate(self.history.prediction.as_ref())
        } else {
            return None;
        };
        let line = row![
            icon(Icon::Info).size(14).style(style::muted),
            caption(caption_text).size(13)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        Some(tip(
            line,
            history::details(self.history.prediction.as_ref()),
        ))
    }

    pub(super) fn insights(&self) -> Element<'_, Message> {
        let mut contents = column![].spacing(12);
        if let Some(error) = &self.history.error {
            contents = contents.push(
                text(format!("History unavailable: {error}"))
                    .size(13)
                    .style(style::danger_text),
            );
        }
        if self.history.points.is_empty() {
            contents = contents.push(caption("No battery history yet").size(13));
        } else {
            let legend = |color: fn(&style::Tokens) -> iced::Color, label: &'static str| {
                row![
                    container(iced::widget::Space::new())
                        .width(10)
                        .height(10)
                        .style(move |theme: &iced::Theme| container::Style {
                            background: Some(color(&style::tokens(theme)).into()),
                            border: iced::border::rounded(999),
                            ..container::Style::default()
                        }),
                    caption(label)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            };
            contents = contents
                .push(canvas(Chart(&self.history)).width(Fill).height(200))
                .push(
                    row![
                        legend(|tokens| tokens.accent, "On battery"),
                        legend(|tokens| tokens.info, "Charging"),
                    ]
                    .spacing(18)
                    .align_y(Alignment::Center),
                );
        }
        contents.into()
    }
}

/// Unapplied edits of a section: nothing reaches the device until Apply.
fn pending(section: Section, enabled: bool) -> Element<'static, Message> {
    container(
        row![
            icon(Icon::Alert).size(15).style(style::warning_text),
            text("Unapplied changes")
                .size(13)
                .style(style::warning_text),
            iced::widget::Space::new().width(Fill),
            action(
                format!("revert-{section:?}"),
                Some(Icon::Revert),
                "Revert",
                Some(Message::Revert(section)),
                Kind::Ghost,
            ),
            action(
                format!("apply-{section:?}"),
                Some(Icon::Check),
                "Apply",
                enabled.then_some(Message::Edit(Edit::Apply(section))),
                Kind::Primary,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([10, 20])
    .width(Fill)
    .style(style::pending_bar)
    .into()
}
