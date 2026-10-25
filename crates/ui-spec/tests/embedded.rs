//! The embedded loader equals the on-disk loader on the committed
//! `ui-spec/` files (ADR 0031, task P4-04 acceptance criterion 1).
//!
//! `Loader::from_embedded` parses the strings of
//! `larrygram_ui_spec::embedded`; `Loader::load_all` reads the same
//! documents from `ui-spec/`. The two must produce identical
//! [`LayoutSets`](larrygram_ui_spec::LayoutSets): one source of truth,
//! embedded for the wasm build (no filesystem).

use std::path::{Path, PathBuf};

use larrygram_ui_spec::{LayoutKind, LayoutSets, Loader};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed data, loaded from disk.
#[allow(clippy::expect_used)] // test helper; tests may panic
fn from_disk() -> LayoutSets {
    Loader::new(workspace_root())
        .expect("the embedded schema compiles")
        .load_all()
        .expect("the committed files load")
}

#[test]
fn from_embedded_matches_load_all_on_the_committed_files() {
    let embedded = Loader::from_embedded().expect("the embedded files load");
    assert_eq!(embedded, from_disk());
}

#[test]
fn from_embedded_matches_load_for_each_kind() {
    let loader = Loader::new(workspace_root()).expect("the embedded schema compiles");
    let embedded = Loader::from_embedded().expect("the embedded files load");
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        assert_eq!(
            embedded.get(kind),
            &loader.load(kind).expect("the committed file loads"),
            "kind {kind:?} differs"
        );
    }
}

#[test]
fn from_embedded_shares_the_tables_between_the_two_layouts() {
    let sets = Loader::from_embedded().expect("the embedded files load");
    assert_ne!(sets.panel_2016.layout, sets.panel_365.layout);
    assert_eq!(sets.panel_2016.commands, sets.panel_365.commands);
    assert_eq!(sets.panel_2016.shortcuts, sets.panel_365.shortcuts);
    assert!(!sets.panel_2016.layout.tabs.is_empty());
    assert!(!sets.panel_365.layout.tabs.is_empty());
    assert!(!sets.panel_2016.commands.entries.is_empty());
    assert!(!sets.panel_2016.shortcuts.entries.is_empty());
}

#[test]
fn from_embedded_is_reachable_without_a_workspace_root() {
    // The whole point of the embedded loader (ADR 0031): no path is
    // consulted. A workspace root of "." would still find the files in
    // this test binary's working directory, so the check here is that
    // the sets come back at all and carry the committed provenance
    // strings.
    let sets = Loader::from_embedded().expect("the embedded files load");
    assert!(sets.panel_2016.layout.source.contains("2016"));
    assert!(sets.panel_365.layout.source.contains("365"));
    assert_eq!(sets.panel_2016.layout.product, "visio");
}
