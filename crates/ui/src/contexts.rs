//! Selection contexts (ADR 0064, `docs/ui/contexts.md`).
//!
//! [`current`] computes the one context per frame that drives the
//! contextual tabs, the context menus and the enablement cross-check.
//! Task P7-08 implements it. Nothing below L6 learns about the UI, so
//! the enum lives here and not in `crates/ui-spec`
//! ([`ContextId`] there is the name used in the data files).

use larrygram_commands::AppCtx;
use larrygram_model::{CellValue, Page, Shape, ShapeId};
use larrygram_ui_spec::v2::{ContextId, ContextMenuV2, LayoutV2};

/// What the user is doing right now (ADR 0064).
///
/// One variant per context in `ui-spec/contexts.yaml`, plus
/// [`PageSelected`](Self::PageSelected) (the page has shapes but
/// nothing is selected, which counts as
/// [`EmptyPage`](Self::EmptyPage) for availability) and
/// [`StencilEdit`](Self::StencilEdit) (a stencil is open for editing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionContext {
    /// A page with no shapes, nothing selected.
    EmptyPage,
    /// The page has shapes but nothing is selected.
    PageSelected,
    /// One ordinary 2-D shape selected.
    ShapeSelected,
    /// Two or more shapes selected.
    ShapesSelected,
    /// A shape's text is open for editing.
    TextEditing,
    /// One connector selected.
    ConnectorSelected,
    /// One group selected.
    GroupSelected,
    /// Another, empty page is active.
    SecondPage,
    /// One picture selected.
    PictureSelected,
    /// One container selected.
    ContainerSelected,
    /// A ShapeSheet window is the active window.
    ShapesheetWindow,
    /// A stencil is open for editing.
    StencilEdit,
}

impl SelectionContext {
    /// The context name the data files use.
    ///
    /// `PageSelected` counts as [`ContextId::EmptyPage`] for
    /// availability (ADR 0064).
    pub fn id(self) -> ContextId {
        match self {
            Self::EmptyPage | Self::PageSelected => ContextId::EmptyPage,
            Self::ShapeSelected => ContextId::ShapeSelected,
            Self::ShapesSelected => ContextId::ShapesSelected,
            Self::TextEditing => ContextId::TextEditing,
            Self::ConnectorSelected => ContextId::ConnectorSelected,
            Self::GroupSelected => ContextId::GroupSelected,
            Self::SecondPage => ContextId::SecondPage,
            Self::PictureSelected => ContextId::PictureSelected,
            Self::ContainerSelected => ContextId::ContainerSelected,
            Self::ShapesheetWindow => ContextId::ShapesheetWindow,
            Self::StencilEdit => ContextId::StencilEdit,
        }
    }
}

/// The active window mode of the shell (ADR 0064).
///
/// `commands` (L4) cannot depend on the egui crates (L6), so the text
/// editor state and the active window are not fields of [`AppCtx`].
/// The shell supplies them through [`ContextState`]; [`current`] uses
/// the drawing-window default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowMode {
    /// The drawing window is active.
    #[default]
    Drawing,
    /// A ShapeSheet window is active.
    Shapesheet,
    /// An editable stencil is active.
    Stencil,
}

/// The UI-only part of the context computation (ADR 0064).
///
/// The shell has this state (the [`TextEditor`](crate::text_edit::TextEditor)
/// and the active window); `commands` does not, so the shell passes it
/// to [`current_with`]. [`ContextState::default`] is the drawing
/// window with no text edit in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ContextState {
    /// A shape's text is open in the in-place editor.
    pub text_editing: bool,
    /// The active window.
    pub window: WindowMode,
}

/// Computes the current context from the application state.
///
/// This is [`current_with`] with [`ContextState::default`]: the
/// drawing window and no text edit in progress. Use [`current_with`]
/// in the shell, where the editor and the active window are known.
pub fn current(app: &AppCtx) -> SelectionContext {
    current_with(app, ContextState::default())
}

