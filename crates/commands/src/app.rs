//! Application state (ADR 0029): open documents, the active page, the
//! selection, view state and settings.
//!
//! Everything a command mutates lives here, in L4 (ADR 0002), so headless
//! callers (the CLI today, the JSON control channel and MCP in Phase 8)
//! share one state type with the UI. The egui crates never appear.

use std::collections::BTreeMap;
use std::path::PathBuf;

use larrygram_model::{Document, PageId};
use larrygram_ui_spec::LayoutKind;
use serde::{Deserialize, Serialize};

use crate::SettingsError;
use crate::clipboard::Clipboard;
use crate::error::CommandError;
use crate::undo::{Edit, UndoStack};

/// How the UI formats lengths at the screen edge (AGENTS.md section 6:
/// internal units stay inches; only presentation converts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DisplayUnits {
    /// Inches (the internal unit).
    #[default]
    Inches,
    /// Millimeters.
    Millimeters,
    /// Centimeters.
    Centimeters,
    /// Meters.
    Meters,
    /// Feet.
    Feet,
}

impl DisplayUnits {
    /// Every accepted name, in menu order. The `view.units` argument
    /// and the Options dialog use these spellings (P4-15).
    pub const NAMES: [&'static str; 5] = ["Inches", "Millimeters", "Centimeters", "Meters", "Feet"];

    /// The name as [`Self::NAMES`] spells it.
    pub fn name(self) -> &'static str {
        match self {
            DisplayUnits::Inches => "Inches",
            DisplayUnits::Millimeters => "Millimeters",
            DisplayUnits::Centimeters => "Centimeters",
            DisplayUnits::Meters => "Meters",
            DisplayUnits::Feet => "Feet",
        }
    }

    /// Parses a [`Self::NAMES`] spelling, ignoring ASCII case.
    ///
    /// ```
    /// use larrygram_commands::DisplayUnits;
    /// assert_eq!(DisplayUnits::parse("millimeters"), Some(DisplayUnits::Millimeters));
    /// assert_eq!(DisplayUnits::parse("furlongs"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "inches" => Some(DisplayUnits::Inches),
            "millimeters" => Some(DisplayUnits::Millimeters),
            "centimeters" => Some(DisplayUnits::Centimeters),
            "meters" => Some(DisplayUnits::Meters),
            "feet" => Some(DisplayUnits::Feet),
            _ => None,
        }
    }
}

/// The view state of one open document (ADR 0029).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    /// Page-space pan offset, in inches.
    pub pan: larrygram_geom::Point,
    /// View scale multiplier (1.0 = 100%); finite and positive.
    pub zoom: f64,
    /// Whether the grid is drawn (ADR 0033).
    pub show_grid: bool,
    /// Whether the rulers are drawn (ADR 0033).
    pub show_rulers: bool,
}

impl ViewState {
    /// The default view: no pan, 100% zoom, grid and rulers on.
    pub fn new() -> Self {
        Self {
            pan: larrygram_geom::Point::new(0.0, 0.0),
            zoom: 1.0,
            show_grid: true,
            show_rulers: true,
        }
    }

    /// The default view for a new document session: [`Self::new`],
    /// with the grid and rulers toggles seeded from the persisted
    /// settings (ADR 0032 amendment: `Settings::show_grid` and
    /// `Settings::show_rulers` are the defaults for new sessions).
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            show_grid: settings.show_grid,
            show_rulers: settings.show_rulers,
            ..Self::new()
        }
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self::new()
    }
}

/// The selected shapes (ADR 0029, extended by ADR 0041).
///
/// The selection is per application, not per document: the commands that
/// switch document or page clear it. `shapes` is in click order and the
/// last entry is the primary shape (the one the panes show).
///
/// `entered` is the group the user is "inside" (task P5-04, the
/// `shape.enter_group` / `shape.exit_group` state of ADR 0041): while it
/// is set, clicks and `edit.select_all` act on the group's members
/// instead of mapping every hit to its top-level ancestor. It is ours,
/// not a Visio field; the black-box check against Visio is on the
/// screenshot-pass list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// The selected shapes of the active page, in click order.
    pub shapes: Vec<larrygram_model::ShapeId>,
    /// The primary shape, or `None` while the selection is empty.
    pub anchor: Option<larrygram_model::ShapeId>,
    /// The page the selection belongs to.
    pub page: Option<PageId>,
    /// The group currently entered, or `None` at page level.
    pub entered: Option<larrygram_model::ShapeId>,
}

