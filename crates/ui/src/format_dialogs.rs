//! The Fill and Line forms (task P7-28): the field sets the Fill and
//! Line dialogs and the Format Shape pane share.
//!
//! A form is field state plus the widgets that draw it: [`FillForm`]
//! (fill colour, pattern, pattern colour and transparency) and
//! [`LineForm`] (line colour, weight, pattern, cap, transparency, the
//! two arrowheads and their sizes and the corner rounding). Both open
//! with the selection anchor's resolved values
//! ([`FillForm::from_app`], [`LineForm::from_app`], resolved through
//! the model like the pickers' `fill_preview`) and carry the fields as
//! the arguments of `format.fill` / `format.line` (task P7-28: a form
//! never writes the model itself, one command run does — ADR 0028).
//!
//! The dialogs ([`fill_dialog`], [`line_dialog`]) draw the fields, a
//! preview and OK, Cancel and Apply: OK applies every field and
//! closes, Apply applies every field and stays open, Cancel changes
//! nothing (also after an Apply: the applied step stands and only
//! later edits are dropped). Each apply is one command run, so one
//! undo step covers every selected shape.
//!
//! The Format Shape pane draws the same fields without the buttons
//! ([`fill_fields`], [`line_fields`]) and gets one entry per
//! **committed** change (a combo choice at once, a typed value on
//! Enter or when the field loses focus), so the pane applies as the
//! user changes a field — again one command, one undo step, per
//! change.
//!
//! The choice tables come from [`crate::pickers`]: the swatch palettes
//! (`THEME_COLORS`, `STANDARD_COLORS`), the weight ladder
//! (`WEIGHTS`), the pattern indices (`PATTERNS`) and the arrowhead
//! indices (`ARROWS`). The two lists Visio spells as enumerations —
//! the arrowhead sizes and the line caps — are the short labels of the
//! ShapeSheet reference ([`ARROW_SIZES`], [`LINE_CAPS`]). The dialog
//! text and layout are our own; nothing here is copied from a
//! Microsoft dialog (AGENTS.md section 4).

use egui::{Align, Color32, Layout, Pos2, Stroke, Ui, Vec2};
use larrygram_commands::app::AppCtx;
use larrygram_commands::{ArgValue, Args};
use larrygram_model::{CellName, CellReader, Color, SheetRef};

use crate::pickers::{ARROWS, PATTERNS, STANDARD_COLORS, THEME_COLORS, WEIGHTS, swatch_name};

/// The arrowhead sizes of the `*ArrowSize` cells: the class index and
/// its short label (the ShapeSheet reference's enumeration, 0 very
/// small to 6 colossal).
pub const ARROW_SIZES: &[(f64, &str)] = &[
    (0.0, "Very small"),
    (1.0, "Small"),
    (2.0, "Medium"),
    (3.0, "Large"),
    (4.0, "Extra large"),
    (5.0, "Jumbo"),
    (6.0, "Colossal"),
];

/// The line caps of the `LineCap` cell: the value and its short label
/// (the ShapeSheet reference's enumeration: rounded, square,
/// extended).
pub const LINE_CAPS: &[(f64, &str)] = &[(0.0, "Rounded"), (1.0, "Square"), (2.0, "Extended")];

/// One committed field change: the arguments of an accepted value, or
/// the reason the entry was refused (the pane records the reason on
/// the status line and writes nothing).
pub type Change = Result<Args, String>;

/// The Fill form's fields (task P7-28 goal 1).
#[derive(Debug, Clone, PartialEq)]
pub struct FillForm {
    /// `FillForegnd`, a `#RGB` or `#RRGGBB` spelling as typed.
    pub color: String,
    /// `FillPattern`, the index of the pattern rows (0 is no fill).
    pub pattern: f64,
    /// `FillBkgnd`, a `#RGB` or `#RRGGBB` spelling as typed.
    pub background: String,
    /// `FillForegndTrans`, the transparency in percent as typed.
    pub transparency: String,
    /// The message of the last rejected field (cleared by the next
    /// accepted one).
    pub error: Option<String>,
}

/// The Line form's fields (task P7-28 goal 2).
#[derive(Debug, Clone, PartialEq)]
pub struct LineForm {
    /// `LineColor`, a `#RGB` or `#RRGGBB` spelling as typed.
    pub color: String,
    /// `LineWeight`, the weight in points as typed.
    pub weight: String,
    /// `LinePattern`, the index of the pattern rows (0 is no line).
    pub pattern: f64,
    /// `LineCap`, a [`LINE_CAPS`] value.
    pub cap: f64,
    /// `LineColorTrans`, the transparency in percent as typed.
    pub transparency: String,
    /// `BeginArrow`, an [`ARROWS`] index.
    pub begin: f64,
    /// `EndArrow`, an [`ARROWS`] index.
    pub end: f64,
    /// `BeginArrowSize`, an [`ARROW_SIZES`] class.
    pub begin_size: f64,
    /// `EndArrowSize`, an [`ARROW_SIZES`] class.
    pub end_size: f64,
    /// `Rounding`, the corner radius in inches as typed.
    pub rounding: String,
    /// The message of the last rejected field (cleared by the next
    /// accepted one).
    pub error: Option<String>,
}

impl FillForm {
    /// The form over the selection anchor's resolved fill cells (goal
    /// 1: "the fields open with the selection anchor's resolved
    /// values"), including the defaults the model resolves. A lookup
    /// that answers nothing at all takes our own fallback (black for
    /// the fill, white for the pattern colour).
    pub fn from_app(app: &AppCtx) -> Self {
        Self {
            color: hex_or(app, "FillForegnd", (0x00, 0x00, 0x00)),
            pattern: read_number(app, "FillPattern").unwrap_or(1.0),
            background: hex_or(app, "FillBkgnd", (0xff, 0xff, 0xff)),
            transparency: percent_or(app, "FillForegndTrans"),
            error: None,
        }
    }

