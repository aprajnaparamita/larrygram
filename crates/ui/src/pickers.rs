//! The formatting pickers (task P7-21): the colour picker, the font
//! family list, the font size box, the line weight and pattern lists
//! and the arrowhead list.
//!
//! A picker is the **body** of a host control's popup: the command-bar
//! splits and combos of Panel Modern and the toolbar splits of Panel
//! Classic open it instead of the plain row list. The host owns the
//! popup (its anchor, its Escape handling, its close on choice);
//! [`show`] draws the rows and pushes the intent of a choice onto the
//! caller's list. Every choice is an [`Intent`] for a registry command
//! (ADR 0028): the pickers never execute anything themselves.
//!
//! The rows are egui buttons, so the pickers are keyboard-navigable
//! with the standard focus traversal (Tab and the arrow keys move,
//! Enter and Space choose) and Escape closes the popup the host drew.
//! The swatches come from [`THEME_COLORS`] and [`STANDARD_COLORS`],
//! our own palette (the theme hues of `docs/ui/design.md` and the
//! standard colour names); each swatch's tooltip and accessible name
//! is its name and its `#RRGGBB` value.

use egui::{Color32, RichText, Ui, Vec2};
use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Source};

use crate::Intent;

/// The picker that hosts one control's popup, named by the control's
/// own command (`ui-spec/layouts/*.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    /// `text.font`: the font family list.
    Font,
    /// `text.font.size`: the font size box and ladder.
    FontSize,
    /// `text.font.color`: the font colour swatches.
    FontColor,
    /// `format.set.fill.color`: the fill swatches, no fill and
    /// transparency.
    FillColor,
    /// `format.set.line.color`: the line swatches and no line.
    LineColor,
    /// `format.set.line.weight`: the line weight list.
    LineWeight,
    /// `format.set.line.pattern`: the line pattern list.
    LinePattern,
    /// `format.set.line.ends`: the arrowhead list.
    LineEnds,
}

/// The picker that hosts the popup of a control whose command is
/// `command`, if any.
pub fn picker_for(command: &str) -> Option<PickerKind> {
    match command {
        "text.font" => Some(PickerKind::Font),
        "text.font.size" => Some(PickerKind::FontSize),
        "text.font.color" => Some(PickerKind::FontColor),
        "format.set.fill.color" => Some(PickerKind::FillColor),
        "format.set.line.color" => Some(PickerKind::LineColor),
        "format.set.line.weight" => Some(PickerKind::LineWeight),
        "format.set.line.pattern" => Some(PickerKind::LinePattern),
        "format.set.line.ends" => Some(PickerKind::LineEnds),
        _ => None,
    }
}

/// One colour of the picker's palette: its name and its RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swatch {
    /// The colour name ("Accent", "Red").
    pub name: &'static str,
    /// The colour.
    pub color: (u8, u8, u8),
}

/// The theme hues of `docs/ui/design.md` (the "Theme Colors" row).
pub const THEME_COLORS: &[Swatch] = &[
    Swatch {
        name: "Surface",
        color: (0xf5, 0xf5, 0xf5),
    },
    Swatch {
        name: "Text",
        color: (0x1f, 0x1f, 0x1f),
    },
    Swatch {
        name: "Accent",
        color: (0x1f, 0x5b, 0xc4),
    },
    Swatch {
        name: "Accent Hover",
        color: (0x1a, 0x4e, 0xa8),
    },
    Swatch {
        name: "Accent Pressed",
        color: (0x16, 0x42, 0x90),
    },
    Swatch {
        name: "Green",
        color: (0x2f, 0x7d, 0x32),
    },
    Swatch {
        name: "Orange",
        color: (0xb8, 0x56, 0x00),
    },
    Swatch {
        name: "Red",
        color: (0xc4, 0x31, 0x4b),
    },
    Swatch {
        name: "Yellow",
        color: (0x94, 0x6c, 0x00),
    },
    Swatch {
        name: "Purple",
        color: (0x6b, 0x4f, 0xbb),
    },
];

