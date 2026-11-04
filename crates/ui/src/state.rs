use std::{collections::HashMap, sync::Arc};

use razerr_model::{
    Device, Direction, Dpi, DpiStages, Effect, EffectKind, Rgb, Serial, Snapshot, Speed, ZoneId,
};
use razerr_storage::Settings;

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

#[derive(Clone, Debug)]
pub struct ZoneEditor {
    pub kind: Option<EffectKind>,
    pub first: String,
    pub second: String,
    pub speed: u8,
    pub direction: Direction,
    pub brightness: f64,
    pub brightness_known: bool,
    pub effect_dirty: bool,
    pub brightness_dirty: bool,
}

impl ZoneEditor {
    pub fn from_zone(zone: &razerr_model::Zone) -> Self {
        let mut editor = Self {
            kind: None,
            first: "#FFFFFF".into(),
            second: "#0080FF".into(),
            speed: 1,
            direction: Direction::Left,
            brightness: zone
                .brightness
                .as_ref()
                .and_then(|reading| reading.displayed())
                .map_or(0.0, |value| value.get()),
            brightness_known: zone
                .brightness
                .as_ref()
                .and_then(|reading| reading.displayed())
                .is_some(),
            effect_dirty: false,
            brightness_dirty: false,
        };
        if let Some(effect) = zone.effect.displayed() {
            editor.kind = Some(effect.kind());
            match effect {
                Effect::Static { color } | Effect::Breath { color } | Effect::Ripple { color } => {
                    editor.first = color_hex(*color);
                }
                Effect::BreathDual { colors } => {
                    editor.first = color_hex(colors[0]);
                    editor.second = color_hex(colors[1]);
                }
                Effect::Reactive { color, speed } | Effect::Starlight { color, speed } => {
                    editor.first = color_hex(*color);
                    editor.speed = speed.get();
                }
                Effect::StarlightDual { colors, speed } => {
                    editor.first = color_hex(colors[0]);
                    editor.second = color_hex(colors[1]);
                    editor.speed = speed.get();
                }
                Effect::StarlightRandom { speed } => editor.speed = speed.get(),
                Effect::Wave { direction } => editor.direction = *direction,
                _ => {}
            }
        }
        editor
    }

    pub fn effect(&self) -> Result<Effect, String> {
        let kind = self.kind.ok_or("Select an effect first")?;
        let first = || parse_color(&self.first);
        let second = || parse_color(&self.second);
        let speed = || Speed::new(self.speed).map_err(|error| error.to_string());
        Ok(match kind {
            EffectKind::Off => Effect::Off,
            EffectKind::Static => Effect::Static { color: first()? },
            EffectKind::Spectrum => Effect::Spectrum,
            EffectKind::Wave => Effect::Wave {
                direction: self.direction,
            },
            EffectKind::Breath => Effect::Breath { color: first()? },
            EffectKind::BreathDual => Effect::BreathDual {
                colors: [first()?, second()?],
            },
            EffectKind::BreathRandom => Effect::BreathRandom,
            EffectKind::Reactive => Effect::Reactive {
                color: first()?,
                speed: speed()?,
            },
            EffectKind::Starlight => Effect::Starlight {
                color: first()?,
                speed: speed()?,
            },
            EffectKind::StarlightDual => Effect::StarlightDual {
                colors: [first()?, second()?],
                speed: speed()?,
            },
            EffectKind::StarlightRandom => Effect::StarlightRandom { speed: speed()? },
            EffectKind::Ripple => Effect::Ripple { color: first()? },
            EffectKind::RippleRandom => Effect::RippleRandom,
        })
    }
}

pub fn color_hex(color: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", color.red, color.green, color.blue)
}

