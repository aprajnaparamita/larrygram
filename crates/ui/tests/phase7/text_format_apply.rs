//! Task P7-53: text formatting reaches a shape's text, driven through
//! the real input path in the full shell (`ShellRig`) in both layouts
//! (the master note) — canvas clicks, typed text, real control clicks
//! and the rendered picture, on a catalog shape (a Basic Shapes
//! Rectangle from the stencil, with its master) as the acceptance
//! criteria name it.
//!
//! - item 1: with the shape selected and its text set, Bold, Italic,
//!   Underline, a font size, a text colour and Align Left change the
//!   whole text — the cells change, the picture changes against the
//!   plain text, one undo step each;
//! - item 2: while the text is open in the editor a format applies to
//!   the whole text (no selection) or to the selected range only
//!   (the commit keeps it), and the format shows in the editor field;
//! - item 3: a formatted shape keeps its format through `file.save_as`
//!   and `file.open`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::AppCtx;
use larrygram_commands::{Args, CommandId, Invocation, Source};
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::ShellRig;

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The dropped rectangle's centre, in page inches.
const SHAPE: (f64, f64) = (3.0, 3.0);

// ---------------------------------------------------------------------------
// Fixture: a catalog rectangle with its text, centred in the viewport.
// ---------------------------------------------------------------------------

/// One command with args, dispatched the way the shell dispatches it.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn dispatch(app: &mut AppCtx, id: &'static str, args: Args) {
    let registry = larrygram_commands::builtins::registry().expect("registry");
    let command = CommandId::parse(id).expect("id");
    larrygram_commands::dispatch(&registry, app, &command, Invocation::new(args, Source::Ui))
        .expect("the command runs");
}

/// A blank drawing with one Basic Shapes Rectangle dropped at
/// [`SHAPE`], its text typed in the editor and committed.
fn catalog_rig(kind: LayoutKind) -> (ShellRig, ShapeId) {
    let mut app = AppCtx::new(crate::seen_settings());
    app.settings.ui_layout = kind;
    dispatch(&mut app, "file.new.blank.drawing", Args::new());
    let mut args = Args::new();
    args.set(
        "stencil",
        larrygram_commands::ArgValue::Text("Basic Shapes".to_owned()),
    );
    args.set(
        "master",
        larrygram_commands::ArgValue::Text("Rectangle".to_owned()),
    );
    args.set(
        "x",
        larrygram_commands::ArgValue::Number(SHAPE.0),
    );
    args.set(
        "y",
        larrygram_commands::ArgValue::Number(SHAPE.1),
    );
    dispatch(&mut app, "insert.shape", args);
    let shape = {
        let session = app.active_document().expect("session");
        *session
            .document
            .page(session.active_page.expect("page"))
            .expect("page")
            .shapes
            .last()
            .expect("a shape")
    };
    let mut rig = ShellRig::with_app(app);
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
    centre(&mut rig);
    // The text through the real editor: the catalog fill is dark and
    // the black text hides on it (the `text_controls` rule), so the
    // fill lightens first through its picker.
    lighten_fill(&mut rig, kind);
    let pos = screen(&rig, SHAPE.0, SHAPE.1);
    double_click_at(&mut rig, pos);
    assert!(
        has(&mut rig, Role::MultilineTextInput, "Text"),
        "{kind:?}: the double click opens the editor"
    );
    type_into_editor(&mut rig, "MMMM");
    press_key(&mut rig, egui::Key::Escape);
    assert_eq!(
        shape_text(&rig, shape).as_deref(),
        Some("MMMM"),
        "{kind:?}: the typed text commits"
    );
    (rig, shape)
}

