// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The wide-corpus sweep over binary Visio files (task P8-16).
//!
//! Two ignored tests drive the wide corpus that `cargo xtask scout`
//! fetched into `reference/wide-corpus/` (never committed, ADR 0005):
//!
//! - [`wide_corpus_classify_ole`] writes `ole-facts.csv`: for every
//!   `.vsd`, `.vss` and `.vst` file, the real kind, the version byte
//!   and the Microsoft-content filter (AGENTS.md section 4), read
//!   through this crate's public API.
//! - [`wide_corpus_import_sweep`] runs [`import`] over every
//!   non-excluded file in `index.csv` and records, per file, whether it
//!   opened, its error kind, its warning counts by kind and whether it
//!   panicked (a panic is a bug).
//!
//! The index is found through `LARRYGRAM_WIDE_CORPUS` (the directory,
//! or the `index.csv` inside it); the tests skip cleanly when it is
//! unset. Excluded files are never imported: the filter reads their
//! document properties first and stops there.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use larrygram_vsd_binary::model::{Cell, DocumentKind};
use larrygram_vsd_binary::{
    Error, FormatVersion, ImportWarning, Limits, SummaryProperty, SummaryValue, WarningKind,
    import, read_streams, read_summary_properties, version,
};

/// The environment variable that points at the wide corpus.
const ENV: &str = "LARRYGRAM_WIDE_CORPUS";

/// How many files are processed at once.
const WORKERS: usize = 8;

/// The classification facts of one OLE file (`ole-facts.csv` rows).
#[derive(Debug, Default, PartialEq, Eq)]
struct OleFacts {
    /// The git blob SHA-1 (the file name stem).
    blob_id: String,
    /// `ole-drawing`, `ole-stencil`, `ole-template`, `ole-unknown` or
    /// `ole-invalid`.
    kind: String,
    /// The version byte as decimal, empty when unreadable.
    version: String,
    /// A document property names Microsoft as the author.
    ms_author: bool,
    /// A document property names Microsoft as the company.
    ms_company: bool,
    /// The file is a copy of a Visio stock template or stencil.
    template_copy: bool,
    /// The file was made from a Visio stock template.
    template_origin: bool,
}

/// What one sweep recorded for one file.
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
    /// The version byte as decimal, when known.
    version: String,
    /// `opened`, `error` or `panic`.
    outcome: String,
    /// The [`Error`] variant name, when the file did not open.
    error_kind: String,
    /// 1 when the import panicked (a bug).
    panic: String,
    /// Total warnings.
    warnings_total: usize,
    /// Warnings per kind: `skipped_record`, `dropped`,
    /// `unresolved_reference`, `bad_value`, `unsupported`.
    warnings: [usize; 5],
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
}

/// How a run ended.
#[derive(Debug, PartialEq, Eq)]
enum Ran {
    /// No wide corpus: the run skips cleanly.
    Skipped,
    /// The run handled this many files.
    Files(usize),
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

/// Whether a property value names Microsoft (the same rule as
/// `cargo xtask scout`).
fn contains_microsoft(value: &str) -> bool {
    value.to_ascii_lowercase().contains("microsoft")
}

/// Whether `name` is a Visio stock template or stencil identifier (the
/// same rule as `cargo xtask scout`).
fn is_stock_template_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .rsplit_once('.')
        .map(|(stem, _ext)| stem)
        .unwrap_or(lower.as_str());
    const KNOWN: [&str; 3] = ["basicd_m", "basflo_m", "xfunc_m"];
    if KNOWN.contains(&stem) {
        return true;
    }
    let Some((code, suffix)) = stem.rsplit_once('_') else {
        return false;
    };
    suffix == "m"
        && (5..=8).contains(&code.len())
        && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The text value of a summary property, when it has one.
fn summary_text<'a>(props: &'a [SummaryProperty], name: &str) -> Option<&'a str> {
    props.iter().find(|p| p.name == name).and_then(|p| {
        if let SummaryValue::Text(text) = &p.value {
            Some(text.as_str())
        } else {
            None
        }
    })
}

