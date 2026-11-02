//! The real pipeline at scale and on malformed input (task P6-11).
//!
//! `src/recalc.rs`'s unit tests pin the pass's own rules with a small
//! test engine. This file drives the **production** engine
//! ([`recalc`] with [`DocumentEngine`]) over documents the unit tests
//! cannot build cheaply:
//!
//! - a section-level cell (a section, no row) resolves against its
//!   section and its master chain and fills its hole (the P6-08 open
//!   issue, fixed in P6-11);
//! - every warning kind the evaluator can raise reaches
//!   [`RecalcOutcome`] (the P6-11 mapping);
//! - recalculation order and budget over bigger documents, with the
//!   determinism and idempotence ADR 0055 promises;
//! - broken references and cyclic `SETF` are warnings, never a panic
//!   (AGENTS.md section 6).

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Master, MasterId, Preserved, Row, Section, SectionName,
    Shape, ShapeId, SheetRef, Style, StyleId,
};
use larrygram_shapesheet::{
    CellAddress, CellRef, CellWriteOp, DepGraph, Error, Evaluation, Formula, FormulaEngine,
    RecalcOptions, RecalcWarningKind, recalc, recalc_with,
};

/// A cell with `formula` and a cached `value`, in `Unit::Unitsless`.
fn cell(name: &str, formula: Option<&str>, value: Option<f64>) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: formula.map(str::to_owned),
        value: value.map(CellValue::Number),
        unit: Unit::Unitsless,
        preserved: Preserved::new(),
    }
}

/// A one-page document holding `shapes`, with their arena ids in order.
fn document_with(shapes: Vec<Shape>) -> (Document, Vec<ShapeId>) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let ids = shapes
        .into_iter()
        .map(|shape| doc.insert_shape(page, shape, None).expect("insert"))
        .collect();
    (doc, ids)
}

/// A shape whose sheet cells carry `formulas` and no cached values.
fn shape_with(visio_id: i32, formulas: &[(&str, &str)]) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = formulas
        .iter()
        .map(|(name, formula)| cell(name, Some(formula), None))
        .collect();
    shape
}

/// The address of a shape's direct cell.
fn at(shape: ShapeId, name: &str) -> CellAddress {
    CellAddress::new(SheetRef::Shape(shape), CellName::new(name))
}

/// The address of a section-level cell (a section, no row).
fn section_at(shape: ShapeId, section: &str, name: &str) -> CellAddress {
    CellAddress {
        sheet: SheetRef::Shape(shape),
        section: Some(SectionName::new(section.to_owned())),
        row: None,
        cell: CellName::new(name),
    }
}

/// A `Geometry` section whose section-level `NoFill` cell carries
/// `formula` and `value`, and that holds one empty row.
fn geometry_section(formula: Option<&str>, value: Option<f64>) -> Section {
    let mut section = Section::new(SectionName::from_static("Geometry"));
    section.cells.push(cell("NoFill", formula, value));
    section.rows.push(Row::new());
    section
}

/// The cached value of `address`, wherever the cell sits.
fn value_of(doc: &Document, address: &CellAddress) -> Option<CellValue> {
    let name = address.cell.as_str();
    match (address.sheet, &address.section) {
        (SheetRef::Shape(id), None) => doc
            .shape(id)?
            .sheet
            .cells
            .iter()
            .find(|entry| entry.name.as_str() == name)
            .and_then(|entry| entry.value.clone()),
        (SheetRef::Shape(id), Some(section)) => {
            let sheet = &doc.shape(id)?.sheet;
            let section = sheet.sections.iter().find(|entry| entry.name == *section)?;
            section
                .cells
                .iter()
                .find(|entry| entry.name.as_str() == name)
                .and_then(|entry| entry.value.clone())
        }
        (SheetRef::Page(id), None) => doc
            .page(id)?
            .sheet
            .cells
            .iter()
            .find(|entry| entry.name.as_str() == name)
            .and_then(|entry| entry.value.clone()),
        (SheetRef::Page(_), Some(_)) => None,
    }
}

/// The cached number of `address`, or `None`.
fn number_of(doc: &Document, address: &CellAddress) -> Option<f64> {
    match value_of(doc, address) {
        Some(CellValue::Number(number)) => Some(number),
        _ => None,
    }
}

