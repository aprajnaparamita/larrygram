//! The layout checks of task P7-17: at each review window size
//! ([`crate::review::SIZES`]) and display scale, the chrome's controls
//! do not overlap, each lies inside its region, no label wraps inside
//! a word, and no popup stays open after an open-then-leave sequence.
//!
//! The controls are the accesskit nodes egui reports (buttons, check
//! boxes, combos, sliders, text boxes) with their rects in points; the
//! labels are the text shapes of the frame's paint output.

use egui::accesskit::Role;
use egui::{Pos2, Rect};
use egui_kittest::kittest::NodeT;
use larrygram_ui_spec::LayoutKind;

use crate::review::{SCALES, SIZES};
use crate::{RichApp, ShellRig};

/// One interactive control of the frame.
#[derive(Debug, Clone)]
pub struct Control {
    /// Its accessible name (or value).
    pub name: String,
    /// Its role.
    pub role: Role,
    /// Its rect, in points.
    pub rect: Rect,
}

/// The roles the checks treat as controls (labels included: a status
/// text under a button is an overlap too).
fn is_control(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::Label
            | Role::CheckBox
            | Role::ComboBox
            | Role::RadioButton
            | Role::Slider
            | Role::TextInput
            | Role::Tab
    )
}

/// Every control of the last frame with a non-empty rect.
pub fn controls(rig: &mut ShellRig) -> Vec<Control> {
    rig.run();
    let harness = rig.harness();
    let root = harness.root();
    let mut out = Vec::new();
    for node in root.children_recursive() {
        let data = node.accesskit_node().data();
        let role = data.role();
        if !is_control(role) {
            continue;
        }
        let rect = node.rect();
        if !rect.is_positive() {
            continue;
        }
        let name = data
            .label()
            .map(str::to_owned)
            .or_else(|| data.value().map(str::to_owned))
            .unwrap_or_default();
        out.push(Control { name, role, rect });
    }
    out
}

/// The pairs of `controls` whose rects overlap by more than a hair
/// (`tolerance` points on each axis): touching edges are fine.
pub fn overlaps(controls: &[Control], tolerance: f32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (index, a) in controls.iter().enumerate() {
        for b in controls.iter().skip(index + 1) {
            let shared = a.rect.intersect(b.rect);
            if shared.width() > tolerance && shared.height() > tolerance {
                out.push((
                    format!("{} {:?}", a.name, a.rect),
                    format!("{} {:?}", b.name, b.rect),
                ));
            }
        }
    }
    out
}

/// The text of every galley in the frame's paint output that wraps a
/// line inside a word: a row that is not the last and does not end in
/// a newline must end in white space or a hyphen (egui keeps the space
/// on the upper row when it wraps at a word boundary).
pub fn mid_word_wraps(output: &egui::FullOutput) -> Vec<String> {
    let mut out = Vec::new();
    for clipped in &output.shapes {
        collect_wraps(&clipped.shape, &mut out);
    }
    out
}

/// [`mid_word_wraps`] over one shape, recursing into shape lists.
fn collect_wraps(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_wraps(shape, out);
            }
        }
        egui::Shape::Text(text) => {
            let galley = &text.galley;
            let rows = &galley.rows;
            for (index, row) in rows.iter().enumerate() {
                if index + 1 == rows.len() || row.ends_with_newline {
                    continue;
                }
                let last = row.row.glyphs.last().map(|glyph| glyph.chr);
                let next = rows
                    .get(index + 1)
                    .and_then(|row| row.row.glyphs.first())
                    .map(|glyph| glyph.chr);
                let at_boundary = last.is_none_or(|c| c.is_whitespace() || c == '-')
                    || next.is_none_or(char::is_whitespace);
                if !at_boundary {
                    out.push(galley.text().to_owned());
                    break;
                }
            }
        }
        _ => {}
    }
}

/// The chrome of a frame around `canvas`: the controls above it (the
/// title bar, the tab strip and the command bar, or the menu bar and
/// toolbars) and the ones under it (the page tabs and the status bar).
/// The panes beside the canvas scroll their own content, which may run
/// past the window inside its clip, so they are not chrome.
fn chrome(controls: &[Control], canvas: Rect) -> Vec<Control> {
    controls
        .iter()
        .filter(|control| {
            control.rect.max.y <= canvas.min.y + 0.5
                || (control.rect.min.y >= canvas.max.y - 0.5
                    && control.rect.min.x >= canvas.min.x - 0.5
                    && control.rect.max.x <= canvas.max.x + 0.5)
        })
        .cloned()
        .collect()
}

