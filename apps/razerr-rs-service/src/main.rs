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
use razerr_storage::{Paths, Settings, SettingsStore};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use control::{Action, Control};
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

#[allow(
    clippy::too_many_lines,
    reason = "One lifecycle/select loop; device, history and notification policy live in separate modules"
)]
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
    let (desktop_sender, mut desktop_actions) = mpsc::channel(32);
    let tray = match Tray::start(desktop_sender.clone()).await {
        Ok(tray) => Some(tray),
        Err(error) => {
            tracing::warn!(%error, "tray unavailable");
            None
        }
    };
    let mut notifications = None;
    let (watcher, mut file_changes) = settings_watch::start(&paths)?;
    // Catch edits between the initial load and watcher registration, including
    // time spent waiting for the desktop and D-Bus services during startup.
    match settings.reload() {
        Ok(true) => telemetry.configure(settings.current()),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "keeping last valid settings"),
    }
    let origin = Instant::now();
    let mut monitor = Monitor::default();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut keep_window = false;
    let mut failure = None;
    tracing::info!("razerr-rs service started");
    loop {
        let deadline = monitor.deadline();
        tokio::select! {
            _ = terminate.recv() => break,
            _ = interrupt.recv() => break,
            result = worker.snapshots.changed() => {
                if result.is_err() { failure = Some("device snapshot channel closed".to_owned()); break; }
                let snapshot = worker.snapshots.borrow_and_update().clone();
                if let Err(error) = razerr_ipc::publish(&connection).await { tracing::warn!(%error, "publishing device snapshot"); }
                if let Some(tray) = &tray && let Err(error) = tray.update(tray_lines(&snapshot)).await { tracing::warn!(%error, "updating tray"); }
                if snapshot.kind != SnapshotKind::Ack {
                    telemetry.observe(Arc::clone(&snapshot), origin.elapsed());
                    let events = monitor.update(&snapshot, origin.elapsed());
                    notify(events, &mut monitor, &mut notifications, &desktop_sender, settings.current()).await;
                }
            }
            result = worker.events.recv() => {
                match result {
                    Ok(razerr_worker::WorkerEvent::Error(message)) => {
                        tracing::warn!(%message, "device operation");
                        if let Err(error) = razerr_ipc::emit_error(&connection, &message).await { tracing::warn!(%error, "publishing error"); }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => { failure = Some("device event channel closed".to_owned()); break; },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
            Some(()) = file_changes.recv() => {
                match settings.reload() {
                    Ok(true) => { telemetry.configure(settings.current()); tracing::info!("settings reloaded"); }
                    Ok(false) => {},
                    Err(error) => tracing::warn!(%error, "keeping last valid settings"),
                }
            }
            () = wait_deadline(deadline, origin) => {
                let events = monitor.tick(origin.elapsed());
                notify(events, &mut monitor, &mut notifications, &desktop_sender, settings.current()).await;
            }
            Some(request) = incoming.recv() => {
                let result = match request.action {
                    Action::Open => launch_gui(false).map_err(|error| error.to_string()),
                    Action::Settings => launch_gui(true).map_err(|error| error.to_string()),
                    Action::ClearHistory => telemetry.clear().await,
                    Action::Stop | Action::Quit => Ok(()),
                };
                let _ = request.reply.send(result);
                if matches!(request.action, Action::Stop | Action::Quit) {
                    keep_window = matches!(request.action, Action::Stop);
                    break;
                }
            }
            Some(action) = desktop_actions.recv() => {
                match action {
                    DesktopAction::Open | DesktopAction::Settings => if let Err(error) = launch_gui(matches!(action, DesktopAction::Settings)) { tracing::warn!(%error, "opening window"); },
                    DesktopAction::Refresh => { let _ = worker.commands.send(razerr_model::Command::Refresh).await; },
                    DesktopAction::Stop | DesktopAction::Quit => { keep_window = matches!(action, DesktopAction::Stop); break; }
                }
            }
            result = &mut worker.task => { failure = Some(format!("device worker stopped unexpectedly: {result:?}")); break; }
        }
    }
    drop(watcher);
    worker.task.abort();
    telemetry.shutdown().await;
    if let Some(tray) = tray {
        tray.shutdown().await;
    }
    if let Some(notifications) = notifications {
        notifications.shutdown().await;
    }
    if !keep_window {
        close_window(&connection).await;
    }
    tracing::info!("razerr-rs service stopped");
    if let Some(error) = failure {
        anyhow::bail!(error);
    }
    Ok(())
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

async fn notify(
    events: Vec<Event>,
    monitor: &mut Monitor,
    notifications: &mut Option<Notifications>,
    actions: &mpsc::Sender<DesktopAction>,
    settings: &Settings,
) {
    for event in events {
        let enabled = if event.kind.is_battery() {
            settings.notifications.battery
        } else {
            settings.notifications.connections
        };
        if !enabled {
            monitor.acknowledge(&event);
            continue;
        }
        if notifications.is_none() {
            match Notifications::connect(actions.clone()).await {
                Ok(service) => *notifications = Some(service),
                Err(error) => {
                    tracing::warn!(%error, "notifications unavailable");
                    continue;
                }
            }
        }
        let body = event.battery.map_or_else(
            || event.name.clone(),
            |battery| format!("{} — {battery:.0}% remaining", event.name),
        );
        let notification = Notification {
            title: event.kind.title().into(),
            body,
            category: if event.kind == Kind::Connected {
                "device.added"
            } else if event.kind == Kind::Disconnected {
                "device.removed"
            } else {
                "device"
            }
            .into(),
            critical: event.kind == Kind::Critical,
        };
        if let Some(service) = notifications {
            match service.show(&notification).await {
                Ok(_) => monitor.acknowledge(&event),
                Err(error) => tracing::warn!(%error, "notification failed"),
            }
        }
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
