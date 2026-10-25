//! The Phase 7 golden images (task P7-10, ADR 0065 item 5): the four
//! window renders — Panel Classic with a menu open, Panel Modern on
//! the Home tab, Panel Modern with Picture Tools and the File view —
//! plus the welcome wizard's two pages and the About window.
//!
//! Larrygram renders them itself: the goldens come from the same
//! widgets the apps draw, through the `egui_kittest` wgpu renderer.
//! Comparison uses the ADR 0027 metric (the same rule as the render
//! goldens): the images must have the same dimensions, each channel
//! may differ by at most [`MAX_CHANNEL_DELTA`] of 255, and at most
//! [`MAX_FRACTION`] of the pixels may exceed that. It absorbs GPU and
//! libm noise while catching a shifted control, a missing icon or a
//! wrong colour. Regenerate with
//!
//! ```sh
//! LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-ui --test phase7 golden
//! ```
//!
//! and review the changed files as an image diff (ADR 0065).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use egui_kittest::Harness;
use larrygram_ui::wizard;
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig, sets};

/// The largest per-channel difference, of 255 (ADR 0027).
const MAX_CHANNEL_DELTA: u8 = 2;

/// The largest fraction of pixels that may exceed
/// [`MAX_CHANNEL_DELTA`] (ADR 0027: 0.5%).
const MAX_FRACTION: f64 = 0.005;

/// The golden directory of this test: `crates/ui/tests/phase7/golden/`.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("phase7")
        .join("golden")
}

/// The ADR 0027 metric: [`Err`] with a message when the images
/// differ, [`Ok`] when they match.
pub fn compare_images(
    expected: &image::RgbaImage,
    actual: &image::RgbaImage,
) -> Result<(), String> {
    if expected.dimensions() != actual.dimensions() {
        return Err(format!(
            "dimensions differ: expected {:?}, got {:?}",
            expected.dimensions(),
            actual.dimensions()
        ));
    }
    let mut over = 0usize;
    let mut total = 0usize;
    let mut worst = String::new();
    for (x, (e, a)) in expected.pixels().zip(actual.pixels()).enumerate() {
        total += 1;
        let delta =
            e.0.iter()
                .zip(a.0.iter())
                .map(|(e, a)| e.abs_diff(*a))
                .max()
                .unwrap_or(0);
        if delta > MAX_CHANNEL_DELTA {
            over += 1;
            if worst.is_empty() {
                let (w, _h) = expected.dimensions();
                let (px, py) = (x as u32 % w, x as u32 / w);
                worst = format!("first at ({px},{py}): {e:?} vs {a:?}");
            }
        }
    }
    let fraction = over as f64 / total.max(1) as f64;
    if fraction > MAX_FRACTION {
        return Err(format!(
            "{over} of {total} pixels ({:.3}%) exceed the delta: {worst}",
            fraction * 100.0
        ));
    }
    Ok(())
}

/// Checks one golden: compares `image` with `tests/phase7/golden/
/// <name>.png`, or rewrites that file when `LARRYGRAM_UPDATE_GOLDEN=1`
/// (the render goldens' convention, ADR 0065).
fn check_golden(name: &str, image: &image::RgbaImage) {
    let path = golden_dir().join(format!("{name}.png"));
    let update = std::env::var_os("LARRYGRAM_UPDATE_GOLDEN").is_some_and(|v| v == "1");
    if update {
        std::fs::create_dir_all(golden_dir()).expect("the golden directory");
        image
            .save(&path)
            .unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        println!("updated golden {name}.png");
        return;
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| {
            panic!(
                "the golden {path:?} is missing or broken ({e}); \
                 regenerate with LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-ui --test phase7 golden"
            )
        })
        .to_rgba8();
    compare_images(&expected, image).unwrap_or_else(|message| {
        panic!(
            "the golden {name}.png differs: {message}\n\
             regenerate with LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-ui --test phase7 golden"
        )
    });
}

/// Renders the shell of one layout with the fixed document and calls
/// `prepare` (open a menu, a tab, ...) before the snapshot.
fn shell_golden(kind: LayoutKind, name: &str, prepare: impl FnOnce(&mut ShellRig)) {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app(app);
    // Settle: the first frames apply the theme and rasterise the
    // icons, so the snapshot is stable.
    for _ in 0..3 {
        rig.run();
    }
    prepare(&mut rig);
    rig.run();
    rig.run();
    let image = rig.render();
    check_golden(name, &image);
}

/// A small harness drawing `body`, with the Panel Modern theme and the
/// brand image loaders installed (as the apps do at start-up).
fn window_golden(size: egui::Vec2, body: impl FnMut(&mut egui::Ui) + 'static) -> image::RgbaImage {
    let mut harness = Harness::builder()
        .with_size(size)
        .with_context_setup(|ctx| {
            larrygram_ui::install_image_loaders(ctx);
            let theme = sets().get(LayoutKind::PanelModern).theme.clone();
            larrygram_ui::theme::apply(ctx, &theme).expect("the theme applies");
        })
        .build_ui(body);
    for _ in 0..3 {
        harness.run();
    }
    harness.render().expect("the frame renders")
}

/// Panel Classic with the File menu open (ADR 0065 item 5).
#[test]
fn golden_panel_classic_with_a_menu_open() {
    shell_golden(LayoutKind::PanelClassic, "panel-classic-menu-open", |rig| {
        rig.click_named("File");
    });
}