/// The standard colours (the "Standard Colors" row).
pub const STANDARD_COLORS: &[Swatch] = &[
    Swatch {
        name: "White",
        color: (0xff, 0xff, 0xff),
    },
    Swatch {
        name: "Black",
        color: (0x00, 0x00, 0x00),
    },
    Swatch {
        name: "Red",
        color: (0xff, 0x00, 0x00),
    },
    Swatch {
        name: "Orange",
        color: (0xff, 0xa5, 0x00),
    },
    Swatch {
        name: "Yellow",
        color: (0xff, 0xff, 0x00),
    },
    Swatch {
        name: "Green",
        color: (0x00, 0x80, 0x00),
    },
    Swatch {
        name: "Blue",
        color: (0x00, 0x00, 0xff),
    },
    Swatch {
        name: "Purple",
        color: (0x80, 0x00, 0x80),
    },
    Swatch {
        name: "Gray",
        color: (0x80, 0x80, 0x80),
    },
    Swatch {
        name: "Brown",
        color: (0x8b, 0x45, 0x13),
    },
];

/// The font families the picker lists (the bundled faces first).
pub const FAMILIES: &[&str] = &[
    "Selawik",
    "Carlito",
    "Arial",
    "Times New Roman",
    "Courier New",
    "Verdana",
    "Georgia",
    "Trebuchet MS",
];

/// The font size ladder, in points (the picker's rows).
pub const SIZE_LADDER: &[f64] = &[
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0,
];

/// The line weights, in points.
pub const WEIGHTS: &[f64] = &[0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0];

/// The line patterns: `(index, label)`. Index 1 is solid; the rest
/// dash the stroke (the `render::styles` table).
pub const PATTERNS: &[(f64, &str)] = &[
    (1.0, "Solid"),
    (2.0, "Dashed"),
    (3.0, "Dotted"),
    (4.0, "Dash-dot"),
    (5.0, "Dash-dot-dot"),
    (6.0, "Dot-dash-dash"),
    (7.0, "Long dash"),
    (8.0, "Long-short dash"),
];

/// The arrowheads: `(index, label)`. Index 0 is no arrowhead (the
/// `render::styles` table).
pub const ARROWS: &[(f64, &str)] = &[
    (0.0, "None"),
    (1.0, "Arrow"),
    (2.0, "Open"),
    (3.0, "Stealth"),
    (4.0, "Diamond"),
    (5.0, "Dot"),
];

/// The fill transparency rows, in percent.
pub const TRANSPARENCY: &[f64] = &[0.0, 25.0, 50.0, 75.0, 100.0];

/// The first row's label of one picker (the host sweeps look for it
/// to know the picker drew).
pub fn first_row(kind: PickerKind) -> &'static str {
    match kind {
        PickerKind::Font => FAMILIES.first().copied().unwrap_or("Selawik"),
        PickerKind::FontSize => "8 pt",
        PickerKind::FontColor => "Use Default",
        PickerKind::FillColor => "No Fill",
        PickerKind::LineColor | PickerKind::LineWeight | PickerKind::LinePattern => "No Line",
        PickerKind::LineEnds => "No Line Ends",
    }
}

/// The name and `#RRGGBB` value of one swatch (its tooltip and
/// accessible name).
pub fn swatch_name(swatch: &Swatch) -> String {
    let (r, g, b) = swatch.color;
    format!("{} #{:02X}{:02X}{:02X}", swatch.name, r, g, b)
}

