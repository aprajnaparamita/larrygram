# 0073: The macOS open-documents hook (the one `unsafe` module)

- Status: accepted
- Date: 2026-10-25
- Task: R8

## Context

Double-clicking a `.vsdx` or `.vsd` in Finder starts Larrygram but does
not open the file. macOS delivers the file as the
`application:openURLs:` Apple event, not through `argv`, and winit 0.30
(under eframe 0.37) does not handle it.

winit's own macOS documentation recommends registering an
`NSApplicationDelegate` and states that winit registers none. That is
out of date for 0.30.13: it registers `WinitApplicationDelegate`, and
its `NSApplication::sendEvent:` override asserts the application
delegate is still that object. The first task R8 build registered its
own delegate, which replaced winit's, and the process aborted 1.5 s
after launch (`panic!` "tried to get a delegate that was not the one
Winit has registered", then `panic_cannot_unwind`, SIGABRT). The
delegate slot is therefore off limits for the lifetime of this winit.

No safe route exposes the event. eframe 0.37 has no hook for it (its
`App` trait has no open-document callback), and no maintained crate in
the tree turns the event into a Rust callback. Getting the event needs
one Objective-C method, which is `unsafe` code inside our process.
AGENTS.md section 6 forbids `unsafe`, and allows an exception only
behind an accepted ADR.

## Decision

### One small module

`apps/larrygram/src/macos_open_urls.rs` is the only module with
`unsafe` code. It is `cfg(target_os = "macos")`, so no other platform
compiles it.

### Method injection, never a delegate change

The module adds the one method `application:openURLs:` to the class of
whatever `NSApplication` delegate the app already has (winit's),
through one `class_addMethod` call. This is the same method injection
winit itself uses for `NSApplication::sendEvent:`, and AppKit calls the
method like any delegate method (`respondsToSelector:` is answered
from the class). The delegate object and its class keep their
identity: nothing takes the delegate slot, so winit's assertion never
fires.

- The callback's implementation is one plain `extern "C"` function
  with the exact signature of `application:openURLs:`
  (`void (id, SEL, NSApplication *, NSArray<NSURL *> *)`), and the
  registered type encoding is that signature in `@encode` spelling,
  built from `objc2`'s `Encoding` values.
- A repeat install is a no-op, and a class that already carries a
  different implementation of the selector is an error, never an
  override. A concurrent install that adds the same hook first is a
  success.
- `install()` runs in the eframe app creator: after the winit event
  loop exists (the winit documentation says `NSApplication` may only be
  touched after `EventLoop::new`), but before the first frame, so the
  launch event is still queued when the hook is in place. It is called
  only on the main thread and answers an error, never a panic,
  anywhere else (AGENTS.md section 6). A failure is one status-line
  warning; the app still starts.

### No unwind across AppKit

An `extern "C"` callback that unwinds aborts the process (that is how
the first build died). The callback body therefore runs inside
`std::panic::catch_unwind`: any failure is logged and dropped, and the
queue of paths is the only state it touches.

### Lint levels

- `apps/larrygram/Cargo.toml` spells out the workspace lint list with
  `unsafe_code = "deny"` instead of `forbid`, because a command-line
  `--forbid` cannot be lowered by any inner attribute and cargo
  forbids mixing `lints.workspace = true` with overrides.
- The crate root of `apps/larrygram` keeps `#![deny(unsafe_code)]`, so
  every other module is still denied `unsafe`.
- `macos_open_urls.rs` carries `#![allow(unsafe_code)] // ADR 0073`,
  and every `unsafe` block inside it carries a `// SAFETY:` comment
  saying why it holds.

### Dispatch

The callback only queues paths. The UI thread drains the queue once
per frame and dispatches `file.open` per path through the command
registry (ADR 0028), exactly like the File > Open menu item. Every
error is the same status-line error the Open dialog shows, and no
input crashes the app.

### Dependencies

`objc2` 0.6, `objc2-app-kit` 0.3 and `objc2-foundation` 0.3
(MIT OR Apache-2.0), added to `apps/larrygram` for macOS only. They are
already in the tree through eframe 0.37, so the build gains no new
crate versions.

## Consequences

- Finder double-clicks, `open -a Larrygram.app <file>` and drops on the
  Dock icon open documents at launch and while the app runs.
- The exception is one file. A future reviewer can delete it and lose
  only this feature.
- The hook rides on winit's delegate class. If a future winit
  implements `application:openURLs:` itself, the install reports the
  conflict as an error instead of overriding it, and this ADR is
  revisited (then winit's own handling may need to be used).
- The wasm build does not compile the module (it is macOS-only), and
  the command-line paths work on every desktop platform.

## Alternatives considered

- **Registering our own `NSApplicationDelegate`** (the route winit's
  documentation recommends): aborts the process, because winit 0.30.13
  registers its own delegate and asserts it in `sendEvent:`. This is
  the crash the first task R8 build hit.
- **Subclassing winit's delegate and `object_setClass`**: same effect
  as method injection with more moving parts (a runtime class, a class
  swap per instance).
- **File paths in `argv`**: LaunchServices does not pass document paths
  in `argv` for `open -a` or a Finder double-click; only a direct
  terminal launch does (which this task also supports, separately).
- **Swap the event loop to `tao`** (which has an open-event): eframe
  runs winit; a second event loop is not an option.
- **Raw Apple Event Manager (`kAEOpenDocuments`)**: another `unsafe`
  route with descriptor parsing; whether AppKit's own handler still
  consumes the event first is less certain than delegate dispatch.
- **Poll the pasteboard or `NSDocumentController`**: neither reports
  launch documents to a non-document app.