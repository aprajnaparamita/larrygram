//! The Shape Data value field at the default pane width (task P7-44).
//!
//! The pane is drawn in real `egui_kittest` frames (ADR 0060) at the
//! default side-pane width (`PANES_WIDTH`, 220 points) and the field is
//! read the way a user reads it:
//!
//! - an ordinary value ("Process", "Decision", a 30-character string)
//!   shows whole, and a whole value carries no tooltip;
//! - a longer value is cut with an ellipsis and its full text is the
//!   tooltip (the accesskit description, as the phase 7 checks read
//!   it).

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
use larrygram_ui::panes::show_shape_data;

/// The default width of the side panes, in points (`PANES_WIDTH` in
/// `crates/ui/src/shell.rs`, task P7-45).
const PANE_WIDTH: f32 = 240.0;

/// One shape with `(label, value)` Shape Data rows, drawn in the Shape
/// Data pane at [`PANE_WIDTH`] every frame (the P4-04 rig pattern).
struct PaneRig {
    harness: Harness<'static>,
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
        let harness_app = Rc::clone(&app);
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
                show_shape_data(ui, &mut app).expect("the pane renders");
            });
        Self { harness }
    }

    /// The values of the pane's fields, in row order.
    fn values(&mut self) -> Vec<String> {
        self.harness.run();
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
}

/// A cell with a cached number.
fn number_cell(name: &str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(CellValue::Number(value));
    cell.unit = GeomUnit::Inches;
    cell
}

#[test]
fn ordinary_values_show_whole_at_the_default_pane_width() {
    // Exactly 30 characters.
    let thirty = "This is a thirty character str";
    assert_eq!(thirty.chars().count(), 30);
    let mut rig = PaneRig::new(&[("Step", "Process"), ("Kind", "Decision"), ("Cost", thirty)]);
    assert_eq!(rig.values(), ["Process", "Decision", thirty]);
    let field_width = rig.rect(0).width();
    assert!(
        field_width > 140.0,
        "the field fills its row (it was 40 points); width {field_width}"
    );
    assert_eq!(rig.tooltip(0), None, "a whole value has no tooltip");
    assert_eq!(rig.tooltip(2), None, "a whole value has no tooltip");
}

#[test]
fn a_longer_value_is_cut_with_an_ellipsis_and_its_tooltip() {
    let full = "A much longer value string that will not fit in the field";
    let mut rig = PaneRig::new(&[("Cost", full)]);
    let values = rig.values();
    let shown = values.first().expect("one field").clone();
    assert!(
        shown.ends_with('\u{2026}'),
        "{shown:?} is cut with an ellipsis"
    );
    assert!(
        shown.starts_with("A much"),
        "{shown:?} keeps the start of the value"
    );
    assert!(shown.chars().count() < full.chars().count());
    assert_eq!(
        rig.tooltip(0).as_deref(),
        Some(full),
        "the full text is the tooltip"
    );
}