/// The `#RRGGBB` spelling the colour commands take.
pub fn swatch_value(swatch: &Swatch) -> String {
    let (r, g, b) = swatch.color;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

/// Draws one picker body and pushes the intent of every chosen row
/// onto `out`. Returns `true` when a choice was made (the host closes
/// its popup then).
pub fn show(ui: &mut Ui, kind: PickerKind, source: Source, out: &mut Vec<Intent>) -> bool {
    ui.set_min_width(180.0);
    let mut nav = Nav::new(ui, kind);
    let chosen = match kind {
        PickerKind::Font => font_list(ui, &mut nav, source, out),
        PickerKind::FontSize => font_size(ui, &mut nav, source, out),
        PickerKind::FontColor => color_picker(ui, &mut nav, Role::Font, source, out),
        PickerKind::FillColor => color_picker(ui, &mut nav, Role::Fill, source, out),
        PickerKind::LineColor => color_picker(ui, &mut nav, Role::Line, source, out),
        PickerKind::LineWeight => weight_list(ui, &mut nav, source, out),
        PickerKind::LinePattern => pattern_list(ui, &mut nav, source, out),
        PickerKind::LineEnds => arrow_list(ui, &mut nav, source, out),
    };
    nav.save(ui, kind);
    chosen
}

/// One picker's keyboard navigation (task P7-21: the arrow keys move
/// the highlight and Enter chooses it). The highlight is per picker
/// and lives in the ui data while the popup is open.
struct Nav {
    /// The highlighted row index.
    at: usize,
    /// The row being drawn this frame.
    index: usize,
    /// Whether Enter was pressed this frame.
    enter: bool,
}

impl Nav {
    /// Reads the highlight and this frame's arrow and Enter presses.
    fn new(ui: &Ui, kind: PickerKind) -> Self {
        let key = nav_key(kind);
        let (mut at, count): (usize, usize) =
            ui.data_mut(|data| data.get_temp(key)).unwrap_or((0, 0));
        if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            at = at.saturating_add(1);
        }
        if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            at = at.saturating_sub(1);
        }
        if count > 0 {
            at %= count;
        }
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        Self {
            at,
            index: 0,
            enter,
        }
    }

    /// Keeps the highlight for the next frame.
    fn save(self, ui: &Ui, kind: PickerKind) {
        ui.data_mut(|data| data.insert_temp(nav_key(kind), (self.at, self.index)));
    }

    /// Whether the row being drawn is the highlighted one.
    fn highlighted(&mut self) -> bool {
        let index = self.index;
        self.index = self.index.saturating_add(1);
        index == self.at
    }

    /// One chosen row: clicked, or highlighted while Enter was pressed.
    fn chosen(&self, highlighted: bool, response: &egui::Response) -> bool {
        response.clicked() || (highlighted && self.enter)
    }
}

/// The ui-data key of one picker's highlight.
fn nav_key(kind: PickerKind) -> egui::Id {
    egui::Id::unique(("pickers_nav", format!("{kind:?}")))
}

/// What a colour picker colours: the rows before the swatches and the
/// command the swatches dispatch differ per role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Font,
    Fill,
    Line,
}

impl Role {
    /// The command the swatches dispatch.
    fn command(self) -> &'static str {
        match self {
            Role::Font => "text.font.color",
            Role::Fill => "format.set.fill.color",
            Role::Line => "format.set.line.color",
        }
    }
}

/// The font family list: one row per family.
fn font_list(ui: &mut Ui, nav: &mut Nav, source: Source, out: &mut Vec<Intent>) -> bool {
    let mut chosen = false;
    for family in FAMILIES {
        let highlighted = nav.highlighted();
        let response = row(ui, RichText::new(*family), family, highlighted);
        if nav.chosen(highlighted, &response) {
            let args = Args::new()
                .with("font", ArgValue::Text((*family).to_owned()))
                .with("value", ArgValue::Text((*family).to_owned()));
            push(out, "text.font", args, source);
            chosen = true;
        }
    }
    chosen
}

