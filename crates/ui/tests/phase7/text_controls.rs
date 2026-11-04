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

// ---------------------------------------------------------------------------
// Goal 1 (task P7-37 item 1): a new text box can be formatted while it
// is typed. The box is a shape from the click, inside its undo group,
// so the controls enable at once and the format commands write its
// cells. Each ending of the editor closes the group.
// ---------------------------------------------------------------------------

/// Presses Cmd/Ctrl+key (the shortcut table's chord).
fn chord(rig: &mut ShellRig, key: egui::Key) {
    let harness = rig.harness();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    harness.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    rig.run();
}

/// Opens the font-size list (the "Font Size menu" arrow in Panel
/// Classic, the size face in Panel Modern).
fn open_size_list(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => click(rig, Role::Button, "Font Size menu"),
        _ => click(rig, Role::Button, "Font Size"),
    }
}

/// Ends the document through the layout's own control (task P7-37
/// item 5): Panel Classic's File > Close row. Panel Modern's File view
/// carries no Close row, so the command dispatches there.
fn close_document(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Close");
        }
        _ => rig.dispatch("file.close"),
    }
}

/// The undoable steps of the active document, oldest first.
fn undo_labels(rig: &ShellRig) -> Vec<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .undo
        .undo_labels()
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// The text controls are enabled while a new text box is typed, and a
/// Bold click before anything is typed keeps the editor open and
/// formats the box (task P7-41 item 1, review case 1: the old rule
/// committed on the click, cancelled the group and dropped the box).
/// Escape commits the typed text in the open group: one step labelled
/// as the insert, the text bold.
#[test]
fn a_new_text_box_can_be_formatted_while_it_is_typed() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the click opens the box");
        assert_eq!(
            shape_count(&rig),
            before + 1,
            "{kind:?}: the box is a shape while it is typed"
        );
        let box_ = last_shape(&rig.shell.borrow().app);
        for (role, name) in text_controls(kind) {
            assert!(
                !disabled(&mut rig, role, name),
                "{kind:?}: {name} enables while the new box is typed"
            );
        }
        // Bold before typing: the click keeps the edit and the format
        // lands in the open group.
        click(&mut rig, Role::Button, "Bold");
        assert!(
            editing(&mut rig),
            "{kind:?}: the Bold click keeps the editor open"
        );
        assert!(
            undo_labels(&rig).is_empty(),
            "{kind:?}: the steps collect in the open group, not on the stack"
        );
        type_into_editor(&mut rig, "MMMM");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("MMMM"),
            "{kind:?}: the typed text is written"
        );
        assert_eq!(
            row_number(&rig, box_, "Character", "Style"),
            Some(1.0),
            "{kind:?}: the text is bold"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: the commit is one step labelled as the insert"
        );
        // The picture paints the text at that format.
        let after = ink(&mut rig, 3.1, 5.55, 4.9, 5.95);
        assert!(after > 0, "{kind:?}: the text paints ({after})");
    }
}

/// Typing, a Bold click, more typing, a font size from the list and
/// Escape (task P7-41 item 2's second bullet): the editor stays open
/// after every click, every keystroke is kept where the caret sat
/// (the editor keeps the keyboard and the caret), and the whole box
/// is one undo step.
#[test]
fn formatting_while_typing_keeps_every_keystroke_and_the_caret() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        let box_ = last_shape(&rig.shell.borrow().app);
        type_into_editor(&mut rig, "ab");
        click(&mut rig, Role::Button, "Bold");
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the Bold click"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Bold"),
            "{kind:?}: the controls stay enabled after the click"
        );
        assert!(
            undo_labels(&rig).is_empty(),
            "{kind:?}: the steps collect in the open group"
        );
        type_into_editor(&mut rig, "cd");
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "36 pt");
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the size choice"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Bold"),
            "{kind:?}: the controls stay enabled after the size"
        );
        assert!(
            undo_labels(&rig).is_empty(),
            "{kind:?}: the size step is in the open group too"
        );
        click(&mut rig, Role::Button, centre_name(kind));
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the alignment click"
        );
        type_into_editor(&mut rig, "ef");
        // The caret kept its place across the click: the "X" lands
        // where the caret sat (after the "e"), not at the end.
        press_key(&mut rig, egui::Key::ArrowLeft);
        click(&mut rig, Role::Button, "Italic");
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the Italic click"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Italic"),
            "{kind:?}: the controls stay enabled after the click"
        );
        type_into_editor(&mut rig, "X");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("abcdeXf"),
            "{kind:?}: every keystroke kept, at the caret"
        );
        assert_eq!(
            row_number(&rig, box_, "Paragraph", "HorzAlign"),
            Some(1.0),
            "{kind:?}: the alignment records"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step labelled as the insert"
        );
        // The picture paints the text at that format (review minor 6).
        let after = ink(&mut rig, 2.1, 5.3, 3.9, 5.75);
        assert!(after > 0, "{kind:?}: the text paints ({after})");
    }
}

