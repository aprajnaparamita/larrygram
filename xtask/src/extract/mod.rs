//! Phase 1 extractors.
//!
//! Each submodule reads one source under `reference/` and writes one or
//! more files under the workspace (`ui-spec/`, `docs/`,
//! `docs/parity/`). The top-level [`run`] dispatcher is invoked by
//! `cargo xtask extract`. See ADR 0009.
//!
//! Phase 1 task ownership (each module is a stub in P1-02 and is filled
//! in by the named task):
//!
//! | Module          | Implemented in | Output                                    |
//! |-----------------|----------------|-------------------------------------------|
//! | [`fluent`]      | P1-03          | `ui-spec/panel-2016.yaml`, `panel-365.yaml`, `docs/parity/controls.md` |
//! | [`commands`]    | P1-04          | `ui-spec/commands.yaml` (+ fills layout `command` fields) |
//! | [`visuicmds`]   | P1-04          | VisUICmds parser used by [`commands`]     |
//! | [`shapesheet`]  | P1-04          | `ui-spec/shapesheet.toml`                 |
//! | [`shortcuts`]   | P1-04          | `ui-spec/shortcuts.yaml`                  |
//! | [`vsdx_schema`] | P1-05          | `docs/vsdx-inventory.md`                  |
//! | [`markdown`]    | P1-05          | markdown table + fence parser (shared)    |
//! | [`diff`]        | P1-03          | `ui-spec/diff-2016-vs-365.md`             |
//! | [`parity`]      | P1-03          | `docs/parity/controls.md`                 |
//! | [`schema`]      | P1-03          | schema validator shared by all extractors |
//!
//! The dispatcher logs a `commands` row (not `visuicmds`): the merge
//! lives in [`commands`], which calls [`visuicmds::run`] for the
//! enumeration half.

pub mod commands;
pub mod diff;
pub mod fluent;
pub mod markdown;
pub mod parity;
pub mod schema;
pub mod shapesheet;
pub mod shortcuts;
pub mod util;
pub mod visuicmds;
pub mod vsdx_schema;

#[cfg(test)]
mod fixtures;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::util::Result;

/// Error type shared by every extractor. Each variant names the source
/// file and a human-readable message; no panic, no truncation.
#[allow(dead_code)] // Missing/Parse/Io/Schema are constructed by P1-03..P1-05.
#[derive(Debug)]
pub enum ExtractError {
    /// A required input path is missing. The dispatcher reports this
    /// and stops before invoking any extractor.
    Missing {
        /// The path that was not found.
        path: PathBuf,
    },
    /// The extractor for this output is not implemented yet. The
    /// dispatcher treats this as a soft warning under
    /// `--allow-unimplemented`.
    Unimplemented {
        /// The owning task id (e.g. `P1-03`).
        task: &'static str,
    },
    /// An input file could not be parsed. Includes the file path and a
    /// message; never includes the file content.
    Parse {
        /// The file we tried to parse.
        path: PathBuf,
        /// A short reason; may include a row number or column.
        message: String,
    },
    /// A write to the workspace failed. Includes the destination path.
    Io {
        /// Where we tried to write.
        path: PathBuf,
        /// The underlying `std::io::Error` message.
        source: String,
    },
    /// A generated file failed validation against `ui-spec/schema.json`.
    Schema {
        /// The file that failed.
        path: PathBuf,
        /// The validator's error message.
        message: String,
    },
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { path } => write!(f, "missing input: {}", path.display()),
            Self::Unimplemented { task } => write!(f, "not implemented ({task})"),
            Self::Parse { path, message } => {
                write!(f, "parse error in {}: {message}", path.display())
            }
            Self::Io { path, source } => {
                write!(f, "io error on {}: {source}", path.display())
            }
            Self::Schema { path, message } => {
                write!(
                    f,
                    "schema validation failed for {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ExtractError {}

/// Convenience alias used across extractors.
pub type ExtractResult<T> = std::result::Result<T, ExtractError>;

/// Arguments for [`run`]. Defaults are the workspace root and
/// `reference/`.
#[derive(Debug, Clone)]
pub struct ExtractArgs {
    /// Directory containing the raw downloads (P1-01's `reference/`
    /// symlink target).
    pub reference: PathBuf,
    /// Directory that owns the output files. Most extractors use the
    /// workspace root and write to `ui-spec/`, `docs/`,
    /// `docs/parity/` inside it.
    pub out: PathBuf,
    /// When true, [`ExtractError::Unimplemented`] is treated as a soft
    /// warning. CI in P1-02 uses this; later phases drop the flag.
    pub allow_unimplemented: bool,
    /// When true, `cargo xtask extract` runs [`check`] instead of
    /// [`run`]: regenerate into a scratch directory and compare
    /// byte-for-byte with the committed files (P1-08 goal 3).
    pub check: bool,
}

impl ExtractArgs {
    /// Defaults: `<workspace>/reference`, `<workspace>`, `false`.
    pub fn default_paths() -> Self {
        let root = crate::util::root();
        Self {
            reference: root.join("reference"),
            out: root,
            allow_unimplemented: false,
            check: false,
        }
    }
}

/// Parses `cargo xtask extract` arguments.
pub fn parse_args(args: &[String]) -> Result<ExtractArgs> {
    let mut out = ExtractArgs::default_paths();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--reference" => {
                let p = it.next().ok_or("--reference needs a path argument")?;
                out.reference = PathBuf::from(p);
            }
            "--out" => {
                let p = it.next().ok_or("--out needs a path argument")?;
                out.out = PathBuf::from(p);
            }
            "--allow-unimplemented" => out.allow_unimplemented = true,
            "--check" => out.check = true,
            other => return Err(format!("unknown extract argument `{other}`")),
        }
    }
    Ok(out)
}

