//! The manual-pass fixes (task P7-24): every fix driven through the
//! real input path in the full shell (`ShellRig`) in both layouts —
//! a shape selected on the canvas, then the picker row, toolbar
//! button or key pressed — and checked on the shape's cells and the
//! rendered picture. A test that dispatches the command does not
//! count for this task (the master note).
//!
//! Item 0 is the root cause of the dead formatting: a press on a
//! picker or menu popup also reached the canvas under it (raw pointer
//! input), which deselected the shape before the format command ran.
//! `the_picker_click_keeps_the_selection_and_applies` pins the fix.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// A shell in `kind` with the rich state (the rectangle and the
/// picture), the rectangle deselected: the tests select it with a
/// real canvas click.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = kind;
    app.selection.shapes.clear();
    app.selection.anchor = None;
    let mut rig = ShellRig::with_app(app);
    // One double-click window for the text tests (the P7-19 rule).
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
    rig.run();
    // Centre the fixture shape in the viewport (the `interaction`
    // rig's pan rule): zoom 1, pan puts (3, 3) mid-canvas.
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
        session.view.pan = larrygram_geom::Point::new(
            f64::from(viewport.width()) / 2.0 / k - SHAPE.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - SHAPE.1),
        );
    }
    rig.run();
    rig.run();
    rig
}

/// The fixture rectangle of `rig`'s document.
fn rectangle(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the `interaction`
/// module's click rule: one harness run steps past egui's click time,
/// so the two halves must share a frame for the click to register).
fn click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    rig.run();
}

/// A double click at `pos` (two clicks inside one double-click window).
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    click_at(rig, pos);
    click_at(rig, pos);
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

/// Whether a widget with `role` and `name` is drawn.
fn has(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .is_some()
}

/// The disabled state of the first widget with `role` and `name`.
fn disabled(rig: &mut ShellRig, role: Role, name: &str) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"))
        .accesskit_node()
        .data()
        .is_disabled()
}

/// The tooltip (accesskit description) of the first widget with
/// `role` and `name`.
fn tooltip(rig: &mut ShellRig, role: Role, name: &str) -> Option<String> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(role, name)
        .next()
        .and_then(|node| {
            node.accesskit_node()
                .data()
                .description()
                .map(|d| d.to_owned())
        })
}

/// The text of one cell of `shape` (`None` when the shape has no such
/// cell or the cell holds no text).
fn cell_text(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.formula.clone())
}

/// The number value of one cell of `shape`.
fn cell_number(rig: &ShellRig, shape: ShapeId, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// The number value of one cell of `shape`, panicking when absent.
fn number(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
    cell_number(rig, shape, name).unwrap_or_else(|| panic!("no {name} on the shape"))
}

/// The current selection of `rig`.
fn selection(rig: &ShellRig) -> Vec<ShapeId> {
    rig.shell.borrow().app.selection.shapes.clone()
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = rectangle(&rig.shell.borrow().app);
    assert_eq!(selection(rig), vec![shape], "the canvas click selects");
}

/// The pixel at a page point in the rendered frame (the picture check).
fn pixel(rig: &mut ShellRig, x: f64, y: f64) -> (u8, u8, u8) {
    let pos = screen(rig, x, y);
    let image = rig.render();
    let px = image.get_pixel(pos.x as u32, pos.y as u32);
    (px[0], px[1], px[2])
}

/// The Panel Classic formatting split's picker opens from its
/// "Fill Color menu" arrow; Panel Modern's Fill split opens from its
/// "More items" arrow (the split beside the "Fill" label).
fn open_fill_picker(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Fill Color menu"),
        _ => {
            // The "More items" arrow nearest the "Fill" button.
            rig.run();
            let root = rig.harness().root();
            let fill = root
                .query_all_by_role_and_label(Role::Button, "Fill")
                .next()
                .expect("the Fill split")
                .rect();
            let best = root
                .query_all_by_role_and_label(Role::Button, "More items")
                .min_by(|a, b| {
                    (a.rect().center() - fill.center())
                        .length()
                        .total_cmp(&(b.rect().center() - fill.center()).length())
                })
                .expect("a More items arrow");
            best.click();
            rig.run();
        }
    }
}

