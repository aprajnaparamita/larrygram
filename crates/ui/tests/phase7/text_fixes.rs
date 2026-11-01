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
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_geom::{Point, Unit};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId, TextContent, TextPart,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, sets};

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

/// A double click at `pos` (two clicks inside one double-click
/// window). The wait first drains the window: egui counts the second
/// click as a triple one when two earlier clicks (a tool drag's
/// release, a widget click) sit inside it (`click.count == 3`, so
/// `double_clicked_by` stays false). One harness run steps the clock
/// 0.25 s, so 60 runs pass any 10 s window.
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    for _ in 0..60 {
        rig.run();
    }
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

/// Whether the in-place editor (the "Text" multiline input) is on
/// screen.
fn editing(rig: &mut ShellRig) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
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
// Item 5: the Text tool's icon.
// ---------------------------------------------------------------------------

/// The Text tool button's label in one layout.
fn text_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Text Tool",
        _ => "Text",
    }
}

/// Chooses the Text tool with a real control click.
fn choose_text_tool(rig: &mut ShellRig, kind: LayoutKind) {
    click(rig, Role::Button, text_tool_button(kind));
}

/// The rect of the first widget named `name`.
fn button_rect(rig: &mut ShellRig, name: &str) -> egui::Rect {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"))
        .rect()
}

/// The project's own rasterisation of the icon `key` at `size`
/// points, over the frame's button colour (so the tiles composite
/// identically).
fn reference_icon(kind: LayoutKind, key: &str, size: f32, bg: image::Rgba<u8>) -> image::RgbaImage {
    let theme = sets().get(kind).theme.clone();
    let side = size.ceil() as u32 + 8;
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(side as f32, side as f32))
        .with_pixels_per_point(1.0)
        .build_ui(move |ui| {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(side as f32, side as f32)),
                0.0,
                egui::Color32::from_rgba_unmultiplied(bg.0[0], bg.0[1], bg.0[2], bg.0[3]),
            );
            let rect = egui::Rect::from_min_size(egui::pos2(4.0, 4.0), egui::vec2(size, size));
            let _ = larrygram_ui::icons::draw(ui, rect, key, size, &theme, true);
        });
    let image = harness.render().expect("the reference renders");
    image::imageops::crop_imm(&image, 4, 4, size as u32, size as u32).to_image()
}

/// The most common pixel of `rect` in `image` (the flat button fill).
fn mode_pixel(image: &image::RgbaImage, rect: egui::Rect) -> image::Rgba<u8> {
    let mut counts: std::collections::BTreeMap<[u8; 4], usize> = Default::default();
    for y in rect.min.y.max(0.0) as u32..rect.max.y.min(image.height() as f32) as u32 {
        for x in rect.min.x.max(0.0) as u32..rect.max.x.min(image.width() as f32) as u32 {
            *counts.entry(image.get_pixel(x, y).0).or_default() += 1;
        }
    }
    image::Rgba(
        *counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(colour, _)| colour)
            .unwrap_or(&[0, 0, 0, 255]),
    )
}

/// Whether the icon tile appears anywhere inside `rect` of `frame`.
///
/// Both pictures are blurred first: a layout at fractional
/// coordinates samples the icon texture between texels (Panel
/// Modern), which shifts edge pixels too far for an exact compare.
/// The reference's ink pixels must land on the same colours after
/// that, anywhere in the button.
fn icon_match(frame: &image::RgbaImage, rect: egui::Rect, icon: &image::RgbaImage) -> bool {
    let blurred_frame = image::imageops::blur(frame, 1.0);
    let blurred_icon = image::imageops::blur(icon, 1.0);
    let (frame, icon) = (&blurred_frame, &blurred_icon);
    let (iw, ih) = icon.dimensions();
    let bg = *icon.get_pixel(0, 0);
    let ink: Vec<(u32, u32)> = (0..ih)
        .flat_map(|y| {
            (0..iw).filter_map(move |x| {
                let p = icon.get_pixel(x, y);
                (0..3)
                    .any(|c| p.0[c].abs_diff(bg.0[c]) > 8)
                    .then_some((x, y))
            })
        })
        .collect();
    if ink.is_empty() {
        return false;
    }
    let x0 = rect.min.x.round() as i64;
    let y0 = rect.min.y.round() as i64;
    let (w, h) = (frame.width() as i64, frame.height() as i64);
    for dy in -2..=(rect.height() as i64 - i64::from(ih) + 2) {
        for dx in -2..=(rect.width() as i64 - i64::from(iw) + 2) {
            let mut ok = 0usize;
            for &(ix, iy) in &ink {
                let (fx, fy) = (x0 + i64::from(ix) + dx, y0 + i64::from(iy) + dy);
                if fx < 0 || fy < 0 || fx >= w || fy >= h {
                    continue;
                }
                let f = frame.get_pixel(fx as u32, fy as u32);
                let e = icon.get_pixel(ix, iy);
                if (0..3).all(|c| f.0[c].abs_diff(e.0[c]) <= 24) {
                    ok += 1;
                }
            }
            if ok * 100 >= ink.len() * 90 {
                return true;
            }
        }
    }
    false
}

