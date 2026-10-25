//! The Phase 5 exit criterion (task P5-10, ADR 0048, ADR 0049): a user
//! can build a 10-shape flowchart with connected shapes across two
//! pages, save it, reopen it and keep editing.
//!
//! One scripted test through `Shell` + the builtin registry with
//! [`Source::Test`]: `file.new` (which opens its one page), a rename,
//! ten `insert.shape` dispatches
//! (a process/decision/terminator mix from the "Basic Flowchart"
//! stencil), five glued connectors (three `connector.draw`, two
//! `insert.shape` + `connector.glue_begin`/`connector.glue_end`, so
//! both connector forms run end to end), `page.add` and four more
//! shapes on page two, `file.save` to a temp path, `file.open` the
//! same path, the model equality below, then `shape.move` a shape and
//! `file.save` again.
//!
//! # The ADR 0048 model equality
//!
//! ADR 0048 defines its equality over `model::Document` values,
//! ignoring the volatile document properties, `Preserved` attribute
//! order and `AppCtx` state. One thing the model *can* differ on is
//! not content and is ignored here as well (the documented deviation,
//! see the P5-10 Result):
//!
//! - **arena addressing**: `Document::shapes` is an id-addressed arena
//!   and the writer/reader renumber it (the reader fills masters'
//!   shapes first, a model built in memory interleaves them with
//!   instances), so [`canonical`] renames every `ShapeId` into
//!   document order before comparing.
//!
//! The two reader artefacts earlier versions of this comparison also
//! dropped are gone (task P6-16): a read `Page`/`Master` no longer
//! keeps the `<Rel>` child that names its contents part (the writer
//! writes it again) and a cell keeps its `U` spelling only when the
//! writer cannot re-derive it, so the comparison drops neither.
//!
//! The ADR's own identity (`read(write(read(write(d)))) ==
//! read(write(d))`) is asserted literally as well: the reopened
//! document is saved and reopened a second time and the two compares
//! equal (plus the writer's determinism: two saves of one document
//! write the same bytes).

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::path::PathBuf;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Outcome, Source};
use larrygram_model::{
    Cell, CellName, CellValue, Document, PageId, Preserved, QName, Shape, ShapeId, Sheet,
    XmlAttribute, XmlChild, XmlNode,
};
use larrygram_ui::Intent;
use larrygram_ui::dialogs::{FileChoice, FilePicker, OpenRequest, PickState, SaveRequest};
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::Loader;

// ---------------------------------------------------------------------------
// Dispatch plumbing
// ---------------------------------------------------------------------------

/// A picker that must never be called: every dispatch carries its
/// arguments (AGENTS.md section 6: a command never opens a dialog).
struct NoDialogPicker;

impl FilePicker for NoDialogPicker {
    fn open(&self, _request: &OpenRequest) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        panic!("the exit-criterion test must never open a dialog")
    }

    fn save(
        &self,
        _request: &SaveRequest,
        _data: &[u8],
    ) -> Result<Option<FileChoice>, larrygram_ui::UiError> {
        panic!("the exit-criterion test must never open a dialog")
    }

    fn poll(&self) -> PickState {
        PickState::Idle
    }
}

/// The shell the test drives: the builtin registry and the committed
/// layouts, the way the app builds them.
fn shell() -> Shell {
    let registry = larrygram_commands::builtins::registry().expect("the registry builds");
    let layouts = Loader::from_embedded().expect("the embedded ui-spec loads");
    let mut shell = Shell::new(registry, layouts, AppCtx::new(Settings::default()));
    shell.set_picker(Box::new(NoDialogPicker));
    shell
}

/// One dispatch through the shell with [`Source::Test`]; panics on any
/// failure and returns the outcome.
fn run(shell: &mut Shell, command: &'static str, args: Args) -> Outcome {
    let id = CommandId::parse(command).expect("a valid command id");
    let intent = Intent::new(id, Invocation::new(args, Source::Test));
    let mut results = shell.dispatch(&[intent]).expect("the intent dispatches");
    results
        .pop()
        .expect("one result per intent")
        .unwrap_or_else(|error| panic!("{command} failed: {error}"))
}