/// Item 0 (the root cause): a picker click must keep the selection
/// and apply — the press once fell through the popup to the canvas
/// and deselected the shape before the format command ran.
#[test]
fn the_picker_click_keeps_the_selection_and_applies() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        open_fill_picker(&mut rig, kind);
        assert!(has(&mut rig, Role::Button, "Accent #1F5BC4"), "{kind:?}");
        click(&mut rig, Role::Button, "Accent #1F5BC4");
        assert_eq!(
            selection(&rig),
            vec![shape],
            "{kind:?}: the selection stays"
        );
        assert_eq!(
            cell_text(&rig, shape, "FillForegnd").as_deref(),
            Some("RGB(31,91,196)"),
            "{kind:?}: the fill cell changes"
        );
        // The picture: the shape's centre paints the new fill.
        let (r, g, b) = pixel(&mut rig, SHAPE.0, SHAPE.1);
        assert!(
            (r as i32 - 31).abs() <= 4 && (g as i32 - 91).abs() <= 4 && (b as i32 - 196).abs() <= 4,
            "{kind:?}: the picture shows the fill: got ({r},{g},{b})"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 1: formatting applies from the Format menu, the formatting
// toolbar (Panel Classic) and the Home tab (Panel Modern).
// ---------------------------------------------------------------------------

/// Opens the classic Format menu of `rig` and walks to `path` (menu
/// title, then one row label per level), leaving the deepest level
/// open.
fn open_format_menu(rig: &mut ShellRig, path: &[&str]) {
    click(rig, Role::Button, "Format");
    for label in &path[..path.len() - 1] {
        click(rig, Role::MenuItem, label);
    }
}

/// The Format menu's five controls apply from the menu rows (goal 1).
#[test]
fn the_format_menu_rows_apply_to_the_selection() {
    let mut rig = shell_rig(LAYOUTS[0]);
    select_shape(&mut rig);
    let shape = rectangle(&rig.shell.borrow().app);

    // Fill Color ▸ Red.
    open_format_menu(&mut rig, &["Fill Color", "Red"]);
    click(&mut rig, Role::MenuItem, "Red");
    assert_eq!(
        cell_text(&rig, shape, "FillForegnd").as_deref(),
        Some("RGB(255,0,0)"),
        "the Fill Color row applies"
    );

    // Line Weight ▸ 2.25 pt.
    open_format_menu(&mut rig, &["Line Weight", "2.25 pt"]);
    click(&mut rig, Role::MenuItem, "2.25 pt");
    let weight = number(&rig, shape, "LineWeight");
    assert!(
        (weight - 2.25 / 72.0).abs() < 1.0e-9,
        "the Line Weight row applies ({weight})"
    );

    // Line Pattern ▸ Dashed.
    open_format_menu(&mut rig, &["Line Pattern", "Dashed"]);
    click(&mut rig, Role::MenuItem, "Dashed");
    assert_eq!(
        number(&rig, shape, "LinePattern"),
        2.0,
        "the Line Pattern row applies"
    );

    // Arrowheads ▸ Stealth.
    open_format_menu(&mut rig, &["Arrowheads", "Stealth"]);
    click(&mut rig, Role::MenuItem, "Stealth");
    assert_eq!(
        number(&rig, shape, "BeginArrow"),
        3.0,
        "the Arrowheads row applies"
    );
}

/// Without a selection the Format menu rows disable with the reason
/// (goal 1).
#[test]
fn the_format_menu_rows_disable_without_a_selection() {
    let mut rig = shell_rig(LAYOUTS[0]);
    open_format_menu(&mut rig, &["Fill Color", "Red"]);
    assert!(has(&mut rig, Role::MenuItem, "Red"), "the row draws");
    assert!(
        disabled(&mut rig, Role::MenuItem, "Red"),
        "the row disables without a selection"
    );
    assert_eq!(
        tooltip(&mut rig, Role::MenuItem, "Red").as_deref(),
        Some("no selection"),
        "the row carries the reason"
    );
}

/// The Panel Classic formatting toolbar's splits apply from their
/// pickers (goal 1) and disable without a selection.
#[test]
fn the_classic_formatting_toolbar_applies_and_disables() {
    let mut rig = shell_rig(LAYOUTS[0]);
    select_shape(&mut rig);
    let shape = rectangle(&rig.shell.borrow().app);
    // Line Weight split → picker row "2.25 pt".
    click(&mut rig, Role::Button, "Line Weight menu");
    click(&mut rig, Role::Button, "2.25 pt");
    let weight = number(&rig, shape, "LineWeight");
    assert!(
        (weight - 2.25 / 72.0).abs() < 1.0e-9,
        "the toolbar split applies ({weight})"
    );

    // With no selection the split's main button disables with the
    // reason.
    drop(rig);
    let mut rig = shell_rig(LAYOUTS[0]);
    assert!(
        disabled(&mut rig, Role::Button, "Line Weight"),
        "the split disables without a selection"
    );
    assert_eq!(
        tooltip(&mut rig, Role::Button, "Line Weight").as_deref(),
        Some("no selection"),
        "the split carries the reason"
    );
}

/// The Panel Modern Home tab applies the same five controls (goal 1).
#[test]
fn the_home_tab_applies_the_line_controls() {
    let mut rig = shell_rig(LAYOUTS[1]);
    select_shape(&mut rig);
    let shape = rectangle(&rig.shell.borrow().app);
    // Line split ▸ Weight ▸ "2.25 pt".
    open_line_weight(&mut rig);
    click(&mut rig, Role::Button, "2.25 pt");
    let weight = number(&rig, shape, "LineWeight");
    assert!(
        (weight - 2.25 / 72.0).abs() < 1.0e-9,
        "the Home tab applies the weight ({weight})"
    );
    // And the fill from the Fill split's picker (goal 1's colour).
    open_fill_picker(&mut rig, LAYOUTS[1]);
    click(&mut rig, Role::Button, "Accent #1F5BC4");
    assert_eq!(
        cell_text(&rig, shape, "FillForegnd").as_deref(),
        Some("RGB(31,91,196)"),
        "the Fill split applies"
    );
}

/// The Home tab's Line split disables without a selection (goal 1).
#[test]
fn the_home_tab_line_split_disables_without_a_selection() {
    let mut rig = shell_rig(LAYOUTS[1]);
    assert!(
        disabled(&mut rig, Role::Button, "Line"),
        "the Line split disables without a selection"
    );
    assert_eq!(
        tooltip(&mut rig, Role::Button, "Line").as_deref(),
        Some("no selection"),
        "the Line split carries the reason"
    );
}

/// Opens the Panel Modern Line split's Weight submenu (the split's
/// arrow, then the "Weight" row).
fn open_line_weight(rig: &mut ShellRig) {
    // The "More items" arrow nearest the "Line" button.
    rig.run();
    let root = rig.harness().root();
    let line = root
        .query_all_by_role_and_label(Role::Button, "Line")
        .next()
        .expect("the Line split")
        .rect();
    let best = root
        .query_all_by_role_and_label(Role::Button, "More items")
        .min_by(|a, b| {
            (a.rect().center() - line.center())
                .length()
                .total_cmp(&(b.rect().center() - line.center()).length())
        })
        .expect("a More items arrow");
    best.click();
    rig.run();
    // The Weight submenu row (Panel Modern menu rows are buttons).
    click(rig, Role::Button, "Weight");
}

// ---------------------------------------------------------------------------
// Shared helpers: the Character-row cells, the newest shape, typing
// and the picture's ink count (goal 2's picture check).
// ---------------------------------------------------------------------------

/// The value of one cell of `shape`'s first row of `section` (the
/// `Character` `Font` and `Size` rows; `format.rs`'s `row_cell_of`).
fn row_cell(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<CellValue> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .document
        .shape(shape)
        .expect("shape")
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|block| block.rows.first())
        .and_then(|row| row.cells.iter().find(|c| c.name.as_str() == name))
        .and_then(|cell| cell.value.clone())
}