/// The formula text of `address`, or `None`.
fn formula_of(doc: &Document, address: &CellAddress) -> Option<String> {
    let name = address.cell.as_str();
    match (address.sheet, &address.section) {
        (SheetRef::Shape(id), None) => doc
            .shape(id)?
            .sheet
            .cells
            .iter()
            .find(|entry| entry.name.as_str() == name)?
            .formula
            .clone(),
        (SheetRef::Shape(id), Some(section)) => {
            let sheet = &doc.shape(id)?.sheet;
            let section = sheet.sections.iter().find(|entry| entry.name == *section)?;
            section
                .cells
                .iter()
                .find(|entry| entry.name.as_str() == name)?
                .formula
                .clone()
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Section-level cells (the P6-08 open issue, fixed in P6-11)
// ---------------------------------------------------------------------------

/// A section-level cell resolves against its section: the fill-in
/// contract (`fill_missing_only`) fills its hole like any other cell.
#[test]
fn a_section_level_cell_fills_its_hole() {
    let mut shape = shape_with(1, &[]);
    shape
        .sheet
        .sections
        .push(geometry_section(Some("2*3"), None));
    let (mut doc, ids) = document_with(vec![shape]);
    let target = section_at(ids[0], "Geometry", "NoFill");
    let options = RecalcOptions {
        fill_missing_only: true,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, std::slice::from_ref(&target), &options).expect("recalc");
    assert_eq!(number_of(&doc, &target), Some(6.0));
    assert_eq!(outcome.warnings, Vec::new());
    assert_eq!(outcome.updates.len(), 1);
}

/// An `Inh` section-level cell takes its formula from the master's
/// section and materialises it when the pass recomputes it (ADR 0056):
/// the instance carries the master's formula text and the value it
/// computes at the instance.
#[test]
fn a_section_level_cell_inherits_its_master_formula() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // The master: `Geometry!NoFill = Width*2` over its own `Width = 5`.
    let mut master_shape = shape_with(1, &[("Width", "5")]);
    master_shape
        .sheet
        .sections
        .push(geometry_section(Some("Width*2"), Some(10.0)));
    let master_root = doc.add_shape(master_shape);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.name = "Box".to_owned();
    master.name_u = "Box".to_owned();
    master.shapes = vec![master_root];
    let master = doc.add_master(master).expect("master");

    // The instance: its own `Width = 7` and an `Inh` `NoFill`.
    let mut instance = shape_with(2, &[("Width", "7")]);
    instance.master = Some(master);
    instance
        .sheet
        .sections
        .push(geometry_section(Some("Inh"), Some(10.0)));
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    let target = section_at(instance, "Geometry", "NoFill");
    let options = RecalcOptions {
        materialise_inherited: true,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, std::slice::from_ref(&target), &options).expect("recalc");
    assert_eq!(
        value_of(&doc, &target),
        Some(CellValue::Number(14.0)),
        "the master's formula computes at the instance's Width"
    );
    let materialised = &doc
        .shape(instance)
        .expect("instance")
        .sheet
        .sections
        .first()
        .expect("geometry")
        .cells;
    assert_eq!(
        materialised.first().expect("NoFill").formula.as_deref(),
        Some("Width*2"),
        "the master's formula is materialised (ADR 0056)"
    );
    assert_eq!(outcome.warnings, Vec::new());
}

/// The master chain continues at a master's own master (a master that
/// is itself an instance): the instance inherits the top master's
/// section-level formula through the middle one.
#[test]
fn a_section_level_cell_walks_a_master_chain() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // The top master: `Geometry!NoFill = 7`.
    let mut top_shape = shape_with(1, &[]);
    top_shape
        .sheet
        .sections
        .push(geometry_section(Some("7"), Some(7.0)));
    let top_root = doc.add_shape(top_shape);
    let mut top = Master::new(MasterId { index: 0 }, 1);
    top.name_u = "Top".to_owned();
    top.shapes = vec![top_root];
    let top = doc.add_master(top).expect("top master");

    // The middle master: an instance of the top master, its `NoFill` is `Inh`.
    let mut middle_shape = shape_with(2, &[]);
    middle_shape.master = Some(top);
    middle_shape
        .sheet
        .sections
        .push(geometry_section(Some("Inh"), Some(7.0)));
    let middle_root = doc.add_shape(middle_shape);
    let mut middle = Master::new(MasterId { index: 1 }, 2);
    middle.name_u = "Middle".to_owned();
    middle.shapes = vec![middle_root];
    let middle = doc.add_master(middle).expect("middle master");

    // The instance: an instance of the middle master, no `NoFill` at all.
    let mut instance = shape_with(3, &[]);
    instance.master = Some(middle);
    instance.sheet.sections.push(geometry_section(None, None));
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    let target = section_at(instance, "Geometry", "NoFill");
    let options = RecalcOptions {
        materialise_inherited: true,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, std::slice::from_ref(&target), &options).expect("recalc");
    assert_eq!(number_of(&doc, &target), Some(7.0));
    assert_eq!(outcome.warnings, Vec::new());
}

/// A rung that has no such section defines nothing but does not stop
/// the chain: the walk continues at that rung's own master.
#[test]
fn a_rung_without_the_section_does_not_stop_the_chain() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // The top master: `Geometry!NoFill = 7`.
    let mut top_shape = shape_with(1, &[]);
    top_shape
        .sheet
        .sections
        .push(geometry_section(Some("7"), Some(7.0)));
    let top_root = doc.add_shape(top_shape);
    let mut top = Master::new(MasterId { index: 0 }, 1);
    top.name_u = "Top".to_owned();
    top.shapes = vec![top_root];
    let top = doc.add_master(top).expect("top master");

    // The middle master: an instance of the top master with **no**
    // `Geometry` section at all.
    let mut middle_shape = shape_with(2, &[]);
    middle_shape.master = Some(top);
    let middle_root = doc.add_shape(middle_shape);
    let mut middle = Master::new(MasterId { index: 1 }, 2);
    middle.name_u = "Middle".to_owned();
    middle.shapes = vec![middle_root];
    let middle = doc.add_master(middle).expect("middle master");

    // The instance: an instance of the middle master, an empty
    // `Geometry` section so the write has somewhere to land.
    let mut instance = shape_with(3, &[]);
    instance.master = Some(middle);
    instance.sheet.sections.push(geometry_section(None, None));
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    let target = section_at(instance, "Geometry", "NoFill");
    let options = RecalcOptions {
        materialise_inherited: true,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, std::slice::from_ref(&target), &options).expect("recalc");
    assert_eq!(number_of(&doc, &target), Some(7.0));
    assert_eq!(outcome.warnings, Vec::new());
}

