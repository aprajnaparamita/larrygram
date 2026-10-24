//! Larrygram developer tasks.
//!
//! Run with `cargo xtask <task>`:
//!
//! - `gate [--quick] [--skip <step>[,<step>...]]`: the quality gate (see AGENTS.md).
//! - `codegen [--check]`: regenerate `crates/model/src/generated/` from
//!   `ui-spec/shapesheet.toml` and `crates/ui/src/icons/table.rs` from
//!   `ui-spec/icons.yaml`, or verify them with `--check`. See ADR 0015
//!   and ADR 0062.
//! - `parity [--check]`: regenerate `docs/parity/controls.md` from the
//!   builtin registry and the `ui-spec` data, or verify it with
//!   `--check`. See ADR 0037.
//! - `coverage [--update-ratchet]`: coverage only; optionally record the ratchet.
//! - `layers`: check crate layering rules.
//! - `corpus [--verify-only] [--offline <dir>]`: fetch and verify the test corpus.
//! - `extract [--reference <path>] [--out <path>] [--allow-unimplemented] [--check]`:
//!   regenerate every Phase 1 output from `reference/`, or verify the
//!   committed files with `--check`. See ADR 0009.
//! - `oracle [status|build|clean] [--build-dir <path>] [--jobs <n>]`:
//!   build or inspect the libvisio test oracle. See ADR 0010.
//!
//! All argument handling lives in [`parse_task`], which validates every
//! subcommand's arguments before anything runs (C3).

#![forbid(unsafe_code)]

mod codegen;
mod corpus;
mod coverage;
mod extract;
mod gate;
mod layers;
mod oracle;
mod parity;
mod util;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <task>

tasks:
  gate [--quick] [--skip <step>,...]   run the quality gate
                                       steps: codegen names parity fmt clippy test doc coverage deny layers wasm fuzz
                                       --quick skips coverage, wasm and fuzz
  codegen [--check]                    regenerate crates/model/src/generated from
                                       ui-spec/shapesheet.toml (ADR 0015) and
                                       crates/ui/src/icons/table.rs from
                                       ui-spec/icons.yaml (ADR 0062)
                                       --check fails when a committed file is stale
  parity [--check]                     regenerate docs/parity/controls.md from the
                                       registry and the ui-spec data (ADR 0037)
                                       --check fails when the committed file is stale
  coverage [--update-ratchet]          run coverage and check thresholds
  layers                               check crate layering rules
  corpus [--verify-only] [--offline d] fetch and verify the test corpus (ADR 0005)
                                       --offline copies from a local dir instead of the network
  extract [--reference <path>] [--out <path>] [--allow-unimplemented] [--check]
                                       regenerate Phase 1 outputs from reference/
                                       --check regenerates into a scratch dir and
                                       compares byte-for-byte with the committed files
  oracle [status|build|clean] [--build-dir <path>] [--jobs <n>]
                                       build/inspect the libvisio test oracle (ADR 0010)";

/// The usage text (`cargo xtask help` prints it).
fn usage() -> &'static str {
    USAGE
}

/// What `cargo xtask` was asked to do. The variants carry the parsed
/// arguments where a subcommand has a parsed options type.
#[derive(Debug)]
enum Task {
    /// `cargo xtask gate` with its raw arguments.
    Gate(Vec<String>),
    /// `cargo xtask codegen`.
    Codegen(codegen::Options),
    /// `cargo xtask parity`.
    Parity(parity::Options),
    /// `cargo xtask coverage`.
    Coverage {
        /// Whether to rewrite the ratchet in `coverage.toml`.
        update_ratchet: bool,
    },
    /// `cargo xtask layers`.
    Layers,
    /// `cargo xtask corpus`.
    Corpus(corpus::Options),
    /// `cargo xtask extract`.
    Extract(extract::ExtractArgs),
    /// `cargo xtask oracle` with its raw arguments.
    Oracle(Vec<String>),
    /// `cargo xtask help` (also the empty command line).
    Help,
}

