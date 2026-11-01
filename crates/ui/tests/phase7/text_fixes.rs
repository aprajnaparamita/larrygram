//! The manual-pass fixes for adding and editing text (task P7-25):
//! every fix driven through the real input path in the full shell
//! (`ShellRig`) in both layouts — canvas pointer presses and drags,
//! tool button clicks, typed text and key presses (F2, Escape,
//! Cmd+Z) — and checked on the shape's text and cells and the
//! rendered picture. A test that dispatches the command does not
//! count for this task (the master note).
//!
//! - item 4: the Text Block tool moves and resizes the text block;
//! - item 5: the Text tool's icon is the clear letter-A `typeface`
//!   glyph, not the thumb-like `letter-a`;
//! - item 8: double click, a typed character and F2 start editing;
//! - item 9: the Text tool's outline, undo, click and Escape rules.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::AppCtx;
use larrygram_geom::{Point, Unit};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId, TextContent, TextPart,
};
use larrygram_ui_spec::LayoutKind;

use crate::ShellRig;

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// The fixture text box's centre, in page inches (2 x 0.5 at (6, 3)).
const BOX: (f64, f64) = (6.0, 3.0);

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// One 2 x `height` in shape with its pin at (`x`, `y`), filled and
/// lineless (the picture checks read the fill, not the shape's own
/// border), and a text block of `text` when `text` is not empty.
fn text_shape(x: f64, y: f64, width: f64, height: f64, text: &str, visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", width),
        cell("Height", height),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", width / 2.0),
        cell("LocPinY", height / 2.0),
        cell("Angle", 0.0),
        cell("FillPattern", 1.0),
        cell("LinePattern", 0.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", width, 0.0),
        ("LineTo", width, height),
        ("LineTo", 0.0, height),
        ("LineTo", 0.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    if !text.is_empty() {
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text(text.to_owned())],
        });
    }
    shape
}

