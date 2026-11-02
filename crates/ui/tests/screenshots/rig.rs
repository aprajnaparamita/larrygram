//! The screenshot rig: a whole [`Shell`] driven headlessly through
//! `egui_kittest` (the Phase 7 `ShellRig` pattern) plus the raw input
//! helpers the shot states need (clicks, right-clicks, double clicks,
//! drags with the pointer held down, typing).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::{Harness, kittest::Queryable};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{Args, CommandId, Invocation, Registry, Source};
use larrygram_geom::Point;
use larrygram_ui::Intent;
use larrygram_ui::dialogs::{FileChoice, FilePicker, OpenRequest, PickState, SaveRequest};
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutKind, LayoutSets, Loader};

/// The committed layouts, parsed once per test binary.
pub fn sets() -> &'static LayoutSets {
    use std::sync::OnceLock;
    static SETS: OnceLock<LayoutSets> = OnceLock::new();
    SETS.get_or_init(|| Loader::from_embedded().expect("the embedded ui-spec loads"))
}

/// Settings for one layout kind (the welcome wizard already seen: the
/// wizard shots drive their own first launch).
pub fn settings_for(kind: LayoutKind) -> Settings {
    let mut settings = Settings::default();
    settings.welcome_seen = true;
    settings.ui_layout = kind;
    settings.default_layout = kind;
    settings
}

/// A picker that must never be called: every dispatch carries its
/// arguments (AGENTS.md section 6: a command never opens a dialog).
pub struct NoDialogPicker;

impl FilePicker for NoDialogPicker {
    fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        panic!("the screenshot build must never open a dialog")
    }

    fn save(
        &self,
        _request: &SaveRequest,
        _data: &[u8],
    ) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        panic!("the screenshot build must never open a dialog")
    }

    fn poll(&self) -> PickState {
        PickState::Idle
    }
}

/// A whole [`Shell`] driven headlessly in a window of `size` logical
/// pixels at `scale` (the display scale: 2x renders 2 pixels per
/// logical pixel).
pub struct Rig {
    /// The shell under test (the same instance across frames).
    pub shell: Rc<RefCell<Shell>>,
    harness: Harness<'static>,
}

impl Rig {
    /// A shell over `app` in a window of `size` logical pixels at
    /// `scale`, with the brand image loaders installed (as the apps
    /// do at start-up). `render_cursor` paints the harness's synthetic
    /// mouse cursor (the mid-drag shots keep it: it is the pointer
    /// that drags).
    pub fn at(app: AppCtx, size: egui::Vec2, scale: f32, render_cursor: bool) -> Self {
        let registry: Registry =
            larrygram_commands::builtins::registry().expect("the registry builds");
        let shell = Rc::new(RefCell::new(Shell::new(registry, sets().clone(), app)));
        {
            let mut shell = shell.borrow_mut();
            shell.set_picker(Box::new(NoDialogPicker));
        }
        let harness_shell = Rc::clone(&shell);
        let harness = Harness::builder()
            .with_size(size)
            .with_pixels_per_point(scale)
            .with_theme(egui::Theme::Light)
            .with_render_cursor(render_cursor)
            .with_context_setup(|ctx| {
                larrygram_ui::install_image_loaders(ctx);
                // One long double-click window for the text-editing
                // shot (the Phase 7 rig's rule): one harness run steps
                // the clock 0.25 s.
                ctx.options_mut(|options| {
                    options.input_options.max_double_click_delay = 10.0;
                });
            })
            .build_ui(move |ui| {
                // `build_ui` hosts the closure in a frame with an 8 px
                // outer margin that nothing paints (transparent in the
                // PNG). The app's root ui is the whole window, so the
                // shell draws into a child ui over the whole screen.
                let screen = ui.ctx().viewport_rect();
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
                child.set_clip_rect(screen);
                let mut shell = harness_shell.borrow_mut();
                let intents = shell.show_in(&mut child).expect("the frame renders");
                shell.dispatch(&intents).expect("the intents dispatch");
            });
        Self { shell, harness }
    }

    /// Draws one frame.
    pub fn run(&mut self) {
        self.harness.run();
    }

    /// Draws `count` frames (the settle rule: the first frames apply
    /// the theme and rasterise the icons).
    pub fn settle(&mut self, count: usize) {
        for _ in 0..count {
            self.harness.run();
        }
    }

