//! `cargo xtask dist mac`: builds the macOS `Larrygram.app` bundle and
//! the `Larrygram-<version>.dmg` disk image (task R7).
//!
//! The artifacts go under [`DIST_ROOT`] on the external drive, never
//! into the repository. The build steps:
//!
//! 1. build the release binary of `apps/larrygram` for the one
//!    supported target, [`TARGET`] (Apple silicon, task R7 decisions);
//! 2. render `Contents/Info.plist` from the committed template in
//!    `apps/larrygram/macos/Info.plist`, with the workspace version
//!    from `Cargo.toml`;
//! 3. rasterise the brand `crates/ui/assets/brand/icon.svg` at the
//!    standard `.iconset` sizes with our own rasteriser (the same
//!    resvg the app uses for its window icon, ADR 0067) and pack the
//!    PNGs into `Contents/Resources/Larrygram.icns` with the system
//!    `iconutil`;
//! 4. copy the licence files the app ships with;
//! 5. sign ad-hoc with the system `codesign` (`--sign -`, no Developer
//!    ID, no notarisation, task R7 decisions);
//! 6. pack the `.dmg` with the system `hdiutil`: the app and an
//!    `Applications` link.
//!
//! The system tools (`iconutil`, `hdiutil`, `codesign`) are invoked,
//! never linked (task R7 master notes). Every command runs through
//! the [`Sys`] trait, so the unit tests drive the whole pipeline with
//! a fake runner and hand-built inputs.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{self, Result};

/// Where the built artifacts go: the external drive, never the
/// repository (task R7). The volume must be mounted.
pub const DIST_ROOT: &str = "/Volumes/Dara/larrygram/dist";

/// The only supported target triple: Apple silicon (task R7
/// decisions). No universal binary for now.
pub const TARGET: &str = "aarch64-apple-darwin";

/// The bundle directory name under [`DIST_ROOT`].
pub const BUNDLE_NAME: &str = "Larrygram.app";

/// The executable name inside `Contents/MacOS`, and the
/// `CFBundleExecutable` value.
pub const EXECUTABLE_NAME: &str = "larrygram";

/// The icon file name inside `Contents/Resources`, and the
/// `CFBundleIconFile` value.
pub const ICON_FILE: &str = "Larrygram.icns";

/// The `.dmg` volume name shown by Finder when it is mounted.
pub const VOLUME_NAME: &str = "Larrygram";

/// The committed `Info.plist` template, relative to the workspace root.
const INFO_PLIST_TEMPLATE: &str = "apps/larrygram/macos/Info.plist";

/// The brand icon source, relative to the workspace root (ADR 0067).
const ICON_SVG: &str = "crates/ui/assets/brand/icon.svg";

/// The built app binary, relative to the workspace root. `cargo`
/// writes it under the workspace `target/` because the build runs
/// with no `CARGO_TARGET_DIR` (AGENTS.md section 2).
const BINARY_RELPATH: &str = "target/aarch64-apple-darwin/release/larrygram";

/// The `.iconset` directory name `iconutil` expects (it must end in
/// `.iconset`).
const ICONSET_DIR: &str = "AppIcon.iconset";

/// The platform `cargo xtask dist` builds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// `dist mac`: the macOS `.app` and `.dmg` (task R7).
    Mac,
}

/// `cargo xtask dist` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The platform to build.
    pub platform: Platform,
}

/// Parses `cargo xtask dist` arguments. Exactly one platform argument
/// (`mac`) is accepted; everything else is a clean error (C3).
pub fn parse_args(args: &[String]) -> Result<Options> {
    let [platform, rest @ ..] = args else {
        return Err("dist needs a platform: `cargo xtask dist mac`".to_owned());
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
        other => Err(format!("unknown dist platform `{other}`; platforms: mac")),
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
    /// The output directory, outside the repository.
    pub dist: PathBuf,
}

impl Layout {
    /// The layout of a real run: the workspace root and [`DIST_ROOT`].
    pub fn real() -> Self {
        Layout {
            root: util::root(),
            dist: PathBuf::from(DIST_ROOT),
        }
    }

    /// The `.app` directory under the output directory.
    pub fn bundle(&self) -> PathBuf {
        self.dist.join(BUNDLE_NAME)
    }

    /// The `.dmg` for `version` under the output directory.
    pub fn dmg(&self, version: &str) -> PathBuf {
        self.dist.join(format!("Larrygram-{version}.dmg"))
    }
}

/// The standard `.iconset` entries: the file name inside
/// `AppIcon.iconset` and its pixel size. The list is the one
/// `iconutil` documents: every base size 16..512 with its `@2x`
/// double, up to 1024 (task R7 icon size list).
pub fn iconset_entries() -> &'static [(&'static str, u32)] {
    &[
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ]
}

