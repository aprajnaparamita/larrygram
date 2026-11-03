//! Golden image comparison with the ADR 0027 metric (task P3-10).
//!
//! The metric is ours and small: two PNG images match when they have the
//! same dimensions, every channel differs by at most [`MAX_CHANNEL_DELTA`]
//! of 255, and at most [`MAX_FRACTION`] of the pixels exceed that. It
//! absorbs libm noise across platforms (ADR 0025) while catching a wrong
//! arc, a missing fill or shifted text. The unit tests below pin the
//! metric itself on synthetic images.
//!
//! The goldens under `tests/golden/` are our own rendering of the
//! hand-authored fixtures in `corpus/fixtures/` (plus one corpus file,
//! `blue-box.vsdx`, skipped when the corpus is absent, and one drawing
//! built from our own stencil masters: task P7-44). Regenerate with
//!
//! ```sh
//! LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-render --test golden
//! ```
//!
//! which rewrites the goldens and prints which files changed; review is an
//! image diff of the changed files.

mod common;

use std::path::PathBuf;

use larrygram_render::{PngOptions, render_png};
use tiny_skia::{Color, Pixmap, PremultipliedColorU8};

/// The largest per-channel difference, of 255 (ADR 0027).
const MAX_CHANNEL_DELTA: u8 = 2;

/// The largest fraction of pixels that may exceed [`MAX_CHANNEL_DELTA`]
/// (ADR 0027: 0.5%).
const MAX_FRACTION: f64 = 0.005;

/// The raster scale of the golden images, in pixels per inch.
const GOLDEN_SCALE: f64 = 72.0;

/// The goldens rendered from the hand-authored fixtures (one file per
/// KICKOFF Phase 3 rendering feature).
const FIXTURE_GOLDENS: &[&str] = &[
    "render-geometry-absolute",
    "render-geometry-relative",
    "render-fills",
    "render-lines",
    "render-shadows",
    "render-text",
    "render-master-inheritance",
    "render-theme",
    "render-layers",
    "render-page-size",
    "render-background-pages",
];

/// The goldens rendered from corpus input (fetched, never committed).
const CORPUS_GOLDENS: &[&str] = &["blue-box"];

/// The goldens rendered from a drawing built in code from our own
/// stencil masters (task P7-44): a dropped Process and a dropped
/// Decision shape with typed labels.
const STENCIL_GOLDENS: &[&str] = &["stencil-text-centered"];

/// A blank drawing with a dropped Process and a dropped Decision shape,
/// each with its typed label, near the top of the page (task P7-44).
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
fn flowchart_labels() -> larrygram_model::Document {
    use larrygram_model::stencils;
    let mut document = larrygram_model::Document::new();
    for (name_u, label, pin_x) in [("Process", "Process", 1.5), ("Decision", "Decision", 4.2)] {
        let def = stencils::basic_flowchart()
            .masters
            .into_iter()
            .find(|master| master.name_u == name_u)
            .unwrap_or_else(|| panic!("no master {name_u}"));
        let shape = stencils::instantiate(&mut document, &def);
        let entity = document.shape_mut(shape).expect("shape");
        for (cell, value) in [("PinX", pin_x), ("PinY", 8.0)] {
            let entry = entity
                .sheet
                .cells
                .iter_mut()
                .find(|entry| entry.name.as_str() == cell)
                .unwrap_or_else(|| panic!("{name_u}: {cell}"));
            entry.value = Some(larrygram_model::CellValue::Number(value));
        }
        entity.text = Some(larrygram_model::TextContent {
            parts: vec![larrygram_model::TextPart::Text(label.to_owned())],
        });
    }
    document
}

/// The document of one code-built stencil golden (task P7-44).
fn stencil_case(name: &str) -> Option<larrygram_model::Document> {
    match name {
        "stencil-text-centered" => Some(flowchart_labels()),
        _ => None,
    }
}

/// The golden directory of this test: `crates/render/tests/golden/`.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// Decodes a PNG, panicking on a broken image (test code).
#[allow(clippy::expect_used)]
fn decode(png: &[u8]) -> Pixmap {
    Pixmap::decode_png(png).expect("decodes as a PNG")
}

/// The ADR 0027 metric: [`Err`] with a message when the images differ,
/// `Ok(())` when they match.
pub fn compare_images(expected: &[u8], actual: &[u8]) -> Result<(), String> {
    let expected = decode(expected);
    let actual = decode(actual);
    compare_pixels(&expected, &actual)
}

