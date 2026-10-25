//! Cached cell values: the `V`, `U` and `E` attributes of a `Cell` element.
//!
//! A `Cell` element carries the cached value of one ShapeSheet cell in three
//! attributes ([MS-VSDX] 2.2.11.2.2.2): `V` is the value, `U` is the unit the
//! value is declared in and `E` is the error state of the last formula
//! evaluation. The model keeps the value in `CellValue` and the declared unit
//! in `Cell::unit` (ADR 0011).
//!
//! Two rules decide what a cached value becomes:
//!
//! - **The value is stored in canonical units** (ADR 0011): lengths in
//!   inches, angles in radians (see the amendment in that ADR), percentages
//!   as fractions. The corpus shows it: `PageWidth V='11.69291338582677'
//!   U='MM'` is a 297 mm page written in inches, `Angle
//!   V='1.5707963267949' U='DEG'` is a right angle written in radians. `V`
//!   is therefore parsed as-is; no unit conversion happens on read (and none
//!   is needed on write).
//! - **`U` declares the kind of the value** and names the unit the file
//!   wrote. The codes are the `U attribute value` table of [MS-VSDX]
//!   2.2.5.3.3.1 (`MM`, `PT`, `IN_F`, `DA`, `DL`, ...). Codes that name a
//!   [`Unit`] variant map to it; every other code (`BOOL`, `STR`, `COLOR`,
//!   `DATE`, `GUID`, `DT`, ...) has no `Unit` spelling and is left in
//!   `Cell::preserved`, so a write can emit it verbatim.
//!
//! Value kinds:
//!
//! | `U` code | `CellValue` |
//! |---|---|
//! | none or unknown | classified by the value: a number literal is `Number`, a `#RGB`/`#RRGGBB` literal is `Color`, anything else is `Str` |
//! | numeric codes (`IN`, `MM`, `DEG`, `DL`, ...) | `Number`; a value that does not parse is `Error::InvalidPackage` naming the part and the cell |
//!
//! One rule covers both number paths (task P6-18 F9): **a numeric
//! literal must be finite**. `1e999999`, `inf` and `nan` parse to the
//! IEEE infinity or NaN and the model has no number for them: the
//! writer refuses to write a non-finite number (`mutate::checked_number`)
//! and the evaluator maps one to `#NUM!`. Visio itself writes such a
//! literal (`V='NaN'`, task P8-19 cause 3), so it is not an error: the
//! value reads as `None` and the caller keeps `V` and `U` verbatim,
//! exactly like an error cell keeps the value it was written with. A
//! numeric literal that does not parse at all is still
//! `Error::InvalidPackage` naming the part and the cell (never the
//! value).
//! | `BOOL` | `Bool`, from `0`/`1` (also `true`/`FALSE`); anything else is `Error::InvalidPackage` |
//! | `COLOR` | `Color` for a `#RGB`/`#RRGGBB` literal, `Str` otherwise (a themed or indexed color is kept as written) |
//! | `STR`, `DATE`, `GUID`, `PNT`, `CY` | `Str` (a currency literal carries its currency code, `V='0.0000USD'`, [MS-VSDX] 2.2.11.2.2.2.4: value plus currency, kept as written) |
//! | `LIST` | `List`, kept as its raw `;`-separated text |
//!
//! `LIST` is **our own spelling** (task P6-08, P2-14): [MS-VSDX] has no
//! `U` code for a list value and Visio writes list strings (the
//! `LayerMember` layer index list, a `Property` row's `Format` choices)
//! without any code, so a list and a string would read back as the same
//! kind. The writer spells `U="LIST"` only for a model-built
//! [`CellValue::List`] (see [`crate::write::value`]); a code-less `V`
//! keeps reading by its shape, exactly as the corpus files spell it.
//!
//! One spelling overrides every kind rule: a `V` of exactly `Themed` is the
//! theme marker and maps to `Str("Themed")` whatever `U` declares (ADR 0021
//! amendment).
//!
//! When `E` is present the cached value is the error itself
//! (`CellValue::Error`), and `V` and `U` are kept in `Preserved` instead:
//! the model has one value slot per cell and an error cell keeps the value
//! and unit it was written with verbatim (task P2-06).

