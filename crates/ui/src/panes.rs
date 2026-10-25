//! The Shape Data and Size & Position panes (ADR 0035, ADR 0058).
//!
//! With no selection the panes show honest page and document data;
//! with a selection they show the anchor shape (the panes' subject,
//! ADR 0041): its `Prop` rows typed (ADR 0058) and its size, position
//! and angle as typed fields. An edit is parsed with the formula
//! grammar's own parser, converted through the engine's unit table and
//! dispatched as one `shape.set.*` command (P5-05); the pane itself
//! never mutates `AppCtx` and never opens a dialog.
//!
//! # Typed values (ADR 0058)
//!
//! A Shape Data row shows the cached value's kind (ADR 0052): a number
//! with its unit in [`DisplayUnits`] (the `Settings::units` display
//! system; angles in degrees, ratios in percent), a boolean as a check
//! box, a colour as its `#RRGGBB` RGB value, a string as written and
//! every other kind as its cached text. The `Prop` row's `Type` cell is
//! only a Shape Data format hint (P4-06); when it disagrees with the
//! cached kind the cached value wins ("it is what the file holds"), so
//! the panes show the cached kind and never consult `Type`.
//!
//! # Typed input (ADR 0058)
//!
//! An editable field takes the literals the grammar does (`2.5 in`,
//! `2.5 cm`, `37%`, `45 deg`, `5 1/2`; ADR 0051) and converts the
//! parsed value through the engine's unit table (ADR 0052) into the
//! field's own unit before the write: the panes take inches and
//! degrees (P5-05). A literal without a unit is a plain number of the
//! field's unit (`5 1/2` is 5.5 in in a width field) and a ratio is its
//! fraction (`37%` is 0.37); any other unit converts over the canonical
//! units of its family (`2.5 cm` is 0.984 in in a width field). The
//! field shows the value in [`DisplayUnits`] (or degrees), so what is
//! shown is itself a literal the grammar accepts.
//!
//! # Formula cells are read-only (ADR 0058)
//!
//! A cell that carries a formula that *computes* — one that reads a cell
//! or calls a function, its own (`Inh` included) or the one that defines
//! it through the lookup — is marked `(formula)` and refuses edits with
//! one status line: typing a bare number into a formula cell would
//! silently replace the formula and break the master link. A constant
//! literal (`5 in`, the literal the `shape.set.*` writes carry) is not
//! such a formula (P6-07's classification) and stays editable.
//! Editing formulas is the formula editor of a later phase.
//!
//! # Editing a Shape Data value (task P6-18)
//!
//! The value column of a `Prop` row takes the value's own spelling in
//! the kind the row caches (ADR 0058's "the cached value wins"): a
//! number literal of the grammar, `TRUE`/`FALSE`, a `#RGB`/`#RRGGBB`
//! colour, or the text of a string. One commit dispatches
//! `data.set.property` with the row's key and the typed text; the
//! command parses the text again (one rule, one grammar). A boolean is
//! a check box whose click dispatches the toggled value. The rows are
//! the union of the shape's own `Property` rows and the ones it inherits
//! from its master chain (the P6-09 note): an edit of an inherited row
//! writes a local row on the instance. A list or error value has no
//! literal spelling to type back and stays read-only.
//!
//! # Missing values
//!
//! A missing, non-finite or empty value renders `"—"` and records one
//! warning on [`AppCtx::warnings`] (the P4-07 warning-sink fix). The row
//! builders take `&mut AppCtx` so the warning goes to the status bar
//! directly instead of riding along in a fake row.

use egui::{RichText, Ui};
use larrygram_commands::{
    AppCtx, ArgValue, Args, CommandId, DisplayUnits, DocumentSession, Invocation, Source,
};
use larrygram_model::{
    CellLookup, CellName, CellReader, CellValue, Page, PageId, ShapeId, SheetRef, ValueSource,
};
use larrygram_shapesheet::{
    CellRef, Error as FormulaError, EvalContext, EvalWarning, Expr, ExprKind, UnaryOp, Unit,
    UnitFamily, Value, eval, parse,
};

use crate::Intent;
use crate::UiError;
use crate::canvas::{format_length, format_number};

/// The value shown for a missing, non-finite or empty value (ADR 0035).
const MISSING_VALUE: &str = "—";

/// The marker a formula cell carries beside its value (ADR 0058).
const FORMULA_MARKER: &str = "(formula)";

/// The hover text of [`FORMULA_MARKER`].
const FORMULA_HINT: &str = "This cell carries a formula: read-only in this release";

/// The status line of a refused edit of a formula cell (ADR 0058).
const FORMULA_REFUSAL: &str = "is a formula cell; formulas are read-only in this release";

/// The width of a typed field, in points.
const FIELD_WIDTH: f32 = 140.0;

/// How far a text box insets its text (egui's default `TextEdit`
/// margin): read-only values start that far in to line up with typed
/// ones.
const FIELD_TEXT_INSET: f32 = 4.0;

/// One label/value row of a pane (ADR 0035).
#[derive(Debug, Clone, PartialEq)]
pub struct PaneRow {
    /// The row label ("Width", "Name", ...).
    pub label: String,
    /// The formatted value; "—" for missing or non-finite values.
    pub value: String,
    /// The edit command and arguments for a typed field; `None` for a
    /// read-only row (a formula cell is read-only, ADR 0058).
    pub edit: Option<(CommandId, Args)>,
    /// The numeric value behind a typed row (inches or degrees).
    pub numeric: Option<f64>,
}

impl PaneRow {
    /// A read-only row.
    pub fn read_only(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            edit: None,
            numeric: None,
        }
    }

    /// An editable numeric row.
    fn editable(
        label: &str,
        value: String,
        numeric: f64,
        command: &'static str,
        args: Args,
    ) -> Result<Self, UiError> {
        let command = CommandId::parse(command).map_err(|error| UiError::Io {
            message: format!("the pane command id {command} is not valid: {error}"),
        })?;
        Ok(Self {
            label: label.to_owned(),
            value,
            edit: Some((command, args)),
            numeric: Some(numeric),
        })
    }
}

/// The unit a typed entry is read in (P5-05: the panes take inches and
/// degrees, [`DisplayUnits`] is the display system only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryUnit {
    /// Inches: the width, height and pin fields.
    Length,
    /// Degrees: the angle field.
    Angle,
}

impl EntryUnit {
    /// The engine unit the entry lands in before the command's own
    /// argument conversion (ADR 0058).
    const fn unit(self) -> Unit {
        match self {
            EntryUnit::Length => Unit::Inches,
            EntryUnit::Angle => Unit::Degrees,
        }
    }

    /// The field value of one number in canonical units (inches or
    /// radians): inches for a length field, degrees for the angle.
    const fn field_value(self, canonical: f64) -> f64 {
        match self {
            EntryUnit::Length => canonical,
            EntryUnit::Angle => canonical.to_degrees(),
        }
    }
}

/// What the value column of one row does (ADR 0058).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    /// A read-only value: a check box for `Some(flag)`, a label else.
    Shown(Option<bool>),
    /// A typed field in this unit, dispatched on commit.
    Typed(EntryUnit),
    /// A typed Shape Data value field of this kind (task P6-18),
    /// dispatched as `data.set.property` on commit.
    Value(PropKind),
    /// An editable boolean check box (task P6-18): a click dispatches
    /// `data.set.property` with the toggled value.
    Flag,
}

/// The kind of one Shape Data value (ADR 0058: "the cached value
/// wins"): what the pane shows and what a typed entry may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropKind {
    /// A number (in the cell's own unit).
    Number,
    /// A boolean (a check box).
    Bool,
    /// A colour (`#RRGGBB`).
    Color,
    /// A string (as written).
    Str,
}

/// The editable kind of a cached value, or `None`: a list or error
/// value has no literal spelling to type back and stays read-only
/// (task P6-18).
fn prop_kind(value: &CellValue) -> Option<PropKind> {
    match value {
        CellValue::Number(_) => Some(PropKind::Number),
        CellValue::Bool(_) => Some(PropKind::Bool),
        CellValue::Color(_) => Some(PropKind::Color),
        CellValue::Str(_) => Some(PropKind::Str),
        _ => None,
    }
}

/// One pane row and what its value column does (ADR 0058).
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    /// The row as [`shape_data_rows`] and [`size_position_rows`] show it.
    row: PaneRow,
    /// What the value column does when it is typed into.
    field: Field,
    /// Whether the cell behind the row carries a formula (ADR 0058).
    formula: bool,
    /// Hover text for the label, when it adds something (a `Prop` row
    /// name the label does not show).
    note: Option<String>,
}

impl Entry {
    /// A read-only row with no note and no formula.
    fn shown(row: PaneRow) -> Self {
        Self {
            row,
            field: Field::Shown(None),
            formula: false,
            note: None,
        }
    }
}

/// The buffer of one typed field between frames.
#[derive(Debug, Clone)]
struct FieldState {
    /// The text shown in the field.
    text: String,
    /// Whether the user is editing it (the buffer wins over the row).
    editing: bool,
}

/// Builds the Shape Data rows (ADR 0035, ADR 0058).
///
/// With a selection: the anchor shape's name and Visio id, then one row
/// per `Prop` row (the label, the row name and the typed `Value`). Without
/// one: the document's `Property` rows (name, value) and the active page's
/// name. A missing value renders "—" and records one warning.
pub fn shape_data_rows(app: &mut AppCtx) -> Result<Vec<PaneRow>, UiError> {
    Ok(shape_data_entries(app)?
        .into_iter()
        .map(|entry| entry.row)
        .collect())
}

/// Builds the Size & Position rows (ADR 0035, ADR 0058).
///
/// With a selection: the anchor shape's `Width`, `Height`, `PinX`,
/// `PinY` and `Angle` as typed fields (the `shape.set.*` commands),
/// then the evaluated `LocPinX`, `LocPinY`, `FlipX` and `FlipY`. Cells
/// that carry a formula are read-only (ADR 0058). Without a selection:
/// the active page's width and height in
/// [`larrygram_commands::Settings::units`], then its name. A missing
/// value renders "—" and records one warning.
pub fn size_position_rows(app: &mut AppCtx) -> Result<Vec<PaneRow>, UiError> {
    Ok(size_position_entries(app)?
        .into_iter()
        .map(|entry| entry.row)
        .collect())
}

/// Draws the Shape Data pane (ADR 0035) and returns the edit intents.
pub fn show_shape_data(ui: &mut Ui, app: &mut AppCtx) -> Result<Vec<Intent>, UiError> {
    let entries = shape_data_entries(app)?;
    Ok(show_pane(ui, app, "Shape Data", &entries))
}

