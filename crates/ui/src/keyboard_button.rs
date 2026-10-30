//! Keyboard focus for iced's standard button, which is pointer-only in 0.14.
//! Rendering, layout and pointer behavior remain the built-in button's work.

use iced::advanced::Renderer as _;
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, renderer,
        widget::{self, Operation, Tree, operation::Focusable, tree},
    },
    keyboard, mouse,
};

use crate::app::Message;

pub fn wrap<'a>(
    id: impl Into<widget::Id>,
    content: impl Into<Element<'a, Message>>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    Element::new(KeyboardButton {
        id: id.into(),
        content: content.into(),
        on_press,
        cancel_drag: false,
    })
}

struct KeyboardButton<'a> {
    id: widget::Id,
    content: Element<'a, Message>,
    on_press: Option<Message>,
    cancel_drag: bool,
}

/// Reset the standard slider's drag state on Escape before restoring its value.
pub fn cancellable<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(KeyboardButton {
        id: widget::Id::unique(),
        content: content.into(),
        on_press: None,
        cancel_drag: true,
    })
}

#[derive(Default)]
struct Focus(bool);
impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.0
    }
    fn focus(&mut self) {
        self.0 = true;
    }
    fn unfocus(&mut self) {
        self.0 = false;
    }
}

impl Widget<Message, Theme, Renderer> for KeyboardButton<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Focus>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(Focus::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<Focus>();
        if self.on_press.is_some() {
            operation.focusable(Some(&self.id), layout.bounds(), state);
        } else {
            state.unfocus();
        }
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.cancel_drag
            && matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                })
            )
        {
            tree.children[0] = Tree::new(&self.content);
            return;
        }
        let state = tree.state.downcast_mut::<Focus>();
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            state.0 = self.on_press.is_some() && cursor.is_over(layout.bounds());
        }
        if state.0
            && let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event
            && matches!(
                key,
                keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space)
            )
            && let Some(message) = &self.on_press
        {
            shell.publish(message.clone());
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if tree.state.downcast_ref::<Focus>().0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: iced::Border {
                        color: theme.palette().primary,
                        width: 2.0,
                        radius: crate::style::RADIUS_CONTROL.into(),
                    },
                    ..renderer::Quad::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Edit, Preference};
    use iced::{
        Point,
        advanced::{clipboard, renderer::Headless, widget::operation::focusable},
        widget::{button, checkbox, slider},
    };

    struct Harness {
        renderer: Renderer,
        content: Element<'static, Message>,
        tree: Tree,
        node: layout::Node,
    }

    impl Harness {
        async fn new(mut content: Element<'static, Message>) -> Self {
            let renderer =
                <Renderer as Headless>::new(iced::Font::DEFAULT, 16.0.into(), Some("tiny-skia"))
                    .await
                    .expect("headless software renderer");
            let mut tree = Tree::new(&content);
            let node = content.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(320.0, 80.0)),
            );
            Self {
                renderer,
                content,
                tree,
                node,
            }
        }
        fn focus(&mut self, id: &str) {
            let mut operation = focusable::focus::<()>(widget::Id::from(id.to_owned()));
            self.content.as_widget_mut().operate(
                &mut self.tree,
                Layout::new(&self.node),
                &self.renderer,
                &mut operation,
            );
        }
        fn event(
            &mut self,
            event: &Event,
            cursor: mouse::Cursor,
        ) -> (Vec<Message>, iced::event::Status) {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            self.content.as_widget_mut().update(
                &mut self.tree,
                event,
                Layout::new(&self.node),
                cursor,
                &self.renderer,
                &mut clipboard::Null,
                &mut shell,
                &self.node.bounds(),
            );
            let status = shell.event_status();
            (messages, status)
        }
    }

    fn key(named: keyboard::key::Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modified_key: keyboard::Key::Named(named),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    #[tokio::test]
    async fn focused_button_publishes_once_for_enter_and_space() {
        let mut control = Harness::new(wrap(
            "apply",
            button("Apply").on_press(Message::Refresh),
            Some(Message::Refresh),
        ))
        .await;
        control.focus("apply");
        for named in [keyboard::key::Named::Enter, keyboard::key::Named::Space] {
            let (messages, status) = control.event(&key(named), mouse::Cursor::Unavailable);
            assert!(matches!(messages.as_slice(), [Message::Refresh]));
            assert_eq!(status, iced::event::Status::Captured);
        }
    }

    #[tokio::test]
    async fn disabled_button_cannot_focus_or_publish() {
        let mut control = Harness::new(wrap("disabled", button("Disabled"), None)).await;
        control.focus("disabled");
        assert!(!control.tree.state.downcast_ref::<Focus>().is_focused());
        for named in [keyboard::key::Named::Enter, keyboard::key::Named::Space] {
            let (messages, status) = control.event(&key(named), mouse::Cursor::Unavailable);
            assert!(messages.is_empty());
            assert_eq!(status, iced::event::Status::Ignored);
        }
    }

    #[tokio::test]
    async fn focused_checkbox_toggles_once_with_space() {
        let checkbox = checkbox(false)
            .label("Dark theme")
            .on_toggle(|value| Message::Preference(Preference::Dark(value)));
        let mut control = Harness::new(wrap(
            "dark",
            checkbox,
            Some(Message::Preference(Preference::Dark(true))),
        ))
        .await;
        control.focus("dark");
        let (messages, status) = control.event(
            &key(keyboard::key::Named::Space),
            mouse::Cursor::Unavailable,
        );
        assert!(matches!(
            messages.as_slice(),
            [Message::Preference(Preference::Dark(true))]
        ));
        assert_eq!(status, iced::event::Status::Captured);
    }

    #[tokio::test]
    async fn escape_cancels_slider_drag_without_a_release_commit() {
        let slider = slider(1_u16..=32000, 800, |value| {
            Message::Edit(Edit::DpiX(value.to_string()))
        })
        .on_release(Message::Refresh);
        let mut control = Harness::new(cancellable(slider)).await;
        let bounds = control.node.bounds();
        let cursor = mouse::Cursor::Available(Point::new(bounds.center_x(), bounds.center_y()));
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let (messages, _) = control.event(&press, cursor);
        assert!(matches!(
            messages.as_slice(),
            [Message::Edit(Edit::DpiX(_))]
        ));
        control.event(&key(keyboard::key::Named::Escape), cursor);
        let (messages, _) = control.event(&release, cursor);
        assert!(messages.is_empty());
        control.event(&press, cursor);
        let (messages, _) = control.event(&release, cursor);
        assert!(matches!(messages.as_slice(), [Message::Refresh]));
    }
    #[tokio::test]
    async fn escape_cancels_audio_slider_drag() {
        let slider = iced_audio::HSlider::new(iced_audio::NormalParam::new(0.5, 0.5))
            .on_gesture(|gesture| Message::Edit(Edit::AxisGesture(crate::state::Axis::X, gesture)));
        let mut control = Harness::new(cancellable(slider)).await;
        let bounds = control.node.bounds();
        let cursor = mouse::Cursor::Available(Point::new(bounds.center_x(), bounds.center_y()));
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let (messages, _) = control.event(&press, cursor);
        assert!(messages.iter().any(|message| matches!(
            message,
            Message::Edit(Edit::AxisGesture(_, iced_audio::Gesture::GestureStart))
        )));
        control.event(&key(keyboard::key::Named::Escape), cursor);
        let (messages, _) = control.event(&release, cursor);
        assert!(messages.is_empty());
    }
}
