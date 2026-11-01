//! Task P7-27 item 5: the typed combo values. The Classic Font Size
//! box applies a typed size with Enter and the Zoom box alone sends
//! the `percent` argument (the P7-24 review A6), driven through the
//! real input path in the full shell (`ShellRig`).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// A shell in Panel Classic with the rich state (the fixture
/// rectangle selected), as in the P7-24 manual-pass rig.
fn shell_rig() -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = LayoutKind::PanelClassic;
    ShellRig::with_app(app)
}

/// The fixture rectangle of `rig`'s document.
fn rectangle(rig: &ShellRig) -> ShapeId {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

/// The number value of one cell of `shape`'s first `section` row.
fn row_number(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|section| section.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// Types `text` into the combo named `name` and presses Enter (the
/// zoom-box test's typed path).
fn type_into_combo(rig: &mut ShellRig, name: &str, text: &str) {
    rig.run();
    let root = rig.harness().root();
    let combo = root
        .query_all_by_role_and_label(Role::ComboBox, name)
        .next()
        .unwrap_or_else(|| panic!("no combo named {name:?}"));
    combo.click();
    combo.type_text(text);
    rig.run();
    rig.harness().key_press(egui::Key::Enter);
    rig.run();
}

/// The zoom of the active document.
fn zoom(rig: &ShellRig) -> f64 {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session.view.zoom
}

/// Item 5: typing a size into the Font Size box applies the size and
/// sends no `percent` (the zoom value never leaks into another combo).
/// **Panel Modern: no typed font-size combo (its face opens the size
/// list, P7-24 goal 2); not applicable there.**
#[test]
fn the_typed_font_size_applies_and_leaves_the_zoom_alone() {
    let mut rig = shell_rig();
    let shape = rectangle(&rig);
    assert_eq!(zoom(&rig), 1.0, "the fixture zoom is 100%");
    type_into_combo(&mut rig, "Font Size", "36");
    assert_eq!(
        row_number(&rig, shape, "Character", "Size"),
        Some(0.5),
        "36 pt records as 0.5 in"
    );
    assert_eq!(zoom(&rig), 1.0, "the zoom does not move");
}
