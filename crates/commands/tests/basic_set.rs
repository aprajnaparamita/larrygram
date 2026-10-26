//! Acceptance tests for task P7-20: the basic set (File, Edit, View
//! and Pages) is reachable from both committed layouts and from the
//! keyboard, every command has a real handler, and each one runs.
//!
//! Reachability is data-level: the Panel Modern and Panel Classic
//! layouts link their controls to command ids, `shortcuts.yaml` binds
//! the chords, and the registry resolves every id to
//! `CommandStatus::Implemented`. Running is dispatch-level: one
//! `Registry::execute` per command over a prepared session.

use std::collections::BTreeSet;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandId, CommandStatus, Invocation, Outcome, Source};
use larrygram_ui_spec::{LayoutKind, Loader};

/// The basic set of the task: the ids that must be implemented.
const BASIC_SET: &[&str] = &[
    // File
    "file.new",
    "file.new.blank.drawing",
    "file.new.blank.drawing.metric",
    "file.new.blank.drawing.us",
    "file.open",
    "file.save",
    "file.save_as",
    "file.close",
    "file.export",
    "tools.options.page.setup",
    // Edit
    "edit.undo",
    "edit.redo",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.paste.in.place",
    "edit.duplicate",
    "edit.paste.duplicate",
    "edit.delete",
    "edit.clear",
    "edit.select_all",
    "edit.deselect",
    // View
    "view.zoom.in",
    "view.zoom.out",
    "view.view50",
    "view.view75",
    "view.view100",
    "view.view150",
    "view.view200",
    "view.view400",
    "shape.draw.zoom",
    "view.zoom.dialog",
    "view.custom",
    "view.zoom.fit",
    "view.fit.in.window",
    "view.zoom.page.width",
    "view.toggle_grid",
    "view.rulers",
    "view.guides",
    "view.toggle_guides",
    "view.show.page.breaks",
    "view.page.breaks",
    "view.full_screen_mode",
    // Pages
    "page.add",
    "tools.options.new.page",
    "tools.blank.page.insert",
    "tools.blank.page.insert.visio",
    "page.delete",
    "page.rename",
    "edit.duplicate.page",
    "page.reorder",
    "page.reorder_pages",
    "page.move_left",
    "page.move_right",
    "page.next",
    "page.previous",
    "tools.options.go.to.drawing",
    "insert.new.background.page",
    "tools.backgrounds",
    "format.set.page.orientation",
    "page.size.dlg",
    // Tools
    "tools.pointer",
    "tools.rectangle",
    "tools.ellipse",
    "shape.line",
    "tools.text",
    "tools.connector",
];

/// The controls that must appear in the Panel Classic layout (the
/// `command` link each one carries is checked against the registry).
const CLASSIC_CONTROLS: &[&str] = &[
    "file.new.blank.drawing",
    "file.open",
    "file.save",
    "file.save_as",
    "file.close",
    "edit.undo",
    "edit.redo",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.clear",
    "edit.select_all",
    "edit.duplicate",
    "view.rulers",
    "view.toggle_grid",
    "view.toggle_guides",
    "view.page.breaks",
    "view.fit.in.window",
    "view.zoom.page.width",
    "view.view100",
    "shape.draw.zoom",
    "view.full_screen_mode",
    "tools.options.page.setup",
    "tools.options.new.page",
    "page.delete",
    "page.rename",
    "page.reorder_pages",
    "tools.options.go.to.drawing",
    "tools.pointer",
    "tools.connector",
    "tools.text",
    "shape.rectangle",
    "shape.ellipse",
    "shape.line",
];

/// The controls that must appear in the Panel Modern layout.
const MODERN_CONTROLS: &[&str] = &[
    "file.new",
    "file.open",
    "file.save",
    "file.save_as",
    "file.close",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.paste.duplicate",
    "edit.select_all",
    "view.rulers",
    "view.toggle_grid",
    "view.guides",
    "view.show.page.breaks",
    "view.zoom.fit",
    "view.zoom.page.width",
    "view.full_screen_mode",
    "tools.blank.page.insert",
    "tools.blank.page.insert.visio",
    "insert.new.background.page",
    "edit.duplicate.page",
    "page.delete",
    "page.rename",
    "page.reorder_pages",
    "tools.options.page.setup",
    "tools.options.new.page",
    "tools.backgrounds",
    "format.set.page.orientation",
    "page.size.dlg",
    "tools.pointer",
    "tools.connector",
    "tools.text",
    "tools.rectangle",
    "tools.ellipse",
    "shape.line",
];

