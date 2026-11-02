//! Reference resolution and inheritance (ADR 0054).
//!
//! A formula's references resolve through the rungs of ADR 0013 (local,
//! master, style chain, section default, hard default), and an inherited
//! formula is evaluated against the sheet it is evaluated *for*: an
//! instance's `Width` in a master's geometry row is the instance's
//! width. [`ResolvedCell`] names where the formula and the cached value
//! came from, so the evaluator and the recalc pass can report provenance.
//!
//! The resolver is the only place that knows the row shapes of the
//! reference forms (ADR 0054): `Char.Size` is a column of the `Character`
//! section's rows while `Prop.Cost` is a named row's default cell. Names
//! match case-insensitively; the file's own spelling is what later gets
//! written back. A name that resolves to nothing becomes `#REF!` with one
//! warning ([`EvalWarningKind::UnsupportedReference`], the `MissingReference`
//! of "About Error Values"), never a Rust error.
//!
//! The value side prefers the local cell's own cached value, snapshot of
//! an `Inh` cell included (ADR 0054 amendment, ADR 0057): the snapshot is
//! the value for *this* sheet at save time. Provenance still comes from
//! the rung that defines the formula, so a snapshot cannot hide an
//! inheritance bug.

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellLookup, CellName, CellValue, Document, Master, MasterId, PageId, Shape, ShapeId,
    Sheet, SheetRef, StyleId, ValueSource,
};

use crate::Error;
use crate::eval::{EvalContext, EvalWarning, EvalWarningKind};
use crate::formula::{CellRef, Formula};
use crate::names::{self, RowKind};
use crate::value::{ErrorCode, Value};

/// The formula `.vsdx` writes on a cell whose value is inherited. It
/// defines nothing; its cached value is the sheet's own snapshot.
const INHERIT_FORMULA: &str = "Inh";

/// How many nested formula evaluations one `cell_value` call may start
/// (ADR 0051 bounds nesting inside one formula; this bounds the chain
/// across formulas). Recursion never runs away.
const MAX_EVAL_DEPTH: u32 = 32;

/// One resolved cell: the formula that defines it and the value to use
/// (ADR 0054).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCell {
    /// The defining formula's source text, or `None` when no rung has one
    /// (a literal or an absent cell).
    pub formula: Option<String>,
    /// The value to use: the cached value of the defining rung, or the
    /// local snapshot of an `Inh` cell (ADR 0054 amendment), or `None`.
    pub value: Option<Value>,
    /// Which rung defines the cell.
    pub source: ValueSource,
}

/// One `SETF` write a formula performed, recorded but **not** applied
/// (ADR 0056, P6-12 review F1): the recalc pass applies every write
/// through the model's mutation API, so one write path owns the
/// document and a write is inside the pass's `CellUpdate` list, its
/// dirty set and the undo snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedWrite {
    /// The sheet the formula was evaluated on (the write's context).
    pub sheet: SheetRef,
    /// The target as written.
    pub target: CellRef,
    /// The value to write.
    pub value: Value,
}

/// One warning with the cell whose formula was under evaluation when it
/// fired (P6-12 review F16): a warning raised inside a referenced cell's
/// formula names that cell, not the outer one.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedWarning {
    /// The referenced cell whose formula was being evaluated, or `None`
    /// for the formula the caller asked for.
    pub origin: Option<CellRef>,
    /// The warning itself.
    pub warning: EvalWarning,
}

/// Resolves references against a [`Document`] (ADR 0054).
///
/// The resolver is the [`EvalContext`] the evaluator runs against: it
/// reads cells with inheritance, records basic `SETF` writes (the recalc
/// pass applies them, ADR 0056) and collects the structured warnings of
/// ADR 0053.
pub struct DocumentResolver<'d> {
    document: &'d mut Document,
    warnings: Vec<TracedWarning>,
    writes: Vec<RecordedWrite>,
    /// The nested cell whose formula is under evaluation (the innermost
    /// first), for the attribution rule of [`TracedWarning`].
    origins: Vec<CellRef>,
    depth: u32,
    evaluations: u32,
    /// Whether an evaluation stood back silently (a Visio-owned call,
    /// task P7-31): the caller keeps the cell's cached value.
    kept_cached: bool,
}

impl<'d> DocumentResolver<'d> {
    /// A resolver over `document`.
    pub fn new(document: &'d mut Document) -> Self {
        Self {
            document,
            warnings: Vec::new(),
            writes: Vec::new(),
            origins: Vec::new(),
            depth: 0,
            evaluations: 0,
            kept_cached: false,
        }
    }

    /// The document the resolver reads.
    pub fn document(&self) -> &Document {
        self.document
    }

    /// The warnings collected so far, each with the cell whose formula
    /// was being evaluated when it fired.
    pub fn warnings(&self) -> &[TracedWarning] {
        &self.warnings
    }

    /// The `SETF` writes collected so far (recorded, never applied).
    pub fn writes(&self) -> &[RecordedWrite] {
        &self.writes
    }

    /// How many **nested** formula evaluations the resolver ran: reads
    /// of a cell that caches nothing re-evaluate its formula inline
    /// (ADR 0057). The recalc budget counts these too (ADR 0055).
    pub fn evaluations(&self) -> u32 {
        self.evaluations
    }

    /// Whether an evaluation stood back silently (a Visio-owned call
    /// such as `_XFTRIGGER`, task P7-31). A **nested** such call (inside
    /// a referenced cell's formula) sets the flag too: like a nested
    /// unknown function, it makes the outer evaluation keep its cached
    /// value.
    pub fn kept_cached(&self) -> bool {
        self.kept_cached
    }

    /// The collected warnings and writes, taking them out of the
    /// resolver.
    pub fn into_parts(self) -> (Vec<TracedWarning>, Vec<RecordedWrite>) {
        (self.warnings, self.writes)
    }

    /// Records one warning against the nested cell being evaluated, if
    /// any (P6-12 review F16).
    fn record(&mut self, warning: EvalWarning) {
        self.warnings.push(TracedWarning {
            origin: self.origins.last().cloned(),
            warning,
        });
    }

