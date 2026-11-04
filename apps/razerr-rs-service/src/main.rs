mod control;
mod monitor;
mod settings_watch;
mod telemetry;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use razerr_desktop::{DesktopAction, Notification, Notifications, Tray, launch_gui};
use razerr_model::{Snapshot, SnapshotKind};
use razerr_storage::{Paths, SettingsStore};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::{broadcast::error::RecvError, mpsc},
};
use tracing_subscriber::EnvFilter;

use control::{Action, Control, Request};
use monitor::{Event, Kind, Monitor};
use telemetry::Telemetry;

const SERVICE: &str = "org.razerr_rs.Service";
const CONTROL_PATH: &str = "/org/razerr_rs/Control";
const DEVICES_PATH: &str = "/org/razerr_rs/Devices";

#[derive(Parser)]
#[command(
    name = "razerr-rs-service",
    version,
    about = "OpenRazer monitoring, history and desktop service"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Subcommand)]
enum Command {
    Open,
    Settings,
    Refresh,
    Status,
    ClearHistory,
    Stop,
    Quit,
}

impl Command {
    fn method(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Settings => "Settings",
            Self::Refresh => "Refresh",
            Self::Status => "Status",
            Self::ClearHistory => "ClearHistory",
            Self::Stop => "Stop",
            Self::Quit => "Quit",
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RAZERR_RS_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let result = match cli.command {
        Some(command) => forward(command).await,
        None => serve().await,
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("razerr-rs-service: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn forward(command: Command) -> Result<()> {
    let connection = zbus::connection::Builder::session()
        .method_timeout(Duration::from_secs(10))
        .build()
        .await
        .context("session D-Bus unavailable")?;
    let proxy =
        zbus::Proxy::new(&connection, SERVICE, CONTROL_PATH, "org.razerr_rs.Control1").await?;
    let flags = zbus::proxy::MethodFlags::NO_AUTO_START;
    if matches!(command, Command::Status) {
        if let Some(status) = proxy
            .call_with_flags::<_, _, String>(command.method(), flags, &())
            .await
            .context("the razerr-rs service is not running or did not complete the request")?
        {
            println!("{status}");
        }
    } else {
        proxy
            .call_with_flags::<_, _, ()>(command.method(), flags, &())
            .await
            .context("the razerr-rs service is not running or did not complete the request")?;
    }
    Ok(())
}

/// Everything the service loop reacts with, once it's on the bus.
struct Service {
    connection: zbus::Connection,
    settings: SettingsStore,
    telemetry: Telemetry,
    tray: Option<Tray>,
    notifications: Option<Notifications>,
    actions: mpsc::Sender<DesktopAction>,
    monitor: Monitor,
    origin: Instant,
}

impl Service {
    async fn snapshot_changed(&mut self, snapshot: Arc<Snapshot>) {
        if let Err(error) = razerr_ipc::publish(&self.connection).await {
            tracing::warn!(%error, "publishing device snapshot");
        }
        if let Some(tray) = &self.tray
            && let Err(error) = tray.update(tray_lines(&snapshot)).await
        {
            tracing::warn!(%error, "updating tray");
        }
        if snapshot.kind != SnapshotKind::Ack {
            let elapsed = self.origin.elapsed();
            self.telemetry.observe(Arc::clone(&snapshot), elapsed);
            let events = self.monitor.update(&snapshot, elapsed);
            self.notify(events).await;
        }
    }

    async fn worker_failed(&self, message: &str) {
        tracing::warn!(%message, "device operation");
        if let Err(error) = razerr_ipc::emit_error(&self.connection, message).await {
            tracing::warn!(%error, "publishing error");
        }
    }

    fn reload_settings(&mut self) {
        match self.settings.reload() {
            Ok(true) => {
                self.telemetry.configure(self.settings.current());
                tracing::info!("settings reloaded");
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "keeping last valid settings"),
        }
    }

    async fn tick(&mut self) {
        let events = self.monitor.tick(self.origin.elapsed());
        self.notify(events).await;
    }

    async fn request(&self, action: Action) -> Result<(), String> {
        match action {
            Action::Open => launch_gui(false).map_err(|error| error.to_string()),
            Action::Settings => launch_gui(true).map_err(|error| error.to_string()),
            Action::ClearHistory => self.telemetry.clear().await,
            Action::Stop | Action::Quit => Ok(()),
        }
    }

    async fn notify(&mut self, events: Vec<Event>) {
        for event in events {
            let settings = &self.settings.current().notifications;
            let enabled = if event.kind.is_battery() {
                settings.battery
            } else {
                settings.connections
            };
            if !enabled {
                self.monitor.acknowledge(&event);
                continue;
            }
            if self.notifications.is_none() {
                match Notifications::connect(self.actions.clone()).await {
                    Ok(service) => self.notifications = Some(service),
                    Err(error) => {
                        tracing::warn!(%error, "notifications unavailable");
                        continue;
                    }
                }
            }
            if let Some(service) = &self.notifications {
                match service.show(&notification(&event)).await {
                    Ok(_) => self.monitor.acknowledge(&event),
                    Err(error) => tracing::warn!(%error, "notification failed"),
                }
            }
        }
    }

    async fn shutdown(self, keep_window: bool) {
        self.telemetry.shutdown().await;
        if let Some(tray) = self.tray {
            tray.shutdown().await;
        }
        if let Some(notifications) = self.notifications {
            notifications.shutdown().await;
        }
        if !keep_window {
            close_window(&self.connection).await;
        }
    }
}

async fn serve() -> Result<()> {
    let paths = Paths::discover()?;
    let mut settings = SettingsStore::new(paths.clone());
    if let Err(error) = settings.reload() {
        tracing::warn!(%error, "using default settings");
    }
    let mut worker = razerr_worker::spawn();
    let (requests, mut incoming) = mpsc::channel(32);
    let control = Control {
        requests,
        commands: worker.commands.clone(),
        snapshots: worker.snapshots.clone(),
    };
    let connection = zbus::connection::Builder::session()
        .serve_at(
            DEVICES_PATH,
            razerr_ipc::Devices::new(worker.commands.clone(), worker.snapshots.clone()),
        )
        .serve_at(CONTROL_PATH, control)
        .name(SERVICE)
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .method_timeout(Duration::from_secs(5))
        .build()
        .await;
    let connection = match connection {
        Ok(connection) => connection,
        Err(zbus::Error::NameTaken) => {
            worker.task.abort();
            return Ok(());
        }
        Err(error) => {
            worker.task.abort();
            return Err(error.into());
        }
    };
    let telemetry = Telemetry::start(paths.clone(), settings.current().clone())?;
    let (actions, mut desktop_actions) = mpsc::channel(32);
    let tray = match Tray::start(actions.clone()).await {
        Ok(tray) => Some(tray),
        Err(error) => {
            tracing::warn!(%error, "tray unavailable");
            None
        }
    };
    let (watcher, mut file_changes) = settings_watch::start(&paths)?;
    // Catch edits between the initial load and watcher registration, including
    // time spent waiting for the desktop and D-Bus services during startup.
    match settings.reload() {
        Ok(true) => telemetry.configure(settings.current()),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "keeping last valid settings"),
    }
    let mut service = Service {
        connection,
        settings,
        telemetry,
        tray,
        notifications: None,
        actions,
        monitor: Monitor::default(),
        origin: Instant::now(),
    };
    tracing::info!("razerr-rs service started");
    let stopped = run(
        &mut service,
        &mut worker,
        &mut incoming,
        &mut desktop_actions,
        &mut file_changes,
    )
    .await;
    drop(watcher);
    worker.task.abort();
    let (keep_window, failure) = match stopped {
        Ok(stopped) => (stopped.keep_window, stopped.failure),
        Err(error) => (false, Some(error.to_string())),
    };
    service.shutdown(keep_window).await;
    tracing::info!("razerr-rs service stopped");
    if let Some(error) = failure {
        anyhow::bail!(error);
    }
    Ok(())
}

/// How the service loop ended.
#[derive(Default)]
struct Stopped {
    keep_window: bool,
    failure: Option<String>,
}

impl Stopped {
    fn failed(failure: String) -> Self {
        Self {
            keep_window: false,
            failure: Some(failure),
        }
    }

    fn by_request(keep_window: bool) -> Self {
        Self {
            keep_window,
            failure: None,
        }
    }
}

async fn run(
    service: &mut Service,
    worker: &mut razerr_worker::Worker,
    incoming: &mut mpsc::Receiver<Request>,
    desktop_actions: &mut mpsc::Receiver<DesktopAction>,
    file_changes: &mut mpsc::UnboundedReceiver<()>,
) -> std::io::Result<Stopped> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    loop {
        let deadline = service.monitor.deadline();
        tokio::select! {
            _ = terminate.recv() => return Ok(Stopped::default()),
            _ = interrupt.recv() => return Ok(Stopped::default()),
            result = worker.snapshots.changed() => {
                if result.is_err() {
                    return Ok(Stopped::failed("device snapshot channel closed".to_owned()));
                }
                let snapshot = worker.snapshots.borrow_and_update().clone();
                service.snapshot_changed(snapshot).await;
            }
            result = worker.events.recv() => match result {
                Ok(razerr_worker::WorkerEvent::Error(message)) => {
                    service.worker_failed(&message).await;
                }
                Err(RecvError::Closed) => {
                    return Ok(Stopped::failed("device event channel closed".to_owned()));
                }
                Err(RecvError::Lagged(_)) => {}
            },
            Some(()) = file_changes.recv() => service.reload_settings(),
            () = wait_deadline(deadline, service.origin) => service.tick().await,
            Some(request) = incoming.recv() => {
                let result = service.request(request.action).await;
                let _ = request.reply.send(result);
                if matches!(request.action, Action::Stop | Action::Quit) {
                    return Ok(Stopped::by_request(matches!(request.action, Action::Stop)));
                }
            }
            Some(action) = desktop_actions.recv() => match action {
                DesktopAction::Open | DesktopAction::Settings => {
                    if let Err(error) = launch_gui(action == DesktopAction::Settings) {
                        tracing::warn!(%error, "opening window");
                    }
                }
                DesktopAction::Refresh => {
                    let _ = worker.commands.send(razerr_model::Command::Refresh.into()).await;
                }
                DesktopAction::Stop | DesktopAction::Quit => {
                    return Ok(Stopped::by_request(action == DesktopAction::Stop));
                }
            },
            result = &mut worker.task => {
                return Ok(Stopped::failed(format!(
                    "device worker stopped unexpectedly: {result:?}"
                )));
            }
        }
    }
}

async fn wait_deadline(deadline: Option<Duration>, origin: Instant) {
    match deadline {
        Some(deadline) => tokio::time::sleep(deadline.saturating_sub(origin.elapsed())).await,
        None => std::future::pending().await,
    }
}

async fn close_window(connection: &zbus::Connection) {
    let parameters: Vec<zbus::wire::Variant<'_>> = Vec::new();
    let platform: std::collections::HashMap<String, zbus::wire::Variant<'_>> =
        std::collections::HashMap::new();
    if let Ok(proxy) = zbus::Proxy::new(
        connection,
        "org.razerr_rs.App",
        "/org/razerr_rs/App",
        "org.freedesktop.Application",
    )
    .await
    {
        // A headless service shutdown must not activate an installed GUI.
        let _ = proxy
            .call_with_flags::<_, _, ()>(
                "ActivateAction",
                zbus::proxy::MethodFlags::NO_AUTO_START,
                &("quit", parameters, platform),
            )
            .await;
    }
}

fn notification(event: &Event) -> Notification {
    let body = event.battery.map_or_else(
        || event.name.clone(),
        |battery| format!("{} — {battery:.0}% remaining", event.name),
    );
    Notification {
        title: event.kind.title().into(),
        body,
        category: match event.kind {
            Kind::Connected => "device.added",
            Kind::Disconnected => "device.removed",
            _ => "device",
        }
        .into(),
        critical: event.kind == Kind::Critical,
    }
}

fn tray_lines(snapshot: &Snapshot) -> Vec<String> {
    if !snapshot.daemon_available {
        return vec!["OpenRazer is unavailable".into()];
    }
    if snapshot.devices.is_empty() {
        return vec!["No devices connected".into()];
    }
    snapshot
        .devices
        .iter()
        .map(|device| {
            let name = device.name.as_deref().unwrap_or(device.serial.as_str());
            device
                .battery
                .as_ref()
                .and_then(|battery| battery.level.displayed())
                .map_or_else(
                    || name.to_owned(),
                    |level| format!("{name} — {:.0}%", level.get()),
                )
        })
        .collect()
}