/// A cyclic master chain is cut (ADR 0013), never looped on: the pass
/// finds no formula, writes nothing and does not hang.
#[test]
fn a_cyclic_master_chain_is_cut() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // A master whose root is an instance of itself.
    let mut root = shape_with(1, &[]);
    root.master = Some(MasterId { index: 0 });
    root.sheet.sections.push(geometry_section(None, None));
    let root_id = doc.add_shape(root);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.name_u = "Loop".to_owned();
    master.shapes = vec![root_id];
    let master = doc.add_master(master).expect("master");

    let mut instance = shape_with(2, &[]);
    instance.master = Some(master);
    instance.sheet.sections.push(geometry_section(None, None));
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    let target = section_at(instance, "Geometry", "NoFill");
    let outcome = recalc(
        &mut doc,
        std::slice::from_ref(&target),
        &RecalcOptions::new(),
    )
    .expect("recalc");
    assert_eq!(outcome.updates, Vec::new());
    assert_eq!(outcome.warnings, Vec::new());
}

// ---------------------------------------------------------------------------
// Style rungs (P6-15's rule through the engine)
// ---------------------------------------------------------------------------

/// P6-15's rule through the engine (ADR 0013 amendment): a formula the
/// master's **style link** supplies is what the pass evaluates for an
/// instance whose master defines nothing, and the instance materialises
/// it (ADR 0056). The link is the shape's own when it sets one; here
/// only the master sets it.
#[test]
fn a_style_formula_reaches_the_instance_through_the_masters_style_link() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // The style the master links to: `LineWeight = 0.5*2`.
    let mut style = Style::new(StyleId { index: 0 });
    style.name = "Normal".to_owned();
    style.name_u = "Normal".to_owned();
    style.visio_id = 1;
    style.sheet.cells = vec![cell("LineWeight", Some("0.5*2"), Some(1.0))];
    doc.stylesheet.styles.push(style);

    // The master: no `LineWeight`, one `LineStyle` link to the style.
    let mut master_shape = shape_with(1, &[]);
    master_shape.line_style = Some(1);
    let master_root = doc.add_shape(master_shape);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.name_u = "Box".to_owned();
    master.shapes = vec![master_root];
    let master = doc.add_master(master).expect("master");

    // The instance: an `Inh` `LineWeight` its master defines nothing for.
    let mut instance = shape_with(2, &[]);
    instance.master = Some(master);
    instance.sheet.cells = vec![cell("LineWeight", Some("Inh"), None)];
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    let target = at(instance, "LineWeight");
    let options = RecalcOptions {
        materialise_inherited: true,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, std::slice::from_ref(&target), &options).expect("recalc");
    assert_eq!(
        number_of(&doc, &target),
        Some(1.0),
        "the style's formula computed at the instance"
    );
    assert_eq!(
        formula_of(&doc, &target).as_deref(),
        Some("0.5*2"),
        "the instance materialises the style's formula (ADR 0056)"
    );
    assert_eq!(outcome.warnings, Vec::new());
}

// ---------------------------------------------------------------------------
// The warning kinds reach the outcome (the P6-11 mapping)
// ---------------------------------------------------------------------------

/// One shape carrying `formulas` in its direct cells, recalculated
/// over all of them.
fn pass_over(
    formulas: &[(&str, &str)],
) -> (Document, ShapeId, larrygram_shapesheet::RecalcOutcome) {
    let (mut doc, ids) = document_with(vec![shape_with(1, formulas)]);
    let dirty: Vec<CellAddress> = formulas.iter().map(|(name, _)| at(ids[0], name)).collect();
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    (doc, ids[0], outcome)
}