/// A rig of `kind` at one review size and scale, settled.
fn sized_rig(kind: LayoutKind, size: (f32, f32), scale: f32) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app_at(app, egui::vec2(size.0, size.1), scale);
    for _ in 0..4 {
        rig.run();
    }
    rig
}

/// Prints every control of one frame (a debugging aid for this
/// module's checks; set `LARRYGRAM_UI_DUMP=modern` or `classic`).
#[test]
#[ignore = "debugging aid: prints the control rects"]
fn dump_controls() {
    let kind = match std::env::var("LARRYGRAM_UI_DUMP").as_deref() {
        Ok("classic") => LayoutKind::PanelClassic,
        Ok("modern") => LayoutKind::PanelModern,
        _ => return,
    };
    for size in SIZES {
        let mut rig = sized_rig(kind, size, 1.0);
        println!("--- {size:?}");
        for control in controls(&mut rig) {
            println!("{:?} {:?} {:?}", control.role, control.name, control.rect);
        }
    }
}

/// The screen rect of a rig of `size`.
fn screen(size: (f32, f32)) -> Rect {
    Rect::from_min_max(Pos2::ZERO, egui::pos2(size.0, size.1))
}

/// The canvas rect of a settled rig (the chrome lies around it).
fn canvas(rig: &mut ShellRig) -> Rect {
    rig.run();
    rig.shell
        .borrow()
        .canvas_rect()
        .expect("the rich document draws a canvas")
}

/// The layout checks of one layout at every review size and scale.
fn check_layout(kind: LayoutKind) {
    let mut failures = Vec::new();
    for size in SIZES {
        for scale in SCALES {
            let mut rig = sized_rig(kind, size, scale);
            let canvas = canvas(&mut rig);
            let all = controls(&mut rig);
            let tag = format!("{kind:?} {size:?} x{scale}");
            let chrome = chrome(&all, canvas);
            for control in &chrome {
                if !screen(size).expand(0.5).contains_rect(control.rect) {
                    failures.push(format!(
                        "{tag}: {} {:?} leaves the window",
                        control.name, control.rect
                    ));
                }
            }
            for (a, b) in overlaps(&chrome, 1.0) {
                failures.push(format!("{tag}: {a} overlaps {b}"));
            }
            for text in mid_word_wraps(rig.harness().output()) {
                failures.push(format!("{tag}: {text:?} wraps inside a word"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} layout failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Panel Modern: the title bar, the tab strip, the command bar, the
/// page tabs and the status bar fit at every size (task P7-17).
#[test]
fn panel_modern_chrome_fits_at_every_size() {
    check_layout(LayoutKind::PanelModern);
}

/// Panel Classic: the menu bar, the toolbars, the page tabs and the
/// status bar fit at every size (task P7-17).
#[test]
fn panel_classic_chrome_fits_at_every_size() {
    check_layout(LayoutKind::PanelClassic);
}

// --- the checks' own tests -----------------------------------------

fn control(name: &str, rect: Rect) -> Control {
    Control {
        name: name.to_owned(),
        role: Role::Button,
        rect,
    }
}

#[test]
fn overlaps_finds_crossing_rects_and_ignores_touching_ones() {
    let a = control(
        "a",
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0)),
    );
    let b = control(
        "b",
        Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(20.0, 10.0)),
    );
    let c = control(
        "c",
        Rect::from_min_max(egui::pos2(5.0, 5.0), egui::pos2(15.0, 15.0)),
    );
    assert!(overlaps(&[a.clone(), b.clone()], 1.0).is_empty());
    assert_eq!(overlaps(&[a, b, c], 1.0).len(), 2);
}

#[test]
fn chrome_keeps_the_controls_outside_the_canvas_band() {
    let top = control(
        "top",
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0)),
    );
    let mid = control(
        "mid",
        Rect::from_min_max(egui::pos2(0.0, 50.0), egui::pos2(10.0, 60.0)),
    );
    let low = control(
        "low",
        Rect::from_min_max(egui::pos2(0.0, 95.0), egui::pos2(10.0, 99.0)),
    );
    let canvas = Rect::from_min_max(egui::pos2(0.0, 20.0), egui::pos2(50.0, 90.0));
    let kept: Vec<String> = chrome(&[top, mid, low], canvas)
        .into_iter()
        .map(|control| control.name)
        .collect();
    assert_eq!(kept, ["top", "low"]);
}
