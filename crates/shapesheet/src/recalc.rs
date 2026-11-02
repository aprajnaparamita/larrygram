//! The recalculation pass (ADR 0055).
//!
//! [`recalc`] recomputes the cells a change dirtied and their transitive
//! dependents, and reports what it changed ([`CellUpdate`]) and what it
//! could not ([`RecalcWarning`]): an unknown function keeps the cached
//! value, a cut cycle keeps the values it found, a budget run-out leaves
//! the rest stale. A recalc never fails the document and never panics.
//!
//! The pass drives a `FormulaEngine`: production runs `DocumentEngine`
//! (`parse`, `eval`, `DocumentResolver`); `recalc_with` takes the
//! engine and the [`DepGraph`] of the session so an edit refreshes only
//! the edges it touched (ADR 0055). What the pass writes follows
//! ADR 0056: a cell with its own formula gets its cached value updated,
//! an inherited cell gets the evaluated formula and value materialised
//! when [`RecalcOptions::materialise_inherited`] says so.

use std::collections::{BTreeSet, VecDeque};

use larrygram_model::{CellValue, CellWrite, Document, SheetRef};

use crate::Error;
use crate::ast::{Expr, ExprKind};
use crate::error::SyntaxKind;
use crate::eval::{EvalWarning, EvalWarningKind};
use crate::formula::{CellRef, Formula};
use crate::graph::{self, CellAddress, DepGraph, EffectiveCell};
use crate::resolve::DocumentResolver;
use crate::value::Value;

/// How hard a recalc pass may work (ADR 0055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecalcOptions {
    /// The most cell evaluations one pass may run. Exceeding it sets
    /// [`RecalcOutcome::truncated`] and leaves the rest stale.
    pub max_evaluations: u32,
    /// When true, a cell that inherits its formula gets its own copy of
    /// the evaluated formula and value (ADR 0056, resize).
    pub materialise_inherited: bool,
    /// When true, the pass recomputes only cells that have a formula and
    /// no cached value (ADR 0057, the fill-in pass after a read).
    pub fill_missing_only: bool,
}

impl RecalcOptions {
    /// The default budget of one pass (ADR 0055).
    pub const DEFAULT_MAX_EVALUATIONS: u32 = 100_000;

    /// Options with the default budget and no materialisation.
    pub const fn new() -> Self {
        Self {
            max_evaluations: Self::DEFAULT_MAX_EVALUATIONS,
            materialise_inherited: false,
            fill_missing_only: false,
        }
    }
}

impl Default for RecalcOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// One cell a recalc pass changed (ADR 0055).
#[derive(Debug, Clone, PartialEq)]
pub struct CellUpdate {
    /// The cell that was recomputed.
    pub target: CellAddress,
    /// The cached value before the pass, or `None`.
    pub before: Option<CellValue>,
    /// The cached value after the pass, or `None` when the cell keeps no
    /// value (an error value stays a value).
    pub after: Option<CellValue>,
}

/// What a recalc pass could not do (ADR 0053, ADR 0055).
#[derive(Debug, Clone, PartialEq)]
pub struct RecalcWarning {
    /// The cell the warning is about, or `None` for a whole-pass warning.
    pub target: Option<CellAddress>,
    /// What happened.
    pub kind: RecalcWarningKind,
}

/// The kind of a [`RecalcWarning`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RecalcWarningKind {
    /// A function the table does not implement; the cell keeps its cached
    /// value.
    UnknownFunction(String),
    /// A reference the resolver could not answer; the cell evaluated to
    /// `#REF!`.
    MissingReference(String),
    /// A dependency cycle: the cells in it keep their cached values.
    Cycle(Vec<CellAddress>),
    /// The pass ran out of its evaluation budget and stopped.
    BudgetExceeded,
    /// A call passed a count outside the table's bounds; the call
    /// evaluated to `#VALUE!` (ADR 0053).
    WrongArgumentCount {
        /// The function name as written.
        function: String,
        /// The fewest arguments the function takes.
        min: u8,
        /// The most arguments it takes, or `None` for a variadic call.
        max: Option<u8>,
        /// How many arguments the call passed.
        got: usize,
    },
    /// A `SETF` whose target is not a writable cell reference; the call
    /// evaluated to `#REF!` (ADR 0056).
    SetfRefused(String),
    /// A value had to be coerced to the type an operator needed and the
    /// conversion was lossy or defaulted (ADR 0052).
    Coercion(String),
    /// A computed number was not finite; it became `#NUM!` (P3-11).
    NonFinite,
    /// The formula text does not parse ([`SyntaxKind`]); the cell keeps
    /// its cached value, the ADR 0053 rule for a formula the engine
    /// cannot compute.
    Syntax(SyntaxKind),
}

/// What one recalc pass did (ADR 0055).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecalcOutcome {
    /// The cells the pass changed, in evaluation order.
    pub updates: Vec<CellUpdate>,
    /// What the pass could not do.
    pub warnings: Vec<RecalcWarning>,
    /// True when the pass stopped at its budget and left cells stale.
    pub truncated: bool,
}

/// One cell write a formula performed (`SETF`, ADR 0056).
#[derive(Debug, Clone, PartialEq)]
pub struct CellWriteOp {
    /// The cell written.
    pub target: CellAddress,
    /// The formula text to store on the target: the write replaces the
    /// target's formula with this literal (the engine spells it).
    pub formula: String,
    /// The cached value to store next to the formula.
    pub value: CellValue,
}

/// What one formula evaluation produced (ADR 0053, ADR 0056).
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// The cached value to store for the cell.
    pub value: CellValue,
    /// Whether the evaluation stood back **silently** (a
    /// [`crate::FunctionKind::VisioOwned`] call, task P7-31): the cell
    /// keeps its cached value and nothing is reported.
    pub kept_cached: bool,
    /// What the evaluation could not do. A [`RecalcWarningKind::UnknownFunction`]
    /// or [`RecalcWarningKind::Syntax`] warning means the cell keeps its
    /// cached value (ADR 0053).
    pub warnings: Vec<RecalcWarning>,
    /// The cells the formula wrote (`SETF`). The pass applies them in
    /// order and dirties their dependents within the same budget. The
    /// writes of an evaluation that stands back are dropped (ADR 0056:
    /// a cell that keeps its cached value has no side effects).
    pub writes: Vec<CellWriteOp>,
    /// How many formula evaluations this call ran: its own plus every
    /// nested one a read of an uncached cell triggered (ADR 0055's
    /// budget counts them, P6-12 review F7). Never 0.
    pub evaluations: u32,
}

