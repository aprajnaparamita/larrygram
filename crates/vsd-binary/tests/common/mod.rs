// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `larrygram-vsd-binary` integration tests: the
//! libvisio oracle smoke helper (P1-07, ADR 0010) and the compound-file
//! writer the hand-authored fixtures are built with (P8-10, ADR 0005).
//!
//! The oracle helper is a copy-paste per consumer crate; there is
//! deliberately no wrapper crate and no path dependency (ADR 0010).

#![allow(dead_code)] // each test binary uses a different subset of the helpers

pub mod cfb;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The oracle tools directory when `LARRYGRAM_LIBVISIO_ORACLE` names an
/// existing directory; `None` when it is unset, empty or missing.
pub fn oracle_dir() -> Option<PathBuf> {
    oracle_dir_from(std::env::var_os("LARRYGRAM_LIBVISIO_ORACLE"))
}

/// Whether `LARRYGRAM_REQUIRE_ORACLE=1` turns a missing oracle into a
/// test failure instead of a skip (set in CI on Linux and macOS).
pub fn require_oracle() -> bool {
    require_from(std::env::var("LARRYGRAM_REQUIRE_ORACLE").as_deref().ok())
}

/// Runs the smoke check for one oracle tool: `<oracle>/<binary> --help`
/// must mention the tool's own name.
///
/// With no oracle directory the check skips (prints a message and
/// returns `Ok`) unless [`require_oracle`] says a missing oracle is a
/// failure.
pub fn smoke(binary: &str) -> Result<(), String> {
    smoke_with(oracle_dir().as_deref(), require_oracle(), binary)
}

/// The logic behind [`smoke`], with the environment passed in so tests
/// can drive every branch without touching process-global state.
pub fn smoke_with(oracle: Option<&Path>, require: bool, binary: &str) -> Result<(), String> {
    let Some(dir) = oracle else {
        if require {
            return Err(
                "LARRYGRAM_REQUIRE_ORACLE=1 but no oracle: LARRYGRAM_LIBVISIO_ORACLE \
                 is unset or not a directory"
                    .to_owned(),
            );
        }
        eprintln!(
            "skipping `{binary} --help`: no oracle; set LARRYGRAM_LIBVISIO_ORACLE (ADR 0010)"
        );
        return Ok(());
    };
    let output = run_help(dir, binary)?;
    if output.contains(binary) {
        Ok(())
    } else {
        Err(format!(
            "`{binary} --help` ran but did not mention `{binary}`:\n{output}"
        ))
    }
}

/// Runs `<dir>/<binary> --help` and returns stdout followed by stderr.
///
/// The exit code is deliberately not checked: some tools print their
/// help and exit non-zero, and the output is what identifies the tool.
pub fn run_help(dir: &Path, binary: &str) -> Result<String, String> {
    let program = find_program(dir, binary);
    let out = Command::new(&program)
        .arg("--help")
        .output()
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}

/// `<dir>/<binary>`, or the same name with the executable suffixes
/// other platforms use: `.exe` for a real Windows build, `.cmd` for the
/// fake tool the unit tests create there.
fn find_program(dir: &Path, binary: &str) -> PathBuf {
    let plain = dir.join(binary);
    if plain.is_file() {
        return plain;
    }
    for suffix in [".exe", ".cmd"] {
        let with_suffix = dir.join(format!("{binary}{suffix}"));
        if with_suffix.is_file() {
            return with_suffix;
        }
    }
    plain
}

/// [`require_oracle`] with an explicit value for the environment
/// variable, so tests do not mutate process state.
fn require_from(var: Option<&str>) -> bool {
    var == Some("1")
}

