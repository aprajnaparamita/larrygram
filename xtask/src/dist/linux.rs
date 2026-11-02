//! `cargo xtask dist linux`: builds the Linux release package (task
//! R9).
//!
//! The command runs on 64-bit x86 Linux only (no cross-building, task
//! R9 goal 1); anywhere else it refuses with a clear message. It
//! builds the release binaries of `larrygram` and `larrygram-cli` for
//! [`TARGET`] and writes
//! `Larrygram-<version>-linux-x64.tar.gz` under `<target dir>/dist/`:
//!
//! - `Larrygram-<version>-linux-x64/larrygram` and `larrygram-cli`
//!   (both binaries in every package, task R9 decisions);
//! - `larrygram.desktop` — the desktop entry from
//!   `apps/larrygram/linux/larrygram.desktop` (task R9 goal 3);
//! - `larrygram.png` — the 256 px icon rasterised from the brand
//!   `icon.svg` at package time (task R9 goal 3);
//! - `INSTALL.txt` — where to copy the desktop entry and the icon
//!   (task R9 goal 3);
//! - `LICENSE`, `NOTICE`, `ATTRIBUTION.md` and the bundled font
//!   licences.
//!
//! The archive is written by the pure-Rust `tar` and `flate2` crates:
//! no `tar` or `7z` binaries (task R9 goal 1). There is no installer
//! (`.deb`, AppImage, Flatpak) and no signing (task R9 decisions).
//! The only command the pipeline runs is `cargo build`, through the
//! [`Sys`] trait, so the unit tests drive it with a fake runner.

use std::path::{Path, PathBuf};

use super::{
    BINARIES, ICON_SVG, LINUX_TARGET, Layout, Sys, check_host, licence_entries, licence_files,
    package_dir_name, package_name, render_png, version_from_root,
};
use crate::util::{self, Result};

/// The only supported target triple (task R9 decisions).
pub const TARGET: &str = LINUX_TARGET;

/// The `<os>` and `<arch>` in the package name (task R9 decisions).
pub const PACKAGE_OS: &str = "linux";
pub const PACKAGE_ARCH: &str = "x64";

/// The package file extension.
pub const EXT: &str = "tar.gz";

/// The committed desktop entry, relative to the workspace root (task
/// R9 goal 3).
pub const DESKTOP_FILE: &str = "apps/larrygram/linux/larrygram.desktop";

/// The icon name inside the package: `larrygram.png`, 256 px, for the
/// `Icon=larrygram` line of the desktop entry (task R9 goal 3).
pub const ICON_NAME: &str = "larrygram.png";

/// The icon size in pixels (task R9 goal 3).
pub const ICON_PX: u32 = 256;

/// The `INSTALL.txt` shipped in the package (task R9 goal 3): our own
/// words, where to copy the desktop entry and the icon.
pub const INSTALL_TXT: &str = "\
Larrygram for Linux
===================

This archive is not an installer. To install Larrygram for one user:

1. Copy `larrygram` and `larrygram-cli` somewhere on your PATH, for
   example ~/.local/bin (tar keeps them executable).
2. Copy `larrygram.desktop` to ~/.local/share/applications/.
3. Copy `larrygram.png` to ~/.local/share/icons/hicolor/256x256/apps/.

For a system-wide install use /usr/bin, /usr/share/applications and
/usr/share/icons/hicolor/256x256/apps instead.

The desktop entry starts `larrygram` from your PATH and registers the
Visio drawing types (application/vnd.ms-visio.drawing.main+xml and
application/vnd.visio).

The binaries are unsigned and built for x86-64 Linux on the Ubuntu
22.04 runner, so they need glibc 2.35 or newer. See
docs/building/testers.md in the repository for how to report a bug.

The bundled fonts Carlito and Selawik are under the SIL Open Font
Licence; the licence texts are in fonts/ next to this file. LICENSE,
NOTICE and ATTRIBUTION.md describe the rest of the package.
";

/// One file in the package: its entry name (relative to the top
/// directory), its unix mode and where its bytes come from.
pub struct Entry {
    /// The entry name inside the package directory.
    pub name: String,
    /// The unix mode the archive records.
    pub mode: u32,
    /// The bytes to write.
    pub source: Source,
}