impl Selection {
    /// An empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of selected shapes.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// True while nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Whether `shape` is selected.
    pub fn contains(&self, shape: larrygram_model::ShapeId) -> bool {
        self.shapes.contains(&shape)
    }

    /// The primary shape (the panes' subject, ADR 0041).
    pub fn primary(&self) -> Option<larrygram_model::ShapeId> {
        self.anchor
    }

    /// Re-derives [`Self::page`] from the selected shapes.
    ///
    /// `PageId` is a list position (task P5-02), so `Document::delete_page`
    /// and `Document::move_page` renumber every later page and a stored
    /// `PageId` goes stale. Every command that renumbers pages calls this
    /// afterwards; the editing commands call it whenever they change the
    /// selection. The anchor's page wins, then the first selected shape's.
    pub fn refresh_page(&mut self, document: &larrygram_model::Document) {
        let derived = self
            .anchor
            .or_else(|| self.shapes.first().copied())
            .and_then(|shape| document.page_of(shape));
        if derived.is_some() {
            self.page = derived;
        }
    }

    /// Clears the selection (shapes, anchor and entered group).
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.anchor = None;
        self.page = None;
        self.entered = None;
    }
}

/// The identity of one [`DocumentSession`] (P4-15).
///
/// The key is assigned once per session and never reused within the
/// process, so an [`Edit`] whose `apply`/`revert` opens or
/// closes documents can still find the session it belongs to: the
/// document index moves, the identity does not. The wrapped number is
/// opaque; compare ids, do not order by them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

/// The next session identity (a process-local counter; wrapping is
/// harmless in practice at 2^64 sessions).
fn next_session_id() -> SessionId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    SessionId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// One open document with its per-document state (ADR 0029).
#[derive(Debug)]
pub struct DocumentSession {
    /// Stable identity of this session (P4-15; see [`SessionId`]).
    pub id: SessionId,
    /// The document (model, L1).
    pub document: Document,
    /// Where it was loaded from or last saved to; `None` when never saved.
    pub path: Option<PathBuf>,
    /// Whether it has unsaved changes.
    pub dirty: bool,
    /// The page shown on the canvas, or `None` before the first page.
    pub active_page: Option<PageId>,
    /// Pan, zoom and the grid and rulers toggles.
    pub view: ViewState,
    /// The per-document undo stack (ADR 0028).
    pub undo: UndoStack,
    /// The document change counter (ADR 0040): every edit bumps it, and
    /// the canvas cache keys on it instead of comparing a document
    /// snapshot every frame (the P4-05 deferred item). It only moves
    /// forward (`wrapping_add`).
    pub changes: u64,
    /// The dependency graph of the document's formulas (ADR 0055): built
    /// on the first recalc and kept beside the undo stack, so a pass
    /// re-links only the nodes an edit touches. `None` until then.
    pub graph: Option<larrygram_shapesheet::DepGraph>,
    /// The package the document was read from, kept for the lossless
    /// patch-based writer (ADR 0018: `write_document` patches the
    /// source parts so unmapped content survives a save). `None` for a
    /// document that was never read from a file (task P4-02 extension
    /// of the ADR 0029 field list).
    pub preserve: Option<larrygram_vsdx::Package>,
}

impl DocumentSession {
    /// Wraps a document. The active page is the document's first page.
    pub fn new(document: Document, path: Option<PathBuf>) -> Self {
        let active_page = document.pages.first().map(|page| page.id);
        Self {
            id: next_session_id(),
            document,
            path,
            dirty: false,
            active_page,
            view: ViewState::new(),
            undo: UndoStack::new(UndoStack::DEFAULT_CAPACITY),
            changes: 0,
            graph: None,
            preserve: None,
        }
    }

    /// Keeps the package the document was read from (ADR 0018).
    pub fn with_preserve(mut self, preserve: larrygram_vsdx::Package) -> Self {
        self.preserve = Some(preserve);
        self
    }

    /// The display name: the file name, or "Untitled" when never saved.
    pub fn display_name(&self) -> String {
        match &self.path {
            Some(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_owned()),
            None => "Untitled".to_owned(),
        }
    }
}

