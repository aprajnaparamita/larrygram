//! The wide twin sweep (task P8-21): the `.vsdx` twins of the wide
//! export set against the imports of their originals, and a
//! side-by-side picture page of every exported page.
//!
//! Two ignored tests drive the local data the master's Visio run left
//! under `reference/wide-twins/` (never committed, ADR 0005): the 77
//! converted binaries in `vsdx/`, the 206 full-page PDFs in `pdf/`,
//! and `reference/wide-corpus/` with the originals and
//! `export-set.csv`. Both skip cleanly unless `LARRYGRAM_WIDE_TWINS`
//! and `LARRYGRAM_WIDE_CORPUS` are set (the skip contract of the
//! libvisio oracle, ADR 0010).
//!
//! - [`the_wide_twin_sweep_compares_every_converted_file`] runs the
//!   shared twin comparison ([`twin_compare::compare_documents`], with
//!   the drawing-units normalisation of P8-21 goal 1) over every
//!   converted file and writes the local `twin-sweep.csv`: one row per
//!   file with the pages, the shapes joined, the cells compared, the
//!   cells that agree and the mismatches by cell family. The totals
//!   print at the end. A file that panics in a reader is a bug: the
//!   sweep records it as `panic` and goes on.
//! - [`every_export_page_gets_a_side_by_side_picture`] renders every
//!   page of every file of the export set with our renderer, rasterises
//!   Visio's PDF at the same width (the system `pdftoppm`; skipped with
//!   a note when it is missing) and writes the local `compare.html`
//!   (worst first) and `picture-scores.csv`, scored with the ADR 0027
//!   metric.
//!
//! Nothing a drawing's content produced enters the repository: the
//! CSVs, the HTML and the PNGs stay under
//! `/Volumes/Dara/larrygram/reference/wide-twins/` (AGENTS.md section
//! 3, master note "Privacy").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The comparison of one import against its `.vsdx` twin (P8-15 goal
/// 8, P8-21 goal 1): format-agnostic, our own MIT code, included from
/// this suite as from the other suites that run it.
#[path = "twins/compare.rs"]
mod twin_compare;

use larrygram_model::{Document, PageId};
use larrygram_render::{BuildOptions, PngOptions, Scene, build_scene, render_png};
use larrygram_vsd_binary::{Limits as VsdLimits, import};
use larrygram_vsdx::{Limits, read_document};
use tiny_skia::{Pixmap, PremultipliedColorU8};
use twin_compare::{Family, compare_documents};

/// The environment variable that points at the wide twins.
const TWINS_ENV: &str = "LARRYGRAM_WIDE_TWINS";

/// The environment variable that points at the wide corpus.
const CORPUS_ENV: &str = "LARRYGRAM_WIDE_CORPUS";

/// The width every picture renders and rasterises at, in pixels.
const PICTURE_WIDTH: u32 = 600;

/// The largest per-channel difference, of 255 (ADR 0027).
const MAX_CHANNEL_DELTA: u8 = 2;

/// The twins directory one environment value names: `None` when the
/// value is unset, when it names no directory or when the directory
/// has no `vsdx/` (the skip contract).
fn wide_twins_dir(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(value?);
    (dir.is_dir() && dir.join("vsdx").is_dir()).then_some(dir)
}

/// The wide-corpus directory one environment value names: `None` when
/// the value is unset or the directory has no `index.csv`.
fn wide_corpus_dir(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(value?);
    (dir.is_dir() && dir.join("index.csv").is_file()).then_some(dir)
}

/// The originals of the corpus, by blob id: `reference/wide-corpus/`
/// holds them as `<tier>/<blob_id>.<ext>`.
fn originals(corpus: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    for tier in ["a", "b"] {
        let Ok(entries) = std::fs::read_dir(corpus.join(tier)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
            else {
                continue;
            };
            out.entry(stem).or_insert(path);
        }
    }
    out
}