/// The document of the text fixtures: a 2 x 1 rectangle at (3, 3)
/// with the text "Hello" and a centred 1 x 0.5 text block, and a 2 x
/// 0.5 text box at (6, 3) with the text "Box".
fn text_document() -> (AppCtx, ShapeId, ShapeId) {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let mut rectangle = text_shape(SHAPE.0, SHAPE.1, 2.0, 1.0, "Hello", 1);
    rectangle.sheet.cells.extend([
        cell("TxtPinX", 1.0),
        cell("TxtPinY", 0.5),
        cell("TxtLocPinX", 0.5),
        cell("TxtLocPinY", 0.25),
        cell("TxtWidth", 1.0),
        cell("TxtHeight", 0.5),
    ]);
    let rectangle = document.add_shape(rectangle);
    let text_box = document.add_shape(text_shape(BOX.0, BOX.1, 2.0, 0.5, "Box", 2));
    page.shapes.push(rectangle);
    page.shapes.push(text_box);
    document.pages.push(page);
    let mut app = AppCtx::new(crate::seen_settings());
    app.open_document(larrygram_commands::DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    (app, rectangle, text_box)
}

/// A shell in `kind` over [`text_document`], nothing selected, the
/// rectangle centred in the viewport.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let (mut app, _, _) = text_document();
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app(app);
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
    rig.run();
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
        session.view.pan = Point::new(
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

/// The fixture text box of `rig`'s document.
fn text_box(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[1]
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the P7-24 helper: one
/// harness run steps past egui's click time, so the two halves must
/// share a frame for the click to register).
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
    click_at(rig, pos);
    click_at(rig, pos);
}

/// A drag from `from` to `to` (press, moves over frames, release).
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
    for step in 1..=4 {
        let pos = from + (to - from) * (step as f32 / 4.0);
        rig.harness().event(egui::Event::PointerMoved(pos));
        rig.run();
    }
    rig.harness().event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

/// One key press and release.
fn press_key(rig: &mut ShellRig, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        rig.harness().event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        rig.run();
    }
}

/// Types `text` one character at a time (the real key events).
fn type_text(rig: &mut ShellRig, text: &str) {
    for character in text.chars() {
        rig.harness()
            .event(egui::Event::Text(character.to_string()));
        rig.run();
    }
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

/// Whether the in-place editor (a `TextInput` row) is on screen.
fn editing(rig: &mut ShellRig) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role(Role::TextInput)
        .next()
        .is_some()
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = rectangle(&rig.shell.borrow().app);
    assert_eq!(selection(rig), vec![shape], "the canvas click selects");
}

/// The current selection of `rig`.
fn selection(rig: &mut ShellRig) -> Vec<ShapeId> {
    rig.shell.borrow().app.selection.shapes.clone()
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

/// The plain text of `shape`.
fn shape_text(rig: &ShellRig, shape: ShapeId) -> String {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .text
        .as_ref()
        .map(|text| {
            text.parts
                .iter()
                .filter_map(|part| match part {
                    TextPart::Text(literal) => Some(literal.as_str()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The number of shapes on the active page.
fn shape_count(rig: &ShellRig) -> usize {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes.len()
}

/// The rendered canvas of `rig` (the picture check): the pointer is
/// parked at the fixture centre first, so the cursor lands in the
/// same place every time, and the crop is the canvas viewport — the
/// status line carries the last command's message, which legitimately
/// differs between two states.
fn picture(rig: &mut ShellRig) -> image::RgbaImage {
    let park = screen(rig, SHAPE.0, SHAPE.1);
    rig.harness().event(egui::Event::PointerMoved(park));
    rig.run();
    let image = rig.render();
    let rect = rig.shell.borrow().canvas_viewport().expect("viewport");
    let (x, y) = (rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32);
    let (w, h) = (rect.width().max(1.0) as u32, rect.height().max(1.0) as u32);
    image::imageops::crop_imm(
        &image,
        x,
        y,
        w.min(image.width() - x),
        h.min(image.height() - y),
    )
    .to_image()
}

// ---------------------------------------------------------------------------
// Item 4: the Text Block tool.
// ---------------------------------------------------------------------------

/// The Text Block tool button's label in one layout (Panel Modern's
/// group label is "Text Block"; the Panel Classic toolbar says
/// "Text Block Tool" inside the Text Tool split's menu).
fn text_block_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Text Block Tool",
        _ => "Text Block",
    }
}

/// Chooses the Text Block tool with a real control click (task P7-25
/// item 4: "can be chosen with a text box or shape selected").
fn choose_text_block(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Text Tool menu");
            click(rig, Role::MenuItem, text_block_button(kind));
        }
        _ => click(rig, Role::Button, text_block_button(kind)),
    }
}

/// Item 4: the Text Block tool is chosen with the shape selected and
/// one drag moves the text block (`TxtPinX`/`TxtPinY`), as one undo
/// step.
#[test]
fn the_text_block_tool_moves_the_text_block_in_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        choose_text_block(&mut rig, kind);
        let before = picture(&mut rig);
        // Drag the block's centre half an inch right and a quarter up.
        let from = screen(&rig, SHAPE.0, SHAPE.1);
        let to = screen(&rig, SHAPE.0 + 0.5, SHAPE.1 + 0.25);
        drag(&mut rig, from, to);
        assert!(
            (number(&rig, shape, "TxtPinX") - 1.5).abs() < 1.0e-9,
            "{kind:?}: TxtPinX moved"
        );
        assert!(
            (number(&rig, shape, "TxtPinY") - 0.75).abs() < 1.0e-9,
            "{kind:?}: TxtPinY moved"
        );
        assert!(
            (number(&rig, shape, "TxtWidth") - 1.0).abs() < 1.0e-9,
            "{kind:?}: the size is untouched"
        );
        // The picture moved with the block.
        let after = picture(&mut rig);
        assert_ne!(before, after, "{kind:?}: the picture moved");
        // One undo removes the whole drag.
        press_key(
            &mut rig,
            egui::Key::Z,
            egui::Modifiers {
                command: true,
                ..egui::Modifiers::NONE
            },
        );
        assert!(
            (number(&rig, shape, "TxtPinX") - 1.0).abs() < 1.0e-9,
            "{kind:?}: one undo restores the pin"
        );
        assert_eq!(before, picture(&mut rig), "{kind:?}: the picture restores");
    }
}

/// Item 4: a corner drag resizes the text block
/// (`TxtWidth`/`TxtHeight`), one undo step again.
#[test]
fn the_text_block_tool_resizes_the_text_block_in_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        choose_text_block(&mut rig, kind);
        // The block is 1 x 0.5 at the shape centre: its north-east
        // corner is (3.5, 3.25).
        let from = screen(&rig, 3.5, 3.25);
        let to = screen(&rig, 4.0, 3.5);
        drag(&mut rig, from, to);
        assert!(
            (number(&rig, shape, "TxtWidth") - 1.5).abs() < 1.0e-9,
            "{kind:?}: TxtWidth grew"
        );
        assert!(
            (number(&rig, shape, "TxtHeight") - 0.75).abs() < 1.0e-9,
            "{kind:?}: TxtHeight grew"
        );
        // The centre stayed put: the pin keeps its place.
        assert!(
            (number(&rig, shape, "TxtPinX") - 1.0).abs() < 1.0e-9,
            "{kind:?}: the centre stayed"
        );
        // One undo removes the whole resize.
        press_key(
            &mut rig,
            egui::Key::Z,
            egui::Modifiers {
                command: true,
                ..egui::Modifiers::NONE
            },
        );
        assert!(
            (number(&rig, shape, "TxtWidth") - 1.0).abs() < 1.0e-9,
            "{kind:?}: one undo restores the size"
        );
    }
}