/// The Microsoft-content filter over the summary properties: author,
/// company, template copy and template origin (goal 4).
fn filter_flags(props: &[SummaryProperty], file_name: &str) -> (bool, bool, bool, bool) {
    let ms_author = summary_text(props, "creator").is_some_and(contains_microsoft);
    let ms_company = summary_text(props, "Company").is_some_and(contains_microsoft);
    let template_copy = summary_text(props, "title").is_some_and(is_stock_template_name)
        || is_stock_template_name(file_name);
    let template_origin = summary_text(props, "Template").is_some_and(is_stock_template_name);
    (ms_author, ms_company, template_copy, template_origin)
}

/// Whether the filter excludes the file, and under which reason.
fn excluded_reason(flags: (bool, bool, bool, bool)) -> Option<&'static str> {
    match flags {
        (true, _, _, _) => Some("microsoft-author"),
        (_, true, _, _) => Some("microsoft-company"),
        (_, _, true, _) => Some("template-copy"),
        (_, _, _, true) => Some("template-origin"),
        _ => None,
    }
}

/// The one line of `ole-facts.csv`.
fn ole_line(facts: &OleFacts) -> String {
    csv_line(&[
        facts.blob_id.clone(),
        facts.kind.clone(),
        facts.version.clone(),
        flag(facts.ms_author),
        flag(facts.ms_company),
        flag(facts.template_copy),
        flag(facts.template_origin),
    ])
}

/// The header of `ole-facts.csv` (the same one `cargo xtask scout` writes).
fn ole_header() -> String {
    "blob_id,kind,version,ms_author,ms_company,template_copy,template_origin".to_owned()
}

fn flag(value: bool) -> String {
    if value {
        "1".to_owned()
    } else {
        "0".to_owned()
    }
}

/// Classifies one OLE file: the container, the version byte, the
/// filter, and (only when the filter does not exclude the file) the
/// document kind from [`import`].
///
/// An excluded file is never imported: the properties are the filter,
/// and nothing more reads the file.
fn classify_ole(bytes: &[u8], blob_id: &str, file_name: &str) -> OleFacts {
    let mut facts = OleFacts {
        blob_id: blob_id.to_owned(),
        kind: "ole-invalid".to_owned(),
        ..OleFacts::default()
    };
    let streams = match read_streams(bytes, &Limits::DEFAULT) {
        Ok(streams) => streams,
        Err(_) => return facts,
    };
    let Some(visio) = streams.visio_document() else {
        facts.kind = "ole-no-visio".to_owned();
        return facts;
    };
    facts.version = match version(visio) {
        Ok(FormatVersion::V6) => "6".to_owned(),
        Ok(FormatVersion::V11) => "11".to_owned(),
        Err(Error::UnsupportedVersion(byte)) => byte.to_string(),
        Err(_) => String::new(),
    };
    let mut props: Vec<SummaryProperty> = Vec::new();
    for stream in [
        streams.summary_information(),
        streams.document_summary_information(),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(mut more) = read_summary_properties(stream) {
            props.append(&mut more);
        }
    }
    let flags = filter_flags(&props, file_name);
    facts.ms_author = flags.0;
    facts.ms_company = flags.1;
    facts.template_copy = flags.2;
    facts.template_origin = flags.3;
    facts.kind = "ole-unknown".to_owned();
    if excluded_reason(flags).is_some() {
        return facts;
    }
    match std::panic::catch_unwind(|| import(bytes, &Limits::DEFAULT)) {
        Ok(Ok(imported)) => {
            facts.kind = match imported.document.kind {
                DocumentKind::Drawing => "ole-drawing",
                DocumentKind::Stencil => "ole-stencil",
                DocumentKind::Template => "ole-template",
                _ => "ole-unknown",
            }
            .to_owned();
        }
        Ok(Err(_)) => facts.kind = "ole-unreadable".to_owned(),
        Err(_) => facts.kind = "ole-panic".to_owned(),
    }
    facts
}