/// The text of one `section` row cell of `shape`.
fn row_str(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<String> {
    match row_cell(rig, shape, section, name) {
        Some(CellValue::Str(text)) => Some(text),
        _ => None,
    }
}

/// The number of one `section` row cell of `shape`.
fn row_number(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<f64> {
    match row_cell(rig, shape, section, name) {
        Some(CellValue::Number(value)) => Some(value),
        _ => None,
    }
}

/// The shape most recently added to the active page.
fn last_shape(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    *session
        .document
        .page(page)
        .expect("page")
        .shapes
        .last()
        .expect("a shape")
}

/// Types `text` into the in-place editor's field (the multiline
/// input named "Text").
fn type_into_editor(rig: &mut ShellRig, text: &str) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
        .next()
        .expect("the editor field");
    node.type_text(text);
    rig.run();
}

/// Presses one key.
fn press_key(rig: &mut ShellRig, key: egui::Key) {
    rig.harness().key_press(key);
    rig.run();
}

/// The count of non-background pixels in a page-rectangle region of
/// the rendered frame (the picture check: the background is the
/// region's most common colour).
fn ink(rig: &mut ShellRig, x0: f64, y0: f64, x1: f64, y1: f64) -> usize {
    let a = screen(rig, x0, y0);
    let b = screen(rig, x1, y1);
    let image = rig.render();
    let width = image.width();
    let height = image.height();
    let xmin = (a.x.min(b.x).max(0.0) as u32).min(width.saturating_sub(1));
    let xmax = (a.x.max(b.x).max(0.0) as u32).min(width);
    let ymin = (a.y.min(b.y).max(0.0) as u32).min(height.saturating_sub(1));
    let ymax = (a.y.max(b.y).max(0.0) as u32).min(height);
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for x in xmin..xmax {
        for y in ymin..ymax {
            let px = image.get_pixel(x, y);
            *counts.entry((px[0], px[1], px[2])).or_default() += 1;
        }
    }
    let background = counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| *colour)
        .unwrap_or((0, 0, 0));
    let mut inked = 0;
    for x in xmin..xmax {
        for y in ymin..ymax {
            let px = image.get_pixel(x, y);
            let far = i32::from(px[0]) - i32::from(background.0);
            let far = far
                .abs()
                .max((i32::from(px[1]) - i32::from(background.1)).abs());
            let far = far.max((i32::from(px[2]) - i32::from(background.2)).abs());
            if far > 40 {
                inked += 1;
            }
        }
    }
    inked
}

