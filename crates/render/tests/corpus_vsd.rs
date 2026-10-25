//! End-to-end tests over the binary `.vsd` corpus (task P8-10): every
//! corpus file opens and renders, the `.vsd` to `.vsdx` round trip
//! keeps the model equal, and the imported drawings keep their PNG
//! goldens (ADR 0027).
//!
//! The corpus is fetched, never committed (ADR 0005): the tests skip
//! with a message when `corpus/vsd/` is absent and fail instead when
//! `LARRYGRAM_REQUIRE_CORPUS=1` is set. The version 5 files are out of
//! scope (ADR 0069): they must fail with a clean
//! `UnsupportedVersion(5)`, never a panic.
//!
//! The goldens under `tests/golden/vsd/` are our own rendering of the
//! imported corpus drawings (every page of each file that draws,
//! named `<name>-p<N>.png`), compared with
//! the ADR 0027 metric as `tests/golden.rs` implements it (a local
//! copy: test binaries cannot share code). A background page (P8-15:
//! `tdf76829-datetime-format.vsd`'s `VHintergrund`) has no golden of
//! its own: it draws composited behind its foreground page, as Visio
//! draws it. Regenerate with
//!
//! ```sh
//! LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-render --test corpus_vsd
//! ```
//!
//! and review the changed images.

use std::path::{Path, PathBuf};

/// The `.vsdx` twin comparison (P8-15 goal 8): the shared comparison
/// of one import against its twin. It lives beside the corpus tests of
/// `larrygram-vsd-binary` (`crates/vsd-binary/tests/twins.rs` owns it
/// and its contract); this suite drives it because the `.vsdx` reader
/// is a same-layer crate `larrygram-vsd-binary` may not depend on
/// (ADR 0002).
#[path = "../../vsd-binary/tests/twins/compare.rs"]
mod twin_compare;

use larrygram_render::{PngOptions, SvgOptions, render_png, render_svg};
use larrygram_vsd_binary::{Error, Limits as VsdLimits, import};
use larrygram_vsdx::{Limits, Package, read_document, write_document};
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// The largest per-channel difference, of 255 (ADR 0027).
const MAX_CHANNEL_DELTA: u8 = 2;

/// The largest fraction of pixels that may exceed [`MAX_CHANNEL_DELTA`]
/// (ADR 0027: 0.5%).
const MAX_FRACTION: f64 = 0.005;

/// The raster scale of the golden images, in pixels per inch.
const GOLDEN_SCALE: f64 = 72.0;

/// The corpus directory with the real `.vsd` files.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("vsd")
}

/// The `.vsd` files of the corpus, sorted for a stable order.
fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(corpus_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsd"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// Fails the calling test when `LARRYGRAM_REQUIRE_CORPUS=1` is set and
/// the corpus is absent; returns `true` (the test returns early) when
/// the corpus is absent and the test may skip.
fn require_corpus_or(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        assert!(
            std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_none(),
            "corpus/vsd is missing and LARRYGRAM_REQUIRE_CORPUS=1 is set"
        );
        eprintln!("skipping: corpus/vsd is absent; run `cargo xtask corpus`");
        true
    } else {
        false
    }
}

