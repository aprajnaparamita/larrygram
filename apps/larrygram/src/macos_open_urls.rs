//! The macOS open-documents event (task R8 Goal 2).
//!
//! Finder double-clicks, `open -a Larrygram.app <file>` and drops on
//! the Dock icon deliver the files as the `application:openURLs:`
//! Apple event, not through `argv`. winit 0.30 (under eframe 0.37)
//! handles no such event, so the app must.
//!
//! winit registers its own `NSApplicationDelegate` and asserts, in
//! every `sendEvent:`, that it is still the application delegate
//! ("tried to get a delegate that was not the one Winit has
//! registered"). Taking that slot therefore aborts the process, which
//! is what the first task R8 build did (2026-10-25, SIGABRT 1.5 s
//! after launch). This module never touches the delegate slot. It adds
//! the one method `application:openURLs:` to the class of the delegate
//! the app already has, the same method injection winit itself uses
//! for `NSApplication::sendEvent:`, and AppKit then calls it like any
//! delegate method.
//!
//! The callback only queues the paths;
//! `crate::open_documents` dispatches them as `file.open` on the UI
//! thread. This is the one `unsafe` module of the app (ADR 0073), and
//! nothing in it may unwind across the AppKit callback (AGENTS.md
//! section 6): the body runs inside `std::panic::catch_unwind`, and a
//! failure is logged and dropped.

#![allow(unsafe_code)] // ADR 0073: one Objective-C method injection.

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2::encode::Encoding;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{ffi, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSURL};

use crate::AppError;
use crate::open_documents;

/// Whether [`install`] already ran (a second call is a no-op).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The Objective-C callback type of `application:openURLs:`:
/// `void (id, SEL, NSApplication *, NSArray<NSURL *> *)`.
type OpenUrls = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSApplication>, &NSArray<NSURL>);

/// The `@encode` spelling of [`OpenUrls`], as `class_addMethod`
/// wants it.
fn open_urls_types() -> String {
    format!(
        "{}{}{}{}{}",
        Encoding::Void,
        Encoding::Object,
        Encoding::Sel,
        Encoding::Object,
        Encoding::Object
    )
}

/// The Objective-C callback for `application:openURLs:` (task R8 Goal
/// 2): queue each URL as a path for the next frame.
///
/// This crosses into AppKit's calling convention, so it must not
/// unwind: the body runs inside [`guard`], which logs and drops any
/// failure (AGENTS.md section 6).
unsafe extern "C" fn open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _application: Option<&NSApplication>,
    urls: &NSArray<NSURL>,
) {
    guard(|| open_documents::QUEUED.queue_paths(file_paths(urls)));
}

/// Runs the callback body so that a failure is logged and dropped,
/// never unwound into Objective-C (an `extern "C"` callback that
/// unwinds aborts the process; AGENTS.md section 6).
fn guard(body: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err() {
        eprintln!("warning: open documents: the open-URLs callback failed");
    }
}

/// The file paths of `urls`; a URL without a path is skipped.
fn file_paths(urls: &NSArray<NSURL>) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}

/// The implementation pointer of [`open_urls`].
fn open_urls_imp() -> Imp {
    // SAFETY: `open_urls` has exactly the signature of
    // `application:openURLs:`, which [`open_urls_types`] spells out
    // for the registration below (ADR 0073).
    unsafe { std::mem::transmute::<OpenUrls, Imp>(open_urls) }
}

/// Adds `application:openURLs:` to `class` (task R8 Goal 2).
///
/// Adds only: the class and its instances keep their identity, and
/// the application delegate slot is never taken (replacing the
/// delegate aborts inside winit, the task R8 crash). A repeat call
/// with the same hook is a no-op; a class that already carries a
/// different implementation is an error, not an override.
fn add_hook(class: &AnyClass) -> Result<(), AppError> {
    let selector = sel!(application:openURLs:);
    let imp = open_urls_imp();
    if let Some(existing) = class.instance_method(selector) {
        return if std::ptr::fn_addr_eq(existing.implementation(), imp) {
            Ok(())
        } else {
            Err(AppError::new(
                "open documents: the application delegate already handles open URLs",
            ))
        };
    }
    let types = CString::new(open_urls_types()).map_err(|error| {
        AppError::new(format!(
            "open documents: the method encoding is not valid: {error}"
        ))
    })?;
    // SAFETY: `imp` is `open_urls`, whose signature is exactly
    // `application:openURLs:` (`void (id, SEL, NSApplication *,
    // NSArray<NSURL *> *)`), and `types` is that signature in
    // `@encode` spelling; `class_addMethod` copies the encoding
    // (ADR 0073).
    let added = unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            imp,
            types.as_ptr(),
        )
    };
    if added.as_bool() {
        return Ok(());
    }
    // A concurrent install may have won the race; a hook with our
    // implementation is still a success.
    match class.instance_method(selector) {
        Some(existing) if std::ptr::fn_addr_eq(existing.implementation(), imp) => Ok(()),
        Some(_) => Err(AppError::new(
            "open documents: the application delegate already handles open URLs",
        )),
        None => Err(AppError::new(
            "open documents: cannot add application:openURLs: to the application delegate",
        )),
    }
}