// ---------------------------------------------------------------------------
// Goal 2: font and size. The Classic font and size boxes get
// drop-down lists like Panel Modern's (P7-21's open issue); both
// layouts apply from the lists with a shape or text selected.
// ---------------------------------------------------------------------------

/// The role the font and size boxes draw in `kind`.
fn combo_role(kind: LayoutKind) -> Role {
    match kind {
        LayoutKind::PanelClassic => Role::ComboBox,
        _ => Role::Button,
    }
}

/// Opens the font family list of `kind`'s font box (Classic's box
/// opens from its "Font menu" strip, Modern's box from its face).
fn open_font_list(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Font menu"),
        _ => click(rig, Role::Button, "Font"),
    }
}

/// Opens the size list of `kind`'s font size box.
fn open_size_list(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Font Size menu"),
        _ => click(rig, Role::Button, "Font Size"),
    }
}

/// The font and size boxes are enabled with the shape selected, and
/// their drop-down lists apply a family and a size (goal 2): the
/// `Character` row cells change and the picture's text grows with
/// the size.
#[test]
fn the_font_and_size_lists_apply_to_the_shape() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        // Give the rectangle text: a double click opens the in-place
        // editor, Escape commits the buffer.
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the double click opens the editor"
        );
        type_into_editor(&mut rig, "MMMM");
        press_key(&mut rig, egui::Key::Escape);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);

        // Enabled with the shape selected.
        let role = combo_role(kind);
        assert!(
            !disabled(&mut rig, role, "Font"),
            "{kind:?}: the font box enables"
        );
        assert!(
            !disabled(&mut rig, role, "Font Size"),
            "{kind:?}: the size box enables"
        );

        // The family list applies "Carlito".
        open_font_list(&mut rig, kind);
        assert!(
            has(&mut rig, Role::Button, "Carlito"),
            "{kind:?}: the drop-down list draws the families"
        );
        click(&mut rig, Role::Button, "Carlito");
        assert_eq!(
            row_str(&rig, shape, "Character", "Font").as_deref(),
            Some("Carlito"),
            "{kind:?}: the family row writes the Character Font cell"
        );

        // The size list applies 18 pt (0.25 in).
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "18 pt");
        let size = row_number(&rig, shape, "Character", "Size")
            .unwrap_or_else(|| panic!("{kind:?}: the Size cell"));
        assert!(
            (size - 0.25).abs() < 1.0e-9,
            "{kind:?}: the size row writes the Size cell ({size})"
        );

        // The picture: the text grows with the size (ink count over
        // the text block region; the text is top-anchored). "No Fill"
        // first: the default fill is dark and the black text hides
        // on it.
        open_fill_picker(&mut rig, kind);
        click(&mut rig, Role::Button, "No Fill");
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "8 pt");
        let small = ink(&mut rig, 2.05, 2.55, 3.95, 3.6);
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "36 pt");
        let big = ink(&mut rig, 2.05, 2.55, 3.95, 3.6);
        assert!(small > 0, "{kind:?}: the text paints ({small})");
        assert!(
            big > small * 2,
            "{kind:?}: the rendered text grows with the size ({small} to {big})"
        );
    }
}

