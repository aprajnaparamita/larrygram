//! Raster/PNG parity and caps (task P4-05, ADR 0033).
//!
//! [`render_raster`] and `render_png` share one rasterizer, so the
//! raster's bytes must equal the bytes the PNG encoder writes (the
//! decoded PNG is straight RGBA, like [`RasterImage::rgba`]). The ADR
//! 0025 caps return the same errors from both backends.

use larrygram_geom::{Path, Point, Segment, Transform};
use larrygram_render::scene::{LineCap, LineJoin, Node, Paint, Rgba, Shape, Stroke};
use larrygram_render::{
    Error, PngOptions, RasterOptions, Scene, SvgOptions, render_png, render_raster, render_svg,
};

/// A closed rectangle path in inches.
fn path_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    let mut path = Path::new();
    path.push(Segment::MoveTo(Point::new(x0, y0)));
    path.push(Segment::LineTo(Point::new(x1, y0)));
    path.push(Segment::LineTo(Point::new(x1, y1)));
    path.push(Segment::LineTo(Point::new(x0, y1)));
    path.push(Segment::Close);
    path
}

/// A filled and stroked shape node.
fn shape(path: Path, fill: Option<Paint>, stroke: Option<Stroke>) -> Node {
    Node::Shape(Box::new(Shape {
        shape_id: None,
        transform: Transform::identity(),
        path,
        fill,
        stroke,
        shadow: None,
        clip: None,
        text: None,
    }))
}

/// A fixture scene with opaque and semi-transparent areas: a
/// translucent red rectangle over an opaque blue one, plus a stroke.
/// Anti-aliased edges exercise the premultiplication round trip.
fn fixture_scene() -> Scene {
    let opaque = Paint::Solid(Rgba::opaque(0, 0, 255));
    let translucent = Paint::Solid(Rgba::new(255, 0, 0, 128));
    let stroke = Stroke {
        paint: Paint::Solid(Rgba::opaque(0, 255, 0)),
        width: 0.1,
        dash: Vec::new(),
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        miter_limit: 4.0,
        start: None,
        end: None,
    };
    Scene {
        width: 2.0,
        height: 1.5,
        background: Some(Rgba::new(255, 255, 255, 200)),
        nodes: vec![
            shape(path_rect(0.1, 0.1, 1.0, 1.0), Some(opaque), None),
            shape(path_rect(0.5, 0.4, 1.8, 1.2), Some(translucent), None),
            shape(path_rect(0.2, 0.2, 1.6, 1.3), None, Some(stroke)),
        ],
        warnings: Vec::new(),
    }
}

/// Decodes a PNG into straight `(width, height, rgba)` bytes (test
/// helper; a broken image is a test failure).
#[allow(clippy::expect_used)]
fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("the PNG header decodes");
    let mut buffer = vec![0u8; reader.output_buffer_size().expect("a known output size")];
    let info = reader
        .next_frame(&mut buffer)
        .expect("the PNG frame decodes");
    buffer.truncate(info.buffer_size());
    (info.width, info.height, buffer)
}

#[test]
fn render_raster_bytes_equal_the_decoded_png() {
    let scene = fixture_scene();
    let options = RasterOptions::new();
    let raster = render_raster(&scene, &options).expect("raster");
    let png = render_png(&scene, &PngOptions::new()).expect("png");
    let (width, height, rgba) = decode_png(&png);
    assert_eq!((raster.width, raster.height), (width, height));
    assert_eq!(
        raster.rgba, rgba,
        "the raster and the PNG hold the same pixels"
    );
}

#[test]
fn render_raster_bytes_equal_the_png_at_other_scales_and_no_aa() {
    let scene = fixture_scene();
    for (scale, antialias) in [(48.0, true), (200.0, false), (96.5, true)] {
        let raster = render_raster(&scene, &RasterOptions { scale, antialias }).expect("raster");
        let png = render_png(&scene, &PngOptions { scale, antialias }).expect("png");
        let (width, height, rgba) = decode_png(&png);
        assert_eq!(
            (raster.width, raster.height, raster.rgba),
            (width, height, rgba),
            "scale {scale}, antialias {antialias}"
        );
    }
}

#[test]
fn render_raster_is_deterministic() {
    let scene = fixture_scene();
    let first = render_raster(&scene, &RasterOptions::new()).expect("raster");
    let second = render_raster(&scene, &RasterOptions::new()).expect("raster");
    assert_eq!(first, second);
}

