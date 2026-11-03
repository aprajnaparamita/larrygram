//! Where connector ends land under real input (task P7-32, ADR 0084
//! rules 3 and 6), in Panel Modern and Panel Classic:
//!
//! - a centre-to-centre drag between two rectangles lands both ends on
//!   the facing side midpoints, and the picture shows a straight
//!   horizontal line between the facing sides;
//! - after dragging the right rectangle below the left one, the ends
//!   move to the top and bottom midpoints and the picture shows a
//!   vertical run with no segment along a shape's edge;
//! - a drag that ends on a connection point glues to that point;
//! - a drag that ends over a connector does not glue to the connector.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The two layouts the Connector tool exists in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelModern, LayoutKind::PanelClassic];

/// The page point of shape A's centre (the drag's start).
const A: (f64, f64) = (3.0, 3.0);

/// The page point of shape B's centre (the drag's end).
const B: (f64, f64) = (7.0, 3.0);

/// The Connector tool button's label in one layout.
fn connector_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Connector Tool",
        _ => "Connector",
    }
}

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// `fixture_shape` with one connection point at the local `(x, y)`.
fn shape_with_point(x: f64, y: f64, visio_id: i32, point: (f64, f64)) -> Shape {
    let mut shape = fixture_shape(x, y, visio_id);
    let mut section = Section::new(SectionName::from_static("Connection"));
    let mut row = Row::new();
    row.row_type = Some("Connection".to_owned());
    row.ix = Some(0);
    row.cells = vec![cell("X", point.0), cell("Y", point.1)];
    section.rows.push(row);
    shape.sheet.sections.push(section);
    shape
}

/// Two 2 x 1 in rectangles at [`A`] and [`B`] (`with_points` adds one
/// connection point per shape, at the local `(0.25, 0.25)`).
fn two_shapes(kind: LayoutKind, with_points: bool) -> (AppCtx, ShapeId, ShapeId) {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let left = if with_points {
        shape_with_point(A.0, A.1, 1, (0.25, 0.25))
    } else {
        fixture_shape(A.0, A.1, 1)
    };
    let right = if with_points {
        shape_with_point(B.0, B.1, 2, (0.25, 0.25))
    } else {
        fixture_shape(B.0, B.1, 2)
    };
    let left = document.add_shape(left);
    let right = document.add_shape(right);
    page.shapes.push(left);
    page.shapes.push(right);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    (app, left, right)
}

/// A shell over [`two_shapes`] with the midpoint of the two shapes
/// centred in the viewport (the text fixtures' rule).
fn rig(kind: LayoutKind, with_points: bool) -> ShellRig {
    let (app, _, _) = two_shapes(kind, with_points);
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
            f64::from(viewport.width()) / 2.0 / k - 5.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 3.0),
        );
    }
    rig.run();
    rig.run();
    rig
}

/// The screen point of a page point in `rig`'s canvas.
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

/// A press, four moves and a release from `from` to `to` (the text
/// fixtures' drag).
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

/// A body drag with Alt held (no snapping: the deltas stay exact).
fn drag_exact(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    rig.run();
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::ALT,
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
        modifiers: egui::Modifiers::ALT,
    });
    rig.run();
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

/// Draws one connector with a real drag between the two fixture
/// shapes' centres.
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind) {
    click(rig, Role::Button, connector_tool_button(kind));
    let from = screen(rig, A.0, A.1);
    let to = screen(rig, B.0, B.1);
    drag(rig, from, to);
}

/// The `n`th drawn connector (the shapes after the two fixtures).
fn drawn(app: &AppCtx, n: usize) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[2 + n]
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

/// The page's `Connects` records.
fn records(document: &Document) -> Vec<larrygram_model::Connect> {
    document
        .page(PageId { index: 0 })
        .expect("page")
        .connects
        .clone()
}

/// Whether the pixel at a page point carries dark ink: the connector's
/// line draws with a dark grey core (~100 to 150) while the canvas
/// grid is a light grey (~196) and the background is white.
fn inked(image: &image::RgbaImage, rig: &ShellRig, x: f64, y: f64) -> bool {
    let at = screen(rig, x, y);
    let px = at.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let py = at.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    let pixel = image.get_pixel(px, py).0;
    pixel[0] < 170 && pixel[1] < 170 && pixel[2] < 170
}

/// One layout's flow: draw from centre to centre and check the cells
/// and the picture.
fn the_ends_land_on_the_facing_side_midpoints(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind);

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let connector = drawn(&rig.shell.borrow().app, 0);
    assert_eq!(
        (
            number(&document, connector, "BeginX"),
            number(&document, connector, "BeginY"),
        ),
        (4.0, 3.0),
        "the left shape's right side midpoint"
    );
    assert_eq!(
        (
            number(&document, connector, "EndX"),
            number(&document, connector, "EndY"),
        ),
        (6.0, 3.0),
        "the right shape's left side midpoint"
    );

    // The picture: a straight horizontal line between the facing sides.
    let image = rig.render();
    for x in [4.25, 4.75, 5.25, 5.75] {
        assert!(inked(&image, &rig, x, 3.0), "no line ink at ({x}, 3)");
    }
    assert!(
        !inked(&image, &rig, 5.0, 3.3),
        "the run is the straight line only"
    );
}

