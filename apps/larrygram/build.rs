//! The desktop app's build script (task R11 goal 3): the Windows
//! icon resource.
//!
//! When the target is Windows, the script rasterises the brand
//! `app-icon.svg` at build time into a multi-size `.ico` (the shared
//! `build_support/icon.rs`, also unit tested through
//! `tests/build_support.rs`) and embeds it with `winresource` (MIT),
//! so Explorer, the zip listing and taskbar pins show the Larrygram
//! icon. A Windows host uses its own `rc`; a cross-build from another
//! host (task R14, ADR 0085) uses `llvm-rc` and `llvm-ar` from
//! Homebrew LLVM. Every other target skips the step. A missing tool or
//! any other failure is a warning, never an error: the build still
//! succeeds, without the icon (AGENTS.md section 6).
//!
//! The `.ico` is derived from the brand art at build time and is never
//! committed (task R11 master notes).
//!
//! The build id (task R11 goal 1) does **not** live here: the About
//! window (`crates/ui`) and the CLI are other packages and cannot see
//! this script's `cargo:rustc-env`, so the id is resolved in
//! `larrygram_commands::build` instead.

#[path = "build_support/icon.rs"]
mod icon;

use std::env;
use std::path::{Path, PathBuf};

/// Runs the icon step, warning (never failing) on any error (task R11
/// goal 3, AGENTS.md section 6).
fn main() {
    // Task R12 goal 4: without a rerun-if-changed cargo re-runs the
    // script on every file change of the package.
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if let Err(error) = embed_windows_icon() {
        println!("cargo:warning=windows icon resource skipped: {error}");
    }
}

/// Embeds the Windows icon resource (task R11 goal 3; task R14 for a
/// cross-build).
fn embed_windows_icon() -> Result<(), String> {
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
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(ico_name);
    if !cfg!(windows) {
        // A cross-build (task R14): Homebrew LLVM's resource compiler
        // and archiver.
        let llvm = Path::new("/opt/homebrew/opt/llvm/bin");
        let (rc, ar) = (llvm.join("llvm-rc"), llvm.join("llvm-ar"));
        if !rc.is_file() || !ar.is_file() {
            return Err(format!(
                "no llvm-rc and llvm-ar in {} (brew install llvm)",
                llvm.display()
            ));
        }
        let rc = rc.to_str().ok_or("the llvm-rc path is not valid text")?;
        let ar = ar.to_str().ok_or("the llvm-ar path is not valid text")?;
        resource.set_windres_path(rc).set_ar_path(ar);
    }
    resource
        .compile()
        .map_err(|error| format!("cannot embed {ico_name}: {error}"))
}

/// The app icon (ADR 0067): `crates/ui/assets/brand/app-icon.svg`.
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
