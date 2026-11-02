//! The ShapeSheet function table (ADR 0053).
//!
//! One entry per function the evaluator knows: the name as Visio spells
//! it, how many arguments it takes and what kind of function it is. The
//! table is **hand-written** in this module (ADR 0053); its rows are the
//! decided set of the KICKOFF core plus the helpers the corpus needs.
//!
//! A name the table does not hold keeps the cell's cached value and
//! records a structured warning (ADR 0053): an unknown function never
//! fails the document.
//!
//! Names and arities come from the ShapeSheet reference's function pages
//! (`docs/sources.md` rows 4 and 19); the descriptions are our own. Two
//! rows are not spelled by those pages and say so in their comments:
//! `MOD` (the alias ADR 0053 gives `MODULUS`) and `EXP` (ADR 0053's
//! exponential; the reference has no page for it).

use crate::Error;

/// One row of the function table (ADR 0053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionDef {
    /// The function name as Visio spells it (`IF`, `GUARD`, `SETF`).
    pub name: &'static str,
    /// The fewest arguments the function takes.
    pub min_args: u8,
    /// The most arguments it takes, or `None` for a variadic function.
    pub max_args: Option<u8>,
    /// What kind of function it is.
    pub kind: FunctionKind,
}

/// What kind of function a [`FunctionDef`] is (ADR 0053).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FunctionKind {
    /// Arithmetic (`MIN`, `ROUND`, `SUM`).
    Arithmetic,
    /// Trigonometry and angle units (`SIN`, `ATAN2`, `DEG`).
    Trigonometry,
    /// Logic and control flow (`IF`, `AND`, `NOT`).
    Logic,
    /// Text (`STRSAME`, `SHAPETEXT`).
    Text,
    /// Colour (`RGB`, `LUM`, `TINT`).
    Color,
    /// Reference and lookup (`INDEX`, `LOOKUP`, `SHEETREF`).
    Reference,
    /// Value guards and assignment (`GUARD`, `SETF`, `SETATREF`).
    Guard,
    /// An action with no value (`RUNADDON`, `OPENTEXTWIN`).
    Action,
    /// Reserved for Visio's own use (`_MOD` and other `_`-prefixed
    /// names the table does not hold): the call stands back with an
    /// [`crate::EvalWarningKind::UnknownFunction`] warning.
    Internal,
    /// A Visio-owned function the evaluator never computes (`PAR`,
    /// `PNT`, `_WALKGLUE`, `_XFTRIGGER`): the call keeps the cell's
    /// cached value and reports nothing (task P7-31, ADR 0084 rule 2).
    /// The glue formulas of a Visio-written file carry these calls; our
    /// router, not the evaluator, moves the endpoints.
    VisioOwned,
}

/// The function table (ADR 0053): every name of the evaluated set, with
/// the argument bounds the ShapeSheet reference gives it.
///
/// The reference spells the modulus function `MODULUS`; `MOD` is the
/// alias ADR 0053 decided on and shares its row's shape. `EXP` has no
/// reference page (ADR 0053 added it to the math set).
pub const FUNCTIONS: &[FunctionDef] = &[
    // Logic and control flow.
    FunctionDef {
        name: "IF",
        min_args: 3,
        max_args: Some(3),
        kind: FunctionKind::Logic,
    },
    FunctionDef {
        name: "AND",
        min_args: 1,
        max_args: None,
        kind: FunctionKind::Logic,
    },
    FunctionDef {
        name: "OR",
        min_args: 1,
        max_args: None,
        kind: FunctionKind::Logic,
    },
    FunctionDef {
        name: "NOT",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Logic,
    },
    FunctionDef {
        name: "IFERROR",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Logic,
    },
    // Arithmetic.
    FunctionDef {
        name: "MIN",
        min_args: 1,
        max_args: None,
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "MAX",
        min_args: 1,
        max_args: None,
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "ABS",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "INT",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "ROUND",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "CEILING",
        min_args: 1,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "FLOOR",
        min_args: 1,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "MODULUS",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "MOD",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "SUM",
        min_args: 1,
        max_args: None,
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "POW",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "SQRT",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "EXP",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "LN",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "LOG10",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "SIGN",
        min_args: 1,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "TRUNC",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Arithmetic,
    },
    FunctionDef {
        name: "PI",
        min_args: 0,
        max_args: Some(0),
        kind: FunctionKind::Arithmetic,
    },
    // Trigonometry and angle units. `DEG` and `RAD` convert between the
    // angle measures (KICKOFF's unit conversion).
    FunctionDef {
        name: "SIN",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "COS",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "TAN",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "ASIN",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "ACOS",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "ATAN",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "ATAN2",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "SINH",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "COSH",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "TANH",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "DEG",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    FunctionDef {
        name: "RAD",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Trigonometry,
    },
    // Text.
    FunctionDef {
        name: "STRSAME",
        min_args: 2,
        max_args: Some(3),
        kind: FunctionKind::Text,
    },
    // Colour.
    FunctionDef {
        name: "RGB",
        min_args: 3,
        max_args: Some(3),
        kind: FunctionKind::Color,
    },
    // Reference and lookup.
    FunctionDef {
        name: "INDEX",
        min_args: 2,
        max_args: Some(4),
        kind: FunctionKind::Reference,
    },
    FunctionDef {
        name: "LOOKUP",
        min_args: 2,
        max_args: Some(3),
        kind: FunctionKind::Reference,
    },
    FunctionDef {
        name: "NA",
        min_args: 0,
        max_args: Some(0),
        kind: FunctionKind::Reference,
    },
    // Guard and assignment.
    FunctionDef {
        name: "GUARD",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::Guard,
    },
    FunctionDef {
        name: "SETF",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::Guard,
    },
    // Visio-owned functions (task P7-31, ADR 0084 rule 2): the glue
    // formulas of a Visio-written file (`docs/ui/connectors.md`
    // section 8) carry these calls. The evaluator keeps the cell's
    // cached value for them and reports nothing; the arities record the
    // observed spellings.
    FunctionDef {
        name: "PAR",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::VisioOwned,
    },
    FunctionDef {
        name: "PNT",
        min_args: 2,
        max_args: Some(2),
        kind: FunctionKind::VisioOwned,
    },
    FunctionDef {
        name: "_WALKGLUE",
        min_args: 3,
        max_args: Some(3),
        kind: FunctionKind::VisioOwned,
    },
    FunctionDef {
        name: "_XFTRIGGER",
        min_args: 1,
        max_args: Some(1),
        kind: FunctionKind::VisioOwned,
    },
];

/// Looks a function up by name (case-insensitive per ADR 0053).
///
/// Returns `Ok(None)` for a name the table does not hold; the caller
/// applies the unknown-function rule (ADR 0053). The `Result` is part of
/// the committed signature (P6-01): this lookup has no failure path.
pub fn lookup_function(name: &str) -> Result<Option<&'static FunctionDef>, Error> {
    Ok(FUNCTIONS
        .iter()
        .find(|def| def.name.eq_ignore_ascii_case(name)))
}

