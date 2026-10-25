//! Tool commands, the Shapes-window drag and the in-place editor through
//! the real [`Shell`] (task P5-17, ADR 0049).
//!
//! A shortcut selects the rectangle tool and a canvas drag draws one
//! rectangle; a Shapes-window row drag drops one master instance, and
//! Escape cancels it; a double click edits text in one undo step and a
//! group still enters.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::AppCtx;
use larrygram_commands::{
    ActiveTool, Command, CommandError, CommandId, CommandMeta, CommandStatus, Enabled, Invocation,
    Outcome, Registry, builtins,
};
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::Loader;

use crate::{PAGE_HEIGHT, app_with_one_shape};

const RULER: f32 = 16.0;
/// The shell canvas shows the whole page at this zoom.
const SHELL_ZOOM: f64 = 0.25;

/// A shell driven by `egui_kittest` with the real registry and layouts.
struct ShellRig {
    shell: Rc<RefCell<Shell>>,
    harness: Harness<'static>,
}

impl ShellRig {
    fn new(app: AppCtx) -> Self {
        Self::with_registry(builtins::registry().expect("registry"), app)
    }

    /// The same rig over a caller-chosen registry (the connector tests
    /// replace `connector.draw`).
    fn with_registry(registry: Registry, mut app: AppCtx) -> Self {
        if let Some(session) = app.active_document_mut() {
            session.view.zoom = SHELL_ZOOM;
        }
        let shell = Rc::new(RefCell::new(Shell::new(
            registry,
            Loader::from_embedded().expect("layouts"),
            app,
        )));
        let harness_shell = Rc::clone(&shell);
        let harness = Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .with_accessibility_check(false)
            .with_context_setup(|ctx| {
                // The harness advances its clock between internal steps;
                // keep two deliberate clicks in one double-click window.
                ctx.options_mut(|options| {
                    options.input_options.max_double_click_delay = 10.0;
                });
            })
            .build_ui(move |ui| {
                let mut shell = harness_shell.borrow_mut();
                let intents = shell.show_in(ui).expect("the frame renders");
                let _ = shell.dispatch(&intents).expect("the intents dispatch");
            });
        let mut rig = Self { shell, harness };
        rig.run();
        rig
    }

    fn run(&mut self) {
        self.harness.run();
    }

    /// Dispatches one command by id between frames (the group-entry
    /// commands of the entered-group walk).
    fn dispatch(&self, id: &'static str) {
        let command = CommandId::parse(id).expect("id");
        let intent =
            larrygram_ui::Intent::new(command, Invocation::empty(larrygram_commands::Source::Ui));
        let mut shell = self.shell.borrow_mut();
        let results = shell.dispatch(&[intent]).expect("dispatch");
        for result in results {
            let outcome = result.expect("the command runs");
            let _ = outcome;
        }
    }

    fn key(&mut self, modifiers: egui::Modifiers, key: egui::Key) {
        self.harness.key_down_modifiers(modifiers, key);
        self.run();
        self.harness.key_up_modifiers(modifiers, key);
        self.run();
    }

    fn press_at(&mut self, pos: egui::Pos2) {
        self.harness.event(egui::Event::PointerMoved(pos));
        self.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
        self.run();
    }

    fn move_at(&mut self, pos: egui::Pos2) {
        self.harness.event(egui::Event::PointerMoved(pos));
        self.run();
    }

