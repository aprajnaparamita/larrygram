//! The modal windows' rects (task P7-17): the welcome wizard and the
//! About window size from the app window, sit centred in it, keep the
//! artwork's aspect ratio and draw the same in points at display
//! scales 1 and 2, at every review window size.

use egui::Rect;
use egui_kittest::kittest::Queryable;
use larrygram_ui::window_size::{ABOUT, ABOUT_WINDOW, SizeRule, WIZARD, WIZARD_WINDOW};
use larrygram_ui_spec::LayoutKind;

use crate::review::{SCALES, SIZES};
use crate::{RichApp, ShellRig};

/// A settled Panel Modern rig at one size and scale with `command`
/// dispatched (it opens the window under test).
fn rig_with(size: (f32, f32), scale: f32, command: &'static str) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = LayoutKind::PanelModern;
    let mut rig = ShellRig::with_app_at(app, egui::vec2(size.0, size.1), scale);
    rig.run();
    rig.dispatch(command);
    for _ in 0..4 {
        rig.run();
    }
    rig
}

/// The rect of the egui window with the id source `id` (its area,
/// frame included).
fn window_rect(rig: &mut ShellRig, id: &str) -> Rect {
    let ctx = rig.harness().ctx.clone();
    ctx.memory(|memory| memory.area_rect(egui::Id::unique(id)))
        .unwrap_or_else(|| panic!("no window {id:?}"))
}

/// The rect of the first widget named `name`.
fn node_rect(rig: &mut ShellRig, name: &str) -> Rect {
    rig.harness()
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"))
        .rect()
}

/// The checks every modal window shares; returns the window rect.
fn check_window(
    rig: &mut ShellRig,
    title: &str,
    rule: &SizeRule,
    size: (f32, f32),
    tag: &str,
) -> Rect {
    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    let rect = window_rect(rig, title);
    let content = rule.size(screen.size());
    assert!(
        screen.contains_rect(rect),
        "{tag}: {rect:?} inside the window {screen:?}"
    );
    assert!(
        (rect.center() - screen.center()).length() <= 1.0,
        "{tag}: {rect:?} centred in {screen:?}"
    );
    // The frame (and the About title bar) add a little around the
    // content; never less than the content, never much more.
    assert!(
        rect.width() >= content.x && rect.width() <= content.x + 40.0,
        "{tag}: width {} for content {}",
        rect.width(),
        content.x
    );
    assert!(
        rect.height() >= content.y && rect.height() <= content.y + 60.0,
        "{tag}: height {} for content {}",
        rect.height(),
        content.y
    );
    rect
}

/// Width over height.
fn aspect(rect: Rect) -> f32 {
    rect.width() / rect.height().max(1.0)
}

#[test]
fn the_wizard_sizes_from_the_window_at_both_scales() {
    for size in SIZES {
        let mut rects = Vec::new();
        for scale in SCALES {
            let tag = format!("{size:?} x{scale}");
            let mut rig = rig_with(size, scale, "help.welcome");
            let window = check_window(&mut rig, WIZARD_WINDOW, &WIZARD, size, &tag);
            let art = node_rect(&mut rig, "Larrygram welcome picture");
            assert!(
                window.contains_rect(art),
                "{tag}: the art inside the window"
            );
            assert!(
                (aspect(art) - 1.6).abs() < 0.02,
                "{tag}: art aspect {}",
                aspect(art)
            );
            assert!(
                art.width() >= window.width() * 0.45,
                "{tag}: the art scales with the window"
            );
            for button in ["Next", "Cancel"] {
                let rect = node_rect(&mut rig, button);
                assert!(window.contains_rect(rect), "{tag}: {button} inside");
                assert!(
                    window.max.y - rect.max.y <= 24.0,
                    "{tag}: {button} sits on the bottom edge ({rect:?} in {window:?})"
                );
            }
            // The layout page keeps the window and its pictures' aspect.
            rig.click_named("Next");
            for _ in 0..3 {
                rig.run();
            }
            let second = window_rect(&mut rig, WIZARD_WINDOW);
            assert!(
                (second.size() - window.size()).length() <= 1.0,
                "{tag}: both pages share one size"
            );
            for picture in [
                "Panel Modern layout picture",
                "Panel Classic layout picture",
            ] {
                let rect = node_rect(&mut rig, picture);
                assert!(second.contains_rect(rect), "{tag}: {picture} inside");
                assert!((aspect(rect) - 1.6).abs() < 0.02, "{tag}: {picture} aspect");
            }
            rects.push(window);
        }
        assert!(
            rects
                .windows(2)
                .all(|pair| (pair[0].size() - pair[1].size()).length() <= 1.0),
            "{size:?}: the same size in points at both scales: {rects:?}"
        );
    }
}

#[test]
fn the_about_window_sizes_from_the_window_at_both_scales() {
    for size in SIZES {
        let mut rects = Vec::new();
        for scale in SCALES {
            let tag = format!("{size:?} x{scale}");
            let mut rig = rig_with(size, scale, "help.about");
            let window = check_window(&mut rig, ABOUT_WINDOW, &ABOUT, size, &tag);
            let photo = node_rect(&mut rig, "Larry Jeffus");
            assert!(
                window.contains_rect(photo),
                "{tag}: the photo inside the window"
            );
            assert!(
                (aspect(photo) - 1.0).abs() < 0.02,
                "{tag}: photo aspect {}",
                aspect(photo)
            );
            assert!(
                photo.height() >= window.height() * 0.6,
                "{tag}: the photo scales"
            );
            let close = node_rect(&mut rig, "Close");
            assert!(window.contains_rect(close), "{tag}: Close inside");
            assert!(
                window.max.y - close.max.y <= 24.0,
                "{tag}: Close on the bottom edge"
            );
            rects.push(window);
        }
        assert!(
            rects
                .windows(2)
                .all(|pair| (pair[0].size() - pair[1].size()).length() <= 1.0),
            "{size:?}: the same size in points at both scales: {rects:?}"
        );
    }
}
