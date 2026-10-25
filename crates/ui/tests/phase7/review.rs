//! The UI display review sweep (task P7-17): renders every surface of
//! both Panel layouts at three window sizes and display scales 1 and
//! 2, and writes the PNGs under `LARRYGRAM_UI_REVIEW_DIR`.
//!
//! It is a review tool, not a check: the test is ignored and does
//! nothing unless the variable is set. The images stay local (the
//! task keeps them under `reference/ui-review/`, never in git).
//!
//! ```sh
//! LARRYGRAM_UI_REVIEW_DIR=/Volumes/Dara/larrygram/reference/ui-review/after \
//!   cargo test -p larrygram-ui --test phase7 review -- --ignored
//! ```

use std::path::{Path, PathBuf};

use egui_kittest::kittest::Queryable;
use larrygram_ui_spec::LayoutKind;
use larrygram_ui_spec::v2::ItemKind;

use crate::{RichApp, ShellRig, sets};

/// The three window sizes of the review, in logical pixels.
pub const SIZES: [(f32, f32); 3] = [(1024.0, 768.0), (1440.0, 900.0), (1920.0, 1080.0)];

/// The two display scales of the review.
pub const SCALES: [f32; 2] = [1.0, 2.0];

/// One surface: its file stem and how to reach it from a fresh rig.
type Prepare = Box<dyn Fn(&mut ShellRig)>;

/// A rig over the rich document in `kind`, at one size and scale,
/// settled for three frames.
fn rig(kind: LayoutKind, size: (f32, f32), scale: f32, picture: bool) -> ShellRig {
    let mut rich = RichApp::rich();
    if picture {
        rich.select_picture();
    }
    let mut app = rich.0;
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app_at(app, egui::vec2(size.0, size.1), scale);
    for _ in 0..3 {
        rig.run();
    }
    rig
}

/// Clicks the first widget named `name`, when one is drawn.
fn click_if(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    let found = rig.harness().query_all_by_label(name).next().is_some();
    if found {
        rig.click_named(name);
    }
    found
}

/// The surfaces of Panel Modern.
fn modern_surfaces() -> Vec<(String, bool, Prepare)> {
    let set = sets().get(LayoutKind::PanelModern);
    let mut out: Vec<(String, bool, Prepare)> = Vec::new();
    for tab in &set.v2.tabs {
        let label = tab.label.clone().unwrap_or_default();
        out.push((
            format!("tab-{}", tab.id.replace('.', "-")),
            false,
            Box::new(move |rig| {
                click_if(rig, &label);
            }),
        ));
    }
    out.push((
        "picture-tools".into(),
        true,
        Box::new(|rig| {
            click_if(rig, "Format");
        }),
    ));
    if let Some(view) = &set.v2.file_view {
        for entry in &view.nav {
            let label = entry.label.clone();
            if !matches!(entry.kind, larrygram_ui_spec::v2::FileViewEntryKind::Page) {
                continue;
            }
            out.push((
                format!("file-view-{}", entry.id.replace('.', "-")),
                false,
                Box::new(move |rig| {
                    click_if(rig, "File");
                    rig.run();
                    click_if(rig, &label);
                }),
            ));
        }
    }
    // The menus of the Home tab's menu controls: the Paste split's
    // arrow (the first "More items" arrow) and the Quick Styles
    // gallery.
    out.push((
        "menu-paste".into(),
        false,
        Box::new(|rig| {
            // The QAT's Undo arrow shares the name; the Paste arrow is
            // the first one below the tab strip.
            rig.run();
            let node = rig
                .harness()
                .query_all_by_label("More items")
                .find(|node| node.rect().min.y > 60.0)
                .map(|node| node.rect().center());
            if let Some(pos) = node {
                rig.harness().hover_at(pos);
                rig.run();
                click_at(rig, pos, egui::PointerButton::Primary);
            }
        }),
    ));
    out.push((
        "menu-quick-styles".into(),
        false,
        Box::new(|rig| {
            click_if(rig, "Quick Styles");
        }),
    ));
    out.extend(common_surfaces());
    out
}

