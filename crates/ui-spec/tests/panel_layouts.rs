//! Validates the committed Panel Modern and Panel Classic layouts
//! with the loader's own staged path (ADR 0061, ADR 0065).
//!
//! Task P7-01 added this file with an ad-hoc walk of the raw YAML
//! (schema validation, decode, a command-id walk that skipped the
//! `ui-spec/commands-phase7.yaml` proposals). Task P7-03 moved those
//! checks into the loader (`crates/ui-spec/src/v2/load.rs`): this test
//! now drives the real load path over the committed data, from disk
//! and from the embedded copies.

use std::path::{Path, PathBuf};

use larrygram_ui_spec::Loader;
use larrygram_ui_spec::v2::{LayoutKind, PanelStyle, load_layout};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn both_layouts_load_from_disk_through_the_loader() {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    for (kind, style) in [
        (LayoutKind::PanelModern, PanelStyle::PanelModern),
        (LayoutKind::PanelClassic, PanelStyle::PanelClassic),
    ] {
        let set = loader
            .load(kind)
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert_eq!(set.v2.schema_version, 2, "{kind:?}");
        assert_eq!(set.v2.kind, style, "{kind:?}");
        assert!(
            !set.v2.context_menus.is_empty(),
            "{kind:?} has context menus"
        );
        assert!(!set.theme.colors.is_empty(), "{kind:?} theme has colours");
    }
}

#[test]
fn both_layouts_load_from_embedded_too() {
    // The embedded copies go through the same staged load: parse,
    // schema validation, decode and cross-checks.
    for kind in [LayoutKind::PanelModern, LayoutKind::PanelClassic] {
        let layout = load_layout(kind).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert_eq!(layout.id, kind.id(), "{kind:?}");
    }
}
