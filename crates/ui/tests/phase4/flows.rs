//! Shell flow edges (task P4-10, ADR 0028/0036).
//!
//! `tests/phase4/file_flow.rs` (P4-09) covers the synchronous picker
//! flow; this file hunts the edges around it:
//!
//! - an asynchronous pick (the wasm upload) completing across frames
//!   through [`Shell::show_in`]'s `finish_pick`;
//! - a cancelled asynchronous pick clearing the pending command;
//! - the window commands (`app.quit`, `app.office.center.options`)
//!   from the programmatic side: [`dispatch_one`] returns their
//!   [`UiRequest`](larrygram_commands::UiRequest) with no side effect
//!   (P4-15 implements them; P4-16 removed the shell intercepts);
//! - quitting: the flag the shell resets when it hands the close to
//!   the viewport.
//!
//! A panic is the bug (AGENTS.md section 6).

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::Cell;
use std::path::{Path, PathBuf};

use larrygram_commands::app::AppCtx;
use larrygram_commands::{CommandId, Invocation, Source, UiRequest};
use larrygram_ui::dialogs::{FileChoice, FilePicker, OpenRequest, PickState, SaveRequest};
use larrygram_ui::shell::dispatch_one;

use crate::ShellRig;

/// A committed hand-authored fixture (our own work, ADR 0005).
fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("fixtures")
        .join("minimal-drawing.vsdx")
}

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture_path()).expect("the fixture file is committed")
}

/// A picker whose `open` never answers and whose `poll` walks a
/// script: the wasm upload shape (ADR 0036).
struct ScriptedPicker {
    /// The `poll` answers by call index; the last one repeats.
    script: Vec<PickState>,
    polls: Cell<usize>,
}

impl ScriptedPicker {
    fn new(script: Vec<PickState>) -> Self {
        Self {
            script,
            polls: Cell::new(0),
        }
    }
}

impl FilePicker for ScriptedPicker {
    fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        Ok(None)
    }

    fn save(
        &self,
        _request: &SaveRequest,
        _data: &[u8],
    ) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        Ok(None)
    }

    fn poll(&self) -> PickState {
        let index = self.polls.get();
        self.polls.set(index + 1);
        self.script
            .get(index.min(self.script.len().saturating_sub(1)))
            .cloned()
            .unwrap_or(PickState::Idle)
    }
}

/// The upload answer: `file.open` re-dispatches with `name` + `data`.
fn picked_upload() -> PickState {
    PickState::Picked(FileChoice::Bytes {
        name: "minimal-drawing.vsdx".to_owned(),
        data: fixture_bytes(),
    })
}

#[test]
fn an_asynchronous_pick_completes_the_open_across_frames() {
    let mut rig = ShellRig::new();
    rig.shell
        .borrow_mut()
        .set_picker(Box::new(ScriptedPicker::new(vec![
            PickState::Pending,
            picked_upload(),
        ])));

    // The interactive dispatch asks, gets no synchronous answer and
    // waits (ADR 0036).
    let outcome = rig.dispatch("file.open", Source::Ui).expect("no error");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Waiting for the file dialog"),
        "{outcome:?}"
    );
    assert!(rig.shell.borrow().app.documents.is_empty(), "nothing yet");

    // The next frame polls the picker and completes the command.
    rig.run();
    let shell = rig.shell.borrow();
    let app = &shell.app;
    assert_eq!(app.documents.len(), 1, "the upload opened the document");
    // `name` + `data` (the upload shape) carries no `path`, so the
    // document is "Untitled"; what matters is that the bytes loaded.
    let session = app.active_document().expect("the opened document");
    assert!(!session.document.pages.is_empty(), "the fixture read back");
}

#[test]
fn a_cancelled_asynchronous_pick_forgets_the_pending_command() {
    let mut rig = ShellRig::new();
    rig.shell
        .borrow_mut()
        .set_picker(Box::new(ScriptedPicker::new(vec![
            PickState::Pending,
            PickState::Cancelled,
        ])));

    let outcome = rig.dispatch("file.open", Source::Ui).expect("no error");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Waiting for the file dialog")
    );
    rig.run();
    assert!(rig.shell.borrow().app.documents.is_empty(), "a cancel");

    // The pending slot is empty: a later pick starts clean and the
    // shell still renders.
    rig.run();
    assert!(rig.shell.borrow().app.documents.is_empty());
}

#[test]
fn the_window_commands_only_request_their_windows() {
    // P4-15 implements `app.quit` and `app.office.center.options`, so
    // a programmatic caller through `dispatch_one` gets the request
    // (`UiRequest::Quit` / `UiRequest::ShowOptions`) and no side
    // effect: never a silent quit, never a dialog (AGENTS.md
    // section 6). P4-16 removed the shell intercepts; the shell acts
    // on the requests for interactive sources only.
    let registry = larrygram_commands::builtins::registry().expect("the registry builds");
    let mut app = AppCtx::new(crate::phase4_settings());
    for (id, request) in [
        ("app.quit", UiRequest::Quit),
        ("app.office.center.options", UiRequest::ShowOptions),
    ] {
        let command = CommandId::parse(id).expect("id");
        let outcome = dispatch_one(
            &registry,
            &mut app,
            &command,
            Invocation::empty(Source::Mcp),
        )
        .unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(outcome.ui_request, Some(request), "{id}");
    }
    assert!(
        app.documents.is_empty() && app.active.is_none(),
        "nothing was opened or closed"
    );
}

#[test]
fn quit_is_handed_to_the_viewport() {
    // `app.quit` runs through the registry (P4-16: no shell
    // intercept); its `UiRequest::Quit` reaches the viewport.
    let mut rig = ShellRig::new();
    let outcome = rig.dispatch("app.quit", Source::Ui).expect("no error");
    assert_eq!(outcome.message.as_deref(), Some("Quit"));
    assert!(rig.shell.borrow().quit_requested());
    // The next frame sends the viewport close and resets the flag.
    rig.run();
    assert!(!rig.shell.borrow().quit_requested());
}
