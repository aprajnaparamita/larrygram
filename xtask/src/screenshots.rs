//! `cargo xtask screenshots [--only <id>]... [--to <dir>]` (task
//! P7-38): render the website's screenshots with Larrygram itself and
//! check them.
//!
//! 1. run `cargo test -p larrygram-ui --test screenshots -- --ignored
//!    --test-threads=1` (with `<id>` filters for `--only`) with
//!    `LARRYGRAM_SCREENSHOTS_DIR` set to the output directory;
//! 2. check every expected PNG exists, decodes, and meets its aspect
//!    and minimum size;
//! 3. with `--to`, copy the PNGs into `<dir>` (overwriting files of
//!    the same name, touching nothing else).
//!
//! The table below is this crate's own copy of the shot list in
//! `crates/ui/tests/screenshots/main.rs`; a unit test checks the two
//! have the same ids. `cli-render` needs a terminal and a browser and
//! is taken by hand: it is in neither table.

use std::path::{Path, PathBuf};

/// One shot the website needs.
pub struct Shot {
    /// The website's shot id (the PNG's file name stem).
    pub id: &'static str,
    /// The exact crop aspect (width : height).
    pub aspect: (u32, u32),
    /// The minimum PNG size in pixels.
    pub min: (u32, u32),
}

/// Every generated shot (the ids, aspects and minimum sizes).
pub const SHOTS: &[Shot] = &[
    Shot {
        id: "hero-panel-modern",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    Shot {
        id: "layout-panel-classic",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    Shot {
        id: "connectors-reroute",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    Shot {
        id: "shapes-window",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    Shot {
        id: "text-editing",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    Shot {
        id: "pages-tabs",
        aspect: (16, 9),
        min: (1920, 1080),
    },
    Shot {
        id: "welcome-wizard",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    Shot {
        id: "about-window",
        aspect: (16, 10),
        min: (2560, 1600),
    },
];

/// The parsed arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// The shot ids to render (all of them when empty).
    pub only: Vec<String>,
    /// Where to copy the finished PNGs, if anywhere.
    pub to: Option<PathBuf>,
}

/// Parses `cargo xtask screenshots`'s arguments (never runs anything).
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        only: Vec::new(),
        to: None,
    };
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--only" => {
                let Some(id) = args.get(index + 1) else {
                    return Err("--only needs a shot id".to_owned());
                };
                if !SHOTS.iter().any(|shot| shot.id == id) {
                    return Err(format!(
                        "--only {id:?} is not a shot id ({})",
                        ids().join(", ")
                    ));
                }
                options.only.push(id.clone());
                index += 2;
            }
            "--to" => {
                let Some(dir) = args.get(index + 1) else {
                    return Err("--to needs a directory".to_owned());
                };
                options.to = Some(PathBuf::from(dir));
                index += 2;
            }
            other => {
                return Err(format!(
                    "unknown argument {other:?}\n\
                     usage: cargo xtask screenshots [--only <id>]... [--to <dir>]"
                ));
            }
        }
    }
    Ok(options)
}

/// Every shot id, in table order.
fn ids() -> Vec<&'static str> {
    SHOTS.iter().map(|shot| shot.id).collect()
}

/// The shots `options` selected (all of them when `only` is empty).
fn selected(only: &[String]) -> Vec<&'static Shot> {
    SHOTS
        .iter()
        .filter(|shot| only.is_empty() || only.iter().any(|id| id == shot.id))
        .collect()
}

/// The test binary's name for a shot id (a Rust identifier cannot
/// hold the id's dashes).
fn test_name(id: &str) -> String {
    id.replace('-', "_")
}

/// The output directory: `<target dir>/screenshots`, with the cargo
/// target directory as `cargo metadata` reports it (the test target
/// asks cargo the same way when `LARRYGRAM_SCREENSHOTS_DIR` is unset).
pub fn out_dir(target: &Path) -> PathBuf {
    target.join("screenshots")
}

/// The `cargo test` invocation that renders `shots` into `out`. Each
/// shot's full test name is passed with `--exact`, so a name never
/// matches a longer one and a renamed test runs nothing (which
/// [`ran_every_test`] then reports).
fn test_command(root: &Path, out: &Path, shots: &[&Shot]) -> std::process::Command {
    let mut command = std::process::Command::new(crate::util::cargo());
    command
        .current_dir(root)
        .env("LARRYGRAM_SCREENSHOTS_DIR", out)
        .args([
            "test",
            "-p",
            "larrygram-ui",
            "--test",
            "screenshots",
            "--",
            "--ignored",
            "--test-threads=1",
            "--exact",
        ]);
    for shot in shots {
        command.arg(test_name(shot.id));
    }
    command
}

