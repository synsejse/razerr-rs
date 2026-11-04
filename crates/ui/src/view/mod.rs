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
    state::Page,
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

fn kind_label(kind: Option<&str>) -> String {
    let kind = kind.unwrap_or("Device");
    let mut letters = kind.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The bounded numeric input's look: it has no step buttons, only a text field.
fn number_input(_: &iced::Theme, _: iced_aw::style::Status) -> iced_aw::style::number_input::Style {
    iced_aw::style::number_input::Style {
        button_background: None,
        icon_color: color::MUTED,
    }
}

impl App {
    /// The device's picture, once it has been downloaded.
    fn loaded_picture(&self, device: &Device, size: u32) -> Option<image::Image> {
        let source = device.image.as_ref()?;
        let handle = self.images.get(source)?;
        Some(
            image(handle.clone())
                .width(size)
                .height(size)
                .content_fit(iced::ContentFit::Contain),
        )
    }

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

    /// What the app is doing or just finished, in words. A change the docked bar
    /// reports on stays there instead.
    fn activity_line(&self) -> Option<Element<'_, Message>> {
        let docked = self.docked_control();
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

    /// The change this page's docked bar reports on, if it has one.
    fn docked_control(&self) -> Option<Control> {
        if self.state.preferences {
            return Some(Control::Save);
        }
        self.state.selected()?;
        self.state.current_page().section().map(Control::Apply)
    }

    /// Whether this page has edits waiting, and why they can't go yet if so.
    fn draft(&self, control: Control) -> (bool, Option<String>) {
        if let Control::Apply(section) = control {
            return (
                self.state.editor.dirty(section),
                edit::commands(&self.state, section).err(),
            );
        }
        let valid = self.state.draft.validated();
        (valid.as_ref() != Ok(&self.state.settings), valid.err())
    }

    /// What the docked bar has to say and in which color. With nothing to say
    /// there is no bar, on any page.
    pub(crate) fn docked_status(&self) -> Option<(String, iced::Color)> {
        let control = self.docked_control()?;
        let (dirty, invalid) = self.draft(control);
        if let Some(activity) = self.pending.as_ref().filter(|_| self.in_flight(control)) {
            Some((activity.progress.clone(), color::MUTED))
        } else if let Some(error) = invalid.filter(|_| dirty) {
            Some((error, color::DANGER))
        } else if dirty {
            let status = if control == Control::Save {
                "Unsaved changes"
            } else {
                "Unapplied changes"
            };
            Some((status.to_owned(), color::WARNING))
        } else {
            self.confirmed
                .as_ref()
                .filter(|confirmed| confirmed.control == control)
                .map(|confirmed| (confirmed.text.clone(), color::SUCCESS))
        }
    }

    /// Edits are drafts; the bar keeps them and their action in view while the
    /// page scrolls. Every page uses this one bar, so saving works the same.
    fn docked_bar(&self) -> Option<Element<'_, Message>> {
        let (status, tint) = self.docked_status()?;
        let control = self.docked_control()?;
        let (dirty, invalid) = self.draft(control);
        let working = self.in_flight(control);
        let (label, revert, action, usable) = match control {
            Control::Apply(section) => (
                "Apply",
                Message::Revert(section),
                Message::Edit(Edit::Apply(section)),
                self.state.controls_enabled(),
            ),
            _ => ("Save", Message::RevertPreferences, Message::Save, true),
        };
        let marker: Element<'_, Message> = if working {
            t3::dots()
        } else if dirty {
            t3::icon(icon::INFO).size(14).color(tint).into()
        } else {
            t3::check()
        };
        Some(t3::composer(
            row![
                marker,
                t3::text(status, 13).color(tint).width(Fill),
                t3::ghost(icon::UNDO_2, "Revert", dirty.then_some(revert)),
                if working {
                    t3::primary_busy(label)
                } else {
                    t3::primary(
                        label,
                        (dirty && invalid.is_none() && usable).then_some(action),
                    )
                },
            ]
            .spacing(8)
            .align_y(iced::Center),
            PAGE_WIDTH,
            dirty,
        ))
    }
}