/// A scratch directory for the saved files (removed on drop).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "larrygram-flowchart-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory is created");
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Building the flowchart
// ---------------------------------------------------------------------------

/// The stencil the flowchart masters come from.
const STENCIL: &str = "Basic Flowchart";

/// The selection anchor's Visio id (`insert.shape` selects the new
/// instance; its id is never assumed).
fn anchor_visio_id(shell: &Shell) -> i32 {
    let session = shell.app.active_document().expect("an open document");
    let anchor = shell
        .app
        .selection
        .anchor
        .expect("the new shape is selected");
    session
        .document
        .shape(anchor)
        .expect("the anchor shape exists")
        .visio_id
}

/// Inserts one catalog master at (`x`, `y`) and answers its Visio id.
fn insert_shape(shell: &mut Shell, master: &str, x: f64, y: f64) -> i32 {
    let args = Args::new()
        .with("stencil", ArgValue::Text(STENCIL.to_owned()))
        .with("master", ArgValue::Text(master.to_owned()))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    run(shell, "insert.shape", args);
    anchor_visio_id(shell)
}

/// One connection-row `IX` of `target` (`index` into the file-order
/// point list, `None` = the last point): the glue `point` argument's
/// contract (P5-17).
fn point_ix(shell: &Shell, target: i32, index: Option<usize>) -> i32 {
    let session = shell.app.active_document().expect("an open document");
    let page = session.active_page.expect("an active page");
    let shape = session
        .document
        .shape_by_visio_id(page, target)
        .expect("the glue target is on the page");
    let points = session.document.connection_points_indexed(shape);
    let position = index.unwrap_or(points.len().saturating_sub(1));
    points
        .get(position)
        .map(|(ix, _)| *ix)
        .expect("the target has connection points")
}

/// One `connector.draw`: inserts the "Connector" master, glues the
/// given ends (a `None` point is outline glue) and routes it in one
/// undo step (P5-17). Answers the connector's Visio id.
fn draw_connector(
    shell: &mut Shell,
    x: f64,
    y: f64,
    begin: Option<(i32, Option<i32>)>,
    end: Option<(i32, Option<i32>)>,
) -> i32 {
    let mut args = Args::new()
        .with("stencil", ArgValue::Text(STENCIL.to_owned()))
        .with("master", ArgValue::Text("Connector".to_owned()))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    if let Some((target, point)) = begin {
        args.set("begin_target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("begin_point", ArgValue::Integer(i64::from(ix)));
        }
    }
    if let Some((target, point)) = end {
        args.set("end_target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("end_point", ArgValue::Integer(i64::from(ix)));
        }
    }
    run(shell, "connector.draw", args);
    anchor_visio_id(shell)
}

/// The second connector form (P5-17): `insert.shape` plus one
/// `connector.glue_begin`/`connector.glue_end` per end.
fn glued_connector(
    shell: &mut Shell,
    x: f64,
    y: f64,
    begin: (i32, Option<i32>),
    end: (i32, Option<i32>),
) -> i32 {
    let connector = insert_shape(shell, "Connector", x, y);
    for (command, (target, point)) in [("connector.glue_begin", begin), ("connector.glue_end", end)]
    {
        let mut args = Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("point", ArgValue::Integer(i64::from(ix)));
        }
        run(shell, command, args);
    }
    connector
}

/// One flowchart shape (Visio id + master `NameU`) for the later
/// assertions.
#[derive(Debug, Clone, Copy)]
struct ShapeRef {
    /// The page's list position.
    page: usize,
    /// The shape's Visio `ID`.
    visio_id: i32,
    /// The master's `NameU`.
    master: &'static str,
}

/// One connector (Visio id + its two glue targets' Visio ids).
#[derive(Debug, Clone, Copy)]
struct ConnectorRef {
    /// The page's list position.
    page: usize,
    /// The connector's Visio `ID`.
    visio_id: i32,
    /// The glued begin/end target Visio ids.
    ends: (i32, i32),
}

/// What the build step created.
struct Flowchart {
    shapes: Vec<ShapeRef>,
    connectors: Vec<ConnectorRef>,
}