    /// Resolves one referenced cell (ADR 0054).
    ///
    /// `sheet` is the sheet the formula being evaluated lives on: local
    /// names bind to it, so a master's formula evaluated for an instance
    /// reads the instance's `Width`. A name that resolves to nothing
    /// yields `#REF!` with one warning; it is never an error.
    pub fn resolve(&mut self, sheet: SheetRef, reference: &CellRef) -> Result<ResolvedCell, Error> {
        let found = self.find(sheet, reference);
        Ok(match found {
            Some(found) => self.resolved_cell(found),
            None => self.missing(sheet, reference),
        })
    }

    /// The `#REF!` answer for a name that resolves to nothing, with
    /// exactly one warning (ADR 0054, "About Error Values").
    fn missing(&mut self, sheet: SheetRef, reference: &CellRef) -> ResolvedCell {
        self.record(EvalWarning {
            sheet,
            kind: EvalWarningKind::UnsupportedReference(describe(reference)),
        });
        ResolvedCell {
            formula: None,
            value: Some(Value::Err(ErrorCode::Ref)),
            source: ValueSource::HardDefault,
        }
    }

    /// Maps what a resolution found to the answer, reading the cached
    /// value through `Value::from_cached` (P2-14, P6-03).
    fn resolved_cell(&self, found: Found) -> ResolvedCell {
        let value = found
            .value
            .and_then(|(value, unit)| Value::from_cached(&value, unit).ok());
        ResolvedCell {
            formula: found.formula,
            value,
            source: found.source,
        }
    }

    /// The [`Target`] the reference's sheet parts name, or `None` when
    /// one of them names nothing (ADR 0054).
    fn target(&self, sheet: SheetRef, parts: &[String]) -> Option<Target> {
        let mut current = Target::Sheet(sheet);
        let mut index = 0;
        while let Some(part) = parts.get(index) {
            let part = unquote_part(part);
            index += 1;
            if names::eq_ci(&part, "Styles") {
                // `Styles!Ledger`: the stylesheet row named next.
                let name = unquote_part(parts.get(index)?);
                index += 1;
                current = Target::Style(self.style_named(&name)?);
                continue;
            }
            current = self.step(current, &part)?;
        }
        Some(current)
    }

    /// One sheet part of a reference, applied to `current`.
    fn step(&self, current: Target, part: &str) -> Option<Target> {
        if names::eq_ci(part, "ThePage") {
            return Some(Target::Sheet(SheetRef::Page(self.page_context(current)?)));
        }
        if names::eq_ci(part, "TheDoc") {
            return Some(Target::Document);
        }
        if let Some(name) = bracketed(part, "Masters") {
            let master = self.master_named(&name)?;
            let content = master_content(self.document, master)?;
            return Some(Target::Sheet(SheetRef::Shape(content)));
        }
        if let Some(name) = bracketed(part, "Pages") {
            return Some(Target::Sheet(SheetRef::Page(self.page_named(&name)?)));
        }
        let (name, visio_id) = split_numbered(part);
        if let Some(visio_id) = visio_id {
            let page = self.page_context(current);
            let found = page
                .and_then(|page| shape_by_visio_id(self.document, page, visio_id, name.as_str()))
                .or_else(|| self.descendant_by_visio_id(current, visio_id));
            return found.map(|shape| Target::Sheet(SheetRef::Shape(shape)));
        }
        self.named_shape(current, &name)
            .map(|shape| Target::Sheet(SheetRef::Shape(shape)))
    }

    /// The named sub-shape or page shape one bare sheet part names: the
    /// target's descendants first (a master's `Hub`), then the shapes of
    /// the target's page (`Wheel` on the same page).
    fn named_shape(&self, current: Target, name: &str) -> Option<ShapeId> {
        self.descendant_named(current, name).or_else(|| {
            let page = self.page_context(current)?;
            let roots: Vec<ShapeId> = self
                .document
                .page(page)
                .map(|page| page.shapes.clone())
                .unwrap_or_default();
            first_named(self.document, &roots, name)
        })
    }

    /// The named descendant of the current shape's sheet: the shape's own
    /// children first (a master's `Hub`), then the root itself.
    fn descendant_named(&self, current: Target, name: &str) -> Option<ShapeId> {
        let root = self.sheet_shape(current)?;
        let children = self
            .document
            .shape(root)
            .map(|shape| shape.children.clone())
            .unwrap_or_default();
        // The root's own name never names the root: a reference part names
        // a shape *on* the current sheet.
        first_named(self.document, &children, name).or_else(|| {
            let roots = vec![root];
            first_named(self.document, &roots, name)
        })
    }

    /// The descendant of the current sheet whose Visio `ID` is `visio_id`
    /// (a `Sheet.8` part on a master's sheet, which sits on no page).
    fn descendant_by_visio_id(&self, current: Target, visio_id: i32) -> Option<ShapeId> {
        let root = self.sheet_shape(current)?;
        let children: Vec<ShapeId> = self
            .document
            .shape(root)
            .map(|shape| shape.children.clone())
            .unwrap_or_default();
        first_by_visio_id(self.document, &children, visio_id)
    }

    /// The shape whose sheet the current target carries, if any.
    fn sheet_shape(&self, current: Target) -> Option<ShapeId> {
        match current {
            Target::Sheet(SheetRef::Shape(shape)) => Some(shape),
            _ => None,
        }
    }

    /// The page the current target sits on.
    fn page_context(&self, current: Target) -> Option<PageId> {
        match current {
            Target::Sheet(SheetRef::Page(page)) => Some(page),
            Target::Sheet(SheetRef::Shape(shape)) => self.document.page_of(shape),
            Target::Style(_) | Target::Document => None,
        }
    }

    /// The stylesheet row named `name`, matched case-insensitively.
    fn style_named(&self, name: &str) -> Option<StyleId> {
        self.document
            .stylesheet
            .styles
            .iter()
            .find(|style| names::eq_ci(&style.name, name) || names::eq_ci(&style.name_u, name))
            .map(|style| style.id)
    }

    /// The master named `name`, matched case-insensitively.
    fn master_named(&self, name: &str) -> Option<MasterId> {
        self.document
            .masters
            .iter()
            .enumerate()
            .find(|(_, master)| {
                names::eq_ci(&master.name, name) || names::eq_ci(&master.name_u, name)
            })
            .map(|(index, _)| MasterId {
                index: u32::try_from(index).unwrap_or(u32::MAX),
            })
    }

