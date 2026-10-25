//! The scene builder: model to scene graph through the lookup (ADR 0020).
//!
//! `build_scene` walks one page of a document, resolves every cell through
//! the inheritance lookup with the cached-value policy (ADR 0021), composes
//! the shape hierarchy transforms (ADR 0020) and maps the typed geometry
//! rows to paths (ADR 0022). It never panics: bad values become warnings
//! and defaults.
//!
//! # Policy decisions (the alpha's conventions)
//!
//! Every choice the ADRs leave open is listed here and in the P3-05 Result:
//!
//! - **Flattening tolerance**: `0.25 / scale` inches (0.25 device pixels at
//!   [`BuildOptions::scale`] pixels per inch, 96 by default; ADR 0022).
//! - **Transforms** compose exactly as ADR 0020: `translate(PinX, PinY) ·
//!   rotate(Angle) · scale(FlipX, FlipY) · translate(-LocPinX, -LocPinY)`,
//!   A node's transform is relative to its enclosing `Group` node (the
//!   page for a top-level node), so the backends compose the chain: a
//!   group carries its own transform, a group's own shape node is the
//!   identity inside it, and a child carries its own transform. The
//!   absolute `own.then(parent)` is used only where page coordinates are
//!   needed (the `InfiniteLine` page clip).
//! - **Geometry rows** map through [`Curve::to_cubics`](larrygram_geom::Curve::to_cubics):
//!   `MoveTo` starts a subpath (also mid-section: a stroking gap), the
//!   `Rel*` rows scale by the shape's `Width`/`Height`, `NurbsTo` assembles
//!   its knot list from the formula plus the row's `A`..`D` cells (the
//!   first and last control point), `SplineStart`+`SplineKnot` assemble one
//!   `Spline` per start row, `Ellipse` flattens its conjugate-diameter
//!   parameterization and `InfiniteLine` is clipped to the page rectangle.
//!   A `NurbsTo` whose formula does not parse draws a straight segment to
//!   the row's `X`/`Y` and warns (the ADR 0022 amendment of P3-05).
//! - **Fills**: `FillPattern` 0 draws nothing, 1 is a solid of
//!   `FillForegnd`, 2..24 are the tiles of `render::styles` and 25..40 (or
//!   `FillGradientEnabled`) build a gradient from the `FillGradient` stop
//!   rows (falling back to `FillForegnd` → `FillBkgnd` when the section
//!   holds fewer than two stops). `FillGradientDir` 0 is a linear gradient
//!   along `FillGradientAngle`, 1..7 a radial gradient and 8..13
//!   (rectangular and path gradients) degrade to linear with a warning.
//!   Pattern tiles follow the unit-box convention of `styles.rs`: the tile
//!   is scaled from the unit box to a `spacing × line width` square and
//!   `invert` swaps the two colours.
//! - **Strokes**: `LinePattern` 0 draws no line, other indices take the
//!   dash rhythm of `render::styles` scaled by the line width; `LineCap`
//!   maps 0/1/2 to Round/Square/Butt (the ShapeSheet reference's
//!   Rounded/Square/Extended); `Rounding > 0` becomes a round join (the
//!   alpha does not round the geometry corners); the miter limit is 4.
//! - **Line ends**: `BeginArrow`/`EndArrow` select the arrowhead template
//!   (the unit-box convention: x = 0 the tip at the vertex, x = 1 the tail
//!   along the line) and `*ArrowSize` 0..6 scale it by an arrow size class
//!   of our own (`ARROW_CLASS_IN`, medium = 0.25 in).
//! - **Shadows**: `ShdwPattern` selects the template (alpha, blur factor,
//!   default offset) and `ShapeShdwShow` must be on; the offset comes from
//!   `ShapeShdwOffsetX`/`Y` when either is non-zero and from the template
//!   (× the line width) otherwise, the blur is the template's ×
//!   `ShapeShdwBlur`, and `ShdwForegndTrans` fades the shadow colour.
//! - **Text**: runs come from `TextContent` and the `Character`/`Paragraph`
//!   rows (the `cp`/`pp`/`tp` markers name rows by their `IX`, matched by
//!   `IX` through the lookup, ADR 0019 decision 5), `\n` in literal text
//!   starts a new paragraph and `fld`
//!   markers are skipped (their display text follows as literal text). The
//!   text block uses the shape's own `Txt*` cells when it has any and the
//!   shape box otherwise. Font sizes are stored in inches and converted to
//!   points, `Style` bits are bold/italic/underline (the ShapeSheet
//!   reference), `SpLine` negative is a spacing multiple, and a bullet
//!   renders when `Bullet > 0` (the `BulletStr` cell or a symbol of our own
//!   per style index).
//! - **Layers**: a shape is dropped when it belongs to at least one layer
//!   and every one of those layers is invisible; a shape that names no
//!   layer renders.
//! - **Background pages**: with [`BuildOptions::background`] the page's
//!   background page renders first as a leading [`Group`], once per page
//!   id (a cycle renders one copy).
//! - **Warnings**: values are read only through `CellReader` (ADR 0021 and
//!   its amendment), so `Themed` values and palette indices resolve in the
//!   model. The builder adds warnings of its own for a value it cannot
//!   draw: an unknown `Geometry` row type ([`WarningKind::Missing`]), an
//!   unparsable `NURBS`/`POLYLINE` formula
//!   ([`WarningKind::ErrorValue`]), an undefined style index
//!   ([`WarningKind::Missing`]) and a non-finite cell value
//!   ([`WarningKind::KindMismatch`]). All of them land in
//!   [`Scene::warnings`](crate::scene::Scene::warnings).

use larrygram_geom::{
    Angle, Arc, Cubic, Curve, EllipticalArc, Nurbs, NurbsPoint, Path, Point, Polyline, Segment,
    Size, Spline, Transform,
};
use larrygram_model::{
    CellLookup, CellName, CellReader, CellValue, CellWarning, Color, Document, GeometryRow, PageId,
    RowTarget, ShapeId, SheetRef, TextPart, WarningKind,
};
use larrygram_text::{self as text, default_fonts};

use crate::error::Error;
use crate::scene::{
    GradientStop, Group, LineCap, LineEnd, LineJoin, Node, Paint, PatternPaint, Rgba, Scene,
    Shadow, Shape, Stroke, TextNode, TextRun,
};
use crate::styles::{PatternKind, VisualStyles};

/// The default device scale of a build: 96 device pixels per inch.
pub const DEFAULT_SCALE: f64 = 96.0;

/// The flattening deviation in device pixels (ADR 0022: 0.25 / scale
/// inches).
const FLAT_PIXELS: f64 = 0.25;

/// The recursion cap for groups and background chains (the writer's cap,
/// ADR 0020).
const MAX_DEPTH: usize = 128;

/// The miter limit of every stroke (our own default).
const MITER_LIMIT: f64 = 4.0;

/// The arrow size class in inches, 0 (very small) to 6 (colossal; the
/// `VisArrowSizes` enumeration). Our own scale table: class 2 (medium) is
/// a quarter inch (ADR 0023 leaves the class scale to the builder).
const ARROW_CLASS_IN: [f64; 7] = [0.125, 0.1875, 0.25, 0.375, 0.5, 0.625, 0.75];

/// The arrow size class used when `*ArrowSize` is out of range (medium).
const ARROW_CLASS_DEFAULT: usize = 2;

/// The font size in points used for a run whose `Size` cell is not
/// positive (the `text` crate's own default, ADR 0024).
const DEFAULT_SIZE_PT: f64 = 12.0;

/// The bullet symbol of each `Bullet` style index (1..7; the ShapeSheet
/// reference's round, diamond, square, check box, four diamond, procedure
/// arrow and checkmark bullets). Symbols of our own choosing.
const BULLETS: [&str; 7] = ["•", "◆", "▪", "☐", "❖", "➔", "✔"];

/// Options for building a scene (ADR 0020).
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOptions {
    /// Whether to include the background page (drawn first).
    pub background: bool,
    /// The target device scale in pixels per inch, used for the curve
    /// flattening tolerance (`0.25 / scale` inches, ADR 0022). A value
    /// that is not finite or not positive becomes [`DEFAULT_SCALE`].
    pub scale: f64,
}

impl BuildOptions {
    /// The default options: include the background page, flatten for
    /// [`DEFAULT_SCALE`].
    pub fn new() -> Self {
        Self {
            background: true,
            scale: DEFAULT_SCALE,
        }
    }
}

impl Default for BuildOptions {
    /// [`BuildOptions::new`]: `Default` and `new` agree (ADR 0021
    /// amendment, P3-16).
    fn default() -> Self {
        Self::new()
    }
}

/// Builds the scene graph of one page (ADR 0020).
///
/// Every bad value becomes a warning and a default (ADR 0021); the
/// function errors only for a stale page id or broken committed style
/// data, and never panics (AGENTS.md section 6).
pub fn build_scene(doc: &Document, page: PageId, options: &BuildOptions) -> Result<Scene, Error> {
    let styles = VisualStyles::load()?;
    let scale = if options.scale.is_finite() && options.scale > 0.0 {
        options.scale
    } else {
        DEFAULT_SCALE
    };
    let mut ctx = Ctx::new(doc, styles, FLAT_PIXELS / scale);
    let entity = doc
        .page(page)
        .ok_or(Error::Model(larrygram_model::Error::UnknownPage(page)))?;
    let (width, height) = page_size(&mut ctx, page);
    let mut scene = Scene::new(width, height);

    let mut seen = Vec::new();
    if options.background
        && let Some(background) = entity.background
    {
        seen.push(page);
        if let Some(group) = background_group(&mut ctx, background, &mut seen) {
            scene.nodes.push(Node::Group(group));
        }
    }
    scene.nodes.extend(page_nodes(&mut ctx, page, MAX_DEPTH));
    scene.warnings = ctx.reader.take_warnings();
    scene.warnings.extend(ctx.warnings);
    Ok(scene)
}

/// The state of one build: the document, the style table, the cell reader
/// and the warnings the builder records itself.
struct Ctx<'d> {
    /// The document being built.
    doc: &'d Document,
    /// The committed visual styles (ADR 0023).
    styles: VisualStyles,
    /// The cached-value reader collecting the ADR 0021 warnings.
    reader: CellReader<'d>,
    /// The bundled fonts for text layout (ADR 0024).
    fonts: text::FontStack,
    /// The curve flattening tolerance, in inches (ADR 0022).
    tolerance: f64,
    /// The warnings the builder records itself.
    warnings: Vec<CellWarning>,
}

impl<'d> Ctx<'d> {
    /// A build context over `doc`, flattening to `tolerance` inches.
    fn new(doc: &'d Document, styles: VisualStyles, tolerance: f64) -> Self {
        Self {
            doc,
            styles,
            reader: CellReader::new(doc),
            fonts: default_fonts(),
            tolerance,
            warnings: Vec::new(),
        }
    }

    /// Records one builder warning (a value the policy of ADR 0021 does
    /// not cover).
    fn warn(&mut self, sheet: SheetRef, cell: &str, kind: WarningKind, substituted: CellValue) {
        self.warnings.push(CellWarning {
            sheet,
            cell: CellName::new(cell),
            kind,
            substituted,
        });
    }

    /// Reads a number in canonical units (ADR 0021 policy). The reader
    /// rejects non-finite cached values with one [`WarningKind::KindMismatch`]
    /// warning, so a returned number is always finite.
    fn number(&mut self, sheet: SheetRef, cell: &str) -> f64 {
        let name = CellName::new(cell);
        match self.reader.number(sheet, &name) {
            Ok(value) => value,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                0.0
            }
        }
    }

    /// Reads an angle in radians (ADR 0021 policy).
    fn angle(&mut self, sheet: SheetRef, cell: &str) -> Angle {
        Angle::from_radians(self.number(sheet, cell))
    }

    /// Reads a boolean (ADR 0021 policy).
    fn boolean(&mut self, sheet: SheetRef, cell: &str) -> bool {
        let name = CellName::new(cell);
        match self.reader.boolean(sheet, &name) {
            Ok(flag) => flag,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                false
            }
        }
    }

    /// Reads a colour (ADR 0021 policy; `Themed` and palette indices
    /// resolve in the model).
    fn color(&mut self, sheet: SheetRef, cell: &str) -> Color {
        let name = CellName::new(cell);
        match self.reader.color(sheet, &name) {
            Ok(color) => color,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                Color::new(0, 0, 0)
            }
        }
    }

    /// Reads a colour with its transparency cell into an RGBA value.
    fn rgba(&mut self, sheet: SheetRef, color_cell: &str, trans_cell: &str) -> Rgba {
        let color = self.color(sheet, color_cell);
        let trans = self.number(sheet, trans_cell);
        rgba_of(color, trans)
    }

    /// Reads a number from one row of a repeated section (ADR 0021
    /// policy).
    fn row_number(&mut self, sheet: SheetRef, section: &str, row: RowTarget, cell: &str) -> f64 {
        let name = CellName::new(cell);
        match self.reader.row_number(sheet, section, row, &name) {
            Ok(value) => value,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                0.0
            }
        }
    }

    /// Reads a string from one row of a repeated section (ADR 0021
    /// policy).
    fn row_text(&mut self, sheet: SheetRef, section: &str, row: RowTarget, cell: &str) -> String {
        let name = CellName::new(cell);
        match self.reader.row_text(sheet, section, row, &name) {
            Ok(value) => value,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                String::new()
            }
        }
    }

    /// Reads a boolean from one row of a repeated section (ADR 0021
    /// policy).
    fn row_boolean(&mut self, sheet: SheetRef, section: &str, row: RowTarget, cell: &str) -> bool {
        let name = CellName::new(cell);
        match self.reader.row_boolean(sheet, section, row, &name) {
            Ok(flag) => flag,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                false
            }
        }
    }

    /// Reads a colour from one row of a repeated section (ADR 0021
    /// policy).
    fn row_color(&mut self, sheet: SheetRef, section: &str, row: RowTarget, cell: &str) -> Color {
        let name = CellName::new(cell);
        match self.reader.row_color(sheet, section, row, &name) {
            Ok(color) => color,
            Err(error) => {
                self.warnings.push(CellWarning {
                    sheet,
                    cell: name,
                    kind: WarningKind::Missing,
                    substituted: CellValue::Error(error.to_string()),
                });
                Color::new(0, 0, 0)
            }
        }
    }

    /// Whether any of `cells` resolves from the sheet's own cells (the
    /// `Txt*` placement cells: a shape's own cells override, everything
    /// else falls back to the shape box).
    fn has_local(&self, sheet: SheetRef, cells: &[&str]) -> bool {
        let SheetRef::Shape(shape) = sheet else {
            return false;
        };
        cells.iter().any(|cell| {
            let name = CellName::new(*cell);
            matches!(
                self.doc.resolve(shape, &name),
                Ok(resolved) if resolved.source == larrygram_model::ValueSource::Local
            )
        })
    }

    /// The shape transform of ADR 0020: `translate(PinX, PinY) ·
    /// rotate(Angle) · scale(FlipX, FlipY) · translate(-LocPinX, -LocPinY)`.
    fn shape_transform(&mut self, sheet: SheetRef) -> Transform {
        let pin_x = self.number(sheet, "PinX");
        let pin_y = self.number(sheet, "PinY");
        let angle = self.angle(sheet, "Angle").radians();
        let flip_x = self.boolean(sheet, "FlipX");
        let flip_y = self.boolean(sheet, "FlipY");
        let loc_pin_x = self.number(sheet, "LocPinX");
        let loc_pin_y = self.number(sheet, "LocPinY");
        Transform::translate(Point::new(-loc_pin_x, -loc_pin_y))
            .then(Transform::scale(
                if flip_x { -1.0 } else { 1.0 },
                if flip_y { -1.0 } else { 1.0 },
            ))
            .then(Transform::rotate(Angle::from_radians(angle)))
            .then(Transform::translate(Point::new(pin_x, pin_y)))
    }
}

