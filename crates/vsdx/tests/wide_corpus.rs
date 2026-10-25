//! The wide-corpus sweep through the `.vsdx` reader (task P8-16).
//!
//! One ignored test runs [`read_document`] over every non-excluded
//! file of `index.csv` (the index `cargo xtask scout classify` wrote
//! into `reference/wide-corpus/`, found through
//! `LARRYGRAM_WIDE_CORPUS`) and records, per file, whether it opened,
//! its error kind and whether it panicked (a panic is a bug). The
//! `.vsdx` reader has no warning stream yet, so its warning count is
//! always zero; the sweep still records the column.
//!
//! The reader runs over every non-excluded file, including the binary
//! and `.vdx` ones: a foreign file must fail cleanly, never crash.
//! Files the Microsoft-content filter excluded (AGENTS.md section 4)
//! are never read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use larrygram_model::{Cell, DocumentKind};
use larrygram_vsdx::{Error, Limits, read_document};

/// The environment variable that points at the wide corpus.
const ENV: &str = "LARRYGRAM_WIDE_CORPUS";

/// How many files are processed at once.
const WORKERS: usize = 8;

/// How a run ended.
#[derive(Debug, PartialEq, Eq)]
enum Ran {
    /// No wide corpus: the run skips cleanly.
    Skipped,
    /// The run handled this many files.
    Files(usize),
}

/// What the sweep recorded for one file.
#[derive(Debug, Default, PartialEq, Eq)]
struct SweepRow {
    /// The git blob SHA-1.
    blob_id: String,
    /// The tier directory (`a` or `b`).
    tier: String,
    /// The file extension.
    ext: String,
    /// The document kind, when the file opened.
    kind: String,
    /// `opened`, `error` or `panic`.
    outcome: String,
    /// The [`Error`] variant name, when the file did not open.
    error_kind: String,
    /// 1 when the read panicked (a bug).
    panic: String,
    /// Page count.
    pages: usize,
    /// Master count.
    masters: usize,
    /// Shape count (the whole arena).
    shapes: usize,
    /// Group shapes (shapes with children).
    groups: usize,
    /// Connect records across all pages.
    connects: usize,
    /// 1-D shapes (shapes with a `BeginX` cell).
    one_d: usize,
    /// Total text length.
    text_chars: usize,
    /// The page's `DrawingScale` cell, when set.
    drawing_scale: String,
    /// Theme parts.
    themes: usize,
    /// Document properties.
    properties: usize,
}

/// The wide corpus directory named by the environment, or `None`.
fn wide_corpus_dir() -> Option<PathBuf> {
    let value = std::env::var_os(ENV)?;
    Some(resolve_dir(&value.to_string_lossy()))
}

/// Resolves the environment value: the directory itself, or the
/// `index.csv` inside it.
fn resolve_dir(value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.file_name().is_some_and(|name| name == "index.csv") {
        path.parent().map(Path::to_path_buf).unwrap_or(path)
    } else {
        path
    }
}

/// One `index.csv` row: the fields the sweep needs.
#[derive(Debug, PartialEq, Eq)]
struct IndexRow {
    /// The git blob SHA-1.
    blob_id: String,
    /// The tier directory.
    tier: String,
    /// The file extension.
    ext: String,
    /// Whether the Microsoft-content filter excluded the file.
    excluded: bool,
}

/// Parses `index.csv` (written by `cargo xtask scout classify`).
fn index_rows(text: &str) -> Vec<IndexRow> {
    let mut out = Vec::new();
    for (i, row) in csv_rows(text).into_iter().enumerate() {
        if i == 0 && row.first().map(String::as_str) == Some("blob_id") {
            continue;
        }
        let get = |j: usize| -> String { row.get(j).cloned().unwrap_or_default() };
        out.push(IndexRow {
            blob_id: get(0),
            tier: get(1),
            ext: get(6),
            excluded: get(16) == "1",
        });
    }
    out
}