/// The [`Error`] variant name of a failed import.
fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::UnsupportedVersion(_) => "UnsupportedVersion",
        Error::NotAVisioFile(_) => "NotAVisioFile",
        Error::Invalid(_) => "Invalid",
        Error::LimitExceeded { .. } => "LimitExceeded",
        Error::Model(_) => "Model",
        Error::NotImplemented => "NotImplemented",
        _ => "Other",
    }
}

/// The warning kind of one warning: the variant name and its `what`.
fn warning_kind(warning: &ImportWarning) -> (&'static str, &'static str) {
    match &warning.kind {
        WarningKind::SkippedRecord { name } => ("skipped_record", name),
        WarningKind::Dropped { what } => ("dropped", what),
        WarningKind::UnresolvedReference { what } => ("unresolved_reference", what),
        WarningKind::BadValue { what } => ("bad_value", what),
        WarningKind::Unsupported { what } => ("unsupported", what),
        _ => ("other", ""),
    }
}

/// The index of a warning variant in [`SweepRow::warnings`].
fn warning_slot(kind: &str) -> usize {
    match kind {
        "skipped_record" => 0,
        "dropped" => 1,
        "unresolved_reference" => 2,
        "bad_value" => 3,
        "unsupported" => 4,
        _ => 5,
    }
}

