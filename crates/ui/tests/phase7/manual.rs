//! The manual-pass fixes of task P7-23, each driven through the real
//! input path in both curated layouts (a pointer drag from a tile
//! picture and a tile label, typed text in the search box, a hover
//! over a menu item, menu clicks and key chords in the full shell):
//! the whole Shapes tile, the search filter and its empty result, the
//! menu hints, the Shape Data and Size & Position panes with nothing
//! selected, the status message in the status bar only, the layout
//! switch and the Options dialog reachability.

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::AppCtx;
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// The two curated layouts every test runs in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The window of every test, in points.
const WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A shell over the rich state (a rectangle and a picture, the
/// rectangle selected) in one layout.
fn layout_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    rig_of(app)
}

/// A shell over caller-chosen application state in one layout kind
/// (the state's `settings.ui_layout` is kept).
fn rig_of(app: AppCtx) -> ShellRig {
    let mut rig = ShellRig::with_app_at(app, WINDOW, 1.0);
    // The harness advances its clock between steps; keep two clicks
    // in one double-click window.
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
    rig.run();
    rig
}

fn press(rig: &mut ShellRig, pos: egui::Pos2) {
    let harness = rig.harness();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

fn move_to(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
}

fn release(rig: &mut ShellRig, pos: egui::Pos2) {
    let harness = rig.harness();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

/// A press, a move in four steps and a release.
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    press(rig, from);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        move_to(rig, from + (to - from) * t);
    }
    release(rig, to);
}

