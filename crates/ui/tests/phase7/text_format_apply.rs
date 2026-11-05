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

/// One command with args, dispatched through the shell.
fn shell_dispatch(rig: &mut ShellRig, id: &'static str, args: Args) {
    let command = CommandId::parse(id).expect("id");
    let intent = larrygram_ui::Intent::new(command, Invocation::new(args, Source::Ui));
    let mut shell = rig.shell.borrow_mut();
    shell.dispatch(&[intent]).expect("the command runs");
}

/// The count of inked pixels in a page rectangle of `image` (the
/// `text_controls` ink rule: the background is the region's most
/// common colour).
fn ink_count(
    rig: &mut ShellRig,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    image: &image::RgbaImage,
) -> usize {
    let a = screen(rig, x0, y1);
    let b = screen(rig, x1, y0);
    let (xmin, xmax) = (a.x.min(b.x).max(0.0) as u32, a.x.max(b.x) as u32);
    let (ymin, ymax) = (a.y.min(b.y).max(0.0) as u32, a.y.max(b.y) as u32);
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for y in ymin..ymax.min(image.height()) {
        for x in xmin..xmax.min(image.width()) {
            let px = image.get_pixel(x, y);
            *counts.entry((px[0], px[1], px[2])).or_default() += 1;
        }
    }
    let background = counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| *colour)
        .unwrap_or((0, 0, 0));
    let mut inked = 0;
    for y in ymin..ymax.min(image.height()) {
        for x in xmin..xmax.min(image.width()) {
            let px = image.get_pixel(x, y);
            let far = i32::from(px[0]) - i32::from(background.0);
            let far = far
                .abs()
                .max((i32::from(px[1]) - i32::from(background.1)).abs());
            let far = far.max((i32::from(px[2]) - i32::from(background.2)).abs());
            if far > 40 {
                inked += 1;
            }
        }
    }
    inked
}

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
    args.set("x", larrygram_commands::ArgValue::Number(SHAPE.0));
    args.set("y", larrygram_commands::ArgValue::Number(SHAPE.1));
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

/// Presses one key with modifiers (the range-selection keys).
fn press_key_with(rig: &mut ShellRig, key: egui::Key, modifiers: egui::Modifiers) {
    rig.harness().event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    rig.run();
    rig.harness().event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    rig.run();
}

/// Whether every glyph of `shape`'s drawn text is bold: one flag per
/// glyph, in text order (the picture's runs, task P7-53).
fn drawn_bold(rig: &ShellRig, shape: ShapeId) -> Vec<bool> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    let scene = larrygram_render::build_scene(
        &session.document,
        page,
        &larrygram_render::BuildOptions::default(),
    )
    .expect("scene");
    let mut stack: Vec<&larrygram_render::scene::Node> = scene.nodes.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            larrygram_render::scene::Node::Shape(body) => {
                if body.shape_id == Some(shape)
                    && let Some(text) = body.text.as_ref()
                {
                    return text
                        .layout
                        .lines
                        .iter()
                        .flat_map(|line| line.glyphs.iter())
                        .map(|glyph| text.fonts[glyph.font].bold())
                        .collect();
                }
            }
            larrygram_render::scene::Node::Group(group) => {
                stack.extend(group.children.iter());
            }
            _ => {}
        }
    }
    panic!("no text node");
}