/// The persisted user settings (ADR 0029, ADR 0032).
///
/// A missing field is corrupt, not a partial load (ADR 0032), with
/// two exceptions: `ui_layout` and `default_layout` carry the
/// pre-rename spellings as aliases and the current default, so a
/// settings store saved before the P7-00 rename still loads, and the
/// Phase 7 fields (`welcome_seen`, the per-layout state) have serde
/// defaults, so a Phase 6 store loads unchanged (task P7-09).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Settings {
    /// The Panel layout (ADR 0032).
    #[serde(default = "default_ui_layout")]
    pub ui_layout: LayoutKind,
    /// The layout the welcome wizard chose (ADR 0066). The shell
    /// keeps it in step with `ui_layout`.
    #[serde(default = "default_ui_layout")]
    pub default_layout: LayoutKind,
    /// Whether the grid is drawn by default.
    pub show_grid: bool,
    /// Whether the rulers are drawn by default.
    pub show_rulers: bool,
    /// How the UI formats lengths.
    pub units: DisplayUnits,
    /// Recently opened files, most recent first.
    pub recent_files: Vec<PathBuf>,
    /// Whether the welcome wizard has been shown (ADR 0066). It opens
    /// on launch only while this is `false`, and both Cancel and Start
    /// set it.
    #[serde(default)]
    pub welcome_seen: bool,
    /// Panel Classic toolbar visibility, per window mode:
    /// `"<mode>/<toolbar id>"` to the user's choice (ADR 0061).
    #[serde(default)]
    pub panel_classic_toolbars: BTreeMap<String, bool>,
    /// Whether the Panel Modern command bar is collapsed to the tab
    /// strip.
    #[serde(default)]
    pub command_bar_collapsed: bool,
    /// The last active core tab of the Panel Modern command bar (the
    /// tab id).
    #[serde(default)]
    pub last_core_tab: Option<String>,
}

/// The serde default of [`Settings::ui_layout`] and
/// [`Settings::default_layout`]: Panel Modern, the default layout for
/// new stores (ADR 0061, task P7-09).
fn default_ui_layout() -> LayoutKind {
    LayoutKind::PanelModern
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_layout: LayoutKind::PanelModern,
            default_layout: LayoutKind::PanelModern,
            show_grid: true,
            show_rulers: true,
            units: DisplayUnits::Inches,
            recent_files: Vec::new(),
            welcome_seen: false,
            panel_classic_toolbars: BTreeMap::new(),
            command_bar_collapsed: false,
            last_core_tab: None,
        }
    }
}

/// Loads and saves [`Settings`] (ADR 029 seam, ADR 0032 storage).
///
/// The apps implement this over `eframe::Storage`; the CLI never
/// persists settings. A corrupt store is a [`SettingsError::Format`],
/// never a panic.
pub trait SettingsStore: Send {
    /// Reads the stored settings, or reports why they could not be read.
    fn load(&mut self) -> Result<Settings, SettingsError>;

    /// Writes the settings.
    fn save(&mut self, settings: &Settings) -> Result<(), SettingsError>;
}

/// The active interaction tool (task P5-17, ADR 0042).
///
/// `crates/ui` maps this to its own `Tool`; `commands` cannot depend on
/// the egui crates (ADR 0002). Choosing a tool is a command, not an
/// edit: it adds no undo step and does not bump the change counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActiveTool {
    /// Select, move, resize and rotate (the default).
    #[default]
    Pointer,
    /// Draw a rectangle.
    Rectangle,
    /// Draw an ellipse.
    Ellipse,
    /// Draw a line.
    Line,
    /// Draw a dynamic connector.
    Connector,
    /// Draw a text box or edit text.
    Text,
}

impl ActiveTool {
    /// The tool's name as a user sees it.
    pub fn name(self) -> &'static str {
        match self {
            ActiveTool::Pointer => "Pointer",
            ActiveTool::Rectangle => "Rectangle",
            ActiveTool::Ellipse => "Ellipse",
            ActiveTool::Line => "Line",
            ActiveTool::Connector => "Connector",
            ActiveTool::Text => "Text",
        }
    }
}

