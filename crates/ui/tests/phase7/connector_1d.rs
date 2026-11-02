//! The 1-D connector through real input (task P7-31): two shapes on
//! the canvas, the Connector tool from the command bar, a drag from
//! one shape to the other — in Panel Modern and Panel Classic.
//!
//! The tests check the things the task names: the connector's endpoint
//! cells and their formulas and the `Connects` records; the resize
//! handles of the selected connector (today's handle code draws the
//! full eight for a 1-D shape; P7-33 owns the handles); and the
//! rendered picture — the line's pixels run between the two shapes,
//! and a right-to-left drag draws the same pixels (negative `Width`,
//! ADR 0084 rule 1).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{CellValue, ConnectorEnd, Document, Page, PageId, ShapeId};
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

/// Two 2 x 1 in rectangles at [`A`] and [`B`], nothing selected.
fn two_shapes(kind: LayoutKind) -> (AppCtx, ShapeId, ShapeId) {
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
    (app, left, right)
}

/// A shell over [`two_shapes`] with the midpoint of the two shapes
/// centred in the viewport (the text fixtures' rule).
fn rig(kind: LayoutKind) -> ShellRig {
    let (app, _, _) = two_shapes(kind);
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

/// Draws one connector with a real drag between the two fixture
/// shapes, `from_a` choosing the direction.
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind, from_a: bool) {
    click(rig, Role::Button, connector_tool_button(kind));
    let (start, end) = if from_a { (A, B) } else { (B, A) };
    let from = screen(rig, start.0, start.1);
    let to = screen(rig, end.0, end.1);
    drag(rig, from, to);
}

