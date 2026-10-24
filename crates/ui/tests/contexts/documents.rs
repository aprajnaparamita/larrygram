//! One test document per selection context (task P7-08).
//!
//! The documents are built with the model API only: no corpus file is
//! needed. They serve the context test and the enablement cross-check.

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::{ClipboardContent, ClipboardKind};
use larrygram_geom::Unit;
use larrygram_model::mutate::{VIS_BEGIN_X, VIS_CONNECTION_POINT};
use larrygram_model::{
    Cell, CellName, CellValue, Connect, Document, PageId, Preserved, QName, Row, Section,
    SectionName, Shape, ShapeId, TextContent, TextPart, XmlNode,
};
use larrygram_ui::contexts::{ContextState, SelectionContext, WindowMode};

/// One context document and the state that makes it that context.
pub struct Case {
    /// The expected context.
    pub context: SelectionContext,
    /// The UI-only editor and window state.
    pub ui: ContextState,
    /// The application state.
    pub app: AppCtx,
}

/// A number cell in inches.
fn number(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(value.to_string()),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A text cell.
fn text(name: &str, value: &str) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("\"{value}\"")),
        value: Some(CellValue::Str(value.to_owned())),
        unit: Unit::Unitsless,
        preserved: Preserved::new(),
    }
}

/// A filled rectangle at `(x, y)`.
fn rectangle(document: &mut Document, page: PageId, x: f64) -> ShapeId {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    shape.sheet.cells = vec![
        number("Width", 2.0),
        number("Height", 1.0),
        number("PinX", x),
        number("PinY", 1.0),
        number("LocPinX", 1.0),
        number("LocPinY", 0.5),
    ];
    document
        .insert_shape(page, shape, None)
        .expect("insert rectangle")
}

/// An empty shape with a name and no text.
fn shape() -> Shape {
    Shape::new(ShapeId { index: 0 }, 0)
}

/// One open document with the first page active, or `page` when given.
fn app_with(document: Document, page: Option<PageId>) -> AppCtx {
    let mut app = AppCtx::new(Settings::default());
    let source_page = document
        .pages
        .first()
        .map_or(PageId { index: 0 }, |entry| entry.id);
    let index = app.open_document(DocumentSession::new(document, None));
    if let Some(page) = page {
        app.documents[index].active_page = Some(page);
    }
    // A test document starts with one copied shape on the clipboard, so
    // `edit.paste` is judged by its context row rather than by an empty
    // clipboard.
    app.clipboard.content = Some(ClipboardContent {
        kind: ClipboardKind::Shapes,
        shapes: vec![Shape::new(ShapeId { index: 0 }, 1)],
        connects: Vec::new(),
        source_page,
    });
    app
}

/// Selects `selection` on `page`.
fn select(app: &mut AppCtx, page: PageId, selection: &[ShapeId]) {
    app.selection.shapes = selection.to_vec();
    app.selection.anchor = selection.last().copied();
    app.selection.page = Some(page);
}

/// A document with one empty page.
fn one_empty_page() -> (Document, PageId) {
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    (document, page)
}

/// `empty_page`: one empty page, nothing selected.
pub fn empty_page() -> Case {
    let (document, _) = one_empty_page();
    Case {
        context: SelectionContext::EmptyPage,
        ui: ContextState::default(),
        app: app_with(document, None),
    }
}

