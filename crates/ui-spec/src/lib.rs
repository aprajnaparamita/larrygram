//! Loader for command-bar, backstage, context-menu and shortcut layout data.
//!
//! The public types below are the contract of `ui-spec/schema.json`
//! (ADR 0007 format, ADR 0008 ShapeSheet table). The loader itself
//! lives in the [`loader`] module: [`Loader`] reads
//! `ui-spec/panel-2016.yaml`, `ui-spec/panel-365.yaml`,
//! `ui-spec/commands.yaml` and `ui-spec/shortcuts.yaml` from a
//! workspace root, validates every file against the schema, decodes
//! it into these types and cross-checks the references between the
//! three files. The [`lookup`] module builds the [`CommandLookup`]
//! index; [`LayoutSets::get`] is the layout switch.
//!
//! The crate is L0 (no workspace deps), so it is usable by any other
//! layer without breaking the layering rules (ADR 0002).

#![forbid(unsafe_code)]
#![allow(clippy::derive_partial_eq_without_eq)] // IDs are content-addressed; equality is fine.

use std::collections::BTreeMap;

pub mod embedded;
pub mod error;
pub mod loader;
pub mod lookup;
pub mod v2;

pub use error::{IdError, LoadError};
pub use loader::Loader;
/// The one layout-kind enum (ADR 0061 amendment): the Phase 1 enum
/// merged into [`v2::LayoutKind`] in task P7-00. Its `Panel2016` and
/// `Panel365` variants carry the pre-rename spellings as serde
/// aliases, so a settings store saved before the rename still loads.
pub use v2::LayoutKind;

/// A Larrygram command id. See ADR 0006.
///
/// The grammar (enforced by `ui-spec/schema.json`):
///
/// ```text
/// command_id  := scope '.' verb ('.' qualifier)*
/// scope       := 'app' | 'file' | 'edit' | 'view' | 'insert'
///              | 'format' | 'text' | 'arrange' | 'shape'
///              | 'connector' | 'page' | 'layer' | 'tools'
///              | 'data' | 'review' | 'developer' | 'help'
/// verb        := [a-z][a-z0-9_]*
/// qualifier   := [a-z][a-z0-9_]*
/// ```
///
/// Scopes are **functional**, not command-bar tabs. The same command may be
/// surfaced on the Home tab in 2016 and on the Arrange tab in 365; the
/// id does not change. The `Control` type's `command` field is the
/// link from a Fluent control to one of these ids.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct CommandId(pub String);

impl CommandId {
    /// Parses and validates a command id (ADR 0006).
    ///
    /// Enforces the same grammar as the `commandId` pattern in
    /// `ui-spec/schema.json`:
    ///
    /// ```text
    /// command_id := scope '.' verb ('.' qualifier)*
    /// token      := [a-z][a-z0-9_]*
    /// ```
    ///
    /// At least two dot-separated tokens (scope and verb); no empty
    /// token (no leading, trailing or consecutive dots). The ADR 0006
    /// scope *vocabulary* (`app`, `file`, `edit`, ...) is a further,
    /// policy-level restriction on top of this grammar; this parser
    /// checks the grammar only, exactly like the schema pattern does.
    ///
    /// ```
    /// use larrygram_ui_spec::CommandId;
    /// assert!(CommandId::parse("file.save").is_ok());
    /// assert!(CommandId::parse("File.Save").is_err()); // uppercase
    /// assert!(CommandId::parse("file").is_err()); // no verb
    /// ```
    pub fn parse(s: &str) -> Result<CommandId, IdError> {
        if s.is_empty() {
            return Err(IdError::new("id is empty", s));
        }
        let mut tokens = 0;
        for token in s.split('.') {
            tokens += 1;
            if token.is_empty() {
                return Err(IdError::new(
                    "empty token (leading, trailing or consecutive '.')",
                    s,
                ));
            }
            if !token.starts_with(|c: char| c.is_ascii_lowercase()) {
                return Err(IdError::new("token must start with a lowercase letter", s));
            }
            if !token
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(IdError::new("token may only contain [a-z0-9_]", s));
            }
        }
        if tokens < 2 {
            return Err(IdError::new("missing '.' between scope and verb", s));
        }
        Ok(CommandId(s.to_owned()))
    }

    /// Wraps a string in `CommandId` **without validation**.
    ///
    /// For trusted data (decoded by the loader, which validates every
    /// document against the schema) and for tests. New code that
    /// accepts an id from a caller should use [`Self::parse`].
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The dotted id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CommandId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CommandId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A Microsoft Fluent UI control name (e.g. `FileSave`).
///
/// Distinct from [`CommandId`] (ADR 0006). The Fluent name is the
/// primary id of every control, group, tab, backstage tab and
/// context menu in the layout files (ADR 0007 B1). The pattern
/// (enforced by `ui-spec/schema.json`) is `^[A-Za-z_][A-Za-z0-9_]*$`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct FluentId(pub String);

