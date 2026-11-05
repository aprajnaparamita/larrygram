//! Connection points under real input (task P7-34), in Panel Modern
//! and Panel Classic:
//!
//! - the Connection Points check turns the markers on in the picture
//!   and off again;
//! - the Connection Point tool adds a point with Ctrl+click (cells and
//!   picture), and Esc returns to the Pointer Tool;
//! - a connector drawn to the new point follows it when a plain drag
//!   moves the point (cells and picture);
//! - the point's context menu sets Outward (cell);
//! - Delete re-glues the point's end dynamically (formula and record);
//! - undo restores every step.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{ActiveTool, AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The page point of the fixture shape's pin.
const A: (f64, f64) = (3.0, 3.0);

/// The fixture shape's two connection points in page inches (the
/// local `(0.25, 0.25)` and `(1.75, 0.25)` of a 2 x 1 box at [`A`]).
const POINTS: [(f64, f64); 2] = [(2.25, 2.75), (3.75, 2.75)];

/// Where the tool adds a point (page inches).
const ADDED: (f64, f64) = (2.5, 2.5);

/// Where the drag moves it (page inches).
const MOVED: (f64, f64) = (2.75, 3.25);

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// The 2 x 1 box at [`A`] with two connection points.
fn shape_with_points(visio_id: i32) -> Shape {
    let mut shape = fixture_shape(A.0, A.1, visio_id);
    let mut section = Section::new(SectionName::from_static("Connection"));
    for (ix, (px, py)) in [(0.25, 0.25), (1.75, 0.25)].into_iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        section.rows.push(row);
    }
    shape.sheet.sections.push(section);
    shape
}

/// A shell with one shape (and its two points) on one page, centred
/// in the viewport (the text fixtures' rule).
fn rig(kind: LayoutKind) -> ShellRig {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let shape = document.add_shape(shape_with_points(1));
    page.shapes.push(shape);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    let mut rig = ShellRig::with_app(app);
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
            f64::from(viewport.width()) / 2.0 / k - 5.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 3.0),
        );
    }
    rig.run();
    rig.run();
    rig
}

fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(Point::new(x, y))
        .expect("the canvas drew")
}

/// Clicks the first widget with `role` and `name` (a real control
/// click).
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

/// A primary press and release in one frame at `pos`.
fn click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    crate::review::click_at(rig, pos, egui::PointerButton::Primary);
}

/// A Ctrl+click (press and release with Command held) at `pos`.
fn ctrl_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    let modifiers = egui::Modifiers::COMMAND;
    rig.harness()
        .event(egui::Event::ModifiersChanged(modifiers));
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    rig.run();
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

/// A plain drag from `from` to `to` (the connector draw).
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
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

/// One key press (down and up) with no modifiers.
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

/// One Command chord (the undo step).
fn chord(rig: &mut ShellRig, key: egui::Key) {
    let modifiers = egui::Modifiers::COMMAND;
    rig.harness()
        .event(egui::Event::ModifiersChanged(modifiers));
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
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

fn snapshot(rig: &ShellRig) -> Document {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone()
}

/// The shape's own `Connection` rows.
fn connection_rows(document: &Document, shape: ShapeId) -> Vec<Row> {
    let entity = document.shape(shape).expect("shape");
    let mut rows = Vec::new();
    for section in &entity.sheet.sections {
        if section.name.as_str() == "Connection" {
            rows.extend(section.rows.iter().cloned());
        }
    }
    rows
}

/// The formula of the shape's own cell.
fn formula(document: &Document, shape: ShapeId, name: &str) -> Option<String> {
    document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.formula.clone())
}

/// The number of the shape's own cell.
fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    document
        .shape(shape)
        .and_then(|entity| {
            entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
        })
        .and_then(|cell| cell.value.clone())
        .and_then(|value| match value {
            CellValue::Number(number) => Some(number),
            _ => None,
        })
        .unwrap_or(f64::NAN)
}

