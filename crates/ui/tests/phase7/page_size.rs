//! Task P7-27 item 1: one page-size resolver. The Page Setup dialog,
//! the rendered page and zoom-to-fit read one size — resolved by
//! `Document::page_size` (the P7-27 amendment of ADR 0019) — for a
//! page whose size comes from its style. Every assertion here drives
//! the real input path in the full shell (`ShellRig`) in both layouts
//! (the master note: a test that dispatches the command does not
//! count).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_model::{CellName, CellValue, QName, Style, StyleId, XmlAttribute};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The size the fixture style sheet carries (P7-21's case: the page's
/// own sheet has no size; the style its `LineStyle` names does).
const STYLE_SIZE: (f64, f64) = (10.0, 6.0);

/// One cached number cell.
fn number_cell(name: &'static str, value: f64) -> larrygram_model::Cell {
    let mut cell = larrygram_model::Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(value));
    cell
}

/// A shell whose active page takes its size from the style its
/// `LineStyle` link names (`background` adds a second page with
/// `STYLE_SIZE` and points the foreground's `BackPage` at it: the
/// dropped rung, the P7-27 amendment of ADR 0019).
fn style_rig(kind: LayoutKind, styled: bool, background: bool) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    {
        let session = app.active_document_mut().expect("session");
        if styled {
            let mut style = Style::new(StyleId { index: 0 });
            style.visio_id = 7;
            style.sheet.cells = vec![
                number_cell("PageWidth", STYLE_SIZE.0),
                number_cell("PageHeight", STYLE_SIZE.1),
            ];
            session.document.stylesheet.styles.push(style);
            session.document.pages[0]
                .preserved
                .attributes
                .push(XmlAttribute {
                    name: QName {
                        prefix: None,
                        uri: None,
                        local: "LineStyle".to_owned(),
                    },
                    value: "7".to_owned(),
                });
        }
        if background {
            let second = session.document.add_page("Page-2").expect("page");
            session.document.pages[1].sheet.cells = vec![
                number_cell("PageWidth", STYLE_SIZE.0),
                number_cell("PageHeight", STYLE_SIZE.1),
            ];
            session.document.pages[0].background = Some(second);
        }
    }
    let mut rig = ShellRig::with_app(app);
    rig.run();
    rig
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

/// The value of the dialog field named `name`.
fn field_text(rig: &mut ShellRig, name: &str) -> String {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::TextInput, name)
        .next()
        .unwrap_or_else(|| panic!("no field named {name:?}"))
        .accesskit_node()
        .data()
        .value()
        .unwrap_or_default()
        .to_owned()
}

/// Opens the Page Setup dialog from `kind`'s own control.
fn open_page_setup(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Page Setup...");
        }
        _ => {
            click(rig, Role::Button, "Design");
            click(rig, Role::Button, "Page Setup...");
        }
    }
}

/// The scene builder's page size of the active page.
fn rendered_size(rig: &ShellRig) -> (f64, f64) {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    let scene = larrygram_render::build_scene(
        &session.document,
        page,
        &larrygram_render::BuildOptions::new(),
    )
    .expect("the scene builds");
    (scene.width, scene.height)
}

/// Runs zoom-to-fit through `kind`'s own control and answers the zoom
/// and the measured viewport it fit into.
fn zoom_fit(rig: &mut ShellRig, kind: LayoutKind) -> (f64, (f64, f64)) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "View");
            click(rig, Role::MenuItem, "Zoom");
            click(rig, Role::MenuItem, "Whole Page");
        }
        _ => {
            click(rig, Role::Button, "View");
            click(rig, Role::Button, "Fit to Window");
        }
    }
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let (width, height) = session.view.viewport.expect("the host measured the canvas");
    (session.view.zoom, (width, height))
}

/// The fit of `size` into the `viewport` rectangle, the `view.zoom.fit`
/// formula (points per inch: 96).
fn expected_fit(size: (f64, f64), viewport: (f64, f64)) -> f64 {
    ((viewport.0 / (size.0 * 96.0)).min(viewport.1 / (size.1 * 96.0))).clamp(0.05, 40.0)
}

/// Item 1's acceptance: for a page whose size comes from its style,
/// the dialog, the rendered page and zoom-to-fit agree, in both
/// layouts, through each layout's own controls.
#[test]
fn the_dialog_the_rendered_page_and_zoom_to_fit_agree() {
    for kind in LAYOUTS {
        let mut rig = style_rig(kind, true, false);
        open_page_setup(&mut rig, kind);
        assert_eq!(
            field_text(&mut rig, "Width").as_str(),
            "10",
            "{kind:?}: the dialog width"
        );
        assert_eq!(
            field_text(&mut rig, "Height").as_str(),
            "6",
            "{kind:?}: the dialog height"
        );
        click(&mut rig, Role::Button, "Cancel");
        assert_eq!(
            rendered_size(&rig),
            STYLE_SIZE,
            "{kind:?}: the rendered page"
        );
        let (zoom, viewport) = zoom_fit(&mut rig, kind);
        let fit = expected_fit(STYLE_SIZE, viewport);
        assert!(
            (zoom - fit).abs() < 1.0e-9,
            "{kind:?}: zoom-to-fit uses the style size: {zoom} vs {fit}"
        );
    }
}

/// The dropped rung: a background page does not give its size to the
/// dialog or the rendered page (the P7-27 amendment of ADR 0019).
#[test]
fn a_background_page_gives_no_size_to_the_dialog_or_the_page() {
    for kind in LAYOUTS {
        let mut rig = style_rig(kind, false, true);
        open_page_setup(&mut rig, kind);
        assert_eq!(
            field_text(&mut rig, "Width").as_str(),
            "8.5",
            "{kind:?}: the dialog falls back to Letter"
        );
        assert_eq!(
            field_text(&mut rig, "Height").as_str(),
            "11",
            "{kind:?}: the dialog falls back to Letter"
        );
        click(&mut rig, Role::Button, "Cancel");
        assert_eq!(
            rendered_size(&rig),
            (8.5, 11.0),
            "{kind:?}: the rendered page falls back to Letter"
        );
    }
}
