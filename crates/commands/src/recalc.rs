//! The recalc hook after an edit (ADR 0056).
//!
//! Every document change goes through [`AppCtx::push_edit`] (ADR 0040).
//! Phase 6 adds one step to that seam: recalculate the cells the edit
//! dirtied and their dependents (ADR 0055), and record what the pass
//! changed in the same undo step, so undo puts the old values back.
//! Recalc never opens a second undo path and never fails the document:
//! a cell it cannot compute keeps its cached value and one warning line
//! is recorded ([`RecalcReport`]).
//!
//! The pass runs over the step's dirty cells after `Edit::apply` and
//! again on every re-apply (the redo that follows a revert). `revert`
//! puts the pass's writes back **by snapshot** (`RecalcEdit`), never by
//! recomputation: the pre-edit value of a cell (an unknown function's
//! cached value, a cycle's kept values) is what the user had, and only
//! the snapshot restores it unchanged.
//!
//! What each writer owns follows ADR 0056's split: a geometry cell whose
//! formula computes from other cells belongs to the pass
//! ([`formula_computes`]); a literal cell is scaled by the editor (the
//! `GeometryEdit` of a resize). The two never write the same cell.

use std::collections::BTreeSet;

use larrygram_model::{
    Cell, CellValue, Document, PageId, RowTarget, Section, ShapeId, Sheet, SheetRef,
};
use larrygram_shapesheet::{
    CellAddress, CellRef, DepGraph, DocumentEngine, Evaluation, Expr, ExprKind, Formula,
    FormulaEngine, Literal, RecalcOptions, RecalcOutcome, RecalcWarning, RecalcWarningKind,
    UnaryOp, parse_formula, recalc_with,
};

use crate::app::{AppCtx, SessionId};
use crate::error::CommandError;
use crate::undo::Edit;

/// What one recalc pass changed (ADR 0056).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecalcReport {
    /// How many cells got a new cached value.
    pub updated: usize,
    /// What the pass could not do (unknown functions, cycles, budget).
    pub warnings: Vec<RecalcWarning>,
    /// True when the pass stopped at its budget and left cells stale.
    pub truncated: bool,
}

/// What one pass run left behind: its report (or the engine error) and
/// the snapshot of every sheet it wrote.
///
/// The snapshots come back **even when the pass fails mid-way** (P6-12
/// review F13): the writes a partial pass made are undone by restoring
/// them, never by dropping them with the error.
struct PassRun {
    /// What the pass reported, or the engine error that stopped it.
    report: Result<RecalcReport, CommandError>,
    /// The sheets it wrote, in their pre-pass state.
    before: Vec<SheetSnapshot>,
}

/// The options of the edit pass (ADR 0056): recompute the dirty cells
/// and their dependents, materialising an inherited formula onto every
/// cell the pass recomputes.
fn edit_options() -> RecalcOptions {
    RecalcOptions {
        materialise_inherited: true,
        ..RecalcOptions::new()
    }
}

/// The options of the fill-in pass (ADR 0057): only cells with a formula
/// and no cached value.
fn fill_in_options() -> RecalcOptions {
    RecalcOptions {
        fill_missing_only: true,
        ..RecalcOptions::new()
    }
}

/// Recalculates the cells an edit dirtied and their dependents.
///
/// Runs after the edit's cells are written and before the undo step is
/// pushed; `session` names the document the edit changed. A cell the pass
/// cannot compute keeps its cached value and one warning line lands on
/// [`AppCtx::warnings`] (never a failure of the document).
///
/// The document keeps its dependency graph on the session (ADR 0055), so
/// a pass re-links only the nodes it touches.
pub fn recalc_after_edit(
    ctx: &mut AppCtx,
    session: SessionId,
    dirty: &[CellAddress],
) -> Result<RecalcReport, CommandError> {
    run_pass(ctx, session, dirty, &edit_options()).report
}

/// Recalculates every cell that has a formula and no cached value (ADR
/// 0057's fill-in pass, consumed by task P6-08 after a read).
///
/// Not an edit: the pass takes no undo entry and records nothing on the
/// undo stack. Cells that carry a cached value are untouched.
pub fn fill_missing(ctx: &mut AppCtx, session: SessionId) -> Result<RecalcReport, CommandError> {
    fill_missing_with(ctx, session, fill_in_options())
}

/// [`fill_missing`] with caller-chosen options (task P6-08 sets
/// `materialise_inherited` when the open pass should materialise too).
pub fn fill_missing_with(
    ctx: &mut AppCtx,
    session: SessionId,
    options: RecalcOptions,
) -> Result<RecalcReport, CommandError> {
    let dirty = {
        let Some(slot) = ctx.documents.iter().find(|slot| slot.id == session) else {
            return Err(CommandError::failed("the document session is gone"));
        };
        document_addresses(&slot.document)
    };
    run_pass(ctx, session, &dirty, &options).report
}

// ---------------------------------------------------------------------------
// The undo step: one edit plus the pass's snapshot (ADR 0056).
// ---------------------------------------------------------------------------

/// The exact entries of one sheet before a pass wrote to it.
///
/// The snapshot is per sheet, not per cell: a pass writes the cell it
/// evaluates, the derived cells [`Document::write_cell`] refreshes beside
/// it (P6-06 open issue 3) and any `SETF` target, and the first sight of
/// a sheet always comes before the first write to it. Restoring the whole
/// sheet therefore covers every write, including the side effects that
/// `RecalcOutcome::updates` does not list.
#[derive(Debug, Clone, PartialEq)]
struct SheetSnapshot {
    /// The sheet.
    sheet: SheetRef,
    /// Its direct and row cells before the pass.
    cells: Vec<Cell>,
    /// Its sections (the geometry rows a resize recomputes) before the pass.
    sections: Vec<Section>,
}