/// The page's `Connects` records naming `connector`'s begin or end.
fn records_of(document: &Document, connector_visio: i32) -> Vec<larrygram_model::Connect> {
    document
        .page(PageId { index: 0 })
        .expect("page")
        .connects
        .iter()
        .filter(|record| record.from_sheet == connector_visio)
        .cloned()
        .collect()
}

/// The last shape on the page: the drawn connector, with its Visio id.
fn drawn_connector(rig: &ShellRig) -> (ShapeId, i32) {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    let shapes = session.document.page(page).expect("page").shapes.clone();
    let connector = *shapes.last().expect("the drawn connector");
    let visio = session
        .document
        .shape(connector)
        .expect("connector")
        .visio_id;
    (connector, visio)
}

/// The Connection Point tool button's label in one layout (Panel
/// Classic's sits in the Connector Tool split's menu).
fn point_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Connection Point Tool",
        _ => "Connection Point",
    }
}

/// Arms the Connection Point tool with a real control click (task
/// P7-34 item 2).
fn arm_point_tool(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Connector Tool menu");
            click(rig, Role::MenuItem, point_tool_button(kind));
        }
        _ => click(rig, Role::Button, point_tool_button(kind)),
    }
}

/// Flips the Connection Points check from its control (task P7-34
/// item 1: Panel Modern's View > Visual Aids check, Panel Classic's
/// View menu row).
fn flip_check(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "View");
            click(rig, Role::MenuItem, "Connection Points");
        }
        _ => {
            rig.click_named("View");
            rig.run();
            click(rig, Role::CheckBox, "Connection Points");
        }
    }
}

/// Selects the fixture shape with a real click on its centre.
fn select_shape(rig: &mut ShellRig) {
    let at = screen(rig, A.0, A.1);
    click_at(rig, at);
}

/// Draws one connector with the Connector tool from `from` to `to`
/// and returns to the Pointer Tool.
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind, from: (f64, f64), to: (f64, f64)) {
    match kind {
        // The split remembers the last tool (its face may say
        // "Connection Point Tool"), so the row opens from the menu.
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "Connector Tool menu");
            click(rig, Role::MenuItem, "Connector Tool");
        }
        _ => click(rig, Role::Button, "Connector"),
    }
    drag(rig, screen(rig, from.0, from.1), screen(rig, to.0, to.1));
    click(rig, Role::Button, "Pointer Tool");
}

/// The pixel at a screen point.
fn pixel(image: &image::RgbaImage, at: egui::Pos2) -> image::Rgba<u8> {
    let px = at.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let py = at.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    *image.get_pixel(px, py)
}

/// Whether new ink appeared within `radius` screen pixels of a page
/// point (markers are hollow, so the exact centre may stay empty).
fn changed_near(
    before: &image::RgbaImage,
    after: &image::RgbaImage,
    rig: &ShellRig,
    at: (f64, f64),
    radius: f32,
) -> bool {
    let center = screen(rig, at.0, at.1);
    for y in -radius as i32..=radius as i32 {
        for x in -radius as i32..=radius as i32 {
            let pos = egui::pos2(center.x + x as f32, center.y + y as f32);
            let a = pixel(before, pos);
            let b = pixel(after, pos);
            if a.0.iter().zip(b.0.iter()).any(|(u, v)| u.abs_diff(*v) > 12) {
                return true;
            }
        }
    }
    false
}

/// Whether two numbers agree to `eps`.
fn assert_close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-6, "{what}: {a} != {b}");
}

/// Item 1: the check draws every shape's points as markers and hides
/// them again, whatever the layout.
fn the_view_check_shows_and_hides_the_markers(kind: LayoutKind) {
    let mut rig = rig(kind);
    let before = rig.render();
    flip_check(&mut rig, kind);
    assert!(
        rig.shell.borrow().app.show_connection_points,
        "{kind:?}: the check turns on"
    );
    let on = rig.render();
    for point in POINTS {
        assert!(
            changed_near(&before, &on, &rig, point, 6.0),
            "{kind:?}: the marker shows at {point:?}"
        );
    }
    flip_check(&mut rig, kind);
    assert!(
        !rig.shell.borrow().app.show_connection_points,
        "{kind:?}: the check turns off"
    );
    let off = rig.render();
    for point in POINTS {
        assert!(
            changed_near(&on, &off, &rig, point, 6.0),
            "{kind:?}: the marker goes away at {point:?}"
        );
    }
}