/// Every command id the two layouts link (`command` on a control),
/// collected over the whole v2 layout tree.
#[allow(clippy::expect_used)] // test helper: the embedded data must load
fn linked_commands(kind: LayoutKind) -> BTreeSet<String> {
    let sets = Loader::from_embedded().expect("the embedded data loads");
    let value = serde_json::to_value(sets.get(kind).v2.clone()).expect("the layout serialises");
    let mut out = BTreeSet::new();
    collect_commands(&value, &mut out);
    out
}

/// Recursing over the serialized layout: every `command` string.
fn collect_commands(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(command)) = map.get("command") {
                out.insert(command.clone());
            }
            for entry in map.values() {
                collect_commands(entry, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_commands(item, out);
            }
        }
        _ => {}
    }
}

/// The commands one layout's shortcuts dispatch, keyed by the flattened
/// chord (`"Ctrl+N"`).
#[allow(clippy::expect_used)] // test helper
fn shortcut_map(kind: LayoutKind) -> std::collections::BTreeMap<String, String> {
    let sets = Loader::from_embedded().expect("the embedded data loads");
    sets.get(kind)
        .shortcuts
        .entries
        .iter()
        .map(|shortcut| {
            let chord = shortcut
                .sequence
                .iter()
                .map(|chord| chord.join("+"))
                .collect::<Vec<_>>()
                .join(",");
            (chord, shortcut.command.as_str().to_owned())
        })
        .collect()
}

/// Every basic-set id resolves to an implemented command in the
/// registry of both layouts' shared data.
#[test]
fn every_basic_set_command_is_implemented() {
    let registry = registry().expect("registry");
    for id in BASIC_SET {
        let id = CommandId::parse(id).expect("basic-set id");
        assert_eq!(
            registry.status(&id),
            CommandStatus::Implemented,
            "{id} must have a real handler"
        );
    }
}

/// Both layouts link their basic-set controls to implemented commands
/// (task P7-20 goal 2: reachable from both layouts).
#[test]
fn the_layouts_link_the_basic_set() {
    let registry = registry().expect("registry");
    for (kind, controls) in [
        (LayoutKind::PanelClassic, CLASSIC_CONTROLS),
        (LayoutKind::PanelModern, MODERN_CONTROLS),
    ] {
        let linked = linked_commands(kind);
        for id in controls {
            assert!(linked.contains(*id), "{kind:?} must link {id} to a control");
            let id = CommandId::parse(id).expect("control id");
            assert_eq!(
                registry.status(&id),
                CommandStatus::Implemented,
                "{kind:?} control {id} must be implemented"
            );
        }
    }
}