/// Builds the 10-shape, two-page, five-connector flowchart.
///
/// Page one: Terminator -> Process -> Decision -> Process, the
/// Decision's second exit and the final Terminator beside the flow;
/// page two: Process -> Decision -> Process and a Terminator. Three
/// connectors are drawn with `connector.draw` (one outline-glued, two
/// connection-point-glued) and two with `insert.shape` +
/// `connector.glue_begin`/`connector.glue_end`.
fn build_flowchart(shell: &mut Shell) -> Flowchart {
    run(shell, "file.new", Args::new());
    // Task P7-20: `file.new` opens a blank drawing with one page, so
    // the flow page is that page renamed (it was `page.add` before).
    run(
        shell,
        "page.rename",
        Args::new().with("name", ArgValue::Text("Flow".to_owned())),
    );

    // Page one: six shapes.
    let mut shapes = Vec::new();
    for (master, x, y) in [
        ("Terminator", 2.0, 9.0),
        ("Process", 2.0, 7.0),
        ("Decision", 2.0, 5.0),
        ("Process", 5.5, 5.0),
        ("Process", 2.0, 3.0),
        ("Terminator", 2.0, 1.0),
    ] {
        let visio_id = insert_shape(shell, master, x, y);
        shapes.push(ShapeRef {
            page: 0,
            visio_id,
            master,
        });
    }

    // Page one: three connectors. The first two run through
    // `connector.draw` (form 1: insert, glue and route in one step),
    // the third through `insert.shape` + `connector.glue_begin`/
    // `connector.glue_end` (form 2). The second and third use
    // connection-point glue (`point` = the row `IX`).
    let mut connectors = Vec::new();
    let first = draw_connector(
        shell,
        4.0,
        8.0,
        Some((shapes[0].visio_id, None)),
        Some((shapes[1].visio_id, None)),
    );
    connectors.push(ConnectorRef {
        page: 0,
        visio_id: first,
        ends: (shapes[0].visio_id, shapes[1].visio_id),
    });

    let begin_point = point_ix(shell, shapes[1].visio_id, None);
    let end_point = point_ix(shell, shapes[2].visio_id, Some(0));
    let second = draw_connector(
        shell,
        4.0,
        6.0,
        Some((shapes[1].visio_id, Some(begin_point))),
        Some((shapes[2].visio_id, Some(end_point))),
    );
    connectors.push(ConnectorRef {
        page: 0,
        visio_id: second,
        ends: (shapes[1].visio_id, shapes[2].visio_id),
    });

    let third = glued_connector(
        shell,
        4.0,
        4.0,
        (
            shapes[2].visio_id,
            Some(point_ix(shell, shapes[2].visio_id, None)),
        ),
        (
            shapes[3].visio_id,
            Some(point_ix(shell, shapes[3].visio_id, None)),
        ),
    );
    connectors.push(ConnectorRef {
        page: 0,
        visio_id: third,
        ends: (shapes[2].visio_id, shapes[3].visio_id),
    });

    // Page two: four shapes and two connectors.
    run(
        shell,
        "page.add",
        Args::new().with("name", ArgValue::Text("Detail".to_owned())),
    );
    for (master, x, y) in [
        ("Process", 2.0, 7.0),
        ("Decision", 2.0, 5.0),
        ("Process", 5.5, 5.0),
        ("Terminator", 2.0, 3.0),
    ] {
        let visio_id = insert_shape(shell, master, x, y);
        shapes.push(ShapeRef {
            page: 1,
            visio_id,
            master,
        });
    }

    let fourth = draw_connector(
        shell,
        4.0,
        6.0,
        Some((
            shapes[6].visio_id,
            Some(point_ix(shell, shapes[6].visio_id, Some(0))),
        )),
        Some((
            shapes[7].visio_id,
            Some(point_ix(shell, shapes[7].visio_id, None)),
        )),
    );
    connectors.push(ConnectorRef {
        page: 1,
        visio_id: fourth,
        ends: (shapes[6].visio_id, shapes[7].visio_id),
    });
    let fifth = glued_connector(
        shell,
        4.0,
        4.0,
        (shapes[7].visio_id, None),
        (shapes[8].visio_id, None),
    );
    connectors.push(ConnectorRef {
        page: 1,
        visio_id: fifth,
        ends: (shapes[7].visio_id, shapes[8].visio_id),
    });

    Flowchart { shapes, connectors }
}

