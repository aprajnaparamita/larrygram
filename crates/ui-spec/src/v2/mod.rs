//! Layout format v2 (ADR 0061): the type surface the Panel Modern and
//! Panel Classic widgets read.
//!
//! The machine-checked shape of the data is
//! `ui-spec/layout-v2.schema.json`, and the guide is
//! `docs/ui/layout-format.md`. [`load_layout`] loads one committed
//! layout from the embedded copies; [`crate::Loader`] reads the same
//! documents from disk, with the cross-checks of task P7-03
//! ([`crate::LoadError`]). [`from_v1`] converts the v1 Fluent-data
//! layouts into this model.
//!
//! The four layouts are Panel Modern (`panel-modern`, the 2013 data),
//! Panel Classic (`panel-classic`, the 2007 data), Panel (2016 data)
//! and Panel (365 data). The two Fluent-data layouts are converted
//! into this model by [`from_v1`] (ADR 0061 "Adapter for v1
//! layouts").

mod adapter;
pub(crate) mod load;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CommandId, FluentId};

pub use adapter::from_v1;
pub use load::load_layout;

/// Which of the four layouts a file or setting names.
///
/// The variants map 1:1 to the layout ids, the file names under
/// `ui-spec/layouts/` and the `--ui-layout` values. The first two read
/// hand-curated v2 data; the last two read the v1 Fluent-data files and
/// are converted by [`from_v1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayoutKind {
    /// The 2013 data: a tabbed command bar with a File view.
    PanelModern,
    /// The 2007 data: a menu bar, docked toolbars and context menus.
    PanelClassic,
    /// Panel (2016 data): the Office 2016 Fluent control data.
    ///
    /// The serde alias is the pre-P7-00 spelling, so a settings store
    /// saved before the rename still loads.
    #[serde(alias = "Visio2016")]
    Panel2016,
    /// Panel (365 data): the Microsoft 365 Current Channel data.
    ///
    /// The serde alias is the pre-P7-00 spelling, so a settings store
    /// saved before the rename still loads.
    #[serde(alias = "Visio365")]
    Panel365,
}

impl LayoutKind {
    /// The layout id, as used in settings and `--ui-layout`.
    pub fn id(self) -> &'static str {
        match self {
            Self::PanelModern => "panel-modern",
            Self::PanelClassic => "panel-classic",
            Self::Panel2016 => "panel-2016",
            Self::Panel365 => "panel-365",
        }
    }

    /// The layout name as the user sees it.
    pub fn label(self) -> &'static str {
        match self {
            Self::PanelModern => "Panel Modern",
            Self::PanelClassic => "Panel Classic",
            Self::Panel2016 => "Panel (2016 data)",
            Self::Panel365 => "Panel (365 data)",
        }
    }

    /// The file name of the layout's data file: `panel-*.yaml` under
    /// `ui-spec/layouts/` for the two curated layouts, under
    /// `ui-spec/` for the two Fluent-data ones.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::PanelModern => "panel-modern.yaml",
            Self::PanelClassic => "panel-classic.yaml",
            Self::Panel2016 => "panel-2016.yaml",
            Self::Panel365 => "panel-365.yaml",
        }
    }
}

/// The container style a layout uses, as the `kind` field of a layout
/// file: `panel-classic` or `panel-modern`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelStyle {
    /// Menu bars, toolbars and context menus.
    PanelClassic,
    /// Tabs with placed controls, a File view and context menus.
    #[default]
    PanelModern,
}

/// The surface an item is drawn on. Mirrors the `kind` field of an
/// item in `ui-spec/layouts/*.yaml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Runs its `command` when activated.
    Button,
    /// A latched button.
    Toggle,
    /// A check box, or a menu item with a check mark.
    Check,
    /// The main part runs `command`; the arrow part opens `items`.
    Split,
    /// A split whose main part shows the last item chosen.
    SplitRecent,
    /// A button whose arrow opens `items`.
    Dropdown,
    /// A menu entry with a submenu arrow.
    Submenu,
    /// A dropdown whose content is a grid of `list` entries.
    Gallery,
    /// An editable combo box.
    Combo,
    /// A toolbar dropdown showing graphic choices.
    Palette,
    /// The small corner button of a group caption.
    Launcher,
    /// A non-clickable section heading.
    Heading,
    /// Run-time entries inserted at this point.
    List,
    /// Fixed text choices inserted at this point.
    Options,
}

