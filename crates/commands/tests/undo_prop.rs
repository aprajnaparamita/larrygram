//! Undo is the inverse of every editing command (task P5-11, ADR 0040
//! rule 4, ADR 0049 rule 2).
//!
//! Two layers:
//!
//! - a **hand-written pair test per command** (`every_case_inverts_on_the_seed`):
//!   run the command over the seeded document, undo, compare with the
//!   seed, undo + redo, compare with the post-command state. The
//!   assertion names the command;
//! - a **`proptest` command-sequence property**
//!   (`undo_is_the_inverse_of_random_command_sequences`): random
//!   sequences of editing commands over the same seed; after undoing
//!   every pushed step the state equals the seed, and redoing the
//!   sequence reproduces the state after each step.
//!
//! Both compare the **reachable** state (task P5-11 note), not
//! `Document` equality: the pages, the shapes reachable from them,
//! their sheets, text and `Connect` records, and the masters their
//! instances use. Group, insert and delete undo leave detached arena
//! entries behind by design, and the first drop of a catalog master
//! leaves the materialised master in the document (P5-06): both are
//! known differences and outside the reachable state. They are pinned
//! as observable in `known_undo_differences_stay_outside_the_reachable_state`.
//!
//! The comparison ignores the change counter, the undo stack and
//! `AppCtx::warnings` (they are not part of the document), and it
//! normalises writer-managed spellings the way
//! `apps/larrygram/tests/flowchart.rs` does (`Preserved` attribute
//! order, preserved `Rel` children, a cell's `U` kind spelling, the
//! volatile document properties). Numbers fold to 12 significant
//! digits ([`rounded`]): a re-route pass re-derives a **free**
//! connector end through the shape's transform, and the f64 round trip
//! drifts about one ulp per pass (ADR 0045 keeps a free point "until
//! the connector itself moves" within rounding), so exact bit equality
//! of routed paths is not achievable. Everything else — structure,
//! ids, text, formulas that are not number literals, `Connect`
//! records — compares exactly.
//!
//! Re-routing after an edit is not undoable on its own (ADR 0045 rule
//! 3), so the comparison is of the routed paths themselves: the seed's
//! connector paths are produced by `connector.reroute_all`, so a
//! re-route on undo or redo recomputes exactly the compared geometry.
//! Selection and tool choice are not edits (`edit.select`, `tools.*`:
//! P5-16, P5-17) and never enter the compared state.
//!
//! A panic is the bug (AGENTS.md section 6).

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use std::collections::{BTreeSet, HashMap};

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandId, CommandStatus, Invocation, Registry, Source};
use larrygram_model::{
    Cell, CellName, CellValue, Document, MasterId, PageId, Preserved, Row, Section, SectionName,
    Shape, ShapeId, Sheet, XmlChild, XmlNode,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// The seeded document
// ---------------------------------------------------------------------------

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A 2 x 1 box at `(x, y)` with a rectangle path and two connection
/// points (top and bottom edge midpoints).
fn box_shape(x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", 2.0, 0.0),
        ("LineTo", 2.0, 1.0),
        ("LineTo", 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    // One Shape Data row (`Prop.Cost.Value`), so `data.set.property`
    // has a subject (task P6-18).
    let mut property = Section::new(SectionName::from_static("Property"));
    let mut cost = Row::new();
    cost.name = Some(String::from("Cost"));
    cost.ix = Some(0);
    cost.cells = vec![cell("Value", 1.0)];
    property.rows.push(cost);
    shape.sheet.sections.push(property);
    let mut connection = Section::new(SectionName::from_static("Connection"));
    for (ix, (px, py)) in [(1.0, 1.0), (1.0, 0.0)].into_iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        connection.rows.push(row);
    }
    shape.sheet.sections.push(connection);
    shape
}

/// A two-point connector at `(x, y)`; the transform cells are all the
/// path writer needs.
fn connector_shape(x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [("MoveTo", 0.0, 0.5), ("LineTo", 2.0, 0.5)]
        .into_iter()
        .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

/// The seed: two pages, five boxes, two connectors and one group of two
/// shapes on page one, two boxes on page two, with both glue kinds on
/// the connectors and every connector path routed by
/// `connector.reroute_all` (so the seed's paths are what a re-route
/// computes). The setup commands' undo entries are cleared: the
/// property starts from a clean stack.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let first = doc.add_page("Page-1").expect("page one");
    let second = doc.add_page("Page-2").expect("page two");
    for page in [first, second] {
        if let Some(entry) = doc.pages.get_mut(page.index as usize) {
            entry.sheet.cells = vec![cell("PageWidth", 8.5), cell("PageHeight", 11.0)];
        }
    }
    for (x, y) in [(1.0, 8.0), (4.0, 8.0), (1.0, 5.0), (4.0, 5.0), (2.5, 2.5)] {
        doc.insert_shape(first, box_shape(x, y), None).expect("box");
    }
    for (x, y) in [(1.0, 6.5), (4.0, 3.5)] {
        doc.insert_shape(first, connector_shape(x, y), None)
            .expect("connector");
    }
    // One group of two sub-shapes (so `arrange.ungroup` has a subject).
    let children: Vec<ShapeId> = [(0.5, 1.0), (1.5, 1.0)]
        .into_iter()
        .map(|(x, y)| doc.add_shape(box_shape(x, y)))
        .collect();
    let mut group = box_shape(1.0, 1.5);
    group.children = children;
    doc.insert_shape(first, group, None).expect("group");
    for (x, y) in [(1.0, 8.0), (4.0, 4.0)] {
        doc.insert_shape(second, box_shape(x, y), None)
            .expect("page-two box");
    }

    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    let registry = registry().expect("registry");
    // Setup: glue both connector ends (one outline glue and one
    // connection-point glue per connector) and route the paths.
    let tops = top_level(&ctx);
    let visio: Vec<i32> = tops.iter().map(|shape| visio_of(&ctx, *shape)).collect();
    for (connector, begin, end) in [(5usize, 0usize, 1usize), (6, 2, 3)] {
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            Args::new()
                .with("connector", ArgValue::Integer(i64::from(visio[connector])))
                .with("target", ArgValue::Integer(i64::from(visio[begin]))),
        )
        .expect("seed glue begin");
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_end",
            Args::new()
                .with("connector", ArgValue::Integer(i64::from(visio[connector])))
                .with("target", ArgValue::Integer(i64::from(visio[end])))
                .with("point", ArgValue::Integer(1)),
        )
        .expect("seed glue end");
    }
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("seed route");
    if let Some(session) = ctx.active_document_mut() {
        session.undo.clear();
    }
    (ctx, registry, first)
}