pub fn parse_color(value: &str) -> Result<Rgb, String> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if value.len() != 6 || !value.is_ascii() {
        return Err("Colors use six hexadecimal digits, for example #72ACFF".into());
    }
    let parse = |range| {
        u8::from_str_radix(&value[range], 16)
            .map_err(|_| "Color contains a non-hexadecimal digit".to_owned())
    };
    Ok(Rgb {
        red: parse(0..2)?,
        green: parse(2..4)?,
        blue: parse(4..6)?,
    })
}

/// Which DPI vector the pad, the X/Y sliders and their inputs edit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Current,
    /// Zero-based stage index.
    Stage(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent input linking and dirty flags preserve separate drafts during asynchronous snapshots"
)]
#[derive(Clone, Debug, Default)]
pub struct Editor {
    pub x: String,
    pub y: String,
    /// Edits of the target move both axes together.
    pub linked: bool,
    pub dpi_dirty: bool,
    pub stages: Vec<(String, String)>,
    pub active: u8,
    pub stages_dirty: bool,
    pub target: Target,
    pub idle: String,
    pub idle_dirty: bool,
    pub threshold: String,
    pub threshold_dirty: bool,
    pub zones: HashMap<ZoneId, ZoneEditor>,
}

impl Editor {
    /// The X/Y drafts of the target; `None` if its stage was removed.
    pub fn target_values(&self) -> Option<(&str, &str)> {
        match self.target {
            Target::Current => Some((&self.x, &self.y)),
            Target::Stage(index) => self
                .stages
                .get(index)
                .map(|(x, y)| (x.as_str(), y.as_str())),
        }
    }

    fn target_mut(&mut self) -> Result<(&mut String, &mut String), String> {
        match self.target {
            Target::Current => {
                self.dpi_dirty = true;
                Ok((&mut self.x, &mut self.y))
            }
            Target::Stage(index) => {
                self.stages_dirty = true;
                self.stages
                    .get_mut(index)
                    .map(|(x, y)| (x, y))
                    .ok_or_else(|| "Stage was removed".to_owned())
            }
        }
    }

    /// Edit one axis of the target; linked axes follow.
    pub fn set_axis(&mut self, axis: Axis, value: String) -> Result<(), String> {
        let linked = self.linked;
        let (x, y) = self.target_mut()?;
        if linked {
            x.clone_from(&value);
            *y = value;
        } else if axis == Axis::X {
            *x = value;
        } else {
            *y = value;
        }
        Ok(())
    }

    /// Set the whole target vector, as the pad does.
    pub fn set_vector(&mut self, x: u16, y: u16) -> Result<(), String> {
        let linked = self.linked;
        let (draft_x, draft_y) = self.target_mut()?;
        *draft_x = x.to_string();
        *draft_y = if linked { x } else { y }.to_string();
        Ok(())
    }

    pub fn set_linked(&mut self, linked: bool) -> Result<(), String> {
        self.linked = linked;
        if linked && self.target_values().is_some_and(|(x, y)| x != y) {
            let (x, y) = self.target_mut()?;
            y.clone_from(x);
        }
        Ok(())
    }

    pub fn select_target(&mut self, target: Target) {
        if let Target::Stage(index) = target
            && index >= self.stages.len()
        {
            return;
        }
        self.target = target;
        self.relink();
    }

    /// After new readings: drop a removed stage target and relink a clean one.
    fn settle_target(&mut self) {
        if let Target::Stage(index) = self.target
            && index >= self.stages.len()
        {
            self.target = Target::Current;
        }
        let clean = match self.target {
            Target::Current => !self.dpi_dirty,
            Target::Stage(_) => !self.stages_dirty,
        };
        if clean {
            self.relink();
        }
    }

    /// Linking follows the target: equal axes start linked, unequal ones do not.
    fn relink(&mut self) {
        if let Some((x, y)) = self.target_values() {
            self.linked = x == y;
        }
    }

