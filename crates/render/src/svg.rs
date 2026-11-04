//! The SVG backend: our own deterministic writer (ADR 0025).
//!
//! Text is emitted as glyph outlines, never as `<text>` elements, so the
//! output is self-contained and byte-stable.
//!
//! # Document shape
//!
//! [`render_svg`] writes one standalone document with a fixed layout:
//!
//! ```text
//! <?xml version="1.0" encoding="UTF-8"?>
//! <svg xmlns="http://www.w3.org/2000/svg" version="1.1"
//!      width="W" height="H" viewBox="0 0 W H">
//!   <defs>...</defs>            (only when the scene needs defs)
//!   <g transform="matrix(1.000000 0.000000 0.000000 -1.000000 0.000000 H)">
//!     ... the scene ...
//!   </g>
//! </svg>
//! ```
//!
//! Every number is written with fixed 6-decimal formatting and every
//! attribute list is written in a fixed order, so the same scene always
//! renders to the same bytes (ADR 0025). The root group flips y: the
//! scene is in page coordinates (x right, y up, ADR 0020), SVG is y
//! down.
//!
//! # Coordinates
//!
//! The scene is in inches (canonical units, ADR 0011). Every number in
//! the output is scaled to pixels by [`SvgOptions::scale`]. A node's
//! geometry (its `d` attribute) is written in the node's own local
//! space, scaled to pixels; its `transform` maps that space to the
//! parent's. Gradients (`gradientUnits="userSpaceOnUse"`), pattern
//! tiles and clip paths resolve in the same local space as the path
//! they paint.
//!
//! The element subset is the one fixed by ADR 0025: `svg`, `g`, `path`,
//! `defs`, `linearGradient`, `radialGradient`, `pattern`, `clipPath`
//! (plus `stop` inside gradients, which gradients cannot exist
//! without). A rectangle is a `path`; there is no `rect` element.
//!
//! # Conventions the builder follows
//!
//! - Paint coordinates (gradient points, pattern tiles) are in the
//!   painted node's local space, in inches. A pattern tile is drawn in
//!   one tile of the paint's `tile_size`; the tile mark fills when
//!   `PatternPaint::filled` and strokes with
//!   `PatternPaint::tile_stroke_width` otherwise — the same mark the PNG
//!   backend rasterizes.
//! - A line end follows the unit-box convention of `render::styles`
//!   (ADR 0023): `x = 0` is the tip at the path's vertex, `x = 1` is
//!   the tail along the line, `y = 0.5` is the line centre; `length`
//!   scales `x` and `width` scales `y`, both in inches. A filled line
//!   end paints with its `color`, or with the stroke paint when `color`
//!   is `None`; an unfilled one strokes with the stroke's paint and
//!   width. A line end on an end of the path that carries no direction
//!   (an empty or degenerate path) is not drawn.
//! - A shadow is an offset copy of the shape painted in the shadow
//!   colour. The blur radius is not drawn: the SVG subset has no
//!   `filter` element (ADR 0025).
//!
//! # Text
//!
//! Each glyph is one `path` from `larrygram_text::Font::outline` (em
//! units, font orientation, y up), translated to its laid-out position
//! and scaled by `size / 72` inches per em. The layout bakes the text
//! block rotation into the glyph positions (ADR 0024 amendment), so
//! positions are used as they are; the glyph outline is rotated by
//! minus the block rotation about its own origin.
//!
//! The size comes from `GlyphPos::size` and the rotation from
//! `Layout::rotation`, and each glyph takes its run colour from
//! `TextNode::runs` (the block colour is the fallback), as in `png.rs`.
//! One private helper (`Writer::glyph_transform`) holds the placement.

use larrygram_geom::{Path, Point, Segment, Transform};

use crate::error::Error;
use crate::scene::{
    Clip, GradientStop, Group, LineCap, LineEnd, LineJoin, Node, Paint, PatternPaint, Rgba, Scene,
    Shape, Stroke, TextNode,
};

/// Options for SVG output (ADR 0025).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgOptions {
    /// Pixels per inch (default 96). Must be finite, positive and at
    /// most [`crate::MAX_DEVICE_EXTENT`], or the render fails with
    /// [`Error::InvalidOptions`].
    pub scale: f64,
}

impl SvgOptions {
    /// The default options: 96 pixels per inch.
    pub fn new() -> Self {
        Self { scale: 96.0 }
    }
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The cap on the output document, in bytes (AGENTS.md section 6:
/// capped allocations on input-derived values).
const OUTPUT_CAP: usize = 64 * 1024 * 1024;

/// The cap on group nesting depth (AGENTS.md section 6: bounded
/// recursion).
const MAX_DEPTH: usize = 128;

/// The default SVG miter limit; written only when the scene differs.
const DEFAULT_MITER_LIMIT: f64 = 4.0;

/// One element attribute, in writing order.
type Attr = (String, String);

/// Writes a scene as a standalone SVG document (ADR 0025).
///
/// The output is deterministic: the same scene and options always
/// render to the same bytes. Returns [`Error::InvalidOptions`] for a
/// scale that is not finite, not positive or over the shared scale cap
/// ([`crate::MAX_DEVICE_EXTENT`] pixels per inch), and
/// [`Error::TooLarge`] when the scene dimensions or the device extent
/// exceed the caps, when the group nesting exceeds the depth cap, or
/// when the document would exceed the output byte cap. Never panics
/// (AGENTS.md section 6).
pub fn render_svg(scene: &Scene, options: &SvgOptions) -> Result<String, Error> {
    let scale = crate::checked_scale(options.scale)?;
    let (width, height) = (scene.width, scene.height);
    if !width.is_finite() || !height.is_finite() || width < 0.0 || height < 0.0 {
        return Err(Error::TooLarge);
    }
    let (w_px, h_px) = (width * scale, height * scale);
    // The same device extent cap as `png.rs` (ADR 0025 amendment).
    if !w_px.is_finite()
        || !h_px.is_finite()
        || w_px > f64::from(crate::MAX_DEVICE_EXTENT)
        || h_px > f64::from(crate::MAX_DEVICE_EXTENT)
    {
        return Err(Error::TooLarge);
    }
    let mut writer = Writer::new(scale, OUTPUT_CAP);
    if let Some(background) = scene.background {
        let d = format!(
            "M 0.000000 0.000000 L {} 0.000000 L {} {} L 0.000000 {} Z",
            num(w_px),
            num(w_px),
            num(h_px),
            num(h_px)
        );
        let mut attrs = vec![("d".to_string(), d)];
        attrs.extend(fill_attrs(&mut writer, Some(&Paint::Solid(background)))?);
        writer.tag(false, 2, "path", &attrs, true)?;
    }
    for node in &scene.nodes {
        writer.node(node, 2, 1)?;
    }
    writer.finish(w_px, h_px)
}

/// The document writer: a body buffer and a defs buffer, both counted
/// against the output cap.
struct Writer {
    scale: f64,
    cap: usize,
    written: usize,
    defs: String,
    body: String,
    gradients: usize,
    patterns: usize,
    clips: usize,
}

impl Writer {
    /// A writer for the given pixels-per-inch scale and byte cap.
    fn new(scale: f64, cap: usize) -> Self {
        Self {
            scale,
            cap,
            written: 0,
            defs: String::new(),
            body: String::new(),
            gradients: 0,
            patterns: 0,
            clips: 0,
        }
    }

    /// Appends `s` to the defs or body buffer, counting it against the
    /// cap.
    fn push(&mut self, defs: bool, s: &str) -> Result<(), Error> {
        self.written = self.written.saturating_add(s.len());
        if self.written > self.cap {
            return Err(Error::TooLarge);
        }
        if defs {
            self.defs.push_str(s);
        } else {
            self.body.push_str(s);
        }
        Ok(())
    }

    /// Writes one element line: indentation, `<name attrs />` or
    /// `<name attrs>`. Every attribute value is XML-escaped.
    fn tag(
        &mut self,
        defs: bool,
        indent: usize,
        name: &str,
        attrs: &[Attr],
        self_closing: bool,
    ) -> Result<(), Error> {
        let mut s = String::new();
        for _ in 0..indent {
            s.push_str("  ");
        }
        s.push('<');
        s.push_str(name);
        for (key, value) in attrs {
            s.push(' ');
            s.push_str(key);
            s.push_str("=\"");
            s.push_str(&escape_attr(value));
            s.push('"');
        }
        if self_closing {
            s.push_str(" />");
        } else {
            s.push('>');
        }
        s.push('\n');
        self.push(defs, &s)
    }

    /// Writes a closing tag line.
    fn close_tag(&mut self, defs: bool, indent: usize, name: &str) -> Result<(), Error> {
        let mut s = String::new();
        for _ in 0..indent {
            s.push_str("  ");
        }
        s.push_str("</");
        s.push_str(name);
        s.push_str(">\n");
        self.push(defs, &s)
    }

    /// Assembles the final document from the collected buffers.
    fn finish(self, w_px: f64, h_px: f64) -> Result<String, Error> {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\"");
        out.push_str(&format!(
            " width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
            num(w_px),
            num(h_px),
            num(w_px),
            num(h_px)
        ));
        if !self.defs.is_empty() {
            out.push_str("  <defs>\n");
            out.push_str(&self.defs);
            out.push_str("  </defs>\n");
        }
        out.push_str(&format!(
            "  <g transform=\"matrix(1.000000 0.000000 0.000000 -1.000000 0.000000 {})\">\n",
            num(h_px)
        ));
        out.push_str(&self.body);
        out.push_str("  </g>\n</svg>\n");
        if out.len() > self.cap {
            return Err(Error::TooLarge);
        }
        Ok(out)
    }

    /// Writes one node of the scene graph at the given indent.
    fn node(&mut self, node: &Node, indent: usize, depth: usize) -> Result<(), Error> {
        match node {
            Node::Group(group) => self.group(group, indent, depth),
            Node::Shape(shape) => self.shape(shape, indent),
        }
    }

    /// Writes a group node: a `g` with its transform and clip, then its
    /// children in draw order.
    fn group(&mut self, group: &Group, indent: usize, depth: usize) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        let mut attrs = Vec::new();
        if let Some(transform) = matrix_attr(group.transform, self.scale) {
            attrs.push(("transform".to_string(), transform));
        }
        if let Some(clip) = &group.clip {
            let clip = self.clip_def(clip)?;
            attrs.push(("clip-path".to_string(), clip));
        }
        self.tag(false, indent, "g", &attrs, false)?;
        for child in &group.children {
            self.node(child, indent + 1, depth + 1)?;
        }
        self.close_tag(false, indent, "g")
    }

