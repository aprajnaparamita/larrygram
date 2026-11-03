//! The eight app shots (task P7-38 goal 1): each builder sets up the
//! shot's state through real input in the full shell, asserts that
//! the shell reached that state (task P7-43 item 3: a shot that drifts
//! from its description fails instead of writing a wrong picture),
//! renders the last frame with the wgpu renderer and returns it with
//! the crop rectangle (logical window pixels). The crop and size rules
//! live in [`crate::helpers`].
//!
//! The state checks read what the shell exposes: the app state
//! (`shell.app`: selection, pages, settings), the Shapes window, and
//! the accessibility tree the widgets publish (the shell keeps its
//! menus, editor and windows private).
//!
//! Every document is named "Order process.vsdx" (a neutral name: no
//! user name, home directory or machine path may appear in a shot),
//! every shot runs on the app's light theme, and the build steps'
//! status messages are cleared before the shot's own input.
//!
//! Steps that use a command dispatch instead of real input: the
//! drawings themselves (`file.new`, `page.rename`, `insert.shape`,
//! `shape.set.text`, `connector.draw`, the glue commands; the copied
//! exit-test build), `file.new` for the empty drawings of
//! `shapes-window` and `about-window`, and `page.add` for the third
//! page of `pages-tabs`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_geom::Point;
use larrygram_model::{CellLookup, CellName, CellValue};
use larrygram_ui_spec::LayoutKind;

use crate::flowchart::{Flowchart, ThreeShapes, build_flowchart, build_three_connected};
use crate::helpers::{Frame, crop_around};
use crate::rig::{self, Rig};

/// The full-window shots' logical window size (the site's shots pin
/// 1600 x 1000 logical px; the scale comes from the capture).
pub const WINDOW: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// Builds the shot `id`'s state at display `scale` and returns the
/// rendered frame with its crop.
pub fn build(id: &str, scale: f32) -> Frame {
    match id {
        "hero-panel-modern" => hero_panel_modern(scale),
        "layout-panel-classic" => layout_panel_classic(scale),
        "connectors-reroute" => connectors_reroute(scale),
        "shapes-window" => shapes_window(scale),
        "text-editing" => text_editing(scale),
        "pages-tabs" => pages_tabs(scale),
        "welcome-wizard" => welcome_wizard(scale),
        "about-window" => about_window(scale),
        other => panic!("unknown shot {other:?}"),
    }
}

/// The whole window as the crop.
fn full_window() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW)
}

/// Names the open document with the neutral name ("Order
/// process.vsdx"; the title bar shows the file name only). The
/// session's path is set directly, as the view's pan and zoom are:
/// a `file.save_as` leaves its own "Saved <path>" status message on
/// screen, and no machine path may appear in a shot.
fn name_document(rig: &mut Rig) {
    let mut shell = rig.shell.borrow_mut();
    let session = shell.app.active_document_mut().expect("a document");
    session.path = Some(PathBuf::from("Order process.vsdx"));
}

/// A settled shell in `kind` with the named flowchart open on page
/// one ("Flow", the build leaves page two active), framed on the
/// page, with a clear status line. No synthetic cursor: the shots are
/// clean app captures.
fn flowchart_rig(kind: LayoutKind, scale: f32) -> (Rig, Flowchart) {
    let mut rig = Rig::at(AppCtx::new(rig::settings_for(kind)), WINDOW, scale, false);
    let flowchart = build_flowchart(&mut rig);
    name_document(&mut rig);
    rig.settle(3);
    rig.click_named("Flow");
    rig.settle(2);
    assert_eq!(active_page_name(&rig), "Flow", "page one is active");
    rig.center_view(4.25, 5.0, 0.85);
    rig.clear_status();
    (rig, flowchart)
}