/// A press and a release in one frame batch (one harness `run` steps
/// its clock past egui's click time).
fn click(rig: &mut ShellRig, pos: egui::Pos2) {
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

/// One key chord (`key` with Command held: `Ctrl` covers Cmd on
/// macOS, the shortcuts table's token rule).
fn chord(rig: &mut ShellRig, key: egui::Key) {
    let harness = rig.harness();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

/// The rect of the Shapes-window tile named `name` (the tiles sit in
/// the left pane), or `None` when the tile is not drawn.
fn tile(rig: &mut ShellRig, name: &str) -> Option<egui::Rect> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .map(|node| node.rect())
        .find(|rect| rect.center().x < 400.0 && rect.height() > 30.0)
}

/// The screen point of a page point in the shell's canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// The nodes named `name` in the current frame, with their rects.
fn named_rects(rig: &mut ShellRig, name: &str) -> Vec<egui::Rect> {
    rig.run();
    rig.harness()
        .root()
        .children_recursive()
        .filter(|node| {
            let data = node.accesskit_node().data();
            data.label().unwrap_or_default() == name || data.value().unwrap_or_default() == name
        })
        .filter_map(|node| node.accesskit_node().bounding_box().map(|_| node.rect()))
        .collect()
}

/// Whether the frame draws a widget named `name`.
fn has_name(rig: &mut ShellRig, name: &str) -> bool {
    !named_rects(rig, name).is_empty()
}

/// The last shape on the active page.
fn last_shape(rig: &ShellRig) -> larrygram_model::ShapeId {
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

/// The number value of one cell of `shape`.
fn number(rig: &ShellRig, shape: larrygram_model::ShapeId, name: &str) -> f64 {
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

fn page_shape_count(rig: &ShellRig) -> usize {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session
        .document
        .page(page)
        .map_or(0, |page| page.shapes.len())
}

// --- item 1: the whole tile drags ------------------------------------

/// A drag that starts on the tile's picture and one that starts on
/// its label each drop a master where they are released (task P7-23
/// Goal 1: the whole tile works, in both layouts).
#[test]
fn a_tile_drag_from_the_picture_and_from_the_label_each_drop_a_master() {
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        let before = page_shape_count(&rig);
        let target = screen(&rig, 5.0, 7.0);
        // The picture, left of the label.
        let rect = tile(&mut rig, "Ellipse").expect("the Ellipse tile");
        drag(
            &mut rig,
            egui::pos2(rect.min.x + 12.0, rect.center().y),
            target,
        );
        assert_eq!(
            page_shape_count(&rig),
            before + 1,
            "the picture drags, {kind:?}"
        );
        let shape = last_shape(&rig);
        assert!((number(&rig, shape, "PinX") - 5.0).abs() < 0.05, "{kind:?}");
        assert!((number(&rig, shape, "PinY") - 7.0).abs() < 0.05, "{kind:?}");
        // The label, right of the picture.
        let target = screen(&rig, 6.0, 8.0);
        let rect = tile(&mut rig, "Triangle").expect("the Triangle tile");
        drag(
            &mut rig,
            egui::pos2(rect.min.x + 60.0, rect.center().y),
            target,
        );
        assert_eq!(
            page_shape_count(&rig),
            before + 2,
            "the label drags, {kind:?}"
        );
        let shape = last_shape(&rig);
        assert!((number(&rig, shape, "PinX") - 6.0).abs() < 0.05, "{kind:?}");
        assert!((number(&rig, shape, "PinY") - 8.0).abs() < 0.05, "{kind:?}");
    }
}

// --- item 2: the search filters as the user types ---------------------

/// Typed text in the search box filters the tiles at once, in both
/// layouts; an empty result says so.
#[test]
fn typed_search_filters_the_tiles_and_an_empty_result_says_so() {
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        assert!(tile(&mut rig, "Rectangle").is_some(), "{kind:?}");
        let search = rig
            .harness()
            .root()
            .children_recursive()
            .find(|node| node.accesskit_node().role() == Role::TextInput)
            .expect("the search box");
        search.click();
        search.type_text("ellip");
        assert!(
            tile(&mut rig, "Ellipse").is_some(),
            "the match stays, {kind:?}"
        );
        assert!(
            tile(&mut rig, "Rectangle").is_none(),
            "filtered out, {kind:?}"
        );

        // A query with no match says so.
        let mut rig = layout_rig(kind);
        let search = rig
            .harness()
            .root()
            .children_recursive()
            .find(|node| node.accesskit_node().role() == Role::TextInput)
            .expect("the search box");
        search.click();
        search.type_text("no such master anywhere");
        assert!(
            has_name(&mut rig, "No shapes match the search."),
            "{kind:?}"
        );
    }
}

// --- item 3: the menu hints ------------------------------------------

/// The tooltip layer area of the current frame, or `None`.
fn tooltip_rect(rig: &mut ShellRig) -> Option<egui::Rect> {
    rig.harness.ctx.memory(|memory| {
        memory
            .areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|layer| layer.order == egui::Order::Tooltip)
            .filter_map(|layer| memory.area_rect(layer.id))
            .next()
    })
}

/// The distance from `pos` to `rect` (0 inside).
fn rect_distance(rect: egui::Rect, pos: egui::Pos2) -> f32 {
    let dx = (rect.min.x - pos.x).max(0.0).max(pos.x - rect.max.x);
    let dy = (rect.min.y - pos.y).max(0.0).max(pos.y - rect.max.y);
    dx.hypot(dy)
}

/// Hovers `pos` and runs frames until the menu hint appears (the
/// tooltip delay is 0.5 s and egui hides a hint whose click was less
/// than 0.1 s before the move, so the frames pass first).
fn hover_for_hint(rig: &mut ShellRig, pos: egui::Pos2) -> egui::Rect {
    for _ in 0..10 {
        rig.run();
    }
    rig.harness().hover_at(pos);
    for _ in 0..90 {
        rig.run();
        if let Some(rect) = tooltip_rect(rig) {
            return rect;
        }
    }
    panic!("no menu hint appeared");
}

/// Opens the Panel Classic menu titled `title` (a real click on the
/// menu-bar title).
fn open_menu(rig: &mut ShellRig, title: &str) {
    let rect = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::Button, title)
        .next()
        .unwrap_or_else(|| panic!("the {title} title"))
        .rect();
    click(rig, rect.center());
}

