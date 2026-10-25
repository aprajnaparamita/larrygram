//! Interaction polish (task P7-19): the whole Shapes-window tile, the
//! pane splitters and the shape handles, driven through the real
//! [`Shell`](larrygram_ui::shell::Shell) with pointer and key events
//! at display scales 1 and 2.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::{AppCtx, PaneSize};
use larrygram_model::ShapeId;
use larrygram_ui::splitter::COLLAPSED_SIZE;

use crate::{RichApp, ShellRig};

/// The two display scales every test runs at.
const SCALES: [f32; 2] = [1.0, 2.0];

/// The window of every test, in points.
const WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A shell over `app` at `scale`, one frame drawn.
fn rig(app: AppCtx, scale: f32) -> ShellRig {
    let mut rig = ShellRig::with_app_at(app, WINDOW, scale);
    rig.run();
    rig
}

/// The rich sweep state (a rectangle, selected, and a picture).
fn rich() -> AppCtx {
    RichApp::rich().0
}

/// The rich state's rectangle: the first shape of the page.
fn rectangle(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

fn press(rig: &mut ShellRig, pos: egui::Pos2, modifiers: egui::Modifiers) {
    let harness = rig.harness();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    rig.run();
}

fn move_to(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
}

fn release(rig: &mut ShellRig, pos: egui::Pos2, modifiers: egui::Modifiers) {
    let harness = rig.harness();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    rig.run();
}

/// A press, a move in four steps and a release.
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2, modifiers: egui::Modifiers) {
    press(rig, from, modifiers);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        move_to(rig, from + (to - from) * t);
    }
    release(rig, to, modifiers);
}

fn click(rig: &mut ShellRig, pos: egui::Pos2) {
    press(rig, pos, egui::Modifiers::NONE);
    release(rig, pos, egui::Modifiers::NONE);
}

fn double_click(rig: &mut ShellRig, pos: egui::Pos2) {
    click(rig, pos);
    click(rig, pos);
}

fn key(rig: &mut ShellRig, key: egui::Key) {
    rig.harness().key_press(key);
    rig.run();
}

/// Holds Alt (snapping off) or releases it.
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

fn page_shape_count(rig: &ShellRig) -> usize {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).map_or(0, |page| page.shapes.len())
}

fn undo_len(rig: &ShellRig) -> usize {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .undo
        .len()
}

/// The number value of one cell of `shape`.
fn number(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(larrygram_model::CellValue::Number(value)) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {name} on the shape"))
}

/// The last shape on the page.
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

/// The rect of the Shapes-window tile named `name` (the tiles sit in
/// the left pane).
fn tile(rig: &mut ShellRig, name: &str) -> egui::Rect {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .map(|node| node.rect())
        .find(|rect| rect.center().x < 400.0 && rect.height() > 30.0)
        .unwrap_or_else(|| panic!("no Shapes-window tile named {name:?}"))
}

/// Whether a Shapes-window tile named `name` is drawn.
fn has_tile(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .any(|node| node.rect().center().x < 400.0 && node.rect().height() > 30.0)
}

/// The screen point of a page point in the shell's canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// The grab zone of the splitter stored under `key`.
fn zone(rig: &ShellRig, key: &str) -> egui::Rect {
    rig.shell
        .borrow()
        .splitter(key)
        .and_then(|splitter| splitter.zone())
        .unwrap_or_else(|| panic!("no zone for {key}"))
}

fn size(rig: &ShellRig, key: &str) -> f32 {
    rig.shell
        .borrow()
        .splitter(key)
        .and_then(|splitter| splitter.size())
        .unwrap_or_else(|| panic!("no size for {key}"))
}

fn collapsed(rig: &ShellRig, key: &str) -> bool {
    rig.shell
        .borrow()
        .splitter(key)
        .is_some_and(|splitter| splitter.is_collapsed())
}

// --- the Shapes window ------------------------------------------------

