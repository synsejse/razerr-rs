use super::{device_glyph, device_name, kind_label};
use crate::app::{App, Message};
use iced::{
    Element, Fill,
    widget::{column, container, row},
};
use iced_t3::{color, font, style, widget as t3};
use razerr_model::{Availability, Device};

impl App {
    pub(super) fn device_details<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        let picture: Element<'_, Message> = self.loaded_picture(device, 80).map_or_else(
            || {
                t3::icon(device_glyph(device.kind.as_deref()))
                    .size(36)
                    .color(color::MUTED)
                    .into()
            },
            Element::from,
        );
        let hero = container(
            row![
                container(picture)
                    .center_x(96)
                    .center_y(96)
                    .style(style::inset),
                column![
                    t3::text(device_name(device), 16).font(font::SEMIBOLD),
                    t3::muted(kind_label(device.kind.as_deref())),
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(16)
            .align_y(iced::Center),
        )
        .padding([14, 16]);
        let fact = |title, value: String| {
            t3::setting_titled(title, t3::text(value, 13).color(color::MUTED))
        };
        let mut rows = vec![hero.into()];
        rows.push(fact(
            "Connection",
            match device.availability {
                Availability::Responding => "Responding",
                Availability::Unknown => "Not reported",
                Availability::Unavailable => "Unavailable",
            }
            .to_owned(),
        ));
        if let Some(firmware) = &device.firmware {
            rows.push(fact("Firmware", firmware.clone()));
        }
        if let Some(driver) = &device.driver {
            rows.push(fact("Driver", driver.clone()));
        }
        if let Some(usb) = device.usb {
            rows.push(fact(
                "USB ID",
                format!("{:04x}:{:04x}", usb.vendor, usb.product),
            ));
        }
        rows.push(fact("Serial", device.serial.to_string()));
        t3::section(t3::section_title("Device"), None, rows)
    }
}