    /// The page named `name`, matched case-insensitively.
    fn page_named(&self, name: &str) -> Option<PageId> {
        self.document
            .pages
            .iter()
            .find(|page| names::eq_ci(&page.name, name) || names::eq_ci(&page.name_u, name))
            .map(|page| page.id)
    }

    /// What a reference names inside its target (ADR 0054).
    fn find(&self, sheet: SheetRef, reference: &CellRef) -> Option<Found> {
        let target = self.target(sheet, &reference.sheets)?;
        let (section, names) = split_names(reference);
        match target {
            Target::Document => self.find_document_cell(section, names),
            Target::Style(style) => {
                let style = self.document.style(style)?;
                match section {
                    Some(section) => self.find_section_in(&style.sheet, &[], section, names),
                    None => self.find_bare_in(&style.sheet, names),
                }
            }
            Target::Sheet(target) => self.find_sheet_cell(target, section, names),
        }
    }

    /// A document-sheet cell (`TheDoc!DocScale`). The model keeps
    /// the document sheet as its `properties`, so a property by that name
    /// is the cell.
    fn find_document_cell(&self, section: Option<&str>, names: &[String]) -> Option<Found> {
        if section.is_some() {
            return None;
        }
        let name = names.first()?;
        let property = self
            .document
            .properties
            .iter()
            .find(|property| names::eq_ci(&property.name, name));
        if let Some(property) = property {
            return Some(Found {
                formula: None,
                source: ValueSource::Local,
                value: Some((CellValue::Str(property.value.clone()), Unit::Unitsless)),
            });
        }
        Some(Found {
            formula: None,
            source: ValueSource::HardDefault,
            value: self.table_default(None, name),
        })
    }

    /// A bare cell name on a sheet with no rungs behind it (a style's
    /// sheet): the sheet's own entry, then the table default.
    fn find_bare_in(&self, sheet: &Sheet, names: &[String]) -> Option<Found> {
        let name = names.first()?;
        if let Some((cell, _)) = names::find_entry(sheet, name) {
            let value = cell.value.clone().map(|value| (value, cell.unit));
            if defines(cell) {
                return Some(Found {
                    formula: cell.formula.clone(),
                    source: ValueSource::Local,
                    value,
                });
            }
            if value.is_some() {
                // An `Inh` snapshot is this sheet's own value (ADR 0054).
                return Some(Found {
                    formula: None,
                    source: ValueSource::Local,
                    value,
                });
            }
        }
        self.table_default(None, name).map(|value| Found {
            formula: None,
            source: ValueSource::HardDefault,
            value: Some(value),
        })
    }

    /// One cell of a shape or page sheet.
    fn find_sheet_cell(
        &self,
        target: SheetRef,
        section: Option<&str>,
        names: &[String],
    ) -> Option<Found> {
        match section {
            Some(section) => self.find_section_cell(target, section, names),
            None => self.find_bare_cell(target, names),
        }
    }

    /// A bare cell name (`Width`, `Sheet.12!FillForegnd`): the model's
    /// lookup runs the rungs of ADR 0013 for it. The local entry's own
    /// cached value wins over the rung's (the `Inh` snapshot rule), and a
    /// case-only spelling difference still resolves.
    fn find_bare_cell(&self, target: SheetRef, names: &[String]) -> Option<Found> {
        let name = names.first()?;
        let sheet = self.sheet_of(target)?;
        let own = names::find_entry(sheet, name);
        let spelling = own
            .map(|(cell, _)| cell.name.as_str().to_owned())
            .or_else(|| names::table_spelling(name))
            .unwrap_or_else(|| name.clone());
        let cell_name = CellName::new(spelling);
        let resolved = match target {
            SheetRef::Shape(shape) => self.document.resolve(shape, &cell_name).ok()?,
            SheetRef::Page(page) => self.document.resolve_page_cell(page, &cell_name).ok()?,
        };
        if resolved.value.is_none() && resolved.formula.is_none() {
            // No rung and no table entry: the name resolves to nothing.
            return None;
        }
        let snapshot = own
            .and_then(|(cell, _)| cell.value.clone().map(|value| (value, cell.unit)))
            .or_else(|| {
                resolved
                    .value
                    .cloned()
                    .map(|value| (value, self.rung_unit(target, &cell_name)))
            });
        Some(Found {
            formula: resolved.formula.map(str::to_owned),
            source: resolved.source,
            value: snapshot,
        })
    }

    /// The unit the rung that defines `cell` recorded, when the resolver
    /// can find that rung's cell. A style or default rung keeps no cell
    /// the resolver can name, so its value reads back unit-less.
    fn rung_unit(&self, target: SheetRef, cell: &CellName) -> Unit {
        let Some(sheet) = self.sheet_of(target) else {
            return Unit::Unitsless;
        };
        if let Some((found, _)) = names::find_entry(sheet, cell.as_str())
            && defines(found)
        {
            return found.unit;
        }
        if let SheetRef::Shape(shape) = target {
            for (_, rung) in master_rungs(self.document, shape) {
                let Some(rung_sheet) = self.document.shape(rung).map(|shape| &shape.sheet) else {
                    continue;
                };
                if let Some((found, _)) = names::find_entry(rung_sheet, cell.as_str())
                    && defines(found)
                {
                    return found.unit;
                }
            }
        }
        Unit::Unitsless
    }

    /// A section-form reference (`Char.Size`, `Prop.Cost`, `Scratch.B3`,
    /// `Geometry1.X2`): the section's row shape decides whether the name
    /// is a column or a named row (ADR 0054).
    fn find_section_cell(
        &self,
        target: SheetRef,
        section: &str,
        names: &[String],
    ) -> Option<Found> {
        let sheet = self.sheet_of(target)?;
        let rungs = match target {
            SheetRef::Shape(shape) => master_rungs(self.document, shape),
            SheetRef::Page(_) => Vec::new(),
        };
        self.find_section_in(sheet, &rungs, section, names)
    }