/// Item 5: a real click on the Text tool button chooses the tool and
/// its icon is the clear letter-A `typeface` glyph (task P7-25: the
/// old `letter-a` read as a thumb), not some other icon.
#[test]
fn the_text_tool_button_draws_the_typeface_icon() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let name = text_tool_button(kind);
        choose_text_tool(&mut rig, kind);
        assert_eq!(
            rig.shell.borrow().app.active_tool(),
            larrygram_commands::app::ActiveTool::Text,
            "{kind:?}: the button click chooses the Text tool"
        );
        let rect = button_rect(&mut rig, name);
        // Park the pointer away from the button: the drawn cursor
        // would sit on the icon.
        let park = screen(&rig, SHAPE.0, SHAPE.1);
        rig.harness().event(egui::Event::PointerMoved(park));
        rig.run();
        let frame = rig.render();
        let bg = mode_pixel(&frame, rect);
        // The layouts draw the small icon at their own metric size, so
        // the check probes the sizes the metrics use.
        let matches = |key: &str| {
            [12.0f32, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0]
                .iter()
                .any(|size| icon_match(&frame, rect, &reference_icon(kind, key, *size, bg)))
        };
        assert!(
            matches("tools.text"),
            "{kind:?}: the Text tool button draws the typeface icon"
        );
        assert!(!matches("edit.cut"), "{kind:?}: and not some other icon");
    }
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
        white_fill(&mut rig, kind);
        choose_text_block(&mut rig, kind);
        let before = picture(&mut rig);
        // The ink before the drag (the black text on the white fill).
        let image = quiet_picture(&mut rig);
        let ink_before = ink_top_left(ink_box(&rig, &image, (2.0, 2.4, 4.0, 3.6)));
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
        // The picture moved with the block: its ink moved by exactly
        // the drag's distance (task P7-26 item 5).
        let after = picture(&mut rig);
        assert_ne!(before, after, "{kind:?}: the picture moved");
        let image = quiet_picture(&mut rig);
        let ink_after = ink_top_left(ink_box(&rig, &image, (2.4, 2.7, 4.4, 4.0)));
        assert!(
            (ink_after.0 - (ink_before.0 + 0.5)).abs() < 0.05
                && (ink_after.1 - (ink_before.1 + 0.25)).abs() < 0.05,
            "{kind:?}: the ink moved by the drag's distance: {ink_before:?} to {ink_after:?}"
        );
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
        // The opposite corner stays put (task P7-26 item 8): the
        // south-west corner of the block keeps its place and the pin
        // follows the grown sides.
        assert!(
            (number(&rig, shape, "TxtPinX") - 1.25).abs() < 1.0e-9,
            "{kind:?}: the pin follows the grown sides"
        );
        assert!(
            (number(&rig, shape, "TxtPinY") - 0.625).abs() < 1.0e-9,
            "{kind:?}: the pin follows the grown sides"
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

// ---------------------------------------------------------------------------
// Item 8: starting text on a shape.
// ---------------------------------------------------------------------------

/// Item 8: double-clicking a shape starts editing its text (the
/// pointer tool keeps the group click rule); the edit commits on
/// Escape and keeps the text.
#[test]
fn double_clicking_a_shape_starts_editing_its_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(shape_text(&rig, shape), "Hello", "{kind:?}: the fixture");
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the double click opens the editor"
        );
        type_text(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!editing(&mut rig), "{kind:?}: Escape ends editing");
        assert_eq!(
            shape_text(&rig, shape),
            "Hello!",
            "{kind:?}: the typed text lands and stays"
        );
    }
}

/// Item 8: typing a character with a shape selected starts the text
/// with that character (the old text is replaced). The next two
/// characters land after it (task P7-26 item 5: the caret sits after
/// the seed and no key is dropped between `text.begin` and the editor
/// taking focus).
#[test]
fn typing_a_character_starts_the_text_with_it() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        select_shape(&mut rig);
        type_text(&mut rig, "Q");
        assert!(
            editing(&mut rig),
            "{kind:?}: the typed character opens the editor"
        );
        type_text(&mut rig, "r");
        type_text(&mut rig, "s");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(
            shape_text(&rig, shape),
            "Qrs",
            "{kind:?}: the text starts with the typed character and keeps the next keys"
        );
    }
}

/// Item 8: F2 starts editing the selected shape's text.
#[test]
fn f2_starts_editing_the_shape_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        select_shape(&mut rig);
        press_key(&mut rig, egui::Key::F2, egui::Modifiers::NONE);
        assert!(editing(&mut rig), "{kind:?}: F2 opens the editor");
        type_text(&mut rig, "Y");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(
            shape_text(&rig, shape),
            "HelloY",
            "{kind:?}: the edit keeps its text"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 9: the Text tool.
// ---------------------------------------------------------------------------

/// Clicks the Undo button (Panel Modern names it for the last edit,
/// "Undo Insert Text Box"; the split's "Undo menu" is not the button).
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

/// The newest shape on `rig`'s page.
fn last_shape(rig: &ShellRig) -> ShapeId {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    *session
        .document
        .page(page)
        .expect("page")
        .shapes
        .last()
        .expect("a shape")
}

/// Makes one text object with the Text tool at (1, 5): click on empty
/// page, type, Escape (task P7-25 item 9).
fn make_text_object(rig: &mut ShellRig) -> ShapeId {
    let pos = screen(rig, 1.0, 5.0);
    click_at(rig, pos);
    assert!(
        editing(rig),
        "the click on empty page starts a new text object"
    );
    type_text(rig, "Hi");
    press_key(rig, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!editing(rig), "Escape ends editing and keeps the text");
    let shape = last_shape(rig);
    assert_eq!(
        shape_text(rig, shape),
        "Hi",
        "the text object carries its text"
    );
    shape
}

/// Item 9: a click on empty page starts a new text object, Escape
/// ends editing and keeps the text, and Undo — Cmd+Z, Ctrl+Z and the
/// Undo button — removes the object with its text in one step.
#[test]
fn the_text_tool_makes_a_text_object_in_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);

        // Cmd+Z.
        let before = shape_count(&rig);
        make_text_object(&mut rig);
        assert_eq!(shape_count(&rig), before + 1, "{kind:?}: one new object");
        press_key(
            &mut rig,
            egui::Key::Z,
            egui::Modifiers {
                command: true,
                ..egui::Modifiers::NONE
            },
        );
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: Cmd+Z removes the object with its text in one step"
        );

        // Ctrl+Z.
        make_text_object(&mut rig);
        press_key(
            &mut rig,
            egui::Key::Z,
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::NONE
            },
        );
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: Ctrl+Z removes it in one step"
        );

        // The Undo button.
        make_text_object(&mut rig);
        undo(&mut rig);
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: the Undo button removes it in one step"
        );
    }
}

