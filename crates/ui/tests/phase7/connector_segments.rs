//! Connector segment moves through real input (task P7-35), in Panel
//! Modern and Panel Classic:
//!
//! - a selected right-angle connector shows a midpoint handle on each
//!   segment of its drawn route (straight and curved ones show none);
//! - dragging one moves that segment perpendicular to itself and the
//!   picture draws the moved route; one undo restores the geometry;
//! - a moved segment survives an unrelated edit, and is replaced by a
//!   fresh route when a glued shape moves;
//! - Reset Connector from the connector context menu re-routes the
//!   selection fresh;
//! - in Panel Classic, Never Reroute from the Layout & Routing toolbar
//!   keeps the route's interior when a glued shape moves.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{
    CellLookup, CellName, CellValue, Document, GeometryRow, Page, PageId, Row, ShapeId,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The page point of shape A's centre (the drag's start).
const A: (f64, f64) = (3.0, 3.0);

/// The page point of shape B's centre (the drag's end). At a
/// different height than [`A`], so the right-angle route bends.
const B: (f64, f64) = (7.0, 5.0);

/// The page point of shape C: an unrelated shape (no glue to the
/// connector) for the unrelated-edit check.
const C: (f64, f64) = (3.0, 7.0);

/// The Connector tool button's label in one layout.
fn connector_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Connector Tool",
        _ => "Connector",
    }
}

/// Three 2 x 1 in rectangles at [`A`], [`B`] and [`C`], nothing
/// selected.
fn three_shapes(kind: LayoutKind) -> AppCtx {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let left = document.add_shape(fixture_shape(A.0, A.1, 1));
    let right = document.add_shape(fixture_shape(B.0, B.1, 2));
    let third = document.add_shape(fixture_shape(C.0, C.1, 3));
    page.shapes.push(left);
    page.shapes.push(right);
    page.shapes.push(third);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    app
}

/// A shell over [`three_shapes`] with the midpoint of the two shapes
/// centred in the viewport (the connector fixtures' rule).
fn rig(kind: LayoutKind) -> ShellRig {
    let app = three_shapes(kind);
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
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 4.0),
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

/// Holds Alt (snapping and glue off) or releases it.
fn set_alt(rig: &mut ShellRig, alt: bool) {
    let modifiers = if alt {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::NONE
    };
    rig.harness()
        .event(egui::Event::ModifiersChanged(modifiers));
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

/// A press, four moves and a release from `from` to `to` with Alt held
/// (no snapping: the deltas stay exact).
fn drag_alt(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    set_alt(rig, true);
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
    set_alt(rig, false);
}

/// A press and a release in one frame at `pos`.
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

/// A right click at `pos`: opens the context menu (the shell's
/// right-click router).
fn right_click(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        rig.run();
    }
    rig.run();
    rig.run();
}

/// Draws one right-angle connector with a real drag between [`A`] and
/// [`B`] and returns to the Pointer Tool (the segment handles are
/// pointer-tool behaviour).
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind) -> ShapeId {
    click(rig, Role::Button, connector_tool_button(kind));
    let from = screen(rig, A.0, A.1);
    let to = screen(rig, B.0, B.1);
    drag(rig, from, to);
    click(rig, Role::Button, "Pointer Tool");
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    *session
        .document
        .page(page)
        .expect("page")
        .shapes
        .last()
        .expect("the drawn connector")
}

fn document(rig: &ShellRig) -> Document {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone()
}

/// One resolved number cell (own or inherited).
fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    document
        .resolve(shape, &CellName::new(name))
        .ok()
        .and_then(|found| match found.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// The drawn path in page inches, mapped from the geometry rows
/// through the shape's own cells.
fn drawn(document: &Document, shape: ShapeId) -> Vec<Point> {
    let (pin_x, pin_y) = (
        number(document, shape, "PinX"),
        number(document, shape, "PinY"),
    );
    let (loc_x, loc_y) = (
        number(document, shape, "LocPinX"),
        number(document, shape, "LocPinY"),
    );
    let (sin, cos) = number(document, shape, "Angle").sin_cos();
    let geometry = document.shape_geometry(shape).expect("geometry");
    geometry.paths[0]
        .rows
        .iter()
        .filter_map(|row| match row {
            GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
                let (dx, dy) = (x - loc_x, y - loc_y);
                Some(Point::new(
                    pin_x + dx * cos - dy * sin,
                    pin_y + dx * sin + dy * cos,
                ))
            }
            _ => None,
        })
        .collect()
}