/// How a toolbar item shows its label and icon (`display`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// The icon only (the default).
    Icon,
    /// The label only.
    Text,
    /// Icon and label.
    IconText,
}

/// The size of a command-bar control (`size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSize {
    /// 32 px icon over a two-line label.
    Large,
    /// 16 px icon beside the label.
    Medium,
    /// 16 px icon only.
    Small,
}

/// How a toolbar docks (`dock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dock {
    /// Docked along the top edge.
    #[default]
    Top,
    /// Docked along the bottom edge.
    Bottom,
    /// Docked along the left edge.
    Left,
    /// Docked along the right edge.
    Right,
    /// A small tool window.
    Floating,
}

/// Where a window region sits (`edge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    /// Along the top edge.
    Top,
    /// Along the bottom edge.
    Bottom,
    /// Along the left edge.
    Left,
    /// Along the right edge.
    Right,
    /// The drawing area.
    Fill,
    /// The rulers, at the top left of the drawing.
    TopLeftOfDrawing,
    /// The page-tab strip, below the drawing.
    BottomOfDrawing,
}

/// How a group lays out its controls (`layout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupLayout {
    /// Columns; a large control fills its column.
    #[default]
    Stack,
    /// Two rows that flow left to right.
    Rows,
}

/// A context name from `ui-spec/contexts.yaml` (ADR 0064).
///
/// The first ten variants are the selection contexts; `StencilEdit` is
/// the window mode that `shown_in` also accepts. The UI-side enum with
/// `PageSelected` lives in `crates/ui/src/contexts.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextId {
    /// A page with no shapes, nothing selected.
    EmptyPage,
    /// One ordinary 2-D shape selected.
    ShapeSelected,
    /// Two or more shapes selected.
    ShapesSelected,
    /// A shape's text is open for editing.
    TextEditing,
    /// One connector selected.
    ConnectorSelected,
    /// One group selected.
    GroupSelected,
    /// Another, empty page is active.
    SecondPage,
    /// One picture selected.
    PictureSelected,
    /// One container selected.
    ContainerSelected,
    /// A ShapeSheet window is the active window.
    ShapesheetWindow,
    /// A stencil is open for editing (a window mode).
    StencilEdit,
}

impl ContextId {
    /// The id string used in `ui-spec/contexts.yaml` and `shown_in`.
    pub fn as_str(self) -> &'static str {
        use ContextId as C;
        match self {
            C::EmptyPage => "empty_page",
            C::ShapeSelected => "shape_selected",
            C::ShapesSelected => "shapes_selected",
            C::TextEditing => "text_editing",
            C::ConnectorSelected => "connector_selected",
            C::GroupSelected => "group_selected",
            C::SecondPage => "second_page",
            C::PictureSelected => "picture_selected",
            C::ContainerSelected => "container_selected",
            C::ShapesheetWindow => "shapesheet_window",
            C::StencilEdit => "stencil_edit",
        }
    }
}

/// The kind of a File view navigation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileViewEntryKind {
    /// Runs `command` and closes the File view.
    Command,
    /// Shows a page of settings inside the File view.
    Page,
}