/// The number of undo entries of the active document.
fn undo_len(ctx: &AppCtx) -> usize {
    ctx.active_document().map_or(0, |state| state.undo.len())
}

/// One `edit.undo` / `edit.redo` dispatch.
fn rewind(ctx: &mut AppCtx, registry: &Registry, id: &str) {
    dispatch(ctx, registry, id, Args::new()).expect("undo/redo runs");
}

fn document(ctx: &AppCtx) -> Document {
    ctx.active_document().expect("document").document.clone()
}

/// The top-level shapes of the active page, in z-order.
fn top_level(ctx: &AppCtx) -> Vec<ShapeId> {
    let session = ctx.active_document().expect("document");
    let page = session.active_page.expect("an active page");
    session.document.page(page).expect("page").shapes.to_vec()
}

/// The Visio `ID` of one shape.
fn visio_of(ctx: &AppCtx, shape: ShapeId) -> i32 {
    ctx.active_document()
        .expect("document")
        .document
        .shape(shape)
        .expect("shape")
        .visio_id
}

/// Selects the first `count` top-level shapes of the active page.
fn select_tops(ctx: &mut AppCtx, count: usize) {
    let tops = top_level(ctx);
    let chosen: Vec<ShapeId> = tops.into_iter().take(count).collect();
    ctx.selection.anchor = chosen.last().copied();
    ctx.selection.shapes = chosen;
    let page = ctx.active_document().and_then(|state| state.active_page);
    ctx.selection.page = page;
}

/// Selects the top-level shapes named by `count` Visio `ID`s of the
/// active page (nothing when the id is gone).
fn select_visio(ctx: &mut AppCtx, ids: &[i32]) {
    let (page, chosen) = {
        let session = ctx.active_document().expect("document");
        let page = session.active_page.expect("an active page");
        let chosen: Vec<ShapeId> = ids
            .iter()
            .filter_map(|id| session.document.shape_by_visio_id(page, *id))
            .collect();
        (page, chosen)
    };
    ctx.selection.anchor = chosen.last().copied();
    ctx.selection.shapes = chosen;
    ctx.selection.page = Some(page);
}

/// A small deterministic number per `(pick, salt)` in `[-1, 1]` steps of
/// a quarter: enough variety for the property without floating noise.
fn jitter(pick: u32, salt: u32) -> f64 {
    let steps = (pick.wrapping_mul(7).wrapping_add(salt.wrapping_mul(13))) % 9;
    (f64::from(steps) - 4.0) * 0.25
}

/// A positive size per `(pick, salt)`.
fn size(pick: u32, salt: u32) -> f64 {
    0.5 + f64::from(pick.wrapping_add(salt) % 7) * 0.5
}

/// Builds `Args` from `(name, number)` pairs.
fn numbers(pairs: &[(&str, f64)]) -> Args {
    let mut args = Args::new();
    for (name, value) in pairs {
        args.set(*name, ArgValue::Number(*value));
    }
    args
}

// ---------------------------------------------------------------------------
// The reachable state (task P5-11 note)
// ---------------------------------------------------------------------------

/// The shapes reachable from `roots`, in wrapper order (a cyclic child
/// list stops at the first repeat), plus the masters their instances
/// use.
fn collect(
    document: &Document,
    roots: &[ShapeId],
    order: &mut Vec<ShapeId>,
    seen: &mut BTreeSet<u32>,
    masters: &mut BTreeSet<u32>,
) {
    let mut stack: Vec<ShapeId> = roots.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.index) {
            continue;
        }
        let Some(entry) = document.shape(id) else {
            continue;
        };
        if let Some(master) = entry.master {
            masters.insert(master.index);
        }
        order.push(id);
        for child in entry.children.iter().rev() {
            stack.push(*child);
        }
    }
}

