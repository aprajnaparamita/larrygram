//! Unsaved changes (task P7-42): the changed marker, the three-way
//! close dialog and the quit walk, driven through real input in the
//! full shell in both layouts (the master note) — canvas drags that
//! edit the shape, the File menu and the backstage, the dialog through
//! accesskit, the keyboard chord and the window close request. A test
//! that dispatches the command does not count for the UI items here.
//!
//! - goal 2: closing a changed drawing from the File menu or the
//!   backstage shows the dialog; Cancel keeps it open and changed,
//!   Don't Save closes it, Save writes the file (a temp path through
//!   the test file picker) and closes;
//! - goal 3: the same through Quit with two changed drawings,
//!   including Cancel on the second, and the window close request is
//!   cancelled and shows the dialog;
//! - goal 4: the title shows the marker after an edit and loses it
//!   after a save.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_commands::app::DocumentSession;
use larrygram_ui::UiError;
use larrygram_ui::dialogs::{FileChoice, FilePicker, OpenRequest, PickState, SaveRequest};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// The second drawing's name (a path on its session, so the two
/// dialogs are told apart).
const SECOND: &str = "second.vsdx";

/// The dialog's own labels (task P7-42: short command labels; the
/// prompt text is ours).
const SAVE: &str = "Save";
const DONT_SAVE: &str = "Don't Save";
const CANCEL: &str = "Cancel";

// ---------------------------------------------------------------------------
// Rig and input helpers (the `text_controls` rig pattern).
// ---------------------------------------------------------------------------

/// A shell in `kind` with the rich state (the fixture rectangle),
/// ready to edit: the rectangle centred in the canvas viewport.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    let mut rig = ShellRig::with_app(app);
    settle(&mut rig);
    rig
}

/// A shell in `kind` with **two** changed drawings on the way: the
/// fixture drawing (untitled) and a copy of it under `SECOND`. Both
/// views centre the fixture rectangle, so the same drag edits each.
fn two_drawing_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    let second = app
        .active_document()
        .expect("the fixture drawing")
        .document
        .clone();
    app.open_document(DocumentSession::new(
        second,
        Some(PathBuf::from("/tmp/second.vsdx")),
    ));
    app.active = Some(0);
    let mut rig = ShellRig::with_app(app);
    settle(&mut rig);
    rig
}

/// Runs the rig settled with the fixture rectangle mid-canvas (the
/// `interaction` rig's pan rule).
fn settle(rig: &mut ShellRig) {
    rig.run();
    let viewport = rig
        .shell
        .borrow()
        .canvas_viewport()
        .expect("the canvas drew");
    let pan = larrygram_geom::Point::new(
        f64::from(viewport.width()) / 2.0 / 96.0 - SHAPE.0,
        f64::from(viewport.height()) / 2.0 / 96.0 - (11.0 - SHAPE.1),
    );
    {
        let mut shell = rig.shell.borrow_mut();
        for session in &mut shell.app.documents {
            session.view.zoom = 1.0;
            session.view.pan = pan;
        }
    }
    rig.run();
    rig.run();
}

/// Edits the active drawing with real input: a pointer drag moves the
/// fixture rectangle half an inch (one undo step, and the change
/// counter moves).
fn edit_shape(rig: &mut ShellRig) {
    let from = screen(rig, SHAPE.0, SHAPE.1);
    let to = screen(rig, SHAPE.0 + 0.5, SHAPE.1);
    drag(rig, from, to);
    assert!(changed(rig), "the drag edited the drawing");
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A drag from `from` to `to` (press, moves over frames, release).
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

/// Clicks the first widget with `role` and `name`.
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

/// Clicks the first widget named `name`, whatever its role.
fn click_named(rig: &mut ShellRig, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"));
    node.click();
    rig.run();
}

/// Presses one chord of Cmd+key (the `Ctrl` token of the shortcut
/// table is the command key on macOS; the `text_controls` chord rule).
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

/// Whether the active drawing reads as changed (task P7-42's derived
/// rule).
fn changed(rig: &ShellRig) -> bool {
    rig.shell
        .borrow()
        .app
        .active_document()
        .expect("a document")
        .is_dirty()
}

/// Closes the active drawing the way a user does: the File menu in
/// Panel Classic, the File view (backstage) in Panel Modern.
fn close_drawing(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Close");
        }
        _ => {
            click_named(rig, "File");
            click(rig, Role::Button, "Close");
        }
    }
}

