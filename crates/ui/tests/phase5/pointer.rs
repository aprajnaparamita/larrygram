//! Pointer tool tests through the real canvas and registry (task P5-05).
//!
//! Click-select, shift-toggle, marquee, one move/resize/rotate drag each,
//! Alt disabling snap, and the keyboard focus scope (ADR 0041/0042).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui_kittest::Harness;
use larrygram_commands::app::AppCtx;
use larrygram_commands::{
    ArgValue, Command, CommandError, CommandId, CommandMeta, CommandStatus, Enabled, Invocation,
    Outcome, Registry,
};
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutSets, Loader, ShortcutScope};

use crate::{CanvasRig, app_with_one_shape, screen, shape_number};

#[test]
fn a_click_selects_the_shape_under_the_pointer() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    assert!(rig.app.borrow().selection.is_empty());
    rig.click(screen(3.0, 3.0));
    {
        let app = rig.app.borrow();
        assert_eq!(app.selection.shapes, vec![id]);
        assert_eq!(app.selection.anchor, Some(id));
        assert_eq!(
            app.selection.page,
            Some(larrygram_model::PageId { index: 0 })
        );
    }
    // The panes show the anchor shape.
    let rows =
        larrygram_ui::panes::size_position_rows(&mut rig.app.borrow_mut()).expect("pane rows");
    assert_eq!(rows[0].label, "Width");
    assert_eq!(rows[0].numeric, Some(2.0));
}

/// An `edit.select` command that counts its runs, to prove the pointer
/// reaches the registry (the real handler is covered in `commands`).
struct CountingSelect(Arc<AtomicUsize>);

impl Command for CountingSelect {
    fn meta(&self) -> &CommandMeta {
        use std::sync::OnceLock;
        static META: OnceLock<CommandMeta> = OnceLock::new();
        META.get_or_init(|| {
            CommandMeta::new(
                CommandId::parse("edit.select").expect("id"),
                "Select",
                CommandStatus::Implemented,
            )
        })
    }
    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::yes()
    }
    fn execute(&self, ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let ids = match inv.args.get("shapes") {
            Some(ArgValue::List(ids)) => ids.clone(),
            _ => Vec::new(),
        };
        let session = ctx
            .active_document()
            .ok_or_else(|| CommandError::failed("no document"))?;
        let page = session
            .active_page
            .ok_or_else(|| CommandError::failed("no page"))?;
        let mut shapes = Vec::new();
        for id in ids {
            let visio_id = id
                .parse::<i32>()
                .map_err(|_| CommandError::invalid_args("bad id"))?;
            if let Some(shape) = session.document.shape_by_visio_id(page, visio_id) {
                shapes.push(shape);
            }
        }
        ctx.selection.anchor = shapes.last().copied();
        ctx.selection.shapes = shapes;
        ctx.selection.page = Some(page);
        Ok(Outcome::message("selected"))
    }
}

#[test]
fn a_click_selection_goes_through_the_registry() {
    let (app, id) = app_with_one_shape();
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::new();
    registry
        .register(Box::new(CountingSelect(Arc::clone(&runs))))
        .expect("register");
    let mut rig = CanvasRig::with_registry(app, registry);
    rig.click(screen(3.0, 3.0));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "one edit.select dispatch");
    assert_eq!(rig.app.borrow().selection.shapes, vec![id]);
}

#[test]
fn a_shift_click_toggles_a_selected_shape_off() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    assert_eq!(rig.app.borrow().selection.shapes, vec![id]);
    // Shift-click the same shape: toggle off.
    rig.shift_click(screen(3.0, 3.0));
    assert!(rig.app.borrow().selection.is_empty());
}

#[test]
fn a_marquee_selects_intersecting_shapes() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    // Start on empty page space below-left of the shape, drag across it.
    rig.drag(screen(1.0, 1.0), screen(5.0, 5.0));
    assert_eq!(rig.app.borrow().selection.shapes, vec![id]);
}