/// `shape_selected` (pending data): one rectangle selected.
pub fn shape_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let shape = rectangle(&mut document, page, 1.0);
    let mut app = app_with(document, None);
    select(&mut app, page, &[shape]);
    Case {
        context: SelectionContext::ShapeSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `shapes_selected`: two rectangles selected.
pub fn shapes_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let first = rectangle(&mut document, page, 1.0);
    let second = rectangle(&mut document, page, 4.0);
    let mut app = app_with(document, None);
    select(&mut app, page, &[first, second]);
    Case {
        context: SelectionContext::ShapesSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `text_editing`: a rectangle with text open in the editor.
pub fn text_editing() -> Case {
    let (mut document, page) = one_empty_page();
    let shape = rectangle(&mut document, page, 1.0);
    document
        .set_shape_text(
            shape,
            TextContent {
                parts: vec![TextPart::Text("Hello".to_owned())],
            },
        )
        .expect("shape text");
    let mut app = app_with(document, None);
    select(&mut app, page, &[shape]);
    Case {
        context: SelectionContext::TextEditing,
        ui: ContextState {
            text_editing: true,
            window: WindowMode::Drawing,
        },
        app,
    }
}

/// `connector_selected` (pending data): a glued connector selected.
pub fn connector_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let first = rectangle(&mut document, page, 1.0);
    let second = rectangle(&mut document, page, 5.0);
    let connector = document
        .insert_shape(page, shape(), None)
        .expect("insert connector");
    let connector_visio = document.shape(connector).expect("connector").visio_id;
    let first_visio = document.shape(first).expect("first").visio_id;
    let second_visio = document.shape(second).expect("second").visio_id;
    document
        .add_connect(
            page,
            Connect {
                from_sheet: connector_visio,
                from_cell: "BeginX".to_owned(),
                from_part: VIS_BEGIN_X,
                to_sheet: first_visio,
                to_cell: "PinX".to_owned(),
                to_part: VIS_CONNECTION_POINT,
                preserved: Preserved::new(),
            },
        )
        .expect("begin glue");
    document
        .add_connect(
            page,
            Connect {
                from_sheet: connector_visio,
                from_cell: "EndX".to_owned(),
                from_part: larrygram_model::mutate::VIS_END_X,
                to_sheet: second_visio,
                to_cell: "PinX".to_owned(),
                to_part: VIS_CONNECTION_POINT,
                preserved: Preserved::new(),
            },
        )
        .expect("end glue");
    let mut app = app_with(document, None);
    select(&mut app, page, &[connector]);
    Case {
        context: SelectionContext::ConnectorSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `group_selected`: two rectangles grouped, the group selected.
pub fn group_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let first = rectangle(&mut document, page, 1.0);
    let second = rectangle(&mut document, page, 5.0);
    let group = document
        .group_shapes(page, &[first, second])
        .expect("group shapes");
    let mut app = app_with(document, None);
    select(&mut app, page, &[group]);
    Case {
        context: SelectionContext::GroupSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `second_page`: another, empty page is active.
pub fn second_page() -> Case {
    let (mut document, _) = one_empty_page();
    let second = document.add_page("Page-2").expect("second page");
    Case {
        context: SelectionContext::SecondPage,
        ui: ContextState::default(),
        app: app_with(document, Some(second)),
    }
}

/// `picture_selected`: a shape with a `ForeignData` child.
pub fn picture_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let picture = rectangle(&mut document, page, 1.0);
    document
        .shape_mut(picture)
        .expect("picture")
        .preserved
        .children
        .push(XmlNode {
            name: QName::local("ForeignData"),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        });
    let mut app = app_with(document, None);
    select(&mut app, page, &[picture]);
    Case {
        context: SelectionContext::PictureSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `container_selected`: a shape whose `User.msvStructureType` cell is
/// `Container`.
pub fn container_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let container = rectangle(&mut document, page, 1.0);
    let entity = document.shape_mut(container).expect("container");
    let mut section = Section::new(SectionName::from_static("User"));
    let mut row = Row::new();
    row.ix = Some(0);
    row.cells.push(text("msvStructureType", "Container"));
    section.rows.push(row);
    entity.sheet.sections.push(section);
    let mut app = app_with(document, None);
    select(&mut app, page, &[container]);
    Case {
        context: SelectionContext::ContainerSelected,
        ui: ContextState::default(),
        app,
    }
}

/// `shapesheet_window`: a ShapeSheet window is active for a selected
/// shape.
pub fn shapesheet_window() -> Case {
    let (mut document, page) = one_empty_page();
    let shape = rectangle(&mut document, page, 1.0);
    let mut app = app_with(document, None);
    select(&mut app, page, &[shape]);
    Case {
        context: SelectionContext::ShapesheetWindow,
        ui: ContextState {
            text_editing: false,
            window: WindowMode::Shapesheet,
        },
        app,
    }
}

/// `PageSelected`: the page has shapes but nothing is selected (it
/// counts as `empty_page` for availability).
pub fn page_selected() -> Case {
    let (mut document, page) = one_empty_page();
    let _shape = rectangle(&mut document, page, 1.0);
    Case {
        context: SelectionContext::PageSelected,
        ui: ContextState::default(),
        app: app_with(document, None),
    }
}

/// A stencil is open for editing (a window mode, not one of the ten).
pub fn stencil_edit() -> Case {
    let (document, _) = one_empty_page();
    Case {
        context: SelectionContext::StencilEdit,
        ui: ContextState {
            text_editing: false,
            window: WindowMode::Stencil,
        },
        app: app_with(document, None),
    }
}

/// Every context document, in context-table order, plus the window
/// modes.
pub fn all() -> Vec<Case> {
    vec![
        empty_page(),
        page_selected(),
        shape_selected(),
        shapes_selected(),
        text_editing(),
        connector_selected(),
        group_selected(),
        second_page(),
        picture_selected(),
        container_selected(),
        shapesheet_window(),
        stencil_edit(),
    ]
}

/// The case for one data-side context id, if a document exists.
pub fn for_id(id: larrygram_ui_spec::v2::ContextId) -> Option<Case> {
    all()
        .into_iter()
        .find(|case| case.context.id() == id && case.context != SelectionContext::PageSelected)
}