/// Draws the Size & Position pane (ADR 0035) and returns the edit
/// intents.
pub fn show_size_position(ui: &mut Ui, app: &mut AppCtx) -> Result<Vec<Intent>, UiError> {
    let entries = size_position_entries(app)?;
    Ok(show_pane(ui, app, "Size & Position", &entries))
}

/// Builds the Shape Data entries (ADR 0058).
fn shape_data_entries(app: &mut AppCtx) -> Result<Vec<Entry>, UiError> {
    let units = app.settings.units;
    let mut rows = Vec::new();
    let Some(session) = app.active_document() else {
        return Ok(rows);
    };
    if let Some(anchor) = app.selection.anchor
        && let Some(shape) = session.document.shape(anchor)
    {
        let name = if !shape.name.is_empty() {
            shape.name.clone()
        } else if !shape.name_u.is_empty() {
            shape.name_u.clone()
        } else {
            format!("Shape {}", shape.visio_id)
        };
        let visio_id = shape.visio_id;
        let props = prop_rows(session, anchor);
        rows.push(Entry::shown(PaneRow::read_only("Name", name)));
        rows.push(Entry::shown(PaneRow::read_only("ID", visio_id.to_string())));
        for prop in &props {
            rows.push(prop_entry(app, prop, units));
        }
        return Ok(rows);
    }
    let properties: Vec<(String, String)> = session
        .document
        .properties
        .iter()
        .map(|property| (property.name.clone(), property.value.clone()))
        .collect();
    let name = active_page(session)
        .map(|page| page.name.clone())
        .unwrap_or_default();
    for (property, value) in properties {
        push_text(app, &mut rows, &property, &value);
    }
    push_text(app, &mut rows, "Name", &name);
    Ok(rows)
}

/// Builds the Size & Position entries (ADR 0058).
fn size_position_entries(app: &mut AppCtx) -> Result<Vec<Entry>, UiError> {
    let units = app.settings.units;
    let mut rows = Vec::new();
    let Some(session) = app.active_document() else {
        return Ok(rows);
    };
    if let Some(anchor) = app.selection.anchor
        && session.document.shape(anchor).is_some()
    {
        let sheet = SheetRef::Shape(anchor);
        let data = read_rows(session, sheet);
        for entry in shape_row_entries(app, &data, units)? {
            rows.push(entry);
        }
        return Ok(rows);
    }
    let page = active_page(session);
    let page_id = page.map(|page| page.id);
    let width = page.and_then(|page| page_number(session, page.id, "PageWidth"));
    let height = page.and_then(|page| page_number(session, page.id, "PageHeight"));
    let name = page.map(|page| page.name.clone()).unwrap_or_default();
    let width_formula = page_id
        .map(|id| cell_has_formula(session, SheetRef::Page(id), "PageWidth"))
        .unwrap_or(false);
    let height_formula = page_id
        .map(|id| cell_has_formula(session, SheetRef::Page(id), "PageHeight"))
        .unwrap_or(false);
    rows.push(push_length(app, "Width", width, units, width_formula));
    rows.push(push_length(app, "Height", height, units, height_formula));
    push_text(app, &mut rows, "Name", &name);
    Ok(rows)
}

/// One Size & Position row of a selected shape (ADR 0058).
struct Spec {
    /// The row label.
    label: &'static str,
    /// The ShapeSheet cell behind the row.
    cell: &'static str,
    /// The field unit of an editable row, `None` when read-only.
    unit: Option<EntryUnit>,
    /// The editing command id and its argument name, or `None`.
    edit: Option<(&'static str, &'static str)>,
    /// Whether the cell holds a boolean (shown as a check box).
    flag: bool,
}

/// The Size & Position rows of a selected shape (ADR 0058).
const SHAPE_ROWS: &[Spec] = &[
    Spec {
        label: "Width",
        cell: "Width",
        unit: Some(EntryUnit::Length),
        edit: Some(("shape.set.width", "width")),
        flag: false,
    },
    Spec {
        label: "Height",
        cell: "Height",
        unit: Some(EntryUnit::Length),
        edit: Some(("shape.set.height", "height")),
        flag: false,
    },
    Spec {
        label: "Pin X",
        cell: "PinX",
        unit: Some(EntryUnit::Length),
        edit: Some(("shape.set.pin", "x")),
        flag: false,
    },
    Spec {
        label: "Pin Y",
        cell: "PinY",
        unit: Some(EntryUnit::Length),
        edit: Some(("shape.set.pin", "y")),
        flag: false,
    },
    Spec {
        label: "Angle",
        cell: "Angle",
        unit: Some(EntryUnit::Angle),
        edit: Some(("shape.set.angle", "angle")),
        flag: false,
    },
    Spec {
        label: "Loc Pin X",
        cell: "LocPinX",
        unit: None,
        edit: None,
        flag: false,
    },
    Spec {
        label: "Loc Pin Y",
        cell: "LocPinY",
        unit: None,
        edit: None,
        flag: false,
    },
    Spec {
        label: "Flip X",
        cell: "FlipX",
        unit: None,
        edit: None,
        flag: true,
    },
    Spec {
        label: "Flip Y",
        cell: "FlipY",
        unit: None,
        edit: None,
        flag: true,
    },
];

/// The evaluated data of one [`Spec`] row (ADR 0055: the recalc result).
struct Row {
    /// The row's specification.
    spec: &'static Spec,
    /// The evaluated number (inches or radians), when there is one.
    number: Option<f64>,
    /// The evaluated flag, when the cell holds one.
    flag: Option<bool>,
    /// Whether the cell carries a formula (ADR 0058).
    formula: bool,
}

/// Reads the evaluated Size & Position rows of one sheet. The reads
/// borrow the session, so the entries are built after (they record
/// warnings on [`AppCtx`]).
fn read_rows(session: &DocumentSession, sheet: SheetRef) -> Vec<Row> {
    SHAPE_ROWS
        .iter()
        .map(|spec| Row {
            spec,
            number: if spec.flag {
                None
            } else {
                read_number(session, sheet, spec.cell)
            },
            flag: if spec.flag {
                read_flag(session, sheet, spec.cell)
            } else {
                None
            },
            formula: cell_has_formula(session, sheet, spec.cell),
        })
        .collect()
}

/// Builds the entries of the evaluated rows (ADR 0058).
fn shape_row_entries(
    app: &mut AppCtx,
    data: &[Row],
    units: DisplayUnits,
) -> Result<Vec<Entry>, UiError> {
    let pin = pin_pair(data);
    let mut rows = Vec::new();
    for row in data {
        rows.push(shape_row_entry(app, row, pin, units)?);
    }
    Ok(rows)
}

/// The pin coordinates both `shape.set.pin` arguments need (the command
/// sets the pair, ADR 0035).
fn pin_pair(data: &[Row]) -> (Option<f64>, Option<f64>) {
    let x = data
        .iter()
        .find(|row| row.spec.cell == "PinX")
        .and_then(|row| row.number);
    let y = data
        .iter()
        .find(|row| row.spec.cell == "PinY")
        .and_then(|row| row.number);
    (x, y)
}

/// Builds one entry of the Size & Position pane (ADR 0058).
fn shape_row_entry(
    app: &mut AppCtx,
    row: &Row,
    pin: (Option<f64>, Option<f64>),
    units: DisplayUnits,
) -> Result<Entry, UiError> {
    let label = row.spec.label;
    if row.spec.flag {
        return Ok(match row.flag {
            Some(flag) => Entry {
                row: PaneRow::read_only(label, flag_text(flag)),
                field: Field::Shown(Some(flag)),
                formula: row.formula,
                note: None,
            },
            None => missing_entry(app, label, "no value"),
        });
    }
    let Some(number) = row.number.filter(|number| number.is_finite()) else {
        let reason = if row.number.is_some() {
            "not a number"
        } else {
            "no value"
        };
        return Ok(missing_entry(app, label, reason));
    };
    let text = match row.spec.unit {
        Some(EntryUnit::Angle) => format!("{}°", format_number(number.to_degrees())),
        _ => format_length(number, units),
    };
    let (Some(unit), Some((command, argument))) = (row.spec.unit, row.spec.edit) else {
        // A read-only row (LocPinX and friends): no command edits it.
        return Ok(Entry {
            row: PaneRow::read_only(label, text),
            field: Field::Shown(None),
            formula: row.formula,
            note: None,
        });
    };
    let numeric = unit.field_value(number);
    let args = if command == "shape.set.pin" {
        let (x, y) = pin;
        Args::new()
            .with("x", ArgValue::Number(x.unwrap_or(0.0)))
            .with("y", ArgValue::Number(y.unwrap_or(0.0)))
    } else {
        // The command's own unit: inches, radians for `shape.set.angle`
        // (P5-05); the field shows and takes the display form.
        Args::new().with(argument, ArgValue::Number(number))
    };
    let edit = if row.formula {
        None
    } else {
        PaneRow::editable(label, text.clone(), numeric, command, args)?.edit
    };
    Ok(Entry {
        row: PaneRow {
            label: label.to_owned(),
            value: text,
            edit,
            numeric: Some(numeric),
        },
        field: Field::Typed(unit),
        formula: row.formula,
        note: None,
    })
}

/// One `Prop` row of the Shape Data pane (ADR 0058).
struct PropRow {
    /// The label column: the `Label` cell's text, else the row name.
    label: String,
    /// Hover note (the row name), when the label does not show it.
    note: Option<String>,
    /// The row's key (`Prop.<row>.Value`), for the edit command.
    key: String,
    /// The `Value` cell's cached value and unit, when it has one.
    value: Option<(CellValue, larrygram_geom::Unit)>,
    /// Whether the `Value` cell carries a formula (ADR 0058).
    formula: bool,
}

