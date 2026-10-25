//! Integration tests for the [`Loader`], driven by the hand-written
//! fixture files in `tests/fixtures/`.
//!
//! The fixtures are small workspace roots: `good/` holds a complete
//! consistent data set (both layouts plus `commands.yaml` and
//! `shortcuts.yaml`), `xref-*/` each break one cross-reference, and
//! `bad.yaml` violates the schema. Every workspace fixture carries
//! **both** layout files: the loader reads both on every load, so the
//! `fluent_id` cross-reference can run against their union.

use std::path::{Path, PathBuf};

use larrygram_ui_spec::{CommandId, FluentId, LayoutKind, LayoutSet, LoadError, Loader};

/// The fixture directory.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A fresh empty workspace root in the temp directory, with the
/// `ui-spec/` subdirectory created.
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn temp_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("larrygram-ui-spec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ui-spec")).unwrap();
    dir
}

/// Copies `commands.yaml` and `shortcuts.yaml` from the good fixture
/// into a temp workspace.
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn copy_good_tables(dir: &Path) {
    let source = fixtures().join("good").join("ui-spec");
    for name in ["commands.yaml", "shortcuts.yaml"] {
        std::fs::copy(source.join(name), dir.join("ui-spec").join(name)).unwrap();
    }
}

/// Copies both layout files from the good fixture into a temp
/// workspace (every load reads both).
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn copy_good_layouts(dir: &Path) {
    let source = fixtures().join("good").join("ui-spec");
    for name in ["panel-2016.yaml", "panel-365.yaml"] {
        std::fs::copy(source.join(name), dir.join("ui-spec").join(name)).unwrap();
    }
}

/// Copies the layout format v2 documents (and the icon SVGs) from the
/// good fixture into a temp workspace (every load reads them too).
#[allow(clippy::unwrap_used)] // test helper; tests may panic
fn copy_good_v2(dir: &Path) {
    let good = fixtures().join("good");
    let source = good.join("ui-spec");
    for name in ["icons.yaml", "contexts.yaml"] {
        std::fs::copy(source.join(name), dir.join("ui-spec").join(name)).unwrap();
    }
    for sub in [
        "layouts/panel-modern.yaml",
        "layouts/panel-classic.yaml",
        "themes/panel-modern.yaml",
        "themes/panel-classic.yaml",
    ] {
        let target = dir.join("ui-spec").join(sub);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(source.join(sub), target).unwrap();
    }
    let svg = good.join("assets/icons/save.svg");
    let target = dir.join("assets/icons/save.svg");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(svg, target).unwrap();
}

#[test]
fn loader_reads_both_layouts_from_the_good_fixture() {
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let set_2016 = loader.load(LayoutKind::Panel2016).unwrap();
    let set_365 = loader.load(LayoutKind::Panel365).unwrap();

    // A LayoutSet has layout, commands and shortcuts all populated,
    // and layout ids are Fluent ids, not command ids.
    assert_eq!(set_2016.layout.tabs[0].id, FluentId::new("TabHome"));
    assert_eq!(set_2016.layout.tabs[1].id, FluentId::new("TabInsert"));
    assert_eq!(set_2016.commands.entries.len(), 7);
    assert_eq!(set_2016.shortcuts.entries.len(), 3);

    // Both layouts load from the same commands/shortcuts tables.
    assert_eq!(set_365.layout.tabs[0].id, FluentId::new("TabHome"));
    assert_eq!(set_365.commands.entries, set_2016.commands.entries);
    assert_eq!(set_365.shortcuts.entries, set_2016.shortcuts.entries);
}

#[test]
fn command_lookup_indexes_labels_commands_and_reverse() {
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let set_2016 = loader.load(LayoutKind::Panel2016).unwrap();
    let lookup = set_2016.command_lookup();

    assert_eq!(
        lookup.labels.get(&FluentId::new("FileSave")),
        Some(&"Save".to_owned())
    );
    assert_eq!(
        lookup.commands.get(&FluentId::new("FileSave")),
        Some(&CommandId::new("file.save"))
    );
    // `file.save` is surfaced by two controls in the 2016 layout.
    assert_eq!(
        lookup.fluent_by_command.get(&CommandId::new("file.save")),
        Some(&vec![
            FluentId::new("FileSave"),
            FluentId::new("FileSaveQuickAccess")
        ])
    );
    // An alias resolves to the control's label but is not a dispatch
    // key: the primary path wins.
    assert_eq!(
        lookup.labels.get(&FluentId::new("PasteMenu")),
        Some(&"Paste".to_owned())
    );
    assert!(!lookup.commands.contains_key(&FluentId::new("PasteMenu")));
    // Tabs and groups are labelled too.
    assert_eq!(
        lookup.labels.get(&FluentId::new("GroupClipboard")),
        Some(&"Clipboard".to_owned())
    );
}

