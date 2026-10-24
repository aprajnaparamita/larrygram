//! The Panel Classic widgets: menu bar, menus, toolbars, context
//! menus (ADR 0061, `docs/ui/panel-classic.md`).
//!
//! The widgets read a [`LayoutV2`] and return [`Intent`]s; they never
//! execute commands themselves (ADR 0028). A menu bar per window mode
//! (`drawing`, `shapesheet`, `stencil`), the icon-gutter menus with
//! submenus and run-time lists, the docked and floating toolbars with
//! split buttons, palettes and combos, and the context menus.
//!
//! State that must survive frames (the open menu, the toolbar
//! visibility per window mode, the `split_recent` choices and the
//! combo edit buffers) lives in [`PanelClassic`]; the shell owns one
//! instance (task P7-09). The free [`show`] keeps the P7-01 stub
//! signature and stores the state in the egui memory instead.
//!
//! Colours come from the egui defaults until task P7-05 merges the
//! theme tokens.

pub mod menu;
pub mod rows;
pub mod toolbar;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use egui::{Id, Pos2, Rect, Ui};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{CommandId, Registry, Source};
use larrygram_ui_spec::v2::{ContextId, Item, LayoutV2, Menu, MenuBar};

use crate::Intent;
use crate::lists::{ListProvider, LiveLists};
use menu::MenuOutcome;
use rows::{Activation, Row};

pub use crate::UiError;

/// Tooltip of a control whose command is a stub (ADR 0031; the exact
/// string is the contract the UI tests pin).
pub(crate) const NOT_YET_IMPLEMENTED: &str = "Not yet implemented";

/// Tooltip of a control listed in `ui-spec/out-of-scope.txt` (ADR 0031).
pub(crate) const OUT_OF_SCOPE: &str = "Out of scope for this run";

/// Recursion cap for nested menu items (AGENTS.md section 6).
pub(crate) const MAX_DEPTH: u32 = 8;

/// Hover delay before a submenu opens, in seconds
/// (`docs/ui/panel-classic.md`: "about 400 ms").
const SUBMENU_DELAY: f64 = 0.4;

/// Height of the menu bar (`docs/ui/panel-classic.md`).
const MENU_BAR: f32 = 24.0;

/// Which window is active: it selects the menu bar and the mode
/// toolbars (`docs/ui/panel-classic.md` "Menu bars per window mode").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowMode {
    /// A drawing window is active (the default bar).
    #[default]
    Drawing,
    /// A ShapeSheet window is active.
    Shapesheet,
    /// A stencil is open for editing.
    Stencil,
}

impl WindowMode {
    /// The window-mode id (`drawing`, `shapesheet`, `stencil`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Drawing => "drawing",
            Self::Shapesheet => "shapesheet",
            Self::Stencil => "stencil",
        }
    }

    /// The context the data names the mode with (`shown_in`).
    pub fn context(self) -> Option<ContextId> {
        match self {
            Self::Drawing => None,
            Self::Shapesheet => Some(ContextId::ShapesheetWindow),
            Self::Stencil => Some(ContextId::StencilEdit),
        }
    }
}

/// The `checked` state of a toggle or check command.
///
/// The command registry has no checked-state seam yet (the P5-17
/// Result, Open issues), so the caller supplies one; the widgets read
/// it for latched buttons and check marks.
pub trait CheckedState {
    /// `Some(on)` when the command reports a checked state.
    fn is_checked(&self, command: &str, app: &AppCtx) -> Option<bool>;
}

/// A checked state that reports nothing (no latched controls).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoChecks;

impl CheckedState for NoChecks {
    fn is_checked(&self, _command: &str, _app: &AppCtx) -> Option<bool> {
        None
    }
}

/// The latched state of the tool commands, from
/// [`AppCtx::active_tool`] (the P5-17 rule: the shell derives tool
/// state from the active tool).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolLatch;