/// The keyboard chords of the basic set resolve to implemented
/// commands (task P7-20 goal 2: and its keyboard shortcut).
#[test]
fn the_basic_set_shortcuts_dispatch_implemented_commands() {
    let registry = registry().expect("registry");
    let expected: &[(&str, &str)] = &[
        ("Ctrl+N", "file.new.blank.drawing"),
        ("Ctrl+O", "file.open"),
        ("Ctrl+S", "file.save"),
        ("Ctrl+Z", "edit.undo"),
        ("Ctrl+Y", "edit.redo"),
        ("Ctrl+X", "edit.cut"),
        ("Ctrl+C", "edit.copy"),
        ("Ctrl+V", "edit.paste"),
        ("Delete", "edit.clear"),
        ("Ctrl+A", "edit.select_all"),
        ("Ctrl+D", "edit.duplicate"),
        ("Ctrl+Alt+P", "page.reorder_pages"),
        ("Ctrl+1", "tools.pointer"),
        ("Ctrl+2", "tools.text"),
        ("Ctrl+3", "tools.connector"),
        ("Ctrl+6", "shape.line"),
        ("Ctrl+8", "shape.rectangle"),
        ("Ctrl+9", "shape.ellipse"),
    ];
    for kind in [LayoutKind::PanelClassic, LayoutKind::PanelModern] {
        let map = shortcut_map(kind);
        for (chord, command) in expected {
            assert_eq!(
                map.get(*chord).map(String::as_str),
                Some(*command),
                "{kind:?}: {chord} must dispatch {command}"
            );
            let id = CommandId::parse(command).expect("command id");
            assert_eq!(
                registry.status(&id),
                CommandStatus::Implemented,
                "{command} behind {chord}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The smoke run: every basic-set command dispatches.
// ---------------------------------------------------------------------------

/// Dispatches one command over the registry.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn run(
    ctx: &mut AppCtx,
    id: &'static str,
    args: Args,
) -> Result<Outcome, larrygram_commands::CommandError> {
    let registry = registry().expect("registry");
    let id = CommandId::parse(id).expect("id");
    let invocation = Invocation::new(args, Source::Test);
    larrygram_commands::dispatch(&registry, ctx, &id, invocation)
}

/// One number argument.
fn number(name: &str, value: f64) -> Args {
    Args::new().with(name, ArgValue::Number(value))
}

/// One text argument.
fn text(name: &str, value: &str) -> Args {
    Args::new().with(name, ArgValue::Text(value.to_owned()))
}

/// A session with one shape selected (so the edit commands can run).
#[allow(clippy::expect_used)] // test helper
fn prepared() -> AppCtx {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new.blank.drawing", Args::new()).expect("new drawing");
    let session = ctx.active_document_mut().expect("session");
    let page = session.active_page.expect("page");
    let shape = session
        .document
        .insert_shape(
            page,
            larrygram_model::Shape::new(larrygram_model::ShapeId { index: 0 }, 1),
            None,
        )
        .expect("shape");
    ctx.selection.shapes = vec![shape];
    ctx.selection.anchor = Some(shape);
    ctx.selection.page = Some(page);
    ctx
}

/// Every command of the basic set runs over a prepared session (task
/// P7-20 acceptance: "every command of the basic set runs"), with the
/// arguments its contract takes.
#[test]
fn every_basic_set_command_runs() {
    let dir = std::env::temp_dir().join(format!(
        "larrygram-basic-set-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let out = dir.join("png");

    // The file commands.
    let mut ctx = AppCtx::new(Settings::default());
    for id in [
        "file.new",
        "file.new.blank.drawing",
        "file.new.blank.drawing.metric",
        "file.new.blank.drawing.us",
    ] {
        run(&mut ctx, id, Args::new()).expect(id);
    }
    let save = Args::new().with("path", ArgValue::Path(dir.join("plan.vsdx")));
    run(&mut ctx, "file.save_as", save).expect("file.save_as");
    run(&mut ctx, "file.save", Args::new()).expect("file.save");
    let export = Args::new()
        .with("path", ArgValue::Path(dir.join("plan.vsdx")))
        .with("out", ArgValue::Path(out.clone()));
    run(&mut ctx, "file.export", export).expect("file.export");
    assert!(out.join("page-1.png").is_file(), "the export wrote the PNG");
    let svg = dir.join("svg");
    let export_svg = Args::new()
        .with("path", ArgValue::Path(dir.join("plan.vsdx")))
        .with("out", ArgValue::Path(svg.clone()))
        .with("format", ArgValue::Text("svg".to_owned()));
    run(&mut ctx, "file.export", export_svg).expect("file.export svg");
    assert!(svg.join("page-1.svg").is_file(), "the export wrote the SVG");
    run(&mut ctx, "file.close", Args::new()).expect("file.close");

    // The edit commands, with an undo and a redo of one delete.
    let mut ctx = prepared();
    run(&mut ctx, "edit.select_all", Args::new()).expect("edit.select_all");
    run(&mut ctx, "edit.copy", Args::new()).expect("edit.copy");
    run(&mut ctx, "edit.paste", Args::new()).expect("edit.paste");
    run(&mut ctx, "edit.paste.in.place", Args::new()).expect("edit.paste.in.place");
    run(&mut ctx, "edit.duplicate", Args::new()).expect("edit.duplicate");
    run(&mut ctx, "edit.paste.duplicate", Args::new()).expect("edit.paste.duplicate");
    run(&mut ctx, "edit.delete", Args::new()).expect("edit.delete");
    run(&mut ctx, "edit.undo", Args::new()).expect("edit.undo");
    run(&mut ctx, "edit.redo", Args::new()).expect("edit.redo");
    // Each of these needs a selection of its own.
    let mut ctx = prepared();
    run(&mut ctx, "edit.cut", Args::new()).expect("edit.cut");
    let mut ctx = prepared();
    run(&mut ctx, "edit.clear", Args::new()).expect("edit.clear");
    let mut ctx = prepared();
    run(&mut ctx, "edit.deselect", Args::new()).expect("edit.deselect");

    // The view commands.
    let mut ctx = prepared();
    for id in [
        "view.zoom.in",
        "view.zoom.out",
        "view.view50",
        "view.view75",
        "view.view100",
        "view.view150",
        "view.view200",
        "view.view400",
    ] {
        run(&mut ctx, id, Args::new()).expect(id);
    }
    for id in ["shape.draw.zoom", "view.zoom.dialog", "view.custom"] {
        run(&mut ctx, id, number("percent", 125.0)).expect(id);
    }
    let rect = Args::new()
        .with("width", ArgValue::Number(612.0))
        .with("height", ArgValue::Number(792.0));
    run(&mut ctx, "view.zoom.fit", rect.clone()).expect("view.zoom.fit");
    run(&mut ctx, "view.fit.in.window", rect).expect("view.fit.in.window");
    run(&mut ctx, "view.zoom.page.width", number("width", 816.0)).expect("view.zoom.page.width");
    for id in [
        "view.toggle_grid",
        "view.rulers",
        "view.guides",
        "view.toggle_guides",
        "view.show.page.breaks",
        "view.page.breaks",
        "view.full_screen_mode",
    ] {
        run(&mut ctx, id, Args::new()).expect(id);
    }

    // The page commands.
    let mut ctx = prepared();
    run(&mut ctx, "page.add", Args::new()).expect("page.add");
    run(&mut ctx, "tools.options.new.page", Args::new()).expect("tools.options.new.page");
    run(&mut ctx, "tools.blank.page.insert", Args::new()).expect("tools.blank.page.insert");
    run(&mut ctx, "tools.blank.page.insert.visio", Args::new())
        .expect("tools.blank.page.insert.visio");
    run(&mut ctx, "edit.duplicate.page", Args::new()).expect("edit.duplicate.page");
    run(&mut ctx, "page.rename", text("name", "Flow")).expect("page.rename");
    let move_by = Args::new()
        .with("from", ArgValue::Integer(2))
        .with("to", ArgValue::Integer(0));
    run(&mut ctx, "page.reorder", move_by.clone()).expect("page.reorder");
    run(&mut ctx, "page.reorder_pages", move_by).expect("page.reorder_pages");
    run(&mut ctx, "page.move_left", Args::new()).expect("page.move_left");
    run(&mut ctx, "page.move_right", Args::new()).expect("page.move_right");
    run(&mut ctx, "page.next", Args::new()).expect("page.next");
    run(&mut ctx, "page.previous", Args::new()).expect("page.previous");
    run(&mut ctx, "tools.options.go.to.drawing", number("page", 1.0)).expect("go to page");
    run(&mut ctx, "insert.new.background.page", Args::new()).expect("background page");
    run(&mut ctx, "tools.backgrounds", text("background", "Page-1")).expect("tools.backgrounds");
    let setup = Args::new()
        .with("width", ArgValue::Number(8.5))
        .with("height", ArgValue::Number(11.0))
        .with("orientation", ArgValue::Text("portrait".to_owned()))
        .with("units", ArgValue::Text("inches".to_owned()));
    run(&mut ctx, "tools.options.page.setup", setup).expect("page setup");
    run(
        &mut ctx,
        "format.set.page.orientation",
        text("orientation", "landscape"),
    )
    .expect("orientation");
    let size = Args::new()
        .with("width", ArgValue::Number(11.0))
        .with("height", ArgValue::Number(8.5));
    run(&mut ctx, "page.size.dlg", size).expect("page.size.dlg");
    run(&mut ctx, "page.delete", Args::new()).expect("page.delete");

    // The tools.
    let mut ctx = prepared();
    for id in [
        "tools.pointer",
        "tools.rectangle",
        "tools.ellipse",
        "shape.line",
        "tools.text",
        "tools.connector",
    ] {
        run(&mut ctx, id, Args::new()).expect(id);
    }

    std::fs::remove_dir_all(&dir).expect("clean up");
}