/// A drag that starts on the tile's preview (not its label) drops one
/// master where it is released, in one undo step.
#[test]
fn a_tile_dragged_by_its_preview_drops_one_master_on_the_page() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        let before = page_shape_count(&rig);
        let undo = undo_len(&rig);
        let rect = tile(&mut rig, "Ellipse");
        let preview = egui::pos2(rect.min.x + 12.0, rect.center().y);
        set_alt(&mut rig, true);
        let target = screen(&rig, 5.0, 7.0);
        drag(&mut rig, preview, target, egui::Modifiers::ALT);
        set_alt(&mut rig, false);
        assert_eq!(page_shape_count(&rig), before + 1, "scale {scale}");
        assert_eq!(undo_len(&rig), undo + 1, "one undo step, scale {scale}");
        let shape = last_shape(&rig);
        assert!((number(&rig, shape, "PinX") - 5.0).abs() < 0.05, "scale {scale}");
        assert!((number(&rig, shape, "PinY") - 7.0).abs() < 0.05, "scale {scale}");
        let selected = rig.shell.borrow().shapes_window().selected().copied();
        assert_eq!(selected.map(|pick| pick.master), Some("Ellipse"));
    }
}

/// A click anywhere on the tile selects it; the arrows move the
/// selection and Enter drops the selected master at the page centre.
#[test]
fn a_tile_click_selects_and_the_keys_move_and_drop() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        let before = page_shape_count(&rig);
        let rect = tile(&mut rig, "Rectangle");
        click(&mut rig, egui::pos2(rect.min.x + 8.0, rect.min.y + 6.0));
        {
            let shell = rig.shell.borrow();
            let window = shell.shapes_window();
            assert_eq!(window.selected().map(|pick| pick.master), Some("Rectangle"));
            assert!(window.is_active(), "the click gives the window the keys");
        }
        key(&mut rig, egui::Key::ArrowDown);
        let next = rig
            .shell
            .borrow()
            .shapes_window()
            .selected()
            .map(|pick| pick.master);
        assert_ne!(next, Some("Rectangle"), "Down moves the selection");
        key(&mut rig, egui::Key::ArrowUp);
        key(&mut rig, egui::Key::Enter);
        assert_eq!(page_shape_count(&rig), before + 1, "Enter drops, scale {scale}");
        let shape = last_shape(&rig);
        let (width, height) = {
            let shell = rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            larrygram_ui::canvas::page_size_of(&session.document, session.active_page)
        };
        assert!((number(&rig, shape, "PinX") - width / 2.0).abs() < 1.0e-6);
        assert!((number(&rig, shape, "PinY") - height / 2.0).abs() < 1.0e-6);

        // A press on the canvas takes the keys back: Down nudges, it no
        // longer moves the Shapes-window selection.
        let empty = screen(&rig, 7.5, 9.5);
        click(&mut rig, empty);
        assert!(!rig.shell.borrow().shapes_window().is_active());
        let selected = rig.shell.borrow().shapes_window().selected().copied();
        key(&mut rig, egui::Key::ArrowDown);
        assert_eq!(
            rig.shell.borrow().shapes_window().selected().copied(),
            selected
        );
    }
}

/// A double-click on a tile drops its master at the page centre.
#[test]
fn a_tile_double_click_drops_at_the_page_centre() {
    let mut rig = rig(rich(), 1.0);
    let before = page_shape_count(&rig);
    let rect = tile(&mut rig, "Triangle");
    double_click(&mut rig, egui::pos2(rect.max.x - 10.0, rect.center().y));
    assert_eq!(page_shape_count(&rig), before + 1);
}

