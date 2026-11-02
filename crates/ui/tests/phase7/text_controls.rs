//! The manual-pass fixes of task P7-29: the text controls follow the
//! text and the Text tool edits the shape it is used on, each driven
//! through the real input path in the full shell (`ShellRig`) in both
//! layouts (the master note) — canvas clicks, tool button clicks, the
//! context-menu row, typed text and the rendered picture. A test that
//! dispatches the command does not count for this task.
//!
//! - item 1: the text controls disable on a shape without text (the
//!   rule reverses P7-24 item 8's) and enable as soon as it has text
//!   or while its text is being edited;
//! - item 2: the Text tool on one selected shape starts that shape's
//!   text, caret at the end;
//! - item 3: a Text tool click on any shape edits that shape's text;
//! - item 4: the shape context menu's Edit Text row starts editing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::{ActiveTool, AppCtx};
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig};

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// The fixture picture's centre, in page inches (2 x 1 at (6, 3)).
const PICTURE: (f64, f64) = (6.0, 3.0);

/// The empty-page click target, below the fixture shapes.
const EMPTY: (f64, f64) = (3.0, 5.5);

/// The disabled hint of the text controls (task P7-29 item 1).
const ADD_TEXT: &str = "Add text first: double-click the shape or start typing.";

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
    // Centre the fixture shapes in the viewport (the `interaction`
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

/// The fixture picture of `rig`'s document.
fn picture(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[1]
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

/// The number of shapes on the active page.
fn shape_count(rig: &ShellRig) -> usize {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes.len()
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

/// A secondary press and release in one frame at `pos` (the canvas
/// context menu trigger, task P7-09 Goal 4).
fn right_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(pos));
    rig.run();
    for pressed in [true, false] {
        rig.harness().event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    rig.run();
}

/// A double click at `pos` (two clicks inside one double-click
/// window; the wait first drains any earlier window).
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    for _ in 0..60 {
        rig.run();
    }
    click_at(rig, pos);
    click_at(rig, pos);
}

/// A drag from `from` to `to` (press, moves over frames, release):
/// the pointer tool's selection marquee.
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2) {
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
    for step in 1..=4 {
        let pos = from + (to - from) * (step as f32 / 4.0);
        rig.harness().event(egui::Event::PointerMoved(pos));
        rig.run();
    }
    rig.harness().event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    rig.run();
}

