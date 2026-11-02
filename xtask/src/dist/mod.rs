//! `cargo xtask dist <mac|windows|linux>`: builds the release
//! packages for the three desktop platforms (tasks R7 and R9).
//!
//! Each platform builds its release binaries for its one supported
//! target and writes its package under `<target dir>/dist/` (task R9
//! goal 1), named `Larrygram-<version>-<os>-<arch>.<ext>` (task R9
//! decisions):
//!
//! | Command | Target | Package |
//! |---|---|---|
//! | `dist mac` | `aarch64-apple-darwin` | `Larrygram-<version>-macos-arm64.dmg` |
//! | `dist windows` | `x86_64-pc-windows-msvc` | `Larrygram-<version>-windows-x64.zip` |
//! | `dist linux` | `x86_64-unknown-linux-gnu` | `Larrygram-<version>-linux-x64.tar.gz` |
//!
//! Every package carries both binaries (`larrygram` and
//! `larrygram-cli`) and the licence files from [`licence_files`] (task
//! R9 decisions). There is no cross-building: every command refuses to
//! run on the wrong host OS with a clear message ([`check_host`]).
//!
//! The platform pipelines live in [`mac`], [`windows`] and [`linux`].
//! They share this module: the [`Sys`] command runner, the [`Layout`]
//! of the source and output directories, the workspace version, the
//! licence list and the icon rasteriser. Every command runs through
//! [`Sys`], so the unit tests drive each pipeline with a fake runner
//! and hand-built inputs.

pub mod linux;
#[cfg(unix)]
pub mod mac;
pub mod windows;

#[cfg(test)]
pub(crate) mod fakes;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{self, Result};

/// The platform `cargo xtask dist` builds for (task R9 decisions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// `dist mac`: the macOS `.app` and `.dmg` (task R7).
    Mac,
    /// `dist windows`: the Windows `.zip` (task R9).
    Windows,
    /// `dist linux`: the Linux `.tar.gz` (task R9).
    Linux,
}

impl Platform {
    /// The command-line name (`mac`, `windows`, `linux`).
    pub fn name(self) -> &'static str {
        match self {
            Platform::Mac => "mac",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }
}

/// `cargo xtask dist` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The platform to build.
    pub platform: Platform,
}

/// Parses `cargo xtask dist` arguments. Exactly one platform argument
/// (`mac`, `windows` or `linux`) is accepted; everything else is a
/// clean error (C3).
pub fn parse_args(args: &[String]) -> Result<Options> {
    let [platform, rest @ ..] = args else {
        return Err("dist needs a platform: `cargo xtask dist <mac|windows|linux>`".to_owned());
    };
    if !rest.is_empty() {
        return Err(format!(
            "dist takes one platform argument (got `{}`)",
            args.join(" ")
        ));
    }
    match platform.as_str() {
        "mac" => Ok(Options {
            platform: Platform::Mac,
        }),
        "windows" => Ok(Options {
            platform: Platform::Windows,
        }),
        "linux" => Ok(Options {
            platform: Platform::Linux,
        }),
        other => Err(format!(
            "unknown dist platform `{other}`; platforms: mac, windows, linux"
        )),
    }
}

/// The system side effects the build needs: running a tool while
/// inheriting stdio. The production implementation is [`StdSys`]; the
/// tests use a fake that records the calls and fabricates the
/// outputs.
pub trait Sys {
    /// Runs `program args`, inheriting stdio. Fails naming the command
    /// when the command fails.
    fn run(&mut self, program: &str, args: &[&str]) -> Result<()>;
}

/// The real [`Sys`]: every command runs in the workspace root.
pub struct StdSys;

impl Sys for StdSys {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<()> {
        util::run(Command::new(program).args(args))
    }
}

/// The directories a build works in.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The workspace root: the sources and the cargo build live here.
    pub root: PathBuf,
    /// The cargo target directory the release binaries come from.
    pub target: PathBuf,
    /// The output directory, `<target dir>/dist` (task R9 goal 1).
    pub dist: PathBuf,
}