/// The licence files the bundle ships: `(destination under
/// Contents/Resources, source under the workspace root)` — the three
/// repository licences and the OFL texts of the two bundled fonts
/// (task R7 goal 1).
pub fn licence_files() -> &'static [(&'static str, &'static str)] {
    &[
        ("LICENSE", "LICENSE"),
        ("NOTICE", "NOTICE"),
        ("ATTRIBUTION.md", "ATTRIBUTION.md"),
        ("fonts/Carlito-OFL.txt", "crates/text/fonts/Carlito-OFL.txt"),
        ("fonts/Selawik-OFL.txt", "crates/text/fonts/Selawik-OFL.txt"),
    ]
}

/// The files the bundle contains, relative to the `.app` directory,
/// sorted: the executable, the plist, the icon and the licence files.
/// The tests pin the bundle layout against this list (task R7 checks).
pub fn bundle_entries() -> Vec<String> {
    let mut entries = vec![
        "Contents/Info.plist".to_owned(),
        format!("Contents/MacOS/{EXECUTABLE_NAME}"),
        format!("Contents/Resources/{ICON_FILE}"),
    ];
    for (dest, _) in licence_files() {
        entries.push(format!("Contents/Resources/{dest}"));
    }
    entries.sort();
    entries
}

/// The architecture check: only macOS on Apple silicon is supported
/// (task R7 decisions). `os` and `arch` are `std::env::consts`
/// values, passed in so the test is pure.
pub fn check_target(os: &str, arch: &str) -> Result<()> {
    if os == "macos" && arch == "aarch64" {
        Ok(())
    } else {
        Err(format!(
            "dist mac builds on macOS on Apple silicon only (got {os}/{arch}, target {TARGET})"
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

/// Renders the committed `Info.plist` template for `version`:
/// substitutes every `{{VERSION}}` placeholder and fails on any
/// placeholder left over. An empty version is an error.
pub fn render_info_plist(template: &str, version: &str) -> Result<String> {
    if version.is_empty() {
        return Err("Info.plist version is empty".to_owned());
    }
    let rendered = template.replace("{{VERSION}}", version);
    if let Some(start) = rendered.find("{{") {
        let rest = &rendered[start..];
        let end = rest.find("}}").map(|i| i + 2).unwrap_or(rest.len());
        return Err(format!("unknown Info.plist placeholder `{}`", &rest[..end]));
    }
    Ok(rendered)
}

/// Rasterises the brand SVG into `out_dir` at every
/// [`iconset_entries`] size, as PNG, using resvg (the same rasteriser
/// the app uses for its window icon, ADR 0067). Returns the PNG paths
/// in entry order.
pub fn rasterise_iconset(svg: &str, out_dir: &Path) -> Result<Vec<PathBuf>> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|e| format!("cannot parse {ICON_SVG}: {e}"))?;
    let width = tree.size().width().max(1.0);
    let height = tree.size().height().max(1.0);
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut written = Vec::new();
    for (name, size) in iconset_entries() {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(*size, *size)
            .ok_or_else(|| format!("cannot allocate a {size}x{size} pixmap for {name}"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(*size as f32 / width, *size as f32 / height),
            &mut pixmap.as_mut(),
        );
        let png = pixmap
            .encode_png()
            .map_err(|e| format!("cannot encode {name}: {e}"))?;
        let path = out_dir.join(name);
        std::fs::write(&path, png).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Rasterises the iconset into `scratch/AppIcon.iconset` and packs it
/// into `scratch/Larrygram.icns` with the system `iconutil`. Returns
/// the `.icns` path.
fn build_icon(sys: &mut dyn Sys, root: &Path, scratch: &Path) -> Result<PathBuf> {
    let svg_path = root.join(ICON_SVG);
    let svg = std::fs::read_to_string(&svg_path)
        .map_err(|e| format!("cannot read {}: {e}", svg_path.display()))?;
    let iconset = scratch.join(ICONSET_DIR);
    rasterise_iconset(&svg, &iconset)?;
    let icns = scratch.join(ICON_FILE);
    sys.run(
        "iconutil",
        &["-c", "icns", &path_arg(&iconset), "-o", &path_arg(&icns)],
    )?;
    if !icns.is_file() {
        return Err(format!(
            "iconutil produced no {}: {}",
            ICON_FILE,
            icns.display()
        ));
    }
    Ok(icns)
}

/// Copies one directory tree from `src` into `dst` (created).
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", src.display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("cannot copy {} -> {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Writes the `.app` bundle under `app`: the executable (made
/// executable), the rendered `Info.plist`, the packed icon and the
/// licence files from `root`. Returns the relative paths written, in
/// the order of [`bundle_entries`] (task R7 bundle layout).
pub fn write_bundle(
    root: &Path,
    app: &Path,
    executable: &Path,
    info_plist: &str,
    icon: &Path,
) -> Result<Vec<String>> {
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    for dir in [&macos, &resources] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut written = Vec::new();

    let plist_path = contents.join("Info.plist");
    std::fs::write(&plist_path, info_plist)
        .map_err(|e| format!("cannot write {}: {e}", plist_path.display()))?;
    written.push("Contents/Info.plist".to_owned());

    let bin_path = macos.join(EXECUTABLE_NAME);
    std::fs::copy(executable, &bin_path).map_err(|e| {
        format!(
            "cannot copy {} -> {}: {e}",
            executable.display(),
            bin_path.display()
        )
    })?;
    let mut mode = std::fs::metadata(&bin_path)
        .map_err(|e| format!("cannot stat {}: {e}", bin_path.display()))?
        .permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(&bin_path, mode)
        .map_err(|e| format!("cannot chmod {}: {e}", bin_path.display()))?;
    written.push(format!("Contents/MacOS/{EXECUTABLE_NAME}"));

    let icon_path = resources.join(ICON_FILE);
    std::fs::copy(icon, &icon_path).map_err(|e| {
        format!(
            "cannot copy {} -> {}: {e}",
            icon.display(),
            icon_path.display()
        )
    })?;
    written.push(format!("Contents/Resources/{ICON_FILE}"));

    for (dest, source) in licence_files() {
        let from = root.join(source);
        let to = resources.join(dest);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(&from, &to)
            .map_err(|e| format!("cannot copy {} -> {}: {e}", from.display(), to.display()))?;
        written.push(format!("Contents/Resources/{dest}"));
    }

    written.sort();
    let expected = bundle_entries();
    if written != expected {
        return Err(format!(
            "bundle layout {written:?} does not match the expected {expected:?}"
        ));
    }
    Ok(written)
}

/// Stages the `.dmg` contents under `staging`: the `.app` copied in
/// and an `Applications` link, the classic drag-to-install layout.
pub fn stage_dmg(staging: &Path, app: &Path) -> Result<()> {
    if staging.exists() {
        std::fs::remove_dir_all(staging)
            .map_err(|e| format!("cannot clear {}: {e}", staging.display()))?;
    }
    let staged_app = staging.join(app.file_name().unwrap_or_default());
    copy_dir(app, &staged_app)?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))
        .map_err(|e| format!("cannot create the Applications link: {e}"))?;
    Ok(())
}

/// Packs the staged directory into `dmg` with the system `hdiutil`
/// (compressed UDZO), then removes the staging directory.
fn make_dmg(sys: &mut dyn Sys, staging: &Path, dmg: &Path) -> Result<()> {
    sys.run(
        "hdiutil",
        &[
            "create",
            "-volname",
            VOLUME_NAME,
            "-srcfolder",
            &path_arg(staging),
            "-ov",
            "-format",
            "UDZO",
            &path_arg(dmg),
        ],
    )?;
    if !dmg.is_file() {
        return Err(format!("hdiutil produced no image: {}", dmg.display()));
    }
    std::fs::remove_dir_all(staging)
        .map_err(|e| format!("cannot remove {}: {e}", staging.display()))?;
    Ok(())
}

/// The argument spelling of a path (a `&str` for the command line).
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The whole build through `sys`: release binary, bundle, ad-hoc
/// signature and `.dmg`. Returns the `.dmg` path. `os` and `arch` are
/// the host's `std::env::consts` values (see [`check_target`]); the
/// tests pass their own and use a fake [`Sys`], so the whole pipeline
/// runs on hand-built inputs (task R7 checks).
pub fn dist_mac(sys: &mut dyn Sys, layout: &Layout, os: &str, arch: &str) -> Result<PathBuf> {
    check_target(os, arch)?;
    let root = &layout.root;
    let dist = &layout.dist;
    std::fs::create_dir_all(dist).map_err(|e| format!("cannot create {}: {e}", dist.display()))?;

    let cargo_toml = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read {}: {e}", root.join("Cargo.toml").display()))?;
    let version = workspace_version(&cargo_toml)?;
    println!("dist mac: building Larrygram {version} for {TARGET}");

    // 1. The release binary (AGENTS.md section 2: no CARGO_TARGET_DIR).
    sys.run(
        &util::cargo(),
        &["build", "--release", "-p", "larrygram", "--target", TARGET],
    )?;
    let binary = root.join(BINARY_RELPATH);
    if !binary.is_file() {
        return Err(format!(
            "cargo built no release binary at {}",
            binary.display()
        ));
    }

    // 2. The Info.plist.
    let template_path = root.join(INFO_PLIST_TEMPLATE);
    let template = std::fs::read_to_string(&template_path)
        .map_err(|e| format!("cannot read {}: {e}", template_path.display()))?;
    let info_plist = render_info_plist(&template, &version)?;

    // 3. The icon, rasterised by us and packed by iconutil.
    let scratch = dist.join(format!(".build-{}", std::process::id()));
    let icns = match build_icon(sys, root, &scratch) {
        Ok(icns) => icns,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(e);
        }
    };

    // 4. The bundle.
    let app = layout.bundle();
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|e| format!("cannot clear {}: {e}", app.display()))?;
    }
    let bundle_result = write_bundle(root, &app, &binary, &info_plist, &icns);
    let _ = std::fs::remove_dir_all(&scratch);
    bundle_result?;

    // 5. The ad-hoc signature (task R7 decisions: no Developer ID).
    let app_arg = path_arg(&app);
    sys.run("codesign", &["--force", "--sign", "-", &app_arg])?;
    sys.run("codesign", &["--verify", "--strict", &app_arg])?;

    // 6. The disk image.
    let staging = dist.join(format!(".dmg-staging-{}", std::process::id()));
    stage_dmg(&staging, &app)?;
    let dmg = layout.dmg(&version);
    make_dmg(sys, &staging, &dmg)?;

    println!("dist mac: {}", app.display());
    println!("dist mac: {}", dmg.display());
    Ok(dmg)
}

/// `cargo xtask dist` entry point: the real layout and the real
/// system tools.
pub fn run(options: &Options) -> Result<()> {
    match options.platform {
        Platform::Mac => dist_mac(
            &mut StdSys,
            &Layout::real(),
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
        .map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::TempDir;

    /// A [`Sys`] that records every call and fabricates the outputs:
    /// `cargo build` writes a dummy binary, `iconutil` a dummy
    /// `.icns`, `hdiutil` a dummy `.dmg`, `codesign` nothing.
    struct FakeSys {
        root: PathBuf,
        calls: Vec<String>,
    }

    impl FakeSys {
        fn new(root: &Path) -> Self {
            FakeSys {
                root: root.to_path_buf(),
                calls: Vec::new(),
            }
        }

        /// Fabricates the artifact the recorded command produces. The
        /// program is matched by its file name, so a full toolchain
        /// path (the `$CARGO` cargo) counts as `cargo`.
        fn fabricate(&mut self, program: &str, args: &[&str]) -> Result<()> {
            self.calls.push(format!("{program} {}", args.join(" ")));
            let name = Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| program.to_owned());
            match name.as_str() {
                "cargo" => {
                    let bin = self.root.join(BINARY_RELPATH);
                    std::fs::create_dir_all(bin.parent().unwrap_or(Path::new("/")))
                        .map_err(|e| format!("fake cargo: {e}"))?;
                    std::fs::write(&bin, b"fake binary").map_err(|e| format!("fake cargo: {e}"))
                }
                "iconutil" => write_after_flag(args, "-o", b"fake icns", "iconutil"),
                "hdiutil" => {
                    let last = args.last().copied().unwrap_or_default();
                    std::fs::write(last, b"fake dmg").map_err(|e| format!("fake hdiutil: {e}"))
                }
                "codesign" => Ok(()),
                other => Err(format!("unexpected command `{other}`")),
            }
        }
    }

    /// Writes `bytes` to the path after `flag` in `args`.
    fn write_after_flag(args: &[&str], flag: &str, bytes: &[u8], who: &str) -> Result<()> {
        let idx = args
            .iter()
            .position(|a| *a == flag)
            .ok_or_else(|| format!("fake {who}: no {flag}"))?;
        let out = args
            .get(idx + 1)
            .ok_or_else(|| format!("fake {who}: {flag} has no value"))?;
        std::fs::write(out, bytes).map_err(|e| format!("fake {who}: {e}"))
    }

    impl Sys for FakeSys {
        fn run(&mut self, program: &str, args: &[&str]) -> Result<()> {
            self.fabricate(program, args)
        }
    }

    /// A temp workspace with the Cargo.toml, the template, the icon
    /// source and the licence sources that the build reads.
    fn workspace() -> TempDir {
        let tmp = TempDir::new("dist-mac");
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-alpha.1\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("apps/larrygram/macos")).unwrap();
        std::fs::write(
            root.join(INFO_PLIST_TEMPLATE),
            "<plist><dict><key>CFBundleVersion</key><string>{{VERSION}}</string></dict></plist>",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/ui/assets/brand")).unwrap();
        std::fs::write(
            root.join(ICON_SVG),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        for (_, source) in licence_files() {
            let path = root.join(source);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "licence text").unwrap();
        }
        tmp
    }

    /// C3: `dist mac` parses, and every other argument list is a clean
    /// error.
    #[test]
    fn parse_args_accepts_only_mac() {
        assert_eq!(
            parse_args(&["mac".to_owned()]).unwrap(),
            Options {
                platform: Platform::Mac
            }
        );
        let err = parse_args(&[]).unwrap_err();
        assert!(err.contains("dist needs a platform"), "{err}");
        let err = parse_args(&["--bogus".to_owned()]).unwrap_err();
        assert!(err.contains("unknown dist platform `--bogus`"), "{err}");
        let err = parse_args(&["mac".to_owned(), "extra".to_owned()]).unwrap_err();
        assert!(err.contains("dist takes one platform argument"), "{err}");
    }

    /// The architecture check accepts Apple silicon and rejects
    /// everything else (task R7 decisions).
    #[test]
    fn check_target_accepts_only_apple_silicon() {
        assert!(check_target("macos", "aarch64").is_ok());
        let err = check_target("linux", "aarch64").unwrap_err();
        assert!(err.contains("macOS on Apple silicon only"), "{err}");
        let err = check_target("macos", "x86_64").unwrap_err();
        assert!(err.contains("macOS on Apple silicon only"), "{err}");
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

    /// The plist rendering substitutes the version and rejects a
    /// leftover placeholder or an empty version.
    #[test]
    fn render_info_plist_substitutes_the_version() {
        let out = render_info_plist("<string>{{VERSION}}</string>", "1.0.0-alpha.1").unwrap();
        assert_eq!(out, "<string>1.0.0-alpha.1</string>");
        let err = render_info_plist("{{OTHER}}", "1.0").unwrap_err();
        assert!(
            err.contains("unknown Info.plist placeholder `{{OTHER}}`"),
            "{err}"
        );
        let err = render_info_plist("{{VERSION}}", "").unwrap_err();
        assert!(err.contains("version is empty"), "{err}");
    }

    /// The icon size list is the standard `.iconset` list: every base
    /// size with its `@2x` double (task R7 checks).
    #[test]
    fn iconset_entries_are_the_standard_sizes() {
        let entries = iconset_entries();
        assert_eq!(entries.len(), 10);
        let expect: &[(&str, u32)] = &[
            ("icon_16x16.png", 16),
            ("icon_16x16@2x.png", 32),
            ("icon_32x32.png", 32),
            ("icon_32x32@2x.png", 64),
            ("icon_128x128.png", 128),
            ("icon_128x128@2x.png", 256),
            ("icon_256x256.png", 256),
            ("icon_256x256@2x.png", 512),
            ("icon_512x512.png", 512),
            ("icon_512x512@2x.png", 1024),
        ];
        assert_eq!(entries, expect);
    }

    /// Every licence source exists in the real workspace, and the
    /// bundle entries are the executable, the plist, the icon and
    /// those licences.
    #[test]
    fn licence_sources_exist_and_shape_the_bundle() {
        let root = util::root();
        for (dest, source) in licence_files() {
            assert!(root.join(source).is_file(), "missing licence {source}");
            assert!(bundle_entries().contains(&format!("Contents/Resources/{dest}")));
        }
        assert!(bundle_entries().contains(&format!("Contents/MacOS/{EXECUTABLE_NAME}")));
        assert!(bundle_entries().contains(&"Contents/Info.plist".to_owned()));
        assert!(bundle_entries().contains(&format!("Contents/Resources/{ICON_FILE}")));
    }

    /// `write_bundle` builds the expected layout with the expected
    /// contents from hand-built inputs (task R7 bundle layout check).
    #[test]
    fn write_bundle_builds_the_bundle_layout() {
        let tmp = TempDir::new("bundle");
        let root = tmp.join("root");
        let app = tmp.join("dist/Larrygram.app");
        for (dest, source) in licence_files() {
            let _ = dest;
            let path = root.join(source);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "licence text").unwrap();
        }
        let bin = tmp.join("built-larrygram");
        std::fs::write(&bin, b"binary").unwrap();
        let icns = tmp.join("Larrygram.icns");
        std::fs::write(&icns, b"icns").unwrap();

        let written = write_bundle(&root, &app, &bin, "<plist/>", &icns).unwrap();
        assert_eq!(written, bundle_entries());
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
            "<plist/>"
        );
        assert_eq!(
            std::fs::read(app.join("Contents/MacOS/larrygram")).unwrap(),
            b"binary"
        );
        let mode = std::fs::metadata(app.join("Contents/MacOS/larrygram"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            std::fs::read(app.join("Contents/Resources/Larrygram.icns")).unwrap(),
            b"icns"
        );
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/Resources/fonts/Carlito-OFL.txt")).unwrap(),
            "licence text"
        );
    }

    /// A missing licence source is a clean error, not a panic.
    #[test]
    fn write_bundle_reports_a_missing_source() {
        let tmp = TempDir::new("bundle-missing");
        let bin = tmp.join("bin");
        std::fs::write(&bin, b"binary").unwrap();
        let icns = tmp.join("icon.icns");
        std::fs::write(&icns, b"icns").unwrap();
        let err = write_bundle(
            tmp.join("empty").as_path(),
            tmp.join("dist/Larrygram.app").as_path(),
            &bin,
            "<plist/>",
            &icns,
        )
        .unwrap_err();
        assert!(err.contains("cannot copy"), "{err}");
    }

    /// The rasteriser writes one PNG per iconset entry at the entry's
    /// pixel size (task R7 icon size list).
    #[test]
    fn rasterise_iconset_writes_the_standard_pngs() {
        let root = util::root();
        let svg = std::fs::read_to_string(root.join(ICON_SVG)).expect("the brand icon.svg reads");
        let tmp = TempDir::new("iconset");
        let out = tmp.join("AppIcon.iconset");
        let written = rasterise_iconset(&svg, &out).unwrap();
        assert_eq!(written.len(), iconset_entries().len());
        for ((name, size), path) in iconset_entries().iter().zip(&written) {
            let png = std::fs::read(path).unwrap();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{name} is not a PNG");
            // The IHDR fields: width at byte 16, height at byte 20.
            let be32 = |start: usize| {
                u32::from_be_bytes([png[start], png[start + 1], png[start + 2], png[start + 3]])
            };
            assert_eq!(be32(16), *size, "{name} width");
            assert_eq!(be32(20), *size, "{name} height");
        }
    }

    /// A broken SVG is a clean error.
    #[test]
    fn rasterise_iconset_reports_a_broken_svg() {
        let tmp = TempDir::new("iconset-broken");
        let err = rasterise_iconset("not an svg", tmp.path()).unwrap_err();
        assert!(err.contains("cannot parse"), "{err}");
    }

    /// The dmg staging is the app plus the `Applications` link.
    #[test]
    fn stage_dmg_stages_the_app_and_the_applications_link() {
        let tmp = TempDir::new("stage");
        let app = tmp.join("Larrygram.app/Contents");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Info.plist"), "<plist/>").unwrap();
        let staging = tmp.join("staging");
        stage_dmg(&staging, tmp.join("Larrygram.app").as_path()).unwrap();
        assert!(staging.join("Larrygram.app/Contents/Info.plist").is_file());
        let link = std::fs::read_link(staging.join("Applications")).unwrap();
        assert_eq!(link, PathBuf::from("/Applications"));
    }

    /// The whole pipeline on hand-built inputs: the bundle tree, the
    /// rendered plist, the `.dmg`, and the tool calls in order (task R7
    /// checks).
    #[test]
    fn dist_mac_pipeline_builds_the_app_and_the_dmg() {
        let tmp = workspace();
        let root = tmp.path().to_path_buf();
        let dist = tmp.join("dist");
        let layout = Layout {
            root: root.clone(),
            dist: dist.clone(),
        };
        let mut sys = FakeSys::new(&root);
        let dmg = dist_mac(&mut sys, &layout, "macos", "aarch64").unwrap();

        assert_eq!(dmg, dist.join("Larrygram-1.0.0-alpha.1.dmg"));
        assert!(dmg.is_file());
        let app = layout.bundle();
        let mut found = Vec::new();
        collect_entries(&app, Path::new(""), &mut found);
        found.sort();
        assert_eq!(found, bundle_entries());
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("1.0.0-alpha.1"), "{plist}");
        assert!(!plist.contains("{{"), "{plist}");

        let calls = sys.calls.join("\n");
        assert!(
            calls.contains("build --release -p larrygram --target aarch64-apple-darwin"),
            "{calls}"
        );
        assert!(calls.contains("iconutil -c icns"), "{calls}");
        assert!(calls.contains("codesign --force --sign -"), "{calls}");
        assert!(calls.contains("codesign --verify --strict"), "{calls}");
        assert!(
            calls.contains("hdiutil create -volname Larrygram"),
            "{calls}"
        );
        assert!(calls.contains("-format UDZO"), "{calls}");

        // The scratch and staging directories are cleaned up.
        assert!(!dist.join(format!(".build-{}", std::process::id())).exists());
        assert!(
            !dist
                .join(format!(".dmg-staging-{}", std::process::id()))
                .exists()
        );
    }

    /// The pipeline refuses a non-Apple-silicon host before anything
    /// runs.
    #[test]
    fn dist_mac_rejects_the_wrong_host() {
        let tmp = workspace();
        let mut sys = FakeSys::new(tmp.path());
        let layout = Layout {
            root: tmp.path().to_path_buf(),
            dist: tmp.join("dist"),
        };
        let err = dist_mac(&mut sys, &layout, "macos", "x86_64").unwrap_err();
        assert!(err.contains("Apple silicon only"), "{err}");
        assert!(sys.calls.is_empty(), "{:?}", sys.calls);
    }

    /// Collects the file tree under `dir`, as paths relative to `base`.
    fn collect_entries(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let rel = base.join(entry.file_name());
            if entry.path().is_dir() {
                collect_entries(&entry.path(), &rel, out);
            } else {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
}
