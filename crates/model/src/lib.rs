//! Document model: documents, pages, masters, shapes, stylesheets and
//! ShapeSheet sheets, sections, rows and cells.
//!
//! The model is format-agnostic (layer L1, ADR 0002). It keeps Visio's own
//! identifiers (`ID`, `NameU`, `Name`) on every entity, stores each ShapeSheet
//! cell as its formula, its cached value and its unit separately (ADR 0011),
//! and preserves unknown XML verbatim (ADR 0012). Master and stylesheet
//! inheritance is resolved through a lookup, never flattened on load
//! (ADR 0013).
//!
//! Part of Larrygram. Alpha.

#![forbid(unsafe_code)]

pub mod aliases;
mod cell;
mod connection;
mod connector;
mod defaults;
mod document;
mod error;
pub mod generated;
mod geometry;
mod id;
mod layer;
mod lookup;
mod master;
pub mod mutate;
mod name;
mod page;
mod raw;
mod section;
mod shape;
mod sheet;
pub mod stencils;
mod style;
mod text;
mod theme;
mod values;

pub use cell::{Cell, CellValue, Color};
pub use connector::{ConnectorEnd, EndGlue};
pub use defaults::{DocumentedDefault, documented_default};
pub use document::{Document, DocumentKind, Property};
pub use error::Error;
pub use geometry::{
    GeometryPath, GeometryRow, NurbsData, NurbsDataPoint, PolylineData, ShapeGeometry,
};
pub use id::{MasterId, PageId, ShapeId, StyleId};
pub use layer::Layer;
pub use lookup::{CellLookup, DEFAULT_PAGE_SIZE, Resolved, ValueSource};
pub use master::Master;
pub use mutate::{CellWrite, DetachedShapes};
pub use name::{CellName, SectionName};
pub use page::{Connect, Page};
pub use raw::{
    DifferenceKind, Preserved, QName, XmlAttribute, XmlChild, XmlDifference, XmlNode, diff,
};
pub use section::{Row, Section};
pub use shape::Shape;
pub use sheet::Sheet;
pub use style::{Style, Stylesheet};
pub use text::{TextContent, TextPart};
pub use theme::{
    ColorTableEntry, Theme, ThemeColors, ThemeFillKind, ThemeFillStyle, ThemeFonts, ThemeLineKind,
    ThemeLineStyle, ThemeVariation, ThemeVariationStyles,
};
pub use values::{CellReader, CellWarning, RowTarget, SheetRef, WarningKind, palette};
