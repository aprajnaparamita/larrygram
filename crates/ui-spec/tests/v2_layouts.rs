//! Loads the layout format v2 documents from disk and from the
//! embedded copies, and drives every cross-check with a failing input
//! (ADR 0061, ADR 0065, task P7-03).
//!
//! The failing-input tests start from a copy of the `good` fixture
//! workspace, break exactly one thing and assert the loader reports
//! that thing (`crates/ui-spec/src/v2/load.rs` unit-tests the same
//! checks on hand-built layouts).

use std::path::{Path, PathBuf};

use larrygram_ui_spec::v2::{LayoutKind, PanelStyle, load_layout};
use larrygram_ui_spec::{LoadError, Loader};

/// The fixture directory.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A copy of the good fixture workspace, in the temp directory.
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn good_copy(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "larrygram-ui-spec-v2-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_tree(&fixtures().join("good"), &dir);
    dir
}

/// Recursive file copy (the fixture tree is tiny).
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Replaces `from` with `to` in one file of the workspace.
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn patch(root: &Path, name: &str, from: &str, to: &str) {
    let path = root.join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{name} holds {from:?}");
    std::fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn all_four_kinds_load_from_disk() {
    let loader = Loader::new(fixtures().join("good")).expect("the fixture workspace loads");
    for kind in [
        LayoutKind::PanelModern,
        LayoutKind::PanelClassic,
        LayoutKind::Panel2016,
        LayoutKind::Panel365,
    ] {
        let set = loader
            .load(kind)
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert_eq!(set.v2.id, kind.id(), "{kind:?}");
        assert_eq!(
            set.theme.id, set.v2.theme,
            "{kind:?} theme follows the layout"
        );
    }
    // The curated kinds are their own style; the adapted ones are
    // panel-modern (ADR 0061).
    let modern = loader.load(LayoutKind::PanelModern).expect("loads");
    assert_eq!(modern.v2.kind, PanelStyle::PanelModern);
    let classic = loader.load(LayoutKind::PanelClassic).expect("loads");
    assert_eq!(classic.v2.kind, PanelStyle::PanelClassic);
    let adapted = loader.load(LayoutKind::Panel2016).expect("loads");
    assert_eq!(adapted.v2.kind, PanelStyle::PanelModern);
    assert!(!adapted.layout.tabs.is_empty(), "keeps the v1 data");
}

#[test]
fn all_four_kinds_load_from_embedded_and_match_disk() {
    let loader = Loader::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("the repository workspace loads");
    let sets = Loader::from_embedded().expect("the embedded documents load");
    for kind in [
        LayoutKind::PanelModern,
        LayoutKind::PanelClassic,
        LayoutKind::Panel2016,
        LayoutKind::Panel365,
    ] {
        let from_disk = loader
            .load(kind)
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        // `LayoutSets::get` is the shell's v1-widget seam and answers
        // `None` for the v2-only kinds (task P7-00), so read the
        // fields here.
        let embedded = match kind {
            LayoutKind::PanelModern => &sets.panel_modern,
            LayoutKind::PanelClassic => &sets.panel_classic,
            LayoutKind::Panel2016 => &sets.panel_2016,
            LayoutKind::Panel365 => &sets.panel_365,
        };
        // The two v1-data kinds also answer through the shell's
        // `LayoutSets::get` seam.
        if matches!(kind, LayoutKind::Panel2016 | LayoutKind::Panel365) {
            assert_eq!(sets.get(kind), Some(embedded), "{kind:?} get matches");
        }
        assert_eq!(embedded.v2, from_disk.v2, "{kind:?} v2 matches");
        assert_eq!(embedded.theme, from_disk.theme, "{kind:?} theme matches");
        assert_eq!(embedded.icons, from_disk.icons, "{kind:?} icons match");
        assert_eq!(
            embedded.contexts, from_disk.contexts,
            "{kind:?} contexts match"
        );
        let via_load_layout = load_layout(kind).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert_eq!(
            via_load_layout, from_disk.v2,
            "{kind:?} load_layout matches"
        );
    }
}

#[test]
fn an_unknown_command_is_an_item_command_cross_reference() {
    let dir = good_copy("item-command");
    // The fixture items are Phase 7 proposals (`new_command`); drop
    // the flag so the command must resolve.
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "  command: file.save\n  new_command: true\n  icon: file.save",
        "  command: nope.unknown\n  icon: file.save",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "item_command");
            assert_eq!(id, "nope.unknown");
            assert_eq!(referenced_by, "qat.save");
        }
        other => panic!("expected an item_command error, got {:?}", other.err()),
    }
}

#[test]
fn a_command_marked_as_a_proposal_may_be_unknown() {
    // The same file loads when the placement carries `new_command`:
    // the flag is how `ui-spec/commands-phase7.yaml` ids pass (ADR
    // 0061 amendments).
    let dir = good_copy("proposal");
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "  command: file.save\n  new_command: true\n  icon: file.save",
        "  command: nope.unknown\n  new_command: true\n  icon: file.save",
    );
    Loader::new(&dir)
        .expect("compiles")
        .load_all()
        .expect("loads");
}