    /// The section walk behind [`Self::find_section_cell`], over one
    /// sheet and the master rungs behind it (empty for a page sheet or a
    /// style's sheet).
    fn find_section_in(
        &self,
        sheet: &Sheet,
        rungs: &[(MasterId, ShapeId)],
        section: &str,
        names: &[String],
    ) -> Option<Found> {
        let name = names.first()?;
        let explicit = names.get(1).map(String::as_str);
        let (base, instance) = names::split_section_name(section);
        let canon = names::canonical_section(base.as_str());

        // The row shape: a column of the section's rows (the ShapeSheet
        // table, or the cells the file itself writes in those rows), else
        // a named row of a named-row section — the one rule of
        // `crate::names`, shared with the dependency graph.
        let local_section = names::match_section(sheet, canon.as_str(), instance);
        let kind = names::row_kind(canon.as_str(), local_section, name);

        let mut snapshot = None;
        let mut saw_row = false;
        let mut saw_cell = false;

        // 1. Local rung.
        if let Some(section) = local_section {
            if names::row_known(section, &kind) {
                saw_row = true;
            }
            if let Some(cell) = names::section_cell(section, &kind, explicit) {
                saw_cell = true;
                if cell.value.is_some() {
                    snapshot = cell.value.clone().map(|value| (value, cell.unit));
                }
                if defines(cell) {
                    return Some(Found {
                        formula: cell.formula.clone(),
                        source: ValueSource::Local,
                        value: cell.value.clone().map(|value| (value, cell.unit)),
                    });
                }
            }
        }

        // 2. Master rungs, with the sub-shape and position rules of
        //    ADR 0013 (no style rung for a section cell, ADR 0019).
        for (master, rung) in rungs {
            let Some(rung_sheet) = self.document.shape(*rung).map(|shape| &shape.sheet) else {
                continue;
            };
            let Some(section) = names::match_section(rung_sheet, canon.as_str(), instance) else {
                continue;
            };
            if names::row_known(section, &kind) {
                saw_row = true;
            }
            if let Some(cell) = names::section_cell(section, &kind, explicit) {
                saw_cell = true;
                if defines(cell) {
                    let value =
                        snapshot.or_else(|| cell.value.clone().map(|value| (value, cell.unit)));
                    return Some(Found {
                        formula: cell.formula.clone(),
                        source: ValueSource::Master(*master),
                        value,
                    });
                }
            }
        }

        // 3./4. The section default and the hard default, for a name the
        //       row shape knows. Row 0 is the section's own first row and
        //       exists conceptually; a named row or a higher row must
        //       exist on some rung (ADR 0054: a missing row is `#REF!`).
        let row_ok = match &kind {
            RowKind::Column { index, .. } => *index == 0 || saw_row,
            RowKind::Named(_) => saw_row,
        };
        if !row_ok {
            return None;
        }
        let known = saw_cell || names::table_column(canon.as_str(), kind.cell_name());
        if !known {
            return None;
        }
        Some(Found {
            formula: None,
            source: ValueSource::HardDefault,
            value: self.table_default(Some(canon.as_str()), kind.cell_name()),
        })
    }

    /// The sheet of a shape or page target.
    fn sheet_of(&self, target: SheetRef) -> Option<&Sheet> {
        match target {
            SheetRef::Shape(shape) => self.document.shape(shape).map(|shape| &shape.sheet),
            SheetRef::Page(page) => self.document.page(page).map(|page| &page.sheet),
        }
    }

    /// The hard default of a table cell, as a cached value.
    fn table_default(&self, section: Option<&str>, name: &str) -> Option<(CellValue, Unit)> {
        let kind = names::column_kind(section, name)?;
        hard_default(kind)
    }

    /// Evaluates the formula a reference's cell defines, in `sheet`'s
    /// context (ADR 0054: an inherited formula binds local names to the
    /// sheet it is evaluated *for*).
    fn eval_reference(&mut self, sheet: SheetRef, reference: &CellRef) -> Result<Value, Error> {
        let resolved = self.resolve(sheet, reference)?;
        // The value side prefers the cell's cached value (ADR 0054, the
        // `Inh` snapshot rule): reading a cell never re-runs its
        // formula, so a referenced unknown function keeps that cell's
        // cached value (ADR 0053) and a read is idempotent (ADR 0055's
        // determinism). The formula is evaluated only when nothing is
        // cached (the fill-in case of ADR 0057).
        if let Some(value) = resolved.value {
            return Ok(value);
        }
        if let Some(source) = resolved.formula.as_deref() {
            let formula = Formula::new(source);
            // The warnings and the recorded `SETF` writes of this nested
            // evaluation belong to the referenced cell (P6-12 reviews
            // F16 and F1), and the evaluation counts against the pass's
            // budget (F7).
            self.origins.push(reference.clone());
            self.evaluations = self.evaluations.saturating_add(1);
            let outcome = crate::eval::eval_formula(&formula, sheet, self);
            self.origins.pop();
            if let Ok(value) = outcome {
                return Ok(value);
            }
            // A stored formula that does not parse, on a cell that
            // caches nothing: there is no value to answer with.
        }
        Ok(Value::Err(ErrorCode::Ref))
    }

    /// The sheet and slot a `SETF` write would land in (ADR 0056): the
    /// reference must name a writable sheet cell that exists, checked
    /// with the same [`crate::names`] lookup the pass addresses the
    /// write with. [`Error::SetfRefused`] names the refusal (the
    /// evaluator turns it into `#REF!` with one warning).
    fn write_target(
        &self,
        sheet: SheetRef,
        reference: &CellRef,
    ) -> Result<(SheetRef, names::Slot), Error> {
        let target = self
            .target(sheet, &reference.sheets)
            .ok_or(Error::SetfRefused("target does not exist"))?;
        let target = match target {
            Target::Sheet(target) => target,
            Target::Style(_) | Target::Document => {
                return Err(Error::SetfRefused("target is not writable"));
            }
        };
        let (section, parts) = split_names(reference);
        let found_sheet = self
            .sheet_of(target)
            .ok_or(Error::SetfRefused("target does not exist"))?;
        let name = parts
            .first()
            .ok_or(Error::SetfRefused("target does not exist"))?;
        let slot = match section {
            Some(section) => {
                let explicit = parts.get(1).map(String::as_str);
                let (base, instance) = names::split_section_name(section);
                let canon = names::canonical_section(base.as_str());
                let local_section = names::match_section(found_sheet, canon.as_str(), instance);
                let kind = names::row_kind(canon.as_str(), local_section, name);
                names::section_slot(found_sheet, canon.as_str(), instance, &kind, explicit)
                    .ok_or(Error::SetfRefused("target does not exist"))?
            }
            None => {
                let (_, slot) = names::find_entry(found_sheet, name)
                    .ok_or(Error::SetfRefused("target does not exist"))?;
                slot
            }
        };
        Ok((target, slot))
    }
}

