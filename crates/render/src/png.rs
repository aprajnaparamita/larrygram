//! The PNG backend: rasterize a scene with `tiny-skia` (ADR 0025).
//!
//! [`render_png`] turns a [`Scene`] into PNG bytes at [`PngOptions::scale`]
//! pixels per inch. The raster is `ceil(width * scale)` by
//! `ceil(height * scale)` pixels; the device transform maps page inches
//! (y up) to image pixels (y down). The background is [`Scene::background`]
//! or transparent.
//!
//! # How the scene is drawn
//!
//! Node transforms are composed down the tree and combined with the device
//! transform at draw time, so paints and stroke widths stay in local
//! inches. Fill and stroke paths draw as given (the builder resolves the
//! visual styles, ADR 0023); dashes, caps, joins and the miter limit map
//! straight onto `tiny-skia`. Line ends use the unit-box convention of
//! `render::styles`: `x = 0` is the tip at the path's vertex, `x = 1` is
//! the tail `length` inches along the line into the shape, and `y = 0.5`
//! is the line centre scaled by `width`. Patterns fill the tile box
//! (background, then the tile mark: filled when `PatternPaint::filled`,
//! stroked with `PatternPaint::tile_stroke_width` otherwise) and repeat
//! with the `tile_size` pitch. Shadows are an offset copy of the shape filled (and
//! stroked) in the shadow colour with its alpha; `Shadow::blur` is not
//! applied (`tiny-skia` has no blur). A [`Clip`] on a group clips its
//! children; a clip on a shape clips its fill, stroke, line ends and text,
//! but not its shadow.
//!
//! # Text
//!
//! Text draws as filled glyph outlines (`Font::outline`, em units). The
//! layout space is y down with the origin at the top-left of the block,
//! so a glyph maps to shape-local space as: scale the outline by
//! `GlyphPos::size / 72` inches per point (y flipped), rotate it by
//! `Layout::rotation` about the glyph origin (the positions already carry
//! the rotation, ADR 0024 amendment), translate it to the glyph position
//! and the block origin, and map the block frame with y up.
//!
//! # Caps (AGENTS.md section 6)
//!
//! Input-derived sizes are capped: either raster dimension above
//! [`crate::MAX_DEVICE_EXTENT`] pixels or a raster above 256 MiB returns
//! [`Error::TooLarge`], as does a degenerate request (a zero, negative or
//! non-finite pixel size). A `scale` that is not finite, not positive or
//! above [`crate::MAX_DEVICE_EXTENT`] pixels per inch returns
//! [`Error::InvalidOptions`] (the scale cap both backends share, ADR 0025
//! amendment). Scene nesting is followed to a depth of 128 (the builder's
//! cap); deeper nodes are not drawn. Non-finite coordinates in paths,
//! strokes, gradients, patterns, shadows, line ends and glyphs are
//! skipped. Nothing here panics or allocates without bound.
//!
//! # Determinism (ADR 0025)
//!
//! The same scene renders to the same bytes on one machine: every step is
//! fixed-order integer or `f32`/`f64` arithmetic and the PNG encoder is
//! deterministic.

use larrygram_geom::{Path, Point, Segment, Transform};

use tiny_skia::{
    FillRule, FilterQuality, GradientStop as SkGradientStop, LineCap as SkLineCap,
    LineJoin as SkLineJoin, Mask, Paint, Path as SkPath, PathBuilder, Pattern, Pixmap,
    Point as SkPoint, Shader, SpreadMode, Stroke as SkStroke, StrokeDash, Transform as SkTransform,
};

use crate::error::Error;
use crate::scene::{
    Clip, GradientStop, Group, LineCap, LineEnd, LineJoin, Node, Paint as ScenePaint, PatternPaint,
    Rgba, Scene, Shadow, Shape, Stroke, TextNode,
};

/// The largest raster size in bytes (256 MiB at four bytes per pixel).
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The scene nesting depth the backend follows (AGENTS.md section 6:
/// bound recursion; the builder caps at the same depth).
const MAX_DEPTH: u32 = 128;

/// Below this a direction or a dash rhythm counts as degenerate, in
/// inches.
const EPS: f64 = 1.0e-12;

/// Options for PNG output (ADR 0025).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PngOptions {
    /// Pixels per inch (default 96). Must be finite, positive and at
    /// most [`crate::MAX_DEVICE_EXTENT`], or the render fails with
    /// [`Error::InvalidOptions`].
    pub scale: f64,
    /// Whether to anti-alias (default true).
    pub antialias: bool,
}

impl PngOptions {
    /// The default options: 96 pixels per inch, anti-aliasing on.
    pub fn new() -> Self {
        Self {
            scale: 96.0,
            antialias: true,
        }
    }
}

impl Default for PngOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Rasterizes a scene to PNG bytes at `scale` pixels per inch (ADR 0025).
///
/// The raster is `ceil(width * scale)` by `ceil(height * scale)` pixels.
/// Returns [`Error::InvalidOptions`] for a scale that is not finite, not
/// positive or over the shared scale cap ([`crate::MAX_DEVICE_EXTENT`]
/// pixels per inch), [`Error::TooLarge`] when the raster exceeds the caps
/// of AGENTS.md section 6 (or cannot form a raster at all) and
/// [`Error::Encode`] when the PNG encoder fails.
pub fn render_png(scene: &Scene, options: &PngOptions) -> Result<Vec<u8>, Error> {
    rasterize(scene, options.scale, options.antialias)?
        .encode_png()
        .map_err(|_| Error::Encode)
}

/// The rasterizer both backends share (ADR 0033): `render_png` encodes
/// this pixmap and [`crate::render_raster`] returns the same pixels as
/// demultiplied RGBA bytes, so the PNG goldens cover the canvas.
pub(crate) fn rasterize(scene: &Scene, scale: f64, antialias: bool) -> Result<Pixmap, Error> {
    let scale = crate::checked_scale(scale)?;
    let (width, height) = raster_size(scene, scale)?;
    let mut pixmap = Pixmap::new(width, height).ok_or(Error::TooLarge)?;
    if let Some(background) = scene.background {
        pixmap.fill(color(background));
    }
    let ctx = Ctx {
        scale,
        antialias,
        width,
        height,
    };
    // Page inches (y up) to image pixels (y down).
    let device = Transform {
        m11: scale,
        m12: 0.0,
        m21: 0.0,
        m22: -scale,
        dx: 0.0,
        dy: scene.height * scale,
    };
    let mut clips = Vec::new();
    draw_nodes(&mut pixmap, &ctx, &scene.nodes, device, &mut clips, 0);
    Ok(pixmap)
}

/// What every draw call needs besides the pixmap.
struct Ctx {
    /// Pixels per inch.
    scale: f64,
    /// Whether to anti-alias.
    antialias: bool,
    /// Raster width in pixels.
    width: u32,
    /// Raster height in pixels.
    height: u32,
}

/// One clip in the active stack, as a device-space path.
type ClipFrame = (SkPath, FillRule);

/// The raster size in pixels, under the caps of AGENTS.md section 6.
fn raster_size(scene: &Scene, scale: f64) -> Result<(u32, u32), Error> {
    let width = pixels(scene.width, scale)?;
    let height = pixels(scene.height, scale)?;
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|px| px.checked_mul(4))
        .ok_or(Error::TooLarge)?;
    if bytes > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    Ok((width, height))
}

/// `ceil(inches * scale)` pixels, or [`Error::TooLarge`] when the size is
/// degenerate (zero, negative or non-finite) or over the device extent
/// cap ([`crate::MAX_DEVICE_EXTENT`]).
fn pixels(inches: f64, scale: f64) -> Result<u32, Error> {
    let px = inches * scale;
    if !px.is_finite() || px <= 0.0 || px > f64::from(crate::MAX_DEVICE_EXTENT) {
        return Err(Error::TooLarge);
    }
    Ok(px.ceil() as u32)
}

/// Draws the nodes at `total` (node space to device pixels).
fn draw_nodes(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    nodes: &[Node],
    total: Transform,
    clips: &mut Vec<ClipFrame>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for node in nodes {
        match node {
            Node::Group(group) => {
                let total = group.transform.then(total);
                draw_group(pixmap, ctx, group, total, clips, depth);
            }
            Node::Shape(shape) => {
                let total = shape.transform.then(total);
                draw_shape(pixmap, ctx, shape, total, clips);
            }
        }
    }
}

/// Draws a group's children under the group's clip.
fn draw_group(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    group: &Group,
    total: Transform,
    clips: &mut Vec<ClipFrame>,
    depth: u32,
) {
    let pushed = push_clip(clips, group.clip.as_ref(), total);
    draw_nodes(pixmap, ctx, &group.children, total, clips, depth + 1);
    clips.truncate(clips.len() - pushed);
}

