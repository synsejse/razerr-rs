//! What the app is doing to a device, so that progress can appear on the control
//! being changed rather than on the app as a whole.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use iced::Task;
use razerr_model::{Command, DpiStages, PollRate, Serial, Snapshot};
use razerr_storage::Settings;

use crate::{
    app::{App, Message},
    demo::Demo,
    state::Section,
    view,
};

/// How long a finished change keeps its check mark.
pub const CONFIRMATION: Duration = Duration::from_millis(1600);

/// The control a change belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Refresh,
    /// One-based DPI stage being activated.
    Stage(u8),
    Poll(u16),
    Apply(Section),
    Save,
    ClearHistory,
}

/// A change in flight, and what to say about it before and after.
#[derive(Clone, Debug)]
pub struct Activity {
    pub id: u64,
    pub control: Control,
    pub progress: String,
    pub done: String,
    /// The device it was sent to, if any.
    pub serial: Option<Serial>,
}

/// A change the device confirmed.
#[derive(Clone, Debug)]
pub struct Confirmed {
    pub control: Control,
    pub text: String,
    pub at: Instant,
}

impl Confirmed {
    pub fn expired(&self) -> bool {
        self.at.elapsed() >= CONFIRMATION
    }
}

/// What device changes go through.
pub enum Backend {
    /// The razerr-rs service, once connected.
    Service(Option<razerr_ipc::Client>),
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

impl App {
    /// Claims the one slot for a change in flight and says how to report its end.
    pub(crate) fn begin(&mut self, control: Control, progress: String, done: String) -> u64 {
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

    /// Switches to a stage using the values the device reported, so unapplied
    /// stage edits can't reach it this way.
    pub(crate) fn activate_stage(&mut self, stage: u8) -> Task<Message> {
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

    pub(crate) fn set_poll_rate(&mut self, rate: u16) -> Task<Message> {
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
    pub(crate) fn send(&self, id: u64, commands: Vec<Command>) -> Task<Message> {
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
                                    .send(command)
                                    .await
                                    .map_err(|error| error.to_string())
                            }
                        })
                        .await?;
                        let snapshot = service.snapshot().await;
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

    pub(crate) fn finished(
        &mut self,
        id: u64,
        result: Result<Arc<Snapshot>, String>,
    ) -> Task<Message> {
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

    pub(crate) fn save(&mut self) -> Task<Message> {
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

    pub(crate) fn saved(&mut self, id: u64, result: Result<Settings, String>) {
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

    pub(crate) fn cleared(&mut self, id: u64, result: Result<(), String>) -> Task<Message> {
        if let Some(activity) = self.finish(id) {
            match result {
                Ok(()) => self.confirm(activity),
                Err(error) => self.state.error = Some(error),
            }
        }
        self.load_history()
    }

    pub(crate) fn clear_history(&mut self) -> Task<Message> {
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
                razerr_ipc::ControlProxy::builder(service.connection())
                    .destination(service.destination())
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
}