impl EvalContext for DocumentResolver<'_> {
    fn cell_value(&mut self, sheet: SheetRef, reference: &CellRef) -> Result<Value, Error> {
        if self.depth >= MAX_EVAL_DEPTH {
            // A formula chain deeper than the bound keeps its cached
            // values: the resolver never recurses away.
            return Ok(Value::Err(ErrorCode::Ref));
        }
        self.depth += 1;
        let outcome = self.eval_reference(sheet, reference);
        self.depth -= 1;
        outcome
    }

    fn set_cell(
        &mut self,
        sheet: SheetRef,
        reference: &CellRef,
        value: Value,
    ) -> Result<(), Error> {
        // The value must be storable before anything is recorded.
        let _cached = value.to_cached()?;
        // The write is **recorded, not applied** (ADR 0056, P6-12
        // review F1): the recalc pass applies it through the model's
        // mutation API, so one write path owns the document and a write
        // (a nested `SETF` included) gets a `CellUpdate`, dirties its
        // dependents and sits inside the undo snapshot. The check below
        // is the same lookup the pass addresses the write with, so a
        // target this refuses is `#REF!` and no write is lost.
        self.write_target(sheet, reference)?;
        self.writes.push(RecordedWrite {
            sheet,
            target: reference.clone(),
            value,
        });
        Ok(())
    }

    fn warn(&mut self, warning: EvalWarning) {
        self.record(warning);
    }

    fn keep_cached(&mut self) {
        self.kept_cached = true;
    }
}

/// What one resolution found (ADR 0054): the formula of the rung that
/// defines the cell, which rung that is, and the value to use.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    /// The defining formula's source text.
    formula: Option<String>,
    /// The rung that defines the cell.
    source: ValueSource,
    /// The cached value and the unit it was written with.
    value: Option<(CellValue, Unit)>,
}

/// The target a reference's sheet parts name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A shape or page sheet.
    Sheet(SheetRef),
    /// A stylesheet row's sheet.
    Style(StyleId),
    /// The document sheet.
    Document,
}

/// Removes the quoting of a nonstandard name (`'2-C'`, with `''` as the
/// quote escape) and returns the name itself.
fn unquote_part(part: &str) -> String {
    let unescaped = part.replace("''", "'");
    if unescaped.len() >= 2
        && unescaped.starts_with('\'')
        && unescaped.ends_with('\'')
        && let Some(inner) = unescaped.get(1..unescaped.len() - 1)
    {
        return inner.to_owned();
    }
    unescaped
}

/// Splits `Masters[Bearing]` into the name inside its brackets when the part
/// carries `prefix`.
fn bracketed(part: &str, prefix: &str) -> Option<String> {
    let rest = part.strip_prefix(prefix)?.strip_prefix('[')?;
    let name = rest.strip_suffix(']')?;
    Some(unquote_part(name))
}

/// Splits `Sheet.8` or `Frame.2` into the written name and the
/// numeric suffix. A part with no numeric suffix comes back whole.
fn split_numbered(part: &str) -> (String, Option<i32>) {
    let Some(dot) = part.rfind('.') else {
        return (part.to_owned(), None);
    };
    let tail = part.get(dot + 1..).unwrap_or("");
    if tail.is_empty() || !tail.chars().all(|character| character.is_ascii_digit()) {
        return (part.to_owned(), None);
    }
    match tail.parse::<i32>() {
        Ok(visio_id) => (part.get(..dot).unwrap_or("").to_owned(), Some(visio_id)),
        Err(_) => (part.to_owned(), None),
    }
}

/// The reference as written, for the warning a missing name records.
fn describe(reference: &CellRef) -> String {
    let mut text = reference.sheets.join("!");
    if !text.is_empty() {
        text.push('!');
    }
    if let Some(section) = reference.section.as_deref() {
        text.push_str(section);
        text.push('.');
    }
    text.push_str(&reference.names.join("."));
    text
}

/// Splits a reference's written names into the section part and the name
/// parts (`Prop.Cost.Value` is section `Prop`, names `Cost`, `Value`).
pub(crate) fn split_names(reference: &CellRef) -> (Option<&str>, &[String]) {
    if let Some(section) = reference.section.as_deref() {
        return (Some(section), &reference.names[..]);
    }
    if reference.names.len() > 1
        && let Some(first) = reference.names.first()
    {
        // A section that a parser left in the name list.
        return (
            Some(first.as_str()),
            reference.names.get(1..).unwrap_or(&[]),
        );
    }
    (None, &reference.names[..])
}

/// Whether a cell defines its source (ADR 0013): `Inh` defines nothing.
pub(crate) fn defines(cell: &Cell) -> bool {
    if cell.formula.as_deref() == Some(INHERIT_FORMULA) {
        return false;
    }
    cell.value.is_some() || cell.formula.is_some()
}

/// The hard default of one cell type (ADR 0013: 0, empty string, false,
/// black), in the canonical units of ADR 0011.
fn hard_default(kind: &str) -> Option<(CellValue, Unit)> {
    let value = match kind {
        "Number" => CellValue::Number(0.0),
        "String" => CellValue::Str(String::new()),
        "Bool" => CellValue::Bool(false),
        "Color" => CellValue::Color(larrygram_model::Color::new(0, 0, 0)),
        "List" => CellValue::List(String::new()),
        _ => return None,
    };
    Some((value, Unit::Unitsless))
}