/// [`compare_images`] on two decoded rasters.
fn compare_pixels(expected: &Pixmap, actual: &Pixmap) -> Result<(), String> {
    let (ew, eh) = (expected.width(), expected.height());
    let (aw, ah) = (actual.width(), actual.height());
    if (ew, eh) != (aw, ah) {
        return Err(format!("dimensions differ: {ew}x{eh} vs {aw}x{ah}"));
    }
    let mut above = 0usize;
    let mut worst = 0u8;
    for (index, (e, a)) in expected.pixels().iter().zip(actual.pixels()).enumerate() {
        let delta = channel_delta(e, a);
        if delta > MAX_CHANNEL_DELTA {
            above += 1;
            if delta > worst {
                worst = delta;
            }
            if above == 1 {
                let (x, y) = (index % ew as usize, index / ew as usize);
                eprintln!("first differing pixel at ({x}, {y}): {e:?} vs {a:?}");
            }
        }
    }
    let total = expected.pixels().len();
    let fraction = if total == 0 {
        0.0
    } else {
        above as f64 / total as f64
    };
    if fraction > MAX_FRACTION {
        return Err(format!(
            "{above} of {total} pixels ({:.3}%) differ by more than {MAX_CHANNEL_DELTA}, worst {worst}",
            fraction * 100.0
        ));
    }
    Ok(())
}

/// The largest difference of the four channels of two pixels.
fn channel_delta(a: &PremultipliedColorU8, b: &PremultipliedColorU8) -> u8 {
    let (a, b) = (a.demultiply(), b.demultiply());
    a.red()
        .abs_diff(b.red())
        .max(a.green().abs_diff(b.green()))
        .max(a.blue().abs_diff(b.blue()))
        .max(a.alpha().abs_diff(b.alpha()))
}

/// The rendered PNG of one case's first page.
#[allow(clippy::expect_used)]
fn render_case(name: &str) -> Option<Vec<u8>> {
    let document = if let Some(document) = stencil_case(name) {
        document
    } else if name.starts_with("render-") {
        common::fixture_document(&format!("{name}.vsdx"))
    } else {
        common::corpus_document(&format!("{name}.vsdx"))?
    };
    let scenes = common::page_scenes(&document, true);
    let scene = scenes.first().expect("the fixture has a page");
    let options = PngOptions {
        scale: GOLDEN_SCALE,
        ..PngOptions::new()
    };
    Some(render_png(scene, &options).expect("renders"))
}

#[test]
fn golden_images_match_the_adr_0027_metric() {
    let mut cases: Vec<String> = FIXTURE_GOLDENS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    cases.extend(STENCIL_GOLDENS.iter().map(|name| (*name).to_owned()));
    for name in CORPUS_GOLDENS {
        if common::corpus_document(&format!("{name}.vsdx")).is_some() {
            cases.push((*name).to_owned());
        } else {
            eprintln!("skipping golden {name}: corpus/vsdx is absent");
        }
    }
    let update = std::env::var_os("LARRYGRAM_UPDATE_GOLDEN").is_some_and(|v| v == "1");
    for name in &cases {
        let rendered = render_case(name).expect("renders");
        let path = golden_dir().join(format!("{name}.png"));
        if update {
            let old = std::fs::read(&path).ok();
            if old.as_deref() == Some(rendered.as_slice()) {
                continue;
            }
            let what = if old.is_some() { "updated" } else { "created" };
            println!("{what} golden: {}", path.display());
            std::fs::create_dir_all(golden_dir()).expect("creates the golden directory");
            std::fs::write(&path, &rendered).expect("writes the golden");
            continue;
        }
        let saved = std::fs::read(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {name}.png; regenerate with \
                 LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-render --test golden"
            )
        });
        if let Err(diff) = compare_images(&saved, &rendered) {
            panic!("golden {name}.png: {diff}");
        }
    }
}

// --- the scene check (task P7-44) ------------------------------------