/// A call outside the table's arity bounds is `#VALUE!` and one
/// [`RecalcWarningKind::WrongArgumentCount`] warning (ADR 0053).
#[test]
fn a_wrong_argument_count_warning_reaches_the_outcome() {
    let (doc, shape, outcome) = pass_over(&[("Alpha", "ABS(1,2)")]);
    assert_eq!(
        value_of(&doc, &at(shape, "Alpha")),
        Some(CellValue::Error(String::from("#VALUE!")))
    );
    assert!(outcome.warnings.iter().any(|warning| matches!(
        &warning.kind,
        RecalcWarningKind::WrongArgumentCount { function, min: 1, max: Some(1), got: 2 }
            if function == "ABS"
    )));
}

/// A `SETF` target that is not a writable cell reference is `#REF!`
/// and one [`RecalcWarningKind::SetfRefused`] warning (ADR 0056).
#[test]
fn a_refused_setf_warning_reaches_the_outcome() {
    let (doc, shape, outcome) = pass_over(&[("Alpha", "SETF(\"Width\",3)")]);
    assert_eq!(
        value_of(&doc, &at(shape, "Alpha")),
        Some(CellValue::Error(String::from("#REF!")))
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::SetfRefused(_)))
    );
}

/// A lossy or defaulted conversion is one
/// [`RecalcWarningKind::Coercion`] warning and the typed default
/// (ADR 0052: `5 * "sheep"` is 0).
#[test]
fn a_coercion_warning_reaches_the_outcome() {
    let (doc, shape, outcome) = pass_over(&[("Alpha", "5*\"sheep\"")]);
    assert_eq!(number_of(&doc, &at(shape, "Alpha")), Some(0.0));
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::Coercion(_)))
    );
}

/// A computed non-finite number is `#NUM!` and one
/// [`RecalcWarningKind::NonFinite`] warning (P3-11).
#[test]
fn a_non_finite_result_reaches_the_outcome() {
    let (doc, shape, outcome) = pass_over(&[("Alpha", "10^1000")]);
    assert_eq!(
        value_of(&doc, &at(shape, "Alpha")),
        Some(CellValue::Error(String::from("#NUM!")))
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::NonFinite))
    );
}

/// A huge numeric literal is a syntax error (the parser rejects a
/// non-finite literal), and a formula that does not parse keeps the
/// cell's cached value with one [`RecalcWarningKind::Syntax`] warning
/// (the ADR 0053 rule): it never fails the pass.
#[test]
fn a_formula_that_does_not_parse_keeps_the_cached_value() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![cell("Alpha", Some("1e999999"), Some(3.0))];
    let (mut doc, ids) = document_with(vec![shape]);
    let dirty = [at(ids[0], "Alpha")];
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert_eq!(
        number_of(&doc, &dirty[0]),
        Some(3.0),
        "the cached value stays"
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::Syntax(_))),
        "{:?}",
        outcome.warnings
    );
    assert_eq!(outcome.updates, Vec::new());
}

// ---------------------------------------------------------------------------
// Order and budget over bigger documents (ADR 0055)
// ---------------------------------------------------------------------------

/// A chain of `length` cells: `A0 = 0` and `A(n) = A(n-1)+1`.
fn chain_shape(visio_id: i32, length: usize) -> Shape {
    let formulas: Vec<(String, String)> = (0..length)
        .map(|index| {
            (
                format!("A{index}"),
                if index == 0 {
                    String::from("0")
                } else {
                    format!("A{}+1", index - 1)
                },
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = formulas
        .iter()
        .map(|(name, formula)| (name.as_str(), formula.as_str()))
        .collect();
    shape_with(visio_id, &borrowed)
}

/// A 200-cell dependency chain recomputes in dependency order, in the
/// same order every run (determinism), and a second pass over the same
/// cells changes nothing (idempotence, ADR 0055).
#[test]
fn a_long_chain_recomputes_in_order_and_is_idempotent() {
    let (mut doc, ids) = document_with(vec![chain_shape(1, 200)]);
    let chain: Vec<CellAddress> = (0..200)
        .map(|index| at(ids[0], &format!("A{index}")))
        .collect();
    let outcome = recalc(&mut doc, &chain, &RecalcOptions::new()).expect("recalc");
    assert_eq!(outcome.updates.len(), 200);
    for (index, update) in outcome.updates.iter().enumerate() {
        assert_eq!(update.target, chain[index], "dependencies first");
        assert_eq!(update.after, Some(CellValue::Number(index as f64)));
    }
    assert!(!outcome.truncated);
    assert_eq!(outcome.warnings, Vec::new());

    // Determinism: the same input, the same outcome.
    let (mut again, ids) = document_with(vec![chain_shape(1, 200)]);
    let chain: Vec<CellAddress> = (0..200)
        .map(|index| at(ids[0], &format!("A{index}")))
        .collect();
    let second = recalc(&mut again, &chain, &RecalcOptions::new()).expect("recalc");
    assert_eq!(second.updates, outcome.updates);

    // Idempotence: the computed document recomputes to the same values.
    let third = recalc(&mut doc, &chain, &RecalcOptions::new()).expect("recalc");
    assert_eq!(third.updates, Vec::new());
}

/// A tiny budget truncates a long chain: the first cells compute, the
/// rest stay stale and one [`RecalcWarningKind::BudgetExceeded`]
/// warning says so (ADR 0055).
#[test]
fn a_tiny_budget_truncates_a_long_chain() {
    let (mut doc, ids) = document_with(vec![chain_shape(1, 200)]);
    let chain: Vec<CellAddress> = (0..200)
        .map(|index| at(ids[0], &format!("A{index}")))
        .collect();
    let options = RecalcOptions {
        max_evaluations: 50,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, &chain, &options).expect("recalc");
    assert!(outcome.truncated);
    assert_eq!(outcome.updates.len(), 50);
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::BudgetExceeded))
    );
    assert_eq!(number_of(&doc, &chain[49]), Some(49.0));
    assert_eq!(number_of(&doc, &chain[50]), None, "the rest stays stale");
}