/// The font size box: an editable size field (Enter applies) over the
/// size ladder.
fn font_size(ui: &mut Ui, nav: &mut Nav, source: Source, out: &mut Vec<Intent>) -> bool {
    let mut chosen = false;
    let mut buffer = String::new();
    let mut typed = None;
    ui.horizontal(|ui| {
        ui.label("Size:");
        let edit = ui
            .add(
                egui::TextEdit::singleline(&mut buffer)
                    .desired_width(56.0)
                    .hint_text("12"),
            )
            .accessible_name("Size");
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            typed = buffer.trim().parse::<f64>().ok();
        }
    });
    if let Some(points) = typed {
        push_size(out, points, source);
        chosen = true;
    }
    ui.separator();
    for size in SIZE_LADDER {
        let highlighted = nav.highlighted();
        let response = row(
            ui,
            RichText::new(format!("{size}")),
            &format!("{size} pt"),
            highlighted,
        );
        if nav.chosen(highlighted, &response) {
            push_size(out, *size, source);
            chosen = true;
        }
    }
    chosen
}

/// The size dispatch: `text.font.size` with the size in points.
fn push_size(out: &mut Vec<Intent>, points: f64, source: Source) {
    let args = Args::new()
        .with("size", ArgValue::Number(points))
        .with("value", ArgValue::Number(points));
    push(out, "text.font.size", args, source);
}

/// The colour picker: the role's rows, then the theme and standard
/// swatches. The fill role also lists the transparencies.
fn color_picker(
    ui: &mut Ui,
    nav: &mut Nav,
    role: Role,
    source: Source,
    out: &mut Vec<Intent>,
) -> bool {
    let mut chosen = false;
    let plain: &[(&str, &str)] = match role {
        Role::Font => &[("Use Default", "format.text_color.use_default")],
        Role::Fill => &[
            ("No Fill", "format.fill_color.no_fill"),
            ("Use Default", "format.fill_color.use_default"),
        ],
        Role::Line => &[
            ("No Line", "format.line_color.no_line"),
            ("Use Default", "format.line_color.use_default"),
        ],
    };
    for (label, command) in plain {
        let highlighted = nav.highlighted();
        let response = row(ui, RichText::new(*label), label, highlighted);
        if nav.chosen(highlighted, &response) {
            push(out, command, Args::new(), source);
            chosen = true;
        }
    }
    ui.separator();
    for (heading, palette) in [
        ("Theme Colors", THEME_COLORS),
        ("Standard Colors", STANDARD_COLORS),
    ] {
        ui.label(RichText::new(heading).weak());
        if swatches(ui, nav, palette, role.command(), source, out) {
            chosen = true;
        }
    }
    if role == Role::Fill {
        ui.separator();
        ui.label(RichText::new("Transparency").weak());
        for percent in TRANSPARENCY {
            let label = format!("{percent:.0}%");
            let highlighted = nav.highlighted();
            let response = row(ui, RichText::new(label.clone()), &label, highlighted);
            if nav.chosen(highlighted, &response) {
                let args = Args::new()
                    .with("transparency", ArgValue::Number(*percent))
                    .with("value", ArgValue::Number(*percent));
                push(out, "shape.transparency", args, source);
                chosen = true;
            }
        }
    }
    chosen
}

/// One row of swatches: five per line, each a small colour button.
fn swatches(
    ui: &mut Ui,
    nav: &mut Nav,
    palette: &[Swatch],
    command: &'static str,
    source: Source,
    out: &mut Vec<Intent>,
) -> bool {
    let mut chosen = false;
    for row_entries in palette.chunks(5) {
        let row_chosen = ui
            .horizontal(|ui| {
                let mut row_chosen = false;
                for swatch in row_entries {
                    let (r, g, b) = swatch.color;
                    let name = swatch_name(swatch);
                    let highlighted = nav.highlighted();
                    let widget = egui::Button::new("")
                        .fill(Color32::from_rgb(r, g, b))
                        .stroke(egui::Stroke::new(1.0, Color32::from_gray(0x8a)))
                        .min_size(Vec2::new(22.0, 18.0));
                    let response = ui.add(widget);
                    if highlighted {
                        let rect = response.rect.expand(2.0);
                        let fill = crate::theme::color_at(ui.ctx(), "menu.highlight.fill");
                        ui.painter().rect_filled(rect, 2.0, fill);
                    }
                    let response = response
                        .on_hover_text(name.as_str())
                        .accessible_name(name.as_str());
                    if nav.chosen(highlighted, &response) {
                        push(
                            out,
                            command,
                            Args::new()
                                .with("color", ArgValue::Text(swatch_value(swatch)))
                                .with("value", ArgValue::Text(swatch_value(swatch))),
                            source,
                        );
                        row_chosen = true;
                    }
                }
                row_chosen
            })
            .inner;
        chosen = chosen || row_chosen;
    }
    chosen
}

