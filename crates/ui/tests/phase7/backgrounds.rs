//! The Backgrounds gallery (task P7-54 goal 2), driven through the
//! real input path in the full shell (`ShellRig`) in Panel Modern:
//! the Design tab's Backgrounds gallery applies each design, the
//! page's link and the picture show it, one Undo takes the whole step
//! back, the Background Color submenu colours the design, and the
//! design and its link save to `.vsdx` and reopen.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_commands::{ArgValue, Args, CommandId, Invocation, Source};
use larrygram_geom::Point;
use larrygram_model::{CellValue, PageId, ShapeId};
use larrygram_ui::Intent;
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// The gallery labels, in gallery order (the designs' own names).
const DESIGNS: [&str; 3] = ["Solid Tint", "Soft Gradient", "Light Grid"];

/// The pages that hold the designs (the designs' own names).
const PAGES: [&str; 3] = ["Background Tint", "Background Gradient", "Background Grid"];

// ---------------------------------------------------------------------------
// The rig (the `colors` helpers' pattern)
// ---------------------------------------------------------------------------

/// A shell in Panel Modern with the rich state: the tests drive the
/// Design tab with real clicks.
fn shell_rig() -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = LayoutKind::PanelModern;
    let mut rig = ShellRig::with_app(app);
    rig.run();
    {
        let viewport = rig
            .shell
            .borrow()
            .canvas_viewport()
            .expect("the canvas drew");
        let mut shell = rig.shell.borrow_mut();
        let session = shell.app.active_document_mut().expect("session");
        let k = 96.0;
        session.view.zoom = 1.0;
        session.view.pan = Point::new(
            f64::from(viewport.width()) / 2.0 / k - 3.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - 3.0),
        );
    }
    rig.run();
    rig.run();
    rig
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(Point::new(x, y))
        .expect("the canvas drew")
}

/// Clicks the first widget with `role` and `name`.
fn click(rig: &mut ShellRig, role: Role, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(role, name)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    node.click();
    rig.run();
}

/// Clicks the first widget named `name`, whatever its role.
fn click_named(rig: &mut ShellRig, name: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_label(name)
        .next()
        .unwrap_or_else(|| panic!("no widget named {name:?}"));
    node.click();
    rig.run();
}

/// Whether a widget named `name` is drawn.
fn has_named(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_label(name)
        .next()
        .is_some()
}

/// Whether any pixel inside the page region matches `test` (the
/// picture checks).
fn region_matches(
    rig: &mut ShellRig,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    test: fn(&image::Rgba<u8>) -> bool,
) -> bool {
    let a = screen(rig, x0, y0);
    let b = screen(rig, x1, y1);
    let image = rig.render();
    let xmin = (a.x.min(b.x).max(0.0) as u32).min(image.width().saturating_sub(1));
    let xmax = (a.x.max(b.x).max(0.0) as u32).min(image.width());
    let ymin = (a.y.min(b.y).max(0.0) as u32).min(image.height().saturating_sub(1));
    let ymax = (a.y.max(b.y).max(0.0) as u32).min(image.height());
    for x in xmin..xmax {
        for y in ymin..ymax {
            if test(image.get_pixel(x, y)) {
                return true;
            }
        }
    }
    false
}

/// Blue-dominant (the tint, the gradient top and the grid lines draw
/// blue-ish).
fn blue(px: &image::Rgba<u8>) -> bool {
    i32::from(px[2]) > i32::from(px[0]) + 20 && i32::from(px[2]) > i32::from(px[1]) + 4
}

/// Red-dominant (the Background Color "Red" recolour).
fn red(px: &image::Rgba<u8>) -> bool {
    i32::from(px[0]) > i32::from(px[1]) + 20 && i32::from(px[0]) > i32::from(px[2]) + 20
}

/// Clicks the Undo button (Panel Modern names it for the last edit).
fn undo(rig: &mut ShellRig) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .children_recursive()
        .find(|node| {
            let data = node.accesskit_node().data();
            data.role() == Role::Button
                && data.label().is_some_and(|label| {
                    label == "Undo" || (label.starts_with("Undo ") && label != "Undo menu")
                })
        })
        .expect("the Undo button");
    node.click();
    rig.run();
}

/// One dispatch through the shell with arguments (the save and open
/// of the round trip).
fn dispatch_args(rig: &mut ShellRig, id: &'static str, args: Args) {
    let command = CommandId::parse(id).expect("id");
    let intent = Intent::new(command, Invocation::new(args, Source::Ui));
    let mut shell = rig.shell.borrow_mut();
    shell.dispatch(&[intent]).expect("the command runs");
}

// ---------------------------------------------------------------------------
// The gallery's own input rules
// ---------------------------------------------------------------------------

/// Opens Panel Modern's Design > Backgrounds gallery (the dropdown's
/// face click opens the popup and reports the current background).
fn open_gallery(rig: &mut ShellRig) {
    click(rig, Role::Button, "Design");
    click(rig, Role::Button, "Backgrounds");
}

/// The pages of the active document (`(name, background)` per page).
fn pages(app: &AppCtx) -> Vec<(String, Option<PageId>)> {
    let session = app.active_document().expect("session");
    session
        .document
        .pages
        .iter()
        .map(|page| (page.name.clone(), page.background))
        .collect()
}

/// The top-level shape count of the page named `name`.
fn shape_count(app: &AppCtx, name: &str) -> usize {
    let session = app.active_document().expect("session");
    session
        .document
        .pages
        .iter()
        .find(|page| page.name == name)
        .map(|page| page.shapes.len())
        .expect("the background page")
}