/// Item 2: Ctrl+click adds a point at the local position and selects
/// it; Esc returns to the Pointer Tool; one undo restores the rows.
fn the_tool_adds_a_point_on_ctrl_click(kind: LayoutKind) {
    let mut rig = rig(kind);
    select_shape(&mut rig);
    arm_point_tool(&mut rig, kind);
    let before = rig.render();
    let added_at = screen(&rig, ADDED.0, ADDED.1);
    ctrl_click_at(&mut rig, added_at);

    let document = snapshot(&rig);
    let rows = connection_rows(&document, ShapeId { index: 0 });
    assert_eq!(rows.len(), 3, "{kind:?}: the point was added");
    let added = rows.last().expect("the new row");
    let x = added
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "X")
        .and_then(|cell| cell.value.clone());
    assert!(
        matches!(x, Some(CellValue::Number(value)) if (value - 0.5).abs() < 1e-6),
        "{kind:?}: the point lands at the local (0.5, 0): {x:?}"
    );

    // The picture: the new point's marker (the selected style).
    let after = rig.render();
    assert!(
        changed_near(&before, &after, &rig, ADDED, 6.0),
        "{kind:?}: the new point draws"
    );

    // Esc returns to the Pointer Tool (task P7-34 item 2).
    press_key(&mut rig, egui::Key::Escape);
    assert_eq!(
        rig.shell.borrow().app.active_tool,
        ActiveTool::Pointer,
        "{kind:?}: Esc returns to the Pointer Tool"
    );

    // One undo restores the exact rows.
    chord(&mut rig, egui::Key::Z);
    let document = snapshot(&rig);
    assert_eq!(
        connection_rows(&document, ShapeId { index: 0 }).len(),
        2,
        "{kind:?}: the undo removes the point"
    );
}

/// Item 4: a connector drawn to the new point follows it when the
/// point moves (cells and picture), and one undo restores both.
fn a_moved_point_moves_the_connector_end(kind: LayoutKind) {
    let mut rig = rig(kind);
    select_shape(&mut rig);
    arm_point_tool(&mut rig, kind);
    let added_at = screen(&rig, ADDED.0, ADDED.1);
    ctrl_click_at(&mut rig, added_at);

    // Draw a connector whose end lands on the new point.
    draw_connector(&mut rig, kind, (4.5, ADDED.1), ADDED);
    let (connector, _) = drawn_connector(&rig);
    {
        let document = snapshot(&rig);
        assert!(
            formula(&document, connector, "EndX").is_some_and(|text| text.starts_with("PAR(PNT(")),
            "{kind:?}: the end glues to the point"
        );
    }

    // Select the shape again, arm the tool and drag the point straight
    // away (a plain drag, no Ctrl: the user's 2026-11-05 pass).
    select_shape(&mut rig);
    arm_point_tool(&mut rig, kind);
    let added_at = screen(&rig, ADDED.0, ADDED.1);
    let before = rig.render();
    let moved_at = screen(&rig, MOVED.0, MOVED.1);
    drag(&mut rig, added_at, moved_at);

    // Cells: the glued end follows the moved point.
    let document = snapshot(&rig);
    assert_close(
        number(&document, connector, "EndX"),
        MOVED.0,
        "the end follows the point (x)",
    );
    assert_close(
        number(&document, connector, "EndY"),
        MOVED.1,
        "the end follows the point (y)",
    );
    assert!(
        formula(&document, connector, "EndX").is_some_and(|text| text.starts_with("PAR(PNT(")),
        "the glue formula stays"
    );

    // Picture: the point marker (and the line end) moved.
    let after = rig.render();
    assert!(
        changed_near(&before, &after, &rig, ADDED, 8.0)
            && changed_near(&before, &after, &rig, MOVED, 8.0),
        "{kind:?}: the moved point and the end draw at the new place"
    );

    // One undo restores the point and the route together.
    chord(&mut rig, egui::Key::Z);
    let document = snapshot(&rig);
    assert_close(
        number(&document, connector, "EndX"),
        ADDED.0,
        "the undo moves the end back",
    );
    assert_close(
        number(&document, connector, "EndY"),
        ADDED.1,
        "the undo moves the end back",
    );
}