/// The blobs of `export-set.csv`, in file order, with the extension of
/// the upstream path.
fn export_set(corpus: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(corpus.join("export-set.csv")) else {
        return Vec::new();
    };
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields = csv_row(line);
            let blob = fields.first()?.clone();
            let path = fields.get(5).cloned().unwrap_or_default();
            let ext = path.rsplit('.').next().unwrap_or_default().to_lowercase();
            Some((blob, ext))
        })
        .collect()
}

/// The fields of one CSV line (quoted fields included).
fn csv_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(std::mem::take(&mut field));
            }
            _ => field.push(c),
        }
    }
    out.push(field);
    out
}

/// The CSV spelling of one field (quoted when it needs it).
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Reads one document of either format, by the original's extension.
fn import_document(bytes: &[u8], ext: &str) -> Result<Document, String> {
    match ext {
        "vsd" | "vss" | "vst" => import(bytes, &VsdLimits::DEFAULT)
            .map(|imported| imported.document)
            .map_err(|error| format!("{error:?}")),
        "vsdx" | "vssx" | "vstx" => {
            read_document(bytes, &Limits::DEFAULT).map_err(|error| format!("{error:?}"))
        }
        other => Err(format!("unsupported extension {other}")),
    }
}

/// Runs one fallible read under `catch_unwind`: a panic is a bug and
/// reports as `Err("panic")` instead of ending the sweep.
fn catch_read<F>(what: F) -> Result<Document, String>
where
    F: FnOnce() -> Result<Document, String>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(what)) {
        Ok(result) => result,
        Err(_) => Err("panic".to_owned()),
    }
}

/// The scene of every page that draws: every page that is not another
/// page's background page, in page order, with its background page
/// composited behind it (the rule of `corpus_vsd.rs`).
fn drawing_scenes(document: &Document) -> Vec<Scene> {
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
        .filter_map(|(index, _)| {
            build_scene(
                document,
                PageId {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                },
                &options,
            )
            .ok()
        })
        .collect()
}

/// Renders one scene at `width` pixels across, as PNG bytes.
fn render_page(scene: &Scene, width: u32) -> Result<Vec<u8>, String> {
    if !(scene.width.is_finite() && scene.width > 0.0) {
        return Err(format!("the scene has no width ({})", scene.width));
    }
    let scale = f64::from(width) / scene.width;
    render_png(
        scene,
        &PngOptions {
            scale,
            ..PngOptions::new()
        },
    )
    .map_err(|error| format!("render_png: {error}"))
}

/// Rasterises one PDF at [`PICTURE_WIDTH`] pixels across with the
/// system `pdftoppm`, into `out_dir`, and renames the page rasters to
/// `<stem>-p<N>-visio.png`: the page rasters, in page order.
/// `Err("missing")` names the missing tool; any other message is the
/// tool's failure.
fn rasterise_pdf(pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let stem = pdf
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = out_dir.join(format!("{stem}-pdf"));
    let run = std::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-scale-to-x")
        .arg(PICTURE_WIDTH.to_string())
        .arg("-scale-to-y")
        .arg("-1")
        .arg(pdf)
        .arg(&prefix)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "missing".to_owned()
            } else {
                format!("pdftoppm: {error}")
            }
        })?;
    if !run.status.success() {
        return Err(format!(
            "pdftoppm: {}",
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    let mut pages: Vec<(u32, PathBuf)> = Vec::new();
    let head = format!("{stem}-pdf-");
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&head) else {
                continue;
            };
            let Some(number) = rest.strip_suffix(".png") else {
                continue;
            };
            if let Ok(number) = number.parse::<u32>() {
                pages.push((number, entry.path()));
            }
        }
    }
    pages.sort();
    let mut out = Vec::new();
    for (number, path) in pages {
        let renamed = out_dir.join(format!("{stem}-p{number}-visio.png"));
        let _ = std::fs::rename(&path, &renamed);
        out.push(renamed);
    }
    Ok(out)
}