/// The master rungs of `shape` in rung order (ADR 0013): the sub-shape
/// the instance's path walks down to (or the master's own shape), once per
/// rung, with the position rule for masters with several top-level
/// shapes. A cyclic master chain is cut at the first repeat and the walk
/// is bounded.
pub(crate) fn master_rungs(doc: &Document, shape: ShapeId) -> Vec<(MasterId, ShapeId)> {
    const MAX_RUNGS: usize = 16;
    let mut rungs = Vec::new();
    let Some((first, root, mut path)) = instance_chain_start(doc, shape) else {
        return rungs;
    };
    let position = sibling_position(doc, root);
    let mut visited: Vec<MasterId> = Vec::new();
    let mut current = first;
    loop {
        if visited.contains(&current) || rungs.len() >= MAX_RUNGS {
            break;
        }
        visited.push(current);
        let mut chained = None;
        if let Some(content) = doc
            .master(current)
            .and_then(|master| master_shape(doc, master, position))
        {
            let target = walk_down(doc, content, &path).unwrap_or(content);
            rungs.push((current, target.id));
            chained = target.master;
            path.clear();
        }
        match chained {
            Some(next) => current = next,
            None => break,
        }
    }
    rungs
}

/// The master of the nearest shape that is an instance (the shape itself
/// or an ancestor), that instance root, and the child positions from the
/// root down to `shape` (the lookup's chain start).
fn instance_chain_start(doc: &Document, shape: ShapeId) -> Option<(MasterId, ShapeId, Vec<usize>)> {
    let mut path = Vec::new();
    let mut visited = Vec::new();
    let mut current = shape;
    loop {
        if visited.contains(&current) {
            return None;
        }
        visited.push(current);
        let node = doc.shape(current)?;
        if let Some(master) = node.master {
            path.reverse();
            return Some((master, current, path));
        }
        let parent = node.parent?;
        let position = doc
            .shape(parent)?
            .children
            .iter()
            .position(|child| *child == current)?;
        path.push(position);
        current = parent;
    }
}

/// The position of `shape` among its siblings (the lookup's position
/// rule).
fn sibling_position(doc: &Document, shape: ShapeId) -> Option<usize> {
    let node = doc.shape(shape)?;
    if let Some(parent) = node.parent {
        return doc
            .shape(parent)?
            .children
            .iter()
            .position(|child| *child == shape);
    }
    doc.pages
        .iter()
        .find_map(|page| page.shapes.iter().position(|entry| *entry == shape))
}

/// The master shape an instance at `position` matches (P2-06b decision 2):
/// the master's only shape when it has exactly one, the shape at
/// `position` otherwise, degrading to the first.
fn master_shape<'a>(
    doc: &'a Document,
    master: &Master,
    position: Option<usize>,
) -> Option<&'a Shape> {
    match master.shapes.as_slice() {
        [] => None,
        [only] => doc.shape(*only),
        many => {
            let picked = position
                .and_then(|position| many.get(position))
                .or_else(|| many.first())?;
            doc.shape(*picked)
        }
    }
}

/// The corresponding shape of `root` at the child positions `path`, or
/// `None` when the structure does not match.
fn walk_down<'a>(doc: &'a Document, root: &'a Shape, path: &[usize]) -> Option<&'a Shape> {
    let mut current = root;
    for position in path {
        let child = *current.children.get(*position)?;
        current = doc.shape(child)?;
    }
    Some(current)
}

/// The master's content shape: its only shape, else its first.
fn master_content(doc: &Document, master: MasterId) -> Option<ShapeId> {
    let master = doc.master(master)?;
    master_shape(doc, master, Some(0)).map(|shape| shape.id)
}

/// The first shape of `roots` (a depth-first walk, bounded by the arena)
/// whose name matches `name` case-insensitively.
fn first_named(doc: &Document, roots: &[ShapeId], name: &str) -> Option<ShapeId> {
    walk_shapes(doc, roots, |shape| {
        names::eq_ci(&shape.name, name)
            || names::eq_ci(&shape.name_u, name)
            || shape
                .name_u
                .rsplit_once('.')
                .is_some_and(|(base, _)| names::eq_ci(base, name))
    })
}

/// The first shape of `roots` whose Visio `ID` is `visio_id`, preferring
/// one whose written name matches `name`.
fn shape_by_visio_id(doc: &Document, page: PageId, visio_id: i32, name: &str) -> Option<ShapeId> {
    let roots = doc.page(page)?.shapes.clone();
    let by_id = |shape: &Shape| shape.visio_id == visio_id;
    let named = |shape: &Shape| {
        by_id(shape)
            && (name == "Sheet"
                || names::eq_ci(&shape.name, name)
                || shape
                    .name_u
                    .rsplit_once('.')
                    .is_some_and(|(base, _)| names::eq_ci(base, name)))
    };
    walk_shapes(doc, &roots, named).or_else(|| walk_shapes(doc, &roots, by_id))
}

/// The first shape of `roots` whose Visio `ID` is `visio_id` (the master
/// fallback of a `Sheet.8` part).
fn first_by_visio_id(doc: &Document, roots: &[ShapeId], visio_id: i32) -> Option<ShapeId> {
    walk_shapes(doc, roots, |shape| shape.visio_id == visio_id)
}