/// Saves the active drawing the way a user does: the File menu in
/// Panel Classic, the File view in Panel Modern.
fn save_drawing(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Save");
        }
        _ => {
            click_named(rig, "File");
            click(rig, Role::Button, "Save");
        }
    }
}

/// Quits the way a user does: the File menu's Exit in Panel Classic,
/// the Cmd+Q (Ctrl+Q off macOS) chord in Panel Modern (whose File view
/// has no Exit row).
fn quit(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Exit");
        }
        _ => chord(rig, egui::Key::Q),
    }
}

/// Whether the three-way dialog is up and names `name`.
fn dialog_names(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.names()
        .contains_key(&format!("Unsaved changes in {name}."))
}

/// The rect of the unsaved-changes dialog's window.
fn dialog_rect(rig: &mut ShellRig) -> egui::Rect {
    rig.run();
    rig.harness()
        .ctx
        .memory(|memory| {
            memory.area_rect(egui::Id::unique(larrygram_ui::window_size::UNSAVED_WINDOW))
        })
        .expect("the dialog window")
}

/// Whether the dialog draws a button named `name` (scoped to its
/// window: the toolbars carry buttons with the same names).
fn dialog_has(rig: &mut ShellRig, name: &str) -> bool {
    let rect = dialog_rect(rig);
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .any(|node| rect.contains_rect(node.rect()))
}

/// Clicks the dialog's button named `name` (scoped to its window).
fn dialog_click(rig: &mut ShellRig, name: &str) {
    dialog_press(rig, name);
    rig.run();
}

/// Like [`dialog_click`], but no frame runs after the click: the
/// caller watches the frames that answer it (the quit walk).
fn dialog_press(rig: &mut ShellRig, name: &str) {
    let rect = dialog_rect(rig);
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(Role::Button, name)
        .find(|node| rect.contains_rect(node.rect()))
        .unwrap_or_else(|| panic!("no dialog button named {name:?}"));
    node.click();
}

/// The test file picker: every save asks for a temp path and gets it
/// (task P7-42's "a temp path through the test file picker"), every
/// open is cancelled. It counts the saves it answered.
#[derive(Clone)]
struct TempPicker {
    /// The path every save picks.
    path: PathBuf,
    /// How many saves were asked for.
    saves: Rc<RefCell<usize>>,
}

impl TempPicker {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            saves: Rc::new(RefCell::new(0)),
        }
    }
}

impl FilePicker for TempPicker {
    fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, UiError> {
        Ok(None)
    }

    fn save(&self, _request: &SaveRequest, _data: &[u8]) -> Result<Option<FileChoice>, UiError> {
        *self.saves.borrow_mut() += 1;
        Ok(Some(FileChoice::Path(self.path.clone())))
    }

    fn poll(&self) -> PickState {
        PickState::Idle
    }
}

/// A private temp file path for one test's save.
fn temp_target(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "larrygram-unsaved-{}-{name}.vsdx",
        std::process::id()
    ))
}

/// The viewport commands of the last frame (egui_kittest keeps the
/// frame's [`egui::ViewportOutput`], commands included).
fn sent(rig: &mut ShellRig, match_: fn(&egui::ViewportCommand) -> bool) -> bool {
    rig.harness()
        .output()
        .viewport_output
        .values()
        .any(|output| output.commands.iter().any(match_))
}

/// Whether the last frame sent a window title.
fn sent_title(rig: &mut ShellRig) -> Option<String> {
    let output = rig.harness().output();
    output.viewport_output.values().find_map(|output| {
        output.commands.iter().find_map(|command| match command {
            egui::ViewportCommand::Title(title) => Some(title.clone()),
            _ => None,
        })
    })
}

