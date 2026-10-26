//! The one-palette check of the design pass (task P7-18,
//! `docs/ui/design.md`): no widget paints a colour that is not a theme
//! token.
//!
//! Each surface of both layouts is drawn, and every colour in the
//! frame's shapes (fills, strokes, text) is matched against the colour
//! tokens of the active theme. A colour matches a token when its
//! unmultiplied RGB is within [`TOLERANCE`] of the token's RGB: alpha
//! is free, so egui's fades (a disabled widget, a shadow, an
//! animation) still match the token they fade.
//!
//! The canvas is checked too: its page, grid, rulers and tool overlay
//! read the `canvas.*`, `page.*` and `ruler.*` tokens, and the drawing
//! itself is a raster texture. Left out: textured meshes (the drawing,
//! icon and brand textures carry their own pixels; the icon tints are
//! tokens by construction, `icons/mod.rs`).

use std::collections::BTreeMap;

use egui::Color32;
use egui::epaint::{ColorMode, Shape};
use larrygram_ui_spec::LayoutKind;

use crate::review::click_at;
use crate::{RichApp, ShellRig};

/// The largest per-channel RGB difference of a match, of 255 (the
/// unmultiply of a faded colour rounds).
const TOLERANCE: i32 = 3;

/// Below this alpha the unmultiplied RGB is too coarse to compare; such
/// a colour is all but invisible and passes.
const MIN_ALPHA: u8 = 24;

/// The surfaces the check draws: a name and how to reach it.
type Prepare = fn(&mut ShellRig);

fn idle(_: &mut ShellRig) {}

fn options(rig: &mut ShellRig) {
    rig.dispatch("app.office.center.options");
}

fn about(rig: &mut ShellRig) {
    rig.dispatch("help.about");
}

fn wizard(rig: &mut ShellRig) {
    rig.dispatch("help.welcome");
}

fn context_menu(rig: &mut ShellRig) {
    click_at(
        rig,
        egui::pos2(120.0, 500.0),
        egui::PointerButton::Secondary,
    );
}

fn first_menu(rig: &mut ShellRig) {
    // Panel Classic: the Edit menu. Panel Modern: the File view.
    rig.run();
    for name in ["Edit", "File"] {
        if rig.has(name) {
            rig.click_named(name);
            return;
        }
    }
}

const SURFACES: &[(&str, Prepare)] = &[
    ("idle", idle),
    ("options", options),
    ("about", about),
    ("wizard", wizard),
    ("context menu", context_menu),
    ("menu or File view", first_menu),
];

/// Every colour a shape paints, unmultiplied.
fn shape_colors(shape: &Shape, out: &mut Vec<Color32>) {
    let mut push = |color: Color32| out.push(color);
    match shape {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(shapes) => {
            for shape in shapes {
                shape_colors(shape, out);
            }
        }
        Shape::Circle(circle) => {
            push(circle.fill);
            push(circle.stroke.color);
        }
        Shape::Ellipse(ellipse) => {
            push(ellipse.fill);
            push(ellipse.stroke.color);
        }
        Shape::LineSegment { stroke, .. } => push(stroke.color),
        Shape::Path(path) => {
            push(path.fill);
            if let ColorMode::Solid(color) = path.stroke.color {
                push(color);
            }
        }
        Shape::Band(band) => {
            push(band.fill);
            push(band.stroke.color);
        }
        Shape::Rect(rect) => {
            push(rect.fill);
            push(rect.stroke.color);
        }
        Shape::Text(text) => {
            push(text.underline.color);
            match text.override_text_color {
                Some(color) => push(color),
                None => {
                    for section in &text.galley.job.sections {
                        let format = &section.format;
                        if format.color == Color32::PLACEHOLDER {
                            push(text.fallback_color);
                        } else {
                            push(format.color);
                        }
                        push(format.background);
                        push(format.underline.color);
                        push(format.strikethrough.color);
                    }
                }
            }
        }
        Shape::Mesh(mesh) => {
            if mesh.texture_id == egui::TextureId::default() {
                for vertex in &mesh.vertices {
                    push(vertex.color);
                }
            }
        }
        Shape::QuadraticBezier(curve) => {
            push(curve.fill);
            if let ColorMode::Solid(color) = curve.stroke.color {
                push(color);
            }
        }
        Shape::CubicBezier(curve) => {
            push(curve.fill);
            if let ColorMode::Solid(color) = curve.stroke.color {
                push(color);
            }
        }
    }
}

