//! The website's screenshots (task P7-38), rendered by Larrygram
//! itself: one `#[ignore]` test per app shot, named after the shot id
//! (a Rust identifier cannot hold the id's dashes, so `hero-panel-
//! modern` is the test `hero_panel_modern`). Each builds its state
//! through real input in the full shell (`egui_kittest`), renders it
//! with the wgpu renderer, crops it to the shot's aspect and writes
//! `<out>/<id>.png`.
//!
//! `<out>` is `$LARRYGRAM_SCREENSHOTS_DIR`, or `<target dir>/
//! screenshots` when it is unset (the target dir from `cargo
//! metadata`). The normal way to run them is
//!
//! ```sh
//! cargo xtask screenshots [--only <id>]... [--to <dir>]
//! ```
//!
//! which sets the variable, checks every PNG against the table below
//! and optionally copies the shots into the website's assets. The
//! table ([`SHOTS`]) is this test target's own copy of the list in
//! `xtask/src/screenshots.rs`; an xtask unit test checks the two have
//! the same ids.
//!
//! `cli-render` (a terminal and a browser) needs a real screen and is
//! taken by hand, not generated here. The helper rules (exact aspect,
//! minimum size, the 3x fallback) have ordinary unit tests in
//! [`helpers`], which the gate runs.

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod flowchart;
mod helpers;
mod rig;
mod shots;

use std::path::PathBuf;

/// One website shot this target renders.
pub struct ShotSpec {
    /// The website's shot id (the PNG's file name stem).
    pub id: &'static str,
    /// The exact crop aspect (width : height).
    pub aspect: (u32, u32),
    /// The minimum PNG size in pixels.
    pub min: (u32, u32),
}

/// The shots `cargo xtask screenshots` generates (this target's copy
/// of the xtask table; `cli-render` is taken by hand and not here).
pub const SHOTS: &[ShotSpec] = &[
    ShotSpec {
        id: "hero-panel-modern",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    ShotSpec {
        id: "layout-panel-classic",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    ShotSpec {
        id: "connectors-reroute",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    ShotSpec {
        id: "shapes-window",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    ShotSpec {
        id: "text-editing",
        aspect: (4, 3),
        min: (1600, 1200),
    },
    ShotSpec {
        id: "pages-tabs",
        aspect: (16, 9),
        min: (1920, 1080),
    },
    ShotSpec {
        id: "welcome-wizard",
        aspect: (16, 10),
        min: (2560, 1600),
    },
    ShotSpec {
        id: "about-window",
        aspect: (16, 10),
        min: (2560, 1600),
    },
];

/// The spec of one shot id.
pub fn spec(id: &str) -> &'static ShotSpec {
    SHOTS
        .iter()
        .find(|shot| shot.id == id)
        .unwrap_or_else(|| panic!("no shot named {id:?}"))
}

/// The output directory: `$LARRYGRAM_SCREENSHOTS_DIR`, or
/// `<target dir>/screenshots` when it is unset (the target dir as
/// [`target_dir`] asks cargo for it, the same rule as the xtask).
fn out_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LARRYGRAM_SCREENSHOTS_DIR") {
        return PathBuf::from(dir);
    }
    target_dir().join("screenshots")
}

/// The cargo target directory as `cargo metadata` reports it, which
/// honours `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` and
/// `build.target-dir` (`$CARGO` is the cargo that runs the tests).
fn target_dir() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    PathBuf::from(
        json["target_directory"]
            .as_str()
            .expect("cargo metadata names the target directory"),
    )
}

/// Builds, renders, crops and writes one shot.
fn render_shot(id: &str) {
    let spec = spec(id);
    let image = helpers::capture(|scale| shots::build(id, scale), spec.aspect, spec.min);
    // Every pixel is painted: an unpainted margin (the harness frame
    // the rig draws over) would show as a transparent edge.
    assert!(
        image.pixels().all(|pixel| pixel.0[3] == 255),
        "{id}: the shot has transparent pixels"
    );
    let out = out_dir();
    std::fs::create_dir_all(&out).expect("the output directory exists");
    let path = out.join(format!("{id}.png"));
    image
        .save(&path)
        .unwrap_or_else(|e| panic!("write {path:?}: {e}"));
    println!(
        "wrote {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
}

#[test]
fn the_default_output_is_under_the_target_dir() {
    // This test binary lives in `<target dir>/<profile>/deps/`.
    let exe = std::env::current_exe().expect("the test binary's path");
    let target = target_dir();
    assert!(
        exe.starts_with(&target),
        "{} is under {}",
        exe.display(),
        target.display()
    );
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn hero_panel_modern() {
    render_shot("hero-panel-modern");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn layout_panel_classic() {
    render_shot("layout-panel-classic");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn connectors_reroute() {
    render_shot("connectors-reroute");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn shapes_window() {
    render_shot("shapes-window");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn text_editing() {
    render_shot("text-editing");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn pages_tabs() {
    render_shot("pages-tabs");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn welcome_wizard() {
    render_shot("welcome-wizard");
}

#[test]
#[ignore = "a website screenshot; run via `cargo xtask screenshots`"]
fn about_window() {
    render_shot("about-window");
}
