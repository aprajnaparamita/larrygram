//! The custom colour dialog (task P7-54): the "More ... Colors..."
//! rows of the colour pickers and the Panel Classic toolbars open it
//! (`UiRequest::ShowColorDialog`). One dialog serves all three roles:
//! a hue/saturation/value picker, a `#RRGGBB` hex field, and OK and
//! Cancel.
//!
//! The picker is ours (AGENTS.md section 4: our own widgets): egui's
//! stock colour picker draws a clipboard glyph the bundled fonts
//! cannot draw, and a missing glyph panics (AGENTS.md section 6).
//! The body changes nothing itself: OK answers the chosen colour and
//! the shell re-dispatches the role's colour command with it —
//! [`Target::command`], the same command the swatches dispatch — as
//! one undo step. Cancel changes nothing.

use egui::{Align, Color32, Layout, Sense, Ui, Vec2};
use larrygram_commands::app::AppCtx;
use larrygram_model::{CellName, CellReader, RowTarget, SheetRef};

use crate::theme;

/// What the dialog's colour applies to: the role's colour command and
/// its window title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The selection's fill colour (`format.set.fill.color`).
    Fill,
    /// The selection's line colour (`format.set.line.color`).
    Line,
    /// The selection's text colour (`text.font.color`).
    Text,
}

impl Target {
    /// The role of a `target` argument (`"fill"`, `"line"`,
    /// `"text"`), case-insensitive; `None` for anything else.
    pub fn parse(target: &str) -> Option<Self> {
        match target.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "line" => Some(Self::Line),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    /// The command the dialog's OK dispatches (the same command the
    /// picker's swatches dispatch).
    pub fn command(self) -> &'static str {
        match self {
            Self::Fill => "format.set.fill.color",
            Self::Line => "format.set.line.color",
            Self::Text => "text.font.color",
        }
    }

    /// The dialog window title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Fill => "Fill Color",
            Self::Line => "Line Color",
            Self::Text => "Text Color",
        }
    }
}

/// The `#RRGGBB` spelling of one colour (the swatch spelling,
/// `pickers::swatch_value`).
fn hex_of(color: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", color.0, color.1, color.2)
}

