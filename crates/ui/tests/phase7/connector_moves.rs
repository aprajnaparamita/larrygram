//! 1-D shapes under real input (task P7-39, ADR 0084 rule 1 as
//! amended on 2026-11-03), in Panel Modern and Panel Classic:
//!
//! - a body drag of a free connector moves its endpoint cells, and the
//!   line draws at its new place and no longer at its old one;
//! - Ctrl+D on a selected connector makes a copy whose endpoint cells
//!   sit at the paste offset, and the copy draws where its cells say;
//! - a drag of the Connector master from the Shapes window puts its
//!   ends around the drop point (the master's own length), and the
//!   line draws there.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{
    CellLookup, CellName, CellValue, ConnectorEnd, Document, GeometryRow, Page, PageId, ShapeId,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The two layouts the Connector tool exists in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelModern, LayoutKind::PanelClassic];

/// The Connector tool button's label in one layout.
fn connector_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Connector Tool",
        _ => "Connector",
    }
}

/// One 2 x 1 in rectangle at (3, 3), out of the way of the connectors.
fn one_shape(kind: LayoutKind) -> AppCtx {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let shape = document.add_shape(fixture_shape(3.0, 3.0, 1));
    page.shapes.push(shape);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    app
}

/// A shell with page point (5, 6) centred in the viewport at zoom 1.
fn rig(kind: LayoutKind) -> ShellRig {
    let mut rig = ShellRig::with_app(one_shape(kind));
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
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 6.0),
        );
    }
    rig.run();
    rig.run();
    rig
}

fn screen(rig: &ShellRig, point: Point) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(point)
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

/// A press, four moves and a release from `from` to `to`, with Alt held
/// (no snapping: the deltas stay exact).
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
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

fn document(rig: &ShellRig) -> Document {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone()
}

/// The page's 1-D shapes, in page order.
fn connectors(document: &Document) -> Vec<ShapeId> {
    document
        .page(PageId { index: 0 })
        .expect("page")
        .shapes
        .iter()
        .copied()
        .filter(|shape| document.is_connector(*shape))
        .collect()
}

