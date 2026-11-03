//! The Shape Data value field at the default pane width (tasks P7-44
//! and P7-45).
//!
//! The pane is drawn in real `egui_kittest` frames (ADR 0060) at the
//! default side-pane width (`PANES_WIDTH`, 240 points) and the field is
//! read the way a user reads it:
//!
//! - an ordinary value ("Process", "Decision", a 30-character string)
//!   shows whole, and a whole value carries no tooltip;
//! - a longer value keeps the **whole text** in the field's buffer and
//!   its accessibility value (task P7-45 goal 4: only the drawing is
//!   cut, one row with an ellipsis — the galley is pinned in
//!   `crates/ui/src/panes.rs`'s unit tests), and its full text is the
//!   tooltip (the accesskit description);
//! - focusing the field redraws it whole on the first frame;
//! - Ctrl+Z inside the field after an edit never commits text with an
//!   ellipsis.

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_geom::Unit as GeomUnit;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Row, Section, SectionName, Shape, ShapeId,
};
use larrygram_ui::Intent;
use larrygram_ui::panes::show_shape_data;

/// The default width of the side panes, in points (`PANES_WIDTH` in
/// `crates/ui/src/shell.rs`, task P7-45).
const PANE_WIDTH: f32 = 240.0;

/// The 30-character value of the ordinary case.
const THIRTY: &str = "This is a thirty character str";

/// A value too wide for the field at the default pane width.
const LONG: &str = "A much longer value string that will not fit in the field";

/// One shape with `(label, value)` Shape Data rows, drawn in the Shape
/// Data pane at [`PANE_WIDTH`] every frame (the P4-04 rig pattern).
struct PaneRig {
    harness: Harness<'static>,
    intents: Rc<RefCell<Vec<Intent>>>,
}

impl PaneRig {
    fn new(rows: &[(&str, &str)]) -> Self {
        let mut document = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        shape.name = String::from("Box");
        if !rows.is_empty() {
            let mut section = Section::new(SectionName::new("Property"));
            for (position, (label, value)) in rows.iter().enumerate() {
                let mut row = Row::new();
                row.name = Some(format!("Row_{label}"));
                row.ix = Some(i32::try_from(position).unwrap_or(0));
                let mut name = Cell::new(CellName::new("Label"));
                name.value = Some(CellValue::Str((*label).to_owned()));
                let mut cached = Cell::new(CellName::new("Value"));
                cached.value = Some(CellValue::Str((*value).to_owned()));
                cached.unit = GeomUnit::Unitsless;
                row.cells.push(name);
                row.cells.push(cached);
                section.rows.push(row);
            }
            shape.sheet.sections.push(section);
        }
        let id = document.add_shape(shape);
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.name = String::from("Page-1");
        page.sheet.cells = vec![
            number_cell("PageWidth", 8.5),
            number_cell("PageHeight", 11.0),
        ];
        page.shapes.push(id);
        document.pages.push(page);
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(document, None));
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        app.selection.page = Some(PageId { index: 0 });
        let app = Rc::new(RefCell::new(app));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let harness_app = Rc::clone(&app);
        let harness_intents = Rc::clone(&intents);
        let harness = Harness::builder()
            .with_size(egui::vec2(PANE_WIDTH, 900.0))
            // The app's own theme (fonts included), so the field is as
            // wide and the text as narrow as in the running shell.
            .with_context_setup(|ctx| {
                let sets = larrygram_ui_spec::Loader::from_embedded().expect("the layouts load");
                larrygram_ui::theme::apply(
                    ctx,
                    &sets.get(larrygram_ui_spec::LayoutKind::PanelModern).theme,
                )
                .expect("the theme applies");
            })
            .build_ui(move |ui| {
                let mut app = harness_app.borrow_mut();
                let produced = show_shape_data(ui, &mut app).expect("the pane renders");
                drop(app);
                harness_intents.borrow_mut().extend(produced);
            });
        Self { harness, intents }
    }

    /// The values of the pane's fields, in row order.
    fn values(&mut self) -> Vec<String> {
        self.harness.run();
        self.fields()
    }

    /// The values of the pane's fields as the tree has them now.
    fn fields(&mut self) -> Vec<String> {
        self.harness
            .root()
            .query_all_by_role(Role::TextInput)
            .map(|node| node.value().unwrap_or_default())
            .collect()
    }

    /// The rect of the `nth` field.
    fn rect(&mut self, nth: usize) -> egui::Rect {
        self.harness.run();
        self.harness
            .root()
            .query_all_by_role(Role::TextInput)
            .nth(nth)
            .unwrap_or_else(|| panic!("no field #{nth}"))
            .rect()
    }

    /// The tooltip (accesskit description) of the `nth` field.
    fn tooltip(&mut self, nth: usize) -> Option<String> {
        self.harness.run();
        self.harness
            .root()
            .query_all_by_role(Role::TextInput)
            .nth(nth)
            .unwrap_or_else(|| panic!("no field #{nth}"))
            .accesskit_node()
            .data()
            .description()
            .map(|text| text.to_owned())
    }

    /// The intents produced since the last take.
    fn take_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut *self.intents.borrow_mut())
    }
}