/// Item 3: the point's right-click menu sets Outward, and one undo
/// restores the rows.
fn the_context_menu_sets_the_point_type(kind: LayoutKind) {
    let mut rig = rig(kind);
    select_shape(&mut rig);
    arm_point_tool(&mut rig, kind);
    let point_at = screen(&rig, POINTS[0].0, POINTS[0].1);
    click_at(&mut rig, point_at);
    crate::review::click_at(&mut rig, point_at, egui::PointerButton::Secondary);
    rig.run();
    assert!(
        rig.has("Inward") && rig.has("Inward & Outward"),
        "{kind:?}: the connection-point menu opens"
    );
    rig.click_named("Outward");
    rig.run();

    let document = snapshot(&rig);
    let rows = connection_rows(&document, ShapeId { index: 0 });
    let kind_cell = rows
        .first()
        .expect("row 0")
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "Type")
        .and_then(|cell| cell.value.clone());
    assert_eq!(
        kind_cell,
        Some(CellValue::Number(1.0)),
        "{kind:?}: the row's Type is Outward"
    );

    chord(&mut rig, egui::Key::Z);
    let document = snapshot(&rig);
    let rows = connection_rows(&document, ShapeId { index: 0 });
    let kind_cell = rows
        .first()
        .expect("row 0")
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "Type")
        .and_then(|cell| cell.value.clone());
    assert_eq!(kind_cell, None, "{kind:?}: the undo restores the rows");
}

/// Item 4: Delete removes the selected point and its end re-glues
/// dynamically (formula and record); one undo restores the glue.
fn the_deleted_point_re_glues_the_end_dynamically(kind: LayoutKind) {
    let mut rig = rig(kind);
    select_shape(&mut rig);
    draw_connector(&mut rig, kind, (4.5, POINTS[0].1), POINTS[0]);
    let (connector, visio) = drawn_connector(&rig);
    select_shape(&mut rig);
    arm_point_tool(&mut rig, kind);
    let point_at = screen(&rig, POINTS[0].0, POINTS[0].1);
    click_at(&mut rig, point_at);
    press_key(&mut rig, egui::Key::Delete);

    let document = snapshot(&rig);
    assert_eq!(
        connection_rows(&document, ShapeId { index: 0 }).len(),
        1,
        "{kind:?}: the point is gone"
    );
    assert!(
        formula(&document, connector, "EndX").is_some_and(|text| text.starts_with("_WALKGLUE(")),
        "{kind:?}: the end re-glued dynamically"
    );
    let records = records_of(&document, visio);
    let record = records.first().expect("the end's record");
    assert_eq!(
        record.to_cell, "PinX",
        "{kind:?}: dynamic glue names the pin"
    );
    assert_eq!(record.to_part, 3, "{kind:?}: ToPart is the whole shape");

    // One undo restores the rows and the glue.
    chord(&mut rig, egui::Key::Z);
    let document = snapshot(&rig);
    assert_eq!(
        connection_rows(&document, ShapeId { index: 0 }).len(),
        2,
        "{kind:?}: the undo restores the rows"
    );
    assert!(
        formula(&document, connector, "EndX").is_some_and(|text| text.starts_with("PAR(PNT(")),
        "{kind:?}: the undo restores the point glue"
    );
    let records = records_of(&document, visio);
    let record = records.first().expect("the end's record");
    assert_eq!(record.to_cell, "Connections.X1");
    assert_eq!(record.to_part, 100);
}