/// The difference score of two page rasters (the ADR 0027 metric as a
/// score): the fraction of pixels whose four channels exceed
/// [`MAX_CHANNEL_DELTA`], over the two rasters' common area. A
/// dimension that differs is reported with the score.
fn difference_score(ours: &Pixmap, theirs: &Pixmap) -> (f64, bool) {
    let (ow, oh) = (ours.width(), ours.height());
    let (tw, th) = (theirs.width(), theirs.height());
    let (w, h) = (ow.min(tw), oh.min(th));
    if w == 0 || h == 0 {
        return (1.0, true);
    }
    let mut above = 0usize;
    let mut total = 0usize;
    for y in 0..h {
        for x in 0..w {
            let (Some(a), Some(b)) = (ours.pixel(x, y), theirs.pixel(x, y)) else {
                continue;
            };
            total += 1;
            if channel_delta(&a, &b) > MAX_CHANNEL_DELTA {
                above += 1;
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        above as f64 / total as f64
    };
    (score, (ow, oh) != (tw, th))
}

/// The largest difference of the three channels of two pixels, over
/// white (a PDF raster is transparent where the page is).
fn channel_delta(a: &PremultipliedColorU8, b: &PremultipliedColorU8) -> u8 {
    let a = over_white(a);
    let b = over_white(b);
    a.0.abs_diff(b.0)
        .max(a.1.abs_diff(b.1))
        .max(a.2.abs_diff(b.2))
}

/// One pixel composited over white, as an RGB triple. The channels are
/// premultiplied, so the transparent part of the pixel is what the
/// white shows through: `channel + (255 - alpha)`.
fn over_white(pixel: &PremultipliedColorU8) -> (u8, u8, u8) {
    let base = 255u16.saturating_sub(u16::from(pixel.alpha()));
    let over = |value: u8| u16::from(value).saturating_add(base).min(255) as u8;
    (over(pixel.red()), over(pixel.green()), over(pixel.blue()))
}

/// One page's result of the picture run: the score, the blob, the page
/// number and the note (why there is no pair, or the size difference).
struct PageScore {
    score: f64,
    blob: String,
    page: usize,
    note: String,
}

/// The wide twin sweep (P8-21 goal 2): one row per converted file in
/// `twin-sweep.csv` and the totals on stdout. Ignored like the corpus
/// sweeps: it needs the local wide data
/// (`LARRYGRAM_WIDE_TWINS`/`LARRYGRAM_WIDE_CORPUS`).
#[test]
#[ignore = "needs LARRYGRAM_WIDE_TWINS and LARRYGRAM_WIDE_CORPUS"]
fn the_wide_twin_sweep_compares_every_converted_file() {
    let (Some(twins), Some(corpus)) = (
        wide_twins_dir(std::env::var_os(TWINS_ENV)),
        wide_corpus_dir(std::env::var_os(CORPUS_ENV)),
    ) else {
        eprintln!(
            "skipping the wide twin sweep: set {TWINS_ENV} (the folder with vsdx/) and \
             {CORPUS_ENV} (the folder with index.csv)"
        );
        return;
    };
    let originals = originals(&corpus);
    let mut converted: Vec<PathBuf> = match std::fs::read_dir(twins.join("vsdx")) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => Vec::new(),
    };
    converted.sort();
    if converted.is_empty() {
        eprintln!(
            "skipping the wide twin sweep: {} holds no converted file",
            twins.join("vsdx").display()
        );
        return;
    }
    // The columns: identity, outcome, the counts and one column per
    // cell family.
    let mut header = vec![
        "blob_id".to_owned(),
        "twin_ext".to_owned(),
        "outcome".to_owned(),
        "error".to_owned(),
        "pages".to_owned(),
        "joined".to_owned(),
        "compared".to_owned(),
        "agreed".to_owned(),
        "mismatches".to_owned(),
    ];
    for family in Family::ALL {
        header.push(format!("mismatch_{}", family.as_str()));
    }
    let mut rows: Vec<String> = Vec::new();
    let mut totals = [0usize; 5]; // compared, agreed, joined, mismatches, files
    let mut panics = 0usize;
    let mut families = [0usize; 8];
    // The causes: `(cause, family)` to `(mismatches, files)`.
    let mut causes: BTreeMap<(String, String), [usize; 2]> = BTreeMap::new();
    for path in &converted {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(original) = originals.get(&name) else {
            eprintln!(
                "sweep {name}: no original under {}/a or /b",
                corpus.display()
            );
            continue;
        };
        let original_ext = original
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned())
            .unwrap_or_default();
        let our_bytes = std::fs::read(original).unwrap_or_default();
        let our = catch_read(|| import_document(&our_bytes, &original_ext));
        let twin_bytes = std::fs::read(path).unwrap_or_default();
        let twin = catch_read(|| import_document(&twin_bytes, "vsdx"));
        let mut counts = [0usize; 8];
        let mut fields = vec![name.clone(), ext.clone()];
        match (our, twin) {
            (Ok(document), Ok(twin_document)) => {
                let result = compare_documents(&document, &twin_document);
                let mut seen: BTreeMap<(String, String), ()> = BTreeMap::new();
                for mismatch in &result.mismatches {
                    let slot = Family::ALL
                        .iter()
                        .position(|family| *family == mismatch.family)
                        .unwrap_or(0);
                    counts[slot] += 1;
                    families[slot] += 1;
                    let key = (
                        mismatch.cause.clone(),
                        Family::ALL[slot].as_str().to_owned(),
                    );
                    causes.entry(key.clone()).or_default()[0] += 1;
                    seen.insert(key, ());
                }
                for key in seen.keys() {
                    causes.entry(key.clone()).or_default()[1] += 1;
                }
                totals[0] += result.compared;
                totals[1] += result.agreed;
                totals[2] += result.joined;
                totals[3] += result.mismatches.len();
                totals[4] += 1;
                println!(
                    "sweep {name}: {} pages, {} shapes joined, {} of {} cells agree, {} differ",
                    document.pages.len(),
                    result.joined,
                    result.agreed,
                    result.compared,
                    result.mismatches.len()
                );
                fields.push("compared".to_owned());
                fields.push(String::new());
                fields.push(document.pages.len().to_string());
                fields.push(result.joined.to_string());
                fields.push(result.compared.to_string());
                fields.push(result.agreed.to_string());
                fields.push(result.mismatches.len().to_string());
                for count in counts {
                    fields.push(count.to_string());
                }
            }
            (our, twin) => {
                let (outcome, why) = match (our, twin) {
                    (Err(why), _) | (_, Err(why)) if why == "panic" => ("panic", why),
                    (Err(why), _) | (_, Err(why)) => ("error", why),
                    _ => ("error", "unreadable".to_owned()),
                };
                panics += usize::from(outcome == "panic");
                fields.push(outcome.to_owned());
                fields.push(csv_field(&why));
                for _ in 0..5 {
                    fields.push(String::new());
                }
                for _ in counts {
                    fields.push(String::new());
                }
            }
        }
        rows.push(fields.join(","));
    }
    let mut text = format!("{}\n", header.join(","));
    for row in &rows {
        text.push_str(row);
        text.push('\n');
    }
    let out = twins.join("twin-sweep.csv");
    std::fs::write(&out, text).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    let mut cause_rows: Vec<(&(String, String), &[usize; 2])> = causes.iter().collect();
    cause_rows.sort_by(|a, b| b.1[0].cmp(&a.1[0]).then_with(|| a.0.cmp(b.0)));
    let mut cause_text = String::from("cause,family,mismatches,files\n");
    for ((cause, family), counts) in &cause_rows {
        cause_text.push_str(&format!(
            "{},{},{},{}\n",
            csv_field(cause),
            family,
            counts[0],
            counts[1]
        ));
    }
    let cause_out = twins.join("twin-sweep-causes.csv");
    std::fs::write(&cause_out, cause_text)
        .unwrap_or_else(|e| panic!("{}: {e}", cause_out.display()));
    println!(
        "wide twin sweep: {} files, {} cells compared, {} agree, {} differ, \
         {} joined shapes, {} panics",
        totals[4], totals[0], totals[1], totals[3], totals[2], panics
    );
    for (family, count) in Family::ALL.iter().map(|f| f.as_str()).zip(families) {
        println!("  mismatch_{family}: {count}");
    }
    println!("most common causes:");
    for ((cause, family), counts) in cause_rows.iter().take(10) {
        println!(
            "  {cause} ({family}): {} mismatches in {} files",
            counts[0], counts[1]
        );
    }
    println!("wrote {} and {}", out.display(), cause_out.display());
    assert!(totals[4] > 0, "no file compared");
}

