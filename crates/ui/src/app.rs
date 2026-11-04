use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use iced::{
    Subscription, Task, Theme, keyboard,
    widget::{image, operation},
    window,
};
use razerr_model::{Command, Direction, EffectKind, Rgb, Serial, Snapshot, SnapshotKind, ZoneId};
use razerr_storage::Settings;

use crate::{
    Activation, Options,
    history::{self, History},
    state::{Axis, Confirmation, Preferences, Section, State, Target},
    subscriptions::{self, Activations},
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
    /// Normalized logarithmic slider gesture.
    AxisGesture(Axis, iced_audio::Gesture),
    Vector(u16, u16),
    Linked(bool),
    ActiveStage(u8),
    AddStage,
    RemoveStage(usize),
    Poll(u16),
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
    Dark(bool),
    Connections(bool),
    Battery(bool),
    History(bool),
    Days(String),
    Seconds(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    Connected(Service),
    Disconnected(String),
    Snapshot(Arc<Snapshot>),
    Error(String),
    Dismiss,
    Select(Serial),
    Preferences,
    Back,
    Refresh,
    Toggle(Section),
    Edit(Edit),
    /// Discard a section's unapplied device edits.
    Revert(Section),
    Preference(Preference),
    CommandFinished(Result<(), String>),
    Image(String, Result<image::Handle, String>),
    History(History),
    Tick,
    Save,
    Saved(Result<Settings, String>),
    Reloaded(Result<Settings, String>),
    Ask(Confirmation),
    Cancel,
    Confirm,
    Cleared(Result<(), String>),
    Key(keyboard::Key, keyboard::Modifiers),
    Opened(window::Id),
    Close(window::Id),
    Activation(Activation),
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent read coalescing and window shutdown flags"
)]
pub struct App {
    pub state: State,
    pub options: Options,
    pub images: HashMap<String, image::Handle>,
    /// The application icon, shown in the sidebar.
    pub icon: image::Handle,
    requested: HashSet<String>,
    loader: Option<razerr_images::ImageLoader>,
    pub history: History,
    history_loading: bool,
    history_again: bool,
    connection_error: Option<String>,
    service: Option<Service>,
    activations: Activations,
    window: Option<window::Id>,
    closing: bool,
}

