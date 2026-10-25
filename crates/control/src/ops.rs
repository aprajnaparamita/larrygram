//! The typed operations both front ends share (ADR 0080: one
//! implementation, two front ends).
//!
//! An [`Op`] is what a remote caller asks for; an [`OpResult`] is what it
//! gets back. The control protocol ([`crate::protocol`]) carries them as
//! JSON-RPC methods whose params and results are exactly the structs
//! below; MCP ([`crate::mcp`]) maps its tools onto the same values. The
//! [`crate::exec::Executor`] runs them against a [`crate::host::Host`].
//!
//! JSON conventions (ADR 0078): field names are `snake_case`; lengths
//! are inches, angles degrees, positions page coordinates (origin at the
//! bottom left, y up, as Visio stores them); pages are 1-based indexes
//! or names; shapes are their Visio ids on the page; documents are the
//! opaque handles of [`DocumentInfo::handle`]. Params refuse unknown
//! fields, so a misspelt option is an `InvalidParams` error, not a
//! silent default. Fields may be added within a protocol major version,
//! never removed or retyped.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One operation a remote caller can ask for.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Op {
    /// The server's identity, versions, capabilities and limits.
    Discover,
    /// The registry's commands, filtered and paged (ADR 0077).
    ListCommands(ListCommands),
    /// One command with its argument schema (ADR 0077).
    DescribeCommand(DescribeCommand),
    /// Run one command through the registry (ADR 0077).
    Invoke(Invoke),
    /// The open documents (ADR 0076).
    ListDocuments,
    /// One document's pages and masters (ADR 0078).
    InspectDocument(DocumentQuery),
    /// One page's shapes and connections, filtered and paged (ADR 0078).
    InspectPage(PageQuery),
    /// One shape with its text and, on request, its cells (ADR 0078).
    InspectShape(ShapeQuery),
    /// The selection of one document (ADR 0078).
    InspectSelection(DocumentQuery),
    /// Render a page or a region of it (ADR 0079).
    Render(Render),
}

impl Op {
    /// Every control-protocol method name, in the order of the protocol
    /// document (ADR 0075).
    pub const METHODS: [&'static str; 10] = [
        "server/discover",
        "commands/list",
        "commands/describe",
        "commands/invoke",
        "documents/list",
        "inspect/document",
        "inspect/page",
        "inspect/shape",
        "inspect/selection",
        "render/page",
    ];

    /// The control-protocol method name of this operation (ADR 0075).
    pub fn method(&self) -> &'static str {
        match self {
            Op::Discover => "server/discover",
            Op::ListCommands(_) => "commands/list",
            Op::DescribeCommand(_) => "commands/describe",
            Op::Invoke(_) => "commands/invoke",
            Op::ListDocuments => "documents/list",
            Op::InspectDocument(_) => "inspect/document",
            Op::InspectPage(_) => "inspect/page",
            Op::InspectShape(_) => "inspect/shape",
            Op::InspectSelection(_) => "inspect/selection",
            Op::Render(_) => "render/page",
        }
    }

    /// Whether the operation can change the document or the app state
    /// (only [`Op::Invoke`]; ADR 0081 rate-limits and logs these).
    pub fn mutates(&self) -> bool {
        matches!(self, Op::Invoke(_))
    }
}

/// The result of one [`Op`], one variant per operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpResult {
    /// For [`Op::Discover`].
    Discover(ServerInfo),
    /// For [`Op::ListCommands`].
    Commands(CommandList),
    /// For [`Op::DescribeCommand`].
    Command(CommandInfo),
    /// For [`Op::Invoke`].
    Invoked(Invoked),
    /// For [`Op::ListDocuments`].
    Documents(DocumentList),
    /// For [`Op::InspectDocument`].
    Document(DocumentView),
    /// For [`Op::InspectPage`].
    Page(PageView),
    /// For [`Op::InspectShape`].
    Shape(ShapeView),
    /// For [`Op::InspectSelection`].
    Selection(SelectionView),
    /// For [`Op::Render`].
    Rendered(Rendered),
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// A page: its 1-based index, or its name (ADR 0078).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PageRef {
    /// The 1-based position in the document's page list.
    Index(u32),
    /// The page name (`Page::name`, exact match).
    Name(String),
}

