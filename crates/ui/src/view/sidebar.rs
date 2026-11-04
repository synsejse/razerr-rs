use super::{APP_ICON, ICON_SIZE, device_glyph, device_name, kind_label};
use crate::{
    app::{App, Message},
    state::Page,
};
use iced::{
    Element, Fill,
    widget::{Column, Space, column, container, image, row},
};
use iced_t3::{chrome, font, icon, widget as t3};
use razerr_model::{Availability, Device};

impl App {
    /// Navigation, with the device at the foot.
    pub(super) fn sidebar(&self) -> Element<'_, Message> {
        let header = chrome::drag_region(
            container(
                row![
                    image(APP_ICON.clone()).width(ICON_SIZE).height(ICON_SIZE),
                    Space::new().width(4),
                    t3::text("razerr-rs", 14).font(font::SEMIBOLD),
                ]
                .spacing(5)
                .align_y(iced::Center),
            )
            .padding([0, 10])
            .center_y(44)
            .width(Fill),
            Message::Window,
        );
        let device = self.state.selected();
        let page = self.state.current_page();
        let mut nav = Column::new().spacing(2);
        if let Some(device) = device {
            for item in Page::ALL.into_iter().filter(|item| item.available(device)) {
                nav = nav.push(t3::sidebar_nav(
                    item.title(),
                    item.glyph(),
                    item.title(),
                    !self.state.preferences && page == item,
                    Some(Message::Page(item)),
                ));
            }
        }
        nav = nav.push(t3::sidebar_nav(
            "preferences",
            icon::SETTINGS,
            "Preferences",
            self.state.preferences,
            Some(Message::Preferences),
        ));
        t3::sidebar(header, nav, self.device_footer())
    }

    fn device_footer(&self) -> Element<'_, Message> {
        let devices = &self.state.snapshot.devices;
        let mut rows = Column::new().spacing(1);
        for device in devices {
            let selected =
                !self.state.preferences && self.state.selected.as_ref() == Some(&device.serial);
            let item = t3::SidebarDevice {
                media: self.device_picture(device),
                title: device_name(device).into(),
                detail: device_detail(device).into(),
                selected,
            };
            rows = rows.push(if devices.len() > 1 {
                t3::sidebar_device(
                    format!("device-{}", device.serial),
                    item,
                    Some(Message::Select(device.serial.clone())),
                )
            } else {
                device_label(item)
            });
        }
        if devices.is_empty() {
            rows = rows.push(device_label(t3::SidebarDevice {
                media: t3::icon_tile(icon::MOUSE, t3::TileSize::Compact),
                title: "No device".into(),
                detail: if self.state.connected {
                    "Nothing connected"
                } else {
                    "Waiting for the service"
                }
                .into(),
                selected: false,
            }));
        }
        column![t3::divider(), container(rows).padding([6, 0])].into()
    }

    fn device_picture<'a>(&self, device: &Device) -> Element<'a, Message> {
        self.loaded_picture(device, 32).map_or_else(
            || t3::icon_tile(device_glyph(device.kind.as_deref()), t3::TileSize::Compact),
            Element::from,
        )
    }
}

// A single device is a passive label, not a disabled selection button.
fn device_label(item: t3::SidebarDevice<'_, Message>) -> Element<'_, Message> {
    container(
        row![
            item.media,
            column![
                t3::text(item.title, 13).font(font::MEDIUM),
                t3::muted(item.detail).size(11),
            ]
            .spacing(2)
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([6, 8])
    .into()
}

fn device_detail(device: &Device) -> String {
    if device.availability == Availability::Unavailable {
        return "Unavailable".into();
    }
    let kind = kind_label(device.kind.as_deref());
    let Some(battery) = &device.battery else {
        return kind;
    };
    match battery.level.displayed() {
        Some(level) if battery.charging == Some(true) => {
            format!("{kind} · {:.0}% · Charging", level.get())
        }
        Some(level) => format!("{kind} · {:.0}%", level.get()),
        None => kind,
    }
}