/// The same on an existing shape's edit (task P7-41 item 2's third
/// bullet): one step per format click (as Visio), the editor still
/// open after every click, and the typed text lands as its own last
/// step.
#[test]
fn format_clicks_on_an_existing_shape_are_one_step_each() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        // A light fill first (the `text_controls` rule): the black
        // text hides on the fixture's dark fill, and the flow checks
        // the picture (review minor 6).
        lighten_fill(&mut rig, kind);
        let shape = rectangle(&rig.shell.borrow().app);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the double click edits");
        type_into_editor(&mut rig, "Hi");
        click(&mut rig, Role::Button, "Bold");
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the Bold click"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Set fill color #F5F5F5".to_owned(), "Bold".to_owned()],
            "{kind:?}: one step per format click (as Visio)"
        );
        open_size_list(&mut rig, kind);
        click(&mut rig, Role::Button, "36 pt");
        assert!(
            editing(&mut rig),
            "{kind:?}: still editing after the size choice"
        );
        assert_eq!(
            undo_labels(&rig),
            vec![
                "Set fill color #F5F5F5".to_owned(),
                "Bold".to_owned(),
                "Set font size".to_owned()
            ],
            "{kind:?}: the size is its own step"
        );
        // The Bold and 36 pt clicks record their cells (review minor
        // 6), and the controls stay enabled after the clicks.
        assert_eq!(
            row_number(&rig, shape, "Character", "Style"),
            Some(1.0),
            "{kind:?}: the Bold click records the Style bit"
        );
        assert_eq!(
            row_number(&rig, shape, "Character", "Size"),
            Some(0.5),
            "{kind:?}: the 36 pt choice records the Size cell"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Bold"),
            "{kind:?}: the controls stay enabled after the clicks"
        );
        type_into_editor(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("Hi!"),
            "{kind:?}: the typed text is kept"
        );
        assert_eq!(
            undo_labels(&rig),
            vec![
                "Set fill color #F5F5F5".to_owned(),
                "Bold".to_owned(),
                "Set font size".to_owned(),
                "Write shape text".to_owned()
            ],
            "{kind:?}: the text write is one more step"
        );
        // The picture paints the edited text (review minor 6).
        let after = ink(&mut rig, 2.1, 3.2, 3.9, 3.5);
        assert!(after > 0, "{kind:?}: the text paints ({after})");
    }
}

/// Cmd+B while typing folds into the group (task P7-41 item 2's
/// fourth bullet, the path the review probed by hand): the chord keeps
/// the editor open and the commit is one step.
#[test]
fn cmd_b_while_typing_folds_into_the_group() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        let box_ = last_shape(&rig.shell.borrow().app);
        type_into_editor(&mut rig, "Hi");
        chord(&mut rig, egui::Key::B);
        assert!(
            editing(&mut rig),
            "{kind:?}: the chord keeps the editor open"
        );
        type_into_editor(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi!"),
            "{kind:?}: the typed text is kept"
        );
        assert_eq!(
            row_number(&rig, box_, "Character", "Style"),
            Some(1.0),
            "{kind:?}: the chord folded its bold into the group"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step labelled as the insert"
        );
        // The picture paints the bold text (review minor 6).
        let after = ink(&mut rig, 2.1, 5.3, 3.9, 5.75);
        assert!(after > 0, "{kind:?}: the text paints ({after})");
    }
}

/// The commit is one undo step, labelled as the insert: one undo
/// removes the whole new box (task P7-37 item 1).
#[test]
fn a_new_text_box_commits_as_one_undo_step() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        type_into_editor(&mut rig, "Hi");
        // Escape ends editing and keeps the text (the commit path).
        press_key(&mut rig, egui::Key::Escape);
        assert!(!editing(&mut rig), "{kind:?}: Escape commits");
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi"),
            "{kind:?}: the text lands in the box"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one undo step, labelled as the insert"
        );
        // One undo removes the whole box.
        chord(&mut rig, egui::Key::Z);
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: one undo removes the box"
        );
    }
}

/// A commit with no text leaves no shape, no undo entry and the
/// document unchanged (and not marked as changed), task P7-37 item 1.
#[test]
fn a_new_box_committed_empty_leaves_no_shape_and_no_undo_entry() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        press_key(&mut rig, egui::Key::Escape);
        assert!(!editing(&mut rig), "{kind:?}: the empty edit ends");
        assert_eq!(shape_count(&rig), before, "{kind:?}: no shape stays");
        let shell = rig.shell.borrow();
        let session = shell.app.active_document().expect("session");
        assert!(session.undo.is_empty(), "{kind:?}: no undo entry");
        assert!(
            !session.is_dirty(),
            "{kind:?}: the document is not marked as changed"
        );
    }
}

/// The P7-25 click elsewhere commits the box being typed and the next
/// click opens another (task P7-37 item 5: the click closes the
/// group); the second box committed empty cancels.
#[test]
fn a_click_elsewhere_commits_the_box_and_the_next_click_opens_another() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, 1.0, 6.0);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the first box opens");
        let first = last_shape(&rig.shell.borrow().app);
        type_into_editor(&mut rig, "Hi");
        // The click elsewhere (P7-25) commits the first box.
        let pos = screen(&rig, 5.5, 6.0);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the second box is typed");
        assert_eq!(
            shape_count(&rig),
            before + 2,
            "{kind:?}: the first box committed, the second opened"
        );
        assert_eq!(
            shape_text(&rig, first).as_deref(),
            Some("Hi"),
            "{kind:?}: the typed text lands in the first box"
        );
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_count(&rig),
            before + 1,
            "{kind:?}: the empty second box cancels"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step for the first box"
        );
    }
}