    /// The harness itself (pointer, key and size events).
    pub fn harness(&mut self) -> &mut Harness<'static> {
        &mut self.harness
    }

    /// Renders the last frame with the wgpu renderer.
    pub fn render(&mut self) -> image::RgbaImage {
        self.harness.render().expect("the frame renders")
    }

    /// One dispatch through the shell (no arguments).
    pub fn dispatch(&mut self, id: &'static str) {
        self.dispatch_args(id, Args::new());
    }

    /// One dispatch through the shell with arguments (the registry
    /// path every control uses).
    pub fn dispatch_args(&mut self, id: &'static str, args: Args) {
        let command = CommandId::parse(id).expect("id");
        let intent = Intent::new(command, Invocation::new(args, Source::Test));
        let mut shell = self.shell.borrow_mut();
        let mut results = shell.dispatch(&[intent]).expect("the intent dispatches");
        results
            .pop()
            .expect("one result per intent")
            .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    }

    /// The screen point of a page point in the canvas.
    pub fn screen(&self, point: Point) -> egui::Pos2 {
        self.shell
            .borrow()
            .page_to_screen(point)
            .expect("the canvas drew")
    }

    /// Pans the view so page point (`x`, `y`) sits mid-canvas at
    /// `zoom` (the Phase 7 rig's pan rule, solved from
    /// `page_to_screen`).
    pub fn center_view(&mut self, x: f64, y: f64, zoom: f64) {
        self.run();
        {
            let mut shell = self.shell.borrow_mut();
            let session = shell.app.active_document_mut().expect("session");
            session.view.zoom = zoom;
        }
        self.run();
        let viewport = self
            .shell
            .borrow()
            .canvas_viewport()
            .expect("the canvas drew");
        let target = viewport.center();
        let current = self.screen(Point::new(x, y));
        let k = 96.0 * zoom;
        let mut shell = self.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("session");
        session.view.pan.x -= f64::from(current.x - target.x) / k;
        session.view.pan.y -= f64::from(current.y - target.y) / k;
        drop(shell);
        self.run();
        self.run();
    }

    /// Stops the text caret from blinking. `egui_kittest` turns the
    /// blink off when the harness starts, but the app's theme (applied
    /// in the first frames) installs visuals with the blink back on,
    /// and one harness frame steps the clock far enough to land in the
    /// caret's hidden phase. Call after the theme has applied.
    pub fn steady_caret(&mut self) {
        self.run();
        self.harness()
            .ctx
            .global_style_mut(|style| style.visuals.text_cursor.blink = false);
        self.run();
    }

    /// Clears the status line, so no message from the build steps
    /// shows in a shot (the shot's own clicks may set a new one). The
    /// shell has no public setter for the status; `note_error` with an
    /// empty message is the one public way to set it, and the empty
    /// warning it records is taken back off the list at once.
    pub fn clear_status(&mut self) {
        {
            let mut shell = self.shell.borrow_mut();
            let before = shell.app.warnings.len();
            shell.note_error("");
            if shell.app.warnings.len() > before {
                shell.app.warnings.pop();
            }
        }
        self.settle(2);
        assert_eq!(self.shell.borrow().status(), "", "the status is clear");
    }

    /// The rect of every widget named `name` on screen this frame.
    pub fn rects_named(&mut self, name: &str) -> Vec<egui::Rect> {
        self.run();
        self.harness()
            .query_all_by_label(name)
            .map(|node| node.rect())
            .collect()
    }

    /// Whether a widget named `name` is on screen this frame.
    pub fn has(&mut self, name: &str) -> bool {
        !self.rects_named(name).is_empty()
    }

    /// The Visio id of the selection's primary shape, if any.
    pub fn anchor_visio_id(&self) -> Option<i32> {
        let shell = self.shell.borrow();
        let session = shell.app.active_document()?;
        let anchor = shell.app.selection.anchor?;
        session.document.shape(anchor).map(|shape| shape.visio_id)
    }

    /// Whether the primary pointer button is held down.
    pub fn pointer_down(&mut self) -> bool {
        self.harness()
            .ctx
            .input(|input| input.pointer.primary_down())
    }

    /// Clicks the first widget named `name` (the widget seam).
    pub fn click_named(&mut self, name: &str) {
        self.run();
        let pos = self
            .harness()
            .query_all_by_label(name)
            .next()
            .unwrap_or_else(|| panic!("no widget named {name:?}"))
            .rect()
            .center();
        click_at(self, pos, egui::PointerButton::Primary);
    }
}

// --- raw input -----------------------------------------------------

/// A press and a release in one frame at `pos` (one harness run steps
/// past egui's click time, so the two halves must share a frame for
/// the click to register).
pub fn click_at(rig: &mut Rig, pos: egui::Pos2, button: egui::PointerButton) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    rig.run();
}

/// A double click at `pos` (two clicks inside one double-click
/// window; the wait first drains any earlier window).
pub fn double_click_at(rig: &mut Rig, pos: egui::Pos2) {
    rig.settle(60);
    click_at(rig, pos, egui::PointerButton::Primary);
    click_at(rig, pos, egui::PointerButton::Primary);
}

/// A full drag from `from` to `to` (press, moves over frames,
/// release).
pub fn drag(rig: &mut Rig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
    move_while_held(rig, from, to);
    rig.harness().event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

/// Presses at `from` and moves to `to` in four steps without ever
/// releasing the pointer (the mid-drag screenshot states).
pub fn drag_and_hold(rig: &mut Rig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
    move_while_held(rig, from, to);
}

/// The four intermediate moves of a held drag.
fn move_while_held(rig: &mut Rig, from: egui::Pos2, to: egui::Pos2) {
    for step in 1..=4 {
        let pos = from + (to - from) * (step as f32 / 4.0);
        rig.harness().event(egui::Event::PointerMoved(pos));
        rig.run();
    }
}

/// Scrolls the area under `pos` by `lines` wheel lines.
pub fn wheel_lines(rig: &mut Rig, pos: egui::Pos2, lines: f32) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    rig.harness().event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, lines),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}