/// The page-inch point (`x`, `y`).
fn at(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A page-inch point kept in an `egui` position.
fn page_point(pos: egui::Pos2) -> Point {
    at(f64::from(pos.x), f64::from(pos.y))
}

/// The active page's name.
fn active_page_name(rig: &Rig) -> String {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("a document");
    let page = session.active_page.expect("an active page");
    session
        .document
        .page(page)
        .map(|page| page.name.clone())
        .expect("the active page exists")
}

/// One number cell of the active page's shape `visio_id`.
fn cell(rig: &Rig, visio_id: i32, name: &str) -> f64 {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("a document");
    let page = session.active_page.expect("an active page");
    let shape = session
        .document
        .shape_by_visio_id(page, visio_id)
        .expect("the shape is on the page");
    match session
        .document
        .resolve(shape, &CellName::new(name))
        .ok()
        .and_then(|found| found.value.cloned())
    {
        Some(CellValue::Number(value)) => value,
        other => panic!("{name} of shape {visio_id} is not a number: {other:?}"),
    }
}

/// The page-inch bounds of the 2-D shape `visio_id` (its pin is its
/// centre: the flowchart masters' `LocPin` is half the size).
fn bounds(rig: &Rig, visio_id: i32) -> egui::Rect {
    let (x, y) = (cell(rig, visio_id, "PinX"), cell(rig, visio_id, "PinY"));
    let (w, h) = (cell(rig, visio_id, "Width"), cell(rig, visio_id, "Height"));
    egui::Rect::from_center_size(
        egui::pos2(x as f32, y as f32),
        egui::vec2(w as f32, h as f32),
    )
}

/// The screen rect of a page-inch rect.
fn screen_rect(rig: &Rig, page: egui::Rect) -> egui::Rect {
    let a = rig.screen(at(f64::from(page.min.x), f64::from(page.min.y)));
    let b = rig.screen(at(f64::from(page.max.x), f64::from(page.max.y)));
    egui::Rect::from_two_pos(a, b)
}

/// The rect of the one widget named `name` inside `area`; fails the
/// shot when there is none.
fn rect_in(rig: &mut Rig, name: &str, area: egui::Rect) -> egui::Rect {
    rig.rects_named(name)
        .into_iter()
        .find(|rect| area.contains(rect.center()))
        .unwrap_or_else(|| panic!("no {name:?} in {area:?}"))
}

/// `hero-panel-modern`: Panel Modern, Home tab, the named flowchart,
/// one Process shape selected with handles, the Shapes pane open on
/// Basic Flowchart. The full window at 16:10.
fn hero_panel_modern(scale: f32) -> Frame {
    let (mut rig, flowchart) = flowchart_rig(LayoutKind::PanelModern, scale);
    // A real canvas click on the "Fix order" Process shape selects it
    // (its handles draw with the Pointer tool).
    let fix_order = flowchart.shapes[3].visio_id;
    let pos = rig.screen(page_point(bounds(&rig, fix_order).center()));
    rig::click_at(&mut rig, pos, egui::PointerButton::Primary);
    rig.settle(2);
    // Collapse the "Basic Shapes" section so the pane is open on
    // Basic Flowchart (its header click toggles the section).
    let header = section_header(&mut rig, "Basic Shapes");
    rig::click_at(&mut rig, header, egui::PointerButton::Primary);
    rig.settle(2);

    assert_eq!(rig.shell.borrow().layout_kind(), LayoutKind::PanelModern);
    assert_eq!(rig.anchor_visio_id(), Some(fix_order), "Fix order selected");
    assert_eq!(rig.shell.borrow().app.selection.shapes.len(), 1);
    let shell = rig.shell.borrow();
    assert!(shell.shapes_window().is_collapsed("Basic Shapes"));
    assert!(!shell.shapes_window().is_collapsed("Basic Flowchart"));
    drop(shell);
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

/// The centre of the Shapes pane's stencil section header `name`.
fn section_header(rig: &mut Rig, name: &str) -> egui::Pos2 {
    let pane = rig.shell.borrow().shapes_window_rect().expect("the pane");
    rect_in(rig, name, pane).center()
}

/// `layout-panel-classic`: the same drawing in Panel Classic with the
/// Standard and Formatting toolbars docked and the Edit menu open.
fn layout_panel_classic(scale: f32) -> Frame {
    let (mut rig, _flowchart) = flowchart_rig(LayoutKind::PanelClassic, scale);
    assert!(!rig.has("Select All"), "no menu is open yet");
    // The Edit menu opens from a real pointer click on the menu bar.
    rig.click_named("Edit");
    rig.settle(3);
    assert_eq!(rig.shell.borrow().layout_kind(), LayoutKind::PanelClassic);
    assert!(rig.has("Select All"), "the Edit menu is open");
    assert_eq!(rig.shell.borrow().status(), "", "no build message shows");
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

/// `connectors-reroute`: three shapes joined by two glued connectors.
/// Until the connector tasks (P7-32 on) re-route connectors live
/// during a drag, the shot shows the state after the release: the
/// middle shape moved up and right, both connectors re-routed to it.
/// The crop is a 4:3 region of the page viewport around the shapes.
fn connectors_reroute(scale: f32) -> Frame {
    let mut rig = Rig::at(
        AppCtx::new(rig::settings_for(LayoutKind::PanelModern)),
        WINDOW,
        scale,
        false,
    );
    let ThreeShapes { middle, connectors } = build_three_connected(&mut rig);
    name_document(&mut rig);
    rig.settle(3);
    rig.center_view(4.25, 6.0, 1.0);
    rig.clear_status();
    let before = bounds(&rig, middle);
    let from = rig.screen(page_point(before.center()));
    rig::drag(&mut rig, from, from + egui::vec2(48.0, -144.0));
    rig.settle(3);

    // The middle shape moved, and each connector's glued end follows
    // it (the end lies on the moved shape's outline or inside it).
    let after = bounds(&rig, middle);
    assert!(
        after.center().y - before.center().y > 1.0,
        "the middle shape moved up: {before:?} -> {after:?}"
    );
    let near = after.expand(0.05);
    for (connector, end) in [(connectors[0], "End"), (connectors[1], "Begin")] {
        let point = egui::pos2(
            cell(&rig, connector, &format!("{end}X")) as f32,
            cell(&rig, connector, &format!("{end}Y")) as f32,
        );
        assert!(
            near.contains(point),
            "connector {connector}'s {end} {point:?} follows the shape {after:?}"
        );
    }

    let viewport = rig.shell.borrow().canvas_viewport().expect("the canvas");
    let row = egui::Rect::from_min_max(egui::pos2(0.75, 5.625), egui::pos2(7.75, 6.375));
    let focus = screen_rect(&rig, row.union(after)).expand(32.0);
    let crop = crop_around(focus, viewport, (4, 3), (1600, 1200), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// `shapes-window`: the Shapes pane with Basic Shapes and Basic
/// Flowchart expanded, a Decision master held mid-drag onto the page.
/// The crop is the pane plus the left third of the canvas.
fn shapes_window(scale: f32) -> Frame {
    // The synthetic cursor stays on: it is the pointer that drags.
    // The window is 1600 x 1100 (this shot pins no window size): the
    // taller pane shows the Basic Shapes and Basic Flowchart sections
    // and the Decision tile at once.
    let window = egui::vec2(1600.0, 1100.0);
    let mut rig = Rig::at(
        AppCtx::new(rig::settings_for(LayoutKind::PanelModern)),
        window,
        scale,
        true,
    );
    rig.dispatch("file.new");
    name_document(&mut rig);
    rig.settle(4);
    rig.clear_status();
    // Scroll the pane until the Decision tile is fully visible (it
    // sits below the Basic Shapes tiles).
    let pane = rig.shell.borrow().shapes_window_rect().expect("the pane");
    for _ in 0..12 {
        let tile = rect_in(&mut rig, "Decision", pane);
        if tile.max.y < pane.max.y - 12.0 && tile.min.y > pane.min.y {
            break;
        }
        rig::wheel_lines(&mut rig, pane.center(), -3.0);
    }
    let tile = rect_in(&mut rig, "Decision", pane);
    let canvas = rig.shell.borrow().canvas_rect().expect("the canvas drew");
    let drop = egui::pos2(canvas.min.x + canvas.width() * 0.35, canvas.center().y);
    rig::drag_and_hold(&mut rig, tile.center(), drop);

    assert!(rig.pointer_down(), "the drag is still in progress");
    {
        let shell = rig.shell.borrow();
        let pick = shell
            .shapes_window()
            .selected()
            .expect("a master is picked");
        assert_eq!(pick.master, "Decision", "the Decision master is dragged");
        assert!(!shell.shapes_window().is_collapsed("Basic Shapes"));
        assert!(!shell.shapes_window().is_collapsed("Basic Flowchart"));
    }
    // The crop: the pane and the left third of the canvas.
    let left_third = egui::Rect::from_min_max(
        canvas.min,
        egui::pos2(canvas.min.x + canvas.width() / 3.0, canvas.max.y),
    );
    let focus = pane.union(left_third);
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, window);
    let crop = crop_around(focus, bounds, (4, 3), (1600, 1200), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// `text-editing`: the in-place editor open on the "Review order"
/// Process shape with the caret after the text. The view is centred
/// on the shape at a zoom where the crop stays inside the page.
fn text_editing(scale: f32) -> Frame {
    let (mut rig, flowchart) = flowchart_rig(LayoutKind::PanelModern, scale);
    let review = flowchart.shapes[1].visio_id;
    let shape = bounds(&rig, review);
    rig.center_view(
        f64::from(shape.center().x),
        f64::from(shape.center().y),
        1.25,
    );
    // The caret must be in its visible phase whichever frame renders.
    rig.steady_caret();
    // A double click on the shape opens the editor (ADR 0044), caret
    // at the end of "Review order".
    let pos = rig.screen(page_point(shape.center()));
    rig::double_click_at(&mut rig, pos);
    rig.settle(1);

    assert_eq!(rig.anchor_visio_id(), Some(review), "Review order selected");
    rig.run();
    let value = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
        .next()
        .map(|node| node.accesskit_node().value())
        .expect("the in-place editor is open");
    assert_eq!(value.as_deref(), Some("Review order"), "the editor's text");
    assert!(
        !rig.harness().ctx.global_style().visuals.text_cursor.blink,
        "the caret does not blink, so the render shows it"
    );
    // The crop is centred on the shape (it fits in the viewport, so
    // nothing pushes it off-centre) and stays on the page.
    let screen = screen_rect(&rig, shape);
    let viewport = rig.shell.borrow().canvas_viewport().expect("the canvas");
    let crop = crop_around(screen, viewport, (4, 3), (1600, 1200), scale);
    let page = screen_rect(
        &rig,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(8.5, 11.0)),
    );
    assert!(
        page.contains_rect(crop),
        "the crop {crop:?} stays on the page {page:?}"
    );
    assert!(
        (crop.center() - screen.center()).length() < 1.0,
        "the shape sits in the middle of the crop"
    );
    Frame {
        image: rig.render(),
        crop,
    }
}

/// The second page's name in `pages-tabs`.
const SECOND_PAGE: &str = "Detail";

/// `pages-tabs`: three pages, the second active, its page-tab menu
/// open with all three tabs in view. The crop is the tab strip and
/// the menu, right of the Shapes pane.
fn pages_tabs(scale: f32) -> Frame {
    let (mut rig, _flowchart) = flowchart_rig(LayoutKind::PanelModern, scale);
    // A third page, then the second page active (its tab click).
    rig.dispatch_args(
        "page.add",
        larrygram_commands::Args::new().with(
            "name",
            larrygram_commands::ArgValue::Text("Review".to_owned()),
        ),
    );
    rig.settle(2);
    rig.clear_status();
    let tab = tab_named(&mut rig, SECOND_PAGE);
    rig::click_at(&mut rig, tab.center(), egui::PointerButton::Primary);
    rig.settle(2);
    rig.clear_status();
    // The page-tab menu acts on the active page from anywhere on the
    // strip (every tab, "All" and "+" record the same `PageTab` hit
    // target), and it draws at the pointer and runs right. A press on
    // a tab would cover the tabs after it, so the right-click lands at
    // the strip's end, on the "+" button's right edge: the menu opens
    // beside the three tabs instead of over them.
    let plus = tab_named(&mut rig, "New page");
    let press = egui::pos2(plus.max.x - 3.0, plus.center().y);
    rig::click_at(&mut rig, press, egui::PointerButton::Secondary);
    rig.settle(3);

    assert_eq!(active_page_name(&rig), SECOND_PAGE, "page 2 is active");
    let status = status_rect(&mut rig);
    let menu = menu_rect(&mut rig, &plus);
    let tabs: Vec<egui::Rect> = ["Flow", SECOND_PAGE, "Review"]
        .into_iter()
        .map(|name| tab_named(&mut rig, name))
        .collect();
    for (rect, name) in tabs.iter().zip(["Flow", SECOND_PAGE, "Review"]) {
        assert!(
            !rect.intersects(menu),
            "the {name} tab {rect:?} is clear of the menu {menu:?}"
        );
    }

    let mut focus = menu.union(status);
    for rect in &tabs {
        focus = focus.union(*rect);
    }
    let pane = rig.shell.borrow().shapes_window_rect().expect("the pane");
    let bounds = egui::Rect::from_min_max(egui::pos2(pane.max.x, 0.0), WINDOW.to_pos2());
    let crop = crop_around(focus.expand(24.0), bounds, (16, 9), (1920, 1080), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// The page tab `name`'s rect (the tab strip at the bottom of the
/// window; other panes may draw widgets with the same name).
fn tab_named(rig: &mut Rig, name: &str) -> egui::Rect {
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, WINDOW.y - 120.0), WINDOW.to_pos2());
    rect_in(rig, name, strip)
}

/// The status line's "Page 2 of 3" label (the three-pages evidence).
fn status_rect(rig: &mut Rig) -> egui::Rect {
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, WINDOW.y - 60.0), WINDOW.to_pos2());
    rect_in(rig, "Page 2 of 3", strip)
}

/// The open page-tab menu's rect: the union of its rows near the
/// press (the tab bar also has an "Insert" tab, and a "Delete" may
/// appear in panes). Every row must be there.
fn menu_rect(rig: &mut Rig, near: &egui::Rect) -> egui::Rect {
    let near = egui::Rect::from_center_size(near.center(), egui::vec2(800.0, 800.0));
    let mut menu = ["Insert", "Delete", "Rename", "Duplicate"]
        .into_iter()
        .map(|name| rect_in(rig, name, near))
        .reduce(egui::Rect::union)
        .expect("four rows");
    // The rows whose names end in an ellipsis.
    rig.run();
    for start in ["Page Setup", "Reorder Pages"] {
        let row = rig
            .harness()
            .root()
            .query_all(egui_kittest::kittest::by().label_contains(start))
            .map(|node| node.rect())
            .find(|rect| near.contains(rect.center()))
            .unwrap_or_else(|| panic!("no {start:?} row"));
        menu = menu.union(row);
    }
    menu
}

/// `welcome-wizard`: the first launch (clear settings), the wizard on
/// page 2 (Choose your layout) with Panel Modern selected, as the app
/// shows it over the window.
fn welcome_wizard(scale: f32) -> Frame {
    let mut rig = Rig::at(AppCtx::new(Settings::default()), WINDOW, scale, false);
    rig.settle(3);
    assert!(!rig.has("Choose your layout"), "the wizard opens on page 1");
    rig.click_named("Next");
    rig.settle(3);
    // Panel Modern selected (the current choice).
    rig.click_named("Panel Modern");
    rig.settle(3);

    assert!(rig.has("Choose your layout"), "the wizard is on page 2");
    assert!(rig.has("Start"), "page 2's footer");
    rig.run();
    let selected = rig
        .harness()
        .root()
        .query_all_by_label("Panel Modern")
        .find_map(|node| node.accesskit_node().toggled());
    assert_eq!(
        selected,
        Some(egui::accesskit::Toggled::True),
        "Panel Modern is selected"
    );
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

/// The About window's first line (the dedication).
const DEDICATION: &str = "This app is dedicated to Larry Jeffus.";

/// `about-window`: Help > About Larrygram over an empty drawing.
fn about_window(scale: f32) -> Frame {
    let mut rig = Rig::at(
        AppCtx::new(rig::settings_for(LayoutKind::PanelModern)),
        WINDOW,
        scale,
        false,
    );
    rig.dispatch("file.new");
    name_document(&mut rig);
    rig.settle(3);
    rig.clear_status();
    assert!(!rig.has(DEDICATION), "the About window is closed");
    rig.click_named("Help");
    rig.settle(3);
    rig.click_named("About Larrygram");
    rig.settle(4);
    assert!(rig.has(DEDICATION), "the About window is shown");
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}
