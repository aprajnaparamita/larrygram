//! The eight app shots (task P7-38 goal 1): each builder sets up the
//! shot's state through real input in the full shell, renders the last
//! frame with the wgpu renderer and returns it with the crop
//! rectangle (logical window pixels). The crop and size rules live in
//! [`crate::helpers`].
//!
//! Every document is named "Order process.vsdx" (a neutral name: no
//! user name, home directory or machine path may appear in a shot)
//! and every shot runs on the app's light theme.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_geom::Point;
use larrygram_ui_spec::LayoutKind;

use crate::flowchart::{ThreeShapes, build_flowchart, build_three_connected};
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
/// flow. No synthetic cursor: the shots are clean app captures.
fn flowchart_rig(kind: LayoutKind, scale: f32) -> (Rig, crate::flowchart::Flowchart) {
    let mut rig = Rig::at(AppCtx::new(rig::settings_for(kind)), WINDOW, scale, false);
    let flowchart = build_flowchart(&mut rig);
    name_document(&mut rig);
    rig.settle(3);
    rig.click_named("Flow");
    rig.settle(2);
    rig.center_view(4.5, 5.0, 0.85);
    (rig, flowchart)
}

/// The page-inch centre of the shape (`x`, `y`) as the flowchart
/// builds it: the master's pin is its centre.
fn at(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// `hero-panel-modern`: Panel Modern, Home tab, the named flowchart,
/// one Process shape selected with handles, the Shapes pane open on
/// Basic Flowchart. The full window at 16:10.
fn hero_panel_modern(scale: f32) -> Frame {
    let (mut rig, _flowchart) = flowchart_rig(LayoutKind::PanelModern, scale);
    // A real canvas click on the "Fix order" Process shape (page one)
    // selects it (its handles draw with the Pointer tool).
    let pos = rig.screen(at(5.5, 5.0));
    rig::click_at(&mut rig, pos, egui::PointerButton::Primary);
    rig.settle(2);
    // Collapse the "Basic Shapes" section so the pane is open on
    // Basic Flowchart (its header click toggles the section).
    rig.settle(2);
    if let Some(rect) = section_header(&mut rig, "Basic Shapes") {
        rig::click_at(&mut rig, rect, egui::PointerButton::Primary);
    }
    rig.settle(2);
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

/// The centre of the Shapes pane's stencil section header `name`.
fn section_header(rig: &mut Rig, name: &str) -> Option<egui::Pos2> {
    rig.run();
    let pane = rig.shell.borrow().shapes_window_rect()?;
    let root = rig.harness().root();
    use egui_kittest::kittest::Queryable;
    root.query_all_by_label(name)
        .map(|node| node.rect())
        .find(|rect| rect.center().x >= pane.min.x && rect.center().x <= pane.max.x)
        .map(|rect| rect.center())
}

/// `layout-panel-classic`: the same drawing in Panel Classic with the
/// Standard and Formatting toolbars docked and the Edit menu open.
fn layout_panel_classic(scale: f32) -> Frame {
    let (mut rig, _flowchart) = flowchart_rig(LayoutKind::PanelClassic, scale);
    // The Edit menu opens from a real pointer click on the menu bar.
    rig.click_named("Edit");
    rig.settle(3);
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

/// `connectors-reroute`: three shapes joined by two glued connectors,
/// the middle shape held mid-drag so the connectors follow. The crop
/// is a 4:3 region of the canvas around the drag.
fn connectors_reroute(scale: f32) -> Frame {
    // The synthetic cursor stays on: it is the pointer that drags.
    let mut rig = Rig::at(
        AppCtx::new(rig::settings_for(LayoutKind::PanelModern)),
        WINDOW,
        scale,
        true,
    );
    let ThreeShapes { middle } = build_three_connected(&mut rig);
    name_document(&mut rig);
    rig.settle(3);
    rig.center_view(4.25, 6.0, 1.0);
    let middle_screen = rig.screen(at(4.25, 6.0));
    let from = middle_screen;
    let to = middle_screen + egui::vec2(60.0, -120.0);
    rig::drag_and_hold(&mut rig, from, to);
    // The crop: the canvas area around the row and the drag preview
    // (the shot crops to the canvas).
    let canvas = rig.shell.borrow().canvas_rect().expect("the canvas drew");
    let mut focus = egui::Rect::from_center_size(from, egui::vec2(760.0, 120.0));
    focus = focus.union(egui::Rect::from_center_size(to, egui::vec2(160.0, 160.0)));
    let crop = crop_around(focus, canvas, (4, 3), (1600, 1200), scale);
    let _ = middle;
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
    let mut rig = Rig::at(
        AppCtx::new(rig::settings_for(LayoutKind::PanelModern)),
        egui::vec2(1600.0, 1100.0),
        scale,
        true,
    );
    rig.dispatch("file.new");
    name_document(&mut rig);
    rig.settle(4);
    // Scroll the pane until the Decision tile is fully visible (it
    // sits below the Basic Shapes tiles).
    let pane = rig.shell.borrow().shapes_window_rect().expect("the pane");
    for _ in 0..12 {
        rig.run();
        let tile = decision_tile(&mut rig);
        if tile.max.y < pane.max.y - 12.0 && tile.min.y > pane.min.y {
            break;
        }
        rig::wheel_lines(&mut rig, pane.center(), -3.0);
    }
    let tile = decision_tile(&mut rig);
    let canvas = rig.shell.borrow().canvas_rect().expect("the canvas drew");
    let drop = egui::pos2(canvas.min.x + canvas.width() * 0.35, canvas.center().y);
    rig::drag_and_hold(&mut rig, tile.center(), drop);
    // The crop: the pane and the left third of the canvas.
    let left_third = egui::Rect::from_min_max(
        canvas.min,
        egui::pos2(canvas.min.x + canvas.width() / 3.0, canvas.max.y),
    );
    let focus = pane.union(left_third);
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1100.0));
    let crop = crop_around(focus, bounds, (4, 3), (1600, 1200), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// The Decision tile's rect in the Shapes pane.
fn decision_tile(rig: &mut Rig) -> egui::Rect {
    rig.run();
    let pane = rig.shell.borrow().shapes_window_rect().expect("the pane");
    let root = rig.harness().root();
    use egui_kittest::kittest::Queryable;
    root.query_all_by_label("Decision")
        .map(|node| node.rect())
        .find(|rect| rect.center().x <= pane.max.x)
        .unwrap_or_else(|| panic!("no Decision tile in the pane"))
}

/// `text-editing`: the in-place editor open on the "Review order"
/// Process shape with the caret after the text. The crop is the shape
/// and its neighbours.
fn text_editing(scale: f32) -> Frame {
    let (mut rig, _flowchart) = flowchart_rig(LayoutKind::PanelModern, scale);
    rig.center_view(2.0, 7.0, 1.0);
    // A double click on the shape opens the editor (ADR 0044), caret
    // at the end of "Review order".
    let pos = rig.screen(at(2.0, 7.0));
    rig::double_click_at(&mut rig, pos);
    rig.settle(1);
    // The crop: the shape and its neighbours (inside the canvas).
    let top_left = rig.screen(at(1.4, 7.4));
    let bottom_right = rig.screen(at(2.6, 6.6));
    let shape = egui::Rect::from_min_max(top_left, bottom_right);
    let focus = shape.expand(120.0);
    let canvas = rig.shell.borrow().canvas_rect().expect("the canvas drew");
    let crop = crop_around(focus, canvas, (4, 3), (1600, 1200), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// `pages-tabs`: three pages, the second active, its page-tab menu
/// open. The crop is the tab strip and the menu.
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
    let tab = detail_tab(&mut rig);
    rig::click_at(&mut rig, tab.center(), egui::PointerButton::Primary);
    rig.settle(2);
    // A right-click at the tab's right edge draws its menu at the
    // pointer, beside the tab instead of on top of it.
    let tab = detail_tab(&mut rig);
    let at = egui::pos2(tab.max.x - 4.0, tab.center().y);
    rig::click_at(&mut rig, at, egui::PointerButton::Secondary);
    rig.settle(3);
    // The crop: the tab strip and the open menu (the site's "bottom
    // third", sized up to the minimum around the strip and menu).
    let mut focus = tab.expand(20.0);
    for name in ["Flow", "Review"] {
        if let Some(rect) = tab_named(&mut rig, name) {
            focus = focus.union(rect.expand(20.0));
        }
    }
    let focus = menu_rect(&mut rig, &tab)
        .map(|menu| focus.union(menu))
        .unwrap_or(focus);
    // The status line ("Page 2 of 3") is the three-pages evidence.
    let focus = status_rect(&mut rig)
        .map(|status| focus.union(status))
        .unwrap_or(focus);
    let crop = crop_around(focus, full_window(), (16, 9), (1920, 1080), scale);
    Frame {
        image: rig.render(),
        crop,
    }
}

/// The "Detail" page tab's rect (the tab strip at the bottom of the
/// window: the Shape Data pane also has a widget named "Detail").
fn detail_tab(rig: &mut Rig) -> egui::Rect {
    tab_named(rig, "Detail").unwrap_or_else(|| panic!("no Detail page tab"))
}

/// The page tab `name`'s rect (the tab strip at the bottom of the
/// window; other panes may draw widgets with the same name).
fn tab_named(rig: &mut Rig, name: &str) -> Option<egui::Rect> {
    rig.run();
    let root = rig.harness().root();
    use egui_kittest::kittest::Queryable;
    root.query_all_by_label(name)
        .map(|node| node.rect())
        .find(|rect| rect.center().y > WINDOW.y - 120.0)
}

/// The status line's rect (its "Page N of M" label at the window's
/// bottom edge).
fn status_rect(rig: &mut Rig) -> Option<egui::Rect> {
    rig.run();
    let root = rig.harness().root();
    use egui_kittest::kittest::Queryable;
    root.query_all_by_label("Page 2 of 3")
        .map(|node| node.rect())
        .find(|rect| rect.center().y > WINDOW.y - 60.0)
}

/// The open page-tab menu's rect (its rows near the tab: the tab
/// bar also has an "Insert" tab, and a "Delete" may appear in panes).
fn menu_rect(rig: &mut Rig, tab: &egui::Rect) -> Option<egui::Rect> {
    rig.run();
    let root = rig.harness().root();
    use egui_kittest::kittest::Queryable;
    let near = |rect: &egui::Rect| {
        rect.center().y > WINDOW.y - 400.0 && (rect.center() - tab.center()).length() < 400.0
    };
    let mut rows: Vec<egui::Rect> = Vec::new();
    for name in ["Rename", "Delete", "Insert", "Duplicate"] {
        rows.extend(
            root.query_all_by_label(name)
                .map(|node| node.rect())
                .filter(|rect| near(rect)),
        );
    }
    rows.into_iter().reduce(egui::Rect::union)
}

/// `welcome-wizard`: the first launch (clear settings), the wizard on
/// page 2 (Choose your layout) with Panel Modern selected, as the app
/// shows it over the window.
fn welcome_wizard(scale: f32) -> Frame {
    let mut rig = Rig::at(AppCtx::new(Settings::default()), WINDOW, scale, false);
    rig.settle(3);
    rig.click_named("Next");
    rig.settle(3);
    // Panel Modern selected (the current choice).
    rig.click_named("Panel Modern");
    rig.settle(3);
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}

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
    rig.click_named("Help");
    rig.settle(3);
    rig.click_named("About Larrygram");
    rig.settle(4);
    Frame {
        image: rig.render(),
        crop: full_window(),
    }
}