/// The user's 2026-11-05 pass: with a connector selected, switching to
/// the Connection Point tool keeps the selection visible, a click on a
/// shape selects it (its points draw), and a click on empty page clears
/// the selection.
fn the_tool_selects_the_shape_it_works_on(kind: LayoutKind) {
    let mut rig = rig(kind);
    draw_connector(&mut rig, kind, (6.0, 5.0), (8.0, 5.0));
    let (connector, _) = drawn_connector(&rig);
    let at = screen(&rig, 7.0, 5.0);
    click_at(&mut rig, at);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![connector],
        "{kind:?}: the connector is selected"
    );
    arm_point_tool(&mut rig, kind);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![connector],
        "{kind:?}: the tool keeps the selection"
    );
    let with_connector = rig.render();

    // A click on the shape selects it, and its points draw.
    let at = screen(&rig, A.0, A.1);
    click_at(&mut rig, at);
    rig.run();
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![ShapeId { index: 0 }],
        "{kind:?}: the click selects the shape"
    );
    let with_shape = rig.render();
    assert!(
        changed_near(&with_connector, &with_shape, &rig, POINTS[0], 6.0),
        "{kind:?}: the shape's points draw"
    );

    // A click on empty page clears the selection.
    let at = screen(&rig, 6.0, 1.0);
    click_at(&mut rig, at);
    rig.run();
    assert!(
        rig.shell.borrow().app.selection.shapes.is_empty(),
        "{kind:?}: the empty click clears the selection"
    );
    assert_eq!(
        rig.shell.borrow().app.active_tool,
        ActiveTool::ConnectionPoint,
        "{kind:?}: the tool stays on"
    );
}

#[test]
fn the_view_check_shows_and_hides_the_markers_in_panel_modern() {
    the_view_check_shows_and_hides_the_markers(LayoutKind::PanelModern);
}

#[test]
fn the_view_check_shows_and_hides_the_markers_in_panel_classic() {
    the_view_check_shows_and_hides_the_markers(LayoutKind::PanelClassic);
}

#[test]
fn the_tool_adds_a_point_on_ctrl_click_in_panel_modern() {
    the_tool_adds_a_point_on_ctrl_click(LayoutKind::PanelModern);
}

#[test]
fn the_tool_adds_a_point_on_ctrl_click_in_panel_classic() {
    the_tool_adds_a_point_on_ctrl_click(LayoutKind::PanelClassic);
}

#[test]
fn a_moved_point_moves_the_connector_end_in_panel_modern() {
    a_moved_point_moves_the_connector_end(LayoutKind::PanelModern);
}

#[test]
fn a_moved_point_moves_the_connector_end_in_panel_classic() {
    a_moved_point_moves_the_connector_end(LayoutKind::PanelClassic);
}

#[test]
fn the_context_menu_sets_the_point_type_in_panel_modern() {
    the_context_menu_sets_the_point_type(LayoutKind::PanelModern);
}

#[test]
fn the_context_menu_sets_the_point_type_in_panel_classic() {
    the_context_menu_sets_the_point_type(LayoutKind::PanelClassic);
}

#[test]
fn the_deleted_point_re_glues_the_end_dynamically_in_panel_modern() {
    the_deleted_point_re_glues_the_end_dynamically(LayoutKind::PanelModern);
}

#[test]
fn the_deleted_point_re_glues_the_end_dynamically_in_panel_classic() {
    the_deleted_point_re_glues_the_end_dynamically(LayoutKind::PanelClassic);
}

#[test]
fn the_tool_selects_the_shape_it_works_on_in_panel_modern() {
    the_tool_selects_the_shape_it_works_on(LayoutKind::PanelModern);
}

#[test]
fn the_tool_selects_the_shape_it_works_on_in_panel_classic() {
    the_tool_selects_the_shape_it_works_on(LayoutKind::PanelClassic);
}
