//! The custom colour dialog (task P7-54 goal 1), driven through the
//! real input path in the full shell (`ShellRig`) in both layouts: a
//! shape selected with a real canvas click, the "More ... Colors..."
//! row clicked in the fill, line and text pickers, the colour picked
//! through the hex field, OK pressed — then the cells and the
//! picture. Cancel leaves the cells unchanged, one Undo removes the
//! applied colour, and the colour joins the picker's Recent Colors.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_model::{CellValue, PageId, ShapeId, TextContent, TextPart};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// The colour every test picks through the hex field.
const PICKED: (u8, u8, u8) = (0x3a, 0x7f, 0x2c);

// ---------------------------------------------------------------------------
// The rig (the `format_dialogs` helpers' pattern)
// ---------------------------------------------------------------------------

/// A shell in `kind` with the rich state and a rectangle that carries
/// text (the text-colour role needs a shape with text): the tests
/// select it with a real canvas click.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    {
        let session = app.active_document_mut().expect("session");
        let shape = session
            .document
            .page(PageId { index: 0 })
            .expect("page")
            .shapes[0];
        let entry = session.document.shape_mut(shape).expect("shape");
        entry.text = Some(TextContent {
            parts: vec![TextPart::Text("Label".to_owned())],
        });
    }
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
/// `interaction` module).
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

/// Whether a widget named `name` is drawn.
fn has_named(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_label(name)
        .next()
        .is_some()
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

/// The colour of one direct cell of `shape` (`None` when absent).
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

/// The colour of one `Character` row cell of `shape` (`None` when
/// absent).
fn character_color(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<(u8, u8, u8)> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .filter(|section| section.name.as_str() == "Character")
        .flat_map(|section| section.rows.iter())
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(CellValue::Color(color)) => Some((color.r, color.g, color.b)),
            _ => None,
        })
}

/// Whether any pixel inside the page region is green-dominant (the
/// picked `#3A7F2C` over black or white keeps green above red and
/// blue; the picture check).
fn region_is_green(rig: &mut ShellRig, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
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
            if i32::from(px[1]) > i32::from(px[0]) + 8 && i32::from(px[1]) > i32::from(px[2]) + 8 {
                return true;
            }
        }
    }
    false
}

/// Clicks the Undo button (Panel Modern names it for the last edit).
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

/// Opens the colour picker of `role` (`fill`, `line`, `text`) from the
/// control each layout carries, then clicks its "More ... Colors..."
/// row (the dialog opener).
fn open_color_dialog(rig: &mut ShellRig, kind: LayoutKind, role: &str) {
    let (row, classic_menu, modern_split) = match role {
        "fill" => ("More Fill Colors...", "Fill Color menu", "Fill"),
        "line" => ("More Line Colors...", "Line Color menu", "Line"),
        _ => ("More Text Colors...", "Text Color menu", "Font Color"),
    };
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, classic_menu),
        _ => open_split(rig, modern_split),
    }
    click_named(rig, row);
}

/// Types `text` into the hex field (its old value selected first).
fn set_hex(rig: &mut ShellRig, text: &str) {
    click(rig, Role::TextInput, "Hex");
    rig.harness()
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    rig.run();
    {
        let root = rig.harness().root();
        let node = root
            .query_all_by_role_and_label(Role::TextInput, "Hex")
            .next()
            .expect("the Hex field");
        node.type_text(text);
    }
    rig.run();
}

// ---------------------------------------------------------------------------
// Goal 1: the custom colour, both layouts
// ---------------------------------------------------------------------------

/// The fill role: the hex field picks `#3A7F2C`, OK applies it as one
/// undo step (the cell and the picture), and the colour joins the
/// picker's Recent Colors.
#[test]
fn the_fill_dialog_applies_a_custom_colour_in_both_layouts() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        assert!(
            !region_is_green(&mut rig, 2.6, 2.7, 3.4, 3.3),
            "{kind:?}: the fill starts un-coloured"
        );
        open_color_dialog(&mut rig, kind, "fill");
        assert!(
            has_named(&mut rig, "Hex"),
            "{kind:?}: the colour dialog opens"
        );
        set_hex(&mut rig, "#3A7F2C");
        click(&mut rig, Role::Button, "OK");

        assert_eq!(
            cell_color(&rig, shape, "FillForegnd"),
            Some(PICKED),
            "{kind:?}: the fill colour records"
        );
        assert!(
            region_is_green(&mut rig, 2.6, 2.7, 3.4, 3.3),
            "{kind:?}: the rendered fill is green"
        );

        // The colour joins the picker's Recent Colors.
        match kind {
            LayoutKind::PanelClassic => click(&mut rig, Role::Button, "Fill Color menu"),
            _ => open_split(&mut rig, "Fill"),
        }
        assert!(
            has_named(&mut rig, "Recent #3A7F2C"),
            "{kind:?}: the pick joins the picker's Recent Colors"
        );
        rig.harness().key_press(egui::Key::Escape);
        rig.run();

        // One undo step.
        undo(&mut rig);
        assert!(
            cell_color(&rig, shape, "FillForegnd").is_none(),
            "{kind:?}: one undo removes the colour"
        );
    }
}