/// Reads the `Prop` rows of one shape: the row key, the `Label` text
/// and the cached `Value` (what the file holds, ADR 0058).
///
/// The rows are the union of the shape's own `Property` rows and the
/// ones it inherits from its master chain (task P6-18, the P6-09 note):
/// the shape's own row wins on a key clash and the inherited rows fill
/// in behind it.
fn prop_rows(session: &DocumentSession, shape: ShapeId) -> Vec<PropRow> {
    let mut rows = Vec::new();
    for entry in session.document.property_rows(shape) {
        let row = &entry.row;
        let name = row.name.clone();
        let label = row
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "Label")
            .and_then(|cell| cell.value.as_ref())
            .and_then(|value| match value {
                CellValue::Str(text) if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .or_else(|| name.clone())
            .unwrap_or_else(|| format!("Row {}", entry.position + 1));
        // The row name is the hover note when the label does not show
        // it; an inherited row says where it comes from (the P6-09 note).
        let note = match (&name, entry.inherited) {
            (Some(name), false) if *name != label => Some(name.clone()),
            (Some(name), true) => Some(format!("{name} (from the master)")),
            (None, true) => Some(String::from("from the master")),
            _ => None,
        };
        let value_cell = row.cells.iter().find(|cell| cell.name.as_str() == "Value");
        rows.push(PropRow {
            label,
            note,
            key: entry.key,
            value: value_cell.and_then(|cell| cell.value.clone().map(|value| (value, cell.unit))),
            formula: value_cell
                .and_then(|cell| cell.formula.as_deref())
                .is_some_and(formula_computes),
        });
    }
    rows
}

/// Builds the entry of one `Prop` row (ADR 0058): the value typed.
///
/// The value is editable through `data.set.property` (task P6-18) when
/// the row caches a number, boolean, colour or string: a text field, or
/// a check box for a boolean. A formula value is read-only (ADR 0058)
/// and so is a value with no literal spelling to type back (a list or
/// an error).
fn prop_entry(app: &mut AppCtx, prop: &PropRow, units: DisplayUnits) -> Entry {
    let Some((value, unit)) = &prop.value else {
        return missing_entry(app, &prop.label, "no value");
    };
    let Some((text, check)) = typed_value(value, *unit, units) else {
        return missing_entry(app, &prop.label, "unsupported cached value");
    };
    let Some(kind) = prop_kind(value) else {
        // A list or error value has no literal spelling to type back.
        return Entry {
            row: PaneRow::read_only(&prop.label, text),
            field: Field::Shown(check),
            formula: prop.formula,
            note: prop.note.clone(),
        };
    };
    if prop.formula {
        // A formula value keeps its typed field and refuses the edit
        // with one status line (ADR 0058, as in the other panes). A
        // boolean is a shown check box: it takes no click at all.
        return Entry {
            row: PaneRow::read_only(&prop.label, text),
            field: if kind == PropKind::Bool {
                Field::Shown(check)
            } else {
                Field::Value(kind)
            },
            formula: true,
            note: prop.note.clone(),
        };
    }
    let row = value_row(&prop.label, text, &prop.key);
    Entry {
        row,
        field: if kind == PropKind::Bool {
            Field::Flag
        } else {
            Field::Value(kind)
        },
        formula: prop.formula,
        note: prop.note.clone(),
    }
}

/// The editable value row of one `Prop` entry: the `data.set.property`
/// command with the row's key (the commit adds the `value` argument).
fn value_row(label: &str, text: String, key: &str) -> PaneRow {
    let args = Args::new().with("row", ArgValue::Text(key.to_owned()));
    match PaneRow::editable(label, text, 0.0, "data.set.property", args) {
        Ok(mut row) => {
            row.numeric = None;
            row
        }
        // A `CommandId` parse failure is a build bug of this crate (the
        // id is a constant): the row degrades to read-only, never a
        // panic (AGENTS.md section 6).
        Err(_) => PaneRow::read_only(label, MISSING_VALUE),
    }
}

/// The typed display of one cached value (ADR 0058): the text of the
/// value and the flag of a boolean (drawn as a check box).
///
/// The kind is the cached value's kind (ADR 0052); a value the engine
/// cannot type (`Value::from_cached` refuses it) has no display here
/// and shows as "—" with one warning at the caller.
fn typed_value(
    value: &CellValue,
    unit: larrygram_geom::Unit,
    units: DisplayUnits,
) -> Option<(String, Option<bool>)> {
    let value = Value::from_cached(value, unit).ok()?;
    if let Value::Number(number) = &value {
        return Some((number_text(number, units)?, None));
    }
    if let Value::Str(text) = &value {
        return Some((text.clone(), None));
    }
    if let Value::Bool(flag) = &value {
        return Some((flag_text(*flag), Some(*flag)));
    }
    if let Value::Color(color) = &value {
        return Some((color_text(color), None));
    }
    if let Value::Err(code) = &value {
        return Some((code.as_str().to_owned(), None));
    }
    if let Value::List(entries) = &value {
        return Some((list_text(entries), None));
    }
    // `Value` is non-exhaustive: a kind this crate cannot show is not a
    // display (never a panic, AGENTS.md section 6).
    None
}

/// Formats one number for display (ADR 0058): a length in the display
/// units of `Settings::units`, an angle in degrees, a ratio in percent
/// and anything else as the cached number (a duration's spelling is
/// lost in the model, ADR 0052, so its cached number is its text).
fn number_text(number: &larrygram_shapesheet::Number, units: DisplayUnits) -> Option<String> {
    let canonical = number.unit.to_canonical(number.magnitude).ok()?;
    let family = number.unit.family().ok()?;
    let text = if family == UnitFamily::Length || family == UnitFamily::Typographic {
        format_length(canonical, units)
    } else if family == UnitFamily::Angle {
        format!("{}°", format_number(canonical.to_degrees()))
    } else if family == UnitFamily::Ratio {
        format!("{}%", format_number(canonical * 100.0))
    } else {
        format_number(number.magnitude)
    };
    Some(text)
}

/// The text of a boolean: `TRUE` or `FALSE` (the grammar's spelling).
fn flag_text(flag: bool) -> String {
    if flag {
        String::from("TRUE")
    } else {
        String::from("FALSE")
    }
}

/// The `#RRGGBB` RGB value of a colour (the spelling the file writes).
fn color_text(color: &larrygram_model::Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// The cached text of a list value: its entries as the file spells them.
fn list_text(entries: &[Value]) -> String {
    let mut text = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            text.push(';');
        }
        text.push_str(&list_entry_text(entry));
    }
    text
}

/// The cached text of one list entry.
fn list_entry_text(entry: &Value) -> String {
    if let Value::Str(text) = entry {
        return text.clone();
    }
    if let Value::Number(number) = entry {
        return format_number(number.magnitude);
    }
    if let Value::Bool(flag) = entry {
        return flag_text(*flag);
    }
    if let Value::Color(color) = entry {
        return color_text(color);
    }
    if let Value::Err(code) = entry {
        return code.as_str().to_owned();
    }
    String::new()
}

/// A literal-only evaluation context: a typed entry is a constant (ADR
/// 0058), so a reference or a call is a refused entry, and any warning
/// the evaluation records (a coercion, a non-finite result) refuses it
/// too.
#[derive(Default)]
struct LiteralOnly {
    /// How many warnings the evaluation recorded.
    warnings: usize,
}

impl EvalContext for LiteralOnly {
    fn cell_value(
        &mut self,
        _sheet: SheetRef,
        _reference: &CellRef,
    ) -> Result<Value, FormulaError> {
        Err(FormulaError::Conversion("pane entries are constants"))
    }

    fn set_cell(
        &mut self,
        _sheet: SheetRef,
        _reference: &CellRef,
        _value: Value,
    ) -> Result<(), FormulaError> {
        Err(FormulaError::Conversion("pane entries are constants"))
    }

    fn warn(&mut self, _warning: EvalWarning) {
        self.warnings = self.warnings.saturating_add(1);
    }
}

/// Whether an expression is one literal of the grammar (ADR 0058: a
/// number field takes "the same literals the grammar does"): a number,
/// string or boolean literal under any chain of unary `+`, `-` and `%`.
/// A call or a reference belongs to the formula editor of a later phase.
fn is_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Literal(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Plus | UnaryOp::Minus | UnaryOp::Percent,
            operand,
        } => is_literal(operand),
        _ => false,
    }
}

/// Parses one typed entry and converts it to the field's unit (ADR 0058).
///
/// The grammar is the formula grammar (`shapesheet::parse`, so the two
/// never disagree) and the conversion is the engine's unit table (ADR
/// 0052). A literal without a unit is a plain number of the field's
/// unit and a ratio is its fraction; any other unit converts over the
/// canonical units of its family. A failure is the reason for one
/// status line, never a panic (AGENTS.md section 6).
fn typed_entry(text: &str, unit: EntryUnit) -> Result<f64, String> {
    let bad = || format!("{text:?} is not a number");
    let expr = parse(text).map_err(|_| bad())?;
    if !is_literal(&expr) {
        return Err(bad());
    }
    let mut context = LiteralOnly::default();
    let sheet = SheetRef::Shape(ShapeId { index: 0 });
    let value = eval(&expr, sheet, &mut context).map_err(|_| bad())?;
    if context.warnings != 0 {
        return Err(format!("{text:?} is not a plain number"));
    }
    let Value::Number(number) = value else {
        return Err(bad());
    };
    let canonical = number
        .unit
        .to_canonical(number.magnitude)
        .map_err(|_| bad())?;
    let family = number.unit.family().map_err(|_| bad())?;
    let magnitude = if family == UnitFamily::Dimensionless || family == UnitFamily::Ratio {
        canonical
    } else {
        unit.unit().from_canonical(canonical).map_err(|_| bad())?
    };
    if magnitude.is_finite() {
        Ok(magnitude)
    } else {
        Err(format!("{text:?} is not a finite number"))
    }
}

/// Draws one pane: its title and the rows as a two-column grid.
fn show_pane(ui: &mut Ui, app: &mut AppCtx, title: &str, entries: &[Entry]) -> Vec<Intent> {
    ui.label(RichText::new(title).strong());
    if app.active_document().is_none() {
        ui.label("No document");
        return Vec::new();
    }
    let mut intents = Vec::new();
    egui::Grid::new(title).striped(true).show(ui, |ui| {
        for entry in entries {
            let mut label = ui.label(entry.row.label.as_str());
            if let Some(note) = &entry.note {
                label = label.on_hover_text(note.as_str());
            }
            drop(label);
            ui.horizontal(|ui| {
                if matches!(entry.field, Field::Typed(_) | Field::Value(_)) {
                    intents.append(&mut edit_field(ui, app, title, entry));
                } else {
                    intents.append(&mut show_value(ui, entry));
                }
                if entry.formula {
                    ui.label(RichText::new(FORMULA_MARKER).weak())
                        .on_hover_text(FORMULA_HINT);
                }
            });
            ui.end_row();
        }
    });
    intents
}

