//! The macOS app-menu Quit (task P7-42).
//!
//! Quit from the application menu — and its Cmd+Q key equivalent —
//! calls `terminate:`, which asks the delegate
//! `applicationShouldTerminate:` and, with no such method, terminates
//! the process at once: no unsaved-changes prompt, and no settings
//! flush (eframe persists when the event loop ends, which a direct
//! termination skips). winit 0.30 (under eframe 0.37) implements no
//! `applicationShouldTerminate:`, so the app must.
//!
//! The hook is the same `NSObject` method injection as the
//! open-documents hook (`macos_open_urls`, ADR 0074), for the same
//! reason: winit asserts it still owns the application delegate slot,
//! and taking that slot aborts the process (the task R8 crash). The
//! method goes on `NSObject`, the root of the delegate's class
//! hierarchy, and AppKit calls it like any delegate method.
//!
//! The hook refuses the direct termination (`NSTerminateCancel`) and
//! queues a quit request for the UI thread, which runs it as an
//! `app.quit` dispatch (`Source::Menu`). That is the same path as
//! `file.exit`, the Cmd+Q / Ctrl+Q chord and the window's close
//! button: the unsaved-changes walk of task P7-42 asks about each
//! changed drawing and ends in the viewport close. Never a silent
//! discard (AGENTS.md section 6).

#![allow(unsafe_code)] // ADR 0074: one Objective-C method injection.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::ClassType;
use objc2::MainThreadMarker;
use objc2::encode::Encoding;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{ffi, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSObject;

use crate::AppError;

/// `NSApplicationTerminateReply::NSTerminateCancel`: do not terminate
/// now; the queued quit walk closes the window when every changed
/// drawing is resolved (task P7-42).
const TERMINATE_CANCEL: isize = 0;

/// Whether [`install`] already ran (a second call is a no-op).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether an app-menu Quit is waiting for the UI thread (task P7-42).
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The hook that wakes the UI when a quit arrives (task P7-42): an
/// idle window runs no frame until something requests a repaint. The
/// app registers `egui::Context::request_repaint`; tests register a
/// spy. `None` until [`set_wake`] runs.
static WAKE: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// Registers the wake hook (task P7-42). Never panics (AGENTS.md
/// section 6): a poisoned mutex is recovered.
pub(crate) fn set_wake(hook: Box<dyn Fn() + Send + Sync>) {
    *WAKE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
}

/// Takes the queued app-menu Quit, leaving the queue empty (task
/// P7-42). The UI thread runs one `app.quit` dispatch per `true`.
pub(crate) fn take_quit() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Queues an app-menu Quit for the next frame and wakes the UI (task
/// P7-42). Never panics (AGENTS.md section 6).
fn queue_quit() {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    let hook = WAKE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hook) = hook.as_ref() {
        hook();
    }
}

/// The Objective-C callback type of `applicationShouldTerminate:`:
/// `NSInteger (id, SEL, NSApplication *)`.
type ShouldTerminate = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSApplication>) -> isize;

/// The `@encode` spelling of [`ShouldTerminate`], as `class_addMethod`
/// wants it.
fn should_terminate_types() -> String {
    format!(
        "{}{}{}{}",
        Encoding::Long,
        Encoding::Object,
        Encoding::Sel,
        Encoding::Object
    )
}

/// The Objective-C callback for `applicationShouldTerminate:` (task
/// P7-42): queue the quit for the UI thread and refuse the direct
/// termination, so the app keeps running until the unsaved-changes
/// walk closes the window (or the user cancels).
///
/// This crosses into AppKit's calling convention, so it must not
/// unwind: the body runs inside [`guard`], and a failure answers
/// `NSTerminateCancel` (never a silent termination).
unsafe extern "C" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _application: Option<&NSApplication>,
) -> isize {
    guard(|| {
        queue_quit();
        TERMINATE_CANCEL
    })
}

/// Runs the callback body so that a failure is logged and answered
/// with `NSTerminateCancel`, never unwound into Objective-C (an
/// `extern "C"` callback that unwinds aborts the process; AGENTS.md
/// section 6).
fn guard(body: impl FnOnce() -> isize) -> isize {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("warning: quit: the should-terminate callback failed");
            TERMINATE_CANCEL
        }
    }
}

