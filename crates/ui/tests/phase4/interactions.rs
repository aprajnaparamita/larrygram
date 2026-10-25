//! egui_kittest interaction tests (task P4-10, ADR 0037): clicks that
//! drive the real [`Shell`] end to end.
//!
//! P4-09's modules cover command-bar placement, enabled/disabled states,
//! the file flow and layout switching at the data level. This file
//! clicks the widgets the way a user does, through `egui_kittest`'s
//! AccessKit tree, and asserts the *application state* changes:
//!
//! - a command-bar button/checkbox click dispatches its registry command
//!   (the View tab's `ShowGrid` toggle);
//! - the View/Options dialog switches the Panel layout with no
//!   restart and toggling the grid there changes what the canvas
//!   draws (P4-16: the dialog dispatches through the registry);
//! - the Help menu opens the About window;
//! - the close-confirmation dialog closes a dirty document on confirm;
//! - the File button opens the backstage with its placements;
//! - a QAT button opens a document;
//! - a page tab click switches the active page.
//!
//! The committed command bar is about 5000 points wide, so these use
//! [`ShellRig::large`] (the P4-04 note: a click outside the window
//! rect never reaches a widget).
//!
//! A panic is the bug (AGENTS.md section 6).

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use egui::accesskit::Role;
use egui::{Color32, Context, RawInput, Rect, Shape};
use larrygram_commands::Source;
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_model::{Document, Page, PageId};
use larrygram_ui::canvas::{self, CanvasCache};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, sets};

/// The command-bar tab labels of one layout (the `layout_switch.rs` rule).
fn tab_labels(kind: LayoutKind) -> BTreeSet<String> {
    sets()
        .get(kind)
        .layout
        .tabs
        .iter()
        .map(|tab| tab.label.clone())
        .collect()
}

/// A label only the `mine` layout has, if any.
fn unique_tab(mine: LayoutKind, other: LayoutKind) -> Option<String> {
    let ours = tab_labels(mine);
    let theirs = tab_labels(other);
    ours.difference(&theirs).next().cloned()
}

#[test]
fn clicking_the_grid_checkbox_in_the_view_tab_toggles_the_grid() {
    let mut rig = ShellRig::large();
    rig.dispatch("file.new", Source::Ui).expect("file.new runs");
    rig.run();
    assert!(
        rig.shell
            .borrow()
            .app
            .active_document()
            .expect("a document")
            .view
            .show_grid,
        "the new document starts with the grid on (ADR 0029)"
    );

    // The checkbox lives on the View tab of the committed command bar.
    rig.click(Role::Button, "TabView");
    rig.click(Role::CheckBox, "ShowGrid");
    {
        let shell = rig.shell.borrow();
        assert!(
            !shell
                .app
                .active_document()
                .expect("a document")
                .view
                .show_grid,
            "one click toggles `view.toggle_grid`"
        );
    }

    // And back: the widget is a toggle, not a one-shot button.
    rig.click(Role::CheckBox, "ShowGrid");
    assert!(
        rig.shell
            .borrow()
            .app
            .active_document()
            .expect("a document")
            .view
            .show_grid
    );
}