/// The reachable state (module docs): the pages in order, the shapes
/// reachable from them (arena renumbered into wrapper order), their
/// sheets and text, the `Connect` records of those shapes, and the
/// masters their instances use (renumbered into document order).
/// Detached arena entries and unused materialised masters are dropped:
/// the known differences of the undo contract.
fn reachable(document: &Document) -> Document {
    let mut order: Vec<ShapeId> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut masters: BTreeSet<u32> = BTreeSet::new();
    for page in &document.pages {
        collect(document, &page.shapes, &mut order, &mut seen, &mut masters);
    }
    // Masters reachable through instances, to a fixpoint (each master is
    // walked at most once, so the loop is bounded by the master count).
    let mut walked: BTreeSet<u32> = BTreeSet::new();
    loop {
        let next = {
            let mut pending = masters.difference(&walked);
            pending.next().copied()
        };
        let Some(index) = next else {
            break;
        };
        walked.insert(index);
        let roots: Vec<ShapeId> = document
            .master(MasterId { index })
            .map(|master| master.shapes.clone())
            .unwrap_or_default();
        collect(document, &roots, &mut order, &mut seen, &mut masters);
    }
    let walked: Vec<u32> = walked
        .into_iter()
        .filter(|index| document.master(MasterId { index: *index }).is_some())
        .collect();

    let index_map: HashMap<u32, u32> = order
        .iter()
        .enumerate()
        .map(|(new, id)| (id.index, u32::try_from(new).unwrap_or(u32::MAX)))
        .collect();
    let master_map: HashMap<u32, u32> = walked
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, u32::try_from(new).unwrap_or(u32::MAX)))
        .collect();
    let rename = |id: ShapeId| ShapeId {
        index: *index_map.get(&id.index).unwrap_or(&id.index),
    };

    let mut out = Document::new();
    out.kind = document.kind;
    out.stylesheet = document.stylesheet.clone();
    out.properties = document.properties.clone();
    out.themes = document.themes.clone();
    out.color_table = document.color_table.clone();
    out.preserved = document.preserved.clone();

    for id in &order {
        let mut shape = document.shape(*id).expect("reachable shape").clone();
        shape.id = rename(*id);
        shape.parent = shape.parent.map(rename);
        shape.children = shape.children.iter().copied().map(rename).collect();
        shape.master = shape.master.map(|master| MasterId {
            index: *master_map.get(&master.index).unwrap_or(&master.index),
        });
        out.shapes.push(shape);
    }
    for index in &walked {
        let mut master = document
            .master(MasterId { index: *index })
            .expect("kept master")
            .clone();
        master.id = MasterId {
            index: *master_map.get(index).unwrap_or(index),
        };
        master.shapes = master.shapes.iter().copied().map(rename).collect();
        out.masters.push(master);
    }
    for page in &document.pages {
        // The `Connect` records of this page's reachable shapes.
        let mut roots: Vec<ShapeId> = Vec::new();
        let mut page_seen: BTreeSet<u32> = BTreeSet::new();
        let mut page_masters: BTreeSet<u32> = BTreeSet::new();
        collect(
            document,
            &page.shapes,
            &mut roots,
            &mut page_seen,
            &mut page_masters,
        );
        let mut live: BTreeSet<i32> = BTreeSet::new();
        for id in &roots {
            if let Some(entry) = document.shape(*id) {
                live.insert(entry.visio_id);
            }
        }
        let mut page = page.clone();
        page.shapes = page.shapes.iter().copied().map(rename).collect();
        page.connects
            .retain(|record| live.contains(&record.from_sheet));
        out.pages.push(page);
    }
    out
}

// ---------------------------------------------------------------------------
// The normalisation (the `flowchart.rs` pattern, module docs)
// ---------------------------------------------------------------------------

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

/// Sorts the preserved fragment and drops its `Rel` children (the
/// relationship plumbing).
fn sort_preserved(preserved: &mut Preserved) {
    preserved
        .attributes
        .sort_by(|a, b| (&a.name.uri, &a.name.local).cmp(&(&b.name.uri, &b.name.local)));
    preserved.children.retain(|child| child.name.local != "Rel");
    for child in &mut preserved.children {
        sort_node(child);
    }
}

/// [`sort_preserved`] plus the `U` kind/unit spelling dropped
/// (writer-managed).
fn sort_cell_preserved(preserved: &mut Preserved) {
    preserved
        .attributes
        .retain(|attribute| attribute.name.local != "U");
    sort_preserved(preserved);
}

/// Sorts the preserved fragments of one sheet.
fn sort_sheet(sheet: &mut Sheet) {
    for cell in &mut sheet.cells {
        fold_cell(cell);
        sort_cell_preserved(&mut cell.preserved);
    }
    for section in &mut sheet.sections {
        sort_preserved(&mut section.preserved);
        for cell in &mut section.cells {
            fold_cell(cell);
            sort_cell_preserved(&mut cell.preserved);
        }
        for row in &mut section.rows {
            sort_preserved(&mut row.preserved);
            for cell in &mut row.cells {
                fold_cell(cell);
                sort_cell_preserved(&mut cell.preserved);
            }
        }
    }
}

/// Folds f64 rounding noise (module docs): 12 significant digits, and
/// anything within a nano-inch of zero folds to zero (a round trip
/// through a rotation lands at `0 ± 4e-16`).
fn rounded(value: f64) -> f64 {
    if value == 0.0 || value.abs() < 1e-9 {
        return 0.0;
    }
    format!("{value:.12e}").parse::<f64>().unwrap_or(value)
}

/// [`rounded`] for one cell: the value and, when the formula is its
/// literal spelling, the formula too (the routed-path spelling of the
/// same number must fold with it).
fn fold_cell(cell: &mut Cell) {
    let Some(CellValue::Number(value)) = cell.value else {
        return;
    };
    let folded = rounded(value);
    cell.value = Some(CellValue::Number(folded));
    if let Some(formula) = &cell.formula
        && formula.parse::<f64>().is_ok_and(|literal| literal == value)
    {
        cell.formula = Some(format!("{folded}"));
    }
}

/// The comparable form of one document (module docs).
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

/// The compared state (module docs).
fn same(a: &Document, b: &Document) -> bool {
    normalised(&reachable(a)) == normalised(&reachable(b))
}

// ---------------------------------------------------------------------------
// The command cases: one per editing command id (the generated list)
// ---------------------------------------------------------------------------

/// One editing command the property drives: `run` prepares the current
/// state and dispatches the command (its own id) with `pick`-chosen
/// arguments. A run whose command cannot apply (its subject is gone,
/// its predicate says no) pushes nothing and is skipped;
/// `every_case_produces_exactly_one_edit_on_the_seed` proves each case
/// does apply over the seed.
struct Case {
    /// The registry id, dispatched by the runner.
    id: &'static str,
    /// Prepares and dispatches the command.
    run: fn(&mut AppCtx, &Registry, &'static str, u32),
}

/// Dispatches `id`, ignoring the outcome (a failed step pushes nothing).
fn try_dispatch(ctx: &mut AppCtx, registry: &Registry, id: &str, args: Args) {
    let _ = dispatch(ctx, registry, id, args);
}

/// The selection-based commands (`shape.*` setters and the `arrange.*`
/// alignments): the first `count` top-level shapes and no arguments.
fn run_selected(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    select_tops(ctx, 2);
    try_dispatch(ctx, registry, id, Args::new());
}

fn run_move(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1 + (pick % 2) as usize);
    try_dispatch(
        ctx,
        registry,
        id,
        numbers(&[("dx", jitter(pick, 1)), ("dy", jitter(pick, 2))]),
    );
}