impl CheckedState for ToolLatch {
    fn is_checked(&self, command: &str, app: &AppCtx) -> Option<bool> {
        use larrygram_commands::app::ActiveTool;
        let tool = match command {
            "tools.pointer" => Some(ActiveTool::Pointer),
            "tools.rectangle" | "shape.rectangle" => Some(ActiveTool::Rectangle),
            "tools.ellipse" | "shape.ellipse" => Some(ActiveTool::Ellipse),
            "shape.line" => Some(ActiveTool::Line),
            "tools.connector" => Some(ActiveTool::Connector),
            "tools.text" => Some(ActiveTool::Text),
            _ => return None,
        };
        Some(tool.is_some_and(|tool| app.active_tool() == tool))
    }
}

/// Everything the widgets read beyond the layout (task P7-06).
///
/// The shell builds one per frame (task P7-09); the tests build one
/// over a hand-made [`LayoutV2`] and the builtin registry.
pub struct PanelInput<'a> {
    /// The Panel Classic layout data.
    pub layout: &'a LayoutV2,
    /// The command registry (ADR 0028).
    pub registry: &'a Registry,
    /// The application state (enabled states, lists).
    pub app: &'a AppCtx,
    /// The active window mode (which menu bar to draw).
    pub mode: WindowMode,
    /// The run-time lists (`recent_files`, `windows`, `pages`, ...).
    pub lists: &'a dyn ListProvider,
    /// The `checked` state of toggle commands, when there is one.
    pub checked: Option<&'a dyn CheckedState>,
}

impl<'a> PanelInput<'a> {
    /// An input over `layout`, `registry` and `app` in
    /// [`WindowMode::Drawing`] with [`LiveLists`] and no checked
    /// state.
    pub fn new(layout: &'a LayoutV2, registry: &'a Registry, app: &'a AppCtx) -> Self {
        Self {
            layout,
            registry,
            app,
            mode: WindowMode::Drawing,
            lists: &LiveLists,
            checked: None,
        }
    }

    /// The input for another window mode.
    pub fn with_mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// The input with its run-time list provider.
    pub fn with_lists(mut self, lists: &'a dyn ListProvider) -> Self {
        self.lists = lists;
        self
    }

    /// The input with its checked state.
    pub fn with_checked(mut self, checked: &'a dyn CheckedState) -> Self {
        self.checked = Some(checked);
        self
    }
}

/// The run-time state of one Panel Classic chrome.
#[derive(Debug, Clone)]
pub struct PanelClassic {
    /// The open top-level menu, if any.
    open: Option<usize>,
    /// Alt or F10 put the focus on the menu bar.
    bar_focused: bool,
    /// The focused title index.
    title_focus: usize,
    /// The highlighted row per open level.
    highlight: Vec<Option<usize>>,
    /// The open submenu row per level below the top menu.
    chain: Vec<usize>,
    /// A pending submenu hover: (level, row, start time).
    hover_timer: Option<(usize, usize, f64)>,
    /// Alt was tapped with no other key (toggles the bar focus).
    alt_tapped: bool,
    /// The open menu was switched by hovering a title: the click on it
    /// keeps it open instead of toggling it closed.
    hover_switched: bool,
    /// The title rects of the last frame (outside-click closing).
    title_rects: Vec<Rect>,
    /// The open menu rects of the last frame.
    level_rects: Vec<Rect>,
    /// Toolbar visibility: `"<mode id>/<toolbar id>"` to the user's
    /// choice (the per-window-mode memory of `Settings`; task P7-09
    /// moves it onto `Settings`).
    toolbar_visible: BTreeMap<String, bool>,
    /// `split_recent` item id to the last child chosen.
    split_recent: BTreeMap<String, String>,
    /// Combo id to the text being edited.
    combo_text: BTreeMap<String, String>,
    /// Combo id to the last applied text (Esc reverts to it).
    combo_applied: BTreeMap<String, String>,
    /// The toolbar item with an open dropdown or palette popup.
    open_popup: Option<String>,
    /// The anchor rect of the open popup.
    popup_rect: Rect,
    /// The open context menu id.
    context: Option<String>,
    /// Where the open context menu draws.
    context_pos: Pos2,
    /// The context menu Shift+F10 opens (P7-08's hit-target mapping).
    context_target: Option<String>,
    /// The help search box text ("Type a question for help").
    help_search: String,
}