/// The side-by-side picture page (P8-21 goal 3): every page of every
/// export-set file rendered by us and rasterised from Visio's PDF at
/// the same width, scored with the ADR 0027 metric and listed worst
/// first in `compare.html` (and in `picture-scores.csv`). Ignored like
/// the corpus sweeps: it needs the local wide data.
#[test]
#[ignore = "needs LARRYGRAM_WIDE_TWINS and LARRYGRAM_WIDE_CORPUS"]
fn every_export_page_gets_a_side_by_side_picture() {
    let (Some(twins), Some(corpus)) = (
        wide_twins_dir(std::env::var_os(TWINS_ENV)),
        wide_corpus_dir(std::env::var_os(CORPUS_ENV)),
    ) else {
        eprintln!("skipping the picture page: set {TWINS_ENV} and {CORPUS_ENV}");
        return;
    };
    let originals = originals(&corpus);
    let export = export_set(&corpus);
    if export.is_empty() {
        eprintln!(
            "skipping the picture page: no rows in {}",
            corpus.join("export-set.csv").display()
        );
        return;
    }
    let our_dir = twins.join("our-png");
    let pdf_dir = twins.join("pdf-png");
    for dir in [&our_dir, &pdf_dir] {
        if std::fs::create_dir_all(dir).is_err() {
            eprintln!("skipping the picture page: cannot write {}", dir.display());
            return;
        }
    }
    let mut scores: Vec<PageScore> = Vec::new();
    let mut missing_pdf = 0usize;
    let mut tool_missing = false;
    let mut pages = 0usize;
    for (blob, ext) in &export {
        let Some(original) = originals.get(blob) else {
            eprintln!("picture {blob}: no original under the corpus");
            continue;
        };
        let bytes = std::fs::read(original).unwrap_or_default();
        let document = catch_read(|| import_document(&bytes, ext));
        let document = match document {
            Ok(document) => document,
            Err(why) => {
                scores.push(PageScore {
                    score: 1.0,
                    blob: blob.clone(),
                    page: 0,
                    note: format!("import {why}"),
                });
                continue;
            }
        };
        let scenes = drawing_scenes(&document);
        let pdf = twins.join("pdf").join(format!("{blob}.pdf"));
        let raster = if tool_missing {
            Vec::new()
        } else {
            match rasterise_pdf(&pdf, &pdf_dir) {
                Ok(paths) => paths,
                Err(why) if why == "missing" => {
                    tool_missing = true;
                    Vec::new()
                }
                Err(why) => {
                    eprintln!("picture {blob}: {why}");
                    Vec::new()
                }
            }
        };
        if raster.is_empty() {
            missing_pdf += 1;
        }
        for (index, scene) in scenes.iter().enumerate() {
            pages += 1;
            let number = index + 1;
            let label = format!("{blob}-p{number}");
            let rendered = match render_page(scene, PICTURE_WIDTH) {
                Ok(png) => png,
                Err(why) => {
                    scores.push(PageScore {
                        score: 1.0,
                        blob: blob.clone(),
                        page: number,
                        note: why,
                    });
                    continue;
                }
            };
            let our_png = our_dir.join(format!("{label}.png"));
            if std::fs::write(&our_png, &rendered).is_err() {
                scores.push(PageScore {
                    score: 1.0,
                    blob: blob.clone(),
                    page: number,
                    note: "cannot write our PNG".to_owned(),
                });
                continue;
            }
            let Some(theirs) = raster.get(index) else {
                scores.push(PageScore {
                    score: 1.0,
                    blob: blob.clone(),
                    page: number,
                    note: "no PDF page".to_owned(),
                });
                continue;
            };
            let Ok(our_map) = Pixmap::decode_png(&rendered) else {
                scores.push(PageScore {
                    score: 1.0,
                    blob: blob.clone(),
                    page: number,
                    note: "cannot decode our raster".to_owned(),
                });
                continue;
            };
            let their_bytes = std::fs::read(theirs).unwrap_or_default();
            let Ok(their_map) = Pixmap::decode_png(&their_bytes) else {
                scores.push(PageScore {
                    score: 1.0,
                    blob: blob.clone(),
                    page: number,
                    note: "cannot decode the PDF raster".to_owned(),
                });
                continue;
            };
            let (score, size) = difference_score(&our_map, &their_map);
            let note = if size {
                format!(
                    "size {}x{} vs {}x{}",
                    our_map.width(),
                    our_map.height(),
                    their_map.width(),
                    their_map.height()
                )
            } else {
                String::new()
            };
            scores.push(PageScore {
                score,
                blob: blob.clone(),
                page: number,
                note,
            });
        }
    }
    if tool_missing {
        eprintln!(
            "note: the system pdftoppm is missing; the pictures are ours alone and every \
             score is 1.0"
        );
    }
    scores.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.blob.cmp(&b.blob))
            .then_with(|| a.page.cmp(&b.page))
    });
    let mut text = String::from("blob_id,page,score,note\n");
    for score in &scores {
        text.push_str(&format!(
            "{},{},{:.6},{}\n",
            score.blob,
            score.page,
            score.score,
            csv_field(&score.note)
        ));
    }
    let csv = twins.join("picture-scores.csv");
    std::fs::write(&csv, &text).unwrap_or_else(|e| panic!("{}: {e}", csv.display()));
    let html = twins.join("compare.html");
    std::fs::write(&html, compare_html(&scores, pages))
        .unwrap_or_else(|e| panic!("{}: {e}", html.display()));
    println!(
        "pictures: {pages} pages of {} files, {} without a PDF raster; worst score {:.4}",
        export.len(),
        missing_pdf,
        scores.first().map(|score| score.score).unwrap_or(0.0)
    );
    println!("wrote {} and {}", csv.display(), html.display());
    assert!(pages > 0, "no page rendered");
}

