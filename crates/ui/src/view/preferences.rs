use super::PAGE_WIDTH;
use crate::{
    activity::Control,
    app::{App, Message, Preference},
    state::Confirmation,
};
use iced::{
    Element, Fill,
    widget::{Column, row, text_input},
};
use iced_t3::{color, icon, style, widget as t3};

impl App {
    pub(super) fn preferences(&self) -> Element<'_, Message> {
        Column::new()
            .spacing(24)
            .push(self.notifications_section())
            .push(self.history_section())
            .into()
    }

    fn notifications_section(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        t3::section(
            t3::section_title("Notifications"),
            None,
            [
                t3::setting(
                    "Connection changes",
                    t3::muted(
                        "When a device connects, disconnects or switches between wired and wireless",
                    ),
                    t3::switch(
                        "notifications.connections",
                        None,
                        draft.values.notifications.connections,
                        Some(|value| Message::Preference(Preference::Connections(value))),
                    ),
                ),
                t3::setting_titled(
                    "Low battery",
                    t3::switch(
                        "notifications.battery",
                        None,
                        draft.values.notifications.battery,
                        Some(|value| Message::Preference(Preference::Battery(value))),
                    ),
                ),
            ],
        )
    }

    fn history_section(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        let number = |placeholder: &'static str,
                      value: &str,
                      unit: &'static str,
                      change: fn(String) -> Preference| {
            row![
                text_input(placeholder, value)
                    .width(76)
                    .padding([6, 10])
                    .size(13)
                    .line_height(t3::line_height(13))
                    .style(style::input)
                    .on_input(move |value| Message::Preference(change(value))),
                t3::muted(unit).width(52),
            ]
            .spacing(8)
            .align_y(iced::Center)
        };
        t3::section(
            t3::section_title("Battery history"),
            None,
            [
                t3::setting_titled(
                    "Record battery history",
                    t3::switch(
                        "history.enabled",
                        None,
                        draft.values.history.enabled,
                        Some(|value| Message::Preference(Preference::History(value))),
                    ),
                ),
                t3::setting_titled(
                    "Keep history for",
                    number("Days", &draft.days, "days", Preference::Days),
                ),
                t3::setting_titled(
                    "Sample every",
                    number("Seconds", &draft.seconds, "seconds", Preference::Seconds),
                ),
                t3::setting_titled(
                    format!(
                        "{} observations · {} KiB",
                        self.history.usage.samples,
                        self.history.usage.bytes / 1024
                    ),
                    t3::ghost_danger(
                        icon::TRASH_2,
                        "Clear history…",
                        self.state
                            .connected
                            .then_some(Message::Ask(Confirmation::ClearHistory)),
                    ),
                ),
            ],
        )
    }

    pub(super) fn preferences_bar(&self) -> Element<'_, Message> {
        let valid = self.state.draft.validated();
        let changed = valid
            .as_ref()
            .is_ok_and(|settings| settings != &self.state.settings);
        let saving = self.in_flight(Control::Save);
        let mut status = row![].spacing(8).align_y(iced::Center).width(Fill);
        if saving {
            status = status.push(t3::dots());
            status = status.push(t3::text("Saving preferences…", 13).color(color::MUTED));
        } else if let Err(error) = &valid {
            status = status.push(t3::text(error.clone(), 13).color(color::DANGER));
        } else if changed {
            status = status.push(t3::icon(icon::INFO).size(14).color(color::WARNING));
            status = status.push(t3::text("Unsaved changes", 13).color(color::WARNING));
        } else if self.just_confirmed(Control::Save) {
            status = status.push(t3::check());
            status = status.push(t3::text("Preferences saved", 13).color(color::SUCCESS));
        }
        t3::composer(
            row![
                status,
                t3::ghost(
                    icon::UNDO_2,
                    "Restore defaults…",
                    Some(Message::Ask(Confirmation::Defaults)),
                ),
                if saving {
                    t3::primary_busy("Save changes")
                } else {
                    t3::primary("Save changes", changed.then_some(Message::Save))
                },
            ]
            .spacing(8)
            .align_y(iced::Center),
            PAGE_WIDTH,
            changed,
        )
    }

    pub(super) fn dialog<'a>(kind: Confirmation) -> Element<'a, Message> {
        let cancel = t3::outline(icon::X, "Cancel", Some(Message::Cancel));
        match kind {
            Confirmation::Defaults => t3::dialog(
                "Restore default settings?",
                "This replaces your unsaved edits. Save changes to apply the defaults. Battery history is kept.",
                [
                    cancel,
                    t3::primary("Restore defaults", Some(Message::Confirm)),
                ],
            ),
            Confirmation::ClearHistory => t3::dialog(
                "Delete battery history?",
                "All recorded battery observations and learned estimates will be deleted. This cannot be undone.",
                [cancel, t3::danger("Delete history", Some(Message::Confirm))],
            ),
        }
    }

    /// The id of the dialog's confirming button, which Tab moves to.
    pub fn confirm_id(kind: Confirmation) -> &'static str {
        match kind {
            Confirmation::Defaults => "Restore defaults",
            Confirmation::ClearHistory => "Delete history",
        }
    }
}
