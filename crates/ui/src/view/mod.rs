mod lighting;
mod page;
mod preferences;
mod sensitivity;
mod sidebar;

use crate::{
    app::{App, Message},
    keyboard_button,
    style::{self, Icon, Segment},
};
use iced::{
    Alignment, Element, Fill, Length,
    widget::{Row, Space, button, column, container, row, scrollable, text, toggler, tooltip},
};

/// Visual weight of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    GhostDanger,
}

impl Kind {
    fn style(self) -> fn(&iced::Theme, button::Status) -> button::Style {
        match self {
            Self::Primary => style::primary,
            Self::Secondary => style::secondary,
            Self::Ghost => style::ghost,
            Self::Danger => style::danger,
            Self::GhostDanger => style::ghost_danger,
        }
    }
}

pub fn icon<'a>(icon: Icon) -> text::Text<'a> {
    text(icon.glyph().to_string())
        .font(style::ICONS)
        .shaping(text::Shaping::Advanced)
        .size(16)
        .line_height(1.0)
}

pub fn heading<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content).font(style::SEMIBOLD).size(15)
}

pub fn caption<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content).size(12).style(style::muted)
}

/// A focusable button with an optional leading icon.
pub fn action(
    id: impl Into<iced::advanced::widget::Id>,
    leading: Option<Icon>,
    label: impl Into<String>,
    message: Option<Message>,
    kind: Kind,
) -> Element<'static, Message> {
    let mut content = Row::new().spacing(8).align_y(Alignment::Center);
    if let Some(leading) = leading {
        content = content.push(icon(leading).size(15));
    }
    content = content.push(text(label.into()).size(14));
    let control = button(content)
        .padding([7, 14])
        .style(kind.style())
        .on_press_maybe(message.clone());
    keyboard_button::wrap(id, control, message)
}

/// A square icon-only button with a tooltip naming its action.
pub fn icon_button(
    id: impl Into<iced::advanced::widget::Id>,
    glyph: Icon,
    label: &'static str,
    message: Option<Message>,
    kind: Kind,
) -> Element<'static, Message> {
    let control = button(
        container(icon(glyph))
            .center_x(Length::Fixed(18.0))
            .center_y(Length::Fixed(18.0)),
    )
    .padding(7)
    .style(kind.style())
    .on_press_maybe(message.clone());
    tip(keyboard_button::wrap(id, control, message), label)
}

pub fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    tooltip(content, text(label).size(12), tooltip::Position::Bottom)
        .style(style::tooltip)
        .padding(8)
        .gap(6)
        .into()
}

/// A row of joined buttons where exactly one option is selected.
pub fn segmented(
    id: &str,
    options: Vec<(String, Message, bool)>,
    enabled: bool,
) -> Element<'static, Message> {
    let count = options.len();
    let mut segments = Row::new();
    for (index, (label, message, selected)) in options.into_iter().enumerate() {
        let message = (enabled && !selected).then_some(message);
        let control = button(text(label).size(13))
            .padding([6, 14])
            .style(style::segment(selected, Segment::at(index, count)))
            .on_press_maybe(message.clone());
        segments = segments.push(keyboard_button::wrap(
            format!("{id}-{index}"),
            control,
            message,
        ));
    }
    segments.into()
}

/// A labelled switch; Space toggles it when focused.
pub fn switch(
    id: &'static str,
    value: bool,
    change: impl Fn(bool) -> Message + 'static,
) -> Element<'static, Message> {
    let message = change(!value);
    let control = toggler(value)
        .on_toggle(change)
        .size(20)
        .style(style::switch);
    keyboard_button::wrap(id, control, Some(message))
}

/// A settings row: title and description on the left, the control on the right.
pub fn setting<'a>(
    title: impl text::IntoFragment<'a>,
    description: Option<&'a str>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut label = column![text(title).size(14)].spacing(3);
    if let Some(description) = description {
        label = label.push(caption(description));
    }
    row![label.width(Fill), control.into()]
        .spacing(24)
        .align_y(Alignment::Center)
        .into()
}

pub fn divider<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Fill)
        .height(1)
        .style(style::divider)
        .into()
}

pub fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .padding(20)
        .style(style::card)
        .into()
}

pub fn chip<'a>(content: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(content).size(12))
        .padding([3, 10])
        .style(style::chip)
        .into()
}

/// A centered message for states without a device page.
pub fn empty_state<'a>(glyph: Icon, title: &'a str, description: &'a str) -> Element<'a, Message> {
    container(
        column![
            icon(glyph).size(40).style(style::muted),
            text(title).font(style::SEMIBOLD).size(20),
            caption(description).size(14),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .max_width(420),
    )
    .center_x(Fill)
    .padding(iced::padding::top(96))
    .into()
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let mut main = column![].spacing(16);
        if let Some(error) = &self.state.error {
            main = main.push(
                container(
                    row![
                        icon(Icon::Alert).size(18).style(style::danger_text),
                        text(error).size(13).width(Fill),
                        icon_button(
                            "dismiss-error",
                            Icon::Close,
                            "Dismiss",
                            Some(Message::Dismiss),
                            Kind::Ghost
                        )
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 14])
                .style(style::error_banner),
            );
        }
        main = main.push(if self.state.preferences {
            self.preferences()
        } else {
            self.device_page()
        });
        let content = scrollable(
            container(main.max_width(980))
                .padding([24, 32])
                .center_x(Fill),
        )
        .height(Fill)
        .spacing(4);
        let base: Element<_> = container(row![self.sidebar(), content].height(Fill))
            .width(Fill)
            .height(Fill)
            .style(style::page)
            .into();
        if let Some(kind) = self.state.confirmation {
            Self::modal(base, kind)
        } else {
            base
        }
    }
}
