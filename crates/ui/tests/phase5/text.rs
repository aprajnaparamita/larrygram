//! In-place text editing tests (task P5-07, ADR 0044, ADR 0049).
//!
//! The real `TextEditor` is drawn in an `egui_kittest` frame, typed
//! into, and committed through the real registry; the text tool's
//! insert-and-begin intent pair is driven through the real
//! `ToolMachine` and a fake insert command (P5-06 owns the real one).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Source};
use larrygram_model::{
    Document, PageId, Row, Section, SectionName, Shape, ShapeId, TextContent, TextPart,
};
use larrygram_ui::shell::dispatch_one;
use larrygram_ui::shortcuts::{self, ShortcutMap};
use larrygram_ui::text_edit::{EditorAction, TextEditor};
use larrygram_ui::tools::{Modifiers, Tool, ToolEvent, ToolMachine};
use larrygram_ui_spec::{Loader, ShortcutScope};

/// A page with one shape whose text has the first-row markers.
fn fixture() -> (AppCtx, ShapeId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let mut shape = Shape::new(ShapeId { index: 0 }, 1);
    let mut section = Section::new(SectionName::from_static("Character"));
    section.rows.push(Row::new());
    shape.sheet.sections.push(section);
    let shape = doc.insert_shape(page, shape, None).expect("shape");
    let old = TextContent {
        parts: vec![
            TextPart::CharRun { ix: 0 },
            TextPart::Text("old".to_owned()),
        ],
    };
    doc.set_shape_text(shape, old).expect("text");
    let mut app = AppCtx::new(crate::phase5_settings());
    app.open_document(DocumentSession::new(doc, None));
    app.selection.shapes = vec![shape];
    app.selection.anchor = Some(shape);
    app.selection.page = Some(PageId { index: 0 });
    (app, shape)
}

fn shape_text(app: &AppCtx, shape: ShapeId) -> Option<TextContent> {
    app.active_document()
        .and_then(|session| session.document.shape(shape))
        .and_then(|shape| shape.text.clone())
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn typing_into_the_editor_writes_the_shape_text() {
    let (app, shape) = fixture();
    let original = shape_text(&app, shape).expect("text");
    let editor = Rc::new(RefCell::new(TextEditor::new()));
    editor.borrow_mut().begin(shape, &original).expect("begin");
    let action = Rc::new(RefCell::new(EditorAction::Pending));
    let intents = Rc::new(RefCell::new(Vec::new()));
    let app = Rc::new(RefCell::new(app));
    let registry = larrygram_commands::builtins::registry().expect("registry");
    let registry = Rc::new(registry);
    let sets = Loader::from_embedded().expect("layouts");
    let mut map = ShortcutMap::from_shortcuts(&sets.panel_2016.shortcuts).expect("map");
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(220.0, 60.0));
    let harness_editor = Rc::clone(&editor);
    let harness_action = Rc::clone(&action);
    let harness_app = Rc::clone(&app);
    let harness_registry = Rc::clone(&registry);
    let harness_intents = Rc::clone(&intents);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui(move |ui| {
            let app = harness_app.borrow_mut();
            let produced = shortcuts::dispatch(ui.ctx(), &mut map, &app, ShortcutScope::Global)
                .expect("dispatch");
            harness_intents.borrow_mut().extend(produced);
            let mut editor = harness_editor.borrow_mut();
            let produced = editor.show(ui, rect);
            if !matches!(produced, EditorAction::Pending) {
                *harness_action.borrow_mut() = produced;
            }
            drop(editor);
            drop(app);
            let _ = harness_registry;
        });
    harness.run();
    // The first frame requested focus; now typing reaches the text field.
    harness.event(egui::Event::Text("!".to_owned()));
    harness.run();

    let typed = editor.borrow().buffer.clone();
    assert_ne!(typed, "old", "the typed character reached the editor");
    let written = editor
        .borrow_mut()
        .finish()
        .expect("finish")
        .expect("changed");
    let mut app = app.borrow_mut();
    let text_arg = match written.parts.last() {
        Some(TextPart::Text(text)) => text.clone(),
        _ => panic!("the one run carries literal text"),
    };
    let args = larrygram_commands::Args::new()
        .with("text", larrygram_commands::ArgValue::Text(text_arg.clone()));
    let id = CommandId::parse("shape.set.text").expect("id");
    dispatch_one(
        &registry,
        &mut app,
        &id,
        Invocation::new(args, Source::Test),
    )
    .expect("the command writes");
    assert_eq!(
        shape_text(&app, shape),
        Some(TextContent {
            parts: vec![TextPart::CharRun { ix: 0 }, TextPart::Text(text_arg),],
        })
    );
}