use larrygram_geom::Unit;
use larrygram_model::CellValue;

use crate::Error;
use crate::value_parse::{Shaped, parse_color, shape};

/// The kind of value a `U` attribute code declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A number in a declared unit (length, angle, ratio or plain number).
    Number,
    /// A boolean, stored as `0` or `1`.
    Bool,
    /// A color.
    Color,
    /// A string.
    Str,
    /// A list, kept as its raw `;`-separated text.
    List,
    /// No declared kind: the value is classified by its own shape.
    Unknown,
}

/// The `U` codes that declare a numeric value ([MS-VSDX] 2.2.5.3.3.1: every
/// code whose default value is a number). `CY` is not one of them: its
/// literal carries the currency code (`V='0.0000USD'`) and is kept as
/// written (the module docs).
const NUMBER_CODES: &[&str] = &[
    "AC", "AD", "C", "C_D", "CM", "DA", "DE", "DEG", "DP", "DT", "ED", "EH", "EM", "ES", "EW",
    "F_I", "FT", "HA", "IN", "IN_F", "KM", "M", "MI", "MI_F", "MM", "NM", "P", "P_PT", "PER", "PT",
    "RAD", "YD", "DL",
];

/// The kind of value the `U` code `unit_code` declares.
pub(crate) fn kind(unit_code: Option<&str>) -> Kind {
    match unit_code {
        None => Kind::Unknown,
        Some("BOOL") => Kind::Bool,
        Some("COLOR") => Kind::Color,
        Some("LIST") => Kind::List,
        Some("STR") | Some("DATE") | Some("GUID") | Some("PNT") | Some("CY") => Kind::Str,
        Some(code) if NUMBER_CODES.contains(&code) => Kind::Number,
        Some(_) => Kind::Unknown,
    }
}

/// The [`Unit`] the `U` code `unit_code` spells, or `None` when the code has
/// no `Unit` spelling (see the module docs). An absent code is
/// [`Unit::Unitsless`].
///
/// Only codes that map back to the same spelling are translated here, so a
/// write can restore the file's own spelling from `Cell::unit` alone. `NM` is
/// nautical miles ([MS-VSDX] 2.2.5.3.3.1 has no nanometer code), so it is
/// deliberately not [`Unit::Nanometers`].
pub(crate) fn unit(unit_code: Option<&str>) -> Option<Unit> {
    match unit_code {
        None => None,
        Some("IN") => Some(Unit::Inches),
        Some("FT") => Some(Unit::Feet),
        Some("YD") => Some(Unit::Yards),
        Some("MI") => Some(Unit::Miles),
        Some("MM") => Some(Unit::Millimeters),
        Some("CM") => Some(Unit::Centimeters),
        Some("M") => Some(Unit::Meters),
        Some("KM") => Some(Unit::Kilometers),
        Some("PT") => Some(Unit::Points),
        Some("P") => Some(Unit::Picas),
        Some("C") => Some(Unit::Ciceros),
        Some("DEG") => Some(Unit::Degrees),
        Some("RAD") => Some(Unit::Radians),
        Some("PER") => Some(Unit::Percent),
        Some(_) => None,
    }
}