/// The application state every command sees (ADR 0029).
#[derive(Debug)]
pub struct AppCtx {
    /// Open documents, in open order.
    pub documents: Vec<DocumentSession>,
    /// Index of the active document in `documents`, if any.
    pub active: Option<usize>,
    /// The selection of the active page (ADR 0041).
    pub selection: Selection,
    /// The user settings.
    pub settings: Settings,
    /// Recent warnings, capped at 200 entries (ADR 0021 style).
    pub warnings: Vec<String>,
    /// The internal clipboard (ADR 0047).
    pub clipboard: crate::clipboard::Clipboard,
    /// The active interaction tool (task P5-17; `ui` mirrors it into
    /// its `ToolMachine` each frame).
    pub active_tool: ActiveTool,
    /// The undo-recording gate (ADR 0040 rule 2): set while an
    /// [`Edit`]'s `apply`/`revert` runs. While it is set,
    /// [`AppCtx::push_edit`] drops the edit and warns instead of
    /// recording it, so a nested edit can never corrupt the undo
    /// cursor or vanish into the runner's placeholder stack.
    undo_suspended: bool,
}

impl AppCtx {
    /// Maximum number of warnings kept.
    pub const WARNING_CAP: usize = 200;

    /// An empty application state with the given settings.
    pub fn new(settings: Settings) -> Self {
        Self {
            documents: Vec::new(),
            active: None,
            selection: Selection::new(),
            settings,
            warnings: Vec::new(),
            clipboard: Clipboard::new(),
            active_tool: ActiveTool::Pointer,
            undo_suspended: false,
        }
    }

    /// The active document.
    pub fn active_document(&self) -> Option<&DocumentSession> {
        self.active.and_then(|index| self.documents.get(index))
    }

    /// The active document, mutably.
    pub fn active_document_mut(&mut self) -> Option<&mut DocumentSession> {
        self.active.and_then(|index| self.documents.get_mut(index))
    }

    /// The active interaction tool (task P5-17).
    pub fn active_tool(&self) -> ActiveTool {
        self.active_tool
    }

    /// Opens a session and makes it active; returns its index.
    pub fn open_document(&mut self, session: DocumentSession) -> usize {
        self.documents.push(session);
        let index = self.documents.len() - 1;
        self.active = Some(index);
        index
    }

    /// Closes the document at `index`, keeping the active index valid.
    ///
    /// The active document is kept when a *different* document closes:
    /// `active` only shifts down when it sat after `index` (P4-15
    /// review minor 5), and only moves when the active document itself
    /// closes (then the next document takes over, or the previous one
    /// when the last one closed).
    ///
    /// Returns the closed session, or `None` when `index` is stale.
    pub fn close_document(&mut self, index: usize) -> Option<DocumentSession> {
        if index >= self.documents.len() {
            return None;
        }
        let closed = self.documents.remove(index);
        self.active = match self.active {
            None => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => {
                if self.documents.is_empty() {
                    None
                } else {
                    Some(index.min(self.documents.len() - 1))
                }
            }
            Some(active) => Some(active),
        };
        Some(closed)
    }

    /// Bumps the active document's change counter (ADR 0040). Every
    /// document mutation path calls this after editing, including undo
    /// and redo.
    ///
    /// Rule (task P5-04): the editing `Edit`s call
    /// [`AppCtx::touch_session`] with their own session, so every
    /// `apply`/`revert` bumps the counter of the document it changed
    /// (which may not be the active one). A command that mutates a
    /// document through no `Edit` calls this directly. A view-only or
    /// selection-only command never does.
    pub fn touch(&mut self) {
        if let Some(id) = self.active_document().map(|session| session.id) {
            self.touch_session(id);
        }
    }

    /// Bumps the change counter of one session (ADR 0040).
    ///
    /// Returns whether the session still exists; a stale [`SessionId`]
    /// is not an error (the session went away with its counter).
    pub fn touch_session(&mut self, session: SessionId) -> bool {
        if let Some(slot) = self.documents.iter_mut().find(|s| s.id == session) {
            slot.changes = slot.changes.wrapping_add(1);
            return true;
        }
        false
    }