/// Item 5 (task P7-26): redo brings back the shape and its text in one
/// step (Ctrl+Y).
#[test]
fn redo_of_a_new_text_box_restores_the_shape_and_its_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let before = shape_count(&rig);
        let shape = make_text_object(&mut rig);
        assert_eq!(shape_text(&rig, shape), "Hi", "{kind:?}: the fixture");
        press_key(
            &mut rig,
            egui::Key::Z,
            egui::Modifiers {
                command: true,
                ..egui::Modifiers::NONE
            },
        );
        assert_eq!(shape_count(&rig), before, "{kind:?}: one undo removes it");
        press_key(
            &mut rig,
            egui::Key::Y,
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::NONE
            },
        );
        assert_eq!(
            shape_count(&rig),
            before + 1,
            "{kind:?}: one redo brings the box back"
        );
        let shape = last_shape(&rig);
        assert_eq!(
            shape_text(&rig, shape),
            "Hi",
            "{kind:?}: and its text comes back with it"
        );
    }
}

/// Item 9: any click with the Text tool first commits the text being
/// typed (the typed text is never dropped); a click inside an
/// existing text object edits that object's text and no new box
/// appears.
#[test]
fn a_click_with_the_text_tool_commits_first_and_edits_the_object() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let box_ = text_box(&rig.shell.borrow().app);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, BOX.0, BOX.1);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: a click inside a text object edits its text"
        );
        type_text(&mut rig, "!");
        // The second click inside the same object's border commits the
        // typed text first (task P7-25: it used to drop it) and then
        // edits the object again.
        click_at(&mut rig, pos);
        assert_eq!(
            shape_text(&rig, box_),
            "Box!",
            "{kind:?}: the click keeps the typed text"
        );
        assert!(
            editing(&mut rig),
            "{kind:?}: and starts editing the object again"
        );
        assert_eq!(shape_count(&rig), 2, "{kind:?}: no stray text box");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(shape_text(&rig, box_), "Box!", "{kind:?}: Escape keeps it");
    }
}

/// The colours of the page-space vertical strip at `x`, `y0..=y1`.
fn strip_colors(
    rig: &ShellRig,
    image: &image::RgbaImage,
    x: f64,
    y0: f64,
    y1: f64,
) -> Vec<(u8, u8, u8)> {
    let top = screen(rig, x, y1);
    let bottom = screen(rig, x, y0);
    let px = top.x.clamp(0.0, image.width() as f32 - 1.0) as u32;
    let (y0, y1) = (
        top.y.clamp(0.0, image.height() as f32 - 1.0) as u32,
        bottom.y.clamp(0.0, image.height() as f32 - 1.0) as u32,
    );
    (y0..=y1)
        .map(|y| {
            let p = image.get_pixel(px, y);
            (p[0], p[1], p[2])
        })
        .collect()
}

/// Whether any pixel of the screen region between the two page
/// corners is within `tol` of `target`.
#[allow(clippy::too_many_arguments)] // one picture-check seam
fn region_has_color(
    rig: &ShellRig,
    image: &image::RgbaImage,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    target: (u8, u8, u8),
    tol: u8,
) -> bool {
    let top_left = screen(rig, x0, y1);
    let bottom_right = screen(rig, x1, y0);
    let (px0, py0) = (
        top_left.x.clamp(0.0, image.width() as f32 - 1.0) as u32,
        top_left.y.clamp(0.0, image.height() as f32 - 1.0) as u32,
    );
    let (px1, py1) = (
        bottom_right.x.clamp(0.0, image.width() as f32 - 1.0) as u32,
        bottom_right.y.clamp(0.0, image.height() as f32 - 1.0) as u32,
    );
    (py0..=py1).any(|y| {
        (px0..=px1).any(|x| {
            let p = image.get_pixel(x, y);
            near((p[0], p[1], p[2]), target, tol)
        })
    })
}

/// Whether `colour` is within `tol` per channel of `target`.
fn near(colour: (u8, u8, u8), target: (u8, u8, u8), tol: u8) -> bool {
    let (a, b) = (
        [colour.0, colour.1, colour.2],
        [target.0, target.1, target.2],
    );
    (0..3).all(|c| a[c].abs_diff(b[c]) <= tol)
}