/// A page of 200 shapes (1 000 formula cells) recomputes fully within
/// the default budget, and the same pass under a tiny budget truncates
/// and reports it: the budget is measured in evaluations, not time.
#[test]
fn a_thousand_cell_page_serves_the_budget_rule() {
    let shapes: Vec<Shape> = (0..200)
        .map(|index| {
            let mut shape = shape_with(
                index + 1,
                &[
                    ("Width", "2"),
                    ("Height", "3"),
                    ("PinX", "Width*0.5"),
                    ("PinY", "Height*0.5"),
                    ("LocPinX", "Width*0.25"),
                ],
            );
            if index % 2 == 0 {
                // Half the shapes also carry a `Geometry` row that
                // reads their transform.
                let mut section = Section::new(SectionName::from_static("Geometry"));
                let mut row = Row::new();
                row.ix = Some(0);
                row.cells = vec![cell("X", Some("PinX"), None), cell("Y", Some("PinY"), None)];
                section.rows.push(row);
                shape.sheet.sections.push(section);
            }
            shape
        })
        .collect();
    let (mut doc, ids) = document_with(shapes);
    let mut dirty: Vec<CellAddress> = Vec::new();
    for id in &ids {
        for name in ["Width", "Height", "PinX", "PinY", "LocPinX"] {
            dirty.push(at(*id, name));
        }
        if id.index % 2 == 0 {
            for name in ["X", "Y"] {
                dirty.push(CellAddress::in_row(
                    SheetRef::Shape(*id),
                    SectionName::from_static("Geometry"),
                    larrygram_model::RowTarget {
                        position: Some(0),
                        ix: Some(0),
                    },
                    CellName::new(name),
                ));
            }
        }
    }
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert!(!outcome.truncated);
    assert_eq!(outcome.updates.len(), dirty.len());
    assert_eq!(outcome.warnings, Vec::new());

    let options = RecalcOptions {
        max_evaluations: 100,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, &dirty, &options).expect("recalc");
    assert!(
        outcome.truncated,
        "1 200 cells do not fit in 100 evaluations"
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::BudgetExceeded))
    );
}

// ---------------------------------------------------------------------------
// Malformed documents: broken references and cyclic SETF
// ---------------------------------------------------------------------------

/// A document whose formulas name cells that do not exist: every one is
/// a warning and an error value (or the documented default), never a
/// panic and never a failed document (ADR 0054).
#[test]
fn broken_references_are_warnings_not_failures() {
    let (doc, shape, outcome) = pass_over(&[
        ("Gone", "Sheet.99!PinX"),
        ("Named", "Nope!Width"),
        ("Row", "Geometry1.X9"),
        ("Prop", "Prop.Nope"),
        ("Char", "Char.Size[3]"),
        ("Master", "Masters[Bearing]!Hub!Geometry1.X1"),
        ("Page", "Pages[Page-9]!Sheet.4!BeginX"),
        ("Style", "Styles!Ledger!LineColor"),
        ("Doc", "TheDoc!DocScale"),
    ]);
    for name in [
        "Gone", "Named", "Row", "Prop", "Char", "Master", "Page", "Style", "Doc",
    ] {
        assert!(
            value_of(&doc, &at(shape, name)).is_some(),
            "{name} has a value"
        );
    }
    assert!(!outcome.warnings.is_empty());
    assert!(!outcome.truncated);
}

