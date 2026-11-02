//! `cargo xtask dist mac`: builds the macOS `Larrygram.app` bundle and
//! the `Larrygram-<version>-macos-arm64.dmg` disk image (tasks R7 and
//! R9).
//!
//! The artifacts go under `<target dir>/dist/` (task R9 goal 1). The
//! build steps:
//!
//! 1. build the release binaries of `larrygram` and `larrygram-cli`
//!    for the one supported target, [`TARGET`] (Apple silicon, task R7
//!    decisions; both binaries in every package, task R9 decisions);
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
//!
//! This module only compiles on unix hosts (the bundle tools and the
//! symlink are unix); on any other host `cargo xtask dist mac` refuses
//! with a clear message before reaching it.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{
    BINARIES, ICON_SVG, Layout, MAC_TARGET, Sys, licence_files, package_name, render_png,
    version_from_root,
};
use crate::util::Result;

/// The only supported target triple: Apple silicon (task R7
/// decisions). No universal binary for now.
pub const TARGET: &str = MAC_TARGET;

/// The bundle directory name under the output directory.
pub const BUNDLE_NAME: &str = "Larrygram.app";

/// The icon file name inside `Contents/Resources`, and the
/// `CFBundleIconFile` value.
pub const ICON_FILE: &str = "Larrygram.icns";

/// The `.dmg` volume name shown by Finder when it is mounted.
pub const VOLUME_NAME: &str = "Larrygram";

/// The committed `Info.plist` template, relative to the workspace root.
const INFO_PLIST_TEMPLATE: &str = "apps/larrygram/macos/Info.plist";

/// The `.iconset` directory name `iconutil` expects (it must end in
/// `.iconset`).
const ICONSET_DIR: &str = "AppIcon.iconset";

/// The `<os>` and `<arch>` in the package name (task R9 decisions).
const PACKAGE_OS: &str = "macos";
const PACKAGE_ARCH: &str = "arm64";

/// The `.app` directory under the output directory.
pub fn bundle_dir(layout: &Layout) -> PathBuf {
    layout.dist.join(BUNDLE_NAME)
}