/// The cached value of `V` for a cell named `cell` in `part`, given the
/// declared kind of `U`.
///
/// A numeric or boolean literal that does not parse is
/// [`Error::InvalidPackage`] naming the part and the cell. A numeric
/// literal that is not finite has no model representation (task P8-19
/// cause 3) and reads as `Ok(None)`: the caller keeps `V` and `U`
/// verbatim. Anything else degrades to a value of another kind (never a
/// panic).
pub(crate) fn value(
    v: &str,
    unit_code: Option<&str>,
    part: &str,
    cell: &str,
) -> Result<Option<CellValue>, Error> {
    // The `Themed` marker: the value comes from the theme whatever the
    // declared kind (ADR 0021 amendment). It is kept as the string and
    // resolved later by `larrygram_model::CellReader`. Our own `LIST`
    // spelling is the one exception: its `V` is a raw list spelling, and
    // the writer only emits the code for a model-built list (P6-08), so
    // `V='Themed' U='LIST'` is a one-entry list, not a theme marker.
    if v.trim() == THEMED && kind(unit_code) != Kind::List {
        return Ok(Some(CellValue::Str(THEMED.to_owned())));
    }
    Ok(match kind(unit_code) {
        Kind::Number => match v.trim().parse::<f64>() {
            Ok(number) => finite_number(number),
            Err(_) => return Err(bad_value(part, cell, "numeric value does not parse")),
        },
        Kind::Bool => match v.trim() {
            "0" | "false" | "FALSE" => Some(CellValue::Bool(false)),
            "1" | "true" | "TRUE" => Some(CellValue::Bool(true)),
            _ => return Err(bad_value(part, cell, "boolean value is neither 0 nor 1")),
        },
        Kind::Color => Some(match parse_color(v) {
            Some(color) => CellValue::Color(color),
            None => CellValue::Str(v.to_owned()),
        }),
        Kind::Str => Some(CellValue::Str(v.to_owned())),
        Kind::List => Some(CellValue::List(v.to_owned())),
        Kind::Unknown => match shape(v) {
            Shaped::Number(number) => finite_number(number),
            Shaped::Color(color) => Some(CellValue::Color(color)),
            Shaped::Str(text) => Some(CellValue::Str(text)),
        },
    })
}

/// The model number of a finite literal, or `None` for one that is not
/// (task P6-18 F9, task P8-19 cause 3: `1e999999`, `inf` and `nan` parse
/// to the IEEE infinity or NaN and the model holds no such number).
fn finite_number(number: f64) -> Option<CellValue> {
    number.is_finite().then_some(CellValue::Number(number))
}

// What a value of unknown declared kind looks like, and the shape rules
// that classify one, live in `crate::value_parse` (shared with the writer).

/// The literal cached value of a cell whose value comes from the theme
/// (ADR 0021 amendment).
const THEMED: &str = "Themed";

/// An [`Error::InvalidPackage`] naming the part and the cell of a bad cached
/// value. The message carries the part name and the cell name only, never the
/// value itself.
pub(crate) fn bad_value(part: &str, cell: &str, why: &str) -> Error {
    Error::InvalidPackage(format!("{part}: cell `{cell}`: {why}"))
}

#[cfg(test)]
mod tests {
    use larrygram_model::Color;

    use super::*;

    #[test]
    fn numeric_codes_are_numbers() {
        assert_eq!(kind(Some("MM")), Kind::Number);
        assert_eq!(kind(Some("IN_F")), Kind::Number);
        assert_eq!(kind(Some("DA")), Kind::Number);
        assert_eq!(kind(Some("DL")), Kind::Number);
    }

    #[test]
    fn string_and_special_codes_are_not_numbers() {
        assert_eq!(kind(Some("BOOL")), Kind::Bool);
        assert_eq!(kind(Some("COLOR")), Kind::Color);
        assert_eq!(kind(Some("STR")), Kind::Str);
        assert_eq!(kind(Some("DATE")), Kind::Str);
        assert_eq!(kind(Some("GUID")), Kind::Str);
        assert_eq!(kind(Some("PNT")), Kind::Str);
        // A currency literal carries its currency code (task P8-19).
        assert_eq!(kind(Some("CY")), Kind::Str);
        assert_eq!(kind(Some("XYZ")), Kind::Unknown);
        assert_eq!(kind(None), Kind::Unknown);
    }

    #[test]
    fn unit_codes_map_to_the_unit_enum() {
        assert_eq!(unit(Some("IN")), Some(Unit::Inches));
        assert_eq!(unit(Some("MM")), Some(Unit::Millimeters));
        assert_eq!(unit(Some("PT")), Some(Unit::Points));
        assert_eq!(unit(Some("DEG")), Some(Unit::Degrees));
        assert_eq!(unit(Some("RAD")), Some(Unit::Radians));
        assert_eq!(unit(Some("PER")), Some(Unit::Percent));
        assert_eq!(unit(Some("C")), Some(Unit::Ciceros));
    }