/// A layout switch ends the typing interaction and closes the group
/// (task P7-37 item 5): the View menu's row commits first, and the
/// switch command dispatched over an open edit closes the group
/// before it runs.
#[test]
fn a_layout_switch_commits_the_new_text_box() {
    // The View menu's layout row (a real menu click, Panel Classic).
    let mut rig = shell_rig(LayoutKind::PanelClassic);
    choose_text_tool(&mut rig, LayoutKind::PanelClassic);
    let pos = screen(&rig, EMPTY.0, EMPTY.1);
    click_at(&mut rig, pos);
    type_into_editor(&mut rig, "Hi");
    click(&mut rig, Role::Button, "View");
    click(&mut rig, Role::MenuItem, "Panel Modern");
    assert_eq!(
        rig.shell.borrow().app.settings.ui_layout,
        LayoutKind::PanelModern,
        "the layout switched"
    );
    assert!(!editing(&mut rig), "the switch ends the edit");
    let box_ = last_shape(&rig.shell.borrow().app);
    assert_eq!(
        shape_text(&rig, box_).as_deref(),
        Some("Hi"),
        "the text lands"
    );
    assert_eq!(
        undo_labels(&rig),
        vec!["Insert Text Box".to_owned()],
        "one step labelled as the insert"
    );
    drop(rig);

    // The switch command over an open edit (the dispatch path, both
    // layouts): the group closes before it runs.
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        rig.dispatch("view.ui_layout.panel_2016");
        assert!(!editing(&mut rig), "{kind:?}: the switch ends the edit");
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(shape_text(&rig, box_).as_deref(), Some("Hi"), "{kind:?}");
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step labelled as the insert"
        );
    }
}

/// Closing the document ends the typing interaction and closes the
/// group (task P7-37 item 5): with typed text the close commits it
/// first, with no text the box cancels and the document is unchanged.
#[test]
fn closing_the_document_closes_the_new_box_first() {
    for kind in LAYOUTS {
        // With typed text: the close commits the box first.
        let mut rig = shell_rig(kind);
        // The close pauses on its confirmation, so the session stays
        // open for inspection (an earlier edit marked it changed).
        {
            let mut shell = rig.shell.borrow_mut();
            let session = shell.app.active_document_mut().expect("session");
            session.changes = session.changes.wrapping_add(1);
        }
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        close_document(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: the interaction ended");
        assert_eq!(shape_count(&rig), before + 1, "{kind:?}: the box committed");
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi"),
            "{kind:?}: the typed text lands"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step labelled as the insert"
        );
        assert!(
            has(&mut rig, Role::Button, "Don't Save"),
            "{kind:?}: the close waits for its confirmation"
        );
        drop(rig);

        // With no text: the box cancels and the document is unchanged.
        let mut rig = shell_rig(kind);
        {
            let mut shell = rig.shell.borrow_mut();
            let session = shell.app.active_document_mut().expect("session");
            session.changes = session.changes.wrapping_add(1);
        }
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        close_document(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: the interaction ended");
        assert_eq!(shape_count(&rig), before, "{kind:?}: no shape stays");
        assert!(undo_labels(&rig).is_empty(), "{kind:?}: no undo entry");
        assert!(
            has(&mut rig, Role::Button, "Don't Save"),
            "{kind:?}: the close waits for its confirmation"
        );
    }
}

/// Clicks the Undo button (Panel Modern names it for the last edit,
/// "Undo Insert Text Box"; the split's "Undo menu" is not the button,
/// and the disabled button reads "Can't Undo").
fn undo_button(rig: &mut ShellRig) {
    rig.run();
    let root = rig.harness().root();
    let node = root
        .children_recursive()
        .find(|node| {
            let data = node.accesskit_node().data();
            data.role() == Role::Button
                && data.label().is_some_and(|label| {
                    // The disabled button reads "Can't Undo".
                    label == "Undo"
                        || label.starts_with("Can't Undo")
                        || (label.starts_with("Undo ") && label != "Undo menu")
                })
        })
        .unwrap_or_else(|| {
            let labels: Vec<String> = root
                .children_recursive()
                .filter(|n| n.accesskit_node().data().role() == Role::Button)
                .filter_map(|n| n.accesskit_node().data().label().map(|s| s.to_owned()))
                .collect();
            panic!("no Undo button; buttons: {labels:?}");
        });
    node.click();
    rig.run();
}

/// Undo while a box is open (task P7-41 item 5): the button is
/// enabled while the group collects its steps, and one click removes
/// the whole new box — the click commits the typing (the group folds
/// into one step) and undoes that step.
#[test]
fn the_undo_button_removes_the_whole_new_box() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        undo_button(&mut rig);
        assert!(!editing(&mut rig), "{kind:?}: the click ends the edit");
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: one click removes the whole box"
        );
        let shell = rig.shell.borrow();
        let session = shell.app.active_document().expect("session");
        assert!(
            session.undo.undo_labels().is_empty(),
            "{kind:?}: the committed step is undone again"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 3 (task P7-41): a save while the editor has focus commits the
// box first, so a save never writes an empty box. Cmd+S and Cmd+P are
// global bindings and dispatch while the editor holds the keyboard;
// Save As and export reach their `file.*` commands from the File menu
// or the File view, where the press rule ends the edit first.
// ---------------------------------------------------------------------------

/// A picker that answers every save with one temp path and counts the
/// saves (the `unsaved` module's TempPicker pattern).
#[derive(Clone)]
struct SavePicker {
    /// The path every save picks.
    path: std::path::PathBuf,
    /// How many saves were asked for.
    saves: std::rc::Rc<std::cell::RefCell<usize>>,
}

impl SavePicker {
    fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            saves: std::rc::Rc::new(std::cell::RefCell::new(0)),
        }
    }
}

