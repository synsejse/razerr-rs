mod device;
mod lighting;
mod power;
mod preferences;
mod sensitivity;
mod sidebar;

use std::sync::LazyLock;

use iced::{
    Element, Fill, Padding,
    widget::{column, container, image, row},
};
use iced_t3::{Tone, chrome, color, icon, widget as t3};
use razerr_model::Device;

use crate::{
    activity::Control,
    app::{App, Edit, Message},
    edit,
    state::{Page, Section},
};

pub use sensitivity::format_rate;

/// Width of the page column (T3 Code's `max-w-4xl` less its gutters).
pub const PAGE_WIDTH: f32 = 832.0;
/// Display size of the app icon by the title.
pub const ICON_SIZE: u32 = 22;
pub const ICON_PNG: &[u8] = include_bytes!("../../../../assets/icons/razerr-rs.png");
/// The app icon, scaled once to double size so it stays sharp at high DPI.
pub static APP_ICON: LazyLock<image::Handle> = LazyLock::new(|| {
    let size = ICON_SIZE * 2;
    razerr_images::thumbnail(ICON_PNG, size).map_or_else(
        |_| image::Handle::from_rgba(1, 1, vec![0; 4]),
        |icon| image::Handle::from_rgba(icon.width, icon.height, icon.pixels),
    )
});

impl Page {
    fn glyph(self) -> char {
        match self {
            Self::Sensitivity => icon::CROSSHAIR,
            Self::Lighting => icon::LIGHTBULB,
            Self::Power => icon::BATTERY_MEDIUM,
            Self::Device => icon::INFO,
        }
    }
}

/// A device-type icon for OpenRazer's free-form type string.
fn device_glyph(kind: Option<&str>) -> char {
    match kind.unwrap_or_default().to_ascii_lowercase().as_str() {
        "keyboard" | "keypad" => icon::KEYBOARD,
        "headset" => icon::HEADPHONES,
        "mouse" | "mousemat" => icon::MOUSE,
        _ => icon::PLUG,
    }
}

fn device_name(device: &Device) -> &str {
    device.name.as_deref().unwrap_or(device.serial.as_str())
}