    pub fn remove_stage(&mut self, index: usize) {
        if self.stages.len() > 1 && index < self.stages.len() {
            self.stages.remove(index);
            if index + 1 < usize::from(self.active) {
                self.active -= 1;
            }
            self.active = self
                .active
                .min(u8::try_from(self.stages.len()).unwrap_or(5));
            self.target = match self.target {
                Target::Stage(edited) if edited == index => Target::Current,
                Target::Stage(edited) if edited > index => Target::Stage(edited - 1),
                other => other,
            };
            self.stages_dirty = true;
        }
    }

    /// Apply new readings: drafts whose value the device now reports are clean
    /// again, and clean drafts follow the device.
    pub fn sync(&mut self, device: &Device) {
        self.sync_sensitivity(device);
        self.sync_power(device);
        self.sync_zones(device);
    }

    fn sync_sensitivity(&mut self, device: &Device) {
        if device
            .dpi
            .as_ref()
            .and_then(|control| control.current.fresh())
            .is_some_and(|actual| {
                self.dpi(device.capabilities.available_dpi)
                    .is_ok_and(|draft| draft.same_setting(*actual))
            })
        {
            self.dpi_dirty = false;
        }
        if device
            .stages
            .as_ref()
            .and_then(|reading| reading.fresh())
            .is_some_and(|actual| self.stages().as_ref() == Ok(actual))
        {
            self.stages_dirty = false;
        }
        if !self.dpi_dirty
            && let Some(dpi) = device.dpi.as_ref().and_then(|dpi| dpi.current.displayed())
        {
            self.x = dpi.x().to_string();
            self.y = dpi.y().to_string();
        }
        if !self.stages_dirty
            && let Some(stages) = device
                .stages
                .as_ref()
                .and_then(|reading| reading.displayed())
        {
            self.active = stages.active();
            self.stages = stages
                .values()
                .iter()
                .map(|dpi| (dpi.x().to_string(), dpi.y().to_string()))
                .collect();
        }
        self.settle_target();
    }

    fn sync_power(&mut self, device: &Device) {
        let fresh = |reading: Option<String>| reading.unwrap_or_default();
        let actual_idle = fresh(
            device
                .idle_time
                .as_ref()
                .and_then(|reading| reading.fresh())
                .map(|value| value.get().to_string()),
        );
        let actual_threshold = fresh(
            device
                .low_battery
                .as_ref()
                .and_then(|reading| reading.fresh())
                .map(|value| value.get().to_string()),
        );
        if self.idle == actual_idle {
            self.idle_dirty = false;
        }
        if self.threshold == actual_threshold {
            self.threshold_dirty = false;
        }
        if !self.idle_dirty {
            self.idle = device
                .idle_time
                .as_ref()
                .and_then(|reading| reading.displayed())
                .map_or_else(String::new, |value| value.get().to_string());
        }
        if !self.threshold_dirty {
            self.threshold = device
                .low_battery
                .as_ref()
                .and_then(|reading| reading.displayed())
                .map_or_else(String::new, |value| value.get().to_string());
        }
    }

    fn sync_zones(&mut self, device: &Device) {
        self.zones
            .retain(|id, _| device.zones.iter().any(|zone| zone.id == *id));
        for zone in &device.zones {
            let mut reported = ZoneEditor::from_zone(zone);
            if reported.kind.is_some_and(|kind| {
                !device
                    .capabilities
                    .zones
                    .iter()
                    .any(|cap| cap.id == zone.id && cap.effects.contains(&kind))
            }) {
                reported.kind = None;
            }
            let Some(editor) = self.zones.get_mut(&zone.id) else {
                self.zones.insert(zone.id, reported);
                continue;
            };
            if zone
                .effect
                .fresh()
                .is_some_and(|actual| editor.effect().as_ref() == Ok(actual))
            {
                editor.effect_dirty = false;
            }
            if zone
                .brightness
                .as_ref()
                .and_then(|reading| reading.fresh())
                .is_some_and(|actual| (actual.get() - editor.brightness).abs() < 0.5)
            {
                editor.brightness_dirty = false;
            }
            if !editor.effect_dirty {
                editor.kind = reported.kind;
                editor.first = reported.first;
                editor.second = reported.second;
                editor.speed = reported.speed;
                editor.direction = reported.direction;
            }
            if !editor.brightness_dirty {
                editor.brightness = reported.brightness;
                editor.brightness_known = reported.brightness_known;
            }
        }
    }

