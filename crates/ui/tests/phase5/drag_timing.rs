//! The ADR 0042 drag frame budget (task P5-11): a 1000-shape page and
//! N drag frames, measured one frame at a time.
//!
//! **What is measured.** One `egui_kittest` frame while a drag runs:
//! event handling, hit-testing and snapping over the 1000 shapes, the
//! overlay build and the egui paint — the per-frame UI work of ADR 0042
//! rule 3. The frame is the real canvas closure of [`crate::CanvasRig`]
//! (`canvas::show_with_overlay` + `ToolMachine` + command dispatch).
//! The page raster is not rebuilt while the drag runs (ADR 0042), so
//! the setup's first rasterization is outside the measurement, as is
//! the commit after the release.
//!
//! **The budget.** 8 ms per frame on the reference machine (ADR 0042
//! rule 3), multiplied by 2 on a known local machine and by 5 in CI
//! (the generous CI multiplier). The assertion runs only where we have
//! a number: on a known CPU/OS pair or with an explicit
//! `LARRYGRAM_DRAG_BUDGET_MS`. On any other hardware the test still
//! runs and prints the measurements, then skips the assertion with its
//! reason — a timing failure on unknown hardware would be noise, not a
//! finding (task P5-11).
//!
//! A panic is the bug (AGENTS.md section 6).

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::{Duration, Instant};

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_model::{Document, Page, PageId};

use crate::{CanvasRig, PAGE_HEIGHT, ZOOM, cell, screen, shape};

/// The page population of the budget (ADR 0042: "a 1000-shape page").
const SHAPES: usize = 1000;

/// The measured drag frames (task P5-11: "N drag frames").
const FRAMES: usize = 30;

/// The ADR 0042 budget on the reference machine.
const BUDGET_MS: f64 = 8.0;

/// The generous CI multiplier of the budget.
const CI_MULTIPLIER: f64 = 5.0;

/// The local-machine multiplier of the budget.
const LOCAL_MULTIPLIER: f64 = 2.0;

/// The machine class the budget applies to (module docs): the
/// assertion runs on a known pair or with `LARRYGRAM_DRAG_BUDGET_MS`.
fn known_hardware() -> Option<String> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64" | "x86_64", "macos" | "linux") => Some(format!(
            "{} {}",
            std::env::consts::ARCH,
            std::env::consts::OS
        )),
        _ => None,
    }
}

/// The budget to assert, in milliseconds, or `None` to skip (module
/// docs).
fn budget() -> Option<(f64, String)> {
    if let Ok(explicit) = std::env::var("LARRYGRAM_DRAG_BUDGET_MS")
        && let Ok(value) = explicit.trim().parse::<f64>()
    {
        return Some((value, format!("LARRYGRAM_DRAG_BUDGET_MS={value}")));
    }
    let hardware = known_hardware()?;
    let multiplier = if std::env::var_os("CI").is_some() {
        CI_MULTIPLIER
    } else {
        LOCAL_MULTIPLIER
    };
    Some((
        BUDGET_MS * multiplier,
        format!("{hardware}, budget {BUDGET_MS} ms x {multiplier}"),
    ))
}

/// A 1000-shape page: a grid of 1 x 0.5 in rectangles over the page.
fn app_with_grid(count: usize) -> AppCtx {
    let mut doc = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    page.sheet.cells = vec![cell("PageWidth", 8.5), cell("PageHeight", PAGE_HEIGHT)];
    let columns = 25usize;
    for index in 0..count {
        let column = index % columns;
        let row = index / columns;
        let x = 0.5 + (column as f64) * 0.32;
        let y = 10.5 - (row as f64) * 0.4;
        let id = doc.add_shape(shape(x, y, i32::try_from(index).unwrap_or(0) + 1));
        page.shapes.push(id);
    }
    doc.pages.push(page);
    let mut app = AppCtx::new(crate::phase5_settings());
    app.open_document(DocumentSession::new(doc, None));
    if let Some(session) = app.active_document_mut() {
        session.view.zoom = ZOOM;
    }
    app
}

/// The ADR 0042 drag budget over a 1000-shape page (module docs).
#[test]
fn a_drag_frame_stays_inside_the_adr_0042_budget() {
    let app = app_with_grid(SHAPES);
    let mut rig = CanvasRig::new(app);
    // The initial raster (1000 shapes) and the press frame are setup:
    // the commit re-rasterization may take as long as a normal raster
    // (ADR 0042 rule 3).
    rig.run();
    let start = screen(1.0, 8.0);
    rig.down(start);
    rig.run();

    let mut frames: Vec<Duration> = Vec::with_capacity(FRAMES);
    let mut settled = 0u64;
    for frame in 0..FRAMES {
        // A slow walk across the page: every frame re-hit-tests and
        // re-snaps over the whole population. The harness can settle a
        // run in more than one frame; the number is the divisor, so
        // every recorded cost is one frame (module docs).
        let x = 1.0 + (frame as f64) * 0.12;
        let pos = screen(x, 8.0 - (frame as f64) * 0.02);
        let before = Instant::now();
        let steps = rig.move_to(pos).max(1);
        settled += steps;
        frames.push(before.elapsed() / u32::try_from(steps).unwrap_or(u32::MAX).max(1));
    }
    rig.up(screen(6.0, 7.5));

    let mut sorted = frames.clone();
    sorted.sort();
    let median = sorted[sorted.len() / 2];
    let worst = sorted[sorted.len() - 1];
    let p95 = sorted[(sorted.len() * 95) / 100];
    eprintln!(
        "drag frames over {SHAPES} shapes: median {:?}, p95 {:?}, worst {:?}          ({FRAMES} runs, {settled} frames)",
        median, p95, worst
    );

    let Some((budget_ms, reason)) = budget() else {
        eprintln!(
            "skipping the budget assertion: unknown hardware ({} {}); \
             set LARRYGRAM_DRAG_BUDGET_MS to assert",
            std::env::consts::ARCH,
            std::env::consts::OS
        );
        return;
    };
    let budget = Duration::from_secs_f64(budget_ms / 1000.0);
    assert!(
        median <= budget,
        "the median drag frame took {median:?}, over the {budget_ms} ms budget ({reason}); \
         p95 {p95:?}, worst {worst:?}"
    );
}