#[test]
fn get_switches_between_the_two_layouts() {
    // LayoutSets::get is the one layout switch (P1-14: LayoutSet::select is gone).
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let sets = loader.load_all().unwrap();
    let set_2016 = loader.load(LayoutKind::Panel2016).unwrap();
    let set_365 = loader.load(LayoutKind::Panel365).unwrap();
    assert_eq!(sets.get(LayoutKind::Panel2016), &set_2016);
    assert_eq!(sets.get(LayoutKind::Panel365), &set_365);
}

#[test]
fn bad_fixture_is_rejected_with_a_schema_error() {
    let dir = temp_workspace("bad");
    copy_good_layouts(&dir);
    copy_good_tables(&dir);
    copy_good_v2(&dir);
    std::fs::copy(
        fixtures().join("bad.yaml"),
        dir.join("ui-spec").join("panel-2016.yaml"),
    )
    .unwrap();

    let loader = Loader::new(&dir).unwrap();
    match loader.load(LayoutKind::Panel2016) {
        Err(LoadError::Schema { path, message }) => {
            assert!(path.ends_with("ui-spec/panel-2016.yaml"));
            assert!(
                !message.is_empty(),
                "the validator message must not be empty"
            );
        }
        other => panic!("expected LoadError::Schema, got {other:?}"),
    }
}

#[test]
fn unknown_fluent_id_in_commands_is_rejected() {
    let loader = Loader::new(fixtures().join("xref-fluent")).unwrap();
    // Neither layout uses GhostControl, so the id is in no layout at
    // all and both LayoutKinds reject it.
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let err = loader.load(kind).expect_err("must fail");
        let message = err.to_string();
        assert!(
            message.contains("is not in any layout"),
            "{kind:?}: {message}"
        );
        match err {
            LoadError::CrossReference {
                kind,
                id,
                referenced_by,
            } => {
                assert_eq!(kind, "fluent_id");
                assert_eq!(id, "GhostControl");
                assert_eq!(referenced_by, "file.save");
            }
            other => panic!("expected a fluent_id cross-reference error, got {other:?}"),
        }
    }
}

#[test]
fn a_command_mapped_to_a_365_only_control_loads_for_both_kinds() {
    // `edit.redo` in commands.yaml lists RedoButton, which only the
    // 365 layout contains: commands.yaml is shared, so the check
    // runs against the union of both layouts (ADR 0007) and neither
    // kind fails.
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let set_2016 = loader.load(LayoutKind::Panel2016).unwrap();
    let set_365 = loader.load(LayoutKind::Panel365).unwrap();
    for set in [&set_2016, &set_365] {
        let redo = set
            .commands
            .entries
            .iter()
            .find(|entry| entry.id == CommandId::new("edit.redo"))
            .expect("edit.redo is mapped");
        assert_eq!(redo.fluent_ids, vec![FluentId::new("RedoButton")]);
    }
    // RedoButton is a 365-only control...
    let in_365 = set_365.command_lookup();
    assert!(in_365.commands.contains_key(&FluentId::new("RedoButton")));
    // ...so the 2016 layout simply has no control for it.
    let in_2016 = set_2016.command_lookup();
    assert!(!in_2016.labels.contains_key(&FluentId::new("RedoButton")));
}

#[test]
fn a_control_without_a_command_loads_and_is_labelled_only() {
    // FontSizeLabel has `command: null` in both layouts: it is a
    // labelled surface with no dispatch entry.
    let loader = Loader::new(fixtures().join("good")).unwrap();
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let set = loader.load(kind).unwrap();
        let lookup = set.command_lookup();
        assert_eq!(
            lookup.labels.get(&FluentId::new("FontSizeLabel")),
            Some(&"Font Size".to_owned()),
            "{kind:?}"
        );
        assert!(
            !lookup
                .commands
                .contains_key(&FluentId::new("FontSizeLabel")),
            "{kind:?}"
        );
    }
}

#[test]
fn load_all_matches_loading_each_kind() {
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let sets = loader.load_all().unwrap();
    assert_eq!(
        sets.get(LayoutKind::Panel2016),
        &loader.load(LayoutKind::Panel2016).unwrap()
    );
    assert_eq!(
        sets.get(LayoutKind::Panel365),
        &loader.load(LayoutKind::Panel365).unwrap()
    );
    // The shared tables are identical in both sets.
    assert_eq!(sets.panel_2016.commands, sets.panel_365.commands);
    assert_eq!(sets.panel_2016.shortcuts, sets.panel_365.shortcuts);
}