/// Draws one value column: a disabled check box for a shown boolean
/// (ADR 0058's "checkbox-like value"), an editable check box for an
/// editable one (task P6-18: a click dispatches `data.set.property`),
/// and a label for everything else. The editable check box returns the
/// intent of its click.
fn show_value(ui: &mut Ui, entry: &Entry) -> Vec<Intent> {
    match entry.field {
        Field::Shown(Some(flag)) => {
            let mut checked = flag;
            ui.add_enabled(
                false,
                egui::Checkbox::new(&mut checked, entry.row.value.as_str()),
            );
            Vec::new()
        }
        Field::Flag => {
            let mut checked = entry.row.value.as_str() == "TRUE";
            let response = ui.add(egui::Checkbox::new(&mut checked, entry.row.value.as_str()));
            if response.changed()
                && let Some((command, mut args)) = entry.row.edit.clone()
            {
                args.set("value", ArgValue::Text(flag_text(checked)));
                return vec![Intent::new(command, Invocation::new(args, Source::Ui))];
            }
            Vec::new()
        }
        _ => {
            // In line with the text of the typed fields beside it (a
            // text box insets its text by its margin; task P7-17).
            ui.add_space(FIELD_TEXT_INSET);
            ui.label(entry.row.value.as_str());
            Vec::new()
        }
    }
}

/// One typed field: shows the value text and, on commit, parses the
/// typed entry into the command's arguments (ADR 0058).
///
/// A formula cell refuses the edit with one status line and a bad entry
/// is refused the same way; both change nothing (no intent, no model
/// write).
fn edit_field(ui: &mut Ui, app: &mut AppCtx, title: &str, entry: &Entry) -> Vec<Intent> {
    let id = egui::Id::unique(("pane-field", title, entry.row.label.as_str()));
    let mut state = ui.data_mut(|data| {
        data.get_temp::<FieldState>(id)
            .unwrap_or_else(|| FieldState {
                text: entry.row.value.clone(),
                editing: false,
            })
    });
    if !state.editing {
        state.text = entry.row.value.clone();
    }
    let mut text = state.text.clone();
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(ui.available_width().clamp(40.0, FIELD_WIDTH))
            .hint_text(entry.row.label.as_str()),
    );
    state.text = text;
    if response.changed() {
        state.editing = true;
    }
    let mut intents = Vec::new();
    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
    if state.editing && (response.lost_focus() || (enter && response.has_focus())) {
        state.editing = false;
        intents = commit_entry(app, entry, &state.text);
        state.text = entry.row.value.clone();
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    intents
}

/// The commit of one typed entry (ADR 0058): one command to dispatch,
/// or one status line and nothing else — a formula cell refuses the
/// edit and a bad entry is refused the same way. The pane itself
/// never touches the model.
///
/// A Size & Position field carries its number in the command's own
/// unit; a Shape Data value field carries the typed text as the
/// `value` argument of `data.set.property` (task P6-18), which parses
/// it again in the row's kind (one rule, one grammar).
fn commit_entry(app: &mut AppCtx, entry: &Entry, text: &str) -> Vec<Intent> {
    if entry.formula {
        record_line(app, format!("{} {FORMULA_REFUSAL}", entry.row.label));
        return Vec::new();
    }
    let Some((command, mut args)) = entry.row.edit.clone() else {
        return Vec::new();
    };
    match entry.field {
        Field::Value(kind) => match value_entry(kind, text) {
            Ok(()) => {
                args.set("value", ArgValue::Text(text.to_owned()));
                vec![Intent::new(command, Invocation::new(args, Source::Ui))]
            }
            Err(reason) => {
                record_line(app, format!("{}: {}", entry.row.label, reason));
                Vec::new()
            }
        },
        Field::Typed(unit) => match typed_entry(text, unit) {
            Ok(value) => {
                set_edited_argument(&entry.row.label, &mut args, value);
                vec![Intent::new(command, Invocation::new(args, Source::Ui))]
            }
            Err(reason) => {
                record_line(app, format!("{}: {}", entry.row.label, reason));
                Vec::new()
            }
        },
        // A check box commits through [`show_value`], a shown value has
        // no edit.
        _ => Vec::new(),
    }
}

/// The pane's instant feedback on one typed Shape Data value (task
/// P6-18): the text must be a value of the row's kind. The command
/// parses the text again (one rule, one grammar); this check only
/// decides whether an intent leaves the pane.
fn value_entry(kind: PropKind, text: &str) -> Result<(), String> {
    match kind {
        PropKind::Number => typed_entry(text, EntryUnit::Length).map(|_| ()),
        PropKind::Bool => {
            let upper = text.trim().to_ascii_uppercase();
            if matches!(upper.as_str(), "TRUE" | "FALSE" | "1" | "0") {
                Ok(())
            } else {
                Err(format!("{text:?} is not TRUE or FALSE"))
            }
        }
        PropKind::Color => {
            // The `#RGB`/`#RRGGBB` spelling the value shows (the file's
            // own text): the formula grammar has no colour literal.
            let digits = text.trim().strip_prefix('#').unwrap_or_default();
            let hex = digits.bytes().all(|byte| byte.is_ascii_hexdigit());
            if (digits.len() == 3 || digits.len() == 6) && hex {
                Ok(())
            } else {
                Err(format!("{text:?} is not a #RRGGBB colour"))
            }
        }
        PropKind::Str => Ok(()),
    }
}

/// Writes the edited field into the command arguments.
///
/// The value is in the field's own unit (inches or degrees): `Angle`
/// converts to the radians its command takes (P5-05).
fn set_edited_argument(label: &str, args: &mut Args, value: f64) {
    match label {
        "Width" => args.set("width", ArgValue::Number(value)),
        "Height" => args.set("height", ArgValue::Number(value)),
        "Pin X" => args.set("x", ArgValue::Number(value)),
        "Pin Y" => args.set("y", ArgValue::Number(value)),
        "Angle" => args.set("angle", ArgValue::Number(value.to_radians())),
        _ => {}
    }
}

/// Records one status line (a refused entry, ADR 0058). Refusals record
/// every time: one line per refused edit.
fn record_line(app: &mut AppCtx, message: String) {
    app.record_warning(message);
}

/// The active page of a session: the page `active_page` names, or `None`
/// when there is no active page or the id is stale.
fn active_page(session: &DocumentSession) -> Option<&Page> {
    let id = session.active_page?;
    session.document.pages.iter().find(|page| page.id == id)
}

/// Reads one number cell of a sheet (inches or radians).
///
/// `None` when the cell is missing or unusable: the ADR 0021 reader
/// substitutes `0` and records a warning, which the pane treats as "no
/// value" (ADR 0035) instead of showing the substitute.
fn read_number(session: &DocumentSession, sheet: SheetRef, cell: &str) -> Option<f64> {
    let mut reader = CellReader::new(&session.document);
    let before = reader.warnings().len();
    match reader.number(sheet, &CellName::new(cell)) {
        Ok(value) if reader.warnings().len() == before => Some(value),
        _ => None,
    }
}

/// Reads one boolean cell of a sheet, the same policy as
/// [`read_number`].
fn read_flag(session: &DocumentSession, sheet: SheetRef, cell: &str) -> Option<bool> {
    let mut reader = CellReader::new(&session.document);
    let before = reader.warnings().len();
    match reader.boolean(sheet, &CellName::new(cell)) {
        Ok(value) if reader.warnings().len() == before => Some(value),
        _ => None,
    }
}

/// Reads one page number cell (inches).
fn page_number(session: &DocumentSession, page: PageId, cell: &str) -> Option<f64> {
    read_number(session, SheetRef::Page(page), cell)
}

/// Whether a formula computes something (P6-07's classification): one
/// that reads a cell or calls a function. A constant literal (`-2`,
/// `2+3`, `5 in`) computes nothing and is not a formula a write would
/// break; `Inh` parses as a reference and counts.
fn formula_computes(formula: &str) -> bool {
    /// The tree walk over one parsed formula.
    fn computes(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ref(_) => true,
            ExprKind::Call { .. } => true,
            ExprKind::Unary { operand, .. } => computes(operand),
            ExprKind::Binary { left, right, .. } => computes(left) || computes(right),
            // A literal computes nothing.
            ExprKind::Literal(_) => false,
            // `ExprKind` is non-exhaustive: a construct this crate does
            // not know counts as computing, so the cell stays read-only.
            _ => true,
        }
    }
    match parse(formula) {
        Ok(expr) => computes(&expr),
        // A formula the parser refuses is kept read-only: conservative,
        // never a silent replacement (ADR 0058).
        Err(_) => true,
    }
}

/// Whether a cell carries a formula the write would replace (ADR 0058's
/// read-only marking): its own (`Inh` included), or the one a real rung
/// of the lookup defines. A constant literal is not such a formula and a
/// documented default is not one either.
fn cell_has_formula(session: &DocumentSession, sheet: SheetRef, cell: &str) -> bool {
    let name = CellName::new(cell);
    let own = match sheet {
        SheetRef::Shape(id) => session
            .document
            .shape(id)
            .and_then(|shape| shape.sheet.cells.iter().find(|entry| entry.name == name)),
        SheetRef::Page(id) => session
            .document
            .page(id)
            .and_then(|page| page.sheet.cells.iter().find(|entry| entry.name == name)),
    };
    if own
        .and_then(|cell| cell.formula.as_deref())
        .is_some_and(formula_computes)
    {
        return true;
    }
    let resolved = match sheet {
        SheetRef::Shape(id) => session.document.resolve(id, &name),
        SheetRef::Page(id) => session.document.resolve_page_cell(id, &name),
    };
    match resolved {
        Ok(resolved) => {
            let from_table = matches!(
                resolved.source,
                ValueSource::SectionDefault | ValueSource::HardDefault
            );
            resolved.formula.is_some_and(formula_computes) && !from_table
        }
        Err(_) => false,
    }
}

/// Builds the "—" entry of a missing value and records its warning.
fn missing_entry(app: &mut AppCtx, label: &str, reason: &str) -> Entry {
    let message = format!("{label}: {reason}");
    if !app.warnings.iter().any(|warning| warning == &message) {
        app.record_warning(message);
    }
    Entry::shown(PaneRow::read_only(label, MISSING_VALUE))
}

/// Adds one length row: formatted with units, or "—" plus one warning.
/// A formula cell keeps its marker (ADR 0058).
fn push_length(
    app: &mut AppCtx,
    label: &str,
    value: Option<f64>,
    units: DisplayUnits,
    formula: bool,
) -> Entry {
    let mut entry = match value {
        Some(value) if value.is_finite() => {
            Entry::shown(PaneRow::read_only(label, format_length(value, units)))
        }
        Some(_) => missing_entry(app, label, "not a number"),
        None => missing_entry(app, label, "no value"),
    };
    entry.formula = formula;
    entry
}