/// The first shape in a bounded depth-first walk of the shape tree that
/// matches `wanted`.
fn walk_shapes<F>(doc: &Document, roots: &[ShapeId], wanted: F) -> Option<ShapeId>
where
    F: Fn(&Shape) -> bool,
{
    const MAX_VISITED: usize = 100_000;
    let mut stack: Vec<ShapeId> = roots.iter().rev().copied().collect();
    let mut visited = 0;
    while let Some(id) = stack.pop() {
        visited += 1;
        if visited > MAX_VISITED {
            return None;
        }
        let Some(shape) = doc.shape(id) else {
            continue;
        };
        if wanted(shape) {
            return Some(id);
        }
        stack.extend(shape.children.iter().rev().copied());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EvalWarningKind;
    use crate::names::NameParts;
    use larrygram_model::{Page, PageId, Row, Section, SectionName, Shape, Sheet};

    fn sheet() -> SheetRef {
        SheetRef::Page(PageId { index: 0 })
    }

    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    #[test]
    fn a_resolver_holds_its_document_and_warnings() {
        let mut doc = Document::new();
        let mut resolver = DocumentResolver::new(&mut doc);
        assert!(resolver.warnings().is_empty());
        resolver.warn(EvalWarning {
            sheet: sheet(),
            kind: EvalWarningKind::Cycle,
        });
        assert_eq!(resolver.warnings().len(), 1);
        assert_eq!(resolver.warnings()[0].origin, None);
        let (warnings, writes) = resolver.into_parts();
        assert_eq!(warnings.len(), 1);
        assert!(writes.is_empty());
    }

    #[test]
    fn the_document_is_borrowable() {
        let mut doc = Document::new();
        let resolver = DocumentResolver::new(&mut doc);
        assert_eq!(resolver.document().pages.len(), 0);
    }

    #[test]
    fn a_resolved_cell_holds_provenance() {
        let cell = ResolvedCell {
            formula: None,
            value: None,
            source: ValueSource::Local,
        };
        assert_eq!(cell.source, ValueSource::Local);
    }

    #[test]
    fn row_names_split_the_column_and_the_row_index() {
        assert_eq!(
            names::split_row_name("X2"),
            NameParts {
                base: String::from("X"),
                index: 2,
                full: String::from("X2"),
            }
        );
        assert_eq!(
            names::split_row_name("Font[3]"),
            NameParts {
                base: String::from("Font"),
                index: 3,
                full: String::from("Font[3]"),
            }
        );
        assert_eq!(names::split_row_name("Size").index, 0);
        // A name whose base is not a column keeps its digits as part of
        // the row name (`Cost5` is the row `Cost5`).
        assert_eq!(names::split_row_name("Cost5").full, "Cost5");
        assert_eq!(names::split_row_name("A[").full, "A[");
    }

    #[test]
    fn section_names_split_the_instance_index() {
        assert_eq!(
            names::split_section_name("Geometry1"),
            (String::from("Geometry"), 1)
        );
        assert_eq!(names::split_section_name("Char"), (String::from("Char"), 0));
    }

    #[test]
    fn section_names_come_in_short_and_long_spellings() {
        assert_eq!(names::canonical_section("Char"), "Character");
        assert_eq!(names::canonical_section("CHAR"), "Character");
        assert_eq!(names::canonical_section("Prop"), "Shape Data");
        assert_eq!(names::canonical_section("Property"), "Shape Data");
        assert_eq!(names::canonical_section("User"), "User-defined Cells");
        assert_eq!(names::canonical_section("Scratch"), "Scratch");
        assert_eq!(names::canonical_section("Geometry"), "Geometry");
    }

    #[test]
    fn quoted_names_unescape_their_quotes() {
        assert_eq!(unquote_part("'2-C'"), "2-C");
        assert_eq!(unquote_part("'A''B'"), "A'B");
        assert_eq!(unquote_part("Wheel"), "Wheel");
    }

    #[test]
    fn bracketed_parts_split_the_name() {
        assert_eq!(
            bracketed("Masters[Bearing]", "Masters"),
            Some(String::from("Bearing"))
        );
        assert_eq!(
            bracketed("Pages[Sheet-3]", "Pages"),
            Some(String::from("Sheet-3"))
        );
        assert_eq!(bracketed("Pages[Sheet-3]", "Masters"), None);
        assert_eq!(bracketed("Pages[]", "Pages"), Some(String::new()));
    }

    #[test]
    fn numbered_parts_split_the_visio_id() {
        assert_eq!(split_numbered("Sheet.8"), (String::from("Sheet"), Some(8)));
        assert_eq!(split_numbered("Frame.2"), (String::from("Frame"), Some(2)));
        assert_eq!(split_numbered("Wheel"), (String::from("Wheel"), None));
        assert_eq!(split_numbered("A.B"), (String::from("A.B"), None));
    }

    #[test]
    fn a_missing_name_describes_itself_in_the_warning() {
        let mut doc = Document::new();
        let mut resolver = DocumentResolver::new(&mut doc);
        let reference = CellRef {
            sheets: vec![String::from("Sheet.9")],
            section: Some(String::from("Geometry1")),
            names: vec![String::from("X2")],
        };
        let resolved = resolver.resolve(sheet(), &reference).expect("resolves");
        assert_eq!(resolved.value, Some(Value::Err(ErrorCode::Ref)));
        assert_eq!(
            resolver.warnings(),
            [TracedWarning {
                origin: None,
                warning: EvalWarning {
                    sheet: sheet(),
                    kind: EvalWarningKind::UnsupportedReference(String::from(
                        "Sheet.9!Geometry1.X2"
                    )),
                },
            }]
        );
    }

    #[test]
    fn hard_defaults_follow_the_cell_type() {
        assert_eq!(
            hard_default("Number"),
            Some((CellValue::Number(0.0), Unit::Unitsless))
        );
        assert_eq!(
            hard_default("String").map(|(value, _)| value),
            Some(CellValue::Str(String::new()))
        );
        assert_eq!(
            hard_default("Bool").map(|(value, _)| value),
            Some(CellValue::Bool(false))
        );
        assert_eq!(hard_default("Nope"), None);
    }

    #[test]
    fn the_table_knows_columns_and_spelling() {
        assert!(names::table_column("Character", "Size"));
        assert!(names::table_column("Shape Data", "Value"));
        assert!(!names::table_column("Shape Data", "Cost"));
        assert_eq!(names::table_spelling("size"), Some(String::from("Size")));
        assert_eq!(names::table_spelling("nosuch"), None);
    }

    #[test]
    fn section_level_cells_come_before_row_cells() {
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section.cells.push(number_cell("NoFill", 1.0));
        let mut row = Row::new();
        row.cells.push(number_cell("X", 2.0));
        section.rows.push(row);
        let kind = RowKind::Column {
            column: String::from("NoFill"),
            index: 0,
        };
        let cell = names::section_cell(&section, &kind, None).expect("cell");
        assert_eq!(cell.name.as_str(), "NoFill");
        // The row's own column cell comes second.
        let kind = RowKind::Column {
            column: String::from("X"),
            index: 0,
        };
        assert_eq!(
            names::section_cell(&section, &kind, None).map(|c| c.name.as_str()),
            Some("X")
        );
    }

    #[test]
    fn named_rows_match_their_row_name() {
        let mut section = Section::new(SectionName::from_static("Property"));
        let mut row = Row::new();
        row.name = Some(String::from("Cost"));
        row.cells.push(number_cell("Value", 3.0));
        section.rows.push(row);
        let kind = RowKind::Named(String::from("cost"));
        assert_eq!(
            names::section_cell(&section, &kind, None).map(|cell| cell.value.clone()),
            Some(Some(CellValue::Number(3.0)))
        );
        assert_eq!(
            names::section_cell(&section, &RowKind::Named(String::from("Nope")), None),
            None
        );
    }

    #[test]
    fn a_row_index_matches_the_row_ix_first() {
        let mut section = Section::new(SectionName::from_static("Geometry"));
        for (position, ix) in [Some(7), None, Some(3)].into_iter().enumerate() {
            let mut row = Row::new();
            row.ix = ix;
            row.cells.push(number_cell("X", (position as u32).into()));
            section.rows.push(row);
        }
        // The written index is the row's `IX` wherever the row sits.
        for (index, expected) in [(3usize, 2.0), (7, 0.0)] {
            let kind = RowKind::Column {
                column: String::from("X"),
                index,
            };
            assert_eq!(
                names::section_cell(&section, &kind, None).and_then(|cell| cell.value.clone()),
                Some(CellValue::Number(expected)),
                "index {index}"
            );
        }
        // A row no `IX` names falls back to the row at that position.
        let kind = RowKind::Column {
            column: String::from("X"),
            index: 1,
        };
        assert_eq!(
            names::section_cell(&section, &kind, None).and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        // Out of range is no row at all.
        let kind = RowKind::Column {
            column: String::from("X"),
            index: 9,
        };
        assert_eq!(names::section_cell(&section, &kind, None), None);
    }

    #[test]
    fn a_section_instance_names_the_nth_section_of_that_name() {
        let mut sheet = Sheet::new();
        for _ in 0..2 {
            let mut section = Section::new(SectionName::from_static("Geometry"));
            let mut row = Row::new();
            row.ix = Some(1);
            row.cells.push(number_cell("X", 0.0));
            section.rows.push(row);
            sheet.sections.push(section);
        }
        // `Geometry` and `Geometry1` are the first section, `Geometry2`
        // the second (ADR 0054's `Geometry1.X2`).
        for (instance, wanted) in [(0usize, 0usize), (1, 0), (2, 1)] {
            let found = names::match_section(&sheet, "Geometry", instance).expect("section");
            let position = sheet
                .sections
                .iter()
                .position(|section| std::ptr::eq(section, found))
                .expect("in the sheet");
            assert_eq!(position, wanted, "instance {instance}");
        }
        assert!(names::match_section(&sheet, "Geometry", 3).is_none());
    }

    #[test]
    fn a_document_sheet_cell_reads_the_property() {
        let mut doc = Document::new();
        doc.properties
            .push(larrygram_model::Property::new("DocScale", "1"));
        let mut resolver = DocumentResolver::new(&mut doc);
        let reference = CellRef {
            sheets: vec![String::from("TheDoc")],
            section: None,
            names: vec![String::from("docscale")],
        };
        let resolved = resolver.resolve(sheet(), &reference).expect("resolves");
        assert_eq!(resolved.source, ValueSource::Local);
        assert_eq!(resolved.formula, None);
    }

    #[test]
    fn writes_go_into_the_local_cell() {
        let mut doc = Document::new();
        let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(shape)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 1.0));
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(shape);
        doc.pages.push(page);
        let mut resolver = DocumentResolver::new(&mut doc);
        let reference = CellRef::new("PinX");
        // The value mapping belongs to P6-03: the write succeeds once the
        // mapping does, and refuses before that.
        let value = Value::Number(crate::value::Number::new(5.0, crate::units::Unit::Inches));
        match value.to_cached() {
            Ok(_) => {
                resolver
                    .set_cell(SheetRef::Shape(shape), &reference, value)
                    .expect("writes");
            }
            Err(_) => {
                assert!(
                    resolver
                        .set_cell(SheetRef::Shape(shape), &reference, value)
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn a_write_to_a_missing_target_is_refused() {
        let mut doc = Document::new();
        let mut resolver = DocumentResolver::new(&mut doc);
        let value = Value::Number(crate::value::Number::new(5.0, crate::units::Unit::Inches));
        assert!(
            resolver
                .set_cell(sheet(), &CellRef::new("PinX"), value)
                .is_err()
        );
    }

    #[test]
    fn a_read_prefers_the_cached_value_over_the_stored_formula() {
        // `PinX`'s stored formula would compute 2 and `Width`'s calls an
        // unknown function; the file's snapshots are 9 and 4. A read
        // answers the snapshot (ADR 0054) and an unknown function keeps
        // its cached value (ADR 0053), so `PinX` reads 9.
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let mut pin = Cell::new(CellName::new("PinX"));
        pin.formula = Some(String::from("Width*0.5"));
        pin.value = Some(CellValue::Number(9.0));
        page.sheet.cells.push(pin);
        let mut width = Cell::new(CellName::new("Width"));
        width.formula = Some(String::from("FOO(4)"));
        width.value = Some(CellValue::Number(4.0));
        page.sheet.cells.push(width);
        doc.pages.push(page);

        let mut resolver = DocumentResolver::new(&mut doc);
        for (name, expected) in [("PinX", 9.0), ("Width", 4.0)] {
            let value = resolver
                .cell_value(sheet(), &CellRef::new(name))
                .expect("value");
            match value {
                Value::Number(number) => assert_eq!(number.magnitude, expected, "{name}"),
                other => panic!("{name}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_read_evaluates_the_formula_when_nothing_is_cached() {
        // The fill-in case (ADR 0057): a formula with no cached value
        // computes, reading the cached values around it.
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let mut pin = Cell::new(CellName::new("PinX"));
        pin.formula = Some(String::from("Width*0.5"));
        page.sheet.cells.push(pin);
        page.sheet.cells.push(number_cell("Width", 4.0));
        doc.pages.push(page);

        let mut resolver = DocumentResolver::new(&mut doc);
        let value = resolver
            .cell_value(sheet(), &CellRef::new("PinX"))
            .expect("value");
        match value {
            Value::Number(number) => assert_eq!(number.magnitude, 2.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn deep_formula_chains_stop_at_the_bound() {
        let mut doc = Document::new();
        let mut resolver = DocumentResolver::new(&mut doc);
        resolver.depth = MAX_EVAL_DEPTH;
        let value = resolver
            .cell_value(sheet(), &CellRef::new("Width"))
            .expect("value");
        assert_eq!(value, Value::Err(ErrorCode::Ref));
    }
}