/// The `compare.html` of the picture page: one row per page, worst
/// first, our raster beside Visio's.
fn compare_html(scores: &[PageScore], pages: usize) -> String {
    let mut out = String::new();
    out.push_str(
        "<!doctype html>
<html lang=\"en\"><head><meta charset=\"utf-8\">
<title>Larrygram wide twin pictures</title>
<style>
 body { font-family: sans-serif; margin: 1em; }
 .row { border-top: 1px solid #ccc; padding: 0.5em 0; display: flex; gap: 1em; }
 .meta { width: 16em; font-size: 0.85em; }
 img { width: 320px; border: 1px solid #999; }
 figure { margin: 0; }
 figcaption { font-size: 0.8em; }
</style></head><body>
<h1>Wide twin pictures</h1>
<p>Every page of the export set with our renderer beside Visio's PDF
rasterised at the same width. The difference score is the ADR 0027
metric as a score: the fraction of pixels whose channels differ by more
than 2 of 255 (the images' common area; a size difference is noted).
Worst first.</p>
",
    );
    out.push_str(&format!("<p>{pages} pages.</p>\n"));
    for score in scores {
        let label = format!("{}-p{}", score.blob, score.page);
        out.push_str("<div class=\"row\"><div class=\"meta\">");
        out.push_str(&format!("score <b>{:.4}</b><br>{label}<br>", score.score));
        if !score.note.is_empty() {
            out.push_str(&score.note);
            out.push_str("<br>");
        }
        out.push_str(&format!("blob {}</div>", score.blob));
        out.push_str("<figure><img src=\"our-png/");
        out.push_str(&label);
        out.push_str(".png\" alt=\"our rendering\"><figcaption>ours</figcaption></figure>");
        out.push_str("<figure><img src=\"pdf-png/");
        out.push_str(&label);
        out.push_str("-visio.png\" alt=\"Visio's PDF raster\">");
        out.push_str("<figcaption>Visio</figcaption></figure></div>\n");
    }
    out.push_str("</body></html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_values_need_their_marker_files() {
        assert_eq!(wide_twins_dir(None), None);
        assert_eq!(wide_corpus_dir(None), None);
        assert_eq!(
            wide_twins_dir(Some("no/such/directory".into())),
            None,
            "no vsdx/ folder, no sweep"
        );
        assert_eq!(
            wide_corpus_dir(Some("no/such/directory".into())),
            None,
            "no index.csv, no sweep"
        );
    }

    #[test]
    fn the_csv_fields_quote_what_needs_it() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_row("a,\"b,c\""), ["a", "b,c"]);
    }

    #[test]
    fn the_difference_score_counts_the_pixels_over_the_delta() {
        let mut ours = Pixmap::new(2, 1).expect("a raster");
        for pixel in ours.pixels_mut() {
            *pixel = PremultipliedColorU8::from_rgba(0, 0, 0, 255).expect("opaque black");
        }
        let mut same = Pixmap::new(2, 1).expect("a raster");
        same.pixels_mut().copy_from_slice(ours.pixels());
        assert_eq!(difference_score(&ours, &same), (0.0, false));
        let mut other = Pixmap::new(2, 1).expect("a raster");
        for pixel in other.pixels_mut() {
            *pixel = PremultipliedColorU8::from_rgba(255, 255, 255, 255).expect("opaque white");
        }
        assert_eq!(difference_score(&ours, &other), (1.0, false));
        let bigger = Pixmap::new(3, 1).expect("a raster");
        assert_eq!(difference_score(&ours, &bigger), (1.0, true));
    }

    #[test]
    fn a_transparent_pixel_composites_over_white() {
        let clear = PremultipliedColorU8::from_rgba(0, 0, 0, 0).expect("transparent");
        assert_eq!(over_white(&clear), (255, 255, 255));
        let half = PremultipliedColorU8::from_rgba(100, 100, 100, 128).expect("a pixel");
        let (r, g, b) = over_white(&half);
        assert!(r > 100 && r < 255, "{r}");
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    #[test]
    fn the_html_page_lists_the_scores_worst_first() {
        let scores = vec![
            PageScore {
                score: 0.5,
                blob: "aa".to_owned(),
                page: 1,
                note: String::new(),
            },
            PageScore {
                score: 0.1,
                blob: "bb".to_owned(),
                page: 2,
                note: "size 2x2 vs 3x3".to_owned(),
            },
        ];
        let html = compare_html(&scores, 2);
        assert!(html.contains("score <b>0.5000</b><br>aa-p1"));
        assert!(html.contains("our-png/aa-p1.png"));
        assert!(html.contains("pdf-png/bb-p2-visio.png"));
        assert!(html.find("0.5000") < html.find("0.1000"), "worst first");
    }
}