impl FluentId {
    /// Parses and validates a Fluent control name.
    ///
    /// Enforces the same pattern as `ui-spec/schema.json`:
    /// `^[A-Za-z_][A-Za-z0-9_]*$`.
    ///
    /// ```
    /// use larrygram_ui_spec::FluentId;
    /// assert!(FluentId::parse("FileSave").is_ok());
    /// assert!(FluentId::parse("2Cut").is_err()); // starts with a digit
    /// ```
    pub fn parse(s: &str) -> Result<FluentId, IdError> {
        if s.is_empty() {
            return Err(IdError::new("id is empty", s));
        }
        if !s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            return Err(IdError::new("id must start with a letter or '_'", s));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(IdError::new("id may only contain [A-Za-z0-9_]", s));
        }
        Ok(FluentId(s.to_owned()))
    }

    /// Wraps a string **without validation**.
    ///
    /// For trusted data (decoded by the loader, which validates every
    /// document against the schema) and for tests. New code that
    /// accepts an id from a caller should use [`Self::parse`].
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The Fluent name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FluentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for FluentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Fluent control types. The full vocabulary is enumerated in
/// `ui-spec/schema.json` under `definitions/controlType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlType {
    /// A top-level command-bar tab.
    Tab,
    /// A tab set inside a command bar (rarely used in Visio).
    TabSet,
    /// A group of controls inside a tab.
    Group,
    /// A click-only button.
    Button,
    /// A two-state button.
    ToggleButton,
    /// A button with a primary action and a dropdown.
    SplitButton,
    /// A grid of items.
    Gallery,
    /// A drop-down menu.
    Menu,
    /// A checkable item.
    CheckBox,
    /// A drop-down list.
    DropDown,
    /// A combined edit + dropdown.
    ComboBox,
    /// A non-interactive label.
    LabelControl,
    /// A generic container.
    Control,
    /// A small button that opens a dialog from a group's corner.
    ButtonDialogBoxLauncher,
    /// A context menu.
    ContextMenu,
    /// A task.
    Task,
    /// A task form group.
    TaskFormGroup,
    /// A backstage category.
    Category,
    /// A placeholder (used internally; never appears in a real layout).
    None,
}

impl ControlType {
    /// Parses the Fluent type string. Returns `None` for unknown
    /// values; the loader treats unknowns as a schema error.
    pub fn parse(s: &str) -> Option<Self> {
        use ControlType as Ct;
        let v = match s {
            "tab" => Ct::Tab,
            "tabSet" => Ct::TabSet,
            "group" => Ct::Group,
            "button" => Ct::Button,
            "toggleButton" => Ct::ToggleButton,
            "splitButton" => Ct::SplitButton,
            "gallery" => Ct::Gallery,
            "menu" => Ct::Menu,
            "checkBox" => Ct::CheckBox,
            "dropDown" => Ct::DropDown,
            "comboBox" => Ct::ComboBox,
            "labelControl" => Ct::LabelControl,
            "control" => Ct::Control,
            "buttonDialogBoxLauncher" => Ct::ButtonDialogBoxLauncher,
            "contextMenu" => Ct::ContextMenu,
            "task" => Ct::Task,
            "taskFormGroup" => Ct::TaskFormGroup,
            "category" => Ct::Category,
            "none" => Ct::None,
            _ => return None,
        };
        Some(v)
    }

    /// The string form used in YAML.
    pub fn as_str(&self) -> &'static str {
        use ControlType::*;
        match self {
            Tab => "tab",
            TabSet => "tabSet",
            Group => "group",
            Button => "button",
            ToggleButton => "toggleButton",
            SplitButton => "splitButton",
            Gallery => "gallery",
            Menu => "menu",
            CheckBox => "checkBox",
            DropDown => "dropDown",
            ComboBox => "comboBox",
            LabelControl => "labelControl",
            Control => "control",
            ButtonDialogBoxLauncher => "buttonDialogBoxLauncher",
            ContextMenu => "contextMenu",
            Task => "task",
            TaskFormGroup => "taskFormGroup",
            Category => "category",
            None => "none",
        }
    }
}

impl<'de> serde::Deserialize<'de> for ControlType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown control type: {raw:?}")))
    }
}

impl serde::Serialize for ControlType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Bounding box in device-independent pixels. Filled in by the
/// screenshot pass (ADR 0007).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Bounds {
    /// X coordinate (left edge).
    pub x: f32,
    /// Y coordinate (top edge).
    pub y: f32,
    /// Width in pixels.
    pub w: f32,
    /// Height in pixels.
    pub h: f32,
}

/// A control in a command bar, backstage, or context menu. See
/// `ui-spec/schema.json#/definitions/control`.
///
/// `id` is the Fluent control name (e.g. `FileSave`); `command` is
/// the Larrygram command id dispatched when the control is activated
/// (e.g. `file.save`). `command` may be `None` for non-actionable
/// controls and for controls whose mapping is not yet known.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Control {
    /// The Fluent control name (primary id).
    pub id: FluentId,
    /// The Fluent type.
    #[serde(rename = "type")]
    pub r#type: ControlType,
    /// Short UI label (never a description).
    pub label: String,
    /// Sibling order.
    pub order: u32,
    /// Immediate parent control id (Fluent), or `None` for top-level.
    #[serde(default)]
    pub parent: Option<FluentId>,
    /// Larrygram command id dispatched on activation, or `None` if
    /// the control is non-actionable or its mapping is unknown.
    #[serde(default)]
    pub command: Option<CommandId>,
    /// Alternate Fluent ids for the same control across command-bar
    /// placements (e.g. when `ConnectorTool` appears on both the
    /// Home and Insert tabs in 365). P1-03 picks a primary
    /// parent by `tab` match; the others are recorded here.
    #[serde(default)]
    pub aliases: Vec<FluentId>,
    /// Reserved for the screenshot pass.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Reserved for the screenshot pass. Path or id of an icon asset.
    #[serde(default)]
    pub icon_ref: Option<String>,
    /// Reserved for the screenshot pass. The keytip shown next to the
    /// label.
    #[serde(default)]
    pub keytip: Option<String>,
    /// Reserved for the screenshot pass. The button size variant.
    #[serde(default)]
    pub size_variant: Option<SizeVariant>,
    /// Reserved for the screenshot pass. Free-text annotation; never a
    /// description of behaviour.
    #[serde(default)]
    pub visual_notes: Option<String>,
}

/// Reserved button size variant. See ADR 0007.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeVariant {
    /// Small button.
    Small,
    /// Medium button.
    Medium,
    /// Large button.
    Large,
    /// Extra-large button.
    ExtraLarge,
}

impl SizeVariant {
    /// Parses the YAML string.
    pub fn parse(s: &str) -> Option<Self> {
        let v = match s {
            "small" => Self::Small,
            "medium" => Self::Medium,
            "large" => Self::Large,
            "extra-large" => Self::ExtraLarge,
            _ => return None,
        };
        Some(v)
    }