    /// Every field as `format.fill` arguments (one apply sets the
    /// whole fill at once, goal 1), or the reason a field did not
    /// parse.
    pub fn args(&self) -> Result<Args, String> {
        Ok(Args::new()
            .with(
                "color",
                ArgValue::Text(hex_arg("fill colour", &self.color)?),
            )
            .with("pattern", ArgValue::Number(self.pattern))
            .with(
                "background",
                ArgValue::Text(hex_arg("pattern colour", &self.background)?),
            )
            .with(
                "transparency",
                ArgValue::Number(percent_arg("transparency", &self.transparency)?),
            ))
    }

    /// The one-field arguments of a committed change (`name` is the
    /// `format.fill` argument the field writes).
    fn field_args(&self, name: &str) -> Result<Args, String> {
        let value = match name {
            "color" => ArgValue::Text(hex_arg("fill colour", &self.color)?),
            "pattern" => ArgValue::Number(self.pattern),
            "background" => ArgValue::Text(hex_arg("pattern colour", &self.background)?),
            "transparency" => ArgValue::Number(percent_arg("transparency", &self.transparency)?),
            _ => return Err(format!("unknown fill field {name:?}")),
        };
        Ok(Args::new().with(name, value))
    }
}

impl LineForm {
    /// The form over the selection anchor's resolved line cells (goal
    /// 2), including the defaults the model resolves. A lookup that
    /// answers nothing at all falls back to black and 0.75 pt.
    pub fn from_app(app: &AppCtx) -> Self {
        Self {
            color: hex_or(app, "LineColor", (0x00, 0x00, 0x00)),
            weight: trim_number(
                read_number(app, "LineWeight")
                    .map(points_of)
                    .unwrap_or(0.75),
            ),
            pattern: read_number(app, "LinePattern").unwrap_or(1.0),
            cap: read_number(app, "LineCap").unwrap_or(0.0),
            transparency: percent_or(app, "LineColorTrans"),
            begin: read_number(app, "BeginArrow").unwrap_or(0.0),
            end: read_number(app, "EndArrow").unwrap_or(0.0),
            begin_size: read_number(app, "BeginArrowSize").unwrap_or(2.0),
            end_size: read_number(app, "EndArrowSize").unwrap_or(2.0),
            rounding: trim_number(read_number(app, "Rounding").unwrap_or(0.0)),
            error: None,
        }
    }

    /// Every field as `format.line` arguments (one apply sets the
    /// whole line at once, goal 2), or the reason a field did not
    /// parse.
    pub fn args(&self) -> Result<Args, String> {
        let mut args = Args::new()
            .with(
                "color",
                ArgValue::Text(hex_arg("line colour", &self.color)?),
            )
            .with(
                "weight",
                ArgValue::Number(weight_arg("line weight", &self.weight)?),
            )
            .with("pattern", ArgValue::Number(self.pattern))
            .with("cap", ArgValue::Number(self.cap))
            .with(
                "transparency",
                ArgValue::Number(percent_arg("transparency", &self.transparency)?),
            )
            .with("begin", ArgValue::Number(self.begin))
            .with("end", ArgValue::Number(self.end))
            .with("begin_size", ArgValue::Number(self.begin_size))
            .with("end_size", ArgValue::Number(self.end_size));
        args.set(
            "rounding",
            ArgValue::Number(rounding_arg("corner rounding", &self.rounding)?),
        );
        Ok(args)
    }

    /// The one-field arguments of a committed change (`name` is the
    /// `format.line` argument the field writes).
    fn field_args(&self, name: &str) -> Result<Args, String> {
        let value = match name {
            "color" => ArgValue::Text(hex_arg("line colour", &self.color)?),
            "weight" => ArgValue::Number(weight_arg("line weight", &self.weight)?),
            "pattern" => ArgValue::Number(self.pattern),
            "cap" => ArgValue::Number(self.cap),
            "transparency" => ArgValue::Number(percent_arg("transparency", &self.transparency)?),
            "begin" => ArgValue::Number(self.begin),
            "end" => ArgValue::Number(self.end),
            "begin_size" => ArgValue::Number(self.begin_size),
            "end_size" => ArgValue::Number(self.end_size),
            "rounding" => ArgValue::Number(rounding_arg("corner rounding", &self.rounding)?),
            _ => return Err(format!("unknown line field {name:?}")),
        };
        Ok(Args::new().with(name, value))
    }
}

/// What a dialog body did this frame (the Page Setup pattern, task
/// P7-21).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Nothing yet (or a field that did not parse; the form keeps the
    /// message in its `error`).
    None,
    /// The user pressed OK: apply every field and close.
    Ok(Args),
    /// The user pressed Apply: apply every field and keep the dialog
    /// open.
    Apply(Args),
    /// The user pressed Cancel (or closed the window): change nothing.
    Cancel,
}

/// Draws the Fill dialog body: the fields, the preview swatch and OK,
/// Cancel and Apply. The host owns the [`egui::Window`] and its close
/// button.
pub fn fill_dialog(ui: &mut Ui, form: &mut FillForm) -> Outcome {
    // The bordered controls of the P7-18 design pass.
    crate::theme::raise_controls(ui);
    let _ = fill_fields(ui, form);
    ui.add_space(6.0);
    fill_preview(ui, form);
    let args = form.args();
    buttons(ui, &mut form.error, &args)
}

/// Draws the Line dialog body: the fields, the preview line and OK,
/// Cancel and Apply. The host owns the [`egui::Window`].
pub fn line_dialog(ui: &mut Ui, form: &mut LineForm) -> Outcome {
    crate::theme::raise_controls(ui);
    let _ = line_fields(ui, form);
    ui.add_space(6.0);
    line_preview(ui, form);
    let args = form.args();
    buttons(ui, &mut form.error, &args)
}