/// Imports one corpus file: `None` for the version 5 files (a clean
/// error is their contract), a panic on any other failure.
#[allow(clippy::panic)] // test helper: a file that must import must say so
fn imported(name: &str) -> Option<larrygram_vsd_binary::Import> {
    let bytes = std::fs::read(corpus_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    match import(&bytes, &VsdLimits::DEFAULT) {
        Ok(value) => Some(value),
        Err(Error::UnsupportedVersion(5)) => None,
        Err(other) => panic!("{name}: does not import: {other:?}"),
    }
}

/// The scene of every page that draws: every page that is not another
/// page's background page, in page order, with its background page
/// composited behind it (a background page like
/// `tdf76829-datetime-format.vsd`'s `VHintergrund` draws behind its
/// foreground page and never as a page of its own).
#[allow(clippy::expect_used)] // test helper: the page scene must build
fn drawing_scenes(document: &larrygram_model::Document) -> Vec<larrygram_render::Scene> {
    use larrygram_model::PageId;
    use larrygram_render::{BuildOptions, build_scene};
    let backgrounds: std::collections::BTreeSet<u32> = document
        .pages
        .iter()
        .filter_map(|page| page.background.map(|background| background.index))
        .collect();
    let options = BuildOptions {
        background: true,
        ..BuildOptions::new()
    };
    document
        .pages
        .iter()
        .enumerate()
        .filter(|(index, _)| !backgrounds.contains(&u32::try_from(*index).unwrap_or(u32::MAX)))
        .map(|(index, _)| {
            build_scene(
                document,
                PageId {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                },
                &options,
            )
            .expect("builds the page scene")
        })
        .collect()
}

/// Every corpus file opens and every page that draws renders through
/// both backends, its background page composited behind it. Warnings
/// are allowed (ADR 0071); an `Err` or a panic fails. The version 5
/// files must fail cleanly.
#[test]
fn every_corpus_vsd_opens_and_renders_every_page() {
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    let mut pages = 0usize;
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = std::fs::read(file).unwrap_or_else(|e| panic!("{name}: {e}"));
        let imported = match import(&bytes, &VsdLimits::DEFAULT) {
            Ok(imported) => imported,
            Err(Error::UnsupportedVersion(5)) => continue,
            Err(other) => panic!("{name}: does not import: {other:?}"),
        };
        for scene in drawing_scenes(&imported.document) {
            let png = render_png(&scene, &PngOptions::new())
                .unwrap_or_else(|e| panic!("{name} page {pages}: render_png: {e}"));
            assert!(!png.is_empty(), "{name} page {pages}: empty PNG");
            let svg = render_svg(&scene, &SvgOptions::new())
                .unwrap_or_else(|e| panic!("{name} page {pages}: render_svg: {e}"));
            assert!(
                svg.starts_with("<?xml") && svg.ends_with("</svg>\n"),
                "{name} page {pages}: not a standalone SVG document"
            );
            assert!(
                scene.width.is_finite()
                    && scene.height.is_finite()
                    && scene.width > 0.0
                    && scene.height > 0.0,
                "{name} page {pages}: non-finite page size"
            );
            pages += 1;
        }
    }
    assert!(pages > 0, "no pages rendered");
}

/// The round trip (task P8-10 goal 1): open a `.vsd`, save it as
/// `.vsdx`, reopen it, and the model must be equal (the model diff is
/// empty). The writer is the one every other save path uses (ADR 0070:
/// the model is the `.vsdx` reader's).
#[test]
fn the_vsd_to_vsdx_round_trip_keeps_the_model_equal() {
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    let mut compared = 0usize;
    let mut failures = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(imported) = imported(&name) else {
            continue;
        };
        let written = write_document(&imported.document, &Package::new())
            .unwrap_or_else(|e| panic!("{name}: does not save: {e:?}"));
        let reopened = read_document(&written, &Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: does not reopen: {e:?}"));
        if let Some(diff) = model_diff(&comparable(&imported.document), &comparable(&reopened)) {
            failures.push(format!("{name}:\n{diff}"));
        }
        compared += 1;
    }
    assert!(
        failures.is_empty(),
        "the round trip changed the model:\n\n{}",
        failures.join("\n\n")
    );
    assert!(compared > 0, "no file round-tripped");
}

/// The model compared by the round trip: without the `Preserved`
/// bookkeeping and without arena shapes no page or master references.
///
/// Not compared (and why):
///
/// - `Preserved` content at every level: the file's verbatim XML kept
///   beside the mapped model (ADR 0012). The `.vsdx` reader keeps
///   elements the `.vsd` file never had (it keeps `Text` and `Colors`
///   verbatim beside their typed copies), so preserved content cannot
///   compare equal across formats. The typed model is what must survive
///   the round trip.
/// - Shapes no page or master references: a `.vsd` document-scope shape
///   record (every corpus file carries one, named `Gesture Format` or
///   unnamed) has no `.vsdx` slot at all, so the save drops it. The
///   oracle renders no layer for it either. See the task report's open
///   issues.
fn comparable(document: &larrygram_model::Document) -> larrygram_model::Document {
    let mut document = project_referenced_shapes(document);
    strip_preserved(&mut document);
    document
}