/// Steps up to ten frames and answers whether the quit completed: the
/// shell's quit flag set in a frame (it is consumed at the top of the
/// next one) or the window close handed to the viewport.
fn quit_completed(rig: &mut ShellRig) -> bool {
    for _ in 0..10 {
        rig.harness().step();
        if rig.shell.borrow().quit_requested() {
            return true;
        }
        if sent(rig, |command| {
            matches!(command, egui::ViewportCommand::Close)
        }) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Goal 2: close asks.
// ---------------------------------------------------------------------------

/// Cancel keeps the drawing open and changed (task P7-42 goal 2).
#[test]
fn closing_a_changed_drawing_asks_and_cancel_keeps_it() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_shape(&mut rig);
        close_drawing(&mut rig, kind);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the close shows the dialog"
        );
        assert!(dialog_has(&mut rig, SAVE), "{kind:?}: the Save answer");
        assert!(
            dialog_has(&mut rig, DONT_SAVE),
            "{kind:?}: the Don't Save answer"
        );
        assert!(dialog_has(&mut rig, CANCEL), "{kind:?}: the Cancel answer");
        dialog_click(&mut rig, CANCEL);
        assert!(
            rig.shell.borrow().app.active_document().is_some(),
            "{kind:?}: Cancel keeps the drawing open"
        );
        assert!(changed(&rig), "{kind:?}: Cancel keeps it changed");
        assert!(
            !dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the dialog closed"
        );
    }
}

/// Don't Save closes the drawing (task P7-42 goal 2).
#[test]
fn dont_save_closes_a_changed_drawing() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_shape(&mut rig);
        close_drawing(&mut rig, kind);
        assert!(dialog_names(&mut rig, "Untitled"), "{kind:?}: it asks");
        dialog_click(&mut rig, DONT_SAVE);
        assert!(
            rig.shell.borrow().app.documents.is_empty(),
            "{kind:?}: Don't Save closes the drawing"
        );
    }
}

/// Save writes the file (a temp path through the test file picker)
/// and closes (task P7-42 goal 2).
#[test]
fn save_writes_the_file_and_closes() {
    for kind in LAYOUTS {
        let target = temp_target("save");
        let picker = TempPicker::new(target.clone());
        let saves = picker.saves.clone();
        let mut rig = shell_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        edit_shape(&mut rig);
        close_drawing(&mut rig, kind);
        assert!(dialog_names(&mut rig, "Untitled"), "{kind:?}: it asks");
        dialog_click(&mut rig, SAVE);
        assert!(
            rig.shell.borrow().app.documents.is_empty(),
            "{kind:?}: Save closes the drawing"
        );
        assert!(target.is_file(), "{kind:?}: the save wrote the file");
        assert_eq!(*saves.borrow(), 1, "{kind:?}: one pick for the target");
        let _ = std::fs::remove_file(&target);
    }
}

/// A drawing with nothing to save closes without a dialog (the walk
/// only asks about changed drawings).
#[test]
fn an_unchanged_drawing_closes_without_a_dialog() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        close_drawing(&mut rig, kind);
        assert!(
            rig.shell.borrow().app.documents.is_empty(),
            "{kind:?}: an unchanged drawing closes at once"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 3: quit asks, one dialog per changed drawing.
// ---------------------------------------------------------------------------

/// Quit with two changed drawings: the same dialog on each, and
/// Cancel on the second stops the quit with it open and changed (task
/// P7-42 goal 3).
#[test]
fn quit_asks_about_each_changed_drawing_and_cancel_stops_it() {
    for kind in LAYOUTS {
        let mut rig = two_drawing_rig(kind);
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(1);
        }
        rig.run();
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(0);
        }
        rig.run();

        quit(&mut rig, kind);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the first drawing is asked about"
        );
        dialog_click(&mut rig, DONT_SAVE);
        assert!(
            dialog_names(&mut rig, SECOND),
            "{kind:?}: the second drawing is asked about in turn"
        );
        dialog_click(&mut rig, CANCEL);
        assert!(
            !rig.shell.borrow().quit_requested(),
            "{kind:?}: Cancel stops the quit"
        );
        let shell = rig.shell.borrow();
        assert_eq!(
            shell.app.documents.len(),
            1,
            "{kind:?}: the answered drawing closed, the cancelled one stays"
        );
        let session = shell.app.active_document().expect("the second drawing");
        assert_eq!(session.display_name(), SECOND, "{kind:?}");
        assert!(session.is_dirty(), "{kind:?}: still changed");
    }
}

/// Quit with two changed drawings, both Don't Save: the walk
/// completes and the window close goes to the viewport (task P7-42
/// goal 3).
#[test]
fn quit_completes_when_every_changed_drawing_is_answered() {
    for kind in LAYOUTS {
        let mut rig = two_drawing_rig(kind);
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(1);
        }
        rig.run();
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(0);
        }
        rig.run();

        quit(&mut rig, kind);
        assert!(dialog_names(&mut rig, "Untitled"), "{kind:?}: it asks");
        dialog_click(&mut rig, DONT_SAVE);
        assert!(dialog_names(&mut rig, SECOND), "{kind:?}: in turn");
        // The last answer is pressed without a settling run: the
        // frames that follow carry the walk's completion and the
        // viewport close, and `quit_completed` watches each one.
        dialog_press(&mut rig, DONT_SAVE);
        assert!(
            quit_completed(&mut rig),
            "{kind:?}: the quit completes when the last drawing is resolved"
        );
    }
}

