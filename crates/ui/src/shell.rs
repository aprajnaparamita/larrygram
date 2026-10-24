//! The application shell (ADR 0029, ADR 0031): hosts the command bar, the
//! canvas, the panes and the status bar, and dispatches every [`Intent`]
//! through the command registry.
//!
//! The frame (top to bottom) is: a title strip with the quick access
//! toolbar, the menu bar (File, View, Help), the command bar, the canvas
//! area (the Shapes window on the left, the Shape Data and Size &
//! Position panes on the right, page tabs and the status bar below),
//! and the File backstage over the canvas when it is open. Widget
//! failures are shown in the status bar and never crash the frame
//! (AGENTS.md section 6).
//!
//! [`Shell::dispatch`] is the interactive dispatcher (ADR 0028): a
//! [`CommandError::NeedsInput`] from an interactive source is answered
//! through the [`FilePicker`](crate::dialogs) seam (ADR 0036) and the
//! same command is dispatched again with the value filled in;
//! programmatic sources get the error back untouched and no dialog
//! ever opens (AGENTS.md section 6).
//!
//! Owned by task P4-03.

use egui::{Context, PointerButton, Ui};
use larrygram_commands::{
    ActiveTool, AppCtx, ArgValue, CommandError, CommandId, Invocation, NeededInput, Outcome,
    PathPurpose, Payload, Registry, SessionId, Source, UiRequest,
};
use larrygram_commands::DocumentSession;
use larrygram_model::{PageId, ShapeId, TextContent, TextPart};
use larrygram_render::Overlay;
use larrygram_ui_spec::{LayoutKind, LayoutSet, LayoutSets, ShortcutScope, Shortcuts};

use crate::canvas::{self, CanvasCache};
use crate::contexts::{self, HitTarget, SelectionContext};
use crate::dialogs::{
    FileChoice, FilePicker, NativeFilePicker, OpenRequest, PickState, SaveRequest,
};
use crate::handles;
use crate::lists::{ListProvider, LiveLists};
use crate::panel_classic::{self, PanelClassic, PanelInput, ToolLatch};
use crate::panel_modern::{ListData, PanelModern, metrics::Metrics};
use crate::panes;
use crate::settings;
use crate::shapes_window::{self, ShapesWindow};
use crate::shortcuts::{self, ShortcutMap};
use crate::tab_bar::TabBar;
use crate::text_edit::{EditorAction, TextEditor};
use crate::tools::{Tool, ToolMachine, stencil};
use crate::{Intent, UiError, about, theme, wizard};

/// The File menu items (registry ids, in order).
const FILE_MENU: &[&str] = &[
    "file.new",
    "file.open",
    "file.save",
    "file.save_as",
    "file.close",
    "app.quit",
];

/// The View menu items (registry ids, in order). The four layout ids
/// render as radio items (ADR 0032, task P7-09).
const VIEW_MENU: &[&str] = &[
    "view.zoom.in",
    "view.zoom.out",
    "view.zoom.fit",
    "view.toggle_grid",
    "view.rulers",
    "view.ui_layout.panel_modern",
    "view.ui_layout.panel_classic",
    "view.ui_layout.panel_2016",
    "view.ui_layout.panel_365",
    "app.office.center.options",
];

/// The Help menu items (registry ids, in order). Every layout offers
/// the wizard and the About window (ADR 0066, task P7-09).
const HELP_MENU: &[&str] = &["help.welcome", "help.about"];

/// The zoom range of the status-bar slider (the Phase 4 range: 10%
/// to 6400%, P4-05).
const MIN_ZOOM: f64 = 0.1;

/// The largest zoom of the status-bar slider (see [`MIN_ZOOM`]).
const MAX_ZOOM: f64 = 64.0;

/// The file extensions the picker accepts (ADR 0036).
const OPEN_EXTENSIONS: &[&str] = &["vsdx", "vssx", "vstx"];

/// The default file name of a save dialog for a never-saved document.
const DEFAULT_SAVE_NAME: &str = "drawing.vsdx";

/// Maps the commands-layer active tool to the UI's [`Tool`] (L4 cannot
/// depend on L6, so the enum lives on both sides; task P5-17).
fn tool_for(active: ActiveTool) -> Tool {
    match active {
        ActiveTool::Pointer => Tool::Pointer,
        ActiveTool::Rectangle => Tool::Rectangle,
        ActiveTool::Ellipse => Tool::Ellipse,
        ActiveTool::Line => Tool::Line,
        ActiveTool::Connector => Tool::Connector,
        ActiveTool::Text => Tool::Text,
        _ => Tool::Pointer,
    }
}

/// The application shell (one per window, single window in Phase 4).
pub struct Shell {
    /// The command registry every intent dispatches through (ADR 0028).
    pub registry: Registry,
    /// Both Panel layouts, loaded from `ui-spec` (ADR 0031).
    pub layouts: LayoutSets,
    /// The application state (ADR 0029).
    pub app: AppCtx,
    /// The command-bar widget (its tab and backstage state, ADR 0031).
    tab_bar: TabBar,
    /// The Panel Classic chrome (menus and toolbars, ADR 0061); also
    /// the context-menu widget of every layout (task P7-09).
    panel_classic: PanelClassic,
    /// The Panel Modern chrome (tab strip, command bar, QAT and File
    /// view, ADR 0061).
    panel_modern: PanelModern,
    /// The layout kind the widgets were built for (they rebuild when
    /// it changes; ADR 0032, task P7-09).
    drawn_kind: Option<LayoutKind>,
    /// The active window kind (drawing, ShapeSheet or stencil), for
    /// the Panel Classic menu bars and the contextual tabs (ADR 0064,
    /// task P7-09 Goal 4).
    window_mode: contexts::WindowMode,
    /// The open context menu's layout id (task P7-09 Goal 4).
    context_menu_id: Option<String>,
    /// The Shapes window widget (ADR 0034).
    shapes: ShapesWindow,
    /// The canvas raster cache (ADR 0033).
    cache: CanvasCache,
    /// The dialog seam (ADR 0036); tests inject a fake picker.
    picker: Box<dyn FilePicker>,
    /// The keyboard shortcut map from `shortcuts.yaml`, dispatched
    /// before the widgets (ADR 0036, P4-14).
    shortcuts: ShortcutMap,
    /// The surface the keyboard shortcuts apply to (ADR 0036 scopes,
    /// P4-14 focus tracking).
    focus: ShortcutScope,
    /// The canvas area of the previous frame (focus tracking).
    canvas_rect: Option<egui::Rect>,
    /// The canvas widget id of the previous frame (keyboard focus).
    canvas_id: Option<egui::Id>,
    /// The session and page of the previous frame (page-switch selection
    /// clearing, ADR 0041).
    last_page: Option<(SessionId, Option<PageId>)>,
    /// The pointer tool state machine (ADR 0042).
    tools: ToolMachine,
    /// The in-place text editor (ADR 0044, task P5-17).
    editor: TextEditor,
    /// Whether a synthetic `PointerDown` reached the stencil tool for the
    /// current Shapes-window drag.
    stencil_down: bool,
    /// Escape was pressed this frame while the editor or a stencil drag
    /// owns it (the shell consumes it before the shortcut table, whose
    /// `Esc` binding would otherwise win; ADR 0044).
    escape_pressed: bool,
    /// The transient overlay marks drawn over the cached raster.
    overlay: Overlay,
    /// The command-bar strip of the previous frame (focus tracking).
    tab_bar_rect: Option<egui::Rect>,
    /// A command waiting for an asynchronous pick (the wasm upload,
    /// ADR 0036).
    pending: Option<(CommandId, Invocation)>,
    /// The last command message, shown in the title strip.
    status: String,
    /// Whether the View/Options dialog is open.
    options_open: bool,
    /// Whether the About window is open.
    about_open: bool,
    /// The welcome wizard (ADR 0066): `Some` while it is open.
    wizard: Option<wizard::State>,
    /// The first launch has not shown the wizard yet (ADR 0066).
    welcome_pending: bool,
    /// The document name awaiting the close confirmation (ADR 0028
    /// `ConfirmClose`).
    confirm_close: Option<String>,
    /// The window was asked to close; sent to the viewport in
    /// [`Shell::show_in`].
    quit_requested: bool,
}

impl Shell {
    /// Builds a shell from its three owned parts.
    ///
    /// The widget state, the canvas cache, the native file picker and
    /// the keyboard shortcut map are created here (ADR 0029:
    /// widget-local state lives in `crates/ui`; ADR 0036: the map is
    /// built from the loader's `shortcuts.yaml`, the same embedded
    /// source the command bar uses). `Shell::new`'s signature is fixed; a
    /// table that fails to load falls back to an empty map with one
    /// warning, and [`Shell::set_shortcuts`] replaces the map later.
    pub fn new(registry: Registry, layouts: LayoutSets, app: AppCtx) -> Self {
        let mut app = app;
        let welcome_pending = !app.settings.welcome_seen;
        let (shortcuts, warnings) = load_shortcuts(&layouts.get(app.settings.ui_layout).shortcuts);
        for warning in warnings {
            app.record_warning(warning);
        }
        let tab_bar = TabBar::new(app.settings.ui_layout);
        Self {
            registry,
            layouts,
            app,
            tab_bar,
            panel_classic: PanelClassic::new(),
            panel_modern: PanelModern::new(),
            drawn_kind: None,
            window_mode: contexts::WindowMode::Drawing,
            context_menu_id: None,
            shapes: ShapesWindow::new(),
            cache: CanvasCache::new(),
            picker: Box::new(NativeFilePicker),
            shortcuts,
            focus: ShortcutScope::Global,
            canvas_rect: None,
            canvas_id: None,
            last_page: None,
            tab_bar_rect: None,
            tools: ToolMachine::new(),
            editor: TextEditor::new(),
            stencil_down: false,
            escape_pressed: false,
            overlay: Overlay::new(),
            pending: None,
            status: String::new(),
            options_open: false,
            about_open: false,
            wizard: None,
            welcome_pending,
            confirm_close: None,
            quit_requested: false,
        }
    }

    /// The active shortcut scope (the keyboard focus, ADR 0036/0041).
    pub fn focus_scope(&self) -> ShortcutScope {
        self.focus
    }

    /// The active Panel layout (ADR 0032).
    pub fn layout_kind(&self) -> LayoutKind {
        self.app.settings.ui_layout
    }

    /// Switches the Panel layout without a restart (ADR 0032). The
    /// default layout follows the active one (task P7-09 Goal 6).
    pub fn set_layout_kind(&mut self, kind: LayoutKind) {
        self.app.settings.ui_layout = kind;
        self.app.settings.default_layout = kind;
    }

    /// The layout set for the active kind (ADR 0031): the curated v2
    /// files for Panel Modern and Panel Classic, the v1 data plus the
    /// [`v2::from_v1`](larrygram_ui_spec::v2::from_v1) conversion for
    /// the two Fluent-data kinds (task P7-09).
    pub fn layout_set(&self) -> &LayoutSet {
        self.layouts.get(self.layout_kind())
    }

    /// The active window kind (drawing, ShapeSheet or stencil). A
    /// change swaps the Panel Classic menu bar and its toolbars
    /// (task P7-09 Goal 4).
    pub fn set_window_mode(&mut self, mode: contexts::WindowMode) {
        self.window_mode = mode;
    }