    /// Writes a shape node. A plain shape is one `path` carrying the
    /// transform; a shape with a clip, a shadow, a line end or text is
    /// a `g` carrying the transform and clip, with the shape's parts as
    /// children in draw order (shadow, fill and stroke, line ends,
    /// glyphs).
    fn shape(&mut self, shape: &Shape, indent: usize) -> Result<(), Error> {
        let stroke = shape.stroke.as_ref();
        let has_ends = stroke.is_some_and(|s| s.start.is_some() || s.end.is_some());
        let wrapped =
            shape.clip.is_some() || shape.shadow.is_some() || shape.text.is_some() || has_ends;
        if !wrapped {
            let mut attrs = vec![("d".to_string(), path_d(&shape.path, self.scale))];
            if let Some(transform) = matrix_attr(shape.transform, self.scale) {
                attrs.push(("transform".to_string(), transform));
            }
            attrs.extend(fill_attrs(self, shape.fill.as_ref())?);
            if let Some(stroke) = stroke {
                attrs.extend(self.stroke_attrs(stroke)?);
            }
            return self.tag(false, indent, "path", &attrs, true);
        }
        let mut attrs = Vec::new();
        if let Some(transform) = matrix_attr(shape.transform, self.scale) {
            attrs.push(("transform".to_string(), transform));
        }
        if let Some(clip) = &shape.clip {
            let clip = self.clip_def(clip)?;
            attrs.push(("clip-path".to_string(), clip));
        }
        self.tag(false, indent, "g", &attrs, false)?;
        if let Some(shadow) = &shape.shadow {
            self.shadow(shape, shadow, indent + 1)?;
        }
        let mut attrs = vec![("d".to_string(), path_d(&shape.path, self.scale))];
        attrs.extend(fill_attrs(self, shape.fill.as_ref())?);
        if let Some(stroke) = stroke {
            attrs.extend(self.stroke_attrs(stroke)?);
        }
        self.tag(false, indent + 1, "path", &attrs, true)?;
        if let Some(stroke) = stroke {
            self.line_ends(&shape.path, stroke, indent + 1)?;
        }
        if let Some(text) = &shape.text {
            self.text(text, indent + 1)?;
        }
        self.close_tag(false, indent, "g")
    }

    /// Writes the shadow of a shape: an offset copy of its path painted
    /// in the shadow colour, drawn before the shape itself.
    fn shadow(
        &mut self,
        shape: &Shape,
        shadow: &crate::scene::Shadow,
        indent: usize,
    ) -> Result<(), Error> {
        let mut attrs = vec![("d".to_string(), path_d(&shape.path, self.scale))];
        let offset = format!(
            "matrix(1.000000 0.000000 0.000000 1.000000 {} {})",
            num(shadow.offset.x * self.scale),
            num(shadow.offset.y * self.scale)
        );
        attrs.push(("transform".to_string(), offset));
        attrs.extend(fill_attrs(self, Some(&Paint::Solid(shadow.color)))?);
        if let Some(stroke) = &shape.stroke {
            let (value, opacity) = fill_value(self, &stroke.paint)?;
            attrs.push(("stroke".to_string(), value));
            if let Some(opacity) = opacity {
                attrs.push(("stroke-opacity".to_string(), opacity));
            }
            attrs.push(("stroke-width".to_string(), num(stroke.width * self.scale)));
        }
        self.tag(false, indent, "path", &attrs, true)
    }

    /// Writes the line ends of one stroked path (the unit-box
    /// convention of `render::styles`, ADR 0023; see the module docs).
    fn line_ends(&mut self, path: &Path, stroke: &Stroke, indent: usize) -> Result<(), Error> {
        for (end, at_start) in [(&stroke.start, true), (&stroke.end, false)] {
            let Some(end) = end else { continue };
            let Some(shape) = line_end_path(path, end, at_start) else {
                continue;
            };
            let mut attrs = vec![("d".to_string(), path_d(&shape, self.scale))];
            if end.filled {
                let paint = match end.color {
                    Some(color) => Paint::Solid(color),
                    None => stroke.paint.clone(),
                };
                attrs.extend(fill_attrs(self, Some(&paint))?);
            } else {
                attrs.push(("fill".to_string(), "none".to_string()));
                let (stroke_value, stroke_opacity) = fill_value(self, &stroke.paint)?;
                attrs.push(("stroke".to_string(), stroke_value));
                if let Some(opacity) = stroke_opacity {
                    attrs.push(("stroke-opacity".to_string(), opacity));
                }
                attrs.push(("stroke-width".to_string(), num(stroke.width * self.scale)));
            }
            self.tag(false, indent, "path", &attrs, true)?;
        }
        Ok(())
    }

    /// Writes a text block: one filled `path` per glyph outline (ADR
    /// 0025: never a `<text>` element). A glyph whose font index is out
    /// of range, whose outline fails or is empty is skipped.
    fn text(&mut self, text: &TextNode, indent: usize) -> Result<(), Error> {
        for line in &text.layout.lines {
            for glyph in &line.glyphs {
                if !glyph.size.is_finite() || glyph.size <= 0.0 {
                    continue;
                }
                let Some(font) = text.fonts.get(glyph.font) else {
                    continue;
                };
                let Ok(outline) = font.outline(glyph.glyph) else {
                    continue;
                };
                if outline.is_empty() {
                    continue;
                }
                let mut attrs = vec![
                    ("d".to_string(), path_d(&outline, 1.0)),
                    (
                        "transform".to_string(),
                        self.glyph_transform(text, glyph.size, glyph.x, glyph.y),
                    ),
                ];
                let color = text.runs.get(glyph.run).map_or(text.color, |run| run.color);
                attrs.extend(fill_attrs(self, Some(&Paint::Solid(color)))?);
                self.tag(false, indent, "path", &attrs, true)?;
            }
        }
        Ok(())
    }