/// The line role: the same dialog (the "More Line Colors..." row),
/// the line cell and the drawn edge (the edge first thickened with
/// the weight picker's "6 pt", so the picture check is not about
/// antialiasing a hairline).
#[test]
fn the_line_dialog_applies_a_custom_colour_in_both_layouts() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        // 6 pt through the line weight picker's real rows.
        match kind {
            LayoutKind::PanelClassic => {
                click(&mut rig, Role::Button, "Line Weight menu");
                click_named(&mut rig, "6 pt");
            }
            _ => {
                open_split(&mut rig, "Line");
                click_named(&mut rig, "Weight");
                click_named(&mut rig, "6 pt");
            }
        }
        assert!(
            !region_is_green(&mut rig, 2.6, 2.4, 3.4, 2.6),
            "{kind:?}: the edge starts un-coloured"
        );
        open_color_dialog(&mut rig, kind, "line");
        set_hex(&mut rig, "#3A7F2C");
        click(&mut rig, Role::Button, "OK");

        assert_eq!(
            cell_color(&rig, shape, "LineColor"),
            Some(PICKED),
            "{kind:?}: the line colour records"
        );
        assert!(
            region_is_green(&mut rig, 2.6, 2.4, 3.4, 2.6),
            "{kind:?}: the drawn edge is green"
        );
    }
}

/// The text role: the same dialog (the "More Text Colors..." row) and
/// the run's `Color` cell (the fixture's text draws too, but the cell
/// is the contract).
#[test]
fn the_text_dialog_applies_a_custom_colour_in_both_layouts() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_color_dialog(&mut rig, kind, "text");
        set_hex(&mut rig, "#3A7F2C");
        click(&mut rig, Role::Button, "OK");

        assert_eq!(
            character_color(&rig, shape, "Color"),
            Some(PICKED),
            "{kind:?}: the text colour records"
        );
    }
}

/// Cancel changes nothing, also after a typed hex.
#[test]
fn cancel_changes_nothing_in_both_layouts() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_color_dialog(&mut rig, kind, "fill");
        set_hex(&mut rig, "#FF0000");
        click(&mut rig, Role::Button, "Cancel");

        assert!(
            cell_color(&rig, shape, "FillForegnd").is_none(),
            "{kind:?}: Cancel leaves the cells unchanged"
        );
    }
}

/// A hex spelling that does not parse keeps the dialog open with its
/// message and applies nothing.
#[test]
fn a_bad_hex_spelling_keeps_the_dialog_open() {
    let mut rig = shell_rig(LayoutKind::PanelClassic);
    select_shape(&mut rig);
    let shape = rectangle(&rig.shell.borrow().app);
    open_color_dialog(&mut rig, LayoutKind::PanelClassic, "fill");
    set_hex(&mut rig, "3A7F2C");
    click(&mut rig, Role::Button, "OK");
    assert!(
        has_named(&mut rig, "use #RGB or #RRGGBB"),
        "the message shows"
    );
    assert!(
        cell_color(&rig, shape, "FillForegnd").is_none(),
        "nothing was applied"
    );
    // The corrected spelling applies.
    set_hex(&mut rig, "#3A7F2C");
    click(&mut rig, Role::Button, "OK");
    assert_eq!(cell_color(&rig, shape, "FillForegnd"), Some(PICKED));
}

/// The window's close button is a Cancel: the picked colour is
/// dropped (task P7-54's dialog rules).
#[test]
fn the_colour_dialog_window_close_is_a_cancel() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_color_dialog(&mut rig, kind, "fill");
        set_hex(&mut rig, "#3A7F2C");
        rig.run();
        {
            let root = rig.harness().root();
            let node = root
                .query_all_by_label("Close window")
                .next()
                .unwrap_or_else(|| panic!("{kind:?}: the window's close button"));
            node.click();
        }
        rig.run();
        assert!(
            !has_named(&mut rig, "Hex"),
            "{kind:?}: the dialog is gone"
        );
        assert_eq!(
            cell_color(&rig, shape, "FillForegnd"),
            None,
            "{kind:?}: the close dropped the picked colour"
        );
    }
}
