//! Settings persistence and the View/Options dialog (ADR 0029,
//! ADR 0032).
//!
//! [`load_settings`] and [`save_settings`] are the thin seam over the
//! app's [`SettingsStore`]; the
//! store is the only thing that touches `eframe::Storage` (ADR 0032).
//! A corrupt or unreadable store falls back to [`Settings::default`]
//! with one `warning:` line (ADR 0021: bad data substitutes a default
//! and warns, never crashes); the apps record the same warning on
//! [`AppCtx::warnings`](larrygram_commands::AppCtx) so the status bar
//! shows it.
//!
//! [`show_options`] draws the View/Options dialog body: the Panel
//! layout radio choice over all four layouts (ADR 0032, task P7-09),
//! the grid and rulers toggles, the display units and "Show the
//! welcome wizard on next start" (ADR 0066). The dialog never mutates
//! [`Settings`] (the ADR 0032 amendment): it returns the [`Intent`]s
//! for the rows the user changed and the shell dispatches them
//! through the command registry (`view.ui_layout.*`,
//! `view.toggle_grid`, `view.rulers`, `view.units`); the welcome row
//! has no command id and comes back as an [`OptionsChange`] flag the
//! shell writes into `Settings::welcome_seen`.

use egui::Ui;
use larrygram_commands::{
    ArgValue, Args, DisplayUnits, Invocation, Settings, SettingsError, SettingsStore, Source,
};
use larrygram_ui_spec::LayoutKind;

use crate::{Intent, UiError};

/// The settings JSON slot of the eframe storage (ADR 0032).
///
/// `eframe::Storage` is not `Send`, while
/// [`SettingsStore`] is; the apps
/// therefore read the stored slot into this plain-data store, load and
/// save through the [`SettingsStore`] seam, and write
/// the serialized text back to the slot. `crates/ui` never touches
/// eframe itself (ADR 0002).
pub struct TextStore {
    /// The stored settings text, when the slot has one.
    stored: Option<String>,
    /// The serialized settings of the last [`TextStore::save`].
    written: Option<String>,
    /// The one-line warning of a corrupt slot (ADR 0021).
    warning: Option<String>,
}

impl TextStore {
    /// A store over the text of one storage slot (`None` when empty).
    pub fn new(stored: Option<String>) -> Self {
        Self {
            stored,
            written: None,
            warning: None,
        }
    }

    /// The serialized settings to write back to the slot.
    pub fn take_written(&mut self) -> Option<String> {
        self.written.take()
    }

    /// The warning of a corrupt slot, once.
    pub fn take_warning(&mut self) -> Option<String> {
        self.warning.take()
    }
}

impl SettingsStore for TextStore {
    fn load(&mut self) -> Result<Settings, SettingsError> {
        let Some(text) = self.stored.clone() else {
            return Ok(Settings::default());
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => Ok(settings),
            Err(error) => {
                self.warning = Some(format!("settings: {error}"));
                Err(SettingsError::Format {
                    message: error.to_string(),
                })
            }
        }
    }

    fn save(&mut self, settings: &Settings) -> Result<(), SettingsError> {
        let text = serde_json::to_string(settings).map_err(|error| SettingsError::Format {
            message: error.to_string(),
        })?;
        self.written = Some(text);
        Ok(())
    }
}

/// The display units offered by the options dialog, in menu order.
///
/// `DisplayUnits` is `#[non_exhaustive]`, so the dialog lists its
/// choices explicitly instead of matching over the enum.
const UNIT_CHOICES: &[(DisplayUnits, &str)] = &[
    (DisplayUnits::Inches, "Inches"),
    (DisplayUnits::Millimeters, "Millimeters"),
    (DisplayUnits::Centimeters, "Centimeters"),
    (DisplayUnits::Meters, "Meters"),
    (DisplayUnits::Feet, "Feet"),
];

/// Loads settings through a [`SettingsStore`], falling back to
/// [`Settings::default`] on any [`SettingsError`] (ADR 0032: a corrupt
/// or missing store warns, never crashes).
///
/// The fallback prints exactly one `warning: <error>` line so the
/// failure is never silent; the caller that owns an
/// [`AppCtx`](larrygram_commands::AppCtx) records the same text for
/// the status bar.
pub fn load_settings(store: &mut dyn SettingsStore) -> Result<Settings, UiError> {
    match store.load() {
        Ok(settings) => Ok(settings),
        Err(error) => {
            eprintln!("warning: {error}");
            Ok(Settings::default())
        }
    }
}