/// The window close request with a changed drawing is cancelled and
/// shows the dialog (task P7-42 goal 3: `ViewportCommand::CancelClose`
/// on `close_requested`).
#[test]
fn the_window_close_request_is_cancelled_and_shows_the_dialog() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_shape(&mut rig);
        {
            let harness = rig.harness();
            let viewports = harness
                .input_mut()
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default();
            viewports.events.push(egui::ViewportEvent::Close);
        }
        rig.harness().step();
        assert!(
            sent(&mut rig, |command| {
                matches!(command, egui::ViewportCommand::CancelClose)
            }),
            "{kind:?}: the close request is cancelled"
        );
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the dialog shows"
        );
        assert!(
            rig.shell.borrow().app.active_document().is_some(),
            "{kind:?}: nothing closed"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 4: the title marks a changed drawing.
// ---------------------------------------------------------------------------

/// The title shows the marker after an edit and loses it after a
/// save (task P7-42 goal 4), in both layouts' title bars and the OS
/// window title.
#[test]
fn the_title_shows_the_marker_and_loses_it_after_a_save() {
    for kind in LAYOUTS {
        let target = temp_target("title");
        let picker = TempPicker::new(target.clone());
        let mut rig = shell_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        assert!(
            rig.names().contains_key("Untitled"),
            "{kind:?}: the clean title names the drawing"
        );
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: the clean window title"
        );

        edit_shape(&mut rig);
        assert!(
            rig.names().contains_key("• Untitled"),
            "{kind:?}: the title marks the changed drawing"
        );
        assert_eq!(
            rig.shell.borrow().window_title(),
            "• Untitled - Larrygram",
            "{kind:?}: the window title carries the same mark"
        );

        save_drawing(&mut rig, kind);
        assert!(
            !rig.names().contains_key("• Untitled"),
            "{kind:?}: the save loses the mark"
        );
        assert!(rig.names().contains_key("Untitled"), "{kind:?}: the name");
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: the window title loses the mark"
        );
        let _ = std::fs::remove_file(&target);
    }
}

/// The window-title sync sends one title command per change (task
/// P7-42 goal 4's "keep one place for it"): the marked name comes
/// from `title_text` alone and reaches the viewport once per change.
#[test]
fn the_window_title_follows_the_marker_with_one_command_per_change() {
    let mut rig = shell_rig(LayoutKind::PanelModern);
    // One frame after the state changes carries the title; the next
    // one repeats nothing.
    {
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("a document");
        session.changes = session.changes.wrapping_add(1);
    }
    rig.harness().step();
    assert_eq!(
        sent_title(&mut rig).as_deref(),
        Some("• Untitled - Larrygram"),
        "the change carries the marked title"
    );
    rig.harness().step();
    assert_eq!(
        sent_title(&mut rig),
        None,
        "an unchanged frame repeats nothing"
    );
}

// ---------------------------------------------------------------------------
// Task P7-46 goal 7: the review's missing flows.
// ---------------------------------------------------------------------------

/// A picker whose every save is cancelled (task P7-46 goal 7: the
/// walk stops and the drawing stays open and changed).
struct CancelledPicker;

impl FilePicker for CancelledPicker {
    fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, UiError> {
        Ok(None)
    }

    fn save(&self, _request: &SaveRequest, _data: &[u8]) -> Result<Option<FileChoice>, UiError> {
        Ok(None)
    }

    fn poll(&self) -> PickState {
        PickState::Idle
    }
}

/// Asks for the window to close the way the close button does (task
/// P7-46 goal 7): one `close_requested` event and one frame.
fn request_window_close(rig: &mut ShellRig) {
    {
        let harness = rig.harness();
        let viewports = harness
            .input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        viewports.events.push(egui::ViewportEvent::Close);
    }
    rig.harness().step();
}