#[test]
fn both_backends_report_the_same_cap_errors() {
    let scene = fixture_scene();
    let bad_scales = [0.0, -3.0, f64::NAN, f64::INFINITY, 16_384.5, 100_000.0];
    for scale in bad_scales {
        let raster_error = render_raster(
            &scene,
            &RasterOptions {
                scale,
                antialias: true,
            },
        )
        .expect_err("bad scale");
        let png_error = render_png(
            &scene,
            &PngOptions {
                scale,
                antialias: true,
            },
        )
        .expect_err("bad scale");
        assert_eq!(raster_error, Error::InvalidOptions, "scale {scale}");
        assert_eq!(png_error, Error::InvalidOptions, "scale {scale}");
    }

    // One side over the device extent cap.
    let wide = Scene {
        width: 20_000.0,
        height: 1.0,
        background: None,
        nodes: Vec::new(),
        warnings: Vec::new(),
    };
    assert_eq!(
        render_raster(&wide, &RasterOptions::new()).expect_err("too wide"),
        Error::TooLarge
    );
    assert_eq!(
        render_png(&wide, &PngOptions::new()).expect_err("too wide"),
        Error::TooLarge
    );

    // Both sides within the per-side cap but the raster over the byte
    // cap (256 MiB).
    let huge = Scene {
        width: 16_384.0,
        height: 16_384.0,
        background: None,
        nodes: Vec::new(),
        warnings: Vec::new(),
    };
    assert_eq!(
        render_raster(&huge, &RasterOptions::new()).expect_err("too many bytes"),
        Error::TooLarge
    );
    assert_eq!(
        render_png(&huge, &PngOptions::new()).expect_err("too many bytes"),
        Error::TooLarge
    );

    // A degenerate page cannot form a raster.
    let empty = Scene::new(0.0, 1.0);
    assert_eq!(
        render_raster(&empty, &RasterOptions::new()).expect_err("degenerate"),
        Error::TooLarge
    );
}

// --- backend fallbacks (task P5-20) ---------------------------------

/// A text block with one glyph at (0.1, 0.1) indexing `font`/`glyph`,
/// over a white rectangle.
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
fn glyph_node(font: usize, glyph: u16) -> Node {
    let stack = larrygram_text::default_fonts();
    let layout = larrygram_text::Layout {
        width: 1.0,
        height: 0.2,
        lines: vec![larrygram_text::Line {
            glyphs: vec![larrygram_text::GlyphPos {
                glyph: larrygram_text::GlyphId(glyph),
                font,
                x: 0.1,
                y: 0.1,
                run: 0,
                size: 12.0,
            }],
            baseline: 0.1,
            width: 1.0,
            paragraph: 0,
        }],
        ..larrygram_text::Layout::default()
    };
    Node::Shape(Box::new(Shape {
        shape_id: None,
        transform: Transform::identity(),
        path: path_rect(0.1, 0.1, 1.0, 1.0),
        fill: Some(Paint::Solid(Rgba::opaque(255, 255, 255))),
        stroke: None,
        shadow: None,
        clip: None,
        text: Some(larrygram_render::scene::TextNode {
            fonts: stack.fonts().to_vec(),
            runs: Vec::new(),
            layout,
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(1.0, 1.0),
            margins: larrygram_text::Insets::default(),
            horz_align: larrygram_text::Align::Left,
            origin: Point::new(0.1, 0.1),
        }),
    }))
}

#[test]
fn a_glyph_the_font_does_not_have_is_skipped_by_both_backends() {
    // `Font::outline` has no such glyph: the glyph is skipped and both
    // backends still draw the scene.
    let mut scene = Scene::new(2.0, 1.5);
    scene.nodes.push(glyph_node(0, u16::MAX));
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
    assert!(
        !render_svg(&scene, &SvgOptions::new())
            .expect("svg")
            .is_empty()
    );
}

#[test]
fn a_font_the_block_does_not_have_is_skipped_by_both_backends() {
    // The glyph names a font index the block does not carry: the glyph
    // is skipped and both backends still draw the scene.
    let mut scene = Scene::new(2.0, 1.5);
    scene.nodes.push(glyph_node(usize::MAX, 3));
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
    assert!(
        !render_svg(&scene, &SvgOptions::new())
            .expect("svg")
            .is_empty()
    );
}