fn run_resize(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(
        ctx,
        registry,
        id,
        numbers(&[("width", size(pick, 1)), ("height", size(pick, 2))]),
    );
}

fn run_rotate(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, id, numbers(&[("angle", jitter(pick, 3))]));
}

fn run_set_width(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, id, numbers(&[("width", size(pick, 3))]));
}

fn run_set_height(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, id, numbers(&[("height", size(pick, 4))]));
}

fn run_set_pin(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(
        ctx,
        registry,
        id,
        numbers(&[("x", 2.0 + jitter(pick, 5)), ("y", 2.0 + jitter(pick, 6))]),
    );
}

fn run_set_angle(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, id, numbers(&[("angle", jitter(pick, 7))]));
}

/// `data.set.property`: the anchor shape's `Cost` row, a new number
/// value of the row's own kind (task P6-18).
fn run_set_property(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    let mut args = Args::new();
    args.set("row", ArgValue::Text("Cost".to_owned()));
    args.set("value", ArgValue::Text(size(pick, 7).to_string()));
    try_dispatch(ctx, registry, id, args);
}

/// `shape.set.text` and `text.commit` (with its `text` argument).
fn run_text(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    let mut args = Args::new();
    args.set("text", ArgValue::Text(format!("{id} {pick}")));
    try_dispatch(ctx, registry, id, args);
}

fn run_delete(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1 + (pick % 2) as usize);
    try_dispatch(ctx, registry, id, Args::new());
}

fn run_cut(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1 + (pick % 2) as usize);
    try_dispatch(ctx, registry, id, Args::new());
}

fn run_duplicate(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, id, Args::new());
}

/// `edit.paste` copies its subject first (a clipboard action, no edit).
fn run_paste(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    select_tops(ctx, 1);
    try_dispatch(ctx, registry, "edit.copy", Args::new());
    try_dispatch(
        ctx,
        registry,
        id,
        numbers(&[("x", 3.0 + jitter(pick, 8)), ("y", 3.0 + jitter(pick, 9))]),
    );
}

/// `insert.shape`: a catalog master instance (the first drop also
/// materialises the master, a known difference of the undo contract).
fn run_insert_shape(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let mut args = numbers(&[("x", 3.0 + jitter(pick, 10)), ("y", 3.0 + jitter(pick, 11))]);
    args.set("stencil", ArgValue::Text("Basic Shapes".to_owned()));
    args.set(
        "master",
        ArgValue::Text(if pick.is_multiple_of(2) {
            "Rectangle".to_owned()
        } else {
            "Rounded Rectangle".to_owned()
        }),
    );
    try_dispatch(ctx, registry, id, args);
}

/// `insert.drawing.*`: a rectangle, an ellipse, a text box or a line
/// (the line's `width`/`height` are its delta).
fn run_drawing(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let (width, height) = if id == "insert.drawing.line" {
        (jitter(pick, 14), jitter(pick, 15))
    } else {
        (size(pick, 5), size(pick, 6))
    };
    try_dispatch(
        ctx,
        registry,
        id,
        numbers(&[
            ("x", 3.0 + jitter(pick, 12)),
            ("y", 3.0 + jitter(pick, 13)),
            ("width", width),
            ("height", height),
        ]),
    );
}

/// `connector.glue_begin` / `connector.glue_end`: the first top-level
/// shape glues to the last one; `point` is omitted for outline glue
/// every third pick (both glue kinds, ADR 0045 rule 1).
fn run_glue(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let tops = top_level(ctx);
    let (Some(connector), Some(target)) = (tops.first().copied(), tops.last().copied()) else {
        return;
    };
    if tops.len() < 2 || connector == target {
        return;
    }
    let mut args = numbers(&[]);
    args.set(
        "connector",
        ArgValue::Integer(i64::from(visio_of(ctx, connector))),
    );
    args.set(
        "target",
        ArgValue::Integer(i64::from(visio_of(ctx, target))),
    );
    if !pick.is_multiple_of(3) {
        args.set("point", ArgValue::Integer(i64::from(pick % 2)));
    }
    try_dispatch(ctx, registry, id, args);
}

/// `connector.unglue` over the first shape that has a glue record.
fn run_unglue(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let glued: Vec<i32> = {
        let session = ctx.active_document().expect("document");
        let page = session.active_page.expect("an active page");
        session
            .document
            .page(page)
            .map(|entry| {
                entry
                    .connects
                    .iter()
                    .map(|record| record.from_sheet)
                    .collect()
            })
            .unwrap_or_default()
    };
    let Some(connector) = glued.first().copied() else {
        return;
    };
    let mut args = numbers(&[]);
    args.set("connector", ArgValue::Integer(i64::from(connector)));
    args.set(
        "end",
        ArgValue::Text(if pick.is_multiple_of(2) {
            "begin".to_owned()
        } else {
            "end".to_owned()
        }),
    );
    try_dispatch(ctx, registry, id, args);
}

/// `connector.style` over the first two top-level shapes.
fn run_style(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let ids: Vec<i32> = top_level(ctx)
        .iter()
        .take(2)
        .map(|s| visio_of(ctx, *s))
        .collect();
    select_visio(ctx, &ids);
    let mut args = numbers(&[]);
    args.set(
        "style",
        ArgValue::Text(if pick.is_multiple_of(2) {
            "right_angle".to_owned()
        } else {
            "straight".to_owned()
        }),
    );
    try_dispatch(ctx, registry, id, args);
}

