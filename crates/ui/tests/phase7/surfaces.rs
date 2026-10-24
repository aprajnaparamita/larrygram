//! The per-layout surfaces over an open document: the page-tab
//! strips, the zoom controls and the Panel Classic status bar (task
//! P7-09 Goal 1).

use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId,
};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, settings_for};

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`) (the
/// Phase 5 fixture shape).
fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`).
fn shape(x: f64, y: f64, visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
        cell("FillPattern", 1.0),
        cell("LinePattern", 1.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", 2.0, 0.0),
        ("LineTo", 2.0, 1.0),
        ("LineTo", 0.0, 1.0),
        ("LineTo", 0.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

/// An app with two named pages; the first page carries one shape when
/// `with_shape`.
fn app(kind: LayoutKind, with_shape: bool) -> AppCtx {
    let mut document = Document::new();
    let mut first = Page::new(PageId { index: 0 }, 1);
    first.name = "First".to_owned();
    if with_shape {
        let id = document.add_shape(shape(3.0, 3.0, 1));
        first.shapes.push(id);
    }
    let mut second = Page::new(PageId { index: 1 }, 2);
    second.name = "Second".to_owned();
    document.pages.push(first);
    document.pages.push(second);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    app
}

#[test]
fn the_panel_modern_page_tabs_and_zoom_controls_work() {
    let mut rig = ShellRig::with_app(app(LayoutKind::PanelModern, true));
    let names = rig.names();
    assert!(names.contains_key("First"), "the page tab draws: {names:?}");
    assert!(names.contains_key("Second"), "the page tab draws: {names:?}");
    assert!(names.contains_key("All"), "the All-pages menu draws");
    assert!(names.contains_key("New page"), "the insert-page button draws");
    // The zoom controls of the status bar (docs/ui/panel-modern.md).
    for name in ["Zoom In", "Zoom Out", "Fit to Window"] {
        assert!(names.contains_key(name), "{name:?} draws: {names:?}");
    }

    // A tab click switches the active page directly (as the Phase 4
    // strip does).
    rig.click_named("Second");
    rig.run();
    assert_eq!(
        rig.shell
            .borrow()
            .app
            .active_document()
            .expect("a document")
            .active_page,
        Some(PageId { index: 1 })
    );

    // The zoom buttons run the registry commands.
    let before = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("a document")
        .view
        .zoom;
    rig.click_named("Zoom In");
    rig.run();
    let after = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("a document")
        .view
        .zoom;
    assert!(after > before, "Zoom In raises the zoom ({before} -> {after})");
}

#[test]
fn the_panel_classic_page_strip_has_first_and_last() {
    let mut rig = ShellRig::with_app(app(LayoutKind::PanelClassic, true));
    let names = rig.names();
    // The strip's buttons carry their symbols; the tooltips name
    // them (`|<` First page, `<` Previous page, `>` Next page, `>|`
    // Last page).
    for name in ["First", "Second", "|<", "<", ">", ">|"] {
        assert!(names.contains_key(name), "{name:?} draws: {names:?}");
    }
    // The last-page button jumps to the last page and the first-page
    // button back.
    rig.click_named(">|");
    rig.run();
    assert_eq!(
        rig.shell
            .borrow()
            .app
            .active_document()
            .expect("a document")
            .active_page,
        Some(PageId { index: 1 })
    );
    rig.click_named("|<");
    rig.run();
    assert_eq!(
        rig.shell
            .borrow()
            .app
            .active_document()
            .expect("a document")
            .active_page,
        Some(PageId { index: 0 })
    );
}

#[test]
fn the_panel_classic_status_bar_reports_the_selection() {
    let mut rig = ShellRig::with_app(app(LayoutKind::PanelClassic, true));
    {
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("a document");
        let id = *session.document.pages[0]
            .shapes
            .first()
            .expect("one shape");
        shell.app.selection.shapes = vec![id];
        shell.app.selection.anchor = Some(id);
    }
    let names = rig.names();
    let widths: Vec<&String> = names.keys().filter(|name| name.starts_with("W ")).collect();
    assert!(
        !widths.is_empty(),
        "the status bar reports the selection width: {names:?}"
    );
    let angles: Vec<&String> = names.keys().filter(|name| name.starts_with("A ")).collect();
    assert!(
        !angles.is_empty(),
        "the status bar reports the selection angle: {names:?}"
    );
}