/// The OK, Apply and Cancel row (the Page Setup dialog's button row,
/// with Apply added). `args` is the arguments of the current fields,
/// already validated; a rejected field keeps the dialog open with its
/// message.
fn buttons(ui: &mut Ui, error: &mut Option<String>, args: &Result<Args, String>) -> Outcome {
    let mut outcome = Outcome::None;
    ui.add_space(12.0);
    if let Some(message) = error {
        ui.colored_label(crate::theme::color_at(ui.ctx(), "error"), message.as_str());
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The default button of a dialog is the accent one
            // (P7-18's `accent_controls`).
            crate::theme::accent_controls(ui);
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
            if ui.button("Apply").clicked() {
                match args {
                    Ok(values) => {
                        *error = None;
                        outcome = Outcome::Apply(values.clone());
                    }
                    Err(message) => *error = Some(message.clone()),
                }
            }
            if ui.button("OK").clicked() {
                match args {
                    Ok(values) => {
                        *error = None;
                        outcome = Outcome::Ok(values.clone());
                    }
                    Err(message) => *error = Some(message.clone()),
                }
            }
        });
    });
    outcome
}

// ---------------------------------------------------------------------------
// The fields (one implementation for the dialog and the pane)
// ---------------------------------------------------------------------------

/// Draws the Fill form's fields and returns one entry per committed
/// change (a combo choice at once, a typed value on Enter or when the
/// field loses focus). The dialog ignores the entries and applies
/// every field on OK; the pane dispatches each entry as its own
/// `format.fill` call.
pub fn fill_fields(ui: &mut Ui, form: &mut FillForm) -> Vec<Change> {
    let mut changes = Vec::new();
    if color_field(ui, "Fill colour", &mut form.color) {
        changes.push(commit_fill(form, "color"));
    }
    let patterns = pattern_rows();
    if combo_field(ui, "Fill pattern", &mut form.pattern, &patterns) {
        changes.push(commit_fill(form, "pattern"));
    }
    if color_field(ui, "Pattern colour", &mut form.background) {
        changes.push(commit_fill(form, "background"));
    }
    if text_field(ui, "Fill transparency", &mut form.transparency) {
        changes.push(commit_fill(form, "transparency"));
    }
    changes
}

/// Draws the Line form's fields and returns one entry per committed
/// change ([`fill_fields`] describes the rule).
pub fn line_fields(ui: &mut Ui, form: &mut LineForm) -> Vec<Change> {
    let mut changes = Vec::new();
    if color_field(ui, "Line colour", &mut form.color) {
        changes.push(commit_line(form, "color"));
    }
    // The weight is a typed number with the picker's ladder beside it
    // (our own take on the editable combo box).
    ui.horizontal(|ui| {
        ui.label("Line weight:");
        if text_edit(ui, "Line weight", &mut form.weight) {
            changes.push(commit_line(form, "weight"));
        }
        let weights: Vec<(f64, String)> = WEIGHTS
            .iter()
            .map(|weight| (*weight, format!("{weight} pt")))
            .collect();
        let mut weight = form.weight.trim().parse::<f64>().unwrap_or(0.75);
        if combo_in(ui, "Line weight list", &mut weight, &weights) {
            form.weight = trim_number(weight);
            changes.push(commit_line(form, "weight"));
        }
    });
    let patterns = pattern_rows();
    if combo_field(ui, "Line pattern", &mut form.pattern, &patterns) {
        changes.push(commit_line(form, "pattern"));
    }
    if cap_field(ui, &mut form.cap) {
        changes.push(commit_line(form, "cap"));
    }
    if text_field(ui, "Line transparency", &mut form.transparency) {
        changes.push(commit_line(form, "transparency"));
    }
    let arrows = rows_of(ARROWS);
    if combo_field(ui, "Begin arrowhead", &mut form.begin, &arrows) {
        changes.push(commit_line(form, "begin"));
    }
    if combo_field(ui, "End arrowhead", &mut form.end, &arrows) {
        changes.push(commit_line(form, "end"));
    }
    let sizes = rows_of(ARROW_SIZES);
    if combo_field(ui, "Begin arrow size", &mut form.begin_size, &sizes) {
        changes.push(commit_line(form, "begin_size"));
    }
    if combo_field(ui, "End arrow size", &mut form.end_size, &sizes) {
        changes.push(commit_line(form, "end_size"));
    }
    if text_field(ui, "Corner rounding", &mut form.rounding) {
        changes.push(commit_line(form, "rounding"));
    }
    changes
}

/// The pattern rows: "None" (0) over the picker's pattern labels
/// (task P7-28 item 6: the picker table, plus the one index the table
/// leaves out).
fn pattern_rows() -> Vec<(f64, String)> {
    let mut rows = vec![(0.0, "None".to_owned())];
    rows.extend(rows_of(PATTERNS));
    rows
}

/// The row list of one picker table.
fn rows_of(table: &[(f64, &str)]) -> Vec<(f64, String)> {
    table
        .iter()
        .map(|(index, label)| (*index, (*label).to_owned()))
        .collect()
}

/// The label of a value in the row list (the value itself when no row
/// holds it).
fn row_label(value: f64, rows: &[(f64, String)]) -> String {
    rows.iter()
        .find(|(index, _)| (*index - value).abs() < 1.0e-9)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| trim_number(value))
}

/// The one-field arguments of a committed change of [`FillForm`], and
/// the form's `error` on a refused entry.
fn commit_fill(form: &mut FillForm, name: &str) -> Change {
    match form.field_args(name) {
        Ok(args) => {
            form.error = None;
            Ok(args)
        }
        Err(reason) => {
            form.error = Some(reason.clone());
            Err(reason)
        }
    }
}

/// The one-field arguments of a committed change of [`LineForm`]
/// ([`commit_fill`]'s twin).
fn commit_line(form: &mut LineForm, name: &str) -> Change {
    match form.field_args(name) {
        Ok(args) => {
            form.error = None;
            Ok(args)
        }
        Err(reason) => {
            form.error = Some(reason.clone());
            Err(reason)
        }
    }
}

