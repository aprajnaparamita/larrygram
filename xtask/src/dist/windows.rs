//! `cargo xtask dist windows`: builds the Windows release package (task
//! R9).
//!
//! The command runs on 64-bit Windows only (no cross-building, task R9
//! goal 1); anywhere else it refuses with a clear message. It builds
//! the release binaries of `larrygram` and `larrygram-cli` for
//! [`TARGET`] and writes
//! `Larrygram-<version>-windows-x64.zip` under `<target dir>/dist/`:
//!
//! - `Larrygram-<version>-windows-x64/larrygram.exe` and
//!   `larrygram-cli.exe` (both binaries in every package, task R9
//!   decisions);
//! - `Larrygram-<version>-windows-x64/LICENSE`, `NOTICE`,
//!   `ATTRIBUTION.md` and the bundled font licences.
//!
//! The archive is written by the pure-Rust `zip` crate: no PowerShell,
//! `tar` or `7z` binaries (task R9 goal 1). There is no installer and
//! no signing (task R9 decisions); `docs/building/testers.md` covers
//! the SmartScreen prompt. The only command the pipeline runs is
//! `cargo build`, through the [`Sys`] trait, so the unit tests drive
//! it with a fake runner.

use std::path::{Path, PathBuf};

use super::{
    BINARIES, Layout, Sys, WINDOWS_TARGET, check_host, licence_files, package_dir_name,
    package_name, version_from_root,
};
use crate::util::{self, Result};

/// The only supported target triple (task R9 decisions).
pub const TARGET: &str = WINDOWS_TARGET;

/// The `<os>` and `<arch>` in the package name (task R9 decisions).
pub const PACKAGE_OS: &str = "windows";
pub const PACKAGE_ARCH: &str = "x64";

/// The package file extension.
pub const EXT: &str = "zip";

/// The binary file names inside the package (the Windows spellings of
/// [`BINARIES`]).
pub fn binary_names() -> Vec<String> {
    BINARIES.iter().map(|b| format!("{b}.exe")).collect()
}

/// The files the package contains, relative to its top directory,
/// sorted: the two binaries and the licence files (task R9 decisions).
pub fn package_entries() -> Vec<String> {
    let mut entries = binary_names();
    entries.extend(super::licence_entries());
    entries.sort();
    entries
}

/// The files to put in the archive, sorted by entry name:
/// `(entry name, source path, unix mode)`. The binaries come from the
/// build; the licences from the workspace root.
fn package_sources(root: &Path, binaries: &[(String, PathBuf)]) -> Vec<(String, PathBuf, u32)> {
    let mut sources = Vec::new();
    for (name, built) in binaries {
        sources.push((name.clone(), built.clone(), 0o755));
    }
    for (dest, source) in licence_files() {
        sources.push(((*dest).to_owned(), root.join(source), 0o644));
    }
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    sources
}

/// Writes `sources` into the zip `out`, every entry under `dir_name/`
/// (the task R9 package layout). Exes keep their executable bit in the
/// archive metadata.
fn write_zip(out: &Path, dir_name: &str, sources: &[(String, PathBuf, u32)]) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let file =
        std::fs::File::create(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let dir_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    zip.add_directory(format!("{dir_name}/"), dir_opts)
        .map_err(|e| format!("cannot write the {dir_name}/ entry: {e}"))?;
    for (entry, source, mode) in sources {
        let mut from = std::fs::File::open(source)
            .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(*mode);
        zip.start_file(format!("{dir_name}/{entry}"), opts)
            .map_err(|e| format!("cannot write the {entry} entry: {e}"))?;
        std::io::copy(&mut from, &mut zip)
            .map_err(|e| format!("cannot copy {entry} into the archive: {e}"))?;
    }
    zip.finish()
        .map_err(|e| format!("cannot finish {}: {e}", out.display()))?;
    Ok(())
}

/// Reads back the names and contents of a written archive (the tests
/// pin the package contents against this, task R9 tests).
#[cfg(test)]
fn read_zip(path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("cannot read entry {i}: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("cannot read entry {i} name: {e}"))?
            .to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes)
            .map_err(|e| format!("cannot read entry {name}: {e}"))?;
        out.push((name, bytes));
    }
    Ok(out)
}