/// The `Character` rows of `shape` as `(IX, Style)` pairs, in row
/// order.
fn character_rows(rig: &ShellRig, shape: ShapeId) -> Vec<(Option<i32>, Option<f64>)> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .map(|block| {
            block
                .rows
                .iter()
                .map(|row| {
                    let style = row
                        .cells
                        .iter()
                        .find(|cell| cell.name.as_str() == "Style")
                        .and_then(|cell| match cell.value {
                            Some(CellValue::Number(value)) => Some(value),
                            _ => None,
                        });
                    (row.ix, style)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `Character` run markers of `shape`'s text, in order.
fn run_markers(rig: &ShellRig, shape: ShapeId) -> Vec<u32> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .document
        .shape(shape)
        .expect("shape")
        .text
        .clone()
        .map(|text| {
            text.parts
                .iter()
                .filter_map(|part| match part {
                    larrygram_model::TextPart::CharRun { ix } => Some(*ix),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the current frame draws a widget named `name`.
fn has(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    let root = rig.harness().root();
    root.query_all_by_role_and_label(role, name)
        .next()
        .is_some()
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
    session
        .document
        .shape(shape)
        .expect("shape")
        .text
        .clone()
        .map(|text| {
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
            assert!(
                (size - 0.5).abs() < 1.0e-9,
                "36 pt records as 0.5 in ({size})"
            );
        },
    );
}

/// A text colour changes the whole text (the accent swatch).
#[test]
fn a_text_colour_changes_the_whole_text() {
    one_format_changes_the_whole_text("text colour", pick_colour, |rig, shape| {
        assert_eq!(
            row_color(rig, shape, "Character", "Color"),
            Some((0x1F, 0x5B, 0xC4)),
            "the swatch records the run colour"
        );
    });
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

// ---------------------------------------------------------------------------
// Item 2: text being edited.
// ---------------------------------------------------------------------------

/// With no text selected a format applies to the whole text (the
/// simple rule for the alpha, task P7-53 item 2): a bare caret in the
/// editor, one format click, and every glyph draws at it after the
/// commit.
#[test]
fn a_format_at_a_bare_caret_applies_to_the_whole_text() {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the double click opens the editor"
        );
        // The caret is a bare caret (no selection) at the end.
        let plain = rig.render();
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);
        assert!(
            drawn_bold(&rig, shape).iter().all(|bold| *bold),
            "{kind:?}: every glyph is bold after the commit"
        );
        let after = rig.render();
        let differ = text_differ(&rig, &plain, &after);
        assert!(
            differ > 0,
            "{kind:?}: the bold picture differs from the plain text ({differ} px)"
        );
    }
}

/// With a range selected the format applies to that range only, and
/// the commit keeps it (task P7-53 item 2): the range's glyphs draw at
/// the format, the rest stays plain, and the written text carries one
/// run marker per run.
#[test]
fn a_format_with_a_range_selected_applies_to_the_range_only() {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the double click opens the editor"
        );
        // Select the last two characters ("MMMM" with the caret at the
        // end): Shift+Left twice.
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        let plain = rig.render();
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);

        // The drawn text: the range's glyphs bold, the rest plain.
        let bold = drawn_bold(&rig, shape);
        assert_eq!(
            bold,
            vec![false, false, true, true],
            "{kind:?}: only the selected range is bold ({bold:?})"
        );
        let after = rig.render();
        let differ = text_differ(&rig, &plain, &after);
        assert!(
            differ > 0,
            "{kind:?}: the bold range differs in the picture ({differ} px)"
        );

        // The commit keeps it: one run marker per run and one local
        // row per run (the bold run's row carries the Style bit).
        let markers = run_markers(&rig, shape);
        assert_eq!(
            markers.len(),
            2,
            "{kind:?}: the commit keeps the run markers ({markers:?})"
        );
        let rows = character_rows(&rig, shape);
        assert_eq!(rows.len(), 2, "{kind:?}: one row per run ({rows:?})");
        let styles: Vec<Option<f64>> = rows.iter().map(|(_, style)| *style).collect();
        assert!(
            styles.contains(&Some(1.0)),
            "{kind:?}: the range's row is bold ({styles:?})"
        );
        assert!(
            styles.contains(&Some(0.0)),
            "{kind:?}: the rest's row stays plain ({styles:?})"
        );

        // One undo step for the format click.
        assert!(
            undo_labels(&rig).iter().any(|label| label == "Bold"),
            "{kind:?}: the format is one undo step"
        );
    }
}

/// A range format survives a later edit of the same text (task P7-53
/// item 2: "the commit keeps it"): typing beside the range and
/// committing again keeps both runs.
#[test]
fn a_range_format_survives_a_later_edit() {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);

        // Edit again: append "!" and commit.
        double_click_at(&mut rig, pos);
        type_into_editor(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape);
        let bold = drawn_bold(&rig, shape);
        assert_eq!(
            bold,
            vec![false, false, true, true, true],
            "{kind:?}: the typed text joins the run at the caret ({bold:?})"
        );
    }
}

/// The format shows in the editor field while the text is being
/// edited (task P7-53 item 2): the field itself redraws at the
/// format before anything is committed.
#[test]
fn the_format_shows_in_the_editor_field_while_editing() {
    for kind in LAYOUTS {
        let (mut rig, _shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the double click opens the editor"
        );
        // A bare caret: the format applies to the whole text and the
        // whole field redraws (no selection highlight in the way).
        let plain = rig.render();
        click(&mut rig, Role::Button, "Bold");
        let bold = rig.render();
        let differ = text_differ(&rig, &plain, &bold);
        assert!(
            differ > 0,
            "{kind:?}: the field shows the bold before the commit ({differ} px)"
        );
        pick_size(&mut rig, kind, "36 pt");
        let sized = rig.render();
        let differ = text_differ(&rig, &bold, &sized);
        assert!(
            differ > 0,
            "{kind:?}: the field shows the size before the commit ({differ} px)"
        );
        press_key(&mut rig, egui::Key::Escape);
    }
}

/// A second format over a formatted range refines it (task P7-53):
/// the range's own row takes the second write, the rest is untouched.
#[test]
fn a_second_format_refines_an_already_formatted_range() {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);
        // Select the very last character (inside the bold range) and
        // unbold it.
        double_click_at(&mut rig, pos);
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);
        let bold = drawn_bold(&rig, shape);
        assert_eq!(
            bold,
            vec![false, false, true, false],
            "{kind:?}: the refined range takes the second write ({bold:?})"
        );
    }
}