    /// The last command message (the title strip shows it).
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The canvas area of the previous frame (tests and the editor
    /// placement; task P5-17).
    pub fn canvas_rect(&self) -> Option<egui::Rect> {
        self.canvas_rect
    }

    /// The commands-layer active tool (task P5-17).
    pub fn active_tool(&self) -> ActiveTool {
        self.app.active_tool
    }

    /// The status line shown in the title strip: the chords typed so
    /// far while a shortcut sequence is half-typed (ADR 0036, P4-14),
    /// else the last command message.
    pub fn status_line(&self) -> String {
        if self.shortcuts.pending() {
            self.shortcuts.pending_display()
        } else {
            self.status.clone()
        }
    }

    /// Replaces the shortcut map (tests, and a future layout switch):
    /// the same `shortcuts.yaml` load rules as in [`Shell::new`], with
    /// the load warnings copied into `AppCtx::warnings` (ADR 0036).
    pub fn set_shortcuts(&mut self, shortcuts: &Shortcuts) {
        let (map, warnings) = load_shortcuts(shortcuts);
        for warning in warnings {
            self.app.record_warning(warning);
        }
        self.shortcuts = map;
    }

    /// Whether the window was asked to close (File > Exit, ADR 0028).
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Replaces the dialog seam (tests, and the web app's picker).
    pub fn set_picker(&mut self, picker: Box<dyn FilePicker>) {
        self.picker = picker;
    }

    /// Records a widget or dispatch failure for the status bar. A
    /// failure never crashes the frame (AGENTS.md section 6).
    pub fn note_error(&mut self, error: impl Into<String>) {
        let message = error.into();
        if self.app.warnings.last() != Some(&message) {
            self.app.record_warning(message);
        }
    }

    /// Records which surface the pointer pressed last (the ADR 0036
    /// shortcut scopes): the canvas area is `Page`, the command-bar strip
    /// is `TabBar`, and any other press falls back to `Global`. This
    /// is the smallest focus tracking that gives the three scopes
    /// (P4-14): the rects are the ones the previous frame drew, so a
    /// press takes effect in its own frame, before the widgets draw.
    fn note_pointer_focus(&mut self, egui_ctx: &Context) {
        let pressed = egui_ctx.input(|input| {
            if input.pointer.button_pressed(PointerButton::Primary) {
                input.pointer.interact_pos()
            } else {
                None
            }
        });
        if let Some(pos) = pressed {
            if self.canvas_rect.is_some_and(|rect| rect.contains(pos)) {
                self.focus = ShortcutScope::Page;
            } else if self.tab_bar_rect.is_some_and(|rect| rect.contains(pos)) {
                self.focus = ShortcutScope::CommandBar;
            } else {
                self.focus = ShortcutScope::Global;
            }
            return;
        }
        // P4-14 fix (ADR 0041 rule 4): a keyboard-only focus move (Tab)
        // changes the scope too. The canvas widget reports its id from
        // the previous frame.
        if let Some(focused) = egui_ctx.memory(|memory| memory.focused())
            && self.canvas_id == Some(focused)
        {
            self.focus = ShortcutScope::Page;
        }
    }

    /// Draws one frame into `ui` and returns the intents (ADR 0031).
    ///
    /// This is the fixed P4-01 entry point. egui 0.37 hands the root
    /// [`Ui`] to `eframe::App::ui`, and [`Context::root_ui`] panics
    /// while that ui is borrowed (the never-crash rule of AGENTS.md
    /// section 6, P4-16 review minor 10), so the callers pass the ui
    /// they were given.
    ///
    /// The frame is built from the active layout's `window.regions`
    /// (ADR 0061, task P7-09): Panel Classic draws its menu bars and
    /// toolbars, Panel Modern its title bar, tab strip and command
    /// bar, and the two Fluent-data kinds the Phase 4 command bar.
    /// The keyboard shortcut pass runs before the widgets (ADR 0036,
    /// P4-14) and its intents join the widget intents. A widget
    /// failure goes to the status bar and the frame still renders.
    pub fn show_in(&mut self, ui: &mut Ui) -> Result<Vec<Intent>, UiError> {
        let mut intents = Vec::new();
        self.clear_selection_on_page_switch();
        self.finish_pick();
        self.sync_active_tool();
        self.capture_escape(ui.ctx());
        if self.quit_requested {
            self.quit_requested = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // The default layout follows the active one (task P7-09 Goal
        // 6) and the first launch opens the welcome wizard before any
        // document is drawn (ADR 0066).
        self.app.settings.default_layout = self.app.settings.ui_layout;
        if self.welcome_pending {
            self.welcome_pending = false;
            self.wizard = Some(wizard::State::new());
        }
        self.sync_layout_widgets(ui.ctx());

        // Keyboard shortcuts first, before the widgets draw (ADR 0036
        // "Precedence over egui"): the intent joins the frame's other
        // intents and runs through `Shell::dispatch` like any other
        // (`Source::Shortcut`).
        self.note_pointer_focus(ui.ctx());
        if self.editor.is_active() {
            // The in-place editor keeps the `Text` scope active until it
            // closes (ADR 0036, ADR 0044).
            self.focus = ShortcutScope::Text;
        }
        match shortcuts::dispatch(ui.ctx(), &mut self.shortcuts, &self.app, self.focus) {
            Ok(produced) => intents.extend(produced),
            Err(error) => self.note_error(error.to_string()),
        }

        match self.app.settings.ui_layout {
            LayoutKind::PanelClassic => self.show_classic_frame(ui, &mut intents),
            LayoutKind::PanelModern => self.show_modern_frame(ui, &mut intents),
            LayoutKind::Panel2016 | LayoutKind::Panel365 => {
                self.show_fluent_frame(ui, &mut intents);
            }
        }
        self.draw_context_menus(ui, &mut intents);
        self.show_dialogs(ui, &mut intents);
        self.record_layout_state();
        Ok(intents)
    }

    /// Rebuilds the per-layout widgets when the layout kind changed
    /// since the last frame (ADR 0032 "rebuilds on the next frame")
    /// and applies the active theme (ADR 0063). The rebuilt widgets
    /// start from the per-layout state kept in [`Settings`]
    /// (larrygram_commands) (task P7-09 Goal 3).
    fn sync_layout_widgets(&mut self, ctx: &Context) {
        let kind = self.app.settings.ui_layout;
        let theme = self.layouts.get(kind).theme.clone();
        if theme::current(ctx).as_ref() != Some(&theme)
            && let Err(error) = theme::apply(ctx, &theme)
        {
            self.note_error(error.to_string());
        }
        if self.drawn_kind == Some(kind) {
            return;
        }
        self.drawn_kind = Some(kind);
        self.tab_bar.set_kind(kind);
        let context = self.selection_context();
        let set = self.layouts.get(kind);
        self.panel_modern = PanelModern::with_metrics(Metrics::from_theme(&set.theme));
        self.panel_modern
            .set_collapsed(self.app.settings.command_bar_collapsed);
        if let Some(id) = self.app.settings.last_core_tab.clone() {
            self.panel_modern.set_active_core_id(&set.v2, context, &id);
        }
        self.panel_classic = PanelClassic::new();
        for (key, visible) in self.app.settings.panel_classic_toolbars.clone() {
            if let Some((mode, toolbar)) = split_toolbar_key(&key) {
                self.panel_classic
                    .set_toolbar_visible_in(mode, &toolbar, visible);
            }
        }
    }

    /// Copies the per-layout widget state back into [`Settings`]
    /// (larrygram_commands) (task P7-09 Goal 3), so the store keeps
    /// the toolbar choices, the command-bar collapse and the last
    /// core tab across launches.
    fn record_layout_state(&mut self) {
        let kind = self.app.settings.ui_layout;
        self.app.settings.default_layout = kind;
        self.app.settings.command_bar_collapsed = self.panel_modern.is_collapsed();
        let context = self.selection_context();
        let set = self.layouts.get(kind);
        if let Some(id) = self.panel_modern.active_core_id(&set.v2, context) {
            self.app.settings.last_core_tab = Some(id);
        }
        if kind == LayoutKind::PanelClassic {
            for toolbar in &set.v2.toolbars {
                for mode in [
                    panel_classic::WindowMode::Drawing,
                    panel_classic::WindowMode::Shapesheet,
                    panel_classic::WindowMode::Stencil,
                ] {
                    let visible = self.panel_classic.toolbar_visible_in(&set.v2, mode, &toolbar.id);
                    self.app.settings.panel_classic_toolbars.insert(
                        format!("{}/{}", mode.id(), toolbar.id),
                        visible,
                    );
                }
            }
        }
    }

    /// The Panel Classic window mode of the active window (task P7-09
    /// Goal 4: a window-mode change swaps the menu bar and toolbars).
    fn classic_window_mode(&self) -> panel_classic::WindowMode {
        match self.window_mode {
            contexts::WindowMode::Drawing => panel_classic::WindowMode::Drawing,
            contexts::WindowMode::Shapesheet => panel_classic::WindowMode::Shapesheet,
            contexts::WindowMode::Stencil => panel_classic::WindowMode::Stencil,
        }
    }

    /// The selection context for the command bar's contextual tabs
    /// (ADR 0064) and the Panel Classic window mode (task P7-09
    /// Goal 4).
    fn selection_context(&self) -> SelectionContext {
        contexts::current_with(
            &self.app,
            contexts::ContextState {
                text_editing: self.editor.is_active(),
                window: self.window_mode,
            },
        )
    }

    /// The run-time lists for the Panel Modern widgets, filled from
    /// [`LiveLists`] (task P7-09 wiring of the P7-06 providers).
    fn list_data(&self) -> ListData {
        let live = LiveLists;
        let mut lists = ListData::new();
        for name in ["recent_files", "windows", "pages"] {
            let entries = live.list(name, &self.app);
            lists.insert(
                name,
                entries.into_iter().map(|entry| entry.label).collect(),
            );
        }
        lists
    }

    // --- the frames (ADR 0061 window regions) ----------------------

    /// The Panel Classic frame: title bar, menu bars and toolbars,
    /// the Shapes window, the drawing with its page-tab strip and the
    /// status bar (docs/ui/panel-classic.md, "Window").
    fn show_classic_frame(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        // Title bar (23 px): the document name and the app name.
        egui::Panel::top("shell-title").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.title_text());
                ui.separator();
                ui.label("Larrygram");
                ui.separator();
                ui.label(self.status_line());
            });
        });
        // The `menu_bar` and `toolbars` regions: one chrome (the
        // command-bar focus scope, ADR 0036).
        egui::Panel::top("shell-chrome").show(ui, |ui| {
            self.tab_bar_rect = Some(ui.max_rect());
            let mut produced = Vec::new();
            let mut errors = Vec::new();
            {
                let kind = self.app.settings.ui_layout;
                let set = self.layouts.get(kind);
                let mode = self.classic_window_mode();
                let live = LiveLists;
                let input = PanelInput::new(&set.v2, &self.registry, &self.app)
                    .with_mode(mode)
                    .with_lists(&live)
                    .with_checked(&ToolLatch);
                match self.panel_classic.show(ui, &input) {
                    Ok(mut more) => produced.append(&mut more),
                    Err(error) => errors.push(error.to_string()),
                }
            }
            for error in errors {
                self.note_error(error);
            }
            intents.append(&mut produced);
        });
        self.show_dock_panels(ui, intents, FrameStyle::Classic);
        self.show_bottom_panels(ui, intents, FrameStyle::Classic);
        egui::CentralPanel::default().show(ui, |ui| {
            self.show_canvas_area(ui, intents);
            self.show_editor(ui, intents);
        });
    }

    /// The Panel Modern frame: the title bar with the Quick Access
    /// Toolbar and the help menu, the tab strip and command bar, the
    /// Shapes window, the drawing with the page tabs and the status
    /// bar with the zoom controls (docs/ui/panel-modern.md,
    /// "Window"). The File view covers the frame when it is open.
    fn show_modern_frame(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let context = self.selection_context();
        let lists = self.list_data();
        // Title bar (25 px): QAT left, document title, help right.
        egui::Panel::top("shell-title").show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut produced = Vec::new();
                let mut errors = Vec::new();
                {
                    let kind = self.app.settings.ui_layout;
                    let set = self.layouts.get(kind);
                    match self.panel_modern.show_qat(ui, &set.v2, &self.app, &self.registry) {
                        Ok(mut more) => produced.append(&mut more),
                        Err(error) => errors.push(error.to_string()),
                    }
                }
                ui.separator();
                ui.label(self.title_text());
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.menu_button("Help", |ui| {
                            for id in ["help.welcome", "help.about"] {
                                if let Some(intent) =
                                    menu_item(ui, &self.registry, &self.app, id, None)
                                {
                                    produced.push(intent);
                                    ui.close();
                                }
                            }
                        });
                    },
                );
                for error in errors {
                    self.note_error(error);
                }
                intents.append(&mut produced);
            });
        });
        if self.panel_modern.is_file_view_open() {
            // The File view covers the frame below the title bar (the
            // Browse, Print and Export rows dispatch `file.open`,
            // `file.save_as`, `file.print` and `file.export`; the
            // shell gives those their dialog behaviour through
            // `Shell::dispatch`).
            egui::CentralPanel::default().show(ui, |ui| {
                self.draw_modern_panel(ui, &lists, context, intents);
            });
            return;
        }
        // Tab strip and command bar (the `tab_strip` and `command_bar`
        // regions), one chrome (the command-bar focus scope, ADR 0036).
        egui::Panel::top("shell-chrome").show(ui, |ui| {
            self.tab_bar_rect = Some(ui.max_rect());
            self.draw_modern_panel(ui, &lists, context, intents);
        });
        self.show_dock_panels(ui, intents, FrameStyle::Modern);
        self.show_bottom_panels(ui, intents, FrameStyle::Modern);
        egui::CentralPanel::default().show(ui, |ui| {
            self.show_canvas_area(ui, intents);
            self.show_editor(ui, intents);
        });
    }

    /// One `PanelModern::show` pass (the tab strip and command bar,
    /// or the File view while it is open).
    fn draw_modern_panel(
        &mut self,
        ui: &mut Ui,
        lists: &ListData,
        context: SelectionContext,
        intents: &mut Vec<Intent>,
    ) {
        let kind = self.app.settings.ui_layout;
        let set = self.layouts.get(kind);
        match self
            .panel_modern
            .show(ui, &set.v2, context, &self.app, lists, &self.registry)
        {
            Ok(mut more) => intents.append(&mut more),
            Err(error) => self.note_error(error.to_string()),
        }
    }

    /// The Phase 4 frame of the two Fluent-data kinds: the title
    /// strip with the quick access toolbar, the File/View/Help menu
    /// bar, the command bar over the backstage or the canvas.
    fn show_fluent_frame(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        // Title strip: document title, status line, quick access
        // toolbar (ADR 0031).
        egui::Panel::top("shell-title").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Larrygram");
                ui.separator();
                ui.label(self.title_text());
                ui.separator();
                ui.label(self.status_line());
                let kind = self.app.settings.ui_layout;
                let set = self.layouts.get(kind);
                let produced = self
                    .tab_bar
                    .show_quick_access(ui, set, &self.registry, &self.app);
                self.absorb(produced, intents);
            });
        });

        // Menu bar (File, View, Help).
        egui::Panel::top("shell-menu").show(ui, |ui| {
            egui::menu::MenuBar::default().ui(ui, |ui| {
                show_menu(
                    ui,
                    &self.registry,
                    &self.app,
                    "File",
                    FILE_MENU,
                    intents,
                );
                show_menu(
                    ui,
                    &self.registry,
                    &self.app,
                    "View",
                    VIEW_MENU,
                    intents,
                );
                show_menu(
                    ui,
                    &self.registry,
                    &self.app,
                    "Help",
                    HELP_MENU,
                    intents,
                );
            });
        });

        // The command bar (or the backstage over the canvas, ADR 0031).
        let kind = self.app.settings.ui_layout;
        egui::Panel::top("shell-command-bar").show(ui, |ui| {
            self.tab_bar_rect = Some(ui.max_rect());
            let set = self.layouts.get(kind);
            let produced = self.tab_bar.show(ui, set, &self.registry, &self.app);
            self.absorb(produced, intents);
        });

        self.show_dock_panels(ui, intents, FrameStyle::Fluent);
        self.show_bottom_panels(ui, intents, FrameStyle::Fluent);
        egui::CentralPanel::default().show(ui, |ui| {
            if self.tab_bar.backstage_open() {
                let set = self.layouts.get(kind);
                let produced = self
                    .tab_bar
                    .show_backstage(ui, set, &self.registry, &self.app);
                let opened = matches!(&produced, Ok(intents) if !intents.is_empty());
                self.absorb(produced, intents);
                if opened {
                    // A backstage command runs: leave the File view
                    // (ADR 0031).
                    self.tab_bar.set_backstage_open(false);
                }
            } else {
                self.show_canvas_area(ui, intents);
                self.show_editor(ui, intents);
            }
        });
    }

    /// The docked side windows: the Shapes window on the left and the
    /// Shape Data and Size & Position panes on the right.
    fn show_dock_panels(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>, style: FrameStyle) {
        let shapes_width = match style {
            FrameStyle::Classic => 190.0,
            FrameStyle::Modern => 262.0,
            FrameStyle::Fluent => 200.0,
        };
        egui::Panel::left("shell-shapes")
            .resizable(true)
            .default_size(shapes_width)
            .show(ui, |ui| {
                let produced = self.shapes.show(ui, &self.app, style.shapes_header());
                if let Some(_picks) = self.keep(produced) {
                    // Picking a stencil inserts a shape in Phase 5;
                    // the Shapes window keeps the selection itself.
                }
                if let Some(pick) = self.shapes.take_drag_started() {
                    stencil::begin_drag(&mut self.tools, pick.stencil, pick.master);
                    self.stencil_down = false;
                }
                // Right-clicking the window opens its context menu
                // (task P7-09 Goal 4, `contexts::menu_for`).
                if ui.input(|input| input.pointer.secondary_clicked()) {
                    self.open_context_menu(HitTarget::ShapesWindow);
                }
            });
        egui::Panel::right("shell-panes")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                let shape_data = panes::show_shape_data(ui, &mut self.app);
                if let Some(mut more) = self.keep(shape_data) {
                    intents.append(&mut more);
                }
                let size_position = panes::show_size_position(ui, &mut self.app);
                if let Some(mut more) = self.keep(size_position) {
                    intents.append(&mut more);
                }
            });
    }

    /// The status bar and the page-tab strip below the drawing, in
    /// the active layout's look (the `status_bar` and `page_tabs`
    /// regions).
    fn show_bottom_panels(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>, style: FrameStyle) {
        egui::Panel::bottom("shell-status").show(ui, |ui| match style {
            FrameStyle::Fluent => {
                let produced = canvas::show_status_bar(ui, &self.app);
                let _ = self.keep(produced);
            }
            FrameStyle::Modern => {
                let mut more = self.show_modern_status(ui);
                intents.append(&mut more);
            }
            FrameStyle::Classic => self.show_classic_status(ui),
        });
        egui::Panel::bottom("shell-tabs").show(ui, |ui| {
            if self.app.active.is_some() {
                let session = self.app.active_document_mut();
                if let Some(session) = session {
                    let produced = match style {
                        FrameStyle::Fluent => canvas::show_page_tabs(ui, session),
                        FrameStyle::Modern => modern_page_tabs(ui, session),
                        FrameStyle::Classic => classic_page_tabs(ui, session),
                    };
                    match produced {
                        Ok(mut more) => intents.append(&mut more),
                        Err(error) => self.note_error(error.to_string()),
                    }
                }
            }
        });
    }

    /// The Panel Modern status bar: "Page n of m" and the message on
    /// the left, the presentation button and the zoom controls on the
    /// right (docs/ui/panel-modern.md, "Window"). The switch-window
    /// button has no registry id yet (`window.activate`, listed in
    /// the task's "not yours" set) and draws disabled.
    fn show_modern_status(&mut self, ui: &mut Ui) -> Vec<Intent> {
        let mut produced = Vec::new();
        ui.horizontal(|ui| {
            ui.label(self.page_count_label("of"));
            ui.separator();
            ui.label(self.status_line());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(false, |ui| ui.button("Switch window"))
                    .inner
                    .on_disabled_hover_text("Not yet implemented");
                if let Some(intent) =
                    menu_item(ui, &self.registry, &self.app, "view.zoom.fit", None)
                {
                    produced.push(intent);
                }
                self.zoom_controls(ui, &mut produced);
                if let Some(intent) =
                    menu_item(ui, &self.registry, &self.app, "view.full_screen_mode", None)
                {
                    produced.push(intent);
                }
            });
        });
        produced
    }

    /// The Panel Classic status bar: the width, height and angle of
    /// the selection on the left, "Page n/m" on the right
    /// (docs/ui/panel-classic.md, "Window").
    fn show_classic_status(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Ok(Some((bounds, angle))) = handles::selection_frame(&self.app, None) {
                let units = self.app.settings.units;
                ui.label(format!("W {}", canvas::format_length(bounds.width(), units)));
                ui.label(format!("H {}", canvas::format_length(bounds.height(), units)));
                ui.label(format!("A {:.0}\u{b0}", angle.degrees()));
            }
            ui.separator();
            ui.label(self.status_line());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(self.page_count_label("/"));
            });
        });
    }

    /// The zoom controls of the Panel Modern status bar: the slider,
    /// the minus and plus buttons, the percentage and fit-to-window.
    fn zoom_controls(&mut self, ui: &mut Ui, produced: &mut Vec<Intent>) {
        let mut zoom = self
            .app
            .active_document()
            .map(|session| session.view.zoom)
            .unwrap_or(1.0);
        if let Some(intent) = menu_item(ui, &self.registry, &self.app, "view.zoom.out", None) {
            produced.push(intent);
        }
        let response = ui.add(
            egui::Slider::new(&mut zoom, MIN_ZOOM..=MAX_ZOOM)
                .logarithmic(true)
                .show_value(false),
        );
        let changed = response.dragged() || response.changed();
        if let Some(intent) = menu_item(ui, &self.registry, &self.app, "view.zoom.in", None) {
            produced.push(intent);
        }
        ui.label(format!("{:.0}%", zoom * 100.0));
        if changed && let Some(session) = self.app.active_document_mut() {
            session.view.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    /// "Page n of m" with `join` between the numbers ("of" for Panel
    /// Modern, "/" for Panel Classic).
    fn page_count_label(&self, join: &str) -> String {
        let (current, total) = match self.app.active_document() {
            Some(session) => {
                let total = session.document.pages.len();
                let current = session
                    .active_page
                    .and_then(|page| {
                        session
                            .document
                            .pages
                            .iter()
                            .position(|entry| entry.id == page)
                    })
                    .unwrap_or(0)
                    + 1;
                (current, total)
            }
            None => (0, 0),
        };
        if join == "of" {
            format!("PAGE {current} {join} {}", total.max(1)).to_uppercase()
        } else {
            format!("Page {current}{join}{}", total.max(1))
        }
    }

    /// Opens the context menu for a right-clicked target (task P7-09
    /// Goal 4): [`contexts::menu_for`] maps the hit target to the
    /// active layout's menu, and the Panel Classic menu widget draws
    /// it in every layout.
    fn open_context_menu(&mut self, target: HitTarget) {
        let kind = self.app.settings.ui_layout;
        let set = self.layouts.get(kind);
        if let Some(menu) = contexts::menu_for(&set.v2, target) {
            let id = menu.id.clone();
            self.context_menu_id = Some(id.clone());
            self.panel_classic.open_context_menu(id);
        }
    }

    /// Draws the open context menu over the frame (task P7-09
    /// Goal 4). The menu closes on Esc or a click outside (the
    /// widget's own rule).
    fn draw_context_menus(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let Some(menu_id) = self.context_menu_id.clone() else {
            return;
        };
        let kind = self.app.settings.ui_layout;
        let set = self.layouts.get(kind);
        let mode = self.classic_window_mode();
        let live = LiveLists;
        let input = PanelInput::new(&set.v2, &self.registry, &self.app)
            .with_mode(mode)
            .with_lists(&live)
            .with_checked(&ToolLatch);
        match self.panel_classic.context_menu(ui, &input, &menu_id, None) {
            Ok(mut more) => intents.append(&mut more),
            Err(error) => self.note_error(error.to_string()),
        }
    }

    /// The canvas area: the page raster with the rulers over its
    /// bands (ADR 0033), or a placeholder with no open document. The
    /// area is remembered for the shortcut focus tracking (P4-14).
    /// A right-click maps to its context menu (task P7-09 Goal 4).
    fn show_canvas_area(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let units = self.app.settings.units;
        let rect = ui.available_rect_before_wrap();
        self.canvas_rect = Some(rect);
        self.canvas_context_click(ui, rect, intents);
        if self.app.active_document().is_none() {
            ui.centered_and_justified(|ui| {
                ui.label("No document");
            });
            return;
        }
        // The two canvas functions draw into the same area (the
        // rulers paint over the canvas bands, P4-05) and their errors
        // are recorded after the document borrow ends.
        let mut produced = Vec::new();
        let mut errors = Vec::new();
        let mut events = Vec::new();
        let overlay = self.overlay.clone();
        {
            let session = self.app.active_document_mut();
            if let Some(session) = session {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    match canvas::show_with_overlay(ui, session, &mut self.cache, &overlay) {
                        Ok(output) => {
                            produced.extend(output.intents);
                            events = output.events;
                            if let Some(id) = output.focus_id {
                                self.canvas_id = Some(id);
                            }
                        }
                        Err(error) => errors.push(error.to_string()),
                    }
                });
            }
        }
        let had_events = !events.is_empty();
        if stencil::is_dragging(&self.tools) {
            self.route_stencil_drag(ui, &mut produced, &mut errors);
        } else {
            for event in events {
                match self.tools.handle(event, &self.app) {
                    Ok(effect) => {
                        self.overlay = effect.overlay;
                        produced.extend(effect.intents);
                    }
                    // A tool module that has not landed yet (P5-06..P5-08)
                    // reports its stub; the pointer tool's events never
                    // reach it while the pointer tool is active.
                    Err(UiError::NotImplemented) => {}
                    Err(error) => errors.push(error.to_string()),
                }
            }
        }
        if !had_events && !stencil::is_dragging(&self.tools) {
            match self.tools.idle_overlay(&self.app) {
                Ok(Some(overlay)) => self.overlay = overlay,
                Ok(None) => {}
                Err(error) => errors.push(error.to_string()),
            }
        }
        {
            let session = self.app.active_document();
            if let Some(session) = session {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    if let Err(error) = canvas::show_rulers(ui, session, units) {
                        errors.push(error.to_string());
                    }
                });
            }
        }
        for error in errors {
            self.note_error(error);
        }
        intents.extend(produced);
    }

    /// A right-click or Shift+F10 in the canvas area opens the
    /// context menu for the hit target (task P7-09 Goal 4): a shape
    /// under the pointer is selected first and opens `context.shape`,
    /// an empty page opens `context.page`, and text being edited
    /// opens `context.text_edit` (docs/ui/panel-classic.md).
    fn canvas_context_click(&mut self, ui: &Ui, rect: egui::Rect, intents: &mut Vec<Intent>) {
        let secondary = ui.input(|input| {
            (
                input.pointer.secondary_clicked(),
                input.pointer.interact_pos(),
            )
        });
        let shift_f10 = ui.input(|input| {
            input.modifiers.shift && input.key_pressed(egui::Key::F10)
        });
        if secondary.0 {
            let Some(pos) = secondary.1 else {
                return;
            };
            if !rect.contains(pos) {
                return;
            }
            if self.editor.is_active() {
                self.open_context_menu(HitTarget::TextEditing);
                return;
            }
            match self.shape_under(pos, rect) {
                Some(shape) => {
                    if self.app.selection.anchor != Some(shape) {
                        if let Some(intent) = select_intent(&self.app, shape) {
                            intents.push(intent);
                        }
                    }
                    self.open_context_menu(HitTarget::Shape);
                }
                None => self.open_context_menu(HitTarget::Page),
            }
        } else if shift_f10 {
            let target = if self.editor.is_active() {
                HitTarget::TextEditing
            } else if self.app.selection.is_empty() {
                HitTarget::Page
            } else {
                HitTarget::Shape
            };
            self.open_context_menu(target);
        }
    }

    /// The topmost shape under a screen point in the canvas, through
    /// the pointer tool's cached page scene (task P7-09).
    fn shape_under(&mut self, screen: egui::Pos2, rect: egui::Rect) -> Option<ShapeId> {
        let session = self.app.active_document()?;
        let page = canvas::page_position(rect, session, screen)?;
        let zoom = session.view.zoom;
        let hit = self.tools.state.scene(&self.app).ok().flatten().and_then(
            |scene| {
                let tolerance = 4.0 / (zoom * canvas::POINTS_PER_INCH);
                let hits = larrygram_render::hit_test(
                    scene,
                    page,
                    &larrygram_render::HitOptions::new(tolerance),
                )
                .ok()?;
                hits.iter().find_map(|hit| hit.shape_id)
            },
        );
        hit
    }

    /// Consumes Escape while the text editor or a stencil drag owns it
    /// (ADR 0044, task P5-17 Goal 3 and 5). The committed shortcut table
    /// binds `Esc` to full-screen mode; the editor and the drag must see
    /// it first.
    fn capture_escape(&mut self, egui_ctx: &Context) {
        let escape = egui_ctx.input(|input| input.key_pressed(egui::Key::Escape));
        self.escape_pressed =
            escape && (self.editor.is_active() || stencil::is_dragging(&self.tools));
        if self.escape_pressed {
            egui_ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        }
                    )
                });
            });
        }
    }

    /// Mirrors [`AppCtx::active_tool`] into the [`ToolMachine`] (task
    /// P5-17). A tool change cancels any drag in flight by replacing the
    /// machine (ADR 0042).
    fn sync_active_tool(&mut self) {
        let wanted = tool_for(self.app.active_tool());
        if self.tools.tool != wanted {
            self.tools = ToolMachine::new();
            self.tools.tool = wanted;
            self.stencil_down = false;
        }
    }

    /// Feeds a Shapes-window drag to the stencil tool (task P5-17,
    /// Goal 3). The drag may have started outside the canvas (on a row),
    /// so the first move inside it synthesizes the `PointerDown`. Escape
    /// and a release outside the canvas cancel the drag.
    fn route_stencil_drag(&mut self, ui: &Ui, intents: &mut Vec<Intent>, errors: &mut Vec<String>) {
        let Some(rect) = self.canvas_rect else {
            return;
        };
        let (pos, down, released, modifiers) = ui.input(|input| {
            (
                input.pointer.interact_pos(),
                input.pointer.primary_down(),
                input.pointer.button_released(PointerButton::Primary),
                input.modifiers,
            )
        });
        let escape = self.escape_pressed;
        let modifiers = crate::tools::Modifiers {
            shift: modifiers.shift,
            ctrl: modifiers.ctrl || modifiers.command,
            alt: modifiers.alt,
        };
        let mut send = |event: crate::tools::ToolEvent| match stencil::handle(
            &mut self.tools,
            &event,
            &self.app,
        ) {
            Ok(effect) => {
                self.overlay = effect.overlay;
                intents.extend(effect.intents);
            }
            Err(UiError::NotImplemented) => {}
            Err(error) => errors.push(error.to_string()),
        };
        if escape {
            send(crate::tools::ToolEvent::Cancel);
            self.stencil_down = false;
            return;
        }
        let Some(screen) = pos else {
            return;
        };
        let page = self
            .app
            .active_document()
            .and_then(|session| canvas::page_position(rect, session, screen));
        if !self.stencil_down {
            match page {
                Some(page) if down => {
                    send(crate::tools::ToolEvent::PointerDown {
                        pos: page,
                        modifiers,
                    });
                    self.stencil_down = true;
                }
                _ if released => {
                    send(crate::tools::ToolEvent::Cancel);
                    return;
                }
                _ => return,
            }
        }
        match page {
            Some(page) if released => {
                send(crate::tools::ToolEvent::PointerUp {
                    pos: page,
                    modifiers,
                });
                self.stencil_down = false;
            }
            Some(page) if down => {
                send(crate::tools::ToolEvent::PointerMove {
                    pos: page,
                    modifiers,
                });
            }
            Some(_) => {}
            None if released => {
                // Released outside the canvas: cancel.
                send(crate::tools::ToolEvent::Cancel);
                self.stencil_down = false;
            }
            // Outside the canvas while still dragging: keep the drag
            // until the pointer comes back or is released.
            None => {}
        }
    }

    /// Shows the in-place editor over the canvas and dispatches its
    /// commit (ADR 0044, task P5-17 Goal 5). Escape and an outside click
    /// commit; an unchanged buffer dispatches nothing.
    fn show_editor(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let Some(rect) = self.canvas_rect else {
            return;
        };
        if !self.editor.is_active() {
            return;
        }
        let shape = self.editor.shape;
        let action = if self.escape_pressed {
            // Escape commits (ADR 0044). The shell took it from the
            // shortcut table before the editor's own input saw it.
            match self.editor.finish() {
                Ok(text) => Ok(EditorAction::Committed(text)),
                Err(error) => Err(error),
            }
        } else {
            let Some(session) = self.app.active_document() else {
                self.editor.cancel();
                return;
            };
            canvas::show_text_editor(ui, session, rect, &mut self.editor)
        };
        match action {
            Ok(EditorAction::Pending) => {}
            Ok(EditorAction::Committed(Some(text))) => {
                if let Some(mut produced) = self.text_commit_intents(shape, &text) {
                    intents.append(&mut produced);
                }
            }
            Ok(EditorAction::Committed(None)) => {}
            Err(error) => self.note_error(error.to_string()),
        }
    }

    /// Presses the editor's shape, then writes its text: one command
    /// pair so the selection cannot drift before the write (the editor
    /// closes on a click that may select another shape).
    fn text_commit_intents(
        &self,
        shape: Option<ShapeId>,
        text: &TextContent,
    ) -> Option<Vec<Intent>> {
        let shape = shape?;
        let session = self.app.active_document()?;
        let visio_id = session.document.shape(shape)?.visio_id;
        let literal: String = text
            .parts
            .iter()
            .filter_map(|part| match part {
                TextPart::Text(part) => Some(part.as_str()),
                _ => None,
            })
            .collect();
        let mut args = larrygram_commands::Args::new();
        args.set("shapes", ArgValue::List(vec![visio_id.to_string()]));
        args.set("mode", ArgValue::Text("replace".to_owned()));
        let command = CommandId::parse("edit.select").ok()?;
        let select = Intent::new(command, Invocation::new(args, Source::Ui));
        let mut args = larrygram_commands::Args::new();
        args.set("text", ArgValue::Text(literal));
        let command = CommandId::parse("shape.set.text").ok()?;
        let write = Intent::new(command, Invocation::new(args, Source::Ui));
        Some(vec![select, write])
    }

    /// Opens the editor after a successful `text.begin` (task P5-17).
    fn begin_text_editor(&mut self) {
        let Some(shape) = self
            .app
            .selection
            .anchor
            .or_else(|| self.app.selection.shapes.last().copied())
        else {
            return;
        };
        let text = self
            .app
            .active_document()
            .and_then(|session| session.document.shape(shape))
            .and_then(|entry| entry.text.clone())
            .unwrap_or_default();
        if let Err(error) = self.editor.begin(shape, &text) {
            self.note_error(error.to_string());
        }
    }

    /// The modal windows: the welcome wizard, View/Options, About and
    /// the close confirmation (ADR 0028 [`UiRequest`] flow, ADR 0066).
    ///
    /// The Options dialog body returns the [`Intent`]s of the rows the
    /// user changed (the ADR 0032 amendment) plus the welcome-wizard
    /// row; the intents join the frame's intents and run through the
    /// registry like any other control. The wizard applies
    /// [`wizard::Outcome`] per ADR 0066 and the About window opens
    /// the video through egui's open-URL output.
    fn show_dialogs(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        if self.wizard.is_some() {
            let mut outcome: Option<wizard::Outcome> = None;
            let mut failure: Option<UiError> = None;
            egui::Window::new("Welcome to Larrygram")
                .collapsible(false)
                .resizable(false)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    if let Some(state) = &mut self.wizard {
                        match wizard::show(ui, state) {
                            Ok(value) => outcome = value,
                            Err(error) => failure = Some(error),
                        }
                    }
                });
            match outcome {
                Some(wizard::Outcome::Start { default_layout }) => {
                    // Start saves the choice as the default layout and
                    // applies it at once (ADR 0066); the widgets
                    // rebuild on the next frame (ADR 0032).
                    self.set_layout_kind(default_layout);
                    self.app.settings.welcome_seen = true;
                    self.wizard = None;
                }
                Some(wizard::Outcome::Cancel) => {
                    // Cancel keeps the current layout and marks the
                    // wizard as seen (ADR 0066).
                    self.app.settings.welcome_seen = true;
                    self.wizard = None;
                }
                None => {}
            }
            if let Some(error) = failure {
                self.note_error(error.to_string());
            }
        }
        if self.options_open {
            let mut open = true;
            let mut changed = settings::OptionsChange::none();
            let mut failure: Option<UiError> = None;
            egui::Window::new("Options")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| match settings::show_options(ui, &self.app.settings) {
                    Ok(value) => changed = value,
                    Err(error) => failure = Some(error),
                });
            self.options_open = open;
            if let Some(show) = changed.show_wizard_next_start {
                // The welcome row has no command id (task P7-09):
                // checked means the wizard opens on the next start.
                self.app.settings.welcome_seen = !show;
            }
            self.absorb(Ok(changed.intents), intents);
            if let Some(error) = failure {
                self.note_error(error.to_string());
            }
        }
        if self.about_open {
            let mut open = true;
            let mut outcome = about::Outcome::Open;
            let mut failure: Option<UiError> = None;
            egui::Window::new("About Larrygram")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| match about::show(ui) {
                    Ok(value) => outcome = value,
                    Err(error) => failure = Some(error),
                });
            self.about_open = open;
            match outcome {
                about::Outcome::Close => self.about_open = false,
                about::Outcome::WatchVideo => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(about::VIDEO_URL));
                }
                about::Outcome::Open => {}
            }
            if let Some(error) = failure {
                self.note_error(error.to_string());
            }
        }
        if let Some(name) = self.confirm_close.clone() {
            let mut answer: Option<bool> = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Unsaved changes in {name}."));
                    ui.horizontal(|ui| {
                        if ui.button("Close without saving").clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            answer = Some(false);
                        }
                    });
                });
            if let Some(close) = answer {
                self.confirm_close = None;
                if close && let Ok(command) = CommandId::parse("file.close") {
                    let args =
                        larrygram_commands::Args::new().with("confirm", ArgValue::Flag(true));
                    intents.push(Intent::new(command, Invocation::new(args, Source::Ui)));
                }
            }
        }
    }

    /// Clears the selection when the active document or page changed
    /// since the previous frame (ADR 0041: the page-switch commands and
    /// the page tabs clear it).
    fn clear_selection_on_page_switch(&mut self) {
        let current = self
            .app
            .active_document()
            .map(|session| (session.id, session.active_page));
        if self.last_page.is_some() && self.last_page != current && !self.app.selection.is_empty() {
            self.app.selection.clear();
        }
        self.last_page = current;
    }

    /// The title strip's document text.
    fn title_text(&self) -> String {
        match self.app.active_document() {
            Some(session) => session.display_name(),
            None => "No document".to_owned(),
        }
    }

    /// Keeps the value of a widget call and shows its failure in the
    /// status bar (never a crash, AGENTS.md section 6).
    fn keep<T>(&mut self, produced: Result<T, UiError>) -> Option<T> {
        match produced {
            Ok(value) => Some(value),
            Err(error) => {
                self.note_error(error.to_string());
                None
            }
        }
    }

    /// Keeps the intents of a widget and shows its failure in the
    /// status bar.
    fn absorb(&mut self, produced: Result<Vec<Intent>, UiError>, intents: &mut Vec<Intent>) {
        if let Some(mut more) = self.keep(produced) {
            intents.append(&mut more);
        }
    }

    /// Dispatches intents through the registry (ADR 0028), in order.
    ///
    /// One result per intent, each error back to the caller untouched.
    /// Every [`CommandError`] also reaches the status bar as one line
    /// through [`Shell::note_error`] (ADR 0021; P4-16 review major 2:
    /// a failed command is never silent). A cancelled dialog
    /// dispatches nothing and stays quiet (ADR 0036).
    pub fn dispatch(
        &mut self,
        intents: &[Intent],
    ) -> Result<Vec<Result<Outcome, CommandError>>, UiError> {
        let mut results = Vec::with_capacity(intents.len());
        for intent in intents {
            results.push(self.dispatch_intent(intent)?);
        }
        Ok(results)
    }

    /// Dispatches one intent, answering a `NeedsInput` from an
    /// interactive source through the picker (ADR 0036).
    fn dispatch_intent(
        &mut self,
        intent: &Intent,
    ) -> Result<Result<Outcome, CommandError>, UiError> {
        let source = intent.invocation.source;
        let first = dispatch_one(
            &self.registry,
            &mut self.app,
            &intent.command,
            intent.invocation.clone(),
        );
        let outcome = match first {
            Ok(outcome) => outcome,
            Err(UiError::Command(CommandError::NeedsInput(needed))) if source.is_interactive() => {
                match self.ask_picker(&intent.command, &intent.invocation, &needed)? {
                    Ask::Again(invocation) => {
                        match dispatch_one(
                            &self.registry,
                            &mut self.app,
                            &intent.command,
                            invocation,
                        ) {
                            Ok(outcome) => outcome,
                            Err(UiError::Command(error)) => {
                                self.note_error(error.to_string());
                                return Ok(Err(error));
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Ask::Pending => {
                        self.pending = Some((intent.command, intent.invocation.clone()));
                        return Ok(Ok(Outcome::message("Waiting for the file dialog")));
                    }
                    Ask::Cancelled => return Ok(Ok(Outcome::message("Cancelled"))),
                    Ask::NoDialog(needed) => {
                        let error = CommandError::NeedsInput(needed);
                        self.note_error(error.to_string());
                        return Ok(Err(error));
                    }
                }
            }
            Err(UiError::Command(error)) => {
                self.note_error(error.to_string());
                return Ok(Err(error));
            }
            Err(error) => return Err(error),
        };
        self.apply_outcome(&outcome, source)?;
        if intent.command.as_str() == "text.begin" {
            self.begin_text_editor();
        } else if intent.command.as_str() == "text.cancel" {
            self.editor.cancel();
        }
        Ok(Ok(outcome))
    }

    /// Asks the [`FilePicker`] for a `NeedsInput` value (ADR 0036).
    ///
    /// `Ok(None)` from the picker is a cancel ("cancelling a dialog
    /// dispatches nothing") unless an asynchronous pick is still in
    /// flight (the wasm upload).
    fn ask_picker(
        &mut self,
        command: &CommandId,
        invocation: &Invocation,
        needed: &NeededInput,
    ) -> Result<Ask, UiError> {
        match needed {
            NeededInput::Path {
                purpose: PathPurpose::OpenFile,
                suggested,
            } => {
                let request = OpenRequest {
                    title: format!("Open ({command})"),
                    extensions: OPEN_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
                    suggested: suggested.clone(),
                };
                let choice = match self.picker.open(&request)? {
                    Some(choice) => Some(choice),
                    None => match self.picker.poll() {
                        PickState::Picked(choice) => Some(choice),
                        PickState::Pending => return Ok(Ask::Pending),
                        _ => None,
                    },
                };
                Ok(match choice {
                    Some(choice) => Ask::Again(with_choice(invocation, choice)),
                    None => Ask::Cancelled,
                })
            }
            NeededInput::Path {
                purpose: PathPurpose::SaveFile,
                suggested,
            } => {
                let default_name = suggested
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| DEFAULT_SAVE_NAME.to_owned());
                let request = SaveRequest {
                    title: format!("Save ({command})"),
                    default_name,
                };
                // Empty data: ask for the target only (the command
                // writes the file when dispatched with `path`).
                Ok(match self.picker.save(&request, &[])? {
                    Some(choice) => Ask::Again(with_choice(invocation, choice)),
                    None => Ask::Cancelled,
                })
            }
            // No dialog kind for folders, text and numbers: the
            // programmatic contract, error back untouched.
            _ => Ok(Ask::NoDialog(needed.clone())),
        }
    }

    /// Acts on the follow-up requests of a completed command (ADR
    /// 0028 [`UiRequest`], ADR 0036 [`Payload`]).
    ///
    /// Only for an interactive [`Source`]: a programmatic caller gets
    /// its outcome back untouched — no dialog opens, no download
    /// starts, no window quits (AGENTS.md section 6; P4-16 review
    /// major 3). The status line still shows the message.
    fn apply_outcome(&mut self, outcome: &Outcome, source: Source) -> Result<(), UiError> {
        if let Some(message) = &outcome.message {
            self.status = message.clone();
        }
        if !source.is_interactive() {
            return Ok(());
        }
        match &outcome.ui_request {
            Some(UiRequest::ShowAbout) => self.about_open = true,
            Some(UiRequest::ShowOptions) => self.options_open = true,
            Some(UiRequest::ConfirmClose { name }) => self.confirm_close = Some(name.clone()),
            Some(UiRequest::Quit) => self.quit_requested = true,
            _ => {}
        }
        if let Some(Payload::Bytes { name, data }) = &outcome.payload {
            let request = SaveRequest {
                title: "Save".to_owned(),
                default_name: name.clone(),
            };
            self.picker.save(&request, data)?;
        }
        Ok(())
    }

    /// Completes a pending asynchronous pick (the wasm upload, ADR
    /// 0036): dispatches the waiting command with the picked bytes.
    fn finish_pick(&mut self) {
        let choice = match self.picker.poll() {
            PickState::Picked(choice) => Some(choice),
            PickState::Cancelled => {
                self.pending = None;
                None
            }
            _ => None,
        };
        let Some(choice) = choice else {
            return;
        };
        let Some((command, invocation)) = self.pending.take() else {
            return;
        };
        let intent = Intent::new(command, with_choice(&invocation, choice));
        match self.dispatch_intent(&intent) {
            Ok(Ok(outcome)) => {
                if let Some(message) = outcome.message {
                    self.status = message;
                }
            }
            Ok(Err(error)) => self.note_error(error.to_string()),
            Err(error) => self.note_error(error.to_string()),
        }
    }
}

/// One menu of a shell menu bar (File, View, Help). Every item is a
/// registry id (ADR 0028); the four layout ids render as radios
/// (ADR 0032, task P7-09).
fn show_menu(
    ui: &mut Ui,
    registry: &Registry,
    app: &AppCtx,
    title: &str,
    ids: &[&'static str],
    intents: &mut Vec<Intent>,
) {
    ui.menu_button(title, |ui| {
        for &id in ids {
            let checked = layout_radio_checked(app, id);
            if let Some(intent) = menu_item(ui, registry, app, id, checked) {
                intents.push(intent);
                ui.close();
            }
        }
    });
}

/// The checked state of a layout radio row (ADR 0032): `Some(on)` for
/// the four `view.ui_layout.*` ids, `None` for every other row.
fn layout_radio_checked(app: &AppCtx, id: &str) -> Option<bool> {
    let kind = match id {
        "view.ui_layout.panel_modern" => LayoutKind::PanelModern,
        "view.ui_layout.panel_classic" => LayoutKind::PanelClassic,
        "view.ui_layout.panel_2016" => LayoutKind::Panel2016,
        "view.ui_layout.panel_365" => LayoutKind::Panel365,
        _ => return None,
    };
    Some(app.settings.ui_layout == kind)
}

/// One menu or status-bar item for a registry id: `Some(intent)` when
/// clicked.
///
/// The label and the enabled state come from the registry (the
/// window commands included: their rows are implemented commands
/// like any other, P4-16).
fn menu_item(
    ui: &mut Ui,
    registry: &Registry,
    app: &AppCtx,
    id: &'static str,
    checked: Option<bool>,
) -> Option<Intent> {
    let Ok(command) = CommandId::parse(id) else {
        return None;
    };
    let label = registry
        .meta(&command)
        .map(|meta| meta.label.clone())
        .unwrap_or_else(|| id.to_owned());
    let enabled = registry.enabled(&command, app);
    let response = ui.add_enabled_ui(enabled.value, |ui| match checked {
        Some(selected) => ui.radio(selected, label),
        None => ui.button(label),
    });
    let response = response.inner;
    let response = match &enabled.reason {
        Some(reason) => response.on_hover_text(reason.clone()),
        None => response,
    };
    if response.clicked() {
        Some(Intent::new(command, Invocation::empty(Source::Menu)))
    } else {
        None
    }
}

/// Which layout family draws the frame (task P7-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameStyle {
    /// Panel Classic: menu bars, toolbars, the classic status bar.
    Classic,
    /// Panel Modern: tab strip, command bar, zoom controls.
    Modern,
    /// The Phase 4 frame of the two Fluent-data kinds.
    Fluent,
}

impl FrameStyle {
    /// The Shapes window header of this frame (task P7-09 owns its
    /// header and look).
    fn shapes_header(self) -> shapes_window::Header {
        match self {
            Self::Classic => shapes_window::Header::Classic,
            Self::Modern | Self::Fluent => shapes_window::Header::Modern,
        }
    }
}

/// Splits a `"<mode>/<toolbar id>"` settings key back into the Panel
/// Classic window mode and the toolbar id (task P7-09 Goal 3).
fn split_toolbar_key(key: &str) -> Option<(panel_classic::WindowMode, String)> {
    let (mode, toolbar) = key.split_once('/')?;
    let mode = match mode {
        "drawing" => panel_classic::WindowMode::Drawing,
        "shapesheet" => panel_classic::WindowMode::Shapesheet,
        "stencil" => panel_classic::WindowMode::Stencil,
        _ => return None,
    };
    Some((mode, toolbar.to_owned()))
}

/// The `edit.select` intent that selects one shape (the right-click
/// rule: a right-click selects what it hits first,
/// docs/ui/panel-classic.md).
fn select_intent(app: &AppCtx, shape: ShapeId) -> Option<Intent> {
    let session = app.active_document()?;
    let visio_id = session.document.shape(shape)?.visio_id;
    let mut args = larrygram_commands::Args::new();
    args.set("shapes", ArgValue::List(vec![visio_id.to_string()]));
    args.set("mode", ArgValue::Text("replace".to_owned()));
    let command = CommandId::parse("edit.select").ok()?;
    Some(Intent::new(command, Invocation::new(args, Source::Ui)))
}

/// One dispatch request for a static command id.
fn intent_for(id: &'static str) -> Result<Intent, UiError> {
    let command = CommandId::parse(id).map_err(|error| UiError::Io {
        message: format!("the command id {id} is not valid: {error}"),
    })?;
    Ok(Intent::new(command, Invocation::empty(Source::Ui)))
}

/// The Panel Modern page-tab strip: the page tabs, the "All" menu
/// listing every page and the insert-page button
/// (docs/ui/panel-modern.md, "Window"). Page tabs switch the active
/// page directly, as the Phase 4 strip does; the insert button runs
/// `page.add`.
fn modern_page_tabs(ui: &mut Ui, session: &mut DocumentSession) -> Result<Vec<Intent>, UiError> {
    let pages: Vec<(PageId, String)> = session
        .document
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.id, canvas::page_label(page, index)))
        .collect();
    let active = session.active_page;
    let mut clicked = None;
    let mut add = false;
    ui.horizontal_wrapped(|ui| {
        for (id, label) in &pages {
            let response = ui
                .selectable_label(active == Some(*id), label)
                .on_hover_text("Show this page");
            if response.clicked() {
                clicked = Some(*id);
            }
        }
        ui.menu_button("All", |ui| {
            for (id, label) in &pages {
                if ui.button(label).clicked() {
                    clicked = Some(*id);
                    ui.close();
                }
            }
        });
        add = ui
            .button("\u{2295}")
            .on_hover_text("New page")
            .clicked();
    });
    if let Some(id) = clicked {
        session.active_page = Some(id);
    }
    let mut intents = Vec::new();
    if add {
        intents.push(intent_for("page.add")?);
    }
    Ok(intents)
}