    /// Records a completed edit on the undo stack of `session` (ADR 0040
    /// rule 2): the **only** push path. The dispatcher of
    /// [`Outcome::edit`](crate::Outcome::edit) calls it after `execute`
    /// returns `Ok`.
    ///
    /// The Phase 6 extension (ADR 0056): the step is already applied, so
    /// this runs one recalc pass over the step's
    /// [`dirty_cells`](Edit::dirty_cells) and pushes **one** entry holding
    /// both the step and the pass's snapshot
    /// (an internal `RecalcEdit`): undo restores the
    /// recalculated cells by snapshot and the step's own writes by their
    /// inverse.
    ///
    /// - While an [`Edit`]'s `apply`/`revert` runs (the recording gate,
    ///   [`AppCtx::undo_recording_suspended`]) the edit is **dropped**
    ///   and one warning line is recorded ("an edit tried to record
    ///   during undo; step dropped"). The runner's zero-capacity
    ///   placeholder stack is therefore inert: nothing can land on it.
    /// - The session is found by [`SessionId`], never by index, so a
    ///   command that opened or closed documents cannot land its edit on
    ///   the wrong stack.
    /// - A [`SessionId`] that no longer names a session is an error:
    ///   there is no stack to record on. Nothing is recalculated then
    ///   (a failing edit changes nothing, ADR 0039).
    pub fn push_edit(
        &mut self,
        session: SessionId,
        edit: Box<dyn Edit>,
    ) -> Result<(), CommandError> {
        if self.undo_suspended {
            self.record_warning("an edit tried to record during undo; step dropped");
            return Ok(());
        }
        if !self.documents.iter().any(|slot| slot.id == session) {
            return Err(CommandError::failed("no document session for the edit"));
        }
        let entry = crate::recalc::RecalcEdit::wrap(self, session, edit)?;
        let Some(slot) = self.documents.iter_mut().find(|slot| slot.id == session) else {
            return Err(CommandError::failed("no document session for the edit"));
        };
        slot.undo.push(entry);
        Ok(())
    }

    /// Whether undo recording is suspended (the recording gate, ADR 0040).
    pub fn undo_recording_suspended(&self) -> bool {
        self.undo_suspended
    }