/// Save inside a quit walk: the drawing is written (a temp path
/// through the test file picker) and the walk goes on to the next
/// drawing in turn (task P7-46 goal 7).
#[test]
fn save_inside_a_quit_walk_writes_the_drawing_and_the_walk_goes_on() {
    for kind in LAYOUTS {
        let target = temp_target("walk-save");
        let picker = TempPicker::new(target.clone());
        let mut rig = two_drawing_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(1);
        }
        rig.run();
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(0);
        }
        rig.run();

        quit(&mut rig, kind);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the first drawing is asked about"
        );
        dialog_click(&mut rig, SAVE);
        assert!(target.is_file(), "{kind:?}: the drawing was written");
        assert!(
            dialog_names(&mut rig, SECOND),
            "{kind:?}: the walk goes on to the next drawing"
        );
        let _ = std::fs::remove_file(&target);
    }
}

/// A cancelled picker on Save: the walk stops and the drawing stays
/// open and changed (task P7-46 goal 7).
#[test]
fn a_cancelled_save_stops_the_walk_and_keeps_the_drawing() {
    for kind in LAYOUTS {
        let mut rig = two_drawing_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(CancelledPicker));
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(1);
        }
        rig.run();
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(0);
        }
        rig.run();

        quit(&mut rig, kind);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the first drawing is asked about"
        );
        dialog_click(&mut rig, SAVE);
        assert!(
            !dialog_names(&mut rig, SECOND),
            "{kind:?}: the walk stopped at the cancelled save"
        );
        assert!(
            !rig.shell.borrow().quit_requested(),
            "{kind:?}: the quit stops with the walk"
        );
        let shell = rig.shell.borrow();
        assert_eq!(
            shell.app.documents.len(),
            2,
            "{kind:?}: both drawings are still open"
        );
        let session = shell.app.active_document().expect("the first drawing");
        assert!(session.is_dirty(), "{kind:?}: still changed");
    }
}

/// The close button with no changed drawing: no `CancelClose` and the
/// window closes (task P7-46 goal 7: a logout-style clean exit is
/// never interrupted).
#[test]
fn the_close_button_with_no_changed_drawing_closes_at_once() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        request_window_close(&mut rig);
        assert!(
            !sent(&mut rig, |command| {
                matches!(command, egui::ViewportCommand::CancelClose)
            }),
            "{kind:?}: nothing to ask about, nothing cancelled"
        );
        assert!(
            sent(&mut rig, |command| {
                matches!(command, egui::ViewportCommand::Close)
            }),
            "{kind:?}: the window closes"
        );
    }
}

/// The close button path runs to the end after the user answers (task
/// P7-46 goal 7): Don't Save on the dialog resolves the last changed
/// drawing and the window close goes to the viewport.
#[test]
fn the_close_button_path_finishes_after_the_answer() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_shape(&mut rig);
        request_window_close(&mut rig);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the close button shows the dialog"
        );
        dialog_press(&mut rig, DONT_SAVE);
        assert!(
            quit_completed(&mut rig),
            "{kind:?}: the walk finishes and the window closes"
        );
    }
}

/// A second quit request while a Close dialog is up keeps the quit
/// (task P7-46 goal 2, its UI half): the walk asks about the other
/// changed drawing in turn instead of stopping at the first.
#[test]
fn a_quit_during_a_close_walk_keeps_the_quit() {
    for kind in LAYOUTS {
        let mut rig = two_drawing_rig(kind);
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(1);
        }
        rig.run();
        edit_shape(&mut rig);
        {
            let mut shell = rig.shell.borrow_mut();
            shell.app.active = Some(0);
        }
        rig.run();

        // Close the first drawing: its Close dialog is up.
        close_drawing(&mut rig, kind);
        assert!(
            dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the close dialog is up"
        );
        // A quit request while it runs: the walk keeps the quit and
        // asks about the other changed drawing after this one.
        quit(&mut rig, kind);
        dialog_click(&mut rig, DONT_SAVE);
        assert!(
            dialog_names(&mut rig, SECOND),
            "{kind:?}: the quit walk goes on to the second drawing"
        );
        dialog_click(&mut rig, CANCEL);
        assert!(
            !rig.shell.borrow().quit_requested(),
            "{kind:?}: Cancel still stops the quit"
        );
        let shell = rig.shell.borrow();
        assert_eq!(
            shell.app.documents.len(),
            1,
            "{kind:?}: the answered drawing closed, the cancelled one stays"
        );
        let session = shell.app.active_document().expect("the second drawing");
        assert_eq!(session.display_name(), SECOND, "{kind:?}");
        assert!(session.is_dirty(), "{kind:?}: still changed");
    }
}