/// A rectangle on a page, in inches, page coordinates (ADR 0079).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Left edge.
    pub x: f64,
    /// Bottom edge.
    pub y: f64,
    /// Width, positive.
    pub width: f64,
    /// Height, positive.
    pub height: f64,
}

// ---------------------------------------------------------------------------
// Params
// ---------------------------------------------------------------------------

/// Params of `commands/list` (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ListCommands {
    /// Keep commands whose id or label contains this text (case
    /// insensitive), or whose id starts with it.
    pub filter: Option<String>,
    /// Include registered stubs (not implemented yet). Default: no.
    pub include_stubs: bool,
    /// Include each command's argument list. Default: no (use
    /// `commands/describe` for one command).
    pub include_args: bool,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Items per page; capped by `Limits::max_page_items`.
    pub limit: Option<usize>,
}

/// Params of `commands/describe` (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeCommand {
    /// The command id (`scope.verb[.qualifier]`, ADR 0006).
    pub command: String,
}

/// Params of `commands/invoke` (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invoke {
    /// The command id.
    pub command: String,
    /// The command's arguments by name, typed by its argument schema.
    #[serde(default)]
    pub args: Map<String, Value>,
    /// Make this document active first (through the registry), else the
    /// active document.
    #[serde(default)]
    pub document: Option<String>,
    /// Make this page active first (through the registry), else the
    /// active page.
    #[serde(default)]
    pub page: Option<PageRef>,
    /// Allow a path argument to replace an existing file (ADR 0081).
    #[serde(default)]
    pub overwrite: bool,
}

/// Params naming a document: `inspect/document`, `inspect/selection`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DocumentQuery {
    /// The document handle, else the active document.
    pub document: Option<String>,
}

/// Which shapes `inspect/page` returns (ADR 0078). Every set field must
/// match; an empty filter keeps every shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShapeFilter {
    /// Only these Visio ids.
    pub ids: Option<Vec<i32>>,
    /// Name contains this text (case insensitive).
    pub name_contains: Option<String>,
    /// Instances of the master with this name (exact).
    pub master: Option<String>,
    /// Text contains this text (case insensitive).
    pub text_contains: Option<String>,
}

/// Params of `inspect/page` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PageQuery {
    /// The document handle, else the active document.
    pub document: Option<String>,
    /// The page, else the active page.
    pub page: Option<PageRef>,
    /// Which shapes to keep.
    pub filter: ShapeFilter,
    /// Walk into groups (children follow their group). Default: top
    /// level only.
    pub recursive: bool,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Items per page; capped by `Limits::max_page_items`.
    pub limit: Option<usize>,
}

/// Params of `inspect/shape` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeQuery {
    /// The document handle, else the active document.
    #[serde(default)]
    pub document: Option<String>,
    /// The page, else the active page.
    #[serde(default)]
    pub page: Option<PageRef>,
    /// The shape's Visio id on that page.
    pub shape: i32,
    /// Which cells to return: section names (`"Geometry1"`,
    /// `"Character"`), `"cells"` for the shape's single cells, or `"all"`.
    /// Default: none.
    #[serde(default)]
    pub cells: Vec<String>,
}

/// The image format of `render/page` (ADR 0079).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderFormat {
    /// PNG raster (the default).
    #[default]
    Png,
    /// SVG document.
    Svg,
}

impl RenderFormat {
    /// The format's name on the wire and its file extension.
    pub fn name(self) -> &'static str {
        match self {
            RenderFormat::Png => "png",
            RenderFormat::Svg => "svg",
        }
    }

    /// The format's media type (MCP image content).
    pub fn mime_type(self) -> &'static str {
        match self {
            RenderFormat::Png => "image/png",
            RenderFormat::Svg => "image/svg+xml",
        }
    }
}