/// [`oracle_dir`] with an explicit value for the environment variable.
fn oracle_dir_from(var: Option<OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(var?);
    dir.is_dir().then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    /// A unique temp directory, removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "larrygram-oracle-smoke-{label}-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Creates a runnable fake tool whose `--help` prints `help_line`.
    #[cfg(unix)]
    fn write_fake_tool(dir: &Path, binary: &str, help_line: &str) {
        use std::os::unix::fs::PermissionsExt;
        let tool = dir.join(binary);
        fs::write(&tool, format!("#!/bin/sh\necho '{help_line}'\n")).unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Creates a runnable fake tool whose `--help` prints `help_line`.
    #[cfg(windows)]
    fn write_fake_tool(dir: &Path, binary: &str, help_line: &str) {
        fs::write(
            dir.join(format!("{binary}.cmd")),
            format!("@echo {help_line}\r\n"),
        )
        .unwrap();
    }

    #[test]
    fn oracle_dir_from_needs_an_existing_directory() {
        assert_eq!(oracle_dir_from(None), None);
        assert_eq!(oracle_dir_from(Some(OsString::new())), None);
        assert_eq!(
            oracle_dir_from(Some(OsString::from("/nonexistent-larrygram-oracle-dir"))),
            None
        );
        let tmp = TempDir::new("oracle-dir");
        assert_eq!(
            oracle_dir_from(Some(tmp.path().as_os_str().to_owned())),
            Some(tmp.path().to_owned())
        );
    }

    #[test]
    fn oracle_dir_is_unset_or_points_at_a_directory() {
        if let Some(dir) = oracle_dir() {
            assert!(dir.is_dir());
        }
    }

    #[test]
    fn require_oracle_only_accepts_one() {
        assert!(require_from(Some("1")));
        assert!(!require_from(Some("0")));
        assert!(!require_from(Some("")));
        assert!(!require_from(Some("true")));
        assert!(!require_from(None));
        // The env-reading wrapper agrees with the value it read.
        let env = std::env::var("LARRYGRAM_REQUIRE_ORACLE").ok();
        assert_eq!(require_oracle(), require_from(env.as_deref()));
    }

    #[test]
    fn skips_without_an_oracle() {
        assert!(smoke_with(None, false, "vsd2raw").is_ok());
    }

    #[test]
    fn fails_when_the_oracle_is_required_but_missing() {
        let err = smoke_with(None, true, "vsd2raw").unwrap_err();
        assert!(err.contains("LARRYGRAM_REQUIRE_ORACLE=1"), "{err}");
    }

    #[test]
    fn runs_the_tool_and_checks_the_output_mentions_it() {
        let tmp = TempDir::new("fake-ok");
        write_fake_tool(tmp.path(), "vsd2raw", "Usage: vsd2raw [options] file.vsd");
        assert!(smoke_with(Some(tmp.path()), false, "vsd2raw").is_ok());
        assert!(smoke_with(Some(tmp.path()), true, "vsd2raw").is_ok());
    }

    #[test]
    fn rejects_output_that_does_not_mention_the_tool() {
        let tmp = TempDir::new("fake-bad");
        write_fake_tool(tmp.path(), "vsd2raw", "Usage: something else");
        let err = smoke_with(Some(tmp.path()), false, "vsd2raw").unwrap_err();
        assert!(err.contains("did not mention"), "{err}");
    }

    #[test]
    fn reports_a_tool_that_cannot_run() {
        let tmp = TempDir::new("empty");
        let err = smoke_with(Some(tmp.path()), false, "vsd2raw").unwrap_err();
        assert!(err.contains("cannot run"), "{err}");
    }

    #[test]
    fn find_program_prefers_the_plain_name_then_a_known_suffix() {
        let tmp = TempDir::new("find");
        assert_eq!(
            find_program(tmp.path(), "vsd2raw"),
            tmp.path().join("vsd2raw"),
            "missing tool: the plain name is reported"
        );
        write_fake_tool(tmp.path(), "vsd2raw", "Usage: vsd2raw");
        #[cfg(unix)]
        assert_eq!(
            find_program(tmp.path(), "vsd2raw"),
            tmp.path().join("vsd2raw")
        );
        #[cfg(windows)]
        assert_eq!(
            find_program(tmp.path(), "vsd2raw"),
            tmp.path().join("vsd2raw.cmd")
        );
        fs::write(tmp.path().join("vss2raw.exe"), b"MZ").unwrap();
        assert_eq!(
            find_program(tmp.path(), "vss2raw"),
            tmp.path().join("vss2raw.exe")
        );
    }
}