impl larrygram_ui::dialogs::FilePicker for SavePicker {
    fn open(
        &self,
        _request: &larrygram_ui::dialogs::OpenRequest,
    ) -> Result<Option<larrygram_ui::dialogs::FileChoice>, larrygram_ui::UiError> {
        Ok(None)
    }

    fn save(
        &self,
        _request: &larrygram_ui::dialogs::SaveRequest,
        _data: &[u8],
    ) -> Result<Option<larrygram_ui::dialogs::FileChoice>, larrygram_ui::UiError> {
        *self.saves.borrow_mut() += 1;
        Ok(Some(larrygram_ui::dialogs::FileChoice::Path(
            self.path.clone(),
        )))
    }

    fn poll(&self) -> larrygram_ui::dialogs::PickState {
        larrygram_ui::dialogs::PickState::Idle
    }
}

/// A private temp file path for one test's save.
fn temp_target(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "larrygram-text-controls-{}-{name}.vsdx",
        std::process::id()
    ))
}

/// The shape count of the saved file's active page (what the save
/// wrote, not what the document holds after the fact).
fn saved_shapes(target: &std::path::Path) -> usize {
    let bytes = std::fs::read(target).expect("the save wrote the file");
    let doc = larrygram_vsdx::read_document(&bytes, &larrygram_vsdx::Limits::default())
        .expect("the saved file parses");
    let page = doc.pages.first().expect("a page");
    page.shapes.len()
}

/// The Save As command through the layout's own navigation: Panel
/// Classic's File menu row, Panel Modern's File view page and its
/// Browse button.
fn save_as_command(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => {
            click(rig, Role::Button, "File");
            click(rig, Role::MenuItem, "Save As...");
        }
        _ => {
            rig.click_named("File");
            click(rig, Role::Button, "Save As");
            click(rig, Role::Button, "Browse\u{2026}");
        }
    }
}

/// Cmd+S dispatches while the editor has focus: the global binding
/// runs `file.save`, and the commit rule writes the typed text and
/// closes the group first, so the file carries the box with its text.
#[test]
fn a_save_while_typing_commits_the_box_and_writes_it() {
    for kind in LAYOUTS {
        let target = temp_target("typed");
        let picker = SavePicker::new(target.clone());
        let saves = picker.saves.clone();
        let mut rig = shell_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        chord(&mut rig, egui::Key::S);
        assert!(!editing(&mut rig), "{kind:?}: the save ends the edit");
        assert_eq!(
            *saves.borrow(),
            1,
            "{kind:?}: Cmd+S dispatched while the editor had focus"
        );
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi"),
            "{kind:?}: the typed text committed before the write"
        );
        assert_eq!(shape_count(&rig), before + 1, "{kind:?}: the box is there");
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: one step labelled as the insert"
        );
        assert_eq!(
            saved_shapes(&target),
            before + 1,
            "{kind:?}: the file carries the box"
        );
        {
            let shell = rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            assert!(!session.is_dirty(), "{kind:?}: the save marked it saved");
        }
        let _ = std::fs::remove_file(&target);
    }
}

/// A save with an empty new box open writes no empty box (item 3):
/// the commit rule cancels the box before `file.save` runs, and the
/// file holds only the fixture shapes.
#[test]
fn a_save_of_an_empty_new_box_writes_no_box() {
    for kind in LAYOUTS {
        let target = temp_target("empty");
        let picker = SavePicker::new(target.clone());
        let saves = picker.saves.clone();
        let mut rig = shell_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        chord(&mut rig, egui::Key::S);
        assert!(!editing(&mut rig), "{kind:?}: the save ends the edit");
        assert_eq!(*saves.borrow(), 1, "{kind:?}: the save ran");
        assert_eq!(shape_count(&rig), before, "{kind:?}: the empty box cancels");
        assert!(undo_labels(&rig).is_empty(), "{kind:?}: no undo entry");
        assert_eq!(
            saved_shapes(&target),
            before,
            "{kind:?}: the file holds no empty box"
        );
        {
            let shell = rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            assert!(
                !session.is_dirty(),
                "{kind:?}: the cancelled box left it unchanged"
            );
        }
        let _ = std::fs::remove_file(&target);
    }
}

/// The other file commands over an open edit commit it first too (item
/// 3's check, item 4's pin): Cmd+P is `file.print`'s global binding —
/// a stub today — and it still commits (or cancels) the box before it
/// runs; Save As sees the committed box as well.
#[test]
fn a_file_command_over_an_open_edit_commits_it_first() {
    for kind in LAYOUTS {
        // Cmd+P over an empty box: the box cancels before the stub
        // runs.
        let mut rig = shell_rig(kind);
        let before = shape_count(&rig);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        chord(&mut rig, egui::Key::P);
        assert!(
            !editing(&mut rig),
            "{kind:?}: the print command ends the edit"
        );
        assert_eq!(
            shape_count(&rig),
            before,
            "{kind:?}: the empty box cancels before the file command"
        );
        drop(rig);

        // Save As over typed text: the edit ends first and the file
        // command saves the committed box.
        let target = temp_target("save_as");
        let picker = SavePicker::new(target.clone());
        let saves = picker.saves.clone();
        let mut rig = shell_rig(kind);
        rig.shell.borrow_mut().set_picker(Box::new(picker));
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        type_into_editor(&mut rig, "Hi");
        save_as_command(&mut rig, kind);
        assert!(!editing(&mut rig), "{kind:?}: the edit ended");
        assert_eq!(
            *saves.borrow(),
            1,
            "{kind:?}: Save As saved the committed box"
        );
        assert_eq!(
            saved_shapes(&target),
            before + 1,
            "{kind:?}: the file carries the committed box"
        );
        let _ = std::fs::remove_file(&target);
    }
}

