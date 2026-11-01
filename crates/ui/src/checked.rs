//! The checked state the text toggles read (task P7-27 item 2): one
//! layout-neutral seam both panels share, so Panel Modern does not
//! call into [`crate::panel_classic`].
//!
//! The seam answers `Some(on)` for the `format.text.*` toggles of a
//! selected shape and `None` for every other command and with no
//! selection.

use larrygram_commands::app::AppCtx;
use larrygram_model::{CellName, SheetRef};

/// The checked state of the text toggles (task P7-24 item 10): Bold,
/// Italic and Underline read the selection anchor's `Character`
/// `Style` bits (1, 2 and 4) and the alignment rows its `Paragraph`
/// `HorzAlign` (0 left, 1 centre, 2 right, 3 justify), both through
/// the model's row resolution (P6-05) so a row the shape inherits
/// counts. `None` for every other command and with no selection.
pub fn text_toggle_checked(app: &AppCtx, command: &str) -> Option<bool> {
    let is_bold = matches!(
        command,
        "format.text.bold" | "format.text.italic" | "format.text.underline"
    );
    let is_align = matches!(
        command,
        "format.text.align_left"
            | "format.text.align_center"
            | "format.text.align_right"
            | "format.text.align_justify"
    );
    if !is_bold && !is_align {
        return None;
    }
    let session = app.active_document()?;
    let shape = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied())?;
    let document = &session.document;
    let row_number = |section: &str, cell: &str| -> f64 {
        document
            .resolve_row_cell(SheetRef::Shape(shape), section, 0, &CellName::new(cell))
            .ok()
            .and_then(|resolved| match resolved.value {
                Some(larrygram_model::CellValue::Number(value)) => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    if is_bold {
        let style = row_number("Character", "Style") as i64;
        return Some(match command {
            "format.text.bold" => style & 1 != 0,
            "format.text.italic" => style & 2 != 0,
            _ => style & 4 != 0,
        });
    }
    let align = row_number("Paragraph", "HorzAlign") as i64;
    Some(match command {
        "format.text.align_left" => align == 0,
        "format.text.align_center" => align == 1,
        "format.text.align_right" => align == 2,
        _ => align == 3,
    })
}

#[cfg(test)]
mod tests {
    use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
    use larrygram_model::{Cell, CellName, CellValue, Document, Page, PageId, Shape, ShapeId};

    use super::*;

    /// An app with one selected shape whose `Character` `Style` is
    /// `style` and whose `Paragraph` `HorzAlign` is `align`.
    fn app_with_text(style: f64, align: f64) -> AppCtx {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        for (section, cell, value) in [
            ("Character", "Style", style),
            ("Paragraph", "HorzAlign", align),
        ] {
            let mut row = larrygram_model::Row::new();
            row.ix = Some(0);
            let mut cell_value = Cell::new(CellName::new(cell));
            cell_value.value = Some(CellValue::Number(value));
            row.cells.push(cell_value);
            let mut section_row =
                larrygram_model::Section::new(larrygram_model::SectionName::new(section));
            section_row.rows.push(row);
            shape.sheet.sections.push(section_row);
        }
        let id = doc.add_shape(shape);
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(doc, None));
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        app
    }

    #[test]
    fn the_text_toggles_read_the_selection_cells() {
        let app = app_with_text(7.0, 2.0);
        for (command, on) in [
            ("format.text.bold", true),
            ("format.text.italic", true),
            ("format.text.underline", true),
            ("format.text.align_right", true),
            ("format.text.align_left", false),
        ] {
            assert_eq!(text_toggle_checked(&app, command), Some(on), "{command}");
        }
        // Every other command reports no state.
        assert_eq!(text_toggle_checked(&app, "format.set.fill.color"), None);
    }

    #[test]
    fn no_selection_reports_no_state() {
        let mut app = app_with_text(1.0, 0.0);
        app.selection.shapes.clear();
        app.selection.anchor = None;
        assert_eq!(text_toggle_checked(&app, "format.text.bold"), None);
    }
}