/// A cell with a cached number.
fn number_cell(name: &str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(CellValue::Number(value));
    cell.unit = GeomUnit::Inches;
    cell
}

/// The pixels of `rect` in the rendered frame.
fn crop(image: &image::RgbaImage, rect: egui::Rect) -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in rect.top().max(0.0) as u32..rect.bottom().max(0.0) as u32 {
        for x in rect.left().max(0.0) as u32..rect.right().max(0.0) as u32 {
            pixels.extend_from_slice(&image.get_pixel(x, y).0);
        }
    }
    pixels
}

#[test]
fn ordinary_values_show_whole_at_the_default_pane_width() {
    // Exactly 30 characters.
    assert_eq!(THIRTY.chars().count(), 30);
    let mut rig = PaneRig::new(&[("Step", "Process"), ("Kind", "Decision"), ("Cost", THIRTY)]);
    assert_eq!(rig.values(), ["Process", "Decision", THIRTY]);
    let field_width = rig.rect(0).width();
    assert!(
        field_width > 140.0,
        "the field fills its row (it was 40 points); width {field_width}"
    );
    assert_eq!(rig.tooltip(0), None, "a whole value has no tooltip");
    assert_eq!(rig.tooltip(2), None, "a whole value has no tooltip");
}

#[test]
fn a_longer_value_keeps_its_whole_text_with_the_full_tooltip() {
    // Task P7-45 goal 4: the accessibility value is the full text even
    // while the drawing is cut, and the tooltip carries it too.
    let mut rig = PaneRig::new(&[("Cost", LONG)]);
    let values = rig.values();
    assert_eq!(
        values.first().map(String::as_str),
        Some(LONG),
        "the field's value is the whole text while it is unfocused"
    );
    assert_eq!(
        rig.tooltip(0).as_deref(),
        Some(LONG),
        "the full text is the tooltip"
    );
}

#[test]
fn focusing_shows_the_whole_text_on_the_first_frame() {
    // Task P7-45 goal 4: the field reads its focus from egui's memory
    // at layout time, so the first focused frame redraws the text
    // whole (the cut drawing is the ellipsis galley of the panes unit
    // tests).
    let mut rig = PaneRig::new(&[("Cost", LONG)]);
    let rect = rig.rect(0);
    let unfocused = crop(&rig.harness.render().expect("the frame renders"), rect);
    rig.harness
        .root()
        .query_all_by_role(Role::TextInput)
        .next()
        .expect("the field")
        .focus();
    rig.harness.step();
    assert!(
        rig.harness
            .root()
            .query_all_by_role(Role::TextInput)
            .next()
            .expect("the field")
            .is_focused(),
        "the focus lands"
    );
    let focused = crop(&rig.harness.render().expect("the frame renders"), rect);
    assert_ne!(
        unfocused, focused,
        "the first focused frame redraws the field"
    );
    assert_eq!(
        rig.fields().first().map(String::as_str),
        Some(LONG),
        "the value stays whole throughout"
    );
}

#[test]
fn ctrl_z_after_an_edit_never_commits_an_ellipsis() {
    // Task P7-45 goal 4: egui's own field undo runs on the whole
    // buffer, so the committed text can never be the cut display.
    let mut rig = PaneRig::new(&[("Cost", "0")]);
    rig.harness
        .root()
        .query_all_by_role(Role::TextInput)
        .next()
        .expect("the field")
        .focus();
    rig.harness.run();
    rig.harness
        .key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::A]);
    rig.harness.run();
    rig.harness
        .root()
        .query_all_by_role(Role::TextInput)
        .next()
        .expect("the field")
        .type_text(LONG);
    rig.harness.run();
    rig.harness
        .key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::Z]);
    rig.harness.run();
    rig.harness.key_press(egui::Key::Enter);
    rig.harness.run();
    let intents = rig.take_intents();
    let committed: Vec<&str> = intents
        .iter()
        .filter_map(|intent| intent.invocation.args.text("value"))
        .collect();
    assert!(!committed.is_empty(), "the edit commits");
    for value in &committed {
        assert!(
            !value.contains('\u{2026}'),
            "the committed text never carries the ellipsis: {value:?}"
        );
    }
    assert!(
        rig.fields().iter().all(|value| !value.contains('\u{2026}')),
        "no field shows an ellipsis as its value"
    );
}
