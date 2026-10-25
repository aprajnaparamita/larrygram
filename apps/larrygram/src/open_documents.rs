//! Opening documents that arrive from outside the app (task R8).
//!
//! Two sources open a document without the Open dialog: the paths on
//! the command line (`larrygram <path>...`) and the macOS
//! open-documents event (`application:openURLs:`, from a Finder
//! double-click, `open -a Larrygram.app <file>` or a drop on the Dock
//! icon). Both run through the registry's `file.open` command (ADR
//! 0028), so an external open behaves exactly like one from the File
//! menu.
//!
//! The macOS event can arrive between two frames, so its delegate
//! queues the paths and the UI thread drains the queue once per frame
//! (task R8 Goal 2: one `file.open` dispatch per URL, on the UI
//! thread).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Source};
use larrygram_ui::Intent;
use larrygram_ui::shell::Shell;

/// The registry command every external path opens through (task R8
/// Goal 1).
const OPEN_COMMAND: &str = "file.open";

/// A queue of document paths waiting for the next frame (task R8 Goal
/// 2).
///
/// Tests use their own queue; [`QUEUED`] is the app's one.
#[derive(Default)]
pub(crate) struct OpenQueue {
    /// The queued paths, in arrival order.
    paths: Mutex<Vec<PathBuf>>,
}

impl OpenQueue {
    /// An empty queue.
    pub(crate) const fn new() -> Self {
        Self {
            paths: Mutex::new(Vec::new()),
        }
    }

    /// Appends `paths`, in arrival order.
    ///
    /// Never panics (AGENTS.md section 6): a poisoned mutex is
    /// recovered.
    pub(crate) fn queue_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        self.lock().extend(paths);
    }

    /// Takes every queued path, leaving the queue empty.
    pub(crate) fn take_paths(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.lock())
    }

    /// The lock, with a poisoned mutex recovered instead of a panic
    /// (AGENTS.md section 6).
    fn lock(&self) -> MutexGuard<'_, Vec<PathBuf>> {
        self.paths
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The queue the macOS delegate fills (`macos_open_urls`) and the UI
/// thread drains each frame (task R8 Goal 2).
pub(crate) static QUEUED: OpenQueue = OpenQueue::new();

/// Serialises the tests that share [`QUEUED`] (unit tests run in
/// parallel).
#[cfg(test)]
pub(crate) static QUEUE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// The `file.open` [`Intent`] for one path (task R8 Goal 1).
///
/// `None` only when the command id stops parsing, which cannot happen
/// for [`OPEN_COMMAND`]; the caller reports it instead of panicking.
fn open_intent(path: &Path, source: Source) -> Option<Intent> {
    let command = CommandId::parse(OPEN_COMMAND).ok()?;
    let invocation = Invocation::new(
        Args::new().with("path", ArgValue::Path(path.to_owned())),
        source,
    );
    Some(Intent::new(command, invocation))
}

/// Dispatches `paths` as `file.open`, one per path, in order (task R8
/// Goal 1 and 2).
///
/// Every path opens its own document session and the last one becomes
/// active, exactly as `file.open` does from the File menu (task R8
/// Goal 3). A path that does not open shows the same status-line error
/// the Open dialog shows and the remaining paths still run: no input
/// crashes the app (AGENTS.md section 6).
pub(crate) fn dispatch_paths(shell: &mut Shell, paths: &[PathBuf], source: Source) {
    for path in paths {
        let Some(intent) = open_intent(path, source) else {
            shell.note_error(format!("cannot dispatch {OPEN_COMMAND}"));
            continue;
        };
        if let Err(error) = shell.dispatch(&[intent]) {
            shell.note_error(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use larrygram_commands::app::{AppCtx, Settings};
    use larrygram_ui_spec::Loader;

    /// A shell over the real registry and layouts (the app's own
    /// wiring), with the welcome wizard already seen.
    fn shell() -> Shell {
        let mut settings = Settings::default();
        settings.welcome_seen = true;
        Shell::new(
            larrygram_commands::builtins::registry().expect("registry"),
            Loader::from_embedded().expect("layouts"),
            AppCtx::new(settings),
        )
    }

    /// A committed fixture under `corpus/fixtures/` (task P8-10).
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("corpus")
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn queued_paths_wait_in_order_until_they_are_taken() {
        // A test-local queue: the tests share no state.
        let queue = OpenQueue::new();
        let first = fixture("minimal-drawing.vsdx");
        let second = fixture("render-background-pages.vsdx");
        queue.queue_paths([first.clone(), second.clone()]);
        assert_eq!(queue.take_paths(), vec![first, second]);
        assert!(queue.take_paths().is_empty(), "the queue is drained");
    }

    #[test]
    fn a_queued_path_opens_its_document() {
        let queue = OpenQueue::new();
        let path = fixture("minimal-drawing.vsdx");
        let mut shell = shell();
        assert!(shell.app.active_document().is_none());

        queue.queue_paths([path.clone()]);
        let queued = queue.take_paths();
        dispatch_paths(&mut shell, &queued, Source::Ui);

        let session = shell.app.active_document().expect("the document opens");
        assert_eq!(session.path.as_deref(), Some(path.as_path()));
        assert!(shell.app.warnings.is_empty(), "{:?}", shell.app.warnings);
    }

    #[test]
    fn several_paths_open_in_order_and_the_last_is_active() {
        let first = fixture("minimal-drawing.vsdx");
        let second = fixture("render-background-pages.vsdx");
        let mut shell = shell();
        dispatch_paths(&mut shell, &[first.clone(), second.clone()], Source::Ui);
        let session = shell.app.active_document().expect("the documents open");
        assert_eq!(session.path.as_deref(), Some(second.as_path()));
    }

    #[test]
    fn a_path_that_does_not_open_shows_the_error_and_keeps_going() {
        let missing = PathBuf::from("/no/such/file.vsdx");
        let good = fixture("minimal-drawing.vsdx");
        let mut shell = shell();
        dispatch_paths(&mut shell, &[missing, good.clone()], Source::Cli);

        // The failing path is one status-line error (the Open dialog's
        // message) and the next path still opens.
        assert_eq!(shell.app.warnings.len(), 1, "{:?}", shell.app.warnings);
        assert!(
            shell.app.warnings[0].contains("cannot read input"),
            "{:?}",
            shell.app.warnings
        );
        let session = shell.app.active_document().expect("the next path opens");
        assert_eq!(session.path.as_deref(), Some(good.as_path()));
    }

    #[test]
    fn dispatching_nothing_changes_nothing() {
        let mut shell = shell();
        dispatch_paths(&mut shell, &[], Source::Ui);
        assert!(shell.app.active_document().is_none());
        assert!(shell.app.warnings.is_empty());
    }
}