// ---------------------------------------------------------------------------
// The reopened document: content and model equality
// ---------------------------------------------------------------------------

/// Asserts the reopened document holds the built flowchart: the two
/// pages, all ten shapes under their masters, and the five glued
/// connectors with their glue topology (the exit criterion's "reopen
/// it" means the drawing is still there).
fn assert_content(document: &Document, flowchart: &Flowchart) {
    assert_eq!(
        document.pages.len(),
        2,
        "the reopened document has two pages"
    );
    assert_eq!(document.pages[0].name, "Flow");
    assert_eq!(document.pages[1].name, "Detail");

    for shape in &flowchart.shapes {
        let page = PageId {
            index: shape.page as u32,
        };
        let id = document
            .shape_by_visio_id(page, shape.visio_id)
            .unwrap_or_else(|| {
                panic!(
                    "shape {} (Visio id {}) is missing from page {}",
                    shape.master, shape.visio_id, shape.page
                )
            });
        let entry = document.shape(id).expect("the shape exists");
        let master = entry.master.expect("a master instance");
        let master = document.master(master).expect("the master exists");
        assert_eq!(master.name_u, shape.master, "the instance's master");
        let entity = document.page(page).expect("the page exists");
        assert!(
            entity.shapes.contains(&id),
            "shape {} is a top-level shape of page {}",
            shape.visio_id,
            shape.page
        );
    }

    for connector in &flowchart.connectors {
        let page = PageId {
            index: connector.page as u32,
        };
        let id = document
            .shape_by_visio_id(page, connector.visio_id)
            .unwrap_or_else(|| {
                panic!(
                    "connector {} is missing from page {}",
                    connector.visio_id, connector.page
                )
            });
        let ends = document.connects_of(page, id).expect("the page exists");
        let begin = ends[0]
            .as_ref()
            .unwrap_or_else(|| panic!("connector {} has no begin glue", connector.visio_id));
        let end = ends[1]
            .as_ref()
            .unwrap_or_else(|| panic!("connector {} has no end glue", connector.visio_id));
        assert_eq!(
            (begin.to_sheet, end.to_sheet),
            connector.ends,
            "connector {} is glued to the flow",
            connector.visio_id
        );
        // The routed path survived (at least start and end).
        let geometry = document
            .shape(id)
            .expect("the connector exists")
            .sheet
            .sections
            .iter()
            .find(|section| section.name.as_str() == "Geometry")
            .unwrap_or_else(|| panic!("connector {} has no path", connector.visio_id));
        assert!(
            geometry.rows.len() >= 2,
            "connector {} has a routed path",
            connector.visio_id
        );
    }

    // The ten flowchart shapes plus five connectors, top-level.
    assert_eq!(document.pages[0].shapes.len(), 9, "page one shape count");
    assert_eq!(document.pages[1].shapes.len(), 6, "page two shape count");
}

/// Sorts the attributes of every preserved element (attribute order is
/// not semantic, ADR 0012).
fn sort_node(node: &mut XmlNode) {
    node.attributes
        .sort_by(|a, b| (&a.name.uri, &a.name.local).cmp(&(&b.name.uri, &b.name.local)));
    for child in &mut node.children {
        if let XmlChild::Element(element) = child {
            sort_node(element);
        }
    }
}

/// Sorts the preserved fragment (attribute order is not semantic, ADR
/// 0012).
fn sort_preserved(preserved: &mut Preserved) {
    preserved
        .attributes
        .sort_by(|a, b| (&a.name.uri, &a.name.local).cmp(&(&b.name.uri, &b.name.local)));
    for child in &mut preserved.children {
        sort_node(child);
    }
}

