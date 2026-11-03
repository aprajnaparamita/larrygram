//! The desktop app's build script (task R11 goal 3): the Windows
//! icon resource.
//!
//! On a Windows host building for Windows, the script rasterises the
//! brand `icon.svg` at build time into a multi-size `.ico` (the
//! shared `build_support/icon.rs`, also unit tested through
//! `tests/build_support.rs`) and embeds it with `winresource` (MIT),
//! so Explorer, the zip listing and taskbar pins show the Larrygram
//! icon. The `.ico` is derived from the brand art at build time and
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
    #[cfg(windows)]
    if let Err(error) = embed_windows_icon() {
        eprintln!("cargo:warning=windows icon resource skipped: {error}");
    }
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

/// The brand `icon.svg` (ADR 0067): `crates/ui/assets/brand/icon.svg`.
#[cfg(windows)]
fn icon_svg_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crates")
        .join("ui")
        .join("assets")
        .join("brand")
        .join("icon.svg")
}
