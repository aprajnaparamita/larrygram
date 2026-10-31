//! The manual-pass fixes (task P7-24): every fix driven through the
//! real input path in the full shell (`ShellRig`) in both layouts —
//! a shape selected on the canvas, then the picker row, toolbar
//! button or key pressed — and checked on the shape's cells and the
//! rendered picture. A test that dispatches the command does not
//! count for this task (the master note).
//!
//! Item 0 is the root cause of the dead formatting: a press on a
//! picker or menu popup also reached the canvas under it (raw pointer
//! input), which deselected the shape before the format command ran.
//! `the_picker_click_keeps_the_selection_and_applies` pins the fix.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// A shell in `kind` with the rich state (the rectangle and the
/// picture), the rectangle deselected: the tests select it with a
/// real canvas click.
fn rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.selection.shapes.clear();
    app.selection.anchor = None;
    let mut rig = ShellRig::with_app(app);
    // One double-click window for the text tests (the P7-19 rule).
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
    rig.run();
    // Centre the fixture shape in the viewport (the `interaction`
    // rig's pan rule): zoom 1, pan puts (3, 3) mid-canvas.
    {
        let viewport = rig.shell.borrow().canvas_viewport().expect("the canvas drew");
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("session");
        let k = 96.0;
        session.view.zoom = 1.0;
        session.view.pan = larrygram_geom::Point::new(
            f64::from(viewport.width()) / 2.0 / k - SHAPE.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - SHAPE.1),
        );
    }
    rig.run();
    rig.run();
    rig
}

/// The fixture rectangle of `rig`'s document.
fn rectangle(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the `interaction`
/// module's click rule: one harness run steps past egui's click time).
fn click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.harness().event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

/// A double click at `pos` (two clicks inside one double-click window).
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    click_at(rig, pos);
    click_at(rig, pos);
}

/// Clicks the first widget with `role` and `name`.
fn click(rig: &mut ShellRig, role: Role, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(role, name)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    node.click();
    rig.run();
}

/// Whether a widget with `role` and `name` is drawn.
fn has(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .is_some()
}

/// The disabled state of the first widget with `role` and `name`.
fn disabled(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"))
        .accesskit_node()
        .data()
        .is_disabled()
}

/// The tooltip (accesskit description) of the first widget with
/// `role` and `name`.
fn tooltip(rig: &mut ShellRig, role: Role, name: &str) -> Option<String> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .and_then(|node| {
            node.accesskit_node()
                .data()
                .description()
                .map(|d| d.to_owned())
        })
}

/// The text of one cell of `shape` (`None` when the shape has no such
/// cell or the cell holds no text).
fn cell_text(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.formula.clone())
}

/// The number value of one cell of `shape`.
fn cell_number(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// The number value of one cell of `shape`, panicking when absent.
fn number(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
    cell_number(rig, shape, name).unwrap_or_else(|| panic!("no {name} on the shape"))
}

/// The current selection of `rig`.
fn selection(rig: &ShellRig) -> Vec<ShapeId> {
    rig.shell.borrow().app.selection.shapes.clone()
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = rectangle(&rig.shell.borrow().app);
    assert_eq!(selection(rig), vec![shape], "the canvas click selects");
}

/// The pixel at a page point in the rendered frame (the picture check).
fn pixel(rig: &mut ShellRig, x: f64, y: f64) -> (u8, u8, u8) {
    let pos = screen(rig, x, y);
    let image = rig.render();
    let px = image.get_pixel(pos.x as u32, pos.y as u32);
    (px[0], px[1], px[2])
}

/// The Panel Classic formatting split's picker opens from its
/// "Fill Color menu" arrow; Panel Modern's Fill split opens from its
/// "More items" arrow (the split beside the "Fill" label).
fn open_fill_picker(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Fill Color menu"),
        _ => {
            // The "More items" arrow nearest the "Fill" button.
            rig.run();
            let root = rig.harness().root();
            let fill = root
                .query_all_by_role_and_label(Role::Button, "Fill")
                .next()
                .expect("the Fill split")
                .rect();
            let best = root
                .query_all_by_role_and_label(Role::Button, "More items")
                .min_by(|a, b| {
                    (a.rect().center() - fill.center())
                        .length()
                        .total_cmp(&(b.rect().center() - fill.center()).length())
                })
                .expect("a More items arrow");
            best.click();
            rig.run();
        }
    }
}

/// Item 0 (the root cause): a picker click must keep the selection
/// and apply — the press once fell through the popup to the canvas
/// and deselected the shape before the format command ran.
#[test]
fn the_picker_click_keeps_the_selection_and_applies() {
    for kind in LAYOUTS {
        let mut rig = rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_fill_picker(&mut rig, kind);
        assert!(has(&mut rig, Role::Button, "Accent #1F5BC4"), "{kind:?}");
        click(&mut rig, Role::Button, "Accent #1F5BC4");
        assert_eq!(selection(&rig), vec![shape], "{kind:?}: the selection stays");
        assert_eq!(
            cell_text(&rig, shape, "FillForegnd").as_deref(),
            Some("RGB(31,91,196)"),
            "{kind:?}: the fill cell changes"
        );
        // The picture: the shape's centre paints the new fill.
        let (r, g, b) = pixel(&mut rig, SHAPE.0, SHAPE.1);
        assert!(
            (r as i32 - 31).abs() <= 4
                && (g as i32 - 91).abs() <= 4
                && (b as i32 - 196).abs() <= 4,
            "{kind:?}: the picture shows the fill: got ({r},{g},{b})"
        );
    }
}