/// Sorts the preserved fragments of one sheet.
fn sort_sheet(sheet: &mut Sheet) {
    for cell in &mut sheet.cells {
        sort_preserved(&mut cell.preserved);
    }
    for section in &mut sheet.sections {
        sort_preserved(&mut section.preserved);
        for cell in &mut section.cells {
            sort_preserved(&mut cell.preserved);
        }
        for row in &mut section.rows {
            sort_preserved(&mut row.preserved);
            for cell in &mut row.cells {
                sort_preserved(&mut cell.preserved);
            }
        }
    }
}

/// Normalises a document for the ADR 0048 model comparison: `Preserved`
/// attribute order is not semantic and the volatile document properties are
/// not compared.
fn normalised(document: &Document) -> Document {
    let mut document = document.clone();
    sort_preserved(&mut document.preserved);
    for style in &mut document.stylesheet.styles {
        sort_preserved(&mut style.preserved);
        sort_sheet(&mut style.sheet);
    }
    for page in &mut document.pages {
        sort_preserved(&mut page.preserved);
        sort_sheet(&mut page.sheet);
        for connect in &mut page.connects {
            sort_preserved(&mut connect.preserved);
        }
    }
    for master in &mut document.masters {
        sort_preserved(&mut master.preserved);
    }
    for shape in &mut document.shapes {
        sort_preserved(&mut shape.preserved);
        sort_sheet(&mut shape.sheet);
    }
    document.properties.retain(|property| {
        !matches!(
            property.name.as_str(),
            "modified" | "lastModifiedBy" | "revision" | "lastPrinted"
        )
    });
    document
}

/// Renames every arena [`ShapeId`] into document order (masters first,
/// then pages, each shape tree in wrapper order; unreachable arena
/// entries keep their relative order at the tail), so two documents
/// that differ only in arena addressing compare equal (module docs).
fn canonical(document: &Document) -> Document {
    let mut document = document.clone();
    let mut order: Vec<u32> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    for master in &document.masters {
        for &id in &master.shapes.clone() {
            walk(&document, id, &mut order, &mut seen);
        }
    }
    for page in &document.pages {
        for &id in &page.shapes.clone() {
            walk(&document, id, &mut order, &mut seen);
        }
    }
    for (index, _) in document.shapes.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        if !seen.contains(&index) {
            order.push(index);
        }
    }
    let map: HashMap<u32, u32> = order
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, u32::try_from(new).unwrap_or(u32::MAX)))
        .collect();
    let rename = |id: ShapeId| ShapeId {
        index: *map.get(&id.index).unwrap_or(&id.index),
    };
    for shape in &mut document.shapes {
        shape.id = rename(shape.id);
        shape.parent = shape.parent.map(rename);
        shape.children = shape.children.iter().copied().map(rename).collect();
    }
    let mut slots: Vec<Option<Shape>> = document.shapes.iter().map(|_| None).collect();
    for shape in document.shapes.drain(..) {
        if let Some(slot) = slots.get_mut(shape.id.index as usize) {
            *slot = Some(shape);
        }
    }
    document.shapes = slots.into_iter().flatten().collect();
    for page in &mut document.pages {
        page.shapes = page.shapes.iter().copied().map(rename).collect();
    }
    for master in &mut document.masters {
        master.shapes = master.shapes.iter().copied().map(rename).collect();
    }
    document
}

/// The reachable shape ids of one tree, in wrapper order (a cyclic
/// child list stops at the first repeat).
fn walk(document: &Document, id: ShapeId, order: &mut Vec<u32>, seen: &mut Vec<u32>) {
    if seen.contains(&id.index) {
        return;
    }
    seen.push(id.index);
    order.push(id.index);
    let children = document.shape(id).map(|shape| shape.children.clone());
    if let Some(children) = children {
        for child in children {
            walk(document, child, order, seen);
        }
    }
}

/// The model comparison of the ADR 0048 equality (module docs).
fn model_equal(a: &Document, b: &Document) -> bool {
    canonical(&normalised(a)) == canonical(&normalised(b))
}

// ---------------------------------------------------------------------------
// Keep editing: the moved shape's connectors follow it
// ---------------------------------------------------------------------------

