//! Shapes, including group shapes and their sub-shapes.

use crate::id::{MasterId, ShapeId};
use crate::raw::Preserved;
use crate::sheet::Sheet;
use crate::text::TextContent;

/// One shape of a page or master.
///
/// A group shape holds its sub-shapes as `children` of the same arena; the
/// sub-shapes themselves are ordinary shapes (ADR 0011).
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The arena id of this shape.
    pub id: ShapeId,
    /// Visio's `ID` attribute, unique within its page.
    pub visio_id: i32,
    /// Visio's `Name` attribute (user-visible name), as written.
    pub name: String,
    /// Visio's `NameU` attribute (unique name), as written.
    pub name_u: String,
    /// The master this shape is an instance of, or `None`.
    pub master: Option<MasterId>,
    /// The line style reference (`LineStyle` attribute): the Visio `ID` of
    /// the stylesheet row it names, or `None`.
    pub line_style: Option<u32>,
    /// The fill style reference (`FillStyle` attribute): the Visio `ID` of
    /// the stylesheet row it names, or `None`.
    pub fill_style: Option<u32>,
    /// The text style reference (`TextStyle` attribute): the Visio `ID` of
    /// the stylesheet row it names, or `None`.
    pub text_style: Option<u32>,
    /// The parent group shape, or `None` for a top-level shape.
    pub parent: Option<ShapeId>,
    /// The sub-shapes, in file order (empty for a non-group shape).
    pub children: Vec<ShapeId>,
    /// The shape's text (`Text` element), mapped read-side (ADR 0019).
    pub text: Option<TextContent>,
    /// Whether the shape is a guide (`Type="Guide"` in `.vsdx`, a
    /// guide shape record in `.vsd`). Visio draws guides on screen
    /// only: they never print or export, so the renderer skips them.
    pub is_guide: bool,
    /// The ShapeSheet content of this shape.
    pub sheet: Sheet,
    /// Unmapped attributes and children of the `Shape` element kept verbatim
    /// (ADR 0012).
    pub preserved: Preserved,
}

impl Shape {
    /// Creates a shape with the given arena id and Visio `ID`, empty
    /// otherwise.
    pub fn new(id: ShapeId, visio_id: i32) -> Self {
        Self {
            id,
            visio_id,
            name: String::new(),
            name_u: String::new(),
            master: None,
            line_style: None,
            fill_style: None,
            text_style: None,
            parent: None,
            children: Vec::new(),
            text: None,
            is_guide: false,
            sheet: Sheet::new(),
            preserved: Preserved::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shape_is_empty() {
        let s = Shape::new(ShapeId { index: 3 }, 7);
        assert_eq!(s.id, ShapeId { index: 3 });
        assert_eq!(s.visio_id, 7);
        assert!(s.name.is_empty());
        assert!(s.name_u.is_empty());
        assert_eq!(s.master, None);
        assert_eq!(s.line_style, None);
        assert_eq!(s.fill_style, None);
        assert_eq!(s.text_style, None);
        assert_eq!(s.parent, None);
        assert!(s.children.is_empty());
        assert!(s.sheet.cells.is_empty());
        assert!(s.preserved.children.is_empty());
    }

    #[test]
    fn group_children_are_shape_ids() {
        let mut g = Shape::new(ShapeId { index: 0 }, 1);
        g.children.push(ShapeId { index: 1 });
        assert_eq!(g.children, vec![ShapeId { index: 1 }]);
    }
}