/// The line weight list: one row per weight with its sample stroke.
fn weight_list(ui: &mut Ui, nav: &mut Nav, source: Source, out: &mut Vec<Intent>) -> bool {
    let mut chosen = false;
    let highlighted = nav.highlighted();
    let response = row(ui, RichText::new("No Line"), "No Line", highlighted);
    if nav.chosen(highlighted, &response) {
        push(out, "format.line_color.no_line", Args::new(), source);
        chosen = true;
    }
    ui.separator();
    for weight in WEIGHTS {
        let label = format!("{weight} pt");
        let highlighted = nav.highlighted();
        let response = row(ui, RichText::new(label.clone()), &label, highlighted);
        let rect = response.rect;
        let stroke = egui::Stroke::new((*weight as f32).clamp(0.5, 6.0), Color32::BLACK);
        ui.painter().line_segment(
            [
                egui::Pos2::new(rect.right() - 40.0, rect.center().y),
                egui::Pos2::new(rect.right() - 8.0, rect.center().y),
            ],
            stroke,
        );
        if nav.chosen(highlighted, &response) {
            let args = Args::new()
                .with("weight", ArgValue::Number(*weight))
                .with("value", ArgValue::Number(*weight));
            push(out, "format.set.line.weight", args, source);
            chosen = true;
        }
    }
    chosen
}

/// The line pattern list: one row per pattern with its dashed sample.
fn pattern_list(ui: &mut Ui, nav: &mut Nav, source: Source, out: &mut Vec<Intent>) -> bool {
    let mut chosen = false;
    let highlighted = nav.highlighted();
    let response = row(ui, RichText::new("No Line"), "No Line", highlighted);
    if nav.chosen(highlighted, &response) {
        push(out, "format.line_pattern.no_line", Args::new(), source);
        chosen = true;
    }
    ui.separator();
    for (index, label) in PATTERNS {
        let highlighted = nav.highlighted();
        let response = row(ui, RichText::new(*label), label, highlighted);
        let rect = response.rect;
        let stroke = egui::Stroke::new(1.5, Color32::BLACK);
        let dash = match *index {
            2.0 => vec![8.0, 4.0],
            3.0 => vec![1.0, 3.0],
            4.0 => vec![8.0, 3.0, 1.0, 3.0],
            5.0 => vec![7.0, 3.0, 1.0, 3.0, 1.0, 3.0],
            _ => Vec::new(),
        };
        paint_dashed(
            ui,
            [
                egui::Pos2::new(rect.right() - 48.0, rect.center().y),
                egui::Pos2::new(rect.right() - 8.0, rect.center().y),
            ],
            stroke,
            &dash,
        );
        if nav.chosen(highlighted, &response) {
            let args = Args::new()
                .with("pattern", ArgValue::Number(*index))
                .with("value", ArgValue::Number(*index));
            push(out, "format.set.line.pattern", args, source);
            chosen = true;
        }
    }
    chosen
}