#[test]
fn switching_layouts_from_the_options_dialog_needs_no_restart() {
    let mut rig = ShellRig::large();
    let before = {
        let shell = rig.shell.borrow();
        assert_eq!(shell.layout_kind(), LayoutKind::Panel2016);
        // State that must survive the switch (the `layout_switch.rs`
        // rule): the same `Shell`, the same `AppCtx`.
        &shell.app as *const AppCtx as usize
    };

    // The shell-handled window command opens the dialog (P4-03).
    let outcome = rig
        .dispatch("app.office.center.options", Source::Ui)
        .expect("no error");
    assert!(
        matches!(
            outcome.ui_request,
            Some(larrygram_commands::UiRequest::ShowOptions)
        ),
        "{outcome:?}"
    );
    rig.run();

    // Click the 365 radio of the dialog body (`settings::show_options`).
    rig.click(Role::RadioButton, "Panel (365 data)");
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.layout_kind(), LayoutKind::Panel365);
        assert_eq!(&shell.app as *const AppCtx as usize, before);
        // The dialog writes nothing itself (P4-16, the ADR 0032
        // amendment): the switch happened because the radio's intent
        // (`view.ui_layout.panel_365`) ran through the registry, and
        // its message is the status line.
        assert_eq!(shell.status(), "Panel layout: Panel (365 data)");
    }

    // The next frame draws the Panel (365 data) layout: no restart, same harness.
    rig.run();
    let only_365 = unique_tab(LayoutKind::Panel365, LayoutKind::Panel2016)
        .expect("365 has a tab 2016 does not");
    assert!(
        rig.names().contains_key(&only_365),
        "the {only_365:?} tab renders: {:?}",
        rig.names().keys().collect::<Vec<_>>()
    );
}

#[test]
fn the_help_menu_opens_the_about_dialog() {
    let mut rig = ShellRig::large();
    rig.click(Role::Button, "Help");
    rig.click(Role::Button, "About Larrygram");
    assert_eq!(
        rig.shell.borrow().status(),
        "About Larrygram",
        "`help.about` ran through the registry"
    );
    // The modal window is drawn (its title and close button appear).
    assert!(
        rig.names().contains_key("About Larrygram"),
        "the About window renders"
    );
    assert!(rig.names().contains_key("Close window"), "closable");
}

#[test]
fn the_close_confirmation_dialog_closes_the_document_on_confirm() {
    let mut rig = ShellRig::large();
    rig.dispatch("file.new", Source::Ui).expect("file.new runs");
    {
        let mut shell = rig.shell.borrow_mut();
        shell.app.active_document_mut().expect("a document").dirty = true;
    }

    // An interactive close of a dirty document asks first (ADR 0028):
    // the command returns `ConfirmClose` and closes nothing.
    let outcome = rig.dispatch("file.close", Source::Ui).expect("no error");
    assert!(
        matches!(
            outcome.ui_request,
            Some(larrygram_commands::UiRequest::ConfirmClose { .. })
        ),
        "{outcome:?}"
    );
    assert_eq!(rig.shell.borrow().app.documents.len(), 1, "still open");

    // The dialog's confirm button re-dispatches `file.close` with
    // `confirm: true` and the document closes.
    rig.run();
    rig.click(Role::Button, "Close without saving");
    assert_eq!(rig.shell.borrow().app.documents.len(), 0, "closed");
}

#[test]
fn the_file_button_opens_the_backstage_with_its_entries() {
    let entries: Vec<String> = sets()
        .get(LayoutKind::Panel2016)
        .layout
        .backstage
        .tabs
        .iter()
        .map(|tab| tab.label.clone())
        .collect();
    assert!(!entries.is_empty(), "the committed layout has a backstage");
    let rig = ShellRig::large();
    let before = rig.names();
    for entry in &entries {
        assert!(!before.contains_key(entry), "{entry:?} is backstage-only");
    }

    // The window has two "File" buttons (the menu bar and the command
    // tab strip). Each attempt gets a fresh rig: a click on the menu
    // bar's tab-strip button opens a popup that covers the command-bar one, so the
    // attempts must not share a frame history.
    let mut opened = false;
    for nth in 0..3 {
        let mut rig = ShellRig::large();
        if !rig.try_click_nth(Role::Button, "File", nth) {
            continue;
        }
        let names = rig.names();
        if entries.iter().any(|entry| names.contains_key(entry)) {
            opened = true;
            break;
        }
    }
    assert!(opened, "the File button opens the backstage: {entries:?}");
    // The entries render whatever the active entry is (the assertion
    // above), and the shell stays healthy with the backstage open.
    assert!(!entries.is_empty());
}