/// One control. Items nest through `items` (ADRs 0061, 0031).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Unique dotted placement id within the layout.
    pub id: String,
    /// How the item is drawn.
    pub kind: ItemKind,
    /// Short UI label.
    #[serde(default)]
    pub label: Option<String>,
    /// Access-key letter.
    #[serde(default)]
    pub access: Option<char>,
    /// Shortcut text shown right-aligned in menus.
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Registry id dispatched on activation.
    #[serde(default)]
    pub command: Option<CommandId>,
    /// The `command` id is a Phase 7 proposal.
    #[serde(default)]
    pub new_command: bool,
    /// Fluent control id, cross-reference only.
    #[serde(default)]
    pub fluent: Option<FluentId>,
    /// The documented UI command constant, cross-reference only.
    #[serde(default)]
    pub visuicmd: Option<String>,
    /// Key into `ui-spec/icons.yaml`.
    #[serde(default)]
    pub icon: Option<String>,
    /// Toolbar display mode.
    #[serde(default)]
    pub display: Option<DisplayMode>,
    /// Command-bar size.
    #[serde(default)]
    pub size: Option<ControlSize>,
    /// Slot column in a group.
    #[serde(default)]
    pub column: Option<u32>,
    /// Slot row in a group.
    #[serde(default)]
    pub row: Option<u32>,
    /// Width of a combo, in logical pixels.
    #[serde(default)]
    pub width: Option<f64>,
    /// Draw a separator before this item.
    #[serde(default)]
    pub begin_group: bool,
    /// Part of the layout, but not drawn by default.
    #[serde(default)]
    pub hidden: bool,
    /// The app replaces the label at run time.
    #[serde(default)]
    pub dynamic_label: bool,
    /// The app fills this point from live data (a run-time list name
    /// from `docs/ui/layout-format.md`, such as `recent_files`).
    #[serde(default)]
    pub list: Option<String>,
    /// A fixed list of text choices.
    #[serde(default)]
    pub options: Vec<String>,
    /// Children: submenu entries, dropdown contents, palette entries.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// A group caption launcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Launcher {
    /// The dialog the launcher opens, named in its tooltip.
    #[serde(default)]
    pub label: Option<String>,
    /// The command dispatched on activation.
    pub command: CommandId,
    /// Fluent control id, cross-reference only.
    #[serde(default)]
    pub fluent: Option<FluentId>,
    /// The `command` id is a Phase 7 proposal (task P7-03).
    #[serde(default)]
    pub new_command: bool,
}

/// One group of a tab (`groups`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Caption drawn along the bottom edge.
    #[serde(default)]
    pub label: Option<String>,
    /// Fluent control id, cross-reference only.
    #[serde(default)]
    pub fluent: Option<FluentId>,
    /// Stack or rows.
    #[serde(default)]
    pub layout: GroupLayout,
    /// Stack only: rows per column (default 3).
    #[serde(default)]
    pub rows: Option<u32>,
    /// The controls in their slots.
    #[serde(default)]
    pub controls: Vec<Item>,
    /// The corner launcher, when the group has one.
    #[serde(default)]
    pub launcher: Option<Launcher>,
}

/// One tab, core or contextual (`tabs`, `contextual_tabs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Caption drawn in upper case.
    #[serde(default)]
    pub label: Option<String>,
    /// Fluent control id, cross-reference only.
    #[serde(default)]
    pub fluent: Option<FluentId>,
    /// Tab-set name of a contextual tab.
    #[serde(default)]
    pub tab_set: Option<String>,
    /// The context that shows a contextual tab.
    #[serde(default)]
    pub shown_in: Option<ContextId>,
    /// The groups left to right.
    #[serde(default)]
    pub groups: Vec<Group>,
}

/// One top-level menu of a menu bar (`menus`).
///
/// A menu has no `kind` of its own in the data: it is the title of a
/// dropdown of [`Item`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Menu {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Menu title ("File").
    pub label: String,
    /// Access-key letter; Alt plus it opens the menu.
    #[serde(default)]
    pub access: Option<char>,
    /// The entries top to bottom.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// One menu bar of the Panel Classic layout (`menu_bars`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuBar {
    /// Window mode: `drawing`, `shapesheet` or `stencil`.
    pub id: String,
    /// Description for the layout picker.
    #[serde(default)]
    pub label: Option<String>,
    /// The window mode this bar replaces the default bar in.
    #[serde(default)]
    pub shown_in: Option<ContextId>,
    /// The top-level menus in order.
    #[serde(default)]
    pub menus: Vec<Menu>,
}

