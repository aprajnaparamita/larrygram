//! Function-table tests (ADR 0053, P6-04).
//!
//! The table's names and argument bounds are checked against the
//! ShapeSheet reference's function pages (`docs/sources.md` rows 4 and
//! 19; local copies `reference/ms-vsdx/office-developer-client-docs/
//! docs/visio/*-function*.md`). The descriptions in the table are our
//! own; only the spellings and arities come from the reference.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_shapesheet::{FUNCTIONS, FunctionDef, FunctionKind, is_supported, lookup_function};

/// The evaluated set (ADR 0053) as the reference spells it, with the
/// argument bounds of each function's syntax line and parameter table.
///
/// Two rows are not spelled by the reference and say so:
/// - `MOD` is the alias ADR 0053 gives `MODULUS` (`modulus-function.md`
///   spells the function `MODULUS`).
/// - `EXP` has no reference page; ADR 0053 put the exponential in the
///   math set and this evaluator gives it the standard 1-argument shape.
///
/// Where the page's prose and its table disagree, the prose wins:
/// `CEILING`/`FLOOR` document the one-argument form ("if `multiple` is
/// not specified"), and `SUM` is seen with one argument in the corpus
/// (2 of 3 calls), so its bound starts at 1.
const REFERENCE: &[(&str, u8, Option<u8>)] = &[
    ("ABS", 1, Some(1)),
    ("ACOS", 1, Some(1)),
    ("AND", 1, None),
    ("ASIN", 1, Some(1)),
    ("ATAN", 1, Some(1)),
    ("ATAN2", 2, Some(2)),
    ("CEILING", 1, Some(2)),
    ("COS", 1, Some(1)),
    ("COSH", 1, Some(1)),
    ("DEG", 1, Some(1)),
    ("EXP", 1, Some(1)),
    ("FLOOR", 1, Some(2)),
    ("GUARD", 1, Some(1)),
    ("IF", 3, Some(3)),
    ("IFERROR", 2, Some(2)),
    ("INDEX", 2, Some(4)),
    ("INT", 1, Some(1)),
    ("LN", 1, Some(1)),
    ("LOG10", 1, Some(1)),
    ("LOOKUP", 2, Some(3)),
    ("MAX", 1, None),
    ("MIN", 1, None),
    ("MOD", 2, Some(2)),
    ("MODULUS", 2, Some(2)),
    ("NA", 0, Some(0)),
    ("NOT", 1, Some(1)),
    ("OR", 1, None),
    ("PI", 0, Some(0)),
    ("POW", 2, Some(2)),
    ("RAD", 1, Some(1)),
    ("RGB", 3, Some(3)),
    ("ROUND", 2, Some(2)),
    ("SETF", 2, Some(2)),
    ("SIGN", 1, Some(2)),
    ("SIN", 1, Some(1)),
    ("SINH", 1, Some(1)),
    ("SQRT", 1, Some(1)),
    ("STRSAME", 2, Some(3)),
    ("SUM", 1, None),
    ("TAN", 1, Some(1)),
    ("TANH", 1, Some(1)),
    ("TRUNC", 2, Some(2)),
];

/// The Visio-owned glue functions (task P7-31, ADR 0084 rule 2): the
/// calls a Visio-written file's endpoint and trigger formulas carry
/// (`docs/ui/connectors.md` section 8). Their arities record the
/// observed spellings; the evaluator keeps the cached value for them
/// and computes nothing, so they are in [`FUNCTIONS`] but not in
/// [`REFERENCE`] and not [`is_supported`].
const VISIO_OWNED: &[(&str, u8, Option<u8>)] = &[
    ("PAR", 1, Some(1)),
    ("PNT", 2, Some(2)),
    ("_WALKGLUE", 3, Some(3)),
    ("_XFTRIGGER", 1, Some(1)),
];

