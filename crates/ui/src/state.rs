use std::sync::Arc;

use razerr_model::{Device, Serial, Snapshot};
use razerr_storage::Settings;

use crate::editor::Editor;

/// A group of device drafts that are applied together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Lighting,
    Sensitivity,
    Power,
}

impl Section {
    pub const ALL: [Self; 3] = [Self::Sensitivity, Self::Lighting, Self::Power];
    pub fn title(self) -> &'static str {
        match self {
            Self::Lighting => "Lighting",
            Self::Sensitivity => "Sensitivity",
            Self::Power => "Power",
        }
    }
}

/// A page of the selected device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Sensitivity,
    Lighting,
    Power,
    Device,
}

impl Page {
    pub const ALL: [Self; 4] = [Self::Sensitivity, Self::Lighting, Self::Power, Self::Device];
    pub fn title(self) -> &'static str {
        match self {
            Self::Sensitivity => "Sensitivity",
            Self::Lighting => "Lighting",
            Self::Power => "Power",
            Self::Device => "Device",
        }
    }

    /// Whether `device` has anything to show on this page.
    pub fn available(self, device: &Device) -> bool {
        match self {
            Self::Sensitivity => {
                device.dpi.is_some() || device.capabilities.dpi_stages || device.polling.is_some()
            }
            Self::Lighting => !device.capabilities.zones.is_empty(),
            Self::Power => {
                device.battery.is_some()
                    || device.idle_time.is_some()
                    || device.low_battery.is_some()
            }
            Self::Device => true,
        }
    }

    /// The drafts this page edits.
    pub fn section(self) -> Option<Section> {
        match self {
            Self::Sensitivity => Some(Section::Sensitivity),
            Self::Lighting => Some(Section::Lighting),
            Self::Power => Some(Section::Power),
            Self::Device => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    Defaults,
    ClearHistory,
}

#[derive(Clone, Debug)]
pub struct Preferences {
    pub values: Settings,
    pub days: String,
    pub seconds: String,
}

impl From<Settings> for Preferences {
    fn from(values: Settings) -> Self {
        Self {
            days: values.history.days.to_string(),
            seconds: values.history.sample_seconds.to_string(),
            values,
        }
    }
}

impl Preferences {
    pub fn validated(&self) -> Result<Settings, String> {
        let mut values = self.values.clone();
        values.history.days = self
            .days
            .parse()
            .map_err(|_| "History days must be a whole number".to_owned())?;
        values.history.sample_seconds = self
            .seconds
            .parse()
            .map_err(|_| "Sample interval must be a whole number".to_owned())?;
        values.validate().map_err(|error| error.to_string())?;
        Ok(values)
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent connection, preferences and modal state flags"
)]
pub struct State {
    pub snapshot: Arc<Snapshot>,
    pub selected: Option<Serial>,
    pub editor: Editor,
    pub connected: bool,
    pub received: bool,
    pub preferences: bool,
    pub page: Page,
    pub settings: Settings,
    pub draft: Preferences,
    pub error: Option<String>,
    pub dismissed_error: Option<String>,
    pub confirmation: Option<Confirmation>,
    pub confirm_focused: bool,
}

impl State {
    pub fn new(settings: Settings, preferences: bool) -> Self {
        Self {
            snapshot: Arc::new(Snapshot::default()),
            selected: None,
            editor: Editor::default(),
            connected: false,
            received: false,
            preferences,
            page: Page::default(),
            draft: settings.clone().into(),
            settings,
            error: None,
            dismissed_error: None,
            confirmation: None,
            confirm_focused: false,
        }
    }

    pub fn selected(&self) -> Option<&Device> {
        self.snapshot
            .devices
            .iter()
            .find(|device| Some(&device.serial) == self.selected.as_ref())
    }

    /// The page to show: the chosen one, or the first the selected device has.
    pub fn current_page(&self) -> Page {
        self.selected().map_or(self.page, |device| {
            if self.page.available(device) {
                self.page
            } else {
                Page::ALL
                    .into_iter()
                    .find(|page| page.available(device))
                    .unwrap_or(Page::Device)
            }
        })
    }

    pub fn controls_enabled(&self) -> bool {
        self.connected
            && self.snapshot.reliable
            && self.snapshot.daemon_available
            && self.selected().is_some_and(|device| {
                device.availability != razerr_model::Availability::Unavailable
            })
    }

    pub fn receive(&mut self, snapshot: Arc<Snapshot>) {
        if self.received && snapshot.revision < self.snapshot.revision {
            return;
        }
        let previous = self.selected.clone();
        let previous_usb = self.selected().and_then(|device| device.usb);
        let previous_error = self.snapshot.error.clone();
        // A new service may first publish an empty, not-yet-discovered snapshot.
        // Keep the user's identifier until an actual enumeration can replace it.
        if snapshot.reliable || !snapshot.devices.is_empty() {
            self.selected = retain_selection(
                self.selected.as_ref(),
                snapshot.devices.iter().map(|device| &device.serial),
            );
        }
        self.snapshot = snapshot;
        self.received = true;
        self.connected = true;
        if self.selected != previous
            || self.selected().and_then(|device| device.usb) != previous_usb
        {
            // A serial survives wired/wireless handoff, but its capabilities and
            // readings do not. Discard old drafts even if a new getter failed.
            self.editor = Editor::default();
        }
        if let Some(device) = self.selected().cloned() {
            self.editor.sync(&device);
        }
        if self.snapshot.error.is_none() {
            self.dismissed_error = None;
            if self.error == previous_error {
                self.error = None;
            }
        } else if self.snapshot.error != self.dismissed_error {
            self.error.clone_from(&self.snapshot.error);
        }
    }

    pub fn select(&mut self, serial: Serial) {
        self.preferences = false;
        if self.selected.as_ref() == Some(&serial) {
            return;
        }
        self.selected = Some(serial);
        self.editor = Editor::default();
        if let Some(device) = self.selected().cloned() {
            self.editor.sync(&device);
        }
    }

    /// Discard unapplied edits in `section`, or everywhere; the pad keeps its
    /// target if it still exists.
    pub fn revert(&mut self, section: Option<Section>) {
        let target = self.editor.target;
        match section {
            Some(section) => self.editor.discard(section),
            None => Section::ALL
                .into_iter()
                .for_each(|section| self.editor.discard(section)),
        }
        if let Some(device) = self.selected().cloned() {
            self.editor.sync(&device);
        }
        self.editor.select_target(target);
    }

    /// Hides the error, until the service reports a different one.
    pub fn dismiss_error(&mut self) {
        self.dismissed_error.clone_from(&self.snapshot.error);
        self.error = None;
    }

    pub fn open_confirmation(&mut self, kind: Confirmation) {
        self.confirmation = Some(kind);
        self.confirm_focused = false;
    }

    pub fn cancel(&mut self) {
        self.confirmation = None;
        self.confirm_focused = false;
    }

    pub fn finish_save(&mut self, settings: Settings) {
        if self.draft.validated().as_ref() == Ok(&settings) {
            self.draft = settings.clone().into();
        }
        self.settings = settings;
    }
}

fn retain_selection<'a>(
    selected: Option<&Serial>,
    serials: impl Iterator<Item = &'a Serial>,
) -> Option<Serial> {
    let serials: Vec<_> = serials.collect();
    selected
        .filter(|selected| serials.contains(selected))
        .cloned()
        .or_else(|| serials.first().copied().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wired_snapshot() -> Snapshot {
        serde_json::from_str(include_str!("../tests/captures/wired.json"))
            .expect("actual wired snapshot")
    }

    #[test]
    fn real_transport_handoff_discards_old_drafts_when_new_stages_are_unavailable() {
        let wired: Snapshot = serde_json::from_str(include_str!("../tests/captures/wired.json"))
            .expect("actual wired snapshot");
        let wireless: Snapshot =
            serde_json::from_str(include_str!("../tests/captures/wireless_handoff.json"))
                .expect("actual wireless handoff snapshot");
        wired.validate().expect("valid wired capture");
        wireless.validate().expect("valid wireless capture");
        let mut state = State::new(Settings::default(), false);
        state.receive(Arc::new(wired));
        let serial = state.selected.clone();
        assert_eq!(state.editor.stages.len(), 5);
        state.editor.x = "2000".into();
        state.editor.dpi_dirty = true;
        state.editor.stages_dirty = true;
        state.receive(Arc::new(wireless));
        assert_eq!(state.selected, serial);
        assert_eq!(state.editor.x, "1800");
        assert!(!state.editor.dpi_dirty && !state.editor.stages_dirty);
        assert_eq!(state.editor.stages, []);
    }

    #[test]
    fn preferences_require_valid_ranges_and_keep_draft_separate() {
        let state = State::new(Settings::default(), false);
        let mut draft = state.draft.clone();
        draft.days = "6".into();
        assert!(draft.validated().is_err());
        draft.days = "365".into();
        draft.seconds = "20".into();
        assert_eq!(draft.validated().expect("valid").history.days, 365);
        assert_eq!(state.settings.history.days, 90);
    }

    #[test]
    fn confirmations_start_on_cancel_and_escape_does_not_change_settings() {
        let mut state = State::new(Settings::default(), false);
        state.open_confirmation(Confirmation::Defaults);
        assert!(!state.confirm_focused);
        state.cancel();
        assert!(state.confirmation.is_none());
        assert_eq!(state.settings, Settings::default());
    }

    #[test]
    fn clearing_a_reported_error_allows_the_same_later_failure_to_be_shown() {
        let mut state = State::new(Settings::default(), false);
        let failed = Arc::new(Snapshot {
            error: Some("Service unavailable".into()),
            ..Snapshot::default()
        });
        state.receive(failed.clone());
        state.dismissed_error.clone_from(&state.snapshot.error);
        state.error = None;
        state.receive(failed.clone());
        assert!(state.error.is_none());
        state.receive(Arc::new(Snapshot::default()));
        state.receive(failed);
        assert_eq!(state.error.as_deref(), Some("Service unavailable"));
    }

    #[test]
    fn selection_is_retained_by_identifier_instead_of_list_position() {
        let first = Serial::new("first").expect("identifier");
        let second = Serial::new("second").expect("identifier");
        assert_eq!(
            retain_selection(Some(&first), [&second, &first].into_iter()),
            Some(first.clone())
        );
        assert_eq!(
            retain_selection(Some(&first), [&second].into_iter()),
            Some(second)
        );
        assert_eq!(retain_selection(Some(&first), [].into_iter()), None);
    }

    #[test]
    fn a_page_the_device_lacks_falls_back_to_one_it_has() {
        let mut state = State::new(Settings::default(), false);
        state.receive(Arc::new(wired_snapshot()));
        state.page = Page::Lighting;
        assert_eq!(state.current_page(), Page::Sensitivity, "no lighting zones");
        state.page = Page::Power;
        assert_eq!(state.current_page(), Page::Power);
    }

    #[test]
    fn selecting_the_selected_device_keeps_its_drafts() {
        let mut state = State::new(Settings::default(), false);
        state.receive(Arc::new(wired_snapshot()));
        state.editor.x = "2000".into();
        state.editor.dpi_dirty = true;
        state.preferences = true;
        let serial = state.selected.clone().expect("selected");
        state.select(serial);
        assert_eq!(state.editor.x, "2000");
        assert!(state.editor.dpi_dirty);
        assert!(!state.preferences);
    }

    #[test]
    fn save_result_keeps_edits_made_while_the_write_was_running() {
        let mut state = State::new(Settings::default(), true);
        state.draft.days = "30".into();
        let written = state.draft.validated().expect("valid settings");
        state.draft.days = "60".into();
        state.finish_save(written);
        assert_eq!(state.settings.history.days, 30);
        assert_eq!(state.draft.days, "60");
    }

    #[test]
    fn older_revisions_are_ignored_and_selection_survives_pending_discovery() {
        let mut state = State::new(Settings::default(), false);
        state.selected = Some(Serial::new("selected").expect("identifier"));
        state.receive(Arc::new(Snapshot {
            revision: 10,
            ..Snapshot::default()
        }));
        state.receive(Arc::new(Snapshot {
            revision: 9,
            error: Some("Delayed failure".into()),
            ..Snapshot::default()
        }));
        assert_eq!(state.snapshot.revision, 10);
        assert!(state.error.is_none());
        assert!(state.selected.is_some());
        state.received = false;
        state.receive(Arc::new(Snapshot::default()));
        assert_eq!(state.snapshot.revision, 0);
        assert!(state.selected.is_some());
        state.receive(Arc::new(Snapshot {
            revision: 1,
            reliable: true,
            ..Snapshot::default()
        }));
        assert!(state.selected.is_none());
    }
}