/// One toolbar of the Panel Classic layout (`toolbars`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toolbar {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Toolbar name, as in View > Toolbars.
    #[serde(default)]
    pub label: Option<String>,
    /// Shown by default.
    #[serde(default)]
    pub visible: bool,
    /// Where the toolbar docks.
    #[serde(default)]
    pub dock: Dock,
    /// Docked row, top to bottom.
    #[serde(default)]
    pub row: Option<u32>,
    /// The window mode this toolbar belongs to.
    #[serde(default)]
    pub shown_in: Option<ContextId>,
    /// The controls left to right.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// One context menu (`context_menus`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextMenuV2 {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Short name of the menu.
    #[serde(default)]
    pub label: Option<String>,
    /// What a right-click must hit for this menu to open (our own
    /// sentence, as in `docs/ui/panel-classic.md`).
    #[serde(default)]
    pub trigger: Option<String>,
    /// The items top to bottom.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// The File button that opens the File view (`file_button`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileButton {
    /// The button label ("File").
    pub label: String,
    /// What the button opens (`file_view`).
    pub opens: String,
}

/// One File view navigation entry (`file_view.nav`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileViewEntry {
    /// Unique dotted id within the layout.
    pub id: String,
    /// Page or command.
    pub kind: FileViewEntryKind,
    /// Navigation label.
    pub label: String,
    /// The command dispatched on activation (absent for a page that
    /// shows settings only).
    #[serde(default)]
    pub command: Option<CommandId>,
}

/// The File view (`file_view`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileView {
    /// Show the ← back button.
    #[serde(default)]
    pub back_button: bool,
    /// The nav entry shown first.
    pub default_page: String,
    /// The nav entries top to bottom.
    #[serde(default)]
    pub nav: Vec<FileViewEntry>,
}

/// One frame region (`window.regions`), outermost first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowRegion {
    /// Region id (`title_bar`, `command_bar`, `drawing`, ...).
    pub id: String,
    /// Where the region sits.
    pub edge: Edge,
    /// Nominal size in logical pixels: a height for top and bottom, a
    /// width for left and right.
    #[serde(default)]
    pub size: Option<f64>,
    /// Wraps to a new row when full (toolbars).
    #[serde(default)]
    pub per_row: bool,
    /// Can collapse (the Panel Modern command bar).
    #[serde(default)]
    pub collapsible: bool,
    /// What the region holds.
    #[serde(default)]
    pub notes: Option<String>,
}

/// The frame layout (`window`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// Regions in order, outermost first.
    pub regions: Vec<WindowRegion>,
}

/// A layout file (`ui-spec/layouts/*.yaml`, ADR 0061).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutV2 {
    /// Always `2`.
    pub schema_version: u32,
    /// The layout id (`panel-modern`, `panel-classic`).
    pub id: String,
    /// The layout name as the user sees it.
    pub name: String,
    /// The container style.
    pub kind: PanelStyle,
    /// One line of our own description.
    #[serde(default)]
    pub description: Option<String>,
    /// The theme id (`ui-spec/themes/<theme>.yaml`).
    pub theme: String,
    /// Frame regions.
    pub window: Window,
    /// Panel Classic: menu bars, one per window mode.
    #[serde(default)]
    pub menu_bars: Vec<MenuBar>,
    /// Panel Classic: toolbars.
    #[serde(default)]
    pub toolbars: Vec<Toolbar>,
    /// Panel Modern: quick access toolbar items.
    #[serde(default)]
    pub quick_access_toolbar: Vec<Item>,
    /// Panel Modern: the File button.
    #[serde(default)]
    pub file_button: Option<FileButton>,
    /// Panel Modern: core tabs.
    #[serde(default)]
    pub tabs: Vec<Tab>,
    /// Panel Modern: contextual tabs.
    #[serde(default)]
    pub contextual_tabs: Vec<Tab>,
    /// Panel Modern: the File view.
    #[serde(default)]
    pub file_view: Option<FileView>,
    /// Context menus, both styles.
    #[serde(default)]
    pub context_menus: Vec<ContextMenuV2>,
}

/// A font role in a theme (`fonts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontSpec {
    /// Font family, bundled in `crates/text/fonts`.
    pub family: String,
    /// Size in logical pixels.
    pub size_px: f64,
    /// `uppercase` when the role is drawn transformed.
    #[serde(default)]
    pub transform: Option<String>,
}

/// A metric token in a theme (`metrics`): a number of logical pixels,
/// or a small named set of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Metric {
    /// One number of logical pixels (`menu.gutter.width: 24`).
    Px(f64),
    /// Named logical-pixel fields (`button.large: {min_width, height, icon}`).
    Fields(BTreeMap<String, f64>),
}