/// Item 9: while editing, one outline shows (the text block's), not
/// both the text block's and the shape's.
#[test]
fn while_editing_one_outline_shows() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the edit is open");
        // The picture with no tool cursor over the samples.
        let park = screen(&rig, 0.5, 8.0);
        rig.harness().event(egui::Event::PointerMoved(park));
        rig.run();
        let image = rig.render();
        let selection = {
            let colors = larrygram_ui::canvas::CanvasColors::from_theme(&sets().get(kind).theme);
            (
                colors.selection.r(),
                colors.selection.g(),
                colors.selection.b(),
            )
        };
        // The text block's outline shows (the editor rect: the text
        // extents from the block's top-left corner, inside the shape).
        assert!(
            region_has_color(&rig, &image, 2.2, 2.7, 3.8, 3.4, selection, 8),
            "{kind:?}: the text block's outline shows ({selection:?})"
        );
        // The shape's own top edge shows no second outline.
        let shape_edge = strip_colors(&rig, &image, SHAPE.0, 3.46, 3.54);
        assert!(
            shape_edge.iter().all(|c| !near(*c, selection, 8)),
            "{kind:?}: no second outline on the shape"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 1 (task P7-26): a text block resize lands where the preview
// frame put it. The command writes `TxtLocPinX`/`TxtLocPinY` with the
// pin: without them the renderer reads the cells' documented default,
// which follows the **shape's** width, and draws the block away from
// the frame the drag previewed.
// ---------------------------------------------------------------------------

/// Clicks the first widget labelled `name` whatever its role (the
/// menu rows of the two layouts draw under different roles).
fn click_any(rig: &mut ShellRig, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"));
    node.click();
    rig.run();
}

/// The topmost copy of the button `name`: the Shapes window's tiles
/// share some labels.
fn top_face(rig: &mut ShellRig, name: &str) -> egui::Rect {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .min_by(|a, b| {
            a.rect()
                .center()
                .y
                .total_cmp(&b.rect().center().y)
                .then(a.rect().center().x.total_cmp(&b.rect().center().x))
        })
        .unwrap_or_else(|| panic!("no button named {name:?}"))
        .rect()
}

/// Clicks the topmost copy of the widget `name`: the Shapes window's
/// tiles and the panes share some labels.
fn click_top(rig: &mut ShellRig, role: Role, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(role, name)
        .min_by(|a, b| {
            a.rect()
                .center()
                .y
                .total_cmp(&b.rect().center().y)
                .then(a.rect().center().x.total_cmp(&b.rect().center().x))
        })
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    node.click();
    rig.run();
}

/// Clicks the "More items" arrow nearest `face` (the Panel Modern
/// split arrows) and returns the face's position.
fn more_items_near(rig: &mut ShellRig, name: &str) -> egui::Pos2 {
    let face = top_face(rig, name);
    let root = rig.harness().root();
    let best = root
        .query_all_by_role_and_label(Role::Button, "More items")
        .min_by(|a, b| {
            (a.rect().center() - face.center())
                .length()
                .total_cmp(&(b.rect().center() - face.center()).length())
        })
        .expect("a More items arrow");
    best.click();
    rig.run();
    face.center()
}

/// Clicks the widget labelled `name` nearest `anchor` among the ones
/// strictly below it: the popup row under a split, never the split's
/// own face or a Shapes tile.
fn click_below(rig: &mut ShellRig, name: &str, anchor: egui::Pos2) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_label(name)
        .filter(|node| node.rect().center().y > anchor.y + 2.0)
        .min_by(|a, b| {
            (a.rect().center() - anchor)
                .length()
                .total_cmp(&(b.rect().center() - anchor).length())
        })
        .unwrap_or_else(|| panic!("no {name:?} below {anchor:?}"));
    node.click();
    rig.run();
}

/// Opens the fill colour picker of `kind` (the P7-24 helper's rule).
fn open_fill_picker(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Fill Color menu"),
        _ => {
            more_items_near(rig, "Fill");
        }
    }
}

/// Fills the selection white, so the black text ink reads against a
/// light fill (the picture checks of items 1 and 5).
fn white_fill(rig: &mut ShellRig, kind: LayoutKind) {
    open_fill_picker(rig, kind);
    click(rig, Role::Button, "White #FFFFFF");
}

/// Takes the line off the selection (its dark border would read as
/// text ink in the picture checks).
fn no_line(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Line Pattern menu");
            click(rig, Role::Button, "No Line");
        }
        _ => {
            let anchor = more_items_near(rig, "Line");
            click_below(rig, "No Line", anchor);
        }
    }
}

/// Chooses the Pointer tool with a real control click.
fn choose_pointer_tool(rig: &mut ShellRig) {
    click(rig, Role::Button, "Pointer Tool");
}

/// The Pointer tool and a click on empty page: no tool marks over the
/// picture's samples.
fn deselect(rig: &mut ShellRig) {
    choose_pointer_tool(rig);
    click_at(rig, screen(rig, 0.7, 5.0));
}

/// The rendered frame with the pointer parked on empty page (no tool
/// cursor over the samples).
fn quiet_picture(rig: &mut ShellRig) -> image::RgbaImage {
    let park = screen(rig, 0.7, 5.0);
    rig.harness().event(egui::Event::PointerMoved(park));
    rig.run();
    rig.render()
}

/// The page point of a screen point (the inverse of `screen`, which is
/// axis-aligned at any zoom).
fn to_page(rig: &ShellRig, pos: egui::Pos2) -> (f64, f64) {
    let origin = screen(rig, 0.0, 0.0);
    let along_x = screen(rig, 1.0, 0.0);
    let along_y = screen(rig, 0.0, 1.0);
    (
        f64::from(pos.x - origin.x) / f64::from(along_x.x - origin.x),
        f64::from(pos.y - origin.y) / f64::from(along_y.y - origin.y),
    )
}

/// The page-space bounding box `(min, max)` of the dark text ink in
/// `image` over the page region `bounds` (the picture checks of item
/// 1 and 5: the page is white and the grid light grey, so the black
/// text is the only dark ink).
fn ink_box(
    rig: &ShellRig,
    image: &image::RgbaImage,
    bounds: (f64, f64, f64, f64),
) -> ((f64, f64), (f64, f64)) {
    let top_left = screen(rig, bounds.0, bounds.3);
    let bottom_right = screen(rig, bounds.2, bounds.1);
    let (px0, py0) = (top_left.x.max(0.0) as u32, top_left.y.max(0.0) as u32);
    let (px1, py1) = (
        bottom_right.x.min(image.width() as f32) as u32,
        bottom_right.y.min(image.height() as f32) as u32,
    );
    let mut min = (u32::MAX, u32::MAX);
    let mut max = (0u32, 0u32);
    for y in py0..py1 {
        for x in px0..px1 {
            let p = image.get_pixel(x, y);
            if p[0] < 170 && p[1] < 170 && p[2] < 170 {
                min = (min.0.min(x), min.1.min(y));
                max = (max.0.max(x), max.1.max(y));
            }
        }
    }
    assert!(min.0 <= max.0, "no ink in the region {bounds:?}");
    // The ink must sit clear of the region's own edges: a ruler band
    // or another control over the region would pollute the measure.
    assert!(
        min.0 > px0 + 3 && min.1 > py0 + 3 && max.0 + 3 < px1 && max.1 + 3 < py1,
        "the ink runs into the region's edge {bounds:?}: screen {min:?}..{max:?}"
    );
    // Screen (left, bottom) is page (min x, min y); `y` grows up.
    let lo = to_page(rig, egui::pos2(min.0 as f32, max.1 as f32));
    let hi = to_page(rig, egui::pos2(max.0 as f32, min.1 as f32));
    ((lo.0, lo.1), (hi.0, hi.1))
}