/// The colour of a `#RGB` or `#RRGGBB` spelling (the colour
/// commands' hex rule, without the name table).
fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.trim().strip_prefix('#')?;
    let nibble =
        |at: usize| -> Option<u8> { u8::from_str_radix(hex.get(at..at.checked_add(1)?)?, 16).ok() };
    let byte =
        |at: usize| -> Option<u8> { u8::from_str_radix(hex.get(at..at.checked_add(2)?)?, 16).ok() };
    match hex.len() {
        3 => Some((nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17)),
        6 => Some((byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

/// The hue/saturation/value of one colour: `h` in `0..1` on the
/// standard wheel, `s` and `v` in `0..=1`.
fn hsv_of(color: (u8, u8, u8)) -> (f32, f32, f32) {
    let r = f32::from(color.0) / 255.0;
    let g = f32::from(color.1) / 255.0;
    let b = f32::from(color.2) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        (((g - b) / delta) / 6.0).rem_euclid(1.0)
    } else if max == g {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };
    let s = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (h, s, max)
}

/// The colour of one hue/saturation/value triple (the inverse of
/// [`hsv_of`]).
fn rgb_of(hsv: (f32, f32, f32)) -> (u8, u8, u8) {
    let (h, s, v) = (hsv.0.rem_euclid(1.0), hsv.1, hsv.2);
    let section = h * 6.0;
    let c = v * s;
    let x = c * (1.0 - (section % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (section.floor() as i32).rem_euclid(6) {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |value: f32| -> u8 {
        let scaled = (value + m) * 255.0;
        if scaled <= 0.0 {
            0
        } else if scaled >= 255.0 {
            255
        } else {
            scaled.round() as u8
        }
    };
    (byte(r), byte(g), byte(b))
}

/// The dialog state (the Page Setup pattern).
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    /// What the colour applies to.
    pub target: Target,
    /// The colour the picker edits.
    pub color: (u8, u8, u8),
    /// The hex field's text (`#RRGGBB`), as typed.
    pub hex: String,
    /// The message of the last rejected hex spelling (cleared by the
    /// next accepted one).
    pub error: Option<String>,
}

impl State {
    /// The dialog over the role's resolved current colour (black when
    /// there is none to read: no document, no selection, or a text
    /// run that inherits its colour).
    pub fn from_app(app: &AppCtx, target: Target) -> Self {
        let color = current_color(app, target);
        Self {
            target,
            color,
            hex: hex_of(color),
            error: None,
        }
    }

    /// Accepts the hex field's text: the colour becomes `color` and
    /// the message clears, or the message records why it did not.
    fn commit_hex(&mut self) {
        match parse_hex(&self.hex) {
            Some(color) => {
                self.color = color;
                self.error = None;
            }
            None => {
                self.error = Some("use #RGB or #RRGGBB".to_owned());
            }
        }
    }
}

/// The current colour of the selection anchor's role cell: `FillForegnd`
/// or `LineColor` resolved through the model, the `Character` row's
/// `Color` for the text role (`pickers::fill_preview`'s resolution
/// rule).
fn current_color(app: &AppCtx, target: Target) -> (u8, u8, u8) {
    const BLACK: (u8, u8, u8) = (0x00, 0x00, 0x00);
    let Some(session) = app.active_document() else {
        return BLACK;
    };
    let Some(shape) = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied())
    else {
        return BLACK;
    };
    let mut reader = CellReader::new(&session.document);
    let read = match target {
        Target::Fill => reader.color(SheetRef::Shape(shape), &CellName::new("FillForegnd")),
        Target::Line => reader.color(SheetRef::Shape(shape), &CellName::new("LineColor")),
        Target::Text => reader.row_color(
            SheetRef::Shape(shape),
            "Character",
            RowTarget::by_ix(0),
            &CellName::new("Color"),
        ),
    };
    match read {
        Ok(color) => (color.r, color.g, color.b),
        Err(_) => BLACK,
    }
}

/// What the dialog body did this frame (the Page Setup pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing yet (or a hex spelling that did not parse; the dialog
    /// keeps the message in its `error`).
    None,
    /// The user pressed OK: apply this colour and close.
    Ok((u8, u8, u8)),
    /// The user pressed Cancel (or closed the window): change nothing.
    Cancel,
}

/// The width of the picker's fields, in points.
const PICKER_WIDTH: f32 = 220.0;

/// The slices one gradient band draws (the picker paints its own
/// gradients; see the module comment).
const SLICES: usize = 32;

/// Draws the dialog body: the hue/saturation/value picker, the hex
/// field and OK and Cancel. The host owns the [`egui::Window`] and its
/// close button.
pub fn show(ui: &mut Ui, state: &mut State) -> Outcome {
    // The bordered controls of the P7-18 design pass.
    theme::raise_controls(ui);
    let mut hsv = hsv_of(state.color);
    let mut changed = false;
    changed |= saturation_value(ui, &mut hsv.1, &mut hsv.2, hsv.0);
    ui.add_space(4.0);
    changed |= hue(ui, &mut hsv.0);
    if changed {
        state.color = rgb_of(hsv);
        state.hex = hex_of(state.color);
        state.error = None;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        // The chosen colour, beside its hex spelling.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 18.0), Sense::hover());
        let rgb = Color32::from_rgb(state.color.0, state.color.1, state.color.2);
        ui.painter().rect_filled(rect, 0.0, rgb);
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, Color32::from_gray(0x8a)),
            egui::StrokeKind::Inside,
        );
        ui.label("Hex:");
        let edit = ui
            .add(
                egui::TextEdit::singleline(&mut state.hex)
                    .desired_width(84.0)
                    .hint_text("#3A7F2C"),
            )
            .accessible_name("Hex");
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            state.commit_hex();
        }
    });
    buttons(ui, state)
}