/// Every generated output path, relative to the workspace root. The
/// one list to update when an extractor gains or loses an output;
/// [`check`] compares exactly these and flags committed generated
/// files outside the produced set (R5).
pub const GENERATED_OUTPUTS: &[&str] = &[
    "ui-spec/panel-2016.yaml",
    "ui-spec/panel-365.yaml",
    "ui-spec/commands.yaml",
    "ui-spec/shortcuts.yaml",
    "ui-spec/shapesheet.toml",
    "ui-spec/diff-2016-vs-365.md",
    "docs/parity/controls.md",
    "docs/vsdx-inventory.md",
];

/// Hand-maintained files that live next to the generated outputs.
/// `--check` never flags them (R5): they are inputs or documentation,
/// not extractor outputs.
pub const HAND_MAINTAINED: &[&str] = &[
    "ui-spec/schema.json",
    "ui-spec/out-of-scope.txt",
    "ui-spec/README.md",
    // Project-compiled Phase 7 data (P7-01, ADRs 0061-0064).
    // `commands-phase7.yaml` is merged into `commands.yaml` and deleted
    // by P7-02.
    "ui-spec/commands-phase7.yaml",
    "ui-spec/contexts.yaml",
    "ui-spec/icons.yaml",
];

/// True when `rel` is a hand-maintained file (one of
/// [`HAND_MAINTAINED`] or any `ui-spec/*.txt` input).
fn is_hand_maintained(rel: &Path) -> bool {
    let shown = rel.display().to_string().replace('\\', "/");
    HAND_MAINTAINED.contains(&shown.as_str()) || shown.ends_with(".txt")
}

/// Outcome of one extractor in a run.
#[derive(Debug)]
pub enum Outcome {
    /// The extractor produced its output and the file passes schema
    /// validation.
    Ok {
        /// Files written by the extractor (sorted, absolute paths).
        outputs: Vec<PathBuf>,
    },
    /// The extractor is a stub; the task id is the owner.
    Skipped {
        /// Owning task id (e.g. `P1-03`).
        task: &'static str,
    },
    /// The extractor ran and failed.
    Failed(ExtractError),
}