/// The formula engine the graph and the recalc pass drive (ADR 0055).
///
/// Production wires this to `parse`, `eval` and `DocumentResolver`
/// (`DocumentEngine`); the tests drive the pass with a small engine so
/// the order, cycle and budget rules are testable alone.
pub trait FormulaEngine {
    /// The cells `formula` reads (its dependency edges).
    ///
    /// A formula that fails to parse answers an error; the graph then
    /// records no edges for it and the document is not failed.
    fn references(&mut self, formula: &Formula) -> Result<Vec<CellRef>, Error>;

    /// Evaluates `formula` on `sheet` against `document`, returning the
    /// cached value to store and the writes the formula performed.
    ///
    /// `Err` means the engine is broken (or not implemented yet); the
    /// pass propagates it and stops.
    fn evaluate(
        &mut self,
        document: &mut Document,
        formula: &Formula,
        sheet: SheetRef,
    ) -> Result<Evaluation, Error>;
}

/// The production engine: [`crate::parse`] + [`crate::eval`] +
/// [`crate::DocumentResolver`] (ADR 0051, ADR 0053, ADR 0054).
///
/// `SETF` writes are recorded, not applied inside the resolver: the pass
/// applies every write through the model's mutation API so one write
/// path owns the document (ADR 0056).
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentEngine;

impl FormulaEngine for DocumentEngine {
    fn references(&mut self, formula: &Formula) -> Result<Vec<CellRef>, Error> {
        let expr = crate::parser::parse_formula(formula)?;
        let mut out = Vec::new();
        collect_refs(&expr, &mut out);
        Ok(out)
    }

    fn evaluate(
        &mut self,
        document: &mut Document,
        formula: &Formula,
        sheet: SheetRef,
    ) -> Result<Evaluation, Error> {
        let (value, traced, raw_writes, nested, kept_cached) = {
            // The resolver records `SETF` writes and attributes warnings
            // to the cell whose formula raised them (ADR 0056, P6-12
            // reviews F1 and F16); nothing writes the document here.
            let mut resolver = DocumentResolver::new(document);
            let value = match crate::eval::eval_formula(formula, sheet, &mut resolver) {
                Ok(value) => value,
                Err(Error::Syntax { kind, .. }) => {
                    // A formula the parser rejects is document data, not
                    // a broken engine: the cell keeps its cached value
                    // (the ADR 0053 rule) and one warning says why.
                    return Ok(Evaluation {
                        // Never written: the warning keeps the cell's
                        // cached value.
                        value: CellValue::Error(String::from("#VALUE!")),
                        kept_cached: false,
                        warnings: vec![RecalcWarning {
                            target: None,
                            kind: RecalcWarningKind::Syntax(kind),
                        }],
                        writes: Vec::new(),
                        evaluations: 1,
                    });
                }
                Err(error) => return Err(error),
            };
            let nested = resolver.evaluations();
            let kept_cached = resolver.kept_cached();
            let (traced, raw_writes) = resolver.into_parts();
            (value, traced, raw_writes, nested, kept_cached)
        };
        let cached = value.to_cached()?;
        let mut warnings: Vec<RecalcWarning> = Vec::new();
        for trace in traced {
            // A warning raised inside a referenced cell's formula names
            // that cell (P6-12 review F16); the outer cell fills in for
            // a warning the top-level formula raised itself.
            let target = trace
                .origin
                .as_ref()
                .and_then(|origin| graph::resolve_ref_address(document, sheet, origin));
            warnings.extend(map_warning(target, trace.warning));
        }
        let mut writes: Vec<CellWriteOp> = Vec::new();
        for write in raw_writes {
            let cached_write = write.value.to_cached()?;
            match graph::resolve_write_address(document, write.sheet, &write.target) {
                Some(target) => writes.push(CellWriteOp {
                    target,
                    formula: literal_text(&write.value),
                    value: cached_write.value,
                }),
                None => warnings.push(RecalcWarning {
                    target: None,
                    kind: RecalcWarningKind::MissingReference(
                        write.target.name().unwrap_or_default(),
                    ),
                }),
            }
        }
        Ok(Evaluation {
            value: cached.value,
            kept_cached,
            warnings,
            writes,
            evaluations: nested.saturating_add(1),
        })
    }
}

/// The cells of one parsed expression (ADR 0051's [`ExprKind`]), in
/// source order. Iterative: no recursion on input.
fn collect_refs(expr: &Expr, out: &mut Vec<CellRef>) {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(next) = stack.pop() {
        match &next.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Ref(reference) => out.push(reference.clone()),
            ExprKind::Unary { operand, .. } => stack.push(operand),
            ExprKind::Binary { left, right, .. } => {
                stack.push(right);
                stack.push(left);
            }
            ExprKind::Call { args, .. } => stack.extend(args.iter().rev()),
        }
    }
}

/// Maps an evaluator warning to the recalc vocabulary. A cut evaluation
/// cycle is not a recalc warning (the pass reports its own [`RecalcWarningKind::Cycle`])
/// and is dropped here; every other kind keeps its content and its
/// target (P6-11, P6-12 review F16).
fn map_warning(target: Option<CellAddress>, warning: EvalWarning) -> Option<RecalcWarning> {
    let kind = match &warning.kind {
        EvalWarningKind::UnknownFunction(name) => RecalcWarningKind::UnknownFunction(name.clone()),
        EvalWarningKind::UnsupportedReference(name) => {
            RecalcWarningKind::MissingReference(name.clone())
        }
        EvalWarningKind::WrongArgumentCount {
            function,
            min,
            max,
            got,
        } => RecalcWarningKind::WrongArgumentCount {
            function: function.clone(),
            min: *min,
            max: *max,
            got: *got,
        },
        EvalWarningKind::SetfRefused(target) => RecalcWarningKind::SetfRefused(target.clone()),
        EvalWarningKind::Coercion(why) => RecalcWarningKind::Coercion(why.clone()),
        EvalWarningKind::NonFinite => RecalcWarningKind::NonFinite,
        EvalWarningKind::Cycle => return None,
    };
    Some(RecalcWarning { target, kind })
}

/// The literal formula text a `SETF` write stores (ADR 0056): the
/// workspace's one number spelling (P5-19 review F6), our own spellings
/// for the other kinds. Recursion is bounded by the parse limit
/// ([`crate::MAX_NESTING`]).
fn literal_text(value: &Value) -> String {
    match value {
        Value::Number(number) => larrygram_model::mutate::literal(number.magnitude),
        Value::Str(text) => format!("\"{}\"", text.replace('"', "\"\"")),
        Value::Bool(true) => String::from("TRUE"),
        Value::Bool(false) => String::from("FALSE"),
        Value::Color(color) => format!("RGB({},{},{})", color.r, color.g, color.b),
        Value::List(items) => items
            .iter()
            .map(literal_text)
            .collect::<Vec<String>>()
            .join(";"),
        Value::Err(code) => code.as_str().to_owned(),
    }
}