/// Presses one key.
fn press_key(rig: &mut ShellRig, key: egui::Key) {
    rig.harness().key_press(key);
    rig.run();
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

/// Whether the in-place editor (the "Text" multiline input) is on
/// screen.
fn editing(rig: &mut ShellRig) -> bool {
    has(rig, Role::MultilineTextInput, "Text")
}

/// Types `text` into the in-place editor's field.
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

/// The plain text of `shape` (`None` when it has none).
fn shape_text(rig: &ShellRig, shape: ShapeId) -> Option<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .document
        .shape(shape)
        .expect("shape")
        .text
        .as_ref()
        .map(|text| {
            text.parts
                .iter()
                .filter_map(|part| match part {
                    larrygram_model::TextPart::Text(part) => Some(part.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
}

/// The number of one `section` row cell of `shape`.
fn row_number(rig: &ShellRig, shape: ShapeId, section: &str, name: &str) -> Option<f64> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|block| block.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
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

/// The role the font and size boxes draw in `kind`.
fn combo_role(kind: LayoutKind) -> Role {
    match kind {
        LayoutKind::PanelClassic => Role::ComboBox,
        _ => Role::Button,
    }
}

/// The centre alignment button's name in `kind`.
fn centre_name(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Align Center",
        _ => "Center",
    }
}

/// Every text control checked by item 1: the role and name pairs
/// (font, size, Bold, Italic, Underline and the three alignments).
fn text_controls(kind: LayoutKind) -> Vec<(Role, &'static str)> {
    let role = combo_role(kind);
    vec![
        (role, "Font"),
        (role, "Font Size"),
        (Role::Button, "Bold"),
        (Role::Button, "Italic"),
        (Role::Button, "Underline"),
        (Role::Button, "Align Left"),
        (Role::Button, centre_name(kind)),
        (Role::Button, "Align Right"),
    ]
}

/// Opens the Panel Classic fill picker (the "Fill Color menu" arrow;
/// Panel Modern's Fill split opens from its "More items" arrow).
fn open_fill_picker(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Fill Color menu"),
        _ => {
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

/// Gives the fixture rectangle a light fill: the default fill is dark
/// and the black text hides on it (and "No Fill" would leave the
/// shape's interior unhittable for the Text tool's click).
fn lighten_fill(rig: &mut ShellRig, kind: LayoutKind) {
    open_fill_picker(rig, kind);
    click(rig, Role::Button, "Surface #F5F5F5");
}

/// Selects the fixture rectangle with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = rectangle(&rig.shell.borrow().app);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![shape],
        "the canvas click selects the rectangle"
    );
}

/// Deselects everything with a click on the empty page.
fn deselect(rig: &mut ShellRig) {
    let pos = screen(rig, EMPTY.0, EMPTY.1);
    click_at(rig, pos);
    assert!(
        rig.shell.borrow().app.selection.shapes.is_empty(),
        "the empty-page click deselects"
    );
}

/// Chooses the Text tool with a real control click (the button is
/// "Text Tool" in Panel Classic and "Text" in Panel Modern).
fn choose_text_tool(rig: &mut ShellRig, kind: LayoutKind) {
    let name = match kind {
        LayoutKind::PanelClassic => "Text Tool",
        _ => "Text",
    };
    click(rig, Role::Button, name);
}

// ---------------------------------------------------------------------------
// Item 1: the text controls follow the text.
// ---------------------------------------------------------------------------

/// With the selection holding no shape with text every text control
/// is disabled with the "add text" hint, and the fill and line
/// controls are unchanged (task P7-29 item 1).
#[test]
fn the_text_controls_disable_on_a_shape_without_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        assert!(
            shape_text(&rig, rectangle(&rig.shell.borrow().app)).is_none(),
            "{kind:?}: the fixture starts without text"
        );
        for (role, name) in text_controls(kind) {
            assert!(
                disabled(&mut rig, role, name),
                "{kind:?}: {name} disables on a shape without text"
            );
            assert_eq!(
                tooltip(&mut rig, role, name).as_deref(),
                Some(ADD_TEXT),
                "{kind:?}: {name} carries the hint"
            );
        }
        // The shape formatting controls are unchanged.
        let line_weight = match kind {
            LayoutKind::PanelClassic => "Line Weight",
            _ => "Line",
        };
        assert!(
            !disabled(&mut rig, Role::Button, line_weight),
            "{kind:?}: the line control stays enabled"
        );
    }
}

/// The controls enable as soon as the shape has text, stay enabled
/// while another text-less shape shares the selection only when any
/// selected shape has text (task P7-29 item 1).
#[test]
fn the_text_controls_follow_the_text_across_the_selection() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let role = combo_role(kind);
        assert!(disabled(&mut rig, role, "Font"), "{kind:?}: no text yet");

        // As soon as the shape has text they enable.
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        press_key(&mut rig, egui::Key::Escape);
        assert!(
            !disabled(&mut rig, role, "Font"),
            "{kind:?}: the shape has text now"
        );

        // With several shapes selected the rule is any-of: a marquee
        // over both fixture shapes keeps the controls enabled.
        let from = screen(&rig, 1.5, 2.0);
        let to = screen(&rig, 7.5, 4.5);
        drag(&mut rig, from, to);
        assert_eq!(
            rig.shell.borrow().app.selection.shapes.len(),
            2,
            "the marquee selects both shapes"
        );
        assert!(
            !disabled(&mut rig, role, "Font"),
            "{kind:?}: one selected shape with text enables"
        );

        // Only the text-less picture selected: disabled again (the
        // empty-page click first: a click on a shape of a multi-
        // selection keeps the selection).
        deselect(&mut rig);
        let pos = screen(&rig, PICTURE.0, PICTURE.1);
        click_at(&mut rig, pos);
        assert_eq!(
            rig.shell.borrow().app.selection.shapes,
            vec![picture(&rig.shell.borrow().app)],
            "the click selects the picture"
        );
        assert!(
            disabled(&mut rig, role, "Font"),
            "{kind:?}: the text-less picture disables the controls"
        );
        assert_eq!(
            tooltip(&mut rig, role, "Font").as_deref(),
            Some(ADD_TEXT),
            "{kind:?}: and carries the hint"
        );
    }
}

/// While the shape's text is being edited the controls enable (the
/// format applies to the text being typed), even before it has any
/// text (task P7-29 item 1).
#[test]
fn the_text_controls_enable_while_the_text_is_being_edited() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        let role = combo_role(kind);
        assert!(disabled(&mut rig, role, "Font"), "{kind:?}: no text yet");
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the editor opens");
        assert!(
            shape_text(&rig, rectangle(&rig.shell.borrow().app)).is_none(),
            "{kind:?}: the shape still has no text"
        );
        for (role, name) in text_controls(kind) {
            assert!(
                !disabled(&mut rig, role, name),
                "{kind:?}: {name} enables while the text is being edited"
            );
        }
        press_key(&mut rig, egui::Key::Escape);
    }
}

// ---------------------------------------------------------------------------
// Item 2: the Text tool on a selected shape.
// ---------------------------------------------------------------------------

/// Choosing the Text tool with one shape selected starts that shape's
/// text, with the caret at the end (task P7-29 item 2): the editor
/// opens on the shape and the typed text lands in it.
#[test]
fn the_text_tool_on_a_selected_shape_edits_its_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        lighten_fill(&mut rig, kind);
        choose_text_tool(&mut rig, kind);
        assert!(
            editing(&mut rig),
            "{kind:?}: the choice opens the editor on the shape"
        );
        type_into_editor(&mut rig, "Hello");
        press_key(&mut rig, egui::Key::Escape);
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("Hello"),
            "{kind:?}: the typed text lands in the shape"
        );
        assert_eq!(
            row_number(&rig, shape, "Character", "Style"),
            None,
            "{kind:?}: the cells are the text's own"
        );
        // The picture: the text paints over the shape.
        let after = ink(&mut rig, 2.1, 2.55, 3.9, 3.45);
        assert!(after > 0, "{kind:?}: the text paints ({after})");
    }
}

