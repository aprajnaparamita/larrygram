//! The macOS open-documents event (task R8 Goal 2).
//!
//! Finder double-clicks, `open -a Larrygram.app <file>` and drops on
//! the Dock icon deliver the files as the `application:openURLs:`
//! Apple event, not through `argv`. winit 0.30 (under eframe 0.37)
//! handles no such event and, by its own macOS documentation,
//! registers no `NSApplicationDelegate`, so the application must
//! register one itself.
//!
//! This is the one `unsafe` module of the app (ADR 0073): one
//! Objective-C delegate method, compiled only on macOS. The delegate
//! only queues the paths; `crate::open_documents` dispatches them as
//! `file.open` on the UI thread. Nothing here panics (AGENTS.md
//! section 6).

#![allow(unsafe_code)] // ADR 0073: one Objective-C delegate method.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};

use crate::AppError;
use crate::open_documents;

/// Whether [`install`] already ran (a second call is a no-op).
static INSTALLED: AtomicBool = AtomicBool::new(false);

// The delegate: one method, `application:openURLs:` (ADR 0073).
define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - The class implements no `Drop` and stores no ivars, so the
    //   inherited `init` is enough to construct it.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct OpenUrlsDelegate;

    unsafe impl NSObjectProtocol for OpenUrlsDelegate {}

    // SAFETY: `application:openURLs:` is an optional method of
    // `NSApplicationDelegate`; the signature matches the protocol's
    // (`&self`, `&NSApplication`, `&NSArray<NSURL>`, no return), and
    // the body only queues paths, so it upholds the protocol's
    // contract (ADR 0073).
    unsafe impl NSApplicationDelegate for OpenUrlsDelegate {
        /// `application:openURLs:`: queue each URL as a path for the
        /// next frame (task R8 Goal 2).
        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            open_documents::QUEUED.queue_paths(file_paths(urls));
        }
    }
);

impl OpenUrlsDelegate {
    /// The delegate object, initialised with `NSObject`'s `init`.
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `init` is `NSObject`'s designated initializer and
        // `this` is a freshly allocated `OpenUrlsDelegate` (ADR 0073).
        unsafe { msg_send![super(this), init] }
    }
}

/// The file paths of `urls`; a URL without a path is skipped.
fn file_paths(urls: &NSArray<NSURL>) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}

/// Registers the delegate with the shared `NSApplication` (task R8
/// Goal 2).
///
/// Call it on the main thread after the winit event loop exists (the
/// winit macOS documentation) and before the first frame, so the
/// launch event is still queued when the delegate is set. Off the main
/// thread it answers an error, never a panic (AGENTS.md section 6).
///
/// The delegate object is kept for the process lifetime on purpose:
/// `NSApplication` does not retain its delegate (ADR 0073).
pub(crate) fn install() -> Result<(), AppError> {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let Some(mtm) = MainThreadMarker::new() else {
        INSTALLED.store(false, Ordering::SeqCst);
        return Err(AppError::new(
            "open documents: the app is not on the main thread",
        ));
    };
    let delegate = OpenUrlsDelegate::new(mtm);
    let app = NSApplication::sharedApplication(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path fixture round-tripped through `NSURL`.
    fn urls_of(paths: &[PathBuf]) -> Retained<NSArray<NSURL>> {
        let urls: Vec<Retained<NSURL>> = paths
            .iter()
            .map(|path| NSURL::from_file_path(path).expect("the URL builds"))
            .collect();
        NSArray::from_retained_slice(&urls)
    }

    #[test]
    fn each_url_becomes_its_path() {
        let paths = vec![
            PathBuf::from("/tmp/one.vsdx"),
            PathBuf::from("/tmp/two.vsd"),
            PathBuf::from("/tmp/no extension"),
        ];
        assert_eq!(file_paths(&urls_of(&paths)), paths);
    }

    #[test]
    fn a_url_without_a_path_is_skipped() {
        let good = PathBuf::from("/tmp/one.vsdx");
        let urls = urls_of(std::slice::from_ref(&good));
        assert_eq!(file_paths(&urls), vec![good]);
        assert!(file_paths(&NSArray::from_retained_slice(&[])).is_empty());
    }

    #[test]
    fn install_off_the_main_thread_is_an_error_not_a_crash() {
        // A spawned thread is never the main thread, so `install`
        // takes its error path (task R8: never a panic).
        let outcome = std::thread::spawn(install).join().expect("the thread runs");
        let error = outcome.expect_err("off the main thread");
        assert!(error.to_string().contains("main thread"), "{error}");
    }
}