/// A model colour and a transparency into an RGBA value.
fn rgba_of(color: larrygram_model::Color, trans: f64) -> Rgba {
    let alpha = (1.0 - trans.clamp(0.0, 1.0)) * 255.0;
    Rgba::new(
        color.r,
        color.g,
        color.b,
        alpha.round().clamp(0.0, 255.0) as u8,
    )
}

/// The leading group of a background page (ADR 0020), or `None` for a
/// cycle or a stale id. Every page renders once per chain.
fn background_group(ctx: &mut Ctx<'_>, page: PageId, seen: &mut Vec<PageId>) -> Option<Group> {
    if seen.contains(&page) || seen.len() >= MAX_DEPTH {
        return None;
    }
    seen.push(page);
    let children = if let Some(entity) = ctx.doc.page(page) {
        let nested = entity
            .background
            .and_then(|next| background_group(ctx, next, seen).map(Node::Group));
        let mut nodes = Vec::new();
        if let Some(nested) = nested {
            nodes.push(nested);
        }
        nodes.extend(page_nodes(ctx, page, MAX_DEPTH));
        nodes
    } else {
        Vec::new()
    };
    Some(Group {
        transform: Transform::identity(),
        clip: None,
        children,
    })
}

/// The scene nodes of one page's top-level shapes, in file order.
fn page_nodes(ctx: &mut Ctx<'_>, page: PageId, depth: usize) -> Vec<Node> {
    let Some(entity) = ctx.doc.page(page) else {
        return Vec::new();
    };
    let shapes = entity.shapes.clone();
    let mut nodes = Vec::new();
    for id in shapes {
        if let Some(node) = shape_node(ctx, page, id, Transform::identity(), depth, &mut Vec::new())
        {
            nodes.push(node);
        }
    }
    nodes
}

/// One shape as a scene node: a group for a shape with sub-shapes, a shape
/// node otherwise, and `None` when the shape draws nothing (an invisible
/// layer, an empty geometry without text, a repeat in a cyclic children
/// list).
fn shape_node(
    ctx: &mut Ctx<'_>,
    page: PageId,
    id: ShapeId,
    parent: Transform,
    depth: usize,
    visited: &mut Vec<ShapeId>,
) -> Option<Node> {
    if depth == 0 || visited.contains(&id) {
        return None;
    }
    visited.push(id);
    if hidden_by_layer(ctx, page, id) {
        return None;
    }
    let (children, has_children) = {
        let entity = ctx.doc.shape(id)?;
        (entity.children.clone(), !entity.children.is_empty())
    };
    let local = ctx.shape_transform(SheetRef::Shape(id));
    let absolute = local.then(parent);
    let (path, fill, stroke, shadow, text) = shape_content(ctx, page, id, absolute);

    let mut node = None;
    if !path.is_empty() || text.is_some() {
        // Relative to the enclosing group node (ADR 0020): the group's
        // own shape sits inside the group, which already carries `local`.
        let transform = if has_children {
            Transform::identity()
        } else {
            local
        };
        node = Some(Node::Shape(Box::new(Shape {
            shape_id: Some(id),
            transform,
            path,
            fill,
            stroke,
            shadow,
            clip: None,
            text,
        })));
    }
    if !has_children {
        return node;
    }
    let mut kids = Vec::new();
    if let Some(node) = node {
        kids.push(node);
    }
    for child in children {
        if let Some(node) = shape_node(ctx, page, child, absolute, depth - 1, visited) {
            kids.push(node);
        }
    }
    if kids.is_empty() {
        return None;
    }
    Some(Node::Group(Group {
        transform: local,
        clip: None,
        children: kids,
    }))
}

/// Whether the shape is dropped for being on invisible layers only.
fn hidden_by_layer(ctx: &mut Ctx<'_>, page: PageId, id: ShapeId) -> bool {
    let members = match ctx.doc.shape_layers(id) {
        Ok(members) => members,
        Err(error) => {
            ctx.warnings.push(CellWarning {
                sheet: SheetRef::Shape(id),
                cell: CellName::new("LayerMember"),
                kind: WarningKind::Missing,
                substituted: CellValue::Error(error.to_string()),
            });
            return false;
        }
    };
    if members.is_empty() {
        return false;
    }
    let layers = match ctx.doc.page_layers(page) {
        Ok(layers) => layers,
        Err(error) => {
            ctx.warnings.push(CellWarning {
                sheet: SheetRef::Page(page),
                cell: CellName::new("Layer"),
                kind: WarningKind::Missing,
                substituted: CellValue::Error(error.to_string()),
            });
            return false;
        }
    };
    members.iter().all(|member| {
        layers
            .iter()
            .any(|layer| layer.ix == *member && !layer.visible)
    })
}

/// The path, paint, stroke, shadow and text of one shape.
fn shape_content(
    ctx: &mut Ctx<'_>,
    page: PageId,
    id: ShapeId,
    transform: Transform,
) -> (
    Path,
    Option<Paint>,
    Option<Stroke>,
    Option<Shadow>,
    Option<TextNode>,
) {
    let sheet = SheetRef::Shape(id);
    let width = ctx.number(sheet, "Width").max(0.0);
    let height = ctx.number(sheet, "Height").max(0.0);
    let line_width = ctx.number(sheet, "LineWeight").max(0.0);

    // The typed geometry (ADR 0019) and its row-level warnings.
    let geometry = match ctx.doc.shape_geometry(id) {
        Ok(geometry) => geometry,
        Err(error) => {
            ctx.warnings.push(CellWarning {
                sheet,
                cell: CellName::new("Geometry"),
                kind: WarningKind::Missing,
                substituted: CellValue::Error(error.to_string()),
            });
            Default::default()
        }
    };
    let mut warnings = geometry.warnings;
    let page_rect = page_rect_in_local(ctx, page, transform);

    let mut path = Path::new();
    let mut draw_fill = false;
    let mut draw_line = false;
    for section in &geometry.paths {
        if section.no_show {
            continue;
        }
        draw_fill |= !section.no_fill;
        draw_line |= !section.no_line;
        rows_to_path(
            &section.rows,
            width,
            height,
            ctx.tolerance,
            page_rect,
            sheet,
            &mut warnings,
            &mut path,
        );
    }
    ctx.warnings.append(&mut warnings);

    let fill = if draw_fill {
        build_fill(ctx, sheet, width, height, line_width)
    } else {
        None
    };
    let stroke = if draw_line {
        build_stroke(ctx, sheet)
    } else {
        None
    };
    let shadow = build_shadow(ctx, sheet, line_width);
    let text = build_text(ctx, id, width, height);
    (path, fill, stroke, shadow, text)
}

/// The page size used when a page has no positive `PageWidth` or
/// `PageHeight` (US Letter, portrait, in inches). Files without a page
/// sheet exist in the corpus; a zero-size scene cannot be rasterized.
/// The view commands and the canvas use the same value.
pub const FALLBACK_PAGE_SIZE: (f64, f64) = (8.5, 11.0);

/// The page size in inches. A missing, zero, negative or non-finite
/// `PageWidth` or `PageHeight` takes the [`FALLBACK_PAGE_SIZE`] value (the
/// reader already records a warning for a missing or bad cell).
fn page_size(ctx: &mut Ctx<'_>, page: PageId) -> (f64, f64) {
    let sheet = SheetRef::Page(page);
    let positive = |value: f64, fallback: f64| {
        if value.is_finite() && value > 0.0 {
            value
        } else {
            fallback
        }
    };
    let width = positive(ctx.number(sheet, "PageWidth"), FALLBACK_PAGE_SIZE.0);
    let height = positive(ctx.number(sheet, "PageHeight"), FALLBACK_PAGE_SIZE.1);
    (width, height)
}

/// The page rectangle in the shape's local coordinates (the `InfiniteLine`
/// clip), or `None` when the shape transform cannot be inverted.
fn page_rect_in_local(ctx: &mut Ctx<'_>, page: PageId, transform: Transform) -> Option<[Point; 4]> {
    let (width, height) = page_size(ctx, page);
    let inverse = transform.invert().ok()?;
    Some([
        inverse.apply(Point::new(0.0, 0.0)),
        inverse.apply(Point::new(width, 0.0)),
        inverse.apply(Point::new(width, height)),
        inverse.apply(Point::new(0.0, height)),
    ])
}

// --- geometry rows ---------------------------------------------------

/// The state of a `SplineStart` row awaiting its `SplineKnot` rows.
struct PendingSpline {
    /// The control points: the current point first, then the `SplineStart`
    /// point, then each `SplineKnot` point.
    ctrl: Vec<Point>,
    /// One knot per control point; the final knot is the `SplineStart`
    /// row's `C` cell (ADR 0022, the `geom` knot convention).
    knots: Vec<f64>,
    /// The final knot of the spline (the `SplineStart` row's `C` cell).
    last_knot: f64,
    /// The spline degree (1 to 25).
    degree: i32,
}