/// The connector's raw geometry rows (the stored route, byte for
/// byte).
fn rows(document: &Document, shape: ShapeId) -> Vec<Row> {
    document
        .shape(shape)
        .expect("shape")
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Geometry")
        .expect("a Geometry section")
        .rows
        .clone()
}

/// The page point `t` of the way along the path's segment `index`.
fn along(path: &[Point], index: usize, t: f64) -> Point {
    let (a, b) = (path[index], path[index + 1]);
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// The midpoint of the path's segment `index`.
fn midpoint(path: &[Point], index: usize) -> Point {
    along(path, index, 0.5)
}

fn close(a: Point, b: Point, tolerance: f64) -> bool {
    (a.x - b.x).abs() < tolerance && (a.y - b.y).abs() < tolerance
}

/// The segment midpoint handles of the selection, through the handle
/// code.
fn segment_handles(rig: &ShellRig) -> Vec<(usize, Point)> {
    let shell = rig.shell.borrow();
    let app: &AppCtx = &shell.app;
    larrygram_ui::handles::segment_handles(app, None, &app.selection.shapes).unwrap_or_default()
}

/// The pixel at a screen point.
fn pixel(image: &image::RgbaImage, at: egui::Pos2) -> image::Rgba<u8> {
    let px = at.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let py = at.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    *image.get_pixel(px, py)
}

/// Whether new ink appeared within `radius` screen pixels of a page
/// point.
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

/// Whether the pixel at a page point differs from the plain canvas
/// half an inch away across the line.
fn inked(image: &image::RgbaImage, rig: &ShellRig, at: Point, across: (f64, f64)) -> bool {
    let sample = |point: Point| {
        let pos = screen(rig, point.x, point.y);
        let px = pos.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
        let py = pos.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
        image.get_pixel(px, py).0
    };
    let line = sample(at);
    let background = sample(Point::new(at.x + across.0, at.y + across.1));
    line.iter()
        .zip(background.iter())
        .any(|(a, b)| a.abs_diff(*b) > 12)
}

/// Opens Panel Classic's Layout & Routing toolbar (View > Toolbars >
/// Layout & Routing).
fn show_layout_and_routing(rig: &mut ShellRig) {
    click(rig, Role::Button, "View");
    click(rig, Role::MenuItem, "Toolbars");
    click(rig, Role::MenuItem, "Layout & Routing");
}

// ---------------------------------------------------------------------------
// Item 1: the midpoint handles
// ---------------------------------------------------------------------------

/// One layout's flow: a selected right-angle connector shows a
/// midpoint handle on each segment of its drawn route — the handle
/// code gives one per segment at its midpoint and the picture draws
/// at each one.
fn a_selected_connector_shows_its_segment_midpoint_handles(kind: LayoutKind) {
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let before = document(&rig);
    let path = drawn(&before, connector);
    assert!(path.len() >= 3, "the glued route has bends: {path:?}");

    // Deselect, then select the connector with a real click on its
    // line.
    let empty = screen(&rig, 5.0, 1.5);
    click_at(&mut rig, empty);
    assert!(rig.shell.borrow().app.selection.shapes.is_empty());
    let before_image = rig.render();
    let on_line = screen(&rig, path[1].x, path[1].y);
    click_at(&mut rig, on_line);
    {
        let shell = rig.shell.borrow();
        assert_eq!(
            shell.app.selection.shapes,
            vec![connector],
            "the click selects"
        );
    }

    // The handle code: one handle per segment, at its midpoint.
    let handles = segment_handles(&rig);
    assert_eq!(handles.len(), path.len() - 1, "{handles:?}");
    for (index, at) in &handles {
        assert!(
            close(*at, midpoint(&path, *index), 1e-6),
            "handle {index} at {:?}, not {:?}",
            at,
            midpoint(&path, *index)
        );
    }

    // The picture: a glyph draws at every segment midpoint.
    let after_image = rig.render();
    for (index, at) in &handles {
        assert!(
            changed_near(&before_image, &after_image, &rig, (at.x, at.y), 8.0),
            "no midpoint handle ink at segment {index} ({:?})",
            at
        );
    }
}

#[test]
fn a_selected_connector_shows_its_segment_midpoint_handles_in_panel_modern() {
    a_selected_connector_shows_its_segment_midpoint_handles(LayoutKind::PanelModern);
}

#[test]
fn a_selected_connector_shows_its_segment_midpoint_handles_in_panel_classic() {
    a_selected_connector_shows_its_segment_midpoint_handles(LayoutKind::PanelClassic);
}

/// A straight or curved connector shows no midpoint handles (task
/// P7-35 item 1).
fn a_non_right_angle_connector_shows_no_midpoint_handles(kind: LayoutKind) {
    for style in ["Straight Connector", "Curved Connector"] {
        let mut rig = rig(kind);
        let connector = draw_connector(&mut rig, kind);
        let path = drawn(&document(&rig), connector);
        let pos = screen(&rig, path[1].x, path[1].y);
        right_click(&mut rig, pos);
        click(&mut rig, Role::MenuItem, style);
        let handles = segment_handles(&rig);
        assert!(handles.is_empty(), "{style}: {handles:?}");
    }
}

#[test]
fn a_non_right_angle_connector_shows_no_midpoint_handles_in_panel_modern() {
    a_non_right_angle_connector_shows_no_midpoint_handles(LayoutKind::PanelModern);
}

#[test]
fn a_non_right_angle_connector_shows_no_midpoint_handles_in_panel_classic() {
    a_non_right_angle_connector_shows_no_midpoint_handles(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 2: dragging a segment
// ---------------------------------------------------------------------------

/// One layout's flow: dragging a midpoint handle moves that segment
/// perpendicular to itself, the cells and the picture show the moved
/// route, the ends stay glued, and one undo restores the geometry.
fn a_segment_drag_moves_the_segment_and_one_undo_restores_it(kind: LayoutKind) {
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let before = document(&rig);
    let path = drawn(&before, connector);
    let before_rows = rows(&before, connector);
    let handles = segment_handles(&rig);
    assert!(!handles.is_empty(), "the selection has midpoint handles");

    // Drag segment 1 sideways (along its normal).
    let index = 1;
    let horizontal = path[index].y == path[index + 1].y;
    let (dx, dy) = if horizontal { (0.0, 0.6) } else { (0.6, 0.0) };
    let at = midpoint(&path, index);
    let from = screen(&rig, at.x, at.y);
    let to = screen(&rig, at.x + dx, at.y + dy);
    let before_image = rig.render();
    drag_alt(&mut rig, from, to);

    // The cells: the geometry changed, the glued ends did not.
    let after = document(&rig);
    let after_rows = rows(&after, connector);
    assert_ne!(after_rows, before_rows, "the geometry changed");
    let moved = drawn(&after, connector);
    assert_eq!(moved.len(), path.len(), "{moved:?}");
    assert!(
        close(moved[0], path[0], 1e-6),
        "the begin stays glued: {:?}",
        moved[0]
    );
    assert!(
        close(*moved.last().unwrap(), *path.last().unwrap(), 1e-6),
        "the end stays glued: {:?}",
        moved.last()
    );
    // The moved segment sits on the pointer's normal coordinate.
    let (ma, mb) = (moved[index], moved[index + 1]);
    if horizontal {
        assert!(
            (ma.y - (at.y + dy)).abs() < 0.02 && (mb.y - (at.y + dy)).abs() < 0.02,
            "the segment moved to y={}: {ma:?} {mb:?}",
            at.y + dy
        );
    } else {
        assert!(
            (ma.x - (at.x + dx)).abs() < 0.02 && (mb.x - (at.x + dx)).abs() < 0.02,
            "the segment moved to x={}: {ma:?} {mb:?}",
            at.x + dx
        );
    }

    // The picture draws the moved route where the segment landed.
    let after_image = rig.render();
    let landed = midpoint(&moved, index);
    let normal = if horizontal { (0.0, 0.5) } else { (0.5, 0.0) };
    assert!(
        inked(&after_image, &rig, landed, normal),
        "no line ink at the moved segment {:?}",
        landed
    );
    assert!(
        changed_near(&before_image, &after_image, &rig, (landed.x, landed.y), 8.0),
        "the picture did not change at the moved segment"
    );

    // One undo restores the geometry byte for byte.
    rig.dispatch("edit.undo");
    assert_eq!(
        rows(&document(&rig), connector),
        before_rows,
        "undo restores"
    );
}

#[test]
fn a_segment_drag_moves_the_segment_and_one_undo_restores_it_in_panel_modern() {
    a_segment_drag_moves_the_segment_and_one_undo_restores_it(LayoutKind::PanelModern);
}

#[test]
fn a_segment_drag_moves_the_segment_and_one_undo_restores_it_in_panel_classic() {
    a_segment_drag_moves_the_segment_and_one_undo_restores_it(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 3: the stored route is the memory
// ---------------------------------------------------------------------------

/// One layout's flow: a moved segment survives an unrelated shape's
/// edit unchanged, byte for byte.
fn a_moved_segment_survives_an_unrelated_edit(kind: LayoutKind) {
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let path = drawn(&document(&rig), connector);
    let handles = segment_handles(&rig);
    assert!(!handles.is_empty());
    let index = 1;
    let at = midpoint(&path, index);
    let (dx, dy) = if path[index].y == path[index + 1].y {
        (0.0, 0.6)
    } else {
        (0.6, 0.0)
    };
    let from = screen(&rig, at.x, at.y);
    let to = screen(&rig, at.x + dx, at.y + dy);
    drag_alt(&mut rig, from, to);
    let moved = rows(&document(&rig), connector);

    // Shape C (no glue to the connector) moves.
    let pos = screen(&rig, C.0, C.1);
    click_at(&mut rig, pos);
    let from = screen(&rig, C.0, C.1);
    let to = screen(&rig, C.0 + 0.5, C.1 + 0.25);
    drag_alt(&mut rig, from, to);
    assert_eq!(
        rows(&document(&rig), connector),
        moved,
        "the unrelated edit leaves the route byte for byte"
    );
}

#[test]
fn a_moved_segment_survives_an_unrelated_edit_in_panel_modern() {
    a_moved_segment_survives_an_unrelated_edit(LayoutKind::PanelModern);
}

#[test]
fn a_moved_segment_survives_an_unrelated_edit_in_panel_classic() {
    a_moved_segment_survives_an_unrelated_edit(LayoutKind::PanelClassic);
}

/// One layout's flow: moving a glued shape re-routes the connector
/// fresh, replacing the moved segment.
fn a_moved_segment_is_replaced_when_a_glued_shape_moves(kind: LayoutKind) {
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let path = drawn(&document(&rig), connector);
    let index = 1;
    let at = midpoint(&path, index);
    let (dx, dy) = if path[index].y == path[index + 1].y {
        (0.0, 0.6)
    } else {
        (0.6, 0.0)
    };
    let from = screen(&rig, at.x, at.y);
    let to = screen(&rig, at.x + dx, at.y + dy);
    drag_alt(&mut rig, from, to);
    let moved = rows(&document(&rig), connector);

    // Box A is the begin's glue target: the scoped re-route replaces
    // the stored route with a fresh one (ADR 0084 rule 5).
    let pos = screen(&rig, A.0, A.1);
    click_at(&mut rig, pos);
    let from = screen(&rig, A.0, A.1);
    let to = screen(&rig, A.0 + 0.5, A.1 + 0.25);
    drag_alt(&mut rig, from, to);
    let after = rows(&document(&rig), connector);
    assert_ne!(after, moved, "a fresh route replaced the moved one");
    // The route is still one line between the shapes.
    assert!(drawn(&document(&rig), connector).len() >= 2);
}

#[test]
fn a_moved_segment_is_replaced_when_a_glued_shape_moves_in_panel_modern() {
    a_moved_segment_is_replaced_when_a_glued_shape_moves(LayoutKind::PanelModern);
}

#[test]
fn a_moved_segment_is_replaced_when_a_glued_shape_moves_in_panel_classic() {
    a_moved_segment_is_replaced_when_a_glued_shape_moves(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 4: Reset Connector
// ---------------------------------------------------------------------------

/// One layout's flow: Reset Connector from the connector context menu
/// re-routes the selection fresh — the route the full pass computes,
/// undoable back to the moved one.
fn reset_connector_from_the_context_menu_reroutes_fresh(kind: LayoutKind) {
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let path = drawn(&document(&rig), connector);
    let index = 1;
    let at = midpoint(&path, index);
    let (dx, dy) = if path[index].y == path[index + 1].y {
        (0.0, 0.6)
    } else {
        (0.6, 0.0)
    };
    let from = screen(&rig, at.x, at.y);
    let to = screen(&rig, at.x + dx, at.y + dy);
    drag_alt(&mut rig, from, to);
    let moved = rows(&document(&rig), connector);

    // Right click the route as it is after the drag, and pick Reset
    // Connector.
    let current = drawn(&document(&rig), connector);
    let vertex = current[current.len() / 2];
    let pos = screen(&rig, vertex.x, vertex.y);
    right_click(&mut rig, pos);
    click(&mut rig, Role::MenuItem, "Reset Connector");
    let reset = rows(&document(&rig), connector);
    assert_ne!(reset, moved, "Reset Connector routed fresh");

    // One undo restores the moved route.
    rig.dispatch("edit.undo");
    assert_eq!(rows(&document(&rig), connector), moved, "undo restores");

    // The fresh route is the one the full pass computes.
    rig.dispatch("connector.reroute_all");
    assert_eq!(
        rows(&document(&rig), connector),
        reset,
        "the same fresh route"
    );
}

#[test]
fn reset_connector_from_the_context_menu_reroutes_fresh_in_panel_classic() {
    reset_connector_from_the_context_menu_reroutes_fresh(LayoutKind::PanelClassic);
}

/// Panel Classic: the Layout & Routing toolbar's Reset Connector
/// button re-routes the selection fresh (task P7-35 item 4).
#[test]
fn reset_connector_from_the_layout_and_routing_toolbar_reroutes_fresh() {
    let kind = LayoutKind::PanelClassic;
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    let path = drawn(&document(&rig), connector);
    let index = 1;
    let at = midpoint(&path, index);
    let (dx, dy) = if path[index].y == path[index + 1].y {
        (0.0, 0.6)
    } else {
        (0.6, 0.0)
    };
    let from = screen(&rig, at.x, at.y);
    let to = screen(&rig, at.x + dx, at.y + dy);
    drag_alt(&mut rig, from, to);
    let moved = rows(&document(&rig), connector);

    show_layout_and_routing(&mut rig);
    click(&mut rig, Role::Button, "Reset Connector");
    let reset = rows(&document(&rig), connector);
    assert_ne!(reset, moved, "Reset Connector routed fresh");
    rig.dispatch("edit.undo");
    assert_eq!(rows(&document(&rig), connector), moved, "undo restores");
}

// ---------------------------------------------------------------------------
// Item 5: Never Reroute (Panel Classic)
// ---------------------------------------------------------------------------

/// Panel Classic: the Layout & Routing toolbar's three reroute rows
/// write `ConFixedCode` and show the current value checked (task
/// P7-35 item 5).
#[test]
fn the_reroute_rows_write_their_code_and_show_it_checked() {
    let kind = LayoutKind::PanelClassic;
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    show_layout_and_routing(&mut rig);
    for (button, command, value) in [
        ("Reroute Freely", "format.set.dyn.conn.reroute.freely", 0.0),
        (
            "Reroute on Crossover",
            "format.set.dyn.conn.reroute.on.crossover",
            3.0,
        ),
        ("Never Reroute", "format.set.dyn.conn.reroute.never", 2.0),
    ] {
        click(&mut rig, Role::Button, button);
        let after = document(&rig);
        assert_eq!(number(&after, connector, "ConFixedCode"), value, "{button}");
        {
            let shell = rig.shell.borrow();
            assert_eq!(
                larrygram_ui::checked::reroute_option_checked(&shell.app, command),
                Some(true),
                "{button} shows itself checked"
            );
        }
    }
    // One undo steps back to the previous value.
    rig.dispatch("edit.undo");
    assert_eq!(
        number(&document(&rig), connector, "ConFixedCode"),
        3.0,
        "undo restores the previous value"
    );
}

/// Panel Classic: Never Reroute from the Layout & Routing toolbar
/// keeps the route's interior when a glued shape moves — only the ends
/// follow their glue (task P7-35 item 5, ADR 0084 rule 7). One undo
/// restores both the shape and the route.
#[test]
fn never_reroute_keeps_the_interior_when_a_glued_shape_moves() {
    let kind = LayoutKind::PanelClassic;
    let mut rig = rig(kind);
    let connector = draw_connector(&mut rig, kind);
    show_layout_and_routing(&mut rig);
    click(&mut rig, Role::Button, "Never Reroute");
    {
        let after = document(&rig);
        assert_eq!(number(&after, connector, "ConFixedCode"), 2.0);
    }
    let before = document(&rig);
    let before_rows = rows(&before, connector);
    let path = drawn(&before, connector);

    // Box A (the begin's glue target) moves.
    let pos = screen(&rig, A.0, A.1);
    click_at(&mut rig, pos);
    let a_before = number(&before, shape_at(&rig, 0), "PinX");
    let from = screen(&rig, A.0, A.1);
    let to = screen(&rig, A.0 + 0.25, A.1 + 0.5);
    drag_alt(&mut rig, from, to);
    let after = document(&rig);
    let a_dx = number(&after, shape_at(&rig, 0), "PinX") - a_before;

    // The interior rows are kept byte for byte; only the glued end
    // follows its target.
    let after_rows = rows(&after, connector);
    assert_eq!(after_rows.len(), before_rows.len(), "the row count is kept");
    let moved = drawn(&after, connector);
    for (kept, was) in path[1..path.len() - 1]
        .iter()
        .zip(moved[1..moved.len() - 1].iter())
    {
        assert!(
            close(*was, *kept, 1e-9),
            "the interior point {was:?} moved from {kept:?}"
        );
    }
    assert!(
        close(
            moved[0],
            Point::new(path[0].x + a_dx, path[0].y + 0.5),
            0.02
        ),
        "the begin follows its glue: {:?}",
        moved[0]
    );
    assert!(
        close(*moved.last().unwrap(), *path.last().unwrap(), 1e-6),
        "the end stays: {:?}",
        moved.last()
    );

    // The picture draws the kept interior where it was.
    let image = rig.render();
    let interior = midpoint(&path, 1);
    let normal = if path[1].y == path[2].y {
        (0.0, 0.5)
    } else {
        (0.5, 0.0)
    };
    assert!(
        inked(&image, &rig, interior, normal),
        "no ink at the kept interior vertex {interior:?}"
    );

    // One undo restores both the shape and the route.
    rig.dispatch("edit.undo");
    assert_eq!(
        rows(&document(&rig), connector),
        before_rows,
        "undo restores"
    );
}

/// The page's shape at `nth` (the fixture order).
fn shape_at(rig: &ShellRig, nth: usize) -> ShapeId {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[nth]
}
