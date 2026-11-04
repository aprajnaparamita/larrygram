//! The scene graph: the intermediate representation between the model and
//! the PNG and SVG backends (ADR 0020).
//!
//! A [`Scene`] is plain data in page coordinates, in inches (canonical
//! units, ADR 0011). Visual-style indices are resolved before the scene
//! (ADR 0023): strokes carry concrete dash lengths and arrowhead paths.
//! Text carries positioned glyphs and their fonts (ADR 0024).
//!
//! # Text placement
//!
//! [`TextNode::layout`] positions glyphs in the text block's layout space:
//! the origin is the block box's top-left corner, y points down, and the
//! block rotation is already applied to the positions (ADR 0024
//! amendment). A backend draws one glyph as follows (all lengths in
//! inches):
//!
//! 1. take the em-unit outline of `glyph.glyph` from `fonts[glyph.font]`
//!    (y up, [`Font::outline`]);
//! 2. scale it by `glyph.size / 72` (points to inches,
//!    [`GlyphPos::size`](larrygram_text::GlyphPos));
//! 3. rotate it by minus [`Layout::rotation`](larrygram_text::Layout) in
//!    the y-up sense (a positive layout rotation is a clockwise turn on
//!    screen) and mirror nothing: the flip between the y-down layout
//!    space and the y-up shape space is already in step 4;
//! 4. translate it to `origin + (glyph.x, -glyph.y)` in shape-local
//!    coordinates.
//!
//! The shape's [`Shape::transform`] maps it into the enclosing group's
//! space; the chain of ancestor transforms maps it to the page.
//! Underlines and strike-throughs are drawn from [`TextRun`]; layout does
//! not draw them (ADR 0024). A glyph whose font index is out of range or
//! whose outline is empty (a space, or a glyph id the font does not have)
//! draws nothing; the skip is intentional, `text::Layout` reports missing
//! fonts and glyphs as its own warnings.

use larrygram_geom::{Path, Point, Size, Transform};
use larrygram_model::CellWarning;
use larrygram_text::{Align, Font, Insets, Layout};

/// A colour with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red, 0 to 255.
    pub r: u8,
    /// Green, 0 to 255.
    pub g: u8,
    /// Blue, 0 to 255.
    pub b: u8,
    /// Alpha, 0 to 255.
    pub a: u8,
}

impl Rgba {
    /// A colour from its components.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour.
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
}

/// One gradient stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// The offset along the gradient, 0 to 1.
    pub offset: f64,
    /// The colour at the offset.
    pub color: Rgba,
}

/// A fill or stroke paint (ADR 0020).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Paint {
    /// One colour.
    Solid(Rgba),
    /// A linear gradient from `start` to `end`.
    LinearGradient {
        /// The start point.
        start: Point,
        /// The end point.
        end: Point,
        /// The stops, in order.
        stops: Vec<GradientStop>,
    },
    /// A radial gradient around `center` with the given radius.
    RadialGradient {
        /// The centre.
        center: Point,
        /// The radius.
        radius: f64,
        /// The stops, in order.
        stops: Vec<GradientStop>,
    },
    /// A repeating tile (a fill pattern, ADR 0023).
    Pattern(PatternPaint),
}

/// A repeating tile paint (ADR 0023).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternPaint {
    /// The tile's stroke colour.
    pub foreground: Rgba,
    /// The tile's background colour.
    pub background: Rgba,
    /// The tile shape in inches, inside the `(0, 0)` to `tile_size` box
    /// (the builder scales the unit-box tile of `render::styles` to
    /// `tile_size`).
    pub tile: Path,
    /// The tile size, in inches.
    pub tile_size: Size,
    /// Whether the tile shapes are filled (`true`) or stroked (`false`).
    pub filled: bool,
    /// The stroke width of a stroked tile, in inches (`filled: false`).
    pub tile_stroke_width: f64,
}

/// The shape of a stroke's ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LineCap {
    /// Flat at the end point.
    #[default]
    Butt,
    /// Semicircular.
    Round,
    /// Square past the end point.
    Square,
}

/// The shape of a stroke's joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LineJoin {
    /// Sharp.
    #[default]
    Miter,
    /// Rounded.
    Round,
    /// Flat.
    Bevel,
}

/// A line end (arrowhead) on a stroke (ADR 0020).
#[derive(Debug, Clone, PartialEq)]
pub struct LineEnd {
    /// The arrowhead shape in unit coordinates.
    pub shape: Path,
    /// The arrowhead width, in inches.
    pub width: f64,
    /// The arrowhead length, in inches.
    pub length: f64,
    /// Whether the shape is filled.
    pub filled: bool,
    /// The fill colour, or `None` to use the stroke paint.
    pub color: Option<Rgba>,
}