/// The path of an index row's file inside the corpus directory.
fn file_path(dir: &Path, row: &IndexRow) -> PathBuf {
    dir.join(&row.tier)
        .join(format!("{}.{}", row.blob_id, row.ext))
}

/// The [`Error`] variant name of a failed read.
fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::LimitExceeded { .. } => "LimitExceeded",
        Error::MissingPart(_) => "MissingPart",
        Error::BadRelationship(_) => "BadRelationship",
        Error::InvalidPackage(_) => "InvalidPackage",
        Error::Model(_) => "Model",
        Error::NotImplemented => "NotImplemented",
        _ => "Other",
    }
}

/// Runs `read_document` over one file under `catch_unwind`: every
/// panic is a bug, and the sweep records it instead of dying.
fn sweep_one(bytes: &[u8], row: &IndexRow) -> SweepRow {
    let mut out = SweepRow {
        blob_id: row.blob_id.clone(),
        tier: row.tier.clone(),
        ext: row.ext.clone(),
        ..SweepRow::default()
    };
    match std::panic::catch_unwind(|| read_document(bytes, &Limits::DEFAULT)) {
        Ok(Ok(document)) => {
            out.outcome = "opened".to_owned();
            out.kind = match document.kind {
                DocumentKind::Drawing => "drawing",
                DocumentKind::Stencil => "stencil",
                DocumentKind::Template => "template",
                _ => "unknown",
            }
            .to_owned();
            out.pages = document.pages.len();
            out.masters = document.masters.len();
            out.shapes = document.shapes.len();
            out.themes = document.themes.len();
            out.properties = document.properties.len();
            for shape in &document.shapes {
                if !shape.children.is_empty() {
                    out.groups += 1;
                }
                if cell(&shape.sheet.cells, "BeginX").is_some() {
                    out.one_d += 1;
                }
                if let Some(text) = &shape.text {
                    for part in &text.parts {
                        if let larrygram_model::TextPart::Text(literal) = part {
                            out.text_chars += literal.chars().count();
                        }
                    }
                }
            }
            for page in &document.pages {
                out.connects += page.connects.len();
                if out.drawing_scale.is_empty()
                    && let Some(scale) = cell(&page.sheet.cells, "DrawingScale")
                {
                    out.drawing_scale = scale.to_string();
                }
            }
        }
        Ok(Err(error)) => {
            out.outcome = "error".to_owned();
            out.error_kind = error_kind(&error).to_owned();
        }
        Err(_) => {
            out.outcome = "panic".to_owned();
            out.panic = "1".to_owned();
        }
    }
    out
}

/// The numeric value of one cell of a sheet, when it has one.
fn cell(cells: &[Cell], name: &str) -> Option<f64> {
    cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.value.as_ref())
        .and_then(|value| match value {
            larrygram_model::CellValue::Number(value) => Some(*value),
            _ => None,
        })
}

/// The `sweep-vsdx.csv` header (the same scalar columns as
/// `sweep-vsd.csv`, plus `themes` and `properties`; the warning
/// columns stay zero until the reader has a warning stream).
fn sweep_header() -> String {
    "blob_id,tier,ext,kind,version,outcome,error_kind,panic,warnings_total,\
     warning_skipped_record,warning_dropped,warning_unresolved_reference,warning_bad_value,\
     warning_unsupported,pages,masters,shapes,groups,connects,one_d,text_chars,drawing_scale,\
     themes,properties"
        .to_owned()
}