impl Layout {
    /// The layout of a real run: the workspace root, the cargo target
    /// directory as `cargo metadata` reports it, and the `dist`
    /// directory under it. Asking cargo honours every way the target
    /// directory can be set: `CARGO_TARGET_DIR`,
    /// `CARGO_BUILD_TARGET_DIR` and `build.target-dir` (task R10
    /// item 1).
    pub fn real() -> Result<Self> {
        let target = metadata_target_dir(&mut Command::new(util::cargo()))?;
        Ok(Self::with_target(util::root(), target))
    }

    /// The layout for the workspace `root` and the cargo target
    /// directory `target`: the output goes to `<target>/dist`.
    pub fn with_target(root: PathBuf, target: PathBuf) -> Self {
        let dist = target.join("dist");
        Layout { root, target, dist }
    }

    /// The layout of a test run: everything under `root`.
    #[cfg(test)]
    pub fn under(root: &Path) -> Self {
        Self::with_target(root.to_path_buf(), root.join("target"))
    }

    /// The built binary `name` for `target_triple`, as cargo lays it
    /// out (`<target dir>/<triple>/release/<name>`, AGENTS.md section
    /// 2: builds run with no `CARGO_TARGET_DIR` of their own).
    pub fn binary(&self, target_triple: &str, name: &str) -> PathBuf {
        self.release_dir(target_triple).join(name)
    }

    /// The release directory cargo writes the built binaries into.
    pub fn release_dir(&self, target_triple: &str) -> PathBuf {
        self.target.join(target_triple).join("release")
    }

    /// The package file `name` under the output directory.
    pub fn package(&self, name: &str) -> PathBuf {
        self.dist.join(name)
    }
}

/// Runs `cargo metadata --no-deps` through `cargo` (a bare cargo
/// command; the tests add environment variables to it) and returns
/// the target directory it reports (task R10 item 1).
pub fn metadata_target_dir(cargo: &mut Command) -> Result<PathBuf> {
    let json = util::output(cargo.args(["metadata", "--format-version", "1", "--no-deps"]))?;
    target_directory(&json)
}

/// The `target_directory` of `cargo metadata` JSON.
pub fn target_directory(json: &str) -> Result<PathBuf> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad cargo metadata JSON: {e}"))?;
    value
        .get("target_directory")
        .and_then(|t| t.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata: no `target_directory`".to_owned())
}

/// The brand icon source, relative to the workspace root (ADR 0067).
pub const ICON_SVG: &str = "crates/ui/assets/brand/icon.svg";

/// The macOS target triple (task R9 decisions). Lives here, not in
/// [`mac`], because [`check_host`] names it on every host.
pub const MAC_TARGET: &str = "aarch64-apple-darwin";

/// The Windows target triple (task R9 decisions).
pub const WINDOWS_TARGET: &str = "x86_64-pc-windows-msvc";

/// The Linux target triple (task R9 decisions).
pub const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";

/// The binaries every package carries (task R9 decisions). The
/// Windows spellings add `.exe`.
pub const BINARIES: &[&str] = &["larrygram", "larrygram-cli"];

/// The licence files the packages ship: `(destination inside the
/// package, source under the workspace root)` — the three repository
/// licences and the OFL texts of the two bundled fonts (task R7 goal
/// 1, task R9 decisions).
pub fn licence_files() -> &'static [(&'static str, &'static str)] {
    &[
        ("LICENSE", "LICENSE"),
        ("NOTICE", "NOTICE"),
        ("ATTRIBUTION.md", "ATTRIBUTION.md"),
        ("fonts/Carlito-OFL.txt", "crates/text/fonts/Carlito-OFL.txt"),
        ("fonts/Selawik-OFL.txt", "crates/text/fonts/Selawik-OFL.txt"),
    ]
}

/// The sorted destination paths of [`licence_files`].
pub fn licence_entries() -> Vec<String> {
    let mut entries: Vec<String> = licence_files()
        .iter()
        .map(|(dest, _)| (*dest).to_owned())
        .collect();
    entries.sort();
    entries
}

/// The top directory inside a package: `Larrygram-<version>-<os>-<arch>`
/// (task R9 decisions), the package name without its extension. An
/// empty version is an error.
pub fn package_dir_name(version: &str, os: &str, arch: &str) -> Result<String> {
    if version.is_empty() {
        return Err("package version is empty".to_owned());
    }
    Ok(format!("Larrygram-{version}-{os}-{arch}"))
}

