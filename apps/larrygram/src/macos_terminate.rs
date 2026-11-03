//! The macOS app-menu Quit (task P7-42, review M1 of task P7-46).
//!
//! Quit from the application menu — and its Cmd+Q key equivalent —
//! calls `terminate:`, which asks the delegate
//! `applicationShouldTerminate:` and, with no such method, terminates
//! the process at once: no unsaved-changes prompt. winit 0.30 (under
//! eframe 0.37) implements no `applicationShouldTerminate:`, so the
//! app must.
//!
//! A direct termination is fine when nothing is unsaved — the process
//! ends cleanly and winit's `willTerminate` reaches eframe's `exiting`,
//! which persists the settings. The hook exists for the prompt: when a
//! drawing has unsaved work, the walk must ask before the app goes
//! away. So the hook answers
//!
//! - [`NSApplicationTerminateReply::TerminateNow`] when no drawing is
//!   changed (a logout, restart or shutdown goes straight through),
//! - [`NSApplicationTerminateReply::TerminateLater`] when one is: the
//!   UI thread runs the app-menu Quit as an `app.quit` dispatch
//!   (`Source::Menu`), the same unsaved-changes walk as `file.exit`,
//!   the Cmd+Q / Ctrl+Q chord and the window's close button, and the
//!   walk answers the pending termination with
//!   `replyToApplicationShouldTerminate:` — YES when it finishes, NO
//!   when the user cancels.
//!
//! The "any changed drawing" state the hook reads is published by the
//! UI thread ([`set_changed`]), updated when it changes.
//!
//! The hook is the same `NSObject` method injection as the
//! open-documents hook (`macos_open_urls`, ADR 0074), for the same
//! reason: winit asserts it still owns the application delegate slot,
//! and taking that slot aborts the process (the task R8 crash). The
//! method goes on `NSObject`, the root of the delegate's class
//! hierarchy, and AppKit calls it like any delegate method. Never a
//! silent discard of unsaved work (AGENTS.md section 6).

#![allow(unsafe_code)] // ADR 0074: one Objective-C method injection.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::ClassType;
use objc2::MainThreadMarker;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{ffi, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use objc2_foundation::NSObject;

use crate::AppError;

/// Whether [`install`] already ran (a second call is a no-op).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether an app-menu Quit is waiting for the UI thread (task P7-42).
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether any drawing has unsaved changes (task P7-46 goal 1): the
/// UI thread publishes it, the hook's answer reads it.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Whether a termination is waiting for the walk's answer (task
/// P7-46 goal 1): the hook answered `NSTerminateLater`, and the walk
/// must call [`reply_termination`] with its verdict.
static TERMINATE_LATER: AtomicBool = AtomicBool::new(false);

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

/// Publishes whether any drawing has unsaved changes (task P7-46
/// goal 1). The UI thread calls it when the answer changes; the hook
/// reads it on the main thread (which is the UI thread).
pub(crate) fn set_changed(changed: bool) {
    CHANGED.store(changed, Ordering::SeqCst);
}

/// Takes the queued app-menu Quit, leaving the queue empty (task
/// P7-42). The UI thread runs one `app.quit` dispatch per `true`.
pub(crate) fn take_quit() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Queues an app-menu Quit for the next frame and wakes the UI (task
/// P7-42). Never panics (AGENTS.md section 6). The delegate callback
/// calls it; the app's tests queue a quit through it too.
pub(crate) fn queue_quit() {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    let hook = WAKE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hook) = hook.as_ref() {
        hook();
    }
}

/// Answers a termination the hook deferred (task P7-46 goal 1): the
/// unsaved-changes walk calls it with YES when every changed drawing
/// is resolved and NO when the user cancels. Without a deferred
/// termination this does nothing.
pub(crate) fn reply_termination(should_terminate: bool) {
    if !TERMINATE_LATER.swap(false, Ordering::SeqCst) {
        return;
    }
    // SAFETY: the reply is the documented half of an
    // `NSTerminateLater` answer, sent on the main thread (the UI
    // thread, which is the AppKit main thread) while AppKit waits for
    // it (the objc2-app-kit contract of
    // `replyToApplicationShouldTerminate:`).
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.replyToApplicationShouldTerminate(should_terminate);
    }
}

