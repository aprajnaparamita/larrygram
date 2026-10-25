//! Document inspection as stable JSON (ADR 0078; task P9-04).
//!
//! Pure functions over `&AppCtx`: they never change the state. Shapes
//! are named by their Visio id on their page, pages by their 1-based
//! index or their name, documents by the handles of [`Handles`]. Large
//! pages are paged ([`encode_cursor`]) and filtered
//! ([`crate::ops::ShapeFilter`]); text is cut at the limits and flagged.

use larrygram_commands::SessionId;
use larrygram_commands::app::AppCtx;
use larrygram_model::{Document, PageId};

use crate::error::ControlError;
use crate::limits::Limits;
use crate::ops::{
    DocumentInfo, DocumentList, DocumentQuery, DocumentView, PageQuery, PageRef, PageView,
    SelectionView, ShapeQuery, ShapeView,
};

/// The document handles of one executor (ADR 0076): `"doc-<n>"`, minted
/// in open order, never reused while the process lives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handles {
    minted: Vec<(SessionId, u64)>,
    next: u64,
}

impl Handles {
    /// No handles yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no handle has been minted.
    pub fn is_empty(&self) -> bool {
        self.minted.is_empty()
    }

    /// Mints handles for documents opened since the last sync and
    /// forgets closed ones (task P9-04).
    pub fn sync(&mut self, app: &AppCtx) {
        let _ = (app, &mut self.minted, &mut self.next);
    }

    /// The handle of a session, once minted (task P9-04).
    pub fn handle(&self, session: SessionId) -> Option<String> {
        let _ = session;
        None
    }

    /// The index into `app.documents` of `handle`, or of the active
    /// document when `None` (task P9-04). An unknown handle is
    /// `NotFound`; no document at all is `NoDocument`.
    pub fn resolve(&self, app: &AppCtx, handle: Option<&str>) -> Result<usize, ControlError> {
        let _ = (app, handle);
        Err(ControlError::not_implemented("inspect::Handles::resolve"))
    }
}

/// Encodes a paging offset as an opaque cursor (task P9-04).
pub fn encode_cursor(offset: usize) -> String {
    let _ = offset;
    String::new()
}

/// Decodes a cursor of [`encode_cursor`] (task P9-04); anything else is
/// `InvalidParams`.
pub fn decode_cursor(cursor: &str) -> Result<usize, ControlError> {
    let _ = cursor;
    Err(ControlError::not_implemented("inspect::decode_cursor"))
}

/// The page `page` names in `document`, else `active` (task P9-04). A
/// missing page is `NotFound`.
pub fn resolve_page(
    document: &Document,
    page: Option<&PageRef>,
    active: Option<PageId>,
) -> Result<PageId, ControlError> {
    let _ = (document, page, active);
    Err(ControlError::not_implemented("inspect::resolve_page"))
}

/// The info of `app.documents[index]` (task P9-04).
pub fn document_info(
    app: &AppCtx,
    handles: &Handles,
    index: usize,
) -> Result<DocumentInfo, ControlError> {
    let _ = (app, handles, index);
    Err(ControlError::not_implemented("inspect::document_info"))
}

/// `documents/list` (task P9-04).
pub fn list_documents(app: &AppCtx, handles: &Handles) -> Result<DocumentList, ControlError> {
    let _ = (app, handles);
    Err(ControlError::not_implemented("inspect::list_documents"))
}

/// `inspect/document` (task P9-04).
pub fn inspect_document(
    app: &AppCtx,
    handles: &Handles,
    query: &DocumentQuery,
) -> Result<DocumentView, ControlError> {
    let _ = (app, handles, query);
    Err(ControlError::not_implemented("inspect::inspect_document"))
}

/// `inspect/page` (task P9-04).
pub fn inspect_page(
    app: &AppCtx,
    handles: &Handles,
    query: &PageQuery,
    limits: &Limits,
) -> Result<PageView, ControlError> {
    let _ = (app, handles, query, limits);
    Err(ControlError::not_implemented("inspect::inspect_page"))
}

/// `inspect/shape` (task P9-04).
pub fn inspect_shape(
    app: &AppCtx,
    handles: &Handles,
    query: &ShapeQuery,
    limits: &Limits,
) -> Result<ShapeView, ControlError> {
    let _ = (app, handles, query, limits);
    Err(ControlError::not_implemented("inspect::inspect_shape"))
}

/// `inspect/selection` (task P9-04).
pub fn inspect_selection(
    app: &AppCtx,
    handles: &Handles,
    query: &DocumentQuery,
) -> Result<SelectionView, ControlError> {
    let _ = (app, handles, query);
    Err(ControlError::not_implemented("inspect::inspect_selection"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use larrygram_commands::app::{DocumentSession, Settings};

    fn stub(error: ControlError) {
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(Document::new(), None));
        let mut handles = Handles::new();
        handles.sync(&app);
        assert!(handles.is_empty(), "stub: nothing minted");
        assert_eq!(handles.handle(app.documents[0].id), None);
        stub(handles.resolve(&app, None).expect_err("stub"));
        assert_eq!(encode_cursor(3), "");
        stub(decode_cursor("x").expect_err("stub"));
        stub(resolve_page(&Document::new(), None, None).expect_err("stub"));
        stub(document_info(&app, &handles, 0).expect_err("stub"));
        stub(list_documents(&app, &handles).expect_err("stub"));
        let limits = Limits::default();
        stub(inspect_document(&app, &handles, &DocumentQuery::default()).expect_err("stub"));
        stub(inspect_page(&app, &handles, &PageQuery::default(), &limits).expect_err("stub"));
        stub(inspect_shape(&app, &handles, &ShapeQuery::default(), &limits).expect_err("stub"));
        stub(inspect_selection(&app, &handles, &DocumentQuery::default()).expect_err("stub"));
    }
}