/// Checks that the test run's output reports every shot's test as
/// passed: a filter that ran zero tests (or skipped one) is an error,
/// not a quiet success over last run's PNGs.
fn ran_every_test(stdout: &str, shots: &[&Shot]) -> Result<(), String> {
    let missing: Vec<String> = shots
        .iter()
        .map(|shot| test_name(shot.id))
        .filter(|name| {
            let passed = format!("test {name} ... ok");
            !stdout.lines().any(|line| line.trim() == passed)
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the screenshot test run did not pass {} (no test by that name ran)",
            missing.join(", ")
        ))
    }
}

/// Deletes each selected shot's PNG in `out` before the run, so a shot
/// that is not rendered again can never pass the check with an old
/// file. Files of other names are kept.
fn remove_stale(out: &Path, shots: &[&Shot]) -> Result<(), String> {
    for shot in shots {
        let path = out.join(format!("{}.png", shot.id));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Ok(())
}

/// Checks every expected PNG in `dir`: it exists, decodes, and meets
/// its aspect and minimum size. Answers one `(id, width, height)` per
/// shot in table order.
pub fn check(dir: &Path, shots: &[&Shot]) -> Result<Vec<(String, u32, u32)>, String> {
    let mut checked = Vec::new();
    for shot in shots {
        let path = dir.join(format!("{}.png", shot.id));
        let image = image::open(&path)
            .map_err(|error| format!("{}: cannot decode ({error})", path.display()))?;
        let (width, height) = (image.width(), image.height());
        if width < shot.min.0 || height < shot.min.1 {
            return Err(format!(
                "{}: {width}x{height} is below the minimum {}x{}",
                shot.id, shot.min.0, shot.min.1
            ));
        }
        if u64::from(width) * u64::from(shot.aspect.1)
            != u64::from(height) * u64::from(shot.aspect.0)
        {
            return Err(format!(
                "{}: {width}x{height} is not the aspect {}:{}",
                shot.id, shot.aspect.0, shot.aspect.1
            ));
        }
        checked.push((shot.id.to_owned(), width, height));
    }
    Ok(checked)
}

/// Copies each checked PNG from `out` into `to` (overwriting files of
/// the same name, touching nothing else). Answers one report line per
/// copy.
pub fn copy(to: &Path, out: &Path, checked: &[(String, u32, u32)]) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let mut lines = Vec::new();
    for (id, width, height) in checked {
        let from = out.join(format!("{id}.png"));
        let into = to.join(format!("{id}.png"));
        std::fs::copy(&from, &into).map_err(|error| {
            format!(
                "cannot copy {} to {}: {error}",
                from.display(),
                into.display()
            )
        })?;
        lines.push(format!(
            "copied {id}.png ({width}x{height}) to {}",
            into.display()
        ));
    }
    Ok(lines)
}

/// What a test run reports: whether it succeeded, and its standard
/// output (the per-test result lines).
pub struct RunOutput {
    /// The exit status was a success.
    pub success: bool,
    /// Everything the run printed to standard output.
    pub stdout: String,
}

/// The whole command with an injected `spawn` (the unit tests pass a
/// fake one; [`run`] runs `cargo test` and streams its output).
fn run_with(
    root: &Path,
    out: &Path,
    options: &Options,
    mut spawn: impl FnMut(&mut std::process::Command) -> Result<RunOutput, String>,
) -> Result<Vec<String>, String> {
    let shots = selected(&options.only);
    remove_stale(out, &shots)?;
    let mut command = test_command(root, out, &shots);
    let output = spawn(&mut command)?;
    if !output.success {
        return Err("the screenshot tests failed (see the test output above)".to_owned());
    }
    ran_every_test(&output.stdout, &shots)?;
    let checked = check(out, &shots)?;
    let mut lines = vec![format!(
        "{} screenshot(s) rendered into {}:",
        checked.len(),
        out.display()
    )];
    for (id, width, height) in &checked {
        lines.push(format!("  {id}.png {width}x{height}"));
    }
    if let Some(to) = &options.to {
        lines.extend(copy(to, out, &checked)?);
    }
    Ok(lines)
}