#[test]
fn the_text_shortcut_scope_is_active_while_the_editor_is_focused() {
    let (app, shape) = fixture();
    let original = shape_text(&app, shape).expect("text");
    let editor = Rc::new(RefCell::new(TextEditor::new()));
    editor.borrow_mut().begin(shape, &original).expect("begin");
    let app = Rc::new(RefCell::new(app));
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sets = Loader::from_embedded().expect("layouts");
    let mut map = ShortcutMap::from_shortcuts(&sets.panel_2016.shortcuts).expect("map");
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(220.0, 60.0));
    let harness_editor = Rc::clone(&editor);
    let harness_app = Rc::clone(&app);
    let harness_intents = Rc::clone(&intents);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui(move |ui| {
            let app = harness_app.borrow();
            let produced = shortcuts::dispatch(ui.ctx(), &mut map, &app, ShortcutScope::Global)
                .expect("dispatch");
            harness_intents.borrow_mut().extend(produced);
            drop(app);
            harness_editor.borrow_mut().show(ui, rect);
        });
    harness.run();
    harness.event(key_event(
        egui::Key::B,
        egui::Modifiers {
            ctrl: true,
            ..egui::Modifiers::default()
        },
    ));
    harness.run();
    let intents = intents.borrow();
    assert!(
        intents
            .iter()
            .any(|intent| intent.command.as_str() == "format.text.bold"),
        "the Text scope fired inside the editor: {:?}",
        intents
            .iter()
            .map(|intent| intent.command.as_str())
            .collect::<Vec<_>>()
    );
    // The editor is still active and focused after a non-editing chord.
    assert!(editor.borrow().is_active());
}

#[test]
fn escape_commits_and_leaves_the_editor_idle() {
    let (app, shape) = fixture();
    let original = shape_text(&app, shape).expect("text");
    let editor = Rc::new(RefCell::new(TextEditor::new()));
    editor.borrow_mut().begin(shape, &original).expect("begin");
    editor.borrow_mut().buffer = "changed".to_owned();
    let action = Rc::new(RefCell::new(EditorAction::Pending));
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(220.0, 60.0));
    let harness_editor = Rc::clone(&editor);
    let harness_action = Rc::clone(&action);
    let _app = app;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui(move |ui| {
            let produced = harness_editor.borrow_mut().show(ui, rect);
            if !matches!(produced, EditorAction::Pending) {
                *harness_action.borrow_mut() = produced;
            }
        });
    harness.run();
    harness.event(key_event(egui::Key::Escape, egui::Modifiers::default()));
    harness.run();
    assert!(!editor.borrow().is_active());
    match &*action.borrow() {
        EditorAction::Committed(Some(text)) => {
            assert_eq!(
                text.parts.last(),
                Some(&TextPart::Text("changed".to_owned()))
            );
        }
        other => panic!("expected a commit, got {other:?}"),
    }
}