/// Params of `render/page` (ADR 0079).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Render {
    /// The document handle, else the active document.
    pub document: Option<String>,
    /// The page, else the active page.
    pub page: Option<PageRef>,
    /// Only this part of the page, else the whole page.
    pub region: Option<Region>,
    /// Pixels per inch (default 96, ADR 0025); the result must fit the
    /// render limits.
    pub scale: Option<f64>,
    /// PNG or SVG.
    pub format: RenderFormat,
    /// `"none"` for transparent, `"#RRGGBB"` for a colour, else the
    /// page's own background (ADR 0026 spelling).
    pub background: Option<String>,
    /// Write the image to this path (policy-checked, ADR 0081) instead
    /// of returning it inline.
    pub output: Option<String>,
    /// Allow `output` to replace an existing file.
    pub overwrite: bool,
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// What the server can do (part of [`ServerInfo`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Lists and runs registry commands.
    pub commands: bool,
    /// Answers the `inspect/*` methods.
    pub inspect: bool,
    /// The `render/page` formats (`"png"`, `"svg"`).
    pub render_formats: Vec<String>,
}

/// The limits a client should know (part of [`ServerInfo`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsInfo {
    /// `Limits::max_frame_bytes`.
    pub max_frame_bytes: usize,
    /// `Limits::max_page_items`.
    pub max_page_items: usize,
    /// `Limits::max_render_pixels`.
    pub max_render_pixels: u64,
    /// `Limits::max_render_side`.
    pub max_render_side: u32,
}

/// Result of `server/discover` and of `hello` (ADR 0075).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// `"larrygram"`.
    pub name: String,
    /// The Larrygram version (the workspace version).
    pub version: String,
    /// The control-protocol versions spoken, newest first.
    pub protocol_versions: Vec<String>,
    /// True when attached to the running desktop app, false for a
    /// headless session (ADR 0076).
    pub attached: bool,
    /// What the server can do.
    pub capabilities: Capabilities,
    /// The limits a client should respect.
    pub limits: LimitsInfo,
}

/// One argument of a command, as the registry declares it (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArgInfo {
    /// The argument name (the `Args` key).
    pub name: String,
    /// `text`, `path`, `integer`, `number`, `flag`, `list`, `bytes` or
    /// `choice`.
    pub kind: String,
    /// Whether the command fails without it.
    pub required: bool,
    /// One line on what it does (our own words).
    pub description: String,
    /// The allowed values of a `choice`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// For a `path`: `open file`, `save file` or `choose folder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_purpose: Option<String>,
    /// For a number: the smallest allowed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// For a number: the largest allowed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

/// One command (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandInfo {
    /// The command id.
    pub id: String,
    /// The label menus show.
    pub label: String,
    /// False for a registered stub.
    pub implemented: bool,
    /// The enabled predicate's answer in the current state.
    pub enabled: bool,
    /// Why it is disabled, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    /// `allowed`, `headless_only` or `never` (ADR 0081).
    pub remote: String,
    /// The arguments (empty in a `commands/list` without
    /// `include_args`).
    #[serde(default)]
    pub args: Vec<ArgInfo>,
}

/// Result of `commands/list`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandList {
    /// This page of commands, in id order.
    pub commands: Vec<CommandInfo>,
    /// Pass back as `cursor` for the next page; absent on the last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Matching commands over all pages.
    pub total: usize,
}

/// One open document (ADR 0076).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    /// The opaque handle (`"doc-<n>"`), stable while the document is
    /// open in this process.
    pub handle: String,
    /// The display name (file name, or "Drawing1").
    pub name: String,
    /// The file it was opened from or saved to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Unsaved changes.
    pub dirty: bool,
    /// Whether it is the active document.
    pub active: bool,
    /// Number of pages (foreground and background).
    pub page_count: usize,
    /// The active page's 1-based index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_page: Option<u32>,
    /// The label of the edit Undo would revert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    /// The label of the edit Redo would apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redo: Option<String>,
}

