//! The Fill and Line dialogs and the Format Shape pane (task P7-28),
//! driven through the real input path in the full shell (`ShellRig`)
//! in both layouts: a shape selected with a real canvas click, the
//! dialog or pane opened from the menu, tab or toolbar row, fields
//! changed with clicks and typed values, OK or Apply pressed — then
//! the shape's cells, the rendered picture and one-step undo. A test
//! that dispatches the command does not count (the master note).

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

/// The fixture picture's centre (the pane's second subject).
const PICTURE: (f64, f64) = (6.0, 3.0);

// ---------------------------------------------------------------------------
// The rig (the task P7-24 helpers' pattern)
// ---------------------------------------------------------------------------

/// A shell in `kind` with the rich state, the rectangle deselected:
/// the tests select it with a real canvas click.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.selection.shapes.clear();
    app.selection.anchor = None;
    let mut rig = ShellRig::with_app(app);
    rig.run();
    // Centre the fixture shape in the viewport (the `interaction`
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

/// A press and a release in one frame at `pos` (the click rule of the
/// `interaction` module: one run steps past egui's click time, so the
/// two halves must share a frame).
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

/// Clicks the first widget named `name`, whatever its role.
fn click_named(rig: &mut ShellRig, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"));
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
                .map(|text| text.to_owned())
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
        "the canvas click selects"
    );
}

/// Selects the fixture picture with a real canvas click (the pane
/// follows it).
fn select_picture(rig: &mut ShellRig) {
    let pos = screen(rig, PICTURE.0, PICTURE.1);
    click_at(rig, pos);
}

/// The number value of one cell of `shape` (`None` when absent).
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

/// The colour of one cell of `shape` (`None` when absent).
fn cell_color(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<(u8, u8, u8)> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(CellValue::Color(color)) => Some((color.r, color.g, color.b)),
            _ => None,
        })
}

/// Whether `shape` has no cell named `name` (the undo restores
/// inheritance).
fn cell_absent(rig: &ShellRig, shape: ShapeId, name: &str) -> bool {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    !shape
        .sheet
        .cells
        .iter()
        .any(|cell| cell.name.as_str() == name)
}

/// Whether any pixel of the rendered frame inside the page rectangle
/// is blue-ish (blue more than 20 above red; the picture check: the
/// new fill and line are the Accent blue).
fn region_is_blue(rig: &mut ShellRig, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    let a = screen(rig, x0, y0);
    let b = screen(rig, x1, y1);
    let image = rig.render();
    let xmin = (a.x.min(b.x).max(0.0) as u32).min(image.width().saturating_sub(1));
    let xmax = (a.x.max(b.x).max(0.0) as u32).min(image.width());
    let ymin = (a.y.min(b.y).max(0.0) as u32).min(image.height().saturating_sub(1));
    let ymax = (a.y.max(b.y).max(0.0) as u32).min(image.height());
    for x in xmin..xmax {
        for y in ymin..ymax {
            let px = image.get_pixel(x, y);
            if i32::from(px[2]) > i32::from(px[0]) + 20 {
                return true;
            }
        }
    }
    false
}

/// Clicks the Undo button (Panel Modern names it for the last edit;
/// the split's "Undo menu" is not the button).
fn undo(rig: &mut ShellRig) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .children_recursive()
        .find(|node| {
            let data = node.accesskit_node().data();
            data.role() == Role::Button
                && data.label().is_some_and(|label| {
                    label == "Undo" || (label.starts_with("Undo ") && label != "Undo menu")
                })
        })
        .expect("the Undo button");
    node.click();
    rig.run();
}

/// One key press and release.
fn press_key(rig: &mut ShellRig, key: egui::Key) {
    for pressed in [true, false] {
        rig.harness().event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        rig.run();
    }
}

// ---------------------------------------------------------------------------
// The dialogs' and the pane's own input rules
// ---------------------------------------------------------------------------

/// The value of the typed field named `name` (the accesskit value).
fn field_value(rig: &mut ShellRig, name: &str) -> String {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::TextInput, name)
        .next()
        .unwrap_or_else(|| panic!("no field named {name:?}"))
        .value()
        .unwrap_or_default()
}

/// The shown choice of the combo named `name`.
fn combo_value(rig: &mut ShellRig, name: &str) -> String {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::ComboBox, name)
        .next()
        .unwrap_or_else(|| panic!("no combo named {name:?}"))
        .value()
        .unwrap_or_default()
}