/// `connector.draw`: one connector master instance with one free end.
fn run_draw(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let mut args = numbers(&[
        ("x", 3.0 + jitter(pick, 16)),
        ("y", 3.0 + jitter(pick, 17)),
        ("end_x", 4.0 + jitter(pick, 18)),
        ("end_y", 4.0 + jitter(pick, 19)),
    ]);
    args.set("stencil", ArgValue::Text("Basic Flowchart".to_owned()));
    args.set("master", ArgValue::Text("Connector".to_owned()));
    try_dispatch(ctx, registry, id, args);
}

/// The two distributions over three shapes.
fn run_distribute(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    select_tops(ctx, 3);
    try_dispatch(ctx, registry, id, Args::new());
}

/// `arrange.group`: the first two top-level shapes.
fn run_group(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    select_tops(ctx, 2);
    try_dispatch(ctx, registry, id, Args::new());
}

/// `arrange.ungroup`: the first top-level group, when one exists.
fn run_ungroup(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    let group = top_level(ctx).into_iter().find(|shape| {
        ctx.active_document()
            .and_then(|state| state.document.shape(*shape))
            .is_some_and(|entry| !entry.children.is_empty())
    });
    let Some(group) = group else {
        return;
    };
    ctx.selection.shapes = vec![group];
    ctx.selection.anchor = Some(group);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    try_dispatch(ctx, registry, id, Args::new());
}

/// `page.add` and `page.rename` take a name; `page.delete` and the two
/// one-step moves take nothing.
fn run_page_named(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let mut args = numbers(&[]);
    args.set("name", ArgValue::Text(format!("{id} {pick}")));
    try_dispatch(ctx, registry, id, args);
}

fn run_page_plain(ctx: &mut AppCtx, registry: &Registry, id: &'static str, _pick: u32) {
    try_dispatch(ctx, registry, id, Args::new());
}

/// `page.reorder`: page 0 to page `pick % 2`, when the document has two
/// pages.
fn run_page_reorder(ctx: &mut AppCtx, registry: &Registry, id: &'static str, pick: u32) {
    let pages = ctx
        .active_document()
        .map_or(0, |state| state.document.pages.len());
    if pages < 2 {
        return;
    }
    let mut args = numbers(&[]);
    args.set("from", ArgValue::Integer(0));
    args.set("to", ArgValue::Integer(i64::from(pick % 2)));
    try_dispatch(ctx, registry, id, args);
}

/// Every editing command id of the registry and how the property runs
/// it. The list is generated against the registry:
/// `every_implemented_id_is_classified` fails when an implemented
/// command has no entry here and no non-editing reason below (ADR 0040
/// rule 4).
const CASES: &[Case] = &[
    Case {
        id: "arrange.align.left",
        run: run_selected,
    },
    Case {
        id: "arrange.align.center_x",
        run: run_selected,
    },
    Case {
        id: "arrange.align.right",
        run: run_selected,
    },
    Case {
        id: "arrange.align.top",
        run: run_selected,
    },
    Case {
        id: "arrange.align.middle",
        run: run_selected,
    },
    Case {
        id: "arrange.align.bottom",
        run: run_selected,
    },
    Case {
        id: "arrange.distribute.horizontal",
        run: run_distribute,
    },
    Case {
        id: "arrange.distribute.vertical",
        run: run_distribute,
    },
    Case {
        id: "arrange.bring_forward",
        run: run_selected,
    },
    Case {
        id: "arrange.send_backward",
        run: run_selected,
    },
    Case {
        id: "arrange.bring_to_front",
        run: run_selected,
    },
    Case {
        id: "arrange.send_to_back",
        run: run_selected,
    },
    Case {
        id: "arrange.group",
        run: run_group,
    },
    Case {
        id: "arrange.ungroup",
        run: run_ungroup,
    },
    Case {
        id: "connector.glue_begin",
        run: run_glue,
    },
    Case {
        id: "connector.glue_end",
        run: run_glue,
    },
    Case {
        id: "connector.unglue",
        run: run_unglue,
    },
    Case {
        id: "connector.style",
        run: run_style,
    },
    Case {
        id: "connector.draw",
        run: run_draw,
    },
    Case {
        id: "edit.cut",
        run: run_cut,
    },
    Case {
        id: "edit.delete",
        run: run_delete,
    },
    Case {
        id: "edit.duplicate",
        run: run_duplicate,
    },
    Case {
        id: "edit.paste",
        run: run_paste,
    },
    Case {
        id: "insert.shape",
        run: run_insert_shape,
    },
    Case {
        id: "insert.drawing.rectangle",
        run: run_drawing,
    },
    Case {
        id: "insert.drawing.ellipse",
        run: run_drawing,
    },
    Case {
        id: "insert.drawing.line",
        run: run_drawing,
    },
    Case {
        id: "insert.drawing.text_box",
        run: run_drawing,
    },
    Case {
        id: "page.add",
        run: run_page_named,
    },
    Case {
        id: "page.delete",
        run: run_page_plain,
    },
    Case {
        id: "page.rename",
        run: run_page_named,
    },
    Case {
        id: "page.reorder",
        run: run_page_reorder,
    },
    Case {
        id: "page.move_left",
        run: run_page_plain,
    },
    Case {
        id: "page.move_right",
        run: run_page_plain,
    },
    Case {
        id: "shape.move",
        run: run_move,
    },
    Case {
        id: "shape.resize",
        run: run_resize,
    },
    Case {
        id: "shape.rotate",
        run: run_rotate,
    },
    Case {
        id: "shape.set.width",
        run: run_set_width,
    },
    Case {
        id: "shape.set.height",
        run: run_set_height,
    },
    Case {
        id: "shape.set.pin",
        run: run_set_pin,
    },
    Case {
        id: "shape.set.angle",
        run: run_set_angle,
    },
    Case {
        id: "data.set.property",
        run: run_set_property,
    },
    Case {
        id: "shape.set.text",
        run: run_text,
    },
    Case {
        id: "text.commit",
        run: run_text,
    },
];