/// Runs every extractor in order and returns a per-module outcome.
/// Never panics. A missing `reference/` directory is reported as
/// `Err(ExtractError::Missing { .. })` and no extractor is invoked.
pub fn run(args: &ExtractArgs) -> Result<Vec<(&'static str, Outcome)>> {
    if !args.reference.is_dir() {
        return Err(format!(
            "reference/ not found: {}. See docs/sources.md and task P1-01 for setup, or pass --reference <path>.",
            args.reference.display()
        ));
    }
    let in_dir = &args.reference;
    let out_dir = &args.out;

    let modules: &[Module] = &[
        Module {
            name: "fluent",
            run: fluent::run,
        },
        // P1-04: the `commands` merge takes the old `visuicmds` slot
        // (it calls `visuicmds::run` for the enumeration half). The
        // per-extractor output list logs `commands`, not `visuicmds`.
        Module {
            name: "commands",
            run: commands::run,
        },
        Module {
            name: "shapesheet",
            run: shapesheet::run,
        },
        Module {
            name: "shortcuts",
            run: shortcuts::run,
        },
        Module {
            name: "vsdx_schema",
            run: vsdx_schema::run,
        },
        Module {
            name: "diff",
            run: diff::run,
        },
        Module {
            name: "parity",
            run: parity::run,
        },
    ];

    let mut results = Vec::new();
    for m in modules {
        let outcome = match (m.run)(in_dir, out_dir) {
            Ok(outputs) => Outcome::Ok { outputs },
            Err(ExtractError::Unimplemented { task }) if args.allow_unimplemented => {
                Outcome::Skipped { task }
            }
            Err(e) => Outcome::Failed(e),
        };
        results.push((m.name, outcome));
    }
    Ok(results)
}

/// One row in the dispatcher's module table. Used to keep the
/// `modules` array readable and to avoid the `clippy::type_complexity`
/// lint.
struct Module {
    /// Stable name printed in the dispatcher's output.
    name: &'static str,
    /// The per-extractor entry point.
    run: fn(&Path, &Path) -> ExtractResult<Vec<PathBuf>>,
}

/// `cargo xtask extract --check` (P1-08 goal 3): regenerates every
/// output into a scratch directory and compares each output
/// byte-for-byte with the committed file of the same relative path
/// under `args.out`.
///
/// The report lists the relative paths whose bytes differ, and the
/// committed generated files that no extractor produced (R5: a
/// renamed or dropped output would otherwise survive every check).
/// `Err` means the check could not run at all (no `reference/`, or an
/// extractor failed). Skipped (unimplemented) extractors produce no
/// outputs and are not compared.
///
/// The scratch tree is seeded with `ui-spec/out-of-scope.txt` when the
/// committed tree has one: that file is a hand-maintained *input* to
/// [`parity`], not a generated output.
pub fn check(args: &ExtractArgs) -> Result<CheckReport> {
    let scratch = scratch_dir()?;
    seed_input(&args.out, &scratch, "ui-spec/out-of-scope.txt");
    let mut scratch_args = args.clone();
    scratch_args.out = scratch.clone();
    scratch_args.check = false;
    let results = run(&scratch_args)?;

    let mut report = CheckReport::default();
    let mut produced: BTreeSet<PathBuf> = BTreeSet::new();
    for (name, outcome) in &results {
        match outcome {
            Outcome::Ok { outputs } => {
                for generated in outputs {
                    let rel = generated.strip_prefix(&scratch).unwrap_or(generated);
                    produced.insert(rel.to_path_buf());
                    if !same_bytes(generated, &args.out.join(rel)) {
                        report.differing.push(rel.to_path_buf());
                    }
                }
            }
            Outcome::Skipped { .. } => {}
            Outcome::Failed(e) => {
                return Err(format!("extract[{name}] failed under --check: {e}"));
            }
        }
    }
    report.differing.sort();
    report.differing.dedup();
    report.stale = stale_generated(&produced, &args.out);
    Ok(report)
}

/// Result of `cargo xtask extract --check`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Committed files whose content differs from the regenerated one.
    pub differing: Vec<PathBuf>,
    /// Committed generated files that no extractor produced (R5).
    pub stale: Vec<PathBuf>,
}

impl CheckReport {
    /// True when every output matches and nothing is stale.
    pub fn is_clean(&self) -> bool {
        self.differing.is_empty() && self.stale.is_empty()
    }
}

