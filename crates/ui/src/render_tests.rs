//! The real views, rendered and clicked without a window. Set
//! `RAZERR_UI_CAPTURE_DIR` to save a PNG of every page and state.
#![expect(
    clippy::cast_precision_loss,
    reason = "Window sizes and click positions are small whole numbers"
)]
#![expect(
    clippy::too_many_lines,
    reason = "The list of scenes reads best as one list"
)]

use std::{sync::Arc, time::Instant};

use iced::{
    Point, Rectangle, Size,
    advanced::{
        Layout, Shell, clipboard, layout,
        renderer::{Headless, Style},
        widget::Tree,
    },
    mouse,
};
use iced_t3::font;
use razerr_model::{Availability, Dpi, DpiControl, DpiRange, Reading, Snapshot};

use crate::{
    activity::{Activity, Confirmed, Control},
    app::{App, Edit, Message, Preference},
    state::{Confirmation, Page, Section},
};

fn load_fonts() {
    for bytes in font::ALL {
        iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system")
            .load_font(bytes.into());
    }
    iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system")
        .load_font(iced_aw::ICED_AW_FONT_BYTES.into());
}

fn renderer() -> iced::Renderer {
    pollster::block_on(<iced::Renderer as Headless>::new(
        font::REGULAR,
        14.0.into(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn activity(control: Control, progress: &str) -> Activity {
    Activity {
        id: 1,
        control,
        progress: progress.into(),
        done: String::new(),
        serial: None,
    }
}

fn confirmed(control: Control, text: &str) -> Confirmed {
    Confirmed {
        control,
        text: text.into(),
        at: Instant::now(),
    }
}

fn capture(name: &str) -> Snapshot {
    let json = match name {
        "wired" => include_str!("../tests/captures/wired.json"),
        _ => include_str!("../tests/captures/wireless_handoff.json"),
    };
    serde_json::from_str(json).expect("captured snapshot")
}

/// A demo app showing `page`.
fn demo(page: Page) -> App {
    let mut app = App::demo();
    app.state.page = page;
    app
}

fn with_snapshot(mut app: App, snapshot: Snapshot) -> App {
    app.state.received = false;
    app.state.selected = None;
    app.state.receive(Arc::new(snapshot));
    app
}

/// A single-axis mouse, which gets segments instead of the pad.
fn discrete() -> App {
    let mut snapshot = capture("wired");
    let device = &mut snapshot.devices[0];
    device.capabilities.available_dpi = true;
    device.capabilities.dpi_stages = false;
    device.stages = None;
    device.dpi = Some(DpiControl {
        current: Reading::Fresh(Dpi::new(900, 0).expect("DPI")),
        range: DpiRange::Discrete {
            values: Reading::Fresh(vec![450, 900, 1800, 3500]),
        },
    });
    with_snapshot(App::demo(), snapshot)
}

/// Every page in the states it can be in, by name.
fn scenarios() -> Vec<(&'static str, App)> {
    let mut scenes = Vec::new();
    for page in Page::ALL {
        scenes.push((page.title(), demo(page)));
    }
    let mut preferences = App::demo();
    preferences.state.preferences = true;
    scenes.push(("preferences", preferences));

    let mut app = demo(Page::Sensitivity);
    app.pending = Some(activity(Control::Stage(2), "Switching to stage 2…"));
    scenes.push(("stage-loading", app));

    let mut app = demo(Page::Sensitivity);
    app.confirmed = Some(confirmed(Control::Stage(3), "Stage 3 active"));
    scenes.push(("stage-confirmed", app));

    let mut app = demo(Page::Sensitivity);
    app.pending = Some(activity(Control::Poll(500), "Setting polling to 500 Hz…"));
    scenes.push(("polling-loading", app));

    let mut app = demo(Page::Sensitivity);
    let _ = app.update(Message::Edit(Edit::Vector(2400, 2400)));
    scenes.push(("sensitivity-draft", app));

    let mut app = demo(Page::Sensitivity);
    let _ = app.update(Message::Edit(Edit::Vector(2400, 2400)));
    app.pending = Some(activity(
        Control::Apply(Section::Sensitivity),
        "Applying sensitivity…",
    ));
    scenes.push(("sensitivity-applying", app));

    let mut app = demo(Page::Sensitivity);
    app.confirmed = Some(confirmed(
        Control::Apply(Section::Sensitivity),
        "Sensitivity applied",
    ));
    scenes.push(("sensitivity-applied", app));

    let mut app = demo(Page::Sensitivity);
    let _ = app.update(Message::Edit(Edit::Target(crate::editor::Target::Stage(1))));
    scenes.push(("sensitivity-stage", app));

    let mut app = demo(Page::Sensitivity);
    app.state.error = Some("razer.device.dpi.setDPI: Input/output error".into());
    scenes.push(("error", app));

    let mut app = demo(Page::Lighting);
    let zone = razerr_model::ZoneId::Logo;
    let _ = app.update(Message::Edit(Edit::Color(zone, false, "#12".into())));
    scenes.push(("lighting-invalid", app));

    let mut app = demo(Page::Lighting);
    let _ = app.update(Message::Edit(Edit::Kind(
        razerr_model::ZoneId::Logo,
        razerr_model::EffectKind::Wave,
    )));
    let _ = app.update(Message::Edit(Edit::Kind(
        razerr_model::ZoneId::Scroll,
        razerr_model::EffectKind::Reactive,
    )));
    scenes.push(("lighting-effects", app));

    let mut snapshot = capture("wired");
    snapshot.devices[0]
        .polling
        .as_mut()
        .expect("polling")
        .current = Reading::Fresh(razerr_model::PollRate::new(8000).expect("rate"));
    scenes.push((
        "high-polling",
        with_snapshot(demo(Page::Sensitivity), snapshot),
    ));

    scenes.push((
        "capture-wired",
        with_snapshot(demo(Page::Power), capture("wired")),
    ));
    scenes.push((
        "capture-wireless",
        with_snapshot(demo(Page::Sensitivity), capture("wireless")),
    ));
    scenes.push(("discrete", discrete()));

    let mut snapshot = capture("wired");
    snapshot.devices[0].idle_time = Some(Reading::Unavailable);
    snapshot.devices[0].low_battery = Some(Reading::Unavailable);
    scenes.push(("power-unread", with_snapshot(demo(Page::Power), snapshot)));

    let mut snapshot = capture("wired");
    let mut app = demo(Page::Sensitivity);
    app.state.preferences = false;
    snapshot.devices[0].dpi = app.state.snapshot.devices[0].dpi.clone();
    let mut app = with_snapshot(app, snapshot);
    app.pending = Some(activity(Control::Stage(5), "Switching to stage 5…"));
    scenes.push(("long-title", app));

    let mut app = demo(Page::Lighting);
    let mut snapshot = (*app.state.snapshot).clone();
    snapshot.devices[0].zones[0].brightness = Some(Reading::Unavailable);
    app = with_snapshot(app, snapshot);
    scenes.push(("lighting-unread", app));

    let mut snapshot = capture("wired");
    snapshot.devices[0].availability = Availability::Unavailable;
    scenes.push((
        "device-unavailable",
        with_snapshot(demo(Page::Sensitivity), snapshot),
    ));

    let mut app = demo(Page::Sensitivity);
    app.state.received = false;
    scenes.push(("looking", app));

    let mut app = demo(Page::Sensitivity);
    app.state.connected = false;
    app.state.error = Some("Cannot reach the razerr-rs service: no owner".into());
    scenes.push(("service-disconnected", app));

    let mut snapshot = capture("wired");
    snapshot.daemon_available = false;
    scenes.push((
        "daemon-down",
        with_snapshot(demo(Page::Sensitivity), snapshot),
    ));

    let mut snapshot = capture("wired");
    snapshot.devices.clear();
    scenes.push((
        "no-devices",
        with_snapshot(demo(Page::Sensitivity), snapshot),
    ));

    let mut app = demo(Page::Power);
    app.state.preferences = true;
    let _ = app.update(Message::Preference(Preference::Days("3".into())));
    scenes.push(("preferences-invalid", app));

    let mut app = App::demo();
    app.state.preferences = true;
    let _ = app.update(Message::Preference(Preference::Days("30".into())));
    app.pending = Some(activity(Control::Save, "Saving preferences…"));
    scenes.push(("preferences-saving", app));

    let mut app = App::demo();
    app.state.preferences = true;
    let _ = app.update(Message::Preference(Preference::Days("30".into())));
    scenes.push(("preferences-draft", app));

    let mut app = App::demo();
    app.state.preferences = true;
    app.confirmed = Some(confirmed(Control::Save, "Preferences saved"));
    scenes.push(("preferences-saved", app));

    let mut app = App::demo();
    app.state.preferences = true;
    app.state.open_confirmation(Confirmation::ClearHistory);
    scenes.push(("dialog", app));

    // `razerr-rs-service status` output, to see a real device's readings.
    if let Ok(path) = std::env::var("RAZERR_UI_SNAPSHOT") {
        let json = std::fs::read_to_string(path).expect("snapshot file");
        let snapshot: Snapshot = serde_json::from_str(&json).expect("snapshot");
        for (name, page) in [
            ("real-sensitivity", Page::Sensitivity),
            ("real-power", Page::Power),
            ("real-device", Page::Device),
        ] {
            scenes.push((name, with_snapshot(demo(page), snapshot.clone())));
        }
    }
    scenes
}

const SIZES: [(u32, u32); 3] = [(1120, 780), (820, 560), (1600, 1100)];

fn draw(app: &App, width: u32, height: u32) -> Vec<u8> {
    let mut renderer = renderer();
    let size = Size::new(width as f32, height as f32);
    let mut view = app.view();
    let mut tree = Tree::new(&view);
    let node = view
        .as_widget_mut()
        .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
    let theme = iced_t3::theme();
    let mut messages = Vec::new();
    view.as_widget_mut().update(
        &mut tree,
        &iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now())),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut clipboard::Null,
        &mut Shell::new(&mut messages),
        &Rectangle::with_size(size),
    );
    view.as_widget().draw(
        &tree,
        &mut renderer,
        &theme,
        &Style {
            text_color: theme.palette().text,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &Rectangle::with_size(size),
    );
    renderer.screenshot(Size::new(width, height), 1.0, theme.palette().background)
}

#[test]
fn every_page_and_state_renders() {
    load_fonts();
    let directory = std::env::var("RAZERR_UI_CAPTURE_DIR").ok();
    for (name, app) in scenarios() {
        for (width, height) in SIZES {
            let pixels = draw(&app, width, height);
            assert_eq!(pixels.len(), (width * height * 4) as usize, "{name}");
            if let Some(directory) = &directory {
                std::fs::create_dir_all(directory).expect("capture directory");
                image::save_buffer(
                    format!("{directory}/{name}-{width}x{height}.png"),
                    &pixels,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )
                .expect("capture");
            }
        }
    }
}

/// Heights of the step buttons `NumberInput` lays out even with `ignore_buttons`,
/// which returns from `draw` before reaching them.
const UNDRAWN: [f32; 2] = [9.75, 20.5];

fn off_grid(layout: Layout<'_>, found: &mut Vec<Rectangle>) {
    let bounds = layout.bounds();
    if (bounds.y.fract() != 0.0 || bounds.height.fract() != 0.0)
        && !UNDRAWN.contains(&bounds.height)
    {
        found.push(bounds);
    }
    for child in layout.children() {
        off_grid(child, found);
    }
}

/// Iced truncates fractional positions, so one off-grid box puts everything
/// inside it out of line.
#[test]
fn every_box_sits_on_the_pixel_grid_vertically() {
    load_fonts();
    let renderer = renderer();
    for (name, app) in scenarios() {
        for (width, height) in SIZES {
            let size = Size::new(width as f32, height as f32);
            let mut view = app.view();
            let mut tree = Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut found = Vec::new();
            off_grid(Layout::new(&node), &mut found);
            assert!(found.is_empty(), "{name} at {width}x{height}: {found:#?}");
        }
    }
}

/// Clicks at each position and returns what the controls sent.
fn click(app: &App, positions: impl Iterator<Item = Point>) -> Vec<Message> {
    click_at_size(app, Size::new(1120.0, 780.0), positions)
}

fn click_at_size(app: &App, size: Size, positions: impl Iterator<Item = Point>) -> Vec<Message> {
    let renderer = renderer();
    let mut view = app.view();
    let mut tree = Tree::new(&view);
    let node = view
        .as_widget_mut()
        .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
    let mut messages = Vec::new();
    for position in positions {
        for event in [
            mouse::Event::CursorMoved { position },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
    }
    messages
}

/// Clicks every few pixels of the page area.
fn click_page(app: &App) -> Vec<Message> {
    let rows = (48..640).step_by(8);
    click(
        app,
        rows.flat_map(|y| {
            (280..1100)
                .step_by(8)
                .map(move |x| Point::new(x as f32, y as f32))
        }),
    )
}

#[test]
fn the_lock_asks_for_the_opposite_of_its_state() {
    load_fonts();
    for linked in [false, true] {
        let mut app = demo(Page::Sensitivity);
        app.state.editor.linked = linked;
        let sent: Vec<_> = click_page(&app)
            .into_iter()
            .filter_map(|message| match message {
                Message::Edit(Edit::Linked(value)) => Some(value),
                _ => None,
            })
            .collect();
        assert!(!sent.is_empty(), "no click reached the lock");
        assert!(sent.iter().all(|value| *value != linked));
    }
}

#[test]
fn switches_ask_for_the_opposite_of_their_state() {
    load_fonts();
    for value in [false, true] {
        let mut app = App::demo();
        app.state.preferences = true;
        app.state.draft.values.notifications.connections = value;
        app.state.draft.values.notifications.battery = value;
        app.state.draft.values.history.enabled = value;
        let (mut connections, mut battery, mut history) = (Vec::new(), Vec::new(), Vec::new());
        for message in click_page(&app) {
            match message {
                Message::Preference(Preference::Connections(sent)) => connections.push(sent),
                Message::Preference(Preference::Battery(sent)) => battery.push(sent),
                Message::Preference(Preference::History(sent)) => history.push(sent),
                _ => {}
            }
        }
        for sent in [connections, battery, history] {
            assert!(!sent.is_empty(), "no click reached a switch");
            assert!(sent.iter().all(|sent| *sent != value));
        }
    }
}

fn stages_sent(messages: &[Message]) -> Vec<u8> {
    let mut stages: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Message::ActivateStage(stage) => Some(*stage),
            _ => None,
        })
        .collect();
    stages.sort_unstable();
    stages.dedup();
    stages
}

fn rates_sent(messages: &[Message]) -> Vec<u16> {
    let mut rates: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Message::PollRate(rate) => Some(*rate),
            _ => None,
        })
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

#[test]
fn only_the_other_choices_send_anything_to_the_device() {
    load_fonts();
    let messages = click_page(&demo(Page::Sensitivity));
    assert_eq!(
        stages_sent(&messages),
        [1, 2, 4, 5],
        "stage 3 is already active"
    );
    assert_eq!(
        rates_sent(&messages),
        [125, 500, 2000, 4000],
        "1 kHz is already set"
    );
}

#[test]
fn the_sidebar_holds_no_device_settings() {
    load_fonts();
    let rows = (48..770).step_by(6);
    let messages = click(
        &demo(Page::Sensitivity),
        rows.map(|y| Point::new(100.0, y as f32)),
    );
    assert_eq!(stages_sent(&messages), Vec::<u8>::new());
    assert_eq!(rates_sent(&messages), Vec::<u16>::new());
}

#[test]
fn unapplied_stage_edits_lock_the_stage_choices() {
    load_fonts();
    let mut app = demo(Page::Sensitivity);
    let _ = app.update(Message::Edit(Edit::ActiveStage(1)));
    assert!(app.state.editor.stages_dirty);
    assert_eq!(stages_sent(&click_page(&app)), Vec::<u8>::new());
}

#[test]
fn the_docked_bar_appears_only_with_something_to_say_on_every_page() {
    load_fonts();
    for page in [Page::Sensitivity, Page::Lighting, Page::Power] {
        assert!(demo(page).docked_status().is_none(), "{page:?} at rest");
    }
    let mut preferences = App::demo();
    preferences.state.preferences = true;
    assert!(preferences.docked_status().is_none(), "Preferences at rest");

    let mut app = demo(Page::Power);
    let _ = app.update(Message::Edit(Edit::Idle("600".into())));
    assert_eq!(app.docked_status().expect("a draft").0, "Unapplied changes");
    let _ = app.update(Message::Preference(Preference::Days("30".into())));
    let _ = app.update(Message::Preferences);
    assert_eq!(app.docked_status().expect("a draft").0, "Unsaved changes");
    let _ = app.update(Message::Preference(Preference::Days("3".into())));
    assert!(
        app.docked_status()
            .expect("invalid")
            .0
            .contains("history.days")
    );
    let _ = app.update(Message::RevertPreferences);
    assert!(app.docked_status().is_none(), "reverted");

    app.confirmed = Some(confirmed(Control::Save, "Preferences saved"));
    assert_eq!(app.docked_status().expect("saved").0, "Preferences saved");
}

#[test]
fn a_long_title_never_pushes_the_window_controls_away() {
    load_fonts();
    let (_, app) = scenarios()
        .into_iter()
        .find(|(name, _)| *name == "long-title")
        .expect("scene");
    let close = Point::new(820.0 - 21.0, 22.0);
    let messages = click_at_size(&app, Size::new(820.0, 560.0), std::iter::once(close));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Window(iced_t3::chrome::Action::Close))),
        "{messages:?}"
    );
}