/// Runs `import` over one file under `catch_unwind`: every panic is a
/// bug, and the sweep records it instead of dying.
fn sweep_one(bytes: &[u8], row: &IndexRow, version_hint: &str) -> (SweepRow, Vec<String>) {
    let mut out = SweepRow {
        blob_id: row.blob_id.clone(),
        tier: row.tier.clone(),
        ext: row.ext.clone(),
        version: version_hint.to_owned(),
        ..SweepRow::default()
    };
    let mut kinds: Vec<String> = Vec::new();
    match std::panic::catch_unwind(|| import(bytes, &Limits::DEFAULT)) {
        Ok(Ok(imported)) => {
            out.outcome = "opened".to_owned();
            let document = &imported.document;
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
            for shape in &document.shapes {
                if !shape.children.is_empty() {
                    out.groups += 1;
                }
                if cell(&shape.sheet.cells, "BeginX").is_some() {
                    out.one_d += 1;
                }
                if let Some(text) = &shape.text {
                    for part in &text.parts {
                        if let larrygram_vsd_binary::model::TextPart::Text(literal) = part {
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
            for warning in &imported.warnings {
                let (kind, what) = warning_kind(warning);
                out.warnings_total += 1;
                let slot = warning_slot(kind);
                if let Some(count) = out.warnings.get_mut(slot) {
                    *count += 1;
                }
                kinds.push(format!(
                    "{},{},{}",
                    row.blob_id,
                    csv_field(kind),
                    csv_field(what)
                ));
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
    (out, kinds)
}

/// The numeric value of one cell of a sheet, when it has one.
fn cell(cells: &[Cell], name: &str) -> Option<f64> {
    cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.value.as_ref())
        .and_then(|value| match value {
            larrygram_vsd_binary::model::CellValue::Number(value) => Some(*value),
            _ => None,
        })
}

/// The `sweep-vsd.csv` header.
fn sweep_header() -> String {
    "blob_id,tier,ext,kind,version,outcome,error_kind,panic,warnings_total,\
     warning_skipped_record,warning_dropped,warning_unresolved_reference,warning_bad_value,\
     warning_unsupported,pages,masters,shapes,groups,connects,one_d,text_chars,drawing_scale"
        .to_owned()
}

/// One `sweep-vsd.csv` line.
fn sweep_line(row: &SweepRow) -> String {
    let mut fields = vec![
        row.blob_id.clone(),
        row.tier.clone(),
        row.ext.clone(),
        row.kind.clone(),
        row.version.clone(),
        row.outcome.clone(),
        row.error_kind.clone(),
        row.panic.clone(),
        row.warnings_total.to_string(),
    ];
    for count in &row.warnings {
        fields.push(count.to_string());
    }
    for count in [
        row.pages,
        row.masters,
        row.shapes,
        row.groups,
        row.connects,
        row.one_d,
        row.text_chars,
    ] {
        fields.push(count.to_string());
    }
    fields.push(row.drawing_scale.clone());
    csv_line(&fields)
}

/// Classifies every OLE file of the corpus into `ole-facts.csv`.
fn run_classify(dir: &Path) -> Ran {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for tier in ["a", "b"] {
        let entries = match fs::read_dir(dir.join(tier)) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !matches!(ext.as_str(), "vsd" | "vss" | "vst") {
                continue;
            }
            let blob_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((blob_id, path));
        }
    }
    if files.is_empty() {
        return Ran::Skipped;
    }
    files.sort();
    let rows = Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some((blob_id, path)) = files.get(i) else {
                        break;
                    };
                    let Ok(bytes) = fs::read(path) else {
                        continue;
                    };
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let facts = classify_ole(&bytes, blob_id, &name);
                    rows.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(ole_line(&facts));
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap_or_default();
    rows.sort();
    let mut text = format!("{}\n", ole_header());
    for row in rows {
        text.push_str(&row);
        text.push('\n');
    }
    let _ = fs::write(dir.join("ole-facts.csv"), text);
    Ran::Files(files.len())
}

/// Runs the import sweep over every non-excluded file of `index.csv`,
/// writing `sweep-vsd.csv` and `warning-kinds-vsd.csv`.
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
    let kinds = Mutex::new(Vec::new());
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
                    let (sweep, details) = sweep_one(&bytes, row, "");
                    lines
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.blob_id.clone(), sweep_line(&sweep));
                    kinds
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend(details);
                }
            });
        }
    });
    let lines = lines.into_inner().unwrap_or_default();
    let mut kinds = kinds.into_inner().unwrap_or_default();
    kinds.sort();
    let mut text = format!("{}\n", sweep_header());
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    let _ = fs::write(dir.join("sweep-vsd.csv"), text);
    let mut kind_text = "blob_id,kind,what\n".to_owned();
    for kind in kinds {
        kind_text.push_str(&kind);
        kind_text.push('\n');
    }
    let _ = fs::write(dir.join("warning-kinds-vsd.csv"), kind_text);
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

/// The classification sweep: writes `ole-facts.csv` next to the
/// corpus (task P8-16 goal 3).
#[test]
#[ignore = "runs the wide-corpus sweep; set LARRYGRAM_WIDE_CORPUS"]
fn wide_corpus_classify_ole() {
    let Some(dir) = wide_corpus_dir() else {
        eprintln!("skipping: {ENV} is not set");
        return;
    };
    match run_classify(&dir) {
        Ran::Skipped => eprintln!("skipping: no OLE files under {}", dir.display()),
        Ran::Files(n) => println!("classified {n} OLE files into ole-facts.csv"),
    }
}

