//! Task P7-27 item 3: latch precedence. The text toggles latch from
//! the selection's cells, so they follow the selection — every check
//! here drives the real input path in the full shell (`ShellRig`) in
//! both layouts (the master note), with the latches of Italic,
//! Underline and Align Right clicked too (the review A8 gap).

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

/// The fixture picture's centre, in page inches (the plain shape B).
const PICTURE: (f64, f64) = (6.0, 3.0);

/// A shell in `kind` with the rich state (the rectangle and the
/// picture), the rectangle deselected: the tests select it with a
/// real canvas click.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.selection.shapes.clear();
    app.selection.anchor = None;
    // Both fixture shapes carry text (task P7-29 item 1: the text
    // controls follow the text and disable without it, and the latch
    // contrast here is the style bits, not the text).
    let session = app.active_document_mut().expect("session");
    let page = session.active_page.expect("page");
    let shapes = session.document.page(page).expect("page").shapes.clone();
    for shape in shapes {
        if let Some(entry) = session.document.shape_mut(shape) {
            entry.text = Some(larrygram_model::TextContent {
                parts: vec![larrygram_model::TextPart::Text("x".to_owned())],
            });
        }
    }
    let mut rig = ShellRig::with_app(app);
    rig.run();
    // Centre the fixture shapes in the viewport (the `interaction`
    // rig's pan rule): zoom 1, pan puts (3, 3) mid-canvas.
    {
        let viewport = rig
            .shell
            .borrow()
            .canvas_viewport()
            .expect("the canvas drew");
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

/// The fixture rectangle of `rig`'s document (shape A).
fn rectangle(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

/// The fixture picture of `rig`'s document (shape B).
fn picture(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[1]
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the `interaction`
/// module's click rule: one harness run steps past egui's click time,
/// so the two halves must share a frame).
fn click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    rig.run();
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

/// The checked (toggled) state of the first widget with `role` and
/// `name`.
fn toggled(rig: &mut ShellRig, role: Role, name: &str) -> Option<bool> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"))
        .accesskit_node()
        .data()
        .toggled()
        .map(|state| state == egui::accesskit::Toggled::True)
}

/// The number value of one cell of the row `row_ix` of `shape`'s
/// `section`, or `None` when the shape has no such cell.
fn row_number(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|section| section.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = rectangle(&rig.shell.borrow().app);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![shape],
        "the canvas click selects the rectangle"
    );
}

/// Selects the fixture picture (the plain shape B) with a real canvas
/// click.
fn select_picture(rig: &mut ShellRig) {
    let pos = screen(rig, PICTURE.0, PICTURE.1);
    click_at(rig, pos);
    let shape = picture(&rig.shell.borrow().app);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![shape],
        "the canvas click selects the picture"
    );
}

/// One text toggle: the button label, the row cell its click writes
/// and the value the latch stands for.
const TOGGLES: [(&str, &str, &str, f64); 4] = [
    ("Bold", "Character", "Style", 1.0),
    ("Italic", "Character", "Style", 2.0),
    ("Underline", "Character", "Style", 4.0),
    ("Align Right", "Paragraph", "HorzAlign", 2.0),
];

/// Item 3's acceptance (the review A5/A8): each toggle latches with a
/// real click on shape A, shows off when the selection moves to the
/// plain shape B and shows on again when A comes back — in both
/// layouts. The A5 bug left the display-only latch set after the
/// click, so B showed the toggle on.
#[test]
fn the_toggle_latches_follow_the_selection() {
    for kind in LAYOUTS {
        for (button, section, cell, latched_value) in TOGGLES {
            let mut rig = shell_rig(kind);
            let shape = rectangle(&rig.shell.borrow().app);
            select_shape(&mut rig);
            click(&mut rig, Role::Button, button);
            assert_eq!(
                toggled(&mut rig, Role::Button, button),
                Some(true),
                "{kind:?}: {button} latches on shape A"
            );
            assert_eq!(
                row_number(&rig, shape, section, cell),
                Some(latched_value),
                "{kind:?}: the {section} {cell} cell records {latched_value}"
            );
            select_picture(&mut rig);
            assert_eq!(
                toggled(&mut rig, Role::Button, button),
                Some(false),
                "{kind:?}: {button} shows off on the plain shape"
            );
            select_shape(&mut rig);
            assert_eq!(
                toggled(&mut rig, Role::Button, button),
                Some(true),
                "{kind:?}: {button} shows on again with shape A selected"
            );
        }
    }
}