// ---------------------------------------------------------------------------
// Item 1 (task P7-48, the review's M1): the block is the box. The
// commit test, the fill and the wrap width use the text block, joined
// with the field's own rect.
// ---------------------------------------------------------------------------

/// The fixture rectangle's centre, in page inches, edited with its own
/// text already drawn (the M1 flows need the drawn text to shrink the
/// old ink box).
fn edit_with_text(rig: &mut ShellRig, text: &str) {
    select_shape(rig);
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    double_click_at(rig, pos);
    assert!(editing(rig), "the double click opens the editor");
    type_into_editor(rig, text);
    press_key(rig, egui::Key::Escape);
    double_click_at(rig, pos);
    assert!(editing(rig), "the editor re-opens over the drawn text");
}

/// A click just right of the last letter moves the caret and keeps the
/// edit (task P7-48 item 1): the click lands inside the text block,
/// outside the tight text.
#[test]
fn a_click_right_of_the_last_letter_keeps_the_edit() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_with_text(&mut rig, "MMMM");
        // Right of "MMMM" (which ends near x 2.55), on the first
        // line, inside the text block (page y is up: the block's top
        // edge is y 3.5 and the line sits just below it).
        let pos = screen(&rig, 3.2, 3.35);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click right of the text keeps the edit"
        );
        type_into_editor(&mut rig, "X");
        press_key(&mut rig, egui::Key::Escape);
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("MMMMX"),
            "{kind:?}: the caret moved to the click and typing lands"
        );
    }
}

/// A click on a newly typed second line keeps the edit, and so does a
/// click on a line the field grew to below the text block (task P7-48
/// item 1: the field's own response rect joins the commit test).
#[test]
fn a_click_on_a_typed_line_keeps_the_edit() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_with_text(&mut rig, "MM");
        // Enough text to wrap at the block width (about 14 M's a line
        // at the drawn size): a second line appears below the first.
        type_into_editor(&mut rig, &"M".repeat(20));
        // The second line, inside the block but below the drawn text.
        let pos = screen(&rig, 2.3, 3.15);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click on the second line keeps the edit"
        );
        // Enough text to grow the field past the block's height, then
        // a click on a line below the block (the field's own rect
        // joins the commit test).
        type_into_editor(&mut rig, &"M".repeat(100));
        let pos = screen(&rig, 2.3, 2.2);
        click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the click inside the grown field keeps the edit"
        );
        press_key(&mut rig, egui::Key::Escape);
    }
}

/// The field wraps where the drawn text wraps (task P7-48 item 1): a
/// long append wraps at the block width, not at 80 px.
#[test]
fn the_field_wraps_at_the_block_width() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        edit_with_text(&mut rig, "MM");
        // Seven more M's: about 100 px of text — one line at the 192 px
        // block width, two lines at the old 80 px wrap.
        type_into_editor(&mut rig, &"M".repeat(7));
        let band2 = ink(&mut rig, 2.05, 3.05, 3.95, 3.25);
        assert_eq!(
            band2, 0,
            "{kind:?}: the append wraps at the block width, not at 80 px"
        );
        // The control: enough text wraps, and the second band shows it
        // (the band sits where a wrapped line falls).
        type_into_editor(&mut rig, &"M".repeat(30));
        let band2 = ink(&mut rig, 2.05, 3.05, 3.95, 3.25);
        assert!(
            band2 > 0,
            "{kind:?}: the long text wraps into the second band ({band2})"
        );
        press_key(&mut rig, egui::Key::Escape);
    }
}

// ---------------------------------------------------------------------------
// Item 2 (task P7-48, the review's M2): one text at a time. The canvas
// leaves out the text of the shape being edited; only the editor
// shows it. Its geometry, fill and line still draw.
// ---------------------------------------------------------------------------