/// Parses the command line into a [`Task`]. Every subcommand's
/// arguments are validated here (by the subcommand's own parser), so
/// argument errors surface before anything runs. Never runs a task.
fn parse_task(args: &[String]) -> Result<Task, String> {
    let Some((cmd, rest)) = args.split_first() else {
        return Ok(Task::Help);
    };
    match cmd.as_str() {
        "gate" => {
            gate::parse_args(rest)?;
            Ok(Task::Gate(rest.to_vec()))
        }
        "codegen" => Ok(Task::Codegen(codegen::parse_args(rest)?)),
        "parity" => Ok(Task::Parity(parity::parse_args(rest)?)),
        "coverage" => Ok(Task::Coverage {
            update_ratchet: coverage::parse_args(rest)?,
        }),
        "layers" => {
            if rest.is_empty() {
                Ok(Task::Layers)
            } else {
                Err(format!(
                    "layers takes no arguments (got {})",
                    rest.join(" ")
                ))
            }
        }
        "corpus" => Ok(Task::Corpus(corpus::parse_args(rest)?)),
        "extract" => Ok(Task::Extract(extract::parse_args(rest)?)),
        "oracle" => {
            oracle::parse_args(rest)?;
            Ok(Task::Oracle(rest.to_vec()))
        }
        "-h" | "--help" | "help" => Ok(Task::Help),
        other => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match parse_task(&args) {
        Ok(task) => run_task(task),
        Err(msg) => Err(msg),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("xtask: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one parsed [`Task`].
fn run_task(task: Task) -> Result<(), String> {
    match task {
        Task::Gate(args) => gate::run(&args),
        Task::Codegen(options) => codegen::run(&options),
        Task::Parity(options) => parity::run(&options),
        Task::Coverage { update_ratchet } => coverage::run(update_ratchet),
        Task::Layers => layers::run(),
        Task::Corpus(opts) => corpus::run_opts(&opts),
        Task::Extract(parsed) => extract_dispatch(&parsed),
        Task::Oracle(args) => oracle::run(&args),
        Task::Help => {
            println!("{}", usage());
            Ok(())
        }
    }
}

/// `cargo xtask extract` entry point. Runs the dispatcher, prints one
/// line per extractor, and returns Ok(()) only when every extractor
/// succeeds (or is `Skipped` under `--allow-unimplemented`). With
/// `--check` it regenerates into a scratch directory and compares
/// byte-for-byte with the committed files instead (P1-08 goal 3).
fn extract_dispatch(parsed: &extract::ExtractArgs) -> Result<(), String> {
    if parsed.check {
        let report = extract::check(parsed)?;
        return if report.is_clean() {
            println!("extract --check: OK (every output matches the committed files)");
            Ok(())
        } else {
            let mut lines: Vec<String> = Vec::new();
            if !report.differing.is_empty() {
                lines.push(format!(
                    "{} output file(s) differ from the committed files \
                     (re-run `cargo xtask extract` and commit):",
                    report.differing.len()
                ));
                lines.extend(
                    report
                        .differing
                        .iter()
                        .map(|p| format!("  {}", p.display())),
                );
            }
            if !report.stale.is_empty() {
                lines.push(format!(
                    "{} committed generated file(s) were produced by no extractor \
                     (remove them, or list them in GENERATED_OUTPUTS):",
                    report.stale.len()
                ));
                lines.extend(report.stale.iter().map(|p| format!("  {}", p.display())));
            }
            Err(format!("extract --check failed:\n{}", lines.join("\n")))
        };
    }
    let results = extract::run(parsed)?;
    let mut had_failure = false;
    for (name, outcome) in &results {
        match outcome {
            extract::Outcome::Ok { outputs } => {
                println!("extract[{name}]: OK ({} file(s))", outputs.len());
            }
            extract::Outcome::Skipped { task } => {
                println!("extract[{name}]: SKIPPED (not implemented, {task})");
            }
            extract::Outcome::Failed(e) => {
                had_failure = true;
                eprintln!("extract[{name}]: FAIL: {e}");
            }
        }
    }
    if had_failure {
        Err("one or more extractors failed".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// C3: an unknown subcommand is a clean error carrying the usage.
    #[test]
    fn unknown_task_is_an_error_with_the_usage() {
        let err = parse_task(&args(&["frobnicate"])).unwrap_err();
        assert!(err.contains("unknown task `frobnicate`"), "{err}");
        assert!(err.contains("usage: cargo xtask"), "{err}");
    }

    /// C3: the empty command line and the help flags print the usage.
    #[test]
    fn help_text_is_the_usage() {
        assert!(matches!(parse_task(&args(&[])).unwrap(), Task::Help));
        assert!(matches!(parse_task(&args(&["help"])).unwrap(), Task::Help));
        assert!(matches!(parse_task(&args(&["-h"])).unwrap(), Task::Help));
        assert!(matches!(
            parse_task(&args(&["--help"])).unwrap(),
            Task::Help
        ));
        // The usage lists every task the dispatcher accepts.
        for task in [
            "gate", "codegen", "parity", "coverage", "layers", "corpus", "extract", "oracle",
        ] {
            assert!(usage().contains(task), "usage is missing `{task}`");
        }
    }

    /// C3: each subcommand reports its own argument errors.
    #[test]
    fn subcommand_argument_errors() {
        let err = parse_task(&args(&["gate", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown gate argument"), "{err}");
        let err = parse_task(&args(&["gate", "--skip"])).unwrap_err();
        assert!(err.contains("--skip needs"), "{err}");
        let err = parse_task(&args(&["gate", "--skip", "bogus"])).unwrap_err();
        assert!(err.contains("unknown step"), "{err}");

        let err = parse_task(&args(&["codegen", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown codegen argument"), "{err}");

        let err = parse_task(&args(&["parity", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown parity argument"), "{err}");

        let err = parse_task(&args(&["coverage", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown coverage argument"), "{err}");

        let err = parse_task(&args(&["layers", "extra"])).unwrap_err();
        assert!(err.contains("layers takes no arguments"), "{err}");

        let err = parse_task(&args(&["corpus", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown corpus argument"), "{err}");
        let err = parse_task(&args(&["corpus", "--offline"])).unwrap_err();
        assert!(err.contains("--offline needs"), "{err}");

        let err = parse_task(&args(&["extract", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown extract argument"), "{err}");
        let err = parse_task(&args(&["extract", "--reference"])).unwrap_err();
        assert!(err.contains("--reference needs"), "{err}");

        let err = parse_task(&args(&["oracle", "bogus"])).unwrap_err();
        assert!(err.contains("unknown argument"), "{err}");
        let err = parse_task(&args(&["oracle", "--jobs"])).unwrap_err();
        assert!(err.contains("--jobs needs"), "{err}");
        let err = parse_task(&args(&["oracle", "--jobs", "0"])).unwrap_err();
        assert!(err.contains("positive integer"), "{err}");
    }

    /// Valid command lines parse into the matching task.
    #[test]
    fn valid_command_lines_parse() {
        assert!(matches!(
            parse_task(&args(&["gate", "--quick"])).unwrap(),
            Task::Gate(_)
        ));
        assert!(matches!(
            parse_task(&args(&["codegen"])).unwrap(),
            Task::Codegen(_)
        ));
        assert!(matches!(
            parse_task(&args(&["codegen", "--check"])).unwrap(),
            Task::Codegen(codegen::Options { check: true })
        ));
        assert!(matches!(
            parse_task(&args(&["parity"])).unwrap(),
            Task::Parity(_)
        ));
        assert!(matches!(
            parse_task(&args(&["parity", "--check"])).unwrap(),
            Task::Parity(parity::Options { check: true })
        ));
        assert!(matches!(
            parse_task(&args(&["coverage", "--update-ratchet"])).unwrap(),
            Task::Coverage {
                update_ratchet: true
            }
        ));
        assert!(matches!(
            parse_task(&args(&["layers"])).unwrap(),
            Task::Layers
        ));
        assert!(matches!(
            parse_task(&args(&["corpus", "--verify-only"])).unwrap(),
            Task::Corpus(_)
        ));
        assert!(matches!(
            parse_task(&args(&["extract", "--check"])).unwrap(),
            Task::Extract(_)
        ));
        assert!(matches!(
            parse_task(&args(&["oracle", "status"])).unwrap(),
            Task::Oracle(_)
        ));
    }
}