/// One colour field: the `#RRGGBB` text and its palette combo.
/// Returns `true` when one of the two committed a change (the text on
/// Enter or when the field loses focus, the palette at once).
fn color_field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    let mut chosen = false;
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        if text_edit(ui, label, value) {
            chosen = true;
        }
        let swatches: Vec<(String, String)> = THEME_COLORS
            .iter()
            .chain(STANDARD_COLORS.iter())
            .map(|swatch| (swatch_name(swatch), hex_of(swatch)))
            .collect();
        let current = swatches
            .iter()
            .find(|(_, hex)| hex.trim().eq_ignore_ascii_case(value.trim()))
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "Custom".to_owned());
        let combo = egui::ComboBox::from_id_salt(("format-dialog-combo", label))
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (name, hex) in &swatches {
                    if ui.selectable_label(false, name.as_str()).clicked() {
                        *value = hex.clone();
                        chosen = true;
                    }
                }
            });
        // A combo box with no label atom has no accessible name; egui
        // documents `on_hover_text` as the fix (ADR 0031 rule).
        combo.response.on_hover_text(format!("{label} palette"));
    });
    chosen
}

/// One labelled typed field ([`text_edit`] in its row).
fn text_field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    let mut committed = false;
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        committed = text_edit(ui, label, value);
    });
    committed
}

/// One typed field: the text commits on Enter or when the field loses
/// focus (the panes' rule, ADR 0058).
fn text_edit(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    let id = egui::Id::unique(("format-dialog-field", label));
    let mut editing = ui.data_mut(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let response = ui
        .add(egui::TextEdit::singleline(value).desired_width(80.0))
        .accessible_name(label);
    if response.changed() {
        editing = true;
    }
    let mut committed = false;
    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
    if editing && (response.lost_focus() || (enter && response.has_focus())) {
        editing = false;
        committed = true;
    }
    ui.data_mut(|data| data.insert_temp(id, editing));
    committed
}

/// One labelled choice field ([`combo_in`] in its row).
fn combo_field(ui: &mut Ui, label: &str, value: &mut f64, rows: &[(f64, String)]) -> bool {
    let mut chosen = false;
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        chosen = combo_in(ui, label, value, rows);
    });
    chosen
}

/// One choice field: a combo box over `rows` (a picker table). A
/// choice commits at once.
fn combo_in(ui: &mut Ui, label: &str, value: &mut f64, rows: &[(f64, String)]) -> bool {
    let mut chosen = false;
    let current = row_label(*value, rows);
    let combo = egui::ComboBox::from_id_salt(("format-dialog-combo", label))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (index, name) in rows {
                if ui.selectable_label(false, name.as_str()).clicked() {
                    *value = *index;
                    chosen = true;
                }
            }
        });
    // A combo box with no label atom has no accessible name; egui
    // documents `on_hover_text` as the fix (ADR 0031 rule).
    combo.response.on_hover_text(label);
    chosen
}

/// The line cap radios ([`LINE_CAPS`]); a click commits at once.
fn cap_field(ui: &mut Ui, value: &mut f64) -> bool {
    let mut chosen = false;
    ui.horizontal(|ui| {
        ui.label("Line cap:");
        for (index, name) in LINE_CAPS {
            if ui.radio_value(&mut *value, *index, *name).clicked() {
                chosen = true;
            }
        }
    });
    chosen
}

// ---------------------------------------------------------------------------
// The previews (goal 1: a preview swatch; goal 2: a preview line)
// ---------------------------------------------------------------------------

/// The preview swatch of the Fill form: the pattern colour under the
/// fill colour at its transparency, with diagonal stripes as the
/// patterned fills' approximation (the renderer's pattern table draws
/// the real tiles).
fn fill_preview(ui: &mut Ui, form: &FillForm) {
    let (_, rect) = ui.allocate_space(Vec2::new(160.0, 28.0));
    let painter = ui.painter_at(rect);
    let background = with_alpha(color_or(&form.background, (0xff, 0xff, 0xff)), 255);
    let foreground = color_or(&form.color, (0x00, 0x00, 0x00));
    let alpha = alpha_of(&form.transparency);
    painter.rect_filled(rect, 4.0, background);
    if form.pattern >= 0.5 {
        let fill = with_alpha(foreground, alpha);
        painter.rect_filled(rect, 4.0, fill);
        if form.pattern >= 1.5 {
            let mut x = rect.left() - rect.height();
            while x < rect.right() {
                painter.line_segment(
                    [
                        Pos2::new(x, rect.bottom()),
                        Pos2::new(x + rect.height(), rect.top()),
                    ],
                    Stroke::new(3.0, background),
                );
                x += 8.0;
            }
        }
    }
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_gray(0x80)),
        egui::StrokeKind::Inside,
    );
}

/// The preview line of the Line form: the stroke at its colour,
/// weight, pattern and caps, with the arrowheads at their sizes. The
/// dash rhythm and the head shapes are the picker's own samples (the
/// renderer's tables draw the real ones).
fn line_preview(ui: &mut Ui, form: &LineForm) {
    let (_, rect) = ui.allocate_space(Vec2::new(220.0, 40.0));
    let painter = ui.painter_at(rect);
    let color = color_or(&form.color, (0x00, 0x00, 0x00));
    let solid = with_alpha(color, 255);
    let stroke = Stroke::new(
        weight_arg("line weight", &form.weight)
            .unwrap_or(0.75)
            .max(0.5) as f32,
        with_alpha(color, alpha_of(&form.transparency)),
    );
    let from = Pos2::new(rect.left() + 20.0, rect.center().y);
    let to = Pos2::new(rect.right() - 20.0, rect.center().y);
    let dashes: Vec<f32> = match form.pattern.round() as i64 {
        2 => vec![8.0, 4.0],
        3 => vec![1.0, 3.0],
        4 => vec![8.0, 3.0, 1.0, 3.0],
        5 => vec![7.0, 3.0, 1.0, 3.0, 1.0, 3.0],
        6 => vec![1.0, 3.0, 7.0, 3.0],
        7 => vec![16.0, 6.0],
        8 => vec![16.0, 6.0, 3.0, 6.0],
        _ => Vec::new(),
    };
    if form.pattern >= 0.5 {
        paint_dashed(&painter, from, to, stroke, &dashes);
    }
    let size = 6.0 + 2.0 * form.begin_size.clamp(0.0, 6.0) as f32;
    arrow_head(&painter, from, size, form.begin.round() as i64, solid, true);
    let size = 6.0 + 2.0 * form.end_size.clamp(0.0, 6.0) as f32;
    arrow_head(&painter, to, size, form.end.round() as i64, solid, false);
}