/// The count of non-background pixels of the canvas raster (the
/// canvas layer, under the editor overlay) in a page rectangle.
fn raster_ink(rig: &ShellRig, x0: f64, y0: f64, x1: f64, y1: f64) -> usize {
    let shell = rig.shell.borrow();
    let cache = shell.canvas_cache();
    let raster = cache.raster().expect("the canvas raster");
    let (page_w, page_h) = cache.page_size();
    let scale = f64::from(raster.width) / page_w;
    let px = |x: f64| ((x.clamp(0.0, page_w) * scale) as u32).min(raster.width.saturating_sub(1));
    let py = |y: f64| {
        (((page_h - y.clamp(0.0, page_h)) * scale) as u32).min(raster.height.saturating_sub(1))
    };
    let (xmin, xmax) = (px(x0), px(x1));
    let (ymin, ymax) = (py(y1), py(y0));
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for x in xmin..xmax {
        for y in ymin..ymax {
            let i = ((y * raster.width + x) * 4) as usize;
            let px = &raster.rgba[i..i + 4];
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
            let i = ((y * raster.width + x) * 4) as usize;
            let px = &raster.rgba[i..i + 4];
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

/// The canvas does not draw the text of the shape being edited (task
/// P7-48 item 2): with the editor open the shape's text pixels are
/// absent from the canvas layer; after the edit they are back with the
/// new text.
#[test]
fn the_canvas_leaves_out_the_text_being_edited() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        select_shape(&mut rig);
        lighten_fill(&mut rig, kind);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        type_into_editor(&mut rig, "MMMM");
        press_key(&mut rig, egui::Key::Escape);
        rig.run();
        rig.run();
        let with_text = raster_ink(&rig, 2.05, 3.2, 3.95, 3.47);
        assert!(
            with_text > 0,
            "{kind:?}: the committed text draws ({with_text})"
        );

        // While the editor is open the canvas layer holds no text.
        double_click_at(&mut rig, pos);
        rig.run();
        rig.run();
        let hidden = raster_ink(&rig, 2.05, 3.2, 3.95, 3.47);
        assert_eq!(
            hidden, 0,
            "{kind:?}: the edited text is absent from the canvas"
        );

        // When the edit ends the text comes back, with the new text.
        type_into_editor(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape);
        rig.run();
        rig.run();
        let back = raster_ink(&rig, 2.05, 3.2, 3.95, 3.47);
        assert!(back > 0, "{kind:?}: the text is back ({back})");
        let shape = rectangle(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("MMMM!"),
            "{kind:?}: with the edited text"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 3 (task P7-48, the review's M3): Escape belongs to an open
// popup — it closes and the open edit stays.
// ---------------------------------------------------------------------------

/// Escape in the font size list closes the list and keeps the edit
/// with its text (task P7-48 item 3); Escape again ends the edit.
#[test]
fn escape_in_a_format_popup_closes_the_popup_not_the_edit() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, EMPTY.0, EMPTY.1);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the box opens");
        type_into_editor(&mut rig, "Hi");
        open_size_list(&mut rig, kind);
        assert!(
            has(&mut rig, Role::Button, "36 pt"),
            "{kind:?}: the size list opens"
        );
        press_key(&mut rig, egui::Key::Escape);
        assert!(
            !has(&mut rig, Role::Button, "36 pt"),
            "{kind:?}: the Escape closes the list"
        );
        assert!(
            editing(&mut rig),
            "{kind:?}: and keeps the edit with its text"
        );
        type_into_editor(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape);
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi!"),
            "{kind:?}: the typed text is kept"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 4 (task P7-48, the review's M4): the quit and close paths
// commit the open edit first (real input).
// ---------------------------------------------------------------------------

/// The viewport commands of the last frame (the `unsaved` module's
/// helper).
fn sent(rig: &mut ShellRig, match_: fn(&egui::ViewportCommand) -> bool) -> bool {
    rig.harness()
        .output()
        .viewport_output
        .values()
        .any(|output| output.commands.iter().any(match_))
}

/// The window close request (the window's close button; the `unsaved`
/// module's helper).
fn request_window_close(rig: &mut ShellRig) {
    {
        let harness = rig.harness();
        let viewports = harness
            .input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        viewports.events.push(egui::ViewportEvent::Close);
    }
    rig.harness().step();
}

/// Whether the quit completed within ten frames (the `unsaved`
/// module's helper).
fn quit_completed(rig: &mut ShellRig) -> bool {
    for _ in 0..10 {
        rig.harness().step();
        if rig.shell.borrow().quit_requested() {
            return true;
        }
        if sent(rig, |command| {
            matches!(command, egui::ViewportCommand::Close)
        }) {
            return true;
        }
    }
    false
}

/// Whether the three-way dialog is up and names `name` (the `unsaved`
/// module's helper).
fn unsaved_dialog_names(rig: &mut ShellRig, name: &str) -> bool {
    rig.run();
    rig.names()
        .contains_key(&format!("Unsaved changes in {name}."))
}

/// Presses the unsaved-changes dialog's button named `name` (scoped
/// to its window) with no frame after it: the caller watches the
/// frames that answer it (the `unsaved` module's `dialog_press`).
fn unsaved_dialog_press(rig: &mut ShellRig, name: &str) {
    let rect = rig
        .harness()
        .ctx
        .memory(|memory| {
            memory.area_rect(egui::Id::unique(larrygram_ui::window_size::UNSAVED_WINDOW))
        })
        .expect("the dialog window");
    let root = rig.harness().root();
    let node = root
        .query_all_by_role_and_label(Role::Button, name)
        .find(|node| rect.contains_rect(node.rect()))
        .unwrap_or_else(|| panic!("no dialog button named {name:?}"));
    node.click();
}

/// Clicks the unsaved-changes dialog's button named `name` and runs
/// the frames of the click (`unsaved` module's `dialog_click`).
fn unsaved_dialog_click(rig: &mut ShellRig, name: &str) {
    unsaved_dialog_press(rig, name);
    rig.run();
}

/// Whether the window close finished: the Close command of the last
/// frame, or within the next ten (the walk may answer over frames).
fn close_finished(rig: &mut ShellRig) -> bool {
    sent(rig, |command| {
        matches!(command, egui::ViewportCommand::Close)
    }) || quit_completed(rig)
}

/// Opens a new text box with `text` typed in it (nothing committed).
fn type_a_new_box(rig: &mut ShellRig, kind: LayoutKind, text: &str) {
    choose_text_tool(rig, kind);
    let pos = screen(rig, EMPTY.0, EMPTY.1);
    click_at(rig, pos);
    assert!(editing(rig), "the box opens");
    type_into_editor(rig, text);
}

/// The window close request with an empty new box on a clean drawing:
/// no dialog (the commit rule cancels the box) and the window closes
/// (task P7-48 item 4, the P7-42 review minor 6 case).
#[test]
fn a_close_request_with_an_empty_new_box_closes_quietly() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "");
        request_window_close(&mut rig);
        // The close finishes in the request's frame: read it before
        // the dialog check runs frames of its own.
        let closed = close_finished(&mut rig);
        assert!(
            !unsaved_dialog_names(&mut rig, "Untitled"),
            "{kind:?}: an empty box is no change to ask about"
        );
        assert!(closed, "{kind:?}: the window closes");
    }
}

/// The window close request with typed text: the dialog appears (the
/// commit rule wrote the text first) and Don't Save closes the
/// drawing without saving it (task P7-48 item 4).
#[test]
fn a_close_request_with_typed_text_asks_and_dont_save_closes() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "Hi");
        request_window_close(&mut rig);
        assert!(
            unsaved_dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the typed text is a change to ask about"
        );
        // The answer is watched frame by frame (`run` settles over
        // frames and the close would be gone from the output).
        unsaved_dialog_press(&mut rig, "Don't Save");
        assert!(close_finished(&mut rig), "{kind:?}: Don't Save closes");
        assert!(
            rig.shell.borrow().app.documents.is_empty(),
            "{kind:?}: the drawing closed with its unsaved text"
        );
    }
}

