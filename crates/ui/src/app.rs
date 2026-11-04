use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use iced::{
    Subscription, Task, keyboard,
    widget::{image, operation},
    window,
};
use iced_t3::chrome;
use razerr_model::{
    Command, Direction, DpiStages, EffectKind, PollRate, Rgb, Serial, Snapshot, SnapshotKind,
    ZoneId,
};
use razerr_storage::{Paths, Settings};

use crate::{
    Activation, Options,
    activity::{Activity, Confirmed, Control},
    demo::{self, Demo},
    history::{self, History},
    state::{Axis, Confirmation, Page, Preferences, Section, State, Target},
    subscriptions::{self, Activations},
    view,
};

#[derive(Clone)]
pub struct Service(pub Arc<razerr_ipc::Client>);
impl std::fmt::Debug for Service {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Service")
    }
}

/// Changes to device drafts. Nothing reaches the device until `Apply`.
#[derive(Clone, Debug)]
pub enum Edit {
    /// A discrete (single-axis) DPI choice.
    DpiX(String),
    /// Choose what the DPI pad and X/Y sliders edit.
    Target(Target),
    Axis(Axis, String),
    /// Position on the logarithmic slider, from 0 to 1.
    AxisPosition(Axis, f32),
    Vector(u16, u16),
    Linked(bool),
    ActiveStage(u8),
    AddStage,
    RemoveStage(usize),
    Idle(String),
    Threshold(String),
    Kind(ZoneId, EffectKind),
    Color(ZoneId, bool, String),
    Swatch(ZoneId, bool, Rgb),
    Speed(ZoneId, u8),
    Direction(ZoneId, Direction),
    Brightness(ZoneId, f64),
    /// Send every unapplied draft of a section to the device.
    Apply(Section),
}

#[derive(Clone, Debug)]
pub enum Preference {
    Connections(bool),
    Battery(bool),
    History(bool),
    Days(String),
    Seconds(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Nothing to do: what a control sends while it can't be used.
    Idle,
    Connected(Service),
    Disconnected(String),
    Snapshot(Arc<Snapshot>),
    Error(String),
    Dismiss,
    Select(Serial),
    Page(Page),
    Preferences,
    Refresh,
    /// Switch the device to a one-based DPI stage.
    ActivateStage(u8),
    PollRate(u16),
    Edit(Edit),
    /// Discard a section's unapplied device edits.
    Revert(Section),
    Preference(Preference),
    /// Discard unsaved preferences.
    RevertPreferences,
    /// A change ended; `Ok` carries the state the device reported afterwards.
    Finished(u64, Result<Arc<Snapshot>, String>),
    Image(String, Result<image::Handle, String>),
    History(History),
    Tick,
    /// Expire a confirmation that has been shown for long enough.
    Frame,
    Save,
    Saved(u64, Result<Settings, String>),
    Reloaded(Result<Settings, String>),
    Ask(Confirmation),
    Cancel,
    Confirm,
    Cleared(u64, Result<(), String>),
    Key(keyboard::Key, keyboard::Modifiers),
    Opened(window::Id),
    Close(window::Id),
    Activation(Activation),
    Window(chrome::Action),
}

/// What device changes go through.
enum Backend {
    /// The razerr-rs service, once connected.
    Service(Option<Service>),
    /// The offline stand-in. Nothing it does leaves the process.
    Demo(Demo),
}

/// Runs `commands` in order and stops at the first that fails: stage activation
/// can change the current DPI, so later ones may no longer make sense.
pub(crate) async fn in_order<Sent: Future<Output = Result<(), String>>>(
    commands: Vec<Command>,
    mut send: impl FnMut(Command) -> Sent,
) -> Result<(), String> {
    for command in commands {
        send(command).await?;
    }
    Ok(())
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent read coalescing and window shutdown flags"
)]
pub struct App {
    pub state: State,
    pub images: HashMap<String, image::Handle>,
    /// `None` in the demo, which touches no files.
    paths: Option<Paths>,
    requested: HashSet<String>,
    loader: Option<razerr_images::ImageLoader>,
    pub history: History,
    history_loading: bool,
    history_again: bool,
    connection_error: Option<String>,
    backend: Backend,
    activations: Option<Activations>,
    window: Option<window::Id>,
    closing: bool,
    pub pending: Option<Activity>,
    pub confirmed: Option<Confirmed>,
    next_activity: u64,
}