    /// The YAML string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::ExtraLarge => "extra-large",
        }
    }
}

impl<'de> serde::Deserialize<'de> for SizeVariant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown size variant: {raw:?}")))
    }
}

impl serde::Serialize for SizeVariant {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A group of controls inside a tab.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    /// Group Fluent id.
    pub id: FluentId,
    /// Group label.
    pub label: String,
    /// Sibling order.
    pub order: u32,
    /// Reserved for the screenshot pass.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Reserved for the screenshot pass. Path or id of an icon asset.
    #[serde(default)]
    pub icon_ref: Option<String>,
    /// Reserved for the screenshot pass. The keytip shown next to the
    /// label.
    #[serde(default)]
    pub keytip: Option<String>,
    /// Reserved for the screenshot pass. The button size variant.
    #[serde(default)]
    pub size_variant: Option<SizeVariant>,
    /// Reserved for the screenshot pass. Free-text annotation; never a
    /// description of behaviour.
    #[serde(default)]
    pub visual_notes: Option<String>,
    /// Controls in display order.
    pub controls: Vec<Control>,
}

/// A top-level command-bar tab.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tab {
    /// Tab Fluent id.
    pub id: FluentId,
    /// Tab label.
    pub label: String,
    /// Sibling order.
    pub order: u32,
    /// Fluent tab-set name (e.g. `TabSetPrimary`); `None` for tabs
    /// that are not part of a tab set.
    #[serde(default)]
    pub tab_set: Option<String>,
    /// Reserved for the screenshot pass.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Reserved for the screenshot pass. Path or id of an icon asset.
    #[serde(default)]
    pub icon_ref: Option<String>,
    /// Reserved for the screenshot pass. The keytip shown next to the
    /// label.
    #[serde(default)]
    pub keytip: Option<String>,
    /// Reserved for the screenshot pass. The button size variant.
    #[serde(default)]
    pub size_variant: Option<SizeVariant>,
    /// Reserved for the screenshot pass. Free-text annotation; never a
    /// description of behaviour.
    #[serde(default)]
    pub visual_notes: Option<String>,
    /// Groups in display order.
    pub groups: Vec<Group>,
}

/// Backstage surface. Tabs are flat (no groups).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Backstage {
    /// Tabs under the File button.
    pub tabs: Vec<BackstageTab>,
}

/// A single backstage tab. Always flat (no groups); controls are
/// shown as a vertical stack.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BackstageTab {
    /// Tab Fluent id.
    pub id: FluentId,
    /// Tab label.
    pub label: String,
    /// Sibling order.
    pub order: u32,
    /// Reserved for the screenshot pass.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Reserved for the screenshot pass. Path or id of an icon asset.
    #[serde(default)]
    pub icon_ref: Option<String>,
    /// Reserved for the screenshot pass. The keytip shown next to the
    /// label.
    #[serde(default)]
    pub keytip: Option<String>,
    /// Reserved for the screenshot pass. The button size variant.
    #[serde(default)]
    pub size_variant: Option<SizeVariant>,
    /// Reserved for the screenshot pass. Free-text annotation; never a
    /// description of behaviour.
    #[serde(default)]
    pub visual_notes: Option<String>,
    /// Controls in display order.
    pub controls: Vec<Control>,
}

/// A context menu.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextMenu {
    /// Menu Fluent id.
    pub id: FluentId,
    /// Menu label (shown in the menu header). One to 80 characters,
    /// never empty (the schema's `label` rule; the loader rejects
    /// anything else).
    pub label: String,
    /// Sibling order.
    pub order: u32,
    /// Reserved for the screenshot pass.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Reserved for the screenshot pass. Path or id of an icon asset.
    #[serde(default)]
    pub icon_ref: Option<String>,
    /// Reserved for the screenshot pass. The keytip shown next to the
    /// label.
    #[serde(default)]
    pub keytip: Option<String>,
    /// Reserved for the screenshot pass. The button size variant.
    #[serde(default)]
    pub size_variant: Option<SizeVariant>,
    /// Reserved for the screenshot pass. Free-text annotation; never a
    /// description of behaviour.
    #[serde(default)]
    pub visual_notes: Option<String>,
    /// Controls in display order.
    pub controls: Vec<Control>,
}

/// The Quick Access Toolbar (QAT).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuickAccessToolbar {
    /// Controls in display order.
    pub controls: Vec<Control>,
}

/// A complete layout (e.g. `panel-2016.yaml`).
///
/// `extracted` is intentionally absent: a timestamp would change on
/// every `cargo xtask extract` run, breaking ADR 0009's
/// deterministic-output rule. `source_revision` is the pinned source
/// identifier.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    /// Always `1` for now.
    pub schema_version: u32,
    /// Always `visio` for now.
    pub product: String,
    /// Human-readable source description (e.g. "Microsoft Fluent UI
    /// 365, Current Channel").
    pub source: String,
    /// URL of the source dataset.
    #[serde(default)]
    pub source_url: String,
    /// Pinning info (commit, channel, date).
    #[serde(default)]
    pub source_revision: String,
    /// Top-level command-bar tabs (excludes the File tab's backstage).
    pub tabs: Vec<Tab>,
    /// Backstage tabs under the File button.
    pub backstage: Backstage,
    /// Context menus (canvas + per-shape).
    pub context_menus: Vec<ContextMenu>,
    /// Quick Access Toolbar.
    pub quick_access_toolbar: QuickAccessToolbar,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            schema_version: 1,
            product: "visio".to_owned(),
            source: String::new(),
            source_url: String::new(),
            source_revision: String::new(),
            tabs: Vec::new(),
            backstage: Backstage { tabs: Vec::new() },
            context_menus: Vec::new(),
            quick_access_toolbar: QuickAccessToolbar {
                controls: Vec::new(),
            },
        }
    }
}