/// Centres [`SHAPE`] in the viewport (the `text_controls` pan rule).
fn centre(rig: &mut ShellRig) {
    let viewport = rig
        .shell
        .borrow()
        .canvas_viewport()
        .expect("the canvas drew");
    {
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
}

// ---------------------------------------------------------------------------
// Real input and observations.
// ---------------------------------------------------------------------------

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the `text_controls`
/// click rule: both halves must share a frame for the click).
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

/// A double click at `pos` (two clicks inside one double-click window).
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    for _ in 0..60 {
        rig.run();
    }
    click_at(rig, pos);
    click_at(rig, pos);
}

/// Clicks the first widget with `role` and `name`.
fn click(rig: &mut ShellRig, role: Role, name: &str) {
    rig.click(role, name);
    rig.run();
}

/// Presses one key.
fn press_key(rig: &mut ShellRig, key: egui::Key) {
    rig.harness().key_press(key);
    rig.run();
}

/// Whether the current frame draws a widget named `name`.
fn has(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    let root = rig.harness().root();
    root.query_all_by_role_and_label(role, name).next().is_some()
}

/// Types `text` into the in-place editor's field.
fn type_into_editor(rig: &mut ShellRig, text: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
        .next()
        .expect("the editor field");
    node.type_text(text);
    rig.run();
}

/// The plain text of `shape`.
fn shape_text(rig: &ShellRig, shape: ShapeId) -> Option<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session.document.shape(shape).expect("shape").text.clone().map(|text| {
        text.parts
            .iter()
            .filter_map(|part| match part {
                larrygram_model::TextPart::Text(part) => Some(part.as_str()),
                _ => None,
            })
            .collect()
    })
}

/// The number of one `section` row cell of `shape` (row 0).
fn row_number(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|block| block.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// The colour of one `section` row cell of `shape` (row 0).
fn row_color(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<(u8, u8, u8)> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|block| block.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(CellValue::Color(color)) => Some((color.r, color.g, color.b)),
            _ => None,
        })
}

/// The undoable steps of the active document, oldest first.
fn undo_labels(rig: &ShellRig) -> Vec<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .undo
        .undo_labels()
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// The count of pixels that differ between two rendered frames in the
/// shape's text area (the picture check of the acceptance criteria:
/// compared against the plain text, not only ink > 0).
fn text_differ(rig: &ShellRig, before: &image::RgbaImage, after: &image::RgbaImage) -> usize {
    // The text area: the shape box inset by a quarter (the text sits
    // centred in the box).
    let (x, y) = SHAPE;
    let inset_x = 0.3;
    let inset_y = 0.25;
    let a = screen(rig, x - 0.75 + inset_x, y + 0.5 - inset_y);
    let b = screen(rig, x + 0.75 - inset_x, y - 0.5 + inset_y);
    let (xmin, xmax) = (a.x.min(b.x).max(0.0) as u32, a.x.max(b.x) as u32);
    let (ymin, ymax) = (a.y.min(b.y).max(0.0) as u32, a.y.max(b.y) as u32);
    let mut differ = 0;
    for y in ymin..ymax.min(before.height()).min(after.height()) {
        for x in xmin..xmax.min(before.width()).min(after.width()) {
            let p = before.get_pixel(x, y);
            let q = after.get_pixel(x, y);
            if p.0.iter().zip(q.0.iter()).any(|(p, q)| p.abs_diff(*q) > 20) {
                differ += 1;
            }
        }
    }
    differ
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
}

/// Opens the size list and picks `name` ("36 pt").
fn pick_size(rig: &mut ShellRig, kind: LayoutKind, name: &str) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Font Size menu"),
        _ => click(rig, Role::Button, "Font Size"),
    }
    click(rig, Role::Button, name);
}