/// Recalculates `dirty` and their transitive dependents in `document`.
///
/// The convenience entry point: it builds a throw-away [`DepGraph`] and
/// runs `recalc_with` with `DocumentEngine`. A session that keeps
/// its graph (ADR 0055) calls `recalc_with` directly.
pub fn recalc(
    document: &mut Document,
    dirty: &[CellAddress],
    options: &RecalcOptions,
) -> Result<RecalcOutcome, Error> {
    let mut graph = DepGraph::build(document)?;
    recalc_with(document, dirty, options, &mut graph, &mut DocumentEngine)
}

/// Recalculates `dirty` and their transitive dependents, refreshing only
/// the graph edges the pass touches (ADR 0055).
///
/// The pass writes what ADR 0056 says: a cell with its own formula gets
/// its cached value updated, an inherited cell gets the formula and value
/// materialised when `options.materialise_inherited` is set, and `SETF`
/// writes land as ordinary writes that dirty their dependents within the
/// same budget. Errors mean a broken engine and stop the pass; everything
/// else is a warning and keeps the document.
pub fn recalc_with(
    document: &mut Document,
    dirty: &[CellAddress],
    options: &RecalcOptions,
    graph: &mut DepGraph,
    engine: &mut dyn FormulaEngine,
) -> Result<RecalcOutcome, Error> {
    let mut outcome = RecalcOutcome::default();
    let dirty: Vec<CellAddress> = dirty
        .iter()
        .map(|address| graph::normalize_address(document, address))
        .collect();
    graph.update(document, &dirty, engine);

    let (order, cycles) = graph.order_and_cycles(&dirty);
    let mut done: BTreeSet<CellAddress> = BTreeSet::new();
    for cycle in cycles {
        for cell in &cycle {
            done.insert(cell.clone());
        }
        outcome.warnings.push(RecalcWarning {
            target: None,
            kind: RecalcWarningKind::Cycle(cycle),
        });
    }
    let mut queued: BTreeSet<CellAddress> = order.iter().cloned().collect();
    let mut queue: VecDeque<CellAddress> = order.into_iter().collect();
    let mut changes: Vec<Change> = Vec::new();
    let mut touched: Vec<CellAddress> = Vec::new();
    let mut evaluations: u32 = 0;

    while let Some(address) = queue.pop_front() {
        if done.contains(&address) {
            continue;
        }
        done.insert(address.clone());
        let effective = match graph::effective(document, &address) {
            Ok(Some(effective)) => effective,
            Ok(None) => continue,
            Err(_) => {
                outcome.warnings.push(missing_cell(&address));
                continue;
            }
        };
        if options.fill_missing_only && effective.local_value.is_some() {
            continue;
        }
        if evaluations >= options.max_evaluations {
            outcome.truncated = true;
            outcome.warnings.push(RecalcWarning {
                target: None,
                kind: RecalcWarningKind::BudgetExceeded,
            });
            break;
        }
        let evaluation = engine.evaluate(
            document,
            &Formula::new(effective.formula.as_str()),
            address.sheet,
        )?;
        // The budget counts every evaluation the call ran, nested ones
        // included (ADR 0055, P6-12 review F7).
        evaluations = evaluations.saturating_add(evaluation.evaluations.max(1));
        let keep_cached = evaluation.kept_cached
            || evaluation.warnings.iter().any(|warning| {
                matches!(
                    warning.kind,
                    RecalcWarningKind::UnknownFunction(_) | RecalcWarningKind::Syntax(_)
                )
            });
        for mut warning in evaluation.warnings {
            if warning.target.is_none() {
                warning.target = Some(address.clone());
            }
            outcome.warnings.push(warning);
        }
        // A standing-back evaluation keeps its cached value and drops
        // its `SETF` writes (ADR 0056, P6-12 review F3): a cell that
        // keeps its value has no side effects.
        if !keep_cached {
            if let Some(text) = formula_text(&effective, options) {
                match apply_write(document, &address, &text, &evaluation.value, &mut changes) {
                    Ok(true) => touched.push(address.clone()),
                    Ok(false) => {}
                    Err(_) => outcome.warnings.push(missing_cell(&address)),
                }
            }
            for op in evaluation.writes {
                let target = graph::normalize_address(document, &op.target);
                match apply_write(document, &target, &op.formula, &op.value, &mut changes) {
                    Ok(true) => touched.push(target.clone()),
                    Ok(false) => {}
                    Err(_) => {
                        outcome.warnings.push(missing_cell(&target));
                        continue;
                    }
                }
                // The write dirties its target and the target's dependents
                // (ADR 0056); anything already done or queued is cut by the
                // cycle rule and evaluates at most once per pass.
                for extra in graph.order_and_cycles(std::slice::from_ref(&target)).0 {
                    if done.contains(&extra) || queued.contains(&extra) {
                        continue;
                    }
                    queued.insert(extra.clone());
                    queue.push_back(extra);
                }
            }
        }
    }

    graph.update(document, &touched, engine);
    outcome.updates = changes
        .into_iter()
        .filter(|change| change.before != change.after || change.formula_changed)
        .map(|change| CellUpdate {
            target: change.target,
            before: change.before,
            after: change.after,
        })
        .collect();
    Ok(outcome)
}

/// The warning for a cell the pass could not write or look up. The model
/// error itself is not a document failure (ADR 0055).
fn missing_cell(address: &CellAddress) -> RecalcWarning {
    RecalcWarning {
        target: Some(address.clone()),
        kind: RecalcWarningKind::MissingReference(address.cell.as_str().to_owned()),
    }
}

/// The formula text the pass writes for the cell (ADR 0056): its own
/// formula unchanged, or the evaluated formula materialised onto an
/// inherited cell. `None` means the pass writes nothing (an absent cell
/// keeps no half-written entry).
fn formula_text(effective: &EffectiveCell, options: &RecalcOptions) -> Option<String> {
    if effective.own || options.materialise_inherited {
        return Some(effective.formula.clone());
    }
    effective.local_formula.clone()
}

/// One cell the pass changed.
#[derive(Debug, Clone, PartialEq)]
struct Change {
    /// The cell.
    target: CellAddress,
    /// The cached value before the pass.
    before: Option<CellValue>,
    /// The cached value after the last write.
    after: Option<CellValue>,
    /// Whether any write replaced the formula text.
    formula_changed: bool,
}