/// Adds one text row: the value, or "—" plus one warning when empty.
fn push_text(app: &mut AppCtx, rows: &mut Vec<Entry>, label: &str, value: &str) {
    if value.is_empty() {
        rows.push(missing_entry(app, label, "no value"));
    } else {
        rows.push(Entry::shown(PaneRow::read_only(label, value)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;
    use larrygram_commands::{DocumentSession, Settings};
    use larrygram_geom::Unit as GeomUnit;
    use larrygram_model::{Cell, Color, Document, Page, PageId, Property, Row, Section, Shape};

    /// A page with the given name and an 8.5 x 11 in sheet.
    fn page(name: &str) -> Page {
        let mut page = Page::new(PageId { index: 0 }, 0);
        page.name = name.to_owned();
        page.sheet.cells = vec![cell("PageWidth", 8.5), cell("PageHeight", 11.0)];
        page
    }

    /// A cell with a cached number.
    fn cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A cell with a cached boolean.
    fn flag_cell(name: &str, flag: bool) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Bool(flag));
        cell
    }

    /// An app with one open document in the given display units.
    fn app_with(doc: Document, units: DisplayUnits) -> AppCtx {
        let mut settings = Settings::default();
        settings.units = units;
        let mut app = AppCtx::new(settings);
        app.open_document(DocumentSession::new(doc, None));
        app
    }

    /// A document with one named page and one property.
    fn document() -> Document {
        let mut doc = Document::new();
        doc.properties.push(Property::new("title", "Diagram 1"));
        doc.pages.push(page("Page-1"));
        doc
    }

    /// A document with one selected 2x1 shape at (3, 4), turned 30°.
    fn selected_document() -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        shape.name = "Box".to_owned();
        shape.sheet.cells = vec![
            cell("Width", 2.0),
            cell("Height", 1.0),
            cell("PinX", 3.0),
            cell("PinY", 4.0),
            cell("Angle", core::f64::consts::FRAC_PI_6),
            cell("LocPinX", 1.0),
            cell("LocPinY", 0.5),
            flag_cell("FlipX", false),
            flag_cell("FlipY", false),
        ];
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        app.selection.page = Some(PageId { index: 0 });
        (app, id)
    }

    /// A cell with a formula and a cached value.
    fn formula_cell(name: &str, formula: &str, value: f64) -> Cell {
        let mut cell = cell(name, value);
        cell.formula = Some(formula.to_owned());
        cell
    }

    /// A `Property` section row with the given `Label` and `Value` cells.
    fn prop_row(name: &str, label: Option<&str>, value: Option<Cell>) -> Row {
        let mut row = Row::new();
        row.name = Some(name.to_owned());
        let mut cells = Vec::new();
        if let Some(label) = label {
            let mut cell = Cell::new(CellName::new("Label"));
            cell.value = Some(CellValue::Str(label.to_owned()));
            cells.push(cell);
        }
        if let Some(value) = value {
            cells.push(value);
        }
        row.cells = cells;
        row
    }

    /// A `Value` cell with the given cached value and unit.
    fn value_cell(value: CellValue, unit: GeomUnit) -> Cell {
        let mut cell = Cell::new(CellName::new("Value"));
        cell.value = Some(value);
        cell.unit = unit;
        cell
    }

    /// The entry with `label` of an entry list, or panic (tests may).
    fn entry_of<'a>(entries: &'a [Entry], label: &str) -> &'a Entry {
        entries
            .iter()
            .find(|entry| entry.row.label == label)
            .unwrap_or_else(|| panic!("no entry labelled {label}"))
    }

    /// The row with `label` of a row list, or panic (tests may).
    fn row<'a>(rows: &'a [PaneRow], label: &str) -> &'a PaneRow {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no row labelled {label}"))
    }

    #[test]
    fn editable_rows_reject_an_invalid_command_id() {
        let error = PaneRow::editable("Width", "2 in".to_owned(), 2.0, "", Args::new())
            .expect_err("an empty command id is invalid");
        assert!(matches!(error, UiError::Io { .. }));
    }

    #[test]
    fn edited_arguments_use_the_command_contract_names() {
        for (label, name, expected) in [
            ("Width", "width", 2.0),
            ("Height", "height", 3.0),
            ("Pin X", "x", 4.0),
            ("Pin Y", "y", 5.0),
        ] {
            let mut args = Args::new();
            set_edited_argument(label, &mut args, expected);
            assert_eq!(args.number(name), Some(expected), "{label}");
        }
        let mut args = Args::new();
        set_edited_argument("Angle", &mut args, 180.0);
        let angle = args.number("angle").expect("angle");
        assert!((angle - core::f64::consts::PI).abs() < 1.0e-12);
        set_edited_argument("Unknown", &mut args, 1.0);
        assert_eq!(args.len(), 1, "an unknown label writes nothing");
    }

    #[test]
    fn a_selection_without_a_name_or_cells_still_shows_rows() {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 12);
        shape.name_u = "Connector".to_owned();
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let data = shape_data_rows(&mut app).expect("rows");
        assert_eq!(data[0].value, "Connector");
        let size = size_position_rows(&mut app).expect("rows");
        assert_eq!(size[0].value, MISSING_VALUE);
        for label in ["Pin X", "Pin Y", "Angle", "Flip X", "Flip Y"] {
            assert_eq!(row(&size, label).value, MISSING_VALUE, "{label}");
        }
        // LocPinX/LocPinY have a documented default (`Width*0.5`), so
        // they quietly evaluate to 0 (ADR 0021's quiet defaults).
        assert_eq!(row(&size, "Loc Pin X").value, "0 in");
        assert_eq!(row(&size, "Loc Pin Y").value, "0 in");

        // The unnamed fallback names the Visio id.
        let mut app2 = app;
        let session = app2.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            shape.name_u.clear();
        }
        let data = shape_data_rows(&mut app2).expect("rows");
        assert_eq!(data[0].value, "Shape 12");
    }

    #[test]
    fn a_read_only_row_has_no_edit() {
        let row = PaneRow::read_only("Width", "8.5 in");
        assert_eq!(row.label, "Width");
        assert_eq!(row.value, "8.5 in");
        assert!(row.edit.is_none());
        assert_eq!(row.numeric, None);
    }

    #[test]
    fn shape_data_rows_list_properties_and_the_page_name() {
        let mut app = app_with(document(), DisplayUnits::Inches);
        let rows = shape_data_rows(&mut app).expect("rows");
        assert_eq!(
            rows,
            vec![
                PaneRow::read_only("title", "Diagram 1"),
                PaneRow::read_only("Name", "Page-1"),
            ]
        );
        assert!(rows.iter().all(|row| row.edit.is_none()));
    }

    #[test]
    fn a_selection_makes_the_anchor_the_panes_subject() {
        let (mut app, _id) = selected_document();
        let data = shape_data_rows(&mut app).expect("rows");
        assert_eq!(data[0].value, "Box");
        assert_eq!(data[1].value, "7");
        let size = size_position_rows(&mut app).expect("rows");
        assert_eq!(size[0].label, "Width");
        assert_eq!(size[0].numeric, Some(2.0));
        assert_eq!(
            size[0].edit.as_ref().map(|(id, _)| id.as_str()),
            Some("shape.set.width")
        );
        assert_eq!(size[1].numeric, Some(1.0));
        assert_eq!(size[2].label, "Pin X");
        assert_eq!(size[2].numeric, Some(3.0));
        assert_eq!(size[3].label, "Pin Y");
        assert_eq!(size[3].numeric, Some(4.0));
        assert_eq!(size[4].label, "Angle");
        assert!((size[4].numeric.unwrap_or_default() - 30.0).abs() < 1.0e-9);
    }

    #[test]
    fn the_pin_rows_carry_both_coordinates() {
        let (mut app, _id) = selected_document();
        let size = size_position_rows(&mut app).expect("rows");
        for label in ["Pin X", "Pin Y"] {
            let (_, args) = row(&size, label).edit.as_ref().expect("editable");
            assert_eq!(args.number("x"), Some(3.0), "{label}");
            assert_eq!(args.number("y"), Some(4.0), "{label}");
        }
    }

    #[test]
    fn size_position_rows_show_the_page_size_and_name() {
        let mut app = app_with(document(), DisplayUnits::Inches);
        let rows = size_position_rows(&mut app).expect("rows");
        assert_eq!(
            rows,
            vec![
                PaneRow::read_only("Width", "8.5 in"),
                PaneRow::read_only("Height", "11 in"),
                PaneRow::read_only("Name", "Page-1"),
            ]
        );
        assert!(rows.iter().all(|row| row.edit.is_none()));
    }

    #[test]
    fn page_size_converts_at_the_units_edge() {
        let mut mm = app_with(document(), DisplayUnits::Millimeters);
        let rows = size_position_rows(&mut mm).expect("rows");
        assert_eq!(rows[0].value, "215.9 mm");
        assert_eq!(rows[1].value, "279.4 mm");

        let mut cm = app_with(document(), DisplayUnits::Centimeters);
        let rows = size_position_rows(&mut cm).expect("rows");
        assert_eq!(rows[0].value, "21.59 cm");
    }

    #[test]
    fn missing_values_render_a_dash_and_record_one_warning() {
        let mut doc = Document::new();
        doc.pages.push(Page::new(PageId { index: 0 }, 0));
        let mut app = app_with(doc, DisplayUnits::Inches);
        let rows = size_position_rows(&mut app).expect("rows");
        assert_eq!(rows[0], PaneRow::read_only("Width", MISSING_VALUE));
        assert_eq!(rows[1], PaneRow::read_only("Height", MISSING_VALUE));
        assert_eq!(
            rows.last().cloned(),
            Some(PaneRow::read_only("Name", MISSING_VALUE))
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(app.warnings.len(), 3);
        assert_eq!(app.warnings[0], "Width: no value");

        // A second frame does not duplicate the same warning.
        let _ = size_position_rows(&mut app).expect("rows");
        assert_eq!(app.warnings.len(), 3);

        let rows = shape_data_rows(&mut app).expect("rows");
        assert_eq!(rows, vec![PaneRow::read_only("Name", MISSING_VALUE)]);
        assert_eq!(
            app.warnings.last().map(String::as_str),
            Some("Name: no value")
        );
    }

    #[test]
    fn an_empty_property_value_renders_a_dash() {
        let mut doc = Document::new();
        doc.properties.push(Property::new("title", ""));
        let mut app = app_with(doc, DisplayUnits::Inches);
        let rows = shape_data_rows(&mut app).expect("rows");
        assert_eq!(
            rows,
            vec![
                PaneRow::read_only("title", MISSING_VALUE),
                PaneRow::read_only("Name", MISSING_VALUE),
            ]
        );
        assert!(
            app.warnings
                .iter()
                .any(|warning| warning == "title: no value")
        );
    }

    #[test]
    fn a_non_finite_value_renders_a_dash() {
        let mut doc = Document::new();
        let mut page = page("Page-1");
        page.sheet.cells = vec![
            cell("PageWidth", f64::NAN),
            cell("PageHeight", f64::INFINITY),
        ];
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        let rows = size_position_rows(&mut app).expect("rows");
        for label in ["Width", "Height"] {
            assert_eq!(row(&rows, label).value, MISSING_VALUE, "{label}");
        }
        assert!(
            app.warnings
                .iter()
                .any(|warning| warning.starts_with("Width:")),
            "{:?}",
            app.warnings
        );
    }

    #[test]
    fn no_document_gives_no_rows_and_says_so() {
        let mut app = AppCtx::new(Settings::default());
        assert!(shape_data_rows(&mut app).expect("rows").is_empty());
        assert!(size_position_rows(&mut app).expect("rows").is_empty());
        with_ui(|ui| {
            show_shape_data(ui, &mut app).expect("shape data");
            show_size_position(ui, &mut app).expect("size and position");
        });
    }

    #[test]
    fn both_panes_render_in_an_egui_context() {
        let (mut app, _id) = selected_document();
        with_ui(|ui| {
            show_shape_data(ui, &mut app).expect("shape data");
            show_size_position(ui, &mut app).expect("size and position");
        });
    }

    #[test]
    fn a_stale_active_page_id_falls_back_to_missing_values() {
        let mut app = app_with(document(), DisplayUnits::Inches);
        let session = app.active_document_mut().expect("an open document");
        session.active_page = Some(PageId { index: 9 });
        let rows = size_position_rows(&mut app).expect("rows");
        for row in rows.iter().filter(|row| row.label != "Name") {
            assert_eq!(row.value, MISSING_VALUE, "{}", row.label);
        }
    }

    #[test]
    fn a_constant_literal_is_not_a_refused_formula() {
        // P6-07's classification: a constant literal computes nothing,
        // so the pane keeps the field editable.
        for formula in ["2 in", "-2", "2+3", "0.5235987755982988"] {
            assert!(!formula_computes(formula), "{formula}");
        }
        for formula in ["Inh", "Width*2", "GUARD(2 in)", "not a formula ((("] {
            assert!(formula_computes(formula), "{formula}");
        }
    }

    #[test]
    fn a_constant_literal_cell_stays_editable() {
        let (mut app, id) = selected_document();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            for cell in &mut shape.sheet.cells {
                if cell.name.as_str() == "Width" {
                    cell.set_formula(Some(String::from("2 in")));
                }
            }
        }
        let entries = size_position_entries(&mut app).expect("entries");
        let width = entry_of(&entries, "Width");
        assert!(!width.formula);
        assert!(width.row.edit.is_some(), "a literal cell takes the edit");
    }

    #[test]
    fn a_formula_cell_is_marked_read_only() {
        let (mut app, id) = selected_document();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            for cell in &mut shape.sheet.cells {
                if cell.name.as_str() == "Width" {
                    cell.set_formula(Some(String::from("Guard(2 in)")));
                }
            }
        }
        let entries = size_position_entries(&mut app).expect("entries");
        let width = entries
            .iter()
            .find(|entry| entry.row.label == "Width")
            .expect("the width row");
        assert!(width.formula);
        assert!(width.row.edit.is_none(), "a formula row is read-only");
        assert_eq!(width.row.numeric, Some(2.0));
        assert!(matches!(width.field, Field::Typed(EntryUnit::Length)));
        let height = entries
            .iter()
            .find(|entry| entry.row.label == "Height")
            .expect("the height row");
        assert!(!height.formula);
        assert!(height.row.edit.is_some());
    }

    #[test]
    fn an_inherited_formula_marks_the_cell_read_only() {
        let mut doc = Document::new();
        let mut master = Shape::new(ShapeId { index: 0 }, 1);
        master.sheet.cells = vec![formula_cell("Width", "Height*2", 2.0)];
        let master_id = doc.add_shape(master);
        let mut master_row =
            larrygram_model::Master::new(larrygram_model::MasterId { index: 0 }, 1);
        master_row.shapes.push(master_id);
        let master_row = doc.add_master(master_row).expect("the master adds");
        let mut shape = Shape::new(ShapeId { index: 1 }, 7);
        shape.master = Some(master_row);
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let entries = size_position_entries(&mut app).expect("entries");
        let width = entries
            .iter()
            .find(|entry| entry.row.label == "Width")
            .expect("the width row");
        assert!(width.formula, "the master's formula marks the cell");
    }

    #[test]
    fn a_documented_default_is_not_a_formula() {
        // LocPinX has no entry at all; its documented default is a
        // formula, but a write replaces no formula (ADR 0058).
        let (mut app, _id) = selected_document();
        let entries = size_position_entries(&mut app).expect("entries");
        let loc = entries
            .iter()
            .find(|entry| entry.row.label == "Loc Pin X")
            .expect("the loc pin row");
        assert!(!loc.formula);
    }

    #[test]
    fn flip_rows_show_the_evaluated_flag() {
        let (mut app, id) = selected_document();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            for cell in &mut shape.sheet.cells {
                match cell.name.as_str() {
                    "FlipX" => cell.value = Some(CellValue::Bool(true)),
                    "FlipY" => cell.value = Some(CellValue::Number(0.0)),
                    _ => {}
                }
            }
        }
        let entries = size_position_entries(&mut app).expect("entries");
        let flip = entries
            .iter()
            .find(|entry| entry.row.label == "Flip X")
            .expect("the flip row");
        assert_eq!(flip.row.value, "TRUE");
        assert_eq!(flip.field, Field::Shown(Some(true)));
        let flip_y = entries
            .iter()
            .find(|entry| entry.row.label == "Flip Y")
            .expect("the flip row");
        assert_eq!(flip_y.row.value, "FALSE", "a cached 0 reads as FALSE");
    }

    #[test]
    fn typed_values_show_the_cached_kind() {
        // A number in the display units of Settings::units ...
        let number = typed_value(
            &CellValue::Number(2.0),
            GeomUnit::Inches,
            DisplayUnits::Centimeters,
        )
        .expect("a number displays");
        assert_eq!(number, ("5.08 cm".to_owned(), None));
        // ... a boolean as a check box ...
        let flag = typed_value(
            &CellValue::Bool(true),
            GeomUnit::Unitsless,
            DisplayUnits::Inches,
        )
        .expect("a boolean displays");
        assert_eq!(flag, ("TRUE".to_owned(), Some(true)));
        // ... a colour as its RGB value ...
        let color = typed_value(
            &CellValue::Color(Color::new(255, 128, 0)),
            GeomUnit::Unitsless,
            DisplayUnits::Inches,
        )
        .expect("a colour displays");
        assert_eq!(color, ("#FF8000".to_owned(), None));
        // ... and a string as written.
        let text = typed_value(
            &CellValue::Str("hello".to_owned()),
            GeomUnit::Unitsless,
            DisplayUnits::Inches,
        )
        .expect("a string displays");
        assert_eq!(text, ("hello".to_owned(), None));
    }

    #[test]
    fn the_cached_kind_wins_over_the_type_cell() {
        // A `Prop` row whose Type cell says "number" (2) but whose Value
        // caches a string shows the string (ADR 0058: "the cached value
        // wins").
        let mut cost = prop_row("Row_1", Some("Cost"), None);
        let mut value = value_cell(CellValue::Str("sheep".to_owned()), GeomUnit::Unitsless);
        value.name = CellName::new("Value");
        cost.cells.push(value);
        let mut kind = Cell::new(CellName::new("Type"));
        kind.value = Some(CellValue::Number(2.0));
        cost.cells.push(kind);
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        shape.name = "Box".to_owned();
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        section.rows.push(cost);
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let rows = shape_data_rows(&mut app).expect("rows");
        assert_eq!(row(&rows, "Cost").value, "sheep");
    }

    #[test]
    fn prop_rows_show_the_label_the_row_name_and_the_typed_value() {
        let mut named = prop_row(
            "Cost",
            Some("Unit cost"),
            Some(value_cell(CellValue::Number(2.0), GeomUnit::Inches)),
        );
        named.cells.last_mut().expect("a value cell").formula = Some(String::from("Inh"));
        let unnamed = prop_row(
            "Row_2",
            None,
            Some(value_cell(CellValue::Bool(false), GeomUnit::Unitsless)),
        );
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        shape.name = "Box".to_owned();
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        section.rows.push(named);
        section.rows.push(unnamed);
        section.rows.push(prop_row("Row_3", Some("Empty"), None));
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let entries = shape_data_entries(&mut app).expect("entries");

        let cost = entries
            .iter()
            .find(|entry| entry.row.label == "Unit cost")
            .expect("the labelled row");
        assert_eq!(cost.row.value, "2 in");
        assert_eq!(
            cost.note.as_deref(),
            Some("Cost"),
            "the row name is the note"
        );
        assert!(cost.formula, "a formula value is marked");
        assert!(cost.row.edit.is_none());

        let fallback = entries
            .iter()
            .find(|entry| entry.row.label == "Row_2")
            .expect("the unnamed row");
        assert_eq!(fallback.row.value, "FALSE");
        // Task P6-18: a boolean value is editable (a check box that
        // dispatches `data.set.property` with the row's key).
        assert_eq!(fallback.field, Field::Flag);
        assert_eq!(fallback.note, None);
        let (_, args) = fallback.row.edit.as_ref().expect("editable");
        assert_eq!(
            args.text("row").map(str::to_owned),
            Some(String::from("Row_2"))
        );

        let missing = entries
            .iter()
            .find(|entry| entry.row.label == "Empty")
            .expect("the empty row");
        assert_eq!(missing.row.value, MISSING_VALUE);
    }

    #[test]
    fn an_unsupported_cached_value_is_a_dash() {
        let mut code = prop_row("Row_1", Some("Code"), None);
        code.cells.push(value_cell(
            CellValue::Error(String::from("#NAME?")),
            GeomUnit::Unitsless,
        ));
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        section.rows.push(code);
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Millimeters);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let rows = shape_data_rows(&mut app).expect("rows");
        assert_eq!(row(&rows, "Code").value, MISSING_VALUE);
        assert!(
            app.warnings
                .iter()
                .any(|warning| warning == "Code: unsupported cached value"),
            "{:?}",
            app.warnings
        );
    }

    #[test]
    fn every_cached_kind_has_a_display() {
        let cases = [
            (
                CellValue::Number(core::f64::consts::FRAC_PI_2),
                GeomUnit::Radians,
                "90°",
            ),
            (CellValue::Number(0.37), GeomUnit::Percent, "37%"),
            (CellValue::Number(5.0), GeomUnit::Unitsless, "5"),
            // A typographic length displays in `Settings::units` too
            // (2 in of picas is 2 in).
            (CellValue::Number(2.0), GeomUnit::Points, "2 in"),
            (
                CellValue::Error(String::from("#VALUE!")),
                GeomUnit::Unitsless,
                "#VALUE!",
            ),
            (
                CellValue::List(String::from("1;TRUE;x;#N/A!;#FF0000")),
                GeomUnit::Unitsless,
                "1;TRUE;x;#N/A!;#FF0000",
            ),
        ];
        for (value, unit, expected) in cases {
            let shown = typed_value(&value, unit, DisplayUnits::Inches).expect("a display");
            assert_eq!(shown.0, expected, "{value:?}");
        }
    }

    #[test]
    fn a_nested_list_entry_shows_nothing_extra() {
        assert_eq!(list_entry_text(&Value::List(Vec::new())), "");
    }

    #[test]
    fn typed_entries_parse_like_the_grammar() {
        // A unit suffix converts through the engine's unit table ...
        let width = typed_entry("2.5 cm", EntryUnit::Length).expect("2.5 cm parses");
        assert!((width - 2.5 / 2.54).abs() < 1.0e-12);
        // ... a plain number is a number of the field's unit ...
        let mixed = typed_entry("5 1/2", EntryUnit::Length).expect("5 1/2 parses");
        assert_eq!(mixed, 5.5);
        // ... a ratio is its fraction of the field's unit ...
        let percent = typed_entry("37%", EntryUnit::Length).expect("37% parses");
        assert!((percent - 0.37).abs() < 1.0e-12);
        // ... and an angle literal lands in the angle field.
        let angle = typed_entry("45 deg", EntryUnit::Angle).expect("45 deg parses");
        assert!((angle - 45.0).abs() < 1.0e-12);
        let angle = typed_entry("-45°", EntryUnit::Angle).expect("-45° parses");
        assert!((angle + 45.0).abs() < 1.0e-12);
        // An angle into a length field converts over the canonical
        // units of its family.
        let crossed = typed_entry("45 deg", EntryUnit::Length).expect("it still parses");
        assert!((crossed - core::f64::consts::FRAC_PI_4).abs() < 1.0e-12);
    }

    #[test]
    fn a_bad_entry_is_refused_with_a_reason() {
        for entry in ["abc", "", "Width", "2 in +", "\"text\"", "1/0", "1e999"] {
            let error = typed_entry(entry, EntryUnit::Length).expect_err("a refused entry");
            assert!(!error.is_empty(), "{entry:?}");
        }
        // A function call is not a constant entry either.
        let error = typed_entry("ABS(-2)", EntryUnit::Length).expect_err("a call is refused");
        assert!(error.contains("not a number"), "{error}");
    }

    #[test]
    fn a_non_finite_conversion_is_refused() {
        // The literal parses and the unit converts to infinity: the
        // entry is refused as "not a finite number", never a panic.
        let error =
            typed_entry("1e308 ft", EntryUnit::Length).expect_err("a non-finite conversion");
        assert!(error.contains("not a finite number"), "{error}");
    }

    #[test]
    fn a_coerced_entry_is_refused() {
        // `"x"%` is a literal under unary `%`, so it parses; the
        // evaluation coerces the string and warns, which refuses it.
        let error = typed_entry("\"x\"%", EntryUnit::Length).expect_err("a coercion");
        assert!(error.contains("not a plain number"), "{error}");
    }

    #[test]
    fn the_literal_context_refuses_references_and_writes() {
        let mut context = LiteralOnly::default();
        let reference = CellRef::new("Width");
        assert!(
            context
                .cell_value(SheetRef::Shape(ShapeId { index: 0 }), &reference)
                .is_err()
        );
        assert!(
            context
                .set_cell(
                    SheetRef::Shape(ShapeId { index: 0 }),
                    &reference,
                    Value::Bool(false)
                )
                .is_err()
        );
        context.warn(EvalWarning {
            sheet: SheetRef::Shape(ShapeId { index: 0 }),
            kind: larrygram_shapesheet::EvalWarningKind::Coercion(String::from("x")),
        });
        assert_eq!(context.warnings, 1);
    }

    #[test]
    fn entry_units_convert_at_the_command_edge() {
        assert_eq!(EntryUnit::Length.unit(), Unit::Inches);
        assert_eq!(EntryUnit::Angle.unit(), Unit::Degrees);
        assert_eq!(EntryUnit::Length.field_value(2.5), 2.5);
        assert!(
            (EntryUnit::Angle.field_value(core::f64::consts::FRAC_PI_6) - 30.0).abs() < 1.0e-12
        );
    }

    #[test]
    fn a_formula_row_refuses_the_edit_with_one_status_line() {
        let (mut app, id) = selected_document();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            for cell in &mut shape.sheet.cells {
                if cell.name.as_str() == "Width" {
                    cell.set_formula(Some(String::from("GUARD(2 in)")));
                }
            }
        }
        let before = app.active_document().expect("session").document.clone();
        let entries = size_position_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Width"), "5 in");
        assert!(intents.is_empty(), "the refusal dispatches nothing");
        assert_eq!(app.warnings.len(), 1, "{:?}", app.warnings);
        assert!(
            app.warnings[0].contains("Width is a formula cell"),
            "{:?}",
            app.warnings
        );
        assert_eq!(
            app.active_document().expect("session").document,
            before,
            "the document is unchanged"
        );
    }

    #[test]
    fn a_committed_entry_dispatches_one_command() {
        let (mut app, _id) = selected_document();
        let before = app.active_document().expect("session").document.clone();
        let entries = size_position_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Width"), "2.5 cm");
        assert_eq!(intents.len(), 1, "{intents:?}");
        assert_eq!(intents[0].command.as_str(), "shape.set.width");
        let width = intents[0].invocation.args.number("width").expect("width");
        assert!((width - 2.5 / 2.54).abs() < 1.0e-12, "{width}");
        assert!(
            app.warnings.is_empty(),
            "an accepted edit records no warning: {:?}",
            app.warnings
        );
        assert_eq!(
            app.active_document().expect("session").document,
            before,
            "the pane itself never touches the model"
        );
    }

    #[test]
    fn a_bad_committed_entry_is_refused_with_one_status_line() {
        let (mut app, _id) = selected_document();
        let before = app.active_document().expect("session").document.clone();
        let entries = size_position_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Height"), "abc");
        assert!(intents.is_empty(), "{intents:?}");
        assert_eq!(app.warnings.len(), 1, "{:?}", app.warnings);
        assert!(app.warnings[0].contains("Height: "), "{:?}", app.warnings);
        assert_eq!(
            app.active_document().expect("session").document,
            before,
            "a refused entry changes nothing"
        );
    }

    #[test]
    fn a_read_only_row_commits_nothing() {
        let (mut app, _id) = selected_document();
        let entries = size_position_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Loc Pin X"), "5 in");
        assert!(intents.is_empty());
        assert!(app.warnings.is_empty(), "{:?}", app.warnings);
    }

    #[test]
    fn a_committed_angle_entry_takes_degrees() {
        let (mut app, _id) = selected_document();
        let entries = size_position_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Angle"), "45 deg");
        assert_eq!(intents.len(), 1, "{intents:?}");
        let angle = intents[0].invocation.args.number("angle").expect("angle");
        assert!(
            (angle - core::f64::consts::FRAC_PI_4).abs() < 1.0e-12,
            "{angle}"
        );
    }

    #[test]
    fn value_columns_render_a_check_box_for_a_boolean() {
        let (mut app, id) = selected_document();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            let mut flip = Cell::new(CellName::new("FlipX"));
            flip.value = Some(CellValue::Bool(true));
            shape.sheet.cells.push(flip);
        }
        let entries = size_position_entries(&mut app).expect("entries");
        with_ui(|ui| {
            for entry in &entries {
                show_value(ui, entry);
            }
        });
    }

    /// Task P6-18 (the coverage report's "formula read-only marker"):
    /// the `(formula)` marker and its hover hint render beside a
    /// formula cell's value in a real frame, through [`show_pane`].
    #[test]
    fn a_formula_cell_renders_the_formula_marker() {
        let (mut app, id) = selected_document();
        {
            let session = app.active_document_mut().expect("session");
            if let Some(shape) = session.document.shape_mut(id) {
                for cell in &mut shape.sheet.cells {
                    if cell.name.as_str() == "Width" {
                        cell.set_formula(Some(String::from("GUARD(2 in)")));
                    }
                }
            }
        }
        let entries = size_position_entries(&mut app).expect("entries");
        assert!(entry_of(&entries, "Width").formula);
        with_ui(|ui| {
            let intents = show_pane(ui, &mut app, "Size & Position", &entries);
            assert!(intents.is_empty(), "a read-only row dispatches nothing");
        });
    }

    #[test]
    fn non_finite_values_take_the_warning_path() {
        let mut app = AppCtx::new(Settings::default());
        let mut rows = Vec::new();
        let row = push_length(
            &mut app,
            "Width",
            Some(f64::NAN),
            DisplayUnits::Inches,
            false,
        );
        assert_eq!(row.row.value, MISSING_VALUE);
        rows.push(row);
        assert_eq!(rows[0].row.value, MISSING_VALUE);
    }

    #[test]
    fn push_text_shows_the_value_or_a_dash() {
        let mut app = AppCtx::new(Settings::default());
        let mut rows = Vec::new();
        push_text(&mut app, &mut rows, "Name", "Page-1");
        push_text(&mut app, &mut rows, "Other", "");
        assert_eq!(rows[0].row.value, "Page-1");
        assert_eq!(rows[1].row.value, MISSING_VALUE);
    }

    #[test]
    fn cell_has_formula_is_false_for_a_stale_shape_id() {
        // The lookup error (a stale id) is "no formula", never a panic.
        let session = DocumentSession::new(Document::new(), None);
        assert!(!cell_has_formula(
            &session,
            SheetRef::Shape(ShapeId { index: 9 }),
            "Width"
        ));
    }

    #[test]
    fn a_prop_row_label_falls_back_to_its_position() {
        let doc = Document::new();
        let session = DocumentSession::new(doc, None);
        let rows = prop_rows(&session, ShapeId { index: 9 });
        assert!(rows.is_empty(), "a stale shape id has no rows");

        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        section.rows.push(Row::new());
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let session = DocumentSession::new(doc, None);
        let rows = prop_rows(&session, id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Row 1");
        assert_eq!(rows[0].note, None);
    }

    #[test]
    fn deleted_prop_rows_are_skipped() {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        let mut row = prop_row("Row_1", Some("Gone"), None);
        row.preserved
            .attributes
            .push(larrygram_model::XmlAttribute {
                name: larrygram_model::QName {
                    prefix: None,
                    uri: None,
                    local: String::from("Del"),
                },
                value: String::from("1"),
            });
        section.rows.push(row);
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let session = DocumentSession::new(doc, None);
        assert!(prop_rows(&session, id).is_empty());
    }

    #[test]
    fn a_section_that_is_not_property_is_ignored() {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        let mut section = Section::new(larrygram_model::SectionName::new("Geometry"));
        section.rows.push(prop_row("Row_1", Some("X"), None));
        shape.sheet.sections.push(section);
        let id = doc.add_shape(shape);
        let session = DocumentSession::new(doc, None);
        assert!(prop_rows(&session, id).is_empty());
    }

    /// Adds one `Property` section with `rows` to the selected shape.
    fn add_props(app: &mut AppCtx, id: ShapeId, rows: Vec<Row>) {
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            let mut section = Section::new(larrygram_model::SectionName::new("Property"));
            section.rows = rows;
            shape.sheet.sections.push(section);
        }
    }

    /// Task P6-18: the value field of a Shape Data row dispatches
    /// `data.set.property` with the row's key and the typed text.
    #[test]
    fn a_value_entry_dispatches_data_set_property() {
        let (mut app, id) = selected_document();
        add_props(
            &mut app,
            id,
            vec![prop_row(
                "Cost",
                Some("Unit cost"),
                Some(value_cell(CellValue::Number(2.0), GeomUnit::Inches)),
            )],
        );
        let entries = shape_data_entries(&mut app).expect("entries");
        let cost = entry_of(&entries, "Unit cost");
        assert_eq!(cost.field, Field::Value(PropKind::Number));
        assert!(
            cost.row.numeric.is_none(),
            "a value row has no field number"
        );

        let intents = commit_entry(&mut app, cost, "2.5 cm");
        assert_eq!(intents.len(), 1, "{intents:?}");
        assert_eq!(intents[0].command.as_str(), "data.set.property");
        let args = &intents[0].invocation.args;
        assert_eq!(
            args.text("row").map(str::to_owned),
            Some(String::from("Cost"))
        );
        assert_eq!(
            args.text("value").map(str::to_owned),
            Some(String::from("2.5 cm"))
        );
    }

    /// A boolean value row is a check box whose click dispatches the
    /// toggled value (task P6-18).
    #[test]
    fn a_value_check_box_dispatches_the_toggled_value() {
        let (mut app, id) = selected_document();
        add_props(
            &mut app,
            id,
            vec![prop_row(
                "Rounded",
                None,
                Some(value_cell(CellValue::Bool(false), GeomUnit::Unitsless)),
            )],
        );
        let entries = shape_data_entries(&mut app).expect("entries");
        let rounded = entry_of(&entries, "Rounded");
        assert_eq!(rounded.field, Field::Flag);
        let (_, args) = rounded.row.edit.as_ref().expect("editable");
        assert_eq!(
            args.text("row").map(str::to_owned),
            Some(String::from("Rounded"))
        );

        // The value column draws a check box (a click dispatches; the
        // harness test in `tests/panes_typed.rs` drives the click).
        let seen = with_ui(|ui| show_value(ui, rounded));
        assert!(seen.is_empty(), "no click, no intent");
    }

    /// Task P6-18: a bad value entry is refused with one status line
    /// and dispatches nothing; a formula value refuses the edit the way
    /// the other panes do (ADR 0058).
    #[test]
    fn a_bad_or_formula_value_entry_is_refused_with_one_status_line() {
        let (mut app, id) = selected_document();
        let mut cost = prop_row(
            "Cost",
            Some("Unit cost"),
            Some(value_cell(CellValue::Number(2.0), GeomUnit::Inches)),
        );
        cost.cells
            .last_mut()
            .expect("a value cell")
            .set_formula(Some(String::from("Width*2")));
        let mut plain = prop_row(
            "Plain",
            Some("Plain"),
            Some(value_cell(CellValue::Number(2.0), GeomUnit::Inches)),
        );
        plain
            .cells
            .last_mut()
            .expect("a value cell")
            .set_formula(Some(String::from("2 in")));
        add_props(&mut app, id, vec![cost, plain]);
        let before = app.active_document().expect("session").document.clone();

        let entries = shape_data_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Unit cost"), "5 in");
        assert!(intents.is_empty(), "{intents:?}");
        assert_eq!(app.warnings.len(), 1, "{:?}", app.warnings);
        assert!(
            app.warnings[0].contains("is a formula cell"),
            "{:?}",
            app.warnings
        );

        let entries = shape_data_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Plain"), "abc");
        assert!(intents.is_empty(), "{intents:?}");
        assert_eq!(app.warnings.len(), 2, "{:?}", app.warnings);
        assert!(app.warnings[1].contains("Plain"), "{:?}", app.warnings);
        assert_eq!(
            app.active_document().expect("session").document,
            before,
            "a refused entry changes nothing"
        );

        // A constant literal is not a refused formula (P6-07's rule).
        let entries = shape_data_entries(&mut app).expect("entries");
        let intents = commit_entry(&mut app, entry_of(&entries, "Plain"), "5 in");
        assert_eq!(intents.len(), 1, "{intents:?}");
    }

    /// Task P6-18 (the P6-09 note): the pane lists the union of the
    /// shape's own `Property` rows and the master chain's, the own row
    /// winning a key clash, and an inherited row says where it is from.
    #[test]
    fn shape_data_rows_list_the_master_chain_rows_too() {
        let mut doc = Document::new();
        let mut rung = Shape::new(ShapeId { index: 0 }, 1);
        let mut section = Section::new(larrygram_model::SectionName::new("Property"));
        section.rows.push(prop_row(
            "Cost",
            Some("Master cost"),
            Some(value_cell(CellValue::Number(9.0), GeomUnit::Inches)),
        ));
        section.rows.push(prop_row(
            "Mastered",
            None,
            Some(value_cell(CellValue::Number(3.0), GeomUnit::Inches)),
        ));
        rung.sheet.sections.push(section);
        let rung = doc.add_shape(rung);
        let mut master_row =
            larrygram_model::Master::new(larrygram_model::MasterId { index: 0 }, 1);
        master_row.shapes.push(rung);
        let master_row = doc.add_master(master_row).expect("the master adds");

        let mut shape = Shape::new(ShapeId { index: 1 }, 7);
        shape.name = "Box".to_owned();
        shape.master = Some(master_row);
        let mut own = Section::new(larrygram_model::SectionName::new("Property"));
        own.rows.push(prop_row(
            "Cost",
            Some("Unit cost"),
            Some(value_cell(CellValue::Number(2.0), GeomUnit::Inches)),
        ));
        shape.sheet.sections.push(own);
        let id = doc.add_shape(shape);
        let mut page = page("Page-1");
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = app_with(doc, DisplayUnits::Inches);
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);

        let entries = shape_data_entries(&mut app).expect("entries");
        // The shape's own `Cost` wins the clash ...
        let cost = entry_of(&entries, "Unit cost");
        assert_eq!(cost.row.value, "2 in");
        assert_eq!(cost.note.as_deref(), Some("Cost"));
        // ... and the master's rows fill in behind it.
        let inherited = entry_of(&entries, "Mastered");
        assert_eq!(inherited.row.value, "3 in");
        assert_eq!(
            inherited.note.as_deref(),
            Some("Mastered (from the master)"),
            "an inherited row says where it is from"
        );
        assert!(
            inherited.row.edit.is_some(),
            "an inherited row is editable (the edit writes a local row)"
        );
        let master_cost = entries
            .iter()
            .filter(|entry| entry.note.as_deref() == Some("Cost (from the master)"))
            .count();
        assert_eq!(master_cost, 0, "the master's clashing row is not listed");
    }

    /// Task P6-18: the value entry is validated in the row's kind (the
    /// command parses the text again; this is the pane's feedback).
    #[test]
    fn value_entries_validate_in_the_rows_kind() {
        assert!(value_entry(PropKind::Number, "2.5 cm").is_ok());
        assert!(value_entry(PropKind::Number, "5 1/2").is_ok());
        assert!(value_entry(PropKind::Number, "abc").is_err());
        assert!(value_entry(PropKind::Number, "Width*2").is_err());
        assert!(value_entry(PropKind::Bool, "TRUE").is_ok());
        assert!(value_entry(PropKind::Bool, "false").is_ok());
        assert!(value_entry(PropKind::Bool, "1").is_ok());
        assert!(value_entry(PropKind::Bool, "maybe").is_err());
        assert!(value_entry(PropKind::Color, "#FF8000").is_ok());
        assert!(value_entry(PropKind::Color, "#abc").is_ok());
        assert!(value_entry(PropKind::Color, "red").is_err());
        assert!(value_entry(PropKind::Color, "#12345").is_err());
        assert!(value_entry(PropKind::Str, "").is_ok());
        assert!(value_entry(PropKind::Str, "anything at all").is_ok());
    }

    /// Task P6-18: `prop_kind` types what the cached value holds (the
    /// cached kind wins, ADR 0058) and a list or error has no editable
    /// kind.
    #[test]
    fn prop_kind_follows_the_cached_value() {
        assert_eq!(prop_kind(&CellValue::Number(1.0)), Some(PropKind::Number));
        assert_eq!(prop_kind(&CellValue::Bool(true)), Some(PropKind::Bool));
        assert_eq!(
            prop_kind(&CellValue::Color(Color::new(0, 0, 0))),
            Some(PropKind::Color)
        );
        assert_eq!(
            prop_kind(&CellValue::Str(String::new())),
            Some(PropKind::Str)
        );
        assert_eq!(prop_kind(&CellValue::List(String::from("a;b"))), None);
        assert_eq!(prop_kind(&CellValue::Error(String::from("#VALUE!"))), None);
    }
}
