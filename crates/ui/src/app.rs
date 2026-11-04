use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use iced::{
    Subscription, Task, keyboard,
    widget::{image, operation},
    window,
};
use iced_t3::chrome;
use razerr_model::{Command, Direction, EffectKind, Serial, Snapshot, SnapshotKind, ZoneId};
use razerr_storage::{Paths, Settings};

use crate::{
    Activation, Options,
    activity::{Activity, Backend, Confirmed, Control},
    demo::{self, Demo},
    editor::{Axis, Editor, Target},
    history::{self, History},
    state::{Confirmation, Page, Preferences, Section, State},
    subscriptions::{self, Activations},
};

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
    PollRate(u16),
    AddStage,
    RemoveStage(usize),
    Idle(String),
    Threshold(String),
    Kind(ZoneId, EffectKind),
    Color(ZoneId, bool, String),
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
    Connected(razerr_ipc::Client),
    Disconnected(String),
    Snapshot(Arc<Snapshot>),
    Error(String),
    Dismiss,
    Select(Serial),
    Page(Page),
    Preferences,
    Refresh,
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

pub struct App {
    pub state: State,
    pub images: HashMap<String, image::Handle>,
    /// `None` in the demo, which touches no files.
    pub(crate) paths: Option<Paths>,
    requested: HashSet<String>,
    loader: Option<razerr_images::ImageLoader>,
    pub history: History,
    history_loading: bool,
    history_again: bool,
    connection_error: Option<String>,
    pub(crate) backend: Backend,
    activations: Option<Activations>,
    window: Option<window::Id>,
    closing: bool,
    pub pending: Option<Activity>,
    pub confirmed: Option<Confirmed>,
    pub(crate) next_activity: u64,
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

    #[cfg(test)]
    fn just_confirmed(&self, control: Control) -> bool {
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
                | Message::Edit(_)
                | Message::Revert(_)
                | Message::Preference(_)
                | Message::RevertPreferences
                | Message::Save
                | Message::Ask(_)
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.busy() && Self::held(&message) {
            return Task::none();
        }
        match message {
            Message::Idle => {}
            Message::Connected(service) => self.connected(service),
            Message::Disconnected(error) => self.disconnected(&error),
            Message::Snapshot(snapshot) => return self.snapshot(snapshot),
            Message::Error(error) => self.state.error = Some(error),
            Message::Dismiss => self.state.dismiss_error(),
            Message::Select(serial) => {
                self.state.select(serial);
                return self.load_history();
            }
            Message::Page(page) => {
                self.state.page = page;
                self.state.preferences = false;
            }
            Message::Preferences => return self.open_preferences(),
            Message::Refresh => return self.refresh(),
            Message::Edit(edit) => return self.edit(edit),
            Message::Revert(section) => self.state.revert(Some(section)),
            Message::Preference(preference) => self.preference(preference),
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
            Message::History(history) => return self.history_loaded(history),
            Message::Tick => return self.load_history(),
            Message::Frame => {
                self.confirmed.take_if(|confirmed| confirmed.expired());
            }
            Message::Save => return self.save(),
            Message::Saved(id, result) => self.saved(id, result),
            Message::Reloaded(result) => self.reloaded(result),
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
            Message::Cleared(id, result) => return self.cleared(id, result),
            Message::Key(key, modifiers) => return self.key(&key, modifiers),
            Message::Opened(id) => {
                self.window = Some(id);
                if self.closing {
                    return window::close(id);
                }
            }
            Message::Close(id) => return self.close(id),
            Message::Activation(activation) => return self.activated(activation),
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

    fn connected(&mut self, service: razerr_ipc::Client) {
        if self.state.error == self.connection_error {
            self.state.error = None;
        }
        self.connection_error = None;
        self.backend = Backend::Service(Some(service));
        self.state.connected = true;
        self.state.received = false;
    }

    fn disconnected(&mut self, error: &str) {
        self.backend = Backend::Service(None);
        self.pending = None;
        self.state.connected = false;
        self.state.received = true;
        self.connection_error = Some(format!("Cannot reach the razerr-rs service: {error}"));
        self.state.error.clone_from(&self.connection_error);
    }

    fn snapshot(&mut self, snapshot: Arc<Snapshot>) -> Task<Message> {
        let sampled = snapshot.kind != SnapshotKind::Ack;
        self.state.receive(snapshot);
        let mut tasks = self.image_tasks();
        if sampled {
            tasks.push(self.load_history());
        }
        Task::batch(tasks)
    }

    /// The draft follows saved settings itself, so leaving the page and coming back
    /// keeps unsaved edits.
    fn open_preferences(&mut self) -> Task<Message> {
        self.state.preferences = true;
        let Some(paths) = self.paths.clone() else {
            return Task::none();
        };
        Task::batch([
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
        ])
    }

    fn reloaded(&mut self, result: Result<Settings, String>) {
        match result {
            Ok(settings) => {
                if self.state.draft.validated().as_ref() == Ok(&self.state.settings) {
                    self.state.draft = settings.clone().into();
                }
                self.state.settings = settings;
            }
            Err(error) => self.state.error = Some(error),
        }
    }

    fn refresh(&mut self) -> Task<Message> {
        self.state.editor = Editor::default();
        if let Some(device) = self.state.selected().cloned() {
            self.state.editor.sync(&device);
        }
        let id = self.begin(
            Control::Refresh,
            "Reading devices…".into(),
            "Devices updated".into(),
        );
        self.send(id, vec![Command::Refresh])
    }

    fn history_loaded(&mut self, history: History) -> Task<Message> {
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
        Task::none()
    }

    fn activated(&mut self, activation: Activation) -> Task<Message> {
        match activation {
            Activation::Present => {}
            Activation::Settings => {
                let task = self.open_preferences();
                return Task::batch([task, self.present()]);
            }
            Activation::Quit => {
                if let Some(id) = self.window {
                    return self.close(id);
                }
                self.closing = true;
            }
        }
        self.present()
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

    pub(crate) fn load_history(&mut self) -> Task<Message> {
        let Some(paths) = &self.paths else {
            self.history = demo::history(self.state.selected());
            return Task::none();
        };
        let path = paths.history_file();
        let selected = self.state.selected().map(|device| device.serial.clone());
        if self.history.serial != selected {
            self.history = History {
                serial: selected,
                now: razerr_metrics::unix_now(),
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
mod tests;