/// A copy of `document` with only the shapes a page or a master
/// references (their trees included), the arena renumbered in order and
/// every id rewritten.
///
/// The order is document order: the top-level shapes of every page and
/// master in their list order, each with its sub-shapes depth first.
/// That is the order the model is written and read back in (the writer
/// writes the trees, the reader rebuilds the arena in encounter order),
/// so the round trip compares like with like.
fn project_referenced_shapes(document: &larrygram_model::Document) -> larrygram_model::Document {
    use larrygram_model::ShapeId;
    let mut document = document.clone();
    let mut reachable = vec![false; document.shapes.len()];
    let mut order: Vec<u32> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    for page in &document.pages {
        stack.extend(page.shapes.iter().rev().map(|id| id.index));
    }
    for master in &document.masters {
        stack.extend(master.shapes.iter().rev().map(|id| id.index));
    }
    while let Some(index) = stack.pop() {
        let Some(slot) = reachable.get_mut(index as usize) else {
            continue;
        };
        if *slot {
            continue;
        }
        *slot = true;
        order.push(index);
        if let Some(shape) = document.shapes.get(index as usize) {
            stack.extend(shape.children.iter().rev().map(|id| id.index));
        }
    }
    let mut map = vec![u32::MAX; document.shapes.len()];
    let mut kept = Vec::new();
    for index in order {
        if let Some(slot) = map.get_mut(index as usize) {
            *slot = u32::try_from(kept.len()).unwrap_or(u32::MAX);
        }
        if let Some(shape) = document.shapes.get(index as usize) {
            kept.push(shape.clone());
        }
    }
    let remap = |id: u32| -> ShapeId {
        ShapeId {
            index: map.get(id as usize).copied().unwrap_or(id),
        }
    };
    for shape in &mut kept {
        shape.id = remap(shape.id.index);
        shape.parent = shape.parent.map(|parent| remap(parent.index));
        shape.children = shape
            .children
            .iter()
            .map(|child| remap(child.index))
            .collect();
    }
    document.shapes = kept;
    for page in &mut document.pages {
        page.shapes = page.shapes.iter().map(|id| remap(id.index)).collect();
    }
    for master in &mut document.masters {
        master.shapes = master.shapes.iter().map(|id| remap(id.index)).collect();
    }
    document
}

/// Clears every `Preserved` of the document (the not-compared list of
/// [`comparable`]).
fn strip_preserved(document: &mut larrygram_model::Document) {
    use larrygram_model::{Preserved, Sheet};
    fn sheet(sheet: &mut Sheet) {
        for cell in &mut sheet.cells {
            cell.preserved = Preserved::new();
        }
        for section in &mut sheet.sections {
            section.preserved = Preserved::new();
            for cell in &mut section.cells {
                cell.preserved = Preserved::new();
            }
            for row in &mut section.rows {
                row.preserved = Preserved::new();
                for cell in &mut row.cells {
                    cell.preserved = Preserved::new();
                }
            }
        }
    }
    document.preserved = Preserved::new();
    for shape in &mut document.shapes {
        shape.preserved = Preserved::new();
        sheet(&mut shape.sheet);
    }
    for page in &mut document.pages {
        page.preserved = Preserved::new();
        sheet(&mut page.sheet);
        for connect in &mut page.connects {
            connect.preserved = Preserved::new();
        }
    }
    for master in &mut document.masters {
        master.preserved = Preserved::new();
    }
    for style in &mut document.stylesheet.styles {
        style.preserved = Preserved::new();
        sheet(&mut style.sheet);
    }
}