    /// The sleep timeout, or `None` if the device hasn't reported one.
    pub fn idle_seconds(&self) -> Option<u16> {
        self.idle.parse().ok()
    }

    /// The low-battery threshold, or `None` if the device hasn't reported one.
    pub fn threshold_percent(&self) -> Option<u8> {
        self.threshold.parse().ok()
    }

    /// Whether a section has edits that have not been applied yet.
    pub fn dirty(&self, section: Section) -> bool {
        match section {
            Section::Sensitivity => self.dpi_dirty || self.stages_dirty,
            Section::Lighting => self
                .zones
                .values()
                .any(|zone| zone.effect_dirty || zone.brightness_dirty),
            Section::Power => self.idle_dirty || self.threshold_dirty,
        }
    }

    /// Mark a section clean so the next sync restores the device's values.
    pub fn discard(&mut self, section: Section) {
        match section {
            Section::Sensitivity => {
                self.dpi_dirty = false;
                self.stages_dirty = false;
            }
            Section::Lighting => {
                for zone in self.zones.values_mut() {
                    zone.effect_dirty = false;
                    zone.brightness_dirty = false;
                }
            }
            Section::Power => {
                self.idle_dirty = false;
                self.threshold_dirty = false;
            }
        }
    }

    pub fn dpi(&self, discrete: bool) -> Result<Dpi, String> {
        let x = self
            .x
            .parse()
            .map_err(|_| "X DPI must be a whole number".to_owned())?;
        // Linked edits already mirror X into the Y draft.
        let y = if discrete {
            0
        } else {
            self.y
                .parse()
                .map_err(|_| "Y DPI must be a whole number".to_owned())?
        };
        Dpi::new(x, y).map_err(|error| error.to_string())
    }
    pub fn stages(&self) -> Result<DpiStages, String> {
        let values = self
            .stages
            .iter()
            .map(|(x, y)| {
                Dpi::new(
                    x.parse()
                        .map_err(|_| "Stage DPI must be a whole number".to_owned())?,
                    y.parse()
                        .map_err(|_| "Stage DPI must be a whole number".to_owned())?,
                )
                .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        DpiStages::new(self.active, values).map_err(|error| error.to_string())
    }
}

#[allow(
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

    fn wired_device() -> Device {
        wired_snapshot().devices.remove(0)
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
    fn colors_are_validated_before_constructing_effects() {
        assert_eq!(
            parse_color("#72ACFF").expect("RGB"),
            Rgb {
                red: 114,
                green: 172,
                blue: 255
            }
        );
        for value in ["", "#ff", "#00000000", "é12345", "#GG0000"] {
            assert!(parse_color(value).is_err());
        }
    }
    #[test]
    fn numeric_stage_drafts_preserve_two_axes_and_active_bounds() {
        let mut editor = Editor {
            stages: vec![("800".into(), "900".into())],
            active: 1,
            ..Editor::default()
        };
        assert_eq!(editor.stages().expect("stages").values()[0].y(), 900);
        editor.active = 2;
        assert!(editor.stages().is_err());
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
    fn discrete_dpi_always_sends_zero_for_the_unused_axis() {
        let editor = Editor {
            x: "800".into(),
            y: "invalid".into(),
            linked: true,
            ..Editor::default()
        };
        assert_eq!(editor.dpi(true).expect("discrete DPI").y(), 0);
        assert!(
            editor.dpi(false).is_err(),
            "two-axis DPI needs a valid Y draft"
        );
    }

    #[test]
    fn target_edits_follow_linking_and_removal_keeps_the_selected_value() {
        let mut editor = Editor {
            x: "1600".into(),
            y: "1600".into(),
            stages: vec![
                ("400".into(), "400".into()),
                ("800".into(), "900".into()),
                ("1600".into(), "1600".into()),
            ],
            active: 3,
            ..Editor::default()
        };
        editor.select_target(Target::Stage(0));
        assert!(editor.linked, "equal axes start linked");
        editor.set_vector(500, 700).expect("stage");
        assert_eq!(editor.stages[0], ("500".into(), "500".into()));
        assert!(editor.stages_dirty && !editor.dpi_dirty);

        editor.select_target(Target::Stage(1));
        assert!(!editor.linked, "unequal axes start independent");
        editor.set_axis(Axis::Y, "1000".into()).expect("stage");
        assert_eq!(editor.stages[1], ("800".into(), "1000".into()));
        editor.set_linked(true).expect("stage");
        assert_eq!(editor.stages[1], ("800".into(), "800".into()));

        editor.select_target(Target::Stage(9));
        assert_eq!(
            editor.target,
            Target::Stage(1),
            "missing stages are ignored"
        );

        editor.remove_stage(0);
        assert_eq!(editor.active, 2);
        assert_eq!(editor.target, Target::Stage(0), "target follows its stage");
        assert_eq!(editor.stages().expect("stages").values()[1].x(), 1600);
        editor.remove_stage(0);
        assert_eq!(
            editor.target,
            Target::Current,
            "removing the target resets it"
        );
        editor.remove_stage(0);
        assert_eq!(editor.stages.len(), 1);
        assert_eq!(editor.active, 1);

        editor.select_target(Target::Current);
        editor.set_axis(Axis::X, "2000".into()).expect("current");
        assert_eq!((editor.x.as_str(), editor.y.as_str()), ("2000", "2000"));
        assert!(editor.dpi_dirty);
    }

    #[test]
    fn settings_that_could_not_be_read_stay_unknown() {
        let mut device = wired_device();
        let mut editor = Editor::default();
        editor.sync(&device);
        assert_eq!(editor.idle_seconds(), Some(300));
        assert_eq!(editor.threshold_percent(), Some(5));
        device.idle_time = Some(razerr_model::Reading::Unavailable);
        device.low_battery = Some(razerr_model::Reading::Unavailable);
        let mut editor = Editor::default();
        editor.sync(&device);
        assert_eq!(editor.idle_seconds(), None);
        assert_eq!(editor.threshold_percent(), None);
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
    fn relinking_equal_axes_is_not_an_edit() {
        let mut editor = Editor {
            x: "800".into(),
            y: "800".into(),
            stages: vec![("400".into(), "400".into())],
            active: 1,
            ..Editor::default()
        };
        editor.set_linked(false).expect("current");
        editor.set_linked(true).expect("current");
        assert!(!editor.dirty(Section::Sensitivity));
        editor.select_target(Target::Stage(0));
        editor.set_linked(false).expect("stage");
        editor.set_linked(true).expect("stage");
        assert!(!editor.dirty(Section::Sensitivity));
        // Unequal axes do change: Y follows X.
        editor.select_target(Target::Current);
        editor.linked = false;
        editor.y = "900".into();
        editor.set_linked(true).expect("current");
        assert_eq!(editor.y, "800");
        assert!(editor.dpi_dirty);
    }

    #[test]
    fn single_axis_dpi_is_clean_once_the_device_reports_it() {
        let mut device = wired_device();
        device.capabilities.available_dpi = true;
        device.dpi = Some(razerr_model::DpiControl {
            current: razerr_model::Reading::Fresh(Dpi::new(900, 900).expect("DPI")),
            range: razerr_model::DpiRange::Discrete {
                values: razerr_model::Reading::Fresh(vec![450, 900]),
            },
        });
        let mut editor = Editor {
            x: "900".into(),
            y: "900".into(),
            dpi_dirty: true,
            ..Editor::default()
        };
        editor.sync(&device);
        assert!(!editor.dpi_dirty);
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