/// The implementation pointer of [`should_terminate`].
fn should_terminate_imp() -> Imp {
    // SAFETY: `should_terminate` has exactly the signature of
    // `applicationShouldTerminate:`, which [`should_terminate_types`]
    // spells out for the registration below (ADR 0074).
    unsafe { std::mem::transmute::<ShouldTerminate, Imp>(should_terminate) }
}

/// Adds `applicationShouldTerminate:` to `class` (task P7-42).
///
/// Adds only: the class and its instances keep their identity, and
/// the application delegate slot is never taken (replacing the
/// delegate aborts inside winit, the task R8 crash). A repeat call
/// with the same hook is a no-op; a class that already carries a
/// different implementation is an error, not an override.
fn add_hook(class: &AnyClass) -> Result<(), AppError> {
    let selector = sel!(applicationShouldTerminate:);
    let imp = should_terminate_imp();
    if let Some(existing) = class.instance_method(selector) {
        return if std::ptr::fn_addr_eq(existing.implementation(), imp) {
            Ok(())
        } else {
            Err(AppError::new(
                "quit: the application delegate already answers should-terminate",
            ))
        };
    }
    let types = std::ffi::CString::new(should_terminate_types()).map_err(|error| {
        AppError::new(format!("quit: the method encoding is not valid: {error}"))
    })?;
    // SAFETY: `imp` is `should_terminate`, whose signature is exactly
    // `applicationShouldTerminate:` (`NSInteger (id, SEL, NSApplication
    // *)`), and `types` is that signature in `@encode` spelling;
    // `class_addMethod` copies the encoding (ADR 0074).
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
            "quit: the application delegate already answers should-terminate",
        )),
        None => Err(AppError::new(
            "quit: cannot add applicationShouldTerminate: to the application delegate",
        )),
    }
}

/// Installs the app-menu Quit hook (task P7-42).
///
/// Call it on the main thread **before** the event loop starts (the
/// menu can be used from the first frame). Off the main thread it
/// answers an error, never a panic (AGENTS.md section 6).
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
    let Some(_mtm) = MainThreadMarker::new() else {
        return Err(AppError::new("quit: the app is not on the main thread"));
    };
    add_hook(NSObject::class())
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::ClassBuilder;

    /// A stand-in for the application delegate's class: the hook
    /// target of the tests (registered once per process).
    fn target_class() -> &'static AnyClass {
        static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
        CLASS.get_or_init(|| {
            let builder = ClassBuilder::new(c"P742TerminateTarget", NSObject::class())
                .expect("the name is free");
            builder.register()
        })
    }

    #[test]
    fn the_hook_adds_the_selector_with_the_right_signature() {
        let class = target_class();
        add_hook(class).expect("the hook installs");
        let method = class
            .instance_method(sel!(applicationShouldTerminate:))
            .expect("the selector is added");
        assert!(
            std::ptr::fn_addr_eq(method.implementation(), should_terminate_imp()),
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
            "l",
            "the callback answers an NSInteger"
        );
        assert_eq!(
            arguments,
            ["@", ":", "@"],
            "self, the selector and the application argument"
        );
        // A repeat install is a no-op, not a double override.
        add_hook(class).expect("the second install is a no-op");
    }

    #[test]
    fn the_hook_queues_the_quit_and_refuses_the_termination() {
        let class = target_class();
        add_hook(class).expect("the hook installs");
        let object: Retained<AnyObject> = unsafe { msg_send![class, new] };
        take_quit();
        let woke = std::sync::Arc::new(AtomicBool::new(false));
        let flag = woke.clone();
        set_wake(Box::new(move || {
            flag.store(true, Ordering::SeqCst);
        }));
        let answer = unsafe { should_terminate(&object, sel!(applicationShouldTerminate:), None) };
        assert_eq!(answer, TERMINATE_CANCEL, "the direct termination is refused");
        assert!(take_quit(), "the quit is queued for the UI thread");
        assert!(!take_quit(), "the queue drains once");
        assert!(woke.load(Ordering::SeqCst), "the UI is woken");
    }

    #[test]
    fn the_callback_never_changes_the_class_of_its_target() {
        // The task R8 regression rule (ADR 0074): the hook adds a
        // method only, never a delegate swap.
        let class = target_class();
        let object: Retained<AnyObject> = unsafe { msg_send![class, new] };
        let before = object.class();
        add_hook(class).expect("the hook installs");
        assert_eq!(object.class(), before, "the class identity never changes");
    }
}