/// The whole header row toggles its section, and the search filters
/// the tiles.
#[test]
fn the_header_row_collapses_and_the_search_filters() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        assert!(has_tile(&mut rig, "Rectangle"));
        let header = rig
            .harness()
            .root()
            .query_all_by_role_and_label(Role::Button, "Basic Shapes")
            .map(|node| node.rect())
            .find(|rect| rect.center().x < 400.0)
            .expect("the Basic Shapes header");
        // The far right of the row, away from the arrow and the name.
        click(&mut rig, egui::pos2(header.max.x - 6.0, header.center().y));
        assert!(!has_tile(&mut rig, "Rectangle"), "collapsed, scale {scale}");
        assert!(rig.shell.borrow().shapes_window().is_collapsed("Basic Shapes"));
        click(&mut rig, egui::pos2(header.max.x - 6.0, header.center().y));
        assert!(has_tile(&mut rig, "Rectangle"), "expanded, scale {scale}");

        rig.shell.borrow_mut().shapes_window_mut().set_search("ellip");
        assert!(has_tile(&mut rig, "Ellipse"));
        assert!(!has_tile(&mut rig, "Rectangle"), "filtered out");
        rig.shell.borrow_mut().shapes_window_mut().set_search("");
        assert!(has_tile(&mut rig, "Rectangle"));
    }
}

// --- the splitters ----------------------------------------------------

/// A splitter drag changes the width by the drag distance, within the
/// limits, from either side of the edge; a double-click restores the
/// default; the width is written to the settings on release.
#[test]
fn a_splitter_drag_resizes_by_the_drag_distance_within_the_limits() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        let selection = rig.shell.borrow().app.selection.shapes.clone();
        let default = size(&rig, "shapes");
        let edge = zone(&rig, "shapes");
        // From the pane side of the edge.
        let from = egui::pos2(edge.min.x + 1.0, edge.center().y);
        drag(&mut rig, from, from + egui::vec2(60.0, 0.0), egui::Modifiers::NONE);
        assert_eq!(size(&rig, "shapes"), default + 60.0, "scale {scale}");
        assert_eq!(
            rig.shell.borrow().app.settings.pane_sizes.get("shapes"),
            Some(&PaneSize::new(default + 60.0, false)),
            "written on release"
        );
        // From the canvas side: the zone still wins, and the canvas
        // starts no marquee (the selection stays).
        let edge = zone(&rig, "shapes");
        let from = egui::pos2(edge.max.x - 1.0, edge.center().y);
        drag(&mut rig, from, from - egui::vec2(30.0, 0.0), egui::Modifiers::NONE);
        assert_eq!(size(&rig, "shapes"), default + 30.0, "scale {scale}");
        assert_eq!(rig.shell.borrow().app.selection.shapes, selection);
        // Far past the maximum: the 45% limit.
        let edge = zone(&rig, "shapes");
        drag(
            &mut rig,
            edge.center(),
            edge.center() + egui::vec2(2000.0, 0.0),
            egui::Modifiers::NONE,
        );
        assert_eq!(size(&rig, "shapes"), WINDOW.x * 0.45, "the maximum");
        // Below the minimum but above half of it: the minimum.
        let edge = zone(&rig, "shapes");
        let left = rig.shell.borrow().shapes_window_left();
        drag(
            &mut rig,
            edge.center(),
            egui::pos2(left + 80.0, edge.center().y),
            egui::Modifiers::NONE,
        );
        assert_eq!(size(&rig, "shapes"), 120.0, "the minimum");
        assert!(!collapsed(&rig, "shapes"));
        // A double-click restores the default.
        let edge = zone(&rig, "shapes");
        double_click(&mut rig, edge.center());
        assert_eq!(size(&rig, "shapes"), default, "the default");
    }
}

/// The right-hand panes and the Shape Data / Size & Position border
/// resize by the drag distance too.
#[test]
fn the_side_pane_and_the_shape_data_borders_resize() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        let width = size(&rig, "side_panes");
        let edge = zone(&rig, "side_panes");
        drag(
            &mut rig,
            edge.center(),
            edge.center() - egui::vec2(50.0, 0.0),
            egui::Modifiers::NONE,
        );
        assert_eq!(size(&rig, "side_panes"), width + 50.0, "scale {scale}");
        let height = size(&rig, "shape_data");
        let edge = zone(&rig, "shape_data");
        drag(
            &mut rig,
            edge.center(),
            edge.center() + egui::vec2(0.0, 40.0),
            egui::Modifiers::NONE,
        );
        assert_eq!(size(&rig, "shape_data"), height + 40.0, "scale {scale}");
    }
}

