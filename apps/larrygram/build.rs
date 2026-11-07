//! The desktop app's build script (task R11 goal 3): the Windows
//! icon resource.
//!
//! On a Windows host building for Windows, the script rasterises the
//! brand `app-icon.svg` at build time into a multi-size `.ico` (the
//! shared `build_support/icon.rs`, also unit tested through
//! `tests/build_support.rs`) and embeds it with `winresource` (MIT),
//! so Explorer, the zip listing and taskbar pins show the Larrygram
//! icon.
//!
//! When cross-building for Windows from a non-Windows host (task R14),
//! the script tries the same step with `llvm-rc` from Homebrew LLVM
//! as the resource compiler. If that tool chain is not available the
//! step is skipped with a warning — the build still succeeds, just
//! without the icon in the `.exe` (AGENTS.md section 6).
//!
//! The `.ico` is derived from the brand art at build time and
//! is never committed (task R11 master notes). The build tools are
//! Windows-host build-dependencies (task R12 goal 4): every other
//! host compiles nothing extra and skips the step — no icon, no
//! error, the build still succeeds (AGENTS.md section 6).
//!
//! The build id (task R11 goal 1) does **not** live here: the About
//! window (`crates/ui`) and the CLI are other packages and cannot see
//! this script's `cargo:rustc-env`, so the id is resolved in
//! `larrygram_commands::build` instead.

// Task R12 goal 4: the icon step is Windows-host-only, and so are its
// build tools (`resvg`, `winresource`).
#[cfg(windows)]
#[path = "build_support/icon.rs"]
mod icon;

#[cfg(windows)]
use std::env;
#[cfg(windows)]
use std::path::{Path, PathBuf};

/// Runs the icon step, warning (never failing) on any error (task R11
/// goal 3, AGENTS.md section 6).
fn main() {
    // Task R12 goal 4: without a rerun-if-changed cargo re-runs the
    // script on every file change of the package.
    println!("cargo:rerun-if-changed=build.rs");

    // Task R14: when cross-building for Windows from a non-Windows
    // host, try to embed the icon with llvm-rc. Skip on failure.
    #[cfg(not(windows))]
    {
        if let Ok(target_os) = std::env::var("CARGO_CFG_TARGET_OS") {
            if target_os == "windows" {
                if let Err(error) = embed_windows_icon_cross() {
                    eprintln!("cargo:warning=windows icon resource skipped (cross-build): {error}");
                }
            }
        }
    }

    #[cfg(windows)]
    if let Err(error) = embed_windows_icon() {
        eprintln!("cargo:warning=windows icon resource skipped: {error}");
    }
}

/// Embeds the Windows icon resource when cross-building (task R14):
/// rasterises the SVG, writes a `.ico`, then uses `winresource` with
/// `llvm-rc` and `llvm-ar` from Homebrew LLVM. Warns and returns
/// `Ok(())` if no resource compiler is found — the build still
/// succeeds, just without the icon (AGENTS.md section 6).
#[cfg(not(windows))]
fn embed_windows_icon_cross() -> Result<(), String> {
    use std::path::Path;
    let svg_path = icon_svg_path_cross();
    println!("cargo:rerun-if-changed={}", svg_path.display());
    let svg = std::fs::read_to_string(&svg_path)
        .map_err(|error| format!("cannot read {}: {error}", svg_path.display()))?;

    // Build the .ico using resvg (available as a direct dependency).
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
        .map_err(|error| format!("cannot parse app-icon.svg: {error}"))?;
    let width = tree.size().width().max(1.0);
    let height = tree.size().height().max(1.0);

    let ico_sizes: [u32; 6] = [16, 32, 48, 64, 128, 256];
    let mut images = Vec::with_capacity(ico_sizes.len());
    for size in ico_sizes {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| format!("cannot allocate a {size}x{size} pixmap"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(size as f32 / width, size as f32 / height),
            &mut pixmap.as_mut(),
        );
        let png = pixmap
            .encode_png()
            .map_err(|error| format!("cannot encode the {size} px icon: {error}"))?;
        images.push((size, png));
    }
    let ico = build_ico_container(&images);

    let out_dir = std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    let ico_path = Path::new(&out_dir).join("larrygram.ico");
    std::fs::write(&ico_path, ico)
        .map_err(|error| format!("cannot write {}: {error}", ico_path.display()))?;
    let ico_name = ico_path
        .to_str()
        .ok_or("the .ico path is not valid text")?;

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico_name);

    // Find LLVM tools for cross-compilation (task R14 goal 2).
    // Homebrew LLVM installs at /opt/homebrew/opt/llvm/bin/.
    let llvm_bin = Path::new("/opt/homebrew/opt/llvm/bin");
    let rc = llvm_bin.join("llvm-rc");
    let ar = llvm_bin.join("llvm-ar");
    if rc.is_file() && ar.is_file() {
        if let (Some(rc_s), Some(ar_s)) = (rc.to_str(), ar.to_str()) {
            res.set_windres_path(rc_s);
            res.set_ar_path(ar_s);
        }
    }

    res.compile()
        .map_err(|error| format!("cannot embed {ico_name}: {error}"))?;
    Ok(())
}

/// Builds an ICO container from PNG images (inline version for the
/// cross-build path, matching `build_support/icon.rs`).
#[cfg(not(windows))]
fn build_ico_container(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = u32::from(count) * 16 + 6;
    for (size, png) in images {
        let dim = if *size >= 256 {
            0u8
        } else {
            u8::try_from(*size).unwrap_or(0)
        };
        out.push(dim);
        out.push(dim);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

/// The app icon path (ADR 0067) for the cross-build path.
#[cfg(not(windows))]
fn icon_svg_path_cross() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crates")
        .join("ui")
        .join("assets")
        .join("brand")
        .join("app-icon.svg")
}

/// Embeds the Windows icon resource (task R11 goal 3), or does
/// nothing when the target is not Windows (task R12 goal 4: this
/// module compiles on a Windows host only).
#[cfg(windows)]
fn embed_windows_icon() -> Result<(), String> {
    // A build script runs on the host: `cfg!(windows)` is the host
    // check and `CARGO_CFG_TARGET_OS` the target check (task R11 goal
    // 3: "Windows host only; other hosts skip it"). The resource is
    // embedded by the host's `rc` tool, so only a native Windows
    // build can make it.
    if !cfg!(windows) {
        return Ok(());
    }
    if env::var("CARGO_CFG_TARGET_OS").map_err(|error| error.to_string())? != "windows" {
        return Ok(());
    }
    let svg_path = icon_svg_path();
    println!("cargo:rerun-if-changed={}", svg_path.display());
    let svg = std::fs::read_to_string(&svg_path)
        .map_err(|error| format!("cannot read {}: {error}", svg_path.display()))?;
    let ico = icon::build_ico(&svg)?;
    let ico_path =
        PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?).join("larrygram.ico");
    std::fs::write(&ico_path, ico)
        .map_err(|error| format!("cannot write {}: {error}", ico_path.display()))?;
    let ico_name = ico_path.to_str().ok_or("the .ico path is not valid text")?;
    winresource::WindowsResource::new()
        .set_icon(ico_name)
        .compile()
        .map_err(|error| format!("cannot embed {ico_name}: {error}"))?;
    Ok(())
}

/// The app icon (ADR 0067): `crates/ui/assets/brand/app-icon.svg`.
#[cfg(windows)]
fn icon_svg_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crates")
        .join("ui")
        .join("assets")
        .join("brand")
        .join("app-icon.svg")
}