/// Result of `commands/invoke` (ADR 0077).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invoked {
    /// The command that ran.
    pub command: String,
    /// The command's message (status-bar text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Warnings the run produced, one line each.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The undo label when the command recorded an edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_label: Option<String>,
    /// A UI request the command made, by name (`about`, `options`,
    /// `confirm_close`, ...): recorded, never acted on for a remote
    /// caller (ADR 0077).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_request: Option<String>,
    /// The active document after the run, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentInfo>,
}

/// Result of `documents/list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentList {
    /// The open documents, in open order.
    pub documents: Vec<DocumentInfo>,
}

/// One page in a document view (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PageSummary {
    /// The 1-based index.
    pub index: u32,
    /// The page name.
    pub name: String,
    /// Page width, inches.
    pub width: f64,
    /// Page height, inches.
    pub height: f64,
    /// Whether it is a background page.
    pub background: bool,
    /// The index of the background page it uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses_background: Option<u32>,
    /// Top-level shapes on the page.
    pub shape_count: usize,
}

/// One master in a document view (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterSummary {
    /// The master name (the `master` value `insert.shape` takes).
    pub name: String,
    /// Instances in the document.
    pub instances: usize,
}

/// Result of `inspect/document` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentView {
    /// The document.
    pub document: DocumentInfo,
    /// `drawing`, `stencil` or `template`.
    pub kind: String,
    /// The pages, in order.
    pub pages: Vec<PageSummary>,
    /// The masters, in document order.
    pub masters: Vec<MasterSummary>,
}

/// One glue connection on a page (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectView {
    /// The connector (or glued shape)'s Visio id.
    pub from_shape: i32,
    /// Its glued cell (`BeginX`, `EndX`, ...).
    pub from_cell: String,
    /// The shape it is glued to.
    pub to_shape: i32,
    /// The target cell (`PinX`, `Connections.X1`, ...).
    pub to_cell: String,
}

/// One shape in a page view (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShapeSummary {
    /// The Visio id on its page.
    pub id: i32,
    /// The shape name.
    pub name: String,
    /// The master's name, for an instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<String>,
    /// `shape`, `group`, `connector` or `foreign`.
    pub kind: String,
    /// The plain text, cut at `Limits::max_summary_text_bytes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Whether `text` was cut.
    #[serde(default)]
    pub text_truncated: bool,
    /// `PinX`, inches.
    pub pin_x: f64,
    /// `PinY`, inches.
    pub pin_y: f64,
    /// `Width`, inches.
    pub width: f64,
    /// `Height`, inches.
    pub height: f64,
    /// `Angle`, degrees counter-clockwise.
    pub angle: f64,
    /// The group it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<i32>,
    /// Its members, for a group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<i32>,
}

/// Result of `inspect/page` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PageView {
    /// The document handle.
    pub document: String,
    /// The page.
    pub page: PageSummary,
    /// This page of matching shapes, in z-order (back to front).
    pub shapes: Vec<ShapeSummary>,
    /// Pass back as `cursor` for the next page; absent on the last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Matching shapes over all pages.
    pub total: usize,
    /// The page's glue connections touching the returned shapes, capped
    /// at `Limits::max_page_items`.
    pub connects: Vec<ConnectView>,
    /// Whether `connects` was cut.
    #[serde(default)]
    pub connects_truncated: bool,
}

/// One ShapeSheet cell (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellView {
    /// The section (`Geometry1`, `Character`, ...), absent for a single
    /// cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The 0-based row in the section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<u32>,
    /// The cell name.
    pub name: String,
    /// The formula, as stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The value, as Visio would display it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The value as a number in internal units, when numeric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<f64>,
    /// The unit of the stored value (`IN`, `DEG`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// Result of `inspect/shape` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShapeView {
    /// The document handle.
    pub document: String,
    /// The page's 1-based index.
    pub page: u32,
    /// The shape's summary.
    pub shape: ShapeSummary,
    /// The full text, cut at `Limits::max_arg_bytes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The requested cells, in ShapeSheet order.
    #[serde(default)]
    pub cells: Vec<CellView>,
}