/// True when the evaluator computes the function (ADR 0053).
///
/// The names [`FUNCTIONS`] holds, except the [`FunctionKind::VisioOwned`]
/// ones: those keep the cell's cached value and are never computed, so
/// they are as unimplemented as a name the table does not hold (ADR
/// 0084 rule 2). A caller that compares computed values with cached
/// ones (the crosscheck classifier) skips both the same way.
pub fn is_supported(name: &str) -> bool {
    matches!(
        lookup_function(name),
        Ok(Some(def)) if def.kind != FunctionKind::VisioOwned
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive_and_finds_every_row() {
        for def in FUNCTIONS {
            let found = lookup_function(&def.name.to_ascii_lowercase());
            assert_eq!(found, Ok(Some(def)));
            if def.kind == FunctionKind::VisioOwned {
                assert!(!is_supported(def.name), "{} is keep-cached", def.name);
            } else {
                assert!(is_supported(def.name));
            }
        }
    }

    #[test]
    fn the_visio_owned_functions_are_tabled_but_not_supported() {
        for name in ["PAR", "PNT", "_WALKGLUE", "_XFTRIGGER"] {
            let found = lookup_function(name).expect("lookup");
            let def = found.expect("the Visio-owned rows are in the table");
            assert_eq!(def.kind, FunctionKind::VisioOwned);
            assert!(!is_supported(name));
        }
    }

    #[test]
    fn lookup_reports_an_unknown_name_as_none() {
        assert_eq!(lookup_function("THEMEVAL"), Ok(None));
        assert_eq!(lookup_function(""), Ok(None));
        assert!(!is_supported("THEMEVAL"));
        assert!(!is_supported("_MOD"));
        assert!(!is_supported("RAND"));
    }

    #[test]
    fn every_name_appears_once() {
        for (i, def) in FUNCTIONS.iter().enumerate() {
            for other in FUNCTIONS.iter().skip(i + 1) {
                assert_ne!(def.name, other.name);
            }
        }
    }

    #[test]
    fn every_arity_is_well_formed() {
        for def in FUNCTIONS {
            assert!(def.min_args <= def.max_args.unwrap_or(u8::MAX));
            if let Some(max) = def.max_args {
                assert!(def.min_args <= max);
            }
        }
    }

    #[test]
    fn mod_shares_the_modulus_shape() {
        let modulus = lookup_function("MODULUS");
        let modulus = modulus.ok().flatten();
        let alias = lookup_function("mod").ok().flatten();
        assert_eq!(
            modulus.map(|d| (d.min_args, d.max_args)),
            Some((2, Some(2)))
        );
        assert_eq!(alias.map(|d| (d.min_args, d.max_args)), Some((2, Some(2))));
    }

    #[test]
    fn a_function_def_holds_its_shape() {
        let d = FunctionDef {
            name: "IF",
            min_args: 2,
            max_args: Some(3),
            kind: FunctionKind::Logic,
        };
        assert_eq!(d.name, "IF");
        assert_eq!(d.min_args, 2);
        assert_eq!(d.max_args, Some(3));
        assert_eq!(d.kind, FunctionKind::Logic);
    }

    #[test]
    fn every_function_kind_exists() {
        let kinds = [
            FunctionKind::Arithmetic,
            FunctionKind::Trigonometry,
            FunctionKind::Logic,
            FunctionKind::Text,
            FunctionKind::Color,
            FunctionKind::Reference,
            FunctionKind::Guard,
            FunctionKind::Action,
            FunctionKind::Internal,
        ];
        assert_eq!(kinds.len(), 9);
    }
}