#[test]
fn one_move_drag_is_one_undo_step() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    let before = shape_number(&rig.app.borrow(), id, "PinX");
    rig.drag(screen(3.0, 3.0), screen(3.5, 3.0));
    let after = shape_number(&rig.app.borrow(), id, "PinX");
    assert!((after - before - 0.5).abs() < 1.0e-9, "{before} -> {after}");
    let app = rig.app.borrow();
    let session = app.active_document().expect("session");
    assert!(session.undo.can_undo());
    assert_eq!(
        session.undo.undo_labels().len(),
        1,
        "one drag, one undo step"
    );
}

#[test]
fn one_resize_drag_sets_the_new_size() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    // The east handle of the 2 x 1 box at pin (3, 3).
    rig.drag(screen(4.0, 3.0), screen(5.0, 3.0));
    let width = shape_number(&rig.app.borrow(), id, "Width");
    assert!((width - 3.0).abs() < 1.0e-9, "{width}");
}

#[test]
fn one_rotate_drag_turns_the_shape() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    // The rotate handle sits `ROTATE_OFFSET_PX` screen pixels above
    // the top edge (task P7-19; it was a fixed 0.25 in).
    let offset = larrygram_ui::handles::ROTATE_OFFSET_PX / (96.0 * crate::ZOOM);
    rig.drag(screen(3.0, 3.5 + offset), screen(3.5, 3.5));
    let angle = shape_number(&rig.app.borrow(), id, "Angle");
    assert!(angle.abs() > 0.1, "{angle}");
}

#[test]
fn alt_disables_snapping_for_the_drag() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    rig.set_alt(true);
    rig.drag(screen(3.0, 3.0), screen(3.51, 3.0));
    rig.set_alt(false);
    let pin = shape_number(&rig.app.borrow(), id, "PinX");
    assert!((pin - 3.51).abs() < 1.0e-5, "{pin}");
}

#[test]
fn a_drag_does_not_re_rasterize_until_the_commit() {
    let (app, _id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    assert_eq!(rig.generation(), 1, "the first frame rasterizes");
    rig.click(screen(3.0, 3.0));
    assert_eq!(rig.generation(), 1, "selection is not a document change");
    rig.down(screen(3.0, 3.0));
    rig.move_to(screen(3.5, 3.0));
    assert_eq!(rig.generation(), 1, "the drag preview is overlay-only");
    rig.up(screen(3.5, 3.0));
    // The release commits one edit; the next pass re-rasterizes it.
    assert!(rig.generation() >= 2, "the committed edit re-rasterizes");
}

/// A command that counts its runs (`arrange.nudge` for the focus test).
struct Counts(Arc<AtomicUsize>);

impl Command for Counts {
    fn meta(&self) -> &CommandMeta {
        use std::sync::OnceLock;
        static META: OnceLock<CommandMeta> = OnceLock::new();
        META.get_or_init(|| {
            CommandMeta::new(
                CommandId::parse("arrange.nudge").expect("id"),
                "Nudge",
                CommandStatus::Implemented,
            )
        })
    }
    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::yes()
    }
    fn execute(
        &self,
        _ctx: &mut AppCtx,
        _inv: &Invocation,
    ) -> Result<Outcome, larrygram_commands::CommandError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome::message("nudged"))
    }
}

#[test]
fn tab_focuses_the_canvas_and_enables_page_scoped_shortcuts() {
    let (app, _id) = app_with_one_shape();
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::new();
    registry
        .register(Box::new(Counts(Arc::clone(&runs))))
        .expect("register");
    let mut shell = Shell::new(registry, LayoutSets::default(), app);
    let sets = Loader::from_embedded().expect("layouts");
    shell.set_shortcuts(&sets.panel_2016.shortcuts);
    let shell = Rc::new(RefCell::new(shell));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui({
            let shell = Rc::clone(&shell);
            move |ui| {
                let mut shell = shell.borrow_mut();
                let intents = shell.show_in(ui).expect("the frame renders");
                let _ = shell.dispatch(&intents).expect("dispatch");
            }
        });
    harness.run();

    // Tab moves the keyboard focus; stop when the canvas has it.
    let mut focused = false;
    for _ in 0..64 {
        harness.key_press(egui::Key::Tab);
        harness.run();
        if shell.borrow().focus_scope() == ShortcutScope::Page {
            focused = true;
            break;
        }
    }
    assert!(focused, "Tab reaches the canvas");
    harness.key_press(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the page-scoped nudge fired"
    );
}