/// The menu hint of a hovered menu item appears next to the item
/// under the pointer, never at the far left of the window: the Panel
/// Classic menu bar and the context menus (which both layouts route
/// through the same rows).
#[test]
fn a_menu_hint_appears_next_to_the_item_under_the_pointer() {
    // Panel Classic: a row of the open View menu.
    let mut rig = layout_rig(LayoutKind::PanelClassic);
    open_menu(&mut rig, "View");
    let row = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "Task Pane")
        .next()
        .expect("the Task Pane row")
        .rect();
    let pointer = row.center() + egui::vec2(5.0, 0.0);
    let hint = hover_for_hint(&mut rig, pointer);
    assert!(
        rect_distance(hint, pointer) <= 24.0,
        "the hint {hint:?} is next to the pointer {pointer:?}"
    );

    // Both layouts: a row of the shape context menu (the shell's
    // right-click router draws it through the same menu rows).
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        // Put the rectangle under the canvas centre (the page point
        // of that screen position), so the right-click hits it.
        let canvas = rig.shell.borrow().canvas_rect().expect("the canvas");
        let at = canvas.center();
        let page = {
            let shell = rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            let ruler = larrygram_ui::panel_classic::metrics::ClassicMetrics::default().ruler_size;
            larrygram_ui::canvas::page_position(canvas, session, at, ruler).expect("a page point")
        };
        {
            let mut shell = rig.shell.borrow_mut();
            let session = shell.app.active_document_mut().expect("session");
            let page_id = session.active_page.expect("page");
            let shape = session.document.page(page_id).expect("page").shapes[0];
            let shape = session.document.shape_mut(shape).expect("shape");
            for cell in &mut shape.sheet.cells {
                match cell.name.as_str() {
                    "PinX" => {
                        cell.value = Some(larrygram_model::CellValue::Number(page.x));
                    }
                    "PinY" => {
                        cell.value = Some(larrygram_model::CellValue::Number(page.y));
                    }
                    _ => {}
                }
            }
            // The pointer tool caches the page scene by the session's
            // change counter; direct cell writes bump it here.
            session.changes += 1;
        }
        rig.run();
        rig.harness().event(egui::Event::PointerMoved(at));
        rig.run();
        for pressed in [true, false] {
            rig.harness().event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            rig.run();
        }
        for _ in 0..2 {
            rig.run();
        }
        let menu_items: Vec<String> = rig
            .harness()
            .root()
            .children_recursive()
            .filter(|node| node.accesskit_node().role() == Role::MenuItem)
            .filter_map(|node| node.accesskit_node().data().label().map(str::to_owned))
            .collect();
        let row = rig
            .harness()
            .root()
            .query_all_by_role_and_label(Role::MenuItem, "Cut")
            .next()
            .unwrap_or_else(|| panic!("the context menu opens, {kind:?}: {menu_items:?}"))
            .rect();
        let pointer = row.center() + egui::vec2(5.0, 0.0);
        let hint = hover_for_hint(&mut rig, pointer);
        assert!(
            rect_distance(hint, pointer) <= 24.0,
            "the hint {hint:?} is next to the pointer {pointer:?}, {kind:?}"
        );
    }
}

// --- item 4: the panes with nothing selected --------------------------

/// With nothing selected the Shape Data pane shows the page's data
/// once and the Size & Position pane hides its rows behind a label
/// (task P7-23 Goal 4), in both layouts.
#[test]
fn with_no_shape_selected_the_panes_show_the_page_data_once() {
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        // Ctrl+N: a blank drawing with nothing selected.
        chord(&mut rig, egui::Key::N);
        {
            let mut shell = rig.shell.borrow_mut();
            let app = &mut shell.app;
            let data = larrygram_ui::panes::shape_data_rows(app).expect("rows");
            assert_eq!(
                data,
                vec![larrygram_ui::panes::PaneRow::read_only("Name", "Page-1")],
                "the page's data once, {kind:?}"
            );
            let size = larrygram_ui::panes::size_position_rows(app).expect("rows");
            assert!(size.is_empty(), "no Size & Position rows, {kind:?}");
        }
        // What the panes paint: the label, no dash rows behind it.
        assert!(
            has_name(&mut rig, "No shape selected."),
            "the empty state says so, {kind:?}"
        );
        for gone in ["Width", "Height", "\u{2014}"] {
            assert!(
                !has_name(&mut rig, gone),
                "{gone:?} rows are hidden, {kind:?}"
            );
        }
    }
}