    fn release_at(&mut self, pos: egui::Pos2) {
        self.harness.event(egui::Event::PointerMoved(pos));
        self.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        });
        self.run();
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.press_at(from);
        self.move_at(to);
        self.release_at(to);
    }

    fn double_click(&mut self, pos: egui::Pos2) {
        self.harness.event(egui::Event::PointerMoved(pos));
        for _ in 0..2 {
            for pressed in [true, false] {
                self.harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
            }
        }
        self.run();
        // One more frame: `text.begin` ran after the double click's
        // frame, so the editor requests focus now.
        self.run();
    }

    fn canvas_rect(&self) -> egui::Rect {
        self.shell.borrow().canvas_rect().expect("the canvas drew")
    }

    /// The screen point of a page point in the shell's canvas.
    fn screen(&self, x: f64, y: f64) -> egui::Pos2 {
        let rect = self.canvas_rect();
        let viewport = egui::Rect::from_min_max(rect.min + egui::vec2(RULER, RULER), rect.max);
        let k = 96.0 * SHELL_ZOOM;
        egui::pos2(
            viewport.min.x + (x * k) as f32,
            viewport.min.y + ((PAGE_HEIGHT - y) * k) as f32,
        )
    }

    fn page_shape_count(&self) -> usize {
        let shell = self.shell.borrow();
        let session = shell.app.active_document().expect("session");
        let page = session.active_page.expect("page");
        session
            .document
            .page(page)
            .map(|page| page.shapes.len())
            .unwrap_or(0)
    }

    fn undo_len(&self) -> usize {
        self.shell
            .borrow()
            .app
            .active_document()
            .expect("session")
            .undo
            .len()
    }

    /// The center of the first Shapes-window row named `name` (the row
    /// buttons sit in the left pane).
    fn shapes_row(&self, name: &str) -> egui::Pos2 {
        self.harness
            .root()
            .query_all_by_role_and_label(Role::Button, name)
            .map(|node| node.rect().center())
            .find(|center| center.x < 300.0)
            .unwrap_or_else(|| panic!("no Shapes-window row named {name:?}"))
    }
}

#[test]
fn a_shortcut_picks_the_rectangle_tool_and_a_drag_draws_one_rectangle() {
    let (app, _shape) = app_with_one_shape();
    let before = {
        let session = app.active_document().expect("session");
        session
            .document
            .page(session.active_page.expect("page"))
            .expect("page")
            .shapes
            .len()
    };
    let mut rig = ShellRig::new(app);
    rig.key(egui::Modifiers::CTRL, egui::Key::Num8);
    assert_eq!(
        rig.shell.borrow().active_tool(),
        ActiveTool::Rectangle,
        "Ctrl+8 selects the rectangle tool through the registry"
    );
    let from = rig.screen(1.0, 1.0);
    let to = rig.screen(3.0, 2.0);
    rig.drag(from, to);
    assert_eq!(rig.page_shape_count(), before + 1);
    assert_eq!(rig.undo_len(), 1, "one drag, one undo step");
    assert!(rig.shell.borrow().app.selection.anchor.is_some());
}

#[test]
fn a_shapes_window_drag_drops_one_master_and_escape_cancels() {
    let (app, _shape) = app_with_one_shape();
    let mut rig = ShellRig::new(app);
    let before = rig.page_shape_count();

    let row = rig.shapes_row("Rectangle");
    let target = rig.screen(6.0, 8.0);
    rig.press_at(row);
    rig.move_at(target);
    rig.release_at(target);
    assert_eq!(rig.page_shape_count(), before + 1, "one master dropped");

    let row = rig.shapes_row("Rectangle");
    let target = rig.screen(6.0, 6.0);
    rig.press_at(row);
    rig.move_at(target);
    rig.key(egui::Modifiers::default(), egui::Key::Escape);
    assert_eq!(
        rig.page_shape_count(),
        before + 1,
        "Escape cancels before the release"
    );
    rig.release_at(target);
    assert_eq!(rig.page_shape_count(), before + 1, "Escape drops nothing");
}

