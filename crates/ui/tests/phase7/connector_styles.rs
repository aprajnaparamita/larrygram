//! The connector style controls through real input (task P7-36 item
//! 6): draw a connector and set Curved from its context menu (both
//! layouts), set Straight from Panel Modern's Design > Connectors and
//! Panel Classic's Layout & Routing toolbar, set the page default
//! with nothing selected, check the line jump controls are disabled
//! with our tooltip, and check the style and glue survive a save and
//! reopen. Every step checks the cells and the rendered picture.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Source};
use larrygram_geom::{Curve, Nurbs, NurbsPoint, Point};
use larrygram_model::{CellValue, ConnectorEnd, Document, GeometryRow, Page, PageId, ShapeId};
use larrygram_ui::Intent;
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The two layouts the style controls exist in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelModern, LayoutKind::PanelClassic];

/// The page point of shape A's centre (the drag's start).
const A: (f64, f64) = (3.0, 3.0);

/// The page point of shape B's centre (the drag's end). At a
/// different height than [`A`], so the right-angle route bends and a
/// curve has something to bend.
const B: (f64, f64) = (7.0, 5.0);

/// The page point near the centre of the two shapes where a click
/// hits nothing (the deselect click).
const EMPTY: (f64, f64) = (5.0, 1.5);

/// The Connector tool button's label in one layout.
fn connector_tool_button(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Connector Tool",
        _ => "Connector",
    }
}

/// Two 2 x 1 in rectangles at [`A`] and [`B`], nothing selected.
fn two_shapes(kind: LayoutKind) -> AppCtx {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let left = document.add_shape(fixture_shape(A.0, A.1, 1));
    let right = document.add_shape(fixture_shape(B.0, B.1, 2));
    page.shapes.push(left);
    page.shapes.push(right);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    app
}

/// A shell over [`two_shapes`] with the midpoint of the two shapes
/// centred in the viewport (the connector fixtures' rule).
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let app = two_shapes(kind);
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

/// A press, four moves and a release from `from` to `to` (the
/// connector fixtures' drag).
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

/// Draws one connector with a real drag between the two fixture
/// shapes.
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind) {
    click(rig, Role::Button, connector_tool_button(kind));
    let from = screen(rig, A.0, A.1);
    let to = screen(rig, B.0, B.1);
    drag(rig, from, to);
}

/// The document's `nth` connector (the page's shapes after the two
/// boxes).
fn connector_at(app: &AppCtx, nth: usize) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[2 + nth]
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