/// The package name of the `.dmg` for `version` (task R9 decisions:
/// `Larrygram-<version>-macos-arm64.dmg`).
pub fn dmg_name(version: &str) -> Result<String> {
    package_name(version, PACKAGE_OS, PACKAGE_ARCH, "dmg")
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

/// The files the bundle contains, relative to the `.app` directory,
/// sorted: the two executables, the plist, the icon and the licence
/// files. The tests pin the bundle layout against this list (task R7
/// checks, task R9 decisions: both binaries in every package).
pub fn bundle_entries() -> Vec<String> {
    let mut entries = vec!["Contents/Info.plist".to_owned()];
    for binary in BINARIES {
        entries.push(format!("Contents/MacOS/{binary}"));
    }
    entries.push(format!("Contents/Resources/{ICON_FILE}"));
    for (dest, _) in licence_files() {
        entries.push(format!("Contents/Resources/{dest}"));
    }
    entries.sort();
    entries
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
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut written = Vec::new();
    for (name, size) in iconset_entries() {
        let png = render_png(svg, *size)?;
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

/// Writes the `.app` bundle under `app`: the executables (made
/// executable), the rendered `Info.plist`, the packed icon and the
/// licence files from `root`. `executables` maps the bundle file name
/// to the built binary, in [`BINARIES`] order. Returns the relative
/// paths written, in the order of [`bundle_entries`] (task R7 bundle
/// layout, task R9 decisions).
pub fn write_bundle(
    root: &Path,
    app: &Path,
    executables: &[(String, PathBuf)],
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

    for (name, executable) in executables {
        let bin_path = macos.join(name);
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
        written.push(format!("Contents/MacOS/{name}"));
    }

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

/// Copies one directory tree from `src` into `dst` (created).
pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
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

/// The argument spelling of a path (a `&str` for the command line).
pub fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The whole build through `sys`: release binaries, bundle, ad-hoc
/// signature and `.dmg`. Returns the `.dmg` path. `os` and `arch` are
/// the host's `std::env::consts` values (see [`super::check_host`]);
/// the tests pass their own and use a fake [`Sys`], so the whole
/// pipeline runs on hand-built inputs (task R7 checks).
pub fn dist_mac(sys: &mut dyn Sys, layout: &Layout, os: &str, arch: &str) -> Result<PathBuf> {
    super::check_host(super::Platform::Mac, os, arch)?;
    let root = &layout.root;
    let dist = &layout.dist;
    std::fs::create_dir_all(dist).map_err(|e| format!("cannot create {}: {e}", dist.display()))?;

    let version = version_from_root(root)?;
    println!("dist mac: building Larrygram {version} for {TARGET}");

    // 1. The release binaries (AGENTS.md section 2: no CARGO_TARGET_DIR).
    sys.run(
        &crate::util::cargo(),
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
    let mut executables = Vec::new();
    for binary in BINARIES {
        let built = layout.binary(TARGET, binary);
        if !built.is_file() {
            return Err(format!(
                "cargo built no release binary at {}",
                built.display()
            ));
        }
        executables.push(((*binary).to_owned(), built));
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
    let app = bundle_dir(layout);
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|e| format!("cannot clear {}: {e}", app.display()))?;
    }
    let bundle_result = write_bundle(root, &app, &executables, &info_plist, &icns);
    let _ = std::fs::remove_dir_all(&scratch);
    bundle_result?;

    // 5. The ad-hoc signature (task R7 decisions: no Developer ID).
    let app_arg = path_arg(&app);
    sys.run("codesign", &["--force", "--sign", "-", &app_arg])?;
    sys.run("codesign", &["--verify", "--strict", &app_arg])?;

    // 6. The disk image.
    let staging = dist.join(format!(".dmg-staging-{}", std::process::id()));
    stage_dmg(&staging, &app)?;
    let dmg = layout.package(&dmg_name(&version)?);
    make_dmg(sys, &staging, &dmg)?;

    println!("dist mac: {}", app.display());
    println!("dist mac: {}", dmg.display());
    Ok(dmg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util;
    use crate::util::test_support::TempDir;

    /// A [`Sys`] that records every call and fabricates the outputs:
    /// `cargo build` writes dummy binaries, `iconutil` a dummy
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
                    for binary in BINARIES {
                        let bin = self
                            .root
                            .join("target")
                            .join(TARGET)
                            .join("release")
                            .join(binary);
                        std::fs::create_dir_all(bin.parent().unwrap_or(Path::new("/")))
                            .map_err(|e| format!("fake cargo: {e}"))?;
                        std::fs::write(&bin, b"fake binary")
                            .map_err(|e| format!("fake cargo: {e}"))?;
                    }
                    Ok(())
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

    /// The `.dmg` name follows the task R9 decision scheme.
    #[test]
    fn dmg_name_follows_the_scheme() {
        assert_eq!(
            dmg_name("1.0.0-alpha.1").unwrap(),
            "Larrygram-1.0.0-alpha.1-macos-arm64.dmg"
        );
    }

    /// The bundle entries are the two executables, the plist, the icon
    /// and the licence files.
    #[test]
    fn bundle_entries_hold_both_binaries_and_the_licences() {
        let entries = bundle_entries();
        assert!(entries.contains(&"Contents/MacOS/larrygram".to_owned()));
        assert!(entries.contains(&"Contents/MacOS/larrygram-cli".to_owned()));
        assert!(entries.contains(&"Contents/Info.plist".to_owned()));
        assert!(entries.contains(&format!("Contents/Resources/{ICON_FILE}")));
        for (dest, _) in licence_files() {
            assert!(entries.contains(&format!("Contents/Resources/{dest}")));
        }
        assert_eq!(
            entries.len(),
            1 + BINARIES.len() + 1 + licence_files().len()
        );
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
        let cli = tmp.join("built-cli");
        std::fs::write(&cli, b"cli").unwrap();
        let icns = tmp.join("Larrygram.icns");
        std::fs::write(&icns, b"icns").unwrap();
        let executables = vec![
            ("larrygram".to_owned(), bin),
            ("larrygram-cli".to_owned(), cli),
        ];

        let written = write_bundle(&root, &app, &executables, "<plist/>", &icns).unwrap();
        assert_eq!(written, bundle_entries());
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
            "<plist/>"
        );
        for (name, _source) in &executables {
            let mode = std::fs::metadata(app.join("Contents/MacOS").join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755, "{name} must be executable");
        }
        assert_eq!(
            std::fs::read(app.join("Contents/MacOS/larrygram")).unwrap(),
            b"binary"
        );
        assert_eq!(
            std::fs::read(app.join("Contents/MacOS/larrygram-cli")).unwrap(),
            b"cli"
        );
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
        let executables = vec![("larrygram".to_owned(), bin)];
        let err = write_bundle(
            tmp.join("empty").as_path(),
            tmp.join("dist/Larrygram.app").as_path(),
            &executables,
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

    /// `copy_dir` copies a tree, files and subdirectories.
    #[test]
    fn copy_dir_copies_trees() {
        let tmp = TempDir::new("dist-copy");
        std::fs::create_dir_all(tmp.join("src/inner")).unwrap();
        std::fs::write(tmp.join("src/inner/file.txt"), "x").unwrap();
        copy_dir(&tmp.join("src"), &tmp.join("dst")).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join("dst/inner/file.txt")).unwrap(),
            "x"
        );
    }

    /// `stage_dmg` replaces a stale staging directory.
    #[test]
    fn stage_dmg_clears_a_stale_staging_dir() {
        let tmp = TempDir::new("stage-stale");
        let app = tmp.join("Larrygram.app/Contents");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Info.plist"), "<plist/>").unwrap();
        let staging = tmp.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("stale.txt"), "old").unwrap();
        stage_dmg(&staging, tmp.join("Larrygram.app").as_path()).unwrap();
        assert!(!staging.join("stale.txt").exists());
        assert!(staging.join("Larrygram.app/Contents/Info.plist").is_file());
    }

    /// A fake [`Sys`] that records the calls and produces nothing.
    struct BareSys(Vec<String>);

    impl Sys for BareSys {
        fn run(&mut self, program: &str, args: &[&str]) -> Result<()> {
            self.0.push(format!("{program} {}", args.join(" ")));
            Ok(())
        }
    }

    /// The pipeline reports a cargo build that produces no binary and
    /// an iconutil that produces no icon (the two clean error paths).
    #[test]
    fn dist_mac_reports_missing_build_outputs() {
        let tmp = workspace();
        let layout = Layout::under(tmp.path());
        let mut sys = BareSys(Vec::new());
        let err = dist_mac(&mut sys, &layout, "macos", "aarch64").unwrap_err();
        assert!(err.contains("cargo built no release binary"), "{err}");

        // Now let the fake cargo produce the binaries, but nothing
        // else: iconutil must then produce the .icns and fails to.
        let mut sys = FakeSys::new(tmp.path());
        let err = with_empty_iconutil(&mut sys, &layout).unwrap_err();
        assert!(err.contains("iconutil produced no"), "{err}");
    }

    /// Runs the pipeline with a fake whose iconutil writes nothing.
    fn with_empty_iconutil(sys: &mut FakeSys, layout: &Layout) -> Result<PathBuf> {
        struct EmptyIconutil<'a>(&'a mut FakeSys);
        impl Sys for EmptyIconutil<'_> {
            fn run(&mut self, program: &str, args: &[&str]) -> Result<()> {
                let is_iconutil = Path::new(program)
                    .file_name()
                    .map(|n| n == "iconutil")
                    .unwrap_or(false);
                if is_iconutil {
                    self.0.calls.push(format!("{program} {}", args.join(" ")));
                    return Ok(());
                }
                self.0.run(program, args)
            }
        }
        dist_mac(&mut EmptyIconutil(sys), layout, "macos", "aarch64")
    }

    /// The whole pipeline on hand-built inputs: the bundle tree, the
    /// rendered plist, the `.dmg`, and the tool calls in order (task R7
    /// checks).
    #[test]
    fn dist_mac_pipeline_builds_the_app_and_the_dmg() {
        let tmp = workspace();
        let root = tmp.path().to_path_buf();
        let layout = Layout::under(&root);
        let mut sys = FakeSys::new(&root);
        let dmg = dist_mac(&mut sys, &layout, "macos", "aarch64").unwrap();

        assert_eq!(
            dmg,
            layout.dist.join("Larrygram-1.0.0-alpha.1-macos-arm64.dmg")
        );
        assert!(dmg.is_file());
        let app = bundle_dir(&layout);
        let mut found = Vec::new();
        collect_entries(&app, Path::new(""), &mut found);
        found.sort();
        assert_eq!(found, bundle_entries());
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("1.0.0-alpha.1"), "{plist}");
        assert!(!plist.contains("{{"), "{plist}");

        let calls = sys.calls.join("\n");
        assert!(
            calls.contains(
                "build --release -p larrygram -p larrygram-cli --target aarch64-apple-darwin"
            ),
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
        assert!(
            !layout
                .dist
                .join(format!(".build-{}", std::process::id()))
                .exists()
        );
        assert!(
            !layout
                .dist
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
        let layout = Layout::under(tmp.path());
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