/// The caret sits at the end of the text when the Text tool opens the
/// editor (task P7-29 item 2): typing appends after the old text even
/// when the earlier edit left the caret mid-text.
#[test]
fn the_text_tool_edit_puts_the_caret_at_the_end() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        // An earlier edit leaves the caret before the "b".
        double_click_at(&mut rig, pos);
        type_into_editor(&mut rig, "ab");
        press_key(&mut rig, egui::Key::ArrowLeft);
        press_key(&mut rig, egui::Key::Escape);
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(shape_text(&rig, shape).as_deref(), Some("ab"));

        // The Text tool on the selection: the caret is at the end, so
        // the typed "c" appends.
        select_shape(&mut rig);
        choose_text_tool(&mut rig, kind);
        assert!(editing(&mut rig), "{kind:?}: the editor opens");
        type_into_editor(&mut rig, "c");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("abc"),
            "{kind:?}: the caret sat at the end"
        );
    }
}

/// With nothing selected, or several shapes, choosing the Text tool
/// only arms the tool (task P7-29 item 2's last sentence).
#[test]
fn the_text_tool_with_nothing_or_several_selected_only_arms() {
    for kind in LAYOUTS {
        // Nothing selected: arm only.
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: nothing selected: no editor");
        assert_eq!(
            rig.shell.borrow().app.active_tool(),
            ActiveTool::Text,
            "{kind:?}: the tool is armed"
        );
        drop(rig);

        // Several shapes: arm only.
        let mut rig = shell_rig(kind);
        let from = screen(&rig, 1.5, 2.0);
        let to = screen(&rig, 7.5, 4.5);
        drag(&mut rig, from, to);
        assert_eq!(
            rig.shell.borrow().app.selection.shapes.len(),
            2,
            "the marquee selects both shapes"
        );
        choose_text_tool(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: several shapes: no editor");
        assert_eq!(rig.shell.borrow().app.active_tool(), ActiveTool::Text);
    }
}

// ---------------------------------------------------------------------------
// Item 3: a Text tool click on any shape edits that shape's text.
// ---------------------------------------------------------------------------

/// A Text tool click on a shape without text edits that shape's text
/// (task P7-29 item 3), and a click on empty page still starts a new
/// text box.
#[test]
fn the_text_tool_click_on_a_shape_edits_that_shape() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        lighten_fill(&mut rig, kind);
        deselect(&mut rig);
        let before = ink(&mut rig, 2.1, 2.55, 3.9, 3.45);

        // Arm the Text tool with nothing selected, then click the
        // shape (not a text box made with the tool).
        choose_text_tool(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: the tool only armed");
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click on a shape opens its text"
        );
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(
            rig.shell.borrow().app.selection.shapes,
            vec![shape],
            "{kind:?}: the clicked shape is selected"
        );
        type_into_editor(&mut rig, "Hi");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("Hi"),
            "{kind:?}: the typed text lands in the clicked shape"
        );
        let after = ink(&mut rig, 2.1, 2.55, 3.9, 3.45);
        assert!(
            after > before * 2,
            "{kind:?}: the text paints on the shape ({before} to {after})"
        );

        // A click on empty page still starts a new text box.
        let before = shape_count(&rig);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click on empty page starts a text box"
        );
        type_into_editor(&mut rig, "Box");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(shape_count(&rig), before + 1, "{kind:?}: one new box");
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Box"),
            "{kind:?}: the new text box carries its text"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 4: the shape context menu's Edit Text row.
// ---------------------------------------------------------------------------

/// The Edit Text row of the shape context menu starts editing the
/// shape's text in both layouts (task P7-29 item 4).
#[test]
fn the_edit_text_row_edits_the_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        lighten_fill(&mut rig, kind);
        let before = ink(&mut rig, 2.1, 2.55, 3.9, 3.45);

        // The shape's context menu opens on the right-click and its
        // Edit Text row starts the text.
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        right_click_at(&mut rig, pos);
        assert!(
            has(&mut rig, Role::MenuItem, "Edit Text"),
            "{kind:?}: the shape context menu has the Edit Text row"
        );
        click(&mut rig, Role::MenuItem, "Edit Text");
        assert!(
            editing(&mut rig),
            "{kind:?}: the row opens the editor on the shape"
        );
        type_into_editor(&mut rig, "Hi");
        press_key(&mut rig, egui::Key::Escape);
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("Hi"),
            "{kind:?}: the typed text lands in the shape"
        );
        let after = ink(&mut rig, 2.1, 2.55, 3.9, 3.45);
        assert!(
            after > before * 2,
            "{kind:?}: the text paints on the shape ({before} to {after})"
        );
    }
}