/// Where a package entry's bytes come from.
pub enum Source {
    /// A file on disk (a built binary, a licence, the desktop entry).
    File(PathBuf),
    /// Bytes generated at package time (the icon, `INSTALL.txt`).
    Bytes(Vec<u8>),
}

/// The files the package contains, relative to its top directory,
/// sorted: the two binaries, the desktop entry, the icon,
/// `INSTALL.txt` and the licence files (task R9 decisions and goal 3).
pub fn package_entries() -> Vec<String> {
    let mut entries: Vec<String> = BINARIES.iter().map(|b| (*b).to_owned()).collect();
    entries.extend([
        "INSTALL.txt".to_owned(),
        "larrygram.desktop".to_owned(),
        ICON_NAME.to_owned(),
    ]);
    entries.extend(licence_entries());
    entries.sort();
    entries
}

/// The package contents with their bytes resolved: the binaries from
/// the build, the desktop entry from the workspace, the icon and
/// `INSTALL.txt` generated, the licences from the workspace. Sorted by
/// entry name.
pub fn package_sources(root: &Path, layout: &Layout, icon_svg: &str) -> Result<Vec<Entry>> {
    let mut sources = Vec::new();
    for binary in BINARIES {
        sources.push(Entry {
            name: (*binary).to_owned(),
            mode: 0o755,
            source: Source::File(layout.binary(TARGET, binary)),
        });
    }
    sources.push(Entry {
        name: "larrygram.desktop".to_owned(),
        mode: 0o644,
        source: Source::File(root.join(DESKTOP_FILE)),
    });
    sources.push(Entry {
        name: ICON_NAME.to_owned(),
        mode: 0o644,
        source: Source::Bytes(render_png(icon_svg, ICON_PX)?),
    });
    sources.push(Entry {
        name: "INSTALL.txt".to_owned(),
        mode: 0o644,
        source: Source::Bytes(INSTALL_TXT.as_bytes().to_vec()),
    });
    for (dest, source) in licence_files() {
        sources.push(Entry {
            name: (*dest).to_owned(),
            mode: 0o644,
            source: Source::File(root.join(source)),
        });
    }
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sources)
}

/// Writes `sources` into the gzipped tar `out`, every entry under
/// `dir_name/` (the task R9 package layout).
fn write_tar_gz(out: &Path, dir_name: &str, sources: &[Entry]) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let file =
        std::fs::File::create(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    for entry in sources {
        let path = format!("{dir_name}/{}", entry.name);
        let mut header = tar::Header::new_gnu();
        header.set_mode(entry.mode);
        header.set_entry_type(tar::EntryType::Regular);
        match &entry.source {
            Source::File(from) => {
                let mut f = std::fs::File::open(from)
                    .map_err(|e| format!("cannot read {}: {e}", from.display()))?;
                let size = f
                    .metadata()
                    .map_err(|e| format!("cannot stat {}: {e}", from.display()))?
                    .len();
                header.set_size(size);
                tar.append_data(&mut header, &path, &mut f)
                    .map_err(|e| format!("cannot write {path}: {e}"))?;
            }
            Source::Bytes(bytes) => {
                header.set_size(bytes.len() as u64);
                tar.append_data(&mut header, &path, bytes.as_slice())
                    .map_err(|e| format!("cannot write {path}: {e}"))?;
            }
        }
    }
    let gz = tar
        .into_inner()
        .map_err(|e| format!("cannot finish {}: {e}", out.display()))?;
    gz.finish()
        .map_err(|e| format!("cannot finish {}: {e}", out.display()))?;
    Ok(())
}

/// Reads back the entries of a written archive: `(name, mode,
/// contents)` (the tests pin the package contents against this, task
/// R9 tests).
#[cfg(test)]
fn read_tar_gz(path: &Path) -> Result<Vec<(String, u32, Vec<u8>)>> {
    use std::io::Read;

    let file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut out = Vec::new();
    let entries = archive
        .entries()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("cannot read an entry: {e}"))?;
        let name = entry
            .path()
            .map_err(|e| format!("cannot read an entry path: {e}"))?
            .to_string_lossy()
            .into_owned();
        let mode = entry.header().mode().unwrap_or(0);
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read {name}: {e}"))?;
        out.push((name, mode, bytes));
    }
    Ok(out)
}