#[test]
fn an_unknown_icon_key_is_an_icon_key_cross_reference() {
    let dir = good_copy("icon-key");
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "icon: file.save",
        "icon: nope",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "icon_key");
            assert_eq!(id, "nope");
            assert_eq!(referenced_by, "qat.save");
        }
        other => panic!("expected an icon_key error, got {:?}", other.err()),
    }
}

#[test]
fn a_missing_icon_svg_is_an_icon_file_cross_reference() {
    let dir = good_copy("icon-file");
    std::fs::remove_file(dir.join("assets/icons/save.svg")).expect("the fixture svg");
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "icon_file");
            assert_eq!(id, "save");
            assert_eq!(referenced_by, "file.save");
        }
        other => panic!("expected an icon_file error, got {:?}", other.err()),
    }
}

#[test]
fn an_unknown_shown_in_context_is_a_shown_in_cross_reference() {
    let dir = good_copy("shown-in");
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "contextual_tabs: []",
        "contextual_tabs:\n  - id: tab.picture_tools.format\n    label: Format\n    tab_set: Picture Tools\n    shown_in: picture_selected\n    groups: []",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "shown_in");
            assert_eq!(id, "picture_selected");
            assert_eq!(referenced_by, "tab.picture_tools.format");
        }
        other => panic!("expected a shown_in error, got {:?}", other.err()),
    }
}

#[test]
fn an_unknown_context_command_is_a_context_command_cross_reference() {
    let dir = good_copy("context-command");
    patch(
        &dir,
        "ui-spec/contexts.yaml",
        "commands: {}",
        "commands:\n  nope.unknown:\n    enabled_in: [empty_page]",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "context_command");
            assert_eq!(id, "nope.unknown");
            assert_eq!(referenced_by, "contexts.yaml");
        }
        other => panic!("expected a context_command error, got {:?}", other.err()),
    }
}

#[test]
fn a_duplicate_item_id_is_a_duplicate() {
    let dir = good_copy("item-id");
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "- id: qat.save\n  kind: button\n  label: Save\n  command: file.save\n  new_command: true\n  icon: file.save",
        "- id: qat.save\n  kind: button\n  label: Save\n  command: file.save\n  new_command: true\n  icon: file.save\n- id: qat.save\n  kind: button\n  label: Save again\n  command: file.save\n  new_command: true",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::Duplicate { kind, id, path }) => {
            assert_eq!(kind, "item_id");
            assert_eq!(id, "qat.save");
            assert!(
                path.ends_with("ui-spec/layouts/panel-modern.yaml"),
                "{path:?}"
            );
        }
        other => panic!("expected a duplicate item_id, got {:?}", other.err()),
    }
}

#[test]
fn a_schema_violation_in_a_v2_layout_is_a_schema_error() {
    let dir = good_copy("v2-schema");
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "- id: qat.save\n  kind: button",
        "- id: qat.save\n  kind: not_a_kind",
    );
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::Schema { path, message }) => {
            assert!(
                path.ends_with("ui-spec/layouts/panel-modern.yaml"),
                "{path:?}"
            );
            assert!(!message.is_empty());
        }
        other => panic!("expected a schema error, got {:?}", other.err()),
    }
}

#[test]
fn broken_yaml_in_a_v2_file_is_a_parse_error() {
    let dir = good_copy("v2-parse");
    patch(&dir, "ui-spec/icons.yaml", "icons:", "icons: [");
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::Parse { path, .. }) => {
            assert!(path.ends_with("ui-spec/icons.yaml"), "{path:?}");
        }
        other => panic!("expected a parse error, got {:?}", other.err()),
    }
}

#[test]
fn a_missing_v2_file_is_an_io_error() {
    let dir = good_copy("v2-io");
    std::fs::remove_file(dir.join("ui-spec/layouts/panel-classic.yaml")).expect("the fixture file");
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::Io { path, source }) => {
            assert!(
                path.ends_with("ui-spec/layouts/panel-classic.yaml"),
                "{path:?}"
            );
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected an io error, got {:?}", other.err()),
    }
}

#[test]
fn a_deeply_nested_item_tree_is_rejected_without_crashing() {
    let dir = good_copy("v2-deep");
    // 200 nested `items` lists: past the depth bound of the loader.
    let mut nested = String::new();
    for level in 0..200 {
        let pad = "  ".repeat(2 + level);
        nested.push_str(&format!(
            "{pad}- id: deep{level}\n{pad}  kind: submenu\n{pad}  items:\n"
        ));
    }
    patch(
        &dir,
        "ui-spec/layouts/panel-modern.yaml",
        "  icon: file.save",
        &format!("  icon: file.save\n  items:\n{nested}"),
    );
    // Either the YAML parser's own depth budget or the loader's depth
    // check rejects it; both are `LoadError::Parse` and neither
    // overflows the stack (the unit tests in `src/v2/load.rs` pin the
    // loader's own message).
    match Loader::new(&dir).expect("compiles").load_all() {
        Err(LoadError::Parse { message, .. }) => {
            assert!(!message.is_empty(), "the parse error explains itself");
        }
        other => panic!("expected a depth parse error, got {:?}", other.err()),
    }
}