/// Computes the current context from the application state and the
/// UI-only window and editor state (ADR 0064).
///
/// Precedence: an active ShapeSheet or stencil window, then an open
/// text editor, then the selection. With no active document or no
/// pages the context is [`SelectionContext::EmptyPage`]; an active
/// empty page after the first is [`SelectionContext::SecondPage`];
/// a non-empty page with nothing selected is
/// [`SelectionContext::PageSelected`].
pub fn current_with(app: &AppCtx, ui: ContextState) -> SelectionContext {
    match ui.window {
        WindowMode::Shapesheet => return SelectionContext::ShapesheetWindow,
        WindowMode::Stencil => return SelectionContext::StencilEdit,
        WindowMode::Drawing => {}
    }
    if ui.text_editing {
        return SelectionContext::TextEditing;
    }

    let Some(session) = app.active_document() else {
        return SelectionContext::EmptyPage;
    };
    let Some(page_id) = session
        .active_page
        .or_else(|| session.document.pages.first().map(|p| p.id))
    else {
        return SelectionContext::EmptyPage;
    };
    let Some(page) = session.document.page(page_id) else {
        return SelectionContext::EmptyPage;
    };

    let selected = app.selection.shapes.as_slice();
    match selected {
        [] if page.shapes.is_empty() => {
            if page_id.index == 0 {
                SelectionContext::EmptyPage
            } else {
                SelectionContext::SecondPage
            }
        }
        [] => SelectionContext::PageSelected,
        [shape] => classify(&session.document, page, *shape),
        _ => SelectionContext::ShapesSelected,
    }
}

/// The context of one selected shape, most specific kind first.
fn classify(document: &larrygram_model::Document, page: &Page, shape: ShapeId) -> SelectionContext {
    let Some(entity) = document.shape(shape) else {
        return SelectionContext::ShapeSelected;
    };
    if is_connector(page, entity) {
        SelectionContext::ConnectorSelected
    } else if is_container(entity) {
        SelectionContext::ContainerSelected
    } else if is_picture(entity) {
        SelectionContext::PictureSelected
    } else if !entity.children.is_empty() {
        SelectionContext::GroupSelected
    } else {
        SelectionContext::ShapeSelected
    }
}

/// Whether the shape is a picture: a foreign-data shape (ADR 0064).
fn is_picture(shape: &Shape) -> bool {
    shape
        .preserved
        .children
        .iter()
        .any(|child| child.name.local == "ForeignData")
}

/// Whether the shape is a container: its `User.msvStructureType` cell
/// holds `Container` (ADR 0064).
fn is_container(shape: &Shape) -> bool {
    const CELLS: [&str; 2] = ["msvStructureType", "User.msvStructureType"];
    let cells = shape
        .sheet
        .cells
        .iter()
        .chain(
            shape
                .sheet
                .sections
                .iter()
                .flat_map(|section| &section.cells),
        )
        .chain(
            shape
                .sheet
                .sections
                .iter()
                .flat_map(|section| &section.rows)
                .flat_map(|row| &row.cells),
        );
    cells
        .filter(|cell| CELLS.contains(&cell.name.as_str()))
        .filter_map(|cell| cell.value.as_ref())
        .any(|value| match value {
            CellValue::Str(text) | CellValue::List(text) => text.eq_ignore_ascii_case("Container"),
            _ => false,
        })
}

/// Whether the shape is a connector: it has 1-D glue (the Phase 5
/// connector, ADR 0045).
fn is_connector(page: &Page, shape: &Shape) -> bool {
    page.connects.iter().any(|connect| {
        connect.from_sheet == shape.visio_id
            && (connect.from_cell.starts_with("Begin")
                || connect.from_cell.starts_with("End")
                || matches!(
                    connect.from_part,
                    larrygram_model::mutate::VIS_BEGIN
                        | larrygram_model::mutate::VIS_BEGIN_X
                        | larrygram_model::mutate::VIS_BEGIN_Y
                        | larrygram_model::mutate::VIS_END
                        | larrygram_model::mutate::VIS_END_X
                        | larrygram_model::mutate::VIS_END_Y
                ))
    })
}

/// What a right-click hit, for the context-menu table (ADR 0064).
///
/// The variants are our own hit targets; [`menu_for`] maps each to the
/// layout's menu id. A layout that has no menu for a target returns
/// `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitTarget {
    /// A shape.
    Shape,
    /// An empty part of the page.
    Page,
    /// A page tab.
    PageTab,
    /// The page navigation buttons.
    PageTabNavigation,
    /// Text being edited.
    TextEditing,
    /// A connection point while the connection point tool is active.
    ConnectionPoint,
    /// A comment mark.
    CommentMark,
    /// A hyperlink in text.
    Hyperlink,
    /// An embedded object.
    EmbeddedObject,
    /// The title of a docked window.
    WindowTitle,
    /// The Shapes window.
    ShapesWindow,
    /// The Shape Data window.
    ShapeDataWindow,
    /// The Size & Position window.
    SizePositionWindow,
    /// Inside a ShapeSheet window.
    Shapesheet,
    /// The drawing in full-screen view.
    FullScreen,
    /// The page of a master being edited.
    MasterEdit,
    /// A master in an editable stencil.
    StencilEdit,
}