/// The ink's top-left corner in page inches (`y` grows up: the top of
/// the ink is its `max.y`), the anchor of a top-left laid out text.
fn ink_top_left(ink: ((f64, f64), (f64, f64))) -> (f64, f64) {
    ((ink.0).0, (ink.1).1)
}

/// Item 1: a block resize on the fixture (explicit `Txt*` cells)
/// lands where the preview frame put it: the ink keeps its place at
/// the block's top-left corner and the cells carry the pin's offset
/// inside the block.
#[test]
fn the_text_block_resize_lands_where_the_preview_showed_it() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        select_shape(&mut rig);
        white_fill(&mut rig, kind);
        deselect(&mut rig);
        let image = quiet_picture(&mut rig);
        let before = ink_top_left(ink_box(&rig, &image, (2.0, 2.4, 4.0, 3.6)));

        // Drag the block's north-east corner half an inch right and a
        // quarter up (the block is 1 x 0.5 at (3, 3)).
        select_shape(&mut rig);
        choose_text_block(&mut rig, kind);
        let from = screen(&rig, 3.5, 3.25);
        let to = screen(&rig, 4.0, 3.5);
        drag(&mut rig, from, to);
        // The resize writes the pin's offset inside the block too.
        assert!(
            (number(&rig, shape, "TxtWidth") - 1.5).abs() < 1.0e-9,
            "{kind:?}: TxtWidth grew"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinX") - 0.75).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinX follows the size"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinY") - 0.375).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinY follows the size"
        );
        // The preview keeps the block's south-west corner (task P7-26
        // item 8), so only the top edge rises and the ink follows it.
        deselect(&mut rig);
        let image = quiet_picture(&mut rig);
        let after = ink_top_left(ink_box(&rig, &image, (1.8, 2.3, 4.2, 3.7)));
        let expected = (before.0, before.1 + 0.25);
        assert!(
            (after.0 - expected.0).abs() < 0.05 && (after.1 - expected.1).abs() < 0.05,
            "{kind:?}: the ink sits where the preview showed it: {before:?} to {after:?}"
        );
    }
}

/// Chooses the Rectangle tool with a real control click (task P7-26
/// item 1: the shape it draws carries no `Txt*` cells at all).
fn choose_rectangle_tool(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            // The tool sits on the hidden floating Drawing toolbar
            // (View > Toolbars > Drawing).
            click_any(rig, "View");
            click_any(rig, "Toolbars");
            click_any(rig, "Drawing");
            click(rig, Role::Button, "Rectangle Tool");
        }
        _ => {
            // The Home tab's drawing-tools split opens from its arrow.
            let anchor = more_items_near(rig, "Rectangle");
            click_below(rig, "Rectangle", anchor);
        }
    }
}

/// Item 1: the same check on a shape drawn with the Rectangle tool,
/// whose `Txt*` cells hold nothing of their own — the case the review
/// found: the documented default follows the **shape's** width, so a
/// resize that writes the pin alone drew the block off the preview.
#[test]
fn a_rectangle_tool_shape_resizes_onto_the_preview_too() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        // Draw a rectangle on empty page (the drag snaps to the
        // grid, so the box is read back from the shape's cells).
        choose_rectangle_tool(&mut rig, kind);
        let from = screen(&rig, 3.2, 3.9);
        let to = screen(&rig, 5.2, 4.9);
        drag(&mut rig, from, to);
        let shape = last_shape(&rig);
        let (pin_x, pin_y) = (number(&rig, shape, "PinX"), number(&rig, shape, "PinY"));
        let (loc_x, loc_y) = (
            number(&rig, shape, "LocPinX"),
            number(&rig, shape, "LocPinY"),
        );
        let (width, height) = (number(&rig, shape, "Width"), number(&rig, shape, "Height"));
        let min = (pin_x - loc_x, pin_y - loc_y);
        let max = (min.0 + width, min.1 + height);
        // Give it text: a double click opens the editor, Escape keeps
        // the text and leaves the shape selected.
        choose_pointer_tool(&mut rig);
        let pos = screen(&rig, pin_x, pin_y);
        double_click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the double click opens the editor"
        );
        type_text(&mut rig, "Hi");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(shape_text(&rig, shape), "Hi", "{kind:?}: the text lands");
        white_fill(&mut rig, kind);
        no_line(&mut rig, kind);
        // The drawn shape carries no `Txt*` values of its own: the
        // insert's derived-cell pass materialises the documented
        // defaults, which follow the **shape's** width and height.
        assert!(
            (number(&rig, shape, "TxtWidth") - width).abs() < 1.0e-9,
            "{kind:?}: TxtWidth is the shape's width"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinX") - width / 2.0).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinX follows the shape's width"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinY") - height / 2.0).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinY follows the shape's height"
        );
        // The ink runs (min - 0.4, max + 0.17) at its lowest: keep
        // the region over the page area both layouts show.
        let region = (min.0 - 1.0, min.1 + 0.2, min.0 + 1.5, min.1 + 1.5);
        deselect(&mut rig);
        let image = quiet_picture(&mut rig);
        let before = ink_top_left(ink_box(&rig, &image, region));

        // Resize the text block (the whole shape box) from its
        // north-east corner: +1 in width and +0.5 in height.
        let pos = screen(&rig, pin_x, pin_y);
        click_at(&mut rig, pos);
        choose_text_block(&mut rig, kind);
        let from = screen(&rig, max.0, max.1);
        let to = screen(&rig, max.0 + 1.0, max.1 + 0.5);
        drag(&mut rig, from, to);
        assert!(
            (number(&rig, shape, "TxtWidth") - (width + 1.0)).abs() < 1.0e-9,
            "{kind:?}: TxtWidth grew"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinX") - (width + 1.0) / 2.0).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinX is written with the pin"
        );
        assert!(
            (number(&rig, shape, "TxtLocPinY") - (height + 0.5) / 2.0).abs() < 1.0e-9,
            "{kind:?}: TxtLocPinY is written with the pin"
        );
        deselect(&mut rig);
        let image = quiet_picture(&mut rig);
        let after = ink_top_left(ink_box(&rig, &image, region));
        // The preview keeps the block's south-west corner (task P7-26
        // item 8): the left edge stands still and only the top edge
        // rises, so the ink follows the top edge alone.
        let expected = (before.0, before.1 + 0.5);
        assert!(
            (after.0 - expected.0).abs() < 0.05 && (after.1 - expected.1).abs() < 0.05,
            "{kind:?}: the ink sits where the preview showed it: {before:?} to {after:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 2 (task P7-26): a typed character starts shape text only while
// the canvas holds the keyboard. Another widget's field and an open
// menu keep their own keys.
// ---------------------------------------------------------------------------

/// Item 2: Tab moves egui's keyboard focus off the canvas (the shell's
/// focus scope stays `Page`, nothing was pressed) and the typed
/// character goes to that widget, not to the selected shape.
#[test]
fn a_typed_character_keeps_out_of_another_widgets_keyboard() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        select_shape(&mut rig);
        press_key(&mut rig, egui::Key::Tab, egui::Modifiers::NONE);
        rig.run();
        let focused = rig.harness().ctx.memory(|memory| memory.focused());
        assert!(
            focused.is_some(),
            "{kind:?}: Tab gives another widget the keyboard"
        );
        type_text(&mut rig, "Q");
        assert!(
            !editing(&mut rig),
            "{kind:?}: the focused widget keeps its typing"
        );
        assert_eq!(
            shape_text(&rig, shape),
            "Hello",
            "{kind:?}: no shape text starts"
        );
    }
}