/// Every implemented registry id that produces no undoable edit, with
/// the reason (the other half of the generated list). These run over
/// the seed with no arguments and must never answer an
/// `Outcome::edit`.
const NON_EDITING: &[(&str, &str)] = &[
    ("app.office.center.options", "opens a dialog (UiRequest)"),
    ("app.quit", "quits (UiRequest)"),
    ("help.about", "opens a dialog (UiRequest)"),
    ("help.welcome", "opens a dialog (UiRequest)"),
    ("file.new", "replaces the active document, no undo (P4-02)"),
    ("file.open", "reads a file, no undo (P4-02)"),
    ("file.save", "writes a file (ADR 0048)"),
    ("file.save_as", "writes a file (ADR 0048)"),
    ("file.close", "closes a session (UiRequest::ConfirmClose)"),
    ("file.inspect", "reports a summary"),
    ("file.render", "writes a raster"),
    ("file.export", "writes a file"),
    ("file.exit", "quits (UiRequest::Quit)"),
    ("view.zoom.in", "view state only (ADR 0040 rule 3)"),
    ("view.zoom.out", "view state only (ADR 0040 rule 3)"),
    ("view.zoom.fit", "view state only (ADR 0040 rule 3)"),
    ("view.toggle_grid", "view state only (ADR 0040 rule 3)"),
    ("view.rulers", "view state only (ADR 0040 rule 3)"),
    ("view.units", "view state only (ADR 0040 rule 3)"),
    ("view.ui_layout.panel_2016", "layout choice only (ADR 0032)"),
    ("view.ui_layout.panel_365", "layout choice only (ADR 0032)"),
    (
        "view.ui_layout.panel_classic",
        "layout choice only (ADR 0061)",
    ),
    (
        "view.ui_layout.panel_modern",
        "layout choice only (ADR 0061)",
    ),
    ("page.next", "active-page navigation, no edit"),
    ("page.previous", "active-page navigation, no edit"),
    ("edit.select", "selection only (P5-16)"),
    ("edit.select_all", "selection only (P5-16)"),
    ("edit.deselect", "selection only (P5-16)"),
    ("edit.copy", "clipboard only (ADR 0047)"),
    (
        "edit.undo",
        "the undo stack itself (the property drives it)",
    ),
    (
        "edit.redo",
        "the undo stack itself (the property drives it)",
    ),
    (
        "connector.reroute_all",
        "re-routing is not undoable on its own (ADR 0045 rule 3)",
    ),
    ("shape.enter_group", "selection only (P5-04)"),
    ("shape.exit_group", "selection only (P5-04)"),
    (
        "text.begin",
        "editor lifecycle, no document write (ADR 0044)",
    ),
    (
        "text.cancel",
        "editor lifecycle, no document write (ADR 0044)",
    ),
    ("tools.pointer", "tool choice only (P5-17)"),
    ("tools.rectangle", "tool choice only (P5-17)"),
    ("tools.ellipse", "tool choice only (P5-17)"),
    ("tools.text", "tool choice only (P5-17)"),
    ("tools.connector", "tool choice only (P5-17)"),
    ("shape.rectangle", "tool choice only (P5-17)"),
    ("shape.ellipse", "tool choice only (P5-17)"),
    ("shape.line", "tool choice only (P5-17)"),
];

// ---------------------------------------------------------------------------
// The generated list: every implemented id is classified
// ---------------------------------------------------------------------------

#[test]
fn every_implemented_id_is_classified() {
    let registry = registry().expect("registry");
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for id in registry.ids() {
        if registry.status(&id) != CommandStatus::Implemented {
            continue;
        }
        let id = id.as_str();
        assert!(
            seen.insert(id),
            "{id} is registered twice (the registry would reject it)"
        );
        let has_case = CASES.iter().any(|case| case.id == id);
        let has_reason = NON_EDITING.iter().any(|(other, _)| *other == id);
        assert!(
            has_case || has_reason,
            "{id} is implemented but has no undo property case and no \
             non-editing reason: add one to `CASES` or `NON_EDITING` \
             (ADR 0040 rule 4)"
        );
        assert!(
            !(has_case && has_reason),
            "{id} is both a case and a non-editing reason"
        );
    }
    // The lists name real ids and nothing twice.
    for case in CASES {
        assert!(
            seen.contains(case.id),
            "{} is not an implemented registry id",
            case.id
        );
    }
    for (id, _) in NON_EDITING {
        assert!(seen.contains(id), "{id} is not an implemented registry id");
    }
}

/// Every case pushes exactly one undo step over the seed (ADR 0040
/// rule 1: one command, one edit).
#[test]
fn every_case_produces_exactly_one_edit_on_the_seed() {
    for case in CASES {
        for pick in 0..3 {
            let (mut ctx, registry, _page) = fixture();
            let before = undo_len(&ctx);
            (case.run)(&mut ctx, &registry, case.id, pick);
            assert_eq!(
                undo_len(&ctx),
                before + 1,
                "{} (pick {pick}) must produce exactly one undo step",
                case.id
            );
        }
    }
}

/// The non-editing ids produce no edit and push no undo step.
#[test]
fn non_editing_commands_never_produce_an_edit() {
    for (id, reason) in NON_EDITING {
        let (mut ctx, registry, _page) = fixture();
        let before = undo_len(&ctx);
        let outcome = registry.execute(
            &CommandId::intern(id).expect("id"),
            &mut ctx,
            Invocation::empty(Source::Test),
        );
        if let Ok(outcome) = outcome {
            assert!(
                outcome.edit.is_none(),
                "{id} produced an edit but is listed as non-editing: {reason}"
            );
        }
        assert_eq!(undo_len(&ctx), before, "{id} pushed an undo step");
    }
}