fn shape_text(app: &AppCtx, shape: larrygram_model::ShapeId) -> Option<String> {
    let session = app.active_document().expect("session");
    let entry = session.document.shape(shape)?;
    let text = entry.text.as_ref()?;
    Some(
        text.parts
            .iter()
            .filter_map(|part| match part {
                larrygram_model::TextPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect(),
    )
}

#[test]
fn a_double_click_types_text_in_one_undo_step() {
    let (app, shape) = app_with_one_shape();
    let mut rig = ShellRig::new(app);
    let pos = rig.screen(3.0, 3.0);
    rig.double_click(pos);
    assert!(
        rig.shell.borrow().app.selection.anchor == Some(shape),
        "the double click selects the shape"
    );
    rig.harness.event(egui::Event::Text("Hello".to_owned()));
    rig.run();
    rig.key(egui::Modifiers::default(), egui::Key::Escape);
    rig.run();

    let shell = rig.shell.borrow();
    assert_eq!(
        shape_text(&shell.app, shape).as_deref(),
        Some("Hello"),
        "Escape commits the buffer"
    );
    let session = shell.app.active_document().expect("session");
    assert_eq!(session.undo.len(), 1, "one commit, one undo step");
}

#[test]
fn a_double_click_on_a_group_still_enters_it() {
    let (mut app, _shape) = app_with_one_shape();
    let group = {
        let session = app.active_document_mut().expect("session");
        let child = session.document.add_shape(crate::shape(3.0, 3.0, 5));
        let group = session.document.add_shape(crate::shape(3.0, 3.0, 6));
        if let Some(entry) = session.document.shape_mut(group) {
            entry.children.push(child);
        }
        if let Some(entry) = session.document.shape_mut(child) {
            entry.parent = Some(group);
        }
        let page = session.active_page.expect("page");
        if let Some(page) = session.document.pages.get_mut(page.index as usize) {
            page.shapes.push(group);
        }
        group
    };
    let mut rig = ShellRig::new(app);
    let pos = rig.screen(3.0, 3.0);
    rig.double_click(pos);
    let shell = rig.shell.borrow();
    assert_eq!(
        shell.app.selection.entered,
        Some(group),
        "a group double click still enters it"
    );
    assert!(!shell.app.selection.is_empty(), "the group is selected");
}

// --- task P5-20: tool-state edge cases ------------------------------

/// An app whose page carries one nested group over (5, 5): `outer` >
/// `inner` > `member`. The groups draw nothing of their own (an empty
/// sheet: the scene keeps them as group nodes only), so the only shape
/// under the pointer is the member and every hit maps through the walk.
fn app_with_nested_groups() -> (
    AppCtx,
    larrygram_model::ShapeId,
    larrygram_model::ShapeId,
    larrygram_model::ShapeId,
) {
    let (mut app, _base) = app_with_one_shape();
    let (outer, inner, member) = {
        let session = app.active_document_mut().expect("session");
        let member = session.document.add_shape(crate::shape(5.0, 5.0, 5));
        let inner = session.document.add_shape(larrygram_model::Shape::new(
            larrygram_model::ShapeId { index: 0 },
            6,
        ));
        let outer = session.document.add_shape(larrygram_model::Shape::new(
            larrygram_model::ShapeId { index: 0 },
            7,
        ));
        for (child, parent) in [(member, inner), (inner, outer)] {
            if let Some(entry) = session.document.shape_mut(child) {
                entry.parent = Some(parent);
            }
            if let Some(entry) = session.document.shape_mut(parent) {
                entry.children.push(child);
            }
        }
        let page = session.active_page.expect("page");
        if let Some(page) = session.document.pages.get_mut(page.index as usize) {
            page.shapes.push(outer);
        }
        (outer, inner, member)
    };
    (app, outer, inner, member)
}

#[test]
fn an_entered_group_never_traps_the_selection() {
    // The pointer's entered-group hit walk (`tools/pointer.rs`
    // `selectable_shape`): enter a nested group, click a member, click
    // outside, and the selection climbs back out instead of staying
    // stuck inside. The group-entry commands are dispatched directly
    // (the double-click that runs them is covered by
    // `a_double_click_on_a_group_still_enters_it`).
    let (app, outer, inner, member) = app_with_nested_groups();
    let mut rig = ShellRig::new(app);
    let inside = rig.screen(5.0, 5.0);
    let outside = rig.screen(1.0, 1.0);

    // A click at page level maps the deepest hit to the top-level group
    // (the parent climb).
    rig.press_at(inside);
    rig.release_at(inside);
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.app.selection.shapes, vec![outer], "the climb");
        assert_eq!(shell.app.selection.entered, None, "page level");
    }

    // Enter the outer group: the next click maps the hit to the outer
    // group's direct child (the `entered == Some(parent)` return).
    rig.dispatch("shape.enter_group");
    rig.press_at(inside);
    rig.release_at(inside);
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.app.selection.shapes, vec![inner], "the outer's child");
        assert_eq!(shell.app.selection.entered, Some(outer), "inside the outer");
    }

    // Enter the nested group: the next click selects the member and the
    // entered group stays.
    rig.dispatch("shape.enter_group");
    rig.press_at(inside);
    rig.release_at(inside);
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.app.selection.shapes, vec![member], "the member");
        assert_eq!(shell.app.selection.entered, Some(inner), "still inside");
    }

    // A click outside clears the selection and the entered group.
    rig.press_at(outside);
    rig.release_at(outside);
    {
        let shell = rig.shell.borrow();
        assert!(shell.app.selection.is_empty(), "the click outside cleared");
        assert_eq!(shell.app.selection.entered, None, "and left the group");
    }

    // The next click on the same member climbs to the top-level group:
    // the selection is never stuck inside the nested group.
    rig.press_at(inside);
    rig.release_at(inside);
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.app.selection.shapes, vec![outer], "the climb out");
        assert_eq!(shell.app.selection.entered, None, "back at page level");
    }
}