/// Committed generated files that the run did not produce (R5): every
/// path in [`GENERATED_OUTPUTS`] that exists in the committed tree but
/// was not regenerated, plus every generated-looking file under
/// `ui-spec/` (`*.yaml`, `*.toml`, `*.md`) outside the produced set.
/// Hand-maintained inputs are never stale.
fn stale_generated(produced: &BTreeSet<PathBuf>, committed_root: &Path) -> Vec<PathBuf> {
    let mut stale: Vec<PathBuf> = Vec::new();
    for rel in GENERATED_OUTPUTS {
        let path = Path::new(rel);
        if committed_root.join(path).is_file() && !produced.contains(path) {
            stale.push(path.to_path_buf());
        }
    }
    let ui_dir = committed_root.join("ui-spec");
    if let Ok(entries) = std::fs::read_dir(&ui_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let is_generated_ext = [".yaml", ".toml", ".md"]
                .iter()
                .any(|ext| name.ends_with(ext));
            if !is_generated_ext {
                continue;
            }
            let rel = PathBuf::from("ui-spec").join(name);
            if produced.contains(&rel) || is_hand_maintained(&rel) {
                continue;
            }
            stale.push(rel);
        }
    }
    stale.sort();
    stale.dedup();
    stale
}

/// True when both paths hold the same content. A missing or
/// unreadable file is never equal to anything. Line endings do not
/// count: a Windows checkout with `core.autocrlf` compares equal to
/// the LF the emitters write (R6).
fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => strip_cr(&x) == strip_cr(&y),
        _ => false,
    }
}

/// Drops carriage returns, so CRLF and LF compare equal.
fn strip_cr(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|b| *b != b'\r').collect()
}

/// Copies one committed input file into the scratch tree. Missing
/// source files are ignored (`parity` treats a missing
/// `out-of-scope.txt` as "nothing is out of scope").
fn seed_input(from_root: &Path, to_root: &Path, rel: &str) {
    let from = from_root.join(rel);
    let to = to_root.join(rel);
    if from.is_file()
        && let Some(parent) = to.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::copy(&from, &to);
    }
}

