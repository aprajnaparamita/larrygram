# 0073: The macOS open-documents delegate (the one `unsafe` module)

- Status: accepted
- Date: 2026-10-25
- Task: R8

## Context

Double-clicking a `.vsdx` or `.vsd` in Finder starts Larrygram but does
not open the file. macOS delivers the file as the
`application:openURLs:` Apple event, not through `argv`, and winit 0.30
(under eframe 0.37) does not handle it: its own macOS documentation
says that winit registers no `NSApplicationDelegate`, so the
application must register one.

No safe route exposes the event. eframe 0.37 has no hook for it (its
`App` trait has no open-document callback), winit has no event for it,
and no maintained crate in the tree turns it into a Rust callback. The
only route is one Objective-C delegate method, which is `unsafe` code
inside our process. AGENTS.md section 6 forbids `unsafe`, and section 6
allows an exception only behind an accepted ADR.

## Decision

### One small module

`apps/larrygram/src/macos_open_urls.rs` is the only module with
`unsafe` code. It is `cfg(target_os = "macos")`, so no other platform
compiles it. It defines one `NSApplicationDelegate` subclass with
`objc2::define_class!` (the same crate family eframe 0.37 uses) and
implements exactly one optional method, `application:openURLs:`. The
method converts the URLs to `PathBuf`s and queues them; it does no
other work.

### Lint levels

- `apps/larrygram/Cargo.toml` overrides the workspace lint to
  `unsafe_code = "deny"`, because a command-line `--forbid` cannot be
  lowered by any inner attribute.
- The crate root of `apps/larrygram` keeps `#![deny(unsafe_code)]`, so
  every other module is still denied `unsafe`.
- `macos_open_urls.rs` carries `#![allow(unsafe_code)] // ADR 0073`,
  and every `unsafe` block inside it carries a `// SAFETY:` comment
  saying why it holds.

### Registration and lifetime

- `install()` runs in the eframe app creator: after the winit event
  loop exists (the winit documentation says `NSApplication` may only be
  touched after `EventLoop::new`), but before the first frame, so the
  launch event is still queued when the delegate is set. It takes
  `NSApplication::sharedApplication`'s delegate slot; winit guarantees
  it never sets one.
- `install()` is called only on the main thread and answers an error,
  never a panic, anywhere else (AGENTS.md section 6). A failure is one
  status-line warning; the app still starts.
- The delegate object is kept alive for the process lifetime (the
  `Retained` is forgotten on purpose): `NSApplication` does not retain
  its delegate, and dropping it would leave a dangling delegate.

### Dispatch

The delegate only queues paths. The UI thread drains the queue once per
frame and dispatches `file.open` per path through the command registry
(ADR 0028), exactly like the File > Open menu item. Every error is the
same status-line error the Open dialog shows, and no input crashes the
app.

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
- `NSApplication`'s delegate slot is ours. If eframe ever grows its own
  delegate, the two must be composed (forwarding), and this ADR is
  revisited.
- The wasm build does not compile the module (it is macOS-only), and
  the command-line paths work on every desktop platform.

## Alternatives considered

- **File paths in `argv`**: LaunchServices does not pass document paths
  in `argv` for `open -a` or a Finder double-click; only a direct
  terminal launch does (which this task also supports, separately).
- **Swap the event loop to `tao`** (which has an open-event): eframe
  runs winit; a second event loop is not an option.
- **Wait for a winit API**: winit documents the delegate route as the
  supported answer and keeps no open-document event in its API.
- **Raw Apple Event Manager (`kAEOpenDocuments`)**: still `unsafe`,
  older API, and the delegate method is the documented replacement.
- **Poll the pasteboard or `NSDocumentController`**: neither reports
  launch documents to a non-document app.