/// The arrowhead list: one row per arrowhead with its sample mark.
fn arrow_list(ui: &mut Ui, nav: &mut Nav, source: Source, out: &mut Vec<Intent>) -> bool {
    let mut chosen = false;
    let highlighted = nav.highlighted();
    let response = row(
        ui,
        RichText::new("No Line Ends"),
        "No Line Ends",
        highlighted,
    );
    if nav.chosen(highlighted, &response) {
        push(out, "format.line_ends.no_line_ends", Args::new(), source);
        chosen = true;
    }
    ui.separator();
    for (index, label) in ARROWS {
        let highlighted = nav.highlighted();
        let response = row(ui, RichText::new(*label), label, highlighted);
        let rect = response.rect;
        let tip = egui::Pos2::new(rect.right() - 8.0, rect.center().y);
        ui.painter().line_segment(
            [egui::Pos2::new(rect.right() - 48.0, rect.center().y), tip],
            egui::Stroke::new(1.5, Color32::BLACK),
        );
        let head = match *index {
            0.0 => None,
            3.0 => Some([
                (tip.x - 8.0, tip.y - 4.0),
                (tip.x, tip.y),
                (tip.x - 8.0, tip.y + 4.0),
            ]),
            5.0 => None,
            _ => Some([
                (tip.x - 8.0, tip.y - 4.0),
                (tip.x, tip.y),
                (tip.x - 8.0, tip.y + 4.0),
            ]),
        };
        if let Some(points) = head {
            let path = egui::Shape::convex_polygon(
                points
                    .iter()
                    .map(|(x, y)| egui::Pos2::new(*x, *y))
                    .collect(),
                Color32::BLACK,
                egui::Stroke::NONE,
            );
            ui.painter().add(path);
        } else if *index == 5.0 {
            ui.painter().circle_filled(tip, 3.0, Color32::BLACK);
        }
        if nav.chosen(highlighted, &response) {
            let args = Args::new()
                .with("begin", ArgValue::Number(*index))
                .with("end", ArgValue::Number(*index))
                .with("value", ArgValue::Number(*index));
            push(out, "format.set.line.ends", args, source);
            chosen = true;
        }
    }
    chosen
}