/// The raw first/last `Geometry` row point of `shape` (the routed
/// path's begin/end as the model stores them).
fn path_point(document: &Document, shape: ShapeId, end: bool) -> (f64, f64) {
    let entry = document.shape(shape).expect("the connector exists");
    let section = entry
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Geometry")
        .expect("the connector has a path");
    let row = if end {
        section.rows.last().expect("a routed row")
    } else {
        section.rows.first().expect("a routed row")
    };
    let value = |name: &str| {
        row.cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.clone())
            .and_then(|value| match value {
                CellValue::Number(number) => Some(number),
                _ => None,
            })
            .expect("a numeric geometry cell")
    };
    (value("X"), value("Y"))
}

// ---------------------------------------------------------------------------
// The exit criterion
// ---------------------------------------------------------------------------

#[test]
fn the_flowchart_exit_criterion() {
    let temp = TempDir::new("exit");
    let saved_path = temp.file("flowchart.vsdx");
    let reopened_again_path = temp.file("flowchart-2.vsdx");
    let kept_editing_path = temp.file("flowchart-3.vsdx");

    let mut shell = shell();
    let flowchart = build_flowchart(&mut shell);

    // Ten shapes and five connectors were built.
    assert_eq!(flowchart.shapes.len(), 10);
    assert_eq!(flowchart.connectors.len(), 5);

    // Save.
    run(
        &mut shell,
        "file.save",
        Args::new().with("path", ArgValue::Path(saved_path.clone())),
    );
    assert!(
        std::fs::metadata(&saved_path)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false),
        "the save wrote a non-empty file"
    );
    let saved = shell
        .app
        .active_document()
        .expect("the built document")
        .document
        .clone();

    // Reopen the same path and check the drawing came back.
    run(
        &mut shell,
        "file.open",
        Args::new().with("path", ArgValue::Path(saved_path.clone())),
    );
    let reopened = shell
        .app
        .active_document()
        .expect("the reopened document")
        .document
        .clone();
    assert_content(&reopened, &flowchart);

    // The ADR 0048 model equality of the saved and reopened documents
    // (module docs).
    assert!(
        model_equal(&saved, &reopened),
        "the saved and reopened documents differ"
    );

    // The ADR 0048 identity literally: a re-opened document keeps its
    // model, and the writer is deterministic.
    run(
        &mut shell,
        "file.save",
        Args::new().with("path", ArgValue::Path(reopened_again_path.clone())),
    );
    run(
        &mut shell,
        "file.open",
        Args::new().with("path", ArgValue::Path(reopened_again_path.clone())),
    );
    let reopened_again = shell
        .app
        .active_document()
        .expect("the twice-reopened document")
        .document
        .clone();
    assert!(
        model_equal(&reopened, &reopened_again),
        "a re-opened document changes on a second save and open"
    );
    let first_bytes = std::fs::read(&saved_path).expect("the first save");
    let second_bytes = std::fs::read(&reopened_again_path).expect("the second save");
    assert_eq!(
        first_bytes, second_bytes,
        "a second save writes the same bytes as the first"
    );

    // Keep editing: move the decision shape on page one (the active
    // page) and watch its glued connectors follow it (ADR 0045).
    let moved = flowchart.shapes[2]; // the Decision
    let session = shell.app.active_document().expect("the reopened document");
    let page = session.active_page.expect("an active page");
    let moved_id = session
        .document
        .shape_by_visio_id(page, moved.visio_id)
        .expect("the moved shape is on the active page");
    assert_eq!(
        session
            .document
            .shape(moved_id)
            .expect("the shape")
            .visio_id,
        moved.visio_id
    );
    let end_of_second = session
        .document
        .shape_by_visio_id(page, flowchart.connectors[1].visio_id)
        .expect("the second connector");
    let begin_of_third = session
        .document
        .shape_by_visio_id(page, flowchart.connectors[2].visio_id)
        .expect("the third connector");
    let end_before = path_point(&session.document, end_of_second, true);
    let begin_before = path_point(&session.document, begin_of_third, false);

    run(
        &mut shell,
        "edit.select",
        Args::new().with("shapes", ArgValue::List(vec![moved.visio_id.to_string()])),
    );
    let changes_before = shell
        .app
        .active_document()
        .expect("the reopened document")
        .changes;
    run(
        &mut shell,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(1.0))
            .with("dy", ArgValue::Number(-0.5)),
    );
    let session = shell.app.active_document().expect("the reopened document");
    assert!(
        session.changes > changes_before,
        "the move bumps the change counter"
    );

    // The connectors still end on the moved shape: their glue records
    // name it and their routed endpoints followed it by the move.
    let ends = session
        .document
        .connects_of(page, end_of_second)
        .expect("the page exists");
    assert_eq!(
        ends[1].as_ref().map(|connect| connect.to_sheet),
        Some(moved.visio_id),
        "the second connector still ends on the moved shape"
    );
    let ends = session
        .document
        .connects_of(page, begin_of_third)
        .expect("the page exists");
    assert_eq!(
        ends[0].as_ref().map(|connect| connect.to_sheet),
        Some(moved.visio_id),
        "the third connector still begins on the moved shape"
    );
    let end_after = path_point(&session.document, end_of_second, true);
    let begin_after = path_point(&session.document, begin_of_third, false);
    for (label, before, after) in [
        ("end", end_before, end_after),
        ("begin", begin_before, begin_after),
    ] {
        assert!(
            (after.0 - before.0 - 1.0).abs() < 1e-6 && (after.1 - before.1 + 0.5).abs() < 1e-6,
            "the connector {label} moved with the shape: {before:?} -> {after:?}"
        );
    }

    // And the edited document saves again.
    run(
        &mut shell,
        "file.save",
        Args::new().with("path", ArgValue::Path(kept_editing_path.clone())),
    );
    assert!(
        std::fs::metadata(&kept_editing_path)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false),
        "the keep-editing save wrote a non-empty file"
    );
}