/// The model diff of two documents: `None` when they are equal,
/// otherwise the first differences as short text. The comparison is
/// whole documents ([`larrygram_model::Document`] derives `PartialEq`);
/// this helper only makes a failure readable.
fn model_diff(
    before: &larrygram_model::Document,
    after: &larrygram_model::Document,
) -> Option<String> {
    if before == after {
        return None;
    }
    let mut out = Vec::new();
    if before.kind != after.kind {
        out.push(format!("kind: {:?} vs {:?}", before.kind, after.kind));
    }
    if before.shapes.len() != after.shapes.len() {
        out.push(format!(
            "shape count: {} vs {}",
            before.shapes.len(),
            after.shapes.len()
        ));
    }
    for (index, (a, b)) in before.shapes.iter().zip(after.shapes.iter()).enumerate() {
        if a.text != b.text {
            out.push(format!(
                "shape {index}: text {:?} vs {:?}",
                text_of(a.text.as_ref()),
                text_of(b.text.as_ref())
            ));
        }
        if a.sheet != b.sheet {
            out.push(format!("shape {index}: the sheet differs"));
        }
        if a.name != b.name
            || a.visio_id != b.visio_id
            || a.master != b.master
            || a.parent != b.parent
            || a.children != b.children
        {
            out.push(format!(
                "shape {index}: its id, name, master or tree place differs"
            ));
        }
        if a.line_style != b.line_style
            || a.fill_style != b.fill_style
            || a.text_style != b.text_style
        {
            out.push(format!("shape {index}: the style links differ"));
        }
        if a.preserved != b.preserved {
            out.push(format!("shape {index}: the preserved content differs"));
        }
        if out.len() >= 8 {
            break;
        }
    }
    if before.pages != after.pages {
        if before.pages.len() != after.pages.len() {
            out.push(format!(
                "page count: {} vs {}",
                before.pages.len(),
                after.pages.len()
            ));
        }
        for (index, (a, b)) in before.pages.iter().zip(after.pages.iter()).enumerate() {
            if a != b {
                out.push(format!("page {index} differs"));
            }
        }
    }
    if before.masters != after.masters {
        if before.masters.len() != after.masters.len() {
            out.push(format!(
                "master count: {} vs {}",
                before.masters.len(),
                after.masters.len()
            ));
        }
        for (index, (a, b)) in before.masters.iter().zip(after.masters.iter()).enumerate() {
            if a != b {
                out.push(format!("master {index} differs"));
            }
        }
    }
    if before.stylesheet != after.stylesheet {
        out.push("the stylesheet differs".to_owned());
    }
    if before.color_table != after.color_table {
        out.push(format!(
            "the colour table differs: {} vs {} entries",
            before.color_table.len(),
            after.color_table.len()
        ));
    }
    if before.properties != after.properties {
        out.push("the document properties differ".to_owned());
    }
    if before.themes != after.themes {
        out.push("the theme differs".to_owned());
    }
    if before.preserved != after.preserved {
        out.push("the preserved root content differs".to_owned());
    }
    if out.is_empty() {
        out.push("the documents differ in a field the diff does not list".to_owned());
    }
    Some(out.join("\n"))
}