/// Opens the text colour picker and picks the accent swatch.
fn pick_colour(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Text Color menu");
            click(rig, Role::Button, "Accent #1F5BC4");
        }
        _ => {
            // Panel Modern's split opens its palette from the
            // "More items" arrow (the `open_fill_picker` rule).
            rig.run();
            let root = rig.harness().root();
            let split = root
                .query_all_by_role_and_label(Role::Button, "Font Color")
                .next()
                .expect("the Font Color split")
                .rect();
            let best = root
                .query_all_by_role_and_label(Role::Button, "More items")
                .min_by(|a, b| {
                    (a.rect().center() - split.center())
                        .length()
                        .total_cmp(&(b.rect().center() - split.center()).length())
                })
                .expect("a More items arrow");
            best.click();
            rig.run();
            click(rig, Role::Button, "Accent #1F5BC4");
        }
    }
}

/// Gives the fixture rectangle a light fill (the black text hides on
/// the dark default fill), through the fill picker.
fn lighten_fill(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Fill Color menu");
            click(rig, Role::Button, "Surface #F5F5F5");
        }
        _ => {
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
            click(rig, Role::Button, "Surface #F5F5F5");
        }
    }
}

// ---------------------------------------------------------------------------
// Item 1: a selected shape.
// ---------------------------------------------------------------------------

/// One format applied to the selected catalog shape: the cells change,
/// the picture changes against the plain text and one undo step lands
/// (the acceptance criteria's item 1). `act` drives the real control.
fn one_format_changes_the_whole_text(
    what: &str,
    act: impl Fn(&mut ShellRig, LayoutKind),
    check: impl Fn(&ShellRig, ShapeId),
) {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        select_shape(&mut rig);
        let plain = rig.render();
        let before = undo_labels(&rig).len();
        act(&mut rig, kind);
        check(&rig, shape);
        let after = rig.render();
        let differ = text_differ(&rig, &plain, &after);
        assert!(
            differ > 0,
            "{kind:?}: the {what} picture differs from the plain text ({differ} px)"
        );
        assert_eq!(
            undo_labels(&rig).len(),
            before + 1,
            "{kind:?}: the {what} is one undo step"
        );
    }
}

/// Bold changes the whole text (the `Style` bit 1, the bold face).
#[test]
fn bold_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "bold",
        |rig, _| click(rig, Role::Button, "Bold"),
        |rig, shape| {
            assert_eq!(row_number(rig, shape, "Character", "Style"), Some(1.0));
        },
    );
}

/// Italic changes the whole text (the `Style` bit 2).
#[test]
fn italic_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "italic",
        |rig, _| click(rig, Role::Button, "Italic"),
        |rig, shape| {
            assert_eq!(row_number(rig, shape, "Character", "Style"), Some(2.0));
        },
    );
}

/// Underline changes the whole text (the `Style` bit 4).
#[test]
fn underline_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "underline",
        |rig, _| click(rig, Role::Button, "Underline"),
        |rig, shape| {
            assert_eq!(row_number(rig, shape, "Character", "Style"), Some(4.0));
        },
    );
}

/// A font size changes the whole text (36 pt records as 0.5 in).
#[test]
fn a_font_size_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "font size",
        |rig, kind| pick_size(rig, kind, "36 pt"),
        |rig, shape| {
            let size = row_number(rig, shape, "Character", "Size").expect("the Size cell");
            assert!((size - 0.5).abs() < 1.0e-9, "36 pt records as 0.5 in ({size})");
        },
    );
}

/// A text colour changes the whole text (the accent swatch).
#[test]
fn a_text_colour_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "text colour",
        pick_colour,
        |rig, shape| {
            assert_eq!(
                row_color(rig, shape, "Character", "Color"),
                Some((0x1F, 0x5B, 0xC4)),
                "the swatch records the run colour"
            );
        },
    );
}

/// Align Left changes the whole text (the catalog master draws its
/// text centred; the write is a local `Paragraph` row).
#[test]
fn align_left_changes_the_whole_text() {
    one_format_changes_the_whole_text(
        "align left",
        |rig, _| click(rig, Role::Button, "Align Left"),
        |rig, shape| {
            assert_eq!(row_number(rig, shape, "Paragraph", "HorzAlign"), Some(0.0));
        },
    );
}