/// Typing keeps the runs it joins (task P7-53): a typed character at
/// the start and in the middle joins the run at its position and the
/// later runs shift.
#[test]
fn typing_keeps_the_runs_it_joins() {
    for kind in LAYOUTS {
        let (mut rig, shape) = catalog_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        press_key_with(&mut rig, egui::Key::ArrowLeft, shift);
        click(&mut rig, Role::Button, "Bold");
        press_key(&mut rig, egui::Key::Escape);

        // Type "x" at the start and "y" in the middle of the plain
        // run ("xMyM" + the bold "MM" becomes ...).
        double_click_at(&mut rig, pos);
        press_key(&mut rig, egui::Key::Home);
        type_into_editor(&mut rig, "x");
        press_key_with(&mut rig, egui::Key::ArrowRight, egui::Modifiers::NONE);
        type_into_editor(&mut rig, "y");
        press_key(&mut rig, egui::Key::Escape);
        let bold = drawn_bold(&rig, shape);
        assert_eq!(
            bold,
            vec![false, false, false, false, true, true],
            "{kind:?}: the typed characters join the plain run ({bold:?})"
        );
    }
}

/// A face the theme did not register falls back to the proportional
/// font in the field (task P7-53): the field still draws.
#[test]
fn an_unregistered_face_falls_back_in_the_field() {
    for kind in LAYOUTS {
        let (mut rig, _shape) = catalog_rig(kind);
        // A family egui knows nothing about.
        let mut args = larrygram_commands::Args::new();
        args.set(
            "value",
            larrygram_commands::ArgValue::Text("NoSuchFont".to_owned()),
        );
        shell_dispatch(&mut rig, "text.font", args);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the editor opens over the unknown family"
        );
        let (x, y) = SHAPE;
        let drawn = rig.render();
        let ink = ink_count(&mut rig, x - 0.5, y - 0.25, x + 0.5, y + 0.25, &drawn);
        assert!(ink > 0, "{kind:?}: the field draws at the fallback ({ink})");
        press_key(&mut rig, egui::Key::Escape);
    }
}
