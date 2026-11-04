//! Moving connector ends under real input (task P7-33), in Panel
//! Modern and Panel Classic:
//!
//! - a selected connector shows a begin handle and an end handle only
//!   (no resize box, no rotation handle), and the glued end's marker
//!   differs from the free end's;
//! - dragging the end handle onto another shape's connection point
//!   shows that shape's points and the highlighted point during the
//!   drag, and after release the cells, formula and record name that
//!   shape;
//! - dragging the same end to empty page frees it (no record, literal
//!   cells) where it was released;
//! - an arrow key with the connector selected unglues both ends and
//!   nudges them in one step, and undo restores the glue and the
//!   points;
//! - while drawing with the Connector tool the hovered target
//!   highlights the same way.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, ConnectorEnd, Document, Page, PageId, Preserved, Row, Section,
    SectionName, Shape, ShapeId,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, fixture_shape, settings_for};

/// The two layouts the Connector tool exists in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelModern, LayoutKind::PanelClassic];

/// The page point of shape A's centre (the drag's start).
const A: (f64, f64) = (3.0, 3.0);

/// The page point of shape B's centre (the drag's end).
const B: (f64, f64) = (7.0, 3.0);

/// The page point of shape C's centre (the re-glue target).
const C: (f64, f64) = (7.0, 6.0);

/// Shape C's two connection points in page inches (the local
/// `(0.25, 0.25)` and `(1.75, 0.25)` of a 2 x 1 box at [`C`]).
const C_POINTS: [(f64, f64); 2] = [(6.25, 5.75), (7.75, 5.75)];

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

/// `fixture_shape` with the given connection points (local
/// coordinates).
fn shape_with_points(visio_id: i32, x: f64, y: f64, points: &[(f64, f64)]) -> Shape {
    let mut shape = fixture_shape(x, y, visio_id);
    let mut section = Section::new(SectionName::from_static("Connection"));
    for (ix, (px, py)) in points.iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", *px), cell("Y", *py)];
        section.rows.push(row);
    }
    shape.sheet.sections.push(section);
    shape
}

/// Two shapes at [`A`] and [`B`], or three with [`C`] (which carries
/// the two connection points of [`C_POINTS`]).
fn shapes(kind: LayoutKind, with_c: bool) -> (AppCtx, ShapeId, ShapeId, ShapeId) {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let left = document.add_shape(fixture_shape(A.0, A.1, 1));
    let right = document.add_shape(fixture_shape(B.0, B.1, 2));
    let mut third = ShapeId { index: 0 };
    if with_c {
        third = document.add_shape(shape_with_points(
            3,
            C.0,
            C.1,
            &[(0.25, 0.25), (1.75, 0.25)],
        ));
        page.shapes.push(third);
    }
    page.shapes.push(left);
    page.shapes.push(right);
    document.pages.push(page);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app.selection.page = Some(PageId { index: 0 });
    (app, left, right, third)
}

/// A shell with the fixtures centred in the viewport (the text
/// fixtures' rule).
fn rig(kind: LayoutKind, with_c: bool) -> ShellRig {
    let (app, _, _, _) = shapes(kind, with_c);
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
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 4.5),
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

/// A press, four moves and a release from `from` to `to` (the text
/// fixtures' drag), with `alt` held (glue and snapping off).
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2, alt: bool) {
    let modifiers = if alt {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::NONE
    };
    rig.harness()
        .event(egui::Event::ModifiersChanged(modifiers));
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
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
        modifiers,
    });
    rig.run();
    rig.harness()
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

/// One key press (down and up) with `modifiers`.
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