/// Item 2: the Format menu opened **from the keyboard** keeps its
/// access letters (they reach the input layer as typed text) and
/// starts no shape text.
#[test]
fn a_keyboard_menu_keeps_its_access_letters_and_starts_no_text() {
    let kind = LayoutKind::PanelClassic;
    let mut rig = shell_rig(kind);
    let shape = rectangle(&rig.shell.borrow().app);
    select_shape(&mut rig);
    assert!(
        !has(&mut rig, Role::MenuItem, "Fill Color"),
        "the menu is closed"
    );
    // Alt+O opens the Format menu; "g" is its Fill Color access key.
    rig.harness()
        .key_press_modifiers(egui::Modifiers::ALT, egui::Key::O);
    rig.run();
    assert!(
        has(&mut rig, Role::MenuItem, "Fill Color"),
        "Alt+O opens the Format menu"
    );
    type_text(&mut rig, "g");
    assert!(
        has(&mut rig, Role::MenuItem, "Red"),
        "the access letter opens the Fill Color submenu"
    );
    assert!(
        !editing(&mut rig),
        "{kind:?}: the access letter starts no shape text"
    );
    assert_eq!(
        shape_text(&rig, shape),
        "Hello",
        "{kind:?}: the shape's text is untouched"
    );
    press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
    press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(
        !editing(&mut rig),
        "Escape closes the menu, not a text edit"
    );
}

/// Item 2: a context menu opened from the keyboard (Shift+F10) over the
/// canvas keeps its own keys too.
#[test]
fn a_context_menu_from_the_keyboard_starts_no_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = rectangle(&rig.shell.borrow().app);
        select_shape(&mut rig);
        assert!(
            !has(&mut rig, Role::MenuItem, "Delete"),
            "{kind:?}: the context menu is closed"
        );
        press_key(
            &mut rig,
            egui::Key::F10,
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::NONE
            },
        );
        assert!(
            has(&mut rig, Role::MenuItem, "Delete"),
            "{kind:?}: Shift+F10 opens the context menu"
        );
        type_text(&mut rig, "x");
        assert!(!editing(&mut rig), "{kind:?}: the open menu keeps its keys");
        assert_eq!(
            shape_text(&rig, shape),
            "Hello",
            "{kind:?}: the shape's text is untouched"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 3 (task P7-26): a click inside the new text box being typed is
// a caret move, not a commit and a second box over the first.
// ---------------------------------------------------------------------------

/// Item 3: the click lower in the outlined box keeps the edit (the box
/// is still being typed, nothing is inserted early) and no second box
/// opens; Escape then inserts exactly one box with the typed text.
#[test]
fn a_click_inside_the_new_text_box_keeps_editing_it() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let before = shape_count(&rig);
        // A click on empty page starts a 2 x 0.5 box at (1, 5) as its
        // lower-left corner.
        let pos = screen(&rig, 1.0, 5.0);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the click starts a text box");
        type_text(&mut rig, "Hi");
        // A click lower in the outlined box, below the first row.
        let lower = screen(&rig, 2.0, 5.1);
        click_at(&mut rig, lower);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click inside the box keeps editing"
        );
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: the click neither commits nor opens a second box"
        );
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(
            shape_count(&rig),
            before + 1,
            "{kind:?}: exactly one text box"
        );
        let shape = last_shape(&rig);
        assert_eq!(
            shape_text(&rig, shape),
            "Hi",
            "{kind:?}: the typed text survived the click"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 8 (task P7-26): a resize drag keeps the opposite edge or corner
// fixed and is measured along the shape's own axes.
// ---------------------------------------------------------------------------

/// Rotates the selection one quarter turn right with the layout's own
/// control (task P7-26 item 8).
fn rotate_right(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Shape");
            click(rig, Role::MenuItem, "Rotate or Flip");
            click(rig, Role::MenuItem, "Rotate Right");
        }
        _ => {
            // The Position dropdown opens from its own face; the rows
            // are its menu.
            let anchor = top_face(rig, "Position").center();
            click_top(rig, Role::Button, "Position");
            click_below(rig, "Rotate Shapes", anchor);
            click_below(rig, "Rotate Right 90\u{b0}", anchor);
        }
    }
    rig.run();
}