/// Types `text` into the typed field named `name` (its old value
/// selected first) and commits with Enter (the field's rule).
fn set_field(rig: &mut ShellRig, name: &str, text: &str) {
    click(rig, Role::TextInput, name);
    rig.harness()
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    rig.run();
    {
        let root = rig.harness().root();
        let node = root
            .query_all_by_role_and_label(Role::TextInput, name)
            .next()
            .expect("the field");
        node.type_text(text);
    }
    rig.run();
    press_key(rig, egui::Key::Enter);
}

/// Opens the combo named `name` and clicks the row named `row` in it
/// (a choice commits at once).
fn choose(rig: &mut ShellRig, name: &str, row: &str) {
    click(rig, Role::ComboBox, name);
    {
        let root = rig.harness().root();
        let node = root
            .query_all_by_label(row)
            .next()
            .unwrap_or_else(|| panic!("no row named {row:?}"));
        node.click();
    }
    rig.run();
}

/// Opens the Fill dialog from `kind`'s own control (Panel Classic's
/// Format menu, Panel Modern's Fill split).
fn open_fill_dialog(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Format");
            click(rig, Role::MenuItem, "Fill...");
        }
        _ => {
            open_split(rig, "Fill");
            click_named(rig, "Fill Options...");
        }
    }
}

/// Opens the Line dialog from `kind`'s own control.
fn open_line_dialog(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Format");
            click(rig, Role::MenuItem, "Line...");
        }
        _ => {
            open_split(rig, "Line");
            click_named(rig, "Line Options...");
        }
    }
}

/// Opens the Format Shape pane from `kind`'s own control (Panel
/// Classic's View > Toolbars row, Panel Modern's group launcher).
fn open_format_shape(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "View");
            click(rig, Role::MenuItem, "Toolbars");
            click(rig, Role::MenuItem, "Format Shape");
        }
        _ => click_named(rig, "Format Object..."),
    }
}

/// Opens the Panel Modern split named `label` from its "More items"
/// arrow (the arrow nearest the split's face).
fn open_split(rig: &mut ShellRig, label: &str) {
    rig.run();
    let root = rig.harness().root();
    let face = root
        .query_all_by_role_and_label(Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("no {label} split"));
    let centre = face.rect().center();
    let arrow = root
        .query_all_by_role_and_label(Role::Button, "More items")
        .min_by(|a, b| {
            (a.rect().center() - centre)
                .length()
                .total_cmp(&(b.rect().center() - centre).length())
        })
        .expect("a More items arrow");
    arrow.click();
    rig.run();
}

// ---------------------------------------------------------------------------
// Goal 1: the Fill dialog.
// ---------------------------------------------------------------------------