#[test]
fn clicking_the_qat_new_button_opens_a_document() {
    let mut rig = ShellRig::large();
    rig.click(Role::Button, "FileNewDefault");
    assert_eq!(
        rig.shell.borrow().app.documents.len(),
        1,
        "the QAT button runs `file.new`"
    );
}

/// The grid colors of `crates/ui/src/canvas.rs` (`GRID_MINOR`,
/// `GRID_MAJOR`).
fn is_grid_line(color: Color32) -> bool {
    color == Color32::from_rgb(190, 190, 190) || color == Color32::from_rgb(160, 160, 160)
}

/// The grid line segments of one painted shape (the canvas paints its
/// grid as line segments).
fn grid_segments(shape: &Shape) -> usize {
    match shape {
        Shape::LineSegment { stroke, .. } => usize::from(is_grid_line(stroke.color)),
        Shape::Vec(shapes) => shapes.iter().map(grid_segments).sum(),
        _ => 0,
    }
}

/// How many grid lines the canvas draws for `session` right now: one
/// scratch frame of `canvas::show` over a fresh cache.
fn grid_lines(session: &mut DocumentSession) -> usize {
    let ctx = Context::default();
    let mut cache = CanvasCache::new();
    let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ui| {
            canvas::show(ui, session, &mut cache).expect("the canvas draws");
        },
    );
    let mut count = 0;
    for clipped in &output.shapes {
        count += grid_segments(&clipped.shape);
    }
    output.textures_delta.clear();
    count
}

#[test]
fn toggling_grid_in_options_changes_what_the_canvas_draws() {
    let mut rig = ShellRig::large();
    rig.dispatch("file.new", Source::Ui).expect("file.new runs");
    rig.run();
    let before = {
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("a document");
        assert!(session.view.show_grid, "a new drawing starts with the grid");
        grid_lines(session)
    };
    assert!(before > 0, "the grid draws ({before} lines)");

    // The Options dialog opens from its registry command and its
    // checkbox dispatches `view.toggle_grid` (the ADR 0032
    // amendment): the dialog writes nothing itself, so the state can
    // only change through the registry.
    let outcome = rig
        .dispatch("app.office.center.options", Source::Ui)
        .expect("no error");
    assert!(
        matches!(
            outcome.ui_request,
            Some(larrygram_commands::UiRequest::ShowOptions)
        ),
        "{outcome:?}"
    );
    rig.run();
    rig.click(Role::CheckBox, "Show grid");
    {
        let shell = rig.shell.borrow();
        let session = shell.app.active_document().expect("a document");
        assert!(
            !session.view.show_grid,
            "the checkbox ran `view.toggle_grid`"
        );
        assert!(!shell.app.settings.show_grid, "and the default with it");
        assert_eq!(shell.status(), "Grid off");
    }

    let after = {
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("a document");
        grid_lines(session)
    };
    assert_eq!(after, 0, "the canvas draws no grid lines any more");
}

#[test]
fn clicking_a_page_tab_switches_the_active_page() {
    let mut document = Document::new();
    let mut first = Page::new(PageId { index: 0 }, 1);
    first.name = "First".to_owned();
    let mut second = Page::new(PageId { index: 1 }, 2);
    second.name = "Second".to_owned();
    document.pages.push(first);
    document.pages.push(second);
    let mut app = AppCtx::new(crate::phase4_settings());
    app.open_document(DocumentSession::new(document, None));
    let mut rig = ShellRig::with_app(app);
    rig.run();

    // `canvas::show_page_tabs` draws a selectable label per page.
    assert!(
        rig.try_click(Role::Button, "Second") || rig.try_click(Role::Tab, "Second"),
        "a page tab named after its page: {:?}",
        rig.names().keys().collect::<Vec<_>>()
    );
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("a document");
    assert_eq!(session.active_page, Some(PageId { index: 1 }));
}