// ---------------------------------------------------------------------------
// The undo inverse: hand-written pairs and the sequence property
// ---------------------------------------------------------------------------

/// ADR 0040 rule 4 per command: run it, undo, compare with the seed;
/// run it, undo, redo, compare with the post-command state. The
/// assertion names the command.
#[test]
fn every_case_inverts_on_the_seed() {
    for case in CASES {
        for pick in 0..3 {
            let (mut ctx, registry, _page) = fixture();
            let seed = document(&ctx);
            (case.run)(&mut ctx, &registry, case.id, pick);
            let pushed = undo_len(&ctx);
            let post = document(&ctx);
            assert!(pushed > 0, "{} (pick {pick}) pushed no edit", case.id);
            for _ in 0..pushed {
                rewind(&mut ctx, &registry, "edit.undo");
            }
            assert!(
                same(&document(&ctx), &seed),
                "{} (pick {pick}): undo did not restore the seed",
                case.id
            );
            for _ in 0..pushed {
                rewind(&mut ctx, &registry, "edit.redo");
            }
            assert!(
                same(&document(&ctx), &post),
                "{} (pick {pick}): redo did not restore the post state",
                case.id
            );
        }
    }
}

/// One recorded step of a random sequence: the undo entries it pushed
/// and the reachable state after it.
struct Step {
    /// The command that pushed the entries (for the failure message).
    id: &'static str,
    /// How many undo entries the step pushed (a case may stage more
    /// than one dispatch).
    edits: usize,
    /// The reachable state after the step.
    state: Document,
}

/// Runs `steps` over the seed, then checks the ADR 0040 rule 4
/// inverse: undoing every entry restores the seed, redoing the
/// sequence reproduces the state after each step. Panics name the
/// command at fault.
fn check_sequence(steps: &[(usize, u32)]) {
    let (mut ctx, registry, _page) = fixture();
    let seed = document(&ctx);
    let mut log: Vec<Step> = Vec::new();
    for &(case, pick) in steps {
        let before = undo_len(&ctx);
        (CASES[case].run)(&mut ctx, &registry, CASES[case].id, pick);
        let edits = undo_len(&ctx) - before;
        if edits > 0 {
            log.push(Step {
                id: CASES[case].id,
                edits,
                state: document(&ctx),
            });
        }
    }
    let total: usize = log.iter().map(|step| step.edits).sum();
    for _ in 0..total {
        rewind(&mut ctx, &registry, "edit.undo");
    }
    let names: Vec<&str> = log.iter().map(|step| step.id).collect();
    assert!(
        same(&document(&ctx), &seed),
        "undoing every step did not restore the seed (sequence: {names:?})"
    );
    for step in &log {
        for _ in 0..step.edits {
            rewind(&mut ctx, &registry, "edit.redo");
        }
        assert!(
            same(&document(&ctx), &step.state),
            "redo of {} did not restore its state (sequence: {names:?})",
            step.id
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// ADR 0040 rule 4 over random command sequences (module docs).
    #[test]
    fn undo_is_the_inverse_of_random_command_sequences(
        steps in proptest::collection::vec((0..CASES.len(), 0u32..8), 1..=5),
    ) {
        check_sequence(&steps);
    }
}

// ---------------------------------------------------------------------------
// The known differences (task P5-11 note)
// ---------------------------------------------------------------------------

/// Group, insert and delete undo leave detached arena entries behind,
/// and the first drop of a catalog master leaves the materialised
/// master in the document. Both are visible in the raw `Document` and
/// excluded from the reachable state, which is exactly why the property
/// compares the reachable state (module docs).
#[test]
fn known_undo_differences_stay_outside_the_reachable_state() {
    let (mut ctx, registry, _page) = fixture();
    let seed = document(&ctx);

    // One insert (materialises a catalog master) and one delete.
    run_insert_shape(&mut ctx, &registry, "insert.shape", 0);
    select_tops(&mut ctx, 1);
    dispatch(&mut ctx, &registry, "edit.delete", Args::new()).expect("delete");
    let pushed = undo_len(&ctx);
    assert!(pushed >= 2, "two undo steps, got {pushed}");

    for _ in 0..pushed {
        rewind(&mut ctx, &registry, "edit.undo");
    }
    let undone = document(&ctx);
    assert!(
        same(&undone, &seed),
        "the reachable state is restored despite the leftovers"
    );
    // The leftovers are real: a detached arena entry and an unused
    // materialised master.
    assert!(
        undone.shapes.len() > reachable(&undone).shapes.len(),
        "detached arena entries stay behind (raw: {}, reachable: {})",
        undone.shapes.len(),
        reachable(&undone).shapes.len()
    );
    assert!(
        !undone.masters.is_empty() && reachable(&undone).masters.is_empty(),
        "the first catalog drop leaves its master behind, unused"
    );
}

// ---------------------------------------------------------------------------
// Exact-inverse regressions for the fixes this task made (task P5-11)
// ---------------------------------------------------------------------------

/// The first top-level group of the active page (the fixture's raw
/// group).
fn first_group(ctx: &AppCtx) -> ShapeId {
    top_level(ctx)
        .into_iter()
        .find(|shape| {
            ctx.active_document()
                .and_then(|state| state.document.shape(*shape))
                .is_some_and(|entry| !entry.children.is_empty())
        })
        .expect("a group on the page")
}

/// A nested group's undo is the exact inverse too (task P5-11): the
/// members go back into their **parent's** list at their old positions,
/// not onto the page.
#[test]
fn a_nested_group_undo_restores_the_members_in_their_parent() {
    let (mut ctx, registry, _page) = fixture();
    let seed = document(&ctx);

    // One outer group of the first two boxes, then one inner group of
    // the outer group's two children.
    run_group(&mut ctx, &registry, "arrange.group", 0);
    let outer = *top_level(&ctx).last().expect("the outer group");
    let children = ctx
        .active_document()
        .and_then(|state| state.document.shape(outer))
        .map(|entry| entry.children.clone())
        .unwrap_or_default();
    assert_eq!(children.len(), 2, "the outer group has two members");
    ctx.selection.shapes = children.clone();
    ctx.selection.anchor = children.last().copied();
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("inner group");
    let pushed = undo_len(&ctx);
    assert_eq!(pushed, 2, "two group commands, two undo steps");
    let post = document(&ctx);

    for _ in 0..pushed {
        rewind(&mut ctx, &registry, "edit.undo");
    }
    assert!(
        same(&document(&ctx), &seed),
        "both groups undo back to the seed"
    );
    for _ in 0..pushed {
        rewind(&mut ctx, &registry, "edit.redo");
    }
    assert!(same(&document(&ctx), &post), "both groups redo exactly");
}

/// Undoing an ungroup restores the group's own shape, its position and
/// the `Connects` records that name it (task P5-11).
#[test]
fn an_ungroup_undo_restores_the_group_its_connects_and_its_slot() {
    let (mut ctx, registry, _page) = fixture();
    let group = first_group(&ctx);
    let group_visio = visio_of(&ctx, group);
    // One record naming the group itself: glue the first shape's begin
    // to it (a connector end by the ADR 0045 rule).
    let connector_visio = visio_of(&ctx, top_level(&ctx)[0]);
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector_visio)))
            .with("target", ArgValue::Integer(i64::from(group_visio))),
    )
    .expect("glue to the group");
    let before = document(&ctx);

    run_ungroup(&mut ctx, &registry, "arrange.ungroup", 0);
    let after = document(&ctx);
    assert!(undo_len(&ctx) >= 2, "the glue and the ungroup are undoable");

    // One undo step: the ungroup (the glue step stays applied).
    rewind(&mut ctx, &registry, "edit.undo");
    assert!(
        same(&document(&ctx), &before),
        "the group, its slot and its Connects come back"
    );
    rewind(&mut ctx, &registry, "edit.redo");
    assert!(
        same(&document(&ctx), &after),
        "the redo is the exact inverse"
    );
}