/// Draws one shape: shadow first, then fill, stroke, line ends and text
/// under the shape's clip.
fn draw_shape(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    shape: &Shape,
    total: Transform,
    clips: &mut Vec<ClipFrame>,
) {
    let path = match sk_path(&shape.path) {
        Some(path) => path,
        None => {
            // Nothing to fill or stroke; the text still draws.
            let pushed = push_clip(clips, shape.clip.as_ref(), total);
            if let Some(text) = &shape.text {
                draw_text(pixmap, ctx, text, total, clip_mask(ctx, clips).as_ref());
            }
            clips.truncate(clips.len() - pushed);
            return;
        }
    };
    // The shadow is a copy of the whole shape silhouette and is not
    // clipped (a clip would cut the offset copy away).
    if let Some(shadow) = &shape.shadow {
        draw_shadow(pixmap, ctx, &path, shape.stroke.as_ref(), shadow, total);
    }
    let pushed = push_clip(clips, shape.clip.as_ref(), total);
    let mask = clip_mask(ctx, clips);
    let mask = mask.as_ref();
    if let Some(fill) = &shape.fill {
        draw_fill(pixmap, ctx, &path, fill, sk_transform(total), mask);
    }
    if let Some(stroke) = &shape.stroke {
        draw_stroke(pixmap, ctx, &path, stroke, sk_transform(total), mask);
        draw_line_ends(pixmap, ctx, &shape.path, stroke, sk_transform(total), mask);
    }
    if let Some(text) = &shape.text {
        draw_text(pixmap, ctx, text, total, mask);
    }
    clips.truncate(clips.len() - pushed);
}

/// Draws the offset shadow copy of a shape.
fn draw_shadow(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    path: &SkPath,
    stroke: Option<&Stroke>,
    shadow: &Shadow,
    total: Transform,
) {
    if shadow.color.a == 0 || !finite(shadow.offset) {
        return;
    }
    // `Shadow::blur` is not applied: tiny-skia has no blur filter.
    let total = sk_transform(Transform::translate(shadow.offset).then(total));
    let paint = ScenePaint::Solid(shadow.color);
    draw_fill(pixmap, ctx, path, &paint, total, None);
    if let Some(stroke) = stroke {
        let mut copy = stroke.clone();
        copy.paint = paint;
        copy.dash = Vec::new();
        draw_stroke(pixmap, ctx, path, &copy, total, None);
    }
}

/// Draws both line ends of a path.
fn draw_line_ends(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    path: &Path,
    stroke: &Stroke,
    total: SkTransform,
    mask: Option<&Mask>,
) {
    if stroke.start.is_none() && stroke.end.is_none() {
        return;
    }
    let (start, end) = path_ends(path);
    if let (Some(end), Some((tip, travel))) = (&stroke.end, end) {
        draw_line_end(pixmap, ctx, end, stroke, tip, travel, -1.0, total, mask);
    }
    if let (Some(start), Some((tip, travel))) = (&stroke.start, start) {
        draw_line_end(pixmap, ctx, start, stroke, tip, travel, 1.0, total, mask);
    }
}

/// Draws one line end (arrowhead) at a path vertex (unit-box convention of
/// `render::styles`: the tip is at the vertex, the tail `length` inches
/// along the line into the shape).
#[allow(clippy::too_many_arguments)]
fn draw_line_end(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    end: &LineEnd,
    stroke: &Stroke,
    tip: Point,
    travel: Point,
    into_sign: f64,
    total: SkTransform,
    mask: Option<&Mask>,
) {
    let (length, width) = (end.length, end.width);
    if !length.is_finite() || length <= 0.0 || !width.is_finite() || width <= 0.0 {
        return;
    }
    // Unit box to shape-local space: `x` runs `length` inches from the tip
    // into the line, `y` runs `width` inches across the line centre.
    let into = travel.scale(into_sign);
    let across = Point::new(-travel.y, travel.x);
    let local = Transform {
        m11: into.x * length,
        m12: across.x * width,
        m21: into.y * length,
        m22: across.y * width,
        dx: tip.x - across.x * width * 0.5,
        dy: tip.y - across.y * width * 0.5,
    };
    let path = match sk_path_with(&end.shape, local) {
        Some(path) => path,
        None => return,
    };
    let paint = match end.color {
        Some(color) => ScenePaint::Solid(color),
        None => stroke.paint.clone(),
    };
    if end.filled {
        draw_fill(pixmap, ctx, &path, &paint, total, mask);
    } else {
        let mut outline = stroke.clone();
        outline.paint = paint;
        outline.dash = Vec::new();
        draw_stroke(pixmap, ctx, &path, &outline, total, mask);
    }
}

/// Draws a filled path.
fn draw_fill(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    path: &SkPath,
    paint: &ScenePaint,
    total: SkTransform,
    mask: Option<&Mask>,
) {
    if !fits_device(path, total, 0.0) {
        return;
    }
    with_paint(paint, ctx, |paint| {
        pixmap.fill_path(path, paint, FillRule::Winding, total, mask);
    });
}

/// Draws a stroked path.
fn draw_stroke(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    path: &SkPath,
    stroke: &Stroke,
    total: SkTransform,
    mask: Option<&Mask>,
) {
    if !stroke.width.is_finite() || stroke.width < 0.0 || !fits_device(path, total, stroke.width) {
        return;
    }
    let sk_stroke = SkStroke {
        width: stroke.width as f32,
        miter_limit: miter_of(stroke.miter_limit),
        line_cap: cap_of(stroke.cap),
        line_join: join_of(stroke.join),
        dash: dash_of(&stroke.dash),
    };
    with_paint(&stroke.paint, ctx, |paint| {
        pixmap.stroke_path(path, paint, &sk_stroke, total, mask);
    });
}