/// The page transform of a top-level shape from its cached cells (the
/// connector fixtures' rule).
fn page_point(document: &Document, shape: ShapeId, local: Point) -> Point {
    let number = |name: &str| {
        document
            .shape(shape)
            .and_then(|entity| {
                entity
                    .sheet
                    .cells
                    .iter()
                    .find(|cell| cell.name.as_str() == name)
                    .and_then(|cell| cell.value.clone())
            })
            .and_then(|value| match value {
                CellValue::Number(number) => Some(number),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let (pin_x, pin_y) = (number("PinX"), number("PinY"));
    let (loc_x, loc_y) = (number("LocPinX"), number("LocPinY"));
    let angle = number("Angle");
    let (dx, dy) = (local.x - loc_x, local.y - loc_y);
    let (sin, cos) = angle.sin_cos();
    Point::new(pin_x + dx * cos - dy * sin, pin_y + dx * sin + dy * cos)
}

/// The connector's stored path polyline in page inches.
fn route_points(document: &Document, shape: ShapeId) -> Vec<Point> {
    let geometry = document.shape_geometry(shape).expect("geometry");
    geometry
        .paths
        .first()
        .expect("a path")
        .rows
        .iter()
        .filter_map(|row| match row {
            GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
                Some(page_point(document, shape, Point::new(*x, *y)))
            }
            _ => None,
        })
        .collect()
}

/// The row types of the connector's stored path.
fn row_kinds(document: &Document, shape: ShapeId) -> Vec<&'static str> {
    let geometry = document.shape_geometry(shape).expect("geometry");
    geometry
        .paths
        .first()
        .expect("a path")
        .rows
        .iter()
        .map(|row| match row {
            GeometryRow::MoveTo { .. } => "MoveTo",
            GeometryRow::LineTo { .. } => "LineTo",
            GeometryRow::NurbsTo { .. } => "NURBSTo",
            _ => "other",
        })
        .collect()
}

/// The flattened curve of the connector's `NURBSTo` row in page
/// inches (the renderer's assembly: the row's start, the formula's
/// points, its end).
fn curve_points(document: &Document, shape: ShapeId) -> Vec<Point> {
    let geometry = document.shape_geometry(shape).expect("geometry");
    let rows = &geometry.paths.first().expect("a path").rows;
    let start = match rows.first() {
        Some(GeometryRow::MoveTo { x, y }) => Point::new(*x, *y),
        other => panic!("expected a MoveTo first, got {other:?}"),
    };
    let (end, second_last_knot, last_weight, first_knot, first_weight, data) = match rows.get(1) {
        Some(GeometryRow::NurbsTo {
            x,
            y,
            second_last_knot,
            last_weight,
            first_knot,
            first_weight,
            data,
        }) => (
            Point::new(*x, *y),
            *second_last_knot,
            *last_weight,
            *first_knot,
            *first_weight,
            data.as_ref().expect("E decodes"),
        ),
        other => panic!("expected a NURBSTo row, got {other:?}"),
    };
    let mut points = vec![NurbsPoint {
        x: start.x,
        y: start.y,
        knot: first_knot,
        weight: first_weight,
    }];
    for point in &data.points {
        points.push(NurbsPoint {
            x: point.x,
            y: point.y,
            knot: point.knot,
            weight: point.weight,
        });
    }
    points.push(NurbsPoint {
        x: end.x,
        y: end.y,
        knot: second_last_knot,
        weight: last_weight,
    });
    let nurbs = Nurbs {
        degree: data.degree,
        x_type: 1,
        y_type: 1,
        knot_last: data.knot_last,
        points,
    };
    let mut out = Vec::new();
    for cubic in Curve::Nurbs(nurbs).to_cubics(0.01).expect("flattens") {
        for step in 0..4 {
            let point = cubic.eval(step as f64 / 4.0);
            out.push(page_point(document, shape, point));
        }
    }
    out.push(page_point(document, shape, end));
    out
}

/// The point of the route to right click: a vertex near the middle of
/// the stored polyline.
fn route_midpoint(document: &Document, shape: ShapeId) -> Point {
    let points = route_points(document, shape);
    points[points.len() / 2]
}

/// Whether the pixel at a page point is not the plain canvas: the
/// line's ink (the connector's line may anti-alias to grey). The
/// background sample is the same column half an inch below.
fn inked(image: &image::RgbaImage, rig: &ShellRig, x: f64, y: f64) -> bool {
    let at = screen(rig, x, y);
    let px = at.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let py = at.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    let line = image.get_pixel(px, py).0;
    let below = screen(rig, x, y - 0.6);
    let bx = below.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let by = below.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    let bg = image.get_pixel(bx, by).0;
    line.iter().zip(bg.iter()).any(|(a, b)| a.abs_diff(*b) > 12)
}

/// Whether the ink of the line shows near a page point: the point
/// itself or a few pixels around it (the flattened curve and the
/// rendered line can differ by one pixel, and an anti-aliased hairline
/// is faint at exactly one sample).
fn inked_near(image: &image::RgbaImage, rig: &ShellRig, x: f64, y: f64) -> bool {
    const OFFSETS: [(f32, f32); 9] = [
        (0.0, 0.0),
        (2.0, 0.0),
        (-2.0, 0.0),
        (0.0, 2.0),
        (0.0, -2.0),
        (2.0, 2.0),
        (-2.0, 2.0),
        (2.0, -2.0),
        (-2.0, -2.0),
    ];
    let at = screen(rig, x, y);
    OFFSETS.iter().any(|(dx, dy)| {
        let px = (at.x + dx).round().clamp(0.0, image.width() as f32 - 1.0) as u32;
        let py = (at.y + dy).round().clamp(0.0, image.height() as f32 - 1.0) as u32;
        let line = image.get_pixel(px, py).0;
        // The background sample: the same offset half an inch below.
        let below = screen(rig, x, y - 0.6);
        let bx = (below.x + dx)
            .round()
            .clamp(0.0, image.width() as f32 - 1.0) as u32;
        let by = (below.y + dy)
            .round()
            .clamp(0.0, image.height() as f32 - 1.0) as u32;
        let bg = image.get_pixel(bx, by).0;
        line.iter().zip(bg.iter()).any(|(a, b)| a.abs_diff(*b) > 12)
    })
}

/// The glued line's page-space endpoints (the endpoint cells' cached
/// values).
fn segment(document: &Document, connector: ShapeId) -> (Point, Point) {
    let begin = document
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("the begin end");
    let end = document
        .connector_end(connector, ConnectorEnd::End)
        .expect("the end end");
    (begin, end)
}

/// The distance from `point` to the segment `a`..`b`.
fn distance_to_chord(point: Point, a: Point, b: Point) -> f64 {
    let ab = Point::new(b.x - a.x, b.y - a.y);
    let ap = Point::new(point.x - a.x, point.y - a.y);
    let length2 = ab.x * ab.x + ab.y * ab.y;
    if length2 == 0.0 {
        return (ap.x * ap.x + ap.y * ap.y).sqrt();
    }
    let t = ((ap.x * ab.x + ap.y * ab.y) / length2).clamp(0.0, 1.0);
    let closest = Point::new(a.x + t * ab.x, a.y + t * ab.y);
    ((point.x - closest.x).powi(2) + (point.y - closest.y).powi(2)).sqrt()
}

/// Opens Panel Classic's Layout & Routing toolbar (View > Toolbars >
/// Layout & Routing).
fn show_layout_and_routing(rig: &mut ShellRig) {
    click(rig, Role::Button, "View");
    click(rig, Role::MenuItem, "Toolbars");
    click(rig, Role::MenuItem, "Layout & Routing");
}

/// Opens Panel Modern's Design > Connectors dropdown.
fn open_connectors_gallery(rig: &mut ShellRig) {
    click(rig, Role::Button, "Design");
    click(rig, Role::Button, "Connectors");
}

/// One dispatch through the shell with arguments (the save and open
/// of the round trip).
fn dispatch_args(rig: &mut ShellRig, id: &'static str, args: Args) {
    let command = CommandId::parse(id).expect("id");
    let intent = Intent::new(command, Invocation::new(args, Source::Ui));
    let mut shell = rig.shell.borrow_mut();
    shell.dispatch(&[intent]).expect("the command runs");
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

// ---------------------------------------------------------------------------
// Item 6, test 1: Curved from the context menu
// ---------------------------------------------------------------------------

/// Drawing a connector and setting Curved from its context menu
/// writes the curved style cells and bends the picture (task P7-36
/// item 6): the rows become one `NURBSTo`, the curve deviates from
/// the chord, and the picture carries the curve's ink. The checked
/// seam reports the current style (goal 3).
fn curved_from_the_context_menu_bends_the_picture(kind: LayoutKind) {
    let mut rig = shell_rig(kind);
    draw_connector(&mut rig, kind);
    let drawn = connector_at(&rig.shell.borrow().app, 0);

    // Right click the route and pick Curved Connector.
    let at = {
        let shell = rig.shell.borrow();
        let document = &shell.app.active_document().expect("session").document;
        route_midpoint(document, drawn)
    };
    let at_screen = screen(&rig, at.x, at.y);
    right_click(&mut rig, at_screen);
    click(&mut rig, Role::MenuItem, "Curved Connector");

    // The cells (the ADR 0084 rule 8 table) and the rows.
    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    assert_eq!(number(&document, drawn, "ShapeRouteStyle"), 1.0);
    assert_eq!(number(&document, drawn, "ConLineRouteExt"), 2.0);
    assert_eq!(row_kinds(&document, drawn), vec!["MoveTo", "NURBSTo"]);

    // The checked state reports the current style (goal 3).
    {
        let shell = rig.shell.borrow();
        let app: &AppCtx = &shell.app;
        assert_eq!(
            larrygram_ui::checked::connector_style_checked(app, "connector.curved"),
            Some(true)
        );
        assert_eq!(
            larrygram_ui::checked::connector_style_checked(app, "connector.right_angle"),
            Some(false)
        );
    }

    // The picture shows a curve: the curve deviates from the chord and
    // its ink runs along the deviation.
    let (begin, end) = segment(&document, drawn);
    let samples = curve_points(&document, drawn);
    let deviation = samples
        .iter()
        .map(|point| distance_to_chord(*point, begin, end))
        .fold(0.0_f64, f64::max);
    assert!(
        deviation > 0.2,
        "the curve bows {deviation} in off the chord"
    );
    let image = rig.render();
    for step in 0..6 {
        let at = samples[sample_index(samples.len(), step)];
        assert!(
            inked_near(&image, &rig, at.x, at.y),
            "no curve ink at ({}, {})",
            at.x,
            at.y
        );
    }
}

/// A spread sample index over `len` points.
fn sample_index(len: usize, step: usize) -> usize {
    step * (len - 1) / 5
}

#[test]
fn curved_from_the_context_menu_bends_the_picture_in_panel_modern() {
    curved_from_the_context_menu_bends_the_picture(LayoutKind::PanelModern);
}

#[test]
fn curved_from_the_context_menu_bends_the_picture_in_panel_classic() {
    curved_from_the_context_menu_bends_the_picture(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 6, test 2: Straight from the gallery and the toolbar
// ---------------------------------------------------------------------------

/// Panel Modern's Design > Connectors gallery sets the style on the
/// selected connector and shows the current style checked (task
/// P7-36 item 6, goal 3).
#[test]
fn straight_from_the_connectors_gallery_straightens_the_picture() {
    let kind = LayoutKind::PanelModern;
    let mut rig = shell_rig(kind);
    draw_connector(&mut rig, kind);
    let drawn = connector_at(&rig.shell.borrow().app, 0);
    open_connectors_gallery(&mut rig);
    click(&mut rig, Role::Button, "Straight Lines");

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    assert_eq!(number(&document, drawn, "ShapeRouteStyle"), 2.0);
    assert_eq!(number(&document, drawn, "ConLineRouteExt"), 1.0);
    assert_eq!(row_kinds(&document, drawn), vec!["MoveTo", "LineTo"]);

    // The picture: the ink runs along the straight segment.
    let image = rig.render();
    let (begin, end) = segment(&document, drawn);
    for step in 1..=7 {
        let t = step as f64 / 8.0;
        let x = begin.x + (end.x - begin.x) * t;
        let y = begin.y + (end.y - begin.y) * t;
        assert!(inked_near(&image, &rig, x, y), "no line ink at ({x}, {y})");
    }

    // The reopened gallery reports the current style checked through
    // the layout-neutral seam (goal 3).
    {
        let shell = rig.shell.borrow();
        let app: &AppCtx = &shell.app;
        assert_eq!(
            larrygram_ui::checked::connector_style_checked(app, "connector.straight.lines"),
            Some(true)
        );
        assert_eq!(
            larrygram_ui::checked::connector_style_checked(app, "connector.curved.lines"),
            Some(false)
        );
    }
}

/// Panel Classic's Layout & Routing toolbar sets the style on the
/// selected connector (task P7-36 item 6).
#[test]
fn straight_from_the_layout_and_routing_toolbar_straightens_the_picture() {
    let kind = LayoutKind::PanelClassic;
    let mut rig = shell_rig(kind);
    draw_connector(&mut rig, kind);
    let drawn = connector_at(&rig.shell.borrow().app, 0);
    show_layout_and_routing(&mut rig);
    click(&mut rig, Role::Button, "Straight Lines");

    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    assert_eq!(number(&document, drawn, "ShapeRouteStyle"), 2.0);
    assert_eq!(number(&document, drawn, "ConLineRouteExt"), 1.0);
    assert_eq!(row_kinds(&document, drawn), vec!["MoveTo", "LineTo"]);

    // The picture: the ink runs along the straight segment.
    let image = rig.render();
    let (begin, end) = segment(&document, drawn);
    for step in 1..=7 {
        let t = step as f64 / 8.0;
        let x = begin.x + (end.x - begin.x) * t;
        let y = begin.y + (end.y - begin.y) * t;
        assert!(inked_near(&image, &rig, x, y), "no line ink at ({x}, {y})");
    }
}

// ---------------------------------------------------------------------------
// Item 6, test 3: the page default with nothing selected
// ---------------------------------------------------------------------------

/// With nothing selected the style controls set the page default, and
/// a newly drawn connector is straight (task P7-36 item 6).
fn the_page_default_makes_a_new_connector_straight(kind: LayoutKind) {
    let mut rig = shell_rig(kind);
    // Click empty canvas: nothing is selected.
    let empty = screen(&rig, EMPTY.0, EMPTY.1);
    click_at(&mut rig, empty);
    {
        let shell = rig.shell.borrow();
        assert!(
            shell.app.selection.shapes.is_empty(),
            "the empty click deselects"
        );
    }
    match kind {
        LayoutKind::PanelClassic => {
            show_layout_and_routing(&mut rig);
            click(&mut rig, Role::Button, "Straight Lines");
        }
        _ => {
            open_connectors_gallery(&mut rig);
            click(&mut rig, Role::Button, "Straight Lines");
        }
    }

    // The page default cells (the ADR 0084 rule 8 table).
    {
        let shell = rig.shell.borrow();
        let session = shell.app.active_document().expect("session");
        let document = &session.document;
        let page = session.active_page.expect("page");
        let value = |name: &str| -> f64 {
            document
                .page(page)
                .expect("page")
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .and_then(|cell| cell.value.clone())
                .and_then(|value| match value {
                    CellValue::Number(number) => Some(number),
                    _ => None,
                })
                .unwrap_or(f64::NAN)
        };
        assert_eq!(value("RouteStyle"), 2.0, "the page default is straight");
        assert_eq!(value("LineRouteExt"), 1.0);
    }

    // A newly drawn connector is straight and carries no style cells
    // of its own. Panel Modern draws from the Home tab: the gallery
    // click above left the ribbon on Design.
    if kind == LayoutKind::PanelModern {
        click(&mut rig, Role::Button, "Home");
    }
    draw_connector(&mut rig, kind);
    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let fresh = connector_at(&rig.shell.borrow().app, 0);
    assert_eq!(row_kinds(&document, fresh), vec!["MoveTo", "LineTo"]);
    assert!(
        formula(&document, fresh, "ShapeRouteStyle").is_none(),
        "the new connector inherits the page default"
    );
    assert!(formula(&document, fresh, "ConLineRouteExt").is_none());
}

#[test]
fn the_page_default_makes_a_new_connector_straight_in_panel_modern() {
    the_page_default_makes_a_new_connector_straight(LayoutKind::PanelModern);
}

#[test]
fn the_page_default_makes_a_new_connector_straight_in_panel_classic() {
    the_page_default_makes_a_new_connector_straight(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 4 in the shell: the line jump controls stay disabled
// ---------------------------------------------------------------------------

/// The Show Line Jumps, Line Jump Style and Add Line Jumps To
/// controls are disabled with our own tooltip (task P7-36 item 4).
#[test]
fn the_line_jump_controls_are_disabled_with_our_tooltip() {
    // Panel Modern: Design > Connectors > Show Line Jumps.
    let mut rig = shell_rig(LayoutKind::PanelModern);
    open_connectors_gallery(&mut rig);
    assert!(
        disabled(&mut rig, Role::Button, "Show Line Jumps"),
        "Show Line Jumps is disabled"
    );
    assert_eq!(
        tooltip(&mut rig, Role::Button, "Show Line Jumps").as_deref(),
        Some("Line jumps aren't available yet")
    );

    // Panel Classic: the Layout & Routing toolbar.
    let mut rig = shell_rig(LayoutKind::PanelClassic);
    show_layout_and_routing(&mut rig);
    for name in ["Line Jump Style", "Add Line Jumps To"] {
        assert!(disabled(&mut rig, Role::Button, name), "{name} is disabled");
        assert_eq!(
            tooltip(&mut rig, Role::Button, name).as_deref(),
            Some("Line jumps aren't available yet"),
            "{name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 6, test 5: the style and glue survive save and reopen
// ---------------------------------------------------------------------------

/// The style and the glue survive a save and reopen through the real
/// `file.save_as` and `file.open` commands (task P7-36 item 6).
fn the_style_and_glue_survive_save_and_reopen(kind: LayoutKind) {
    let mut rig = shell_rig(kind);
    draw_connector(&mut rig, kind);
    let drawn = connector_at(&rig.shell.borrow().app, 0);
    let at = {
        let shell = rig.shell.borrow();
        let document = &shell.app.active_document().expect("session").document;
        route_midpoint(document, drawn)
    };
    let at_screen = screen(&rig, at.x, at.y);
    right_click(&mut rig, at_screen);
    click(&mut rig, Role::MenuItem, "Curved Connector");

    let path = std::env::temp_dir().join(format!(
        "larrygram-connector-styles-{}-{kind:?}.vsdx",
        std::process::id()
    ));
    dispatch_args(
        &mut rig,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(path.clone())),
    );
    dispatch_args(
        &mut rig,
        "file.open",
        Args::new().with("path", ArgValue::Path(path.clone())),
    );

    // The reopened document carries the style cells, the glue
    // formulas and the curved rows.
    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let reopened = connector_at(&rig.shell.borrow().app, 0);
    assert_eq!(number(&document, reopened, "ShapeRouteStyle"), 1.0);
    assert_eq!(number(&document, reopened, "ConLineRouteExt"), 2.0);
    assert_eq!(
        formula(&document, reopened, "BeginX").as_deref(),
        Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)")
    );
    assert_eq!(
        formula(&document, reopened, "BegTrigger").as_deref(),
        Some("_XFTRIGGER(Sheet.1!EventXFMod)")
    );
    assert_eq!(row_kinds(&document, reopened), vec!["MoveTo", "NURBSTo"]);
    let records = document
        .page(PageId { index: 0 })
        .expect("page")
        .connects
        .clone();
    assert_eq!(records.len(), 2, "{records:?}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_style_and_glue_survive_save_and_reopen_in_panel_modern() {
    the_style_and_glue_survive_save_and_reopen(LayoutKind::PanelModern);
}

#[test]
fn the_style_and_glue_survive_save_and_reopen_in_panel_classic() {
    the_style_and_glue_survive_save_and_reopen(LayoutKind::PanelClassic);
}