impl App {
    pub fn new(options: Options, activations: Activations) -> (Self, Task<Message>) {
        let loader = razerr_images::ImageLoader::new(options.paths.images_dir()).ok();
        let mut state = State::new(
            options.settings.clone(),
            options.dark_override,
            options.show_settings,
        );
        state.error.clone_from(&options.initial_error);
        let mut app = Self {
            state,
            options,
            images: HashMap::new(),
            icon: app_icon(),
            requested: HashSet::new(),
            loader,
            history: History::default(),
            history_loading: false,
            history_again: false,
            connection_error: None,
            service: None,
            activations,
            window: None,
            closing: false,
        };
        let task = if app.state.preferences {
            app.load_history()
        } else {
            Task::none()
        };
        (app, task)
    }
    pub fn theme(&self) -> Theme {
        let dark = self.state.dark_override
            || if self.state.preferences {
                self.state.draft.values.appearance.dark
            } else {
                self.state.settings.appearance.dark
            };
        crate::style::theme(dark)
    }
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            subscriptions::service(),
            subscriptions::activation_subscription(self.activations.clone()),
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
        ])
    }
    #[allow(
        clippy::too_many_lines,
        reason = "Message dispatch delegates device edits, history, closing and views to separate functions"
    )]
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connected(service) => {
                if self.state.error == self.connection_error {
                    self.state.error = None;
                }
                self.connection_error = None;
                self.service = Some(service);
                self.state.connected = true;
                self.state.received = false;
            }
            Message::Disconnected(error) => {
                self.service = None;
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
            Message::Preferences => {
                // The draft follows saved settings itself, so leaving the page and
                // coming back keeps unsaved edits.
                self.state.preferences = true;
                self.state.saved = false;
                let paths = self.options.paths.clone();
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
            Message::Back => self.state.preferences = false,
            Message::Refresh => {
                self.state.editor = crate::state::Editor::default();
                if let Some(device) = self.state.selected().cloned() {
                    self.state.editor.sync(&device);
                }
                return self.send(Command::Refresh);
            }
            Message::Toggle(section) => {
                if !self.state.collapsed.remove(&section) {
                    self.state.collapsed.insert(section);
                }
            }
            Message::Edit(edit) => return self.edit(edit),
            Message::Revert(section) => self.state.revert(Some(section)),
            Message::Preference(preference) => {
                self.preference(preference);
            }
            Message::CommandFinished(result) => {
                if let Err(error) = result {
                    self.state.error = Some(error);
                }
            }
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
            Message::Save => {
                let settings = match self.state.draft.validated() {
                    Ok(settings) => settings,
                    Err(error) => {
                        self.state.error = Some(error);
                        return Task::none();
                    }
                };
                let paths = self.options.paths.clone();
                self.state.saving = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            settings
                                .save(&paths)
                                .map(|()| settings)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::Saved,
                );
            }
            Message::Saved(result) => {
                self.state.saving = false;
                match result {
                    Ok(settings) => {
                        self.state.finish_save(settings);
                    }
                    Err(error) => self.state.error = Some(error),
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
                return operation::focus("modal-cancel");
            }
            Message::Cancel => self.state.cancel(),
            Message::Confirm => match self.state.confirmation.take() {
                Some(Confirmation::Defaults) => {
                    self.state.draft = Preferences::from(Settings::default());
                    self.state.saved = false;
                    self.state.dark_override = false;
                }
                Some(Confirmation::ClearHistory) => {
                    let service = self.service.clone();
                    return Task::perform(
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
                        Message::Cleared,
                    );
                }
                None => {}
            },
            Message::Cleared(result) => {
                if let Err(error) = result {
                    self.state.error = Some(error);
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
                    Activation::Dark => self.state.dark_override = true,
                    Activation::Quit => {
                        if let Some(id) = self.window {
                            return self.close(id);
                        }
                        self.closing = true;
                    }
                }
                return self.present();
            }
        }
        Task::none()
    }

    fn preference(&mut self, preference: Preference) {
        self.state.saved = false;
        match preference {
            Preference::Dark(value) => {
                self.state.draft.values.appearance.dark = value;
                self.state.dark_override = false;
            }
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
                self.options.paths.history_file(),
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
    pub(crate) fn send(&self, command: Command) -> Task<Message> {
        self.send_all(vec![command])
    }

    pub(crate) fn send_all(&self, commands: Vec<Command>) -> Task<Message> {
        if commands.is_empty() {
            return Task::none();
        }
        let service = self.service.clone();
        Task::perform(
            async move {
                let service = service.ok_or("Service is unavailable")?;
                // Stage activation can change current DPI. Preserve the validated
                // command order and stop if a device write fails.
                for command in commands {
                    service
                        .0
                        .send(command)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
            Message::CommandFinished,
        )
    }
    fn key(&mut self, key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        use keyboard::{Key, key::Named};
        if *key == Key::Named(Named::Escape) {
            if self.state.confirmation.is_some() {
                self.state.cancel();
            } else {
                self.state.revert(None);
            }
            return Task::none();
        }
        if *key == Key::Named(Named::Tab) {
            if self.state.confirmation.is_some() {
                self.state.confirm_focused = !self.state.confirm_focused;
                return operation::focus(if self.state.confirm_focused {
                    "modal-confirm"
                } else {
                    "modal-cancel"
                });
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
            return self.edit(Edit::ActiveStage(active));
        }
        Task::none()
    }
}

fn app_icon() -> image::Handle {
    razerr_images::decode(include_bytes!("../../../assets/icons/razerr-rs.png")).map_or_else(
        |_| image::Handle::from_rgba(1, 1, vec![0; 4]),
        |icon| image::Handle::from_rgba(icon.width, icon.height, icon.pixels),
    )
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
                dark_override: false,
                show_settings: false,
                initial_error: None,
            },
            Activations::new(receiver),
        )
        .0
    }

    #[test]
    fn opening_preferences_again_keeps_unsaved_edits() {
        let mut app = app();
        let _ = app.update(Message::Preferences);
        let _ = app.update(Message::Preference(Preference::Days("30".into())));
        let _ = app.update(Message::Preferences);
        assert_eq!(app.state.draft.days, "30");
        let _ = app.update(Message::Back);
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