/// A `SETF` cycle is cut by the cycle rule (ADR 0056): the cells keep
/// their cached values, one [`RecalcWarningKind::Cycle`] warning names
/// them, and the pass terminates instead of looping writes.
#[test]
fn a_cyclic_setf_is_cut_without_a_panic() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![
        cell("Ping", Some("SETF(Pong,Pong+1)"), Some(1.0)),
        cell("Pong", Some("SETF(Ping,Ping+1)"), Some(2.0)),
    ];
    let (mut doc, ids) = document_with(vec![shape]);
    let dirty = [at(ids[0], "Ping"), at(ids[0], "Pong")];
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert!(!outcome.truncated, "the pass never loops");
    assert_eq!(number_of(&doc, &dirty[0]), Some(1.0), "cached values stay");
    assert_eq!(number_of(&doc, &dirty[1]), Some(2.0));
    let cycles: Vec<&RecalcWarningKind> = outcome
        .warnings
        .iter()
        .map(|warning| &warning.kind)
        .filter(|kind| matches!(kind, RecalcWarningKind::Cycle(_)))
        .collect();
    assert_eq!(cycles.len(), 1, "{:?}", outcome.warnings);
}

/// A `SETF` that writes its own cell is a cycle of one (ADR 0055): the
/// cached value stays and the pass terminates.
#[test]
fn a_self_referencing_setf_is_a_cycle_of_one() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![cell("Self", Some("SETF(Self,Self+1)"), Some(1.0))];
    let (mut doc, ids) = document_with(vec![shape]);
    let dirty = [at(ids[0], "Self")];
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert!(!outcome.truncated);
    assert_eq!(
        number_of(&doc, &dirty[0]),
        Some(1.0),
        "the cached value stays"
    );
    assert_eq!(outcome.updates, Vec::new());
}

/// A formula cycle keeps the cached values and reports one
/// [`RecalcWarningKind::Cycle`] warning (ADR 0055).
#[test]
fn a_formula_cycle_keeps_the_cached_values() {
    let (mut doc, ids) = document_with(vec![shape_with(1, &[])]);
    {
        let shape = doc.shape_mut(ids[0]).expect("shape");
        shape.sheet.cells = vec![
            cell("Alpha", Some("Beta+1"), Some(1.0)),
            cell("Beta", Some("Alpha+1"), Some(2.0)),
        ];
    }
    let dirty = [at(ids[0], "Alpha"), at(ids[0], "Beta")];
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert_eq!(number_of(&doc, &dirty[0]), Some(1.0), "cached values stay");
    assert_eq!(number_of(&doc, &dirty[1]), Some(2.0));
    let cycles: Vec<&RecalcWarningKind> = outcome
        .warnings
        .iter()
        .map(|warning| &warning.kind)
        .filter(|kind| matches!(kind, RecalcWarningKind::Cycle(_)))
        .collect();
    assert_eq!(cycles.len(), 1, "{:?}", outcome.warnings);
}

// ---------------------------------------------------------------------------
// P6-17: the fix-task regressions
// ---------------------------------------------------------------------------

/// A `SETF` inside a **referenced** cell's formula is recorded and the
/// pass applies it (P6-12 review F1): the write lands with a
/// [`CellUpdate`] and the dependents are dirtied in the same pass.
#[test]
fn a_nested_setf_lands_through_the_pass() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![
        cell("Width", None, Some(0.0)),
        cell("Mid", Some("SETF(Width,5)+1"), None),
        cell("Top", Some("Mid*2"), Some(0.0)),
    ];
    let (mut doc, ids) = document_with(vec![shape]);
    let outcome = recalc(&mut doc, &[at(ids[0], "Top")], &RecalcOptions::new()).expect("recalc");
    assert_eq!(
        number_of(&doc, &at(ids[0], "Top")),
        Some(12.0),
        "the nested evaluation returned Mid = 6"
    );
    assert_eq!(
        number_of(&doc, &at(ids[0], "Width")),
        Some(5.0),
        "the nested write landed through the pass"
    );
    assert!(
        outcome.updates.iter().any(|update| {
            update.target == at(ids[0], "Width") && update.after == Some(CellValue::Number(5.0))
        }),
        "{:?}",
        outcome.updates
    );
}

/// The "F1 reproduces" path: the referenced cell stands back (an
/// unknown function) and its `SETF` write is dropped (ADR 0056, P6-12
/// review F3), so nothing writes outside the pass's snapshot; the
/// warning names the referenced cell (F16).
#[test]
fn a_standing_back_evaluation_drops_its_writes_and_names_the_cell() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![
        cell("Width", None, Some(0.0)),
        cell("Mid", Some("SETF(Width,5)+THEMEVAL(x)"), None),
        cell("Top", Some("Mid+1"), Some(0.0)),
    ];
    let (mut doc, ids) = document_with(vec![shape]);
    let outcome = recalc(&mut doc, &[at(ids[0], "Top")], &RecalcOptions::new()).expect("recalc");
    assert_eq!(
        number_of(&doc, &at(ids[0], "Width")),
        Some(0.0),
        "a cell that keeps its cached value has no side effects"
    );
    assert!(
        !outcome
            .updates
            .iter()
            .any(|update| update.target == at(ids[0], "Width")),
        "{:?}",
        outcome.updates
    );
    assert_eq!(
        number_of(&doc, &at(ids[0], "Top")),
        Some(0.0),
        "the outer cell keeps its cached value too"
    );
    let unknown = outcome
        .warnings
        .iter()
        .find(|warning| matches!(warning.kind, RecalcWarningKind::UnknownFunction(_)))
        .expect("one unknown-function warning");
    assert_eq!(
        unknown.target,
        Some(at(ids[0], "Mid")),
        "the warning names the cell whose formula raised it (F16)"
    );
}