/// Cmd+Q (Ctrl+Q off macOS) with typed text: the dialog appears (task
/// P7-48 item 4).
#[test]
fn cmd_q_with_typed_text_asks() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "Hi");
        chord(&mut rig, egui::Key::Q);
        assert!(
            unsaved_dialog_names(&mut rig, "Untitled"),
            "{kind:?}: the dialog asks about the typed text"
        );
        unsaved_dialog_click(&mut rig, "Cancel");
    }
}

/// `file.exit` from the File menu with typed text: the dialog appears
/// (task P7-48 item 4). Panel Modern's File view has no Exit row
/// (P7-42): its quit path is Cmd+Q above.
#[test]
fn file_exit_from_the_menu_with_typed_text_asks() {
    let mut rig = shell_rig(LayoutKind::PanelClassic);
    type_a_new_box(&mut rig, LayoutKind::PanelClassic, "Hi");
    click(&mut rig, Role::Button, "File");
    click(&mut rig, Role::MenuItem, "Exit");
    assert!(
        unsaved_dialog_names(&mut rig, "Untitled"),
        "the dialog asks about the typed text"
    );
    unsaved_dialog_click(&mut rig, "Cancel");
}

// ---------------------------------------------------------------------------
// Item 5 (task P7-48, the review's minor 1): Undo and Redo with an
// open undo group.
// ---------------------------------------------------------------------------

/// The Undo label names the open group's step and one click undoes
/// that step only (a typed box is committed and undone whole); Redo
/// is disabled while the group is open and works afterwards (task
/// P7-48 item 5, the labels and enabled state through accesskit).
#[test]
fn undo_names_the_open_group_and_redo_comes_back_after() {
    {
        // The dynamic label seam is Panel Modern's (task P7-48 item 5).
        let kind = LayoutKind::PanelModern;
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "Hi");
        // The group's step is what Undo names, and it is enabled.
        assert!(
            has(&mut rig, Role::Button, "Undo Insert Text Box"),
            "{kind:?}: the Undo label names the group's step"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Undo Insert Text Box"),
            "{kind:?}: Undo is enabled with the group open"
        );
        // Redo is disabled while the group is open.
        assert!(
            has(&mut rig, Role::Button, "Can't Redo"),
            "{kind:?}: Redo reads as disabled"
        );
        assert!(
            disabled(&mut rig, Role::Button, "Can't Redo"),
            "{kind:?}: Redo is disabled with the group open"
        );

        // One click undoes that step only: the typed box is committed
        // and undone whole.
        let before = shape_count(&rig);
        click(&mut rig, Role::Button, "Undo Insert Text Box");
        assert!(!editing(&mut rig), "{kind:?}: the edit ended");
        assert_eq!(shape_count(&rig), before - 1, "{kind:?}: the box is gone");
        assert!(undo_labels(&rig).is_empty(), "{kind:?}: the step is undone");

        // Afterwards Redo comes back and restores the box with its text.
        assert!(
            has(&mut rig, Role::Button, "Repeat Insert Text Box"),
            "{kind:?}: Redo names the undone step"
        );
        assert!(
            !disabled(&mut rig, Role::Button, "Repeat Insert Text Box"),
            "{kind:?}: Redo is enabled afterwards"
        );
        click(&mut rig, Role::Button, "Repeat Insert Text Box");
        assert_eq!(shape_count(&rig), before, "{kind:?}: the box is back");
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi"),
            "{kind:?}: with its text"
        );
    }
}