/// Deleting one child of a group restores it into the group's child
/// list (task P5-11: `restore_shapes` for a nested root).
#[test]
fn deleting_a_group_child_restores_it_into_the_group() {
    let (mut ctx, registry, _page) = fixture();
    let group = first_group(&ctx);
    let before = document(&ctx);
    let child = ctx
        .active_document()
        .and_then(|state| state.document.shape(group))
        .and_then(|entry| entry.children.first().copied())
        .expect("a child");
    ctx.selection.shapes = vec![child];
    ctx.selection.anchor = Some(child);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    dispatch(&mut ctx, &registry, "edit.delete", Args::new()).expect("delete the child");
    let after = document(&ctx);
    rewind(&mut ctx, &registry, "edit.undo");
    assert!(
        same(&document(&ctx), &before),
        "the child is back inside its group"
    );
    rewind(&mut ctx, &registry, "edit.redo");
    assert!(
        same(&document(&ctx), &after),
        "the redo is the exact inverse"
    );
}

/// The skip warnings of the arrange commands (task P5-11): a selection
/// that is only partly a group or only partly top-level is processed
/// with one line per skipped shape.
#[test]
fn a_partly_eligible_selection_is_skipped_with_one_warning() {
    let (mut ctx, registry, _page) = fixture();
    let group = first_group(&ctx);
    let plain = *top_level(&ctx).first().expect("a box");

    // Z-order with a shape that cannot move (already on top) in the
    // selection.
    let top = *top_level(&ctx).last().expect("the top shape");
    ctx.selection.shapes = vec![top, plain];
    ctx.selection.anchor = Some(plain);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    let outcome =
        dispatch(&mut ctx, &registry, "arrange.bring_to_front", Args::new()).expect("z-order");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|line| line.contains("not top-level on the active page; skipped")),
        "{:?}",
        outcome.warnings
    );

    // Ungroup with a mixed selection: the group and one plain box.
    ctx.selection.shapes = vec![group, plain];
    ctx.selection.anchor = Some(plain);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    let outcome = dispatch(&mut ctx, &registry, "arrange.ungroup", Args::new()).expect("ungroup");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|line| line.contains("is not a group; skipped")),
        "{:?}",
        outcome.warnings
    );
}

/// The thin selection shapes of the arrange and clipboard commands
/// (task P5-11): one shape, a group copy, a nested cut.
#[test]
fn the_thin_selection_shapes_run_cleanly() {
    let (mut ctx, registry, _page) = fixture();

    // A one-shape distribute and align (the "nothing to space" path).
    select_tops(&mut ctx, 1);
    dispatch(
        &mut ctx,
        &registry,
        "arrange.distribute.horizontal",
        Args::new(),
    )
    .expect("distribute one");
    dispatch(&mut ctx, &registry, "arrange.align.left", Args::new()).expect("align one");

    // A group copy and duplicate (the descendants path).
    let group = first_group(&ctx);
    ctx.selection.shapes = vec![group];
    ctx.selection.anchor = Some(group);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy the group");
    dispatch(
        &mut ctx,
        &registry,
        "edit.paste",
        numbers(&[("x", 6.0), ("y", 6.0)]),
    )
    .expect("paste the group");
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate the group");

    // A nested cut (a child leaves its group in one step).
    let child = ctx
        .active_document()
        .and_then(|state| state.document.shape(group))
        .and_then(|entry| entry.children.first().copied())
        .expect("a child");
    ctx.selection.shapes = vec![child];
    ctx.selection.anchor = Some(child);
    ctx.selection.page = ctx.active_document().and_then(|state| state.active_page);
    dispatch(&mut ctx, &registry, "edit.cut", Args::new()).expect("cut the child");
}
