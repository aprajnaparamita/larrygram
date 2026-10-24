// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The neutral document structure both sides of the comparison map
//! into (P8-08, ADR 0072).
//!
//! `dump.rs` builds a [`Doc`] from `vsd2raw` output and `ours.rs`
//! builds one from our [`Import`]; `compare.rs` diffs the two. A field
//! one side cannot state stays `None` and is reported as "not
//! compared", never as a mismatch.
//!
//! Coordinates are in the oracle's dump space: inches, y down, origin
//! at the page's top-left corner (the `vsd2raw` output is a y-down
//! rendering; see `ours.rs` for the conversion). Angles are degrees in
//! `[0, 360)`, measured in the same y-down space. Colours are RGB.

use std::collections::BTreeMap;

/// One whole document, as far as the comparison sees it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    /// Document metadata by the fixed keys [`METADATA_KEYS`].
    pub metadata: BTreeMap<String, String>,
    /// The pages, in file order.
    pub pages: Vec<Page>,
}

/// The metadata keys the comparison states (ADR 0072: title, creator,
/// template, dates).
pub const METADATA_KEYS: [&str; 4] = ["title", "creator", "template", "date"];

/// One page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    /// The page name, when stated.
    pub name: Option<String>,
    /// The page width in inches, when stated.
    pub width: Option<f64>,
    /// The page height in inches, when stated.
    pub height: Option<f64>,
    /// The top-level shapes, in document order; sub-shapes nest under
    /// their group.
    pub shapes: Vec<Shape>,
}

/// One shape (the oracle's `startLayer` group, one per shape).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    /// The place label: the oracle's `draw:id`, or a positional label.
    pub label: String,
    /// The shape's text, concatenated and as written (the comparison
    /// normalises it).
    pub text: Option<String>,
    /// The fill kind.
    pub fill: Option<Fill>,
    /// The fill colour.
    pub fill_color: Option<Rgb>,
    /// The stroke kind.
    pub stroke: Option<Stroke>,
    /// The stroke colour.
    pub stroke_color: Option<Rgb>,
    /// The stroke width in inches.
    pub stroke_width: Option<f64>,
    /// Whether a shadow is drawn.
    pub shadow: Option<bool>,
    /// The shadow colour.
    pub shadow_color: Option<Rgb>,
    /// The text box (position, size, rotation) in dump space.
    pub text_box: Option<TextBox>,
    /// The geometry summary.
    pub geometry: Option<Geometry>,
    /// The sub-shapes of a group, in document order.
    pub children: Vec<Shape>,
}

/// The fill kind of a shape, as the two sides spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// No fill (`FillPattern` 0).
    None,
    /// A solid fill (`FillPattern` 1).
    Solid,
    /// A hatch or pattern fill (`FillPattern` 2 to 24).
    Hatch,
    /// A gradient fill (`FillPattern` 25 to 40).
    Gradient,
    /// Any other pattern.
    Other,
}

impl Fill {
    /// The label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            Fill::None => "none",
            Fill::Solid => "solid",
            Fill::Hatch => "hatch",
            Fill::Gradient => "gradient",
            Fill::Other => "other",
        }
    }
}

/// The stroke kind of a shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// No stroke (`LinePattern` 0).
    None,
    /// A solid stroke (`LinePattern` 1).
    Solid,
    /// A dashed stroke (`LinePattern` 2 and up).
    Dash,
    /// Any other pattern.
    Other,
}

impl Stroke {
    /// The label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            Stroke::None => "none",
            Stroke::Solid => "solid",
            Stroke::Dash => "dash",
            Stroke::Other => "other",
        }
    }
}

/// An RGB colour triple (ADR 0072: colours compare as RGB triples).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// Creates a triple.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The `#rrggbb` spelling used in reports and in the dump.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl std::fmt::Display for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex())
    }
}

/// A text box: the top-left corner of the box, its size and its
/// rotation, all in dump space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBox {
    /// The x of the box's top-left corner, inches.
    pub x: f64,
    /// The y of the box's top-left corner, inches (y down).
    pub y: f64,
    /// The width, inches.
    pub width: f64,
    /// The height, inches.
    pub height: f64,
    /// The rotation in degrees in `[0, 360)`.
    pub rotate_deg: f64,
}

/// The geometry summary of one shape (ADR 0072: row kinds and
/// coordinates).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    /// The number of sub-paths (the `MoveTo` rows / `M` actions).
    pub subpaths: usize,
    /// Whether every row is a straight segment (`MoveTo`/`LineTo`
    /// only, and `M`/`L`/`Z` actions only). This is the row-kind
    /// comparison: the oracle flattens curves to line chords, so row
    /// kinds align only for straight geometry.
    pub polyline: bool,
    /// The vertices in order, in dump space. Meaningful only when
    /// [`Self::polyline`] is set on both sides.
    pub points: Vec<(f64, f64)>,
}