    #[test]
    fn unit_codes_without_a_unit_spelling_stay_unmapped() {
        // `NM` is nautical miles, not nanometers; the fractional and
        // compound codes have no `Unit` variant at all.
        for code in [
            "NM", "IN_F", "F_I", "DT", "DA", "DL", "BOOL", "STR", "COLOR", "LIST", "?",
        ] {
            assert_eq!(unit(Some(code)), None, "code {code}");
        }
        assert_eq!(unit(None), None);
    }

    #[test]
    fn numbers_parse_without_conversion() {
        // V is already canonical: no factor is applied.
        assert_eq!(
            value("11.69291338582677", Some("MM"), "p", "PageWidth"),
            Ok(Some(CellValue::Number(11.69291338582677)))
        );
        // The file's own spelling of a right angle in radians; kept as
        // written, whatever `U` says.
        let right_angle = "1.5707963267949".parse::<f64>().expect("float");
        assert_eq!(
            value("1.5707963267949", Some("DEG"), "p", "Angle"),
            Ok(Some(CellValue::Number(right_angle)))
        );
        assert_eq!(
            value("-6.103171559912468E-6", Some("DA"), "p", "C"),
            Ok(Some(CellValue::Number(-6.103171559912468E-6)))
        );
    }

    #[test]
    fn a_bad_number_is_an_error_naming_part_and_cell() {
        let error = value("word", Some("MM"), "/visio/pages/page1.xml", "PinX");
        assert_eq!(
            error,
            Err(Error::InvalidPackage(
                "/visio/pages/page1.xml: cell `PinX`: numeric value does not parse".to_owned()
            ))
        );
    }

    #[test]
    fn a_non_finite_literal_reads_as_no_value() {
        // Visio writes `V='NaN'` (task P8-19 cause 3): the model holds no
        // non-finite number, so the value slot stays empty and the caller
        // keeps the literal verbatim. A literal that does not parse at
        // all is still an error.
        assert_eq!(value("NaN", Some("DL"), "p", "c"), Ok(None));
        assert_eq!(value("inf", None, "p", "c"), Ok(None));
        assert_eq!(value("-1e999999", Some("IN"), "p", "c"), Ok(None));
    }

    #[test]
    fn a_currency_literal_is_kept_as_written() {
        // `V` carries value and currency code together ([MS-VSDX]
        // 2.2.11.2.2.2.4); the model has no currency kind, so the literal
        // is kept as written (task P8-19 cause 2).
        assert_eq!(
            value("0.0000USD", Some("CY"), "p", "Value"),
            Ok(Some(CellValue::Str("0.0000USD".to_owned())))
        );
        assert_eq!(
            value("5", Some("CY"), "p", "Value"),
            Ok(Some(CellValue::Str("5".to_owned())))
        );
    }

    #[test]
    fn booleans_parse_zero_and_one() {
        assert_eq!(
            value("0", Some("BOOL"), "p", "c"),
            Ok(Some(CellValue::Bool(false)))
        );
        assert_eq!(
            value("1", Some("BOOL"), "p", "c"),
            Ok(Some(CellValue::Bool(true)))
        );
        assert_eq!(
            value("TRUE", Some("BOOL"), "p", "c"),
            Ok(Some(CellValue::Bool(true)))
        );
    }

    #[test]
    fn a_bad_boolean_is_an_error() {
        assert!(matches!(
            value("yes", Some("BOOL"), "p", "c"),
            Err(Error::InvalidPackage(_))
        ));
    }

    #[test]
    fn colors_parse_hex_literals() {
        assert_eq!(
            value("#ffffff", Some("COLOR"), "p", "c"),
            Ok(Some(CellValue::Color(Color::new(255, 255, 255))))
        );
        assert_eq!(
            value("#7F7f7F", None, "p", "c"),
            Ok(Some(CellValue::Color(Color::new(127, 127, 127))))
        );
        assert_eq!(
            value("#abc", None, "p", "c"),
            Ok(Some(CellValue::Color(Color::new(170, 187, 204))))
        );
    }