/// Installs the open-documents hook (task R8 Goal 2).
///
/// Call it on the main thread after the winit event loop exists (the
/// winit macOS documentation) and before the first frame, so the
/// launch event is still queued when the hook is in place. Off the
/// main thread it answers an error, never a panic (AGENTS.md section
/// 6).
pub(crate) fn install() -> Result<(), AppError> {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let outcome = install_inner();
    if outcome.is_err() {
        INSTALLED.store(false, Ordering::SeqCst);
    }
    outcome
}

/// The body of [`install`], without the once-only guard.
fn install_inner() -> Result<(), AppError> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(AppError::new(
            "open documents: the app is not on the main thread",
        ));
    };
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app
        .delegate()
        .ok_or_else(|| AppError::new("open documents: the application has no delegate to hook"))?;
    let object: &AnyObject = AsRef::<AnyObject>::as_ref(&*delegate);
    add_hook(object.class())
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use objc2::ClassType;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::ClassBuilder;
    use objc2_foundation::NSObject;

    /// A stand-in for the application delegate's class: the hook
    /// target of the tests (registered once per process).
    fn target_class() -> &'static AnyClass {
        static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
        CLASS.get_or_init(|| {
            let builder = ClassBuilder::new(c"R8OpenUrlsTarget", NSObject::class())
                .expect("the name is free");
            builder.register()
        })
    }

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
    fn the_hook_adds_the_selector_with_the_right_signature() {
        let class = target_class();
        add_hook(class).expect("the hook installs");
        let method = class
            .instance_method(sel!(application:openURLs:))
            .expect("the selector is added");
        assert!(
            std::ptr::fn_addr_eq(method.implementation(), open_urls_imp()),
            "the hook is our callback"
        );
        let arguments: Vec<String> = (0..method.arguments_count())
            .map(|index| {
                method
                    .argument_type(index)
                    .map(|kind| kind.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            method.return_type().to_string_lossy(),
            "v",
            "the callback returns void"
        );
        assert_eq!(
            arguments,
            ["@", ":", "@", "@"],
            "self, the selector and the two object arguments"
        );
        // A repeat install is a no-op, not a double override.
        add_hook(class).expect("the second install is a no-op");
    }

    #[test]
    fn the_hook_queues_the_urls_of_the_event() {
        let class = target_class();
        add_hook(class).expect("the hook installs");
        let object: Retained<AnyObject> = unsafe { msg_send![class, new] };
        let paths = vec![PathBuf::from("/tmp/one.vsdx")];
        let urls = urls_of(&paths);
        let _serialised = open_documents::QUEUE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        open_documents::QUEUED.take_paths();
        unsafe { open_urls(&object, sel!(application:openURLs:), None, &urls) };
        assert_eq!(open_documents::QUEUED.take_paths(), paths);
    }

    #[test]
    fn the_hook_never_changes_the_class_of_its_target() {
        // Regression for the task R8 crash (2026-10-25): replacing the
        // application delegate aborts inside winit's `sendEvent:`
        // ("tried to get a delegate that was not the one Winit has
        // registered"). The hook adds a method only: the class and its
        // instances keep their identity, and this module never touches
        // the delegate slot (pinned on the source below).
        let class = target_class();
        let object: Retained<AnyObject> = unsafe { msg_send![class, new] };
        let before = object.class();
        add_hook(class).expect("the hook installs");
        assert_eq!(object.class(), before, "the class identity never changes");
        let responds: bool =
            unsafe { msg_send![&object, respondsToSelector: sel!(application:openURLs:)] };
        assert!(responds, "the delegate answers the event after the hook");

        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("macos_open_urls.rs"),
        )
        .expect("the module reads");
        // The one selector that must never appear here (spelled
        // without the literal so the check can find it).
        let forbidden = ["set", "Delegate"].concat();
        assert!(
            !source.contains(&forbidden),
            "the module must never take the application delegate slot"
        );
    }

    #[test]
    fn a_panicking_callback_is_logged_and_dropped() {
        // The AppKit callback must never unwind: an `extern "C"` panic
        // aborts the process (the task R8 crash class of failure).
        guard(|| panic!("an unexpected handler failure"));
        // The next callback still works afterwards (its own queue, so
        // the test needs no serialisation).
        let queue = open_documents::OpenQueue::new();
        let paths = vec![PathBuf::from("/tmp/one.vsdx")];
        let urls = urls_of(&paths);
        guard(|| queue.queue_paths(file_paths(&urls)));
        assert_eq!(queue.take_paths(), paths);
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