#[test]
fn the_table_holds_every_reference_name_with_its_arity() {
    assert_eq!(
        FUNCTIONS.len(),
        REFERENCE.len() + VISIO_OWNED.len(),
        "the evaluated set plus the Visio-owned glue functions"
    );
    for (name, min, max) in REFERENCE {
        let found = lookup_function(name)
            .expect("lookup has no failure path")
            .unwrap_or_else(|| panic!("{name} missing from FUNCTIONS"));
        assert_eq!(found.name, *name, "{name} spelled differently");
        assert_eq!(found.min_args, *min, "{name} min_args");
        assert_eq!(found.max_args, *max, "{name} max_args");
    }
    for (name, min, max) in VISIO_OWNED {
        let found = lookup_function(name)
            .expect("lookup has no failure path")
            .unwrap_or_else(|| panic!("{name} missing from FUNCTIONS"));
        assert_eq!(found.kind, FunctionKind::VisioOwned, "{name} kind");
        assert_eq!(found.min_args, *min, "{name} min_args");
        assert_eq!(found.max_args, *max, "{name} max_args");
    }
}

/// The Visio-owned glue functions are tabled (they must not warn as
/// unknown) but never computed: `is_supported` is false for them (task
/// P7-31 item 4).
#[test]
fn the_visio_owned_functions_are_tabled_but_not_supported() {
    for (name, _, _) in VISIO_OWNED {
        assert!(lookup_function(name).expect("lookup").is_some(), "{name}");
        assert!(!is_supported(name), "{name} keeps the cached value");
    }
}

#[test]
fn is_supported_agrees_with_the_table() {
    for (name, _, _) in REFERENCE {
        assert!(is_supported(name), "{name} should be supported");
    }
    // Every name outside the evaluated set is unknown and keeps the
    // cell's cached value (ADR 0053).
    for name in [
        "THEMEVAL",
        "THEMEGUARD",
        "ISTHEMED",
        "SETATREF",
        "SETATREFEXPR",
        "GETREF",
        "RUNADDON",
        "RUNADDONWARGS",
        "DOCMD",
        "OPENTEXTWIN",
        "DEFAULTEVENT",
        "FIELDPICTURE",
        "FORMATEX",
        "SHAPETEXT",
        "TEXTWIDTH",
        "TEXTHEIGHT",
        "RAND",
        "_MOD",
        "_ELLIPSE_THETA",
        "STRSAMEEX",
        "",
    ] {
        assert!(!is_supported(name), "{name} should not be supported");
    }
}

#[test]
fn lookup_is_case_insensitive() {
    let upper = lookup_function("IF").expect("no failure path");
    let lower = lookup_function("If").expect("no failure path");
    assert_eq!(upper, lower);
    assert_eq!(upper.map(|d| d.name), Some("IF"));
    assert_eq!(lookup_function("iffy"), Ok(None));
}

#[test]
fn every_row_has_a_kind_of_the_documented_set() {
    for def in FUNCTIONS {
        let kinded = matches!(
            def.kind,
            FunctionKind::Arithmetic
                | FunctionKind::Trigonometry
                | FunctionKind::Logic
                | FunctionKind::Text
                | FunctionKind::Color
                | FunctionKind::Reference
                | FunctionKind::Guard
                | FunctionKind::VisioOwned
        );
        assert!(kinded, "{} has an unexpected kind", def.name);
    }
    // The kinds ADR 0053 assigns to the control-flow and guard rows.
    let if_row: FunctionDef = *lookup_function("IF")
        .expect("no failure path")
        .expect("IF is in the table");
    assert_eq!(if_row.kind, FunctionKind::Logic);
    assert_eq!(
        lookup_function("GUARD")
            .expect("no failure path")
            .map(|d| d.kind),
        Some(FunctionKind::Guard)
    );
    assert_eq!(
        lookup_function("RGB")
            .expect("no failure path")
            .map(|d| d.kind),
        Some(FunctionKind::Color)
    );
}