// ---------------------------------------------------------------------------
// The comparison helpers' own tests (run without building a document)
// ---------------------------------------------------------------------------

/// One `Box` master instance at the next Visio id (the helper test's
/// shape builder).
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
fn box_shape(document: &mut Document, page: PageId, name: &str) -> ShapeId {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    shape.name_u = name.to_owned();
    document.insert_shape(page, shape, None).expect("shape")
}

/// Two documents that differ only in arena order compare equal; a
/// content difference does not.
#[test]
fn the_model_comparison_ignores_addressing() {
    // Base: one page, Alpha then Beta in the arena and on the page.
    let mut base = Document::new();
    let page = base.add_page("Page-1").expect("page");
    box_shape(&mut base, page, "Alpha");
    box_shape(&mut base, page, "Beta");

    // Same content with the arena entries swapped (and every id
    // reference rewritten): addressing only.
    let mut renumbered = base.clone();
    renumbered.shapes.swap(0, 1);
    renumbered.shapes[0].id = ShapeId { index: 0 };
    renumbered.shapes[1].id = ShapeId { index: 1 };
    renumbered.pages[0].shapes = vec![ShapeId { index: 1 }, ShapeId { index: 0 }];
    assert!(model_equal(&base, &renumbered));

    // A real difference is one. A preserved `Rel` child or a cell `U`
    // spelling is a real difference too now: the reader produces
    // neither (task P6-16).
    let mut changed = base.clone();
    changed.pages[0].name = "Other".to_owned();
    assert!(!model_equal(&base, &changed));
    let mut changed = base.clone();
    changed.shapes[0].name_u = "Gamma".to_owned();
    assert!(!model_equal(&base, &changed));
    let mut plumbed = base.clone();
    plumbed.pages[0].preserved.children.push(XmlNode {
        name: QName::local("Rel"),
        attributes: vec![XmlAttribute {
            name: QName::local("id"),
            value: "rId1".to_owned(),
        }],
        children: Vec::new(),
        self_closing: true,
    });
    assert!(!model_equal(&base, &plumbed));
    let mut spelled = base.clone();
    let mut cell = Cell::new(CellName::from_static("FlipX"));
    cell.value = Some(CellValue::Bool(false));
    cell.preserved.attributes.push(XmlAttribute {
        name: QName::local("U"),
        value: "BOOL".to_owned(),
    });
    spelled.shapes[0].sheet.cells.push(cell);
    assert!(!model_equal(&base, &spelled));
}