/// The page-space bounding box of the drawn handle frame in `image`
/// over the page region `bounds`: the selection-colour outline and
/// handles (the picture check of item 8). The handle squares stick out
/// a few pixels, which the checks allow for.
fn frame_box(
    rig: &ShellRig,
    image: &image::RgbaImage,
    bounds: (f64, f64, f64, f64),
    kind: LayoutKind,
) -> ((f64, f64), (f64, f64)) {
    let colors = larrygram_ui::canvas::CanvasColors::from_theme(&sets().get(kind).theme);
    let target = (
        colors.selection.r(),
        colors.selection.g(),
        colors.selection.b(),
    );
    let top_left = screen(rig, bounds.0, bounds.3);
    let bottom_right = screen(rig, bounds.2, bounds.1);
    let (px0, py0) = (top_left.x.max(0.0) as u32, top_left.y.max(0.0) as u32);
    let (px1, py1) = (
        bottom_right.x.min(image.width() as f32) as u32,
        bottom_right.y.min(image.height() as f32) as u32,
    );
    let mut min = (u32::MAX, u32::MAX);
    let mut max = (0u32, 0u32);
    for y in py0..py1 {
        for x in px0..px1 {
            let p = image.get_pixel(x, y);
            if near((p[0], p[1], p[2]), target, 24) {
                min = (min.0.min(x), min.1.min(y));
                max = (max.0.max(x), max.1.max(y));
            }
        }
    }
    assert!(min.0 <= max.0, "no handle frame in the region {bounds:?}");
    let lo = to_page(rig, egui::pos2(min.0 as f32, max.1 as f32));
    let hi = to_page(rig, egui::pos2(max.0 as f32, min.1 as f32));
    ((lo.0, lo.1), (hi.0, hi.1))
}

/// The frame's side in the direction `unit` (one of the four unit
/// vectors: the angles of item 8 are 0 and 90 degrees), as a page
/// coordinate.
fn side(frame: ((f64, f64), (f64, f64)), unit: (f64, f64)) -> f64 {
    if unit.0 > 0.5 {
        (frame.1).0
    } else if unit.0 < -0.5 {
        (frame.0).0
    } else if unit.1 > 0.5 {
        (frame.1).1
    } else {
        (frame.0).1
    }
}

/// The midpoint of the frame's side in the direction `unit` (where its
/// resize handle sits).
fn side_mid(frame: ((f64, f64), (f64, f64)), unit: (f64, f64)) -> (f64, f64) {
    let center = (
        ((frame.0).0 + (frame.1).0) / 2.0,
        ((frame.0).1 + (frame.1).1) / 2.0,
    );
    if unit.0.abs() > 0.5 {
        (side(frame, unit), center.1)
    } else {
        (center.0, side(frame, unit))
    }
}

/// One placed box (the shape's or the text block's placement): the
/// pin's offset `loc` inside the box, the box size and the turn. The
/// fixtures carry no flip.
#[derive(Debug, Clone, Copy)]
struct PlacedBox {
    pin: (f64, f64),
    loc: (f64, f64),
    size: (f64, f64),
    angle: f64,
}