/// A dashed line segment (the pickers' `paint_dashed` rule): with no
/// dash rhythm it is one solid segment.
fn paint_dashed(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke, dashes: &[f32]) {
    if dashes.is_empty() {
        painter.line_segment([from, to], stroke);
        return;
    }
    let total = from.distance(to);
    let rhythm: f32 = dashes.iter().sum();
    if rhythm <= 0.0 || total <= 0.0 {
        painter.line_segment([from, to], stroke);
        return;
    }
    let step = |at: f32| from + (to - from) * (at / total);
    let mut at = 0.0;
    let mut index = 0;
    while at < total {
        let length = dashes.get(index % dashes.len()).copied().unwrap_or(rhythm);
        let on = index % 2 == 0;
        let end = (at + length).min(total);
        if on {
            painter.line_segment([step(at), step(end)], stroke);
        }
        at = end;
        index = index.saturating_add(1);
    }
}

/// One arrowhead at a line end: a triangle for the arrow shapes, a
/// dot for the dot shape and nothing for 0 (the picker's samples).
fn arrow_head(
    painter: &egui::Painter,
    tip: Pos2,
    size: f32,
    index: i64,
    color: Color32,
    begin: bool,
) {
    let direction = if begin { 1.0 } else { -1.0 };
    match index {
        0 => {}
        5 => {
            painter.circle_filled(tip, size / 2.0, color);
        }
        _ => {
            let back = Pos2::new(tip.x + direction * size, tip.y);
            let path = egui::Shape::convex_polygon(
                vec![
                    back,
                    Pos2::new(back.x, tip.y - size / 2.0),
                    tip,
                    Pos2::new(back.x, tip.y + size / 2.0),
                ],
                color,
                Stroke::NONE,
            );
            painter.add(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Values in and out (one rule per field; the command parses again)
// ---------------------------------------------------------------------------

/// The number of one direct cell of the selection anchor, resolved
/// through the model (P6-05), or `None` when the shape resolves none.
fn read_number(app: &AppCtx, cell: &str) -> Option<f64> {
    let session = app.active_document()?;
    let shape = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied())?;
    let mut reader = CellReader::new(&session.document);
    reader
        .number(SheetRef::Shape(shape), &CellName::new(cell))
        .ok()
}

/// The colour of one direct cell of the selection anchor ([`read_number`]'s twin).
fn read_color(app: &AppCtx, cell: &str) -> Option<Color> {
    let session = app.active_document()?;
    let shape = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied())?;
    let mut reader = CellReader::new(&session.document);
    reader
        .color(SheetRef::Shape(shape), &CellName::new(cell))
        .ok()
}

/// The `#RRGGBB` text of one colour cell, or our fallback.
fn hex_or(app: &AppCtx, cell: &str, fallback: (u8, u8, u8)) -> String {
    hex_of_color(read_color(app, cell).unwrap_or(Color::new(fallback.0, fallback.1, fallback.2)))
}

/// The percent text of one `*Trans` cell (the cell is a fraction), or
/// `0` when the shape resolves none.
fn percent_or(app: &AppCtx, cell: &str) -> String {
    let percent = read_number(app, cell).unwrap_or(0.0) * 100.0;
    trim_number(percent)
}

/// The `#RRGGBB` spelling of a swatch.
fn hex_of(swatch: &crate::pickers::Swatch) -> String {
    let (r, g, b) = swatch.color;
    hex_of_color(Color::new(r, g, b))
}

/// The `#RRGGBB` spelling of a colour.
fn hex_of_color(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// The parsed colour of a field's text (the previews' fallback).
fn color_or(text: &str, fallback: (u8, u8, u8)) -> Color {
    let fallback = Color::new(fallback.0, fallback.1, fallback.2);
    let Some(hex) = text.trim().strip_prefix('#') else {
        return fallback;
    };
    let digits = hex.len();
    let valid = hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid || (digits != 3 && digits != 6) {
        return fallback;
    }
    let digit = |at: usize| -> u8 {
        hex.get(at..at.saturating_add(1))
            .and_then(|text| u8::from_str_radix(text, 16).ok())
            .unwrap_or(0)
    };
    let byte = |at: usize| -> u8 {
        hex.get(at..at.saturating_add(2))
            .and_then(|text| u8::from_str_radix(text, 16).ok())
            .unwrap_or(0)
    };
    match digits {
        3 => Color::new(digit(0) * 17, digit(1) * 17, digit(2) * 17),
        _ => Color::new(byte(0), byte(2), byte(4)),
    }
}

/// The alpha of one transparency field (percent text): 255 at 0% and
/// 0 at 100%, with a bad entry previewed as opaque.
fn alpha_of(text: &str) -> u8 {
    let percent = text.trim().parse::<f64>().unwrap_or(0.0).clamp(0.0, 100.0);
    ((100.0 - percent) * 255.0 / 100.0).round() as u8
}

/// The colour at `alpha`.
fn with_alpha(color: Color, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, alpha)
}

/// A `#RGB` or `#RRGGBB` field value, unchanged (the command parses
/// names and hex again), or the reason the entry is refused.
fn hex_arg(field: &str, text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    let Some(hex) = trimmed.strip_prefix('#') else {
        return Err(format!("{field} must be a #RGB or #RRGGBB value"));
    };
    let valid = hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid || (hex.len() != 3 && hex.len() != 6) {
        return Err(format!("{field} must be a #RGB or #RRGGBB value"));
    }
    Ok(trimmed.to_owned())
}

/// A percent field value (0 to 100).
fn percent_arg(field: &str, text: &str) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{field} must be a number between 0 and 100"))?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(format!("{field} must be a number between 0 and 100"));
    }
    Ok(value)
}