/// Mapping from Larrygram command id to VisUICmds and to one or more
/// Fluent control names. See `ui-spec/commands.yaml` and ADR 0006.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandMapping {
    /// The Larrygram command id.
    pub id: CommandId,
    /// Short UI label.
    pub label: String,
    /// The VisUICmds constant (e.g. `visCmdFileSave`), or `None` when
    /// no mapping is known.
    #[serde(default)]
    pub visuicmd: Option<String>,
    /// Fluent control names that surface this command.
    pub fluent_ids: Vec<FluentId>,
    /// How confident the mapping is.
    pub mapping_confidence: MappingConfidence,
}

/// Confidence in a command mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappingConfidence {
    /// The VisUICmds name and the Fluent control name are both known
    /// and point at the same action.
    Exact,
    /// At least one side is inferred by name match.
    Inferred,
    /// Neither side is known; the command is registered but unimplemented.
    Unknown,
}

impl MappingConfidence {
    /// Parses the YAML string.
    pub fn parse(s: &str) -> Option<Self> {
        let v = match s {
            "exact" => Self::Exact,
            "inferred" => Self::Inferred,
            "unknown" => Self::Unknown,
            _ => return None,
        };
        Some(v)
    }

    /// The YAML string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Inferred => "inferred",
            Self::Unknown => "unknown",
        }
    }
}

impl<'de> serde::Deserialize<'de> for MappingConfidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown mapping confidence: {raw:?}")))
    }
}

impl serde::Serialize for MappingConfidence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A keyboard shortcut. See `ui-spec/shortcuts.yaml` and ADR 0007.
///
/// `sequence` is the list of chords the user types in order. A
/// single-chord shortcut (e.g. Ctrl+Z) has a one-element sequence;
/// a multi-chord command-bar keytip (e.g. Alt+H, F, O) is a three-element
/// sequence. Each chord is a set of keys pressed simultaneously.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Shortcut {
    /// Ordered list of chords. Each chord is a non-empty list of key
    /// tokens (modifier or key, e.g. `Ctrl`, `Shift`, `A`, `F1`).
    pub sequence: Vec<Vec<String>>,
    /// The command the shortcut dispatches.
    pub command: CommandId,
    /// Where the shortcut applies.
    pub scope: ShortcutScope,
    /// Optional group name of our own (e.g. `Common`, `Text`), used to group shortcuts in the UI.
    #[serde(default)]
    pub section: Option<String>,
}

/// Where a shortcut applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutScope {
    /// Always available.
    Global,
    /// Only when the command bar has focus.
    CommandBar,
    /// Only when a drawing page has focus.
    Page,
    /// Only when a text editor has focus.
    Text,
    /// Only when one or more shapes are selected.
    Selection,
    /// Only when a particular window is active.
    Window,
}

impl ShortcutScope {
    /// Parses the YAML string.
    pub fn parse(s: &str) -> Option<Self> {
        let v = match s {
            "global" => Self::Global,
            "command_bar" => Self::CommandBar,
            "page" => Self::Page,
            "text" => Self::Text,
            "selection" => Self::Selection,
            "window" => Self::Window,
            _ => return None,
        };
        Some(v)
    }

    /// The YAML string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::CommandBar => "command_bar",
            Self::Page => "page",
            Self::Text => "text",
            Self::Selection => "selection",
            Self::Window => "window",
        }
    }
}

impl<'de> serde::Deserialize<'de> for ShortcutScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown shortcut scope: {raw:?}")))
    }
}

impl serde::Serialize for ShortcutScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A `ui-spec/commands.yaml` document.
///
/// Serializes as the bare entry list of the data file, so the
/// document round-trips through the on-disk format.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Commands {
    /// One entry per Larrygram command.
    pub entries: Vec<CommandMapping>,
}

impl serde::Serialize for Commands {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entries.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Commands {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<CommandMapping>::deserialize(deserializer).map(|entries| Commands { entries })
    }
}

/// A `ui-spec/shortcuts.yaml` document.
///
/// Serializes as the bare entry list of the data file, so the
/// document round-trips through the on-disk format.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shortcuts {
    /// One entry per keyboard shortcut.
    pub entries: Vec<Shortcut>,
}

impl serde::Serialize for Shortcuts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entries.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Shortcuts {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<Shortcut>::deserialize(deserializer).map(|entries| Shortcuts { entries })
    }
}

/// The full set of data loaded for a single layout kind: the v1
/// Fluent data where there is any, the v2 model the widgets read
/// (ADR 0061), the shared tables, and the theme, icon map and
/// contexts that belong with it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct LayoutSet {
    /// The v1 command bar + backstage + context menus + QAT. Empty
    /// for Panel Modern and Panel Classic, which are v2-only.
    pub layout: Layout,
    /// The v2 layout every widget reads: the curated file for Panel
    /// Modern and Panel Classic, [`v2::from_v1`] output for the two
    /// Fluent-data kinds.
    pub v2: v2::LayoutV2,
    /// The commands referenced by the layout.
    pub commands: Commands,
    /// The shortcuts dispatched on this layout.
    pub shortcuts: Shortcuts,
    /// The layout's theme (ADR 0063): `themes/<theme>.yaml`. The two
    /// adapted layouts use the Panel Modern theme.
    pub theme: v2::Theme,
    /// The icon map (ADR 0062), shared by every layout.
    pub icons: v2::IconMap,
    /// The selection contexts (ADR 0064), shared by every layout.
    pub contexts: v2::Contexts,
}

/// All four [`LayoutSet`]s, loaded together by
/// [`Loader::load_all`](crate::Loader::load_all) or
/// [`Loader::from_embedded`](crate::Loader::from_embedded).
///
/// `commands.yaml` and `shortcuts.yaml` are shared by the layouts,
/// so the ADR 0007 cross-references are checked over the whole set at
/// once: a Fluent id listed in `commands.yaml` must appear in at
/// least one of the two v1 layouts. [`Self::get`] is the runtime
/// layout switch: it borrows one set by its [`LayoutKind`].
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct LayoutSets {
    /// Panel Modern: the curated 2013 data (ADR 0061).
    pub panel_modern: LayoutSet,
    /// Panel Classic: the curated 2007 data (ADR 0061).
    pub panel_classic: LayoutSet,
    /// Panel (2016 data): the Office 2016 Fluent data.
    pub panel_2016: LayoutSet,
    /// Panel (365 data): the Microsoft 365 Current Channel data.
    pub panel_365: LayoutSet,
}