/// `SETF` writes into a section-level cell, a row cell and the page
/// sheet (the write arms the coverage audit found dark): each write
/// lands in its own slot with its `CellUpdate`.
#[test]
fn a_setf_writes_into_a_section_a_row_and_the_page() {
    let mut shape = shape_with(1, &[]);
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    geometry.cells.push(cell("NoFill", None, Some(0.0)));
    for ix in 0..2 {
        let mut row = Row::new();
        row.ix = Some(ix);
        row.cells = vec![cell("X", None, Some(0.0))];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape.sheet.cells = vec![
        cell("S", Some("SETF(Geometry1.NoFill, 3)"), None),
        cell("R", Some("SETF(Geometry1.X1, 4)"), None),
        cell("P", Some("SETF(ThePage!Tgt, 5)"), None),
    ];
    let (mut doc, ids) = document_with(vec![shape]);
    let page = doc.pages.first_mut().expect("page");
    page.sheet.cells = vec![cell("Tgt", None, Some(0.0))];

    let dirty = [at(ids[0], "S"), at(ids[0], "R"), at(ids[0], "P")];
    let outcome = recalc(&mut doc, &dirty, &RecalcOptions::new()).expect("recalc");
    assert_eq!(outcome.warnings, Vec::new(), "{:?}", outcome.warnings);

    // The section-level cell.
    assert_eq!(
        number_of(&doc, &section_at(ids[0], "Geometry", "NoFill")),
        Some(3.0)
    );
    // The row cell (`X1` is the row with `IX` 1).
    let row_cell = &doc
        .shape(ids[0])
        .expect("shape")
        .sheet
        .sections
        .first()
        .expect("geometry")
        .rows
        .get(1)
        .expect("row 1")
        .cells[0];
    assert_eq!(row_cell.value, Some(CellValue::Number(4.0)));
    assert_eq!(row_cell.formula.as_deref(), Some("4"), "a literal write");
    // The page sheet's cell.
    let page_cell = doc
        .page(larrygram_model::PageId { index: 0 })
        .expect("page")
        .sheet
        .cells
        .first()
        .expect("Tgt");
    assert_eq!(page_cell.value, Some(CellValue::Number(5.0)));

    for target in [
        section_at(ids[0], "Geometry", "NoFill"),
        CellAddress::in_row(
            SheetRef::Shape(ids[0]),
            SectionName::from_static("Geometry"),
            larrygram_model::RowTarget {
                position: Some(1),
                ix: Some(1),
            },
            CellName::from_static("X"),
        ),
        CellAddress::new(
            SheetRef::Page(larrygram_model::PageId { index: 0 }),
            CellName::from_static("Tgt"),
        ),
    ] {
        assert!(
            outcome.updates.iter().any(|update| update.target == target),
            "{target:?} missing from {:?}",
            outcome.updates
        );
    }
}

/// An engine failure aborts the pass with an error and no panic (the
/// hard-error chain of `recalc_with` and `DocumentEngine::evaluate`;
/// the product engine cannot fail on document data, so the test drives
/// the seam with a broken engine).
#[test]
fn a_failing_engine_aborts_the_pass_with_an_error() {
    struct Broken;
    impl FormulaEngine for Broken {
        fn references(&mut self, _formula: &Formula) -> Result<Vec<CellRef>, Error> {
            Ok(Vec::new())
        }
        fn evaluate(
            &mut self,
            _document: &mut Document,
            _formula: &Formula,
            _sheet: SheetRef,
        ) -> Result<Evaluation, Error> {
            Err(Error::Conversion("broken test engine"))
        }
    }
    let (mut doc, ids) = document_with(vec![shape_with(1, &[("Alpha", "1"), ("Beta", "2")])]);
    let dirty = [at(ids[0], "Alpha"), at(ids[0], "Beta")];
    let mut graph = DepGraph::build_with(&doc, &mut Broken).expect("build");
    let outcome = recalc_with(
        &mut doc,
        &dirty,
        &RecalcOptions::new(),
        &mut graph,
        &mut Broken,
    );
    assert!(outcome.is_err(), "the pass propagates the hard error");
    assert_eq!(
        formula_of(&doc, &at(ids[0], "Alpha")),
        Some(String::from("1")),
        "the document is untouched"
    );
}

/// A write whose [`CellAddress`] names a row without its section is an
/// error the pass reports as a warning, never a panic (AGENTS.md
/// section 6).
#[test]
fn a_row_address_without_its_section_is_a_warning_not_a_panic() {
    struct RowWithoutSection;
    impl FormulaEngine for RowWithoutSection {
        fn references(&mut self, _formula: &Formula) -> Result<Vec<CellRef>, Error> {
            Ok(Vec::new())
        }
        fn evaluate(
            &mut self,
            _document: &mut Document,
            _formula: &Formula,
            sheet: SheetRef,
        ) -> Result<Evaluation, Error> {
            let shape = match sheet {
                SheetRef::Shape(id) => id,
                SheetRef::Page(_) => larrygram_model::ShapeId { index: 0 },
            };
            Ok(Evaluation {
                kept_cached: false,
                value: CellValue::Number(0.0),
                warnings: Vec::new(),
                writes: vec![CellWriteOp {
                    target: CellAddress {
                        sheet: SheetRef::Shape(shape),
                        section: None,
                        row: Some(larrygram_model::RowTarget::at(0)),
                        cell: CellName::from_static("X"),
                    },
                    formula: String::from("1"),
                    value: CellValue::Number(1.0),
                }],
                evaluations: 1,
            })
        }
    }
    let (mut doc, ids) = document_with(vec![shape_with(1, &[("Alpha", "1")])]);
    let mut graph = DepGraph::build_with(&doc, &mut RowWithoutSection).expect("build");
    let outcome = recalc_with(
        &mut doc,
        &[at(ids[0], "Alpha")],
        &RecalcOptions::new(),
        &mut graph,
        &mut RowWithoutSection,
    )
    .expect("the error is a warning, not a failure");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::MissingReference(_))),
        "{:?}",
        outcome.warnings
    );
    assert_eq!(
        number_of(&doc, &at(ids[0], "Alpha")),
        Some(0.0),
        "the cell itself was written; only the malformed write was refused"
    );
}