/// Writes one result into the document and records the change. Returns
/// whether the formula text changed (the graph edges must be rebuilt for
/// such a cell).
///
/// The write goes through the model's mutation API (ADR 0039); a write
/// that names no real cell is an error the caller reports as a warning.
fn apply_write(
    document: &mut Document,
    address: &CellAddress,
    formula: &str,
    value: &CellValue,
    changes: &mut Vec<Change>,
) -> Result<bool, larrygram_model::Error> {
    let before = graph::local_cell(document, address).and_then(|cell| cell.value.clone());
    let previous = graph::local_cell(document, address).and_then(|cell| cell.formula.clone());
    let formula_changed = previous.as_deref() != Some(formula);
    let write = CellWrite::new(formula, value.clone());
    match (&address.section, &address.row) {
        (None, None) => document.write_cell(address.sheet, &address.cell, &write)?,
        (Some(section), None) => {
            document.write_section_cell(address.sheet, section, &address.cell, &write)?
        }
        (Some(section), Some(row)) => {
            document.write_row_cell(address.sheet, section, *row, &address.cell, &write)?
        }
        (None, Some(_)) => {
            return Err(larrygram_model::Error::BadCell("row without a section"));
        }
    }
    if let Some(existing) = changes.iter_mut().find(|change| change.target == *address) {
        existing.after = Some(value.clone());
        existing.formula_changed = existing.formula_changed || formula_changed;
    } else {
        changes.push(Change {
            target: address.clone(),
            before,
            after: Some(value.clone()),
            formula_changed,
        });
    }
    Ok(formula_changed)
}

#[cfg(test)]
pub(crate) mod testing {
    //! A small formula engine for the tests of `crate::graph` and this
    //! module (task P6-06).
    //!
    //! The tests write the little language this module reads: a number
    //! (`"0"`), a cell read (`"Beta"`), arithmetic on one cell and one
    //! number (`"Beta+1"`, `"Beta*2"`), a sum of two cells
    //! (`"Beta+Gamma"`) and a basic `SETF` (`"SETF(Beta,5)"`). That is
    //! enough to pin the pass's own rules: order, incrementality,
    //! cycles, budget, writes and idempotence. The real formulas and the
    //! real evaluator arrive with tasks P6-02 to P6-05.

    use super::*;
    use larrygram_model::CellName;
    use larrygram_model::mutate::literal;

    /// The test engine described above.
    #[derive(Debug, Default)]
    pub(crate) struct TestEngine {
        /// The formulas it evaluated, in order.
        pub(crate) evaluated: Vec<String>,
        /// When true, every evaluation warns [`RecalcWarningKind::UnknownFunction`]
        /// (ADR 0053's keep-the-cached-value rule).
        pub(crate) warn_unknown: bool,
    }

    impl TestEngine {
        /// An engine that evaluates the little language.
        pub(crate) fn new() -> Self {
            Self::default()
        }
    }

    impl FormulaEngine for TestEngine {
        fn references(&mut self, formula: &Formula) -> Result<Vec<CellRef>, Error> {
            Ok(scan_refs(formula.as_str()))
        }

        fn evaluate(
            &mut self,
            document: &mut Document,
            formula: &Formula,
            sheet: SheetRef,
        ) -> Result<Evaluation, Error> {
            self.evaluated.push(formula.as_str().to_owned());
            if self.warn_unknown {
                return Ok(Evaluation {
                    kept_cached: false,
                    value: CellValue::Number(0.0),
                    warnings: vec![RecalcWarning {
                        target: None,
                        kind: RecalcWarningKind::UnknownFunction(String::from("THEMEVAL")),
                    }],
                    writes: Vec::new(),
                    evaluations: 1,
                });
            }
            Ok(evaluate_test(document, formula.as_str(), sheet))
        }
    }

    /// The identifiers of `source` that read a cell: everything that is
    /// not a number, a string, `TRUE`/`FALSE` or a function name.
    pub(crate) fn scan_refs(source: &str) -> Vec<CellRef> {
        let mut out = Vec::new();
        let mut chars = source.char_indices().peekable();
        while let Some((at, first)) = chars.next() {
            if first == '"' {
                for (_, skipped) in chars.by_ref() {
                    if skipped == '"' {
                        break;
                    }
                }
                continue;
            }
            if !first.is_ascii_alphabetic() && first != '_' {
                continue;
            }
            let mut end = at.saturating_add(first.len_utf8());
            while let Some((next_at, next)) = chars.peek().copied() {
                if next.is_ascii_alphanumeric() || next == '_' {
                    end = next_at.saturating_add(next.len_utf8());
                    chars.next();
                } else {
                    break;
                }
            }
            let name = source.get(at..end).unwrap_or_default();
            let follows_call = chars.peek().is_some_and(|(_, next)| *next == '(');
            if follows_call
                || name.eq_ignore_ascii_case("TRUE")
                || name.eq_ignore_ascii_case("FALSE")
            {
                continue;
            }
            out.push(CellRef::new(name.to_owned()));
        }
        out
    }

    /// One evaluation of the little language.
    fn evaluate_test(document: &Document, source: &str, sheet: SheetRef) -> Evaluation {
        let text = source.trim();
        if let Some(inner) = text
            .strip_prefix("SETF(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let (name, value) = inner.split_once(',').unwrap_or((inner, "0"));
            let magnitude: f64 = value.trim().parse().unwrap_or(0.0);
            let value = CellValue::Number(magnitude);
            let reference = CellRef::new(name.trim().to_owned());
            let target = graph::resolve_write_address(document, sheet, &reference)
                .unwrap_or_else(|| CellAddress::new(sheet, CellName::new(name.trim().to_owned())));
            return Evaluation {
                kept_cached: false,
                value: value.clone(),
                warnings: Vec::new(),
                writes: vec![CellWriteOp {
                    target,
                    formula: literal(magnitude),
                    value,
                }],
                evaluations: 1,
            };
        }
        let number = if let Some((left, right)) = text.split_once('+') {
            Some(read_operand(document, sheet, left) + read_operand(document, sheet, right))
        } else if let Some((left, right)) = text.split_once('*') {
            Some(read_operand(document, sheet, left) * read_operand(document, sheet, right))
        } else {
            text.parse::<f64>().ok()
        };
        Evaluation {
            kept_cached: false,
            value: CellValue::Number(number.unwrap_or_else(|| read_operand(document, sheet, text))),
            warnings: Vec::new(),
            writes: Vec::new(),
            evaluations: 1,
        }
    }

    /// One operand of the little language: a number or a cell's cached
    /// number.
    fn read_operand(document: &Document, sheet: SheetRef, operand: &str) -> f64 {
        if let Ok(number) = operand.trim().parse::<f64>() {
            return number;
        }
        let address = CellAddress::new(sheet, CellName::new(operand.trim().to_owned()));
        match graph::local_cell(document, &address).and_then(|cell| cell.value.clone()) {
            Some(CellValue::Number(number)) => number,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestEngine;
    use super::*;
    use larrygram_geom::Unit;
    use larrygram_model::{
        Cell, CellName, Master, MasterId, Page, PageId, Preserved, Row, RowTarget, Section,
        SectionName, Shape, ShapeId,
    };

    fn num(value: f64) -> CellValue {
        CellValue::Number(value)
    }

    fn cell(name: &str, formula: Option<&str>, value: Option<CellValue>) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: formula.map(str::to_owned),
            value,
            unit: Unit::Unitsless,
            preserved: Preserved::new(),
        }
    }