/// A theme file (`ui-spec/themes/*.yaml`, ADR 0063).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Always `1`.
    pub schema_version: u32,
    /// The theme id.
    pub id: String,
    /// The theme name as the user sees it.
    pub name: String,
    /// Font roles.
    #[serde(default)]
    pub fonts: BTreeMap<String, FontSpec>,
    /// Colour tokens to sRGB hex.
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    /// Metric tokens.
    #[serde(default)]
    pub metrics: BTreeMap<String, Metric>,
    /// Tab-set to colour family (Panel Modern only).
    #[serde(default)]
    pub contextual_tab_sets: BTreeMap<String, String>,
}

/// The icon set an icon map draws from (`set`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconSet {
    /// Set name.
    pub name: String,
    /// Pinned version.
    pub version: String,
    /// Licence short name.
    pub license: String,
    /// Icon style (`outline`).
    pub style: String,
    /// Where the set was fetched from.
    pub source: String,
    /// Path of the committed SVGs.
    pub path: String,
}

/// One icon map row (`icons`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconEntry {
    /// The upstream icon name (`scissors`, `stack-front`).
    pub icon: String,
    /// Theme token to tint the icon with.
    #[serde(default)]
    pub tint: Option<String>,
}

/// The icon map (`ui-spec/icons.yaml`, ADR 0062).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconMap {
    /// Always `1`.
    pub schema_version: u32,
    /// The icon set the names come from.
    pub set: IconSet,
    /// What a key without a row draws (`placeholder`).
    pub fallback: String,
    /// Tint tokens to their descriptions.
    #[serde(default)]
    pub tints: BTreeMap<String, String>,
    /// Icon key to row.
    #[serde(default)]
    pub icons: BTreeMap<String, IconEntry>,
}

/// One context definition (`contexts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextDef {
    /// The context id.
    pub id: ContextId,
    /// What the context means (our own sentence).
    pub description: String,
    /// `pending` while the table has no data for it.
    #[serde(default)]
    pub status: Option<String>,
}

/// Where one command is available (`commands`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandAvailability {
    /// Contexts the command is available in.
    #[serde(default)]
    pub enabled_in: Vec<ContextId>,
    /// For a toggle: starts switched on.
    #[serde(default)]
    pub on_by_default: Option<bool>,
}

/// The selection-context table (`ui-spec/contexts.yaml`, ADR 0064).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contexts {
    /// Always `1`.
    pub schema_version: u32,
    /// The context definitions in file order.
    pub contexts: Vec<ContextDef>,
    /// Our own reading notes.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Command id to availability.
    #[serde(default)]
    pub commands: BTreeMap<String, CommandAvailability>,
}

impl Default for LayoutV2 {
    /// An empty panel-modern layout: no surfaces and no items. The
    /// loader fills every field from the data files.
    fn default() -> Self {
        Self {
            schema_version: 2,
            id: String::new(),
            name: String::new(),
            kind: PanelStyle::PanelModern,
            description: None,
            theme: String::new(),
            window: Window {
                regions: Vec::new(),
            },
            menu_bars: Vec::new(),
            toolbars: Vec::new(),
            quick_access_toolbar: Vec::new(),
            file_button: None,
            tabs: Vec::new(),
            contextual_tabs: Vec::new(),
            file_view: None,
            context_menus: Vec::new(),
        }
    }
}

impl Default for Theme {
    /// An empty theme: every token falls back to an egui default
    /// (ADR 0063).
    fn default() -> Self {
        Self {
            schema_version: 1,
            id: String::new(),
            name: String::new(),
            fonts: BTreeMap::new(),
            colors: BTreeMap::new(),
            metrics: BTreeMap::new(),
            contextual_tab_sets: BTreeMap::new(),
        }
    }
}

impl Default for IconSet {
    /// The set an empty icon map draws from: nothing.
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            license: String::new(),
            style: String::new(),
            source: String::new(),
            path: String::new(),
        }
    }
}