/// A pane dragged past half its minimum collapses to a strip, keeps
/// its width, and a click on the strip reopens it at that width; the
/// widths and the collapse survive a restart through the settings.
#[test]
fn a_pane_collapses_reopens_and_its_width_survives_a_restart() {
    for scale in SCALES {
        let mut rig = rig(rich(), scale);
        let edge = zone(&rig, "shapes");
        drag(
            &mut rig,
            edge.center(),
            edge.center() + egui::vec2(40.0, 0.0),
            egui::Modifiers::NONE,
        );
        let width = size(&rig, "shapes");
        let left = rig.shell.borrow().shapes_window_left();
        let edge = zone(&rig, "shapes");
        drag(
            &mut rig,
            edge.center(),
            egui::pos2(left + 5.0, edge.center().y),
            egui::Modifiers::NONE,
        );
        assert!(collapsed(&rig, "shapes"), "scale {scale}");
        assert_eq!(size(&rig, "shapes"), width, "the width is kept");
        rig.run();
        let strip = zone(&rig, "shapes");
        assert!(
            (strip.center().x - left - COLLAPSED_SIZE).abs() < 1.0,
            "the strip is {COLLAPSED_SIZE} pt"
        );
        assert!(!has_tile(&mut rig, "Rectangle"), "the tiles are hidden");

        // A restart reads the settings back: still collapsed.
        let settings = rig.shell.borrow().app.settings.clone();
        assert_eq!(
            settings.pane_sizes.get("shapes"),
            Some(&PaneSize::new(width, true))
        );
        let mut app = rich();
        app.settings = settings;
        let mut restarted = self::rig(app, scale);
        assert!(collapsed(&restarted, "shapes"));
        // The strip reopens it at the remembered width.
        click(&mut restarted, egui::pos2(left + 4.0, 300.0));
        assert!(!collapsed(&restarted, "shapes"), "reopened, scale {scale}");
        assert_eq!(size(&restarted, "shapes"), width);
        assert!(has_tile(&mut restarted, "Rectangle"));
        assert_eq!(
            restarted
                .shell
                .borrow()
                .app
                .settings
                .pane_sizes
                .get("shapes"),
            Some(&PaneSize::new(width, false))
        );
    }
}

/// The splitter shows the resize cursor on hover.
#[test]
fn a_splitter_hover_shows_the_resize_cursor() {
    let mut rig = rig(rich(), 1.0);
    let edge = zone(&rig, "shapes");
    move_to(&mut rig, edge.center());
    move_to(&mut rig, edge.center());
    assert_eq!(
        rig.harness().output().platform_output.cursor_icon,
        egui::CursorIcon::ResizeColumn
    );
    let edge = zone(&rig, "shape_data");
    move_to(&mut rig, edge.center());
    move_to(&mut rig, edge.center());
    assert_eq!(
        rig.harness().output().platform_output.cursor_icon,
        egui::CursorIcon::ResizeRow
    );
}

// --- the handles ------------------------------------------------------

/// The rich state at `zoom` with the rectangle (2 x 1 in at (3, 3))
/// panned to the middle of the canvas.
fn handle_rig(zoom: f64, scale: f32) -> ShellRig {
    let mut rig = rig(rich(), scale);
    rig.shell.borrow_mut().center_view_on(zoom, larrygram_geom::Point::new(3.0, 3.0));
    rig.run();
    rig.run();
    rig
}