/// Right after adding text the font and size boxes stay enabled and
/// apply to the new text box (goal 2's "including right after adding
/// text").
#[test]
fn the_font_and_size_apply_right_after_adding_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        // The Text tool: a click on empty canvas makes a text box and
        // opens the editor (classic "Text Tool", modern "Text").
        match kind {
            LayoutKind::PanelClassic => click(&mut rig, Role::Button, "Text Tool"),
            _ => click(&mut rig, Role::Button, "Text"),
        }
        let pos = screen(&rig, 3.0, 5.0);
        click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MultilineTextInput, "Text"),
            "{kind:?}: the editor opens on the new text box"
        );
        type_into_editor(&mut rig, "Hello");
        press_key(&mut rig, egui::Key::Escape);

        // Enabled immediately, with no reselection.
        let role = combo_role(kind);
        assert!(
            !disabled(&mut rig, role, "Font"),
            "{kind:?}: the font box enables right after adding text"
        );
        assert!(
            !disabled(&mut rig, role, "Font Size"),
            "{kind:?}: the size box enables right after adding text"
        );

        // The lists apply to the new shape.
        let new = last_shape(&rig.shell.borrow().app);
        open_font_list(&mut rig, kind);
        click(&mut rig, Role::Button, "Carlito");
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "18 pt");
        assert_eq!(
            row_str(&rig, new, "Character", "Font").as_deref(),
            Some("Carlito"),
            "{kind:?}: the family applies to the new text"
        );
        let size = row_number(&rig, new, "Character", "Size")
            .unwrap_or_else(|| panic!("{kind:?}: the Size cell"));
        assert!(
            (size - 0.25).abs() < 1.0e-9,
            "{kind:?}: the size applies to the new text ({size})"
        );

        // The picture: the new text paints and grows with the size
        // ("No Fill" first: the default fill is dark and the black
        // text hides on it).
        open_fill_picker(&mut rig, kind);
        click(&mut rig, Role::Button, "No Fill");
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "8 pt");
        let small = ink(&mut rig, 2.05, 4.65, 3.95, 5.3);
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "36 pt");
        let big = ink(&mut rig, 2.05, 4.65, 3.95, 5.3);
        assert!(small > 0, "{kind:?}: the new text paints ({small})");
        assert!(
            big > small * 2,
            "{kind:?}: the new text grows with the size ({small} to {big})"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 3: the Panel Classic Zoom box. Panel Modern has no zoom box
// (status-bar slider and the View > Zoom dialog only): not
// applicable there.
// ---------------------------------------------------------------------------

/// The Zoom box's typed value sets the zoom (the `shape.draw.zoom`
/// contract: `percent`) and its drop-down offers the View > Zoom
/// presets (goal 3, Panel Classic only).
#[test]
fn the_zoom_box_sets_the_zoom_and_offers_presets() {
    let mut rig = shell_rig(LAYOUTS[0]);
    // A typed percent and Enter.
    rig.run();
    {
        let root = rig.harness().root();
        let combo = root
            .query_all_by_role_and_label(Role::ComboBox, "Zoom")
            .next()
            .expect("the Zoom box");
        combo.click();
        combo.type_text("150");
    }
    rig.run();
    press_key(&mut rig, egui::Key::Enter);
    let zoom = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .view
        .zoom;
    assert!(
        (zoom - 1.5).abs() < 1.0e-9,
        "typing 150 sets the zoom ({zoom})"
    );

    // The drop-down presets.
    click(&mut rig, Role::Button, "Zoom menu");
    assert!(has(&mut rig, Role::Button, "400%"), "the presets draw");
    click(&mut rig, Role::Button, "50%");
    let zoom = rig
        .shell
        .borrow()
        .app
        .active_document()
        .expect("session")
        .view
        .zoom;
    assert!(
        (zoom - 0.5).abs() < 1.0e-9,
        "the 50% preset applies ({zoom})"
    );
}

// ---------------------------------------------------------------------------
// Goal 6: the Format Painter on the canvas: a click on a shape applies
// the copied format, Escape cancels (P7-21's open issue).
// ---------------------------------------------------------------------------

/// The fixture picture shape of `rig`'s document (the second shape).
fn picture(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[1]
}

/// Selects the fixture rectangle, colours it with the fill picker and
/// arms the Format Painter with the button: the copy source.
fn arm_format_painter(rig: &mut ShellRig, kind: LayoutKind) {
    select_shape(rig);
    open_fill_picker(rig, kind);
    click(rig, Role::Button, "Accent #1F5BC4");
    click(rig, Role::Button, "Format Painter");
    assert!(
        rig.shell.borrow().app.format_painter.is_some(),
        "{kind:?}: the button records the copy"
    );
}

/// After the button, a click on a shape applies the copied format
/// (goal 6): the fill cell follows the source and the picture shows
/// it; the copy clears.
#[test]
fn the_format_painter_applies_from_a_canvas_click() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        arm_format_painter(&mut rig, kind);
        let source = rectangle(&rig.shell.borrow().app);
        let target = picture(&rig.shell.borrow().app);

        // A click on the picture shape at (6, 3).
        let pos = screen(&rig, 6.0, 3.0);
        click_at(&mut rig, pos);
        assert_eq!(
            selection(&rig),
            vec![target],
            "{kind:?}: the click selects the target"
        );
        let source_fill = cell_text(&rig, source, "FillForegnd");
        assert!(source_fill.is_some(), "{kind:?}: the source has a fill");
        assert_eq!(
            cell_text(&rig, target, "FillForegnd"),
            source_fill,
            "{kind:?}: the copied fill reaches the target"
        );
        assert!(
            rig.shell.borrow().app.format_painter.is_none(),
            "{kind:?}: the copy clears after applying"
        );
        // Beside the placeholder mark at the shape's centre.
        let (r, g, b) = pixel(&mut rig, 5.5, 3.2);
        assert!(
            (r as i32 - 31).abs() <= 4
                && (g as i32 - 91).abs() <= 4
                && (b as i32 - 196).abs() <= 4,
            "{kind:?}: the picture paints the copied fill: got ({r},{g},{b})"
        );
    }
}