/// The [`crate::v2::LayoutKind`] of the requested layout, borrowed.
///
/// All four kinds have a [`LayoutSet`]: Panel Modern and Panel
/// Classic carry the hand-curated v2 files, the two Fluent-data kinds
/// carry their v1 layout plus the [`v2::from_v1`] conversion (ADR
/// 0061). The borrow keeps every set usable (task P7-09 drops the
/// v1-data `Option`: the shell draws the curated kinds from
/// [`crate::v2`] and gates the v1 widgets on [`LayoutSet::layout`]).
impl LayoutSets {
    /// The set for `kind` (borrowed, so the caller keeps all four).
    pub fn get(&self, kind: LayoutKind) -> &LayoutSet {
        match kind {
            LayoutKind::Panel2016 => &self.panel_2016,
            LayoutKind::Panel365 => &self.panel_365,
            LayoutKind::PanelModern => &self.panel_modern,
            LayoutKind::PanelClassic => &self.panel_classic,
        }
    }
}

/// A lookup table built from a [`LayoutSet`]. The UI walks the
/// layout tree directly using the Fluent ids; the lookup is a
/// precomputed index for the most common queries.
///
/// [`CommandLookup::build`] fills the three indexes from one layout.
/// Indexes are `BTreeMap`-backed so iteration is sorted.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandLookup {
    /// `FluentId -> label`, built from every control, group, tab,
    /// backstage tab and context menu in the layout.
    pub labels: BTreeMap<FluentId, String>,
    /// `FluentId -> dispatched CommandId`, when the control has a
    /// `command` link. [`CommandLookup::build`] picks the primary
    /// path (the control's own id) when a control has aliases.
    pub commands: BTreeMap<FluentId, CommandId>,
    /// `CommandId -> FluentIds` of every control that dispatches
    /// this command. The reverse of the `commands` index.
    pub fluent_by_command: BTreeMap<CommandId, Vec<FluentId>>,
}