    /// A document with one page and one shape holding `cells`.
    fn doc_with(cells: Vec<Cell>) -> (Document, ShapeId) {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = cells;
        let shape = doc.add_shape(shape);
        doc.pages.push(Page::new(PageId { index: 0 }, 1));
        if let Some(page) = doc.pages.first_mut() {
            page.shapes.push(shape);
        }
        (doc, shape)
    }

    fn address(shape: ShapeId, name: &str) -> CellAddress {
        CellAddress::new(SheetRef::Shape(shape), CellName::new(name))
    }

    fn run(
        doc: &mut Document,
        dirty: &[CellAddress],
        options: &RecalcOptions,
        engine: &mut TestEngine,
    ) -> RecalcOutcome {
        let mut graph = DepGraph::build_with(doc, engine).expect("build");
        recalc_with(doc, dirty, options, &mut graph, engine).expect("recalc")
    }

    fn value_of(doc: &Document, address: &CellAddress) -> Option<CellValue> {
        graph::local_cell(doc, address).and_then(|cell| cell.value.clone())
    }

    fn formula_of(doc: &Document, address: &CellAddress) -> Option<String> {
        graph::local_cell(doc, address).and_then(|cell| cell.formula.clone())
    }

    #[test]
    fn an_empty_dirty_set_changes_nothing() {
        let (mut doc, _shape) = doc_with(vec![cell("Alpha", Some("0"), Some(num(1.0)))]);
        let mut engine = TestEngine::new();
        let outcome = run(&mut doc, &[], &RecalcOptions::new(), &mut engine);
        assert_eq!(outcome, RecalcOutcome::default());
        assert!(engine.evaluated.is_empty());
    }