/// The `FillForegnd` colour of the first shape of the page `name`.
fn first_fill(app: &AppCtx, name: &str) -> Option<(u8, u8, u8)> {
    let session = app.active_document().expect("session");
    let page = session
        .document
        .pages
        .iter()
        .find(|page| page.name == name)
        .expect("the background page");
    let shape: ShapeId = *page.shapes.first()?;
    let cell = session
        .document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "FillForegnd")?;
    match cell.value {
        Some(CellValue::Color(color)) => Some((color.r, color.g, color.b)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Goal 2: the Backgrounds gallery
// ---------------------------------------------------------------------------

/// Each gallery row creates (or reuses) its background page, sets the
/// current page's link and draws the design behind the shapes — and
/// one Undo takes the whole step back.
#[test]
fn each_background_applies_its_design_in_one_undo_step() {
    for (design, page) in DESIGNS.into_iter().zip(PAGES) {
        let mut rig = shell_rig();
        open_gallery(&mut rig);
        assert!(
            has_named(&mut rig, "No Background"),
            "{design}: the gallery lists No Background"
        );
        click_named(&mut rig, design);

        // The link records and the design page is named for it.
        {
            let shell = rig.shell.borrow();
            let pages = pages(&shell.app);
            assert_eq!(pages.len(), 2, "{design}: the background page is created");
            assert_eq!(
                pages[1].0, page,
                "{design}: the design page carries its name"
            );
            assert_eq!(
                pages[0].1,
                Some(PageId { index: 1 }),
                "{design}: the link records"
            );
        }

        // The picture draws the design behind the shapes.
        match design {
            "Solid Tint" => assert!(
                region_matches(&mut rig, 1.0, 4.5, 2.0, 5.5, blue),
                "the tint draws"
            ),
            "Soft Gradient" => {
                assert!(
                    region_matches(&mut rig, 1.0, 4.5, 2.0, 5.5, blue),
                    "the gradient top draws in the colour"
                );
                assert!(
                    !region_matches(&mut rig, 1.0, 1.0, 2.0, 1.6, blue),
                    "the gradient bottom fades to white"
                );
            }
            _ => {
                assert!(
                    region_matches(&mut rig, 1.97, 4.5, 2.03, 5.5, blue),
                    "the grid line draws"
                );
                assert!(
                    !region_matches(&mut rig, 2.3, 4.2, 2.7, 4.8, blue),
                    "the field between the lines stays light"
                );
            }
        }

        // One undo step: the shapes, the page and the link leave.
        undo(&mut rig);
        let shell = rig.shell.borrow();
        assert_eq!(
            pages(&shell.app).len(),
            1,
            "{design}: undo removes the page"
        );
        assert_eq!(
            pages(&shell.app)[0].1,
            None,
            "{design}: undo removes the link"
        );
    }
}

/// A second application of a design reuses its background page, and
/// "No Background" clears the link.
#[test]
fn a_design_is_reused_and_no_background_clears() {
    let mut rig = shell_rig();
    open_gallery(&mut rig);
    click_named(&mut rig, "Solid Tint");
    open_gallery(&mut rig);
    click_named(&mut rig, "Solid Tint");
    {
        let shell = rig.shell.borrow();
        let pages = pages(&shell.app);
        assert_eq!(pages.len(), 2, "the design page is reused");
        assert_eq!(shape_count(&shell.app, "Background Tint"), 1);
    }
    open_gallery(&mut rig);
    click_named(&mut rig, "No Background");
    let shell = rig.shell.borrow();
    let pages = pages(&shell.app);
    assert_eq!(pages.len(), 2, "the page stays");
    assert_eq!(pages[0].1, None, "the link clears");
}

/// The Background Color submenu colours the design: "Red" turns the
/// tint red in the picture (task P7-54: "Background Color colours the
/// background page's design").
#[test]
fn background_color_recolors_the_design() {
    let mut rig = shell_rig();
    open_gallery(&mut rig);
    click_named(&mut rig, "Solid Tint");
    assert!(
        region_matches(&mut rig, 1.0, 4.5, 2.0, 5.5, blue),
        "the tint starts blue-ish"
    );
    open_gallery(&mut rig);
    click_named(&mut rig, "Background Color");
    click_named(&mut rig, "Red");
    assert_eq!(
        first_fill(&rig.shell.borrow().app, "Background Tint"),
        Some((0xff, 0x00, 0x00)),
        "the design's colour cell records"
    );
    assert!(
        region_matches(&mut rig, 1.0, 4.5, 2.0, 5.5, red),
        "the picture recolours"
    );
}

/// The design and its link survive a save and a reopen through the
/// real `file.save_as` and `file.open` commands.
#[test]
fn the_design_and_its_link_survive_save_and_reopen() {
    let mut rig = shell_rig();
    open_gallery(&mut rig);
    click_named(&mut rig, "Soft Gradient");
    let path =
        std::env::temp_dir().join(format!("larrygram-backgrounds-{}.vsdx", std::process::id()));
    dispatch_args(
        &mut rig,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(path.clone())),
    );
    dispatch_args(
        &mut rig,
        "file.open",
        Args::new().with("path", ArgValue::Path(path.clone())),
    );
    {
        let shell = rig.shell.borrow();
        let pages = pages(&shell.app);
        assert_eq!(pages.len(), 2, "the background page reopens");
        assert_eq!(pages[1].0, "Background Gradient");
        assert_eq!(pages[0].1, Some(PageId { index: 1 }), "the link reopens");
        assert_eq!(
            shape_count(&shell.app, "Background Gradient"),
            24,
            "the bands reopen"
        );
        assert_eq!(
            first_fill(&shell.app, "Background Gradient"),
            Some((0x7f, 0xb2, 0xe5)),
            "the design's colour reopens"
        );
    }
    let _ = std::fs::remove_file(&path);
}