impl PlacedBox {
    /// The page point of the box's `at` coordinate.
    fn point(&self, at: (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (at.0 - self.loc.0, at.1 - self.loc.1);
        (
            self.pin.0 + dx * cos - dy * sin,
            self.pin.1 + dx * sin + dy * cos,
        )
    }

    /// The unit vector of the box's own x axis (its `east` side).
    fn east(&self) -> (f64, f64) {
        let (sin, cos) = self.angle.sin_cos();
        (cos.round(), sin.round())
    }

    /// The midpoint of the box's east (`plus`) or west side.
    fn side(&self, plus: bool) -> (f64, f64) {
        let x = if plus { self.size.0 } else { 0.0 };
        self.point((x, self.size.1 / 2.0))
    }
}

/// The number value of one cell of `shape`, panicking when absent.
fn number_of(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
    cell_number(rig, shape, name).unwrap_or_else(|| panic!("no {name} on the shape"))
}

/// The shape's own placement from its cells (task P7-26 item 8).
fn shape_box(rig: &ShellRig, shape: ShapeId) -> PlacedBox {
    PlacedBox {
        pin: (number_of(rig, shape, "PinX"), number_of(rig, shape, "PinY")),
        loc: (
            number_of(rig, shape, "LocPinX"),
            number_of(rig, shape, "LocPinY"),
        ),
        size: (
            number_of(rig, shape, "Width"),
            number_of(rig, shape, "Height"),
        ),
        angle: number_of(rig, shape, "Angle"),
    }
}

/// The text block's placement in page inches: the block's own
/// placement inside the shape's (the fixtures carry no `TxtAngle` and
/// no flip).
fn text_block_box(rig: &ShellRig, shape: ShapeId) -> PlacedBox {
    let outer = shape_box(rig, shape);
    let block = PlacedBox {
        pin: (
            number_of(rig, shape, "TxtPinX"),
            number_of(rig, shape, "TxtPinY"),
        ),
        loc: (
            number_of(rig, shape, "TxtLocPinX"),
            number_of(rig, shape, "TxtLocPinY"),
        ),
        size: (
            number_of(rig, shape, "TxtWidth"),
            number_of(rig, shape, "TxtHeight"),
        ),
        angle: 0.0,
    };
    let origin = outer.point(block.pin);
    PlacedBox {
        pin: origin,
        loc: block.loc,
        size: block.size,
        angle: outer.angle + block.angle,
    }
}

/// One resize drag on the frame's east side (task P7-26 item 8): the
/// dragged handle follows the pointer and the opposite side stays
/// put, in the cells and in the rendered frame. `placed` builds the
/// dragged frame's placement from the cells before and after the drag.
fn check_resize_drag(
    rig: &mut ShellRig,
    kind: LayoutKind,
    shape: ShapeId,
    region: (f64, f64, f64, f64),
    placed: fn(&ShellRig, ShapeId) -> PlacedBox,
    what: &str,
) {
    let before = placed(rig, shape);
    let unit = before.east();
    let handle = before.side(true);
    let fixed_before = before.side(false);
    let image = quiet_picture(rig);
    let frame_before = frame_box(rig, &image, region, kind);
    let to = (handle.0 + 0.5 * unit.0, handle.1 + 0.5 * unit.1);
    let from = screen(rig, handle.0, handle.1);
    let to_screen = screen(rig, to.0, to.1);
    drag(rig, from, to_screen);
    let after = placed(rig, shape);
    assert!(
        (after.size.0 - (before.size.0 + 0.5)).abs() < 1.0e-6,
        "{kind:?}: the {what} grew by the drag's distance ({:?})",
        after.size
    );
    assert!(
        (after.size.1 - before.size.1).abs() < 1.0e-6,
        "{kind:?}: the other axis keeps its size"
    );
    let dragged = after.side(true);
    assert!(
        (dragged.0 - to.0).abs() < 0.01 && (dragged.1 - to.1).abs() < 0.01,
        "{kind:?}: the {what} handle follows the pointer: {dragged:?} vs {to:?}"
    );
    let fixed_after = after.side(false);
    assert!(
        (fixed_after.0 - fixed_before.0).abs() < 1.0e-6
            && (fixed_after.1 - fixed_before.1).abs() < 1.0e-6,
        "{kind:?}: the opposite side stays put: {fixed_before:?} to {fixed_after:?}"
    );
    // And the rendered frame moves with it (its east and west sides).
    let image = quiet_picture(rig);
    let frame_after = frame_box(rig, &image, region, kind);
    // The side in the `unit` direction moves by the drag along it.
    let moved = side(frame_after, unit);
    let stayed = side(frame_after, (-unit.0, -unit.1));
    assert!(
        (moved - (side(frame_before, unit) + 0.5 * (unit.0 + unit.1))).abs() < 0.06,
        "{kind:?}: the drawn frame follows the pointer ({moved})"
    );
    assert!(
        (stayed - side(frame_before, (-unit.0, -unit.1))).abs() < 0.06,
        "{kind:?}: the drawn frame keeps the opposite side ({stayed})"
    );
}

/// Item 8: the Text Block tool's resize drag keeps the opposite edge
/// fixed and is measured along the block's own axes, at 0 and at 90
/// degrees.
#[test]
fn a_text_block_resize_drag_follows_the_pointer_at_0_and_90_degrees() {
    for kind in LAYOUTS {
        for turned in [false, true] {
            let mut rig = shell_rig(kind);
            select_shape(&mut rig);
            if turned {
                rotate_right(&mut rig, kind);
            }
            let shape = rectangle(&rig.shell.borrow().app);
            let angle = shape_box(&rig, shape).angle;
            assert!(
                if turned {
                    (angle.abs() - core::f64::consts::FRAC_PI_2).abs() < 1.0e-9
                } else {
                    angle.abs() < 1.0e-9
                },
                "{kind:?}: the shape turned one quarter ({angle})"
            );
            choose_text_block(&mut rig, kind);
            check_resize_drag(
                &mut rig,
                kind,
                shape,
                (1.4, 1.4, 4.6, 4.6),
                text_block_box,
                "text block",
            );
        }
    }
}

/// Item 8: the pointer tool's resize drag keeps the opposite edge
/// fixed and is measured along the shape's own axes, at 0 and at 90
/// degrees.
#[test]
fn a_pointer_resize_drag_follows_the_pointer_at_0_and_90_degrees() {
    for kind in LAYOUTS {
        for turned in [false, true] {
            let mut rig = shell_rig(kind);
            select_shape(&mut rig);
            if turned {
                rotate_right(&mut rig, kind);
            }
            let shape = rectangle(&rig.shell.borrow().app);
            let angle = shape_box(&rig, shape).angle;
            assert!(
                if turned {
                    (angle.abs() - core::f64::consts::FRAC_PI_2).abs() < 1.0e-9
                } else {
                    angle.abs() < 1.0e-9
                },
                "{kind:?}: the shape turned one quarter ({angle})"
            );
            check_resize_drag(
                &mut rig,
                kind,
                shape,
                (1.4, 1.4, 4.6, 4.6),
                shape_box,
                "shape",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Item 9 (task P7-26): a drag sizes the new text box.
// ---------------------------------------------------------------------------

/// Item 9: a drag on empty page makes a box of the dragged size at the
/// dragged place, and a click keeps the default box (task P7-25 item
/// 9's 2 x 0.5).
#[test]
fn a_drag_with_the_text_tool_sizes_the_new_text_box() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let before = shape_count(&rig);
        // A drag from (1, 5) to (3, 5.4).
        let from = screen(&rig, 1.0, 5.0);
        let to = screen(&rig, 3.0, 5.4);
        drag(&mut rig, from, to);
        assert!(editing(&mut rig), "{kind:?}: the drag starts a text box");
        type_text(&mut rig, "Hi");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(shape_count(&rig), before + 1, "{kind:?}: one text box");
        let shape = last_shape(&rig);
        assert_eq!(shape_text(&rig, shape), "Hi", "{kind:?}: the text lands");
        // The screen-to-page conversion rounds in f32: a fraction of a
        // thousandth of an inch is noise.
        assert!(
            (number(&rig, shape, "Width") - 2.0).abs() < 1.0e-4
                && (number(&rig, shape, "Height") - 0.4).abs() < 1.0e-4,
            "{kind:?}: the box is the dragged size ({:?})",
            (number(&rig, shape, "Width"), number(&rig, shape, "Height"))
        );
        assert!(
            (number(&rig, shape, "PinX") - 2.0).abs() < 1.0e-4
                && (number(&rig, shape, "PinY") - 5.2).abs() < 1.0e-4,
            "{kind:?}: at the dragged place"
        );
        // A click keeps the default 2 x 0.5 box.
        let pos = screen(&rig, 6.0, 5.6);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the click starts a text box");
        type_text(&mut rig, "A");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        let shape = last_shape(&rig);
        assert!(
            (number(&rig, shape, "Width") - 2.0).abs() < 1.0e-4
                && (number(&rig, shape, "Height") - 0.5).abs() < 1.0e-4,
            "{kind:?}: a click keeps the default box"
        );
    }
}