/// One `sweep-vsdx.csv` line.
fn sweep_line(row: &SweepRow) -> String {
    let fields = vec![
        row.blob_id.clone(),
        row.tier.clone(),
        row.ext.clone(),
        row.kind.clone(),
        String::new(),
        row.outcome.clone(),
        row.error_kind.clone(),
        row.panic.clone(),
        "0".to_owned(),
        "0".to_owned(),
        "0".to_owned(),
        "0".to_owned(),
        "0".to_owned(),
        "0".to_owned(),
        row.pages.to_string(),
        row.masters.to_string(),
        row.shapes.to_string(),
        row.groups.to_string(),
        row.connects.to_string(),
        row.one_d.to_string(),
        row.text_chars.to_string(),
        row.drawing_scale.clone(),
        row.themes.to_string(),
        row.properties.to_string(),
    ];
    csv_line(&fields)
}

/// Runs the sweep over every non-excluded file of `index.csv`, writing
/// `sweep-vsdx.csv`.
fn run_sweep(dir: &Path) -> Ran {
    let index = match fs::read_to_string(dir.join("index.csv")) {
        Ok(text) => index_rows(&text),
        Err(_) => return Ran::Skipped,
    };
    let rows: Vec<IndexRow> = index.into_iter().filter(|row| !row.excluded).collect();
    if rows.is_empty() {
        return Ran::Skipped;
    }
    let lines = Mutex::new(BTreeMap::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(row) = rows.get(i) else {
                        break;
                    };
                    let bytes = match fs::read(file_path(dir, row)) {
                        Ok(bytes) => bytes,
                        Err(_) => continue,
                    };
                    let sweep = sweep_one(&bytes, row);
                    lines
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.blob_id.clone(), sweep_line(&sweep));
                }
            });
        }
    });
    let lines = lines.into_inner().unwrap_or_default();
    let mut text = format!("{}\n", sweep_header());
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    let _ = fs::write(dir.join("sweep-vsdx.csv"), text);
    Ran::Files(lines.len())
}

/// Quotes one CSV field when it needs it (the same rule as
/// `cargo xtask scout`).
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Formats one CSV row.
fn csv_line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",")
}

/// Parses RFC 4180 CSV into rows of fields.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter()
        .filter(|r| !(r.len() == 1 && r.first().is_some_and(String::is_empty)))
        .collect()
}