    #[test]
    fn a_non_literal_color_degrades_to_a_string() {
        assert_eq!(
            value("Themed", Some("COLOR"), "p", "c"),
            Ok(Some(CellValue::Str("Themed".to_owned())))
        );
    }

    #[test]
    fn declared_strings_stay_strings() {
        assert_eq!(
            value("Calibri", Some("STR"), "p", "c"),
            Ok(Some(CellValue::Str("Calibri".to_owned())))
        );
        assert_eq!(
            value("2015-07-06T11:05:49", Some("DATE"), "p", "c"),
            Ok(Some(CellValue::Str("2015-07-06T11:05:49".to_owned())))
        );
        assert_eq!(
            value("42", Some("GUID"), "p", "c"),
            Ok(Some(CellValue::Str("42".to_owned())))
        );
    }

    #[test]
    fn unknown_codes_classify_by_shape() {
        assert_eq!(
            value("2.5", None, "p", "c"),
            Ok(Some(CellValue::Number(2.5)))
        );
        assert_eq!(
            value("#4d4d4d", Some("XYZ"), "p", "c"),
            Ok(Some(CellValue::Color(Color::new(77, 77, 77))))
        );
        assert_eq!(
            value("Themed", None, "p", "c"),
            Ok(Some(CellValue::Str("Themed".to_owned())))
        );
        assert_eq!(
            value("", None, "p", "c"),
            Ok(Some(CellValue::Str(String::new())))
        );
        assert_eq!(
            value(";Not Started;Done", Some("XYZ"), "p", "c"),
            Ok(Some(CellValue::Str(";Not Started;Done".to_owned())))
        );
    }

    #[test]
    fn the_list_code_reads_a_raw_list() {
        // Our own kind spelling (P6-08): `LIST` keeps the raw text as a
        // list, whatever shape the text has.
        assert_eq!(kind(Some("LIST")), Kind::List);
        assert_eq!(
            value("1;0", Some("LIST"), "p", "LayerMember"),
            Ok(Some(CellValue::List("1;0".to_owned())))
        );
        assert_eq!(
            value("x", Some("LIST"), "p", "c"),
            Ok(Some(CellValue::List("x".to_owned())))
        );
        assert_eq!(
            value("", Some("LIST"), "p", "c"),
            Ok(Some(CellValue::List(String::new())))
        );
        // A one-entry list of `Themed` stays a list: the theme-marker
        // rule covers the codes a file writes, not our own one.
        assert_eq!(
            value("Themed", Some("LIST"), "p", "c"),
            Ok(Some(CellValue::List("Themed".to_owned())))
        );
    }

    #[test]
    fn a_code_less_list_spelling_stays_a_string() {
        // Visio writes list strings without a code (`LayerMember
        // V='1;0'`, a `Format` choice list); the corpus spelling is
        // kept as written (the module docs).
        assert_eq!(
            value("1;0", None, "p", "c"),
            Ok(Some(CellValue::Str("1;0".to_owned())))
        );
    }

    #[test]
    fn the_themed_marker_overrides_the_declared_kind() {
        // ADR 0021 amendment: `Themed` is a marker, not a bad number.
        assert_eq!(
            value("Themed", Some("MM"), "p", "c"),
            Ok(Some(CellValue::Str("Themed".to_owned())))
        );
        assert_eq!(
            value("Themed", Some("PER"), "p", "c"),
            Ok(Some(CellValue::Str("Themed".to_owned())))
        );
        assert_eq!(
            value("Themed", Some("BOOL"), "p", "c"),
            Ok(Some(CellValue::Str("Themed".to_owned())))
        );
        assert_eq!(
            value("themed", None, "p", "c"),
            Ok(Some(CellValue::Str("themed".to_owned())))
        );
    }
}