/// A weight field value (0 to 100 points).
fn weight_arg(field: &str, text: &str) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{field} must be a number between 0 and 100 points"))?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(format!("{field} must be a number between 0 and 100 points"));
    }
    Ok(value)
}

/// A rounding field value (a non-negative radius in inches).
fn rounding_arg(field: &str, text: &str) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{field} must be a number of inches"))?;
    if !value.is_finite() || !(0.0..=1000.0).contains(&value) {
        return Err(format!(
            "{field} must be a number between 0 and 1000 inches"
        ));
    }
    Ok(value)
}

/// The inches of one `LineWeight` cell to display points.
fn points_of(inches: f64) -> f64 {
    inches * 72.0
}

/// The shortest spelling of a number ("8.5", "11").
fn trim_number(value: f64) -> String {
    let rounded = (value * 10000.0).round() / 10000.0;
    format!("{rounded}")
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};
    use larrygram_commands::DocumentSession;
    use larrygram_commands::app::Settings;
    use larrygram_model::{Cell, CellValue, Document, Page, PageId, Shape, ShapeId};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// One cell of a shape sheet.
    fn cell(name: &'static str, value: CellValue) -> Cell {
        let mut cell = Cell::new(CellName::from_static(name));
        cell.value = Some(value);
        cell
    }

    /// An application with one shape carrying `cells`, selected.
    fn app_with(cells: &[(&'static str, CellValue)]) -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        for (name, value) in cells {
            shape.sheet.cells.push(cell(name, value.clone()));
        }
        let shape = doc.add_shape(shape);
        page.shapes.push(shape);
        doc.pages.push(page);
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(doc, None));
        app.selection.shapes = vec![shape];
        app.selection.anchor = Some(shape);
        (app, shape)
    }

    /// The colour value of one cell.
    fn color(r: u8, g: u8, b: u8) -> CellValue {
        CellValue::Color(Color::new(r, g, b))
    }

    #[test]
    fn the_fill_form_reads_the_anchor_cells() {
        let (app, _) = app_with(&[
            ("FillForegnd", color(0x1f, 0x5b, 0xc4)),
            ("FillPattern", CellValue::Number(2.0)),
            ("FillBkgnd", color(0xff, 0xff, 0xff)),
            ("FillForegndTrans", CellValue::Number(0.5)),
        ]);
        let form = FillForm::from_app(&app);
        assert_eq!(form.color, "#1F5BC4");
        assert_eq!(form.pattern, 2.0);
        assert_eq!(form.background, "#FFFFFF");
        assert_eq!(form.transparency, "50");
    }

    #[test]
    fn the_line_form_reads_the_anchor_cells() {
        let (app, _) = app_with(&[
            ("LineColor", color(0x1f, 0x5b, 0xc4)),
            ("LineWeight", CellValue::Number(1.0 / 72.0)),
            ("LinePattern", CellValue::Number(2.0)),
            ("LineCap", CellValue::Number(1.0)),
            ("LineColorTrans", CellValue::Number(0.25)),
            ("BeginArrow", CellValue::Number(3.0)),
            ("EndArrow", CellValue::Number(5.0)),
            ("BeginArrowSize", CellValue::Number(4.0)),
            ("EndArrowSize", CellValue::Number(1.0)),
            ("Rounding", CellValue::Number(0.25)),
        ]);
        let form = LineForm::from_app(&app);
        assert_eq!(form.color, "#1F5BC4");
        assert_eq!(form.weight, "1");
        assert_eq!(form.pattern, 2.0);
        assert_eq!(form.cap, 1.0);
        assert_eq!(form.transparency, "25");
        assert_eq!(form.begin, 3.0);
        assert_eq!(form.end, 5.0);
        assert_eq!(form.begin_size, 4.0);
        assert_eq!(form.end_size, 1.0);
        assert_eq!(form.rounding, "0.25");
    }

    /// A shape that defines none of the cells: the fields show the
    /// values the model resolves (the ShapeSheet table defaults).
    #[test]
    fn the_forms_open_with_the_resolved_values() {
        let (app, _) = app_with(&[]);
        let fill = FillForm::from_app(&app);
        assert_eq!(fill.color, "#000000");
        assert_eq!(fill.pattern, 0.0);
        assert_eq!(fill.background, "#000000");
        assert_eq!(fill.transparency, "0");
        let line = LineForm::from_app(&app);
        assert_eq!(line.color, "#000000");
        assert_eq!(line.weight, "0");
        assert_eq!(line.pattern, 0.0);
        assert_eq!(line.cap, 0.0);
        assert_eq!(line.begin, 0.0);
        assert_eq!(line.end, 0.0);
        assert_eq!(line.begin_size, 0.0);
        assert_eq!(line.end_size, 0.0);
        assert_eq!(line.rounding, "0");
    }

    #[test]
    fn the_fill_fields_are_the_format_fill_arguments() {
        let form = FillForm {
            color: "#1f5bc4".to_owned(),
            pattern: 2.0,
            background: "#FFFFFF".to_owned(),
            transparency: "50".to_owned(),
            error: None,
        };
        let args = form.args().expect("the fields parse");
        assert_eq!(args.text("color"), Some("#1f5bc4"));
        assert_eq!(args.number("pattern"), Some(2.0));
        assert_eq!(args.text("background"), Some("#FFFFFF"));
        assert_eq!(args.number("transparency"), Some(50.0));
    }

    #[test]
    fn the_line_fields_are_the_format_line_arguments() {
        let form = LineForm {
            color: "#000000".to_owned(),
            weight: "1.5".to_owned(),
            pattern: 3.0,
            cap: 1.0,
            transparency: "25".to_owned(),
            begin: 2.0,
            end: 4.0,
            begin_size: 3.0,
            end_size: 1.0,
            rounding: "0.125".to_owned(),
            error: None,
        };
        let args = form.args().expect("the fields parse");
        assert_eq!(args.number("weight"), Some(1.5));
        assert_eq!(args.number("pattern"), Some(3.0));
        assert_eq!(args.number("cap"), Some(1.0));
        assert_eq!(args.number("transparency"), Some(25.0));
        assert_eq!(args.number("begin"), Some(2.0));
        assert_eq!(args.number("end"), Some(4.0));
        assert_eq!(args.number("begin_size"), Some(3.0));
        assert_eq!(args.number("end_size"), Some(1.0));
        assert_eq!(args.number("rounding"), Some(0.125));
    }

    #[test]
    fn a_bad_field_is_an_error_naming_the_field_not_a_crash() {
        let fill = FillForm {
            color: "chartreuse-ish".to_owned(),
            pattern: 1.0,
            background: "#FFFFFF".to_owned(),
            transparency: "0".to_owned(),
            error: None,
        };
        let error = fill.args().expect_err("the colour does not parse");
        assert!(error.contains("fill colour"), "{error}");
        let line = LineForm {
            color: "#000000".to_owned(),
            weight: "wide".to_owned(),
            pattern: 1.0,
            cap: 0.0,
            transparency: "101".to_owned(),
            begin: 0.0,
            end: 0.0,
            begin_size: 2.0,
            end_size: 2.0,
            rounding: "-1".to_owned(),
            error: None,
        };
        let error = line.args().expect_err("the weight does not parse");
        assert!(error.contains("line weight"), "{error}");
    }

    /// The dialog state and outcome, shared with the harness closure.
    type Shared<T> = Rc<RefCell<T>>;

    /// One Fill dialog drawn over a shared form and outcome.
    fn fill_harness(form: Shared<FillForm>, outcome: Shared<Vec<Outcome>>) -> Harness<'static> {
        Harness::builder()
            .with_size(egui::vec2(360.0, 480.0))
            .build_ui(move |ui| {
                let mut form = form.borrow_mut();
                // Keep the last non-idle outcome: the closure re-runs
                // every frame and an idle frame would clear it.
                let produced = fill_dialog(ui, &mut form);
                if produced != Outcome::None {
                    outcome.borrow_mut().push(produced);
                }
            })
    }

    /// One Line dialog over a shared form and outcome.
    fn line_harness(form: Shared<LineForm>, outcome: Shared<Vec<Outcome>>) -> Harness<'static> {
        Harness::builder()
            .with_size(egui::vec2(360.0, 560.0))
            .build_ui(move |ui| {
                let mut form = form.borrow_mut();
                let produced = line_dialog(ui, &mut form);
                if produced != Outcome::None {
                    outcome.borrow_mut().push(produced);
                }
            })
    }

    /// One body of fields over a shared form and the committed changes.
    fn fields_harness(form: Shared<FillForm>, changes: Shared<Vec<Change>>) -> Harness<'static> {
        Harness::builder()
            .with_size(egui::vec2(360.0, 480.0))
            .build_ui(move |ui| {
                let mut form = form.borrow_mut();
                changes.borrow_mut().extend(fill_fields(ui, &mut form));
            })
    }

    /// The Line form's body over a shared form and changes.
    fn line_fields_harness(
        form: Shared<LineForm>,
        changes: Shared<Vec<Change>>,
    ) -> Harness<'static> {
        Harness::builder()
            .with_size(egui::vec2(360.0, 560.0))
            .build_ui(move |ui| {
                let mut form = form.borrow_mut();
                changes.borrow_mut().extend(line_fields(ui, &mut form));
            })
    }

    /// Clicks the widget named `name` and draws the next frame.
    fn click(harness: &mut Harness<'static>, name: &str) {
        {
            let root = harness.root();
            let node = root
                .query_all_by_label(name)
                .next()
                .unwrap_or_else(|| panic!("no widget named {name:?}"));
            node.click();
        }
        harness.run();
    }

    /// Types `text` into the widget named `name` and commits with
    /// Enter (the field's rule).
    fn type_into(harness: &mut Harness<'static>, name: &str, text: &str) {
        {
            let root = harness.root();
            let node = root
                .query_all_by_label(name)
                .next()
                .unwrap_or_else(|| panic!("no widget named {name:?}"));
            node.click();
            node.type_text(text);
        }
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
    }

    /// A fill form with empty typed fields (so a typed value is the
    /// whole field).
    fn empty_fill() -> FillForm {
        FillForm {
            color: "#000000".to_owned(),
            pattern: 1.0,
            background: "#FFFFFF".to_owned(),
            transparency: String::new(),
            error: None,
        }
    }

    #[test]
    fn ok_applies_apply_applies_and_cancel_cancels() {
        for (button, expected) in [("OK", "ok"), ("Apply", "apply"), ("Cancel", "cancel")] {
            let form = Rc::new(RefCell::new(empty_fill()));
            form.borrow_mut().transparency = "25".to_owned();
            let outcome: Shared<Vec<Outcome>> = Rc::new(RefCell::new(Vec::new()));
            let mut harness = fill_harness(Rc::clone(&form), Rc::clone(&outcome));
            harness.run();
            click(&mut harness, button);
            let seen = outcome.borrow().clone();
            match expected {
                "ok" => {
                    let args = match seen.last() {
                        Some(Outcome::Ok(args)) => args.clone(),
                        other => panic!("OK applies (got {other:?})"),
                    };
                    assert_eq!(args.number("transparency"), Some(25.0));
                }
                "apply" => assert!(
                    matches!(seen.last(), Some(Outcome::Apply(_))),
                    "Apply applies (got {seen:?})"
                ),
                _ => assert_eq!(seen.last(), Some(&Outcome::Cancel), "Cancel cancels"),
            }
        }
    }

    /// A palette row sets the colour at once (the rows past the
    /// combo's scroll window need a scroll first, so this picks one in
    /// view).
    #[test]
    fn a_palette_choice_changes_the_colour_and_commits_one_change() {
        let form = Rc::new(RefCell::new(empty_fill()));
        let changes: Shared<Vec<Change>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = fields_harness(Rc::clone(&form), Rc::clone(&changes));
        harness.run();
        click(&mut harness, "Fill colour palette");
        click(&mut harness, "Accent #1F5BC4");
        assert_eq!(form.borrow().color, "#1F5BC4", "the palette sets the text");
        let seen = changes.borrow().clone();
        assert_eq!(seen.len(), 1, "one committed change (got {seen:?})");
        let args = seen.first().expect("change").as_ref().expect("accepted");
        assert_eq!(args.text("color"), Some("#1F5BC4"));
    }

    #[test]
    fn a_typed_value_commits_on_enter_and_reports_its_arguments() {
        let form = Rc::new(RefCell::new(empty_fill()));
        let changes: Shared<Vec<Change>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = fields_harness(Rc::clone(&form), Rc::clone(&changes));
        harness.run();
        type_into(&mut harness, "Fill transparency", "50");
        assert_eq!(form.borrow().transparency, "50");
        let seen = changes.borrow().clone();
        assert_eq!(seen.len(), 1, "one committed change (got {seen:?})");
        let args = seen.first().expect("change").as_ref().expect("accepted");
        assert_eq!(args.number("transparency"), Some(50.0));
    }

    #[test]
    fn a_bad_typed_value_is_refused_with_the_reason_and_writes_nothing() {
        let form = Rc::new(RefCell::new(empty_fill()));
        let changes: Shared<Vec<Change>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = fields_harness(Rc::clone(&form), Rc::clone(&changes));
        harness.run();
        type_into(&mut harness, "Fill transparency", "wide");
        let seen = changes.borrow().clone();
        assert_eq!(seen.len(), 1, "one committed change (got {seen:?})");
        let reason = seen.first().expect("change").as_ref().expect_err("refused");
        assert!(reason.contains("transparency"), "{reason}");
        assert!(
            form.borrow().error.as_deref() == Some(reason.as_str()),
            "the form keeps the message for the dialog"
        );
    }

    #[test]
    fn a_pattern_choice_commits_at_once() {
        let form = Rc::new(RefCell::new(empty_fill()));
        let changes: Shared<Vec<Change>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = fields_harness(Rc::clone(&form), Rc::clone(&changes));
        harness.run();
        click(&mut harness, "Fill pattern");
        click(&mut harness, "Dashed");
        assert_eq!(form.borrow().pattern, 2.0);
        let seen = changes.borrow().clone();
        let args = seen.first().expect("change").as_ref().expect("accepted");
        assert_eq!(args.number("pattern"), Some(2.0));
    }

    #[test]
    fn the_line_form_covers_its_fields_from_clicks_and_typed_values() {
        let form = Rc::new(RefCell::new(LineForm {
            color: "#000000".to_owned(),
            weight: String::new(),
            pattern: 1.0,
            cap: 0.0,
            transparency: "0".to_owned(),
            begin: 0.0,
            end: 0.0,
            begin_size: 2.0,
            end_size: 2.0,
            rounding: String::new(),
            error: None,
        }));
        let changes: Shared<Vec<Change>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = line_fields_harness(Rc::clone(&form), Rc::clone(&changes));
        harness.run();
        // A click: the end arrowhead and its size.
        click(&mut harness, "End arrowhead");
        click(&mut harness, "Stealth");
        click(&mut harness, "End arrow size");
        click(&mut harness, "Jumbo");
        // A radio click: the cap.
        click(&mut harness, "Extended");
        // Typed values: the weight and the rounding.
        type_into(&mut harness, "Line weight", "1.5");
        type_into(&mut harness, "Corner rounding", "0.25");
        let form = form.borrow().clone();
        assert_eq!(form.end, 3.0);
        assert_eq!(form.end_size, 5.0);
        assert_eq!(form.cap, 2.0);
        assert_eq!(form.weight, "1.5");
        assert_eq!(form.rounding, "0.25");
        let seen = changes.borrow().clone();
        assert_eq!(seen.len(), 5, "one change per committed field ({seen:?})");
        for change in &seen {
            change.as_ref().expect("every change is accepted");
        }
    }

    #[test]
    fn the_line_dialog_applies_every_field_with_ok() {
        let form = Rc::new(RefCell::new(LineForm {
            color: "#1F5BC4".to_owned(),
            weight: "1.5".to_owned(),
            pattern: 2.0,
            cap: 1.0,
            transparency: "25".to_owned(),
            begin: 1.0,
            end: 3.0,
            begin_size: 2.0,
            end_size: 4.0,
            rounding: "0.125".to_owned(),
            error: None,
        }));
        let outcome: Shared<Vec<Outcome>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = line_harness(Rc::clone(&form), Rc::clone(&outcome));
        harness.run();
        click(&mut harness, "OK");
        let seen = outcome.borrow().clone();
        let args = match seen.last() {
            Some(Outcome::Ok(args)) => args.clone(),
            other => panic!("OK applies (got {other:?})"),
        };
        assert_eq!(args.text("color"), Some("#1F5BC4"));
        assert_eq!(args.number("weight"), Some(1.5));
        assert_eq!(args.number("rounding"), Some(0.125));
    }

    #[test]
    fn a_bad_field_keeps_the_dialog_open_with_its_message() {
        let form = Rc::new(RefCell::new(empty_fill()));
        form.borrow_mut().transparency = "wide".to_owned();
        let outcome: Shared<Vec<Outcome>> = Rc::new(RefCell::new(Vec::new()));
        let mut harness = fill_harness(Rc::clone(&form), Rc::clone(&outcome));
        harness.run();
        click(&mut harness, "OK");
        assert!(outcome.borrow().is_empty(), "nothing applies");
        assert!(
            form.borrow()
                .error
                .as_deref()
                .is_some_and(|message| message.contains("transparency")),
            "the dialog names the rejected field"
        );
    }
}