impl Default for IconMap {
    /// An empty icon map: every key draws the placeholder mark (ADR
    /// 0062).
    fn default() -> Self {
        Self {
            schema_version: 1,
            set: IconSet::default(),
            fallback: String::new(),
            tints: BTreeMap::new(),
            icons: BTreeMap::new(),
        }
    }
}

impl Default for Contexts {
    /// An empty contexts table: no context definitions, no rows.
    fn default() -> Self {
        Self {
            schema_version: 1,
            contexts: Vec::new(),
            notes: Vec::new(),
            commands: BTreeMap::new(),
        }
    }
}

/// Why a v2 operation failed (task P7-03).
///
/// The v2 error type folds the loader errors in: loading and
/// cross-checking report [`LoadError`](crate::LoadError), and the
/// adapter reports a layout it cannot convert.
#[derive(Debug, thiserror::Error)]
pub enum V2Error {
    /// A data file failed to load, validate or cross-check.
    #[error(transparent)]
    Load(#[from] crate::LoadError),
    /// The v1 layout cannot be converted (wrong `schema_version`).
    #[error("cannot convert the v1 layout: {message}")]
    Convert {
        /// What is wrong with the layout.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_kind_names() {
        let expected = [
            (
                LayoutKind::PanelModern,
                "panel-modern",
                "Panel Modern",
                "panel-modern.yaml",
            ),
            (
                LayoutKind::PanelClassic,
                "panel-classic",
                "Panel Classic",
                "panel-classic.yaml",
            ),
            (
                LayoutKind::Panel2016,
                "panel-2016",
                "Panel (2016 data)",
                "panel-2016.yaml",
            ),
            (
                LayoutKind::Panel365,
                "panel-365",
                "Panel (365 data)",
                "panel-365.yaml",
            ),
        ];
        for (kind, id, label, file) in expected {
            assert_eq!(kind.id(), id);
            assert_eq!(kind.label(), label);
            assert_eq!(kind.file_name(), file);
        }
    }

    #[test]
    fn v2_error_folds_the_loader_errors_in() {
        let load: V2Error = crate::LoadError::Unknown {
            path: std::path::PathBuf::from("ui-spec/layouts/panel-modern.yaml"),
            message: "broken".to_owned(),
        }
        .into();
        assert_eq!(
            load.to_string(),
            "ui-spec/layouts/panel-modern.yaml: broken"
        );
        let convert = V2Error::Convert {
            message: "nope".to_owned(),
        };
        assert_eq!(convert.to_string(), "cannot convert the v1 layout: nope");
    }

    #[test]
    fn context_ids_are_the_data_names() {
        let names: Vec<&str> = [
            ContextId::EmptyPage,
            ContextId::ShapeSelected,
            ContextId::ShapesSelected,
            ContextId::TextEditing,
            ContextId::ConnectorSelected,
            ContextId::GroupSelected,
            ContextId::SecondPage,
            ContextId::PictureSelected,
            ContextId::ContainerSelected,
            ContextId::ShapesheetWindow,
            ContextId::StencilEdit,
        ]
        .iter()
        .map(|id| id.as_str())
        .collect();
        assert_eq!(
            names,
            vec![
                "empty_page",
                "shape_selected",
                "shapes_selected",
                "text_editing",
                "connector_selected",
                "group_selected",
                "second_page",
                "picture_selected",
                "container_selected",
                "shapesheet_window",
                "stencil_edit",
            ]
        );
    }

    #[test]
    fn item_decodes_with_the_data_field_names() {
        let json = serde_json::json!({
            "id": "tab.home.clipboard.paste",
            "kind": "split_recent",
            "label": "Paste",
            "access": "v",
            "shortcut": "Ctrl+V",
            "command": "edit.paste",
            "new_command": true,
            "fluent": "Paste",
            "visuicmd": "visCmdPaste",
            "icon": "edit.paste",
            "display": "icon_text",
            "size": "large",
            "column": 1,
            "row": 2,
            "width": 107.5,
            "begin_group": true,
            "hidden": false,
            "dynamic_label": true,
            "list": "recent_files",
            "options": ["A4", "Letter"],
            "items": [{"id": "tab.home.clipboard.paste.paste_text", "kind": "button"}]
        });
        let item: Item = serde_json::from_value(json).expect("decodes");
        assert_eq!(item.kind, ItemKind::SplitRecent);
        assert_eq!(item.access, Some('v'));
        assert_eq!(item.command, Some(CommandId::new("edit.paste")));
        assert_eq!(item.fluent, Some(FluentId::new("Paste")));
        assert_eq!(item.display, Some(DisplayMode::IconText));
        assert_eq!(item.size, Some(ControlSize::Large));
        assert!(item.new_command && item.begin_group && item.dynamic_label);
        assert_eq!(item.options, vec!["A4", "Letter"]);
        assert_eq!(item.items.len(), 1);
    }

    #[test]
    fn layout_decodes_with_the_data_field_names() {
        let json = serde_json::json!({
            "schema_version": 2,
            "id": "panel-modern",
            "name": "Panel Modern",
            "kind": "panel-modern",
            "theme": "panel-modern",
            "window": {"regions": [{"id": "drawing", "edge": "fill"}]},
            "quick_access_toolbar": [{"id": "qat.save", "kind": "button", "command": "file.save"}],
            "file_button": {"label": "File", "opens": "file_view"},
            "tabs": [{"id": "tab.home", "label": "Home", "groups": [
                {"id": "tab.home.clipboard", "label": "Clipboard", "layout": "stack",
                 "controls": [{"id": "tab.home.clipboard.paste", "kind": "button"}]}
            ]}],
            "contextual_tabs": [{"id": "tab.picture_tools.format", "tab_set": "Picture Tools",
                                 "shown_in": "picture_selected", "groups": []}],
            "file_view": {"default_page": "file_view.info", "nav": [
                {"id": "file_view.save", "kind": "command", "label": "Save", "command": "file.save"}
            ]},
            "context_menus": [{"id": "context.shape", "items": []}]
        });
        let layout: LayoutV2 = serde_json::from_value(json).expect("decodes");
        assert_eq!(layout.id, "panel-modern");
        assert_eq!(layout.kind, PanelStyle::PanelModern);
        assert_eq!(layout.tabs[0].groups[0].layout, GroupLayout::Stack);
        assert_eq!(
            layout.contextual_tabs[0].shown_in,
            Some(ContextId::PictureSelected)
        );
        assert_eq!(
            layout.file_view.expect("file view").nav[0].kind,
            FileViewEntryKind::Command
        );
    }

    #[test]
    fn theme_icon_map_and_contexts_decode() {
        let theme: Theme = serde_json::from_value(serde_json::json!({
            "schema_version": 1, "id": "panel-modern", "name": "Panel Modern (white)",
            "fonts": {"ui": {"family": "Selawik", "size_px": 12, "transform": "uppercase"}},
            "colors": {"chrome.background": "#ffffff"},
            "metrics": {"command_bar.height": 92, "button.large": {"height": 69, "icon": 32}},
            "contextual_tab_sets": {"Picture Tools": "green"}
        }))
        .expect("decodes");
        assert_eq!(theme.fonts["ui"].transform.as_deref(), Some("uppercase"));
        assert_eq!(theme.metrics["command_bar.height"], Metric::Px(92.0));
        assert!(matches!(theme.metrics["button.large"], Metric::Fields(_)));

        let icons: IconMap = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "set": {"name": "Tabler Icons", "version": "3.49.0", "license": "MIT",
                    "style": "outline", "source": "https://example.invalid", "path": "p"},
            "fallback": "placeholder",
            "tints": {"icon.blue": "accent blue"},
            "icons": {"edit.cut": {"icon": "scissors", "tint": "icon.blue"}}
        }))
        .expect("decodes");
        assert_eq!(icons.icons["edit.cut"].icon, "scissors");

        let contexts: Contexts = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "contexts": [{"id": "empty_page", "description": "Nothing selected."}],
            "notes": ["a note"],
            "commands": {"edit.cut": {"enabled_in": ["shapes_selected"]},
                         "view.rulers": {"enabled_in": ["empty_page"], "on_by_default": true}}
        }))
        .expect("decodes");
        assert_eq!(contexts.contexts[0].id, ContextId::EmptyPage);
        assert_eq!(contexts.commands["view.rulers"].on_by_default, Some(true));
    }
}
