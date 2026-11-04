use super::*;
use crate::activity::in_order;
use razerr_model::DpiStages;
use std::time::Instant;

fn app() -> App {
    let root = std::env::temp_dir().join("razerr-rs-ui-tests");
    let paths = razerr_storage::Paths::from_roots(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .expect("absolute paths");
    let (_, receiver) = tokio::sync::mpsc::channel(1);
    App::new(
        Options {
            paths,
            settings: Settings::default(),
            show_settings: false,
            initial_error: None,
        },
        Activations::new(receiver),
    )
    .0
}

fn wired() -> Arc<Snapshot> {
    Arc::new(
        serde_json::from_str(include_str!("../../tests/captures/wired.json"))
            .expect("actual wired snapshot"),
    )
}

/// An app on the service path, without a service, showing a real capture.
fn connected() -> App {
    let mut app = app();
    let _ = app.update(Message::Snapshot(wired()));
    app
}

/// The current snapshot with `change` applied, as a later revision.
fn changed(app: &App, change: impl FnOnce(&mut Snapshot)) -> Arc<Snapshot> {
    let mut snapshot = (*app.state.snapshot).clone();
    snapshot.revision += 1;
    change(&mut snapshot);
    Arc::new(snapshot)
}

fn active_stage(app: &App) -> u8 {
    app.state
        .selected()
        .expect("device")
        .stages
        .as_ref()
        .expect("stages")
        .displayed()
        .expect("reading")
        .active()
}

fn activate(snapshot: &mut Snapshot, stage: u8) {
    let device = &mut snapshot.devices[0];
    let reading = device.stages.as_mut().expect("stages");
    let values = reading.displayed().expect("reading").values().to_vec();
    *reading = razerr_model::Reading::Fresh(DpiStages::new(stage, values).expect("stages"));
}

fn id(app: &App) -> u64 {
    app.pending.as_ref().expect("a change in flight").id
}

#[test]
fn progress_stays_on_its_control_until_the_write_is_confirmed() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::ActiveStage(1)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    assert!(app.in_flight(Control::Apply(Section::Sensitivity)));
    assert!(!app.in_flight(Control::Apply(Section::Power)));
    assert_eq!(
        app.pending.as_ref().expect("in flight").progress,
        "Applying sensitivity…"
    );
    // A background poll is not the write's confirmation.
    let poll = changed(&app, |_| {});
    let _ = app.update(Message::Snapshot(poll));
    assert!(app.in_flight(Control::Apply(Section::Sensitivity)));
    assert!(app.confirmed.is_none());
    let done = changed(&app, |snapshot| activate(snapshot, 1));
    let _ = app.update(Message::Finished(id(&app), Ok(done)));
    assert!(!app.busy());
    assert!(app.just_confirmed(Control::Apply(Section::Sensitivity)));
    assert_eq!(
        app.confirmed.as_ref().expect("confirmed").text,
        "Sensitivity applied"
    );
}

#[test]
fn the_old_selection_stays_until_the_device_confirms() {
    let mut app = connected();
    let before = active_stage(&app);
    let _ = app.update(Message::Edit(Edit::ActiveStage(1)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    assert_ne!(before, 1);
    assert_eq!(active_stage(&app), before, "still the confirmed stage");
    let done = changed(&app, |snapshot| activate(snapshot, 1));
    let _ = app.update(Message::Finished(id(&app), Ok(done)));
    assert_eq!(active_stage(&app), 1);
}

#[test]
fn confirmation_expires_and_a_failure_never_confirms() {
    let mut app = connected();
    app.confirmed = Some(Confirmed {
        control: Control::Apply(Section::Sensitivity),
        text: "Done".into(),
        at: Instant::now()
            .checked_sub(crate::activity::CONFIRMATION)
            .expect("a recent instant"),
    });
    let _ = app.update(Message::Frame);
    assert!(app.confirmed.is_none());
    let _ = app.update(Message::Edit(Edit::PollRate(500)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    assert!(app.busy());
    let _ = app.update(Message::Finished(id(&app), Err("Write failed".into())));
    assert!(!app.busy());
    assert!(app.confirmed.is_none());
    assert_eq!(app.state.error.as_deref(), Some("Write failed"));
}

#[test]
fn controls_ignore_input_during_a_write() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::Idle("600".into())));
    let draft = app.state.editor.idle.clone();
    let _ = app.update(Message::Edit(Edit::PollRate(500)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    let first = id(&app);
    for message in [
        Message::Edit(Edit::Idle("900".into())),
        Message::Revert(Section::Power),
        Message::Edit(Edit::ActiveStage(1)),
        Message::Edit(Edit::PollRate(125)),
        Message::Refresh,
        Message::Save,
        Message::Ask(Confirmation::Defaults),
        Message::RevertPreferences,
    ] {
        let _ = app.update(message);
    }
    assert_eq!(id(&app), first, "only one change at a time");
    assert_eq!(app.state.editor.idle, draft);
    assert!(app.state.editor.idle_dirty);
    assert!(app.state.confirmation.is_none());
    let key = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.update(Message::Key(key, keyboard::Modifiers::empty()));
    assert_eq!(app.state.editor.idle, draft, "Escape waits too");
    // Moving around isn't a change, so it still works.
    let _ = app.update(Message::Page(Page::Device));
    assert_eq!(app.state.page, Page::Device);
}

#[test]
fn losing_the_service_never_leaves_a_control_busy() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::PollRate(500)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    let stale = id(&app);
    let _ = app.update(Message::Disconnected("gone".into()));
    assert!(!app.busy());
    assert!(app.state.error.is_some());
    // A reply that arrives after the fact can't end a newer change.
    let _ = app.update(Message::Snapshot(wired()));
    app.state.connected = true;
    let _ = app.update(Message::Edit(Edit::PollRate(125)));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    let current = id(&app);
    assert_ne!(stale, current);
    let _ = app.update(Message::Finished(stale, Err("late".into())));
    assert_eq!(id(&app), current);
}

#[test]
fn a_confirmed_apply_replaces_drafts_with_what_the_device_reports() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::Idle("600".into())));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Power)));
    assert!(app.in_flight(Control::Apply(Section::Power)));
    assert!(
        app.state.editor.idle_dirty,
        "unconfirmed until the device answers"
    );
    // The device normalized the value, so the draft would never match it.
    let done = changed(&app, |snapshot| {
        snapshot.devices[0].idle_time = Some(razerr_model::Reading::Fresh(
            razerr_model::IdleTime::new(615).expect("seconds"),
        ));
    });
    let _ = app.update(Message::Finished(id(&app), Ok(done)));
    assert!(!app.state.editor.dirty(Section::Power));
    assert_eq!(app.state.editor.idle, "615");
    assert!(app.just_confirmed(Control::Apply(Section::Power)));
}