/// The Panel Classic page-tab strip: the first, previous, next and
/// last page buttons and the page tabs
/// (docs/ui/panel-classic.md, "Window").
fn classic_page_tabs(ui: &mut Ui, session: &mut DocumentSession) -> Result<Vec<Intent>, UiError> {
    let pages: Vec<(PageId, String)> = session
        .document
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.id, canvas::page_label(page, index)))
        .collect();
    let active = session.active_page;
    let mut clicked = None;
    let mut first = false;
    let mut previous = false;
    let mut next = false;
    let mut last = false;
    ui.horizontal_wrapped(|ui| {
        first = ui.button("|<").on_hover_text("First page").clicked();
        previous = ui.button("<").on_hover_text("Previous page").clicked();
        for (id, label) in &pages {
            let response = ui
                .selectable_label(active == Some(*id), label)
                .on_hover_text("Show this page");
            if response.clicked() {
                clicked = Some(*id);
            }
        }
        next = ui.button(">").on_hover_text("Next page").clicked();
        last = ui.button(">|").on_hover_text("Last page").clicked();
    });
    if let Some(id) = clicked {
        session.active_page = Some(id);
    }
    if first && let Some((id, _)) = pages.first() {
        session.active_page = Some(*id);
    }
    if last && let Some((id, _)) = pages.last() {
        session.active_page = Some(*id);
    }
    let mut intents = Vec::new();
    if previous {
        intents.push(intent_for("page.previous")?);
    }
    if next {
        intents.push(intent_for("page.next")?);
    }
    Ok(intents)
}