/// Escape cancels the Format Painter (goal 6): the copy drops and a
/// click on a shape applies nothing.
#[test]
fn the_escape_cancels_the_format_painter() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        arm_format_painter(&mut rig, kind);
        press_key(&mut rig, egui::Key::Escape);
        assert!(
            rig.shell.borrow().app.format_painter.is_none(),
            "{kind:?}: Escape drops the copy"
        );
        // A click on the picture applies nothing.
        let target = picture(&rig.shell.borrow().app);
        let pos = screen(&rig, 6.0, 3.0);
        click_at(&mut rig, pos);
        assert_eq!(
            cell_text(&rig, target, "FillForegnd").as_deref(),
            None,
            "{kind:?}: the click after Escape applies nothing"
        );
    }
}

// ---------------------------------------------------------------------------
// Goal 7: the transparency rows preview the fill colour over a
// checkerboard at that transparency, and choosing one applies.
// ---------------------------------------------------------------------------

/// Whether any pixel of the frame inside `rect` is within `tolerance`
/// of `target`.
fn row_shows(rig: &mut ShellRig, rect: egui::Rect, target: (i32, i32, i32), tolerance: i32) -> bool {
    let image = rig.render();
    let x0 = (rect.min.x.max(0.0) as u32).min(image.width());
    let x1 = (rect.max.x.max(0.0) as u32).min(image.width());
    let y0 = (rect.min.y.max(0.0) as u32).min(image.height());
    let y1 = (rect.max.y.max(0.0) as u32).min(image.height());
    for x in x0..x1 {
        for y in y0..y1 {
            let px = image.get_pixel(x, y);
            if (i32::from(px[0]) - target.0).abs() <= tolerance
                && (i32::from(px[1]) - target.1).abs() <= tolerance
                && (i32::from(px[2]) - target.2).abs() <= tolerance
            {
                return true;
            }
        }
    }
    false
}