/// The document's connector shape (the third shape of the page).
fn connector(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[2]
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

/// Whether the pixel at a page point is not the plain canvas: the
/// line's ink (the connector's line may anti-alias to grey).
fn inked(image: &image::RgbaImage, rig: &ShellRig, x: f64, y: f64) -> bool {
    let at = screen(rig, x, y);
    let px = at.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let py = at.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    let line = image.get_pixel(px, py).0;
    // The background sample: the same column, half an inch below.
    let below = screen(rig, x, y - 0.6);
    let bx = below.x.round().clamp(0.0, image.width() as f32 - 1.0) as u32;
    let by = below.y.round().clamp(0.0, image.height() as f32 - 1.0) as u32;
    let bg = image.get_pixel(bx, by).0;
    line.iter().zip(bg.iter()).any(|(a, b)| a.abs_diff(*b) > 12)
}

/// The glued line's page-space segment: the connector's endpoint
/// cells (task P7-31: the cached `V` is the resolved point).
fn segment(document: &Document, connector: ShapeId) -> (Point, Point) {
    let begin = document
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("the begin end");
    let end = document
        .connector_end(connector, ConnectorEnd::End)
        .expect("the end end");
    (begin, end)
}

/// One layout's flow: draw, check the cells, the records and the
/// picture; select the connector with a real click and count the
/// handles.
fn the_connector_draws_glues_and_strokes(kind: LayoutKind) {
    let mut rig = rig(kind);
    draw_connector(&mut rig, kind, true);

    // The endpoint cells, their formulas and the records (both ends
    // glue dynamically: the drag runs from shape to shape).
    let document = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let drawn = connector(&rig.shell.borrow().app);
    assert!(document.is_connector(drawn), "the new shape is 1-D");
    assert_eq!(
        formula(&document, drawn, "BeginX"),
        Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)".to_owned()),
        "the begin end glues dynamically"
    );
    assert_eq!(
        formula(&document, drawn, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(
        formula(&document, drawn, "BegTrigger"),
        Some("_XFTRIGGER(Sheet.1!EventXFMod)".to_owned())
    );
    assert_eq!(
        formula(&document, drawn, "EndTrigger"),
        Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
    );
    let page = PageId { index: 0 };
    let records = document.page(page).expect("page").connects.clone();
    assert_eq!(records.len(), 2, "{records:?}");
    for record in &records {
        assert_eq!(record.to_cell, "PinX", "{record:?}");
        assert_eq!(record.to_part, larrygram_model::mutate::VIS_WHOLE_SHAPE);
    }

    // The rendered picture: the line's pixels run between the shapes.
    let image = rig.render();
    let (begin, end) = segment(&document, drawn);
    let mid_y = (begin.y + end.y) / 2.0;
    for step in 1..=7 {
        let t = step as f64 / 8.0;
        let x = begin.x + (end.x - begin.x) * t;
        let y = begin.y + (end.y - begin.y) * t;
        assert!(inked(&image, &rig, x, y), "no line ink at ({x}, {y})");
    }

    // Select the connector with a real click on the line (hit test),
    // and count the handles the current handle code draws (P7-33 owns
    // the handles: it still draws the full eight resize handles).
    click(&mut rig, Role::Button, "Pointer Tool");
    let mid = screen(&rig, mid_x(&document, drawn), mid_y);
    click_at(&mut rig, mid);
    {
        let shell = rig.shell.borrow();
        assert!(
            shell.app.selection.shapes.contains(&drawn),
            "the click hit-tests and selects the connector"
        );
    }
    let resize_handles = {
        let shell = rig.shell.borrow();
        let bounds = larrygram_ui::handles::selection_bounds(&shell.app).expect("bounds");
        let handles = larrygram_ui::handles::handles_for(
            bounds,
            larrygram_geom::Angle::from_radians(0.0),
            1.0,
        )
        .expect("handles");
        handles
            .iter()
            .filter(|(kind, _)| matches!(kind, larrygram_ui::handles::HandleKind::Resize(_)))
            .count()
    };
    assert_eq!(
        resize_handles, 8,
        "today's handle code draws eight resize handles on a 1-D shape (P7-33 owns the handles)"
    );
}

/// The x of the line's midpoint (page inches).
fn mid_x(document: &Document, connector: ShapeId) -> f64 {
    let (begin, end) = segment(document, connector);
    (begin.x + end.x) / 2.0
}

/// A right-to-left drag draws the same pixels as the left-to-right one
/// (negative `Width`, ADR 0084 rule 1): the same window render.
fn a_right_to_left_drag_draws_the_same_pixels(kind: LayoutKind) {
    let mut left_to_right = rig(kind);
    draw_connector(&mut left_to_right, kind, true);
    let forward = left_to_right.render();

    let mut right_to_left = rig(kind);
    draw_connector(&mut right_to_left, kind, false);
    let mirrored = right_to_left.render();

    crate::golden::compare_images(&forward, &mirrored)
        .unwrap_or_else(|why| panic!("the reversed drag draws different pixels: {why}"));

    // The reversed connector is genuinely right to left: its `Width`
    // is negative and its begin lies right of its end.
    let document = right_to_left
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone();
    let drawn = connector(&right_to_left.shell.borrow().app);
    assert!(
        number(&document, drawn, "Width") < 0.0,
        "negative Width: {}",
        number(&document, drawn, "Width")
    );
    assert!(
        number(&document, drawn, "BeginX") > number(&document, drawn, "EndX"),
        "the begin lies right of the end"
    );
}

#[test]
fn the_connector_draws_glues_and_strokes_in_panel_modern() {
    the_connector_draws_glues_and_strokes(LayoutKind::PanelModern);
}

#[test]
fn the_connector_draws_glues_and_strokes_in_panel_classic() {
    the_connector_draws_glues_and_strokes(LayoutKind::PanelClassic);
}

#[test]
fn a_right_to_left_drag_draws_the_same_pixels_in_panel_modern() {
    a_right_to_left_drag_draws_the_same_pixels(LayoutKind::PanelModern);
}

#[test]
fn a_right_to_left_drag_draws_the_same_pixels_in_panel_classic() {
    a_right_to_left_drag_draws_the_same_pixels(LayoutKind::PanelClassic);
}
