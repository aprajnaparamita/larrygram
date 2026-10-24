//! The file flow: `NeedsInput` -> [`FilePicker`] test double ->
//! re-dispatch with `path` or `data: Bytes` (task P4-09, ADR 0036).
//!
//! A command never opens a dialog (AGENTS.md section 6): `file.open`
//! without a `path` answers [`CommandError::NeedsInput`], and only the
//! shell, only for an *interactive* source, asks the picker and
//! dispatches again with the chosen value. The test double stands in
//! for the native dialog (the wasm upload returns bytes instead of a
//! path; the same seam covers it).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{ArgValue, Args, CommandError, Invocation, NeededInput, Source};
use larrygram_commands::{CommandId, Outcome};
use larrygram_ui::Intent;
use larrygram_ui::dialogs::{FileChoice, FilePicker, OpenRequest, PickState, SaveRequest};
use larrygram_ui::shell::Shell;

use crate::sets;

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

/// The `save` calls seen: the request and the data.
type SaveCalls = Rc<RefCell<Vec<(SaveRequest, Vec<u8>)>>>;

/// A picker that answers with a fixed choice and records its calls.
#[derive(Default)]
struct FakePicker {
    /// The answer for `open` (`None` cancels).
    open_with: Option<FileChoice>,
    /// The answer for `save` (`None` cancels).
    save_with: Option<FileChoice>,
    /// The `open` calls seen.
    open_calls: Rc<RefCell<Vec<OpenRequest>>>,
    /// The `save` calls seen (the request and the data).
    save_calls: SaveCalls,
}

impl FilePicker for FakePicker {
    fn open(&self, request: &OpenRequest) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        self.open_calls.borrow_mut().push(request.clone());
        Ok(self.open_with.clone())
    }

    fn save(
        &self,
        request: &SaveRequest,
        data: &[u8],
    ) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        self.save_calls
            .borrow_mut()
            .push((request.clone(), data.to_vec()));
        Ok(self.save_with.clone())
    }

    fn poll(&self) -> PickState {
        PickState::Idle
    }
}

/// A shell over the real registry with a recording picker.
fn shell_with(picker: FakePicker) -> (Shell, Rc<RefCell<Vec<OpenRequest>>>) {
    let registry = larrygram_commands::builtins::registry().expect("the registry builds");
    let mut shell = Shell::new(registry, sets().clone(), AppCtx::new(crate::phase4_settings()));
    let calls = Rc::clone(&picker.open_calls);
    shell.set_picker(Box::new(picker));
    (shell, calls)
}

/// One `file.open` intent from `source` with no arguments.
fn open_intent(source: Source) -> Intent {
    let command = CommandId::parse("file.open").expect("a valid command id");
    Intent::new(command, Invocation::empty(source))
}

/// The one result of dispatching `intents`.
fn dispatch_one_result(shell: &mut Shell, intent: Intent) -> Result<Outcome, CommandError> {
    let mut results = shell.dispatch(&[intent]).expect("the intent dispatches");
    results.pop().expect("one result per intent")
}

#[test]
fn an_interactive_open_with_no_path_asks_the_picker_and_re_dispatches() {
    let picker = FakePicker {
        open_with: Some(FileChoice::Path(fixture_path())),
        ..FakePicker::default()
    };
    let (mut shell, calls) = shell_with(picker);

    let outcome = dispatch_one_result(&mut shell, open_intent(Source::Ui))
        .expect("the picked path opens the document");
    assert!(
        outcome.message.is_some(),
        "the command reports its message: {outcome:?}"
    );
    assert_eq!(calls.borrow().len(), 1, "the picker was asked once");
    let request = &calls.borrow()[0];
    assert!(
        request.extensions.iter().any(|e| e == "vsdx"),
        "the open dialog filters on .vsdx: {request:?}"
    );
    let session = shell
        .app
        .active_document()
        .expect("the document is open after the re-dispatch");
    assert_eq!(session.path.as_deref(), Some(fixture_path().as_path()));
}

