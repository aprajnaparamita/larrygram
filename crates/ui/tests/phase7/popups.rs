//! The popup checks of task P7-17: after an open-then-leave sequence
//! no popup is left open, at every review window size, in both
//! layouts; and the File view's New creates a document.

use egui_kittest::kittest::Queryable;
use larrygram_ui_spec::LayoutKind;

use crate::review::SIZES;
use crate::{RichApp, ShellRig};

/// A settled rig of `kind` at `size`, scale 1.
fn rig(kind: LayoutKind, size: (f32, f32)) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app_at(app, egui::vec2(size.0, size.1), 1.0);
    for _ in 0..4 {
        rig.run();
    }
    rig
}

/// Whether a widget named `name` is drawn.
fn shows(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.harness().query_all_by_label(name).next().is_some()
}

/// Moves the pointer to the middle of the canvas, away from every
/// menu, and lets the submenu delay pass (0.4 s at kittest's step).
fn leave(rig: &mut ShellRig) {
    rig.run();
    let canvas = rig.shell.borrow().canvas_rect().expect("a canvas");
    let pos = canvas.center();
    rig.harness().hover_at(pos);
    rig.harness().run_steps(40);
}

/// A primary click in the middle of the canvas (a click outside
/// every popup).
fn click_outside(rig: &mut ShellRig) {
    let canvas = rig.shell.borrow().canvas_rect().expect("a canvas");
    crate::review::click_at(rig, canvas.center(), egui::PointerButton::Primary);
    rig.harness().run_steps(3);
}

/// A real pointer click on the first widget named `name` (egui opens
/// its popups from pointer clicks).
fn press(rig: &mut ShellRig, name: &str) {
    rig.run();
    let pos = rig
        .harness()
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"))
        .rect()
        .center();
    crate::review::click_at(rig, pos, egui::PointerButton::Primary);
    rig.harness().run_steps(2);
}

/// Hovers the first widget named `name` and lets the hover delay pass.
fn hover(rig: &mut ShellRig, name: &str) {
    rig.run();
    let pos = rig
        .harness()
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"))
        .rect()
        .center();
    rig.harness().hover_at(pos);
    rig.harness().run_steps(40);
}

/// Whether any egui popup is open.
fn any_popup(rig: &mut ShellRig) -> bool {
    rig.run();
    egui::Popup::is_any_open(&rig.harness().ctx)
}

#[test]
fn panel_classic_menus_leave_no_popup_open() {
    for size in SIZES {
        let mut rig = rig(LayoutKind::PanelClassic, size);
        press(&mut rig, "File");
        hover(&mut rig, "New");
        assert!(
            shows(&mut rig, "New Drawing"),
            "{size:?}: the submenu opens on hover"
        );
        leave(&mut rig);
        assert!(
            !shows(&mut rig, "New Drawing"),
            "{size:?}: the submenu closes once the pointer has left"
        );
        assert!(
            shows(&mut rig, "Open..."),
            "{size:?}: the File menu stays open"
        );
        click_outside(&mut rig);
        assert!(
            !shows(&mut rig, "Open..."),
            "{size:?}: a click outside closes the menu"
        );
        assert!(!any_popup(&mut rig), "{size:?}: no popup is left open");
    }
}

#[test]
fn panel_modern_menus_leave_no_popup_open() {
    // No committed Panel Modern submenu has a runnable entry yet (their
    // children are stubs), so the shell-level sequence opens the Paste
    // menu; the submenu leave rule is pinned in `panel_modern`'s unit
    // tests with a fixture submenu.
    for size in SIZES {
        let mut rig = rig(LayoutKind::PanelModern, size);
        rig.run();
        let arrow = rig
            .harness()
            .query_all_by_label("More items")
            .map(|node| node.rect())
            .find(|rect| rect.min.y > 60.0)
            .expect("the Paste split's arrow");
        crate::review::click_at(&mut rig, arrow.center(), egui::PointerButton::Primary);
        rig.harness().run_steps(2);
        assert!(
            shows(&mut rig, "Paste Special..."),
            "{size:?}: the Paste menu opens"
        );
        leave(&mut rig);
        assert!(
            shows(&mut rig, "Paste Special..."),
            "{size:?}: a top-level menu stays open when the pointer leaves"
        );
        click_outside(&mut rig);
        assert!(
            !shows(&mut rig, "Paste Special..."),
            "{size:?}: a click outside closes the menu"
        );
        assert!(!any_popup(&mut rig), "{size:?}: no popup is left open");
    }
}