/// One undo entry: the step plus the snapshot of everything the recalc
/// pass changed (ADR 0056: "one user action is one undo step").
///
/// `apply` re-runs the step and the pass (the pass again after a revert:
/// every redo recomputes from the step's own writes). `revert` restores
/// the snapshot taken before that pass and then reverses the step, so the
/// document comes back exactly as it was.
pub(crate) struct RecalcEdit {
    session: SessionId,
    dirty: Vec<CellAddress>,
    inner: Box<dyn Edit>,
    before: Vec<SheetSnapshot>,
}

impl RecalcEdit {
    /// Wraps a completed step: the pass runs now, over the step's dirty
    /// cells, and the entry records what it changed.
    ///
    /// A step that writes no cell is pushed as it is (nothing to
    /// recompute). A broken engine is one warning, never a lost step;
    /// the snapshots a failed pass already took are kept (P6-12 review
    /// F13), so undo removes its partial writes.
    pub(crate) fn wrap(
        ctx: &mut AppCtx,
        session: SessionId,
        inner: Box<dyn Edit>,
    ) -> Result<Box<dyn Edit>, CommandError> {
        let mut engine = SnapshotEngine::new(DocumentEngine);
        Self::wrap_with(ctx, session, inner, &mut engine)
    }

    /// [`Self::wrap`] over a caller-chosen engine: the seam the tests
    /// drive with a failing engine (the product engine cannot fail on
    /// document data).
    fn wrap_with<E: FormulaEngine>(
        ctx: &mut AppCtx,
        session: SessionId,
        inner: Box<dyn Edit>,
        engine: &mut SnapshotEngine<E>,
    ) -> Result<Box<dyn Edit>, CommandError> {
        let dirty: Vec<CellAddress> = inner.dirty_cells().to_vec();
        if dirty.is_empty() {
            return Ok(inner);
        }
        let run = run_pass_with(ctx, session, &dirty, &edit_options(), engine);
        if let Err(error) = &run.report {
            ctx.record_warning(format!("recalculation failed: {error}"));
        }
        Ok(Box::new(Self {
            session,
            dirty,
            inner,
            before: run.before,
        }))
    }

    /// Puts the pass's writes back (ADR 0056: by snapshot, not by
    /// recomputation). An edit that outlived its document reports the
    /// gone session, the same as the step types do.
    fn restore(&self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let doc = session_document(ctx, self.session)?;
        for snapshot in &self.before {
            let sheet = match snapshot.sheet {
                SheetRef::Shape(id) => doc.shape_mut(id).map(|shape| &mut shape.sheet),
                SheetRef::Page(id) => doc
                    .pages
                    .get_mut(id.index as usize)
                    .map(|page| &mut page.sheet),
            };
            if let Some(sheet) = sheet {
                sheet.cells = snapshot.cells.clone();
                sheet.sections = snapshot.sections.clone();
            }
        }
        Ok(())
    }
}

impl Edit for RecalcEdit {
    fn label(&self) -> &str {
        self.inner.label()
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        self.inner.dirty_cells()
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.inner.apply(ctx)?;
        let run = run_pass(ctx, self.session, &self.dirty.clone(), &edit_options());
        if let Err(error) = &run.report {
            ctx.record_warning(format!("recalculation failed: {error}"));
        }
        // Even a failed pass keeps its snapshots (P6-12 review F13): a
        // later revert removes the writes it managed to make.
        self.before = run.before;
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.restore(ctx)?;
        self.inner.revert(ctx)
    }
}

// ---------------------------------------------------------------------------
// The pass
// ---------------------------------------------------------------------------

/// The session's document, or [`CommandError::Failed`] when the session
/// is gone (an edit outlived its document).
fn session_document(ctx: &mut AppCtx, session: SessionId) -> Result<&mut Document, CommandError> {
    ctx.documents
        .iter_mut()
        .find(|slot| slot.id == session)
        .map(|slot| &mut slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))
}

/// Runs one recalc pass over `dirty` and returns what it changed plus the
/// pre-pass snapshot of every sheet it wrote (ADR 0055).
fn run_pass(
    ctx: &mut AppCtx,
    session: SessionId,
    dirty: &[CellAddress],
    options: &RecalcOptions,
) -> PassRun {
    run_pass_with(
        ctx,
        session,
        dirty,
        options,
        &mut SnapshotEngine::new(DocumentEngine),
    )
}

/// [`run_pass`] over a caller-chosen engine (the failure seam the tests
/// drive).
fn run_pass_with<E: FormulaEngine>(
    ctx: &mut AppCtx,
    session: SessionId,
    dirty: &[CellAddress],
    options: &RecalcOptions,
    engine: &mut SnapshotEngine<E>,
) -> PassRun {
    let Some(slot) = ctx.documents.iter_mut().find(|slot| slot.id == session) else {
        return PassRun {
            report: Err(CommandError::failed("the document session is gone")),
            before: Vec::new(),
        };
    };
    // The graph is built on first use and kept on the session (ADR
    // 0055). `DepGraph::build` never fails a document: its `Result`
    // keeps P6-01's interface, and a hypothetical failure would leave
    // the pass an empty graph (nothing recomputed), never a failed step.
    let graph = slot
        .graph
        .get_or_insert_with(|| DepGraph::build(&slot.document).unwrap_or_default());
    let outcome = match recalc_with(&mut slot.document, dirty, options, graph, engine) {
        Ok(outcome) => outcome,
        Err(error) => {
            // A broken engine mid-pass: the snapshots taken so far come
            // back with the error (P6-12 review F13).
            return PassRun {
                report: Err(engine_error(error)),
                before: std::mem::take(&mut engine.before),
            };
        }
    };
    let report = report_of(&outcome);
    for warning in &outcome.warnings {
        ctx.record_warning(warning_line(warning));
    }
    PassRun {
        report: Ok(report),
        before: std::mem::take(&mut engine.before),
    }
}

/// The report of one pass outcome.
fn report_of(outcome: &RecalcOutcome) -> RecalcReport {
    RecalcReport {
        updated: outcome.updates.len(),
        warnings: outcome.warnings.clone(),
        truncated: outcome.truncated,
    }
}