/// The whole build through `sys`: release binaries and the
/// `.tar.gz`. Returns the package path. `os` and `arch` are the
/// host's `std::env::consts` values (see [`check_host`]); the tests
/// pass their own and use a fake [`Sys`], so the whole pipeline runs
/// on hand-built inputs (task R9 tests).
pub fn dist_linux(sys: &mut dyn Sys, layout: &Layout, os: &str, arch: &str) -> Result<PathBuf> {
    check_host(super::Platform::Linux, os, arch)?;
    let root = &layout.root;
    let dist = &layout.dist;
    std::fs::create_dir_all(dist).map_err(|e| format!("cannot create {}: {e}", dist.display()))?;

    let version = version_from_root(root)?;
    println!("dist linux: building Larrygram {version} for {TARGET}");

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
    for binary in BINARIES {
        let built = layout.binary(TARGET, binary);
        if !built.is_file() {
            return Err(format!(
                "cargo built no release binary at {}",
                built.display()
            ));
        }
    }

    // 2. The sources: the desktop entry, the icon and INSTALL.txt
    // (task R9 goal 3) plus the binaries and the licences.
    let svg_path = root.join(ICON_SVG);
    let icon_svg = std::fs::read_to_string(&svg_path)
        .map_err(|e| format!("cannot read {}: {e}", svg_path.display()))?;
    let sources = package_sources(root, layout, &icon_svg)?;
    let names: Vec<String> = sources.iter().map(|e| e.name.clone()).collect();
    let expected = package_entries();
    if names != expected {
        return Err(format!(
            "package layout {names:?} does not match the expected {expected:?}"
        ));
    }

    // 3. The archive.
    let dir_name = package_dir_name(&version, PACKAGE_OS, PACKAGE_ARCH)?;
    let package = layout.package(&package_name(&version, PACKAGE_OS, PACKAGE_ARCH, EXT)?);
    write_tar_gz(&package, &dir_name, &sources)?;

    println!("dist linux: {}", package.display());
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dist::fakes::{FakeSys, TempDir};

    /// A temp workspace with the Cargo.toml, the licence sources, the
    /// icon source and the desktop entry the package copies (task R9
    /// tests).
    fn workspace() -> TempDir {
        let tmp = TempDir::new("dist-linux");
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-alpha.1\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/ui/assets/brand")).unwrap();
        std::fs::write(
            root.join(ICON_SVG),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        let desktop = root.join(DESKTOP_FILE);
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(&desktop, "[Desktop Entry]\n").unwrap();
        for (_, source) in licence_files() {
            let path = root.join(source);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("licence text of {source}")).unwrap();
        }
        tmp
    }

    /// The entry list is the two binaries, the desktop entry, the
    /// icon, `INSTALL.txt` and the licences, sorted (task R9
    /// decisions, goal 3).
    #[test]
    fn package_entries_hold_the_desktop_files() {
        let entries = package_entries();
        assert!(entries.contains(&"larrygram".to_owned()));
        assert!(entries.contains(&"larrygram-cli".to_owned()));
        assert!(entries.contains(&"larrygram.desktop".to_owned()));
        assert!(entries.contains(&"larrygram.png".to_owned()));
        assert!(entries.contains(&"INSTALL.txt".to_owned()));
        for dest in licence_entries() {
            assert!(entries.contains(&dest), "missing {dest}");
        }
        assert_eq!(entries.len(), BINARIES.len() + 3 + licence_files().len());
        let mut sorted = entries.clone();
        sorted.sort();
        assert_eq!(entries, sorted);
    }

    /// The committed desktop entry exists in the real workspace and
    /// registers the Visio MIME types (task R9 goal 3).
    #[test]
    fn the_desktop_entry_exists_and_names_the_visio_types() {
        let root = util::root();
        let text =
            std::fs::read_to_string(root.join(DESKTOP_FILE)).expect("the desktop entry reads");
        assert!(text.contains("[Desktop Entry]"), "{text}");
        assert!(
            text.contains(
                "MimeType=application/vnd.ms-visio.drawing.main+xml;application/vnd.visio;"
            ),
            "{text}"
        );
        assert!(text.contains("Exec=larrygram"), "{text}");
        assert!(text.contains("Icon=larrygram"), "{text}");
    }

    /// `INSTALL.txt` says where to copy the desktop entry and the
    /// icon (task R9 goal 3).
    #[test]
    fn install_txt_names_the_install_directories() {
        assert!(
            INSTALL_TXT.contains("~/.local/share/applications/"),
            "desktop"
        );
        assert!(
            INSTALL_TXT.contains("~/.local/share/icons/hicolor/256x256/apps/"),
            "icon"
        );
        assert!(INSTALL_TXT.contains("~/.local/bin"), "binaries");
        assert!(
            INSTALL_TXT.contains("docs/building/testers.md"),
            "reporting"
        );
    }

    /// The whole pipeline on hand-built inputs: the package name, the
    /// archive contents and modes read back, and the one tool call
    /// (task R9 tests).
    #[test]
    fn dist_linux_pipeline_writes_the_tar_gz() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram", "larrygram-cli"]);
        let package = dist_linux(&mut sys, &layout, "linux", "x86_64").unwrap();

        assert_eq!(
            package,
            layout.dist.join("Larrygram-1.0.0-alpha.1-linux-x64.tar.gz")
        );
        assert!(package.is_file());

        let dir = "Larrygram-1.0.0-alpha.1-linux-x64";
        let read = read_tar_gz(&package).unwrap();
        let mut names: Vec<String> = read.iter().map(|(n, _, _)| n.clone()).collect();
        names.sort();
        let mut expect: Vec<String> = package_entries()
            .iter()
            .map(|e| format!("{dir}/{e}"))
            .collect();
        expect.sort();
        assert_eq!(names, expect, "{names:?}");
        for (name, mode, bytes) in &read {
            if name.ends_with("larrygram-cli") {
                assert_eq!(bytes, b"fake binary larrygram-cli", "{name}");
                assert_eq!(mode & 0o777, 0o755, "{name} must be executable");
            } else if name.ends_with("larrygram.desktop") {
                assert_eq!(bytes, b"[Desktop Entry]\n".as_slice(), "{name}");
                assert_eq!(mode & 0o777, 0o644, "{name}");
            } else if name.ends_with("larrygram.png") {
                assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{name} is not a PNG");
                assert_eq!(mode & 0o777, 0o644, "{name}");
            } else if name.ends_with("INSTALL.txt") {
                assert!(
                    String::from_utf8_lossy(bytes).contains("~/.local/bin"),
                    "{name}"
                );
            } else if name.ends_with("LICENSE") {
                assert_eq!(bytes, b"licence text of LICENSE".as_slice(), "{name}");
            }
        }
        assert_eq!(sys.calls.len(), 1, "{:?}", sys.calls);
        assert!(
            sys.calls[0].contains(
                "build --release -p larrygram -p larrygram-cli --target x86_64-unknown-linux-gnu"
            ),
            "{:?}",
            sys.calls
        );
    }

    /// The pipeline refuses a non-Linux host before anything runs.
    #[test]
    fn dist_linux_rejects_the_wrong_host() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram"]);
        let err = dist_linux(&mut sys, &layout, "macos", "aarch64").unwrap_err();
        assert!(err.contains("64-bit x86 Linux only"), "{err}");
        assert!(sys.calls.is_empty(), "{:?}", sys.calls);
    }

    /// A missing built binary is a clean error naming the path.
    #[test]
    fn dist_linux_reports_a_missing_binary() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram"]);
        let err = dist_linux(&mut sys, &layout, "linux", "x86_64").unwrap_err();
        assert!(err.contains("cargo built no release binary"), "{err}");
        assert!(err.contains("larrygram-cli"), "{err}");
    }

    /// A missing desktop entry is a clean error.
    #[test]
    fn dist_linux_reports_a_missing_desktop_entry() {
        let tmp = workspace();
        std::fs::remove_file(tmp.join(DESKTOP_FILE)).unwrap();
        let layout = Layout::under(tmp.path());
        let mut sys = FakeSys::new(&layout.release_dir(TARGET), &["larrygram", "larrygram-cli"]);
        let err = dist_linux(&mut sys, &layout, "linux", "x86_64").unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
    }

    /// `read_tar_gz` reports a missing file cleanly.
    #[test]
    fn read_tar_gz_reports_a_missing_file() {
        let tmp = TempDir::new("dist-tar-missing");
        let err = read_tar_gz(&tmp.join("nope.tar.gz")).unwrap_err();
        assert!(err.contains("cannot open"), "{err}");
    }
}