/// The transparency rows show the fill colour over a checkerboard at
/// their transparency (goal 7) and choosing one applies it.
#[test]
fn the_transparency_rows_preview_and_apply() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let shape = rectangle(&rig.shell.borrow().app);
        // The accent fill first: the preview tint is the fill colour.
        open_fill_picker(&mut rig, kind);
        click(&mut rig, Role::Button, "Accent #1F5BC4");
        open_fill_picker(&mut rig, kind);

        // The swatch of the opaque row paints the fill colour; the
        // fully transparent row shows the checkerboard only.
        let opaque = {
            rig.run();
            let root = rig.harness().root();
            root.query_all_by_role_and_label(Role::Button, "0%")
                .next()
                .expect("the 0% row")
                .rect()
        };
        assert!(
            row_shows(&mut rig, opaque, (31, 91, 196), 6),
            "{kind:?}: the opaque swatch paints the fill colour"
        );
        let clear = {
            rig.run();
            let root = rig.harness().root();
            root.query_all_by_role_and_label(Role::Button, "100%")
                .next()
                .expect("the 100% row")
                .rect()
        };
        assert!(
            !row_shows(&mut rig, clear, (31, 91, 196), 6),
            "{kind:?}: the transparent swatch shows the checkerboard only"
        );

        // Choosing a row applies it.
        click(&mut rig, Role::Button, "50%");
        let trans = cell_number(&rig, shape, "FillForegndTrans")
            .unwrap_or_else(|| panic!("{kind:?}: the FillForegndTrans cell"));
        assert!(
            (trans - 0.5).abs() < 1.0e-9,
            "{kind:?}: the 50% row applies ({trans})"
        );
    }
}