impl Default for PanelClassic {
    fn default() -> Self {
        Self {
            open: None,
            bar_focused: false,
            title_focus: 0,
            highlight: Vec::new(),
            chain: Vec::new(),
            hover_timer: None,
            alt_tapped: false,
            hover_switched: false,
            title_rects: Vec::new(),
            level_rects: Vec::new(),
            toolbar_visible: BTreeMap::new(),
            split_recent: BTreeMap::new(),
            combo_text: BTreeMap::new(),
            combo_applied: BTreeMap::new(),
            open_popup: None,
            popup_rect: Rect::NOTHING,
            context: None,
            context_pos: Pos2::ZERO,
            context_target: None,
            help_search: String::new(),
        }
    }
}

impl PanelClassic {
    /// A fresh chrome with the data's default toolbar visibility.
    pub fn new() -> Self {
        Self::default()
    }

    /// The toolbar id whose `label` matches, for the View > Toolbars
    /// rows (a label with no toolbar maps to a synthetic id).
    pub fn toolbar_id_for_label(&self, layout: &LayoutV2, label: &str) -> String {
        layout
            .toolbars
            .iter()
            .find(|toolbar| toolbar.label.as_deref() == Some(label))
            .map(|toolbar| toolbar.id.clone())
            .unwrap_or_else(|| format!("toolbar.{}", label.to_ascii_lowercase().replace(' ', "_")))
    }

    /// Whether `toolbar` is visible in `mode`: the user's choice, else
    /// the data's `visible` default.
    pub fn toolbar_visible_in(&self, layout: &LayoutV2, mode: WindowMode, toolbar: &str) -> bool {
        let key = format!("{}/{}", mode.id(), toolbar);
        if let Some(visible) = self.toolbar_visible.get(&key) {
            return *visible;
        }
        layout
            .toolbars
            .iter()
            .find(|entry| entry.id == toolbar)
            .is_some_and(|entry| entry.visible)
    }

    /// Sets the visibility of `toolbar` in `mode`.
    pub fn set_toolbar_visible_in(&mut self, mode: WindowMode, toolbar: &str, visible: bool) {
        let key = format!("{}/{}", mode.id(), toolbar);
        self.toolbar_visible.insert(key, visible);
    }

    /// The context menu Shift+F10 opens (the P7-08 mapping).
    pub fn set_context_target(&mut self, menu_id: impl Into<String>) {
        self.context_target = Some(menu_id.into());
    }

    /// Opens the context menu `menu_id` at the pointer (Shift+F10).
    pub fn open_context_menu(&mut self, menu_id: impl Into<String>) {
        self.context = Some(menu_id.into());
    }

    /// Draws the chrome (the menu bar and the toolbars) and returns
    /// one [`Intent`] per activation (ADR 0028).
    pub fn show(&mut self, ui: &mut Ui, input: &PanelInput<'_>) -> Result<Vec<Intent>, UiError> {
        let mut intents = Vec::new();
        let mut levels = self.build_levels(input);
        self.handle_keys(ui, input, &mut levels, &mut intents);
        self.close_on_outside_click(ui);
        self.draw_menu_bar(ui, input, &mut intents);
        // The keys and the titles may have opened or closed a menu:
        // rebuild the rows before drawing them.
        let mut levels = self.build_levels(input);
        self.draw_open_menus(ui, input, &mut levels, &mut intents);
        toolbar::draw_toolbars(ui, input, self, &mut intents);
        Ok(intents)
    }