/// The bounded numeric input's look: it has no step buttons, only a text field.
fn number_input(_: &iced::Theme, _: iced_aw::style::Status) -> iced_aw::style::number_input::Style {
    iced_aw::style::number_input::Style {
        button_background: None,
        icon_color: color::MUTED,
    }
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let mut page = column![].spacing(24).max_width(PAGE_WIDTH);
        if let Some(error) = &self.state.error {
            page = page.push(t3::alert_dismissible(
                "dismiss-error",
                icon::CIRCLE_ALERT,
                error,
                Tone::Danger,
                Message::Dismiss,
            ));
        }
        page = page.push(if self.state.preferences {
            self.preferences()
        } else {
            self.device_page()
        });
        let content = t3::scroll(
            container(page)
                .padding(Padding::from([24.0, t3::PAGE_GUTTER]).top(8))
                .center_x(Fill),
        )
        .height(Fill);
        let mut main = column![self.titlebar(), content];
        if let Some(bar) = self.docked_bar() {
            main = main.push(bar);
        }
        let window = row![self.sidebar(), t3::sidebar_divider(), main];
        let window: Element<'_, Message> = if let Some(kind) = self.state.confirmation {
            t3::modal(window, Self::dialog(kind), Message::Cancel)
        } else {
            window.into()
        };
        chrome::frame(window, Message::Window)
    }

    fn device_page(&self) -> Element<'_, Message> {
        if !self.state.received {
            return t3::empty_state(
                t3::dots(),
                "Looking for devices…",
                ["Connecting to the razerr-rs service."],
            );
        }
        if !self.state.connected {
            return t3::empty_state(
                t3::icon_tile(icon::UNPLUG, 36),
                "Service unavailable",
                [
                    "Start razerr-rs-service in your desktop session.",
                    "This window reconnects automatically.",
                ],
            );
        }
        if !self.state.snapshot.daemon_available {
            return t3::empty_state(
                t3::icon_tile(icon::TRIANGLE_ALERT, 36),
                "OpenRazer is unavailable",
                ["Start the OpenRazer daemon, then refresh."],
            );
        }
        let Some(device) = self.state.selected() else {
            return t3::empty_state(
                t3::icon_tile(icon::MOUSE, 36),
                "No supported devices connected",
                ["Connect a Razer device, then refresh."],
            );
        };
        let mut content = column![].spacing(24);
        if !self.state.controls_enabled() {
            content = content.push(t3::alert(
                icon::TRIANGLE_ALERT,
                "Connection unavailable · showing last known values",
                Tone::Warning,
            ));
        }
        content
            .push(match self.state.current_page() {
                Page::Sensitivity => self.sensitivity(device),
                Page::Lighting => self.lighting(device),
                Page::Power => self.power(device),
                Page::Device => self.device_details(device),
            })
            .into()
    }

    /// Dots while `control` is in flight, then a check once the device confirms it.
    fn progress<'a>(&self, control: Control) -> Option<Element<'a, Message>> {
        if self.in_flight(control) {
            Some(t3::dots())
        } else if self.just_confirmed(control) {
            Some(t3::check())
        } else {
            None
        }
    }

    /// `content` with `progress` beside it. Controls sit at the right of their row,
    /// so progress appearing doesn't move them.
    fn with_progress<'a>(
        progress: Option<Element<'a, Message>>,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let mut with_status = row![].spacing(10).align_y(iced::Center);
        if let Some(progress) = progress {
            with_status = with_status.push(progress);
        }
        with_status.push(content).into()
    }

    /// What the app is doing or just finished, in words. A section's Apply reports
    /// in its docked bar instead while that bar is on screen.
    fn activity_line(&self) -> Option<Element<'_, Message>> {
        let docked = self.docked_section().map(Control::Apply);
        if let Some(activity) = self.pending.as_ref() {
            return (Some(activity.control) != docked).then(|| {
                row![t3::dots(), t3::muted(&activity.progress)]
                    .spacing(8)
                    .align_y(iced::Center)
                    .into()
            });
        }
        let confirmed = self.confirmed.as_ref()?;
        (Some(confirmed.control) != docked).then(|| {
            row![
                t3::check(),
                t3::text(&confirmed.text, 12).color(color::SUCCESS),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into()
        })
    }

    fn titlebar(&self) -> Element<'_, Message> {
        let device = self.state.selected();
        let activity = self.activity_line();
        // The activity needs the room more: the device is named in the sidebar too.
        let parents = device.filter(|_| activity.is_none()).map(device_name);
        let path = if self.state.preferences {
            t3::breadcrumb(icon::SETTINGS, [], "Preferences")
        } else if device.is_some() {
            let page = self.state.current_page();
            t3::breadcrumb(page.glyph(), parents, page.title())
        } else {
            t3::breadcrumb(icon::MOUSE, [], "razerr-rs")
        };
        let mut leading = row![path].spacing(16).align_y(iced::Center);
        if let Some(line) = activity {
            leading = leading.push(line);
        }
        let refresh = if self.in_flight(Control::Refresh) {
            container(t3::spinner()).center_x(26).center_y(26).into()
        } else {
            t3::tooltip(
                t3::icon_button(
                    "refresh",
                    icon::REFRESH_CW,
                    self.state.connected.then_some(Message::Refresh),
                ),
                "Refresh · Ctrl+R",
                t3::Position::Bottom,
            )
        };
        chrome::bar(leading, Some(refresh), Message::Window)
    }

    /// The section whose Apply bar this page can show.
    fn docked_section(&self) -> Option<Section> {
        if self.state.preferences {
            return None;
        }
        self.state.selected()?;
        self.state.current_page().section()
    }

    fn docked_bar(&self) -> Option<Element<'_, Message>> {
        if self.state.preferences {
            return Some(self.preferences_bar());
        }
        let section = self.docked_section()?;
        let control = Control::Apply(section);
        (self.state.editor.dirty(section)
            || self.in_flight(control)
            || self.just_confirmed(control))
        .then(|| self.apply_bar(section))
    }

    /// Device edits are drafts; the bar keeps them and Apply in view while the page
    /// scrolls. Nothing reaches the device until Apply.
    fn apply_bar(&self, section: Section) -> Element<'_, Message> {
        let control = Control::Apply(section);
        let saving = self.in_flight(control);
        let dirty = self.state.editor.dirty(section);
        let invalid = edit::commands(&self.state, section).err();
        let (status, tint): (&str, _) =
            if let Some(activity) = self.pending.as_ref().filter(|_| saving) {
                (&activity.progress, color::MUTED)
            } else if let Some(error) = invalid.as_deref().filter(|_| dirty) {
                (error, color::DANGER)
            } else if dirty {
                ("Unapplied changes", color::WARNING)
            } else {
                let confirmed = self.confirmed.as_ref().filter(|_| !dirty);
                (
                    confirmed.map_or("", |confirmed| confirmed.text.as_str()),
                    color::SUCCESS,
                )
            };
        let marker: Element<'_, Message> = if saving {
            t3::dots()
        } else if dirty {
            t3::icon(icon::INFO).size(14).color(tint).into()
        } else {
            t3::check()
        };
        let can_apply = dirty && invalid.is_none() && self.state.controls_enabled();
        t3::composer(
            row![
                marker,
                t3::text(status.to_owned(), 13).color(tint).width(Fill),
                t3::ghost(
                    icon::UNDO_2,
                    "Revert",
                    dirty.then_some(Message::Revert(section)),
                ),
                if saving {
                    t3::primary_busy("Apply")
                } else {
                    t3::primary(
                        "Apply",
                        can_apply.then_some(Message::Edit(Edit::Apply(section))),
                    )
                },
            ]
            .spacing(8)
            .align_y(iced::Center),
            PAGE_WIDTH,
            dirty,
        )
    }
}
