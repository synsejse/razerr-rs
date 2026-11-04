mod instance;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use razerr_storage::{Paths, Settings};
use std::{
    io::{self, Write},
    process::ExitCode,
    time::Duration,
};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "razerr-rs",
    version,
    about = "Desktop controls and battery history for OpenRazer"
)]
struct Cli {
    /// Open the preferences page.
    #[arg(long)]
    settings: bool,
    /// Accepted for old launchers. The interface is always dark now.
    #[arg(long = "dark", hide = true)]
    _dark: bool,
    /// Open with made-up devices, without the service, OpenRazer or any files.
    #[arg(long, conflicts_with = "settings")]
    demo: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Read real OpenRazer devices and print capabilities and state as JSON.
    Probe {
        /// Maximum time for the complete read-only probe, in seconds.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
        timeout: u64,
    },
    /// Close the application window.
    Quit,
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("razerr-rs: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RAZERR_RS_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .try_init()
        .map_err(|error| anyhow::anyhow!("could not initialize logging: {error}"))?;
    // iced runs on the main thread with its own executor. This separate runtime
    // keeps the D-Bus application interface responsive throughout the window's life.
    let runtime = tokio::runtime::Runtime::new()?;
    match cli.command {
        Some(Command::Probe { timeout }) => return runtime.block_on(probe(timeout)),
        Some(Command::Quit) => return runtime.block_on(instance::quit()),
        None => {}
    }
    if cli.demo {
        return razerr_ui::run_demo().context("could not run the desktop window");
    }
    let paths = Paths::discover()?;
    let (settings, initial_error) = match Settings::load(&paths) {
        Ok(settings) => (settings, None),
        Err(error) => (
            Settings::default(),
            Some(format!("Could not load preferences: {error}")),
        ),
    };
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let Some(connection) = runtime.block_on(instance::acquire(sender, cli.settings))? else {
        return Ok(());
    };
    let result = razerr_ui::run(
        razerr_ui::Options {
            paths,
            settings,
            show_settings: cli.settings,
            initial_error,
        },
        receiver,
    );
    drop(connection);
    result.context("could not run the desktop window")
}

async fn probe(timeout: u64) -> Result<()> {
    let report = tokio::time::timeout(Duration::from_secs(timeout), razerr_openrazer::probe())
        .await
        .context("OpenRazer probe timed out; check that the daemon is responding")?
        .context("could not query OpenRazer; it must be running on your desktop session bus")?;
    let json =
        serde_json::to_string_pretty(&report).context("could not encode the device report")?;
    writeln!(io::stdout().lock(), "{json}").context("could not write the device report")?;
    Ok(())
}