/// Task P7-44: the label of a dropped master lays out centred in the
/// shape's text block, both ways (scene check; the pixels are checked
/// by the `stencil-text-centered` golden above).
#[test]
fn stencil_text_lays_out_centred() {
    let document = flowchart_labels();
    let scenes = common::page_scenes(&document, true);
    let scene = scenes.first().expect("the drawing has a page");
    let texts: Vec<_> = scene
        .nodes
        .iter()
        .filter_map(|node| match node {
            larrygram_render::scene::Node::Shape(shape) => shape.text.as_ref(),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2, "Process and Decision draw their text");
    for (text, (width, height)) in texts.iter().zip([(1.5, 0.75), (1.2, 0.9)]) {
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        for line in &text.layout.lines {
            if let Some(first) = line.glyphs.first() {
                min_x = min_x.min(first.x);
                max_x = max_x.max(first.x + line.width);
            }
        }
        assert!(min_x.is_finite(), "the label has glyphs");
        let center_x = (min_x + max_x) * 0.5;
        assert!(
            (center_x - width * 0.5).abs() < 0.05,
            "centred horizontally: text centre {center_x} vs box {}",
            width * 0.5
        );
        let baseline = text.layout.lines.first().expect("a line").baseline;
        let box_mid = height * 0.5;
        assert!(
            (baseline - box_mid).abs() < 0.12,
            "centred vertically: baseline {baseline} vs box middle {box_mid}"
        );
    }
}

// --- the metric, on synthetic images ---------------------------------

/// A `width` by `height` raster of one flat colour, as PNG bytes.
#[allow(clippy::expect_used)]
fn flat(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
    let mut pixmap = Pixmap::new(width, height).expect("size");
    pixmap.fill(Color::from_rgba8(r, g, b, a));
    pixmap.encode_png().expect("encodes")
}

/// A pixel from unpremultiplied 8-bit channels.
fn rgba(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColorU8 {
    PremultipliedColorU8::from_rgba(r, g, b, a).unwrap_or(PremultipliedColorU8::TRANSPARENT)
}

#[test]
fn identical_images_match() {
    let image = flat(4, 4, 10, 20, 30, 255);
    assert_eq!(compare_images(&image, &image), Ok(()));
}

#[test]
fn a_two_channel_step_matches() {
    let a = flat(4, 4, 10, 20, 30, 255);
    let b = flat(4, 4, 12, 18, 32, 255);
    assert_eq!(compare_images(&a, &b), Ok(()));
}

#[test]
fn a_three_channel_step_on_one_pixel_matches() {
    // 0.5% of 16 pixels is 0.08 pixels, so one pixel above the delta is
    // already too many... unless it stays within the delta.
    let mut a = Pixmap::new(4, 4).expect("size");
    a.fill(Color::from_rgba8(10, 20, 30, 255));
    let mut b = Pixmap::new(4, 4).expect("size");
    b.fill(Color::from_rgba8(10, 20, 30, 255));
    if let Some(pixel) = b.pixels_mut().first_mut() {
        *pixel = rgba(13, 20, 30, 255);
    }
    let result = compare_images(
        &a.encode_png().expect("encodes"),
        &b.encode_png().expect("encodes"),
    );
    assert!(result.is_err(), "one pixel of 16 exceeds 0.5%");
}

#[test]
fn a_large_delta_on_a_small_fraction_matches() {
    // 250 of 100_000 pixels (0.25%) may exceed the delta.
    let mut a = Pixmap::new(400, 250).expect("size");
    a.fill(Color::from_rgba8(255, 255, 255, 255));
    let mut b = Pixmap::new(400, 250).expect("size");
    b.fill(Color::from_rgba8(255, 255, 255, 255));
    for pixel in b.pixels_mut().iter_mut().take(250) {
        *pixel = rgba(0, 0, 0, 255);
    }
    assert_eq!(
        compare_images(
            &a.encode_png().expect("encodes"),
            &b.encode_png().expect("encodes")
        ),
        Ok(())
    );
}

#[test]
fn a_large_delta_on_too_many_pixels_differs() {
    // 251 of 100_000 pixels (0.251%) exceed the delta... still under
    // 0.5%; take 600 (0.6%) to cross it.
    let mut a = Pixmap::new(400, 250).expect("size");
    a.fill(Color::from_rgba8(255, 255, 255, 255));
    let mut b = Pixmap::new(400, 250).expect("size");
    b.fill(Color::from_rgba8(255, 255, 255, 255));
    for pixel in b.pixels_mut().iter_mut().take(600) {
        *pixel = rgba(0, 0, 0, 255);
    }
    assert!(
        compare_images(
            &a.encode_png().expect("encodes"),
            &b.encode_png().expect("encodes")
        )
        .is_err()
    );
}

#[test]
fn different_dimensions_never_match() {
    let a = flat(4, 4, 10, 20, 30, 255);
    let b = flat(4, 5, 10, 20, 30, 255);
    let error = compare_images(&a, &b).expect_err("dimensions differ");
    assert!(error.contains("dimensions differ"), "{error}");
}

#[test]
fn an_alpha_step_counts_like_a_colour_step() {
    let a = flat(4, 4, 10, 20, 30, 255);
    let b = flat(4, 4, 10, 20, 30, 253);
    assert_eq!(compare_images(&a, &b), Ok(()));
    let c = flat(4, 4, 10, 20, 30, 250);
    assert!(compare_images(&a, &c).is_err());
}

#[test]
fn channel_delta_is_symmetric_and_bounded() {
    let a = rgba(10, 20, 30, 255);
    let b = rgba(12, 18, 32, 255);
    assert_eq!(channel_delta(&a, &b), 2);
    assert_eq!(channel_delta(&b, &a), 2);
    assert_eq!(channel_delta(&a, &a), 0);
}