#[test]
fn a_collapsed_group_opens_its_controls_and_closes_on_a_click_outside() {
    // At 1024 pt the Home tab collapses its Font group (task P7-17).
    let mut rig = rig(LayoutKind::PanelModern, (1024.0, 768.0));
    assert!(!shows(&mut rig, "Bold"), "the Font group is collapsed");
    rig.click_named("Font");
    rig.harness().run_steps(3);
    assert!(
        shows(&mut rig, "Bold"),
        "the collapsed group opens its controls"
    );
    click_outside(&mut rig);
    assert!(!shows(&mut rig, "Bold"), "a click outside closes it");
    assert!(!any_popup(&mut rig));
}

#[test]
fn new_in_the_file_view_creates_a_document_through_file_new() {
    let mut rig = rig(LayoutKind::PanelModern, (1440.0, 900.0));
    let before = rig.shell.borrow().app.documents.len();
    rig.click_named("File");
    rig.run();
    rig.click_named("New");
    rig.run();
    assert!(
        !shows(&mut rig, "(none)"),
        "the New page draws no empty template tile"
    );
    rig.click_named("Blank Drawing");
    rig.harness().run_steps(3);
    let shell = rig.shell.borrow();
    assert_eq!(
        shell.app.documents.len(),
        before + 1,
        "a new document opened"
    );
    let active = shell
        .app
        .active_document()
        .expect("the new document is active");
    assert!(active.path.is_none(), "the new drawing is unsaved");
    drop(shell);
    assert!(
        shows(&mut rig, "Home"),
        "the File view closed on the new drawing"
    );
}

#[test]
fn new_on_both_layouts_runs_the_same_command() {
    // Panel Modern's Blank Drawing and Panel Classic's File > New >
    // New Drawing from Template... both name `file.new`, the one
    // implemented New command (task P7-17).
    let classic = crate::sets().get(LayoutKind::PanelClassic);
    let file = classic
        .v2
        .menu_bars
        .first()
        .and_then(|bar| bar.menus.iter().find(|menu| menu.id == "drawing.file"))
        .expect("the File menu");
    let new = file
        .items
        .iter()
        .find(|item| item.id == "drawing.file.new")
        .expect("File > New");
    let commands: Vec<String> = new
        .items
        .iter()
        .filter_map(|item| item.command.as_ref().map(|id| id.as_str().to_owned()))
        .collect();
    assert!(commands.iter().any(|id| id == "file.new"), "{commands:?}");
    let registry = crate::builtin_registry();
    let app = larrygram_commands::app::AppCtx::new(crate::seen_settings());
    let id = larrygram_commands::CommandId::parse("file.new").expect("id");
    assert!(
        registry.enabled(&id, &app).value,
        "file.new is implemented and enabled"
    );
}

/// A right-click on the page tab draws its menu at the pointer, in
/// both layouts (task P7-17: the click that opened a shell context
/// menu also closed it, and the menu sat at the window's corner).
#[test]
fn a_right_click_on_the_page_tab_shows_its_menu() {
    for (kind, entry) in [
        (LayoutKind::PanelClassic, "Rename Page"),
        (LayoutKind::PanelModern, "Rename"),
    ] {
        let mut rig = rig(kind, (1440.0, 900.0));
        let pos = rig
            .harness()
            .query_all_by_label("Page-1")
            .next()
            .expect("the page tab")
            .rect()
            .center();
        crate::review::click_at(&mut rig, pos, egui::PointerButton::Secondary);
        rig.harness().run_steps(2);
        assert!(shows(&mut rig, entry), "{kind:?}: the page tab menu draws");
        let row = rig
            .harness()
            .query_all_by_label(entry)
            .next()
            .expect("the menu row")
            .rect();
        assert!(
            (row.min - pos).length() < 300.0,
            "{kind:?}: the menu opens at the pointer ({row:?}, click at {pos:?})"
        );
        click_outside(&mut rig);
        assert!(
            !shows(&mut rig, entry),
            "{kind:?}: a click outside closes it"
        );
    }
}