/// Maps one `Geometry` section's rows onto `path` (ADR 0022).
///
/// `page_rect` is the page rectangle in shape-local coordinates (for
/// `InfiniteLine`); `warnings` collects the ADR 0021 warnings of rows the
/// builder cannot draw faithfully.
#[allow(clippy::too_many_arguments)] // one row-mapping entry point
fn rows_to_path(
    rows: &[GeometryRow],
    width: f64,
    height: f64,
    tolerance: f64,
    page_rect: Option<[Point; 4]>,
    sheet: SheetRef,
    warnings: &mut Vec<CellWarning>,
    path: &mut Path,
) {
    let mut current: Option<Point> = None;
    let mut open = false;
    let mut spline: Option<PendingSpline> = None;
    for row in rows {
        match row {
            GeometryRow::SplineStart {
                x,
                y,
                second_knot,
                first_knot,
                last_knot,
                degree,
            } => {
                if let Some(pending) = spline.take() {
                    flush_spline(path, tolerance, pending, &mut current, &mut open);
                }
                let start = current.unwrap_or(Point::new(0.0, 0.0));
                spline = Some(PendingSpline {
                    ctrl: vec![start, Point::new(*x, *y)],
                    knots: vec![*first_knot, *second_knot],
                    last_knot: *last_knot,
                    degree: *degree,
                });
                continue;
            }
            GeometryRow::SplineKnot { x, y, knot } => {
                match spline.as_mut() {
                    Some(pending) => {
                        pending.ctrl.push(Point::new(*x, *y));
                        pending.knots.push(*knot);
                    }
                    None => {
                        warn_row(
                            warnings,
                            sheet,
                            "SplineKnot",
                            "a SplineKnot row without a SplineStart row",
                        );
                    }
                }
                continue;
            }
            _ => {}
        }
        if let Some(pending) = spline.take() {
            flush_spline(path, tolerance, pending, &mut current, &mut open);
        }
        match row {
            GeometryRow::MoveTo { x, y } => {
                let to = Point::new(*x, *y);
                path.push(Segment::MoveTo(to));
                current = Some(to);
                open = true;
            }
            GeometryRow::LineTo { x, y } => {
                line_to(path, &mut current, &mut open, Point::new(*x, *y));
            }
            GeometryRow::ArcTo { x, y, bow } => {
                let to = Point::new(*x, *y);
                let start = start_point(&mut current, &mut open, path);
                append_curve(
                    path,
                    &Curve::Arc(Arc {
                        start,
                        end: to,
                        bow: *bow,
                    }),
                    tolerance,
                );
                current = Some(to);
            }
            GeometryRow::EllipticalArcTo {
                x,
                y,
                ctrl,
                angle,
                ratio,
            } => {
                let to = Point::new(*x, *y);
                let start = start_point(&mut current, &mut open, path);
                append_curve(
                    path,
                    &Curve::EllipticalArc(EllipticalArc {
                        start,
                        end: to,
                        ctrl: *ctrl,
                        angle: *angle,
                        ratio: *ratio,
                    }),
                    tolerance,
                );
                current = Some(to);
            }
            GeometryRow::NurbsTo {
                x,
                y,
                second_last_knot,
                last_weight,
                first_knot,
                first_weight,
                data,
            } => {
                let to = Point::new(*x, *y);
                let start = start_point(&mut current, &mut open, path);
                match data {
                    Some(data) => {
                        let mut points = vec![NurbsPoint {
                            x: start.x,
                            y: start.y,
                            knot: *first_knot,
                            weight: *first_weight,
                        }];
                        for point in &data.points {
                            let (x, y) = scale_point(
                                point.x,
                                point.y,
                                data.x_type,
                                data.y_type,
                                width,
                                height,
                            );
                            points.push(NurbsPoint {
                                x,
                                y,
                                knot: point.knot,
                                weight: point.weight,
                            });
                        }
                        points.push(NurbsPoint {
                            x: to.x,
                            y: to.y,
                            knot: *second_last_knot,
                            weight: *last_weight,
                        });
                        append_curve(
                            path,
                            &Curve::Nurbs(Nurbs {
                                degree: data.degree,
                                x_type: data.x_type,
                                y_type: data.y_type,
                                knot_last: data.knot_last,
                                points,
                            }),
                            tolerance,
                        );
                    }
                    None => {
                        warn_formula(warnings, sheet, "E");
                        path.push(Segment::LineTo(to));
                    }
                }
                current = Some(to);
            }
            GeometryRow::PolylineTo { x, y, data } => {
                let to = Point::new(*x, *y);
                let start = start_point(&mut current, &mut open, path);
                match data {
                    Some(data) => {
                        let mut points = data.points.clone();
                        if points.last() != Some(&to) {
                            points.push(to);
                        }
                        if points.first() != Some(&start) {
                            points.insert(0, start);
                        }
                        append_curve(path, &Curve::Polyline(Polyline { points }), tolerance);
                    }
                    None => {
                        warn_formula(warnings, sheet, "A");
                        path.push(Segment::LineTo(to));
                    }
                }
                current = Some(to);
            }
            GeometryRow::Ellipse { center, a, b } => {
                let u = a.sub(*center);
                let v = b.sub(*center);
                for cubic in ellipse_cubics(*center, u, v, tolerance) {
                    if !open {
                        path.push(Segment::MoveTo(cubic.from));
                        open = true;
                    }
                    path.push(Segment::CubicTo {
                        ctrl1: cubic.ctrl1,
                        ctrl2: cubic.ctrl2,
                        to: cubic.to,
                    });
                }
                if open {
                    path.push(Segment::Close);
                }
                current = None;
                open = false;
            }
            GeometryRow::InfiniteLine { p1, p2 } => {
                if let Some(rect) = page_rect
                    && let Some((from, to)) = clip_infinite_line(*p1, *p2, &rect)
                {
                    path.push(Segment::MoveTo(from));
                    path.push(Segment::LineTo(to));
                }
                current = None;
                open = false;
            }
            GeometryRow::RelMoveTo { x, y } => {
                let to = Point::new(*x * width, *y * height);
                path.push(Segment::MoveTo(to));
                current = Some(to);
                open = true;
            }
            GeometryRow::RelLineTo { x, y } => {
                line_to(
                    path,
                    &mut current,
                    &mut open,
                    Point::new(*x * width, *y * height),
                );
            }
            GeometryRow::RelQuadBezTo { x, y, ctrl } => {
                let to = Point::new(*x * width, *y * height);
                let ctrl = Point::new(ctrl.x * width, ctrl.y * height);
                let start = start_point(&mut current, &mut open, path);
                append_curve(
                    path,
                    &Curve::Quad {
                        from: start,
                        ctrl,
                        to,
                    },
                    tolerance,
                );
                current = Some(to);
            }
            GeometryRow::RelCubBezTo { x, y, ctrl1, ctrl2 } => {
                let to = Point::new(*x * width, *y * height);
                let ctrl1 = Point::new(ctrl1.x * width, ctrl1.y * height);
                let ctrl2 = Point::new(ctrl2.x * width, ctrl2.y * height);
                let start = start_point(&mut current, &mut open, path);
                append_curve(
                    path,
                    &Curve::Cubic(Cubic {
                        from: start,
                        ctrl1,
                        ctrl2,
                        to,
                    }),
                    tolerance,
                );
                current = Some(to);
            }
            GeometryRow::RelEllipticalArcTo {
                x,
                y,
                ctrl,
                angle,
                ratio,
            } => {
                let to = Point::new(*x * width, *y * height);
                let ctrl = Point::new(ctrl.x * width, ctrl.y * height);
                let start = start_point(&mut current, &mut open, path);
                append_curve(
                    path,
                    &Curve::EllipticalArc(EllipticalArc {
                        start,
                        end: to,
                        ctrl,
                        angle: *angle,
                        ratio: *ratio,
                    }),
                    tolerance,
                );
                current = Some(to);
            }
            GeometryRow::Unknown { row_type } => {
                warn_row(
                    warnings,
                    sheet,
                    row_type,
                    "a Geometry row type the model does not map",
                );
            }
            _ => {
                warn_row(warnings, sheet, "Geometry", "an unmapped Geometry row");
            }
        }
    }
    if let Some(pending) = spline.take() {
        flush_spline(path, tolerance, pending, &mut current, &mut open);
    }
}

/// One builder warning for a formula-encoded value that does not parse
/// (ADR 0021: the row degrades to a straight segment).
///
/// The kind is [`WarningKind::KindMismatch`], not
/// [`WarningKind::ErrorValue`]: the file's cached value is not an error,
/// the builder cannot use the formula (ADR 0021 amendment, P3-16).
fn warn_formula(warnings: &mut Vec<CellWarning>, sheet: SheetRef, cell: &str) {
    warnings.push(CellWarning {
        sheet,
        cell: CellName::new(cell),
        kind: WarningKind::KindMismatch,
        substituted: CellValue::Str("LineTo".to_owned()),
    });
}

/// One builder warning for a geometry row the builder cannot draw.
fn warn_row(warnings: &mut Vec<CellWarning>, sheet: SheetRef, cell: &str, what: &str) {
    warnings.push(CellWarning {
        sheet,
        cell: CellName::new(cell),
        kind: WarningKind::Missing,
        substituted: CellValue::Str(what.to_owned()),
    });
}

/// The current point of a subpath, starting one at `(0, 0)` when the
/// section draws before its first `MoveTo`.
fn start_point(current: &mut Option<Point>, open: &mut bool, path: &mut Path) -> Point {
    match current {
        Some(point) => *point,
        None => {
            let point = Point::new(0.0, 0.0);
            path.push(Segment::MoveTo(point));
            *open = true;
            *current = Some(point);
            point
        }
    }
}

/// A straight segment to `to`, from the current point.
fn line_to(path: &mut Path, current: &mut Option<Point>, open: &mut bool, to: Point) {
    start_point(current, open, path);
    path.push(Segment::LineTo(to));
    *current = Some(to);
}

/// Appends the cubic approximation of `curve` to `path`.
fn append_curve(path: &mut Path, curve: &Curve, tolerance: f64) {
    let Ok(cubics) = curve.to_cubics(tolerance) else {
        return;
    };
    for cubic in cubics {
        path.push(Segment::CubicTo {
            ctrl1: cubic.ctrl1,
            ctrl2: cubic.ctrl2,
            to: cubic.to,
        });
    }
}

/// Turns a finished pending spline into path segments (ADR 0022 and the
/// `geom` knot convention: one knot per control point plus the final
/// knot).
fn flush_spline(
    path: &mut Path,
    tolerance: f64,
    pending: PendingSpline,
    current: &mut Option<Point>,
    open: &mut bool,
) {
    let end = pending.ctrl.last().copied();
    if pending.ctrl.len() >= 2 {
        let knots: Vec<f64> = pending
            .knots
            .iter()
            .copied()
            .chain(std::iter::once(pending.last_knot))
            .collect();
        append_curve(
            path,
            &Curve::Spline(Spline {
                degree: pending.degree.max(1) as u32,
                ctrl: pending.ctrl,
                knots,
            }),
            tolerance,
        );
    }
    if let Some(end) = end {
        *current = Some(end);
        *open = true;
    }
}

/// Scales one NURBS control point from the formula's `xType`/`yType`
/// (0 = fraction of the shape's size, 1 = local; ADR 0022).
fn scale_point(x: f64, y: f64, x_type: u32, y_type: u32, width: f64, height: f64) -> (f64, f64) {
    (
        if x_type == 0 { x * width } else { x },
        if y_type == 0 { y * height } else { y },
    )
}

/// The cubics of the ellipse `{ center + u·cos t + v·sin t }` (the
/// `Ellipse` row's conjugate semi-diameters, ADR 0022), within
/// `tolerance` inches of the true ellipse.
fn ellipse_cubics(center: Point, u: Point, v: Point, tolerance: f64) -> Vec<Cubic> {
    let radius =
        u.x.abs()
            .max(u.y.abs())
            .max(v.x.abs())
            .max(v.y.abs())
            .max((u.x * u.x + u.y * u.y).sqrt())
            .max((v.x * v.x + v.y * v.y).sqrt());
    if !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    // The quarter-arc cubic approximation errs by about 2.7e-4 of the
    // radius; the error scales with the sixth power of the piece sweep
    // (the same bound `geom` uses for circular arcs, ADR 0022).
    let quarter_error = 2.7e-4;
    let mut segments = 4usize;
    let step = loop {
        let step = core::f64::consts::TAU / segments as f64;
        let error = radius * quarter_error * (step / core::f64::consts::FRAC_PI_2).powi(6);
        if error <= tolerance || segments >= 256 {
            break step;
        }
        segments += 4;
    };
    let scale = (4.0 / 3.0) * (step / 4.0).tan();
    let mut cubics = Vec::with_capacity(segments);
    for index in 0..segments {
        let t0 = step * index as f64;
        let t1 = t0 + step;
        let (s0, c0) = t0.sin_cos();
        let (s1, c1) = t1.sin_cos();
        let p0 = Point::new(
            center.x + u.x * c0 + v.x * s0,
            center.y + u.y * c0 + v.y * s0,
        );
        let p1 = Point::new(
            center.x + u.x * c1 + v.x * s1,
            center.y + u.y * c1 + v.y * s1,
        );
        let d0 = Point::new(-u.x * s0 + v.x * c0, -u.y * s0 + v.y * c0);
        let d1 = Point::new(-u.x * s1 + v.x * c1, -u.y * s1 + v.y * c1);
        cubics.push(Cubic {
            from: p0,
            ctrl1: Point::new(p0.x + d0.x * scale, p0.y + d0.y * scale),
            ctrl2: Point::new(p1.x - d1.x * scale, p1.y - d1.y * scale),
            to: p1,
        });
    }
    cubics
}

/// Clips the infinite line through `p1` and `p2` to the convex polygon
/// `rect` (the page rectangle in shape-local coordinates), returning the
/// segment inside it (ADR 0022: `InfiniteLine` renders clipped to the
/// page).
fn clip_infinite_line(p1: Point, p2: Point, rect: &[Point; 4]) -> Option<(Point, Point)> {
    if !finite_point(p1) || !finite_point(p2) {
        return None;
    }
    let dir = p2.sub(p1);
    if dir.x == 0.0 && dir.y == 0.0 {
        return None;
    }
    let mut poly = *rect;
    let area = (0..4)
        .map(|i| {
            let next = (i + 1) % 4;
            poly[i].x * poly[next].y - poly[next].x * poly[i].y
        })
        .sum::<f64>();
    if area < 0.0 {
        poly.reverse();
    }
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    for i in 0..4 {
        let next = (i + 1) % 4;
        let edge = poly[next].sub(poly[i]);
        let offset = edge.x * (p1.y - poly[i].y) - edge.y * (p1.x - poly[i].x);
        let slope = edge.x * dir.y - edge.y * dir.x;
        if slope == 0.0 {
            if offset < 0.0 {
                return None;
            }
        } else {
            let t = -offset / slope;
            if slope > 0.0 {
                low = low.max(t);
            } else {
                high = high.min(t);
            }
        }
    }
    if !low.is_finite() || !high.is_finite() || low > high {
        return None;
    }
    let from = Point::new(p1.x + dir.x * low, p1.y + dir.y * low);
    let to = Point::new(p1.x + dir.x * high, p1.y + dir.y * high);
    Some((from, to))
}

