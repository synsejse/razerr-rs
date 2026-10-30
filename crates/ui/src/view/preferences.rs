use super::{Kind, action, caption, card, divider, heading, setting, switch};
use crate::{
    app::{App, Message, Preference},
    state::Confirmation,
    style::{self, Icon},
};
use iced::{
    Alignment, Element, Fill,
    widget::{Space, column, container, mouse_area, opaque, row, stack, text, text_input},
};

impl App {
    pub(super) fn preferences(&self) -> Element<'_, Message> {
        column![
            row![
                super::icon_button(
                    "back",
                    Icon::Back,
                    "Back to devices",
                    Some(Message::Back),
                    Kind::Ghost
                ),
                text("Preferences").font(style::SEMIBOLD).size(24)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            self.appearance_card(),
            self.notifications_card(),
            self.history_card(),
            self.preferences_footer(),
        ]
        .spacing(16)
        .into()
    }

    fn appearance_card(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        card(
            column![
                heading("Appearance"),
                setting(
                    "Dark theme",
                    None,
                    switch(
                        "appearance.dark",
                        self.state.dark_override || draft.values.appearance.dark,
                        |value| Message::Preference(Preference::Dark(value)),
                    ),
                ),
            ]
            .spacing(16),
        )
    }

    fn notifications_card(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        card(
            column![
                heading("Notifications"),
                setting(
                    "Connection changes",
                    Some(
                        "When a device connects, disconnects or switches between wired and wireless"
                    ),
                    switch(
                        "notifications.connections",
                        draft.values.notifications.connections,
                        |value| Message::Preference(Preference::Connections(value)),
                    ),
                ),
                divider(),
                setting(
                    "Low battery",
                    None,
                    switch(
                        "notifications.battery",
                        draft.values.notifications.battery,
                        |value| Message::Preference(Preference::Battery(value)),
                    ),
                ),
            ]
            .spacing(16),
        )
    }

    fn history_card(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        let number = |placeholder: &'static str,
                      value: &str,
                      unit: &'static str,
                      change: fn(String) -> Preference| {
            row![
                text_input(placeholder, value)
                    .width(76)
                    .padding([6, 10])
                    .size(14)
                    .style(style::input)
                    .on_input(move |value| Message::Preference(change(value))),
                caption(unit).size(13).width(56)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        };

        card(
            column![
                heading("Battery history"),
                setting(
                    "Record battery history",
                    None,
                    switch("history.enabled", draft.values.history.enabled, |value| {
                        Message::Preference(Preference::History(value))
                    }),
                ),
                divider(),
                setting(
                    "Keep history for",
                    None,
                    number("Days", &draft.days, "days", Preference::Days),
                ),
                divider(),
                setting(
                    "Sample every",
                    None,
                    number("Seconds", &draft.seconds, "seconds", Preference::Seconds),
                ),
                divider(),
                setting(
                    format!(
                        "{} observations · {} KiB",
                        self.history.usage.samples,
                        self.history.usage.bytes / 1024
                    ),
                    None,
                    action(
                        "history-clear",
                        Some(Icon::Trash),
                        "Clear history…",
                        self.state
                            .connected
                            .then_some(Message::Ask(Confirmation::ClearHistory)),
                        Kind::GhostDanger,
                    ),
                ),
            ]
            .spacing(16),
        )
    }

    fn preferences_footer(&self) -> Element<'_, Message> {
        let draft = &self.state.draft;
        let valid = draft.validated();
        let changed = valid
            .as_ref()
            .is_ok_and(|settings| settings != &self.state.settings);
        let mut footer = row![action(
            "settings-defaults",
            Some(Icon::Revert),
            "Restore defaults…",
            Some(Message::Ask(Confirmation::Defaults)),
            Kind::Ghost
        )]
        .spacing(12)
        .align_y(Alignment::Center);
        footer = footer.push(Space::new().width(Fill));
        if let Err(error) = &valid {
            footer = footer.push(text(error.clone()).size(13).style(style::danger_text));
        } else if self.state.saved && !changed {
            footer = footer.push(
                row![
                    super::icon(Icon::Check).size(14).style(style::accent_text),
                    text("Saved").size(13).style(style::accent_text)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        footer = footer.push(action(
            "settings-save",
            None,
            if self.state.saving {
                "Saving…"
            } else {
                "Save changes"
            },
            (changed && !self.state.saving).then_some(Message::Save),
            Kind::Primary,
        ));
        footer.into()
    }

    pub(super) fn modal(base: Element<'_, Message>, kind: Confirmation) -> Element<'_, Message> {
        let (title, description, confirm, confirm_kind) = match kind {
            Confirmation::Defaults => (
                "Restore default settings?",
                "This replaces your unsaved edits. Save changes to apply the defaults. Battery history is kept.",
                "Restore defaults",
                Kind::Primary,
            ),
            Confirmation::ClearHistory => (
                "Delete battery history?",
                "All recorded battery observations and learned estimates will be deleted. This cannot be undone.",
                "Delete history",
                Kind::Danger,
            ),
        };
        let dialog = container(
            column![
                text(title).font(style::SEMIBOLD).size(18),
                caption(description).size(14),
                row![
                    Space::new().width(Fill),
                    action(
                        "modal-cancel",
                        None,
                        "Cancel",
                        Some(Message::Cancel),
                        Kind::Secondary
                    ),
                    action(
                        "modal-confirm",
                        None,
                        confirm,
                        Some(Message::Confirm),
                        confirm_kind
                    )
                ]
                .spacing(10)
            ]
            .spacing(14),
        )
        .padding(24)
        .max_width(460)
        .style(style::dialog);
        let background = container(Space::new().width(Fill).height(Fill)).style(style::scrim);
        stack![
            base,
            mouse_area(opaque(background)).on_press(Message::Cancel),
            container(opaque(dialog)).center_x(Fill).center_y(Fill)
        ]
        .into()
    }
}