fn end(document: &Document, shape: ShapeId, which: ConnectorEnd) -> Point {
    document.connector_end(shape, which).expect("end")
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

/// A point `t` of the way along the path's first segment.
fn along_first(path: &[Point], t: f64) -> Point {
    let (a, b) = (path[0], path[1]);
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Whether the pixel at a page point differs from the plain canvas
/// half an inch away across the line.
fn inked(image: &image::RgbaImage, rig: &ShellRig, at: Point, across: (f64, f64)) -> bool {
    let sample = |point: Point| {
        let pos = screen(rig, point);
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

/// The unit normal of the path's first segment (the "across" offset
/// for [`inked`], half an inch).
fn across_first(path: &[Point]) -> (f64, f64) {
    let (a, b) = (path[0], path[1]);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy).max(1e-9);
    (-dy / length * 0.5, dx / length * 0.5)
}

fn close(a: Point, b: Point, tolerance: f64) -> bool {
    (a.x - b.x).abs() < tolerance && (a.y - b.y).abs() < tolerance
}

/// Draws a free connector with the Connector tool from `from` to `to`
/// (empty page points; Alt keeps both ends free) and returns it.
fn draw_free(rig: &mut ShellRig, kind: LayoutKind, from: Point, to: Point) -> ShapeId {
    click(rig, Role::Button, connector_tool_button(kind));
    let (a, b) = (screen(rig, from), screen(rig, to));
    drag(rig, a, b);
    click(rig, Role::Button, "Pointer Tool");
    *connectors(&document(rig)).last().expect("a connector")
}

/// The endpoint cells agree with the drawn path.
fn assert_cells_match_path(document: &Document, shape: ShapeId) {
    let path = drawn(document, shape);
    assert!(close(
        path[0],
        end(document, shape, ConnectorEnd::Begin),
        1e-6
    ));
    assert!(close(
        *path.last().expect("last"),
        end(document, shape, ConnectorEnd::End),
        1e-6
    ));
}

fn a_body_drag_moves_a_free_connector(kind: LayoutKind) {
    let mut rig = rig(kind);
    // Off the whole-inch grid lines, so the ink probes see the line only.
    let connector = draw_free(
        &mut rig,
        kind,
        Point::new(3.25, 6.25),
        Point::new(6.25, 7.35),
    );
    let before = document(&rig);
    let begin = end(&before, connector, ConnectorEnd::Begin);
    let finish = end(&before, connector, ConnectorEnd::End);
    let old_path = drawn(&before, connector);
    let grab = along_first(&old_path, 0.5);

    // Deselect first: the selected connector still shows the eight
    // resize handles of its box (P7-33 owns the 1-D handles), and a
    // press on one is a (refused) resize, not a body drag.
    let empty = screen(&rig, Point::new(7.5, 4.5));
    click_at(&mut rig, empty);
    assert!(rig.shell.borrow().app.selection.shapes.is_empty());
    // Drag the line's body by (1, -1.5) inches.
    let delta = (1.0, -1.5);
    let from = screen(&rig, grab);
    let to = screen(&rig, Point::new(grab.x + delta.0, grab.y + delta.1));
    drag(&mut rig, from, to);

    let after = document(&rig);
    let tolerance = 0.02;
    assert!(
        close(
            end(&after, connector, ConnectorEnd::Begin),
            Point::new(begin.x + delta.0, begin.y + delta.1),
            tolerance
        ),
        "{kind:?}: the begin cell moves with the drag"
    );
    assert!(
        close(
            end(&after, connector, ConnectorEnd::End),
            Point::new(finish.x + delta.0, finish.y + delta.1),
            tolerance
        ),
        "{kind:?}: the end cell moves with the drag"
    );
    assert_cells_match_path(&after, connector);

    // The picture: ink on the moved line, none where it was.
    let image = rig.render();
    let new_path = drawn(&after, connector);
    let across = across_first(&new_path);
    for t in [0.25, 0.5, 0.75] {
        let at = along_first(&new_path, t);
        assert!(
            inked(&image, &rig, at, across),
            "{kind:?}: no ink at {at:?}"
        );
    }
    let old_across = across_first(&old_path);
    let at = along_first(&old_path, 0.5);
    assert!(
        !inked(&image, &rig, at, old_across),
        "{kind:?}: the old place still draws at {at:?}"
    );
}

fn ctrl_d_copies_a_connector_where_its_cells_say(kind: LayoutKind) {
    let mut rig = rig(kind);
    let original = draw_free(
        &mut rig,
        kind,
        Point::new(3.25, 6.25),
        Point::new(6.25, 7.65),
    );
    let path = drawn(&document(&rig), original);
    // Select it with a real click on the line.
    let on_line = screen(&rig, along_first(&path, 0.5));
    click_at(&mut rig, on_line);
    assert!(
        rig.shell.borrow().app.selection.shapes.contains(&original),
        "{kind:?}: the click selects the connector"
    );
    press_key(
        &mut rig,
        egui::Key::D,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        },
    );
    let after = document(&rig);
    let all = connectors(&after);
    assert_eq!(all.len(), 2, "{kind:?}: Ctrl+D adds one connector");
    let copy = all[1];
    let (from, to) = (
        end(&after, original, ConnectorEnd::Begin),
        end(&after, copy, ConnectorEnd::Begin),
    );
    assert!(
        close(to, Point::new(from.x + 0.1, from.y - 0.1), 1e-6),
        "{kind:?}: the copy's begin sits at the paste offset: {from:?} {to:?}"
    );
    assert_cells_match_path(&after, copy);
    // The copy draws where its cells say: ink along its own first
    // segment, between its begin and its first bend.
    let image = rig.render();
    let copy_path = drawn(&after, copy);
    let across = across_first(&copy_path);
    let across = (-across.0, -across.1);
    for t in [0.3, 0.6] {
        let at = along_first(&copy_path, t);
        assert!(
            inked(&image, &rig, at, across),
            "{kind:?}: no copy ink at {at:?}"
        );
    }
}

fn a_shapes_window_drop_puts_the_ends_at_the_drop_point(kind: LayoutKind) {
    let mut rig = rig(kind);
    rig.shell
        .borrow_mut()
        .shapes_window_mut()
        .set_search("Connector");
    rig.run();
    let tile = {
        rig.run();
        rig.harness()
            .root()
            .query_all_by_role_and_label(Role::Button, "Connector")
            .map(|node| node.rect())
            .find(|rect| rect.center().x < 400.0 && rect.height() > 30.0)
            .unwrap_or_else(|| panic!("{kind:?}: no Connector tile"))
    };
    let drop = Point::new(5.3, 7.4);
    let to = screen(&rig, drop);
    drag(&mut rig, tile.center(), to);

    let after = document(&rig);
    let dropped = *connectors(&after).last().expect("a dropped connector");
    let begin = end(&after, dropped, ConnectorEnd::Begin);
    let finish = end(&after, dropped, ConnectorEnd::End);
    let tolerance = 0.05;
    assert!(
        close(begin, Point::new(drop.x - 0.6, drop.y), tolerance),
        "{kind:?}: begin {begin:?}"
    );
    assert!(
        close(finish, Point::new(drop.x + 0.6, drop.y), tolerance),
        "{kind:?}: end {finish:?}"
    );
    assert_cells_match_path(&after, dropped);
    let image = rig.render();
    for t in [0.25, 0.5, 0.75] {
        let at = Point::new(begin.x + (finish.x - begin.x) * t, begin.y);
        assert!(
            inked(&image, &rig, at, (0.0, 0.5)),
            "{kind:?}: no ink at {at:?}"
        );
    }
}

#[test]
fn a_body_drag_moves_a_free_connector_in_both_layouts() {
    for kind in LAYOUTS {
        a_body_drag_moves_a_free_connector(kind);
    }
}

#[test]
fn ctrl_d_copies_a_connector_where_its_cells_say_in_both_layouts() {
    for kind in LAYOUTS {
        ctrl_d_copies_a_connector_where_its_cells_say(kind);
    }
}

#[test]
fn a_shapes_window_drop_puts_the_ends_at_the_drop_point_in_both_layouts() {
    for kind in LAYOUTS {
        a_shapes_window_drop_puts_the_ends_at_the_drop_point(kind);
    }
}