/// The Fill dialog opens with the anchor's resolved values, sets every
/// fill cell from clicks and typed values with OK, shows the change in
/// the picture, and one Undo restores everything (goal 1 and 4).
#[test]
fn the_fill_dialog_sets_every_fill_cell_in_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_fill_dialog(&mut rig, kind);

        // The fields open with the selection anchor's resolved values.
        assert_eq!(
            combo_value(&mut rig, "Fill pattern").as_str(),
            "Solid",
            "{kind:?}: the resolved pattern shows"
        );
        assert_eq!(
            field_value(&mut rig, "Fill transparency").as_str(),
            "0",
            "{kind:?}: the resolved transparency shows"
        );

        // A click and typed values.
        choose(&mut rig, "Fill pattern", "Dashed");
        choose(&mut rig, "Fill colour palette", "Accent #1F5BC4");
        choose(&mut rig, "Pattern colour palette", "Surface #F5F5F5");
        set_field(&mut rig, "Fill transparency", "25");
        click(&mut rig, Role::Button, "OK");

        // Every cell of the contract is written.
        assert_eq!(
            cell_color(&rig, shape, "FillForegnd"),
            Some((0x1f, 0x5b, 0xc4)),
            "{kind:?}: the fill colour records"
        );
        assert_eq!(
            cell_number(&rig, shape, "FillPattern"),
            Some(2.0),
            "{kind:?}: the pattern records"
        );
        assert_eq!(
            cell_color(&rig, shape, "FillBkgnd"),
            Some((0xf5, 0xf5, 0xf5)),
            "{kind:?}: the pattern colour records"
        );
        assert_eq!(
            cell_number(&rig, shape, "FillForegndTrans"),
            Some(0.25),
            "{kind:?}: the transparency records as a fraction"
        );

        // The picture shows the new fill (the shape was black).
        assert!(
            region_is_blue(&mut rig, 2.6, 2.7, 3.4, 3.3),
            "{kind:?}: the rendered fill is blue-ish"
        );

        // One Undo restores the whole fill.
        undo(&mut rig);
        assert!(
            cell_absent(&rig, shape, "FillForegnd"),
            "{kind:?}: one undo removes the colour"
        );
        assert_eq!(
            cell_number(&rig, shape, "FillPattern"),
            Some(1.0),
            "{kind:?}: one undo restores the shape's own pattern"
        );
        assert!(
            cell_absent(&rig, shape, "FillForegndTrans"),
            "{kind:?}: one undo removes the transparency"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 2: the Line dialog.
// ---------------------------------------------------------------------------

/// The Line dialog sets every line cell (colour, weight, pattern, cap,
/// transparency, both arrowheads and their sizes and the rounding)
/// with OK, one undo step again (goals 2 and 4).
#[test]
fn the_line_dialog_sets_every_line_cell_in_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        assert!(
            !region_is_blue(&mut rig, 2.6, 2.44, 3.4, 2.56),
            "{kind:?}: the edge starts un-inked"
        );
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_line_dialog(&mut rig, kind);

        choose(&mut rig, "Line colour palette", "Accent #1F5BC4");
        set_field(&mut rig, "Line weight", "6");
        choose(&mut rig, "Line pattern", "Dotted");
        click(&mut rig, Role::RadioButton, "Square");
        set_field(&mut rig, "Line transparency", "25");
        choose(&mut rig, "Begin arrowhead", "Arrow");
        choose(&mut rig, "End arrowhead", "Stealth");
        choose(&mut rig, "Begin arrow size", "Small");
        choose(&mut rig, "End arrow size", "Large");
        set_field(&mut rig, "Corner rounding", "0.25");
        click(&mut rig, Role::Button, "OK");

        assert_eq!(
            cell_color(&rig, shape, "LineColor"),
            Some((0x1f, 0x5b, 0xc4)),
            "{kind:?}: the line colour records"
        );
        assert_eq!(
            cell_number(&rig, shape, "LineWeight"),
            Some(6.0 / 72.0),
            "{kind:?}: the weight is points on the wire and inches in the cell"
        );
        assert_eq!(
            cell_number(&rig, shape, "LinePattern"),
            Some(3.0),
            "{kind:?}: the pattern records"
        );
        assert_eq!(
            cell_number(&rig, shape, "LineCap"),
            Some(1.0),
            "{kind:?}: the cap records"
        );
        assert_eq!(
            cell_number(&rig, shape, "LineColorTrans"),
            Some(0.25),
            "{kind:?}: the transparency records as a fraction"
        );
        assert_eq!(
            cell_number(&rig, shape, "BeginArrow"),
            Some(1.0),
            "{kind:?}: the begin arrowhead records"
        );
        assert_eq!(
            cell_number(&rig, shape, "EndArrow"),
            Some(3.0),
            "{kind:?}: the end arrowhead records"
        );
        assert_eq!(
            cell_number(&rig, shape, "BeginArrowSize"),
            Some(1.0),
            "{kind:?}: the begin size records"
        );
        assert_eq!(
            cell_number(&rig, shape, "EndArrowSize"),
            Some(3.0),
            "{kind:?}: the end size records"
        );
        assert_eq!(
            cell_number(&rig, shape, "Rounding"),
            Some(0.25),
            "{kind:?}: the rounding records"
        );

        // The picture shows the new line at the shape's bottom edge
        // (deselected first: the selection overlay is blue too).
        let empty = screen(&rig, 1.0, 9.0);
        click_at(&mut rig, empty);
        assert!(
            region_is_blue(&mut rig, 2.6, 2.44, 3.4, 2.56),
            "{kind:?}: the rendered line is blue-ish"
        );

        // One Undo restores the whole line.
        undo(&mut rig);
        assert!(
            cell_absent(&rig, shape, "LineColor"),
            "{kind:?}: one undo removes the colour"
        );
        assert_eq!(
            cell_number(&rig, shape, "LinePattern"),
            Some(1.0),
            "{kind:?}: one undo restores the shape's own pattern"
        );
        assert!(
            cell_absent(&rig, shape, "Rounding"),
            "{kind:?}: one undo removes the rounding"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 4: Apply stays open, Cancel changes nothing.
// ---------------------------------------------------------------------------

/// Apply applies and keeps the dialog open; Cancel closes it and drops
/// only the later edits (the applied step stands, goal 4).
#[test]
fn apply_keeps_the_dialog_and_cancel_drops_only_the_later_edits() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_fill_dialog(&mut rig, kind);
        set_field(&mut rig, "Fill transparency", "50");
        click(&mut rig, Role::Button, "Apply");
        assert_eq!(
            cell_number(&rig, shape, "FillForegndTrans"),
            Some(0.5),
            "{kind:?}: Apply applies"
        );
        assert!(
            has(&mut rig, Role::Button, "Apply"),
            "{kind:?}: Apply keeps the dialog open"
        );

        // A later edit is dropped by Cancel.
        set_field(&mut rig, "Fill transparency", "75");
        click(&mut rig, Role::Button, "Cancel");
        assert!(
            !has(&mut rig, Role::Button, "Apply"),
            "{kind:?}: Cancel closes the dialog"
        );
        assert_eq!(
            cell_number(&rig, shape, "FillForegndTrans"),
            Some(0.5),
            "{kind:?}: Cancel keeps the applied step and drops the later edit"
        );

        // The applied step is one undo step.
        undo(&mut rig);
        assert!(
            cell_absent(&rig, shape, "FillForegndTrans"),
            "{kind:?}: one undo removes the applied step"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 3: the Format Shape pane.
// ---------------------------------------------------------------------------

/// The pane applies as the user changes a field (one undo step per
/// change) and follows the selection (goal 3).
#[test]
fn the_format_shape_pane_applies_per_field_and_follows_the_selection() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_format_shape(&mut rig, kind);
        assert!(
            !has(&mut rig, Role::Button, "OK") && !has(&mut rig, Role::Button, "Cancel"),
            "{kind:?}: the pane has no dialog buttons"
        );

        // A typed field applies at once (one command, one undo step).
        set_field(&mut rig, "Fill transparency", "50");
        assert_eq!(
            cell_number(&rig, shape, "FillForegndTrans"),
            Some(0.5),
            "{kind:?}: the field change applies at once"
        );
        undo(&mut rig);
        assert!(
            cell_absent(&rig, shape, "FillForegndTrans"),
            "{kind:?}: the change is one undo step"
        );

        // A combo choice applies at once too.
        choose(&mut rig, "Fill pattern", "Dotted");
        assert_eq!(
            cell_number(&rig, shape, "FillPattern"),
            Some(3.0),
            "{kind:?}: a choice applies at once"
        );

        // The Line section applies the same way.
        set_field(&mut rig, "Corner rounding", "0.25");
        assert_eq!(
            cell_number(&rig, shape, "Rounding"),
            Some(0.25),
            "{kind:?}: the Line section applies per field too"
        );

        // The pane follows the selection: the picture has no
        // transparency of its own, so the field re-reads to 0.
        assert_eq!(
            field_value(&mut rig, "Fill transparency").as_str(),
            "50",
            "{kind:?}: the field kept the rectangle's value"
        );
        select_picture(&mut rig);
        assert_eq!(
            field_value(&mut rig, "Fill transparency").as_str(),
            "0",
            "{kind:?}: the pane re-reads the new subject"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 5: the command contract's disabled state.
// ---------------------------------------------------------------------------

/// With no selection the Fill and Line entries disable with the reason
/// (goal 5); the programmatic contract (with arguments, no dialog) is
/// the `larrygram-commands` test suite's.
#[test]
fn the_dialog_entries_disable_without_a_selection() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        match kind {
            LayoutKind::PanelClassic => {
                click(&mut rig, Role::Button, "Format");
                assert!(
                    disabled(&mut rig, Role::MenuItem, "Fill..."),
                    "{kind:?}: Fill... disables with no selection"
                );
                assert_eq!(
                    tooltip(&mut rig, Role::MenuItem, "Fill...").as_deref(),
                    Some("no selection"),
                    "{kind:?}: the reason shows"
                );
                assert!(
                    disabled(&mut rig, Role::MenuItem, "Line..."),
                    "{kind:?}: Line... disables with no selection"
                );
                press_key(&mut rig, egui::Key::Escape);
            }
            _ => {
                // Panel Modern's dialog rows live inside the Fill and
                // Line splits, which disable with the same reason; the
                // dropdown does not open while they are disabled.
                for label in ["Fill", "Line"] {
                    assert!(
                        disabled(&mut rig, Role::Button, label),
                        "{kind:?}: {label} disables with no selection"
                    );
                    assert_eq!(
                        tooltip(&mut rig, Role::Button, label).as_deref(),
                        Some("no selection"),
                        "{kind:?}: the reason shows on {label}"
                    );
                }
            }
        }
    }
}