/// Undo with an untyped box: the box is cancelled and nothing else is
/// undone, with no error (task P7-48 item 5).
#[test]
fn undo_on_an_untyped_box_cancels_it_and_undoes_nothing_else() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        // One earlier step to survive the Undo: a committed first box.
        type_a_new_box(&mut rig, kind, "First");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: the first box is one step"
        );
        let with_first = shape_count(&rig);

        // A second box at another empty spot, nothing typed: Undo
        // cancels it and keeps the first box and its step.
        choose_text_tool(&mut rig, kind);
        let pos = screen(&rig, 6.5, 5.5);
        click_at(&mut rig, pos);
        assert!(editing(&mut rig), "{kind:?}: the second box opens");
        undo_button(&mut rig);
        assert!(!editing(&mut rig), "{kind:?}: the edit ended");
        assert_eq!(
            shape_count(&rig),
            with_first,
            "{kind:?}: the empty box is cancelled"
        );
        assert_eq!(
            undo_labels(&rig),
            vec!["Insert Text Box".to_owned()],
            "{kind:?}: the first box's step is not undone"
        );
        assert!(
            rig.shell.borrow().app.warnings.is_empty(),
            "{kind:?}: no error was noted"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 6 (task P7-48, the review's minor 7): an empty new box is not
// a change. (The macOS hook reads the same rule: NSTerminateNow.)
// ---------------------------------------------------------------------------

/// The title carries no changed marker while the pending box is
/// empty, and one once it carries typed text (task P7-48 item 6); a
/// drawing changed before the box stays marked.
#[test]
fn an_empty_new_box_marks_nothing_in_the_title() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: the fixture starts clean"
        );
        type_a_new_box(&mut rig, kind, "");
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: an empty box marks nothing"
        );
        type_into_editor(&mut rig, "x");
        assert_eq!(
            rig.shell.borrow().window_title(),
            "\u{2022} Untitled - Larrygram",
            "{kind:?}: typed text is a change"
        );
        press_key(&mut rig, egui::Key::Backspace);
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: the emptied box marks nothing again"
        );
        press_key(&mut rig, egui::Key::Escape);
        drop(rig);

        // A drawing changed before the box stays marked while the box
        // is typed.
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "First");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            rig.shell.borrow().window_title(),
            "\u{2022} Untitled - Larrygram",
            "{kind:?}: the committed box marks the drawing"
        );
        type_a_new_box(&mut rig, kind, "");
        assert_eq!(
            rig.shell.borrow().window_title(),
            "\u{2022} Untitled - Larrygram",
            "{kind:?}: the empty box keeps the earlier change marked"
        );
        press_key(&mut rig, egui::Key::Escape);
    }
}

// ---------------------------------------------------------------------------
// Item 2 (task P7-51): the empty-box flag is fresh after the editor
// has taken the frame's keystrokes.
// ---------------------------------------------------------------------------

/// The first typed character marks the drawing in its own frame (task
/// P7-51 item 2): the title marker and the published changed flag
/// (`is_dirty`, what `publish_changed` publishes) read fresh after one
/// frame, not one frame late.
#[test]
fn the_first_typed_character_marks_the_drawing_in_its_own_frame() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "");
        assert_eq!(
            rig.shell.borrow().window_title(),
            "Untitled - Larrygram",
            "{kind:?}: an empty box marks nothing"
        );
        // The first character, in exactly one frame.
        {
            rig.run();
            let root = rig.harness().root();
            let node = root
                .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
                .next()
                .expect("the editor field");
            node.type_text("x");
        }
        rig.harness().run_steps(1);
        let shell = rig.shell.borrow();
        let session = shell.app.active_document().expect("session");
        assert!(
            session.is_dirty(),
            "{kind:?}: the published changed flag is fresh"
        );
        assert_eq!(
            shell.window_title(),
            "\u{2022} Untitled - Larrygram",
            "{kind:?}: the title marker is fresh"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 7 (task P7-48, the review's minor 2): a turned or flipped
// shape drops the font metrics — and keeps working.
// ---------------------------------------------------------------------------

/// The editor works over a shape turned 30 degrees (task P7-48 item
/// 7): the metrics drop (its first baseline is no screen axis' y) but
/// the edit opens, takes typing and commits.
#[test]
fn editing_a_turned_shape_works() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let shape = {
            let shell = rig.shell.borrow();
            rectangle(&shell.app)
        };
        {
            let mut shell = rig.shell.borrow_mut();
            let session = shell.app.active_document_mut().expect("session");
            let cell = session
                .document
                .shape_mut(shape)
                .expect("shape")
                .sheet
                .cells
                .iter_mut()
                .find(|cell| cell.name.as_str() == "Angle")
                .expect("the Angle cell");
            cell.value = Some(CellValue::Number(30.0f64.to_radians()));
        }
        rig.run();
        rig.run();
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the edit opens on the turned shape"
        );
        type_into_editor(&mut rig, "Turned");
        press_key(&mut rig, egui::Key::Escape);
        assert_eq!(
            shape_text(&rig, shape).as_deref(),
            Some("Turned"),
            "{kind:?}: the text commits on the turned shape"
        );
    }
}

// ---------------------------------------------------------------------------
// Item 8 (task P7-48, the review's minor 3): a popup's own field keeps
// its typing.
// ---------------------------------------------------------------------------

/// A press in a popup hands the keyboard back to the editor only when
/// the popup has no focusable field of its own (task P7-48 item 8):
/// the digits land in the Size picker's field, not in the shape text.
#[test]
fn a_popup_field_keeps_its_typing_while_editing() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        type_a_new_box(&mut rig, kind, "Hi");
        open_size_list(&mut rig, kind);
        {
            rig.run();
            let root = rig.harness().root();
            let node = root
                .query_all_by_role_and_label(Role::TextInput, "Size")
                .next()
                .expect("the Size picker's field");
            node.click();
            node.type_text("36");
        }
        rig.run();
        {
            let root = rig.harness().root();
            let value = root
                .query_all_by_role_and_label(Role::TextInput, "Size")
                .next()
                .and_then(|node| node.value());
            assert_eq!(
                value.as_deref(),
                Some("36"),
                "{kind:?}: the digits land in the picker's field"
            );
        }
        // The shape text kept its own: the popup's typing never
        // reached it.
        press_key(&mut rig, egui::Key::Escape);
        press_key(&mut rig, egui::Key::Escape);
        let box_ = last_shape(&rig.shell.borrow().app);
        assert_eq!(
            shape_text(&rig, box_).as_deref(),
            Some("Hi"),
            "{kind:?}: not in the shape text"
        );
    }
}