/// What to do with a `NeedsInput` value once the picker answered (or
/// could not).
enum Ask {
    /// Dispatch the command again with these arguments.
    Again(Invocation),
    /// An asynchronous pick is in flight; dispatch when it finishes.
    Pending,
    /// The user cancelled; dispatch nothing.
    Cancelled,
    /// No dialog answers this input; hand the error back.
    NoDialog(NeededInput),
}

/// Builds the shortcut map of a `shortcuts.yaml` table with its load
/// warnings (ADR 0036: `ShortcutMap::conflicts` go to
/// `AppCtx::warnings` at load). A table that fails to load becomes an
/// empty map with one warning (AGENTS.md section 6: never crash).
fn load_shortcuts(shortcuts: &Shortcuts) -> (ShortcutMap, Vec<String>) {
    match ShortcutMap::from_shortcuts(shortcuts) {
        Ok(map) => {
            let warnings = map.conflicts().to_vec();
            (map, warnings)
        }
        Err(error) => (
            ShortcutMap::default(),
            vec![format!("shortcuts not loaded: {error}")],
        ),
    }
}

/// Fills the `path` (desktop) or `name` + `data` (upload) arguments of
/// a re-dispatch (ADR 0036).
fn with_choice(invocation: &Invocation, choice: FileChoice) -> Invocation {
    let mut invocation = invocation.clone();
    match choice {
        FileChoice::Path(path) => {
            invocation.args.set("path", ArgValue::Path(path));
        }
        FileChoice::Bytes { name, data } => {
            invocation.args.set("name", ArgValue::Text(name));
            invocation.args.set("data", ArgValue::Bytes(data));
        }
    }
    invocation
}