    /// Opens `menu_id` when `trigger` was right-clicked, and draws the
    /// open context menu at the pointer.
    ///
    /// The trigger is the hit target P7-08 maps (a shape, a page tab,
    /// ...); `None` draws the menu that
    /// [`PanelClassic::open_context_menu`] named.
    pub fn context_menu(
        &mut self,
        ui: &mut Ui,
        input: &PanelInput<'_>,
        menu_id: &str,
        trigger: Option<&egui::Response>,
    ) -> Result<Vec<Intent>, UiError> {
        let mut intents = Vec::new();
        if let Some(trigger) = trigger
            && trigger.secondary_clicked()
        {
            self.context = Some(menu_id.to_owned());
            self.context_pos = trigger.rect.center();
        }
        if self.context.as_deref() != Some(menu_id) {
            return Ok(intents);
        }
        let Some(context) = input
            .layout
            .context_menus
            .iter()
            .find(|context| context.id == menu_id)
        else {
            self.context = None;
            return Ok(intents);
        };
        let rows = rows::rows(&context.items, input, self, Source::Menu, false, 0);
        let width = menu::menu_width(ui, &rows);
        let pos = self.context_pos;
        let id = Id::unique(("panel_classic_context", menu_id));
        let mut outcome = MenuOutcome::default();
        let inner = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    outcome = menu::draw_menu(ui, &rows, width, None, true);
                });
            });
        let area = inner.response.rect;
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.context = None;
        }
        if let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            && ui.input(|i| i.pointer.any_click())
            && !area.contains(pointer)
        {
            self.context = None;
        }
        if let Some(index) = outcome.clicked {
            if let Some(row) = rows.get(index) {
                self.activate(row, input, &mut intents);
            }
            self.context = None;
        }
        Ok(intents)
    }

    /// Activates one row: dispatches its intent and closes the menus,
    /// or flips a toolbar toggle.
    fn activate(&mut self, row: &Row, input: &PanelInput<'_>, intents: &mut Vec<Intent>) -> bool {
        match &row.activation {
            Activation::Intent(intent) => {
                intents.push(intent.clone());
                self.close_all();
                true
            }
            Activation::ToolbarToggle { toolbar, intent } => {
                if let Some(intent) = intent {
                    intents.push(intent.clone());
                }
                let visible = self.toolbar_visible_in(input.layout, input.mode, toolbar);
                self.set_toolbar_visible_in(input.mode, toolbar, !visible);
                self.close_all();
                true
            }
            _ => false,
        }
    }

    /// Closes every open menu and popup.
    fn close_all(&mut self) {
        self.open = None;
        self.chain.clear();
        self.highlight.clear();
        self.hover_timer = None;
        self.open_popup = None;
    }

    // --- menus -----------------------------------------------------

    /// The rows of every open level: the top menu, then one level per
    /// open submenu chain link.
    fn build_levels(&self, input: &PanelInput<'_>) -> Vec<Vec<Row>> {
        let Some(open) = self.open else {
            return Vec::new();
        };
        let Some(menu) = self.menus_of(input).get(open) else {
            return Vec::new();
        };
        let mut levels = vec![rows::rows(&menu.items, input, self, Source::Menu, false, 0)];
        for link in &self.chain {
            let Some(row) = levels
                .last()
                .and_then(|level| level.get(*link))
                .filter(|row| row.has_children())
            else {
                break;
            };
            levels.push(row.children.clone());
        }
        levels
    }

    /// The menus of the mode's menu bar (`docs/ui/panel-classic.md`
    /// "Menu bars per window mode").
    fn menus_of<'a>(&self, input: &PanelInput<'a>) -> &'a [Menu] {
        menu_menus(input)
    }

    /// The menu bar: titles left, the help search box at the right
    /// end. Hover switches menus while one is open; click opens.
    fn draw_menu_bar(&mut self, ui: &mut Ui, input: &PanelInput<'_>, intents: &mut Vec<Intent>) {
        let menus = self.menus_of(input).to_vec();
        let underlines = self.bar_focused || self.open.is_some();
        let mut rects = Vec::new();
        ui.horizontal(|ui| {
            ui.set_height(MENU_BAR);
            for (index, menu) in menus.iter().enumerate() {
                let open = self.open == Some(index);
                let response = menu::draw_title(ui, &menu.label, menu.access, open, underlines);
                rects.push(response.rect);
                if response.clicked() {
                    if open && !self.hover_switched {
                        self.close_all();
                    } else {
                        self.open_menu(index);
                    }
                    self.hover_switched = false;
                } else if response.hovered() && self.open.is_some() && self.open != Some(index) {
                    self.open_menu(index);
                    self.hover_switched = true;
                }
            }
            // The help search box ("Type a question for help"), which
            // runs `help.search` with the typed text.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let edit = egui::TextEdit::singleline(&mut self.help_search)
                    .hint_text("Type a question for help")
                    .desired_width(160.0);
                let response = ui.add(edit);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::accesskit::Role::ComboBox, true, "Help search")
                });
                let submitted =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submitted && !self.help_search.is_empty() {
                    if let Ok(id) = CommandId::parse("help.search") {
                        let mut args = larrygram_commands::Args::new();
                        args.set(
                            "text",
                            larrygram_commands::ArgValue::Text(self.help_search.clone()),
                        );
                        intents.push(Intent::new(
                            id,
                            larrygram_commands::Invocation::new(args, Source::Menu),
                        ));
                    }
                    self.help_search.clear();
                }
            });
        });
        self.title_rects = rects;
    }

    /// Opens the top-level menu `index` with its first row highlighted.
    fn open_menu(&mut self, index: usize) {
        self.open = Some(index);
        self.chain.clear();
        self.highlight = vec![None];
        self.hover_timer = None;
    }

    /// Draws the open menu chain as foreground areas below the open
    /// title and to the right of each open submenu row.
    fn draw_open_menus(
        &mut self,
        ui: &mut Ui,
        input: &PanelInput<'_>,
        levels: &mut Vec<Vec<Row>>,
        intents: &mut Vec<Intent>,
    ) {
        if self.open.is_none() {
            self.level_rects.clear();
            return;
        }
        let underlines = true;
        let mut level_rects = Vec::new();
        let mut pos = self
            .title_rects
            .get(self.open.unwrap_or(0))
            .map(|rect| rect.left_bottom())
            .unwrap_or_else(|| ui.input(|i| i.pointer.interact_pos()).unwrap_or(Pos2::ZERO));
        let mut activated: Option<(usize, usize)> = None;
        let mut open_child: Option<(usize, usize)> = None;
        let mut hover: Option<(usize, usize)> = None;
        for (level, rows) in levels.iter().enumerate() {
            let width = menu::menu_width(ui, rows);
            let mut outcome = MenuOutcome::default();
            let highlight = self.highlight.get(level).copied().flatten();
            let level_pos = pos;
            let inner = egui::Area::new(Id::unique(("panel_classic_menu", level)))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        outcome = menu::draw_menu(ui, rows, width, highlight, underlines);
                    });
                });
            level_rects.push(inner.response.rect);
            let _ = level_pos;
            if let Some(index) = outcome.clicked {
                if rows.get(index).is_some_and(Row::has_children) {
                    open_child = Some((level, index));
                } else {
                    activated = Some((level, index));
                }
            }
            if let Some(index) = outcome.hovered {
                hover = Some((level, index));
                // A hover in a deeper level keeps the chain.
                if level < self.chain.len() && self.chain[level] == index {
                    // The hover is on the row whose submenu is open.
                }
            }
            // Position the next level at the open submenu row.
            if let Some(link) = self.chain.get(level).copied()
                && let Some(rect) = outcome.rects.get(link).copied()
            {
                let mut child_pos = Pos2::new(rect.right(), rect.top());
                let screen = ui.ctx().content_rect();
                if child_pos.x + width > screen.right() {
                    child_pos.x = pos.x - width;
                }
                pos = child_pos;
            }
        }

        // Hover: start or continue the submenu timer, close deeper
        // levels when the hover moves to a leaf row.
        self.update_hover(ui, levels, hover);
        if let Some((level, index)) = open_child {
            self.chain.truncate(level);
            self.chain.push(index);
            self.highlight.truncate(level + 1);
            self.highlight.push(first_focusable(
                levels.get(level + 1).map(Vec::as_slice).unwrap_or(&[]),
            ));
            self.hover_timer = None;
        }
        if let Some((level, index)) = activated {
            if let Some(row) = levels.get(level).and_then(|level| level.get(index)) {
                self.activate(row, input, intents);
            }
            levels.clear();
        }
        self.level_rects = level_rects;
    }

    /// The hover rules: a submenu opens after [`SUBMENU_DELAY`], any
    /// other row closes the deeper levels.
    fn update_hover(&mut self, ui: &Ui, levels: &[Vec<Row>], hover: Option<(usize, usize)>) {
        let Some((level, index)) = hover else {
            self.hover_timer = None;
            return;
        };
        let has_children = levels
            .get(level)
            .and_then(|level| level.get(index))
            .is_some_and(Row::has_children);
        let open_here =
            self.chain.get(level).copied() == Some(index) && self.chain.len() > level + 1;
        if !has_children {
            if self.chain.len() > level {
                self.chain.truncate(level);
                self.highlight.truncate(level + 1);
            }
            self.hover_timer = None;
            return;
        }
        if open_here {
            self.hover_timer = None;
            return;
        }
        let now = ui.input(|i| i.time);
        match self.hover_timer {
            Some((l, r, start)) if l == level && r == index => {
                if now - start >= SUBMENU_DELAY {
                    self.chain.truncate(level);
                    self.chain.push(index);
                    self.highlight.truncate(level + 1);
                    self.highlight.push(first_focusable(
                        levels.get(level + 1).map(Vec::as_slice).unwrap_or(&[]),
                    ));
                    self.hover_timer = None;
                }
            }
            _ => self.hover_timer = Some((level, index, now)),
        }
    }

    /// Keyboard: Alt/F10 focus, Alt+letter, arrows, Enter, Esc and the
    /// bare access letters (`docs/ui/panel-classic.md` "Menus").
    fn handle_keys(
        &mut self,
        ui: &Ui,
        input: &PanelInput<'_>,
        levels: &mut Vec<Vec<Row>>,
        intents: &mut Vec<Intent>,
    ) {
        let events = ui.input(|i| i.events.clone());
        let has_text = events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(_)));
        let mut modifiers = ui.input(|i| i.modifiers);
        for event in events {
            match event {
                egui::Event::ModifiersChanged(changed) => {
                    if self.alt_tapped && !changed.alt {
                        // A bare Alt tap: focus the menu bar.
                        self.bar_focused = !self.bar_focused;
                        self.title_focus = 0;
                    }
                    self.alt_tapped = changed.alt && !self.alt_tapped;
                    modifiers = changed;
                }
                egui::Event::Key {
                    key: egui::Key::AltLeft | egui::Key::AltRight,
                    pressed: true,
                    ..
                } => {
                    self.alt_tapped = true;
                }
                egui::Event::Key {
                    key: egui::Key::F10,
                    pressed: true,
                    ..
                } if modifiers.shift => {
                    // Shift+F10 opens the context menu of the current
                    // selection (P7-08 maps the target).
                    if let Some(target) = self.context_target.clone() {
                        self.open_context_menu(target);
                    }
                }
                egui::Event::Key {
                    key: egui::Key::F10,
                    pressed: true,
                    ..
                } => {
                    self.bar_focused = !self.bar_focused;
                    self.title_focus = 0;
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } if modifiers.alt => {
                    if let Some(letter) = key_char(key) {
                        self.open_by_access(input, letter);
                        self.alt_tapped = false;
                    }
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } if !modifiers.alt && !has_text && key_char(key).is_some() => {
                    // No text events in this batch (synthetic input):
                    // the letter keys stand in for the typed letters.
                    if let Some(letter) = key_char(key) {
                        self.handle_access_letter(input, levels, letter, intents);
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    if !self.chain.is_empty() {
                        self.chain.pop();
                        self.highlight.pop();
                    } else if self.open.is_some() {
                        self.close_all();
                    } else if self.bar_focused {
                        self.bar_focused = false;
                    }
                }
                egui::Event::Key {
                    key: key @ (egui::Key::ArrowDown | egui::Key::ArrowUp),
                    pressed: true,
                    ..
                } => {
                    self.move_highlight(levels, key == egui::Key::ArrowDown);
                }
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    pressed: true,
                    ..
                } => {
                    self.handle_right(input, levels);
                }
                egui::Event::Key {
                    key: egui::Key::ArrowLeft,
                    pressed: true,
                    ..
                } => {
                    if !self.chain.is_empty() {
                        self.chain.pop();
                        self.highlight.pop();
                    } else {
                        self.cycle_title(input, -1);
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Space,
                    pressed: true,
                    ..
                } => {
                    self.activate_highlighted(input, levels, intents);
                }
                egui::Event::Text(text) => {
                    if let Some(letter) = text.chars().next()
                        && text.chars().count() == 1
                    {
                        self.handle_access_letter(input, levels, letter, intents);
                    }
                }
                _ => {}
            }
        }
    }

    /// Opens the top-level menu whose access key is `letter`.
    fn open_by_access(&mut self, input: &PanelInput<'_>, letter: char) {
        let wanted = letter.to_ascii_lowercase();
        let menus = self.menus_of(input);
        if let Some(index) = menus.iter().position(|menu| {
            menu.access
                .is_some_and(|access| access.to_ascii_lowercase() == wanted)
        }) {
            self.open_menu(index);
            self.bar_focused = true;
        }
    }

    /// Activates the row whose access key is `letter` in the open
    /// menu; with the bar focused but no menu open, Alt-less letters
    /// do nothing.
    fn handle_access_letter(
        &mut self,
        input: &PanelInput<'_>,
        levels: &mut Vec<Vec<Row>>,
        letter: char,
        intents: &mut Vec<Intent>,
    ) {
        if self.open.is_none() {
            return;
        }
        let wanted = letter.to_ascii_lowercase();
        let found = levels.last().and_then(|level| {
            level
                .iter()
                .enumerate()
                .find(|(_, row)| {
                    row.access
                        .is_some_and(|access| access.to_ascii_lowercase() == wanted)
                })
                .map(|(index, _)| index)
        });
        let Some(index) = found else { return };
        if let Some(row) = levels.last().and_then(|level| level.get(index)) {
            if row.has_children() {
                self.chain.truncate(self.highlight.len().saturating_sub(1));
                self.chain.push(index);
                self.highlight.push(first_focusable(&row.children));
            } else {
                self.activate(row, input, intents);
                levels.clear();
            }
        }
    }

    /// Moves the keyboard highlight to the next or previous focusable
    /// row of the deepest open level.
    fn move_highlight(&mut self, levels: &[Vec<Row>], down: bool) {
        if self.open.is_none() {
            return;
        }
        let level_index = self.chain.len();
        let Some(level) = levels.get(level_index) else {
            return;
        };
        let current = self.highlight.get(level_index).copied().flatten();
        let mut order = level
            .iter()
            .enumerate()
            .filter(|(_, row)| row.focusable())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if !down {
            order.reverse();
        }
        if order.is_empty() {
            return;
        }
        let next =
            match current.and_then(|current| order.iter().position(|index| *index == current)) {
                Some(position) => order[(position + 1) % order.len()],
                None => order[0],
            };
        while self.highlight.len() <= level_index {
            self.highlight.push(None);
        }
        self.highlight[level_index] = Some(next);
    }

    /// Arrow right: open the highlighted submenu, else move to the
    /// next top-level menu.
    fn handle_right(&mut self, input: &PanelInput<'_>, levels: &[Vec<Row>]) {
        let level_index = self.chain.len();
        let highlighted = self.highlight.get(level_index).copied().flatten();
        let opens = levels
            .get(level_index)
            .and_then(|level| highlighted.and_then(|index| level.get(index)))
            .is_some_and(Row::has_children);
        if opens {
            let index = highlighted.unwrap_or(0);
            let children = levels
                .get(level_index)
                .and_then(|level| level.get(index))
                .map(|row| row.children.clone())
                .unwrap_or_default();
            self.chain.truncate(level_index);
            self.chain.push(index);
            self.highlight.truncate(level_index + 1);
            self.highlight.push(first_focusable(&children));
        } else {
            self.cycle_title(input, 1);
        }
    }

    /// Arrow left with no submenu open: the previous top-level menu.
    fn cycle_title(&mut self, input: &PanelInput<'_>, step: i32) {
        let count = self.menus_of(input).len();
        if count == 0 {
            return;
        }
        let current = self.open.unwrap_or(self.title_focus);
        let next = (current as i32 + step).rem_euclid(count as i32) as usize;
        if self.open.is_some() {
            self.open_menu(next);
        } else {
            self.title_focus = next;
            self.bar_focused = true;
        }
    }

    /// Enter or Space on the highlighted row.
    fn activate_highlighted(
        &mut self,
        input: &PanelInput<'_>,
        levels: &mut Vec<Vec<Row>>,
        intents: &mut Vec<Intent>,
    ) {
        if self.bar_focused && self.open.is_none() {
            self.open_menu(self.title_focus);
            return;
        }
        let level_index = self.chain.len();
        let highlighted = self.highlight.get(level_index).copied().flatten();
        let Some(row) = levels
            .get(level_index)
            .and_then(|level| highlighted.and_then(|index| level.get(index)))
        else {
            return;
        };
        if row.has_children() {
            let children = row.children.clone();
            let index = highlighted.unwrap_or(0);
            self.chain.truncate(level_index);
            self.chain.push(index);
            self.highlight.truncate(level_index + 1);
            self.highlight.push(first_focusable(&children));
        } else {
            self.activate(row, input, intents);
            levels.clear();
        }
    }

    /// A click outside every open menu and title closes all.
    fn close_on_outside_click(&mut self, ui: &Ui) {
        let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        if !ui.input(|i| i.pointer.any_click()) {
            return;
        }
        let on_title = self.title_rects.iter().any(|rect| rect.contains(pointer));
        let on_menu = self.level_rects.iter().any(|rect| rect.contains(pointer));
        if !on_title && !on_menu {
            self.close_all();
        }
    }
}