/// The saturation/value square (task P7-54: the picker's 2-D field):
/// white through the pure hue on x, down to black on y. Clicking or
/// dragging sets `s` and `v`.
fn saturation_value(ui: &mut Ui, s: &mut f32, v: &mut f32, h: f32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(PICKER_WIDTH, 140.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let (r, g, b) = rgb_of((h, 1.0, 1.0));
    let hue = Color32::from_rgb(r, g, b);
    let width = rect.width() / SLICES as f32;
    for slice in 0..SLICES {
        let from = slice as f32 / SLICES as f32;
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + width * slice as f32, rect.min.y),
            egui::pos2(rect.min.x + width * (slice as f32 + 1.0), rect.max.y),
        );
        let white = (1.0 - from) * 255.0;
        let tint = Color32::from_rgb(
            (white + f32::from(hue.r()) * from).round() as u8,
            (white + f32::from(hue.g()) * from).round() as u8,
            (white + f32::from(hue.b()) * from).round() as u8,
        );
        painter.rect_filled(band, 0.0, tint);
    }
    let height = rect.height() / SLICES as f32;
    for slice in 0..SLICES {
        let dark = (slice as f32 + 1.0) / SLICES as f32;
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.min.x, rect.min.y + height * slice as f32),
            egui::pos2(rect.max.x, rect.min.y + height * (slice as f32 + 1.0)),
        );
        painter.rect_filled(
            band,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, (dark * 255.0) as u8),
        );
    }
    // The marker at the current saturation and value.
    let at = egui::pos2(
        rect.min.x + rect.width() * (*s),
        rect.min.y + rect.height() * (1.0 - *v),
    );
    painter.circle_stroke(at, 5.0, egui::Stroke::new(1.5, Color32::WHITE));
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *s = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        *v = (1.0 - (pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        return true;
    }
    false
}

/// The hue slider: the colour wheel as a band. Clicking or dragging
/// sets `h`.
fn hue(ui: &mut Ui, h: &mut f32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(PICKER_WIDTH, 14.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let width = rect.width() / SLICES as f32;
    for slice in 0..SLICES {
        let at = slice as f32 / SLICES as f32;
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + width * slice as f32, rect.min.y),
            egui::pos2(rect.min.x + width * (slice as f32 + 1.0), rect.max.y),
        );
        let (r, g, b) = rgb_of((at, 1.0, 1.0));
        painter.rect_filled(band, 0.0, Color32::from_rgb(r, g, b));
    }
    let at = rect.min.x + rect.width() * h.rem_euclid(1.0);
    painter.line_segment(
        [
            egui::pos2(at, rect.min.y - 1.0),
            egui::pos2(at, rect.max.y + 1.0),
        ],
        egui::Stroke::new(1.5, Color32::WHITE),
    );
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *h = ((pos.x - rect.min.x) / rect.width())
            .clamp(0.0, 1.0)
            .fract();
        return true;
    }
    false
}