impl HitTarget {
    /// The layout data id of this target's context menu.
    pub fn menu_id(self) -> &'static str {
        match self {
            Self::Shape => "context.shape",
            Self::Page => "context.page",
            Self::PageTab => "context.page_tab",
            Self::PageTabNavigation => "context.page_tab_navigation",
            Self::TextEditing => "context.text_edit",
            Self::ConnectionPoint => "context.connection_point",
            Self::CommentMark => "context.comment",
            Self::Hyperlink => "context.hyperlink",
            Self::EmbeddedObject => "context.embedded_object",
            Self::WindowTitle => "context.window_title",
            Self::ShapesWindow => "context.shapes_window",
            Self::ShapeDataWindow => "context.shape_data_window",
            Self::SizePositionWindow => "context.size_position_window",
            Self::Shapesheet => "context.shapesheet",
            Self::FullScreen => "context.full_screen",
            Self::MasterEdit => "context.master_edit",
            Self::StencilEdit => "context.stencil_edit",
        }
    }
}

/// The context menu a right-click on `hit` opens in `layout`, or
/// `None` when that layout has no such menu (ADR 0064).
///
/// The return borrows the layout, so the shell can draw the menu while
/// it still holds the loaded layout.
pub fn menu_for(layout: &LayoutV2, hit: HitTarget) -> Option<&ContextMenuV2> {
    let id = hit.menu_id();
    layout
        .context_menus
        .iter()
        .find(|menu| menu.id.as_str() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_data_names() {
        let expected = [
            (SelectionContext::EmptyPage, ContextId::EmptyPage),
            (SelectionContext::PageSelected, ContextId::EmptyPage),
            (SelectionContext::ShapeSelected, ContextId::ShapeSelected),
            (SelectionContext::ShapesSelected, ContextId::ShapesSelected),
            (SelectionContext::TextEditing, ContextId::TextEditing),
            (
                SelectionContext::ConnectorSelected,
                ContextId::ConnectorSelected,
            ),
            (SelectionContext::GroupSelected, ContextId::GroupSelected),
            (SelectionContext::SecondPage, ContextId::SecondPage),
            (
                SelectionContext::PictureSelected,
                ContextId::PictureSelected,
            ),
            (
                SelectionContext::ContainerSelected,
                ContextId::ContainerSelected,
            ),
            (
                SelectionContext::ShapesheetWindow,
                ContextId::ShapesheetWindow,
            ),
            (SelectionContext::StencilEdit, ContextId::StencilEdit),
        ];
        for (context, id) in expected {
            assert_eq!(context.id(), id);
        }
    }

    #[test]
    fn current_with_no_document_is_empty_page() {
        let app = AppCtx::new(larrygram_commands::Settings::default());
        assert_eq!(current(&app), SelectionContext::EmptyPage);
        assert_eq!(
            current_with(
                &app,
                ContextState {
                    text_editing: true,
                    window: WindowMode::Drawing,
                }
            ),
            SelectionContext::TextEditing
        );
        assert_eq!(
            current_with(
                &app,
                ContextState {
                    text_editing: false,
                    window: WindowMode::Shapesheet,
                }
            ),
            SelectionContext::ShapesheetWindow
        );
        assert_eq!(
            current_with(
                &app,
                ContextState {
                    text_editing: false,
                    window: WindowMode::Stencil,
                }
            ),
            SelectionContext::StencilEdit
        );
    }

    #[test]
    fn hit_targets_name_their_menus() {
        assert_eq!(HitTarget::Shape.menu_id(), "context.shape");
        assert_eq!(HitTarget::CommentMark.menu_id(), "context.comment");
        assert_eq!(HitTarget::Shapesheet.menu_id(), "context.shapesheet");
    }

    use larrygram_commands::{DocumentSession, Settings};
    use larrygram_model::mutate::VIS_BEGIN;
    use larrygram_model::{
        Cell, CellName, CellValue, Connect, Document, PageId, Preserved, Shape, TextContent,
        TextPart,
    };

    fn cell(name: &str, value: CellValue) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: None,
            value: Some(value),
            unit: larrygram_geom::Unit::Unitsless,
            preserved: Preserved::new(),
        }
    }

    fn rectangle(document: &mut Document, page: PageId) -> ShapeId {
        let mut shape = Shape::new(ShapeId { index: 0 }, 0);
        shape.sheet.cells.push(cell("PinX", CellValue::Number(1.0)));
        document.insert_shape(page, shape, None).expect("shape")
    }

    fn app(document: Document) -> AppCtx {
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(document, None));
        app
    }

    #[test]
    fn a_document_without_pages_or_selection_is_empty_page() {
        assert_eq!(current(&app(Document::new())), SelectionContext::EmptyPage);
        let mut document = Document::new();
        let page = document.add_page("Page-1").expect("page");
        let _shape = rectangle(&mut document, page);
        let mut app = app(document);
        assert_eq!(current(&app), SelectionContext::PageSelected);
        app.documents[0].active_page = None;
        assert_eq!(current(&app), SelectionContext::PageSelected);
    }

    #[test]
    fn a_stale_selected_shape_falls_back_to_shape_selected() {
        let mut document = Document::new();
        let page = document.add_page("Page-1").expect("page");
        let shape = rectangle(&mut document, page);
        let mut app = app(document);
        app.selection.shapes = vec![shape];
        app.selection.anchor = Some(shape);
        app.active_document_mut()
            .expect("document")
            .document
            .shapes
            .clear();
        assert_eq!(current(&app), SelectionContext::ShapeSelected);
    }

    #[test]
    fn container_values_accept_every_spelling_and_kind() {
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        assert!(!is_container(&shape));
        shape.sheet.cells.push(cell(
            "User.msvStructureType",
            CellValue::Str("container".to_owned()),
        ));
        assert!(is_container(&shape));
        shape.sheet.cells = vec![cell(
            "msvStructureType",
            CellValue::List("Container".to_owned()),
        )];
        assert!(is_container(&shape));
        shape.sheet.cells = vec![cell("msvStructureType", CellValue::Number(1.0))];
        assert!(!is_container(&shape));
    }

    #[test]
    fn pictures_and_groups_are_classified_from_the_model() {
        let mut document = Document::new();
        let page_id = document.add_page("Page-1").expect("page");
        let picture = rectangle(&mut document, page_id);
        document
            .shape_mut(picture)
            .expect("picture")
            .preserved
            .children
            .push(larrygram_model::XmlNode {
                name: larrygram_model::QName::local("ForeignData"),
                attributes: Vec::new(),
                children: Vec::new(),
                self_closing: true,
            });
        let group = rectangle(&mut document, page_id);
        let member = document
            .insert_subshape(group, Shape::new(ShapeId { index: 0 }, 9), None)
            .expect("subshape");
        let app = app(document);
        let page = app.documents[0]
            .document
            .page(page_id)
            .expect("page")
            .clone();
        assert_eq!(
            classify(&app.documents[0].document, &page, picture),
            SelectionContext::PictureSelected
        );
        assert_eq!(
            classify(&app.documents[0].document, &page, group),
            SelectionContext::GroupSelected
        );
        assert_eq!(
            classify(&app.documents[0].document, &page, member),
            SelectionContext::ShapeSelected
        );
    }

    #[test]
    fn connector_glue_by_part_is_recognised() {
        let mut document = Document::new();
        let page_id = document.add_page("Page-1").expect("page");
        let connector = rectangle(&mut document, page_id);
        let target = rectangle(&mut document, page_id);
        let connector_visio = document.shape(connector).expect("connector").visio_id;
        let target_visio = document.shape(target).expect("target").visio_id;
        document
            .add_connect(
                page_id,
                Connect {
                    from_sheet: connector_visio,
                    from_cell: String::new(),
                    from_part: VIS_BEGIN,
                    to_sheet: target_visio,
                    to_cell: "PinX".to_owned(),
                    to_part: 0,
                    preserved: Preserved::new(),
                },
            )
            .expect("glue");
        let page = document.page(page_id).expect("page").clone();
        let shape = document.shape(connector).expect("connector").clone();
        assert!(is_connector(&page, &shape));
    }

    #[test]
    fn text_and_window_state_take_precedence() {
        let mut document = Document::new();
        let page = document.add_page("Page-1").expect("page");
        let shape = rectangle(&mut document, page);
        document
            .set_shape_text(
                shape,
                TextContent {
                    parts: vec![TextPart::Text("x".to_owned())],
                },
            )
            .expect("text");
        let mut app = app(document);
        app.selection.shapes = vec![shape];
        assert_eq!(
            current_with(
                &app,
                ContextState {
                    text_editing: true,
                    window: WindowMode::Shapesheet,
                }
            ),
            SelectionContext::ShapesheetWindow
        );
    }
}