/// A drag of the south-east handle resizes the rectangle by the drag
/// distance at 25%, 100% and 400% zoom and at both display scales, in
/// one undo step.
#[test]
fn a_handle_drag_resizes_at_every_zoom_and_scale() {
    for scale in SCALES {
        for zoom in [0.25, 1.0, 4.0] {
            let mut rig = handle_rig(zoom, scale);
            let shape = rectangle(&rig.shell.borrow().app);
            let undo = undo_len(&rig);
            // The south-east corner of the 2 x 1 rectangle.
            let corner = screen(&rig, 4.0, 2.5);
            // Two pixels off the corner, still on its square.
            let from = corner + egui::vec2(2.0, -2.0);
            let delta = egui::vec2(48.0, 24.0);
            set_alt(&mut rig, true);
            drag(&mut rig, from, from + delta, egui::Modifiers::ALT);
            set_alt(&mut rig, false);
            let k = 96.0 * zoom;
            let width = number(&rig, shape, "Width");
            let height = number(&rig, shape, "Height");
            assert!(
                (width - (2.0 + 48.0 / k)).abs() < 1.0e-3,
                "zoom {zoom} scale {scale}: width {width}"
            );
            assert!(
                (height - (1.0 + 24.0 / k)).abs() < 1.0e-3,
                "zoom {zoom} scale {scale}: height {height}"
            );
            assert_eq!(undo_len(&rig), undo + 1, "one undo step");
            assert_eq!(
                rig.shell.borrow().app.selection.shapes,
                vec![shape],
                "the selection stays"
            );
        }
    }
}

/// The rotate handle hits at 25% zoom (it sat on the north handle),
/// a Shift press on a handle still resizes, and no handle press starts
/// a marquee.
#[test]
fn the_rotate_handle_and_a_shift_press_never_start_a_marquee() {
    for scale in SCALES {
        let mut rig = handle_rig(0.25, scale);
        let shape = rectangle(&rig.shell.borrow().app);
        let top = screen(&rig, 3.0, 3.5);
        let rotate = top - egui::vec2(0.0, larrygram_ui::handles::ROTATE_OFFSET_PX as f32);
        drag(
            &mut rig,
            rotate,
            rotate + egui::vec2(40.0, 0.0),
            egui::Modifiers::NONE,
        );
        let angle = number(&rig, shape, "Angle");
        assert!(angle.abs() > 0.1, "rotated, scale {scale}: {angle}");
        assert_eq!(number(&rig, shape, "Width"), 2.0, "not resized");

        let mut rig = handle_rig(1.0, scale);
        let shape = rectangle(&rig.shell.borrow().app);
        let corner = screen(&rig, 4.0, 2.5) + egui::vec2(3.0, 3.0);
        rig.harness()
            .event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        drag(
            &mut rig,
            corner,
            corner + egui::vec2(48.0, 0.0),
            egui::Modifiers::SHIFT,
        );
        rig.harness()
            .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        rig.run();
        assert!(number(&rig, shape, "Width") > 2.2, "Shift still resizes");
        assert_eq!(rig.shell.borrow().app.selection.shapes, vec![shape]);
    }
}

/// The cursor shows the operation over each handle.
#[test]
fn the_cursor_shows_the_handle_operation() {
    for scale in SCALES {
        let mut rig = handle_rig(1.0, scale);
        for (point, cursor) in [
            ((4.0, 2.5), egui::CursorIcon::ResizeNwSe),
            ((2.0, 2.5), egui::CursorIcon::ResizeNeSw),
            ((4.0, 3.0), egui::CursorIcon::ResizeHorizontal),
            ((3.0, 3.5), egui::CursorIcon::ResizeVertical),
            ((3.4, 3.2), egui::CursorIcon::Move),
        ] {
            let pos = screen(&rig, point.0, point.1);
            move_to(&mut rig, pos);
            move_to(&mut rig, pos);
            assert_eq!(
                rig.harness().output().platform_output.cursor_icon,
                cursor,
                "{point:?} at scale {scale}"
            );
        }
        let rotate = screen(&rig, 3.0, 3.5)
            - egui::vec2(0.0, larrygram_ui::handles::ROTATE_OFFSET_PX as f32);
        move_to(&mut rig, rotate);
        move_to(&mut rig, rotate);
        assert_eq!(
            rig.harness().output().platform_output.cursor_icon,
            egui::CursorIcon::Grab
        );
    }
}