    #[test]
    fn a_diamond_evaluates_the_shared_cell_once() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("0"), Some(num(7.0))),
            cell("Beta", Some("Alpha+1"), Some(num(7.0))),
            cell("Gamma", Some("Alpha+2"), Some(num(7.0))),
            cell("Delta", Some("Beta+Gamma"), Some(num(7.0))),
        ]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        // The shared cell evaluates once, before both paths.
        assert_eq!(
            engine.evaluated,
            vec!["0", "Alpha+1", "Alpha+2", "Beta+Gamma"]
        );
        assert_eq!(
            value_of(&doc, &address(shape, "Delta")),
            Some(num(1.0 + 2.0))
        );
        assert_eq!(outcome.updates.len(), 4);
    }

    #[test]
    fn dirtying_one_cell_recomputes_exactly_its_dependents() {
        let mut cells = Vec::new();
        for i in 1..=20 {
            let name = format!("c{i}");
            let formula = if i == 1 {
                String::from("0")
            } else {
                format!("c{}+1", i - 1)
            };
            cells.push(cell(&name, Some(&formula), Some(num(100.0))));
        }
        let (mut doc, shape) = doc_with(cells);
        let mut engine = TestEngine::new();
        let dirty = [address(shape, "c5")];
        let outcome = run(&mut doc, &dirty, &RecalcOptions::new(), &mut engine);
        // c5..c20 recompute (16 evaluations), c1..c4 stay as they were.
        assert_eq!(engine.evaluated.len(), 16);
        assert_eq!(engine.evaluated.first().map(String::as_str), Some("c4+1"));
        assert_eq!(engine.evaluated.last().map(String::as_str), Some("c19+1"));
        assert_eq!(value_of(&doc, &address(shape, "c1")), Some(num(100.0)));
        assert_eq!(value_of(&doc, &address(shape, "c4")), Some(num(100.0)));
        assert_eq!(value_of(&doc, &address(shape, "c5")), Some(num(101.0)));
        assert_eq!(value_of(&doc, &address(shape, "c20")), Some(num(116.0)));
        assert_eq!(outcome.updates.len(), 16);
    }

    #[test]
    fn a_cycle_keeps_cached_values_and_warns_once() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("Beta+1"), Some(num(1.0))),
            cell("Beta", Some("Alpha+1"), Some(num(2.0))),
        ]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert!(
            engine.evaluated.is_empty(),
            "cycle cells keep cached values"
        );
        assert_eq!(
            outcome.warnings,
            vec![RecalcWarning {
                target: None,
                kind: RecalcWarningKind::Cycle(vec![
                    address(shape, "Alpha"),
                    address(shape, "Beta"),
                ]),
            }]
        );
        assert_eq!(value_of(&doc, &address(shape, "Alpha")), Some(num(1.0)));
        assert_eq!(value_of(&doc, &address(shape, "Beta")), Some(num(2.0)));
        assert!(outcome.updates.is_empty());
    }

    #[test]
    fn a_self_reference_is_a_cycle_of_one() {
        let (mut doc, shape) = doc_with(vec![cell("Alpha", Some("Alpha+1"), Some(num(1.0)))]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(
            outcome.warnings,
            vec![RecalcWarning {
                target: None,
                kind: RecalcWarningKind::Cycle(vec![address(shape, "Alpha")]),
            }]
        );
        assert_eq!(value_of(&doc, &address(shape, "Alpha")), Some(num(1.0)));
    }

    #[test]
    fn a_tiny_budget_truncates_and_leaves_the_rest_stale() {
        let mut cells = Vec::new();
        for i in 1..=10 {
            let name = format!("d{i}");
            let formula = if i == 1 {
                String::from("0")
            } else {
                format!("d{}+1", i - 1)
            };
            cells.push(cell(&name, Some(&formula), Some(num(50.0))));
        }
        let (mut doc, shape) = doc_with(cells);
        let mut engine = TestEngine::new();
        let options = RecalcOptions {
            max_evaluations: 3,
            ..RecalcOptions::new()
        };
        let outcome = run(&mut doc, &[address(shape, "d1")], &options, &mut engine);
        assert!(outcome.truncated);
        assert_eq!(engine.evaluated.len(), 3);
        assert_eq!(
            outcome.warnings,
            vec![RecalcWarning {
                target: None,
                kind: RecalcWarningKind::BudgetExceeded,
            }]
        );
        assert_eq!(value_of(&doc, &address(shape, "d3")), Some(num(2.0)));
        // d4..d10 are stale: untouched.
        assert_eq!(value_of(&doc, &address(shape, "d4")), Some(num(50.0)));
        assert_eq!(value_of(&doc, &address(shape, "d10")), Some(num(50.0)));
    }

    #[test]
    fn setf_writes_its_target_and_dirties_it_in_the_same_pass() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("SETF(Beta,5)"), Some(num(0.0))),
            cell("Beta", Some("2"), Some(num(2.0))),
        ]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        // The write lands, and the target is re-evaluated in the same
        // pass (now as the literal the write stored).
        assert_eq!(
            value_of(&doc, &address(shape, "Beta")),
            Some(num(5.0)),
            "the SETF write lands"
        );
        assert_eq!(
            formula_of(&doc, &address(shape, "Beta")),
            Some(String::from("5")),
            "the write stores a literal formula"
        );
        assert_eq!(
            engine.evaluated,
            vec!["SETF(Beta,5)", "5"],
            "the target is dirty within the same pass"
        );
        let targets: Vec<&CellAddress> = outcome
            .updates
            .iter()
            .map(|update| &update.target)
            .collect();
        assert_eq!(
            targets,
            vec![&address(shape, "Alpha"), &address(shape, "Beta")]
        );
    }

    #[test]
    fn a_cell_with_its_own_formula_gets_only_its_value_updated() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("Beta+1"), Some(num(0.0))),
            cell("Beta", Some("5"), Some(num(5.0))),
        ]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(
            formula_of(&doc, &address(shape, "Alpha")),
            Some(String::from("Beta+1")),
            "the formula text is untouched"
        );
        assert_eq!(value_of(&doc, &address(shape, "Alpha")), Some(num(6.0)));
        assert_eq!(
            outcome.updates,
            vec![CellUpdate {
                target: address(shape, "Alpha"),
                before: Some(num(0.0)),
                after: Some(num(6.0)),
            }]
        );
    }

    /// A document whose master defines `Alpha` and whose instance holds
    /// `Alpha` as `Inh` with its own snapshot (ADR 0054).
    fn inherited_document() -> (Document, ShapeId, ShapeId) {
        let mut doc = Document::new();
        let mut master_shape = Shape::new(ShapeId { index: 0 }, 1);
        master_shape.sheet.cells = vec![cell("Alpha", Some("Beta*2"), Some(num(4.0)))];
        let master_shape = doc.add_shape(master_shape);
        doc.masters.push(Master::new(MasterId { index: 0 }, 1));
        if let Some(master) = doc.masters.first_mut() {
            master.shapes.push(master_shape);
        }
        let mut instance = Shape::new(ShapeId { index: 1 }, 2);
        instance.master = Some(MasterId { index: 0 });
        instance.sheet.cells = vec![
            cell("Alpha", Some("Inh"), Some(num(3.0))),
            cell("Beta", Some("0"), Some(num(5.0))),
        ];
        let instance = doc.add_shape(instance);
        (doc, master_shape, instance)
    }

    #[test]
    fn an_inherited_cell_is_materialised_when_the_pass_recomputes_it() {
        let (mut doc, master_shape, instance) = inherited_document();
        let mut engine = TestEngine::new();
        let options = RecalcOptions {
            materialise_inherited: true,
            ..RecalcOptions::new()
        };
        let outcome = run(
            &mut doc,
            &[address(instance, "Alpha")],
            &options,
            &mut engine,
        );
        // The master's formula is evaluated against the instance and
        // materialised on it (ADR 0056).
        assert_eq!(
            formula_of(&doc, &address(instance, "Alpha")),
            Some(String::from("Beta*2")),
            "the evaluated formula is written on the instance"
        );
        assert_eq!(value_of(&doc, &address(instance, "Alpha")), Some(num(10.0)));
        assert_eq!(
            outcome.updates,
            vec![CellUpdate {
                target: address(instance, "Alpha"),
                before: Some(num(3.0)),
                after: Some(num(10.0)),
            }]
        );
        // The master's own cell is untouched.
        assert_eq!(
            formula_of(&doc, &address(master_shape, "Alpha")),
            Some(String::from("Beta*2"))
        );
        assert_eq!(
            value_of(&doc, &address(master_shape, "Alpha")),
            Some(num(4.0))
        );
    }

    #[test]
    fn an_inherited_cell_keeps_its_marker_without_materialisation() {
        let (mut doc, _master_shape, instance) = inherited_document();
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(instance, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(
            formula_of(&doc, &address(instance, "Alpha")),
            Some(String::from("Inh")),
            "no materialisation without the option"
        );
        assert_eq!(value_of(&doc, &address(instance, "Alpha")), Some(num(10.0)));
        assert_eq!(outcome.updates.len(), 1);
    }

    #[test]
    fn recalc_is_idempotent() {
        let (mut doc, _master_shape, instance) = inherited_document();
        let mut engine = TestEngine::new();
        let options = RecalcOptions {
            materialise_inherited: true,
            ..RecalcOptions::new()
        };
        let dirty = [address(instance, "Alpha")];
        let first = run(&mut doc, &dirty, &options, &mut engine);
        assert!(!first.updates.is_empty());
        let second = run(&mut doc, &dirty, &options, &mut engine);
        assert!(
            second.updates.is_empty(),
            "a second pass changes nothing: {:?}",
            second.updates
        );
        // The same for a SETF write: the write repeats, the outcome does
        // not grow.
        let (mut doc2, shape2) = doc_with(vec![
            cell("Alpha", Some("SETF(Beta,5)"), Some(num(0.0))),
            cell("Beta", Some("2"), Some(num(2.0))),
        ]);
        let mut engine2 = TestEngine::new();
        let dirty2 = [address(shape2, "Alpha")];
        let first = run(&mut doc2, &dirty2, &RecalcOptions::new(), &mut engine2);
        assert_eq!(first.updates.len(), 2);
        let second = run(&mut doc2, &dirty2, &RecalcOptions::new(), &mut engine2);
        assert!(second.updates.is_empty());
    }

    #[test]
    fn an_unknown_function_keeps_the_cached_value() {
        let (mut doc, shape) = doc_with(vec![cell("Alpha", Some("THEMEVAL(1)"), Some(num(42.0)))]);
        let mut engine = TestEngine {
            warn_unknown: true,
            ..TestEngine::new()
        };
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(value_of(&doc, &address(shape, "Alpha")), Some(num(42.0)));
        assert!(outcome.updates.is_empty());
        assert_eq!(
            outcome.warnings,
            vec![RecalcWarning {
                target: Some(address(shape, "Alpha")),
                kind: RecalcWarningKind::UnknownFunction(String::from("THEMEVAL")),
            }]
        );
    }

    /// The glue formulas a Visio-written file carries (ADR 0084 rule 2,
    /// `docs/ui/connectors.md` section 8): the Visio-owned calls keep
    /// the cell's cached value and report nothing (task P7-31 item 4).
    #[test]
    fn the_visio_owned_glue_functions_keep_the_cached_value_silently() {
        let (mut doc, shape) = doc_with(vec![
            cell(
                "BeginX",
                Some("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))"),
                Some(num(1.5)),
            ),
            cell(
                "BegTrigger",
                Some("_XFTRIGGER(Sheet.2!EventXFMod)"),
                Some(num(2.0)),
            ),
            cell(
                "EndX",
                Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"),
                Some(num(3.5)),
            ),
        ]);
        let dirty = [
            address(shape, "BeginX"),
            address(shape, "BegTrigger"),
            address(shape, "EndX"),
        ];
        let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
        assert!(
            outcome.warnings.is_empty(),
            "no warning, no UnknownFunction entry: {:?}",
            outcome.warnings
        );
        assert!(outcome.updates.is_empty(), "{:?}", outcome.updates);
        assert_eq!(value_of(&doc, &address(shape, "BeginX")), Some(num(1.5)));
        assert_eq!(
            value_of(&doc, &address(shape, "BegTrigger")),
            Some(num(2.0))
        );
        assert_eq!(value_of(&doc, &address(shape, "EndX")), Some(num(3.5)));
        assert_eq!(
            formula_of(&doc, &address(shape, "BeginX")),
            Some(String::from(
                "PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))"
            )),
            "the glue formula is not replaced"
        );
    }

    /// A Visio-owned formula on a cell that caches nothing writes
    /// nothing: the evaluator computes no replacement value for it.
    #[test]
    fn a_visio_owned_call_without_a_cached_value_writes_nothing() {
        let (mut doc, shape) = doc_with(vec![cell("BeginX", Some("_XFTRIGGER(EventXFMod)"), None)]);
        let outcome =
            recalc(&mut doc, &[address(shape, "BeginX")], &RecalcOptions::new()).expect("recalc");
        assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
        assert!(outcome.updates.is_empty(), "{:?}", outcome.updates);
        assert_eq!(value_of(&doc, &address(shape, "BeginX")), None);
    }

    #[test]
    fn fill_missing_only_recomputes_cells_without_a_cached_value() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("0"), None),
            cell("Beta", Some("0"), Some(num(2.0))),
        ]);
        let mut engine = TestEngine::new();
        let options = RecalcOptions {
            fill_missing_only: true,
            ..RecalcOptions::new()
        };
        let dirty = [address(shape, "Alpha"), address(shape, "Beta")];
        let outcome = run(&mut doc, &dirty, &options, &mut engine);
        assert_eq!(engine.evaluated, vec!["0"]);
        assert_eq!(value_of(&doc, &address(shape, "Alpha")), Some(num(0.0)));
        assert_eq!(value_of(&doc, &address(shape, "Beta")), Some(num(2.0)));
        assert_eq!(outcome.updates.len(), 1);
    }

    #[test]
    fn a_dirty_cell_with_no_formula_is_skipped() {
        let (mut doc, shape) = doc_with(vec![cell("Alpha", None, Some(num(3.0)))]);
        let mut engine = TestEngine::new();
        let outcome = run(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(outcome, RecalcOutcome::default());
        assert!(engine.evaluated.is_empty());
    }

    #[test]
    fn a_write_to_a_missing_cell_is_a_warning_not_a_failure() {
        let (mut doc, shape) = doc_with(vec![cell("Alpha", Some("0"), Some(num(1.0)))]);
        let mut engine = TestEngine::new();
        let missing = CellAddress::in_row(
            SheetRef::Shape(shape),
            SectionName::from_static("Geometry"),
            RowTarget::at(9),
            CellName::from_static("X"),
        );
        let outcome = run(
            &mut doc,
            std::slice::from_ref(&missing),
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(
            outcome.warnings,
            vec![RecalcWarning {
                target: Some(missing),
                kind: RecalcWarningKind::MissingReference(String::from("X")),
            }]
        );
    }

    /// P6-11: a section-level cell (a section, no row) is evaluated and
    /// written through the section write, not skipped as an unresolvable
    /// direct cell (the P6-08 open issue).
    #[test]
    fn a_section_cell_is_evaluated_and_written_in_place() {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.sections.push({
            let mut section = Section::new(SectionName::from_static("Geometry"));
            section
                .cells
                .push(cell("NoFill", Some("0"), Some(num(1.0))));
            section.rows.push({
                let mut row = Row::new();
                row.cells.push(cell("X", Some("NoFill+1"), Some(num(1.0))));
                row
            });
            section
        });
        let shape = doc.add_shape(shape);
        let mut engine = TestEngine::new();
        let target = CellAddress {
            sheet: SheetRef::Shape(shape),
            section: Some(SectionName::from_static("Geometry")),
            row: None,
            cell: CellName::from_static("NoFill"),
        };
        let outcome = run(
            &mut doc,
            std::slice::from_ref(&target),
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(
            engine.evaluated,
            vec!["0", "NoFill+1"],
            "the section cell evaluates, and its row reads it"
        );
        assert_eq!(value_of(&doc, &target), Some(num(0.0)));
        assert_eq!(
            outcome.updates,
            vec![CellUpdate {
                target: target.clone(),
                before: Some(num(1.0)),
                after: Some(num(0.0)),
            }]
        );
        assert_eq!(outcome.warnings, Vec::new(), "nothing is unwritable");
    }

    #[test]
    fn a_row_cell_is_written_through_the_row_write() {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(cell("Width", None, Some(num(4.0))));
        shape.sheet.sections.push({
            let mut section = Section::new(SectionName::from_static("Geometry"));
            section.rows.push({
                let mut row = Row::new();
                row.ix = Some(0);
                row.cells.push(cell("X", Some("Width*0.5"), Some(num(0.0))));
                row
            });
            section
        });
        let shape = doc.add_shape(shape);
        let mut engine = TestEngine::new();
        let target = CellAddress::in_row(
            SheetRef::Shape(shape),
            SectionName::from_static("Geometry"),
            RowTarget {
                position: Some(0),
                ix: Some(0),
            },
            CellName::from_static("X"),
        );
        let outcome = run(
            &mut doc,
            std::slice::from_ref(&target),
            &RecalcOptions::new(),
            &mut engine,
        );
        assert_eq!(value_of(&doc, &target), Some(num(2.0)));
        assert_eq!(
            outcome.updates,
            vec![CellUpdate {
                target,
                before: Some(num(0.0)),
                after: Some(num(2.0)),
            }]
        );
    }

    #[test]
    fn the_production_engine_never_fails_the_document() {
        let (mut doc, shape) = doc_with(vec![
            cell("Alpha", Some("Beta+1"), Some(num(1.0))),
            cell("Beta", Some("0"), Some(num(2.0))),
        ]);
        let dirty = [address(shape, "Alpha")];
        let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new());
        // The real pipeline is `Ok` once tasks P6-02..P6-05 land; while
        // it is stubbed the pass reports it and stops. It never panics
        // and never fails the document in either state.
        assert!(outcome.is_ok() || matches!(outcome, Err(Error::NotImplemented(_))));
    }

    #[test]
    fn collect_refs_walks_the_tree_in_source_order() {
        let expr = Expr {
            span: crate::Span::new(0, 1),
            kind: ExprKind::Binary {
                op: crate::BinaryOp::Add,
                left: Box::new(Expr {
                    span: crate::Span::new(0, 1),
                    kind: ExprKind::Ref(CellRef::new("PinX")),
                }),
                right: Box::new(Expr {
                    span: crate::Span::new(2, 3),
                    kind: ExprKind::Call {
                        name: String::from("MAX"),
                        args: vec![
                            Expr {
                                span: crate::Span::new(4, 5),
                                kind: ExprKind::Ref(CellRef::new("Width")),
                            },
                            Expr {
                                span: crate::Span::new(6, 7),
                                kind: ExprKind::Literal(crate::Literal::Bool(true)),
                            },
                        ],
                    },
                }),
            },
        };
        let mut out = Vec::new();
        collect_refs(&expr, &mut out);
        let names: Vec<String> = out
            .iter()
            .map(|reference| reference.name().unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["PinX", "Width"]);
    }

    #[test]
    fn map_warning_keeps_only_the_recalc_kinds() {
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::UnknownFunction(String::from("THEMEVAL")),
            },
        );
        assert_eq!(
            mapped,
            Some(RecalcWarning {
                target: None,
                kind: RecalcWarningKind::UnknownFunction(String::from("THEMEVAL")),
            })
        );
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::UnsupportedReference(String::from("Sheet.9!PinX")),
            },
        );
        assert_eq!(
            mapped.map(|warning| warning.kind),
            Some(RecalcWarningKind::MissingReference(String::from(
                "Sheet.9!PinX"
            )))
        );
        assert_eq!(
            map_warning(
                None,
                EvalWarning {
                    sheet: SheetRef::Page(PageId { index: 0 }),
                    kind: EvalWarningKind::Cycle,
                }
            ),
            None
        );
    }

    /// P6-11: a [`EvalWarningKind::WrongArgumentCount`] reaches the
    /// recalc outcome with every field kept.
    #[test]
    fn map_warning_keeps_a_wrong_argument_count() {
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::WrongArgumentCount {
                    function: String::from("ABS"),
                    min: 1,
                    max: Some(1),
                    got: 2,
                },
            },
        );
        assert_eq!(
            mapped.map(|warning| warning.kind),
            Some(RecalcWarningKind::WrongArgumentCount {
                function: String::from("ABS"),
                min: 1,
                max: Some(1),
                got: 2,
            })
        );
    }

    /// P6-11: a [`EvalWarningKind::SetfRefused`] reaches the recalc
    /// outcome with its target kept.
    #[test]
    fn map_warning_keeps_a_refused_setf() {
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::SetfRefused(String::from("\"User.Nope\"")),
            },
        );
        assert_eq!(
            mapped.map(|warning| warning.kind),
            Some(RecalcWarningKind::SetfRefused(String::from(
                "\"User.Nope\""
            )))
        );
    }

    /// P6-11: a [`EvalWarningKind::Coercion`] reaches the recalc
    /// outcome with its reason kept.
    #[test]
    fn map_warning_keeps_a_coercion() {
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::Coercion(String::from("sheep")),
            },
        );
        assert_eq!(
            mapped.map(|warning| warning.kind),
            Some(RecalcWarningKind::Coercion(String::from("sheep")))
        );
    }

    /// P6-11: a [`EvalWarningKind::NonFinite`] reaches the recalc
    /// outcome.
    #[test]
    fn map_warning_keeps_a_non_finite() {
        let mapped = map_warning(
            None,
            EvalWarning {
                sheet: SheetRef::Page(PageId { index: 0 }),
                kind: EvalWarningKind::NonFinite,
            },
        );
        assert_eq!(
            mapped.map(|warning| warning.kind),
            Some(RecalcWarningKind::NonFinite)
        );
    }

    #[test]
    fn literal_text_spells_every_value_kind() {
        use crate::units::Unit as FormulaUnit;
        use crate::value::{ErrorCode, Number};
        use larrygram_model::Color;
        assert_eq!(
            literal_text(&Value::Number(Number::new(2.5, FormulaUnit::Unitsless))),
            "2.5"
        );
        assert_eq!(
            literal_text(&Value::Str(String::from("a\"b"))),
            "\"a\"\"b\""
        );
        assert_eq!(literal_text(&Value::Bool(true)), "TRUE");
        assert_eq!(literal_text(&Value::Bool(false)), "FALSE");
        assert_eq!(
            literal_text(&Value::Color(Color::new(1, 2, 3))),
            "RGB(1,2,3)"
        );
        assert_eq!(
            literal_text(&Value::List(vec![
                Value::Bool(false),
                Value::Number(Number::new(1.0, FormulaUnit::Unitsless)),
            ])),
            "FALSE;1"
        );
        assert_eq!(literal_text(&Value::Err(ErrorCode::Div0)), "#DIV/0!");
    }

    #[test]
    fn a_setf_write_to_an_unresolvable_target_is_reported() {
        // The engine writes an address no model cell names: the pass
        // records the change attempt as a write error, not a panic.
        struct BadWriteEngine;
        impl FormulaEngine for BadWriteEngine {
            fn references(&mut self, _formula: &Formula) -> Result<Vec<CellRef>, Error> {
                Ok(Vec::new())
            }
            fn evaluate(
                &mut self,
                _document: &mut Document,
                _formula: &Formula,
                _sheet: SheetRef,
            ) -> Result<Evaluation, Error> {
                Ok(Evaluation {
                    kept_cached: false,
                    value: num(1.0),
                    warnings: Vec::new(),
                    writes: vec![CellWriteOp {
                        target: CellAddress::in_row(
                            SheetRef::Page(PageId { index: 0 }),
                            SectionName::from_static("Geometry"),
                            RowTarget::at(3),
                            CellName::from_static("X"),
                        ),
                        formula: String::from("1"),
                        value: num(1.0),
                    }],
                    evaluations: 1,
                })
            }
        }
        let (mut doc, shape) = doc_with(vec![cell("Alpha", Some("0"), Some(num(0.0)))]);
        let mut graph = DepGraph::build_with(&doc, &mut BadWriteEngine).expect("build");
        let outcome = recalc_with(
            &mut doc,
            &[address(shape, "Alpha")],
            &RecalcOptions::new(),
            &mut graph,
            &mut BadWriteEngine,
        )
        .expect("recalc");
        assert_eq!(outcome.updates.len(), 1);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|warning| matches!(warning.kind, RecalcWarningKind::MissingReference(_)))
        );
    }
}