#[test]
fn a_failed_apply_keeps_its_drafts() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::Idle("600".into())));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Power)));
    let _ = app.update(Message::Finished(id(&app), Err("Write failed".into())));
    assert!(!app.busy());
    assert!(app.state.editor.idle_dirty);
    assert_eq!(app.state.editor.idle, "600");
}

#[test]
fn an_apply_for_another_device_leaves_the_new_drafts_alone() {
    let mut app = connected();
    let _ = app.update(Message::Edit(Edit::Idle("600".into())));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Power)));
    app.pending.as_mut().expect("in flight").serial = Some(Serial::new("other").expect("serial"));
    let done = changed(&app, |_| {});
    let _ = app.update(Message::Finished(id(&app), Ok(done)));
    assert!(app.state.editor.idle_dirty);
}

#[test]
fn a_batch_stops_at_the_first_failed_write() {
    let command = |x| Command::SetDpi {
        serial: Serial::new("target").expect("serial"),
        dpi: razerr_model::Dpi::new(x, x).expect("DPI"),
    };
    let sent = std::cell::RefCell::new(Vec::new());
    let result = pollster::block_on(in_order(
        vec![command(400), command(800), command(1600)],
        |sending| {
            let failed = sending == command(800);
            sent.borrow_mut().push(sending);
            async move {
                if failed {
                    Err("Write failed".to_owned())
                } else {
                    Ok(())
                }
            }
        },
    ));
    assert_eq!(result, Err("Write failed".to_owned()));
    assert_eq!(sent.into_inner().len(), 2);
}

#[test]
fn stage_and_polling_choices_wait_for_apply_and_preserve_other_drafts() {
    let mut app = connected();
    let before = app.state.snapshot.clone();
    let _ = app.update(Message::Edit(Edit::Target(Target::Stage(0))));
    let _ = app.update(Message::Edit(Edit::Vector(9000, 9000)));
    let _ = app.update(Message::Edit(Edit::ActiveStage(1)));
    let _ = app.update(Message::Edit(Edit::PollRate(500)));
    assert!(!app.busy());
    assert!(Arc::ptr_eq(&app.state.snapshot, &before));
    assert_eq!(app.state.editor.stages[0].0, "9000");
    assert_eq!(app.state.editor.active, 1);
    assert_eq!(app.state.editor.poll_rate, Some(500));
    let _ = app.update(Message::Edit(Edit::Apply(Section::Sensitivity)));
    assert!(app.in_flight(Control::Apply(Section::Sensitivity)));
}

#[test]
fn the_demo_keeps_no_paths_and_applies_nothing_until_it_answers() {
    let app = App::demo();
    assert!(app.paths.is_none());
    assert!(app.loader.is_none());
    assert!(app.activations.is_none());
    assert!(matches!(app.backend, Backend::Demo(_)));
}

#[test]
fn opening_preferences_again_keeps_unsaved_edits() {
    let mut app = app();
    let _ = app.update(Message::Preferences);
    let _ = app.update(Message::Preference(Preference::Days("30".into())));
    let _ = app.update(Message::Preferences);
    assert_eq!(app.state.draft.days, "30");
    let _ = app.update(Message::Page(Page::Device));
    let _ = app.update(Message::Preferences);
    assert_eq!(app.state.draft.days, "30");
}

#[test]
fn refresh_shortcut_works_with_caps_lock() {
    let mut app = app();
    app.state.editor.x = "2000".into();
    let key = keyboard::Key::Character("R".into());
    let _ = app.key(&key, keyboard::Modifiers::CTRL);
    assert_eq!(app.state.editor.x, "", "Ctrl+R reset the drafts");
}