/// The `.vsdx`-reader sweep over the wide corpus (task P8-16 goal 5).
#[test]
#[ignore = "runs the wide-corpus sweep; set LARRYGRAM_WIDE_CORPUS"]
fn wide_corpus_read_sweep() {
    let Some(dir) = wide_corpus_dir() else {
        eprintln!("skipping: {ENV} is not set");
        return;
    };
    match run_sweep(&dir) {
        Ran::Skipped => eprintln!("skipping: no non-excluded rows in {}", dir.display()),
        Ran::Files(n) => println!("swept {n} files into sweep-vsdx.csv"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temp directory for one test (removed first).
    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("larrygram-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The fixtures directory of the repository (committed, our own
    /// work).
    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("corpus")
            .join("fixtures")
    }

    #[test]
    fn resolves_the_environment_value() {
        assert_eq!(resolve_dir("/w"), PathBuf::from("/w"));
        assert_eq!(resolve_dir("/w/index.csv"), PathBuf::from("/w"));
    }

    #[test]
    fn parses_the_index() {
        let text = format!(
            "{}\n{},a,mit,o/r,c,p/x.vsdx,vsdx,s,1,ooxml-drawing,,yes,0,0,0,0,0,\n\
             {},b,mit,o/r,c,p/y.vsdx,vsdx,s,1,ooxml-drawing,,yes,0,0,0,0,1,microsoft-author\n",
            "blob_id,tier,license,repo,commit,path,ext,sha256,size,kind,version,kind_matches_ext,ms_author,ms_company,template_copy,template_origin,excluded,exclude_reason",
            "a".repeat(40),
            "b".repeat(40)
        );
        let rows = index_rows(&text);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].excluded);
        assert!(rows[1].excluded);
        assert_eq!(
            file_path(Path::new("/w"), &rows[0]),
            PathBuf::from(format!("/w/a/{}.vsdx", "a".repeat(40)))
        );
    }

    #[test]
    fn csv_fields_round_trip() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_line(&["a".to_owned(), "b,c".to_owned()]), "a,\"b,c\"");
        assert_eq!(
            csv_rows("\"a,b\",c\n"),
            vec![vec!["a,b".to_owned(), "c".to_owned()]]
        );
    }

    #[test]
    fn sweeps_the_committed_fixture() {
        let bytes = fs::read(fixtures_dir().join("minimal-drawing.vsdx")).unwrap();
        let row = IndexRow {
            blob_id: "blob".to_owned(),
            tier: "a".to_owned(),
            ext: "vsdx".to_owned(),
            excluded: false,
        };
        let sweep = sweep_one(&bytes, &row);
        assert_eq!(sweep.outcome, "opened");
        assert_eq!(sweep.kind, "drawing");
        assert_eq!(sweep.panic, "");
        assert!(sweep.pages >= 1);
        let line = sweep_line(&sweep);
        assert!(line.starts_with("blob,a,vsdx,drawing,,opened,,"), "{line}");
    }

    #[test]
    fn sweeps_a_file_that_does_not_open() {
        let row = IndexRow {
            blob_id: "blob".to_owned(),
            tier: "a".to_owned(),
            ext: "vsdx".to_owned(),
            excluded: false,
        };
        let sweep = sweep_one(b"garbage", &row);
        assert_eq!(sweep.outcome, "error");
        assert_eq!(sweep.error_kind, "InvalidPackage");
        let sweep = sweep_one(b"garbage", &row);
        assert_eq!(
            sweep.error_kind,
            error_kind(&Error::InvalidPackage("x".to_owned()))
        );
    }

    #[test]
    fn names_error_kinds() {
        assert_eq!(
            error_kind(&Error::MissingPart("x".to_owned())),
            "MissingPart"
        );
        assert_eq!(
            error_kind(&Error::BadRelationship("x".to_owned())),
            "BadRelationship"
        );
        assert_eq!(error_kind(&Error::NotImplemented), "NotImplemented");
    }

    #[test]
    fn the_run_skips_without_an_index() {
        let dir = temp_dir("wide-corpus-vsdx-skip");
        assert_eq!(run_sweep(&dir.join("absent")), Ran::Skipped);
    }

    /// Builds a tiny corpus: one real fixture and one garbage file,
    /// with an index that excludes the garbage one.
    #[test]
    fn the_run_writes_its_csv_file() {
        let dir = temp_dir("wide-corpus-vsdx-run");
        fs::create_dir_all(dir.join("a")).unwrap();
        let fixture = fs::read(fixtures_dir().join("minimal-stencil.vssx")).unwrap();
        let good = "1".repeat(40);
        let bad = "2".repeat(40);
        fs::write(dir.join(format!("a/{good}.vssx")), &fixture).unwrap();
        fs::write(dir.join(format!("a/{bad}.vsdx")), b"garbage").unwrap();

        let index = format!(
            "{}\n{good},a,mit,o/r,c,p/g.vssx,vssx,s,1,ooxml-stencil,,yes,0,0,0,0,0,\n\
             {bad},b,mit,o/r,c,p/b.vsdx,vsdx,s,1,zip-other,,unknown,0,0,0,0,1,template-copy\n",
            "blob_id,tier,license,repo,commit,path,ext,sha256,size,kind,version,kind_matches_ext,ms_author,ms_company,template_copy,template_origin,excluded,exclude_reason"
        );
        fs::write(dir.join("index.csv"), index).unwrap();
        assert_eq!(run_sweep(&dir), Ran::Files(1));
        let sweep = fs::read_to_string(dir.join("sweep-vsdx.csv")).unwrap();
        assert!(sweep.starts_with(&sweep_header()));
        assert!(sweep.contains("stencil"), "{sweep}");
        assert!(!sweep.contains(&bad), "an excluded file is never swept");
    }
}