#[test]
fn empty_shortcut_table_loads() {
    let dir = temp_workspace("empty-shortcuts");
    copy_good_layouts(&dir);
    copy_good_tables(&dir);
    copy_good_v2(&dir);
    std::fs::write(dir.join("ui-spec/shortcuts.yaml"), "[]\n").unwrap();

    let loader = Loader::new(&dir).unwrap();
    let set = loader.load(LayoutKind::Panel2016).unwrap();
    assert!(set.shortcuts.entries.is_empty());
    assert!(!set.commands.entries.is_empty());
    // Empty is fine for both kinds and for load_all, too.
    assert!(
        loader
            .load(LayoutKind::Panel365)
            .unwrap()
            .shortcuts
            .entries
            .is_empty()
    );
    assert!(
        loader
            .load_all()
            .unwrap()
            .get(LayoutKind::Panel2016)
            .shortcuts
            .entries
            .is_empty()
    );
}

#[test]
fn empty_command_table_loads_when_no_control_dispatches_a_command() {
    // A workspace whose only control has `command: null`, so an
    // empty commands.yaml loses nothing.
    let dir = temp_workspace("empty-commands");
    let layout = r#"
schema_version: 1
product: visio
source: "Empty commands fixture"
source_url: "https://example.com/fixture/empty-commands"
source_revision: "fixture"
tabs: []
backstage:
  kind: backstage
  tabs: []
context_menus: []
quick_access_toolbar:
  id: qat
  controls:
    - id: StatusIndicator
      type: labelControl
      label: Ready
      order: 0
      command: null
      bounds: null
      icon_ref: null
      keytip: null
      size_variant: null
      visual_notes: null
"#;
    for name in ["panel-2016.yaml", "panel-365.yaml"] {
        std::fs::write(dir.join("ui-spec").join(name), layout).unwrap();
    }
    std::fs::write(dir.join("ui-spec/commands.yaml"), "[]\n").unwrap();
    std::fs::write(dir.join("ui-spec/shortcuts.yaml"), "[]\n").unwrap();
    copy_good_v2(&dir);

    let loader = Loader::new(&dir).unwrap();
    let set = loader.load(LayoutKind::Panel2016).unwrap();
    assert!(set.commands.entries.is_empty());
    assert_eq!(
        set.command_lookup()
            .labels
            .get(&FluentId::new("StatusIndicator")),
        Some(&"Ready".to_owned())
    );
    assert!(
        loader
            .load_all()
            .unwrap()
            .get(LayoutKind::Panel365)
            .commands
            .entries
            .is_empty()
    );
}

#[test]
fn unknown_control_command_is_rejected() {
    let loader = Loader::new(fixtures().join("xref-control")).unwrap();
    match loader.load(LayoutKind::Panel2016) {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "control_command");
            assert_eq!(id, "edit.paste");
            assert_eq!(referenced_by, "PasteGallery");
        }
        other => panic!("expected a control_command cross-reference error, got {other:?}"),
    }
}

#[test]
fn unknown_shortcut_command_is_rejected() {
    let loader = Loader::new(fixtures().join("xref-shortcut")).unwrap();
    match loader.load(LayoutKind::Panel2016) {
        Err(LoadError::CrossReference {
            kind,
            id,
            referenced_by,
        }) => {
            assert_eq!(kind, "command_id");
            assert_eq!(id, "app.quit");
            assert_eq!(referenced_by, "Ctrl+Q");
        }
        other => panic!("expected a command_id cross-reference error, got {other:?}"),
    }
}

#[test]
fn json_data_files_load_too() {
    let dir = temp_workspace("json");
    let data = dir.join("ui-spec");
    let layout = r#"{
        "schema_version": 1,
        "product": "visio",
        "source": "JSON fixture",
        "source_url": "https://example.com/fixture/json",
        "source_revision": "fixture",
        "tabs": [],
        "backstage": {"kind": "backstage", "tabs": []},
        "context_menus": [],
        "quick_access_toolbar": {"id": "qat", "controls": []}
    }"#;
    std::fs::write(data.join("panel-2016.json"), layout).unwrap();
    // The loader reads both layout files on every load.
    std::fs::write(data.join("panel-365.json"), layout).unwrap();
    // Both tables get one entry; the empty-table cases have their
    // own tests (the shortcut dispatches `edit.undo`).
    std::fs::write(
        data.join("commands.json"),
        r#"[{"id": "edit.undo", "label": "Undo", "visuicmd": null, "fluent_ids": [], "mapping_confidence": "unknown"}]"#,
    )
    .unwrap();
    std::fs::write(
        data.join("shortcuts.json"),
        r#"[{"sequence": [["Ctrl", "Z"]], "command": "edit.undo", "scope": "global"}]"#,
    )
    .unwrap();
    copy_good_v2(&dir);

    let loader = Loader::new(&dir).unwrap();
    // No `.yaml` data file exists in this workspace: every read goes
    // through the `.json` fallback of `data_file`.
    for name in [
        "panel-2016.yaml",
        "panel-365.yaml",
        "commands.yaml",
        "shortcuts.yaml",
    ] {
        assert!(!data.join(name).exists(), "{name} must not exist");
    }
    let set = loader.load(LayoutKind::Panel2016).unwrap();
    assert!(set.layout.tabs.is_empty());
    assert_eq!(set.commands.entries.len(), 1);
    assert_eq!(set.shortcuts.entries.len(), 1);
    let sets = loader.load_all().unwrap();
    assert_eq!(&sets.get(LayoutKind::Panel2016).commands, &set.commands);
    assert_eq!(&sets.get(LayoutKind::Panel365).commands, &set.commands);
}