/// The literal text of one shape's text, run markers dropped.
fn text_of(content: Option<&larrygram_model::TextContent>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    content
        .parts
        .iter()
        .filter_map(|part| match part {
            larrygram_model::TextPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// The golden images of the imported corpus drawings: every page of
/// each importable corpus file, in `tests/golden/vsd/` as
/// `<name>-p<N>.png` (page numbers from 1).
#[test]
fn golden_images_of_the_imported_corpus_drawings() {
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    let update = std::env::var_os("LARRYGRAM_UPDATE_GOLDEN").is_some_and(|v| v == "1");
    let mut cases = 0usize;
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(imported) = imported(&name) else {
            continue;
        };
        let stem = name.strip_suffix(".vsd").unwrap_or(&name);
        for (index, scene) in drawing_scenes(&imported.document).iter().enumerate() {
            let rendered = render_png(
                scene,
                &PngOptions {
                    scale: GOLDEN_SCALE,
                    ..PngOptions::new()
                },
            )
            .unwrap_or_else(|e| panic!("{name} page {}: render_png: {e}", index + 1));
            let golden_name = format!("{stem}-p{}.png", index + 1);
            let path = golden_vsd_dir().join(&golden_name);
            if update {
                let old = std::fs::read(&path).ok();
                if old.as_deref() == Some(rendered.as_slice()) {
                    cases += 1;
                    continue;
                }
                let what = if old.is_some() { "updated" } else { "created" };
                println!("{what} golden: {}", path.display());
                std::fs::create_dir_all(golden_vsd_dir()).expect("creates the golden directory");
                std::fs::write(&path, &rendered).expect("writes the golden");
                cases += 1;
                continue;
            }
            let saved = std::fs::read(&path).unwrap_or_else(|_| {
                panic!(
                    "missing golden {golden_name}; regenerate with \
                     LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-render --test corpus_vsd"
                )
            });
            if let Err(diff) = compare_images(&saved, &rendered) {
                panic!("golden {golden_name}: {diff}");
            }
            cases += 1;
        }
    }
    assert!(cases > 0, "no golden compared");
}

/// The `.vsdx` twin comparison (P8-15 goal 8): every corpus `.vsd`
/// that has a `.vsdx` twin must import to the document the `.vsdx`
/// reader reads from it (pages, shapes, the cells the renderer reads,
/// and text, per page). The twins are found through
/// `LARRYGRAM_VSD_TWINS`, the folder that holds `vsdx/`; the run skips
/// cleanly when the variable is unset or no twin is there yet, like
/// the libvisio oracle (ADR 0010).
#[test]
fn the_vsd_imports_match_their_vsdx_twins() {
    let Some(dir) = std::env::var_os("LARRYGRAM_VSD_TWINS").map(PathBuf::from) else {
        eprintln!(
            "skipping the twin comparison: LARRYGRAM_VSD_TWINS is unset;              point it at the folder that holds vsdx/"
        );
        return;
    };
    if !dir.join("vsdx").is_dir() {
        eprintln!(
            "skipping the twin comparison: {} has no vsdx/ folder",
            dir.display()
        );
        return;
    }
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    let mut compared = 0usize;
    let mut failures = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = name.strip_suffix(".vsd").unwrap_or(&name);
        let twin_path = dir.join("vsdx").join(format!("{stem}.vsdx"));
        let Ok(twin_bytes) = std::fs::read(&twin_path) else {
            continue; // the twin has not arrived yet
        };
        let Some(imported) = imported(&name) else {
            continue; // the version 5 files have no twin
        };
        let twin = read_document(&twin_bytes, &Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{stem}.vsdx: does not read: {e:?}"));
        let mismatches = twin_compare::compare_documents(&imported.document, &twin);
        if !mismatches.is_empty() {
            failures.push(format!("{name}:\n  {}", mismatches.join("\n  ")));
        }
        compared += 1;
    }
    if compared == 0 {
        eprintln!(
            "skipping the twin comparison: no corpus file has a twin under {} yet",
            dir.display()
        );
        return;
    }
    assert!(
        failures.is_empty(),
        "the imports differ from their twins:\n\n{}",
        failures.join("\n\n")
    );
}

/// The golden directory of this test: `crates/render/tests/golden/vsd/`.
fn golden_vsd_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("vsd")
}

// --- the ADR 0027 metric (a copy of `tests/golden.rs`) ---------------

/// Decodes a PNG, panicking on a broken image (test code).
#[allow(clippy::expect_used)]
fn decode(png: &[u8]) -> Pixmap {
    Pixmap::decode_png(png).expect("decodes as a PNG")
}

/// The ADR 0027 metric: [`Err`] with a message when the images differ,
/// `Ok(())` when they match.
fn compare_images(expected: &[u8], actual: &[u8]) -> Result<(), String> {
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

#[cfg(test)]
mod metric_tests {
    use super::*;
    use tiny_skia::Color;

    /// A `width` by `height` raster of one flat colour, as PNG bytes.
    #[allow(clippy::expect_used)]
    fn flat(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        let mut pixmap = Pixmap::new(width, height).expect("size");
        pixmap.fill(Color::from_rgba8(r, g, b, a));
        pixmap.encode_png().expect("encodes")
    }

    #[test]
    fn identical_images_match() {
        let image = flat(4, 4, 10, 20, 30, 255);
        assert_eq!(compare_images(&image, &image), Ok(()));
    }

    #[test]
    fn different_dimensions_never_match() {
        let a = flat(4, 4, 10, 20, 30, 255);
        let b = flat(4, 5, 10, 20, 30, 255);
        let error = compare_images(&a, &b).expect_err("dimensions differ");
        assert!(error.contains("dimensions differ"), "{error}");
    }
}