/// The first focusable row of a level (the keyboard entry point).
fn first_focusable(rows: &[Row]) -> Option<usize> {
    rows.iter().position(Row::focusable)
}

/// The menus of the input's window mode (empty when the mode has no
/// bar): `shown_in` names the mode (`docs/ui/layout-format.md`).
fn menu_menus<'a>(input: &PanelInput<'a>) -> &'a [Menu] {
    let wanted = input.mode.context();
    menu_bars(input)
        .iter()
        .find(|bar| bar.shown_in == wanted)
        .map(|bar| bar.menus.as_slice())
        .unwrap_or(&[])
}

/// The menu bars of the layout.
fn menu_bars<'a>(input: &PanelInput<'a>) -> &'a [MenuBar] {
    &input.layout.menu_bars
}

/// The `Item` with `id`, searched recursively.
pub(crate) fn find_item<'a>(items: &'a [Item], id: &str) -> Option<&'a Item> {
    items.iter().find_map(|item| {
        if item.id == id {
            Some(item)
        } else {
            find_item(&item.items, id)
        }
    })
}

/// The character an egui key types (`A`..`Z`, `0`..`9`).
fn key_char(key: egui::Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(first), None) if first.is_ascii_alphanumeric() => Some(first.to_ascii_lowercase()),
        _ => None,
    }
}

/// Draws the Panel Classic chrome with the default input (the P7-01
/// entry point).
///
/// The shell (task P7-09) calls [`PanelClassic::show`] with a real
/// [`PanelInput`]; this wrapper keeps the stub signature for callers
/// that have no registry yet and stores the state in the egui memory.
pub fn show(ui: &mut Ui, layout: &LayoutV2) -> Result<Vec<Intent>, UiError> {
    let id = Id::unique("larrygram.panel_classic");
    let mut state = ui
        .data_mut(|data| data.get_persisted::<PanelClassic>(id))
        .unwrap_or_default();
    let registry = Registry::default();
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(layout, &registry, &app);
    let produced = state.show(ui, &input);
    ui.data_mut(|data| data.insert_persisted(id, state));
    produced
}