#[test]
fn missing_data_files_report_an_io_error() {
    let dir = temp_workspace("missing");
    let loader = Loader::new(&dir).unwrap();
    match loader.load(LayoutKind::Panel2016) {
        Err(LoadError::Io { path, source }) => {
            assert!(path.ends_with("ui-spec/panel-2016.yaml"));
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected LoadError::Io, got {other:?}"),
    }
}

#[test]
fn wrong_document_kind_is_rejected() {
    // commands.yaml holding a shortcuts table: schema-valid (the
    // schema's oneOf allows any of the three documents at any path)
    // but not what this file must contain.
    let dir = temp_workspace("wrong-kind");
    let data = dir.join("ui-spec");
    std::fs::copy(
        fixtures().join("good/ui-spec/panel-2016.yaml"),
        data.join("panel-2016.yaml"),
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("good/ui-spec/panel-365.yaml"),
        data.join("panel-365.yaml"),
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("good/ui-spec/shortcuts.yaml"),
        data.join("commands.yaml"),
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("good/ui-spec/shortcuts.yaml"),
        data.join("shortcuts.yaml"),
    )
    .unwrap();

    let loader = Loader::new(&dir).unwrap();
    match loader.load(LayoutKind::Panel2016) {
        Err(LoadError::Unknown { path, message }) => {
            assert!(path.ends_with("ui-spec/commands.yaml"), "path: {path:?}");
            assert!(message.contains("commands"), "message: {message}");
        }
        other => panic!("expected LoadError::Unknown, got {other:?}"),
    }
}

#[test]
fn duplicate_command_id_is_rejected() {
    // commands.yaml defines `file.save` twice (P1-14 goal 1).
    let loader = Loader::new(fixtures().join("duplicate-command")).unwrap();
    let results = [
        loader.load(LayoutKind::Panel2016).map(|_| ()),
        loader.load_all().map(|_| ()),
    ];
    for loader_result in results {
        match loader_result {
            Err(LoadError::Duplicate { path, kind, id }) => {
                assert!(path.ends_with("ui-spec/commands.yaml"), "path: {path:?}");
                assert_eq!(kind, "command_id");
                assert_eq!(id, "file.save");
            }
            other => panic!("expected LoadError::Duplicate, got {other:?}"),
        }
    }
}

#[test]
fn duplicate_fluent_id_currently_loads() {
    // The 2016 fixture uses FileSaveQuickAccess as a primary id
    // twice. The committed data reuses Fluent ids across surfaces the
    // same way (see docs/tasks/P1-14.md, Open issues), so the loader
    // does not reject layout-side duplicates yet. This test pins that
    // behaviour until the master decides the policy.
    let loader = Loader::new(fixtures().join("duplicate-fluent")).unwrap();
    let set = loader.load(LayoutKind::Panel2016).expect("loads today");
    assert!(
        set.command_lookup()
            .labels
            .contains_key(&FluentId::new("FileSaveQuickAccess"))
    );
}

#[test]
fn controls_per_layout_are_stable() {
    // Counts pinned to the fixtures: 8 controls in the 2016 fixture
    // layout, 9 in the 365 one (RedoButton is 365-only;
    // FontSizeLabel, with `command: null`, is in both).
    let loader = Loader::new(fixtures().join("good")).unwrap();
    let count = |set: &LayoutSet| {
        let mut n = set.layout.quick_access_toolbar.controls.len();
        for tab in &set.layout.tabs {
            for group in &tab.groups {
                n += group.controls.len();
            }
        }
        for tab in &set.layout.backstage.tabs {
            n += tab.controls.len();
        }
        for menu in &set.layout.context_menus {
            n += menu.controls.len();
        }
        n
    };
    assert_eq!(count(&loader.load(LayoutKind::Panel2016).unwrap()), 8);
    assert_eq!(count(&loader.load(LayoutKind::Panel365).unwrap()), 9);
}