/// The OK and Cancel row (the Fill dialog's button row without Apply).
/// OK takes the hex field's text as typed (the newest intent wins over
/// an uncommitted picker drag); a spelling that does not parse keeps
/// the dialog open with its message.
fn buttons(ui: &mut Ui, state: &mut State) -> Outcome {
    let mut outcome = Outcome::None;
    ui.add_space(12.0);
    if let Some(message) = &state.error {
        ui.colored_label(theme::color_at(ui.ctx(), "error"), message.as_str());
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The default button of a dialog is the accent one
            // (P7-18's `accent_controls`).
            theme::accent_controls(ui);
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
            if ui.button("OK").clicked() {
                match parse_hex(&state.hex) {
                    Some(color) => {
                        state.color = color;
                        state.error = None;
                        outcome = Outcome::Ok(color);
                    }
                    None => {
                        state.error = Some("use #RGB or #RRGGBB".to_owned());
                    }
                }
            }
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with one selected rectangle (the `pickers` test rule).
    fn app() -> AppCtx {
        use larrygram_commands::app::{DocumentSession, Settings};
        let mut app = AppCtx::new(Settings::default());
        let mut document = larrygram_model::Document::new();
        let mut page = larrygram_model::Page::new(larrygram_model::PageId { index: 0 }, 1);
        let mut shape = larrygram_model::Shape::new(larrygram_model::ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(larrygram_model::Cell {
            name: CellName::new("FillForegnd"),
            formula: Some("RGB(58,127,44)".to_owned()),
            value: Some(larrygram_model::CellValue::Color(
                larrygram_model::Color::new(0x3a, 0x7f, 0x2c),
            )),
            unit: larrygram_geom::Unit::Unitsless,
            preserved: larrygram_model::Preserved::new(),
        });
        let shape = document.add_shape(shape);
        page.shapes.push(shape);
        document.pages.push(page);
        app.open_document(DocumentSession::new(document, None));
        app.selection.shapes = vec![shape];
        app.selection.anchor = Some(shape);
        app.selection.page = Some(larrygram_model::PageId { index: 0 });
        app
    }

    #[test]
    fn the_target_parses_the_three_roles() {
        assert_eq!(Target::parse("fill"), Some(Target::Fill));
        assert_eq!(Target::parse("LINE"), Some(Target::Line));
        assert_eq!(Target::parse(" Text "), Some(Target::Text));
        assert_eq!(Target::parse("nope"), None);
    }

    #[test]
    fn the_target_names_the_swatch_command() {
        assert_eq!(Target::Fill.command(), "format.set.fill.color");
        assert_eq!(Target::Line.command(), "format.set.line.color");
        assert_eq!(Target::Text.command(), "text.font.color");
        assert_eq!(Target::Fill.title(), "Fill Color");
    }

    #[test]
    fn the_hex_field_parses_and_rejects() {
        assert_eq!(parse_hex("#3A7F2C"), Some((0x3a, 0x7f, 0x2c)));
        assert_eq!(parse_hex("#abc"), Some((0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_hex("3A7F2C"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#GGGGGG"), None);
    }

    /// The picker's colour conversions round-trip (the hex field and
    /// the picker agree on one colour).
    #[test]
    fn hsv_round_trips_through_rgb() {
        for color in [
            (0x3a, 0x7f, 0x2c),
            (0x00, 0x00, 0x00),
            (0xff, 0xff, 0xff),
            (0xff, 0x00, 0x00),
            (0x00, 0xff, 0x00),
            (0x00, 0x00, 0xff),
            (0x12, 0x34, 0x56),
            (0x80, 0x80, 0x80),
        ] {
            let back = rgb_of(hsv_of(color));
            assert_eq!(back, color, "{color:?} round-trips");
        }
    }

    #[test]
    fn the_state_opens_on_the_resolved_colour() {
        let state = State::from_app(&app(), Target::Fill);
        assert_eq!(state.color, (0x3a, 0x7f, 0x2c));
        assert_eq!(state.hex, "#3A7F2C");
        // A role with no cell to read opens on black.
        let state = State::from_app(&app(), Target::Text);
        assert_eq!(state.color, (0, 0, 0));
    }

    #[test]
    fn committing_the_hex_field_moves_the_colour_or_the_message() {
        let mut state = State::from_app(&app(), Target::Fill);
        state.hex = "#00FF00".to_owned();
        state.commit_hex();
        assert_eq!(state.color, (0, 0xff, 0x00));
        assert_eq!(state.error, None);
        state.hex = "red".to_owned();
        state.commit_hex();
        assert_eq!(
            state.color,
            (0, 0xff, 0x00),
            "a rejected spelling changes nothing"
        );
        assert_eq!(state.error.as_deref(), Some("use #RGB or #RRGGBB"));
    }

    /// The dialog body draws and its buttons answer (a kittest pass
    /// over the widgets, the `format_dialogs` tests' rule).
    #[test]
    fn the_dialog_draws_and_ok_and_cancel_answer() {
        use egui_kittest::{Harness, kittest::Queryable};
        let state: std::rc::Rc<std::cell::RefCell<State>> = std::rc::Rc::new(
            std::cell::RefCell::new(State::from_app(&app(), Target::Fill)),
        );
        let outcomes: std::rc::Rc<std::cell::RefCell<Vec<Outcome>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 360.0))
            .build_ui({
                let state = std::rc::Rc::clone(&state);
                let outcomes = std::rc::Rc::clone(&outcomes);
                move |ui| {
                    let mut guard = state.borrow_mut();
                    outcomes.borrow_mut().push(show(ui, &mut guard));
                }
            });
        harness.run();
        {
            let root = harness.root();
            let hex = root
                .query_all_by_role_and_label(egui::accesskit::Role::TextInput, "Hex")
                .next()
                .expect("the Hex field");
            assert_eq!(
                hex.value().as_deref(),
                Some("#3A7F2C"),
                "the current colour"
            );
            let cancel = root
                .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
                .next()
                .expect("the Cancel button");
            cancel.click();
        }
        for _ in 0..3 {
            harness.run();
        }
        assert!(
            outcomes.borrow().contains(&Outcome::Cancel),
            "Cancel answers: {:?}",
            outcomes.borrow()
        );
    }
}