/// Panel Modern on the Home tab (ADR 0065 item 5).
#[test]
fn golden_panel_modern_on_the_home_tab() {
    shell_golden(LayoutKind::PanelModern, "panel-modern-home", |_| {});
}

/// Panel Modern with the Picture Tools contextual tab open (ADR 0065
/// item 5): the fixed document's picture shape is selected.
#[test]
fn golden_panel_modern_with_picture_tools() {
    let mut app = RichApp::rich();
    app.select_picture();
    let mut app = app.0;
    app.settings.ui_layout = LayoutKind::PanelModern;
    app.settings.default_layout = LayoutKind::PanelModern;
    let mut rig = ShellRig::with_app(app);
    for _ in 0..3 {
        rig.run();
    }
    // The contextual tab appears for the picture selection; open it.
    rig.click_named("Format");
    rig.run();
    rig.run();
    let image = rig.render();
    check_golden("panel-modern-picture-tools", &image);
}

/// The Panel Modern File view (ADR 0065 item 5).
#[test]
fn golden_the_file_view() {
    shell_golden(LayoutKind::PanelModern, "file-view", |rig| {
        rig.click_named("File");
    });
}

/// The welcome wizard's first page (ADR 0065, the amendment).
#[test]
fn golden_the_wizard_welcome_page() {
    let state = std::rc::Rc::new(std::cell::RefCell::new(wizard::State::new()));
    let image = window_golden(egui::vec2(1000.0, 700.0), {
        let state = std::rc::Rc::clone(&state);
        move |ui| {
            let mut state = state.borrow_mut();
            let _ = wizard::show(ui, &mut state).expect("the wizard renders");
        }
    });
    check_golden("wizard-welcome", &image);
}

/// The welcome wizard's layout page (ADR 0065, the amendment).
#[test]
fn golden_the_wizard_layout_page() {
    let state = std::rc::Rc::new(std::cell::RefCell::new(wizard::State::new()));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_context_setup(|ctx| {
            larrygram_ui::install_image_loaders(ctx);
            let theme = sets().get(LayoutKind::PanelModern).theme.clone();
            larrygram_ui::theme::apply(ctx, &theme).expect("the theme applies");
        })
        .build_ui({
            let state = std::rc::Rc::clone(&state);
            move |ui| {
                let mut state = state.borrow_mut();
                let _ = wizard::show(ui, &mut state).expect("the wizard renders");
            }
        });
    harness.run();
    // Next moves to the layout page.
    {
        let root = harness.root();
        use egui_kittest::kittest::Queryable;
        let node = root
            .query_all_by_label("Next")
            .next()
            .expect("the Next button");
        node.click();
    }
    for _ in 0..3 {
        harness.run();
    }
    assert_eq!(state.borrow().page(), wizard::Page::ChooseLayout);
    let image = harness.render().expect("the frame renders");
    check_golden("wizard-layout", &image);
}

/// The About window (ADR 0065, the amendment).
#[test]
fn golden_the_about_window() {
    let image = window_golden(egui::vec2(900.0, 600.0), |ui| {
        let _ = larrygram_ui::about::show(ui).expect("the About window renders");
    });
    check_golden("about", &image);
}

// --- the metric's own tests (ADR 0027: pin the rule) ---------------

/// Two synthetic images of `size`, filled with `fill`.
fn synthetic(size: (u32, u32), fill: [u8; 4]) -> image::RgbaImage {
    image::RgbaImage::from_pixel(size.0, size.1, image::Rgba(fill))
}

#[test]
fn the_metric_accepts_identical_images() {
    let a = synthetic((8, 8), [10, 20, 30, 255]);
    assert_eq!(compare_images(&a, &a.clone()), Ok(()));
}

#[test]
fn the_metric_accepts_small_channel_noise() {
    let a = synthetic((100, 100), [10, 20, 30, 255]);
    let mut b = a.clone();
    for pixel in b.pixels_mut() {
        pixel.0 = [11, 21, 32, 255];
    }
    assert_eq!(compare_images(&a, &b), Ok(()), "every pixel within 2");
}

#[test]
fn the_metric_accepts_a_few_failing_pixels() {
    let a = synthetic((100, 100), [10, 20, 30, 255]);
    let mut b = a.clone();
    // 10 pixels of 10,000 = 0.1%, inside the 0.5% budget.
    for index in 0..10 {
        b.get_pixel_mut(index, 0).0 = [255, 0, 0, 255];
    }
    assert_eq!(compare_images(&a, &b), Ok(()));
}

#[test]
fn the_metric_rejects_too_many_failing_pixels() {
    let a = synthetic((100, 100), [10, 20, 30, 255]);
    let mut b = a.clone();
    // 100 pixels of 10,000 = 1%, over the budget.
    for x in 0..100 {
        b.get_pixel_mut(x, 50).0 = [255, 0, 0, 255];
    }
    assert!(compare_images(&a, &b).is_err());
}

#[test]
fn the_metric_rejects_different_dimensions() {
    let a = synthetic((8, 8), [10, 20, 30, 255]);
    let b = synthetic((8, 9), [10, 20, 30, 255]);
    assert!(compare_images(&a, &b).is_err());
}