/// The surfaces of Panel Classic: every menu and every first-level
/// submenu, the floating toolbars, then the common ones.
fn classic_surfaces() -> Vec<(String, bool, Prepare)> {
    let set = sets().get(LayoutKind::PanelClassic);
    let mut out: Vec<(String, bool, Prepare)> = Vec::new();
    if let Some(bar) = set.v2.menu_bars.first() {
        for menu in &bar.menus {
            let title = menu.label.clone();
            out.push((
                format!("menu-{}", title.to_lowercase()),
                false,
                Box::new({
                    let title = title.clone();
                    move |rig| {
                        click_if(rig, &title);
                    }
                }),
            ));
            for item in &menu.items {
                if item.kind != ItemKind::Submenu {
                    continue;
                }
                let label = item.label.clone().unwrap_or_default();
                let title = title.clone();
                out.push((
                    format!(
                        "menu-{}-{}",
                        title.to_lowercase(),
                        label
                            .to_lowercase()
                            .replace(|c: char| !c.is_alphanumeric(), "-")
                    ),
                    false,
                    Box::new(move |rig| {
                        click_if(rig, &title);
                        rig.run();
                        click_if(rig, &label);
                    }),
                ));
            }
        }
    }
    out.push((
        "floating-toolbars".into(),
        false,
        Box::new(|rig| {
            for toolbar in ["Drawing", "Ink", "Picture"] {
                click_if(rig, "View");
                rig.run();
                click_if(rig, "Toolbars");
                rig.run();
                click_if(rig, toolbar);
                rig.run();
            }
        }),
    ));
    out.extend(common_surfaces());
    out
}

/// The surfaces both layouts share: the idle window, the dialogs, the
/// wizard pages and a context menu.
fn common_surfaces() -> Vec<(String, bool, Prepare)> {
    vec![
        ("idle".into(), false, Box::new(|_| {})),
        (
            "options".into(),
            false,
            Box::new(|rig| rig.dispatch("app.office.center.options")),
        ),
        (
            "about".into(),
            false,
            Box::new(|rig| rig.dispatch("help.about")),
        ),
        (
            "wizard-welcome".into(),
            false,
            Box::new(|rig| rig.dispatch("help.welcome")),
        ),
        (
            "wizard-layout".into(),
            false,
            Box::new(|rig| {
                rig.dispatch("help.welcome");
                rig.run();
                click_if(rig, "Next");
            }),
        ),
        (
            "context-page-tab".into(),
            false,
            Box::new(|rig| {
                rig.run();
                let pos = rig
                    .harness()
                    .query_all_by_label("Page-1")
                    .next()
                    .map(|node| node.rect().center());
                if let Some(pos) = pos {
                    click_at(rig, pos, egui::PointerButton::Secondary);
                }
            }),
        ),
        (
            "context-shapes-window".into(),
            false,
            Box::new(|rig| {
                click_at(
                    rig,
                    egui::pos2(120.0, 500.0),
                    egui::PointerButton::Secondary,
                );
            }),
        ),
    ]
}

/// One click of `button` at `pos`, pressed and released in two frames.
pub fn click_at(rig: &mut ShellRig, pos: egui::Pos2, button: egui::PointerButton) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        rig.run();
    }
}

/// Writes one render.
fn save(dir: &Path, name: &str, rig: &mut ShellRig) {
    for _ in 0..3 {
        rig.run();
    }
    let image = rig.render();
    let path = dir.join(format!("{name}.png"));
    image
        .save(&path)
        .unwrap_or_else(|e| panic!("write {path:?}: {e}"));
}

/// The sweep. Set `LARRYGRAM_UI_REVIEW_DIR` (and optionally
/// `LARRYGRAM_UI_REVIEW_ONLY`, a substring of the file names) to run.
#[test]
#[ignore = "the P7-17 review tool: writes PNGs, run on demand"]
fn review_every_surface() {
    let Some(dir) = std::env::var_os("LARRYGRAM_UI_REVIEW_DIR").map(PathBuf::from) else {
        return;
    };
    let only = std::env::var("LARRYGRAM_UI_REVIEW_ONLY").unwrap_or_default();
    std::fs::create_dir_all(&dir).expect("the review directory");
    for (kind, tag, surfaces) in [
        (LayoutKind::PanelModern, "modern", modern_surfaces()),
        (LayoutKind::PanelClassic, "classic", classic_surfaces()),
    ] {
        for size in SIZES {
            for scale in SCALES {
                for (name, picture, prepare) in &surfaces {
                    let stem = format!("{tag}-{}x{}-s{}-{name}", size.0, size.1, scale);
                    if !only.is_empty() && !stem.contains(&only) {
                        continue;
                    }
                    let mut rig = rig(kind, size, scale, *picture);
                    prepare(&mut rig);
                    save(&dir, &stem, &mut rig);
                }
            }
        }
    }
}