/// The package name `Larrygram-<version>-<os>-<arch>.<ext>` (task R9
/// decisions). An empty version is an error.
pub fn package_name(version: &str, os: &str, arch: &str, ext: &str) -> Result<String> {
    Ok(format!("{}.{ext}", package_dir_name(version, os, arch)?))
}

/// The host check: every platform builds on one host only, so nothing
/// cross-builds (task R9 goal 1). `os` and `arch` are
/// `std::env::consts` values, passed in so the tests are pure.
pub fn check_host(platform: Platform, os: &str, arch: &str) -> Result<()> {
    let (want_os, want_arch, target, described) = match platform {
        Platform::Mac => ("macos", "aarch64", MAC_TARGET, "macOS on Apple silicon"),
        Platform::Windows => ("windows", "x86_64", WINDOWS_TARGET, "64-bit Windows"),
        Platform::Linux => ("linux", "x86_64", LINUX_TARGET, "64-bit x86 Linux"),
    };
    if os == want_os && arch == want_arch {
        Ok(())
    } else {
        Err(format!(
            "dist {} builds on {} only (got {os}/{arch}, target {target})",
            platform.name(),
            described
        ))
    }
}

/// Reads the workspace version from the root `Cargo.toml` text
/// (`[workspace.package] version`).
pub fn workspace_version(cargo_toml: &str) -> Result<String> {
    let table: toml::Table = cargo_toml
        .parse()
        .map_err(|e| format!("Cargo.toml does not parse: {e}"))?;
    table
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())
}

/// Reads the workspace version from the `Cargo.toml` under `root`.
pub fn version_from_root(root: &Path) -> Result<String> {
    let cargo_toml = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    workspace_version(&text)
}

/// Rasterises the brand SVG to one square PNG of `size` pixels, using
/// resvg (the same rasteriser the app uses for its window icon, ADR
/// 0067). The macOS iconset and the Linux package icon both come from
/// this one function.
pub fn render_png(svg: &str, size: u32) -> Result<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|e| format!("cannot parse icon.svg: {e}"))?;
    let width = tree.size().width().max(1.0);
    let height = tree.size().height().max(1.0);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| format!("cannot allocate a {size}x{size} pixmap"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(size as f32 / width, size as f32 / height),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| format!("cannot encode the {size}x{size} PNG: {e}"))
}

/// `cargo xtask dist` entry point: the real layout and the real system
/// tools. Each platform pipeline checks the host itself before it
/// builds anything.
pub fn run(options: &Options) -> Result<()> {
    let layout = Layout::real()?;
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    match options.platform {
        Platform::Mac => run_mac(&layout, os, arch),
        Platform::Windows => windows::dist_windows(&mut StdSys, &layout, os, arch).map(|_| ()),
        Platform::Linux => {
            let epoch = std::env::var("SOURCE_DATE_EPOCH").ok();
            linux::dist_linux(&mut StdSys, &layout, os, arch, epoch.as_deref()).map(|_| ())
        }
    }
}

/// `dist mac` on a unix host (the only place [`mac`] compiles: its
/// bundle tools are unix-only).
#[cfg(unix)]
fn run_mac(layout: &Layout, os: &str, arch: &str) -> Result<()> {
    mac::dist_mac(&mut StdSys, layout, os, arch).map(|_| ())
}