/// One picker row: a flat button the width of the picker, with the
/// keyboard highlight inset from the row edge.
fn row(ui: &mut Ui, text: RichText, name: &str, highlighted: bool) -> egui::Response {
    let widget = egui::Button::new(text.size(12.0)).frame(false);
    let response = ui.add_sized(Vec2::new(ui.available_width().max(120.0), 22.0), widget);
    if highlighted {
        let rect = response.rect.shrink2(Vec2::new(2.0, 0.0));
        let radius = crate::theme::metric_at(ui.ctx(), "radius.control", 0.0);
        let fill = crate::theme::color_at(ui.ctx(), "menu.highlight.fill");
        let border = crate::theme::color_at(ui.ctx(), "menu.highlight.border");
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(name).accessible_name(name)
}

/// Paints a dashed sample line (the pattern picker's previews).
fn paint_dashed(ui: &Ui, points: [egui::Pos2; 2], stroke: egui::Stroke, dash: &[f64]) {
    if dash.is_empty() {
        ui.painter().line_segment(points, stroke);
        return;
    }
    let start = points[0];
    let end = points[1];
    let total = (end - start).length();
    let mut at = 0.0;
    let mut index = 0;
    while at < total {
        let length = dash
            .get(index % dash.len())
            .copied()
            .map(|value| value as f32)
            .unwrap_or(total);
        let from = start + (end - start) * (at / total);
        let to = start + (end - start) * (((at + length) / total).min(1.0));
        if index % 2 == 0 {
            ui.painter().line_segment([from, to], stroke);
        }
        at += length;
        index += 1;
    }
}

/// One intent of a choice, when the id parses (the picker's ids are
/// registry constants; a parse failure drops the choice, never
/// panics).
fn push(out: &mut Vec<Intent>, id: &'static str, args: Args, source: Source) {
    if let Ok(command) = CommandId::parse(id) {
        out.push(Intent::new(command, Invocation::new(args, source)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// One picker drawn over a shared intent list.
    fn harness_of(kind: PickerKind) -> (Harness<'static>, Rc<RefCell<Vec<Intent>>>) {
        let out: Rc<RefCell<Vec<Intent>>> = Rc::new(RefCell::new(Vec::new()));
        let harness = Harness::builder()
            .with_size(egui::vec2(240.0, 600.0))
            .build_ui({
                let out = Rc::clone(&out);
                move |ui| {
                    show(ui, kind, Source::Test, &mut out.borrow_mut());
                }
            });
        (harness, out)
    }

    /// Draws one picker and returns the intents of one click on the
    /// node labelled `label`.
    fn click(kind: PickerKind, label: &str) -> Vec<Intent> {
        let (mut harness, out) = harness_of(kind);
        harness.run();
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(Role::Button, label)
                .next()
                .unwrap_or_else(|| panic!("no row labelled {label:?}"));
            node.click();
        }
        harness.run();
        out.borrow().clone()
    }

    #[test]
    fn the_font_list_dispatches_a_family() {
        let intents = click(PickerKind::Font, "Selawik");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "text.font");
        assert_eq!(
            intents[0].invocation.args.text("font"),
            Some("Selawik"),
            "the family name is the argument"
        );
    }

    #[test]
    fn the_size_ladder_dispatches_points() {
        let intents = click(PickerKind::FontSize, "12 pt");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "text.font.size");
        assert_eq!(intents[0].invocation.args.number("size"), Some(12.0));
    }

    #[test]
    fn the_fill_picker_dispatches_no_fill_and_a_swatch() {
        let intents = click(PickerKind::FillColor, "No Fill");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.fill_color.no_fill");

        let intents = click(PickerKind::FillColor, "Accent #1F5BC4");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.set.fill.color");
        assert_eq!(
            intents[0].invocation.args.text("color"),
            Some("#1F5BC4"),
            "the swatch dispatches its hex value"
        );
    }

    #[test]
    fn the_fill_picker_dispatches_a_transparency() {
        let intents = click(PickerKind::FillColor, "50%");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "shape.transparency");
        assert_eq!(
            intents[0].invocation.args.number("transparency"),
            Some(50.0)
        );
    }

    #[test]
    fn the_line_picker_dispatches_weight_pattern_and_ends() {
        let intents = click(PickerKind::LineWeight, "1.5 pt");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.set.line.weight");
        assert_eq!(intents[0].invocation.args.number("weight"), Some(1.5));

        let intents = click(PickerKind::LinePattern, "Dashed");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.set.line.pattern");
        assert_eq!(intents[0].invocation.args.number("pattern"), Some(2.0));

        let intents = click(PickerKind::LineEnds, "Stealth");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.set.line.ends");
        assert_eq!(intents[0].invocation.args.number("end"), Some(3.0));
    }

    #[test]
    fn the_line_color_picker_has_no_line_and_a_swatch() {
        let intents = click(PickerKind::LineColor, "No Line");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.line_color.no_line");
    }

    #[test]
    fn the_font_color_picker_has_use_default() {
        let intents = click(PickerKind::FontColor, "Use Default");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "format.text_color.use_default");
    }

    #[test]
    fn a_keyboard_choice_dispatches() {
        let (mut harness, out) = harness_of(PickerKind::Font);
        harness.run();
        // Enter chooses the highlighted first row; the arrow keys move
        // the highlight.
        harness.key_press(egui::Key::Enter);
        harness.run();
        {
            let out = out.borrow().clone();
            assert_eq!(out.len(), 1, "Enter chooses the highlighted row");
            assert_eq!(out[0].command.as_str(), "text.font");
            assert_eq!(out[0].invocation.args.text("font"), Some("Selawik"));
        }
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let out = out.borrow().clone();
        assert_eq!(out.len(), 2, "the arrow keys move the highlight");
        assert_eq!(out[1].invocation.args.text("font"), Some("Carlito"));
    }

    #[test]
    fn picker_for_maps_the_host_commands() {
        assert_eq!(picker_for("text.font"), Some(PickerKind::Font));
        assert_eq!(
            picker_for("format.set.line.ends"),
            Some(PickerKind::LineEnds)
        );
        assert_eq!(picker_for("edit.copy"), None);
    }

    #[test]
    fn swatch_names_carry_the_hex_value() {
        let name = swatch_name(&THEME_COLORS[2]);
        assert_eq!(name, "Accent #1F5BC4");
        assert_eq!(swatch_value(&THEME_COLORS[2]), "#1F5BC4");
    }
}