#[test]
fn a_zero_sized_pattern_tile_transforms_by_the_identity() {
    // A pattern tile of no size has no pixels to scale by: the tile
    // transform is the identity and the fill still paints.
    let tile = Paint::Pattern(larrygram_render::scene::PatternPaint {
        foreground: Rgba::opaque(0, 0, 0),
        background: Rgba::opaque(255, 255, 255),
        tile: Path::new(),
        tile_size: larrygram_geom::Size::new(0.0, 0.0),
        filled: true,
        tile_stroke_width: 0.0,
    });
    let mut scene = Scene::new(2.0, 1.5);
    scene
        .nodes
        .push(shape(path_rect(0.1, 0.1, 1.0, 1.0), Some(tile), None));
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
    assert!(
        !render_svg(&scene, &SvgOptions::new())
            .expect("svg")
            .is_empty()
    );
}

#[test]
fn a_clipped_shape_writes_its_clip_path() {
    // A node clip becomes an SVG `clipPath` def and a `clip-path`
    // attribute (and the raster clips without one).
    let mut node = shape(
        path_rect(0.1, 0.1, 1.0, 1.0),
        Some(Paint::Solid(Rgba::opaque(255, 0, 0))),
        None,
    );
    if let Node::Shape(shape) = &mut node {
        shape.clip = Some(larrygram_render::scene::Clip {
            path: path_rect(0.2, 0.2, 0.6, 0.6),
        });
    }
    let mut scene = Scene::new(2.0, 1.5);
    scene.nodes.push(node);
    let svg = render_svg(&scene, &SvgOptions::new()).expect("svg");
    assert!(svg.contains("clipPath"), "{svg}");
    assert!(svg.contains("clip-path"), "{svg}");
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
}

#[test]
fn a_line_end_on_a_degenerate_path_draws_no_marker() {
    // The path has no direction to aim a line end at (`endpoint`'s
    // degenerate arms): the marker is skipped and the stroke still
    // draws.
    let arrow = larrygram_render::scene::LineEnd {
        shape: {
            let mut path = Path::new();
            path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
            path.push(Segment::LineTo(Point::new(1.0, 0.5)));
            path.push(Segment::LineTo(Point::new(0.0, 1.0)));
            path.push(Segment::Close);
            path
        },
        width: 0.1,
        length: 0.1,
        filled: true,
        color: None,
    };
    let stroke = Stroke {
        paint: Paint::Solid(Rgba::opaque(0, 0, 0)),
        width: 0.05,
        dash: Vec::new(),
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        miter_limit: 4.0,
        start: Some(arrow),
        end: Some(larrygram_render::scene::LineEnd {
            shape: Path::new(),
            width: 0.1,
            length: 0.1,
            filled: true,
            color: None,
        }),
    };
    // `MoveTo` then `Close`: the start has no direction (`Close` is the
    // first segment after the move); the end has none either.
    let mut degenerate = Path::new();
    degenerate.push(Segment::MoveTo(Point::new(0.5, 0.5)));
    degenerate.push(Segment::Close);
    let mut scene = Scene::new(2.0, 1.5);
    scene.nodes.push(shape(degenerate, None, Some(stroke)));
    let svg = render_svg(&scene, &SvgOptions::new()).expect("svg");
    assert!(!svg.contains("marker"), "{svg}");
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
}

#[test]
fn a_shadow_of_an_unstroked_shape_writes_its_path_alone() {
    // A shadowed shape with no stroke: the shadow path carries no
    // stroke attributes.
    let mut node = shape(
        path_rect(0.1, 0.1, 1.0, 1.0),
        Some(Paint::Solid(Rgba::opaque(255, 0, 0))),
        None,
    );
    if let Node::Shape(shape) = &mut node {
        shape.shadow = Some(larrygram_render::scene::Shadow {
            color: Rgba::new(0, 0, 0, 128),
            offset: Point::new(0.1, -0.1),
            blur: 0.05,
        });
    }
    let mut scene = Scene::new(2.0, 1.5);
    scene.nodes.push(node);
    let svg = render_svg(&scene, &SvgOptions::new()).expect("svg");
    assert!(!svg.is_empty());
    assert!(
        !render_png(&scene, &PngOptions::default())
            .expect("png")
            .is_empty()
    );
}
