//! Watch replacement/edit events, excluding the access events caused by reload.

use std::{path::Path, time::Duration};

use notify_debouncer_mini::{
    Config, DebounceEventResult, Debouncer, new_debouncer_opt,
    notify::{self, EventHandler, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind},
};
use razerr_storage::Paths;
use tokio::sync::mpsc;

pub struct WriteWatcher(RecommendedWatcher);

impl Watcher for WriteWatcher {
    fn new<F: EventHandler>(mut handler: F, config: notify::Config) -> notify::Result<Self> {
        RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                if event.as_ref().is_ok_and(|event| event.kind.is_access()) {
                    return;
                }
                handler.handle_event(event);
            },
            config,
        )
        .map(Self)
    }

    fn watch(&mut self, path: &Path, recursive: RecursiveMode) -> notify::Result<()> {
        self.0.watch(path, recursive)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.0.unwatch(path)
    }

    fn configure(&mut self, config: notify::Config) -> notify::Result<bool> {
        self.0.configure(config)
    }

    fn kind() -> WatcherKind {
        RecommendedWatcher::kind()
    }
}

pub fn start(
    paths: &Paths,
) -> anyhow::Result<(Debouncer<WriteWatcher>, mpsc::UnboundedReceiver<()>)> {
    paths.create_config_dir()?;
    let settings = paths.settings_file();
    let (changed, receiver) = mpsc::unbounded_channel();
    let mut watcher = new_debouncer_opt::<_, WriteWatcher>(
        Config::default().with_timeout(Duration::from_millis(100)),
        move |result: DebounceEventResult| match result {
            Ok(events) if events.iter().any(|event| event.path == settings) => {
                let _ = changed.send(());
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "settings watch error"),
        },
    )?;
    watcher
        .watcher()
        .watch(paths.config_dir(), RecursiveMode::NonRecursive)?;
    Ok((watcher, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn atomic_replacements_reload_but_reads_and_unrelated_files_do_not() -> anyhow::Result<()>
    {
        let directory = tempfile::tempdir()?;
        let paths = Paths::from_roots(directory.path(), directory.path(), directory.path())?;
        let (_watcher, mut changes) = start(&paths)?;
        razerr_storage::Settings::default().save(&paths)?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), changes.recv()).await?,
            Some(())
        );
        let _ = razerr_storage::Settings::load(&paths)?;
        std::fs::write(paths.config_dir().join("unrelated.txt"), "unrelated edit")?;
        assert!(
            tokio::time::timeout(Duration::from_millis(350), changes.recv())
                .await
                .is_err()
        );
        std::fs::remove_file(paths.settings_file())?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), changes.recv()).await?,
            Some(())
        );
        Ok(())
    }
}