/// `dist mac` on a non-unix host. [`check_host`] rejects such a host
/// before this runs; the error keeps the command honest everywhere.
#[cfg(not(unix))]
fn run_mac(_layout: &Layout, _os: &str, _arch: &str) -> Result<()> {
    Err("dist mac builds on macOS on Apple silicon only (this host is not unix)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::TempDir;

    /// C3: `dist` parses its three platforms, and every other argument
    /// list is a clean error.
    #[test]
    fn parse_args_accepts_the_three_platforms() {
        assert_eq!(
            parse_args(&["mac".to_owned()]).unwrap(),
            Options {
                platform: Platform::Mac
            }
        );
        assert_eq!(
            parse_args(&["windows".to_owned()]).unwrap(),
            Options {
                platform: Platform::Windows
            }
        );
        assert_eq!(
            parse_args(&["linux".to_owned()]).unwrap(),
            Options {
                platform: Platform::Linux
            }
        );
        let err = parse_args(&[]).unwrap_err();
        assert!(err.contains("dist needs a platform"), "{err}");
        let err = parse_args(&["--bogus".to_owned()]).unwrap_err();
        assert!(err.contains("unknown dist platform `--bogus`"), "{err}");
        let err = parse_args(&["mac".to_owned(), "extra".to_owned()]).unwrap_err();
        assert!(err.contains("dist takes one platform argument"), "{err}");
    }

    /// The host check accepts the one host per platform and rejects
    /// everything else (task R9 goal 1: no cross-building).
    #[test]
    fn check_host_accepts_one_host_per_platform() {
        assert!(check_host(Platform::Mac, "macos", "aarch64").is_ok());
        assert!(check_host(Platform::Windows, "windows", "x86_64").is_ok());
        assert!(check_host(Platform::Linux, "linux", "x86_64").is_ok());

        let err = check_host(Platform::Mac, "macos", "x86_64").unwrap_err();
        assert!(
            err.contains("dist mac builds on macOS on Apple silicon only"),
            "{err}"
        );
        assert!(err.contains("got macos/x86_64"), "{err}");
        assert!(err.contains("aarch64-apple-darwin"), "{err}");

        let err = check_host(Platform::Windows, "linux", "x86_64").unwrap_err();
        assert!(
            err.contains("dist windows builds on 64-bit Windows only"),
            "{err}"
        );
        assert!(err.contains("x86_64-pc-windows-msvc"), "{err}");

        let err = check_host(Platform::Linux, "macos", "aarch64").unwrap_err();
        assert!(
            err.contains("dist linux builds on 64-bit x86 Linux only"),
            "{err}"
        );
        assert!(err.contains("x86_64-unknown-linux-gnu"), "{err}");
    }

    /// The package names follow `Larrygram-<version>-<os>-<arch>.<ext>`
    /// (task R9 decisions).
    #[test]
    fn package_names_follow_the_decision_scheme() {
        assert_eq!(
            package_name("1.0.0-alpha.1", "macos", "arm64", "dmg").unwrap(),
            "Larrygram-1.0.0-alpha.1-macos-arm64.dmg"
        );
        assert_eq!(
            package_name("1.0.0-alpha.1", "windows", "x64", "zip").unwrap(),
            "Larrygram-1.0.0-alpha.1-windows-x64.zip"
        );
        assert_eq!(
            package_name("1.0.0-alpha.1", "linux", "x64", "tar.gz").unwrap(),
            "Larrygram-1.0.0-alpha.1-linux-x64.tar.gz"
        );
        let err = package_name("", "linux", "x64", "tar.gz").unwrap_err();
        assert!(err.contains("version is empty"), "{err}");
    }

    /// The version comes from `[workspace.package]` in Cargo.toml.
    #[test]
    fn workspace_version_reads_the_workspace_package() {
        assert_eq!(
            workspace_version("[workspace.package]\nversion = \"2.3.4\"\n").unwrap(),
            "2.3.4"
        );
        let err = workspace_version("[package]\nversion = \"2.3.4\"\n").unwrap_err();
        assert!(err.contains("no [workspace.package] version"), "{err}");
        let err = workspace_version("not [toml").unwrap_err();
        assert!(err.contains("does not parse"), "{err}");
    }

    /// `version_from_root` reads the same version from a root
    /// directory, and reports a missing file cleanly.
    #[test]
    fn version_from_root_reads_the_workspace_toml() {
        let tmp = TempDir::new("dist-version");
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace.package]\nversion = \"9.9.9\"\n",
        )
        .unwrap();
        assert_eq!(version_from_root(tmp.path()).unwrap(), "9.9.9");
        let err = version_from_root(&tmp.join("elsewhere")).unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
    }

    /// Every licence source exists in the real workspace, and the
    /// licence entries are the sorted destination paths.
    #[test]
    fn licence_sources_exist() {
        let root = util::root();
        for (_dest, source) in licence_files() {
            assert!(root.join(source).is_file(), "missing licence {source}");
        }
        let mut expect = licence_entries();
        expect.sort();
        assert_eq!(licence_entries(), expect);
        assert!(licence_entries().contains(&"LICENSE".to_owned()));
        assert!(licence_entries().contains(&"fonts/Carlito-OFL.txt".to_owned()));
    }

    /// The layout puts the output under `<target dir>/dist` and the
    /// binaries under `<target dir>/<triple>/release` (task R9 goal 1).
    #[test]
    fn layout_lives_under_the_target_dir() {
        let tmp = TempDir::new("dist-layout");
        let layout = Layout::under(tmp.path());
        assert_eq!(layout.target, tmp.join("target"));
        assert_eq!(layout.dist, tmp.join("target/dist"));
        assert_eq!(
            layout.binary("x86_64-unknown-linux-gnu", "larrygram"),
            tmp.join("target/x86_64-unknown-linux-gnu/release/larrygram")
        );
        assert_eq!(
            layout.package("Larrygram-1-windows-x64.zip"),
            tmp.join("target/dist/Larrygram-1-windows-x64.zip")
        );
    }

    /// `Layout::real` puts the output under `<target dir>/dist`, with
    /// the target dir that cargo itself reports, whatever sets it
    /// (task R10 item 1: the master's gate sets `CARGO_TARGET_DIR`).
    #[test]
    fn layout_real_lives_under_the_cargo_target_dir() {
        let layout = Layout::real().unwrap();
        assert_eq!(layout.root, util::root());
        let reported = metadata_target_dir(&mut Command::new(util::cargo())).unwrap();
        assert_eq!(layout.target, reported);
        assert_eq!(layout.dist, reported.join("dist"));
    }

    /// Both environment variables move the target dir `cargo
    /// metadata` reports, so `Layout::real` follows them (task R10
    /// item 1). `build.target-dir` is the config-file spelling of
    /// `CARGO_BUILD_TARGET_DIR`.
    #[test]
    fn metadata_target_dir_follows_the_environment() {
        let tmp = TempDir::new("dist-target-env");
        for var in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
            let want = tmp.join(var);
            let mut cargo = Command::new(util::cargo());
            cargo
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("CARGO_BUILD_TARGET_DIR")
                .env(var, &want);
            assert_eq!(metadata_target_dir(&mut cargo).unwrap(), want, "{var}");
        }
    }

    /// Broken or incomplete metadata is a clean error.
    #[test]
    fn target_directory_reports_bad_metadata() {
        assert_eq!(
            target_directory(r#"{"target_directory":"/t"}"#).unwrap(),
            PathBuf::from("/t")
        );
        let err = target_directory("not json").unwrap_err();
        assert!(err.contains("bad cargo metadata JSON"), "{err}");
        let err = target_directory("{}").unwrap_err();
        assert!(err.contains("no `target_directory`"), "{err}");
    }

    /// `StdSys` runs the real command and reports a failing one
    /// cleanly (the pipeline's one error mapping).
    #[test]
    fn std_sys_runs_and_reports_commands() {
        let mut sys = StdSys;
        assert!(sys.run(&util::cargo(), &["--version"]).is_ok());
        let err = sys
            .run(&util::cargo(), &["definitely-not-a-cargo-subcommand"])
            .unwrap_err();
        assert!(err.contains("exited with"), "{err}");
    }

    /// `run` refuses the platform this host cannot build before
    /// anything runs (task R9 goal 1).
    #[test]
    fn run_refuses_the_platform_this_host_cannot_build() {
        let other = if cfg!(target_os = "windows") {
            Platform::Linux
        } else {
            Platform::Windows
        };
        let err = run(&Options { platform: other }).unwrap_err();
        assert!(err.contains("only"), "{err}");
    }

    /// The rasteriser writes one square PNG of the requested size
    /// (the Linux package icon comes from it).
    #[test]
    fn render_png_rasterises_the_brand_icon() {
        let root = util::root();
        let svg = std::fs::read_to_string(root.join(ICON_SVG)).expect("the brand icon.svg reads");
        let png = render_png(&svg, 256).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        let be32 = |start: usize| {
            u32::from_be_bytes([png[start], png[start + 1], png[start + 2], png[start + 3]])
        };
        assert_eq!(be32(16), 256, "width");
        assert_eq!(be32(20), 256, "height");
    }

    /// A broken SVG is a clean error.
    #[test]
    fn render_png_reports_a_broken_svg() {
        let err = render_png("not an svg", 32).unwrap_err();
        assert!(err.contains("cannot parse"), "{err}");
    }
}