/// Draws a text block as filled glyph outlines.
fn draw_text(
    pixmap: &mut Pixmap,
    ctx: &Ctx,
    text: &TextNode,
    total: Transform,
    mask: Option<&Mask>,
) {
    if !finite(text.origin) {
        return;
    }
    let rotation = text.layout.rotation;
    for line in &text.layout.lines {
        for glyph in &line.glyphs {
            let size_in = glyph.size / 72.0;
            if !size_in.is_finite() || size_in <= 0.0 || !finite(Point::new(glyph.x, glyph.y)) {
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
            // Em units (y up) to layout space (y down), the block
            // rotation about the glyph origin (the positions already
            // carry it), the glyph position, then the block frame with y
            // up at the text origin (ADR 0024 amendment).
            let glyph_total = Transform::scale(size_in, -size_in)
                .then(Transform::rotate(rotation))
                .then(Transform::translate(Point::new(glyph.x, glyph.y)))
                .then(Transform::scale(1.0, -1.0))
                .then(Transform::translate(text.origin))
                .then(total);
            if let Some(path) = sk_path(&outline) {
                // A glyph's run selects its colour; the block colour is
                // the fallback (P3-05 maps bullet runs onto `runs` too).
                let color = text.runs.get(glyph.run).map_or(text.color, |run| run.color);
                let paint = ScenePaint::Solid(color);
                draw_fill(pixmap, ctx, &path, &paint, sk_transform(glyph_total), mask);
            }
        }
    }
}

/// Runs `f` with the `tiny-skia` paint of a scene paint.
///
/// Patterns rasterize their tile first; a degenerate tile falls back to
/// the foreground colour. Returns without calling `f` only when the paint
/// has no drawable gradient.
fn with_paint(paint: &ScenePaint, ctx: &Ctx, f: impl FnOnce(&Paint<'_>)) {
    let mut sk_paint = Paint {
        anti_alias: ctx.antialias,
        ..Paint::default()
    };
    match paint {
        ScenePaint::Solid(c) => {
            sk_paint.set_color(color(*c));
            f(&sk_paint);
        }
        ScenePaint::LinearGradient { .. } | ScenePaint::RadialGradient { .. } => {
            if let Some(shader) = gradient_shader(paint) {
                sk_paint.shader = shader;
                f(&sk_paint);
            }
        }
        ScenePaint::Pattern(pattern) => {
            if let Some(tile) = tile_pixmap(pattern, ctx) {
                let shader = Pattern::new(
                    tile.as_ref(),
                    SpreadMode::Repeat,
                    FilterQuality::Nearest,
                    1.0,
                    pattern_transform(pattern, ctx),
                );
                sk_paint.shader = shader;
                f(&sk_paint);
            } else {
                sk_paint.set_color(color(pattern.foreground));
                f(&sk_paint);
            }
        }
    }
}

/// The shader of a gradient paint, in shape-local inches (the draw
/// transform maps it to the device).
fn gradient_shader(paint: &ScenePaint) -> Option<Shader<'static>> {
    match paint {
        ScenePaint::LinearGradient { start, end, stops } => {
            if !finite(*start) || !finite(*end) {
                return None;
            }
            tiny_skia::LinearGradient::new(
                SkPoint::from_xy(start.x as f32, start.y as f32),
                SkPoint::from_xy(end.x as f32, end.y as f32),
                sk_stops(stops),
                SpreadMode::Pad,
                SkTransform::identity(),
            )
        }
        ScenePaint::RadialGradient {
            center,
            radius,
            stops,
        } => {
            if !finite(*center) || !radius.is_finite() || *radius <= 0.0 {
                return None;
            }
            let center = SkPoint::from_xy(center.x as f32, center.y as f32);
            tiny_skia::RadialGradient::new(
                center,
                0.0,
                center,
                *radius as f32,
                sk_stops(stops),
                SpreadMode::Pad,
                SkTransform::identity(),
            )
        }
        ScenePaint::Solid(_) | ScenePaint::Pattern(_) => None,
    }
}

/// The gradient stops, clamped to `0..=1` and sorted by offset; stops
/// with non-finite offsets are dropped.
fn sk_stops(stops: &[GradientStop]) -> Vec<SkGradientStop> {
    let mut usable: Vec<GradientStop> = stops
        .iter()
        .copied()
        .filter(|stop| stop.offset.is_finite())
        .collect();
    usable.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    usable
        .iter()
        .map(|stop| SkGradientStop::new(stop.offset.clamp(0.0, 1.0) as f32, color(stop.color)))
        .collect()
}

/// The tile of a pattern paint at device resolution: the background box,
/// then the tile mark filled or stroked as [`PatternPaint::filled`]
/// decides.
fn tile_pixmap(pattern: &PatternPaint, ctx: &Ctx) -> Option<Pixmap> {
    let (tile_w, tile_h) = tile_pixels(pattern.tile_size, ctx)?;
    let mut pixmap = Pixmap::new(tile_w, tile_h)?;
    pixmap.fill(color(pattern.background));
    if let Some(path) = sk_path(&pattern.tile) {
        let mut paint = Paint {
            anti_alias: ctx.antialias,
            ..Paint::default()
        };
        paint.set_color(color(pattern.foreground));
        // The tile path is in inches inside the `(0, 0)` to `tile_size`
        // box; the pixmap is that box at device resolution, so the mark
        // fills the tile exactly as it does in `svg.rs`.
        let unit = SkTransform::from_scale(
            (f64::from(tile_w) / pattern.tile_size.width) as f32,
            (f64::from(tile_h) / pattern.tile_size.height) as f32,
        );
        if pattern.filled {
            pixmap.fill_path(&path, &paint, FillRule::Winding, unit, None);
        } else if pattern.tile_stroke_width.is_finite() && pattern.tile_stroke_width > 0.0 {
            let stroke = SkStroke {
                width: pattern.tile_stroke_width as f32,
                miter_limit: 4.0,
                line_cap: SkLineCap::Round,
                line_join: SkLineJoin::Round,
                dash: None,
            };
            pixmap.stroke_path(&path, &paint, &stroke, unit, None);
        }
    }
    Some(pixmap)
}

/// The tile pixmap size in pixels: one tile at device resolution, under
/// the raster caps.
fn tile_pixels(size: larrygram_geom::Size, ctx: &Ctx) -> Option<(u32, u32)> {
    let (w_in, h_in) = (size.width, size.height);
    if !w_in.is_finite() || !h_in.is_finite() || w_in <= 0.0 || h_in <= 0.0 {
        return None;
    }
    let w_px = (w_in * ctx.scale).round();
    let h_px = (h_in * ctx.scale).round();
    if !w_px.is_finite() || !h_px.is_finite() || w_px < 1.0 || h_px < 1.0 {
        return None;
    }
    let w_px = w_px.min(f64::from(crate::MAX_DEVICE_EXTENT)) as u32;
    let h_px = h_px.min(f64::from(crate::MAX_DEVICE_EXTENT)) as u32;
    let bytes = u64::from(w_px) * u64::from(h_px) * 4;
    if bytes > MAX_BYTES {
        return None;
    }
    Some((w_px, h_px))
}

/// The pattern transform: tile pixels to shape-local inches.
fn pattern_transform(pattern: &PatternPaint, ctx: &Ctx) -> SkTransform {
    match tile_pixels(pattern.tile_size, ctx) {
        Some((w_px, h_px)) => SkTransform::from_scale(
            (pattern.tile_size.width / f64::from(w_px)) as f32,
            (pattern.tile_size.height / f64::from(h_px)) as f32,
        ),
        None => SkTransform::identity(),
    }
}

/// Builds the clip mask of the active stack, or `None` with no clips.
fn clip_mask(ctx: &Ctx, clips: &[ClipFrame]) -> Option<Mask> {
    let (first, rest) = clips.split_first()?;
    let mut mask = Mask::new(ctx.width, ctx.height)?;
    mask.fill_path(&first.0, first.1, ctx.antialias, SkTransform::identity());
    for (path, rule) in rest {
        mask.intersect_path(path, *rule, ctx.antialias, SkTransform::identity());
    }
    Some(mask)
}

/// Pushes a clip onto the stack in device space; returns 1 when pushed.
fn push_clip(clips: &mut Vec<ClipFrame>, clip: Option<&Clip>, total: Transform) -> usize {
    let Some(clip) = clip else {
        return 0;
    };
    let Some(path) = sk_path_with(&clip.path, total) else {
        return 0;
    };
    if !fits_device(&path, SkTransform::identity(), 0.0) {
        // A clip too big to rasterize clips nothing on the page.
        return 0;
    }
    clips.push((path, FillRule::Winding));
    1
}

/// The largest device-space coordinate, in pixels, a path may reach.
///
/// The raster is at most [`MAX_DIMENSION`] pixels per side, so a path
/// beyond this bound is absurd input. `tiny-skia` computes in `f32` and
/// fixed point and asserts finite points (P3-14 F4), so such a path is
/// skipped rather than handed to it.
const MAX_DEVICE_COORD: f64 = 4_194_304.0;

/// Whether a path under `total`, widened by a stroke of `width` local
/// units, stays inside [`MAX_DEVICE_COORD`].
///
/// The affine image of the path lies inside the image of its bounding
/// box, so the four corners bound it. The math is in `f64`.
fn fits_device(path: &SkPath, total: SkTransform, width: f64) -> bool {
    let b = path.bounds();
    let (sx, ky, kx, sy, tx, ty) = (
        f64::from(total.sx),
        f64::from(total.ky),
        f64::from(total.kx),
        f64::from(total.sy),
        f64::from(total.tx),
        f64::from(total.ty),
    );
    // The stroke reaches at most `width` times the largest axis scale
    // beyond the path.
    let reach = width * (sx.abs() + kx.abs()).max(ky.abs() + sy.abs());
    if !reach.is_finite() || reach > MAX_DEVICE_COORD {
        return false;
    }
    let corners = [
        (b.left(), b.top()),
        (b.right(), b.top()),
        (b.left(), b.bottom()),
        (b.right(), b.bottom()),
    ];
    corners.iter().all(|&(x, y)| {
        let (x, y) = (f64::from(x), f64::from(y));
        let dx = sx * x + kx * y + tx;
        let dy = ky * x + sy * y + ty;
        dx.is_finite()
            && dy.is_finite()
            && dx.abs() + reach <= MAX_DEVICE_COORD
            && dy.abs() + reach <= MAX_DEVICE_COORD
    })
}

/// The dash of a stroke, or `None` for a solid line.
///
/// Non-finite and negative lengths are dropped; an odd rhythm is repeated
/// to the even length `tiny-skia` needs (SVG semantics); a rhythm with no
/// positive length is solid.
fn dash_of(dash: &[f64]) -> Option<StrokeDash> {
    let mut lengths: Vec<f32> = dash
        .iter()
        .filter(|d| d.is_finite() && **d >= 0.0 && **d < 1.0e6)
        .map(|d| *d as f32)
        .collect();
    if lengths.iter().sum::<f32>() <= 0.0 {
        return None;
    }
    if lengths.len() % 2 == 1 {
        let odd = lengths.clone();
        lengths.extend(odd);
    }
    StrokeDash::new(lengths, 0.0)
}

/// The miter limit, falling back to `tiny-skia`'s default of 4.
fn miter_of(miter_limit: f64) -> f32 {
    if miter_limit.is_finite() && miter_limit > 0.0 {
        miter_limit.min(1.0e6) as f32
    } else {
        4.0
    }
}

/// The `tiny-skia` cap of a scene cap.
fn cap_of(cap: LineCap) -> SkLineCap {
    match cap {
        LineCap::Butt => SkLineCap::Butt,
        LineCap::Round => SkLineCap::Round,
        LineCap::Square => SkLineCap::Square,
    }
}

/// The `tiny-skia` join of a scene join.
fn join_of(join: LineJoin) -> SkLineJoin {
    match join {
        LineJoin::Miter => SkLineJoin::Miter,
        LineJoin::Round => SkLineJoin::Round,
        LineJoin::Bevel => SkLineJoin::Bevel,
    }
}

/// The `tiny-skia` colour of a scene colour.
fn color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// The `tiny-skia` transform of a geometry transform.
fn sk_transform(t: Transform) -> SkTransform {
    // Geometry maps `(x, y)` to `(m11 x + m12 y + dx, m21 x + m22 y +
    // dy)`; `from_row` takes `(sx, ky, kx, sy, tx, ty)`.
    SkTransform::from_row(
        t.m11 as f32,
        t.m21 as f32,
        t.m12 as f32,
        t.m22 as f32,
        t.dx as f32,
        t.dy as f32,
    )
}

/// Converts a geometry path to a `tiny-skia` path.
fn sk_path(path: &Path) -> Option<SkPath> {
    sk_path_with(path, Transform::identity())
}

/// Converts a geometry path to a `tiny-skia` path with `t` applied.
///
/// Segments with non-finite coordinates are dropped (AGENTS.md section 6:
/// malformed input degrades, never panics).
fn sk_path_with(path: &Path, t: Transform) -> Option<SkPath> {
    let mut builder = PathBuilder::new();
    let mut started = false;
    for segment in path.segments() {
        match segment {
            Segment::MoveTo(p) => {
                let p = t.apply(*p);
                if finite(p) {
                    builder.move_to(p.x as f32, p.y as f32);
                    started = true;
                }
            }
            Segment::LineTo(p) if started => {
                let p = t.apply(*p);
                if finite(p) {
                    builder.line_to(p.x as f32, p.y as f32);
                }
            }
            Segment::QuadTo { ctrl, to } if started => {
                let ctrl = t.apply(*ctrl);
                let to = t.apply(*to);
                if finite(ctrl) && finite(to) {
                    builder.quad_to(ctrl.x as f32, ctrl.y as f32, to.x as f32, to.y as f32);
                }
            }
            Segment::CubicTo { ctrl1, ctrl2, to } if started => {
                let ctrl1 = t.apply(*ctrl1);
                let ctrl2 = t.apply(*ctrl2);
                let to = t.apply(*to);
                if finite(ctrl1) && finite(ctrl2) && finite(to) {
                    builder.cubic_to(
                        ctrl1.x as f32,
                        ctrl1.y as f32,
                        ctrl2.x as f32,
                        ctrl2.y as f32,
                        to.x as f32,
                        to.y as f32,
                    );
                }
            }
            Segment::Close if started => builder.close(),
            _ => {}
        }
    }
    if !started {
        return None;
    }
    builder.finish()
}

/// One directed piece of a path for the line ends: start, end and the
/// unnormalized directions at both ends.
type Edge = (Point, Point, Point, Point);

/// A path vertex with its unit tangent.
type VertexDir = (Point, Point);

/// The start and end vertices of a path with their unit tangents, for the
/// line ends. The tip is the subpath's first (last) point; the tangent is
/// the direction of the first (last) non-degenerate segment there.
fn path_ends(path: &Path) -> (Option<VertexDir>, Option<VertexDir>) {
    let mut segments: Vec<Edge> = Vec::new();
    let mut current: Option<Point> = None;
    let mut subpath: Option<Point> = None;
    for segment in path.segments() {
        match segment {
            Segment::MoveTo(p) => {
                if finite(*p) {
                    current = Some(*p);
                    subpath = Some(*p);
                } else {
                    current = None;
                    subpath = None;
                }
            }
            Segment::LineTo(p) => {
                if let Some(from) = current
                    && finite(*p)
                {
                    segments.push((from, *p, p.sub(from), p.sub(from)));
                    current = Some(*p);
                }
            }
            Segment::QuadTo { ctrl, to } => {
                if let Some(from) = current
                    && finite(*ctrl)
                    && finite(*to)
                {
                    segments.push((from, *to, ctrl.sub(from), to.sub(*ctrl)));
                    current = Some(*to);
                }
            }
            Segment::CubicTo { ctrl1, ctrl2, to } => {
                if let Some(from) = current
                    && finite(*ctrl1)
                    && finite(*ctrl2)
                    && finite(*to)
                {
                    segments.push((from, *to, ctrl1.sub(from), to.sub(*ctrl2)));
                    current = Some(*to);
                }
            }
            Segment::Close => {
                if let (Some(from), Some(start)) = (current, subpath) {
                    segments.push((from, start, start.sub(from), start.sub(from)));
                    current = Some(start);
                }
            }
            // `Segment` is non-exhaustive outside `larrygram-geom`.
            _ => {}
        }
    }
    let start = segments.first().and_then(|(from, _, dir, _)| {
        let dir = segments
            .iter()
            .find_map(|(_, _, d, _)| unit_dir(*d))
            .or_else(|| unit_dir(*dir))?;
        Some((*from, dir))
    });
    let end = segments.last().and_then(|(_, to, _, dir)| {
        let dir = segments
            .iter()
            .rev()
            .find_map(|(_, _, _, d)| unit_dir(*d))
            .or_else(|| unit_dir(*dir))?;
        Some((*to, dir))
    });
    (start, end)
}

/// The unit vector of a direction, or `None` when it is degenerate.
fn unit_dir(v: Point) -> Option<Point> {
    let length = (v.x * v.x + v.y * v.y).sqrt();
    if !length.is_finite() || length < EPS {
        return None;
    }
    Some(Point::new(v.x / length, v.y / length))
}

/// Whether a point has finite coordinates.
fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    use larrygram_geom::{Angle, Size};
    use larrygram_text::{Block, Format, Insets, Paragraph, Run, VAlign, default_fonts, layout};

    fn path_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(x0, y0)));
        path.push(Segment::LineTo(Point::new(x1, y0)));
        path.push(Segment::LineTo(Point::new(x1, y1)));
        path.push(Segment::LineTo(Point::new(x0, y1)));
        path.push(Segment::Close);
        path
    }

    fn shape(path: Path, fill: Option<ScenePaint>) -> Node {
        Node::Shape(Box::new(Shape {
            shape_id: None,
            transform: Transform::identity(),
            path,
            fill,
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        }))
    }

    fn scene_of(width: f64, height: f64, nodes: Vec<Node>) -> Scene {
        Scene {
            width,
            height,
            background: None,
            nodes,
            warnings: Vec::new(),
        }
    }

    fn stroke(paint: ScenePaint, width: f64) -> Stroke {
        Stroke {
            paint,
            width,
            dash: Vec::new(),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            start: None,
            end: None,
        }
    }

    /// Renders and decodes a scene at 96 pixels per inch.
    fn raster(scene: &Scene) -> Pixmap {
        let bytes = render_png(scene, &PngOptions::new()).expect("renders");
        Pixmap::decode_png(&bytes).expect("a PNG")
    }

    /// The pixel at `(x, y)` as straight `(r, g, b, a)`.
    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let p = pixmap.pixel(x, y).expect("in bounds");
        (p.red(), p.green(), p.blue(), p.alpha())
    }

    fn solid_red() -> ScenePaint {
        ScenePaint::Solid(Rgba::opaque(255, 0, 0))
    }

    fn solid_blue() -> ScenePaint {
        ScenePaint::Solid(Rgba::opaque(0, 0, 255))
    }

    #[test]
    fn options_default_to_96_dpi_with_antialiasing() {
        let o = PngOptions::new();
        assert_eq!(o.scale, 96.0);
        assert!(o.antialias);
        assert_eq!(PngOptions::default(), o);
    }

    #[test]
    fn output_has_the_png_signature_and_ceil_dimensions() {
        // 1.01 in at 96 ppi is 96.96 px: the raster is 97 pixels wide.
        let scene = scene_of(1.01, 0.5, Vec::new());
        let bytes = render_png(&scene, &PngOptions::new()).expect("renders");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let dims = |b: &[u8]| {
            let be = |r: std::ops::Range<usize>| {
                b.get(r)
                    .and_then(|s| s.try_into().ok())
                    .map(u32::from_be_bytes)
            };
            (be(16..20), be(20..24))
        };
        assert_eq!(dims(&bytes), (Some(97), Some(48)));
    }

    #[test]
    fn background_draws_under_the_scene_and_none_is_transparent() {
        let mut scene = scene_of(1.0, 1.0, Vec::new());
        scene.background = Some(Rgba::opaque(255, 255, 255));
        assert_eq!(pixel(&raster(&scene), 10, 10), (255, 255, 255, 255));

        let scene = scene_of(1.0, 1.0, Vec::new());
        assert_eq!(pixel(&raster(&scene), 10, 10), (0, 0, 0, 0));
    }

    #[test]
    fn y_is_flipped_page_up_image_down() {
        // The red half is the top of the page (y from 1 to 2 inches).
        let node = shape(path_rect(0.0, 1.0, 2.0, 2.0), Some(solid_red()));
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 96, 5).0, 255);
        assert_eq!(pixel(&pixmap, 96, 5).3, 255);
        assert_eq!(pixel(&pixmap, 96, 185).3, 0);
    }

    #[test]
    fn fills_and_strokes_rasterize() {
        let mut node = shape(path_rect(0.25, 0.25, 1.75, 1.75), Some(solid_red()));
        if let Node::Shape(shape) = &mut node {
            shape.stroke = Some(stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.1));
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 96, 96).0, 255); // red interior
        let (r, g, b, a) = pixel(&pixmap, 24, 96); // the left stroke edge
        assert!(
            a == 255 && r < 40 && g < 40 && b < 40,
            "edge is {r} {g} {b}"
        );
        assert_eq!(pixel(&pixmap, 5, 96).3, 0); // outside
    }

    #[test]
    fn dashes_leave_gaps() {
        // A dashed horizontal line: 0.2 in on, 0.2 in off, starting at x 0.2.
        let mut node = shape(
            {
                let mut path = Path::new();
                path.push(Segment::MoveTo(Point::new(0.2, 1.0)));
                path.push(Segment::LineTo(Point::new(1.8, 1.0)));
                path
            },
            None,
        );
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.05);
            stroke.dash = vec![0.2, 0.2];
            shape.stroke = Some(stroke);
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 25, 96).3, 255); // inside the first dash
        assert_eq!(pixel(&pixmap, 45, 96).3, 0); // inside the first gap
        assert_eq!(pixel(&pixmap, 65, 96).3, 255); // inside the second dash
    }

    #[test]
    fn caps_change_the_line_ends() {
        let line = || {
            let mut path = Path::new();
            path.push(Segment::MoveTo(Point::new(0.5, 1.0)));
            path.push(Segment::LineTo(Point::new(1.5, 1.0)));
            path
        };
        let scene_with = |cap: LineCap| {
            let mut node = shape(line(), None);
            if let Node::Shape(shape) = &mut node {
                let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.1);
                stroke.cap = cap;
                shape.stroke = Some(stroke);
            }
            scene_of(2.0, 2.0, vec![node])
        };
        let round = raster(&scene_with(LineCap::Round));
        let butt = raster(&scene_with(LineCap::Butt));
        // Just past the end point, inside the round cap only.
        assert_eq!(pixel(&round, 147, 96).3, 255);
        assert_eq!(pixel(&butt, 147, 96).3, 0);
    }

    #[test]
    fn line_ends_draw_at_the_vertices() {
        let mut node = shape(
            {
                let mut path = Path::new();
                path.push(Segment::MoveTo(Point::new(0.5, 1.0)));
                path.push(Segment::LineTo(Point::new(1.5, 1.0)));
                path
            },
            None,
        );
        let mut triangle = Path::new();
        triangle.push(Segment::MoveTo(Point::new(0.0, 0.5)));
        triangle.push(Segment::LineTo(Point::new(1.0, 0.0)));
        triangle.push(Segment::LineTo(Point::new(1.0, 1.0)));
        triangle.push(Segment::Close);
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.05);
            stroke.end = Some(LineEnd {
                shape: triangle,
                width: 0.2,
                length: 0.3,
                filled: true,
                color: Some(Rgba::opaque(255, 0, 0)),
            });
            shape.stroke = Some(stroke);
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 135, 96).0, 255); // inside the arrowhead
        assert_eq!(pixel(&pixmap, 147, 96).3, 0); // past the tip
    }

    #[test]
    fn gradients_rasterize() {
        let paint = ScenePaint::LinearGradient {
            start: Point::new(0.0, 1.0),
            end: Point::new(2.0, 1.0),
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgba::opaque(0, 0, 0),
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgba::opaque(255, 255, 255),
                },
            ],
        };
        let node = shape(path_rect(0.0, 0.0, 2.0, 2.0), Some(paint));
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        let (left, _, _, _) = pixel(&pixmap, 20, 96);
        let (right, _, _, _) = pixel(&pixmap, 170, 96);
        assert!(left < 40, "left is {left}");
        assert!(right > 210, "right is {right}");
    }

    #[test]
    fn radial_gradients_rasterize() {
        let paint = ScenePaint::RadialGradient {
            center: Point::new(1.0, 1.0),
            radius: 1.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgba::opaque(255, 255, 255),
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgba::opaque(0, 0, 0),
                },
            ],
        };
        let node = shape(path_rect(0.0, 0.0, 2.0, 2.0), Some(paint));
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        let (center, _, _, _) = pixel(&pixmap, 96, 96);
        let (edge, _, _, _) = pixel(&pixmap, 10, 10);
        assert!(center > 210, "centre is {center}");
        assert!(edge < 40, "edge is {edge}");
    }

    #[test]
    fn patterns_tile_the_unit_box() {
        // A red square in the middle of a one-inch tile on a blue ground.
        let mut tile = Path::new();
        tile.push(Segment::MoveTo(Point::new(0.25, 0.25)));
        tile.push(Segment::LineTo(Point::new(0.75, 0.25)));
        tile.push(Segment::LineTo(Point::new(0.75, 0.75)));
        tile.push(Segment::LineTo(Point::new(0.25, 0.75)));
        tile.push(Segment::Close);
        let paint = ScenePaint::Pattern(PatternPaint {
            foreground: Rgba::opaque(255, 0, 0),
            background: Rgba::opaque(0, 0, 255),
            tile,
            tile_size: Size::new(1.0, 1.0),
            filled: true,
            tile_stroke_width: 0.05,
        });
        let node = shape(path_rect(0.0, 0.0, 2.0, 2.0), Some(paint));
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 48, 144).0, 255); // tile centre (0.5, 0.5)
        assert_eq!(pixel(&pixmap, 5, 144).2, 255); // between tiles
    }

    #[test]
    fn shadows_draw_an_offset_copy() {
        let mut node = shape(path_rect(0.5, 0.5, 1.5, 1.5), Some(solid_blue()));
        if let Node::Shape(shape) = &mut node {
            shape.shadow = Some(Shadow {
                color: Rgba::opaque(255, 0, 0),
                offset: Point::new(0.3, 0.0),
                blur: 0.0,
            });
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 158, 96).0, 255); // shadow only
        assert_eq!(pixel(&pixmap, 96, 96).2, 255); // the shape on top
        assert_eq!(pixel(&pixmap, 20, 96).3, 0); // outside both
    }

    #[test]
    fn group_transforms_and_clips_compose() {
        let child = shape(path_rect(0.0, 0.0, 2.0, 2.0), Some(solid_red()));
        let group = Node::Group(Group {
            transform: Transform::translate(Point::new(1.0, 0.0)),
            clip: Some(Clip {
                path: path_rect(0.0, 0.0, 0.5, 2.0),
            }),
            children: vec![child],
        });
        let scene = scene_of(3.0, 2.0, vec![group]);
        let pixmap = raster(&scene);
        // The clip keeps x 1.0..1.5 after the group translation.
        assert_eq!(pixel(&pixmap, 110, 96).3, 255);
        assert_eq!(pixel(&pixmap, 170, 96).3, 0);
        assert_eq!(pixel(&pixmap, 50, 96).3, 0);
    }

    #[test]
    fn shape_clips_its_own_content() {
        let mut node = shape(path_rect(0.0, 0.0, 2.0, 2.0), Some(solid_red()));
        if let Node::Shape(shape) = &mut node {
            shape.clip = Some(Clip {
                path: path_rect(0.0, 0.0, 1.0, 2.0),
            });
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 50, 96).3, 255);
        assert_eq!(pixel(&pixmap, 150, 96).3, 0);
    }

    /// A text block with one 72 pt "A" over the box origin.
    fn text_scene(rotation: Angle) -> Scene {
        let fonts = default_fonts();
        let block = Block {
            paragraphs: vec![Paragraph {
                runs: vec![Run {
                    text: "A",
                    format: Format {
                        family: "Calibri".to_owned(),
                        size: 72.0,
                        bold: false,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                    },
                }],
                align: larrygram_text::Align::Left,
                line_spacing: 1.0,
                space_before: 0.0,
                space_after: 0.0,
                indent_first: 0.0,
                indent_left: 0.0,
                bullet: None,
                tabs: Vec::new(),
            }],
            width: 1.0,
            height: 1.0,
            valign: VAlign::Top,
            margins: Insets::default(),
            rotation,
            wrap: true,
        };
        let layout = layout(&block, &fonts).expect("lays out");
        let text = TextNode {
            fonts: fonts.fonts().to_vec(),
            runs: Vec::new(),
            layout,
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(block.width, block.height),
            origin: Point::new(1.0, 1.5),
        };
        scene_of(
            2.0,
            2.0,
            vec![Node::Shape(Box::new(Shape {
                shape_id: None,
                transform: Transform::identity(),
                path: Path::new(),
                fill: None,
                stroke: None,
                shadow: None,
                clip: None,
                text: Some(text),
            }))],
        )
    }

    /// The pixels a scene paints, in image coordinates.
    fn ink(pixmap: &Pixmap) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                if pixel(pixmap, x, y).3 > 0 {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn glyph_outlines_rasterize() {
        let scene = text_scene(Angle::from_degrees(0.0));
        let pixmap = raster(&scene);
        let ink = ink(&pixmap);
        assert!(!ink.is_empty(), "the glyph drew nothing");
        // The glyph sits right of the block origin (1.0, 1.5) inches.
        assert!(
            ink.iter().all(|(x, _)| *x >= 90),
            "ink left of the text origin: {ink:?}"
        );
    }

    #[test]
    fn block_rotation_rotates_the_glyph_outlines() {
        let upright = ink(&raster(&text_scene(Angle::from_degrees(0.0))));
        let turned = ink(&raster(&text_scene(Angle::from_degrees(90.0))));
        assert_ne!(upright, turned, "the rotation changed nothing");
    }

    #[test]
    fn glyphs_take_their_run_colour() {
        let mut scene = text_scene(Angle::from_degrees(0.0));
        if let Node::Shape(shape) = &mut scene.nodes[0]
            && let Some(text) = &mut shape.text
        {
            text.runs.push(crate::scene::TextRun {
                underline: false,
                strikethrough: false,
                color: Rgba::opaque(255, 0, 0),
            });
        }
        let pixmap = raster(&scene);
        let ink = ink(&pixmap);
        assert!(!ink.is_empty(), "the glyph drew nothing");
        // Premultiplied: a red glyph has no green or blue anywhere.
        assert!(ink.iter().all(|&(x, y)| {
            let (_, g, b, _) = pixel(&pixmap, x, y);
            g == 0 && b == 0
        }));
        assert!(ink.iter().any(|&(x, y)| pixel(&pixmap, x, y).0 > 0));
    }

    #[test]
    fn missing_fonts_and_bad_glyph_sizes_draw_nothing() {
        let mut scene = text_scene(Angle::from_degrees(0.0));
        if let Node::Shape(shape) = &mut scene.nodes[0]
            && let Some(text) = &mut shape.text
        {
            text.fonts.clear();
            text.layout.lines[0].glyphs[0].size = f64::NAN;
        }
        let pixmap = raster(&scene);
        assert!(ink(&pixmap).is_empty());
    }

    #[test]
    fn antialiasing_can_be_turned_off() {
        // A diagonal edge: without anti-aliasing every pixel is exactly
        // the fill or exactly the background.
        let mut node = shape(
            {
                let mut path = Path::new();
                path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
                path.push(Segment::LineTo(Point::new(2.0, 0.0)));
                path.push(Segment::LineTo(Point::new(0.0, 2.0)));
                path.push(Segment::Close);
                path
            },
            Some(ScenePaint::Solid(Rgba::opaque(0, 0, 0))),
        );
        if let Node::Shape(shape) = &mut node {
            shape.transform = Transform::identity();
        }
        let scene = scene_of(2.0, 2.0, vec![node]);
        let mut options = PngOptions::new();
        options.antialias = false;
        let bytes = render_png(&scene, &options).expect("renders");
        let pixmap = Pixmap::decode_png(&bytes).expect("a PNG");
        let mut soft = 0;
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let (r, g, b, a) = pixel(&pixmap, x, y);
                if (r, g, b, a) != (0, 0, 0, 255) && (r, g, b, a) != (0, 0, 0, 0) {
                    soft += 1;
                }
            }
        }
        assert_eq!(soft, 0, "anti-aliasing was off but pixels blend");

        let bytes = render_png(&scene, &PngOptions::new()).expect("renders");
        let pixmap = Pixmap::decode_png(&bytes).expect("a PNG");
        let soft = (0..pixmap.height())
            .flat_map(|y| (0..pixmap.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let (r, g, b, a) = pixel(&pixmap, *x, *y);
                (r, g, b, a) != (0, 0, 0, 255) && (r, g, b, a) != (0, 0, 0, 0)
            })
            .count();
        assert!(soft > 0, "anti-aliasing was on but the edge is hard");
    }

    #[test]
    fn oversize_scenes_are_too_large() {
        // Over the dimension cap.
        let scene = scene_of(200.0, 1.0, Vec::new());
        assert_eq!(render_png(&scene, &PngOptions::new()), Err(Error::TooLarge));
        // Under the dimension cap, over the byte cap (12000 x 12000 px).
        let scene = scene_of(12_000.0, 12_000.0, Vec::new());
        let mut options = PngOptions::new();
        options.scale = 1.0;
        assert_eq!(render_png(&scene, &options), Err(Error::TooLarge));
    }

    #[test]
    fn degenerate_sizes_are_too_large() {
        let options = PngOptions::new();
        for (width, height) in [
            (0.0, 1.0),
            (-1.0, 1.0),
            (1.0, f64::NAN),
            (f64::INFINITY, 1.0),
        ] {
            let scene = scene_of(width, height, Vec::new());
            assert_eq!(render_png(&scene, &options), Err(Error::TooLarge));
        }
    }

    #[test]
    fn a_bad_scale_is_invalid_options() {
        // The scale cap both backends share (ADR 0025 amendment, P3-16):
        // a bad option is `InvalidOptions`, not `TooLarge`.
        let mut options = PngOptions::new();
        for scale in [0.0, -96.0, f64::INFINITY, f64::NAN, 1.0e300, 20_000.0] {
            options.scale = scale;
            let scene = scene_of(1.0, 1.0, Vec::new());
            assert_eq!(render_png(&scene, &options), Err(Error::InvalidOptions));
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let scene = text_scene(Angle::from_degrees(15.0));
        let first = render_png(&scene, &PngOptions::new()).expect("renders");
        let second = render_png(&scene, &PngOptions::new()).expect("renders");
        assert_eq!(first, second);
    }

    #[test]
    fn degenerate_scene_data_never_panics() {
        let nan = f64::NAN;
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(nan, 0.0)));
        path.push(Segment::LineTo(Point::new(1.0, 1.0)));
        path.push(Segment::QuadTo {
            ctrl: Point::new(nan, nan),
            to: Point::new(2.0, 2.0),
        });
        path.push(Segment::CubicTo {
            ctrl1: Point::new(1.0, nan),
            ctrl2: Point::new(1.0, 1.0),
            to: Point::new(nan, 2.0),
        });
        path.push(Segment::Close);
        let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), nan);
        stroke.width = nan;
        stroke.dash = vec![nan, -1.0, 0.0, 0.0];
        stroke.miter_limit = nan;
        stroke.start = Some(LineEnd {
            shape: Path::new(),
            width: nan,
            length: -1.0,
            filled: true,
            color: None,
        });
        let gradient = ScenePaint::RadialGradient {
            center: Point::new(nan, nan),
            radius: nan,
            stops: vec![GradientStop {
                offset: nan,
                color: Rgba::opaque(0, 0, 0),
            }],
        };
        let pattern = ScenePaint::Pattern(PatternPaint {
            foreground: Rgba::opaque(0, 0, 0),
            background: Rgba::opaque(255, 255, 255),
            tile: Path::new(),
            tile_size: Size::new(nan, 0.0),
            filled: false,
            tile_stroke_width: nan,
        });
        let mut node = shape(path, Some(gradient));
        if let Node::Shape(shape) = &mut node {
            shape.stroke = Some(stroke);
            shape.shadow = Some(Shadow {
                color: Rgba::new(0, 0, 0, 255),
                offset: Point::new(nan, nan),
                blur: nan,
            });
        }
        let deep = (0..300).fold(node, |child, _| {
            Node::Group(Group {
                transform: Transform::identity(),
                clip: Some(Clip {
                    path: path_rect(nan, 0.0, 1.0, 1.0),
                }),
                children: vec![child],
            })
        });
        let mut scene = scene_of(1.0, 1.0, vec![deep]);
        scene.nodes.push(shape(Path::new(), Some(pattern)));
        scene.width = nan;
        // The NaN size fails the caps; every other degenerate value is
        // skipped while drawing.
        assert_eq!(render_png(&scene, &PngOptions::new()), Err(Error::TooLarge));
        scene.width = 1.0;
        let bytes = render_png(&scene, &PngOptions::new());
        assert!(bytes.is_ok() || bytes == Err(Error::TooLarge));
    }

    #[test]
    fn huge_finite_coordinates_never_panic() {
        // P3-14 F4: a 1e30 in shape width under a rotation is finite in
        // f64 but overflows the f32 device space of tiny-skia, whose
        // hairline stroker asserts finite points in debug builds.
        let mut wide = Path::new();
        wide.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        wide.push(Segment::LineTo(Point::new(1.0e30, 0.0)));
        wide.push(Segment::CubicTo {
            ctrl1: Point::new(1.0e30, 1.0e30),
            ctrl2: Point::new(-1.0e30, 1.0e30),
            to: Point::new(0.0, 0.0),
        });
        let mut nodes = Vec::new();
        for (width, scale) in [(0.0, 1.0), (1.0, 1.0), (0.0, 1.0e10), (1.0e-30, 1.0e30)] {
            let mut node = shape(wide.clone(), Some(solid_red()));
            if let Node::Shape(shape) = &mut node {
                shape.stroke = Some(stroke(solid_blue(), width));
                shape.transform = Transform::scale(scale, scale)
                    .then(Transform::rotate(Angle::from_radians(-1.0)))
                    .then(Transform::translate(Point::new(5.0e29, -5.0e29)));
                shape.shadow = Some(Shadow {
                    color: Rgba::new(0, 0, 0, 255),
                    offset: Point::new(1.0e30, 0.0),
                    blur: 0.0,
                });
            }
            nodes.push(Node::Group(Group {
                transform: Transform::rotate(Angle::from_radians(-1.0)),
                clip: Some(Clip {
                    path: path_rect(-1.0e30, -1.0e30, 1.0e30, 1.0e30),
                }),
                children: vec![node],
            }));
        }
        let scene = scene_of(1.0, 1.0, nodes);
        assert!(render_png(&scene, &PngOptions::new()).is_ok());
    }

    #[test]
    fn dashes_are_sanitized() {
        // Odd rhythms repeat (SVG semantics); empty and zero rhythms are
        // solid; `tiny-skia` only takes even, non-negative arrays.
        assert!(dash_of(&[]).is_none());
        assert!(dash_of(&[0.0, 0.0]).is_none());
        assert!(dash_of(&[f64::NAN, -2.0]).is_none());
        assert!(dash_of(&[0.1]).is_some()); // repeats to [0.1, 0.1]
        assert!(dash_of(&[0.1, 0.2, 0.3]).is_some()); // repeats to 6
        assert!(dash_of(&[0.1, 0.2]).is_some());
    }

    #[test]
    fn path_ends_find_the_vertices_and_tangents() {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::LineTo(Point::new(3.0, 0.0)));
        let (start, end) = path_ends(&path);
        let (tip, dir) = start.expect("a start");
        assert_eq!(tip, Point::new(0.0, 0.0));
        assert_eq!(dir, Point::new(1.0, 0.0));
        let (tip, dir) = end.expect("an end");
        assert_eq!(tip, Point::new(3.0, 0.0));
        assert_eq!(dir, Point::new(1.0, 0.0));

        // An empty path and a degenerate one have no ends.
        assert_eq!(path_ends(&Path::new()), (None, None));
        let mut point = Path::new();
        point.push(Segment::MoveTo(Point::new(1.0, 1.0)));
        point.push(Segment::LineTo(Point::new(1.0, 1.0)));
        assert_eq!(path_ends(&point), (None, None));
    }

    #[test]
    fn raster_size_is_checked_with_integer_math() {
        let mut options = PngOptions::new();
        assert_eq!(
            raster_size(&scene_of(1.0, 1.0, Vec::new()), options.scale),
            Ok((96, 96))
        );
        // The largest raster the byte cap allows at 16384 pixels wide is
        // 4096 pixels tall.
        options.scale = 16_384.0;
        assert_eq!(
            raster_size(&scene_of(1.0, 0.25, Vec::new()), options.scale),
            Ok((16_384, 4096))
        );
        assert_eq!(
            raster_size(&scene_of(1.0, 0.26, Vec::new()), options.scale),
            Err(Error::TooLarge)
        );
    }

    // --- hostile and degraded paints (P3-17, coverage finding major 2)
    // -------------------------------------------------------------

    /// A gradient paint over the whole scene.
    fn gradient_shape(paint: ScenePaint) -> Scene {
        scene_of(
            1.0,
            1.0,
            vec![shape(path_rect(0.0, 0.0, 1.0, 1.0), Some(paint))],
        )
    }

    #[test]
    fn degenerate_gradients_draw_nothing() {
        let stop = GradientStop {
            offset: 0.0,
            color: Rgba::opaque(255, 0, 0),
        };
        // A linear gradient with non-finite endpoints.
        let scene = gradient_shape(ScenePaint::LinearGradient {
            start: Point::new(f64::NAN, 0.0),
            end: Point::new(1.0, 1.0),
            stops: vec![stop],
        });
        assert!(render_png(&scene, &PngOptions::new()).is_ok());
        assert!(ink(&raster(&scene)).is_empty());

        // A radial gradient with a zero radius and with a non-finite
        // centre.
        let scene = gradient_shape(ScenePaint::RadialGradient {
            center: Point::new(0.5, 0.5),
            radius: 0.0,
            stops: vec![stop],
        });
        assert!(ink(&raster(&scene)).is_empty());
        let scene = gradient_shape(ScenePaint::RadialGradient {
            center: Point::new(f64::INFINITY, 0.5),
            radius: 1.0,
            stops: vec![stop],
        });
        assert!(ink(&raster(&scene)).is_empty());
    }

    #[test]
    fn a_gradient_with_unusable_stops_still_rasterizes() {
        // Non-finite stop offsets are dropped; the clamp keeps the rest.
        let scene = gradient_shape(ScenePaint::LinearGradient {
            start: Point::new(0.0, 0.5),
            end: Point::new(1.0, 0.5),
            stops: vec![
                GradientStop {
                    offset: f64::NAN,
                    color: Rgba::opaque(255, 0, 0),
                },
                GradientStop {
                    offset: 0.0,
                    color: Rgba::opaque(255, 0, 0),
                },
                GradientStop {
                    offset: 2.0,
                    color: Rgba::opaque(0, 0, 255),
                },
            ],
        });
        assert!(!ink(&raster(&scene)).is_empty());
        assert!(gradient_shader(&solid_red()).is_none());
        assert!(
            gradient_shader(&ScenePaint::Pattern(pattern(Size::new(0.2, 0.2), true))).is_none()
        );
    }

    /// A pattern paint with the given tile box.
    fn pattern(tile_size: Size, filled: bool) -> PatternPaint {
        let mut tile = Path::new();
        tile.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        tile.push(Segment::LineTo(Point::new(
            tile_size.width,
            tile_size.height,
        )));
        PatternPaint {
            foreground: Rgba::opaque(255, 0, 0),
            background: Rgba::opaque(0, 0, 255),
            tile,
            tile_size,
            filled,
            tile_stroke_width: 0.02,
        }
    }

    #[test]
    fn degenerate_pattern_tiles_fall_back_to_the_foreground_colour() {
        let degenerate = [
            Size::new(f64::NAN, 0.2), // not finite
            Size::new(0.001, 0.001),  // sub-pixel at 96 ppi
            Size::new(100.0, 100.0),  // over the tile byte cap
        ];
        for tile_size in degenerate {
            let scene = gradient_shape(ScenePaint::Pattern(pattern(tile_size, true)));
            let pixmap = raster(&scene);
            let red = ink(&pixmap)
                .iter()
                .filter(|(x, y)| pixel(&pixmap, *x, *y).0 == 255)
                .count();
            assert!(red > 0, "no foreground fallback for {tile_size:?}");
        }
    }

    #[test]
    fn a_stroked_tile_draws_its_outline() {
        let scene = gradient_shape(ScenePaint::Pattern(pattern(Size::new(0.2, 0.2), false)));
        assert!(!ink(&raster(&scene)).is_empty());
    }

    // --- clips, dashes and shadows (P3-17, minor 5) --------------------

    #[test]
    fn a_second_clip_intersects_into_the_mask() {
        let mut node = shape(path_rect(0.0, 0.0, 1.0, 1.0), Some(solid_red()));
        if let Node::Shape(shape) = &mut node {
            shape.clip = Some(Clip {
                path: path_rect(0.0, 0.0, 0.75, 1.0),
            });
        }
        let group = Node::Group(Group {
            transform: Transform::identity(),
            clip: Some(Clip {
                path: path_rect(0.25, 0.0, 1.0, 1.0),
            }),
            children: vec![node],
        });
        let scene = scene_of(1.0, 1.0, vec![group]);
        let pixmap = raster(&scene);
        assert_eq!(pixel(&pixmap, 12, 48).3, 0); // left of the group clip
        assert!(pixel(&pixmap, 48, 48).3 > 0); // in both clips
        assert_eq!(pixel(&pixmap, 84, 48).3, 0); // right of the shape clip
    }

    #[test]
    fn quad_and_cubic_segments_stroke_and_dash() {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.1, 0.5)));
        path.push(Segment::QuadTo {
            ctrl: Point::new(0.5, 0.1),
            to: Point::new(0.9, 0.5),
        });
        path.push(Segment::CubicTo {
            ctrl1: Point::new(1.1, 0.9),
            ctrl2: Point::new(1.3, 0.9),
            to: Point::new(1.5, 0.5),
        });
        let mut node = shape(path, None);
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.05);
            stroke.dash = vec![0.1, 0.1];
            shape.stroke = Some(stroke);
        }
        let scene = scene_of(2.0, 1.0, vec![node]);
        assert!(!ink(&raster(&scene)).is_empty());
    }

    #[test]
    fn path_ends_follow_curve_and_close_segments() {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(f64::NAN, 0.0)));
        path.push(Segment::LineTo(Point::new(9.0, 9.0))); // no current point
        path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(Segment::QuadTo {
            ctrl: Point::new(1.0, 0.0),
            to: Point::new(2.0, 1.0),
        });
        path.push(Segment::CubicTo {
            ctrl1: Point::new(3.0, 1.0),
            ctrl2: Point::new(4.0, 1.0),
            to: Point::new(5.0, 0.0),
        });
        path.push(Segment::Close);
        let (start, end) = path_ends(&path);
        let (tip, travel) = start.expect("a start vertex");
        assert_eq!(tip, Point::new(0.0, 0.0));
        assert_eq!(travel, Point::new(1.0, 0.0)); // towards the quad control
        let (tip, travel) = end.expect("an end vertex");
        assert_eq!(tip, Point::new(0.0, 0.0)); // the close returns to the start
        // The tangent is the last non-degenerate outgoing direction: the
        // closing segment runs back to the start.
        assert_eq!(travel, Point::new(-1.0, 0.0));
    }

    // --- line ends (P3-17, coverage finding major 4) -------------------

    /// The unit-box arrowhead of the line-end tests.
    fn arrow() -> Path {
        let mut triangle = Path::new();
        triangle.push(Segment::MoveTo(Point::new(0.0, 0.5)));
        triangle.push(Segment::LineTo(Point::new(1.0, 0.0)));
        triangle.push(Segment::LineTo(Point::new(1.0, 1.0)));
        triangle.push(Segment::Close);
        triangle
    }

    /// A 3 x 2 inch scene of one horizontal line from (0.5, 1) to (2.5,
    /// 1) with the given line ends.
    fn line_ends_scene(start: Option<LineEnd>, end: Option<LineEnd>) -> Scene {
        let mut path = Path::new();
        path.push(Segment::MoveTo(Point::new(0.5, 1.0)));
        path.push(Segment::LineTo(Point::new(2.5, 1.0)));
        let mut node = shape(path, None);
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 255)), 0.03);
            stroke.start = start;
            stroke.end = end;
            shape.stroke = Some(stroke);
        }
        scene_of(3.0, 2.0, vec![node])
    }

    #[test]
    fn line_ends_draw_the_start_arrowhead_and_outline_the_end() {
        let scene = line_ends_scene(
            Some(LineEnd {
                shape: arrow(),
                width: 0.4,
                length: 0.6,
                filled: true,
                color: Some(Rgba::opaque(255, 0, 0)),
            }),
            Some(LineEnd {
                shape: arrow(),
                width: 0.4,
                length: 0.6,
                filled: false,
                color: None,
            }),
        );
        let pixmap = raster(&scene);
        // The start arrowhead is filled with its colour override.
        let (r, _, _, a) = pixel(&pixmap, 60, 96);
        assert_eq!((r, a), (255, 255), "the start end did not fill");
        // The end arrowhead is an outline in the stroke's own colour: its
        // body off the line is not filled ...
        assert_eq!(pixel(&pixmap, 210, 89).3, 0, "the end arrowhead filled");
        // ... but its edge is drawn.
        let blue = ink(&pixmap)
            .iter()
            .filter(|(x, y)| (180..=241).contains(x) && (70..=122).contains(y))
            .filter(|(x, y)| {
                let (_, g, b, a) = pixel(&pixmap, *x, *y);
                a > 0 && b > 200 && g < 60
            })
            .count();
        assert!(blue > 5, "the end arrowhead outline drew {blue} pixels");
    }

    #[test]
    fn degenerate_line_ends_are_skipped() {
        // A non-positive length and width draw nothing.
        let scene = line_ends_scene(
            None,
            Some(LineEnd {
                shape: arrow(),
                width: 0.0,
                length: 0.6,
                filled: true,
                color: Some(Rgba::opaque(255, 0, 0)),
            }),
        );
        let pixmap = raster(&scene);
        let red = ink(&pixmap)
            .iter()
            .filter(|(x, y)| pixel(&pixmap, *x, *y).0 == 255)
            .count();
        assert_eq!(red, 0, "a zero-width line end drew {red} pixels");

        // An empty line-end shape draws nothing.
        let scene = line_ends_scene(
            None,
            Some(LineEnd {
                shape: Path::new(),
                width: 0.4,
                length: 0.6,
                filled: true,
                color: Some(Rgba::opaque(255, 0, 0)),
            }),
        );
        let pixmap = raster(&scene);
        let red = ink(&pixmap)
            .iter()
            .filter(|(x, y)| pixel(&pixmap, *x, *y).0 == 255)
            .count();
        assert_eq!(red, 0, "an empty line-end shape drew {red} pixels");
    }

    #[test]
    fn transparent_and_non_finite_shadows_are_skipped() {
        let scene_with = |shadow: Shadow| {
            let mut node = shape(path_rect(0.25, 0.25, 0.75, 0.75), None);
            if let Node::Shape(shape) = &mut node {
                shape.shadow = Some(shadow);
            }
            scene_of(1.0, 1.0, vec![node])
        };
        let base = Shadow {
            color: Rgba::new(0, 0, 0, 255),
            offset: Point::new(0.1, 0.1),
            blur: 0.0,
        };
        // A transparent shadow draws nothing.
        let mut transparent = base;
        transparent.color.a = 0;
        assert!(ink(&raster(&scene_with(transparent))).is_empty());
        // Neither does one with a non-finite offset.
        let mut flying = base;
        flying.offset = Point::new(f64::NAN, 0.1);
        assert!(ink(&raster(&scene_with(flying))).is_empty());
    }

    // --- text guards (P3-17) -------------------------------------------

    #[test]
    fn bad_text_draws_nothing_but_never_panics() {
        let mut scene = text_scene(Angle::from_degrees(0.0));
        if let Node::Shape(shape) = &mut scene.nodes[0]
            && let Some(text) = &mut shape.text
        {
            text.origin = Point::new(f64::NAN, 0.0);
        }
        assert!(ink(&raster(&scene)).is_empty());

        let mut scene = text_scene(Angle::from_degrees(0.0));
        if let Node::Shape(shape) = &mut scene.nodes[0]
            && let Some(text) = &mut shape.text
        {
            for line in &mut text.layout.lines {
                for glyph in &mut line.glyphs {
                    glyph.font = usize::MAX;
                }
            }
        }
        assert!(ink(&raster(&scene)).is_empty());
    }

    #[test]
    fn stroke_cap_join_and_miter_fallbacks_draw() {
        let mut node = shape(path_rect(0.25, 0.25, 0.75, 0.75), None);
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.05);
            stroke.cap = LineCap::Square;
            stroke.join = LineJoin::Bevel;
            stroke.miter_limit = f64::NAN;
            shape.stroke = Some(stroke);
        }
        let scene = scene_of(1.0, 1.0, vec![node]);
        assert!(!ink(&raster(&scene)).is_empty());

        // The round join maps too.
        let mut node = shape(path_rect(0.25, 0.25, 0.75, 0.75), None);
        if let Node::Shape(shape) = &mut node {
            let mut stroke = stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 0)), 0.05);
            stroke.join = LineJoin::Round;
            shape.stroke = Some(stroke);
        }
        let scene = scene_of(1.0, 1.0, vec![node]);
        assert!(!ink(&raster(&scene)).is_empty());
    }

    #[test]
    fn a_stroke_wider_than_the_device_bound_is_skipped() {
        // `fits_device` widens the path bounds by the stroke: a width
        // whose reach is not finite leaves the device bound and is
        // skipped instead of handed to `tiny-skia` (P3-14 F4).
        let mut node = shape(path_rect(0.25, 0.25, 0.75, 0.75), Some(solid_red()));
        if let Node::Shape(shape) = &mut node {
            shape.stroke = Some(stroke(ScenePaint::Solid(Rgba::opaque(0, 0, 255)), 1.0e308));
        }
        let scene = scene_of(1.0, 1.0, vec![node]);
        let pixmap = raster(&scene);
        let blue = ink(&pixmap)
            .iter()
            .filter(|(x, y)| pixel(&pixmap, *x, *y).2 == 255)
            .count();
        assert_eq!(blue, 0, "a stroke past the device bound drew {blue} pixels");
    }

    #[test]
    fn a_pattern_tile_without_a_mark_draws_its_background() {
        let mut tileless = pattern(Size::new(0.2, 0.2), true);
        tileless.tile = Path::new();
        let scene = gradient_shape(ScenePaint::Pattern(tileless));
        assert!(!ink(&raster(&scene)).is_empty());
    }
}