    /// The transform of one glyph outline (em units, y up) into the
    /// shape-local pixel space.
    ///
    /// The layout position is in the block's y-down local space and the
    /// outline is rotated by minus the block rotation (ADR 0024
    /// amendment): `matrix(k·cosθ, −k·sinθ, k·sinθ, k·cosθ, e, f)` with
    /// `k` the glyph size in pixels per em.
    fn glyph_transform(&self, text: &TextNode, size_pt: f64, x: f64, y: f64) -> String {
        let k = (size_pt / 72.0) * self.scale;
        let (sin, cos) = text.layout.rotation.radians().sin_cos();
        let e = (text.origin.x + x) * self.scale;
        let f = (text.origin.y - y) * self.scale;
        format!(
            "matrix({} {} {} {} {} {})",
            num(k * cos),
            num(-k * sin),
            num(k * sin),
            num(k * cos),
            num(e),
            num(f)
        )
    }

    /// The stroke attributes of one stroke, in fixed order. Cap, join
    /// and miter limit are written only when they differ from the SVG
    /// defaults.
    fn stroke_attrs(&mut self, stroke: &Stroke) -> Result<Vec<Attr>, Error> {
        let mut attrs = Vec::new();
        let (value, opacity) = fill_value(self, &stroke.paint)?;
        attrs.push(("stroke".to_string(), value));
        if let Some(opacity) = opacity {
            attrs.push(("stroke-opacity".to_string(), opacity));
        }
        attrs.push(("stroke-width".to_string(), num(stroke.width * self.scale)));
        if !stroke.dash.is_empty() {
            let dashes: Vec<String> = stroke
                .dash
                .iter()
                .map(|d| num((*d * self.scale).max(0.0)))
                .collect();
            attrs.push(("stroke-dasharray".to_string(), dashes.join(" ")));
        }
        match stroke.cap {
            LineCap::Butt => {}
            LineCap::Round => attrs.push(("stroke-linecap".to_string(), "round".to_string())),
            LineCap::Square => attrs.push(("stroke-linecap".to_string(), "square".to_string())),
        }
        match stroke.join {
            LineJoin::Miter => {}
            LineJoin::Round => attrs.push(("stroke-linejoin".to_string(), "round".to_string())),
            LineJoin::Bevel => attrs.push(("stroke-linejoin".to_string(), "bevel".to_string())),
        }
        if stroke.miter_limit.is_finite()
            && stroke.miter_limit >= 1.0
            && stroke.miter_limit != DEFAULT_MITER_LIMIT
        {
            attrs.push(("stroke-miterlimit".to_string(), num(stroke.miter_limit)));
        }
        Ok(attrs)
    }

    /// Registers a clip path in the defs and returns its `url(#...)`
    /// reference.
    fn clip_def(&mut self, clip: &Clip) -> Result<String, Error> {
        let id = format!("c{}", self.clips);
        self.clips = self.clips.saturating_add(1);
        self.tag(
            true,
            2,
            "clipPath",
            &[("id".to_string(), id.clone())],
            false,
        )?;
        let attrs = vec![("d".to_string(), path_d(&clip.path, self.scale))];
        self.tag(true, 3, "path", &attrs, true)?;
        self.close_tag(true, 2, "clipPath")?;
        Ok(format!("url(#{id})"))
    }

    /// Registers a gradient or pattern paint in the defs and returns
    /// its `url(#...)` reference. A solid has no defs entry and writes
    /// as its `#rrggbb` value.
    fn paint_def(&mut self, paint: &Paint) -> Result<String, Error> {
        match paint {
            Paint::Solid(color) => Ok(color_hex(*color)),
            Paint::LinearGradient { start, end, stops } => {
                let id = format!("g{}", self.gradients);
                self.gradients = self.gradients.saturating_add(1);
                let attrs = vec![
                    ("id".to_string(), id.clone()),
                    ("gradientUnits".to_string(), "userSpaceOnUse".to_string()),
                    ("x1".to_string(), num(start.x * self.scale)),
                    ("y1".to_string(), num(start.y * self.scale)),
                    ("x2".to_string(), num(end.x * self.scale)),
                    ("y2".to_string(), num(end.y * self.scale)),
                ];
                self.tag(true, 2, "linearGradient", &attrs, false)?;
                self.stops(stops)?;
                self.close_tag(true, 2, "linearGradient")?;
                Ok(format!("url(#{id})"))
            }
            Paint::RadialGradient {
                center,
                radius,
                stops,
            } => {
                let id = format!("g{}", self.gradients);
                self.gradients = self.gradients.saturating_add(1);
                let attrs = vec![
                    ("id".to_string(), id.clone()),
                    ("gradientUnits".to_string(), "userSpaceOnUse".to_string()),
                    ("cx".to_string(), num(center.x * self.scale)),
                    ("cy".to_string(), num(center.y * self.scale)),
                    ("r".to_string(), num(radius.abs() * self.scale)),
                ];
                self.tag(true, 2, "radialGradient", &attrs, false)?;
                self.stops(stops)?;
                self.close_tag(true, 2, "radialGradient")?;
                Ok(format!("url(#{id})"))
            }
            Paint::Pattern(pattern) => {
                let id = format!("p{}", self.patterns);
                self.patterns = self.patterns.saturating_add(1);
                let tile_w = pattern.tile_size.width.abs() * self.scale;
                let tile_h = pattern.tile_size.height.abs() * self.scale;
                let attrs = vec![
                    ("id".to_string(), id.clone()),
                    ("patternUnits".to_string(), "userSpaceOnUse".to_string()),
                    ("x".to_string(), "0.000000".to_string()),
                    ("y".to_string(), "0.000000".to_string()),
                    ("width".to_string(), num(tile_w)),
                    ("height".to_string(), num(tile_h)),
                ];
                self.tag(true, 2, "pattern", &attrs, false)?;
                self.pattern_tile(pattern, tile_w, tile_h)?;
                self.close_tag(true, 2, "pattern")?;
                Ok(format!("url(#{id})"))
            }
        }
    }

    /// Writes one pattern's background box and tile mark. The mark fills
    /// when [`PatternPaint::filled`] and strokes with
    /// [`PatternPaint::tile_stroke_width`] otherwise, the same mark the
    /// PNG backend rasterizes.
    fn pattern_tile(
        &mut self,
        pattern: &PatternPaint,
        tile_w: f64,
        tile_h: f64,
    ) -> Result<(), Error> {
        let box_d = format!(
            "M 0.000000 0.000000 L {} 0.000000 L {} {} L 0.000000 {} Z",
            num(tile_w),
            num(tile_w),
            num(tile_h),
            num(tile_h)
        );
        let mut box_attrs = vec![("d".to_string(), box_d)];
        box_attrs.extend(fill_attrs(self, Some(&Paint::Solid(pattern.background)))?);
        self.tag(true, 3, "path", &box_attrs, true)?;
        let mut tile_attrs = vec![("d".to_string(), path_d(&pattern.tile, self.scale))];
        if pattern.filled {
            tile_attrs.extend(fill_attrs(self, Some(&Paint::Solid(pattern.foreground)))?);
        } else {
            let width = if pattern.tile_stroke_width.is_finite() {
                (pattern.tile_stroke_width * self.scale).max(0.0)
            } else {
                0.0
            };
            tile_attrs.push(("fill".to_string(), "none".to_string()));
            tile_attrs.push(("stroke".to_string(), color_hex(pattern.foreground)));
            if pattern.foreground.a < 255 {
                tile_attrs.push((
                    "stroke-opacity".to_string(),
                    num(f64::from(pattern.foreground.a) / 255.0),
                ));
            }
            tile_attrs.push(("stroke-width".to_string(), num(width)));
        }
        self.tag(true, 3, "path", &tile_attrs, true)
    }