/// The whole build through `sys`: release binaries and the `.zip`.
/// Returns the package path. `os` and `arch` are the host's
/// `std::env::consts` values (see [`check_host`]); the tests pass
/// their own and use a fake [`Sys`], so the whole pipeline runs on
/// hand-built inputs (task R9 tests).
pub fn dist_windows(sys: &mut dyn Sys, layout: &Layout, os: &str, arch: &str) -> Result<PathBuf> {
    check_host(super::Platform::Windows, os, arch)?;
    let root = &layout.root;
    let dist = &layout.dist;
    std::fs::create_dir_all(dist).map_err(|e| format!("cannot create {}: {e}", dist.display()))?;

    let version = version_from_root(root)?;
    println!("dist windows: building Larrygram {version} for {TARGET}");

    // 1. The release binaries (AGENTS.md section 2: no CARGO_TARGET_DIR).
    sys.run(
        &util::cargo(),
        &[
            "build",
            "--release",
            "-p",
            "larrygram",
            "-p",
            "larrygram-cli",
            "--target",
            TARGET,
        ],
    )?;
    let mut binaries = Vec::new();
    for name in binary_names() {
        let built = layout.binary(TARGET, &name);
        if !built.is_file() {
            return Err(format!(
                "cargo built no release binary at {}",
                built.display()
            ));
        }
        binaries.push((name, built));
    }

    // 2. The archive, every entry under the package directory.
    let dir_name = package_dir_name(&version, PACKAGE_OS, PACKAGE_ARCH)?;
    let sources = package_sources(root, &binaries);
    let expected = package_entries();
    let names: Vec<String> = sources.iter().map(|(n, _, _)| n.clone()).collect();
    if names != expected {
        return Err(format!(
            "package layout {names:?} does not match the expected {expected:?}"
        ));
    }
    let package = layout.package(&package_name(&version, PACKAGE_OS, PACKAGE_ARCH, EXT)?);
    write_zip(&package, &dir_name, &sources)?;

    println!("dist windows: {}", package.display());
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dist::fakes::{FakeSys, TempDir};

    /// A temp workspace with the Cargo.toml and the licence sources
    /// the package copies (task R9 tests).
    fn workspace() -> TempDir {
        let tmp = TempDir::new("dist-windows");
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-alpha.1\"\n",
        )
        .unwrap();
        for (_, source) in licence_files() {
            let path = root.join(source);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("licence text of {source}")).unwrap();
        }
        tmp
    }

    /// The binary and entry lists are the two exes plus the licences,
    /// sorted (task R9 decisions).
    #[test]
    fn package_entries_hold_both_binaries_and_the_licences() {
        assert_eq!(binary_names(), vec!["larrygram.exe", "larrygram-cli.exe"]);
        let entries = package_entries();
        assert!(entries.contains(&"larrygram.exe".to_owned()));
        assert!(entries.contains(&"larrygram-cli.exe".to_owned()));
        for dest in super::super::licence_entries() {
            assert!(entries.contains(&dest), "missing {dest}");
        }
        assert_eq!(entries.len(), 2 + licence_files().len());
        let mut sorted = entries.clone();
        sorted.sort();
        assert_eq!(entries, sorted);
    }

    /// The whole pipeline on hand-built inputs: the package name, the
    /// archive contents read back, and the one tool call (task R9
    /// tests).
    #[test]
    fn dist_windows_pipeline_writes_the_zip() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let bin_dir = layout.release_dir(TARGET);
        let mut sys = FakeSys::new(&bin_dir, &["larrygram.exe", "larrygram-cli.exe"]);
        let package = dist_windows(&mut sys, &layout, "windows", "x86_64").unwrap();

        assert_eq!(
            package,
            layout.dist.join("Larrygram-1.0.0-alpha.1-windows-x64.zip")
        );
        assert!(package.is_file());

        let dir = "Larrygram-1.0.0-alpha.1-windows-x64";
        let read = read_zip(&package).unwrap();
        let names: Vec<String> = read.iter().map(|(n, _)| n.clone()).collect();
        let mut expect: Vec<String> = package_entries()
            .iter()
            .map(|e| format!("{dir}/{e}"))
            .collect();
        expect.push(format!("{dir}/"));
        expect.sort();
        let mut got = names.clone();
        got.sort();
        assert_eq!(got, expect, "{got:?}");
        for (name, bytes) in &read {
            if name.ends_with("larrygram.exe") {
                assert_eq!(bytes, b"fake binary larrygram.exe", "{name}");
            } else if name.ends_with("LICENSE") {
                assert_eq!(bytes, b"licence text of LICENSE".as_slice(), "{name}");
            }
        }
        assert_eq!(sys.calls.len(), 1, "{:?}", sys.calls);
        assert!(
            sys.calls[0].contains(
                "build --release -p larrygram -p larrygram-cli --target x86_64-pc-windows-msvc"
            ),
            "{:?}",
            sys.calls
        );
    }

    /// The pipeline refuses a non-Windows host before anything runs.
    #[test]
    fn dist_windows_rejects_the_wrong_host() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram.exe"]);
        let err = dist_windows(&mut sys, &layout, "linux", "x86_64").unwrap_err();
        assert!(err.contains("64-bit Windows only"), "{err}");
        assert!(sys.calls.is_empty(), "{:?}", sys.calls);
    }

    /// A missing built binary is a clean error naming the path.
    #[test]
    fn dist_windows_reports_a_missing_binary() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        // The fake writes the two exes; the pipeline must still find them.
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram.exe"]);
        let err = dist_windows(&mut sys, &layout, "windows", "x86_64").unwrap_err();
        assert!(err.contains("cargo built no release binary"), "{err}");
        assert!(err.contains("larrygram-cli.exe"), "{err}");
    }

    /// A missing licence source is a clean error.
    #[test]
    fn dist_windows_reports_a_missing_licence() {
        let tmp = TempDir::new("dist-windows-licence");
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-alpha.1\"\n",
        )
        .unwrap();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(
            &layout.release_dir(TARGET),
            &["larrygram.exe", "larrygram-cli.exe"],
        );
        let err = dist_windows(&mut sys, &layout, "windows", "x86_64").unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
    }

    /// `read_zip` reports a missing file cleanly.
    #[test]
    fn read_zip_reports_a_missing_file() {
        let tmp = TempDir::new("dist-zip-missing");
        let err = read_zip(&tmp.join("nope.zip")).unwrap_err();
        assert!(err.contains("cannot open"), "{err}");
    }
}