// --- item 5: the status message shows once ----------------------------

/// A command message ("Selected N shapes") shows in the status bar
/// only, never in the title strip too (task P7-23 Goal 5).
#[test]
fn a_command_message_shows_in_the_status_bar_only() {
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        // Ctrl+A: `edit.select_all`, whose message counts the shapes.
        chord(&mut rig, egui::Key::A);
        let message = {
            let shell = rig.shell.borrow();
            shell.status().to_owned()
        };
        assert!(
            message.starts_with("Selected "),
            "the command message, {kind:?}: {message:?}"
        );
        assert_eq!(message, "Selected 2 shapes", "{kind:?}");
        let rects = named_rects(&mut rig, &message);
        assert!(!rects.is_empty(), "the status bar shows it, {kind:?}");
        for rect in rects {
            assert!(
                rect.min.y > WINDOW.y - 80.0,
                "only in the status bar, {kind:?}: {rect:?}"
            );
        }
    }
}

// --- items 6 and 7: the layout switch and Settings --------------------

/// Panel Classic's View menu switches to Panel Modern as a radio item
/// and Panel Modern's File > Options switches back (task P7-23 Goal
/// 6: both directions).
#[test]
fn the_layout_switches_in_both_directions() {
    let mut rig = layout_rig(LayoutKind::PanelClassic);
    open_menu(&mut rig, "View");
    let row = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "Panel Modern")
        .next()
        .expect("the Panel Modern row")
        .rect();
    click(&mut rig, row.center());
    assert_eq!(
        rig.shell.borrow().app.settings.ui_layout,
        LayoutKind::PanelModern,
        "the View menu switches to Panel Modern"
    );
    assert!(rig.shell.borrow().app.settings.ui_layout != LayoutKind::PanelClassic);

    // Panel Modern: File > Options opens the dialog; its radio
    // switches back to Panel Classic.
    let file = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::Button, "File")
        .next()
        .expect("the File button")
        .rect();
    click(&mut rig, file.center());
    let options = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::Button, "Options")
        .next()
        .expect("the Options row")
        .rect();
    click(&mut rig, options.center());
    assert!(
        has_name(&mut rig, "Panel layout"),
        "the Options window opens"
    );
    let radio = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::RadioButton, "Panel Classic")
        .next()
        .expect("the Panel Classic radio")
        .rect();
    click(&mut rig, radio.center());
    assert_eq!(
        rig.shell.borrow().app.settings.ui_layout,
        LayoutKind::PanelClassic,
        "the Options window switches back"
    );
}

/// The Options window is reachable from Tools > Options in Panel
/// Classic and from Cmd+, in both layouts, and it offers the layout
/// choice (task P7-23 Goal 7).
#[test]
fn the_options_window_reaches_from_the_menu_and_from_cmd_comma() {
    // Tools > Options in Panel Classic (a menu click).
    let mut rig = layout_rig(LayoutKind::PanelClassic);
    open_menu(&mut rig, "Tools");
    let row = rig
        .harness()
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "Options...")
        .next()
        .expect("the Options row")
        .rect();
    click(&mut rig, row.center());
    assert!(
        has_name(&mut rig, "Panel layout"),
        "the Options window opens"
    );
    for choice in [
        "Panel Modern",
        "Panel Classic",
        "Panel (2016 data)",
        "Panel (365 data)",
    ] {
        assert!(
            has_name(&mut rig, choice),
            "the window offers the layout choice {choice:?}"
        );
    }

    // Cmd+, in both layouts (the shortcut table's `Ctrl+,`).
    for kind in LAYOUTS {
        let mut rig = layout_rig(kind);
        chord(&mut rig, egui::Key::Comma);
        assert!(
            has_name(&mut rig, "Panel layout"),
            "Cmd+, opens the Options window, {kind:?}"
        );
    }
}