/// The Objective-C callback type of `applicationShouldTerminate:`:
/// `NSInteger (id, SEL, NSApplication *)`.
type ShouldTerminate = unsafe extern "C" fn(&AnyObject, Sel, Option<&NSApplication>) -> isize;

/// The `@encode` spelling of [`ShouldTerminate`], as `class_addMethod`
/// wants it. The return is an `NSInteger`, spelled as the `isize`
/// encoding (task P7-46 goal 4).
fn should_terminate_types() -> String {
    format!(
        "{}{}{}{}",
        <isize as Encode>::ENCODING,
        Encoding::Object,
        Encoding::Sel,
        Encoding::Object
    )
}

/// The Objective-C callback for `applicationShouldTerminate:` (task
/// P7-42, review M1 of task P7-46): with no changed drawing it lets
/// the termination through (`NSTerminateNow`), with one it queues the
/// quit walk and defers (`NSTerminateLater`), so the walk can answer
/// with [`reply_termination`].
///
/// This crosses into AppKit's calling convention, so it must not
/// unwind: the body runs inside [`guard`], and a failure answers
/// `NSTerminateCancel` (never a silent termination with unsaved work).
unsafe extern "C" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _application: Option<&NSApplication>,
) -> isize {
    guard(|| {
        if !CHANGED.load(Ordering::SeqCst) {
            // Nothing to save: a logout, restart or shutdown goes
            // through (review M1).
            return NSApplicationTerminateReply::TerminateNow.0 as isize;
        }
        TERMINATE_LATER.store(true, Ordering::SeqCst);
        queue_quit();
        NSApplicationTerminateReply::TerminateLater.0 as isize
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
            NSApplicationTerminateReply::TerminateCancel.0 as isize
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

/// Checks that the class AppKit sends `applicationShouldTerminate:` to
/// resolves the selector to **our** implementation (task P7-46 goal
/// 3).
///
/// The hook lives on `NSObject`, the root of the delegate's hierarchy,
/// so the application delegate (winit's `WinitApplicationDelegate`)
/// reaches it by inheritance — unless that class ever starts defining
/// the selector itself, in which case its implementation is the one
/// AppKit calls and the app-menu Quit would skip the unsaved-changes
/// prompt. [`verify_delegate_hook`] runs the check against the live
/// application delegate; the app calls it once on its first frame and
/// reports a failure on the status line (AGENTS.md section 6: the app
/// never crashes on it).
pub(crate) fn hook_is_reachable(delegate_class: &AnyClass) -> Result<(), AppError> {
    let selector = sel!(applicationShouldTerminate:);
    match delegate_class.instance_method(selector) {
        Some(method) if std::ptr::fn_addr_eq(method.implementation(), should_terminate_imp()) => {
            Ok(())
        }
        Some(_) => Err(AppError::new(
            "quit: the application delegate answers applicationShouldTerminate: itself; \
             the app-menu Quit would skip the unsaved-changes prompt",
        )),
        None => Err(AppError::new(
            "quit: the application delegate does not reach the should-terminate hook",
        )),
    }
}

/// [`hook_is_reachable`] against the live application delegate (task
/// P7-46 goal 3). `None` when there is no delegate yet; a failure is
/// the message the app reports on its status line.
pub(crate) fn verify_delegate_hook() -> Option<Result<(), AppError>> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app.delegate()?;
    // The delegate's class, the plain Objective-C `class` message.
    let class: &AnyClass = unsafe { objc2::msg_send![&*delegate, class] };
    Some(hook_is_reachable(class))
}

/// One lock for every test that touches this module's globals
/// (`QUIT_REQUESTED`, `CHANGED`, `TERMINATE_LATER`, `WAKE`): they run
/// one at a time (task P7-46 goal 3). The app's own tests take it too
/// when they queue a quit.
#[cfg(test)]
pub(crate) fn test_serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::ClassBuilder;

    /// The serial lock of the tests that touch the module's globals.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        test_serial()
    }

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

    /// Sends `applicationShouldTerminate:` to `object` through the
    /// Objective-C runtime (task P7-46 goal 3) and returns the reply.
    fn send_should_terminate(object: &AnyObject) -> isize {
        // SAFETY: the hook is installed on the object's class with the
        // `applicationShouldTerminate:` signature, and the message
        // takes no arguments (the test installs it first).
        unsafe { msg_send![object, applicationShouldTerminate: Option::<&NSApplication>::None] }
    }

    #[test]
    fn the_hook_adds_the_selector_with_the_right_signature() {
        let _serial = serial();
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
            <isize as Encode>::ENCODING.to_string(),
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

    /// Task P7-46 goal 3: the hook answers both ways through the
    /// Objective-C runtime — `NSTerminateNow` with no changed drawing
    /// (a logout goes through, review M1) and `NSTerminateLater` with
    /// one (the walk answers).
    #[test]
    fn the_hook_answers_now_without_unsaved_work_and_later_with_it() {
        let _serial = serial();
        let class = target_class();
        add_hook(class).expect("the hook installs");
        let object: Retained<AnyObject> = unsafe { msg_send![class, new] };
        take_quit();
        TERMINATE_LATER.store(false, Ordering::SeqCst);

        // Nothing changed: the termination goes through at once.
        set_changed(false);
        let answer = send_should_terminate(&object);
        assert_eq!(
            answer,
            NSApplicationTerminateReply::TerminateNow.0 as isize,
            "no changed drawing: the logout, restart or shutdown continues"
        );
        assert!(!take_quit(), "no walk is queued");
        assert!(
            !TERMINATE_LATER.load(Ordering::SeqCst),
            "nothing waits for an answer"
        );

        // One changed drawing: the walk is queued and the termination
        // waits for its answer.
        set_changed(true);
        let woke = std::sync::Arc::new(AtomicBool::new(false));
        let flag = woke.clone();
        set_wake(Box::new(move || {
            flag.store(true, Ordering::SeqCst);
        }));
        let answer = send_should_terminate(&object);
        assert_eq!(
            answer,
            NSApplicationTerminateReply::TerminateLater.0 as isize,
            "a changed drawing defers the termination to the walk"
        );
        assert!(take_quit(), "the quit is queued for the UI thread");
        assert!(!take_quit(), "the queue drains once");
        assert!(woke.load(Ordering::SeqCst), "the UI is woken");
        assert!(
            TERMINATE_LATER.load(Ordering::SeqCst),
            "the walk owes AppKit an answer"
        );

        // The walk's answer clears the debt (AppKit side effects
        // cannot run in the harness; the state is what the test can
        // pin).
        reply_termination(false);
        assert!(!TERMINATE_LATER.load(Ordering::SeqCst), "the walk answered");
        reply_termination(true);
    }

    /// Task P7-46 goal 3: `install` checks the main thread and the
    /// method lands on `NSObject` (the class the delegate inherits
    /// from). A spawned thread is never the main thread, so the test
    /// takes the error path; the main-thread body is `add_hook` on the
    /// root class, which the other half pins.
    #[test]
    fn install_checks_the_main_thread_and_lands_the_method_on_the_root_class() {
        let _serial = serial();
        let outcome = std::thread::spawn(install).join().expect("the thread runs");
        let error = outcome.expect_err("off the main thread");
        assert!(error.to_string().contains("main thread"), "{error}");

        // The body `install` runs on the main thread (ADR 0074).
        add_hook(NSObject::class()).expect("the hook lands on NSObject");
        let method = NSObject::class()
            .instance_method(sel!(applicationShouldTerminate:))
            .expect("NSObject answers should-terminate");
        assert!(
            std::ptr::fn_addr_eq(method.implementation(), should_terminate_imp()),
            "the method is ours"
        );
        add_hook(NSObject::class()).expect("a second add is a no-op");
    }

    /// Task P7-46 goal 3: the reach check passes for a delegate class
    /// that inherits our hook and fails clearly for one that answers
    /// the selector itself (the shadow a future winit would add).
    #[test]
    fn the_reach_check_catches_a_delegate_that_shadows_the_hook() {
        let _serial = serial();
        add_hook(NSObject::class()).expect("the hook is on NSObject");
        // A delegate that inherits ours: the check passes.
        let inheriting = ClassBuilder::new(c"P746InheritsTerminate", NSObject::class())
            .expect("the name is free")
            .register();
        hook_is_reachable(inheriting).expect("the inherited hook is the one AppKit reaches");

        // A delegate that answers the selector itself: the check fails
        // with the message the app reports on its status line.
        let foreign = ClassBuilder::new(c"P746ShadowsTerminate", NSObject::class())
            .expect("the name is free")
            .register();
        let types = std::ffi::CString::new(should_terminate_types()).expect("the encoding");
        // SAFETY: `foreign_should_terminate_imp` has exactly the
        // `applicationShouldTerminate:` signature, and `types` is that
        // signature in `@encode` spelling (ADR 0074's rule, the same
        // registration `add_hook` does).
        unsafe {
            ffi::class_addMethod(
                foreign as *const AnyClass as *mut AnyClass,
                sel!(applicationShouldTerminate:),
                foreign_should_terminate_imp(),
                types.as_ptr(),
            );
        }
        let error = hook_is_reachable(foreign).expect_err("the shadow is caught");
        assert!(
            error
                .to_string()
                .contains("answers applicationShouldTerminate: itself"),
            "{error}"
        );
    }

    /// Task P7-46 goal 3: winit's own application delegate class must
    /// not define `applicationShouldTerminate:` itself — if it ever
    /// does, it is the one AppKit reaches and ours never runs. The
    /// class registers with the Objective-C runtime when winit's event
    /// loop starts; where it is present (the running app) this checks
    /// it directly, and where it is not (a test process with no event
    /// loop) [`hook_is_reachable`] is the check the app runs on its
    /// first frame, against the real delegate class.
    #[test]
    fn winit_does_not_answer_should_terminate_itself() {
        let _serial = serial();
        add_hook(NSObject::class()).expect("the hook is on NSObject");
        match AnyClass::get(c"WinitApplicationDelegate") {
            Some(class) => hook_is_reachable(class).unwrap_or_else(|error| {
                panic!("winit's delegate shadows the hook: {error}");
            }),
            // Not registered in this process (no event loop ran): the
            // real-process check is the app's first frame.
            None => {
                assert!(
                    hook_is_reachable(NSObject::class()).is_ok(),
                    "NSObject carries our hook"
                );
            }
        }
    }

    /// A stand-in `applicationShouldTerminate:` (the shadow a future
    /// winit would define). It answers `NSTerminateCancel`.
    unsafe extern "C" fn foreign_should_terminate(
        _this: &AnyObject,
        _cmd: Sel,
        _application: Option<&NSApplication>,
    ) -> isize {
        NSApplicationTerminateReply::TerminateCancel.0 as isize
    }

    /// The implementation pointer of [`foreign_should_terminate`].
    fn foreign_should_terminate_imp() -> Imp {
        // SAFETY: `foreign_should_terminate` has exactly the
        // `applicationShouldTerminate:` signature (ADR 0074).
        unsafe { std::mem::transmute::<ShouldTerminate, Imp>(foreign_should_terminate) }
    }
}
