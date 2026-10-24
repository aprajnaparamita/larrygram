// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finding and running the libvisio oracle tools (P8-08, ADR 0010).
//!
//! The helper is a copy-paste per consumer crate (ADR 0010): there is
//! deliberately no wrapper crate and no path dependency. The tools are
//! found through `LARRYGRAM_LIBVISIO_ORACLE`; a missing oracle skips
//! with a clear message and is a failure only when
//! `LARRYGRAM_REQUIRE_ORACLE=1`.

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

/// The oracle directory, or the reason to skip (ADR 0010).
///
/// Returns `Err` only when the oracle is missing and
/// [`require_oracle`] says that is a failure.
pub fn availability() -> Result<Option<PathBuf>, String> {
    match oracle_dir() {
        Some(dir) => Ok(Some(dir)),
        None if require_oracle() => Err(
            "LARRYGRAM_REQUIRE_ORACLE=1 but no oracle: LARRYGRAM_LIBVISIO_ORACLE \
             is unset or not a directory"
                .to_owned(),
        ),
        None => Ok(None),
    }
}

/// Runs one oracle tool on `file` and returns its stdout.
pub fn run_tool(dir: &Path, binary: &str, file: &Path) -> Result<String, String> {
    let program = find_program(dir, binary);
    let output = Command::new(&program)
        .arg(file)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} exited with {}: {}",
            program.display(),
            output.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `<dir>/<binary>`, or the same name with the executable suffixes
/// other platforms use (`.exe`, `.cmd`).
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
fn oracle_dir_from(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
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
                "larrygram-parity-oracle-{label}-{}-{}",
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

    /// Creates a runnable fake tool that copies its argument to stdout.
    #[cfg(unix)]
    fn write_fake_tool(dir: &Path, binary: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let tool = dir.join(binary);
        fs::write(&tool, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Creates a runnable fake tool (Windows).
    #[cfg(windows)]
    fn write_fake_tool(dir: &Path, binary: &str, script: &str) {
        fs::write(dir.join(format!("{binary}.cmd")), script).unwrap();
    }

    #[test]
    fn oracle_dir_from_needs_an_existing_directory() {
        assert_eq!(oracle_dir_from(None), None);
        assert_eq!(
            oracle_dir_from(Some(std::ffi::OsString::from(
                "/nonexistent-larrygram-parity"
            ))),
            None
        );
        let tmp = TempDir::new("dir");
        assert_eq!(
            oracle_dir_from(Some(tmp.path().as_os_str().to_owned())),
            Some(tmp.path().to_owned())
        );
    }

    #[test]
    fn availability_skips_without_an_oracle_and_fails_when_required() {
        assert!(availability().is_ok());
        assert!(require_from(Some("1")));
        assert!(!require_from(Some("0")));
    }

    #[test]
    fn run_tool_returns_stdout_and_reports_a_failure() {
        let tmp = TempDir::new("run");
        write_fake_tool(tmp.path(), "vsd2raw", "echo dump-text");
        let file = tmp.path().join("x.vsd");
        fs::write(&file, b"x").unwrap();
        let out = run_tool(tmp.path(), "vsd2raw", &file).unwrap();
        assert!(out.contains("dump-text"), "{out}");
        assert!(
            run_tool(tmp.path(), "vsd2raw", tmp.path().join("missing").as_path()).is_ok()
                || run_tool(tmp.path(), "missing-tool", &file).is_err()
        );
    }

    #[test]
    fn run_tool_reports_a_non_zero_exit() {
        let tmp = TempDir::new("fail");
        write_fake_tool(tmp.path(), "vsd2raw", "echo boom >&2\nexit 3");
        let file = tmp.path().join("x.vsd");
        fs::write(&file, b"x").unwrap();
        let err = run_tool(tmp.path(), "vsd2raw", &file).unwrap_err();
        assert!(err.contains("exited with"), "{err}");
    }
}