#[test]
fn the_text_tool_click_defers_the_box_and_the_commit_inserts_it_with_text() {
    let (app, _shape) = fixture();
    let registry = larrygram_commands::builtins::registry().expect("builtins");
    let mut app = app;
    let mut machine = ToolMachine::new();
    machine.tool = Tool::Text;
    let effect = machine
        .handle(
            ToolEvent::PointerUp {
                pos: larrygram_geom::Point::new(2.0, 3.0),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("the text tool handles the release");
    // Task P7-25 item 9: the click starts a pending text box and
    // inserts nothing until the text commits (one undo step).
    assert!(effect.intents.is_empty());
    let pending = machine.take_pending_text_box().expect("a pending box");
    assert_eq!((pending.x, pending.y), (2.0, 3.0));
    assert_eq!(
        (pending.width, pending.height),
        (
            larrygram_ui::tools::text::TEXT_BOX_WIDTH_IN,
            larrygram_ui::tools::text::TEXT_BOX_HEIGHT_IN,
        )
    );
    // The commit the shell builds for it (task P7-25 item 9): one
    // `insert.drawing.text_box` carrying the typed text.
    let args = Args::new()
        .with("x", ArgValue::Number(pending.x))
        .with("y", ArgValue::Number(pending.y))
        .with("width", ArgValue::Number(pending.width))
        .with("height", ArgValue::Number(pending.height))
        .with("text", ArgValue::Text("Hi".to_owned()));
    let command = CommandId::parse("insert.drawing.text_box").expect("a valid command id");
    dispatch_one(
        &registry,
        &mut app,
        &command,
        Invocation::new(args, Source::Test),
    )
    .expect("the insert runs");
    // P5-06's `insert.drawing.text_box` selects the new box.
    let shape = app.selection.anchor.expect("the insert selected the box");
    assert_ne!(shape, _shape, "a new shape, not the fixture's");
    assert_eq!(app.selection.shapes, vec![shape]);
    let written = shape_text(&app, shape).expect("the new box has text");
    assert!(
        written
            .parts
            .iter()
            .any(|part| matches!(part, TextPart::Text(text) if text == "Hi")),
        "the box is created with its text: {written:?}"
    );
}

// ---------------------------------------------------------------------------
// Commit and cancel through the real shell (task P5-11, P5-17's wiring)
// ---------------------------------------------------------------------------

/// One `text.begin` / `text.commit` / `text.cancel` dispatch through the
/// shell (the way the menu, command bar and shortcuts reach the editor).
fn shell_dispatch(shell: &Rc<RefCell<larrygram_ui::shell::Shell>>, id: &'static str) {
    let command = CommandId::parse(id).expect("a valid command id");
    let intent = larrygram_ui::Intent::new(command, Invocation::empty(Source::Test));
    let mut shell = shell.borrow_mut();
    let mut results = shell.dispatch(&[intent]).expect("the intent dispatches");
    let _ = results.pop().expect("one result per intent");
}

/// The text edit through the whole shell: `text.begin` opens the editor
/// on the selection (P5-17), typing reaches it, `text.cancel` discards
/// the buffer and Escape commits it (ADR 0044).
#[test]
fn the_text_edit_cancels_and_commits_through_the_shell() {
    let (app, shape) = fixture();
    let original = shape_text(&app, shape).expect("text");
    let registry = larrygram_commands::builtins::registry().expect("registry");
    let sets = Loader::from_embedded().expect("layouts");
    let shell = Rc::new(RefCell::new(larrygram_ui::shell::Shell::new(
        registry, sets, app,
    )));
    let harness_shell = Rc::clone(&shell);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui(move |ui| {
            let mut shell = harness_shell.borrow_mut();
            let intents = shell.show_in(ui).expect("the frame renders");
            let _ = shell.dispatch(&intents).expect("the intents dispatch");
        });
    harness.run();

    // Cancel: the typed text never reaches the shape.
    shell_dispatch(&shell, "text.begin");
    harness.run();
    harness.event(egui::Event::Text("dropped".to_owned()));
    harness.run();
    shell_dispatch(&shell, "text.cancel");
    harness.run();
    assert_eq!(
        shape_text(&shell.borrow().app, shape),
        Some(original.clone()),
        "text.cancel discards the buffer"
    );

    // Commit: Escape commits the buffer (ADR 0044).
    shell_dispatch(&shell, "text.begin");
    harness.run();
    harness.event(egui::Event::Text("typed".to_owned()));
    harness.run();
    harness.event(key_event(egui::Key::Escape, egui::Modifiers::default()));
    harness.run();
    let written = shape_text(&shell.borrow().app, shape).expect("the commit wrote text");
    assert!(
        written
            .parts
            .iter()
            .any(|part| matches!(part, TextPart::Text(text) if text.contains("typed"))),
        "the typed text reached the shape: {written:?}"
    );
    assert_ne!(written, original, "the shape's text changed");
}