    /// Writes one gradient's stops.
    fn stops(&mut self, stops: &[GradientStop]) -> Result<(), Error> {
        for stop in stops {
            let offset = if stop.offset.is_finite() {
                stop.offset.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut attrs = vec![
                ("offset".to_string(), num(offset)),
                ("stop-color".to_string(), color_hex(stop.color)),
            ];
            if stop.color.a < 255 {
                attrs.push((
                    "stop-opacity".to_string(),
                    num(f64::from(stop.color.a) / 255.0),
                ));
            }
            self.tag(true, 3, "stop", &attrs, true)?;
        }
        Ok(())
    }
}

/// The fill attributes of a paint: `fill` plus `fill-opacity` for a
/// translucent solid, or `fill="none"` for no paint. A gradient or
/// pattern is a `url(#...)` reference whose defs entry is registered on
/// the writer.
fn fill_attrs(writer: &mut Writer, paint: Option<&Paint>) -> Result<Vec<Attr>, Error> {
    let Some(paint) = paint else {
        return Ok(vec![("fill".to_string(), "none".to_string())]);
    };
    let (value, opacity) = fill_value(writer, paint)?;
    let mut attrs = vec![("fill".to_string(), value)];
    if let Some(opacity) = opacity {
        attrs.push(("fill-opacity".to_string(), opacity));
    }
    Ok(attrs)
}

/// The paint reference and its element-level opacity, if any. A
/// gradient or pattern carries its alpha inside (stops, tiles), so it
/// has no element-level opacity.
fn fill_value(writer: &mut Writer, paint: &Paint) -> Result<(String, Option<String>), Error> {
    match paint {
        Paint::Solid(color) => {
            let opacity = if color.a < 255 {
                Some(num(f64::from(color.a) / 255.0))
            } else {
                None
            };
            Ok((color_hex(*color), opacity))
        }
        _ => Ok((writer.paint_def(paint)?, None)),
    }
}

/// The `#rrggbb` form of a colour; alpha is written separately.
fn color_hex(color: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// The `matrix(a b c d e f)` attribute of a node transform, or [`None`]
/// for the identity. The linear part is unchanged (a uniform scale
/// commutes with it); the translation is scaled to pixels.
fn matrix_attr(transform: Transform, scale: f64) -> Option<String> {
    if transform == Transform::identity() {
        return None;
    }
    Some(format!(
        "matrix({} {} {} {} {} {})",
        num(transform.m11),
        num(transform.m21),
        num(transform.m12),
        num(transform.m22),
        num(transform.dx * scale),
        num(transform.dy * scale)
    ))
}

/// The `d` attribute of a path, with every coordinate scaled.
fn path_d(path: &Path, scale: f64) -> String {
    let mut parts: Vec<String> = Vec::new();
    for segment in path.segments() {
        match segment {
            Segment::MoveTo(p) => parts.push(format!("M {}", point(p, scale))),
            Segment::LineTo(p) => parts.push(format!("L {}", point(p, scale))),
            Segment::QuadTo { ctrl, to } => {
                parts.push(format!("Q {} {}", point(ctrl, scale), point(to, scale)));
            }
            Segment::CubicTo { ctrl1, ctrl2, to } => {
                parts.push(format!(
                    "C {} {} {}",
                    point(ctrl1, scale),
                    point(ctrl2, scale),
                    point(to, scale)
                ));
            }
            Segment::Close => parts.push("Z".to_string()),
            _ => {}
        }
    }
    parts.join(" ")
}

/// One point of a `d` attribute: two scaled numbers.
fn point(p: &Point, scale: f64) -> String {
    format!("{} {}", num(p.x * scale), num(p.y * scale))
}

/// One number with fixed 6-decimal formatting (ADR 0025). A non-finite
/// value or a negative zero writes as `0.000000`, so bad scene values
/// never produce invalid SVG numbers and never panic.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0.000000".to_string();
    }
    let formatted = format!("{value:.6}");
    if formatted == "-0.000000" {
        "0.000000".to_string()
    } else {
        formatted
    }
}