/// One layout's flow: drag the right shape below the left one and
/// check the ends walk to the top and bottom midpoints.
fn the_ends_walk_when_one_shape_moves_below_the_other(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind);
    click(&mut rig, Role::Button, "Pointer Tool");
    let from = screen(&rig, B.0, B.1);
    let to = screen(&rig, A.0, 6.0);
    drag_exact(&mut rig, from, to);

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let connector = drawn(&rig.shell.borrow().app, 0);
    let begin = (
        number(&document, connector, "BeginX"),
        number(&document, connector, "BeginY"),
    );
    let end = (
        number(&document, connector, "EndX"),
        number(&document, connector, "EndY"),
    );
    // The drag lands within a pixel of (3, 6), so the midpoints land
    // within a pixel of the rule's points.
    assert!(
        (begin.0 - 3.0).abs() < 0.02 && (begin.1 - 3.5).abs() < 0.02,
        "the left shape's top side midpoint: {begin:?}"
    );
    assert!(
        (end.0 - 3.0).abs() < 0.02 && (end.1 - 5.5).abs() < 0.02,
        "the right shape's bottom side midpoint: {end:?}"
    );

    // The picture: a vertical run between the two sides.
    let image = rig.render();
    for y in [3.75, 4.25, 4.75, 5.25] {
        assert!(inked(&image, &rig, 3.0, y), "no line ink at (3, {y})");
    }
    assert!(
        !inked(&image, &rig, 3.4, 4.5),
        "the run is the vertical line only"
    );
    // No segment runs along a shape's own edge: the written route is
    // the two endpoints and nothing else.
    let geometry = document.shape_geometry(connector).expect("geometry");
    let rows = &geometry.paths.first().expect("a path").rows;
    assert_eq!(rows.len(), 2, "a straight run with no edge-hugging turn: {rows:?}");
}

/// One layout's flow: a drag that ends on a connection point glues to
/// that point.
fn a_drag_that_ends_on_a_connection_point_glues_to_that_point(kind: LayoutKind) {
    let mut rig = rig(kind, true);
    click(&mut rig, Role::Button, connector_tool_button(kind));
    // The right shape's connection point sits at (6.25, 2.75).
    let from = screen(&rig, A.0, A.1);
    let to = screen(&rig, 6.25, 2.75);
    drag(&mut rig, from, to);

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let connector = drawn(&rig.shell.borrow().app, 0);
    assert_eq!(
        (
            number(&document, connector, "EndX"),
            number(&document, connector, "EndY"),
        ),
        (6.25, 2.75),
        "the end lands on the connection point"
    );
    assert_eq!(
        formula(&document, connector, "EndX"),
        Some("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))".to_owned()),
        "point glue, as Visio stores it"
    );
    let end = records(&document)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(end.to_cell, "Connections.X1");
}

/// One layout's flow: a drag that ends over a connector does not glue
/// to the connector.
fn a_drag_that_ends_over_a_connector_does_not_glue_to_the_connector(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind);
    click(&mut rig, Role::Button, connector_tool_button(kind));
    // The first connector's line runs along y = 3 between x = 4 and
    // x = 6: the second drag ends on it.
    let from = screen(&rig, A.0, A.1);
    let to = screen(&rig, 5.0, 3.0);
    drag(&mut rig, from, to);

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let first = drawn(&rig.shell.borrow().app, 0);
    let second = drawn(&rig.shell.borrow().app, 1);
    let first_visio = document.shape(first).expect("first").visio_id;
    let second_visio = document.shape(second).expect("second").visio_id;
    let of_second: Vec<_> = records(&document)
        .into_iter()
        .filter(|record| record.from_sheet == second_visio)
        .collect();
    assert!(
        of_second.iter().all(|record| record.to_sheet != first_visio),
        "the end does not glue to the connector: {of_second:?}"
    );
    assert_eq!(of_second.len(), 1, "only the begin is glued: {of_second:?}");
    assert_eq!(
        formula(&document, second, "EndX"),
        None,
        "the free end keeps a literal point"
    );
    let end_x = number(&document, second, "EndX");
    assert!(
        (end_x - 5.0).abs() < 0.5,
        "the free end sits where the drag released: {end_x}"
    );
}

#[test]
fn the_ends_land_on_the_facing_side_midpoints_in_panel_modern() {
    the_ends_land_on_the_facing_side_midpoints(LayoutKind::PanelModern);
}

#[test]
fn the_ends_land_on_the_facing_side_midpoints_in_panel_classic() {
    the_ends_land_on_the_facing_side_midpoints(LayoutKind::PanelClassic);
}

#[test]
fn the_ends_walk_when_one_shape_moves_below_the_other_in_panel_modern() {
    the_ends_walk_when_one_shape_moves_below_the_other(LayoutKind::PanelModern);
}

#[test]
fn the_ends_walk_when_one_shape_moves_below_the_other_in_panel_classic() {
    the_ends_walk_when_one_shape_moves_below_the_other(LayoutKind::PanelClassic);
}

#[test]
fn a_drag_that_ends_on_a_connection_point_glues_to_that_point_in_panel_modern() {
    a_drag_that_ends_on_a_connection_point_glues_to_that_point(LayoutKind::PanelModern);
}

#[test]
fn a_drag_that_ends_on_a_connection_point_glues_to_that_point_in_panel_classic() {
    a_drag_that_ends_on_a_connection_point_glues_to_that_point(LayoutKind::PanelClassic);
}

#[test]
fn a_drag_that_ends_over_a_connector_does_not_glue_to_the_connector_in_panel_modern() {
    a_drag_that_ends_over_a_connector_does_not_glue_to_the_connector(LayoutKind::PanelModern);
}

#[test]
fn a_drag_that_ends_over_a_connector_does_not_glue_to_the_connector_in_panel_classic() {
    a_drag_that_ends_over_a_connector_does_not_glue_to_the_connector(LayoutKind::PanelClassic);
}