impl App {
    pub fn new(options: Options, activations: Activations) -> (Self, Task<Message>) {
        let loader = razerr_images::ImageLoader::new(options.paths.images_dir()).ok();
        let mut app = Self::with_backend(
            Backend::Service(None),
            State::new(options.settings, options.show_settings),
        );
        app.state.error.clone_from(&options.initial_error);
        app.paths = Some(options.paths);
        app.loader = loader;
        app.activations = Some(activations);
        let task = if app.state.preferences {
            app.load_history()
        } else {
            Task::none()
        };
        (app, task)
    }

    /// An app on the offline stand-in, showing its first device.
    pub fn demo() -> Self {
        let demo = Demo::new();
        let snapshot = demo.snapshot();
        let mut app =
            Self::with_backend(Backend::Demo(demo), State::new(Settings::default(), false));
        app.state.receive(snapshot);
        app.history = demo::history(app.state.selected());
        app
    }

    fn with_backend(backend: Backend, state: State) -> Self {
        Self {
            state,
            images: HashMap::new(),
            paths: None,
            requested: HashSet::new(),
            loader: None,
            history: History::default(),
            history_loading: false,
            history_again: false,
            connection_error: None,
            backend,
            activations: None,
            window: None,
            closing: false,
            pending: None,
            confirmed: None,
            next_activity: 0,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::open_events().map(Message::Opened),
            window::close_requests().map(Message::Close),
            iced::event::listen_with(|event, status, _| {
                if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) =
                    event
                    && (status == iced::event::Status::Ignored
                        || modifiers.command()
                        || matches!(
                            key,
                            keyboard::Key::Named(
                                keyboard::key::Named::Escape | keyboard::key::Named::Tab
                            )
                        ))
                {
                    Some(Message::Key(key, modifiers))
                } else {
                    None
                }
            }),
            iced::time::every(Duration::from_secs(30)).map(|_| Message::Tick),
        ];
        if matches!(self.backend, Backend::Service(_)) {
            subscriptions.push(subscriptions::service());
        }
        if let Some(activations) = &self.activations {
            subscriptions.push(subscriptions::activation_subscription(activations.clone()));
        }
        // Only while there's something to expire: an idle window shouldn't wake.
        if self.confirmed.is_some() {
            subscriptions
                .push(iced::time::every(Duration::from_millis(100)).map(|_| Message::Frame));
        }
        Subscription::batch(subscriptions)
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn in_flight(&self, control: Control) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|activity| activity.control == control)
    }
    pub fn just_confirmed(&self, control: Control) -> bool {
        self.confirmed
            .as_ref()
            .is_some_and(|confirmed| confirmed.control == control)
    }
    /// Whether this change waits for the one in flight. Controls keep their look
    /// during a write; they just don't do anything until the device has answered.
    fn held(message: &Message) -> bool {
        matches!(
            message,
            Message::Select(_)
                | Message::Refresh
                | Message::ActivateStage(_)
                | Message::PollRate(_)
                | Message::Edit(_)
                | Message::Revert(_)
                | Message::Preference(_)
                | Message::RevertPreferences
                | Message::Save
                | Message::Ask(_)
        )
    }
    /// Claims the one slot for a change in flight and says how to report its end.
    fn begin(&mut self, control: Control, progress: String, done: String) -> u64 {
        self.next_activity += 1;
        self.confirmed = None;
        self.pending = Some(Activity {
            id: self.next_activity,
            control,
            progress,
            done,
            serial: self.state.selected.clone(),
        });
        self.next_activity
    }
    /// Ends the change `id`, if it's the one in flight.
    fn finish(&mut self, id: u64) -> Option<Activity> {
        self.pending.take_if(|activity| activity.id == id)
    }
    fn confirm(&mut self, activity: Activity) {
        self.confirmed = Some(Confirmed {
            control: activity.control,
            text: activity.done,
            at: Instant::now(),
        });
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Message dispatch delegates device edits, history, closing and views to separate functions"
    )]
    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.busy() && Self::held(&message) {
            return Task::none();
        }
        match message {
            Message::Idle => {}
            Message::Connected(service) => {
                if self.state.error == self.connection_error {
                    self.state.error = None;
                }
                self.connection_error = None;
                self.backend = Backend::Service(Some(service));
                self.state.connected = true;
                self.state.received = false;
            }
            Message::Disconnected(error) => {
                self.backend = Backend::Service(None);
                self.pending = None;
                self.state.connected = false;
                self.state.received = true;
                self.connection_error =
                    Some(format!("Cannot reach the razerr-rs service: {error}"));
                self.state.error.clone_from(&self.connection_error);
            }
            Message::Snapshot(snapshot) => {
                let sampled = snapshot.kind != SnapshotKind::Ack;
                self.state.receive(snapshot);
                let mut tasks = self.image_tasks();
                if sampled {
                    tasks.push(self.load_history());
                }
                return Task::batch(tasks);
            }
            Message::Error(error) => self.state.error = Some(error),
            Message::Dismiss => {
                self.state
                    .dismissed_error
                    .clone_from(&self.state.snapshot.error);
                self.state.error = None;
            }
            Message::Select(serial) => {
                self.state.select(serial);
                return self.load_history();
            }
            Message::Page(page) => {
                self.state.page = page;
                self.state.preferences = false;
            }
            Message::Preferences => {
                // The draft follows saved settings itself, so leaving the page and
                // coming back keeps unsaved edits.
                self.state.preferences = true;
                let Some(paths) = self.paths.clone() else {
                    return Task::none();
                };
                return Task::batch([
                    self.load_history(),
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                Settings::load(&paths).map_err(|error| error.to_string())
                            })
                            .await
                            .map_err(|error| error.to_string())?
                        },
                        Message::Reloaded,
                    ),
                ]);
            }
            Message::Refresh => {
                self.state.editor = crate::state::Editor::default();
                if let Some(device) = self.state.selected().cloned() {
                    self.state.editor.sync(&device);
                }
                let id = self.begin(
                    Control::Refresh,
                    "Reading devices…".into(),
                    "Devices updated".into(),
                );
                return self.send(id, vec![Command::Refresh]);
            }
            Message::ActivateStage(stage) => return self.activate_stage(stage),
            Message::PollRate(rate) => return self.set_poll_rate(rate),
            Message::Edit(edit) => return self.edit(edit),
            Message::Revert(section) => self.state.revert(Some(section)),
            Message::Preference(preference) => {
                self.preference(preference);
            }
            Message::RevertPreferences => {
                self.state.draft = self.state.settings.clone().into();
            }
            Message::Finished(id, result) => return self.finished(id, result),
            Message::Image(source, result) => match result {
                Ok(image) => {
                    self.images.insert(source, image);
                }
                Err(error) => tracing::debug!(%error,"device image unavailable"),
            },
            Message::History(history) => {
                self.history_loading = false;
                if history.serial.as_ref() == self.state.selected().map(|device| &device.serial) {
                    self.history = history;
                } else {
                    self.history_again = true;
                }
                if self.history_again {
                    self.history_again = false;
                    return self.load_history();
                }
            }
            Message::Tick => return self.load_history(),
            Message::Frame => {
                self.confirmed.take_if(|confirmed| confirmed.expired());
            }
            Message::Save => return self.save(),
            Message::Saved(id, result) => {
                if let Some(activity) = self.finish(id) {
                    match result {
                        Ok(settings) => {
                            self.state.finish_save(settings);
                            self.confirm(activity);
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
            }
            Message::Reloaded(result) => match result {
                Ok(settings) => {
                    if self.state.draft.validated().as_ref() == Ok(&self.state.settings) {
                        self.state.draft = settings.clone().into();
                    }
                    self.state.settings = settings;
                }
                Err(error) => self.state.error = Some(error),
            },
            Message::Ask(kind) => {
                self.state.open_confirmation(kind);
                return operation::focus("Cancel");
            }
            Message::Cancel => self.state.cancel(),
            Message::Confirm => match self.state.confirmation.take() {
                Some(Confirmation::Defaults) => {
                    self.state.draft = Preferences::from(Settings::default());
                }
                Some(Confirmation::ClearHistory) => return self.clear_history(),
                None => {}
            },
            Message::Cleared(id, result) => {
                if let Some(activity) = self.finish(id) {
                    match result {
                        Ok(()) => self.confirm(activity),
                        Err(error) => self.state.error = Some(error),
                    }
                }
                return self.load_history();
            }
            Message::Key(key, modifiers) => return self.key(&key, modifiers),
            Message::Opened(id) => {
                self.window = Some(id);
                if self.closing {
                    return window::close(id);
                }
            }
            Message::Close(id) => return self.close(id),
            Message::Activation(activation) => {
                match activation {
                    Activation::Present => {}
                    Activation::Settings => {
                        let task = self.update(Message::Preferences);
                        return Task::batch([task, self.present()]);
                    }
                    Activation::Quit => {
                        if let Some(id) = self.window {
                            return self.close(id);
                        }
                        self.closing = true;
                    }
                }
                return self.present();
            }
            Message::Window(chrome::Action::Close) => {
                if let Some(id) = self.window {
                    return self.close(id);
                }
                return chrome::perform(chrome::Action::Close);
            }
            Message::Window(action) => return chrome::perform(action),
        }
        Task::none()
    }

    /// Switches to a stage using the values the device reported, so unapplied
    /// stage edits can't reach it this way.
    fn activate_stage(&mut self, stage: u8) -> Task<Message> {
        let Some(device) = self.state.selected() else {
            return Task::none();
        };
        if self.state.editor.stages_dirty {
            return Task::none();
        }
        let serial = device.serial.clone();
        let stages = device
            .stages
            .as_ref()
            .and_then(|reading| reading.fresh())
            .ok_or("DPI stages are unavailable".to_owned())
            .and_then(|stages| {
                DpiStages::new(stage, stages.values().to_vec()).map_err(|error| error.to_string())
            });
        let command = stages.map(|stages| Command::SetDpiStages { serial, stages });
        self.send_validated(
            command,
            Control::Stage(stage),
            format!("Switching to stage {stage}…"),
            format!("Stage {stage} active"),
        )
    }

    fn set_poll_rate(&mut self, rate: u16) -> Task<Message> {
        let Some(device) = self.state.selected() else {
            return Task::none();
        };
        let serial = device.serial.clone();
        let command = PollRate::new(rate)
            .map_err(|error| error.to_string())
            .map(|rate| Command::SetPollRate { serial, rate });
        let label = view::format_rate(rate);
        self.send_validated(
            command,
            Control::Poll(rate),
            format!("Setting polling to {label}…"),
            format!("Polling at {label}"),
        )
    }

    /// One change straight from a control, once the device can accept it.
    fn send_validated(
        &mut self,
        command: Result<Command, String>,
        control: Control,
        progress: String,
        done: String,
    ) -> Task<Message> {
        let checked = command.and_then(|command| {
            let device = self.state.selected().ok_or("Device was removed")?;
            command
                .validate(device)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(command)
        });
        match checked {
            Ok(command) if self.state.controls_enabled() => {
                let id = self.begin(control, progress, done);
                self.send(id, vec![command])
            }
            Ok(_) => Task::none(),
            Err(error) => {
                self.state.error = Some(error);
                Task::none()
            }
        }
    }

    pub(crate) fn apply(&mut self, section: Section, commands: Vec<Command>) -> Task<Message> {
        if commands.is_empty() {
            return Task::none();
        }
        let name = section.title();
        let id = self.begin(
            Control::Apply(section),
            format!("Applying {}…", name.to_lowercase()),
            format!("{name} applied"),
        );
        self.send(id, commands)
    }

    /// Sends `commands` for the activity `id`, and reads the state they left.
    fn send(&self, id: u64, commands: Vec<Command>) -> Task<Message> {
        let result = match &self.backend {
            Backend::Service(service) => {
                let service = service.clone();
                Task::perform(
                    async move {
                        let service = service.ok_or("Service is unavailable")?;
                        in_order(commands, |command| {
                            let service = service.clone();
                            async move {
                                service
                                    .0
                                    .send(command)
                                    .await
                                    .map_err(|error| error.to_string())
                            }
                        })
                        .await?;
                        let snapshot = service.0.snapshot().await;
                        snapshot.map(Arc::new).map_err(|error| error.to_string())
                    },
                    move |result| (id, result),
                )
            }
            Backend::Demo(demo) => {
                let demo = demo.clone();
                Task::perform(
                    async move {
                        in_order(commands, |command| {
                            let demo = demo.clone();
                            async move { demo.send(command).await }
                        })
                        .await?;
                        Ok(demo.snapshot())
                    },
                    move |result| (id, result),
                )
            }
        };
        result.map(|(id, result)| Message::Finished(id, result))
    }

    fn finished(&mut self, id: u64, result: Result<Arc<Snapshot>, String>) -> Task<Message> {
        let Some(activity) = self.finish(id) else {
            return Task::none();
        };
        match result {
            Ok(snapshot) => {
                let task = self.update(Message::Snapshot(snapshot));
                if let Control::Apply(section) = activity.control
                    && activity.serial == self.state.selected
                {
                    // The device has answered, so its values replace the drafts,
                    // even where it normalized what it was given.
                    self.state.revert(Some(section));
                }
                self.confirm(activity);
                task
            }
            Err(error) => {
                self.state.error = Some(error);
                Task::none()
            }
        }
    }

    fn save(&mut self) -> Task<Message> {
        let settings = match self.state.draft.validated() {
            Ok(settings) => settings,
            Err(error) => {
                self.state.error = Some(error);
                return Task::none();
            }
        };
        let paths = self.paths.clone();
        let id = self.begin(
            Control::Save,
            "Saving preferences…".into(),
            "Preferences saved".into(),
        );
        Task::perform(
            async move {
                let Some(paths) = paths else {
                    return Ok(settings);
                };
                tokio::task::spawn_blocking(move || {
                    settings
                        .save(&paths)
                        .map(|()| settings)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::Saved(id, result),
        )
    }

    fn clear_history(&mut self) -> Task<Message> {
        let service = match &self.backend {
            Backend::Service(service) => service.clone(),
            Backend::Demo(_) => {
                self.history.points.clear();
                self.history.prediction = None;
                self.history.usage = razerr_metrics::Usage::default();
                return Task::none();
            }
        };
        let id = self.begin(
            Control::ClearHistory,
            "Deleting history…".into(),
            "History deleted".into(),
        );
        Task::perform(
            async move {
                let service = service.ok_or("Service is unavailable")?;
                razerr_ipc::ControlProxy::builder(service.0.connection())
                    .destination(service.0.destination())
                    .build()
                    .await
                    .map_err(|error| error.to_string())?
                    .clear_history()
                    .await
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Cleared(id, result),
        )
    }

    fn preference(&mut self, preference: Preference) {
        match preference {
            Preference::Connections(value) => {
                self.state.draft.values.notifications.connections = value;
            }
            Preference::Battery(value) => self.state.draft.values.notifications.battery = value,
            Preference::History(value) => self.state.draft.values.history.enabled = value,
            Preference::Days(value) => self.state.draft.days = value,
            Preference::Seconds(value) => self.state.draft.seconds = value,
        }
    }

    fn present(&self) -> Task<Message> {
        self.window.map_or_else(Task::none, |id| {
            Task::batch([
                window::set_mode(id, window::Mode::Windowed),
                window::gain_focus(id),
            ])
        })
    }
    fn close(&mut self, id: window::Id) -> Task<Message> {
        if self.closing {
            return Task::none();
        }
        self.closing = true;
        window::close(id)
    }
    fn load_history(&mut self) -> Task<Message> {
        let Some(paths) = &self.paths else {
            self.history = demo::history(self.state.selected());
            return Task::none();
        };
        let path = paths.history_file();
        let selected = self.state.selected().map(|device| device.serial.clone());
        if self.history.serial != selected {
            self.history = History {
                serial: selected,
                now: history::now(),
                ..History::default()
            };
        }
        if self.history_loading {
            self.history_again = true;
            return Task::none();
        }
        self.history_loading = true;
        Task::perform(
            history::load(
                path,
                self.state.selected().cloned(),
                self.state.connected
                    && self.state.snapshot.reliable
                    && self.state.snapshot.daemon_available,
            ),
            Message::History,
        )
    }
    fn image_tasks(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        if let Some(loader) = &self.loader {
            for source in self
                .state
                .snapshot
                .devices
                .iter()
                .filter_map(|device| device.image.as_ref())
            {
                if self.requested.insert(source.clone()) {
                    let source = source.clone();
                    let loader = loader.clone();
                    tasks.push(Task::perform(
                        async move {
                            let result = loader
                                .load(&source)
                                .await
                                .map(|image| {
                                    image::Handle::from_rgba(
                                        image.width,
                                        image.height,
                                        image.pixels,
                                    )
                                })
                                .map_err(|error| error.to_string());
                            (source, result)
                        },
                        |(source, result)| Message::Image(source, result),
                    ));
                }
            }
        }
        tasks
    }
    fn key(&mut self, key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        use keyboard::{Key, key::Named};
        if *key == Key::Named(Named::Escape) {
            if self.state.confirmation.is_some() {
                self.state.cancel();
            } else if !self.busy() {
                self.state.revert(None);
            }
            return Task::none();
        }
        if *key == Key::Named(Named::Tab) {
            if self.state.confirmation.is_some() {
                self.state.confirm_focused = !self.state.confirm_focused;
                return match self.state.confirmation {
                    Some(kind) if self.state.confirm_focused => {
                        operation::focus(Self::confirm_id(kind))
                    }
                    _ => operation::focus("Cancel"),
                };
            }
            return if modifiers.shift() {
                operation::focus_previous()
            } else {
                operation::focus_next()
            };
        }
        if self.state.confirmation.is_some() {
            return Task::none();
        }
        if modifiers.command()
            && matches!(key.as_ref(), Key::Character(key) if key.eq_ignore_ascii_case("r"))
        {
            return self.update(Message::Refresh);
        }
        if modifiers.command() && key.as_ref() == Key::Character(",") {
            return self.update(Message::Preferences);
        }
        if modifiers.command()
            && let Key::Character(value) = key.as_ref()
            && let Ok(active) = value.parse::<u8>()
            && (1..=5).contains(&active)
        {
            self.state.page = Page::Sensitivity;
            self.state.preferences = false;
            return self.update(Message::Edit(Edit::ActiveStage(active)));
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            serde_json::from_str(include_str!("../tests/captures/wired.json"))
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
        let _ = app.update(Message::ActivateStage(1));
        assert!(app.in_flight(Control::Stage(1)));
        assert!(!app.in_flight(Control::Stage(2)));
        assert_eq!(
            app.pending.as_ref().expect("in flight").progress,
            "Switching to stage 1…"
        );
        // A background poll is not the write's confirmation.
        let poll = changed(&app, |_| {});
        let _ = app.update(Message::Snapshot(poll));
        assert!(app.in_flight(Control::Stage(1)));
        assert!(app.confirmed.is_none());
        let done = changed(&app, |snapshot| activate(snapshot, 1));
        let _ = app.update(Message::Finished(id(&app), Ok(done)));
        assert!(!app.busy());
        assert!(app.just_confirmed(Control::Stage(1)));
        assert_eq!(
            app.confirmed.as_ref().expect("confirmed").text,
            "Stage 1 active"
        );
    }

    #[test]
    fn the_old_selection_stays_until_the_device_confirms() {
        let mut app = connected();
        let before = active_stage(&app);
        let _ = app.update(Message::ActivateStage(1));
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
            control: Control::Stage(1),
            text: "Done".into(),
            at: Instant::now()
                .checked_sub(crate::activity::CONFIRMATION)
                .expect("a recent instant"),
        });
        let _ = app.update(Message::Frame);
        assert!(app.confirmed.is_none());
        let _ = app.update(Message::PollRate(500));
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
        let _ = app.update(Message::PollRate(500));
        let first = id(&app);
        for message in [
            Message::Edit(Edit::Idle("900".into())),
            Message::Revert(Section::Power),
            Message::ActivateStage(1),
            Message::PollRate(125),
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
        let _ = app.update(Message::PollRate(500));
        let stale = id(&app);
        let _ = app.update(Message::Disconnected("gone".into()));
        assert!(!app.busy());
        assert!(app.state.error.is_some());
        // A reply that arrives after the fact can't end a newer change.
        let _ = app.update(Message::Snapshot(wired()));
        app.state.connected = true;
        let _ = app.update(Message::PollRate(125));
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
        app.pending.as_mut().expect("in flight").serial =
            Some(Serial::new("other").expect("serial"));
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
    fn a_stage_draft_never_reaches_the_device_through_a_stage_switch() {
        let mut app = connected();
        let _ = app.update(Message::Edit(Edit::Vector(2000, 2000)));
        let _ = app.update(Message::Edit(Edit::Target(Target::Stage(0))));
        let _ = app.update(Message::Edit(Edit::Vector(9000, 9000)));
        assert!(app.state.editor.stages_dirty);
        let _ = app.update(Message::ActivateStage(1));
        assert!(!app.busy(), "stage edits must be applied or reverted first");
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
}