/// A fresh scratch directory under the system temp dir. The name
/// carries the pid and a counter so parallel runs never collide.
fn scratch_dir() -> Result<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "larrygram-extract-check-{}-{n}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("cannot clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    /// Loader check (P1-04, widened by P1-08 goal 2): every committed
    /// `ui-spec/*.yaml` passes `Schema::validate_file` with the xtask
    /// validator.
    #[test]
    fn committed_commands_and_shortcuts_pass_schema_validation() {
        let root = crate::util::root();
        let schema_path = root.join("ui-spec").join("schema.json");
        if !schema_path.is_file() {
            eprintln!("schema.json not present; skipping");
            return;
        }
        let schema = schema::Schema::load(&schema_path).expect("schema loads");
        for name in [
            "panel-2016.yaml",
            "panel-365.yaml",
            "commands.yaml",
            "shortcuts.yaml",
        ] {
            let path = root.join("ui-spec").join(name);
            if !path.is_file() {
                eprintln!("{name} not present; skipping");
                continue;
            }
            schema
                .validate_file(&path)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    /// P1-08 goal 2: `ui-spec/shapesheet.toml` parses and carries no
    /// duplicate section, row or cell names.
    #[test]
    fn committed_shapesheet_toml_parses_without_duplicate_names() {
        let path = crate::util::root().join("ui-spec").join("shapesheet.toml");
        if !path.is_file() {
            eprintln!("shapesheet.toml not present; skipping");
            return;
        }
        let text = std::fs::read_to_string(&path).expect("shapesheet.toml reads");
        let value: toml::Value = toml::from_str(&text).expect("shapesheet.toml parses as TOML");

        let rows = value
            .get("rows")
            .and_then(toml::Value::as_array)
            .expect("shapesheet.toml has [[rows]]");
        check_unique("row", rows.iter().filter_map(|r| r.get("name")));

        let sections = value
            .get("sections")
            .and_then(toml::Value::as_array)
            .expect("shapesheet.toml has [[sections]]");
        check_unique("section", sections.iter().filter_map(|s| s.get("name")));
        for section in sections {
            let name = section
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or("<unnamed>");
            let empty = Vec::new();
            let cells = section
                .get("cells")
                .and_then(toml::Value::as_array)
                .unwrap_or(&empty);
            check_unique(
                &format!("cell of section {name}"),
                cells.iter().filter_map(|c| c.get("name")),
            );
        }
    }

    /// Asserts that every `name` in `values` is a string and appears at
    /// most once.
    fn check_unique<'a>(what: &str, values: impl Iterator<Item = &'a toml::Value>) {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for value in values {
            let name = value
                .as_str()
                .unwrap_or_else(|| panic!("{what} name is not a string: {value}"));
            assert!(seen.insert(name), "duplicate {what} name: {name}");
        }
    }

    /// Loader check (P1-04): the committed `ui-spec/` data loads
    /// with the runtime [`larrygram_ui_spec::Loader`], including the
    /// three ADR 0007 cross-references. The fixture mirrors the
    /// loader's whole data set (`ui-spec/` with the v2 documents of
    /// task P7-03, plus the icon set the `icon_file` cross-check
    /// resolves) in a temp workspace.
    #[test]
    fn committed_ui_spec_loads_with_the_runtime_loader() {
        use larrygram_ui_spec::Loader;

        let root = crate::util::root();
        let ui = root.join("ui-spec");
        let files = [
            "panel-2016.yaml",
            "panel-365.yaml",
            "commands.yaml",
            "shortcuts.yaml",
        ];
        if files.iter().any(|f| !ui.join(f).is_file()) {
            eprintln!("committed ui-spec not complete; skipping");
            return;
        }
        let tmp = crate::util::test_support::TempDir::new("ui-loader");
        crate::util::test_support::copy_tree(&ui, &tmp.join("ui-spec"));
        crate::util::test_support::copy_tree(
            &root.join("crates/ui/assets/icons/tabler"),
            &tmp.join("crates/ui/assets/icons/tabler"),
        );

        let loader = Loader::new(tmp.path()).expect("loader compiles");
        let sets = loader.load_all().expect("the committed data loads");
        assert!(!sets.panel_365.commands.entries.is_empty());
        assert!(!sets.panel_365.shortcuts.entries.is_empty());
    }

    #[test]
    fn parse_args_defaults() {
        let a = parse_args(&[]).unwrap();
        assert!(a.reference.ends_with("reference"));
        assert_eq!(a.out, crate::util::root());
        assert!(!a.allow_unimplemented);
        assert!(!a.check);
    }

    #[test]
    fn parse_args_overrides() {
        let a = parse_args(&[
            "--reference".into(),
            "/tmp/ref".into(),
            "--out".into(),
            "/tmp/out".into(),
            "--allow-unimplemented".into(),
            "--check".into(),
        ])
        .unwrap();
        assert_eq!(a.reference, PathBuf::from("/tmp/ref"));
        assert_eq!(a.out, PathBuf::from("/tmp/out"));
        assert!(a.allow_unimplemented);
        assert!(a.check);
    }

    #[test]
    fn parse_args_rejects_unknown() {
        assert!(parse_args(&["--bogus".into()]).is_err());
        assert!(parse_args(&["--reference".into()]).is_err());
        assert!(parse_args(&["--out".into()]).is_err());
    }

    #[test]
    fn missing_reference_is_a_clean_error() {
        let args = ExtractArgs {
            reference: PathBuf::from("/nonexistent/never/there"),
            out: crate::util::root(),
            allow_unimplemented: true,
            check: true,
        };
        let err = run(&args).unwrap_err();
        assert!(err.contains("reference/ not found"), "{err}");
        // m3: the error must point to docs/sources.md / P1-01, not
        // to `cargo xtask corpus` (the corpus fetcher does not
        // populate reference/).
        assert!(err.contains("docs/sources.md"), "{err}");
        assert!(!err.contains("cargo xtask corpus"), "{err}");
        // `--check` surfaces the same clean error.
        let err = check(&args).unwrap_err();
        assert!(err.contains("reference/ not found"), "{err}");
    }

    #[test]
    fn error_display_distinct() {
        let e = ExtractError::Unimplemented { task: "P1-03" };
        assert_eq!(e.to_string(), "not implemented (P1-03)");
        let e = ExtractError::Parse {
            path: PathBuf::from("/x/y"),
            message: "bad row 3".into(),
        };
        assert!(e.to_string().contains("bad row 3"));
    }

    // ---- `extract --check` (P1-08 goal 3) ----------------------------

    /// Runs the whole dispatcher over the fixture reference tree and
    /// returns `(reference, out)`: `out` then stands in for the
    /// committed tree of a `--check` run. Panics if any extractor
    /// fails on the fixture tree.
    fn fixture_outputs(name: &str) -> (PathBuf, PathBuf) {
        let reference = fixtures::reference_tree(name);
        let out = crate::util::test_support::temp_path(&format!("check-out-{name}"));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let args = ExtractArgs {
            reference: reference.clone(),
            out: out.clone(),
            allow_unimplemented: false,
            check: false,
        };
        let results = run(&args).unwrap();
        for (module, outcome) in &results {
            match outcome {
                Outcome::Ok { outputs } => assert!(!outputs.is_empty(), "{module} wrote nothing"),
                other => panic!("extractor {module} failed on the fixture tree: {other:?}"),
            }
        }
        (reference, out)
    }

    fn check_args(reference: PathBuf, out: PathBuf) -> ExtractArgs {
        ExtractArgs {
            reference,
            out,
            allow_unimplemented: false,
            check: true,
        }
    }

    #[test]
    fn check_passes_on_a_pristine_fixture_tree() {
        let (reference, out) = fixture_outputs("pristine");
        let report = check(&check_args(reference, out)).unwrap();
        assert!(report.is_clean(), "unexpected report: {report:?}");
    }

    #[test]
    fn check_reports_changed_and_missing_files() {
        let (reference, out) = fixture_outputs("changed");
        // One committed file changed, one gone missing.
        std::fs::write(out.join("ui-spec").join("commands.yaml"), b"# tampered\n").unwrap();
        std::fs::remove_file(out.join("docs").join("parity").join("controls.md")).unwrap();
        let report = check(&check_args(reference, out)).unwrap();
        let names: Vec<String> = report
            .differing
            .iter()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            vec![
                "docs/parity/controls.md".to_string(),
                "ui-spec/commands.yaml".to_string()
            ]
        );
    }

    #[test]
    fn check_seeds_the_out_of_scope_input() {
        // `ui-spec/out-of-scope.txt` is a hand-maintained input to the
        // parity extractor: with it present but unseeded, the scratch
        // run would produce a different `docs/parity/controls.md` and
        // `--check` would report a false difference.
        let (reference, out) = fixture_outputs("seeded");
        std::fs::write(out.join("ui-spec").join("out-of-scope.txt"), b"FileSave\n").unwrap();
        // Regenerate the stand-in committed tree with the input in
        // place, so its outputs are the ones the input produces.
        let regen = ExtractArgs {
            reference: reference.clone(),
            out: out.clone(),
            allow_unimplemented: false,
            check: false,
        };
        run(&regen).unwrap();
        let report = check(&check_args(reference, out.clone())).unwrap();
        assert!(
            report.is_clean(),
            "out-of-scope.txt must be seeded into the scratch tree: {report:?}"
        );
        // Sanity: the input really does change the parity output, so
        // the test above is not vacuous.
        let parity =
            std::fs::read_to_string(out.join("docs").join("parity").join("controls.md")).unwrap();
        let file_save = parity
            .lines()
            .find(|line| line.starts_with("| FileSave |"))
            .expect("the FileSave row is in the parity matrix");
        assert!(
            file_save.ends_with("| out-of-scope |"),
            "FileSave must be out-of-scope, got: {file_save}"
        );
    }

    /// R6: `--check` treats CRLF and LF as equal.
    #[test]
    fn check_treats_crlf_and_lf_as_equal() {
        let tmp = crate::util::test_support::TempDir::new("crlf");
        let lf = tmp.join("a.txt");
        let crlf = tmp.join("b.txt");
        std::fs::write(&lf, b"one\ntwo\n").unwrap();
        std::fs::write(&crlf, b"one\r\ntwo\r\n").unwrap();
        assert!(same_bytes(&lf, &crlf));
        std::fs::write(&crlf, b"one\r\ntoo\r\n").unwrap();
        assert!(!same_bytes(&lf, &crlf));
    }

    // ---- stale outputs (P1-13 goal 5 / R5) --------------------------

    /// The generated-path list is complete: the fixture run writes
    /// exactly the paths in [`GENERATED_OUTPUTS`].
    #[test]
    fn generated_outputs_list_matches_what_extractors_write() {
        let reference = fixtures::reference_tree("gen-list");
        let out = crate::util::test_support::TempDir::new("gen-list-out");
        let args = ExtractArgs {
            reference,
            out: out.path().to_path_buf(),
            allow_unimplemented: false,
            check: false,
        };
        let results = run(&args).unwrap();
        let mut produced: BTreeSet<String> = BTreeSet::new();
        for (_, outcome) in &results {
            match outcome {
                Outcome::Ok { outputs } => {
                    for p in outputs {
                        let rel = p.strip_prefix(out.path()).unwrap_or(p);
                        produced.insert(rel.display().to_string().replace('\\', "/"));
                    }
                }
                other => panic!("extractor failed on the fixture tree: {other:?}"),
            }
        }
        let listed: BTreeSet<String> = GENERATED_OUTPUTS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(produced, listed);
    }

    /// R5: a committed generated file that no extractor produced is
    /// flagged; hand-maintained inputs never are.
    #[test]
    fn check_flags_committed_files_no_extractor_produced() {
        let (reference, out) = fixture_outputs("stale");
        std::fs::write(
            out.join("ui-spec").join("legacy-layout.yaml"),
            b"# gone
",
        )
        .unwrap();
        std::fs::write(
            out.join("ui-spec").join("legacy-tables.toml"),
            b"# gone
",
        )
        .unwrap();
        std::fs::write(
            out.join("ui-spec").join("schema.json"),
            b"{}
",
        )
        .unwrap();
        std::fs::write(
            out.join("ui-spec").join("notes.txt"),
            b"notes
",
        )
        .unwrap();
        std::fs::write(
            out.join("ui-spec").join("README.md"),
            b"# readme
",
        )
        .unwrap();
        let report = check(&check_args(reference, out)).unwrap();
        let names: Vec<String> = report
            .stale
            .iter()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            vec![
                "ui-spec/legacy-layout.yaml".to_string(),
                "ui-spec/legacy-tables.toml".to_string()
            ]
        );
    }

    /// R5: a committed file at a generated output path that the run
    /// did not produce is stale, whatever its name.
    #[test]
    fn stale_generated_flags_unproduced_generated_outputs() {
        let root = crate::util::test_support::TempDir::new("stale-gen");
        std::fs::create_dir_all(root.join("ui-spec")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("ui-spec").join("shapesheet.toml"), b"# x\n").unwrap();
        std::fs::write(root.join("docs").join("vsdx-inventory.md"), b"# y\n").unwrap();

        let empty = BTreeSet::new();
        let stale = stale_generated(&empty, root.path());
        assert_eq!(stale.len(), 2, "{stale:?}");

        let mut produced = BTreeSet::new();
        produced.insert(PathBuf::from("ui-spec/shapesheet.toml"));
        let stale = stale_generated(&produced, root.path());
        assert_eq!(
            stale,
            vec![PathBuf::from("docs/vsdx-inventory.md")],
            "produced files are never stale"
        );
    }

    #[test]
    fn seed_input_copies_and_tolerates_missing_files() {
        let from = crate::util::test_support::TempDir::new("seed");
        let to = crate::util::test_support::TempDir::new("seed-to");
        std::fs::create_dir_all(from.join("ui-spec")).unwrap();
        std::fs::write(from.join("ui-spec").join("out-of-scope.txt"), b"X\n").unwrap();
        seed_input(from.path(), to.path(), "ui-spec/out-of-scope.txt");
        assert_eq!(
            std::fs::read_to_string(to.join("ui-spec").join("out-of-scope.txt")).unwrap(),
            "X\n"
        );
        // A missing source is a no-op, not an error.
        seed_input(from.path(), to.path(), "ui-spec/not-there.txt");
        assert!(!to.join("ui-spec").join("not-there.txt").exists());
    }
}