/// Runs `command` with its standard output piped: every line is
/// printed as it arrives (a full run takes minutes) and collected for
/// [`ran_every_test`]. Standard error (the build progress) passes
/// straight through.
fn spawn_streaming(command: &mut std::process::Command) -> Result<RunOutput, String> {
    use std::io::BufRead;
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run cargo test: {error}"))?;
    let mut stdout = String::new();
    if let Some(pipe) = child.stdout.take() {
        for line in std::io::BufReader::new(pipe).lines() {
            let line = line.map_err(|error| format!("cargo test output: {error}"))?;
            println!("{line}");
            stdout.push_str(&line);
            stdout.push('\n');
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("cargo test: {error}"))?;
    Ok(RunOutput {
        success: status.success(),
        stdout,
    })
}

/// `cargo xtask screenshots`: runs the screenshot tests, checks every
/// PNG and (with `--to`) copies the shots into the given directory.
pub fn run(options: &Options) -> Result<(), String> {
    let root = crate::util::root();
    let target =
        crate::dist::metadata_target_dir(&mut std::process::Command::new(crate::util::cargo()))?;
    let out = out_dir(&target);
    std::fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let lines = run_with(&root, &out, options, spawn_streaming)?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A synthetic PNG of `size` in a temp directory (removed by the
    /// returned guard).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "larrygram-screenshots-xtask-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the temp directory");
            Self { path }
        }

        fn png(&self, name: &str, size: (u32, u32)) -> PathBuf {
            let path = self.path.join(name);
            image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([1, 2, 3, 255]))
                .save(&path)
                .expect("the synthetic PNG");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The checked list of one shot.
    fn one(id: &'static str) -> Vec<&'static Shot> {
        vec![SHOTS.iter().find(|shot| shot.id == id).expect("the shot")]
    }

    #[test]
    fn parse_args_reads_only_and_to() {
        assert_eq!(
            parse_args(&args(&[])).expect("no arguments"),
            Options {
                only: Vec::new(),
                to: None
            }
        );
        assert_eq!(
            parse_args(&args(&["--only", "about-window", "--only", "pages-tabs"]))
                .expect("two ids"),
            Options {
                only: vec!["about-window".to_owned(), "pages-tabs".to_owned()],
                to: None
            }
        );
        assert_eq!(
            parse_args(&args(&["--to", "/tmp/shots"])).expect("--to"),
            Options {
                only: Vec::new(),
                to: Some(PathBuf::from("/tmp/shots"))
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_arguments() {
        let error = parse_args(&args(&["--only"])).expect_err("a missing value");
        assert!(error.contains("needs a shot id"), "{error}");
        let error = parse_args(&args(&["--to"])).expect_err("a missing value");
        assert!(error.contains("needs a directory"), "{error}");
        let error = parse_args(&args(&["--only", "cli-render"])).expect_err("no such shot");
        assert!(error.contains("is not a shot id"), "{error}");
        let error = parse_args(&args(&["--everything"])).expect_err("no such flag");
        assert!(error.contains("usage:"), "{error}");
    }

    #[test]
    fn test_names_replace_dashes() {
        assert_eq!(test_name("hero-panel-modern"), "hero_panel_modern");
        assert_eq!(test_name("about-window"), "about_window");
    }

    #[test]
    fn check_accepts_shots_that_meet_their_rules() {
        let dir = TempDir::new("check-ok");
        dir.png("about-window.png", (2560, 1600));
        let checked = check(&dir.path, &one("about-window")).expect("the PNG checks");
        assert_eq!(
            checked,
            vec![("about-window".to_owned(), 2560, 1600)],
            "one checked shot"
        );
    }

    #[test]
    fn check_reports_a_missing_png() {
        let dir = TempDir::new("check-missing");
        let error = check(&dir.path, &one("about-window")).expect_err("no PNG");
        assert!(
            error.contains("about-window.png") && error.contains("cannot decode"),
            "{error}"
        );
    }

    #[test]
    fn check_reports_a_png_below_the_minimum() {
        let dir = TempDir::new("check-small");
        dir.png("about-window.png", (1280, 800));
        let error = check(&dir.path, &one("about-window")).expect_err("too small");
        assert!(
            error.contains("about-window") && error.contains("below the minimum 2560x1600"),
            "{error}"
        );
    }

    #[test]
    fn check_reports_a_wrong_aspect() {
        let dir = TempDir::new("check-aspect");
        dir.png("about-window.png", (3000, 1600));
        let error = check(&dir.path, &one("about-window")).expect_err("wrong aspect");
        assert!(
            error.contains("about-window") && error.contains("not the aspect 16:10"),
            "{error}"
        );
    }

    #[test]
    fn copy_overwrites_the_same_names_and_touches_nothing_else() {
        let out = TempDir::new("copy-out");
        out.png("about-window.png", (2560, 1600));
        let to = TempDir::new("copy-to");
        to.png("about-window.png", (2, 2));
        let unrelated = to.png("unrelated.png", (3, 3));
        let lines = copy(
            &to.path,
            &out.path,
            &[("about-window".to_owned(), 2560, 1600)],
        )
        .expect("the copy");
        assert_eq!(lines.len(), 1, "one report line: {lines:?}");
        assert!(
            lines[0].contains("about-window.png (2560x1600)"),
            "{lines:?}"
        );
        let copied = image::open(to.path.join("about-window.png")).expect("the copied PNG");
        assert_eq!(
            (copied.width(), copied.height()),
            (2560, 1600),
            "the same name was overwritten"
        );
        let kept = image::open(&unrelated).expect("the unrelated PNG");
        assert_eq!(
            (kept.width(), kept.height()),
            (3, 3),
            "nothing else was touched"
        );
    }

    /// The stdout of a run that passed the tests `ids`.
    fn passed(ids: &[&str]) -> String {
        ids.iter()
            .map(|id| format!("test {} ... ok\n", test_name(id)))
            .collect()
    }

    /// A fake `spawn` that renders `ids` (writes their synthetic PNGs
    /// at their minimum size into the command's output directory) and
    /// reports them passed.
    fn fake_render(
        ids: &'static [&'static str],
    ) -> impl FnMut(&mut std::process::Command) -> Result<RunOutput, String> {
        move |command| {
            let out = command
                .get_envs()
                .find(|(key, _)| *key == "LARRYGRAM_SCREENSHOTS_DIR")
                .and_then(|(_, value)| value)
                .map(PathBuf::from)
                .expect("the output directory is set");
            for id in ids {
                let shot = SHOTS.iter().find(|shot| shot.id == *id).expect("a shot");
                image::RgbaImage::from_pixel(shot.min.0, shot.min.1, image::Rgba([0, 0, 0, 255]))
                    .save(out.join(format!("{id}.png")))
                    .expect("the synthetic PNG");
            }
            Ok(RunOutput {
                success: true,
                stdout: passed(ids),
            })
        }
    }

    #[test]
    fn run_with_checks_and_copies_and_reports_sizes() {
        let root = TempDir::new("run-root");
        let out = root.path.join("out");
        std::fs::create_dir_all(&out).expect("the out directory");
        let to = root.path.join("site");
        let options = Options {
            only: vec!["about-window".to_owned()],
            to: Some(to.clone()),
        };
        let lines =
            run_with(&root.path, &out, &options, fake_render(&["about-window"])).expect("the run");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("about-window.png 2560x1600")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("copied about-window.png")),
            "{lines:?}"
        );
        assert!(
            to.join("about-window.png").exists(),
            "the PNG was copied to the destination"
        );
    }

    #[test]
    fn run_with_reports_a_failing_test_run() {
        let root = TempDir::new("run-fail");
        let options = Options {
            only: Vec::new(),
            to: None,
        };
        let error = run_with(&root.path, &root.path, &options, |_command| {
            Ok(RunOutput {
                success: false,
                stdout: String::new(),
            })
        })
        .expect_err("the tests failed");
        assert!(error.contains("screenshot tests failed"), "{error}");
    }

    #[test]
    fn run_with_reports_a_missing_png() {
        let root = TempDir::new("run-missing");
        let options = Options {
            only: vec!["about-window".to_owned()],
            to: None,
        };
        let error = run_with(&root.path, &root.path, &options, |_command| {
            Ok(RunOutput {
                success: true,
                stdout: passed(&["about-window"]),
            })
        })
        .expect_err("no PNG was written");
        assert!(error.contains("about-window.png"), "{error}");
    }

    #[test]
    fn run_with_deletes_stale_pngs_before_the_run() {
        // Last run's PNG is valid, but this run renders nothing: the
        // check must fail on the missing file, not pass the old one.
        let root = TempDir::new("run-stale");
        let stale = root.png("about-window.png", (2560, 1600));
        let other = root.png("notes.png", (3, 3));
        let options = Options {
            only: vec!["about-window".to_owned()],
            to: None,
        };
        let error = run_with(&root.path, &root.path, &options, |_command| {
            Ok(RunOutput {
                success: true,
                stdout: passed(&["about-window"]),
            })
        })
        .expect_err("the stale PNG is gone");
        assert!(error.contains("cannot decode"), "{error}");
        assert!(!stale.exists(), "the selected shot's old PNG was deleted");
        assert!(other.exists(), "other files are kept");
    }

    #[test]
    fn run_with_rejects_a_filter_that_ran_no_test() {
        // A renamed test: cargo test succeeds with "running 0 tests".
        let root = TempDir::new("run-zero");
        let options = Options {
            only: vec!["about-window".to_owned()],
            to: None,
        };
        let error = run_with(&root.path, &root.path, &options, |_command| {
            Ok(RunOutput {
                success: true,
                stdout: "running 0 tests\n\ntest result: ok. 0 passed\n".to_owned(),
            })
        })
        .expect_err("zero tests ran");
        assert!(
            error.contains("did not pass about_window"),
            "the error names the test: {error}"
        );
    }

    #[test]
    fn ran_every_test_needs_each_name_exactly() {
        let shots = one("about-window");
        assert!(ran_every_test("test about_window ... ok\n", &shots).is_ok());
        assert!(
            ran_every_test("test about_window_large ... ok\n", &shots).is_err(),
            "a longer name is not the test"
        );
        assert!(
            ran_every_test("test about_window ... FAILED\n", &shots).is_err(),
            "a failed test is not a pass"
        );
    }

    #[test]
    fn test_command_passes_exact_full_names_and_the_output_dir() {
        let shots = selected(&["pages-tabs".to_owned(), "about-window".to_owned()]);
        let command = test_command(Path::new("/root"), Path::new("/out"), &shots);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let tail: Vec<&str> = args
            .iter()
            .skip_while(|arg| *arg != "--")
            .map(String::as_str)
            .collect();
        assert_eq!(
            tail,
            [
                "--",
                "--ignored",
                "--test-threads=1",
                "--exact",
                "pages_tabs",
                "about_window"
            ],
            "{args:?}"
        );
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == "LARRYGRAM_SCREENSHOTS_DIR"
                    && value == Some(std::ffi::OsStr::new("/out"))),
            "the output directory is passed"
        );
        let all = test_command(Path::new("/root"), Path::new("/out"), &selected(&[]));
        assert_eq!(
            all.get_args().filter(|arg| *arg == "--exact").count(),
            1,
            "a full run also filters exactly"
        );
        assert_eq!(
            all.get_args().count(),
            9 + SHOTS.len(),
            "a full run names every shot"
        );
    }

    #[test]
    fn spawn_streaming_collects_the_output() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "echo 'test about_window ... ok'; exit 0"]);
        let output = spawn_streaming(&mut command).expect("sh runs");
        assert!(output.success);
        assert_eq!(output.stdout, "test about_window ... ok\n");
        let mut failing = std::process::Command::new("sh");
        failing.args(["-c", "exit 3"]);
        assert!(!spawn_streaming(&mut failing).expect("sh runs").success);
        let mut missing = std::process::Command::new("/nonexistent/larrygram-no-such-binary");
        assert!(spawn_streaming(&mut missing).is_err());
    }

    #[test]
    fn out_dir_is_under_the_target_dir() {
        assert_eq!(
            out_dir(Path::new("/vol/target/main")),
            PathBuf::from("/vol/target/main/screenshots")
        );
    }

    #[test]
    fn the_test_target_names_the_same_shots() {
        let source = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("crates")
                .join("ui")
                .join("tests")
                .join("screenshots")
                .join("main.rs"),
        )
        .expect("crates/ui/tests/screenshots/main.rs");
        let mut found_ids: Vec<String> = Vec::new();
        let mut rest = source.as_str();
        while let Some(found) = rest.find("id: \"") {
            rest = &rest[found + 5..];
            let end = rest.find('"').expect("a closing quote");
            found_ids.push(rest[..end].to_owned());
        }
        assert_eq!(
            found_ids,
            ids().iter().map(|id| (*id).to_owned()).collect::<Vec<_>>(),
            "the two tables carry the same ids in the same order"
        );
    }
}