/// Result of `inspect/selection` (ADR 0078).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionView {
    /// The document handle.
    pub document: String,
    /// The page the selection is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// The selected shapes' Visio ids, in selection order.
    pub shapes: Vec<i32>,
    /// The primary (first selected) shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<i32>,
}

/// Result of `render/page` (ADR 0079).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
    /// `png` or `svg`.
    pub format: String,
    /// Image width, pixels (SVG: the viewport at the scale).
    pub width: u32,
    /// Image height, pixels.
    pub height: u32,
    /// The image bytes in standard base64, when returned inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    /// The file written, when `output` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The image size in bytes (before base64).
    pub bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_names_a_listed_method() {
        let ops = [
            Op::Discover,
            Op::ListCommands(ListCommands::default()),
            Op::DescribeCommand(DescribeCommand::default()),
            Op::Invoke(Invoke::default()),
            Op::ListDocuments,
            Op::InspectDocument(DocumentQuery::default()),
            Op::InspectPage(PageQuery::default()),
            Op::InspectShape(ShapeQuery::default()),
            Op::InspectSelection(DocumentQuery::default()),
            Op::Render(Render::default()),
        ];
        let names: Vec<&str> = ops.iter().map(Op::method).collect();
        assert_eq!(names, Op::METHODS);
        assert_eq!(ops.iter().filter(|op| op.mutates()).count(), 1);
    }

    #[test]
    fn params_refuse_unknown_fields_and_fill_defaults() {
        let invoke: Invoke =
            serde_json::from_value(serde_json::json!({ "command": "page.add" })).unwrap();
        assert_eq!(invoke.command, "page.add");
        assert!(invoke.args.is_empty() && !invoke.overwrite);
        assert!(
            serde_json::from_value::<Invoke>(serde_json::json!({
                "command": "page.add", "overwite": true
            }))
            .is_err()
        );
        let query: PageQuery = serde_json::from_value(serde_json::json!({
            "page": 2, "filter": { "master": "Process" }
        }))
        .unwrap();
        assert_eq!(query.page, Some(PageRef::Index(2)));
        assert_eq!(query.filter.master.as_deref(), Some("Process"));
        let named: PageRef = serde_json::from_value(serde_json::json!("Page-2")).unwrap();
        assert_eq!(named, PageRef::Name("Page-2".into()));
        let render: Render = serde_json::from_value(serde_json::json!({
            "format": "svg", "region": { "x": 0, "y": 0, "width": 1, "height": 2 }
        }))
        .unwrap();
        assert_eq!(render.format, RenderFormat::Svg);
        assert_eq!(render.region.map(|r| r.height), Some(2.0));
        assert!(serde_json::from_value::<ShapeQuery>(serde_json::json!({})).is_err());
    }

    #[test]
    fn render_formats_name_their_media_types() {
        assert_eq!(RenderFormat::default(), RenderFormat::Png);
        assert_eq!(RenderFormat::Png.name(), "png");
        assert_eq!(RenderFormat::Svg.name(), "svg");
        assert_eq!(RenderFormat::Png.mime_type(), "image/png");
        assert_eq!(RenderFormat::Svg.mime_type(), "image/svg+xml");
    }

    #[test]
    fn results_serialize_snake_case_and_skip_empty_options() {
        let info = DocumentInfo {
            handle: "doc-1".into(),
            name: "Drawing1".into(),
            page_count: 1,
            active: true,
            ..DocumentInfo::default()
        };
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["page_count"], 1);
        assert!(value.get("path").is_none());
        let rendered = Rendered {
            format: "png".into(),
            width: 2,
            height: 3,
            bytes: 10,
            ..Rendered::default()
        };
        let value = serde_json::to_value(&rendered).unwrap();
        assert!(value.get("data_base64").is_none());
        assert_eq!(value["width"], 2);
    }
}