/// The import sweep: `import` over every non-excluded file of the
/// index (task P8-16 goal 5).
#[test]
#[ignore = "runs the wide-corpus sweep; set LARRYGRAM_WIDE_CORPUS"]
fn wide_corpus_import_sweep() {
    let Some(dir) = wide_corpus_dir() else {
        eprintln!("skipping: {ENV} is not set");
        return;
    };
    match run_sweep(&dir) {
        Ran::Skipped => eprintln!("skipping: no non-excluded rows in {}", dir.display()),
        Ran::Files(n) => println!("swept {n} files into sweep-vsd.csv"),
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
    /// work: P8-10).
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
            "{}\n{},a,mit,o/r,c,p/x.vsd,vsd,s,1,ole-drawing,11,yes,0,0,0,0,0,\n\
             {},b,mit,o/r,c,p/y.vsdx,vsdx,s,1,ooxml-drawing,,yes,0,0,0,0,1,template-copy\n",
            "blob_id,tier,license,repo,commit,path,ext,sha256,size,kind,version,kind_matches_ext,ms_author,ms_company,template_copy,template_origin,excluded,exclude_reason",
            "a".repeat(40),
            "b".repeat(40)
        );
        let rows = index_rows(&text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ext, "vsd");
        assert!(!rows[0].excluded);
        assert!(rows[1].excluded);
        assert_eq!(
            file_path(Path::new("/w"), &rows[0]),
            PathBuf::from(format!("/w/a/{}.vsd", "a".repeat(40)))
        );
    }

    #[test]
    fn csv_fields_round_trip() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_line(&["a".to_owned(), "b,c".to_owned()]), "a,\"b,c\"");
        assert_eq!(
            csv_rows("\"a,b\",c\n"),
            vec![vec!["a,b".to_owned(), "c".to_owned()]]
        );
    }

    #[test]
    fn the_filter_matches_the_task_file_rules() {
        assert!(contains_microsoft("Microsoft Corporation"));
        assert!(!contains_microsoft("Contoso"));
        assert!(is_stock_template_name("BASICD_M.VSTX"));
        assert!(!is_stock_template_name("my-drawing.vsdx"));

        let props = vec![
            SummaryProperty {
                name: "creator".to_owned(),
                value: SummaryValue::Text("Microsoft Corporation".to_owned()),
            },
            SummaryProperty {
                name: "Company".to_owned(),
                value: SummaryValue::Text("Contoso".to_owned()),
            },
            SummaryProperty {
                name: "Template".to_owned(),
                value: SummaryValue::Text("BASICD_M.VSTX".to_owned()),
            },
            SummaryProperty {
                name: "title".to_owned(),
                value: SummaryValue::Integer(7),
            },
        ];
        assert_eq!(
            summary_text(&props, "creator"),
            Some("Microsoft Corporation")
        );
        assert_eq!(summary_text(&props, "title"), None);
        let flags = filter_flags(&props, "chart.vsd");
        assert_eq!(flags, (true, false, false, true));
        assert_eq!(excluded_reason(flags), Some("microsoft-author"));
        assert_eq!(
            excluded_reason((false, true, false, false)),
            Some("microsoft-company")
        );
        assert_eq!(
            excluded_reason((false, false, true, false)),
            Some("template-copy")
        );
        assert_eq!(
            excluded_reason((false, false, false, true)),
            Some("template-origin")
        );
        assert_eq!(excluded_reason((false, false, false, false)), None);
    }

    #[test]
    fn classifies_the_committed_fixture() {
        let bytes = fs::read(fixtures_dir().join("minimal-drawing.vsd")).unwrap();
        let facts = classify_ole(&bytes, "blob", "minimal-drawing.vsd");
        assert_eq!(facts.kind, "ole-drawing");
        assert_eq!(facts.version, "11");
        assert!(!facts.ms_author);
    }

    #[test]
    fn classifies_garbage_as_invalid() {
        let facts = classify_ole(b"not a compound file", "blob", "x.vsd");
        assert_eq!(facts.kind, "ole-invalid");
    }

    #[test]
    fn sweeps_the_committed_fixture() {
        let bytes = fs::read(fixtures_dir().join("minimal-drawing.vsd")).unwrap();
        let row = IndexRow {
            blob_id: "blob".to_owned(),
            tier: "a".to_owned(),
            ext: "vsd".to_owned(),
            excluded: false,
        };
        let (sweep, kinds) = sweep_one(&bytes, &row, "11");
        assert_eq!(sweep.outcome, "opened");
        assert_eq!(sweep.kind, "drawing");
        assert_eq!(sweep.version, "11");
        assert_eq!(sweep.panic, "");
        assert!(sweep.pages >= 1);
        let line = sweep_line(&sweep);
        assert!(line.starts_with("blob,a,vsd,drawing,11,opened,,"), "{line}");
        for kind in kinds {
            assert!(kind.starts_with("blob,"));
        }
    }

    #[test]
    fn sweeps_a_file_that_does_not_open() {
        let row = IndexRow {
            blob_id: "blob".to_owned(),
            tier: "a".to_owned(),
            ext: "vsd".to_owned(),
            excluded: false,
        };
        let (sweep, kinds) = sweep_one(b"garbage", &row, "");
        assert_eq!(sweep.outcome, "error");
        assert_eq!(sweep.error_kind, "NotAVisioFile");
        assert!(kinds.is_empty());
    }

    #[test]
    fn names_error_and_warning_kinds() {
        assert_eq!(
            error_kind(&Error::UnsupportedVersion(5)),
            "UnsupportedVersion"
        );
        assert_eq!(
            error_kind(&Error::NotAVisioFile("x".to_owned())),
            "NotAVisioFile"
        );
        assert_eq!(error_kind(&Error::NotImplemented), "NotImplemented");
        let warning = ImportWarning {
            context: "page 1".to_owned(),
            kind: WarningKind::SkippedRecord { name: "OLE data" },
        };
        assert_eq!(warning_kind(&warning), ("skipped_record", "OLE data"));
        assert_eq!(warning_slot("dropped"), 1);
        assert_eq!(warning_slot("other"), 5);
    }

    #[test]
    fn the_runs_skip_without_a_corpus() {
        let dir = temp_dir("wide-corpus-skip");
        assert_eq!(run_classify(&dir.join("absent")), Ran::Skipped);
        assert_eq!(run_sweep(&dir.join("absent")), Ran::Skipped);
    }

    /// Builds a tiny corpus: one real fixture, one garbage file, and
    /// an index that excludes the garbage one.
    #[test]
    fn the_runs_write_their_csv_files() {
        let dir = temp_dir("wide-corpus-run");
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        let fixture = fs::read(fixtures_dir().join("minimal-drawing.vsd")).unwrap();
        let good = "1".repeat(40);
        let bad = "2".repeat(40);
        fs::write(dir.join(format!("a/{good}.vsd")), &fixture).unwrap();
        fs::write(dir.join(format!("b/{bad}.vsd")), b"garbage").unwrap();

        assert_eq!(run_classify(&dir), Ran::Files(2));
        let facts = fs::read_to_string(dir.join("ole-facts.csv")).unwrap();
        assert!(facts.starts_with(&ole_header()));
        assert!(facts.contains(&format!("{good},ole-drawing,11")));
        assert!(facts.contains(&format!("{bad},ole-invalid,")));

        let index = format!(
            "{}\n{good},a,mit,o/r,c,p/g.vsd,vsd,s,1,ole-drawing,11,yes,0,0,0,0,0,\n\
             {bad},b,mit,o/r,c,p/b.vsd,vsd,s,1,ole-invalid,,unknown,0,0,0,0,1,microsoft-author\n",
            "blob_id,tier,license,repo,commit,path,ext,sha256,size,kind,version,kind_matches_ext,ms_author,ms_company,template_copy,template_origin,excluded,exclude_reason"
        );
        fs::write(dir.join("index.csv"), index).unwrap();
        assert_eq!(run_sweep(&dir), Ran::Files(1));
        let sweep = fs::read_to_string(dir.join("sweep-vsd.csv")).unwrap();
        assert!(sweep.starts_with(&sweep_header()));
        assert!(sweep.contains("opened"), "{sweep}");
        assert!(!sweep.contains(&bad), "an excluded file is never swept");
        let kinds = fs::read_to_string(dir.join("warning-kinds-vsd.csv")).unwrap();
        assert!(kinds.starts_with("blob_id,kind,what"));
    }
}