/// Saves settings through a [`SettingsStore`].
pub fn save_settings(store: &mut dyn SettingsStore, settings: &Settings) -> Result<(), UiError> {
    store.save(settings).map_err(settings_error)
}

/// What one Options dialog frame changed (ADR 0032 as amended).
///
/// The command rows come back as [`Intent`]s the shell dispatches;
/// the "Show the welcome wizard on next start" row has no command id,
/// so it comes back as a flag the shell writes into
/// [`Settings::welcome_seen`](larrygram_commands::Settings) (task
/// P7-09).
#[derive(Debug, Clone, PartialEq)]
pub struct OptionsChange {
    /// One dispatch request per command-backed row the user changed.
    pub intents: Vec<Intent>,
    /// `Some(on)` when the welcome-wizard row changed this frame.
    pub show_wizard_next_start: Option<bool>,
}

impl OptionsChange {
    /// A dialog frame that changed nothing.
    pub fn none() -> Self {
        Self {
            intents: Vec::new(),
            show_wizard_next_start: None,
        }
    }
}

/// Draws the View/Options dialog body (ADR 0032 as amended): the
/// Panel layout radio choice over all four layouts, grid and rulers
/// toggles, display units and the welcome-wizard row (ADR 0066).
///
/// The dialog draws its rows over a draft copy of `settings` and
/// returns one [`Intent`] per command-backed row the user changed
/// ([`option_changes`]); the caller dispatches them through the
/// registry, so every change is an ordinary command run (status
/// message, undo, parity). The dialog itself writes nothing.
pub fn show_options(ui: &mut Ui, settings: &Settings) -> Result<OptionsChange, UiError> {
    let mut draft = settings.clone();
    let mut show_wizard = !draft.welcome_seen;

    ui.heading("Options");
    ui.separator();
    ui.label("Panel layout");
    for kind in [
        LayoutKind::PanelModern,
        LayoutKind::PanelClassic,
        LayoutKind::Panel2016,
        LayoutKind::Panel365,
    ] {
        ui.radio_value(&mut draft.ui_layout, kind, kind.label());
    }
    ui.separator();
    ui.checkbox(&mut draft.show_grid, "Show grid");
    ui.checkbox(&mut draft.show_rulers, "Show rulers");
    ui.horizontal(|ui| {
        ui.label("Units");
        let choice = egui::ComboBox::from_id_salt("settings-units")
            .selected_text(unit_label(draft.units))
            .show_ui(ui, |ui| {
                for (unit, label) in UNIT_CHOICES {
                    ui.selectable_value(&mut draft.units, *unit, *label);
                }
            });
        // A combo box with no label atom has no accessible name; egui
        // documents `on_hover_text` as the fix (ADR 0031 rule).
        choice.response.on_hover_text("Display units");
    });
    ui.separator();
    ui.checkbox(&mut show_wizard, "Show the welcome wizard on next start");

    let intents = option_changes(settings, &draft)?;
    // The row changed when the wanted state (`show_wizard`) no
    // longer matches the stored one (`welcome_seen`).
    let show_wizard_next_start = (show_wizard == settings.welcome_seen).then_some(show_wizard);
    Ok(OptionsChange {
        intents,
        show_wizard_next_start,
    })
}

/// The dispatch requests for the Options rows `after` changed against
/// `before` (ADR 0032 amendment): the layout radios dispatch
/// `view.ui_layout.panel_2016` / `view.ui_layout.panel_365`, the grid
/// and rulers checkboxes dispatch `view.toggle_grid` / `view.rulers`,
/// and the units combo dispatches `view.units` with its `units`
/// argument. All are `Source::Ui` (the dialog is interactive).
pub fn option_changes(before: &Settings, after: &Settings) -> Result<Vec<Intent>, UiError> {
    let mut intents = Vec::new();
    if before.ui_layout != after.ui_layout {
        let id = match after.ui_layout {
            LayoutKind::Panel2016 => "view.ui_layout.panel_2016",
            LayoutKind::Panel365 => "view.ui_layout.panel_365",
            // The curated layouts gain their registry rows in P7-09;
            // the dialog never offers them before then.
            LayoutKind::PanelModern => "view.ui_layout.panel_modern",
            LayoutKind::PanelClassic => "view.ui_layout.panel_classic",
        };
        intents.push(intent(id)?);
    }
    if before.show_grid != after.show_grid {
        intents.push(intent("view.toggle_grid")?);
    }
    if before.show_rulers != after.show_rulers {
        intents.push(intent("view.rulers")?);
    }
    if before.units != after.units {
        let args = Args::new().with("units", ArgValue::Text(after.units.name().to_owned()));
        let mut changed = intent("view.units")?;
        changed.invocation = Invocation::new(args, Source::Ui);
        intents.push(changed);
    }
    Ok(intents)
}