    /// Sets the recording gate and returns its previous value.
    ///
    /// The gate is a flag, not a counter: the outermost runner restores
    /// the saved value, so nested `apply`/`revert` calls keep it closed
    /// until the whole step is done (task P5-04).
    pub(crate) fn set_undo_suspended(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.undo_suspended, value)
    }

    /// Records a warning for the status bar (capped ring, ADR 0029).
    pub fn record_warning(&mut self, message: impl Into<String>) {
        if self.warnings.len() >= Self::WARNING_CAP {
            self.warnings.remove(0);
        }
        self.warnings.push(message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::Document;

    #[test]
    fn view_state_defaults() {
        let view = ViewState::new();
        assert_eq!(view.zoom, 1.0);
        assert_eq!(view.pan, larrygram_geom::Point::new(0.0, 0.0));
        assert!(view.show_grid);
        assert!(view.show_rulers);
        assert_eq!(view, ViewState::default());
    }

    /// The ADR 0032 amendment: a new session's toggles come from the
    /// settings defaults (P4-15 review major 1).
    #[test]
    fn view_state_seeds_the_toggles_from_the_settings() {
        let settings = Settings {
            show_grid: false,
            show_rulers: true,
            ..Settings::default()
        };
        let view = ViewState::from_settings(&settings);
        assert!(!view.show_grid);
        assert!(view.show_rulers);
        assert_eq!(view.zoom, 1.0);
        assert_eq!(view.pan, larrygram_geom::Point::new(0.0, 0.0));
    }

    #[test]
    fn display_units_names_round_trip() {
        for name in DisplayUnits::NAMES {
            let units = DisplayUnits::parse(name).unwrap_or_else(|| panic!("{name} parses"));
            assert_eq!(units.name(), name);
            let lower = name.to_ascii_lowercase();
            assert_eq!(DisplayUnits::parse(&lower), Some(units), "{lower}");
        }
        assert_eq!(DisplayUnits::parse("furlongs"), None);
        assert_eq!(DisplayUnits::parse(""), None);
    }

    #[test]
    fn selection_is_empty_in_phase_four() {
        let selection = Selection::new();
        assert!(selection.is_empty());
        assert_eq!(selection.len(), 0);
    }

    #[test]
    fn settings_default_to_panel_modern_and_first_run_state() {
        // New stores default to Panel Modern (ADR 0061, task P7-09),
        // and a fresh install has not seen the welcome wizard (ADR
        // 0066).
        let settings = Settings::default();
        assert_eq!(settings.ui_layout, LayoutKind::PanelModern);
        assert_eq!(settings.default_layout, LayoutKind::PanelModern);
        assert!(!settings.welcome_seen);
        assert!(settings.panel_classic_toolbars.is_empty());
        assert!(!settings.command_bar_collapsed);
        assert_eq!(settings.last_core_tab, None);
        assert_eq!(settings.units, DisplayUnits::Inches);
        assert!(settings.recent_files.is_empty());
    }

    #[test]
    fn a_phase_6_settings_store_loads_unchanged() {
        // The exact JSON a Phase 6 build wrote: no Phase 7 fields.
        let old = concat!(
            r#"{"ui_layout":"Panel2016","show_grid":false,"show_rulers":true,"#,
            r#""units":"Inches","recent_files":["/tmp/a.vsdx"]}"#
        );
        let settings: Settings = serde_json::from_str(old).expect("the Phase 6 store loads");
        assert_eq!(
            settings.ui_layout,
            LayoutKind::Panel2016,
            "stored value kept"
        );
        assert_eq!(
            settings.default_layout,
            LayoutKind::PanelModern,
            "the Phase 7 default fills in"
        );
        assert!(!settings.show_grid);
        assert_eq!(
            settings.recent_files,
            vec![PathBuf::from("/tmp/a.vsdx")],
            "stored value kept"
        );
        assert!(!settings.welcome_seen);
        assert!(settings.panel_classic_toolbars.is_empty());
        assert!(!settings.command_bar_collapsed);
        assert_eq!(settings.last_core_tab, None);
    }

    #[test]
    fn a_settings_store_written_before_the_rename_still_loads() {
        let old = concat!(
            r#"{"ui_layout":"Visio2016","show_grid":false,"show_rulers":true,"#,
            r#""units":"Inches","recent_files":[]}"#
        );
        let settings: Settings = serde_json::from_str(old).expect("the old store loads");
        assert_eq!(settings.ui_layout, LayoutKind::Panel2016);
        assert_eq!(settings.default_layout, LayoutKind::PanelModern);
    }

    #[test]
    fn the_per_layout_state_round_trips() {
        let mut settings = Settings::default();
        settings
            .panel_classic_toolbars
            .insert("drawing/standard".to_owned(), false);
        settings.command_bar_collapsed = true;
        settings.last_core_tab = Some("tab.insert".to_owned());
        let text = serde_json::to_string(&settings).expect("serialize");
        let back: Settings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, settings);
    }

    #[test]
    fn document_session_takes_the_first_page_and_names_itself() {
        let mut document = Document::new();
        document
            .pages
            .push(larrygram_model::Page::new(PageId { index: 0 }, 0));
        let session = DocumentSession::new(document, None);
        assert_eq!(session.active_page, Some(PageId { index: 0 }));
        assert_eq!(session.display_name(), "Untitled");
        assert!(!session.dirty);
        assert!(!session.undo.can_undo());

        let mut document = Document::new();
        document
            .pages
            .push(larrygram_model::Page::new(PageId { index: 0 }, 0));
        let session = DocumentSession::new(document, Some(PathBuf::from("/tmp/a.vsdx")));
        assert_eq!(session.display_name(), "a.vsdx");
    }

    #[test]
    fn document_session_keeps_the_source_package() {
        let mut document = Document::new();
        document
            .pages
            .push(larrygram_model::Page::new(PageId { index: 0 }, 0));
        let session = DocumentSession::new(document, None);
        assert!(session.preserve.is_none());
        let session = session.with_preserve(larrygram_vsdx::Package::new());
        assert!(session.preserve.is_some());
    }

    #[test]
    fn app_ctx_tracks_the_active_document() {
        let mut ctx = AppCtx::new(Settings::default());
        assert!(ctx.active_document().is_none());
        assert!(ctx.active_document_mut().is_none());

        let first = ctx.open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(first, 0);
        assert!(ctx.active_document().is_some());

        let second = ctx.open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(second, 1);

        assert!(ctx.close_document(9).is_none());
        assert!(ctx.close_document(0).is_some());
        assert_eq!(ctx.active, Some(0));
        assert!(ctx.close_document(0).is_some());
        assert_eq!(ctx.active, None);
    }

    /// P4-15 review minor 5: closing a *different* document keeps the
    /// user where they are; `active` only shifts when it sat after the
    /// closed index and only moves when the active document closes.
    #[test]
    fn closing_another_document_keeps_the_active_one() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.open_document(DocumentSession::new(Document::new(), None));
        let third = ctx.active_document().expect("active").id;

        // Close the first while the third is active: the third stays
        // active and shifts down to index 1.
        assert!(ctx.close_document(0).is_some());
        assert_eq!(ctx.active, Some(1));
        assert_eq!(ctx.active_document().map(|s| s.id), Some(third));

        // Close the active one: the next document takes over.
        assert!(ctx.close_document(1).is_some());
        assert_eq!(ctx.active, Some(0));

        // A close before the active index shifts it down.
        ctx.open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(ctx.active, Some(1));
        assert!(ctx.close_document(0).is_some());
        assert_eq!(ctx.active, Some(0));
    }

    /// P4-15 review minor 3: sessions carry their own identity.
    #[test]
    fn session_ids_identify_sessions_not_indices() {
        let one = DocumentSession::new(Document::new(), None);
        let two = DocumentSession::new(Document::new(), None);
        assert_ne!(one.id, two.id, "ids are unique per session");
        assert_eq!(one.id, one.id, "ids are stable");
    }

    #[test]
    fn warnings_are_capped() {
        let mut ctx = AppCtx::new(Settings::default());
        for i in 0..(AppCtx::WARNING_CAP + 5) {
            ctx.record_warning(format!("w{i}"));
        }
        assert_eq!(ctx.warnings.len(), AppCtx::WARNING_CAP);
        assert_eq!(ctx.warnings.first().map(String::as_str), Some("w5"));
        assert_eq!(ctx.warnings.last().map(String::as_str), Some("w204"));
    }

    #[test]
    fn selections_answer_membership_and_the_primary_shape() {
        let a = larrygram_model::ShapeId { index: 1 };
        let b = larrygram_model::ShapeId { index: 2 };
        let mut selection = Selection::new();
        assert!(selection.is_empty());
        assert_eq!(selection.primary(), None);
        selection.shapes = vec![a, b];
        selection.anchor = Some(a);
        assert_eq!(selection.len(), 2);
        assert!(!selection.is_empty());
        assert!(selection.contains(b));
        assert!(!selection.contains(larrygram_model::ShapeId { index: 3 }));
        assert_eq!(selection.primary(), Some(a));
    }

    #[test]
    fn touch_bumps_the_active_document_change_counter() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.touch();
        assert!(ctx.clipboard.is_empty());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(ctx.active_document().map(|s| s.changes), Some(0));
        ctx.touch();
        ctx.touch();
        assert_eq!(ctx.active_document().map(|s| s.changes), Some(2));
    }

    /// An edit that records its label in the session's warnings.
    struct LabelEdit(&'static str);

    impl crate::undo::Edit for LabelEdit {
        fn label(&self) -> &str {
            self.0
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
    }

    #[test]
    fn touch_session_names_the_session() {
        let mut ctx = AppCtx::new(Settings::default());
        assert!(!ctx.touch_session(SessionId(0)), "a stale session id");
        let index = ctx.open_document(DocumentSession::new(Document::new(), None));
        let id = ctx.documents.get(index).expect("session").id;
        assert!(ctx.touch_session(id));
        assert_eq!(ctx.active_document().map(|s| s.changes), Some(1));
    }

    /// ADR 0040 rule 2: `push_edit` is the only push path and finds the
    /// session by identity, never by index.
    #[test]
    fn push_edit_reports_a_stale_session() {
        let mut ctx = AppCtx::new(Settings::default());
        let stale = SessionId(u64::MAX);
        let error = ctx
            .push_edit(stale, Box::new(LabelEdit("lost")))
            .expect_err("no session");
        assert!(error.to_string().contains("no document session"), "{error}");
    }

    #[test]
    fn push_edit_lands_on_the_named_session() {
        let mut ctx = AppCtx::new(Settings::default());
        let first = ctx.open_document(DocumentSession::new(Document::new(), None));
        let first_id = ctx.documents.get(first).expect("session").id;
        ctx.open_document(DocumentSession::new(Document::new(), None));
        let second_id = ctx.active_document().expect("session").id;

        ctx.push_edit(first_id, Box::new(LabelEdit("one")))
            .expect("push");
        assert!(!ctx.undo_recording_suspended());
        let first = ctx
            .documents
            .iter()
            .find(|s| s.id == first_id)
            .expect("first session");
        assert!(first.undo.can_undo());
        assert_eq!(first.undo.undo_labels(), vec!["one"]);
        let second = ctx
            .documents
            .iter()
            .find(|s| s.id == second_id)
            .expect("second session");
        assert!(second.undo.is_empty(), "the other session is untouched");

        // A stale session has no stack to record on.
        let error = ctx
            .push_edit(SessionId(0), Box::new(LabelEdit("lost")))
            .expect_err("no such session");
        assert!(error.to_string().contains("no document session"), "{error}");
        assert!(
            !ctx.warnings.iter().any(|w| w.contains("lost")),
            "an error is not a silent drop"
        );
    }

    /// ADR 0040: an edit that records while undo runs is dropped with
    /// exactly one warning.
    #[test]
    fn push_edit_drops_the_edit_while_recording_is_suspended() {
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(Document::new(), None));
        let id = ctx.documents.get(index).expect("session").id;
        assert!(!ctx.set_undo_suspended(true), "the gate was open before");
        let result = ctx.push_edit(id, Box::new(LabelEdit("nested")));
        assert!(result.is_ok(), "a dropped edit is not an error");
        assert_eq!(ctx.warnings.len(), 1);
        assert_eq!(
            ctx.warnings.first().map(String::as_str),
            Some("an edit tried to record during undo; step dropped")
        );
        assert!(
            ctx.documents
                .get(index)
                .map(|s| s.undo.is_empty())
                .unwrap_or(false),
            "nothing landed on the stack"
        );
        assert!(ctx.undo_recording_suspended());
        assert!(ctx.set_undo_suspended(false));
        assert!(!ctx.undo_recording_suspended());
    }

    #[test]
    fn selection_refresh_page_follows_the_shapes() {
        let mut document = Document::new();
        document
            .pages
            .push(larrygram_model::Page::new(PageId { index: 0 }, 1));
        document
            .pages
            .push(larrygram_model::Page::new(PageId { index: 1 }, 2));
        let shape = document.add_shape(larrygram_model::Shape::new(
            larrygram_model::ShapeId { index: 0 },
            1,
        ));
        document.pages[1].shapes.push(shape);
        let mut selection = Selection::new();
        selection.shapes = vec![shape];
        selection.anchor = Some(shape);
        assert_eq!(selection.page, None);
        selection.refresh_page(&document);
        assert_eq!(selection.page, Some(PageId { index: 1 }));

        // A page delete renumbers the pages: the refresh follows the
        // shape, not the recorded index (task P5-02 note).
        document.delete_page(PageId { index: 0 }).expect("delete");
        selection.refresh_page(&document);
        assert_eq!(selection.page, Some(PageId { index: 0 }));

        // An empty selection keeps its page (nothing derives a new one).
        selection.shapes.clear();
        selection.anchor = None;
        selection.refresh_page(&document);
        assert_eq!(selection.page, Some(PageId { index: 0 }));

        selection.clear();
        assert_eq!(selection, Selection::new());
    }

    /// The label edits the push tests hand out run like any other edit.
    #[test]
    fn label_edits_apply_and_revert() {
        let mut ctx = AppCtx::new(Settings::default());
        let mut edit = LabelEdit("x");
        assert_eq!(crate::undo::Edit::label(&edit), "x");
        assert!(edit.apply(&mut ctx).is_ok());
        assert!(edit.revert(&mut ctx).is_ok());
    }

    /// The two `close_document` arms the earlier tests never took: no
    /// active document at all, and a close *after* the active one.
    #[test]
    fn closing_with_no_active_document_or_after_it() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.active = None;
        assert!(ctx.close_document(0).is_some());
        assert_eq!(ctx.active, None);

        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.open_document(DocumentSession::new(Document::new(), None));
        ctx.active = Some(0);
        assert!(ctx.close_document(2).is_some());
        assert_eq!(ctx.active, Some(0), "the active document keeps its index");
    }
}