#[test]
fn a_picker_that_returns_bytes_re_dispatches_with_name_and_data() {
    let picker = FakePicker {
        open_with: Some(FileChoice::Bytes {
            name: "minimal-drawing.vsdx".to_owned(),
            data: fixture_bytes(),
        }),
        ..FakePicker::default()
    };
    let (mut shell, calls) = shell_with(picker);

    dispatch_one_result(&mut shell, open_intent(Source::Ui))
        .expect("the picked bytes open the document");
    assert_eq!(calls.borrow().len(), 1);
    let session = shell
        .app
        .active_document()
        .expect("the document is open after the re-dispatch");
    // A wasm upload has no path: the name and the bytes are the
    // arguments (ADR 0036).
    assert_eq!(session.path, None, "an upload is never-saved");
    assert!(session.preserve.is_some(), "the package is kept for saving");
}

#[test]
fn cancelling_the_dialog_dispatches_nothing() {
    let picker = FakePicker {
        open_with: None,
        ..FakePicker::default()
    };
    let (mut shell, _calls) = shell_with(picker);

    let outcome = dispatch_one_result(&mut shell, open_intent(Source::Ui))
        .expect("a cancel is a normal outcome");
    assert_eq!(outcome.message.as_deref(), Some("Cancelled"));
    assert!(
        shell.app.active_document().is_none(),
        "a cancelled open opens nothing"
    );
}

#[test]
fn a_programmatic_source_gets_needs_input_and_no_dialog() {
    let picker = FakePicker {
        open_with: Some(FileChoice::Path(fixture_path())),
        ..FakePicker::default()
    };
    let (mut shell, calls) = shell_with(picker);

    // `Source::Test` is programmatic (ADR 0028): the error goes back
    // to the caller untouched and no dialog can appear.
    let error = dispatch_one_result(&mut shell, open_intent(Source::Test))
        .expect_err("a programmatic open without a path needs input");
    match error {
        CommandError::NeedsInput(NeededInput::Path { purpose, .. }) => {
            assert_eq!(
                purpose,
                larrygram_commands::PathPurpose::OpenFile,
                "the purpose names the dialog kind"
            );
        }
        other => panic!("expected NeedsInput(Path), got {other}"),
    }
    assert!(calls.borrow().is_empty(), "the picker was never asked");
    assert!(shell.app.active_document().is_none());
}

#[test]
fn a_never_saved_document_asks_where_to_save_and_writes_there() {
    let tmp = std::env::temp_dir().join(format!("p4-09-file-flow-{}.vsdx", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let picker = FakePicker {
        save_with: Some(FileChoice::Path(tmp.clone())),
        ..FakePicker::default()
    };
    let registry = larrygram_commands::builtins::registry().expect("the registry builds");
    let mut shell = Shell::new(registry, sets().clone(), AppCtx::new(crate::phase4_settings()));
    let save_calls = Rc::clone(&picker.save_calls);
    shell.set_picker(Box::new(picker));

    // `file.new`: a document that was never saved.
    let command = CommandId::parse("file.new").expect("id");
    dispatch_one_result(
        &mut shell,
        Intent::new(command, Invocation::empty(Source::Ui)),
    )
    .expect("file.new runs");

    // `file.save` without a `path`: the picker chooses where, and the
    // command runs again with the path filled in (ADR 0036).
    let command = CommandId::parse("file.save").expect("id");
    let outcome = dispatch_one_result(
        &mut shell,
        Intent::new(command, Invocation::empty(Source::Ui)),
    )
    .expect("the save runs after the pick");
    assert!(outcome.message.is_some());
    assert_eq!(
        save_calls.borrow().len(),
        1,
        "the save dialog was asked once"
    );
    assert!(tmp.is_file(), "the picker's path received the file");
    assert!(std::fs::metadata(&tmp).expect("written").len() > 0);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn a_supplied_path_skips_the_picker_entirely() {
    let picker = FakePicker {
        open_with: Some(FileChoice::Path(fixture_path())),
        ..FakePicker::default()
    };
    let (mut shell, calls) = shell_with(picker);

    let command = CommandId::parse("file.open").expect("id");
    let args = Args::new().with("path", ArgValue::Path(fixture_path()));
    let intent = Intent::new(command, Invocation::new(args, Source::Ui));
    dispatch_one_result(&mut shell, intent).expect("the document opens");
    assert!(
        calls.borrow().is_empty(),
        "an argument-bearing command never opens a dialog"
    );
    assert!(shell.app.active_document().is_some());
}