/// One dispatch request for a static command id.
fn intent(id: &'static str) -> Result<Intent, UiError> {
    let command = larrygram_commands::CommandId::parse(id).map_err(|error| UiError::Io {
        message: format!("the options command id {id} is not valid: {error}"),
    })?;
    Ok(Intent::new(command, Invocation::empty(Source::Ui)))
}

/// The display label of one unit choice, or "Inches" for a unit this
/// build does not know.
fn unit_label(units: DisplayUnits) -> &'static str {
    UNIT_CHOICES
        .iter()
        .find(|(unit, _)| *unit == units)
        .map(|(_, label)| *label)
        .unwrap_or("Inches")
}

/// Adapts any [`SettingsError`] into a [`UiError`].
pub fn settings_error(error: SettingsError) -> UiError {
    UiError::Io {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::test_util::with_ui;

    struct MemoryStore(Result<Settings, SettingsError>);

    impl SettingsStore for MemoryStore {
        fn load(&mut self) -> Result<Settings, SettingsError> {
            self.0.clone()
        }

        fn save(&mut self, _settings: &Settings) -> Result<(), SettingsError> {
            Ok(())
        }
    }

    /// A store that always fails its save with `error`.
    struct FailingSave(SettingsError);

    impl SettingsStore for FailingSave {
        fn load(&mut self) -> Result<Settings, SettingsError> {
            Ok(Settings::default())
        }

        fn save(&mut self, _settings: &Settings) -> Result<(), SettingsError> {
            Err(self.0.clone())
        }
    }

    #[test]
    fn load_returns_the_stored_settings() {
        let mut stored = Settings::default();
        stored.units = DisplayUnits::Feet;
        stored.show_grid = false;
        let mut store = MemoryStore(Ok(stored.clone()));
        let settings = load_settings(&mut store).expect("load");
        assert_eq!(settings, stored);
    }

    #[test]
    fn a_corrupt_store_falls_back_to_the_defaults() {
        let mut store = MemoryStore(Err(SettingsError::Format {
            message: "truncated".into(),
        }));
        let settings = load_settings(&mut store).expect("fallback");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn an_unreadable_store_falls_back_to_the_defaults() {
        let mut store = MemoryStore(Err(SettingsError::Io {
            message: "denied".into(),
        }));
        let settings = load_settings(&mut store).expect("fallback");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn save_writes_through_the_store() {
        let mut store = MemoryStore(Ok(Settings::default()));
        save_settings(&mut store, &Settings::default()).expect("save");
    }

    #[test]
    fn a_failed_save_becomes_a_ui_error() {
        let mut store = FailingSave(SettingsError::Io {
            message: "disk full".into(),
        });
        let error = save_settings(&mut store, &Settings::default()).expect_err("save fails");
        assert_eq!(error.to_string(), "io error: settings store: disk full");
    }

    #[test]
    fn a_quiet_options_dialog_produces_no_intents() {
        let settings = Settings::default();
        with_ui(|ui| {
            let changed = show_options(ui, &settings).expect("the dialog draws");
            assert!(changed.intents.is_empty());
            assert_eq!(changed.show_wizard_next_start, None);
        });
    }

    #[test]
    fn options_dialog_edits_no_settings_and_returns_the_change() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let settings = Rc::new(RefCell::new(Settings::default()));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let mut harness = Harness::builder().build_ui({
            let settings = Rc::clone(&settings);
            let intents = Rc::clone(&intents);
            move |ui| {
                let settings = settings.borrow();
                let mut changed = show_options(ui, &settings).expect("the dialog draws");
                intents.borrow_mut().append(&mut changed.intents);
            }
        });
        harness.run();
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(Role::RadioButton, "Panel (365 data)")
                .next()
                .expect("the 365 radio");
            node.click();
        }
        harness.run();
        let intents = intents.borrow();
        assert_eq!(
            intents
                .iter()
                .map(|intent| intent.command.as_str())
                .collect::<Vec<_>>(),
            vec!["view.ui_layout.panel_365"],
            "the radio dispatches its registry command (ADR 0032 amendment)"
        );
        assert_eq!(intents[0].invocation.source, larrygram_commands::Source::Ui);
        // The dialog writes nothing itself: the settings still hold
        // the old layout until the shell dispatches the intent.
        assert_eq!(settings.borrow().ui_layout, LayoutKind::PanelModern);
    }

    #[test]
    fn every_changed_row_maps_to_its_registry_command() {
        let before = Settings::default();
        let mut after = Settings::default();
        after.ui_layout = LayoutKind::Panel365;
        after.show_grid = false;
        after.show_rulers = false;
        after.units = DisplayUnits::Feet;
        let intents = option_changes(&before, &after).expect("intents");
        let ids: Vec<&str> = intents
            .iter()
            .map(|intent| intent.command.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "view.ui_layout.panel_365",
                "view.toggle_grid",
                "view.rulers",
                "view.units"
            ]
        );
        let units = &intents[3].invocation.args;
        assert_eq!(
            units.text("units"),
            Some("Feet"),
            "`view.units` takes its `units` argument"
        );
    }

    #[test]
    fn the_layout_radio_names_the_chosen_layout() {
        let mut before = Settings::default();
        before.ui_layout = LayoutKind::Panel365;
        let mut after = Settings::default();
        after.ui_layout = LayoutKind::Panel2016;
        let intents = option_changes(&before, &after).expect("intents");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "view.ui_layout.panel_2016");
    }

    #[test]
    fn unchanged_settings_produce_no_intents() {
        let settings = Settings::default();
        assert!(
            option_changes(&settings, &settings)
                .expect("intents")
                .is_empty()
        );
    }

    #[test]
    fn every_unit_has_a_label() {
        for (unit, label) in UNIT_CHOICES {
            assert_eq!(unit_label(*unit), *label);
        }
    }

    #[test]
    fn the_text_store_round_trips_the_settings() {
        let mut store = TextStore::new(None);
        let mut settings = Settings::default();
        settings.units = DisplayUnits::Meters;
        settings.recent_files = vec![std::path::PathBuf::from("/tmp/a.vsdx")];
        save_settings(&mut store, &settings).expect("save");
        let text = store.take_written().expect("serialized");
        let mut reloaded = TextStore::new(Some(text));
        assert_eq!(load_settings(&mut reloaded).expect("load"), settings);
    }

    #[test]
    fn an_empty_slot_loads_the_defaults() {
        let mut store = TextStore::new(None);
        assert_eq!(store.load().expect("load"), Settings::default());
        assert!(store.take_warning().is_none());
    }

    #[test]
    fn a_corrupt_slot_warns_and_falls_back() {
        let mut store = TextStore::new(Some("{not json".to_owned()));
        let error = store.load().expect_err("corrupt");
        assert!(matches!(error, SettingsError::Format { .. }));
        let warning = store.take_warning().expect("one warning");
        assert!(warning.starts_with("settings:"), "{warning}");
        assert!(store.take_warning().is_none(), "exactly one warning");
        assert_eq!(
            load_settings(&mut store).expect("fallback"),
            Settings::default()
        );
    }

    #[test]
    fn option_changes_cover_every_layout_kind() {
        // The dialog offers all four layouts (task P7-09) and the
        // change rule names an id for each one (task P7-00).
        for (kind, id) in [
            (LayoutKind::PanelModern, "view.ui_layout.panel_modern"),
            (LayoutKind::PanelClassic, "view.ui_layout.panel_classic"),
            (LayoutKind::Panel2016, "view.ui_layout.panel_2016"),
            (LayoutKind::Panel365, "view.ui_layout.panel_365"),
        ] {
            let mut before = Settings::default();
            before.ui_layout = LayoutKind::PanelModern;
            let mut after = Settings::default();
            after.ui_layout = kind;
            let intents = option_changes(&before, &after).expect("intents");
            if kind == LayoutKind::PanelModern {
                assert!(intents.is_empty(), "Panel Modern is the draft's start");
                continue;
            }
            assert_eq!(intents.len(), 1);
            assert_eq!(intents[0].command.as_str(), id);
        }
    }

    #[test]
    fn a_settings_store_saved_before_the_rename_still_loads() {
        // The exact JSON a pre-P7-00 build wrote: the old enum
        // spellings and none of the later fields.
        let old = concat!(
            r#"{"ui_layout":"Visio2016","show_grid":false,"show_rulers":true,"#,
            r#""units":"Inches","recent_files":[]}"#
        );
        let mut store = TextStore::new(Some(old.to_owned()));
        let settings = store.load().expect("the old store loads");
        assert_eq!(
            settings.ui_layout,
            LayoutKind::Panel2016,
            "the old spelling maps to Panel (2016 data)"
        );
        assert!(!settings.show_grid);
        assert_eq!(settings.units, DisplayUnits::Inches);
        assert!(
            store.take_warning().is_none(),
            "no warning for an old store"
        );
        // The serde default on `ui_layout` covers a store written
        // before the field existed (ADR 0032 keeps the other fields
        // strict: a missing one is corrupt, not a partial load).
        let mut older = TextStore::new(Some(
            r#"{"show_grid":false,"show_rulers":true,"units":"Inches","recent_files":[]}"#
                .to_owned(),
        ));
        let settings = older.load().expect("the older store loads");
        assert_eq!(
            settings.ui_layout,
            LayoutKind::PanelModern,
            "a store with no `ui_layout` gets the current default"
        );
        assert!(!settings.show_grid);
    }

    #[test]
    fn settings_errors_wrap_into_ui_errors() {
        let error = settings_error(SettingsError::Format {
            message: "truncated".into(),
        });
        assert_eq!(error.to_string(), "io error: settings format: truncated");
    }
}

