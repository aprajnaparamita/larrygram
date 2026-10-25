//! A disabled control draws its icon in the disabled text colour
//! (`chrome.text_disabled`; task P7-18 replaced ADR 0062's 40% alpha so
//! a disabled icon matches a disabled label, docs/ui/design.md).
//!
//! The rule lives in `icons::draw` / `icons::icon_themed` behind their
//! `enabled` parameter; this test renders one icon in both states over
//! a white background and asserts the disabled glyph core is the
//! disabled colour (the P7-11 F2 regression pin, updated).

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui_kittest::Harness;
use larrygram_ui::icons;
use larrygram_ui_spec::{LayoutKind, LayoutSets, Loader};

/// The largest per-channel difference from the disabled colour, of 255
/// (rendering and rounding noise).
const MAX_DELTA: u8 = 6;

/// The committed Panel Modern theme (the icon tints come from it).
fn theme() -> larrygram_ui_spec::v2::Theme {
    let sets: LayoutSets = Loader::from_embedded().expect("the embedded ui-spec loads");
    sets.get(LayoutKind::PanelModern).theme.clone()
}

/// Renders `key` in one enabled state over white and returns the
/// image.
fn render(key: &str, enabled: bool) -> image::RgbaImage {
    let theme = theme();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(64.0, 64.0))
        .build_ui(move |ui| {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)),
                0.0,
                egui::Color32::WHITE,
            );
            let rect = egui::Rect::from_min_size(egui::pos2(16.0, 16.0), egui::vec2(32.0, 32.0));
            icons::draw(ui, rect, key, 32.0, &theme, enabled).expect("the icon draws");
        });
    harness.run();
    harness.render().expect("the frame renders")
}

/// The darkest (most painted) pixel of an image: the glyph core of a
/// dark icon on white.
fn darkest(image: &image::RgbaImage) -> [u8; 3] {
    let mut best = [255_u8, 255, 255];
    for pixel in image.pixels() {
        if pixel.0[..3].iter().map(|c| u16::from(*c)).sum::<u16>()
            < best.iter().map(|c| u16::from(*c)).sum::<u16>()
        {
            best = [pixel.0[0], pixel.0[1], pixel.0[2]];
        }
    }
    best
}

#[test]
fn a_disabled_icon_draws_in_the_disabled_text_colour() {
    let enabled = render("edit.cut", true);
    let disabled = render("edit.cut", false);
    let on = darkest(&enabled);
    let off = darkest(&disabled);
    // The glyph core is painted in both renders, and differently.
    assert!(
        on.iter().any(|c| *c < 128),
        "the enabled icon paints: {on:?}"
    );
    assert_ne!(on, off, "the disabled icon changes colour");
    let expected = larrygram_ui::theme::color(&theme(), "chrome.text_disabled");
    let expected = [expected.r(), expected.g(), expected.b()];
    for channel in 0..3 {
        let delta = expected[channel].abs_diff(off[channel]);
        assert!(
            delta <= MAX_DELTA,
            "channel {channel}: expected {} (chrome.text_disabled), got {}",
            expected[channel],
            off[channel]
        );
    }
}