/// Escapes an attribute value for XML.
fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Maps a line-end unit box onto one end of a path (the unit-box
/// convention of `render::styles`, ADR 0023): `x = 0` is the tip at the
/// path's vertex, `x = 1` is the tail along the line, `y = 0.5` is the
/// line centre; `length` scales `x` and `width` scales `y`. Returns
/// [`None`] when the path carries no direction at that end.
fn line_end_path(path: &Path, end: &LineEnd, at_start: bool) -> Option<Path> {
    let (vertex, dir) = endpoint(path, at_start)?;
    let across_dir = Point::new(-dir.y, dir.x);
    let sign = if at_start { 1.0 } else { -1.0 };
    let map = |p: Point| {
        let along = dir.scale(sign * p.x * end.length);
        let across = across_dir.scale((p.y - 0.5) * end.width);
        Point::new(vertex.x + along.x + across.x, vertex.y + along.y + across.y)
    };
    let mut out = Path::new();
    for segment in end.shape.segments() {
        out.push(match segment {
            Segment::MoveTo(p) => Segment::MoveTo(map(*p)),
            Segment::LineTo(p) => Segment::LineTo(map(*p)),
            Segment::QuadTo { ctrl, to } => Segment::QuadTo {
                ctrl: map(*ctrl),
                to: map(*to),
            },
            Segment::CubicTo { ctrl1, ctrl2, to } => Segment::CubicTo {
                ctrl1: map(*ctrl1),
                ctrl2: map(*ctrl2),
                to: map(*to),
            },
            Segment::Close => Segment::Close,
            _ => continue,
        });
    }
    Some(out)
}

/// The vertex and unit direction at one end of a path. The direction
/// follows the path's travel: into the path at the start, out of it at
/// the end. [`None`] when that end has no direction (an empty or
/// degenerate path).
fn endpoint(path: &Path, at_start: bool) -> Option<(Point, Point)> {
    if at_start {
        let mut prev: Option<Point> = None;
        for segment in path.segments() {
            match segment {
                Segment::MoveTo(p) => {
                    if prev.is_some() {
                        return None;
                    }
                    prev = Some(*p);
                }
                Segment::LineTo(p) => {
                    let start = prev?;
                    return Some((start, unit(p.sub(start))?));
                }
                Segment::QuadTo { ctrl, .. } => {
                    let start = prev?;
                    return Some((start, unit(ctrl.sub(start))?));
                }
                Segment::CubicTo { ctrl1, .. } => {
                    let start = prev?;
                    return Some((start, unit(ctrl1.sub(start))?));
                }
                _ => return None,
            }
        }
        return None;
    }
    let mut sub_start: Option<Point> = None;
    let mut prev: Option<Point> = None;
    let mut last: Option<(Point, Point)> = None;
    for segment in path.segments() {
        match segment {
            Segment::MoveTo(p) => {
                sub_start = Some(*p);
                prev = Some(*p);
                last = None;
            }
            Segment::LineTo(p) => {
                if let Some(q) = prev
                    && let Some(dir) = unit(p.sub(q))
                {
                    last = Some((*p, dir));
                }
                prev = Some(*p);
            }
            Segment::QuadTo { ctrl, to } => {
                if let Some(dir) = unit(to.sub(*ctrl)) {
                    last = Some((*to, dir));
                }
                prev = Some(*to);
            }
            Segment::CubicTo { ctrl2, to, .. } => {
                if let Some(dir) = unit(to.sub(*ctrl2)) {
                    last = Some((*to, dir));
                }
                prev = Some(*to);
            }
            Segment::Close => {
                if let (Some(start), Some(q)) = (sub_start, prev)
                    && let Some(dir) = unit(start.sub(q))
                {
                    last = Some((start, dir));
                }
                prev = sub_start;
            }
            _ => {}
        }
    }
    last
}

/// The unit vector of `v`, or [`None`] when it is zero or not finite.
fn unit(v: Point) -> Option<Point> {
    if !v.x.is_finite() || !v.y.is_finite() {
        return None;
    }
    let length = (v.x * v.x + v.y * v.y).sqrt();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    Some(Point::new(v.x / length, v.y / length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Shadow;
    use larrygram_geom::Angle;
    use larrygram_text::{GlyphId, GlyphPos, Layout, Line, default_fonts};

    fn color(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba::new(r, g, b, a)
    }

    fn solid(r: u8, g: u8, b: u8) -> Paint {
        Paint::Solid(Rgba::opaque(r, g, b))
    }

    fn plain_stroke(paint: Paint) -> Stroke {
        Stroke {
            paint,
            width: 0.01,
            dash: Vec::new(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            start: None,
            end: None,
        }
    }

    fn unit_path(points: &[(f64, f64)], close: bool) -> Path {
        let mut path = Path::new();
        for (i, (x, y)) in points.iter().enumerate() {
            let p = Point::new(*x, *y);
            if i == 0 {
                path.push(Segment::MoveTo(p));
            } else {
                path.push(Segment::LineTo(p));
            }
        }
        if close {
            path.push(Segment::Close);
        }
        path
    }

    fn line_node() -> Shape {
        Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0)], false),
            fill: None,
            stroke: Some(plain_stroke(solid(0, 0, 0))),
            shadow: None,
            clip: None,
            text: None,
        }
    }

    fn parse_xml(svg: &str) {
        let mut reader = quick_xml::Reader::from_str(svg);
        let mut depth = 0usize;
        let mut seen_svg = false;
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(_)) => {
                    depth += 1;
                    seen_svg = true;
                }
                Ok(quick_xml::events::Event::End(_)) => depth = depth.saturating_sub(1),
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("XML does not parse: {e}"),
            }
        }
        assert!(seen_svg, "no elements parsed");
        assert_eq!(depth, 0, "unbalanced tags");
    }

    #[test]
    fn options_default_to_96_dpi() {
        assert_eq!(SvgOptions::new().scale, 96.0);
        assert_eq!(SvgOptions::default(), SvgOptions::new());
    }

    #[test]
    fn empty_scene_is_a_standalone_document() {
        let scene = Scene::new(2.0, 1.0);
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert_eq!(
            svg,
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" ",
                "width=\"192.000000\" height=\"96.000000\" viewBox=\"0 0 192.000000 96.000000\">\n",
                "  <g transform=\"matrix(1.000000 0.000000 0.000000 -1.000000 0.000000 96.000000)\">\n",
                "  </g>\n",
                "</svg>\n",
            )
        );
    }

    #[test]
    fn background_and_one_filled_stroked_shape_pin_the_output() {
        let mut scene = Scene::new(2.0, 1.0);
        scene.background = Some(Rgba::opaque(255, 255, 255));
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::Solid(color(255, 0, 0, 128))),
            stroke: Some(plain_stroke(solid(0, 0, 0))),
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert_eq!(
            svg,
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" ",
                "width=\"192.000000\" height=\"96.000000\" viewBox=\"0 0 192.000000 96.000000\">\n",
                "  <g transform=\"matrix(1.000000 0.000000 0.000000 -1.000000 0.000000 96.000000)\">\n",
                "    <path d=\"M 0.000000 0.000000 L 192.000000 0.000000 L 192.000000 96.000000 ",
                "L 0.000000 96.000000 Z\" fill=\"#ffffff\" />\n",
                "    <path d=\"M 0.000000 0.000000 L 96.000000 0.000000 L 96.000000 96.000000 Z\" ",
                "fill=\"#ff0000\" fill-opacity=\"0.501961\" stroke=\"#000000\" ",
                "stroke-width=\"0.960000\" />\n",
                "  </g>\n",
                "</svg>\n",
            )
        );
    }

    #[test]
    fn the_same_scene_renders_byte_identically() {
        let mut scene = Scene::new(3.0, 2.0);
        scene.background = Some(color(255, 255, 255, 255));
        let gradient = Paint::LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 0.0),
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: color(255, 0, 0, 255),
                },
                GradientStop {
                    offset: 1.0,
                    color: color(0, 0, 255, 128),
                },
            ],
        };
        let mut stroke = plain_stroke(gradient);
        stroke.dash = vec![0.1, 0.05];
        stroke.cap = LineCap::Round;
        stroke.join = LineJoin::Bevel;
        stroke.miter_limit = 8.0;
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0)], true),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: Some(Rgba::opaque(0, 128, 0)),
        });
        let shape = Shape {
            shape_id: None,
            transform: Transform::translate(Point::new(1.0, 1.0)),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], true),
            fill: Some(Paint::Pattern(PatternPaint {
                foreground: Rgba::opaque(0, 0, 0),
                background: Rgba::opaque(255, 255, 255),
                tile: unit_path(&[(0.0, 0.0), (0.05, 0.05), (0.1, 0.0)], false),
                tile_size: larrygram_geom::Size::new(0.1, 0.1),
                filled: false,
                tile_stroke_width: 0.01,
            })),
            stroke: Some(stroke),
            shadow: Some(Shadow {
                color: color(0, 0, 0, 64),
                offset: Point::new(0.02, -0.02),
                blur: 0.01,
            }),
            clip: Some(Clip {
                path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
            }),
            text: None,
        };
        let group = Group {
            transform: Transform::identity(),
            clip: None,
            children: vec![Node::Shape(Box::new(shape))],
        };
        scene.nodes.push(Node::Group(group));
        let first = render_svg(&scene, &SvgOptions::new()).unwrap();
        let second = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert_eq!(first, second);
        parse_xml(&first);
    }

    #[test]
    fn a_linear_gradient_is_defined_once_and_referenced() {
        let mut scene = Scene::new(1.0, 1.0);
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::LinearGradient {
                start: Point::new(0.1, 0.2),
                end: Point::new(0.3, 0.4),
                stops: vec![GradientStop {
                    offset: -0.5,
                    color: color(255, 0, 0, 128),
                }],
            }),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains(
            "<linearGradient id=\"g0\" gradientUnits=\"userSpaceOnUse\" x1=\"9.600000\" \
             y1=\"19.200000\" x2=\"28.800000\" y2=\"38.400000\">"
        ));
        assert!(svg.contains(
            "<stop offset=\"0.000000\" stop-color=\"#ff0000\" stop-opacity=\"0.501961\" />"
        ));
        assert!(svg.contains("fill=\"url(#g0)\""));
        assert!(svg.contains("  <defs>\n"));
        parse_xml(&svg);
    }

    #[test]
    fn a_radial_gradient_is_defined_in_the_defs() {
        let mut scene = Scene::new(1.0, 1.0);
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::RadialGradient {
                center: Point::new(0.5, 0.5),
                radius: 0.25,
                stops: vec![GradientStop {
                    offset: 2.0,
                    color: Rgba::opaque(0, 0, 255),
                }],
            }),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains(
            "<radialGradient id=\"g0\" gradientUnits=\"userSpaceOnUse\" cx=\"48.000000\" \
             cy=\"48.000000\" r=\"24.000000\">"
        ));
        assert!(svg.contains("<stop offset=\"1.000000\" stop-color=\"#0000ff\" />"));
        parse_xml(&svg);
    }

    #[test]
    fn a_closed_pattern_tile_fills_over_its_background_box() {
        let mut scene = Scene::new(1.0, 1.0);
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::Pattern(PatternPaint {
                foreground: color(0, 0, 0, 255),
                background: color(255, 255, 255, 128),
                tile: unit_path(&[(0.0, 0.0), (0.25, 0.0), (0.25, 0.5)], true),
                tile_size: larrygram_geom::Size::new(0.25, 0.5),
                filled: true,
                tile_stroke_width: 0.05,
            })),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains(
            "<pattern id=\"p0\" patternUnits=\"userSpaceOnUse\" x=\"0.000000\" y=\"0.000000\" \
             width=\"24.000000\" height=\"48.000000\">"
        ));
        assert!(svg.contains(
            "<path d=\"M 0.000000 0.000000 L 24.000000 0.000000 L 24.000000 48.000000 \
             L 0.000000 48.000000 Z\" fill=\"#ffffff\" fill-opacity=\"0.501961\" />"
        ));
        assert!(svg.contains(
            "<path d=\"M 0.000000 0.000000 L 24.000000 0.000000 L 24.000000 48.000000 Z\" \
             fill=\"#000000\" />"
        ));
        parse_xml(&svg);
    }

    #[test]
    fn an_open_pattern_tile_strokes_with_its_width() {
        // The scene is the single source of drawing intent (ADR 0020,
        // P3-16): the tile strokes with `tile_stroke_width`, not with a
        // hairline of its own.
        let mut scene = Scene::new(1.0, 1.0);
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::Pattern(PatternPaint {
                foreground: Rgba::opaque(0, 0, 0),
                background: Rgba::opaque(255, 255, 255),
                tile: unit_path(&[(0.0, 0.0), (0.1, 0.1)], false),
                tile_size: larrygram_geom::Size::new(0.1, 0.1),
                filled: false,
                tile_stroke_width: 0.02,
            })),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // 0.02 in at 96 pixels per inch.
        assert!(svg.contains("stroke-width=\"1.920000\""));
        assert!(svg.contains("<path d=\"M 0.000000 0.000000 L 9.600000 9.600000\" fill=\"none\""));
        parse_xml(&svg);
    }

    #[test]
    fn stroke_attributes_follow_the_fixed_order() {
        let mut scene = Scene::new(1.0, 1.0);
        let mut stroke = plain_stroke(solid(0, 0, 255));
        stroke.dash = vec![0.1, -0.05, f64::NAN];
        stroke.cap = LineCap::Square;
        stroke.join = LineJoin::Round;
        stroke.miter_limit = 8.0;
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0)], false),
            fill: None,
            stroke: Some(stroke),
            shadow: None,
            clip: None,
            text: None,
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains(
            "<path d=\"M 0.000000 0.000000 L 96.000000 0.000000\" fill=\"none\" \
             stroke=\"#0000ff\" stroke-width=\"0.960000\" stroke-dasharray=\"9.600000 0.000000 \
             0.000000\" stroke-linecap=\"square\" stroke-linejoin=\"round\" \
             stroke-miterlimit=\"8.000000\" />"
        ));
        parse_xml(&svg);
    }

    #[test]
    fn a_filled_line_end_sits_at_the_path_end_in_the_unit_box() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        let mut stroke = shape
            .stroke
            .take()
            .unwrap_or_else(|| plain_stroke(solid(0, 0, 0)));
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0)], true),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: Some(Rgba::opaque(255, 0, 0)),
        });
        shape.stroke = Some(stroke);
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // Vertex (1, 0) in = (96, 0) px; tip at x = 0; the tail at x = 1
        // is 0.4 in back along the line = 38.4 px; y = 0 and y = 1 are
        // 0.1 in off the centre line = 9.6 px.
        assert!(svg.contains(
            "<path d=\"M 96.000000 0.000000 L 57.600000 -9.600000 L 57.600000 9.600000 Z\" \
             fill=\"#ff0000\" />"
        ));
        parse_xml(&svg);
    }

    #[test]
    fn an_unfilled_line_end_strokes_with_the_stroke_paint() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        let mut stroke = shape
            .stroke
            .take()
            .unwrap_or_else(|| plain_stroke(solid(0, 0, 0)));
        stroke.start = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0)], false),
            width: 0.2,
            length: 0.4,
            filled: false,
            color: None,
        });
        shape.stroke = Some(stroke);
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // Vertex (0, 0); the start tail lies into the line, +x.
        assert!(svg.contains(
            "<path d=\"M 0.000000 0.000000 L 38.400000 -9.600000\" fill=\"none\" \
             stroke=\"#000000\" stroke-width=\"0.960000\" />"
        ));
        parse_xml(&svg);
    }

    #[test]
    fn a_line_end_on_a_directionless_path_is_not_drawn() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        shape.path = unit_path(&[(0.5, 0.5)], false);
        let mut stroke = shape
            .stroke
            .take()
            .unwrap_or_else(|| plain_stroke(solid(0, 0, 0)));
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0)], true),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        });
        stroke.start = stroke.end.clone();
        shape.stroke = Some(stroke);
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // The wrapper `g` and the shape's own path: both line ends are
        // skipped because the one-point path has no direction.
        assert_eq!(svg.matches("<path").count(), 1);
        parse_xml(&svg);
    }

    #[test]
    fn a_line_end_fills_with_the_stroke_paint_when_it_has_no_colour() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        shape.fill = Some(solid(1, 2, 3));
        let mut stroke = shape
            .stroke
            .take()
            .unwrap_or_else(|| plain_stroke(solid(0, 0, 0)));
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0)], true),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        });
        shape.stroke = Some(stroke);
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("fill=\"#000000\""));
        parse_xml(&svg);
    }

    #[test]
    fn a_shadow_draws_an_offset_copy_behind_the_shape() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        shape.fill = Some(solid(255, 0, 0));
        shape.shadow = Some(Shadow {
            color: color(0, 0, 0, 64),
            offset: Point::new(0.02, -0.02),
            blur: 0.05,
        });
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        let shadow = "transform=\"matrix(1.000000 0.000000 0.000000 1.000000 1.920000 -1.920000)\" fill=\"#000000\" \
                      fill-opacity=\"0.250980\"";
        assert!(svg.contains(shadow));
        // The shadow copy comes before the shape's own path.
        let shadow_at = svg.find(shadow).unwrap_or(0);
        let shape_at = svg.find("fill=\"#ff0000\"").unwrap_or(usize::MAX);
        assert!(shadow_at < shape_at, "shadow must draw first");
        // The blur is not drawn: the subset has no filter element.
        assert!(!svg.contains("filter"));
        parse_xml(&svg);
    }

    #[test]
    fn clips_go_through_the_defs_and_the_referencing_element() {
        let mut scene = Scene::new(2.0, 1.0);
        let mut shape = line_node();
        shape.clip = Some(Clip {
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
        });
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let group = Group {
            transform: Transform::identity(),
            clip: Some(Clip {
                path: unit_path(&[(0.0, 0.0), (2.0, 0.0)], false),
            }),
            children: Vec::new(),
        };
        scene.nodes.push(Node::Group(group));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("<clipPath id=\"c0\">"));
        assert!(svg.contains("<clipPath id=\"c1\">"));
        assert!(svg.contains("clip-path=\"url(#c0)\""));
        assert!(svg.contains("clip-path=\"url(#c1)\""));
        assert_eq!(svg.matches("clipPath").count(), 4);
        parse_xml(&svg);
    }

    #[test]
    fn a_group_writes_its_transform_and_children() {
        let mut scene = Scene::new(1.0, 1.0);
        let shape = line_node();
        let inner = Group {
            transform: Transform::translate(Point::new(0.5, 0.5)),
            clip: None,
            children: vec![Node::Shape(Box::new(shape))],
        };
        let outer = Group {
            transform: Transform::identity(),
            clip: None,
            children: vec![Node::Group(inner)],
        };
        scene.nodes.push(Node::Group(outer));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("  <g>\n"));
        assert!(svg.contains(
            "    <g transform=\"matrix(1.000000 0.000000 0.000000 1.000000 48.000000 48.000000)\">\n"
        ));
        assert!(svg.contains("      <path d=\"M 0.000000 0.000000 L 96.000000 0.000000\""));
        parse_xml(&svg);
    }

    #[test]
    fn deep_nesting_beyond_the_cap_errors() {
        let mut node = Node::Shape(Box::new(line_node()));
        for _ in 0..=(MAX_DEPTH + 1) {
            node = Node::Group(Group {
                transform: Transform::identity(),
                clip: None,
                children: vec![node],
            });
        }
        let mut scene = Scene::new(1.0, 1.0);
        scene.nodes.push(node);
        assert_eq!(render_svg(&scene, &SvgOptions::new()), Err(Error::TooLarge));
    }

    #[test]
    fn text_draws_one_filled_outline_path_per_glyph() {
        let fonts = default_fonts();
        let Some(font) = fonts.fonts().first().cloned() else {
            return;
        };
        let glyph_id = GlyphId(36);
        let Ok(outline) = font.outline(glyph_id) else {
            return;
        };
        assert!(!outline.is_empty(), "test needs a glyph with an outline");
        let mut scene = Scene::new(2.0, 2.0);
        let text = TextNode {
            fonts: vec![font],
            runs: Vec::new(),
            layout: Layout {
                lines: vec![Line {
                    glyphs: vec![GlyphPos {
                        glyph: glyph_id,
                        font: 0,
                        x: 0.25,
                        y: 0.5,
                        run: 0,
                        size: 12.0,
                    }],
                    baseline: 0.5,
                    width: 0.2,
                    paragraph: 0,
                }],
                width: 0.2,
                height: 0.2,
                overflow: false,
                rotation: Angle::from_degrees(0.0),
                warnings: Vec::new(),
            },
            color: Rgba::opaque(0, 0, 255),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(1.0, 2.0),
        };
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: Path::new(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(text),
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // The glyph origin is (1.0 + 0.25, 2.0 - 0.5) in = (120, 144)
        // px; the em outline scales by 12 pt / 72 * 96 px/in = 16.
        assert!(svg.contains(
            "transform=\"matrix(16.000000 0.000000 0.000000 16.000000 120.000000 144.000000)\""
        ));
        assert!(svg.contains(&format!("d=\"{}\"", path_d(&outline, 1.0))));
        assert!(svg.contains("fill=\"#0000ff\""));
        assert!(!svg.contains("<text"));
        parse_xml(&svg);
    }

    #[test]
    fn glyphs_use_their_size_rotation_and_run_colour() {
        let fonts = default_fonts();
        let Some(font) = fonts.fonts().first().cloned() else {
            return;
        };
        let mut scene = Scene::new(2.0, 2.0);
        let text = TextNode {
            fonts: vec![font],
            runs: vec![crate::scene::TextRun {
                underline: false,
                strikethrough: false,
                color: Rgba::opaque(255, 0, 0),
            }],
            layout: Layout {
                lines: vec![Line {
                    glyphs: vec![GlyphPos {
                        glyph: GlyphId(36),
                        font: 0,
                        x: 0.25,
                        y: 0.5,
                        run: 0,
                        size: 24.0,
                    }],
                    baseline: 0.5,
                    width: 0.2,
                    paragraph: 0,
                }],
                width: 0.2,
                height: 0.2,
                overflow: false,
                rotation: Angle::from_degrees(90.0),
                warnings: Vec::new(),
            },
            color: Rgba::opaque(0, 0, 255),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(1.0, 2.0),
        };
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: Path::new(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(text),
        })));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // 24 pt at 96 px/in is 32 px per em; a 90 degree block rotation
        // turns the outline by minus 90 degrees in the y-up shape space.
        assert!(
            svg.contains(
                "transform=\"matrix(0.000000 -32.000000 32.000000 0.000000 120.000000 144.000000)\""
            ),
            "{svg}"
        );
        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(!svg.contains("fill=\"#0000ff\""));
    }

    #[test]
    fn text_skips_bad_font_indexes_and_empty_outlines() {
        let fonts = default_fonts();
        let Some(font) = fonts.fonts().first().cloned() else {
            return;
        };
        // An id the font does not have draws nothing (ADR 0024).
        let missing = GlyphId(60_000);
        let mut scene = Scene::new(2.0, 2.0);
        let text = TextNode {
            fonts: vec![font],
            runs: Vec::new(),
            layout: Layout {
                lines: vec![Line {
                    glyphs: vec![
                        GlyphPos {
                            glyph: missing,
                            font: 7,
                            x: 0.0,
                            y: 0.0,
                            run: 0,
                            size: 12.0,
                        },
                        GlyphPos {
                            glyph: missing,
                            font: 0,
                            x: 0.25,
                            y: 0.0,
                            run: 0,
                            size: 12.0,
                        },
                    ],
                    baseline: 0.5,
                    width: 0.5,
                    paragraph: 0,
                }],
                width: 0.5,
                height: 0.2,
                overflow: false,
                rotation: Angle::from_degrees(0.0),
                warnings: Vec::new(),
            },
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(0.0, 0.0),
        };
        let shape = Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: Path::new(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(text),
        };
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        // Both glyphs are skipped: only the shape's own empty path
        // remains inside the wrapper `g`.
        assert_eq!(svg.matches("<path").count(), 1);
        parse_xml(&svg);
    }

    #[test]
    fn non_finite_scene_values_write_as_zero() {
        assert_eq!(num(f64::NAN), "0.000000");
        assert_eq!(num(f64::INFINITY), "0.000000");
        assert_eq!(num(-0.0), "0.000000");
        assert_eq!(num(1.5), "1.500000");
        let mut scene = Scene::new(1.0, 1.0);
        let mut shape = line_node();
        shape.transform = Transform::translate(Point::new(f64::NAN, 1.0));
        shape.path = unit_path(&[(f64::INFINITY, 0.0), (1.0, 0.0)], false);
        scene.nodes.push(Node::Shape(Box::new(shape)));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("matrix(1.000000 0.000000 0.000000 1.000000 0.000000 96.000000)"));
        assert!(svg.contains("M 0.000000 0.000000"));
        parse_xml(&svg);
    }

    #[test]
    fn invalid_options_and_dimensions_error() {
        // The scale cap both backends share (ADR 0025 amendment, P3-16).
        let scene = Scene::new(1.0, 1.0);
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 1.0e300, 20_000.0] {
            let options = SvgOptions { scale };
            assert_eq!(render_svg(&scene, &options), Err(Error::InvalidOptions));
        }
        let options = SvgOptions::new();
        for (w, h) in [
            (-1.0, 1.0),
            (1.0, -1.0),
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::MAX, f64::MAX),
            // Over the device extent cap at the default scale.
            (200.0, 1.0),
        ] {
            let scene = Scene::new(w, h);
            assert_eq!(render_svg(&scene, &options), Err(Error::TooLarge));
        }
    }

    #[test]
    fn the_output_byte_cap_stops_the_writer() {
        let mut writer = Writer::new(96.0, 16);
        let attrs = vec![("d".to_string(), "M 0.000000 0.000000".to_string())];
        assert_eq!(
            writer.tag(false, 0, "path", &attrs, true),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn attribute_values_are_escaped() {
        assert_eq!(
            escape_attr("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
        assert_eq!(escape_attr("plain"), "plain");
    }

    #[test]
    fn line_end_geometry_is_hand_checked() {
        // The unit-box mapping: tip at the vertex, tail one length along
        // the line, width across the centre line.
        let end = LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 1.0)], false),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        };
        let path = unit_path(&[(0.0, 0.0), (1.0, 0.0)], false);
        let drawn = line_end_path(&path, &end, false).unwrap_or_default();
        let segments = drawn.segments();
        assert_eq!(
            segments.first(),
            Some(&Segment::MoveTo(Point::new(1.0, 0.0)))
        );
        assert_eq!(
            segments.get(1),
            Some(&Segment::LineTo(Point::new(0.6, 0.1)))
        );
        let drawn = line_end_path(&path, &end, true).unwrap_or_default();
        let segments = drawn.segments();
        assert_eq!(
            segments.first(),
            Some(&Segment::MoveTo(Point::new(0.0, 0.0)))
        );
        assert_eq!(
            segments.get(1),
            Some(&Segment::LineTo(Point::new(0.4, 0.1)))
        );
        // A closed path ends at its subpath start.
        let closed = unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true);
        let (vertex, dir) =
            endpoint(&closed, false).unwrap_or((Point::new(0.0, 0.0), Point::new(0.0, 0.0)));
        assert_eq!(vertex, Point::new(0.0, 0.0));
        assert!(dir.x < 0.0, "direction points back to the subpath start");
        // An empty path has no ends.
        assert_eq!(endpoint(&Path::new(), true), None);
        assert_eq!(endpoint(&Path::new(), false), None);
        // A zero-length direction is no direction.
        let zero = unit_path(&[(1.0, 1.0), (1.0, 1.0)], false);
        assert_eq!(endpoint(&zero, true), None);
    }

    #[test]
    fn unit_vectors_reject_zero_and_non_finite() {
        assert_eq!(unit(Point::new(0.0, 0.0)), None);
        assert_eq!(unit(Point::new(f64::NAN, 1.0)), None);
        assert_eq!(unit(Point::new(3.0, 4.0)), Some(Point::new(0.6, 0.8)));
    }

    #[test]
    fn curves_map_into_line_end_geometry() {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::QuadTo {
            ctrl: Point::new(1.0, 1.0),
            to: Point::new(2.0, 0.0),
        });
        path.push(Segment::CubicTo {
            ctrl1: Point::new(3.0, 1.0),
            ctrl2: Point::new(4.0, -1.0),
            to: Point::new(5.0, 0.0),
        });
        let end = LineEnd {
            shape: unit_path(&[(0.0, 0.5)], false),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        };
        let drawn = line_end_path(&path, &end, false).unwrap_or_default();
        // The tip sits on the end vertex either way.
        assert_eq!(
            drawn.segments().first(),
            Some(&Segment::MoveTo(Point::new(5.0, 0.0)))
        );
        let drawn = line_end_path(&path, &end, true).unwrap_or_default();
        assert_eq!(
            drawn.segments().first(),
            Some(&Segment::MoveTo(Point::new(0.0, 0.0)))
        );
    }

    #[test]
    fn the_kitchen_sink_renders_and_parses() {
        let mut scene = Scene::new(4.0, 3.0);
        scene.background = Some(Rgba::opaque(255, 255, 255));
        let mut stroke = plain_stroke(solid(0, 0, 0));
        stroke.dash = vec![0.1, 0.05];
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0)], true),
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        });
        let shape = Shape {
            shape_id: None,
            transform: Transform::translate(Point::new(1.0, 1.0)),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
            fill: Some(solid(200, 100, 50)),
            stroke: Some(stroke),
            shadow: Some(Shadow {
                color: color(0, 0, 0, 32),
                offset: Point::new(0.01, 0.01),
                blur: 0.02,
            }),
            clip: None,
            text: None,
        };
        let group = Group {
            transform: Transform::identity(),
            clip: Some(Clip {
                path: unit_path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)], true),
            }),
            children: vec![Node::Shape(Box::new(shape))],
        };
        scene.nodes.push(Node::Group(group));
        let svg = render_svg(&scene, &SvgOptions { scale: 200.0 }).unwrap();
        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg "));
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.contains("viewBox=\"0 0 800.000000 600.000000\""));
        assert!(svg.contains("<defs>"));
        parse_xml(&svg);
    }

    // --- caps and segment mapping (P3-17, coverage finding major 3) ---

    #[test]
    fn the_document_assembly_enforces_the_output_cap() {
        // The header and footer count against the cap too: a writer
        // whose pushes fit can still overflow when `finish` assembles
        // the document.
        let mut writer = Writer::new(96.0, 100);
        let attrs = vec![("d".to_string(), "M 0.000000 0.000000".to_string())];
        writer
            .tag(false, 0, "path", &attrs, true)
            .expect("the body fits the cap");
        assert_eq!(writer.finish(10.0, 10.0), Err(Error::TooLarge));
    }

    #[test]
    fn an_absurd_document_hits_the_output_cap() {
        // One path whose coordinates print as 300-digit numbers: no
        // output cap can hold it. The cap is 64 MiB; each quadratic
        // prints about 1.2 KiB.
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        for _ in 0..60_000 {
            path.push(Segment::QuadTo {
                ctrl: Point::new(1.0e300, 1.0e300),
                to: Point::new(1.0e300, 1.0e300),
            });
        }
        let mut scene = Scene::new(1.0, 1.0);
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path,
            fill: Some(solid(0, 0, 0)),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        })));
        assert_eq!(render_svg(&scene, &SvgOptions::new()), Err(Error::TooLarge));
    }

    #[test]
    fn line_end_path_maps_quad_and_cubic_segments() {
        let path = unit_path(&[(0.0, 0.0), (1.0, 0.0)], false);
        let mut shape = Path::new();
        shape.push(Segment::MoveTo(Point::new(0.0, 0.5)));
        shape.push(Segment::QuadTo {
            ctrl: Point::new(0.5, 0.0),
            to: Point::new(1.0, 0.5),
        });
        shape.push(Segment::CubicTo {
            ctrl1: Point::new(1.2, 0.5),
            ctrl2: Point::new(1.4, 0.5),
            to: Point::new(1.6, 0.5),
        });
        shape.push(Segment::Close);
        let end = LineEnd {
            shape,
            width: 0.2,
            length: 0.4,
            filled: true,
            color: None,
        };
        let drawn = line_end_path(&path, &end, false).expect("maps the shape");
        assert!(
            drawn
                .segments()
                .iter()
                .any(|segment| matches!(segment, Segment::QuadTo { .. })),
            "{:?}",
            drawn.segments()
        );
        assert!(
            drawn
                .segments()
                .iter()
                .any(|segment| matches!(segment, Segment::CubicTo { .. })),
            "{:?}",
            drawn.segments()
        );
        assert!(matches!(drawn.segments().last(), Some(Segment::Close)));
    }

    #[test]
    fn endpoint_reads_the_first_segment_direction() {
        // A quad first segment points the start at its control point.
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::QuadTo {
            ctrl: Point::new(1.0, 0.0),
            to: Point::new(2.0, 2.0),
        });
        assert_eq!(
            endpoint(&path, true),
            Some((Point::new(0.0, 0.0), Point::new(1.0, 0.0)))
        );

        // A cubic first segment the same, at its first control point.
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::CubicTo {
            ctrl1: Point::new(0.0, 1.0),
            ctrl2: Point::new(2.0, 1.0),
            to: Point::new(2.0, 0.0),
        });
        assert_eq!(
            endpoint(&path, true),
            Some((Point::new(0.0, 0.0), Point::new(0.0, 1.0)))
        );

        // A second subpath start has no single start vertex.
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::MoveTo(Point::new(2.0, 2.0)));
        assert_eq!(endpoint(&path, true), None);
    }

    // --- attribute fallbacks (P3-17 minor 3 and minor 5) --------------

    #[test]
    fn shadow_and_line_end_strokes_write_their_attributes() {
        let mut stroke = plain_stroke(solid(0, 0, 0));
        // A translucent stroke paint writes `stroke-opacity` on the
        // shape, on the shadow's stroke and on an outlined line end.
        stroke.paint = Paint::Solid(color(0, 0, 0, 128));
        stroke.end = Some(LineEnd {
            shape: unit_path(&[(0.0, 0.5), (1.0, 0.0), (1.0, 1.0)], true),
            width: 0.2,
            length: 0.4,
            filled: false,
            color: None,
        });
        let mut scene = Scene::new(2.0, 2.0);
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0)], false),
            fill: None,
            stroke: Some(stroke),
            shadow: Some(Shadow {
                color: color(0, 0, 0, 64),
                offset: Point::new(0.02, -0.02),
                blur: 0.0,
            }),
            clip: None,
            text: None,
        })));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("stroke-opacity=\"0.501961\""));
        parse_xml(&svg);
    }

    #[test]
    fn a_stroked_pattern_tile_writes_its_stroke_attributes() {
        // A non-finite tile stroke width falls back to 0 and a
        // translucent foreground writes a stroke opacity on the tile.
        let mut scene = Scene::new(1.0, 1.0);
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::Pattern(PatternPaint {
                foreground: color(0, 0, 0, 128),
                background: Rgba::opaque(255, 255, 255),
                tile: unit_path(&[(0.0, 0.0), (0.05, 0.05), (0.1, 0.0)], false),
                tile_size: larrygram_geom::Size::new(0.1, 0.1),
                filled: false,
                tile_stroke_width: f64::NAN,
            })),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        })));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("stroke-width=\"0.000000\""));
        assert!(svg.contains("stroke-opacity=\"0.501961\""));
        parse_xml(&svg);
    }

    #[test]
    fn a_gradient_stop_with_a_non_finite_offset_writes_zero() {
        let mut scene = Scene::new(1.0, 1.0);
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: unit_path(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
            fill: Some(Paint::LinearGradient {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0),
                stops: vec![GradientStop {
                    offset: f64::NAN,
                    color: Rgba::opaque(255, 0, 0),
                }],
            }),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        })));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(svg.contains("offset=\"0.000000\""));
        parse_xml(&svg);
    }

    #[test]
    fn a_solid_paint_def_is_its_colour_value() {
        let mut writer = Writer::new(96.0, 1 << 20);
        assert_eq!(
            writer.paint_def(&Paint::Solid(Rgba::opaque(1, 2, 3))),
            Ok("#010203".to_owned())
        );
    }

    #[test]
    fn text_skips_glyphs_without_a_size() {
        let fonts = default_fonts();
        let Some(font) = fonts.fonts().first().cloned() else {
            return;
        };
        let mut scene = Scene::new(2.0, 2.0);
        let text = TextNode {
            fonts: vec![font],
            runs: Vec::new(),
            layout: Layout {
                lines: vec![Line {
                    glyphs: vec![GlyphPos {
                        glyph: GlyphId(36),
                        font: 0,
                        x: 0.25,
                        y: 0.5,
                        run: 0,
                        size: 0.0,
                    }],
                    baseline: 0.5,
                    width: 0.2,
                    paragraph: 0,
                }],
                width: 0.2,
                height: 0.2,
                overflow: false,
                rotation: Angle::from_degrees(0.0),
                warnings: Vec::new(),
            },
            color: Rgba::opaque(0, 0, 255),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(1.0, 2.0),
        };
        scene.nodes.push(Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path: Path::new(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(text),
        })));
        let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
        assert!(!svg.contains("matrix(16.000000"));
        parse_xml(&svg);
    }
}