#[cfg(test)]
mod welcome_row_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn defaults_are_first_run_and_panel_modern() {
        let settings = Settings::default();
        assert!(!settings.welcome_seen);
        assert_eq!(settings.default_layout, LayoutKind::PanelModern);
    }

    #[test]
    fn the_welcome_row_clears_welcome_seen() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let settings = Rc::new(RefCell::new(Settings::default()));
        {
            settings.borrow_mut().welcome_seen = true;
        }
        let changes = Rc::new(RefCell::new(Vec::new()));
        let mut harness = Harness::builder().build_ui({
            let settings = Rc::clone(&settings);
            let changes = Rc::clone(&changes);
            move |ui| {
                let settings = settings.borrow();
                let changed = show_options(ui, &settings).expect("the dialog draws");
                if let Some(show) = changed.show_wizard_next_start {
                    changes.borrow_mut().push(show);
                }
            }
        });
        harness.run();
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(
                    Role::CheckBox,
                    "Show the welcome wizard on next start",
                )
                .next()
                .expect("the welcome row");
            node.click();
        }
        harness.run();
        assert_eq!(
            changes.borrow().as_slice(),
            &[true],
            "checking the row asks for the wizard on the next start"
        );
        // The dialog writes nothing itself: the shell writes
        // `welcome_seen = !show` (task P7-09).
        assert!(settings.borrow().welcome_seen);
    }

    #[test]
    fn a_store_without_the_phase_7_fields_loads_with_the_defaults() {
        let mut store = TextStore::new(Some(
            r#"{"ui_layout":"Panel2016","show_grid":false,"show_rulers":true,"#.to_owned(),
        ));
        // The truncated JSON above is corrupt; the real Phase 6 shape
        // is the next assertion.
        let _ = store.load().expect_err("corrupt");
        let mut store = TextStore::new(Some(
            concat!(
                r#"{"ui_layout":"Panel2016","show_grid":false,"show_rulers":true,"#,
                r#""units":"Inches","recent_files":[]}"#
            )
            .to_owned(),
        ));
        let settings = load_settings(&mut store).expect("the Phase 6 store loads");
        assert_eq!(
            settings.ui_layout,
            LayoutKind::Panel2016,
            "stored value kept"
        );
        assert_eq!(settings.default_layout, LayoutKind::PanelModern);
        assert!(!settings.welcome_seen);
        assert!(
            store.take_warning().is_none(),
            "a Phase 6 store is not corrupt"
        );
    }
}