/// Maps an engine failure (the engine broken, not the document).
fn engine_error(error: larrygram_shapesheet::Error) -> CommandError {
    CommandError::failed(error.to_string())
}

/// One warning line for the status bar.
fn warning_line(warning: &RecalcWarning) -> String {
    let target = warning
        .target
        .as_ref()
        .map(|address| format!(" of {}", address.cell.as_str()))
        .unwrap_or_default();
    match &warning.kind {
        RecalcWarningKind::UnknownFunction(name) => {
            format!("recalc: unknown function {name}{target}; the value was kept")
        }
        RecalcWarningKind::MissingReference(name) => {
            format!("recalc: missing reference {name}{target}")
        }
        RecalcWarningKind::Cycle(cells) => {
            let names: Vec<&str> = cells.iter().map(|cell| cell.cell.as_str()).collect();
            format!(
                "recalc: dependency cycle between {}; the cached values were kept",
                names.join(", ")
            )
        }
        RecalcWarningKind::BudgetExceeded => {
            String::from("recalc: evaluation budget exceeded; some cells are stale")
        }
        RecalcWarningKind::WrongArgumentCount {
            function,
            min,
            max,
            got,
        } => {
            let span = match max {
                Some(max) if max == min => format!("exactly {min}"),
                Some(max) => format!("{min} to {max}"),
                None => format!("at least {min}"),
            };
            let noun = if *got == 1 { "argument" } else { "arguments" };
            format!(
                "recalc: {function} passed {got} {noun}, expected {span}{target}; the call evaluated to #VALUE!"
            )
        }
        RecalcWarningKind::SetfRefused(name) => {
            format!("recalc: SETF refused {name}{target}; the call evaluated to #REF!")
        }
        RecalcWarningKind::Coercion(why) => {
            format!("recalc: a value had to be coerced ({why}){target}")
        }
        RecalcWarningKind::NonFinite => {
            format!("recalc: a number was not finite{target}; the value became #NUM!")
        }
        RecalcWarningKind::Syntax(kind) => {
            format!("recalc: the formula does not parse ({kind}){target}; the value was kept")
        }
        // `RecalcWarningKind` is `#[non_exhaustive]`.
        _ => format!("recalc: warning{target}"),
    }
}

/// The [`FormulaEngine`] that snapshots every sheet before the first
/// write to it (the pass writes through the model's mutation API, so the
/// sight of a write's target is the last moment the sheet is pristine).
struct SnapshotEngine<E> {
    inner: E,
    seen: BTreeSet<(u8, u32)>,
    before: Vec<SheetSnapshot>,
}

impl<E> SnapshotEngine<E> {
    /// An engine that snapshots around `inner`.
    fn new(inner: E) -> Self {
        Self {
            inner,
            seen: BTreeSet::new(),
            before: Vec::new(),
        }
    }

    /// Records the sheet's entries once, before any write to it.
    fn snapshot(&mut self, document: &Document, sheet: SheetRef) {
        let key = match sheet {
            SheetRef::Page(id) => (0, id.index),
            SheetRef::Shape(id) => (1, id.index),
        };
        if !self.seen.insert(key) {
            return;
        }
        let entry = match sheet {
            SheetRef::Shape(id) => document
                .shape(id)
                .map(|shape| (shape.sheet.cells.clone(), shape.sheet.sections.clone())),
            SheetRef::Page(id) => document
                .pages
                .get(id.index as usize)
                .map(|page| (page.sheet.cells.clone(), page.sheet.sections.clone())),
        };
        if let Some((cells, sections)) = entry {
            self.before.push(SheetSnapshot {
                sheet,
                cells,
                sections,
            });
        }
    }
}

impl<E: FormulaEngine> FormulaEngine for SnapshotEngine<E> {
    fn references(
        &mut self,
        formula: &Formula,
    ) -> Result<Vec<CellRef>, larrygram_shapesheet::Error> {
        self.inner.references(formula)
    }

    fn evaluate(
        &mut self,
        document: &mut Document,
        formula: &Formula,
        sheet: SheetRef,
    ) -> Result<Evaluation, larrygram_shapesheet::Error> {
        self.snapshot(document, sheet);
        let evaluation = self.inner.evaluate(document, formula, sheet)?;
        for write in &evaluation.writes {
            self.snapshot(document, write.target.sheet);
        }
        Ok(evaluation)
    }
}

// ---------------------------------------------------------------------------
// Dirty addresses
// ---------------------------------------------------------------------------

/// Pushes the addresses of every cell of one sheet (direct, section and
/// row cells) into `out`, in sheet order.
fn push_sheet_addresses(sheet: SheetRef, data: &Sheet, out: &mut Vec<CellAddress>) {
    for cell in &data.cells {
        out.push(CellAddress::new(sheet, cell.name.clone()));
    }
    for section in &data.sections {
        for cell in &section.cells {
            out.push(CellAddress {
                sheet,
                section: Some(section.name.clone()),
                row: None,
                cell: cell.name.clone(),
            });
        }
        for (position, row) in section.rows.iter().enumerate() {
            let target = RowTarget {
                position: Some(position),
                ix: row.ix,
            };
            for cell in &row.cells {
                out.push(CellAddress::in_row(
                    sheet,
                    section.name.clone(),
                    target,
                    cell.name.clone(),
                ));
            }
        }
    }
}

/// The addresses of every cell of `root`'s subtree (an insert or delete
/// dirties the cells it adds or removes, ADR 0056). The walk is iterative
/// and bounded (AGENTS.md section 6).
pub(crate) fn subtree_addresses(doc: &Document, root: ShapeId) -> Vec<CellAddress> {
    const MAX_WALK: usize = 1 << 16;
    let mut out = Vec::new();
    let mut stack = vec![root];
    let mut seen: BTreeSet<(u8, u32)> = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if seen.len() >= MAX_WALK || !seen.insert((1, id.index)) {
            continue;
        }
        if let Some(shape) = doc.shape(id) {
            push_sheet_addresses(SheetRef::Shape(id), &shape.sheet, &mut out);
            stack.extend(shape.children.iter().copied());
        }
    }
    out
}