impl CommandLookup {
    /// An empty lookup. [`CommandLookup::build`] constructs a
    /// populated one from a [`Layout`].
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of Fluent ids in the lookup.
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// True if the lookup has no entries.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_type_round_trip() {
        for s in [
            "tab",
            "tabSet",
            "group",
            "button",
            "toggleButton",
            "splitButton",
            "gallery",
            "menu",
            "checkBox",
            "dropDown",
            "comboBox",
            "labelControl",
            "control",
            "buttonDialogBoxLauncher",
            "contextMenu",
            "task",
            "taskFormGroup",
            "category",
            "none",
        ] {
            let t = ControlType::parse(s).unwrap_or_else(|| panic!("missing {s}"));
            assert_eq!(t.as_str(), s, "round trip failed for {s}");
        }
    }

    #[test]
    fn control_type_rejects_unknown() {
        assert!(ControlType::parse("notAType").is_none());
        assert!(ControlType::parse("Button").is_none()); // case-sensitive
    }

    #[test]
    fn mapping_confidence_round_trip() {
        for s in ["exact", "inferred", "unknown"] {
            let c = MappingConfidence::parse(s).unwrap();
            assert_eq!(c.as_str(), s);
        }
        assert!(MappingConfidence::parse("maybe").is_none());
    }

    #[test]
    fn shortcut_scope_round_trip() {
        for s in [
            "global",
            "command_bar",
            "page",
            "text",
            "selection",
            "window",
        ] {
            let v = ShortcutScope::parse(s).unwrap();
            assert_eq!(v.as_str(), s);
        }
        assert!(ShortcutScope::parse("everywhere").is_none());
    }

    #[test]
    fn layout_kind_file_names() {
        assert_eq!(LayoutKind::Panel2016.file_name(), "panel-2016.yaml");
        assert_eq!(LayoutKind::Panel365.file_name(), "panel-365.yaml");
    }

    #[test]
    fn command_id_display() {
        let id = CommandId::new("arrange.align.left");
        assert_eq!(id.to_string(), "arrange.align.left");
        assert_eq!(id.as_str(), "arrange.align.left");
    }

    #[test]
    fn command_id_ref() {
        let id = CommandId::new("file.save");
        let s: &str = id.as_ref();
        assert_eq!(s, "file.save");
    }

    #[test]
    fn fluent_id_display_and_ref() {
        let f = FluentId::new("FileSave");
        assert_eq!(f.to_string(), "FileSave");
        assert_eq!(f.as_str(), "FileSave");
        let s: &str = f.as_ref();
        assert_eq!(s, "FileSave");
    }

    #[test]
    fn empty_lookup_is_empty() {
        let l = CommandLookup::empty();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert!(l.labels.is_empty());
        assert!(l.commands.is_empty());
        assert!(l.fluent_by_command.is_empty());
    }

    #[test]
    fn default_layout_has_empty_surfaces() {
        let l = Layout::default();
        assert_eq!(l.schema_version, 1);
        assert_eq!(l.product, "visio");
        assert!(l.tabs.is_empty());
        assert!(l.backstage.tabs.is_empty());
        assert!(l.context_menus.is_empty());
        assert!(l.quick_access_toolbar.controls.is_empty());
    }

    #[test]
    fn default_layout_set_is_empty() {
        let s = LayoutSet::default();
        assert!(s.layout.tabs.is_empty());
        assert!(s.commands.entries.is_empty());
        assert!(s.shortcuts.entries.is_empty());
    }

    #[test]
    fn layout_sets_get_borrows_the_kind_matching_set() {
        let mut sets = LayoutSets::default();
        sets.panel_2016.layout.source = "2016".to_owned();
        sets.panel_365.layout.source = "365".to_owned();
        assert_eq!(sets.get(LayoutKind::Panel2016).layout.source, "2016");
        assert_eq!(sets.get(LayoutKind::Panel365).layout.source, "365");
        // Borrows, so both sets stay usable.
        assert_eq!(sets.panel_2016.layout.product, "visio");
        assert_eq!(sets.panel_365.layout.source, "365");
        assert!(
            LayoutSets::default()
                .get(LayoutKind::Panel365)
                .layout
                .tabs
                .is_empty()
        );
    }

    #[test]
    fn layout_sets_get_answers_every_kind() {
        // Task P7-09 drops the v1-data `Option`: the shell draws the
        // curated kinds from `v2`, so `get` borrows the kind's own set
        // for all four kinds. (The committed sets carry v1 data only
        // for the two Fluent-data kinds; `tests/v2_layouts.rs` pins
        // that.)
        let sets = LayoutSets::default();
        let cases = [
            (LayoutKind::Panel2016, &sets.panel_2016),
            (LayoutKind::Panel365, &sets.panel_365),
            (LayoutKind::PanelModern, &sets.panel_modern),
            (LayoutKind::PanelClassic, &sets.panel_classic),
        ];
        for (kind, expected) in cases {
            assert!(std::ptr::eq(sets.get(kind), expected), "{kind:?}");
        }
    }

    #[test]
    fn the_thin_accessors_run_through_function_pointers() {
        // The shell and the lookup users call these accessors
        // directly; calling them through function pointers here too
        // keeps the standalone instrumented copies exercised whatever
        // the inliner decides, so the coverage of the seams stays
        // stable across builds.
        let sets = LayoutSets::default();
        let get: fn(&LayoutSets, LayoutKind) -> &LayoutSet = LayoutSets::get;
        assert!(get(&sets, LayoutKind::Panel2016).layout.tabs.is_empty());
        assert!(get(&sets, LayoutKind::PanelClassic).v2.tabs.is_empty());
        let empty: fn() -> CommandLookup = CommandLookup::empty;
        let lookup = empty();
        let len: fn(&CommandLookup) -> usize = CommandLookup::len;
        let is_empty: fn(&CommandLookup) -> bool = CommandLookup::is_empty;
        assert_eq!(len(&lookup), 0);
        assert!(is_empty(&lookup));
    }

    #[test]
    fn bounds_default_is_zero() {
        let b = Bounds::default();
        assert_eq!(b.x, 0.0);
        assert_eq!(b.y, 0.0);
        assert_eq!(b.w, 0.0);
        assert_eq!(b.h, 0.0);
    }

    #[test]
    fn size_variant_round_trip() {
        for s in ["small", "medium", "large", "extra-large"] {
            let v = SizeVariant::parse(s).unwrap();
            assert_eq!(v.as_str(), s);
        }
        assert!(SizeVariant::parse("huge").is_none());
    }

    #[test]
    fn command_id_ord_and_eq() {
        let a = CommandId::new("file.save");
        let b = CommandId::new("file.save");
        let c = CommandId::new("file.save_as");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a < c);
    }

    #[test]
    fn fluent_id_ord() {
        let a = FluentId::new("AlignLeftButton");
        let b = FluentId::new("AlignRightButton");
        let c = FluentId::new("FileSave");
        assert!(a < b);
        assert!(b < c);
        assert_eq!(a, FluentId::new("AlignLeftButton"));
    }

    #[test]
    fn layout_kind_eq_and_hash() {
        use std::collections::HashSet;
        let mut s = HashSet::new();
        s.insert(LayoutKind::Panel2016);
        s.insert(LayoutKind::Panel365);
        s.insert(LayoutKind::Panel2016);
        assert_eq!(s.len(), 2);
        assert_ne!(LayoutKind::Panel2016, LayoutKind::Panel365);
    }

    #[test]
    fn control_carries_fluent_id_and_command_link() {
        // Sanity: Control.id is a FluentId, Control.command is an
        // Option<CommandId>. B1 finding.
        let c = Control {
            id: FluentId::new("FileSave"),
            r#type: ControlType::Button,
            label: "Save".into(),
            order: 0,
            parent: None,
            command: Some(CommandId::new("file.save")),
            aliases: vec![],
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
        };
        assert_eq!(c.id, FluentId::new("FileSave"));
        assert_eq!(c.command, Some(CommandId::new("file.save")));
        assert!(c.aliases.is_empty());
    }

    #[test]
    fn shortcut_carries_sequence_of_chords() {
        // m2 finding: shortcut.sequence is an ordered list of chords.
        let s = Shortcut {
            sequence: vec![vec!["Ctrl".into(), "Z".into()]],
            command: CommandId::new("edit.undo"),
            scope: ShortcutScope::Global,
            section: Some("Common".into()),
        };
        assert_eq!(s.sequence.len(), 1);
        assert_eq!(s.sequence[0], vec!["Ctrl", "Z"]);

        // Multi-chord keytip (Alt+H, F, O).
        let s2 = Shortcut {
            sequence: vec![
                vec!["Alt".into(), "H".into()],
                vec!["F".into()],
                vec!["O".into()],
            ],
            command: CommandId::new("file.open"),
            scope: ShortcutScope::Global,
            section: None,
        };
        assert_eq!(s2.sequence.len(), 3);
    }

    #[test]
    fn serde_decodes_control_with_null_command() {
        let json = r#"{
            "id": "Cut", "type": "button", "label": "Cut", "order": 3,
            "command": null, "aliases": ["CutMenu"],
            "bounds": null, "icon_ref": null, "keytip": null,
            "size_variant": "small", "visual_notes": null
        }"#;
        let c: Control = serde_json::from_str(json).unwrap();
        assert_eq!(c.id, FluentId::new("Cut"));
        assert_eq!(c.r#type, ControlType::Button);
        assert_eq!(c.order, 3);
        assert_eq!(c.command, None);
        assert_eq!(c.aliases, vec![FluentId::new("CutMenu")]);
        assert_eq!(c.size_variant, Some(SizeVariant::Small));
    }

    #[test]
    fn serde_decodes_control_with_missing_optional_fields() {
        let json = r#"{"id": "Cut", "type": "menu", "label": "Cut", "order": 0}"#;
        let c: Control = serde_json::from_str(json).unwrap();
        assert_eq!(c.r#type, ControlType::Menu);
        assert_eq!(c.command, None);
        assert!(c.aliases.is_empty());
        assert!(c.bounds.is_none());
        assert!(c.parent.is_none());
    }

    #[test]
    fn serde_control_type_field_is_named_type() {
        // `r#type` must map to the YAML key `type`, not `r#type`.
        let json = r#"{"id": "X", "type": "splitButton", "label": "X", "order": 0}"#;
        let c: Control = serde_json::from_str(json).unwrap();
        assert_eq!(c.r#type, ControlType::SplitButton);
    }

    #[test]
    fn serde_enums_accept_their_as_str_values() {
        for s in [
            "tab",
            "tabSet",
            "group",
            "button",
            "toggleButton",
            "splitButton",
            "gallery",
            "menu",
            "checkBox",
            "dropDown",
            "comboBox",
            "labelControl",
            "control",
            "buttonDialogBoxLauncher",
            "contextMenu",
            "task",
            "taskFormGroup",
            "category",
            "none",
        ] {
            let t: ControlType = serde_json::from_str(&format!("\"{s}\"")).unwrap();
            assert_eq!(t.as_str(), s);
        }
        for s in ["small", "medium", "large", "extra-large"] {
            let v: SizeVariant = serde_json::from_str(&format!("\"{s}\"")).unwrap();
            assert_eq!(v.as_str(), s);
        }
        for s in ["exact", "inferred", "unknown"] {
            let c: MappingConfidence = serde_json::from_str(&format!("\"{s}\"")).unwrap();
            assert_eq!(c.as_str(), s);
        }
        for s in [
            "global",
            "command_bar",
            "page",
            "text",
            "selection",
            "window",
        ] {
            let v: ShortcutScope = serde_json::from_str(&format!("\"{s}\"")).unwrap();
            assert_eq!(v.as_str(), s);
        }
    }

    #[test]
    fn serde_enums_reject_unknown_values() {
        assert!(serde_json::from_str::<ControlType>("\"notAType\"").is_err());
        assert!(serde_json::from_str::<SizeVariant>("\"huge\"").is_err());
        assert!(serde_json::from_str::<MappingConfidence>("\"maybe\"").is_err());
        assert!(serde_json::from_str::<ShortcutScope>("\"everywhere\"").is_err());
    }

    #[test]
    fn serde_decodes_ids_and_bounds() {
        let id: CommandId = serde_json::from_str("\"file.save\"").unwrap();
        assert_eq!(id, CommandId::new("file.save"));
        let f: FluentId = serde_json::from_str("\"FileSave\"").unwrap();
        assert_eq!(f, FluentId::new("FileSave"));
        let b: Bounds = serde_json::from_str("{\"x\": 1.5, \"w\": 2.0}").unwrap();
        assert_eq!(
            b,
            Bounds {
                x: 1.5,
                y: 0.0,
                w: 2.0,
                h: 0.0
            }
        );
    }

    #[test]
    fn command_id_parse_accepts_the_grammar() {
        for s in [
            "file.save",
            "app.quit",
            "arrange.align.left",
            "format.text.bold",
            "tools.snap_to_grid_1",
            "a.b",
            "x2.y9.z_",
        ] {
            assert_eq!(CommandId::parse(s).expect(s), CommandId::new(s), "for {s}");
        }
    }

    #[test]
    fn command_id_parse_rejects_broken_ids() {
        let cases = [
            ("", "id is empty"),
            (
                ".save",
                "empty token (leading, trailing or consecutive '.')",
            ),
            (
                "file.",
                "empty token (leading, trailing or consecutive '.')",
            ),
            (
                "file..save",
                "empty token (leading, trailing or consecutive '.')",
            ),
            ("File.save", "token must start with a lowercase letter"),
            ("file.Save", "token must start with a lowercase letter"),
            ("file.9save", "token must start with a lowercase letter"),
            ("file.sav-e", "token may only contain [a-z0-9_]"),
            ("file.save!", "token may only contain [a-z0-9_]"),
            ("fi le.save", "token may only contain [a-z0-9_]"),
            ("file", "missing '.' between scope and verb"),
        ];
        for (s, reason) in cases {
            let err = CommandId::parse(s).expect_err(s);
            assert_eq!(err.reason(), reason, "for {s:?}");
            assert_eq!(err.value(), s);
        }
    }

    #[test]
    fn command_id_parse_agrees_with_new_on_valid_ids() {
        for s in ["file.save", "edit.undo"] {
            assert_eq!(CommandId::parse(s).unwrap(), CommandId::new(s));
        }
    }

    #[test]
    fn fluent_id_parse_accepts_the_pattern() {
        for s in ["FileSave", "_private", "a", "A1_b", "visCmdFileSave", "x"] {
            assert_eq!(FluentId::parse(s).expect(s), FluentId::new(s), "for {s}");
        }
    }

    #[test]
    fn fluent_id_parse_rejects_broken_ids() {
        let cases = [
            ("", "id is empty"),
            ("2Cut", "id must start with a letter or '_'"),
            ("-Cut", "id must start with a letter or '_'"),
            ("Cut!", "id may only contain [A-Za-z0-9_]"),
            ("Cut Menu", "id may only contain [A-Za-z0-9_]"),
            ("Cut.Menu", "id may only contain [A-Za-z0-9_]"),
        ];
        for (s, reason) in cases {
            let err = FluentId::parse(s).expect_err(s);
            assert_eq!(err.reason(), reason, "for {s:?}");
            assert_eq!(err.value(), s);
        }
        assert_eq!(
            FluentId::parse("FileSave").unwrap(),
            FluentId::new("FileSave")
        );
    }

    #[test]
    fn enums_serialize_as_their_as_str_values() {
        assert_eq!(
            serde_json::to_string(&ControlType::SplitButton).unwrap(),
            "\"splitButton\""
        );
        assert_eq!(
            serde_json::to_string(&SizeVariant::ExtraLarge).unwrap(),
            "\"extra-large\""
        );
        assert_eq!(
            serde_json::to_string(&MappingConfidence::Inferred).unwrap(),
            "\"inferred\""
        );
        assert_eq!(
            serde_json::to_string(&ShortcutScope::Selection).unwrap(),
            "\"selection\""
        );
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        assert_eq!(
            serde_json::to_string(&CommandId::new("file.save")).unwrap(),
            "\"file.save\""
        );
        assert_eq!(
            serde_json::to_string(&FluentId::new("FileSave")).unwrap(),
            "\"FileSave\""
        );
    }

    #[test]
    fn commands_and_shortcuts_serialize_as_bare_entry_lists() {
        let commands = Commands {
            entries: vec![CommandMapping {
                id: CommandId::new("file.save"),
                label: "Save".to_owned(),
                visuicmd: Some("visCmdFileSave".to_owned()),
                fluent_ids: vec![FluentId::new("FileSave")],
                mapping_confidence: MappingConfidence::Exact,
            }],
        };
        let value = serde_json::to_value(&commands).unwrap();
        assert_eq!(value[0]["id"], "file.save");
        assert_eq!(value[0]["fluent_ids"][0], "FileSave");
        let back: Commands = serde_json::from_value(value).unwrap();
        assert_eq!(back, commands);

        let shortcuts = Shortcuts {
            entries: vec![Shortcut {
                sequence: vec![vec!["Ctrl".to_owned(), "S".to_owned()]],
                command: CommandId::new("file.save"),
                scope: ShortcutScope::Global,
                section: None,
            }],
        };
        let value = serde_json::to_value(&shortcuts).unwrap();
        assert_eq!(value[0]["command"], "file.save");
        let back: Shortcuts = serde_json::from_value(value).unwrap();
        assert_eq!(back, shortcuts);
    }

    /// Round-trips a value through JSON: deserialize, serialize,
    /// deserialize must give an equal value.
    fn round_trip<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_value(value).expect("serializes");
        let back: T = serde_json::from_value(json).expect("deserializes");
        assert_eq!(&back, value);
    }

    #[test]
    fn model_types_round_trip_through_json() {
        let control = Control {
            id: FluentId::new("FileSave"),
            r#type: ControlType::Button,
            label: "Save".to_owned(),
            order: 3,
            parent: Some(FluentId::new("GroupFile")),
            command: Some(CommandId::new("file.save")),
            aliases: vec![FluentId::new("FileSaveMenu")],
            bounds: Some(Bounds {
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            }),
            icon_ref: Some("icons/save.svg".to_owned()),
            keytip: Some("S".to_owned()),
            size_variant: Some(SizeVariant::Large),
            visual_notes: Some("note".to_owned()),
        };
        round_trip(&control);
        round_trip(&control.r#type);
        round_trip(&control.size_variant);
        round_trip(&control.bounds.unwrap());
        round_trip(&control.id);
        round_trip(&control.command.clone().unwrap());
        round_trip(&MappingConfidence::Unknown);
        round_trip(&ShortcutScope::Window);
        round_trip(&LayoutKind::Panel365);

        let group = Group {
            id: FluentId::new("GroupFile"),
            label: "File".to_owned(),
            order: 0,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            controls: vec![control.clone()],
        };
        round_trip(&group);

        let tab = Tab {
            id: FluentId::new("TabHome"),
            label: "Home".to_owned(),
            order: 0,
            tab_set: Some("TabSetPrimary".to_owned()),
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            groups: vec![group],
        };
        round_trip(&tab);

        let backstage_tab = BackstageTab {
            id: FluentId::new("FileSave"),
            label: "Save".to_owned(),
            order: 0,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            controls: vec![control.clone()],
        };
        round_trip(&backstage_tab);
        round_trip(&Backstage {
            tabs: vec![backstage_tab],
        });

        let menu = ContextMenu {
            id: FluentId::new("ShapeShortcutMenu"),
            label: "Shape".to_owned(),
            order: 0,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            controls: vec![control.clone()],
        };
        round_trip(&menu);
        round_trip(&QuickAccessToolbar {
            controls: vec![control],
        });

        let layout = Layout {
            tabs: vec![tab],
            ..Layout::default()
        };
        round_trip(&layout);

        let mapping = CommandMapping {
            id: CommandId::new("file.save"),
            label: "Save".to_owned(),
            visuicmd: None,
            fluent_ids: vec![FluentId::new("FileSave")],
            mapping_confidence: MappingConfidence::Inferred,
        };
        round_trip(&mapping);
        round_trip(&Commands {
            entries: vec![mapping],
        });

        let shortcut = Shortcut {
            sequence: vec![vec!["Ctrl".to_owned(), "S".to_owned()]],
            command: CommandId::new("file.save"),
            scope: ShortcutScope::Global,
            section: None,
        };
        round_trip(&shortcut);
        round_trip(&Shortcuts {
            entries: vec![shortcut],
        });

        let set = LayoutSet {
            layout,
            ..LayoutSet::default()
        };
        round_trip(&set);
        round_trip(&LayoutSets {
            panel_2016: set.clone(),
            panel_365: set,
            ..LayoutSets::default()
        });
        round_trip(&CommandLookup::empty());
        round_trip(&CommandLookup {
            labels: BTreeMap::from([(FluentId::new("FileSave"), "Save".to_owned())]),
            commands: BTreeMap::from([(FluentId::new("FileSave"), CommandId::new("file.save"))]),
            fluent_by_command: BTreeMap::from([(
                CommandId::new("file.save"),
                vec![FluentId::new("FileSave")],
            )]),
        });
    }
}