fn snapshot(rig: &ShellRig) -> Document {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .document
        .clone()
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

/// The page's `Connects` records.
fn records(document: &Document) -> Vec<larrygram_model::Connect> {
    document
        .page(PageId { index: 0 })
        .expect("page")
        .connects
        .clone()
}

/// The last shape on the page: the drawn connector.
fn drawn_last(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    let shapes = session.document.page(page).expect("page").shapes.clone();
    *shapes.last().expect("the drawn connector")
}

/// Draws one connector with the Connector tool from [`A`] to `to`.
fn draw_connector(rig: &mut ShellRig, kind: LayoutKind, to: (f64, f64)) {
    click(rig, Role::Button, connector_tool_button(kind));
    let from = screen(rig, A.0, A.1);
    let to = screen(rig, to.0, to.1);
    drag(rig, from, to, false);
    click(rig, Role::Button, "Pointer Tool");
}

/// Selects the drawn connector with a real click on its line.
fn select_line(rig: &mut ShellRig, at: (f64, f64)) {
    let on_line = screen(rig, at.0, at.1);
    click_at(rig, on_line);
}

/// The end handles of the selected connector, through the handle code
/// (task P7-33 item 1).
fn end_handles(rig: &ShellRig) -> Option<[larrygram_render::EndMark; 2]> {
    let shell = rig.shell.borrow();
    let app = &shell.app;
    let connector = *app.selection.shapes.first().expect("one selected");
    larrygram_ui::handles::end_handles(app, None, &[connector])
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

/// Draws [`A`] to a free end at (5.5, 5.5): the begin glues
/// dynamically to A, the end is free. The route has a bend, so the
/// selection bounds have real corners beside the two end points. The
/// draw leaves the connector selected; the caller deselects it (for
/// its "before" picture) and selects it again by clicking its route.
fn rig_free_end(kind: LayoutKind) -> ShellRig {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind, (5.5, 5.5));
    // Click empty page: the draw's own selection goes.
    let empty = screen(&rig, 2.0, 5.5);
    click_at(&mut rig, empty);
    rig
}

/// Clicks the connector's begin stub (a right-angle route always
/// leaves the glued end along its exit direction) to select it.
fn select_connector(rig: &mut ShellRig) {
    select_line(rig, (4.125, 3.0));
}

// ---------------------------------------------------------------------------
// Item 1: the 1-D selection handles
// ---------------------------------------------------------------------------

/// One layout's flow: a selected connector shows a begin handle and an
/// end handle only, and the glued end's marker differs from the free
/// end's (the changed pixels of the picture sit at the two handles and
/// nowhere else).
fn a_selected_connector_shows_two_end_handles_only(kind: LayoutKind) {
    let mut rig = rig_free_end(kind);
    let before = rig.render();
    select_connector(&mut rig);

    // The handle code: exactly two end handles, one glued and one
    // free, and no resize box or outline in the overlay.
    let marks = end_handles(&rig).expect("a connector selection has end handles");
    assert_eq!(marks[0].end, ConnectorEnd::Begin);
    assert!(marks[0].glued, "the begin is glued to shape A");
    assert_eq!(marks[1].end, ConnectorEnd::End);
    assert!(!marks[1].glued, "the end is free");
    {
        let shell = rig.shell.borrow();
        let app = &shell.app;
        let overlay =
            larrygram_ui::handles::selection_overlay(app, None, &app.selection.shapes).expect("ok");
        assert_eq!(
            overlay.marks.len(),
            1,
            "end handles only: {:?}",
            overlay.marks
        );
        match overlay.marks.first() {
            Some(larrygram_render::OverlayMark::Handles { ends: Some(_), .. }) => {}
            other => panic!("expected the end handles, got {other:?}"),
        }
    }

    // The picture: the selection draws at the two end handles, and
    // none of the old resize or rotate handle anchors shows a glyph.
    let after = rig.render();
    assert!(
        changed_near(&before, &after, &rig, (marks[0].at.x, marks[0].at.y), 8.0),
        "the begin handle draws"
    );
    assert!(
        changed_near(&before, &after, &rig, (marks[1].at.x, marks[1].at.y), 8.0),
        "the end handle draws"
    );
    {
        let shell = rig.shell.borrow();
        let app = &shell.app;
        let bounds = larrygram_ui::handles::selection_bounds(app).expect("bounds");
        drop(shell);
        let zoom = 1.0;
        let anchors = larrygram_ui::handles::handles_for(
            bounds,
            larrygram_geom::Angle::from_radians(0.0),
            zoom,
        )
        .expect("handles");
        // The segment midpoint handles draw ink of their own (task
        // P7-35 item 1): an anchor on one is not a resize-box glyph.
        let segments = {
            let shell = rig.shell.borrow();
            let app = &shell.app;
            larrygram_ui::handles::segment_handles(app, None, &app.selection.shapes)
                .unwrap_or_default()
        };
        let far = |at: larrygram_geom::Point| {
            marks
                .iter()
                .all(|mark| (mark.at.x - at.x).hypot(mark.at.y - at.y) * 96.0 > 30.0)
                && segments
                    .iter()
                    .all(|(_, mid)| (mid.x - at.x).hypot(mid.y - at.y) * 96.0 > 30.0)
        };
        for (_kind, anchor) in anchors.iter().filter(|(_, at)| far(*at)) {
            assert!(
                !changed_near(&before, &after, &rig, (anchor.x, anchor.y), 3.0),
                "no resize-box glyph at the old anchor {anchor:?}"
            );
        }
    }

    // The glued marker is filled and the free one hollow: the pixels
    // just inside each marker differ.
    let begin = screen(&rig, marks[0].at.x, marks[0].at.y);
    let finish = screen(&rig, marks[1].at.x, marks[1].at.y);
    let inside = |at: egui::Pos2| pixel(&after, egui::pos2(at.x, at.y - 3.0));
    let glued = inside(begin);
    let free = inside(finish);
    assert_ne!(
        glued, free,
        "the glued-end marker differs from the free one"
    );
}

#[test]
fn a_selected_connector_shows_two_end_handles_only_in_panel_modern() {
    a_selected_connector_shows_two_end_handles_only(LayoutKind::PanelModern);
}

#[test]
fn a_selected_connector_shows_two_end_handles_only_in_panel_classic() {
    a_selected_connector_shows_two_end_handles_only(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 2: dragging an end onto a connection point
// ---------------------------------------------------------------------------

/// One layout's flow: drag the end handle from B onto C's connection
/// point. During the drag the picture shows C's points and the
/// highlighted point; after release the cells, formula and record name
/// C; undo restores.
fn an_end_drag_glues_to_the_connection_point_under_the_pointer(kind: LayoutKind) {
    let mut rig = rig(kind, true);
    draw_connector(&mut rig, kind, B);
    select_line(&mut rig, (5.0, 3.0));
    let marks = end_handles(&rig).expect("end handles");
    let finish = marks[1];
    assert_eq!(finish.end, ConnectorEnd::End);

    let before = rig.render();
    // Drag the end handle to C's first connection point, and stop just
    // before the release to look at the feedback.
    let from = screen(&rig, finish.at.x, finish.at.y);
    let to = screen(&rig, C_POINTS[0].0, C_POINTS[0].1);
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
    let during = rig.render();
    // C's two connection points show as markers and the landing point
    // is highlighted: ink appears at both points.
    for point in C_POINTS {
        assert!(
            changed_near(&before, &during, &rig, point, 8.0),
            "no feedback ink at C's point {point:?}"
        );
    }
    rig.harness().event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();

    // After release: the cells, formula and record name C.
    let document = snapshot(&rig);
    let connector = drawn_last(&rig.shell.borrow().app);
    assert_eq!(
        (
            number(&document, connector, "EndX"),
            number(&document, connector, "EndY"),
        ),
        C_POINTS[0],
        "the end lands on C's connection point"
    );
    assert_eq!(
        formula(&document, connector, "EndX"),
        Some("PAR(PNT(Sheet.3!Connections.X1,Sheet.3!Connections.Y1))".to_owned()),
        "the formula names C's row"
    );
    let record = records(&document)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(record.to_sheet, 3, "the record names C");
    assert_eq!(record.to_cell, "Connections.X1");
    assert_eq!(record.to_part, 100);

    // Undo: the end glues to B again (both records restored).
    rig.dispatch("edit.undo");
    let document = snapshot(&rig);
    let record = records(&document)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(record.to_sheet, 2, "undo restores the glue to B");
}

#[test]
fn an_end_drag_glues_to_the_connection_point_under_the_pointer_in_panel_modern() {
    an_end_drag_glues_to_the_connection_point_under_the_pointer(LayoutKind::PanelModern);
}

#[test]
fn an_end_drag_glues_to_the_connection_point_under_the_pointer_in_panel_classic() {
    an_end_drag_glues_to_the_connection_point_under_the_pointer(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 2: dragging an end to empty page
// ---------------------------------------------------------------------------

/// One layout's flow: drag the same end to empty page (Alt held: no
/// snapping, no glue) and check the end is free — no record, literal
/// cells — where it was released.
fn an_end_drag_to_empty_page_frees_the_end_where_it_lands(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind, B);
    select_line(&mut rig, (5.0, 3.0));
    let marks = end_handles(&rig).expect("end handles");
    let finish = marks[1];
    assert!(finish.glued, "the end starts glued to B");
    let connector = rig
        .shell
        .borrow()
        .app
        .selection
        .shapes
        .first()
        .copied()
        .expect("one selected");

    // The end comes loose on empty page (Alt held: no snapping, so the
    // release point is exact).
    let from = screen(&rig, finish.at.x, finish.at.y);
    let to = screen(&rig, 5.5, 4.5);
    drag(&mut rig, from, to, true);

    let document = snapshot(&rig);
    assert_eq!(formula(&document, connector, "EndX"), None, "a literal");
    assert_eq!(formula(&document, connector, "EndY"), None, "a literal");
    assert_eq!(
        formula(&document, connector, "EndTrigger"),
        Some("_XFTRIGGER(EventXFMod)".to_owned()),
        "the trigger names its own sheet"
    );
    assert!(
        records(&document)
            .iter()
            .all(|record| record.from_cell != "EndX"),
        "no record for the free end"
    );
    assert!(
        (number(&document, connector, "EndX") - 5.5).abs() < 1.0e-3,
        "free where it was released: {}",
        number(&document, connector, "EndX")
    );
    assert!(
        (number(&document, connector, "EndY") - 4.5).abs() < 1.0e-3,
        "free where it was released: {}",
        number(&document, connector, "EndY")
    );

    // Undo: one step restores the end's glue to B.
    rig.dispatch("edit.undo");
    let document = snapshot(&rig);
    let record = records(&document)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(record.to_sheet, 2, "undo restores the glue to B");
    assert!(
        formula(&document, connector, "EndX").is_some(),
        "undo restores the formula"
    );
}

#[test]
fn an_end_drag_to_empty_page_frees_the_end_where_it_lands_in_panel_modern() {
    an_end_drag_to_empty_page_frees_the_end_where_it_lands(LayoutKind::PanelModern);
}

#[test]
fn an_end_drag_to_empty_page_frees_the_end_where_it_lands_in_panel_classic() {
    an_end_drag_to_empty_page_frees_the_end_where_it_lands(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 5: the arrow keys
// ---------------------------------------------------------------------------

/// One layout's flow: an arrow key with the connector selected unglues
/// both ends (formulas and records) and nudges it by one nudge in one
/// undo step; undo restores the glue and the points.
fn an_arrow_key_unglues_both_ends_and_nudges_in_one_step(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    draw_connector(&mut rig, kind, B);
    select_line(&mut rig, (5.0, 3.0));
    let connector = rig
        .shell
        .borrow()
        .app
        .selection
        .shapes
        .first()
        .copied()
        .expect("one selected");
    let before = snapshot(&rig);
    let begin = before
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("begin");
    let finish = before
        .connector_end(connector, ConnectorEnd::End)
        .expect("end");
    assert_eq!(records(&before).len(), 2, "both ends start glued");

    let steps = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .undo
        .len();
    // Ctrl, Alt and Cmd arrows are not the nudge (the P7-33 review's
    // major 2): Ctrl+arrow belongs to `connector.auto_connect`, and
    // the connector keeps its glue.
    for modifiers in [
        egui::Modifiers::CTRL,
        egui::Modifiers::ALT,
        egui::Modifiers::MAC_CMD,
    ] {
        press_key(&mut rig, egui::Key::ArrowRight, modifiers);
        let held = snapshot(&rig);
        assert_eq!(records(&held).len(), 2, "{modifiers:?} keeps both glues");
        assert_eq!(
            held.connector_end(connector, ConnectorEnd::Begin),
            Ok(begin),
            "{modifiers:?} does not nudge"
        );
    }
    press_key(&mut rig, egui::Key::ArrowRight, egui::Modifiers::NONE);

    let after = snapshot(&rig);
    assert!(
        records(&after)
            .iter()
            .all(|record| record.from_sheet != after.shape(connector).expect("c").visio_id),
        "both records go"
    );
    for name in ["BeginX", "BeginY", "EndX", "EndY"] {
        assert_eq!(formula(&after, connector, name), None, "{name} is literal");
    }
    for name in ["BegTrigger", "EndTrigger"] {
        assert_eq!(
            formula(&after, connector, name),
            Some("_XFTRIGGER(EventXFMod)".to_owned()),
            "{name} names its own sheet"
        );
    }
    let nudged = after
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("begin");
    assert!(
        (nudged.x - (begin.x + larrygram_ui::shell::NUDGE_IN)).abs() < 1.0e-3,
        "one nudge: {nudged:?} vs {begin:?}"
    );
    assert!((nudged.y - begin.y).abs() < 1.0e-3, "{nudged:?}");
    let nudged = after
        .connector_end(connector, ConnectorEnd::End)
        .expect("end");
    assert!(
        (nudged.x - (finish.x + larrygram_ui::shell::NUDGE_IN)).abs() < 1.0e-3,
        "one nudge: {nudged:?} vs {finish:?}"
    );
    let steps_after = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .undo
        .len();
    assert_eq!(steps_after, steps + 1, "one undo step");

    rig.dispatch("edit.undo");
    let restored = snapshot(&rig);
    assert_eq!(records(&restored).len(), 2, "undo restores both records");
    assert_eq!(
        formula(&restored, connector, "BeginX"),
        Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)".to_owned()),
        "undo restores the formula"
    );
    let back = restored
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("begin");
    assert!((back.x - begin.x).abs() < 1.0e-3, "{back:?} vs {begin:?}");

    // Shift+arrow nudges one screen pixel at the current zoom (100%:
    // one inch/96).
    press_key(&mut rig, egui::Key::ArrowRight, egui::Modifiers::SHIFT);
    let shifted = snapshot(&rig);
    let nudged = shifted
        .connector_end(connector, ConnectorEnd::Begin)
        .expect("begin");
    assert!(
        (nudged.x - (begin.x + 1.0 / 96.0)).abs() < 1.0e-3,
        "one pixel: {nudged:?} vs {begin:?}"
    );
}

/// Ordinary shapes keep today's arrow behaviour: the connector unglue
/// step never touches them (task P7-33 item 5).
fn the_arrow_step_leaves_ordinary_shapes_alone(kind: LayoutKind) {
    let mut rig = rig(kind, false);
    let at = screen(&rig, A.0, A.1);
    click_at(&mut rig, at);
    {
        let app = &rig.shell.borrow().app;
        assert!(
            app.selection.shapes.iter().any(|shape| shape.index == 0),
            "the click selects shape A"
        );
    }
    let before = snapshot(&rig);
    press_key(&mut rig, egui::Key::ArrowRight, egui::Modifiers::NONE);
    assert_eq!(
        snapshot(&rig),
        before,
        "ordinary shapes keep today's arrow behaviour"
    );
}

#[test]
fn the_arrow_step_leaves_ordinary_shapes_alone_in_panel_modern() {
    the_arrow_step_leaves_ordinary_shapes_alone(LayoutKind::PanelModern);
}

#[test]
fn the_arrow_step_leaves_ordinary_shapes_alone_in_panel_classic() {
    the_arrow_step_leaves_ordinary_shapes_alone(LayoutKind::PanelClassic);
}

#[test]
fn an_arrow_key_unglues_both_ends_and_nudges_in_one_step_in_panel_modern() {
    an_arrow_key_unglues_both_ends_and_nudges_in_one_step(LayoutKind::PanelModern);
}

#[test]
fn an_arrow_key_unglues_both_ends_and_nudges_in_one_step_in_panel_classic() {
    an_arrow_key_unglues_both_ends_and_nudges_in_one_step(LayoutKind::PanelClassic);
}

// ---------------------------------------------------------------------------
// Item 3: hover feedback while drawing
// ---------------------------------------------------------------------------

/// One layout's flow: while drawing with the Connector tool the
/// hovered target's connection points show as markers and the landing
/// point is highlighted.
fn the_draw_gesture_shows_the_hover_feedback(kind: LayoutKind) {
    let mut rig = rig(kind, true);
    let before = rig.render();
    click(&mut rig, Role::Button, connector_tool_button(kind));
    let from = screen(&rig, A.0, A.1);
    let to = screen(&rig, C_POINTS[0].0, C_POINTS[0].1);
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
    let during = rig.render();
    for point in C_POINTS {
        assert!(
            changed_near(&before, &during, &rig, point, 8.0),
            "no hover feedback ink at C's point {point:?}"
        );
    }
    rig.harness().event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

#[test]
fn the_draw_gesture_shows_the_hover_feedback_in_panel_modern() {
    the_draw_gesture_shows_the_hover_feedback(LayoutKind::PanelModern);
}

#[test]
fn the_draw_gesture_shows_the_hover_feedback_in_panel_classic() {
    the_draw_gesture_shows_the_hover_feedback(LayoutKind::PanelClassic);
}