/// The addresses of every cell of every reachable sheet (the fill-in
/// pass's dirty set).
fn document_addresses(doc: &Document) -> Vec<CellAddress> {
    let mut out = Vec::new();
    for (index, page) in doc.pages.iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let sheet = SheetRef::Page(PageId { index });
        push_sheet_addresses(sheet, &page.sheet, &mut out);
        for shape in &page.shapes {
            out.extend(subtree_addresses(doc, *shape));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Formula classification (ADR 0056's writer split and the `GUARD` rule)
// ---------------------------------------------------------------------------

/// Whether `formula` computes its value from other cells.
///
/// This is ADR 0056's ownership split: a computing formula (a reference,
/// an operator, a function call, `Inh`, or text the parser refuses) is
/// recomputed by the pass; a bare literal (or no formula at all) is
/// constant geometry the editor scales. The Phase 5 router writes literal
/// formulas on the paths it owns (ADR 0045), so a literal formula keeps
/// the router (and the resize scaler) as its writer.
pub fn formula_computes(formula: &str) -> bool {
    match parse_formula(&Formula::new(formula)) {
        Ok(expr) => reads_or_calls(&expr),
        Err(_) => true,
    }
}

/// Whether the tree reads another cell or calls a function: the formula
/// computes its value. A literal (even `-2` or `2 + 3` over literals) is
/// constant. The walk is iterative; an unknown node computes (the
/// conservative answer).
fn reads_or_calls(expr: &Expr) -> bool {
    let mut stack = vec![expr];
    while let Some(next) = stack.pop() {
        match &next.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Ref(_) | ExprKind::Call { .. } => return true,
            ExprKind::Unary { operand, .. } => stack.push(operand),
            ExprKind::Binary { left, right, .. } => {
                stack.push(right);
                stack.push(left);
            }
            _ => return true,
        }
    }
    false
}

/// Whether the formula wraps its value in `GUARD` (ADR 0056): a
/// programmatic write is skipped for such a cell, a direct user edit
/// replaces it. Text the parser refuses falls back to a textual scan.
pub fn formula_is_guarded(formula: &str) -> bool {
    match parse_formula(&Formula::new(formula)) {
        Ok(expr) => {
            let mut stack = vec![&expr];
            while let Some(next) = stack.pop() {
                match &next.kind {
                    ExprKind::Call { name, args } => {
                        if name.eq_ignore_ascii_case("GUARD") {
                            return true;
                        }
                        stack.extend(args.iter());
                    }
                    ExprKind::Unary { operand, .. } => stack.push(operand),
                    ExprKind::Binary { left, right, .. } => {
                        stack.push(right);
                        stack.push(left);
                    }
                    _ => {}
                }
            }
            false
        }
        Err(_) => formula.to_ascii_lowercase().contains("guard("),
    }
}

/// Whether the shape's geometry rows carry computing formulas (ADR 0056:
/// such a connector is owned by recalc and the router skips it).
pub fn geometry_is_formula_driven(doc: &Document, shape: ShapeId) -> bool {
    let Some(entity) = doc.shape(shape) else {
        return false;
    };
    entity
        .sheet
        .sections
        .iter()
        .filter(|section| section.name.as_str() == "Geometry")
        .any(|section| {
            section.rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| cell.formula.as_deref().is_some_and(formula_computes))
            })
        })
}

/// The number a cell's formula spells when it is a bare numeric literal,
/// or the cell's cached number when it has no formula: the constant the
/// editor scales (ADR 0056). A formula that computes answers `None`.
pub(crate) fn literal_number(formula: Option<&str>, value: Option<&CellValue>) -> Option<f64> {
    match formula {
        None => match value {
            Some(CellValue::Number(number)) if number.is_finite() => Some(*number),
            _ => None,
        },
        Some(text) => match parse_formula(&Formula::new(text)) {
            Ok(expr) => constant_number(&expr),
            Err(_) => None,
        },
    }
}