/// The message the connector tool raises when its master lookup finds
/// nothing (`tools/connector.rs`).
const MISSING_MASTER: &str = "the Basic Flowchart Connector master is missing";

/// A `connector.draw` that reports the missing-master failure and counts
/// its runs.
struct MissingConnectorMaster(Arc<AtomicUsize>);

impl Command for MissingConnectorMaster {
    fn meta(&self) -> &CommandMeta {
        use std::sync::OnceLock;
        static META: OnceLock<CommandMeta> = OnceLock::new();
        META.get_or_init(|| {
            CommandMeta::new(
                CommandId::parse("connector.draw").expect("id"),
                "Draw Connector",
                CommandStatus::Implemented,
            )
        })
    }
    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::yes()
    }
    fn execute(&self, _ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(CommandError::failed(MISSING_MASTER))
    }
}

#[test]
fn the_connector_master_missing_error_reaches_the_status_bar_as_one_line() {
    // The connector tool raises `UiError::Io { message: MISSING_MASTER }`
    // when its master lookup finds nothing. The lookup reads the static
    // stencil catalog, which always ships the "Connector" master, so the
    // arm cannot fire from a test (an Open issue of the P5-20 Result);
    // this pins the contract at the seam the error feeds: the one-line
    // message reaches the status bar and nothing is inserted.
    let (mut app, _shape) = app_with_one_shape();
    app.active_tool = ActiveTool::Connector;
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::new();
    registry
        .register(Box::new(MissingConnectorMaster(Arc::clone(&runs))))
        .expect("register");
    let mut rig = ShellRig::with_registry(registry, app);
    let before = rig.page_shape_count();

    // A connector drag long enough to commit (`MIN_DRAG_IN`).
    let from = rig.screen(2.0, 2.0);
    let to = rig.screen(4.0, 2.0);
    rig.drag(from, to);

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the drag reached the command"
    );
    assert_eq!(rig.page_shape_count(), before, "nothing was inserted");
    assert_eq!(rig.undo_len(), 0, "nothing to undo");
    let shell = rig.shell.borrow();
    let warning = shell
        .app
        .warnings
        .last()
        .expect("the error reached the status bar");
    assert!(
        warning.contains(MISSING_MASTER),
        "the status line carries the message: {warning}"
    );
    assert!(
        !warning.contains('\n'),
        "the error is one line: {warning:?}"
    );
}

#[test]
fn a_click_outside_the_text_editor_commits_it() {
    // The black-box pin (P5-07's question): a click outside the editor
    // commits the buffer, like Escape (ADR 0044, `TextEditor::show`).
    let (app, shape) = app_with_one_shape();
    let mut rig = ShellRig::new(app);
    let pos = rig.screen(3.0, 3.0);
    rig.double_click(pos);
    rig.harness.event(egui::Event::Text("Hello".to_owned()));
    rig.run();

    // A click on empty page space, well outside the editor's rect.
    let outside = rig.screen(7.0, 8.0);
    rig.press_at(outside);
    rig.release_at(outside);

    assert_eq!(
        shape_text(&rig.shell.borrow().app, shape).as_deref(),
        Some("Hello"),
        "the outside click committed the buffer"
    );
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    assert_eq!(session.undo.len(), 1, "one commit, one undo step");
}
