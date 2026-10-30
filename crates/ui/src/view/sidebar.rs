use super::{caption, divider, icon};
use crate::{
    app::{App, Message},
    keyboard_button,
    style::{self, Icon},
};
use iced::{
    Alignment, Element, Fill, Length,
    widget::{Column, Space, button, column, container, image, row, scrollable, text},
};
use razerr_model::Device;

impl App {
    pub(super) fn sidebar(&self) -> Element<'_, Message> {
        let brand = row![
            image(self.icon.clone()).width(26).height(26),
            text("razerr-rs").font(style::SEMIBOLD).size(17)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut devices = Column::new().spacing(4);
        for device in &self.state.snapshot.devices {
            devices = devices.push(self.device_tile(device));
        }
        if self.state.snapshot.devices.is_empty() {
            devices = devices.push(container(caption("No devices")).padding([6, 10]));
        }

        let refreshing = self.state.snapshot.refreshing;
        let footer = column![
            super::tip(
                nav(
                    "refresh",
                    Icon::Refresh,
                    if refreshing {
                        "Refreshing…"
                    } else {
                        "Refresh devices"
                    },
                    (!refreshing && self.state.connected).then_some(Message::Refresh),
                ),
                "Ctrl+R",
            ),
            nav(
                "preferences",
                Icon::Settings,
                "Preferences",
                Some(Message::Preferences),
            ),
        ]
        .spacing(2);

        container(
            column![
                container(brand).padding([4, 8]),
                Space::new().height(18),
                container(caption("DEVICES").size(11)).padding([0, 10]),
                Space::new().height(6),
                scrollable(devices).height(Fill),
                divider(),
                footer,
            ]
            .spacing(6),
        )
        .width(260)
        .height(Fill)
        .padding([20, 12])
        .style(style::sidebar)
        .into()
    }

    fn device_tile<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        let selected =
            !self.state.preferences && self.state.selected.as_ref() == Some(&device.serial);
        let thumbnail: Element<'_, Message> = if let Some(handle) = device
            .image
            .as_ref()
            .and_then(|source| self.images.get(source))
        {
            image(handle.clone())
                .width(40)
                .height(40)
                .content_fit(iced::ContentFit::Contain)
                .into()
        } else {
            container(icon(Icon::for_kind(device.kind.as_deref())).size(18))
                .center_x(Length::Fixed(40.0))
                .center_y(Length::Fixed(40.0))
                .style(style::chip)
                .into()
        };
        let mut details =
            column![text(device.name.as_deref().unwrap_or(device.serial.as_str())).size(13),]
                .spacing(3);
        let mut meta = row![caption(kind_label(device.kind.as_deref()))]
            .spacing(6)
            .align_y(Alignment::Center);
        if let Some(level) = device
            .battery
            .as_ref()
            .and_then(|battery| battery.level.displayed())
        {
            let charging = device
                .battery
                .as_ref()
                .is_some_and(|battery| battery.charging == Some(true));
            meta = meta.push(caption("·")).push(
                icon(if charging {
                    Icon::BatteryCharging
                } else {
                    Icon::Battery
                })
                .size(13)
                .style(style::muted),
            );
            meta = meta.push(caption(format!("{:.0}%", level.get())));
        }
        if device.availability == razerr_model::Availability::Unavailable {
            meta = meta.push(caption("· Unavailable"));
        }
        details = details.push(meta);
        let message = Message::Select(device.serial.clone());
        let control = button(
            row![thumbnail, details.width(Fill)]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .padding([8, 10])
        .width(Fill)
        .style(style::tile(selected))
        .on_press(message.clone());
        keyboard_button::wrap(format!("device-{}", device.serial), control, Some(message))
    }
}

pub fn kind_label(kind: Option<&str>) -> String {
    let kind = kind.unwrap_or("Device");
    let mut letters = kind.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// A full-width sidebar action.
fn nav(
    id: &'static str,
    glyph: Icon,
    label: &'static str,
    message: Option<Message>,
) -> Element<'static, Message> {
    let control = button(
        row![icon(glyph).size(15), text(label).size(14)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([8, 12])
    .style(style::ghost)
    .on_press_maybe(message.clone());
    keyboard_button::wrap(id, control, message)
}