/// Whether both coordinates are finite.
fn finite_point(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

// --- paint, stroke, shadow -------------------------------------------

/// The fill paint of one shape (ADR 0023), or `None` for no fill.
fn build_fill(
    ctx: &mut Ctx<'_>,
    sheet: SheetRef,
    width: f64,
    height: f64,
    line_width: f64,
) -> Option<Paint> {
    let index = ctx.number(sheet, "FillPattern").round();
    if index == 0.0 {
        return None;
    }
    let gradient = ctx.boolean(sheet, "FillGradientEnabled") || (25.0..=40.0).contains(&index);
    if gradient {
        return Some(build_gradient(ctx, sheet, width, height));
    }
    let index = u32::try_from(index as i64).unwrap_or(0);
    let template = match ctx.styles.fill_pattern(index) {
        Some(template) => template,
        None => {
            warn_style(ctx, sheet, "FillPattern", index);
            return None;
        }
    };
    match template.kind {
        PatternKind::None => None,
        PatternKind::Solid => Some(Paint::Solid(ctx.rgba(
            sheet,
            "FillForegnd",
            "FillForegndTrans",
        ))),
        PatternKind::Tile => {
            let mut foreground = ctx.rgba(sheet, "FillForegnd", "FillForegndTrans");
            let mut background = ctx.rgba(sheet, "FillBkgnd", "FillBkgndTrans");
            if template.invert {
                std::mem::swap(&mut foreground, &mut background);
            }
            let unit = if line_width > 0.0 { line_width } else { 0.01 };
            let pitch = template.spacing * unit;
            Some(Paint::Pattern(PatternPaint {
                foreground,
                background,
                tile: scale_path(&template.tile, pitch, pitch),
                tile_size: Size::new(pitch, pitch),
                filled: template.filled,
                // The template's stroke width is a fraction of the tile
                // box side (the unit-box convention of `styles.rs`).
                tile_stroke_width: template.tile_stroke_width * pitch,
            }))
        }
    }
}

/// The gradient paint of one shape, from the `FillGradient` stop rows
/// (falling back to `FillForegnd` → `FillBkgnd`).
fn build_gradient(ctx: &mut Ctx<'_>, sheet: SheetRef, width: f64, height: f64) -> Paint {
    let mut stops = Vec::new();
    let rows = section_rows(ctx, sheet, "FillGradient");
    for row in 0..rows {
        let target = RowTarget::at(row);
        let offset = ctx.row_number(sheet, "FillGradient", target, "GradientStopPosition");
        let color = ctx.row_color(sheet, "FillGradient", target, "GradientStopColor");
        let trans = ctx.row_number(sheet, "FillGradient", target, "GradientStopColorTrans");
        stops.push(GradientStop {
            offset: offset.clamp(0.0, 1.0),
            color: rgba_of(color, trans),
        });
    }
    if stops.len() < 2 {
        stops.clear();
        stops.push(GradientStop {
            offset: 0.0,
            color: ctx.rgba(sheet, "FillForegnd", "FillForegndTrans"),
        });
        stops.push(GradientStop {
            offset: 1.0,
            color: ctx.rgba(sheet, "FillBkgnd", "FillBkgndTrans"),
        });
    } else {
        stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    }
    let direction = ctx.number(sheet, "FillGradientDir").round();
    if (8.0..=13.0).contains(&direction) {
        // Rectangular and path gradients have no scene form yet; the
        // linear gradient keeps the colours in place (ADR 0020).
        ctx.warn(
            sheet,
            "FillGradientDir",
            WarningKind::Missing,
            CellValue::Number(direction),
        );
    }
    if (1.0..=7.0).contains(&direction) {
        return Paint::RadialGradient {
            center: Point::new(width / 2.0, height / 2.0),
            radius: 0.5 * (width * width + height * height).sqrt(),
            stops,
        };
    }
    let angle = ctx.number(sheet, "FillGradientAngle");
    let (sin, cos) = angle.sin_cos();
    let half = (width * cos.abs() + height * sin.abs()) / 2.0;
    let center = Point::new(width / 2.0, height / 2.0);
    Paint::LinearGradient {
        start: Point::new(center.x - cos * half, center.y - sin * half),
        end: Point::new(center.x + cos * half, center.y + sin * half),
        stops,
    }
}

/// The number of rows of one repeated section (the shape's own rows, the
/// master chain's otherwise).
///
/// The master shapes are the ones the cell lookup resolves through
/// ([`Document::master_chain_shapes`], the ADR 0013 rungs), so the row
/// count and the row cells come from the same master shape; the walk is
/// bounded by the builder's [`MAX_DEPTH`] like every other recursion.
fn section_rows(ctx: &Ctx<'_>, sheet: SheetRef, section: &str) -> usize {
    let SheetRef::Shape(shape) = sheet else {
        return 0;
    };
    let mut names = vec![section];
    if section == "FillGradient" {
        names.push("Fill Gradient");
    }
    let count = |entity: &larrygram_model::Shape| {
        names.iter().find_map(|name| {
            entity
                .sheet
                .sections
                .iter()
                .find(|found| found.name.as_str() == *name && !found.rows.is_empty())
                .map(|found| found.rows.len())
        })
    };
    if let Some(entity) = ctx.doc.shape(shape)
        && let Some(rows) = count(entity)
    {
        return rows;
    }
    for id in ctx.doc.master_chain_shapes(shape, MAX_DEPTH) {
        if let Some(entity) = ctx.doc.shape(id)
            && let Some(rows) = count(entity)
        {
            return rows;
        }
    }
    0
}

/// One builder warning for a style index the data file does not define
/// (the no-op style is used; ADR 0021, ADR 0023).
fn warn_style(ctx: &mut Ctx<'_>, sheet: SheetRef, cell: &str, index: u32) {
    ctx.warn(
        sheet,
        cell,
        WarningKind::Missing,
        CellValue::Number(f64::from(index)),
    );
}

/// Scales a unit-box path into a `sx` × `sy` box at the origin.
fn scale_path(path: &Path, sx: f64, sy: f64) -> Path {
    let scale = |point: Point| Point::new(point.x * sx, point.y * sy);
    let mut out = Path::new();
    for segment in path.segments() {
        out.push(match segment {
            Segment::MoveTo(point) => Segment::MoveTo(scale(*point)),
            Segment::LineTo(point) => Segment::LineTo(scale(*point)),
            Segment::QuadTo { ctrl, to } => Segment::QuadTo {
                ctrl: scale(*ctrl),
                to: scale(*to),
            },
            Segment::CubicTo { ctrl1, ctrl2, to } => Segment::CubicTo {
                ctrl1: scale(*ctrl1),
                ctrl2: scale(*ctrl2),
                to: scale(*to),
            },
            Segment::Close => Segment::Close,
            other => *other,
        });
    }
    out
}

/// The stroke of one shape (ADR 0023), or `None` for an invisible line.
fn build_stroke(ctx: &mut Ctx<'_>, sheet: SheetRef) -> Option<Stroke> {
    let index = ctx.number(sheet, "LinePattern").round();
    if index == 0.0 {
        return None;
    }
    let width = ctx.number(sheet, "LineWeight").max(0.0);
    let dash = match ctx
        .styles
        .line_pattern(u32::try_from(index as i64).unwrap_or(0))
    {
        Some(template) if template.invisible => return None,
        Some(template) => template
            .dashes
            .iter()
            .map(|dash| dash * width)
            .collect::<Vec<f64>>(),
        None => {
            warn_style(
                ctx,
                sheet,
                "LinePattern",
                u32::try_from(index as i64).unwrap_or(0),
            );
            Vec::new()
        }
    };
    let cap = match ctx.number(sheet, "LineCap").round() as i32 {
        0 => LineCap::Round,
        1 => LineCap::Square,
        _ => LineCap::Butt,
    };
    let join = if ctx.number(sheet, "Rounding") > 0.0 {
        LineJoin::Round
    } else {
        LineJoin::Miter
    };
    let start = line_end(ctx, sheet, true);
    let end = line_end(ctx, sheet, false);
    Some(Stroke {
        paint: Paint::Solid(ctx.rgba(sheet, "LineColor", "LineColorTrans")),
        width,
        dash,
        cap,
        join,
        miter_limit: MITER_LIMIT,
        start,
        end,
    })
}

/// One line end (arrowhead) of a stroke: the template in the unit box,
/// scaled by the `*ArrowSize` class (ADR 0020, ADR 0023).
fn line_end(ctx: &mut Ctx<'_>, sheet: SheetRef, begin: bool) -> Option<LineEnd> {
    let (cell, size_cell) = if begin {
        ("BeginArrow", "BeginArrowSize")
    } else {
        ("EndArrow", "EndArrowSize")
    };
    let index = ctx.number(sheet, cell).round();
    if index == 0.0 {
        return None;
    }
    let template = match ctx
        .styles
        .line_end(u32::try_from(index as i64).unwrap_or(0))
    {
        Some(template) if !template.shape.is_empty() => template,
        Some(_) => return None,
        None => {
            warn_style(ctx, sheet, cell, u32::try_from(index as i64).unwrap_or(0));
            return None;
        }
    };
    let class = ctx.number(sheet, size_cell).round();
    let class = if (0.0..=6.0).contains(&class) {
        class as usize
    } else {
        warn_style(ctx, sheet, size_cell, class.max(0.0) as u32);
        ARROW_CLASS_DEFAULT
    };
    let size = ARROW_CLASS_IN[class];
    Some(LineEnd {
        shape: template.shape,
        width: template.width * size,
        length: template.length * size,
        filled: template.filled,
        color: None,
    })
}

/// The drop shadow of one shape (ADR 0023), or `None` for none.
fn build_shadow(ctx: &mut Ctx<'_>, sheet: SheetRef, line_width: f64) -> Option<Shadow> {
    if !ctx.boolean(sheet, "ShapeShdwShow") {
        return None;
    }
    let index = ctx.number(sheet, "ShdwPattern").round();
    let template = match ctx.styles.shadow(u32::try_from(index as i64).unwrap_or(0)) {
        Some(template) if template.alpha > 0.0 => template,
        Some(_) => return None,
        None => {
            warn_style(
                ctx,
                sheet,
                "ShdwPattern",
                u32::try_from(index as i64).unwrap_or(0),
            );
            return None;
        }
    };
    let offset_x = ctx.number(sheet, "ShapeShdwOffsetX");
    let offset_y = ctx.number(sheet, "ShapeShdwOffsetY");
    let unit = if line_width > 0.0 { line_width } else { 0.01 };
    let (dx, dy) = if offset_x != 0.0 || offset_y != 0.0 {
        (offset_x, offset_y)
    } else {
        (template.dx * unit, template.dy * unit)
    };
    let blur = template.blur * ctx.number(sheet, "ShapeShdwBlur").max(0.0);
    let mut color = ctx.rgba(sheet, "ShdwForegnd", "ShdwForegndTrans");
    let alpha = f64::from(color.a) * template.alpha;
    color.a = alpha.round().clamp(0.0, 255.0) as u8;
    Some(Shadow {
        color,
        offset: Point::new(dx, dy),
        blur,
    })
}

// --- text ------------------------------------------------------------

/// The placement of one shape's text block: the box-local to shape-local
/// transform, the box size and the block angle (radians, relative to the
/// shape).
struct TextPlacement {
    /// The box-local (y up, `(0, 0)` bottom-left) to shape-local
    /// transform.
    transform: Transform,
    /// The box width, in inches.
    width: f64,
    /// The box height, in inches.
    height: f64,
    /// The block rotation relative to the shape, in radians.
    angle: f64,
}

/// The cells of the `Text Transform` section (the file spellings).
const TXT_CELLS: [&str; 7] = [
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "TxtWidth",
    "TxtHeight",
    "TxtAngle",
];

/// The text placement of one shape: the shape's own `Txt*` cells when it
/// has any, the shape box otherwise (the `Txt*` defaults, ADR 0024).
fn text_placement(ctx: &mut Ctx<'_>, sheet: SheetRef, width: f64, height: f64) -> TextPlacement {
    if ctx.has_local(sheet, &TXT_CELLS) {
        let pin_x = ctx.number(sheet, "TxtPinX");
        let pin_y = ctx.number(sheet, "TxtPinY");
        let loc_pin_x = ctx.number(sheet, "TxtLocPinX");
        let loc_pin_y = ctx.number(sheet, "TxtLocPinY");
        let box_width = ctx.number(sheet, "TxtWidth").max(0.0);
        let box_height = ctx.number(sheet, "TxtHeight").max(0.0);
        let angle = ctx.number(sheet, "TxtAngle");
        return TextPlacement {
            transform: Transform::translate(Point::new(-loc_pin_x, -loc_pin_y))
                .then(Transform::rotate(Angle::from_radians(angle)))
                .then(Transform::translate(Point::new(pin_x, pin_y))),
            width: if box_width > 0.0 { box_width } else { width },
            height: if box_height > 0.0 { box_height } else { height },
            angle,
        };
    }
    TextPlacement {
        transform: Transform::identity(),
        width,
        height,
        angle: 0.0,
    }
}

/// One text paragraph under construction: its runs as (text, character
/// row) pairs and the `Paragraph` row that formats it.
struct ParaDraft {
    /// The runs, in order.
    runs: Vec<(String, RowTarget)>,
    /// The formatting `Paragraph` row (named by its `IX`).
    para_row: RowTarget,
}

/// The text block of one shape (ADR 0020, ADR 0024), or `None` for no
/// text.
fn build_text(ctx: &mut Ctx<'_>, id: ShapeId, width: f64, height: f64) -> Option<TextNode> {
    let content = ctx.doc.shape(id)?.text.as_ref()?;
    if content.is_empty() {
        return None;
    }
    let sheet = SheetRef::Shape(id);
    let placement = text_placement(ctx, sheet, width, height);

    // The text parts to paragraphs of runs (ADR 0019).
    let mut drafts: Vec<ParaDraft> = vec![ParaDraft {
        runs: Vec::new(),
        para_row: RowTarget::at(0),
    }];
    let mut char_row = RowTarget::at(0);
    let mut para_row = RowTarget::at(0);
    for part in &content.parts {
        match part {
            TextPart::Text(literal) => {
                for (index, piece) in literal.split('\n').enumerate() {
                    if index > 0 {
                        drafts.push(ParaDraft {
                            runs: Vec::new(),
                            para_row,
                        });
                    }
                    if piece.is_empty() {
                        continue;
                    }
                    if let Some(draft) = drafts.last_mut() {
                        match draft.runs.last_mut() {
                            Some(last) if last.1 == char_row => last.0.push_str(piece),
                            _ => draft.runs.push((piece.to_owned(), char_row)),
                        }
                    }
                }
            }
            TextPart::CharRun { ix } => {
                // A `cp` marker names its row by `IX` (ADR 0019 decision
                // 5): the row is matched by `IX` in the sheet and in the
                // master chain, never by position.
                char_row = RowTarget::by_ix(marker_ix(*ix));
            }
            TextPart::ParaRun { ix } => {
                para_row = RowTarget::by_ix(marker_ix(*ix));
                drafts.push(ParaDraft {
                    runs: Vec::new(),
                    para_row,
                });
            }
            TextPart::TabsRun { .. } => {
                // A `tp` marker names a `Tabs` row; `tabs_of` reads the
                // stops in row order and does not single markers out.
            }
            TextPart::Field { .. } => {}
            _ => {}
        }
    }

    // The runs' styles and the paragraphs for `text::layout`.
    let mut runs: Vec<TextRun> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    let mut bullets: Vec<Option<usize>> = Vec::new();
    let mut paragraphs: Vec<text::Paragraph<'_>> = Vec::new();
    for draft in &drafts {
        offsets.push(runs.len());
        let mut text_runs = Vec::new();
        let mut first_size = DEFAULT_SIZE_PT;
        let mut first_family = String::new();
        for (literal, row) in &draft.runs {
            let (style, format) = run_style(ctx, sheet, *row);
            if first_family.is_empty() {
                first_family = format.family.clone();
            }
            if text_runs.is_empty() {
                first_size = format.size;
            }
            runs.push(style);
            text_runs.push(text::Run {
                text: literal.as_str(),
                format,
            });
        }
        let bullet = paragraph_bullet(ctx, sheet, draft.para_row, &first_family, first_size);
        bullets.push(if bullet.is_some() {
            let style = bullet.as_ref().map(|(style, _)| *style);
            runs.push(style.unwrap_or(TextRun {
                underline: false,
                strikethrough: false,
                color: Rgba::opaque(0, 0, 0),
            }));
            Some(runs.len() - 1)
        } else {
            None
        });
        paragraphs.push(paragraph_of(
            ctx,
            sheet,
            draft.para_row,
            text_runs,
            bullet.map(|(_, bullet)| bullet),
            first_size,
        ));
    }

    let margins = text::Insets {
        left: ctx.number(sheet, "LeftMargin").max(0.0),
        right: ctx.number(sheet, "RightMargin").max(0.0),
        top: ctx.number(sheet, "TopMargin").max(0.0),
        bottom: ctx.number(sheet, "BottomMargin").max(0.0),
    };
    let valign = match ctx.number(sheet, "VerticalAlign").round() as i32 {
        1 => text::VAlign::Middle,
        2 => text::VAlign::Bottom,
        _ => text::VAlign::Top,
    };
    let block = text::Block {
        paragraphs,
        width: placement.width,
        height: placement.height,
        valign,
        margins,
        // `text::layout` rotates in its y-down space (ADR 0024
        // amendment): a shape-local angle of +a is a layout angle of -a.
        rotation: Angle::from_radians(-placement.angle),
        wrap: true,
    };
    let mut layout = match text::layout(&block, &ctx.fonts) {
        Ok(layout) => layout,
        Err(error) => {
            ctx.warn(
                sheet,
                "Text",
                WarningKind::KindMismatch,
                CellValue::Error(error.to_string()),
            );
            return None;
        }
    };

    // The layout's run indices are per paragraph; the scene's are global
    // (and bullet glyphs get their own entry).
    for line in &mut layout.lines {
        for glyph in &mut line.glyphs {
            let target = if glyph.run == text::BULLET_RUN {
                bullets.get(line.paragraph).copied().flatten()
            } else {
                offsets
                    .get(line.paragraph)
                    .map(|offset| offset.saturating_add(glyph.run))
            };
            if let Some(target) = target {
                glyph.run = target;
            }
        }
    }

    let color = runs
        .first()
        .map(|run| run.color)
        .unwrap_or_else(|| Rgba::opaque(0, 0, 0));
    Some(TextNode {
        fonts: ctx.fonts.fonts().to_vec(),
        runs,
        layout,
        color,
        origin: placement.transform.apply(Point::new(0.0, placement.height)),
    })
}

/// A run marker's `IX` as a row `IX`; a marker beyond `i32` matches no
/// row and substitutes the defaults with one warning.
fn marker_ix(ix: u32) -> i32 {
    i32::try_from(ix).unwrap_or(i32::MAX)
}

/// The style of one text run from its `Character` row (ADR 0019, the
/// ShapeSheet reference).
fn run_style(ctx: &mut Ctx<'_>, sheet: SheetRef, row: RowTarget) -> (TextRun, text::Format) {
    let mut family = ctx.row_text(sheet, "Character", row, "Font");
    if family.is_empty() {
        family = theme_minor_latin(ctx.doc);
    }
    let size_in = ctx.row_number(sheet, "Character", row, "Size");
    let size = if size_in > 0.0 {
        size_in * 72.0
    } else {
        DEFAULT_SIZE_PT
    };
    let style = ctx.row_number(sheet, "Character", row, "Style").round() as i32;
    let bold = style & 1 != 0;
    let italic = style & 2 != 0;
    let underline = style & 4 != 0 || ctx.row_boolean(sheet, "Character", row, "DblUnderline");
    let strikethrough = ctx.row_boolean(sheet, "Character", row, "Strikethru")
        || ctx.row_boolean(sheet, "Character", row, "DoubleStrikethrough");
    let color = ctx.row_rgba(sheet, "Character", row, "Color", "ColorTrans");
    (
        TextRun {
            underline,
            strikethrough,
            color,
        },
        text::Format {
            family,
            size,
            bold,
            italic,
            underline,
            strikethrough,
        },
    )
}

/// The theme's minor latin font family, or the empty string when the
/// document has no theme fonts (ADR 0024).
fn theme_minor_latin(doc: &Document) -> String {
    doc.themes
        .first()
        .and_then(|theme| theme.fonts.as_ref())
        .map(|fonts| fonts.minor_latin.clone())
        .unwrap_or_default()
}

/// The bullet of one paragraph (`Bullet > 0`), as its scene style and its
/// layout bullet.
fn paragraph_bullet(
    ctx: &mut Ctx<'_>,
    sheet: SheetRef,
    row: RowTarget,
    family: &str,
    size: f64,
) -> Option<(TextRun, text::Bullet)> {
    let index = ctx.row_number(sheet, "Paragraph", row, "Bullet").round();
    if index < 0.5 {
        return None;
    }
    let mut literal = ctx.row_text(sheet, "Paragraph", row, "BulletStr");
    if literal.is_empty() {
        literal = BULLETS
            .get(index as usize - 1)
            .copied()
            .unwrap_or(BULLETS[0])
            .to_owned();
    }
    let bullet_size = ctx.row_number(sheet, "Paragraph", row, "BulletFontSize");
    let size = if bullet_size > 0.0 { bullet_size } else { size };
    let indent = ctx.row_number(sheet, "Paragraph", row, "TextPosAfterBullet");
    let color = ctx.row_rgba(sheet, "Character", row, "Color", "ColorTrans");
    Some((
        TextRun {
            underline: false,
            strikethrough: false,
            color,
        },
        text::Bullet {
            text: literal,
            family: family.to_owned(),
            size,
            indent,
        },
    ))
}

/// One paragraph for `text::layout` from its `Paragraph` row (ADR 0019,
/// the ShapeSheet reference).
fn paragraph_of<'a>(
    ctx: &mut Ctx<'_>,
    sheet: SheetRef,
    row: RowTarget,
    runs: Vec<text::Run<'a>>,
    bullet: Option<text::Bullet>,
    first_size: f64,
) -> text::Paragraph<'a> {
    let align = match ctx.row_number(sheet, "Paragraph", row, "HorzAlign").round() as i32 {
        1 => text::Align::Center,
        2 => text::Align::Right,
        3 => text::Align::Justify,
        _ => text::Align::Left,
    };
    let spacing = ctx.row_number(sheet, "Paragraph", row, "SpLine");
    let line_spacing = if spacing < 0.0 {
        -spacing
    } else if spacing == 0.0 {
        1.0
    } else if first_size > 0.0 {
        spacing / first_size
    } else {
        1.0
    };
    let space_before = ctx.row_number(sheet, "Paragraph", row, "SpBefore");
    let space_after = ctx.row_number(sheet, "Paragraph", row, "SpAfter");
    let indent_first = ctx.row_number(sheet, "Paragraph", row, "IndFirst");
    let indent_left = ctx.row_number(sheet, "Paragraph", row, "IndLeft");
    let tabs = tabs_of(ctx, sheet);
    text::Paragraph {
        runs,
        align,
        line_spacing,
        space_before,
        space_after,
        indent_first,
        indent_left,
        bullet,
        tabs,
    }
}