/// Whether `color` is one of `tokens` (RGB within [`TOLERANCE`]), or
/// too faint to judge.
fn matches_a_token(color: Color32, tokens: &[Color32]) -> bool {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a < MIN_ALPHA {
        return true;
    }
    tokens.iter().any(|token| {
        let [tr, tg, tb, _] = token.to_srgba_unmultiplied();
        (i32::from(r) - i32::from(tr)).abs() <= TOLERANCE
            && (i32::from(g) - i32::from(tg)).abs() <= TOLERANCE
            && (i32::from(b) - i32::from(tb)).abs() <= TOLERANCE
    })
}

/// The colours of the last frame of `rig` that match no token, with
/// the number of shapes that paint each one.
fn strangers(rig: &mut ShellRig, tokens: &[Color32]) -> BTreeMap<String, usize> {
    for _ in 0..3 {
        rig.run();
    }
    let mut out = BTreeMap::new();
    for clipped in &rig.harness().output().shapes {
        let mut colors = Vec::new();
        shape_colors(&clipped.shape, &mut colors);
        for color in colors {
            if matches_a_token(color, tokens) {
                continue;
            }
            *out.entry(format!("{:?}", color.to_srgba_unmultiplied()))
                .or_insert(0) += 1;
        }
    }
    out
}

#[test]
fn every_painted_colour_is_a_theme_token() {
    let mut failures = Vec::new();
    for kind in [LayoutKind::PanelModern, LayoutKind::PanelClassic] {
        let theme = &crate::sets().get(kind).theme;
        let tokens: Vec<Color32> = theme
            .colors
            .keys()
            .map(|token| larrygram_ui::theme::color(theme, token))
            .collect();
        for (name, prepare) in SURFACES {
            let mut rich = RichApp::rich();
            rich.0.settings.ui_layout = kind;
            rich.0.settings.default_layout = kind;
            let mut rig = ShellRig::with_app_at(rich.0, egui::vec2(1440.0, 900.0), 1.0);
            rig.run();
            prepare(&mut rig);
            let found = strangers(&mut rig, &tokens);
            if !found.is_empty() {
                failures.push(format!("{kind:?} {name}: {found:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "colours outside the theme tokens:\n{}",
        failures.join("\n")
    );
}

/// The check sees the frame: with no tokens at all, the idle window is
/// nothing but strangers.
#[test]
fn with_no_tokens_every_colour_is_a_stranger() {
    let rich = RichApp::rich();
    let mut rig = ShellRig::with_app_at(rich.0, egui::vec2(1440.0, 900.0), 1.0);
    let found = strangers(&mut rig, &[]);
    let shapes: usize = found.values().sum();
    assert!(shapes > 100, "only {shapes} coloured shapes: {found:?}");
}

#[test]
fn a_faded_token_still_matches_and_a_stranger_does_not() {
    let accent = Color32::from_rgb(0x1f, 0x5b, 0xc4);
    let tokens = [accent];
    assert!(matches_a_token(accent, &tokens));
    assert!(matches_a_token(accent.gamma_multiply(0.5), &tokens));
    assert!(matches_a_token(Color32::TRANSPARENT, &tokens));
    assert!(!matches_a_token(
        Color32::from_rgb(0xff, 0x00, 0x00),
        &tokens
    ));
}