/// Dispatches one already-built invocation; the seam the CLI and the
/// Phase 8 control channel reuse (ADR 0028).
///
/// A thin wrapper over [`larrygram_commands::dispatch`]:
/// the run and the undo-step recording (the session the command ran
/// against, ADR 0040 rule 2) live in `larrygram_commands`, so every
/// programmatic caller shares one rule (task P5-19 review F3).
pub fn dispatch_one(
    registry: &Registry,
    app: &mut AppCtx,
    command: &CommandId,
    invocation: Invocation,
) -> Result<Outcome, UiError> {
    Ok(larrygram_commands::dispatch(
        registry, app, command, invocation,
    )?)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::dialogs::{OpenRequest, SaveRequest};
    use larrygram_commands::{
        Command, CommandMeta, CommandStatus, DocumentSession, Edit, Enabled, Settings, StubCommand,
    };
    use larrygram_model::Document;
    use larrygram_ui_spec::Loader;

    /// The committed layouts (the app loads the same way).
    fn sets() -> LayoutSets {
        Loader::from_embedded().expect("the embedded layouts load")
    }

    /// Settings that already saw the welcome wizard (most tests do
    /// not want the first-run modal; ADR 0066).
    fn seen_settings() -> Settings {
        let mut settings = Settings::default();
        settings.welcome_seen = true;
        settings
    }

    fn shell() -> Shell {
        Shell::new(
            Registry::new(),
            LayoutSets::default(),
            AppCtx::new(seen_settings()),
        )
    }

    /// A shell over the real builtin registry and layouts.
    fn full_shell() -> Shell {
        Shell::new(
            larrygram_commands::builtins::registry().expect("registry"),
            sets(),
            AppCtx::new(seen_settings()),
        )
    }

    /// A picker that records its calls and answers with a fixed
    /// choice; its asynchronous side (`poll`) answers `poll_state`.
    #[derive(Default)]
    struct FakePicker {
        choice: Option<FileChoice>,
        poll_state: PickState,
        open_calls: Rc<RefCell<usize>>,
        save_calls: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl FilePicker for FakePicker {
        fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, UiError> {
            *self.open_calls.borrow_mut() += 1;
            Ok(self.choice.clone())
        }
        fn save(&self, _request: &SaveRequest, data: &[u8]) -> Result<Option<FileChoice>, UiError> {
            self.save_calls.borrow_mut().push(data.to_vec());
            Ok(self.choice.clone())
        }
        fn poll(&self) -> PickState {
            self.poll_state.clone()
        }
    }

    /// A command that demands a path once and succeeds when it gets
    /// one; counts its runs.
    struct NeedsPath {
        runs: Arc<AtomicUsize>,
    }

    impl Command for NeedsPath {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.needs_path").expect("id"),
                    "Needs Path",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            _ctx: &mut AppCtx,
            invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            match (
                invocation.args.path("path"),
                invocation.args.text("name"),
                invocation.args.bytes("data"),
            ) {
                (Some(path), _, _) => Ok(Outcome::message(format!("got {}", path.display()))),
                (None, Some(name), Some(data)) => Ok(Outcome::message(format!(
                    "got {name} ({} bytes)",
                    data.len()
                ))),
                _ => Err(CommandError::NeedsInput(NeededInput::Path {
                    purpose: PathPurpose::OpenFile,
                    suggested: None,
                })),
            }
        }
    }

    /// A command whose outcome carries a payload (the wasm download).
    struct MakesPayload;

    impl Command for MakesPayload {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.makes_payload").expect("id"),
                    "Makes Payload",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            _ctx: &mut AppCtx,
            _invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            let mut outcome = Outcome::message("saved");
            outcome.payload = Some(Payload::Bytes {
                name: "drawing.vsdx".to_owned(),
                data: vec![1, 2, 3],
            });
            Ok(outcome)
        }
    }

    /// A command whose outcome carries one undoable edit.
    struct NoopEdit;

    impl Edit for NoopEdit {
        fn label(&self) -> &str {
            "noop"
        }
        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
    }

    struct MakesEdit;

    impl Command for MakesEdit {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.makes_edit").expect("id"),
                    "Makes Edit",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            _ctx: &mut AppCtx,
            _invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            let mut outcome = Outcome::message("edited");
            outcome.edit = Some(Box::new(NoopEdit));
            Ok(outcome)
        }
    }

    fn intent(id: &str, source: Source) -> Intent {
        let command = CommandId::intern(id).expect("id");
        Intent::new(command, Invocation::empty(source))
    }

    #[test]
    fn shell_tracks_the_layout_kind() {
        let mut shell = shell();
        assert_eq!(
            shell.layout_kind(),
            LayoutKind::PanelModern,
            "new stores default to Panel Modern (task P7-09 Goal 2)"
        );
        shell.set_layout_kind(LayoutKind::Panel365);
        assert_eq!(shell.layout_kind(), LayoutKind::Panel365);
        assert_eq!(
            shell.app.settings.default_layout,
            LayoutKind::Panel365,
            "the default layout stays in step with the active one (Goal 6)"
        );
        assert_eq!(shell.layout_set(), &shell.layouts.panel_365);
    }

    #[test]
    fn every_layout_kind_draws_its_frame_without_an_error() {
        // The frame comes from the active layout's `window.regions`
        // (ADR 0061, task P7-09): the two curated kinds draw their v2
        // widgets and the two Fluent-data kinds the Phase 4 command
        // bar. Both an empty and the committed set must render.
        for layouts in [LayoutSets::default(), sets().clone()] {
            for kind in [
                LayoutKind::PanelModern,
                LayoutKind::PanelClassic,
                LayoutKind::Panel2016,
                LayoutKind::Panel365,
            ] {
                let mut settings = seen_settings();
                settings.ui_layout = kind;
                let mut shell =
                    Shell::new(Registry::new(), layouts.clone(), AppCtx::new(settings));
                assert_eq!(shell.layout_kind(), kind);
                assert_eq!(shell.layout_set(), layouts.get(kind));
                shell_frame(&mut shell, Vec::new(), 0.0);
            }
        }
    }

    #[test]
    fn switching_pages_clears_the_selection() {
        let mut shell = full_shell();
        shell
            .dispatch(&[intent("file.new", Source::Test)])
            .expect("dispatch");
        let session = shell.app.active_document_mut().expect("a document");
        session.document.pages.push(larrygram_model::Page::new(
            larrygram_model::PageId { index: 1 },
            2,
        ));
        shell.app.selection.shapes = vec![larrygram_model::ShapeId { index: 0 }];
        shell.app.selection.anchor = Some(larrygram_model::ShapeId { index: 0 });
        shell.clear_selection_on_page_switch();
        assert!(!shell.app.selection.is_empty());
        shell
            .app
            .active_document_mut()
            .expect("a document")
            .active_page = Some(larrygram_model::PageId { index: 1 });
        shell.clear_selection_on_page_switch();
        assert!(shell.app.selection.is_empty(), "the page switch cleared it");
    }

    #[test]
    fn dispatch_one_runs_a_command() {
        let registry = larrygram_commands::builtins::registry().expect("registry");
        let mut app = AppCtx::new(Settings::default());
        let id = CommandId::parse("file.new").expect("id");
        let outcome = dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test))
            .expect("file.new runs");
        assert_eq!(outcome.message.as_deref(), Some("New drawing"));
        assert_eq!(app.documents.len(), 1);
    }

    #[test]
    fn dispatch_one_wraps_command_errors() {
        let registry = Registry::new();
        let mut app = AppCtx::new(Settings::default());
        let id = CommandId::parse("file.new").expect("id");
        let error = dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test))
            .expect_err("unknown");
        assert!(
            matches!(error, UiError::Command(CommandError::UnknownCommand { .. })),
            "{error:?}"
        );
    }

    #[test]
    fn dispatch_one_pushes_the_undo_edit() {
        let mut registry = Registry::new();
        registry.register(Box::new(MakesEdit)).expect("register");
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(Document::new(), None));
        let id = CommandId::parse("file.makes_edit").expect("id");
        dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test)).expect("edit");
        let session = app.active_document().expect("document");
        assert!(session.undo.can_undo());
    }

    #[test]
    fn an_edit_without_a_document_is_dropped_with_a_warning() {
        let mut registry = Registry::new();
        registry.register(Box::new(MakesEdit)).expect("register");
        let mut app = AppCtx::new(Settings::default());
        let id = CommandId::parse("file.makes_edit").expect("id");
        dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test)).expect("edit");
        assert_eq!(app.warnings.len(), 1);
    }

    #[test]
    fn the_window_commands_run_through_the_registry() {
        // No shell intercept (P4-16): `app.quit` reaches its registry
        // row. A stub row fails like any other stub and the shell does
        // not quit on its own.
        let mut registry = Registry::new();
        registry
            .register(Box::new(StubCommand::new(CommandMeta::new(
                CommandId::parse("app.quit").expect("id"),
                "Exit",
                CommandStatus::Stub,
            ))))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        let results = shell
            .dispatch(&[intent("app.quit", Source::Menu)])
            .expect("dispatch");
        assert!(matches!(results[0], Err(CommandError::Unsupported { .. })));
        assert!(!shell.quit_requested(), "no silent quit behind a stub");

        // The builtin row is a real command: its `UiRequest::Quit`
        // closes the window (`show_in` hands it to the viewport).
        let mut shell = full_shell();
        let results = shell
            .dispatch(&[intent("app.quit", Source::Menu)])
            .expect("dispatch");
        assert_eq!(
            results[0].as_ref().expect("quit").message.as_deref(),
            Some("Quit")
        );
        assert!(shell.quit_requested());

        // `app.office.center.options` asks for the dialog the same way.
        let results = shell
            .dispatch(&[intent("app.office.center.options", Source::Menu)])
            .expect("dispatch");
        assert!(matches!(
            results[0].as_ref().expect("options").ui_request,
            Some(UiRequest::ShowOptions)
        ));
    }

    #[test]
    fn needs_input_picks_a_path_and_dispatches_again() {
        let mut registry = Registry::new();
        let runs = Arc::new(AtomicUsize::new(0));
        registry
            .register(Box::new(NeedsPath {
                runs: Arc::clone(&runs),
            }))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: Some(FileChoice::Path("/tmp/a.vsdx".into())),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.needs_path", Source::Ui)])
            .expect("dispatch");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "once without input, once with"
        );
        let outcome = results[0].as_ref().expect("picked");
        assert_eq!(outcome.message.as_deref(), Some("got /tmp/a.vsdx"));
    }

    #[test]
    fn needs_input_takes_the_upload_bytes() {
        let mut registry = Registry::new();
        let runs = Arc::new(AtomicUsize::new(0));
        registry
            .register(Box::new(NeedsPath {
                runs: Arc::clone(&runs),
            }))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: Some(FileChoice::Bytes {
                name: "a.vsdx".into(),
                data: vec![7, 7],
            }),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.needs_path", Source::CommandBar)])
            .expect("dispatch");
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        let outcome = results[0].as_ref().expect("uploaded");
        assert_eq!(outcome.message.as_deref(), Some("got a.vsdx (2 bytes)"));
    }

    #[test]
    fn cancelling_a_dialog_dispatches_nothing() {
        let mut registry = Registry::new();
        let runs = Arc::new(AtomicUsize::new(0));
        registry
            .register(Box::new(NeedsPath {
                runs: Arc::clone(&runs),
            }))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker::default()));
        let results = shell
            .dispatch(&[intent("file.needs_path", Source::Menu)])
            .expect("dispatch");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "only the unanswered dispatch ran"
        );
        let outcome = results[0].as_ref().expect("cancelled");
        assert_eq!(outcome.message.as_deref(), Some("Cancelled"));
    }

    #[test]
    fn programmatic_sources_get_the_error_back_untouched() {
        let mut registry = Registry::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let opens = Rc::new(RefCell::new(0));
        registry
            .register(Box::new(NeedsPath {
                runs: Arc::clone(&runs),
            }))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: Some(FileChoice::Path("/tmp/a.vsdx".into())),
            open_calls: Rc::clone(&opens),
            save_calls: Rc::new(RefCell::new(Vec::new())),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.needs_path", Source::Cli)])
            .expect("dispatch");
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(*opens.borrow(), 0, "no dialog for a programmatic call");
        assert!(matches!(
            results[0],
            Err(CommandError::NeedsInput(NeededInput::Path { .. }))
        ));
    }

    #[test]
    fn a_payload_is_handed_to_the_picker() {
        let mut registry = Registry::new();
        registry.register(Box::new(MakesPayload)).expect("register");
        let saves = Rc::new(RefCell::new(Vec::new()));
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: None,
            open_calls: Rc::new(RefCell::new(0)),
            save_calls: Rc::clone(&saves),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.makes_payload", Source::Ui)])
            .expect("dispatch");
        assert_eq!(
            results[0].as_ref().expect("payload").message.as_deref(),
            Some("saved")
        );
        assert_eq!(saves.borrow().as_slice(), [vec![1, 2, 3]]);
    }

    #[test]
    fn input_no_dialog_kind_answers_keeps_the_error() {
        /// A command that demands a number: no dialog answers that.
        struct NeedsNumber;

        impl Command for NeedsNumber {
            fn meta(&self) -> &CommandMeta {
                use std::sync::OnceLock;
                static META: OnceLock<CommandMeta> = OnceLock::new();
                META.get_or_init(|| {
                    CommandMeta::new(
                        CommandId::parse("file.needs_number").expect("id"),
                        "Needs Number",
                        CommandStatus::Implemented,
                    )
                })
            }
            fn enabled(&self, _ctx: &AppCtx) -> Enabled {
                Enabled::yes()
            }
            fn execute(
                &self,
                _ctx: &mut AppCtx,
                _invocation: &Invocation,
            ) -> Result<Outcome, CommandError> {
                Err(CommandError::NeedsInput(NeededInput::Number {
                    prompt: "a scale".to_owned(),
                    default: None,
                }))
            }
        }

        let mut registry = Registry::new();
        registry.register(Box::new(NeedsNumber)).expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker::default()));
        let results = shell
            .dispatch(&[intent("file.needs_number", Source::Ui)])
            .expect("dispatch");
        assert!(matches!(
            results[0],
            Err(CommandError::NeedsInput(NeededInput::Number { .. }))
        ));
    }

    #[test]
    fn a_stub_command_reports_unsupported() {
        let mut registry = Registry::new();
        registry
            .register(Box::new(StubCommand::new(CommandMeta::new(
                CommandId::parse("file.stub").expect("id"),
                "Stub",
                CommandStatus::Stub,
            ))))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        let results = shell
            .dispatch(&[intent("file.stub", Source::Ui)])
            .expect("dispatch");
        assert!(matches!(results[0], Err(CommandError::Unsupported { .. })));
    }

    #[test]
    fn a_real_frame_renders_every_widget_without_errors() {
        let mut shell = full_shell();
        shell
            .app
            .open_document(DocumentSession::new(Document::new(), None));
        let ctx = Context::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let intents = shell.show_in(ui).expect("the frame renders");
                let results = shell.dispatch(&intents).expect("dispatch");
                assert!(results.iter().all(|result| result.is_ok()));
            });
            output.textures_delta.clear();
        }
        // Every widget is implemented (P4-07 filled the last stub), so a
        // plain frame over an empty document records no widget error.
        // `note_error` reaching the status bar is tested separately.
        assert!(
            !shell
                .app
                .warnings
                .iter()
                .any(|warning| warning.contains("not implemented")),
            "no widget stub errors: {:?}",
            shell.app.warnings
        );
    }

    #[test]
    fn show_in_through_the_root_ui_draws_the_frame() {
        let mut shell = full_shell();
        let ctx = Context::default();
        ctx.begin_pass(Default::default());
        let intents = ctx
            .root_ui(|ui| shell.show_in(ui))
            .expect("the frame renders");
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        assert!(intents.is_empty());
    }

    #[test]
    fn with_choice_fills_path_or_upload_arguments() {
        let invocation = Invocation::empty(Source::Ui);
        let path = with_choice(&invocation, FileChoice::Path("/tmp/a.vsdx".into()));
        assert_eq!(
            path.args.path("path"),
            Some(std::path::Path::new("/tmp/a.vsdx"))
        );
        let upload = with_choice(
            &invocation,
            FileChoice::Bytes {
                name: "a.vsdx".into(),
                data: vec![1],
            },
        );
        assert_eq!(upload.args.text("name"), Some("a.vsdx"));
        assert_eq!(upload.args.bytes("data"), Some(&[1u8][..]));
    }

    #[test]
    fn the_title_text_names_the_document() {
        let mut shell = shell();
        assert_eq!(shell.title_text(), "No document");
        shell
            .app
            .open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(shell.title_text(), "Untitled");
    }

    #[test]
    fn repeated_errors_are_recorded_once() {
        let mut shell = shell();
        shell.note_error("command bar is not implemented yet");
        shell.note_error("command bar is not implemented yet");
        assert_eq!(shell.app.warnings.len(), 1);
    }

    /// One shell frame with the given input events (the shortcut pass
    /// runs inside `show_in`, before the widgets).
    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    fn shell_frame(shell: &mut Shell, events: Vec<egui::Event>, time: f64) {
        let ctx = Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                let intents = shell.show_in(ui).expect("the frame renders");
                let _ = shell.dispatch(&intents).expect("dispatch");
            },
        );
        output.textures_delta.clear();
    }

    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    fn press_event(pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// The committed `shortcuts.yaml` table (the loader's).
    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    fn committed_shortcuts() -> Shortcuts {
        sets()
            .get(LayoutKind::Panel2016)
            
            .shortcuts
            .clone()
    }

    /// A two-chord sequence (`Alt+H, F`): the committed table has
    /// none (P4-14 Result).
    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    fn keytip_table() -> Shortcuts {
        Shortcuts {
            entries: vec![larrygram_ui_spec::Shortcut {
                sequence: vec![vec!["Alt".to_owned(), "H".to_owned()], vec!["F".to_owned()]],
                command: larrygram_ui_spec::CommandId::parse("arrange.group").expect("id"),
                scope: ShortcutScope::Global,
                section: None,
            }],
        }
    }

    #[test]
    fn the_shell_loads_the_committed_table_without_conflicts() {
        // The same embedded `shortcuts.yaml` the command bar uses; the
        // committed table has no (sequence, scope) conflict (P4-14).
        let mut shell = full_shell();
        shell.set_shortcuts(&committed_shortcuts());
        assert!(shell.app.warnings.is_empty(), "{:?}", shell.app.warnings);
    }

    #[test]
    fn alt_f6_dispatches_view_zoom_in_through_the_registry() {
        // `Alt+F6` is `view.zoom.in` (`global` in shortcuts.yaml): a
        // synthesized key event in a real frame runs the command
        // through the registry and changes the zoom, and the key event
        // never reaches the widgets (P4-14 acceptance).
        let mut shell = full_shell();
        shell
            .app
            .open_document(DocumentSession::new(Document::new(), None));
        let zoom_before = shell.app.active_document().expect("document").view.zoom;
        let ctx = Context::default();
        // egui may run the closure for several passes (`run_ui`
        // multipass); the key event arrives in the first one. Collect
        // the intents of every pass, like the app does per pass.
        let mut events_after: Vec<usize> = Vec::new();
        let mut intents_seen: Vec<Intent> = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![key_event(
                    egui::Key::F6,
                    egui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                )],
                time: Some(0.0),
                ..Default::default()
            },
            |ui| {
                let intents = shell.show_in(ui).expect("the frame renders");
                events_after.push(ui.input(|input| input.events.len()));
                let results = shell.dispatch(&intents).expect("dispatch");
                assert!(results.iter().all(|result| result.is_ok()));
                intents_seen.extend(intents);
            },
        );
        output.textures_delta.clear();
        let fired: Vec<&Intent> = intents_seen
            .iter()
            .filter(|intent| intent.invocation.source == Source::Shortcut)
            .collect();
        assert_eq!(fired.len(), 1, "{intents_seen:?}");
        assert_eq!(fired[0].command.as_str(), "view.zoom.in");
        assert!(
            events_after.iter().all(|left| *left == 0),
            "the key event does not reach the widgets: {events_after:?}"
        );
        let zoom_after = shell.app.active_document().expect("document").view.zoom;
        assert!(zoom_after > zoom_before, "view.zoom.in changed the zoom");
    }

    #[test]
    fn a_half_typed_sequence_shows_in_the_status_line() {
        let mut shell = full_shell();
        shell.set_shortcuts(&keytip_table());
        shell_frame(&mut shell, Vec::new(), 0.0);
        assert_eq!(shell.status_line(), "");

        // `Alt+H` starts the sequence: the chords typed so far show.
        shell_frame(
            &mut shell,
            vec![key_event(
                egui::Key::H,
                egui::Modifiers {
                    alt: true,
                    ..Default::default()
                },
            )],
            1.0,
        );
        assert!(shell.shortcuts.pending(), "half-typed");
        assert_eq!(shell.status_line(), "Alt+H");

        // `F` completes it: the status line is the message again.
        shell_frame(
            &mut shell,
            vec![key_event(egui::Key::F, egui::Modifiers::default())],
            2.0,
        );
        assert!(!shell.shortcuts.pending());
        assert_eq!(shell.status_line(), shell.status());
    }

    #[test]
    fn clicking_the_canvas_or_the_command_bar_sets_the_focus_scope() {
        let mut shell = full_shell();
        shell_frame(&mut shell, Vec::new(), 0.0);
        let canvas = shell.canvas_rect.expect("the canvas rect is recorded");
        let tab_bar = shell
            .tab_bar_rect
            .expect("the command bar rect is recorded");
        assert_eq!(shell.focus, ShortcutScope::Global);

        shell_frame(&mut shell, vec![press_event(canvas.center())], 1.0);
        assert_eq!(shell.focus, ShortcutScope::Page);

        shell_frame(&mut shell, vec![press_event(tab_bar.center())], 2.0);
        assert_eq!(shell.focus, ShortcutScope::CommandBar);

        shell_frame(&mut shell, vec![press_event(egui::pos2(0.0, 0.0))], 3.0);
        assert_eq!(shell.focus, ShortcutScope::Global);
    }

    #[test]
    fn shortcut_conflicts_reach_the_warnings_at_load() {
        let mut shell = shell();
        let table = Shortcuts {
            entries: vec![
                larrygram_ui_spec::Shortcut {
                    sequence: vec![vec!["Ctrl".to_owned(), "G".to_owned()]],
                    command: larrygram_ui_spec::CommandId::parse("arrange.group").expect("id"),
                    scope: ShortcutScope::Global,
                    section: None,
                },
                larrygram_ui_spec::Shortcut {
                    sequence: vec![vec!["Ctrl".to_owned(), "G".to_owned()]],
                    command: larrygram_ui_spec::CommandId::parse("arrange.ungroup").expect("id"),
                    scope: ShortcutScope::Global,
                    section: None,
                },
                larrygram_ui_spec::Shortcut {
                    sequence: vec![vec!["Ctrl".to_owned(), "H".to_owned()]],
                    command: larrygram_ui_spec::CommandId::parse("arrange.group").expect("id"),
                    scope: ShortcutScope::Global,
                    section: None,
                },
            ],
        };
        shell.set_shortcuts(&table);
        assert_eq!(
            shell.app.warnings.len(),
            1,
            "one real conflict, the rest is legal"
        );
        assert!(shell.app.warnings[0].contains("conflicting shortcut"));
    }

    #[test]
    fn a_table_that_fails_to_load_records_one_warning() {
        let mut shell = shell();
        let bad = Shortcuts {
            entries: vec![larrygram_ui_spec::Shortcut {
                sequence: vec![vec!["Warp".to_owned()]],
                command: larrygram_ui_spec::CommandId::parse("edit.undo").expect("id"),
                scope: ShortcutScope::Global,
                section: None,
            }],
        };
        shell.set_shortcuts(&bad);
        assert_eq!(shell.app.warnings.len(), 1);
        assert!(shell.app.warnings[0].contains("shortcuts not loaded"));
    }

    #[test]
    fn a_disabled_command_shows_its_reason_in_the_status_bar() {
        /// A command whose enabled-state says no, with a reason.
        struct Disabled;

        impl Command for Disabled {
            fn meta(&self) -> &CommandMeta {
                use std::sync::OnceLock;
                static META: OnceLock<CommandMeta> = OnceLock::new();
                META.get_or_init(|| {
                    CommandMeta::new(
                        CommandId::parse("file.disabled").expect("id"),
                        "Disabled",
                        CommandStatus::Implemented,
                    )
                })
            }
            fn enabled(&self, _ctx: &AppCtx) -> Enabled {
                Enabled::no("nothing to close")
            }
            fn execute(
                &self,
                _ctx: &mut AppCtx,
                _invocation: &Invocation,
            ) -> Result<Outcome, CommandError> {
                Ok(Outcome::message("ran"))
            }
        }

        let mut registry = Registry::new();
        registry.register(Box::new(Disabled)).expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        let results = shell
            .dispatch(&[intent("file.disabled", Source::Shortcut)])
            .expect("dispatch");
        assert!(matches!(results[0], Err(CommandError::NotEnabled { .. })));
        // The reason reaches the status bar for any intent source.
        assert!(
            shell
                .app
                .warnings
                .last()
                .is_some_and(|warning| warning.contains("nothing to close")),
            "{:?}",
            shell.app.warnings
        );
    }

    /// A command that fails with an ADR 0026 one-line report (the
    /// corrupt-file case of the P4-16 review major 2).
    struct Fails;

    impl Command for Fails {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.fails").expect("id"),
                    "Fails",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            _ctx: &mut AppCtx,
            _invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            Err(CommandError::failed("cannot read input: corrupt.vsdx"))
        }
    }

    #[test]
    fn a_failed_command_reaches_the_status_bar() {
        // P4-16 review major 2: every `Err` `Shell::dispatch` gets
        // back for an intent becomes one `note_error` line (ADR 0021),
        // not only `NotEnabled`. The error itself still goes back to
        // the caller untouched.
        let mut registry = Registry::new();
        registry.register(Box::new(Fails)).expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        let results = shell
            .dispatch(&[intent("file.fails", Source::Ui)])
            .expect("dispatch");
        assert!(matches!(results[0], Err(CommandError::Failed { .. })));
        assert_eq!(
            shell.app.warnings.last().map(String::as_str),
            Some("cannot read input: corrupt.vsdx"),
            "the one-line report reaches the status bar: {:?}",
            shell.app.warnings
        );
    }

    #[test]
    fn a_programmatic_payload_goes_back_untouched() {
        // P4-16 review major 3: a save dialog (or a wasm download) is
        // for interactive sources only; a programmatic outcome keeps
        // its payload and the picker is never touched (AGENTS.md
        // section 6).
        let mut registry = Registry::new();
        registry.register(Box::new(MakesPayload)).expect("register");
        let saves = Rc::new(RefCell::new(Vec::new()));
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: None,
            open_calls: Rc::new(RefCell::new(0)),
            save_calls: Rc::clone(&saves),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.makes_payload", Source::Control)])
            .expect("dispatch");
        let outcome = results[0].as_ref().expect("the outcome comes back");
        assert!(
            matches!(outcome.payload, Some(Payload::Bytes { .. })),
            "the payload is still there: {outcome:?}"
        );
        assert!(saves.borrow().is_empty(), "no dialog, no download");
    }

    #[test]
    fn a_programmatic_quit_only_returns_the_request() {
        // The window commands have no shell intercept (P4-16): a
        // programmatic `app.quit` / `app.office.center.options` gets
        // its `UiRequest` back and the shell does nothing (never a
        // silent quit, never a dialog).
        let mut shell = full_shell();
        for (id, request) in [
            ("app.quit", UiRequest::Quit),
            ("app.office.center.options", UiRequest::ShowOptions),
        ] {
            let results = shell
                .dispatch(&[intent(id, Source::Mcp)])
                .expect("dispatch");
            assert_eq!(
                results[0].as_ref().expect(id).ui_request,
                Some(request),
                "{id}"
            );
        }
        assert!(!shell.quit_requested());
        assert!(!shell.options_open);
    }

    /// A command that demands a path once and then fails with it.
    struct FailsAfterPick;

    impl Command for FailsAfterPick {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.fails_after_pick").expect("id"),
                    "Fails After Pick",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            _ctx: &mut AppCtx,
            invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            if invocation.args.path("path").is_some() {
                Err(CommandError::failed("cannot read input: bad.vsdx"))
            } else {
                Err(CommandError::NeedsInput(NeededInput::Path {
                    purpose: PathPurpose::OpenFile,
                    suggested: None,
                }))
            }
        }
    }

    #[test]
    fn a_failed_retry_after_a_pick_reports_the_error() {
        // The `Ask::Again` error arm (P4-12 coverage major 2): the
        // re-dispatch with the picked path fails and its error reaches
        // the status bar like any other.
        let mut registry = Registry::new();
        registry
            .register(Box::new(FailsAfterPick))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: Some(FileChoice::Path("/tmp/bad.vsdx".into())),
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.fails_after_pick", Source::Ui)])
            .expect("dispatch");
        assert!(matches!(results[0], Err(CommandError::Failed { .. })));
        assert_eq!(
            shell.app.warnings.last().map(String::as_str),
            Some("cannot read input: bad.vsdx")
        );
    }

    #[test]
    fn the_pending_arm_waits_for_an_asynchronous_pick() {
        // The `Ask::Pending` arm (P4-12 coverage major 2): the picker
        // has an asynchronous pick in flight (the wasm upload) and the
        // command waits for `finish_pick`.
        let mut registry = Registry::new();
        let runs = Arc::new(AtomicUsize::new(0));
        registry
            .register(Box::new(NeedsPath {
                runs: Arc::clone(&runs),
            }))
            .expect("register");
        let mut shell = Shell::new(
            registry,
            LayoutSets::default(),
            AppCtx::new(Settings::default()),
        );
        shell.set_picker(Box::new(FakePicker {
            choice: None,
            poll_state: PickState::Pending,
            ..FakePicker::default()
        }));
        let results = shell
            .dispatch(&[intent("file.needs_path", Source::Ui)])
            .expect("dispatch");
        let outcome = results[0].as_ref().expect("waiting");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Waiting for the file dialog")
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1, "no re-dispatch yet");
    }

    /// A command that opens a document and produces an undoable edit
    /// (the P4-15 open issue 4 shape).
    struct OpensAndEdits;

    impl Command for OpensAndEdits {
        fn meta(&self) -> &CommandMeta {
            use std::sync::OnceLock;
            static META: OnceLock<CommandMeta> = OnceLock::new();
            META.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("file.opens_and_edits").expect("id"),
                    "Opens And Edits",
                    CommandStatus::Implemented,
                )
            })
        }
        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            Enabled::yes()
        }
        fn execute(
            &self,
            ctx: &mut AppCtx,
            _invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            ctx.open_document(DocumentSession::new(Document::new(), None));
            let mut outcome = Outcome::message("edited");
            outcome.edit = Some(Box::new(NoopEdit));
            Ok(outcome)
        }
    }

    #[test]
    fn an_edit_lands_on_the_session_the_command_ran_against() {
        // P4-15 open issue 4: the edit belongs to the session the
        // command ran against (the active one before the run), not to
        // the document the command opened.
        let mut registry = Registry::new();
        registry
            .register(Box::new(OpensAndEdits))
            .expect("register");
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(Document::new(), None));
        let id = CommandId::parse("file.opens_and_edits").expect("id");
        dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test)).expect("run");
        assert_eq!(app.documents.len(), 2);
        assert!(
            app.documents[0].undo.can_undo(),
            "the edit is on the session it ran against"
        );
        assert!(
            !app.documents[1].undo.can_undo(),
            "not on the document the command opened"
        );
    }

    #[test]
    fn an_edit_lands_on_the_session_the_command_opened_first() {
        // With no session before the run the command opened the first
        // document and the edit belongs to it.
        let mut registry = Registry::new();
        registry
            .register(Box::new(OpensAndEdits))
            .expect("register");
        let mut app = AppCtx::new(Settings::default());
        let id = CommandId::parse("file.opens_and_edits").expect("id");
        dispatch_one(&registry, &mut app, &id, Invocation::empty(Source::Test)).expect("run");
        assert_eq!(app.documents.len(), 1);
        assert!(app.documents[0].undo.can_undo());
    }
}
