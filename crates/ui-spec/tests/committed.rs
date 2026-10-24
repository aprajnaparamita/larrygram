//! Loads the committed `ui-spec/` data and pins the real numbers.
//!
//! The loader tests in `tests/loader.rs` run against the small hand-
//! written fixtures; this file drives the same public API over the
//! data the extractors actually committed (P1-08 goal 1, closing the
//! deferred P1-06b item). The workspace root is two levels above
//! `CARGO_MANIFEST_DIR`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use larrygram_ui_spec::{CommandId, CommandLookup, Control, FluentId, Layout, LayoutKind, Loader};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every control of the layout, in document order (tab groups,
/// backstage tabs, context menus, quick access toolbar).
fn all_controls(layout: &Layout) -> Vec<&Control> {
    let tab_controls = layout
        .tabs
        .iter()
        .flat_map(|tab| tab.groups.iter())
        .flat_map(|group| group.controls.iter());
    let backstage_controls = layout
        .backstage
        .tabs
        .iter()
        .flat_map(|tab| tab.controls.iter());
    let menu_controls = layout
        .context_menus
        .iter()
        .flat_map(|menu| menu.controls.iter());
    tab_controls
        .chain(backstage_controls)
        .chain(menu_controls)
        .chain(layout.quick_access_toolbar.controls.iter())
        .collect()
}

/// The counts pinned by this file, for one layout.
#[derive(Debug, PartialEq, Eq)]
struct LayoutCounts {
    /// Command bar tabs.
    tabs: usize,
    /// Backstage tabs.
    backstage_tabs: usize,
    /// Context menus.
    context_menus: usize,
    /// Controls (every surface, including the quick access toolbar).
    controls: usize,
}

fn counts(layout: &Layout) -> LayoutCounts {
    LayoutCounts {
        tabs: layout.tabs.len(),
        backstage_tabs: layout.backstage.tabs.len(),
        context_menus: layout.context_menus.len(),
        controls: all_controls(layout).len(),
    }
}

/// Pinned numbers for the committed `ui-spec/` data set. Update these
/// deliberately when a regenerated output changes the shape of the
/// data (never to make a broken run pass).
const PIN_2016: LayoutCounts = LayoutCounts {
    tabs: 16,
    backstage_tabs: 8,
    context_menus: 43,
    controls: 497,
};
const PIN_365: LayoutCounts = LayoutCounts {
    tabs: 19,
    backstage_tabs: 10,
    context_menus: 43,
    controls: 670,
};
// 935 before P4-02; the "our own commands" list (ADR 0006 amendment)
// adds 11 rows (P4-15 adds `view.units`) and two Fluent-only rows merge
// into them. P5-04 adds the 31 Phase 5 editing ids the mechanical
// derivation does not produce (task P5-04, forced companion update to
// the regenerated `commands.yaml`). P5-16 adds `edit.select`. P6-18
// adds `data.set.property` (the Shape Data pane's edit, ADR 0035
// amendment). P7-02 adds the 155 Phase 7 ids the Panel Classic and
// Panel Modern layouts use, merges the duplicates among them and drops
// the two rows that only held a moved Fluent id (`tools.objects.ungroup`
// and `help.about.visio`).
const COMMANDS: usize = 1131;
const SHORTCUTS: usize = 90;

#[test]
fn committed_data_loads_and_counts_are_pinned() {
    let loader = Loader::new(workspace_root()).expect("the embedded schema compiles");
    let sets = loader.load_all().expect("the committed ui-spec loads");
    assert_eq!(counts(&sets.panel_2016.layout), PIN_2016, "panel-2016");
    assert_eq!(counts(&sets.panel_365.layout), PIN_365, "panel-365");
    assert_eq!(sets.panel_2016.commands.entries.len(), COMMANDS, "commands");
    assert_eq!(
        sets.panel_365.commands.entries.len(),
        COMMANDS,
        "commands (365)"
    );
    assert_eq!(
        sets.panel_2016.shortcuts.entries.len(),
        SHORTCUTS,
        "shortcuts"
    );
    assert_eq!(
        sets.panel_365.shortcuts.entries.len(),
        SHORTCUTS,
        "shortcuts (365)"
    );
}

/// `CommandLookup` invariants, for both layouts:
///
/// - every `commands` value is a command id defined in `commands.yaml`,
/// - `fluent_by_command` is the exact inverse of `commands`,
/// - every `command` in `shortcuts.yaml` has an entry in `commands.yaml`.
#[test]
fn command_lookup_invariants_hold_on_both_layouts() {
    let loader = Loader::new(workspace_root()).expect("the embedded schema compiles");
    let sets = loader.load_all().expect("the committed ui-spec loads");
    let known: BTreeSet<&CommandId> = sets
        .panel_2016
        .commands
        .entries
        .iter()
        .map(|entry| &entry.id)
        .collect();

    for (name, layout) in [
        ("panel-2016", &sets.panel_2016),
        ("panel-365", &sets.panel_365),
    ] {
        let lookup: CommandLookup = layout.command_lookup();

        // Every dispatched command is defined in commands.yaml.
        for (fluent, command) in &lookup.commands {
            assert!(
                known.contains(command),
                "{name}: {fluent} dispatches unknown command {command}"
            );
        }

        // fluent_by_command is the exact inverse of commands.
        let mut inverse: std::collections::BTreeMap<&CommandId, Vec<&FluentId>> =
            std::collections::BTreeMap::new();
        for (fluent, command) in &lookup.commands {
            inverse.entry(command).or_default().push(fluent);
        }
        assert_eq!(
            lookup.fluent_by_command.len(),
            inverse.len(),
            "{name}: fluent_by_command has a different number of commands"
        );
        for (command, ids) in &lookup.fluent_by_command {
            let expected: Vec<&FluentId> = inverse
                .get(command)
                .unwrap_or_else(|| panic!("{name}: {command} missing from fluent_by_command"))
                .clone();
            let got: Vec<&FluentId> = ids.iter().collect();
            assert_eq!(got, expected, "{name}: fluent_by_command[{command}]");
        }

        // Every shortcut dispatches a defined command.
        for shortcut in &layout.shortcuts.entries {
            assert!(
                known.contains(&shortcut.command),
                "{name}: shortcut for unknown command {}",
                shortcut.command
            );
        }
    }
}

/// Both `LayoutKind`s pick their own set from the same load.
#[test]
fn layout_kind_file_names_match_the_committed_files() {
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let path = workspace_root().join("ui-spec").join(kind.file_name());
        assert!(path.is_file(), "{} must exist", path.display());
    }
}

/// Serialize round-trip (P1-14 goal 3): deserialize, serialize,
/// deserialize must give an equal value — over the whole committed
/// data set, for every public model type.
#[test]
fn model_types_round_trip_over_the_committed_data() {
    let loader = Loader::new(workspace_root()).expect("the embedded schema compiles");
    let sets = loader.load_all().expect("the committed ui-spec loads");
    for (name, set) in [
        ("panel-2016", &sets.panel_2016),
        ("panel-365", &sets.panel_365),
    ] {
        round_trip(&set.layout, name);
        round_trip(&set.commands, name);
        round_trip(&set.shortcuts, name);
        round_trip(set, name);
        round_trip(&set.command_lookup(), name);
    }
    round_trip(&sets, "both layouts");
}

/// Round-trips one value through JSON and asserts the result is
/// equal.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helper; tests may panic
fn round_trip<T>(value: &T, name: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_value(value).expect("serializes");
    let back: T = serde_json::from_value(json).expect("deserializes");
    assert_eq!(&back, value, "{name}: round trip changed the value");
}