/// A stroke over a path (ADR 0020).
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// The stroke paint.
    pub paint: Paint,
    /// The stroke width, in inches.
    pub width: f64,
    /// The dash lengths, in inches; empty means solid.
    pub dash: Vec<f64>,
    /// The cap shape.
    pub cap: LineCap,
    /// The join shape.
    pub join: LineJoin,
    /// The miter limit.
    pub miter_limit: f64,
    /// The line end at the start of the path.
    pub start: Option<LineEnd>,
    /// The line end at the end of the path.
    pub end: Option<LineEnd>,
}

/// A drop shadow behind a shape (ADR 0020).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// The shadow colour.
    pub color: Rgba,
    /// The shadow offset, in inches.
    pub offset: Point,
    /// The blur radius, in inches.
    pub blur: f64,
}

/// A clip path.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The clipping path.
    pub path: Path,
}

/// The style of one text run (the `Character` rows, ADR 0024).
///
/// A glyph's `run` index selects its style here (the builder maps the
/// layout's run indices, bullet runs included, onto [`TextNode::runs`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRun {
    /// Whether the run is underlined (the renderer draws it).
    pub underline: bool,
    /// Whether the run is struck through (the renderer draws it).
    pub strikethrough: bool,
    /// The run's colour.
    pub color: Rgba,
}

/// A laid-out text block (ADR 0020).
#[derive(Debug, Clone, PartialEq)]
pub struct TextNode {
    /// The fonts the glyphs index into (ADR 0024).
    pub fonts: Vec<Font>,
    /// The style of every run the glyphs index into.
    pub runs: Vec<TextRun>,
    /// The laid-out glyphs.
    pub layout: Layout,
    /// The text colour (the runs carry their own colour).
    pub color: Rgba,
    /// The block box's size, in shape-local inches: the box the text
    /// lays out in at [`TextNode::origin`] (the wrap box, task P7-48
    /// item 1: the in-place editor's box and wrap width follow it, not
    /// the laid-out content).
    pub block: Size,
    /// The block origin in shape-local coordinates, in inches: the
    /// position of the layout's `(0, 0)` (the unrotated block box's
    /// top-left corner). See the module docs for the full placement
    /// recipe. The block rotation lives on [`Layout::rotation`].
    pub origin: Point,
    /// The text margins (the `LeftMargin`, `RightMargin`, `TopMargin`
    /// and `BottomMargin` cells), in inches: the box the laid-out text
    /// sits in, inset from [`TextNode::block`] (task P7-51 item 1: the
    /// in-place editor's field follows them, so it cannot disagree
    /// with the drawn text).
    pub margins: Insets,
    /// The first paragraph's horizontal alignment (its `Paragraph`
    /// row's `HorzAlign`, ADR 0019; task P7-51 item 1): the in-place
    /// editor's field aligns its typed text like the drawn text
    /// ([`Align::Justify`] draws justified but edits left).
    pub horz_align: Align,
}

/// One shape node: a filled and stroked path with optional text.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The model shape this node was built from, when there is one (ADR
    /// 0041): hit-testing and selection map a node back to the model
    /// through it. The builder records the id at the shape construction
    /// site in `build.rs`; a node the builder synthesises (a shadow body)
    /// has `None`.
    pub shape_id: Option<larrygram_model::ShapeId>,
    /// The shape-local to parent transform (ADR 0020): relative to the
    /// enclosing [`Group`] node, or to the page for a top-level node.
    pub transform: Transform,
    /// The shape's path in shape-local coordinates.
    pub path: Path,
    /// The fill, when the shape fills.
    pub fill: Option<Paint>,
    /// The stroke, when the shape strokes.
    pub stroke: Option<Stroke>,
    /// The drop shadow, when the shape has one.
    pub shadow: Option<Shadow>,
    /// The clip, when the shape clips its children or content.
    pub clip: Option<Clip>,
    /// The text block, when the shape has text.
    pub text: Option<TextNode>,
}

/// A group node: transformed children.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The group-local to parent transform (ADR 0020): relative to the
    /// enclosing [`Group`] node, or to the page for a top-level node.
    /// Backends compose it with every child's transform.
    pub transform: Transform,
    /// The clip applied to the children.
    pub clip: Option<Clip>,
    /// The children, in draw order.
    pub children: Vec<Node>,
}

/// One node of the scene graph.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Node {
    /// A group of nodes.
    Group(Group),
    /// A shape, boxed to keep the enum small.
    Shape(Box<Shape>),
}