/// The budget counts the nested evaluations a read triggers (P6-12
/// review F7): each dirty cell costs one evaluation plus one nested
/// one, so a budget of 2 runs out at the second cell.
#[test]
fn the_budget_counts_nested_evaluations() {
    let mut shape = shape_with(1, &[]);
    shape.sheet.cells = vec![
        cell("Alpha", Some("Beta+1"), Some(0.0)),
        cell("Beta", Some("1"), None),
        cell("Gamma", Some("Delta+1"), Some(0.0)),
        cell("Delta", Some("2"), None),
    ];
    let (mut doc, ids) = document_with(vec![shape]);
    let dirty = [at(ids[0], "Alpha"), at(ids[0], "Gamma")];
    let options = RecalcOptions {
        max_evaluations: 2,
        ..RecalcOptions::new()
    };
    let outcome = recalc(&mut doc, &dirty, &options).expect("recalc");
    assert_eq!(
        number_of(&doc, &at(ids[0], "Alpha")),
        Some(2.0),
        "the nested read evaluated Beta"
    );
    assert!(outcome.truncated, "{:?}", outcome.warnings);
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, RecalcWarningKind::BudgetExceeded))
    );
}

/// The "F2 reproduces" path: an instance with no own `Character` rows
/// reads `Char.Size` from its master, and an edit of the master's cell
/// re-evaluates the dependent formula (P6-12 review F2).
#[test]
fn an_edit_of_a_master_cell_reevaluates_the_dependent_formula() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");

    // The master: a `Character` section whose first row carries `Size`.
    let mut master_shape = Shape::new(ShapeId { index: 0 }, 10);
    {
        let mut section = Section::new(SectionName::from_static("Character"));
        let mut row = Row::new();
        row.ix = Some(0);
        row.cells = vec![cell("Size", None, Some(5.0))];
        section.rows.push(row);
        master_shape.sheet.sections.push(section);
    }
    let master_root = doc.add_shape(master_shape);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.name = String::from("Badge");
    master.name_u = String::from("Badge");
    master.shapes = vec![master_root];
    let master = doc.add_master(master).expect("master");

    // The instance: no own `Character` rows and one stale dependent.
    let mut instance = Shape::new(ShapeId { index: 0 }, 11);
    instance.master = Some(master);
    instance.sheet.cells = vec![cell("PinX", Some("Char.Size*2"), Some(0.0))];
    let instance = doc.insert_shape(page, instance, None).expect("insert");

    // The edit changes the master's `Size`; the pass must reach `PinX`.
    let edited = CellAddress::in_row(
        SheetRef::Shape(master_root),
        SectionName::from_static("Character"),
        larrygram_model::RowTarget {
            position: Some(0),
            ix: Some(0),
        },
        CellName::from_static("Size"),
    );
    let outcome = recalc(
        &mut doc,
        std::slice::from_ref(&edited),
        &RecalcOptions::new(),
    )
    .expect("recalc");
    assert_eq!(
        number_of(&doc, &at(instance, "PinX")),
        Some(10.0),
        "the dependent recomputes from the master's cell"
    );
    assert!(outcome.updates.iter().any(|update| {
        update.target == at(instance, "PinX") && update.after == Some(CellValue::Number(10.0))
    }));
}