/// The number a constant expression spells: a numeric literal, signed or
/// not. Anything that reads a cell or calls a function answers `None`.
fn constant_number(expr: &Expr) -> Option<f64> {
    match &expr.kind {
        ExprKind::Literal(Literal::Number(literal)) if literal.value.is_finite() => {
            Some(literal.value)
        }
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
        } => constant_number(operand).map(|value| -value),
        ExprKind::Unary {
            op: UnaryOp::Plus,
            operand,
        } => constant_number(operand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DocumentSession, Settings};
    use larrygram_model::{CellName, Preserved, Row, SectionName, Shape};

    fn cell(name: &str, formula: Option<&str>, value: Option<f64>) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: formula.map(str::to_owned),
            value: value.map(CellValue::Number),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        }
    }

    /// The identity of a session that was never opened.
    fn ghost_session() -> SessionId {
        DocumentSession::new(Document::new(), None).id
    }

    /// A one-shape document: `PinX = Width*0.5` and one geometry row
    /// `X = Width`, both with stale cached values.
    fn fixture() -> (AppCtx, SessionId, ShapeId) {
        let mut ctx = AppCtx::new(Settings::default());
        let session = DocumentSession::new(Document::new(), None);
        let id = session.id;
        ctx.open_document(session);
        let shape = {
            let doc = &mut ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == id)
                .expect("session")
                .document;
            let page = doc.add_page("Page-1").expect("page");
            let mut shape = Shape::new(ShapeId { index: 0 }, 1);
            shape.sheet.cells = vec![
                cell("Width", Some("2"), Some(2.0)),
                cell("PinX", Some("Width*0.5"), Some(9.0)),
            ];
            let mut geometry = Section::new(SectionName::from_static("Geometry"));
            let mut row = Row::new();
            row.row_type = Some("LineTo".to_owned());
            row.ix = Some(0);
            row.cells = vec![cell("X", Some("Width"), Some(7.0))];
            geometry.rows.push(row);
            shape.sheet.sections.push(geometry);
            doc.insert_shape(page, shape, None).expect("insert")
        };
        (ctx, id, shape)
    }

    fn number(doc: &Document, shape: ShapeId, cell_name: &str) -> Option<f64> {
        doc.shape(shape)?
            .sheet
            .cells
            .iter()
            .find(|entry| entry.name.as_str() == cell_name)
            .and_then(|entry| match entry.value {
                Some(CellValue::Number(value)) => Some(value),
                _ => None,
            })
    }

    #[test]
    fn a_report_starts_empty() {
        let report = RecalcReport::default();
        assert_eq!(report.updated, 0);
        assert!(report.warnings.is_empty());
        assert!(!report.truncated);
    }

    #[test]
    fn an_edit_reports_an_empty_dirty_set_by_default() {
        use crate::edit::CompositeEdit;
        let edit = CompositeEdit::new("label");
        assert!(edit.dirty_cells().is_empty());
    }

    /// The pass recomputes the dirty cell's dependents; a [`SessionId`]
    /// that names no document is an error.
    #[test]
    fn recalc_after_edit_recomputes_dependents_and_rejects_a_gone_session() {
        let (mut ctx, id, shape) = fixture();
        let dirty = [CellAddress::new(
            SheetRef::Shape(shape),
            CellName::new("Width"),
        )];
        let report = recalc_after_edit(&mut ctx, id, &dirty).expect("recalc");
        assert_eq!(report.updated, 2, "PinX and the geometry row recompute");
        let doc = &ctx.active_document().expect("doc").document;
        assert_eq!(number(doc, shape, "PinX"), Some(1.0));
        let row = &doc.shape(shape).expect("shape").sheet.sections[0].rows[0];
        assert_eq!(row.cells[0].value, Some(CellValue::Number(2.0)));

        let error = recalc_after_edit(&mut ctx, ghost_session(), &dirty).expect_err("gone");
        assert!(error.to_string().contains("session is gone"), "{error}");
    }

    /// ADR 0057's fill-in: a formula without a cached value gains one;
    /// a valued cell is untouched and no undo entry appears.
    #[test]
    fn fill_missing_only_fills_the_holes() {
        let (mut ctx, id, shape) = fixture();
        {
            let doc = &mut ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == id)
                .expect("session")
                .document;
            if let Some(entry) = doc.shape_mut(shape).and_then(|shape| {
                shape
                    .sheet
                    .cells
                    .iter_mut()
                    .find(|c| c.name.as_str() == "PinX")
            }) {
                entry.value = None;
            }
        }
        let report = fill_missing(&mut ctx, id).expect("fill");
        assert_eq!(report.updated, 1);
        let doc = &ctx.active_document().expect("doc").document;
        assert_eq!(number(doc, shape, "PinX"), Some(1.0), "the hole is filled");
        assert_eq!(number(doc, shape, "Width"), Some(2.0), "valued cells stay");
        assert!(ctx.active_document().expect("doc").undo.is_empty());

        let error =
            fill_missing_with(&mut ctx, ghost_session(), fill_in_options()).expect_err("gone");
        assert!(error.to_string().contains("session is gone"), "{error}");
    }

    #[test]
    fn formula_classification_splits_the_writers() {
        assert!(!formula_computes("2"));
        assert!(!formula_computes("2.5 in"));
        assert!(!formula_computes("-2"));
        assert!(!formula_computes("2+3"));
        assert!(!formula_computes("\"x\""));
        assert!(formula_computes("Width*0.5"));
        assert!(formula_computes("Inh"));
        assert!(formula_computes("SETF(Width, 2)"));
        assert!(formula_computes("FORMATEX(1,)"), "unparsable text computes");
    }

    #[test]
    fn guard_formulas_are_detected() {
        assert!(formula_is_guarded("GUARD(Width*0.5)"));
        assert!(formula_is_guarded("guard (2)"));
        assert!(formula_is_guarded("IF(1, GUARD(2), 3)"));
        assert!(formula_is_guarded("-GUARD(2)"), "under a unary operator");
        assert!(formula_is_guarded("1+GUARD(2)"), "under a binary operator");
        assert!(formula_is_guarded("= GUARD(1"));
        assert!(!formula_is_guarded("Width*0.5"));
        assert!(!formula_is_guarded("2"));
    }

    #[test]
    fn literal_numbers_come_from_a_formula_or_a_cached_value() {
        assert_eq!(literal_number(Some("2.5"), None), Some(2.5));
        assert_eq!(literal_number(Some("-2.5"), None), Some(-2.5));
        assert_eq!(literal_number(Some("+2.5"), None), Some(2.5));
        assert_eq!(
            literal_number(Some("FORMATEX(1,)"), None),
            None,
            "unparsable"
        );
        assert_eq!(literal_number(Some("2 in"), None), Some(2.0));
        assert_eq!(
            literal_number(None, Some(&CellValue::Number(3.0))),
            Some(3.0)
        );
        assert_eq!(literal_number(Some("Width"), None), None);
        assert_eq!(literal_number(Some("\"x\""), None), None);
        assert_eq!(
            literal_number(None, Some(&CellValue::Str("x".into()))),
            None
        );
    }

    #[test]
    fn formula_geometry_is_detected_on_rows() {
        let (ctx, _id, shape) = fixture();
        let doc = &ctx.active_document().expect("doc").document;
        assert!(geometry_is_formula_driven(doc, shape));
        let (mut ctx, id, shape) = fixture();
        {
            let doc = &mut ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == id)
                .expect("session")
                .document;
            if let Some(entity) = doc.shape_mut(shape) {
                for section in &mut entity.sheet.sections {
                    for row in &mut section.rows {
                        for entry in &mut row.cells {
                            entry.formula = Some(String::from("2"));
                        }
                    }
                }
            }
        }
        let doc = &ctx.active_document().expect("doc").document;
        assert!(!geometry_is_formula_driven(doc, shape));
        assert!(!geometry_is_formula_driven(doc, ShapeId { index: 9 }));
    }

    /// The address builders cover direct, section and row cells.
    #[test]
    fn address_builders_cover_direct_section_and_row_cells() {
        let (ctx, _id, shape) = fixture();
        let doc = &ctx.active_document().expect("doc").document;
        let mut addresses = Vec::new();
        push_sheet_addresses(
            SheetRef::Shape(shape),
            &doc.shape(shape).expect("s").sheet,
            &mut addresses,
        );
        let names: Vec<String> = addresses
            .iter()
            .map(|address| address.cell.as_str().to_owned())
            .collect();
        assert_eq!(names, ["Width", "PinX", "X"]);
        assert!(addresses[2].row.is_some());

        // A section-level cell is addressed with the section and no row.
        let mut sheet = doc.shape(shape).expect("s").sheet.clone();
        sheet.sections[0].cells.push(Cell {
            name: CellName::new("NoFill"),
            formula: Some(String::from("0")),
            value: Some(CellValue::Number(0.0)),
            unit: larrygram_geom::Unit::Unitsless,
            preserved: Preserved::new(),
        });
        let mut with_section_cell = Vec::new();
        push_sheet_addresses(SheetRef::Shape(shape), &sheet, &mut with_section_cell);
        let section_cell = with_section_cell
            .iter()
            .find(|address| address.cell.as_str() == "NoFill")
            .expect("NoFill");
        assert!(section_cell.row.is_none());
        assert!(section_cell.section.is_some());

        let subtree = subtree_addresses(doc, shape);
        assert_eq!(subtree.len(), 3);

        // A cyclic child list stops at the visited set, never loops.
        let mut cyclic = fixture();
        {
            let doc = &mut cyclic
                .0
                .documents
                .iter_mut()
                .find(|slot| slot.id == cyclic.1)
                .expect("session")
                .document;
            if let Some(entity) = doc.shape_mut(cyclic.2) {
                entity.children.push(cyclic.2);
            }
        }
        let doc = &cyclic.0.active_document().expect("doc").document;
        assert_eq!(subtree_addresses(doc, cyclic.2).len(), 3);
        let all = document_addresses(doc);
        assert_eq!(all.len(), 3, "one page sheet and one shape sheet");
        assert!(
            all.iter()
                .all(|address| address.sheet == SheetRef::Shape(shape)),
            "the page sheet is empty, so every address is the shape's"
        );
    }

    /// The snapshot engine records each sheet once, before the first
    /// write to it.
    #[test]
    fn the_snapshot_engine_records_each_sheet_once() {
        let (mut ctx, id, shape) = fixture();
        let dirty = [CellAddress::new(
            SheetRef::Shape(shape),
            CellName::new("Width"),
        )];
        let run = run_pass(&mut ctx, id, &dirty, &edit_options());
        run.report.expect("pass");
        let before = run.before;
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].sheet, SheetRef::Shape(shape));
        let names: Vec<&str> = before[0].cells.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Width", "PinX"]);
    }

    /// One step that reports a fixed dirty set and mutates nothing.
    struct StubEdit(Vec<CellAddress>);

    impl Edit for StubEdit {
        fn label(&self) -> &str {
            "stub"
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn dirty_cells(&self) -> &[CellAddress] {
            &self.0
        }
    }

    /// The page sheet cells the wrap test works over: a stale dependent
    /// and a `SETF` write, so both the page snapshot and the write-target
    /// snapshot are exercised (ADR 0056, P6-06 open issue 3).
    fn page_fixture() -> (AppCtx, SessionId) {
        let (mut ctx, id, _shape) = fixture();
        let doc = &mut ctx
            .documents
            .iter_mut()
            .find(|slot| slot.id == id)
            .expect("session")
            .document;
        let page = doc.pages.first_mut().expect("page");
        page.sheet.cells = vec![
            cell("PageWidth", Some("2"), Some(2.0)),
            cell("PageHeight", Some("PageWidth*2"), Some(9.0)),
            cell("Src", Some("SETF(Tgt, 2)"), Some(0.0)),
            cell("Tgt", Some("0"), Some(0.0)),
        ];
        (ctx, id)
    }

    fn page_number(ctx: &AppCtx, name: &str) -> Option<f64> {
        let doc = &ctx.active_document().expect("doc").document;
        doc.pages
            .first()?
            .sheet
            .cells
            .iter()
            .find(|entry| entry.name.as_str() == name)
            .and_then(|entry| match entry.value {
                Some(CellValue::Number(value)) => Some(value),
                _ => None,
            })
    }

    /// `RecalcEdit` wraps the step and the pass's snapshot: the redo
    /// re-runs the pass, the undo restores the snapshot (page sheets and
    /// `SETF` write targets included).
    #[test]
    fn recalc_edit_wraps_the_step_and_restores_by_snapshot() {
        let (mut ctx, id) = page_fixture();
        let dirty = vec![
            CellAddress::new(
                SheetRef::Page(PageId { index: 0 }),
                CellName::new("PageWidth"),
            ),
            CellAddress::new(SheetRef::Page(PageId { index: 0 }), CellName::new("Src")),
        ];
        let mut wrapped =
            RecalcEdit::wrap(&mut ctx, id, Box::new(StubEdit(dirty.clone()))).expect("wrap");
        assert_eq!(wrapped.dirty_cells().len(), 2, "delegated to the step");
        assert_eq!(page_number(&ctx, "PageHeight"), Some(4.0), "recomputed");
        assert_eq!(page_number(&ctx, "Tgt"), Some(2.0), "the SETF write landed");

        wrapped.revert(&mut ctx).expect("revert");
        assert_eq!(page_number(&ctx, "PageHeight"), Some(9.0), "snapshot back");
        assert_eq!(
            page_number(&ctx, "Tgt"),
            Some(0.0),
            "the SETF target's snapshot comes back too"
        );

        wrapped.apply(&mut ctx).expect("apply runs the pass again");
        assert_eq!(page_number(&ctx, "PageHeight"), Some(4.0));
        assert_eq!(page_number(&ctx, "Tgt"), Some(2.0));
    }

    /// A wrap whose pass cannot run keeps the step and warns once; a
    /// later `apply`/`revert` warns again and never fails the step.
    #[test]
    fn a_wrap_that_cannot_recalculate_keeps_the_step() {
        let (mut ctx, id, shape) = fixture();
        let dirty = [CellAddress::new(
            SheetRef::Shape(shape),
            CellName::new("Width"),
        )];
        let mut wrapped = RecalcEdit::wrap(
            &mut ctx,
            ghost_session(),
            Box::new(StubEdit(dirty.to_vec())),
        )
        .expect("wrap");
        assert_eq!(ctx.warnings.len(), 1, "{:?}", ctx.warnings);
        assert!(
            ctx.warnings[0].contains("recalculation failed"),
            "{:?}",
            ctx.warnings
        );
        assert_eq!(wrapped.dirty_cells().len(), 1);
        let error = wrapped.revert(&mut ctx).expect_err("the session is gone");
        assert!(error.to_string().contains("session is gone"), "{error}");
        assert_eq!(ctx.warnings.len(), 1);

        // The same for a re-apply against a session that went away.
        let mut wrapped =
            RecalcEdit::wrap(&mut ctx, id, Box::new(StubEdit(dirty.to_vec()))).expect("wrap");
        wrapped
            .apply(&mut ctx)
            .expect("a broken pass is a warning, not a failure");
        assert!(
            ctx.warnings
                .iter()
                .any(|line| line.contains("recalculation failed")
                    || line.contains("unknown function FOO")),
            "{:?}",
            ctx.warnings
        );
    }

    /// A `SETF` inside a referenced cell's formula is applied by the
    /// pass, so the snapshot `RecalcEdit` takes covers it and undo
    /// restores the target (P6-12 review F1).
    #[test]
    fn a_nested_setf_write_is_inside_the_undo_snapshot() {
        let (mut ctx, id, shape) = fixture();
        {
            let doc = &mut ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == id)
                .expect("session")
                .document;
            if let Some(entity) = doc.shape_mut(shape) {
                entity.sheet.cells = vec![
                    cell("Width", None, Some(0.0)),
                    cell("Mid", Some("SETF(Width,5)+1"), None),
                    cell("Top", Some("Mid*2"), Some(0.0)),
                ];
            }
        }
        let dirty = vec![CellAddress::new(
            SheetRef::Shape(shape),
            CellName::new("Top"),
        )];
        let mut wrapped = RecalcEdit::wrap(&mut ctx, id, Box::new(StubEdit(dirty))).expect("wrap");
        let doc = &ctx.active_document().expect("doc").document;
        assert_eq!(number(doc, shape, "Width"), Some(5.0), "the write landed");
        assert_eq!(number(doc, shape, "Top"), Some(12.0), "the read saw it");

        wrapped.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("doc").document;
        assert_eq!(
            number(doc, shape, "Width"),
            Some(0.0),
            "undo restored the SETF target"
        );
        assert_eq!(number(doc, shape, "Top"), Some(0.0));
    }

    /// The write-then-restore round trip for a section-level and a row
    /// `SETF` target (the coverage audit's remaining half of that
    /// cluster): the per-sheet snapshot covers sections and rows.
    #[test]
    fn a_setf_into_a_section_and_a_row_restores_with_the_snapshot() {
        let (mut ctx, id, shape) = fixture();
        {
            let doc = &mut ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == id)
                .expect("session")
                .document;
            let sheet = &mut doc.shape_mut(shape).expect("shape").sheet;
            let geometry = sheet.sections.first_mut().expect("geometry");
            geometry.cells.push(cell("NoFill", None, Some(0.0)));
            let mut row = Row::new();
            row.ix = Some(1);
            row.cells = vec![cell("X", None, Some(0.0))];
            geometry.rows.push(row);
            sheet
                .cells
                .push(cell("S", Some("SETF(Geometry1.NoFill, 3)"), None));
            sheet
                .cells
                .push(cell("R", Some("SETF(Geometry1.X1, 4)"), None));
        }
        let dirty = vec![
            CellAddress::new(SheetRef::Shape(shape), CellName::new("S")),
            CellAddress::new(SheetRef::Shape(shape), CellName::new("R")),
        ];
        let mut wrapped = RecalcEdit::wrap(&mut ctx, id, Box::new(StubEdit(dirty))).expect("wrap");
        {
            let doc = &ctx.active_document().expect("doc").document;
            let geometry = doc
                .shape(shape)
                .expect("shape")
                .sheet
                .sections
                .first()
                .expect("geometry");
            assert_eq!(
                geometry.cells.first().expect("NoFill").value,
                Some(CellValue::Number(3.0)),
                "the section-level write landed"
            );
            assert_eq!(
                geometry.rows.get(1).expect("row 1").cells[0].value,
                Some(CellValue::Number(4.0)),
                "the row write landed"
            );
        }

        wrapped.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("doc").document;
        let geometry = doc
            .shape(shape)
            .expect("shape")
            .sheet
            .sections
            .first()
            .expect("geometry");
        assert_eq!(
            geometry.cells.first().expect("NoFill").value,
            Some(CellValue::Number(0.0)),
            "the section-level snapshot comes back"
        );
        assert_eq!(
            geometry.rows.get(1).expect("row 1").cells[0].value,
            Some(CellValue::Number(0.0)),
            "the row snapshot comes back"
        );
    }

    /// An engine that computes one evaluation and then fails (the
    /// failure seam the product engine cannot reach).
    struct FailAfterOne {
        done: bool,
    }

    impl FormulaEngine for FailAfterOne {
        fn references(
            &mut self,
            _formula: &Formula,
        ) -> Result<Vec<CellRef>, larrygram_shapesheet::Error> {
            Ok(Vec::new())
        }

        fn evaluate(
            &mut self,
            _document: &mut Document,
            _formula: &Formula,
            _sheet: SheetRef,
        ) -> Result<Evaluation, larrygram_shapesheet::Error> {
            if self.done {
                return Err(larrygram_shapesheet::Error::Conversion(
                    "test engine failed",
                ));
            }
            self.done = true;
            Ok(Evaluation {
                value: CellValue::Number(7.0),
                kept_cached: false,
                warnings: Vec::new(),
                writes: Vec::new(),
                evaluations: 1,
            })
        }
    }

    /// A pass that fails mid-way keeps the snapshots it already took
    /// (P6-12 review F13): undo removes the partial writes.
    #[test]
    fn a_pass_that_fails_mid_way_keeps_its_snapshots_for_undo() {
        let (mut ctx, id, shape) = fixture();
        let dirty = vec![
            CellAddress::new(SheetRef::Shape(shape), CellName::new("PinX")),
            CellAddress::new(SheetRef::Shape(shape), CellName::new("Width")),
        ];
        let mut engine = SnapshotEngine::new(FailAfterOne { done: false });
        let mut wrapped =
            RecalcEdit::wrap_with(&mut ctx, id, Box::new(StubEdit(dirty)), &mut engine)
                .expect("wrap");
        assert!(
            ctx.warnings
                .iter()
                .any(|line| line.contains("recalculation failed")),
            "{:?}",
            ctx.warnings
        );
        {
            let doc = &ctx.active_document().expect("doc").document;
            assert_eq!(
                number(doc, shape, "PinX"),
                Some(7.0),
                "the first evaluation wrote before the failure"
            );
        }

        wrapped.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("doc").document;
        assert_eq!(
            number(doc, shape, "PinX"),
            Some(9.0),
            "the kept snapshot removes the partial write"
        );
    }

    /// The redo arm of `RecalcEdit::apply` whose re-run fails: the step
    /// is kept and one warning records the failure (ADR 0056's "one
    /// user action is one undo step" includes the redo direction).
    #[test]
    fn a_redo_whose_pass_fails_keeps_the_step_and_warns() {
        let (mut ctx, _id, shape) = fixture();
        let dirty = [CellAddress::new(
            SheetRef::Shape(shape),
            CellName::new("Width"),
        )];
        let mut wrapped = RecalcEdit::wrap(
            &mut ctx,
            ghost_session(),
            Box::new(StubEdit(dirty.to_vec())),
        )
        .expect("wrap");
        assert_eq!(ctx.warnings.len(), 1, "{:?}", ctx.warnings);

        wrapped
            .apply(&mut ctx)
            .expect("a failed pass is a warning, not a failure");
        assert_eq!(ctx.warnings.len(), 2, "{:?}", ctx.warnings);
        assert!(
            ctx.warnings[1].contains("recalculation failed"),
            "{:?}",
            ctx.warnings
        );
        assert_eq!(wrapped.dirty_cells().len(), 1, "the step is kept");
        assert_eq!(wrapped.label(), "stub");
    }

    #[test]
    fn warning_lines_name_the_problem() {
        use larrygram_shapesheet::SyntaxKind;
        let address = CellAddress::new(SheetRef::Shape(ShapeId { index: 0 }), CellName::new("X"));
        for (kind, text) in [
            (
                RecalcWarningKind::UnknownFunction(String::from("FOO")),
                "unknown function FOO of X",
            ),
            (
                RecalcWarningKind::MissingReference(String::from("Sheet!Cell")),
                "missing reference Sheet!Cell",
            ),
            (
                RecalcWarningKind::Cycle(vec![address.clone()]),
                "dependency cycle between X",
            ),
            (RecalcWarningKind::BudgetExceeded, "budget exceeded"),
            (
                RecalcWarningKind::WrongArgumentCount {
                    function: String::from("ABS"),
                    min: 1,
                    max: Some(1),
                    got: 2,
                },
                "ABS passed 2 arguments, expected exactly 1 of X",
            ),
            (
                RecalcWarningKind::WrongArgumentCount {
                    function: String::from("INDEX"),
                    min: 2,
                    max: Some(3),
                    got: 4,
                },
                "INDEX passed 4 arguments, expected 2 to 3",
            ),
            (
                RecalcWarningKind::WrongArgumentCount {
                    function: String::from("ROUND"),
                    min: 2,
                    max: Some(2),
                    got: 1,
                },
                "ROUND passed 1 argument, expected exactly 2",
            ),
            (
                RecalcWarningKind::WrongArgumentCount {
                    function: String::from("SUM"),
                    min: 1,
                    max: None,
                    got: 0,
                },
                "SUM passed 0 arguments, expected at least 1",
            ),
            (
                RecalcWarningKind::SetfRefused(String::from("Nope")),
                "SETF refused Nope of X",
            ),
            (
                RecalcWarningKind::Coercion(String::from("sheep")),
                "coerced (sheep) of X",
            ),
            (RecalcWarningKind::NonFinite, "a number was not finite of X"),
            (
                RecalcWarningKind::Syntax(SyntaxKind::UnexpectedEnd),
                "the formula does not parse",
            ),
        ] {
            let line = warning_line(&RecalcWarning {
                target: Some(address.clone()),
                kind,
            });
            assert!(line.contains(text), "{line} should contain {text}");
        }
        let line = warning_line(&RecalcWarning {
            target: None,
            kind: RecalcWarningKind::MissingReference(String::from("x")),
        });
        assert!(line.contains("missing reference x"), "{line}");
    }
}