/// One page as a scene graph (ADR 0020).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// The page width, in inches.
    pub width: f64,
    /// The page height, in inches.
    pub height: f64,
    /// The page background, or `None` for transparent.
    pub background: Option<Rgba>,
    /// The top-level nodes, in draw order.
    pub nodes: Vec<Node>,
    /// The warnings recorded while building (ADR 0021).
    pub warnings: Vec<CellWarning>,
}

impl Scene {
    /// An empty scene of the given page size, in inches.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            background: None,
            nodes: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use larrygram_geom::Angle;

    use super::*;

    #[test]
    fn rgba_holds_its_components() {
        let c = Rgba::new(1, 2, 3, 4);
        assert_eq!((c.r, c.g, c.b, c.a), (1, 2, 3, 4));
        assert_eq!(Rgba::opaque(1, 2, 3), Rgba::new(1, 2, 3, 255));
    }

    #[test]
    fn new_scene_is_empty() {
        let s = Scene::new(11.0, 8.5);
        assert_eq!(s.width, 11.0);
        assert_eq!(s.height, 8.5);
        assert_eq!(s.background, None);
        assert!(s.nodes.is_empty());
        assert!(s.warnings.is_empty());
        assert_eq!(Scene::default().nodes.len(), 0);
    }

    #[test]
    fn nodes_hold_their_data() {
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: Path::new(),
            fill: Some(Paint::Solid(Rgba::opaque(0, 0, 0))),
            stroke: Some(Stroke {
                paint: Paint::Solid(Rgba::opaque(0, 0, 0)),
                width: 0.01,
                dash: vec![0.1, 0.05],
                cap: LineCap::Round,
                join: LineJoin::Bevel,
                miter_limit: 4.0,
                start: None,
                end: Some(LineEnd {
                    shape: Path::new(),
                    width: 0.1,
                    length: 0.2,
                    filled: true,
                    color: None,
                }),
            }),
            shadow: Some(Shadow {
                color: Rgba::new(0, 0, 0, 128),
                offset: Point::new(0.02, -0.02),
                blur: 0.01,
            }),
            clip: Some(Clip { path: Path::new() }),
            text: Some(TextNode {
                fonts: Vec::new(),
                runs: vec![TextRun {
                    underline: true,
                    strikethrough: false,
                    color: Rgba::opaque(255, 255, 255),
                }],
                layout: Layout {
                    rotation: Angle::from_radians(0.25),
                    ..Layout::default()
                },
                color: Rgba::opaque(255, 255, 255),
                block: Size::new(1.0, 1.0),
                margins: larrygram_text::Insets::default(),
                horz_align: larrygram_text::Align::Left,
                origin: Point::new(0.1, 0.1),
            }),
        };
        let group = Group {
            transform: Transform::identity(),
            clip: None,
            children: vec![Node::Shape(Box::new(shape))],
        };
        let mut scene = Scene::new(1.0, 1.0);
        scene.nodes.push(Node::Group(group));
        assert_eq!(scene.nodes.len(), 1);
        match &scene.nodes[0] {
            Node::Group(group) => match group.children.first() {
                Some(Node::Shape(shape)) => {
                    let text = shape.text.as_ref().expect("text");
                    assert!(text.runs[0].underline);
                    // The rotation lives on the layout (P3-16).
                    assert_eq!(text.layout.rotation.radians(), 0.25);
                    assert_eq!(text.origin, Point::new(0.1, 0.1));
                }
                _ => panic!("expected a shape child"),
            },
            _ => panic!("expected a group"),
        }
    }

    #[test]
    fn paints_hold_their_data() {
        let p = Paint::LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
            stops: vec![GradientStop {
                offset: 0.0,
                color: Rgba::opaque(255, 255, 255),
            }],
        };
        match p {
            Paint::LinearGradient { stops, .. } => assert_eq!(stops.len(), 1),
            _ => panic!("expected a linear gradient"),
        }
        let radial = Paint::RadialGradient {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
            stops: Vec::new(),
        };
        match radial {
            Paint::RadialGradient { radius, .. } => assert_eq!(radius, 1.0),
            _ => panic!("expected a radial gradient"),
        }
        let pat = Paint::Pattern(PatternPaint {
            foreground: Rgba::opaque(0, 0, 0),
            background: Rgba::opaque(255, 255, 255),
            tile: Path::new(),
            tile_size: Size::new(0.1, 0.1),
            filled: true,
            tile_stroke_width: 0.005,
        });
        match pat {
            Paint::Pattern(pattern) => {
                assert!(pattern.filled);
                assert_eq!(pattern.tile_stroke_width, 0.005);
            }
            _ => panic!("expected a pattern"),
        }
        assert_eq!(LineCap::default(), LineCap::Butt);
        assert_eq!(LineJoin::default(), LineJoin::Miter);
    }
}