/// The tab stops of one sheet's `Tabs` section (one stop per row's
/// `Position` cell; the row a `tp` marker names is not distinguished).
fn tabs_of(ctx: &mut Ctx<'_>, sheet: SheetRef) -> Vec<f64> {
    let rows = section_rows(ctx, sheet, "Tabs");
    (0..rows)
        .map(|row| ctx.row_number(sheet, "Tabs", RowTarget::at(row), "Position"))
        .filter(|position| position.is_finite())
        .collect()
}

impl Ctx<'_> {
    /// Reads a colour and its transparency from one section row.
    fn row_rgba(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        color_cell: &str,
        trans_cell: &str,
    ) -> Rgba {
        let color = self.row_color(sheet, section, row, color_cell);
        let trans = self.row_number(sheet, section, row, trans_cell);
        rgba_of(color, trans)
    }
}

#[cfg(test)]
mod tests {
    use larrygram_model::{
        Cell, CellValue as Value, Master, MasterId, Page, Row, Section, SectionName,
        Shape as ModelShape, TextContent, TextPart,
    };

    use super::*;

    /// A cell with a cached number.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(Value::Number(value));
        cell
    }

    /// A cell with a cached string.
    fn string_cell(name: &str, value: &str) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(Value::Str(value.to_owned()));
        cell
    }

    /// A one-page document with one empty shape.
    fn document_with_shape() -> (Document, PageId, ShapeId) {
        let mut doc = Document::new();
        let shape = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 1));
        let page = PageId { index: 0 };
        let mut entity = Page::new(page, 1);
        entity.shapes.push(shape);
        doc.pages.push(entity);
        (doc, page, shape)
    }

    /// Adds one `Geometry` section of `rows` to a shape.
    fn with_geometry(doc: &mut Document, shape: ShapeId, rows: Vec<Row>) {
        let mut section = Section::new(SectionName::new("Geometry"));
        section.rows = rows;
        if let Some(entity) = doc.shape_mut(shape) {
            entity.sheet.sections.push(section);
        }
    }

    /// One typed geometry row with numeric cells.
    fn grow(row_type: &str, cells: &[(&str, f64)]) -> Row {
        let mut row = Row::new();
        row.row_type = Some(row_type.to_owned());
        row.cells = cells
            .iter()
            .map(|(name, value)| number_cell(name, *value))
            .collect();
        row
    }

    /// The scene of the document's first page with the default options.
    fn build(doc: &Document) -> Scene {
        build_scene(doc, PageId { index: 0 }, &BuildOptions::new()).expect("builds")
    }

    // --- options and transforms --------------------------------------

    #[test]
    fn a_page_without_a_size_falls_back_to_letter() {
        // Regression: corpus files without a page sheet built a 0 x 0
        // scene, which the PNG backend rejects.
        let (doc, _, _) = document_with_shape();
        let scene = build(&doc);
        assert_eq!((scene.width, scene.height), (8.5, 11.0));
    }

    #[test]
    fn a_zero_page_size_falls_back_per_axis() {
        let (mut doc, _, _) = document_with_shape();
        if let Some(entity) = doc.pages.first_mut() {
            entity.sheet.cells.push(number_cell("PageWidth", 0.0));
            entity.sheet.cells.push(number_cell("PageHeight", 4.0));
        }
        let scene = build(&doc);
        assert_eq!((scene.width, scene.height), (8.5, 4.0));
    }

    #[test]
    fn new_options_include_the_background() {
        assert!(BuildOptions::new().background);
        assert_eq!(BuildOptions::new().scale, 96.0);
        // `Default` agrees with `new` (P3-16): both include the
        // background page.
        assert!(BuildOptions::default().background);
        assert_eq!(BuildOptions::default().scale, 96.0);
        assert_eq!(BuildOptions::default(), BuildOptions::new());
    }

    #[test]
    fn build_scene_reports_a_stale_page() {
        let doc = Document::new();
        let error = build_scene(&doc, PageId { index: 3 }, &BuildOptions::new());
        assert_eq!(
            error,
            Err(Error::Model(larrygram_model::Error::UnknownPage(PageId {
                index: 3
            })))
        );
    }

    #[test]
    fn transform_composition_matches_the_adr_formula() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("PinX", 3.0),
                number_cell("PinY", 4.0),
                number_cell("LocPinX", 1.0),
                number_cell("LocPinY", 2.0),
                number_cell("Width", 2.0),
                number_cell("Height", 4.0),
            ];
        }
        let mut ctx = Ctx::new(&doc, VisualStyles::load().expect("styles"), 0.01);
        let transform = ctx.shape_transform(SheetRef::Shape(shape));
        // translate(3, 4) · rotate(0) · scale(1, 1) · translate(-1, -2):
        // (0, 0) -> (2, 2), (1, 2) -> (3, 4), (2, 4) -> (4, 6).
        assert_eq!(transform.apply(Point::new(0.0, 0.0)), Point::new(2.0, 2.0));
        assert_eq!(transform.apply(Point::new(1.0, 2.0)), Point::new(3.0, 4.0));
        assert_eq!(transform.apply(Point::new(2.0, 4.0)), Point::new(4.0, 6.0));
    }

    #[test]
    fn transform_rotation_and_flips_follow_the_adr_formula() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("PinX", 0.0),
                number_cell("PinY", 0.0),
                number_cell("Angle", core::f64::consts::FRAC_PI_2),
                number_cell("FlipX", 1.0),
                number_cell("LocPinX", 0.0),
                number_cell("LocPinY", 0.0),
            ];
        }
        let mut ctx = Ctx::new(&doc, VisualStyles::load().expect("styles"), 0.01);
        let transform = ctx.shape_transform(SheetRef::Shape(shape));
        // rotate(90°) · scale(-1, 1): (1, 0) -> (0, -1).
        let at = transform.apply(Point::new(1.0, 0.0));
        assert!((at.x - 0.0).abs() < 1e-9, "{at:?}");
        assert!((at.y + 1.0).abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn a_non_finite_cell_renders_at_the_default_with_one_warning() {
        // `V='inf'` parses to a non-finite number; the model rejects it
        // with one `KindMismatch` warning and the shape renders at the
        // default (ADR 0021, never a silent substitution).
        let (mut doc, page, shape) = document_with_shape();
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.sheet.cells = vec![
                number_cell("PageWidth", 8.5),
                number_cell("PageHeight", 11.0),
            ];
        }
        if let Some(entity) = doc.shape_mut(shape) {
            entity.sheet.cells = vec![
                number_cell("PinX", f64::INFINITY),
                number_cell("PinY", 0.0),
                number_cell("Angle", 0.0),
                number_cell("FlipX", 0.0),
                number_cell("FlipY", 0.0),
                number_cell("LocPinX", 0.0),
                number_cell("LocPinY", 0.0),
                number_cell("Width", 1.0),
                number_cell("Height", 1.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        // Exactly one warning for the non-finite value (the style cells
        // warn `Missing` on their own, unrelated to it).
        let mismatches: Vec<_> = scene
            .warnings
            .iter()
            .filter(|w| w.kind == larrygram_model::WarningKind::KindMismatch)
            .collect();
        assert_eq!(mismatches.len(), 1, "{:?}", scene.warnings);
        assert_eq!(mismatches[0].cell, CellName::from_static("PinX"));
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert_eq!(
                    shape.transform.apply(Point::new(0.0, 0.0)),
                    Point::new(0.0, 0.0)
                );
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    // --- geometry rows -----------------------------------------------

    /// The path one row maps to, from `(0, 0)` as the current point.
    fn path_of_rows(rows: Vec<Row>) -> Path {
        let (mut doc, _page, shape) = document_with_shape();
        with_geometry(&mut doc, shape, rows);
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => shape.path.clone(),
            _ => Path::new(),
        }
    }

    #[test]
    fn move_to_starts_a_subpath() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 1.0), ("Y", 2.0)]),
            grow("LineTo", &[("X", 3.0), ("Y", 4.0)]),
        ]);
        assert_eq!(
            path.segments(),
            &[
                Segment::MoveTo(Point::new(1.0, 2.0)),
                Segment::LineTo(Point::new(3.0, 4.0)),
            ]
        );
    }

    #[test]
    fn line_to_draws_from_the_current_point() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 1.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 1.0), ("Y", 1.0)]),
        ]);
        assert_eq!(path.segments().len(), 3);
        assert_eq!(path.segments()[2], Segment::LineTo(Point::new(1.0, 1.0)));
    }

    #[test]
    fn arc_to_flattens_to_cubics() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("ArcTo", &[("X", 2.0), ("Y", 0.0), ("A", 1.0)]),
        ]);
        assert!(path.segments().len() > 2, "{:?}", path.segments());
        assert!(matches!(path.segments()[0], Segment::MoveTo(_)));
        assert!(
            path.segments()[1..]
                .iter()
                .all(|segment| matches!(segment, Segment::CubicTo { .. }))
        );
    }

    #[test]
    fn elliptical_arc_to_flattens_to_cubics() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "EllipticalArcTo",
                &[
                    ("X", 2.0),
                    ("Y", 0.0),
                    ("A", 1.0),
                    ("B", 1.0),
                    ("C", 0.0),
                    ("D", 2.0),
                ],
            ),
        ]);
        assert!(
            path.segments()
                .iter()
                .any(|segment| matches!(segment, Segment::CubicTo { .. }))
        );
    }

    #[test]
    fn nurbs_to_assembles_the_knot_list_from_the_formula() {
        let mut extra = grow(
            "NURBSTo",
            &[
                ("X", 3.0),
                ("Y", 0.0),
                ("A", 1.0),
                ("B", 1.0),
                ("C", 0.0),
                ("D", 1.0),
            ],
        );
        extra.cells.push({
            let mut cell = Cell::new(CellName::new("E"));
            cell.value = Some(Value::Str("NURBS(2, 1, 1, 1, 1, 1, 0.5, 1)".to_owned()));
            cell
        });
        let path = path_of_rows(vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), extra]);
        assert!(
            path.segments()
                .iter()
                .any(|segment| matches!(segment, Segment::CubicTo { .. }))
        );
    }

    #[test]
    fn nurbs_to_without_a_parsable_formula_draws_a_line_and_warns() {
        let mut extra = grow(
            "NURBSTo",
            &[
                ("X", 3.0),
                ("Y", 0.0),
                ("A", 1.0),
                ("B", 1.0),
                ("C", 0.0),
                ("D", 1.0),
            ],
        );
        extra.cells.push(string_cell("E", "NURBS(broken"));
        let (mut doc, _page, shape) = document_with_shape();
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), extra],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert_eq!(
                    shape.path.segments()[1],
                    Segment::LineTo(Point::new(3.0, 0.0))
                );
            }
            _ => panic!("expected a shape"),
        }
        // The degradation warns `KindMismatch`, not `ErrorValue`: the
        // cached value is not an error (ADR 0021 amendment, P3-16).
        assert!(scene.warnings.iter().any(|warning| {
            warning.cell.as_str() == "E" && warning.kind == WarningKind::KindMismatch
        }));
    }

    #[test]
    fn polyline_to_draws_through_its_points() {
        let mut extra = grow("PolylineTo", &[("X", 3.0), ("Y", 0.0)]);
        extra.cells.push(string_cell("A", "1 0; 2 1"));
        let path = path_of_rows(vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), extra]);
        // The mapping goes through `Curve::to_cubics` (ADR 0022), so the
        // polyline's straight segments arrive as line-like cubics.
        let ends: Vec<Point> = path
            .segments()
            .iter()
            .map(|segment| match segment {
                Segment::CubicTo { to, .. } => *to,
                Segment::MoveTo(point) => *point,
                other => panic!("unexpected segment {other:?}"),
            })
            .collect();
        assert_eq!(
            ends,
            vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
                Point::new(2.0, 1.0),
                Point::new(3.0, 0.0),
            ]
        );
    }

    #[test]
    fn spline_start_and_knots_form_one_curve() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "SplineStart",
                &[
                    ("X", 1.0),
                    ("Y", 1.0),
                    ("A", 1.0),
                    ("B", 0.0),
                    ("C", 2.0),
                    ("D", 2.0),
                ],
            ),
            grow("SplineKnot", &[("X", 2.0), ("Y", 0.0), ("A", 1.5)]),
        ]);
        assert!(
            path.segments()
                .iter()
                .any(|segment| matches!(segment, Segment::CubicTo { .. }))
        );
    }

    #[test]
    fn spline_knot_without_a_start_warns() {
        let (mut doc, _page, shape) = document_with_shape();
        with_geometry(
            &mut doc,
            shape,
            vec![grow("SplineKnot", &[("X", 2.0), ("Y", 0.0), ("A", 1.5)])],
        );
        let scene = build(&doc);
        assert!(
            scene
                .warnings
                .iter()
                .any(|warning| warning.cell.as_str() == "SplineKnot")
        );
    }

    #[test]
    fn ellipse_flattens_to_cubics_and_closes() {
        let path = path_of_rows(vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "Ellipse",
                &[
                    ("X", 0.0),
                    ("Y", 0.0),
                    ("A", 1.0),
                    ("B", 0.0),
                    ("C", 0.0),
                    ("D", 1.0),
                ],
            ),
        ]);
        assert!(
            path.segments()
                .iter()
                .any(|segment| matches!(segment, Segment::CubicTo { .. }))
        );
        assert_eq!(path.segments().last(), Some(&Segment::Close));
    }

    #[test]
    fn infinite_line_clips_to_the_page() {
        let (mut doc, page, shape) = document_with_shape();
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.sheet.cells = vec![
                number_cell("PageWidth", 10.0),
                number_cell("PageHeight", 8.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow(
                "InfiniteLine",
                &[("X", 0.0), ("Y", 4.0), ("A", 10.0), ("B", 4.0)],
            )],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert_eq!(
                    shape.path.segments(),
                    &[
                        Segment::MoveTo(Point::new(0.0, 4.0)),
                        Segment::LineTo(Point::new(10.0, 4.0)),
                    ]
                );
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn rel_rows_scale_by_the_shape_size() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 4.0)];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![
                grow("RelMoveTo", &[("X", 0.0), ("Y", 0.0)]),
                grow("RelLineTo", &[("X", 1.0), ("Y", 1.0)]),
                grow(
                    "RelQuadBezTo",
                    &[("X", 0.5), ("Y", 0.0), ("A", 0.25), ("B", 1.0)],
                ),
                grow(
                    "RelCubBezTo",
                    &[
                        ("X", 0.0),
                        ("Y", 0.5),
                        ("A", 0.25),
                        ("B", 1.0),
                        ("C", 0.75),
                        ("D", 1.0),
                    ],
                ),
                grow(
                    "RelEllipticalArcTo",
                    &[
                        ("X", 1.0),
                        ("Y", 0.0),
                        ("A", 0.5),
                        ("B", 0.5),
                        ("C", 0.0),
                        ("D", 1.0),
                    ],
                ),
            ],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert_eq!(
                    shape.path.segments()[1],
                    Segment::LineTo(Point::new(2.0, 4.0))
                );
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn an_unknown_row_type_renders_nothing_and_warns() {
        let (mut doc, _page, shape) = document_with_shape();
        with_geometry(
            &mut doc,
            shape,
            vec![grow("Mystery", &[("X", 1.0), ("Y", 1.0)])],
        );
        let scene = build(&doc);
        assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
        assert!(
            scene
                .warnings
                .iter()
                .any(|warning| warning.cell.as_str() == "Mystery")
        );
    }

    #[test]
    fn no_show_geometry_draws_nothing() {
        let (mut doc, _page, shape) = document_with_shape();
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 1.0), ("Y", 1.0)])],
        );
        if let Some(entity) = doc.shape_mut(shape)
            && let Some(section) = entity.sheet.sections.first_mut()
        {
            section.cells.push(number_cell("NoShow", 1.0));
        }
        let scene = build(&doc);
        assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
    }

    // --- fill, stroke, shadow ----------------------------------------

    #[test]
    fn solid_fill_uses_the_foreground_colour() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("FillPattern", 1.0),
                number_cell("FillForegnd", 2.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert_eq!(shape.fill, Some(Paint::Solid(Rgba::opaque(255, 0, 0))));
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn fill_pattern_zero_draws_no_fill() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("FillPattern", 0.0)];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert_eq!(shape.fill, None),
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_tile_fill_follows_the_tile_box_convention() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("FillPattern", 24.0),
                number_cell("LineWeight", 0.01),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => match &shape.fill {
                Some(Paint::Pattern(pattern)) => {
                    // Index 24: spacing 4 × 0.01 in line width.
                    assert_eq!(pattern.tile_size, Size::new(0.04, 0.04));
                    assert!(!pattern.tile.is_empty());
                }
                other => panic!("expected a pattern, got {other:?}"),
            },
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn an_unknown_fill_pattern_warns_and_draws_nothing() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("FillPattern", 200.0)];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        assert!(
            scene
                .warnings
                .iter()
                .any(|warning| warning.cell.as_str() == "FillPattern")
        );
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert_eq!(shape.fill, None),
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_linear_gradient_comes_from_the_stop_rows() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("FillPattern", 1.0),
                number_cell("FillGradientEnabled", 1.0),
                number_cell("FillGradientDir", 0.0),
                number_cell("FillGradientAngle", 0.0),
                number_cell("Width", 2.0),
                number_cell("Height", 1.0),
            ];
            let mut section = Section::new(SectionName::new("FillGradient"));
            for (position, index) in [(0.0, 0.0), (1.0, 2.0)] {
                let mut row = Row::new();
                row.cells = vec![
                    number_cell("GradientStopPosition", position),
                    number_cell("GradientStopColor", index),
                ];
                section.rows.push(row);
            }
            entity.sheet.sections.push(section);
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => match &shape.fill {
                Some(Paint::LinearGradient { start, end, stops }) => {
                    assert_eq!(stops.len(), 2);
                    assert_eq!(stops[0].color, Rgba::opaque(0, 0, 0));
                    assert_eq!(stops[1].color, Rgba::opaque(255, 0, 0));
                    assert_eq!(*start, Point::new(0.0, 0.5));
                    assert_eq!(*end, Point::new(2.0, 0.5));
                }
                other => panic!("expected a linear gradient, got {other:?}"),
            },
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_radial_gradient_comes_from_the_direction_cell() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("FillPattern", 30.0),
                number_cell("FillGradientDir", 1.0),
                number_cell("Width", 3.0),
                number_cell("Height", 4.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => match &shape.fill {
                Some(Paint::RadialGradient {
                    center,
                    radius,
                    stops,
                }) => {
                    assert_eq!(*center, Point::new(1.5, 2.0));
                    assert!((*radius - 2.5).abs() < 1e-9);
                    assert_eq!(stops.len(), 2);
                }
                other => panic!("expected a radial gradient, got {other:?}"),
            },
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_stroke_takes_its_dash_width_and_ends() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("LinePattern", 2.0),
                number_cell("LineWeight", 0.01),
                number_cell("LineCap", 1.0),
                number_cell("Rounding", 0.5),
                number_cell("EndArrow", 4.0),
                number_cell("EndArrowSize", 2.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let stroke = shape.stroke.as_ref().expect("stroke");
                assert_eq!(stroke.width, 0.01);
                assert_eq!(stroke.dash.len(), 2);
                assert!((stroke.dash[0] - 0.06).abs() < 1e-12);
                assert!((stroke.dash[1] - 0.04).abs() < 1e-12);
                assert_eq!(stroke.cap, LineCap::Square);
                assert_eq!(stroke.join, LineJoin::Round);
                let end = stroke.end.as_ref().expect("end");
                assert!(end.filled);
                assert_eq!(end.width, 1.1 * 0.25);
                assert_eq!(end.length, 1.3 * 0.25);
                assert_eq!(stroke.start, None);
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn line_pattern_zero_draws_no_line() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("LinePattern", 0.0)];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert_eq!(shape.stroke, None),
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_shadow_uses_the_template_and_the_shadow_cells() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("ShapeShdwShow", 1.0),
                number_cell("ShdwPattern", 1.0),
                number_cell("ShapeShdwOffsetX", 0.1),
                number_cell("ShapeShdwOffsetY", -0.2),
                number_cell("ShapeShdwBlur", 0.05),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let shadow = shape.shadow.expect("shadow");
                assert_eq!(shadow.offset, Point::new(0.1, -0.2));
                assert!((shadow.blur - 0.05).abs() < 1e-12);
                assert_eq!(shadow.color.a, 128);
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_hidden_shadow_draws_nothing() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("ShapeShdwShow", 0.0),
                number_cell("ShdwPattern", 1.0),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert_eq!(shape.shadow, None),
            _ => panic!("expected a shape"),
        }
    }

    // --- layers, groups, pages ---------------------------------------

    #[test]
    fn a_shape_on_an_invisible_layer_is_dropped() {
        let (mut doc, page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![string_cell("LayerMember", "0")];
        }
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            let mut section = Section::new(SectionName::new("Layer"));
            let mut row = Row::new();
            row.ix = Some(0);
            row.cells = vec![string_cell("Name", "Hidden"), number_cell("Visible", 0.0)];
            section.rows.push(row);
            entity.sheet.sections.push(section);
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
    }

    #[test]
    fn a_shape_on_a_visible_layer_renders() {
        let (mut doc, page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![string_cell("LayerMember", "0")];
        }
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            let mut section = Section::new(SectionName::new("Layer"));
            let mut row = Row::new();
            row.ix = Some(0);
            row.cells = vec![string_cell("Name", "Shown"), number_cell("Visible", 1.0)];
            section.rows.push(row);
            entity.sheet.sections.push(section);
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        assert_eq!(scene.nodes.len(), 1);
    }

    #[test]
    fn a_group_composes_the_parent_transform() {
        let (mut doc, _page, group) = document_with_shape();
        let child = doc.add_shape(ModelShape::new(ShapeId { index: 1 }, 2));
        if let Some(entity) = doc.shape_mut(group) {
            entity.children.push(child);
            entity.sheet.cells = vec![number_cell("PinX", 5.0), number_cell("PinY", 0.0)];
        }
        if let Some(entity) = doc.shape_mut(child) {
            entity.parent = Some(group);
            entity.sheet.cells = vec![number_cell("PinX", 1.0), number_cell("PinY", 2.0)];
        }
        with_geometry(
            &mut doc,
            child,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Group(group)) => {
                assert_eq!(
                    group.transform.apply(Point::new(0.0, 0.0)),
                    Point::new(5.0, 0.0)
                );
                match group.children.first() {
                    Some(Node::Shape(shape)) => {
                        // Relative to the group: (1, 2) inside it, and
                        // (6, 2) on the page once a backend composes the
                        // chain. Regression: the child used to carry the
                        // page transform too, so backends applied the
                        // group offset twice.
                        assert_eq!(
                            shape.transform.apply(Point::new(0.0, 0.0)),
                            Point::new(1.0, 2.0)
                        );
                        assert_eq!(
                            shape
                                .transform
                                .then(group.transform)
                                .apply(Point::new(0.0, 0.0)),
                            Point::new(6.0, 2.0)
                        );
                    }
                    _ => panic!("expected a shape child"),
                }
            }
            _ => panic!("expected a group"),
        }
    }

    #[test]
    fn a_cyclic_children_list_renders_once() {
        let (mut doc, _page, group) = document_with_shape();
        if let Some(entity) = doc.shape_mut(group) {
            entity.children.push(group);
        }
        with_geometry(
            &mut doc,
            group,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        assert_eq!(scene.nodes.len(), 1, "{:?}", scene.nodes);
    }

    #[test]
    fn a_background_page_renders_as_a_leading_group_once() {
        let (mut doc, page, shape) = document_with_shape();
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let background = PageId { index: 1 };
        let bg_shape = doc.add_shape(ModelShape::new(ShapeId { index: 1 }, 2));
        let mut entity = Page::new(background, 2);
        entity.shapes.push(bg_shape);
        // The background chain is cyclic: the background page backgrounds
        // itself through the main page.
        entity.background = Some(page);
        doc.pages.push(entity);
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.background = Some(background);
        }
        with_geometry(
            &mut doc,
            bg_shape,
            vec![grow("MoveTo", &[("X", 1.0), ("Y", 1.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Group(group)) => assert_eq!(group.children.len(), 1),
            other => panic!("expected a leading group, got {other:?}"),
        }
        assert_eq!(scene.nodes.len(), 2);
    }

    #[test]
    fn section_rows_counts_the_master_shape_the_cells_resolve_from() {
        // A master with two top-level shapes: the first holds three
        // `FillGradient` rows (colours 4..6), the second two (colours 2
        // and 3). The instance sits at page position 1, so its row cells
        // resolve through the master's second shape and the row count
        // must come from the same shape (P3-16; the old walk counted the
        // master's first shape).
        let mut doc = Document::new();
        let filler = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 1));
        let instance = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 2));
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.sheet.cells = vec![
            number_cell("PageWidth", 8.5),
            number_cell("PageHeight", 11.0),
        ];
        page.shapes.push(filler);
        page.shapes.push(instance);
        doc.pages.push(page);

        let mid = doc.masters.len();
        let mut master = Master::new(MasterId { index: mid as u32 }, 1);
        let many = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 3));
        let one = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 4));
        for (id, colors) in [(many, vec![4.0, 5.0, 6.0]), (one, vec![2.0, 3.0])] {
            let entity = doc.shape_mut(id).expect("shape");
            let mut section = Section::new(SectionName::new("FillGradient"));
            for (index, color) in colors.iter().enumerate() {
                let mut row = Row::new();
                row.ix = Some(index as i32);
                row.cells = vec![
                    number_cell("GradientStopPosition", index as f64 / 2.0),
                    number_cell("GradientStopColor", *color),
                    number_cell("GradientStopColorTrans", 0.0),
                ];
                section.rows.push(row);
            }
            entity.sheet.sections.push(section);
        }
        master.shapes = vec![many, one];
        doc.masters.push(master);
        doc.shape_mut(instance).expect("shape").master = Some(MasterId { index: mid as u32 });
        {
            let entity = doc.shape_mut(instance).expect("shape");
            entity.sheet.cells = vec![
                number_cell("FillPattern", 30.0),
                number_cell("FillGradientDir", 0.0),
                number_cell("FillGradientAngle", 0.0),
                number_cell("Width", 1.0),
                number_cell("Height", 1.0),
            ];
        }
        with_geometry(
            &mut doc,
            instance,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => match shape.fill.as_ref() {
                Some(Paint::LinearGradient { stops, .. }) => {
                    // Two stops, the colours of the master's second shape
                    // (palette indices 2 and 3).
                    assert_eq!(stops.len(), 2);
                    for (stop, index) in stops.iter().zip([2u32, 3]) {
                        let expected = larrygram_model::palette(index).expect("palette entry");
                        assert_eq!(stop.color, Rgba::opaque(expected.r, expected.g, expected.b));
                    }
                }
                other => panic!("expected a linear gradient, got {other:?}"),
            },
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn page_size_comes_from_the_page_sheet() {
        let (mut doc, page, _shape) = document_with_shape();
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.sheet.cells = vec![
                number_cell("PageWidth", 11.0),
                number_cell("PageHeight", 8.5),
            ];
        }
        let scene = build(&doc);
        assert_eq!((scene.width, scene.height), (11.0, 8.5));
    }

    // --- text --------------------------------------------------------

    #[test]
    fn text_runs_resolve_through_the_character_rows() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
            let mut section = Section::new(SectionName::new("Character"));
            let mut row = Row::new();
            row.ix = Some(0);
            row.cells = vec![
                string_cell("Font", "Calibri"),
                number_cell("Size", 0.1666666666666667),
                number_cell("Style", 1.0),
            ];
            section.rows.push(row);
            entity.sheet.sections.push(section);
            entity.text = Some(larrygram_model::TextContent {
                parts: vec![TextPart::Text("Hi".to_owned())],
            });
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let text = shape.text.as_ref().expect("text");
                assert_eq!(text.runs.len(), 1);
                assert_eq!(text.origin, Point::new(0.0, 1.0));
                // The size lives on the glyphs and the rotation on the
                // layout (P3-16).
                assert_eq!(text.layout.rotation.radians(), 0.0);
                assert!(!text.layout.lines.is_empty());
                let glyph = &text.layout.lines[0].glyphs[0];
                assert!((glyph.size - 12.0).abs() < 1e-9, "{glyph:?}");
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn a_run_marker_reads_the_master_row_with_that_ix() {
        // The instance has no `Character` section; the master's rows are
        // `IX` 0 and 5. The run marked `IX` 5 reads the master's row 5
        // (matched by `IX`), not a row at position 5.
        let (mut doc, page, shape) = document_with_shape();
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.sheet.cells = vec![
                number_cell("PageWidth", 8.5),
                number_cell("PageHeight", 11.0),
            ];
        }
        let mid = doc.masters.len();
        let mut master = Master::new(MasterId { index: mid as u32 }, 1);
        let content = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 2));
        {
            let entity = doc.shape_mut(content).expect("content");
            let mut section = Section::new(SectionName::new("Character"));
            let mut row_zero = Row::new();
            row_zero.ix = Some(0);
            row_zero.cells = vec![string_cell("Font", "Zero"), number_cell("Color", 2.0)];
            let mut row_five = Row::new();
            row_five.ix = Some(5);
            row_five.cells = vec![string_cell("Font", "Five"), number_cell("Color", 3.0)];
            section.rows.push(row_zero);
            section.rows.push(row_five);
            entity.sheet.sections.push(section);
        }
        master.shapes = vec![content];
        doc.masters.push(master);
        doc.shape_mut(shape).expect("shape").master = Some(MasterId { index: mid as u32 });
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
            entity.text = Some(larrygram_model::TextContent {
                parts: vec![TextPart::CharRun { ix: 5 }, TextPart::Text("Hi".to_owned())],
            });
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let text = shape.text.as_ref().expect("text");
                assert_eq!(text.runs.len(), 1);
                // The master's `IX` 5 row caches colour index 3; no row
                // sits at position 5, so a positional read would fall
                // back to the defaults (black).
                let expected = larrygram_model::palette(3).expect("palette entry");
                assert_eq!(
                    text.runs[0].color,
                    Rgba::opaque(expected.r, expected.g, expected.b)
                );
            }
            other => panic!("expected a shape, got {other:?}"),
        }
        assert!(
            !scene.warnings.iter().any(|w| w.cell.as_str() == "Color"),
            "{:?}",
            scene.warnings
        );
    }

    #[test]
    fn a_marker_beyond_i32_matches_no_row_and_warns() {
        // `marker_ix` clamps a marker `IX` beyond `i32::MAX`: the run
        // matches no row and reads the defaults with warnings, never a
        // wrapped `IX`.
        let (mut doc, page, shape) = document_with_shape();
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.sheet.cells = vec![
                number_cell("PageWidth", 8.5),
                number_cell("PageHeight", 11.0),
            ];
        }
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
            entity.text = Some(larrygram_model::TextContent {
                parts: vec![
                    TextPart::CharRun { ix: u32::MAX },
                    TextPart::Text("Hi".to_owned()),
                ],
            });
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert!(shape.text.is_some()),
            other => panic!("expected a shape, got {other:?}"),
        }
        assert!(
            scene.warnings.iter().any(|w| w.cell.as_str() == "Size"),
            "{:?}",
            scene.warnings
        );
    }

    #[test]
    fn paragraph_markers_split_paragraphs_and_bullets_get_a_run() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
            let mut section = Section::new(SectionName::new("Paragraph"));
            let mut row = Row::new();
            row.ix = Some(0);
            row.cells = vec![number_cell("Bullet", 1.0)];
            section.rows.push(row);
            entity.sheet.sections.push(section);
            entity.text = Some(larrygram_model::TextContent {
                parts: vec![
                    TextPart::Text("one".to_owned()),
                    TextPart::ParaRun { ix: 0 },
                    TextPart::Text("two".to_owned()),
                ],
            });
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let text = shape.text.as_ref().expect("text");
                // Two input runs plus one bullet entry per paragraph.
                assert_eq!(text.runs.len(), 4);
                let paragraphs: Vec<usize> = text
                    .layout
                    .lines
                    .iter()
                    .map(|line| line.paragraph)
                    .collect();
                assert!(
                    paragraphs.contains(&0) && paragraphs.contains(&1),
                    "{paragraphs:?}"
                );
            }
            _ => panic!("expected a shape"),
        }
    }

    #[test]
    fn empty_text_builds_no_text_node() {
        let (mut doc, _page, shape) = document_with_shape();
        if let Some(entity) = doc.shape_mut(shape) {
            entity.text = Some(larrygram_model::TextContent::new());
        }
        with_geometry(
            &mut doc,
            shape,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => assert_eq!(shape.text, None),
            _ => panic!("expected a shape"),
        }
    }

    // --- helpers -----------------------------------------------------

    #[test]
    fn ellipse_cubics_cover_a_circle() {
        let cubics = ellipse_cubics(
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            0.001,
        );
        assert_eq!(cubics.len(), 4);
        let first = cubics[0];
        assert_eq!(first.from, Point::new(1.0, 0.0));
        assert!((first.to.x - 0.0).abs() < 1e-9);
        assert!((first.to.y - 1.0).abs() < 1e-9);
        assert!(
            ellipse_cubics(
                Point::new(0.0, 0.0),
                Point::new(0.0, 0.0),
                Point::new(0.0, 0.0),
                0.1
            )
            .is_empty()
        );
    }

    #[test]
    fn clip_infinite_line_cuts_and_rejects() {
        let rect = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 8.0),
            Point::new(0.0, 8.0),
        ];
        let (from, to) =
            clip_infinite_line(Point::new(-5.0, 4.0), Point::new(15.0, 4.0), &rect).expect("clips");
        assert_eq!(from, Point::new(0.0, 4.0));
        assert_eq!(to, Point::new(10.0, 4.0));
        assert_eq!(
            clip_infinite_line(Point::new(-5.0, 20.0), Point::new(15.0, 20.0), &rect),
            None
        );
        assert_eq!(
            clip_infinite_line(Point::new(0.0, 0.0), Point::new(0.0, 0.0), &rect),
            None
        );
    }

    #[test]
    fn rgba_of_applies_the_transparency() {
        assert_eq!(rgba_of(Color::new(1, 2, 3), 0.0), Rgba::new(1, 2, 3, 255));
        assert_eq!(rgba_of(Color::new(1, 2, 3), 0.5), Rgba::new(1, 2, 3, 128));
        assert_eq!(rgba_of(Color::new(1, 2, 3), 2.0), Rgba::new(1, 2, 3, 0));
    }

    #[test]
    fn scale_path_scales_the_unit_box() {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(1.0, 1.0)));
        path.push(Segment::LineTo(Point::new(0.5, 0.0)));
        path.push(Segment::Close);
        let scaled = scale_path(&path, 2.0, 4.0);
        assert_eq!(
            scaled.segments(),
            &[
                Segment::MoveTo(Point::new(2.0, 4.0)),
                Segment::LineTo(Point::new(1.0, 0.0)),
                Segment::Close,
            ]
        );
    }

    // --- degraded input: substituted defaults (P3-17, coverage finding
    // major 1) ----------------------------------------------------------

    /// A cached string cell.
    fn str_cell(name: &str, value: &str) -> Cell {
        string_cell(name, value)
    }

    /// Whether `warnings` holds one warning for `cell` with `kind`.
    fn warned(warnings: &[CellWarning], cell: &str, kind: WarningKind) -> bool {
        warnings
            .iter()
            .any(|warning| warning.cell.as_str() == cell && warning.kind == kind)
    }

    /// The style set of every test build.
    fn styles() -> VisualStyles {
        VisualStyles::load().expect("styles")
    }

    #[test]
    fn reader_errors_substitute_defaults_for_every_kind() {
        // The fallback arms of `Ctx` for a sheet the document does not
        // hold: every reader call errors and the builder substitutes the
        // documented default with one `Missing` warning.
        let (doc, _page, _shape) = document_with_shape();
        let mut ctx = Ctx::new(&doc, styles(), 0.01);
        let missing = SheetRef::Shape(ShapeId { index: 9 });

        assert_eq!(ctx.number(missing, "Width"), 0.0);
        assert!(!ctx.boolean(missing, "FlipX"));
        assert_eq!(ctx.color(missing, "FillForegnd"), Color::new(0, 0, 0));
        assert_eq!(
            ctx.row_number(missing, "Geometry", RowTarget::at(0), "X"),
            0.0
        );
        assert_eq!(
            ctx.row_text(missing, "Character", RowTarget::at(0), "Font"),
            ""
        );
        assert!(!ctx.row_boolean(missing, "Character", RowTarget::at(0), "Strikethru"));
        assert_eq!(
            ctx.row_color(missing, "Character", RowTarget::at(0), "Color"),
            Color::new(0, 0, 0)
        );

        for cell in [
            "Width",
            "FlipX",
            "FillForegnd",
            "X",
            "Font",
            "Strikethru",
            "Color",
        ] {
            assert!(
                warned(&ctx.warnings, cell, WarningKind::Missing),
                "no Missing warning for {cell}: {:?}",
                ctx.warnings
            );
        }
        // A page sheet the document does not hold degrades the same way
        // in `has_local` and `section_rows`.
        let missing_page = SheetRef::Page(PageId { index: 9 });
        assert!(!ctx.has_local(missing_page, &TXT_CELLS));
        assert_eq!(section_rows(&ctx, missing_page, "Character"), 0);
    }

    #[test]
    fn a_dangling_sub_shape_warns_about_its_layer_membership() {
        // The `LayerMember` fallback: a child id the arena does not hold.
        let (mut doc, _page, shape) = document_with_shape();
        if let Some(entity) = doc.shape_mut(shape) {
            entity.children.push(ShapeId { index: 9 });
        }
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "LayerMember", WarningKind::Missing));
    }

    #[test]
    fn layer_visibility_of_an_unknown_page_warns_about_the_layer_list() {
        let (mut doc, _page, shape) = document_with_shape();
        if let Some(entity) = doc.shape_mut(shape) {
            entity.sheet.cells.push(number_cell("LayerMember", 1.0));
        }
        let mut ctx = Ctx::new(&doc, styles(), 0.01);
        assert!(!hidden_by_layer(&mut ctx, PageId { index: 9 }, shape));
        assert!(warned(&ctx.warnings, "Layer", WarningKind::Missing));
    }

    #[test]
    fn shape_content_of_an_unknown_shape_warns_about_its_geometry() {
        // The `Geometry` fallback: a sheet the document does not hold
        // draws nothing and substitutes the empty geometry.
        let (doc, page, _shape) = document_with_shape();
        let mut ctx = Ctx::new(&doc, styles(), 0.01);
        let (path, fill, stroke, shadow, text) =
            shape_content(&mut ctx, page, ShapeId { index: 9 }, Transform::identity());
        assert!(path.is_empty());
        assert!(fill.is_none());
        assert!(stroke.is_none());
        assert!(shadow.is_none());
        assert!(text.is_none());
        assert!(warned(&ctx.warnings, "Geometry", WarningKind::Missing));
    }

    #[test]
    fn a_text_layout_failure_warns_about_the_text_cell() {
        // A negative `SpBefore` makes `text::layout` reject the block
        // (`Error::InvalidBlock`); the builder drops the text and warns.
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.text = Some(TextContent {
                parts: vec![TextPart::Text("hi".to_owned())],
            });
            let mut section = Section::new(SectionName::new("Paragraph"));
            let mut row = Row::new();
            row.cells.push(number_cell("SpBefore", -1.0));
            section.rows.push(row);
            entity.sheet.sections.push(section);
        }
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "Text", WarningKind::KindMismatch));
        // With the text dropped the shape draws nothing and is omitted.
        assert!(scene.nodes.is_empty());
    }

    #[test]
    fn wrong_kind_cells_substitute_defaults_and_warn() {
        // One fixture per substituted kind: a number, a boolean and a
        // colour cell caching the wrong kind, plus a row number cell.
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                str_cell("Width", "wide"),
                str_cell("Height", "tall"),
                str_cell("FlipX", "yes"),
                number_cell("FillPattern", 1.0),
                str_cell("FillForegnd", "red"),
            ];
            let mut section = Section::new(SectionName::new("Geometry"));
            let mut row = grow("MoveTo", &[("Y", 1.0)]);
            row.cells.push(str_cell("X", "zero"));
            section.rows.push(row);
            entity.sheet.sections.push(section);
        }
        let scene = build(&doc);
        for cell in ["Width", "Height", "FlipX", "FillForegnd", "X"] {
            assert!(
                warned(&scene.warnings, cell, WarningKind::KindMismatch),
                "no KindMismatch warning for {cell}: {:?}",
                scene.warnings
            );
        }
    }

    #[test]
    fn missing_cells_without_a_documented_default_warn() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
        }
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "LineWeight", WarningKind::Missing));
    }

    // --- style-level warnings (P3-17, coverage finding minor 3) -------

    /// A shape with `cells`, drawn by a one-inch square geometry.
    fn shape_with_cells(doc: &mut Document, shape: ShapeId, cells: Vec<Cell>) {
        if let Some(entity) = doc.shape_mut(shape) {
            entity.sheet.cells = cells;
        }
        with_geometry(
            doc,
            shape,
            vec![
                grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
                grow("LineTo", &[("X", 1.0), ("Y", 0.0)]),
                grow("LineTo", &[("X", 1.0), ("Y", 1.0)]),
            ],
        );
    }

    #[test]
    fn an_unknown_line_pattern_warns_and_draws_solid() {
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(&mut doc, shape, vec![number_cell("LinePattern", 99.0)]);
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "LinePattern", WarningKind::Missing));
    }

    #[test]
    fn an_unknown_line_cap_falls_back_to_butt() {
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![number_cell("LinePattern", 1.0), number_cell("LineCap", 5.0)],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let stroke = shape.stroke.as_ref().expect("stroke");
                assert_eq!(stroke.cap, LineCap::Butt);
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_arrow_index_and_size_class_warn() {
        // An index the data file does not define.
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("LinePattern", 1.0),
                number_cell("EndArrow", 99.0),
                number_cell("EndArrowSize", 9.0),
            ],
        );
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "EndArrow", WarningKind::Missing));

        // A defined index with a size class outside 0..=6.
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("LinePattern", 1.0),
                number_cell("EndArrow", 1.0),
                number_cell("EndArrowSize", 9.0),
            ],
        );
        let scene = build(&doc);
        assert!(warned(
            &scene.warnings,
            "EndArrowSize",
            WarningKind::Missing
        ));
    }

    #[test]
    fn an_unknown_shadow_pattern_warns() {
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("ShapeShdwShow", 1.0),
                number_cell("ShdwPattern", 99.0),
            ],
        );
        let scene = build(&doc);
        assert!(warned(&scene.warnings, "ShdwPattern", WarningKind::Missing));
    }

    #[test]
    fn shadow_offsets_default_to_the_template() {
        // No `ShapeShdwOffsetX`/`Y` cells: the template offsets scale
        // with the line width.
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("ShapeShdwShow", 1.0),
                number_cell("ShdwPattern", 1.0),
                number_cell("LineWeight", 0.02),
            ],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                assert!(shape.shadow.is_some(), "no shadow built");
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn a_rectangular_gradient_direction_warns() {
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("FillPattern", 1.0),
                number_cell("FillGradientEnabled", 1.0),
                number_cell("FillGradientDir", 9.0),
            ],
        );
        let scene = build(&doc);
        assert!(warned(
            &scene.warnings,
            "FillGradientDir",
            WarningKind::Missing
        ));
    }

    #[test]
    fn txt_placement_cells_override_the_shape_box() {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.text = Some(TextContent {
                parts: vec![TextPart::Text("hi".to_owned())],
            });
            entity.sheet.cells = vec![
                number_cell("Width", 1.0),
                number_cell("Height", 1.0),
                number_cell("TxtPinX", 2.0),
                number_cell("TxtPinY", 3.0),
                number_cell("TxtLocPinX", 0.5),
                number_cell("TxtLocPinY", 0.5),
                number_cell("TxtWidth", 4.0),
                number_cell("TxtHeight", 5.0),
                number_cell("TxtAngle", core::f64::consts::PI),
            ];
        }
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(shape)) => {
                let text = shape.text.as_ref().expect("text");
                // The block origin is the placement transform applied to
                // (0, height): translate(-0.5, -0.5), a half turn and
                // translate(2, 3) put (0, 5) at (2.5, -1.5).
                assert!(
                    (text.origin.x - 2.5).abs() < 1e-9 && (text.origin.y + 1.5).abs() < 1e-9,
                    "{:?}",
                    text.origin
                );
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn vertical_align_bottom_moves_the_lines_down() {
        let scene_with = |align: f64| {
            let (mut doc, _page, shape) = document_with_shape();
            {
                let entity = doc.shape_mut(shape).expect("shape");
                entity.text = Some(TextContent {
                    parts: vec![TextPart::Text("hi".to_owned())],
                });
                entity.sheet.cells = vec![
                    number_cell("Width", 1.0),
                    number_cell("Height", 2.0),
                    number_cell("VerticalAlign", align),
                ];
            }
            build(&doc)
        };
        let glyph_y = |scene: &Scene| -> f64 {
            match scene.nodes.first() {
                Some(Node::Shape(shape)) => {
                    let text = shape.text.as_ref().expect("text");
                    text.layout
                        .lines
                        .first()
                        .and_then(|line| line.glyphs.first())
                        .map(|glyph| glyph.y)
                        .expect("a glyph")
                }
                other => panic!("expected a shape, got {other:?}"),
            }
        };
        // In the layout's y-down space a bottom-aligned line sits below
        // the top-aligned one.
        assert!(glyph_y(&scene_with(2.0)) > glyph_y(&scene_with(0.0)));
    }

    /// A paragraph row for `shape` with `cells`, and text that uses it.
    fn shape_with_paragraph(doc: &mut Document, shape: ShapeId, cells: Vec<Cell>) {
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.text = Some(TextContent {
                parts: vec![TextPart::Text("hi".to_owned())],
            });
            let mut section = Section::new(SectionName::new("Paragraph"));
            let mut row = Row::new();
            row.ix = Some(0);
            row.cells = cells;
            section.rows.push(row);
            entity.sheet.sections.push(section);
        }
    }

    #[test]
    fn paragraph_alignment_and_line_spacing_branches() {
        // `HorzAlign` 2 (right) and a negative `SpLine` (multiple).
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_paragraph(
            &mut doc,
            shape,
            vec![number_cell("HorzAlign", 2.0), number_cell("SpLine", -1.5)],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(node)) => {
                let text = node.text.as_ref().expect("text");
                assert_eq!(text.layout.lines.len(), 1);
            }
            other => panic!("expected a shape, got {other:?}"),
        }

        // `HorzAlign` 3 (justify) and a positive `SpLine` (a multiple of
        // the first run's size).
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_paragraph(
            &mut doc,
            shape,
            vec![number_cell("HorzAlign", 3.0), number_cell("SpLine", 24.0)],
        );
        let scene = build(&doc);
        assert!(
            scene
                .nodes
                .iter()
                .any(|node| matches!(node, Node::Shape(_)))
        );
    }

    // --- style data fallbacks (P3-17 minor 4) -------------------------

    /// A cached colour cell.
    fn color_cell(name: &str, value: Color) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(Value::Color(value));
        cell
    }

    #[test]
    fn a_line_end_template_with_no_shape_draws_nothing() {
        // A data file whose line-end entry carries no shape: the parse
        // substitutes an empty path and the builder drops the line end
        // without a warning.
        let yaml = "meta:\n  version: 1\nline_ends:\n  - index: 5\n";
        let styles = crate::styles::parse_for_test(yaml).expect("parses");
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("LinePattern", 1.0),
                number_cell("EndArrow", 5.0),
            ],
        );
        let mut ctx = Ctx::new(&doc, styles, 0.01);
        let (_path, _fill, stroke, _shadow, _text) =
            shape_content(&mut ctx, PageId { index: 0 }, shape, Transform::identity());
        let stroke = stroke.expect("a stroke");
        assert!(stroke.end.is_none(), "{:?}", stroke.end);
    }

    #[test]
    fn an_inverted_tile_swaps_the_colours() {
        // `invert: true` tiles draw the background colour over a
        // foreground-coloured box: the builder swaps the two.
        let yaml = concat!(
            "meta:\n  version: 1\nfill_patterns:\n",
            "  - index: 7\n    kind: tile\n    tile: \"M 0 0 L 1 0 L 1 1 z\"\n",
            "    spacing: 1.0\n    invert: true\n"
        );
        let styles = crate::styles::parse_for_test(yaml).expect("parses");
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(
            &mut doc,
            shape,
            vec![
                number_cell("FillPattern", 7.0),
                color_cell("FillForegnd", Color::new(255, 0, 0)),
                color_cell("FillBkgnd", Color::new(0, 0, 255)),
            ],
        );
        let mut ctx = Ctx::new(&doc, styles, 0.01);
        let paint = build_fill(&mut ctx, SheetRef::Shape(shape), 1.0, 1.0, 0.02);
        match paint {
            Some(Paint::Pattern(pattern)) => {
                assert_eq!(pattern.foreground, Rgba::opaque(0, 0, 255));
                assert_eq!(pattern.background, Rgba::opaque(255, 0, 0));
            }
            other => panic!("expected a pattern, got {other:?}"),
        }
    }

    #[test]
    fn an_infinite_line_clips_through_a_flipped_page() {
        // A flipped shape reverses the page winding in its local space:
        // the infinite-line clip reverses the polygon before it clips.
        let (mut doc, _page, shape) = document_with_shape();
        shape_with_cells(&mut doc, shape, vec![number_cell("FlipY", 1.0)]);
        with_geometry(
            &mut doc,
            shape,
            vec![grow(
                "InfiniteLine",
                &[("X", 1.0), ("Y", -0.5), ("A", 7.0), ("B", -0.5)],
            )],
        );
        let scene = build(&doc);
        match scene.nodes.first() {
            Some(Node::Shape(node)) => {
                assert!(
                    !node.path.is_empty(),
                    "the line through the page drew nothing"
                );
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }
}
