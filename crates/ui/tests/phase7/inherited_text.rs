//! The inherited-text work of task P7-40: an instance whose text
//! lives on its master draws that text, enables the text controls and
//! edits with it as the starting text, each driven through the real
//! input path in the full shell (`ShellRig`) in both layouts (the
//! master note) — canvas clicks, double clicks, typed keys, the undo
//! button, the model's cells and the rendered picture. A test that
//! dispatches the command does not count for this task.
//!
//! - item 4: the editor starts with the master's text; committing a
//!   change writes the instance's own text and never touches the
//!   master; committing unchanged text writes nothing and pushes no
//!   undo entry; one undo puts the instance back to inheriting.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::AppCtx;
use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Color, Document, Master, MasterId, Page, PageId, Preserved, Row,
    Section, SectionName, Shape, ShapeId, TextContent, TextPart,
};
use larrygram_ui_spec::LayoutKind;

use crate::ShellRig;

/// Both committed layouts every item must work in.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The fixture rectangle's centre, in page inches (2 x 1 at (3, 3)).
const SHAPE: (f64, f64) = (3.0, 3.0);

/// The master's text (the instance inherits it).
const MASTER_TEXT: &str = "Hello";

/// A numeric cell.
fn number_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A colour cell.
fn colour_cell(name: &str, value: Color) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: None,
        value: Some(CellValue::Color(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// One 2 x `height` in shape with its pin at (`x`, `y`): a light fill
/// (the black text must read on it) and no line.
fn base_shape(x: f64, y: f64, width: f64, height: f64, visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        number_cell("Width", width),
        number_cell("Height", height),
        number_cell("PinX", x),
        number_cell("PinY", y),
        number_cell("LocPinX", width / 2.0),
        number_cell("LocPinY", height / 2.0),
        number_cell("Angle", 0.0),
        number_cell("FillPattern", 1.0),
        colour_cell(
            "FillForegnd",
            Color {
                r: 245,
                g: 245,
                b: 245,
            },
        ),
        number_cell("LinePattern", 0.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", width, 0.0),
        ("LineTo", width, height),
        ("LineTo", 0.0, height),
        ("LineTo", 0.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![number_cell("X", px), number_cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

/// The document of the task: one instance of a master whose shape
/// carries the text `Hello`. The instance has no `Text` element of its
/// own (task P7-40's fixture: the text lives on the master).
fn inherited_document() -> (AppCtx, ShapeId, ShapeId) {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();

    // The master's shape: the same box, with the text.
    let mut master_shape = base_shape(3.0, 3.0, 2.0, 1.0, 101);
    master_shape.text = Some(TextContent {
        parts: vec![TextPart::Text(MASTER_TEXT.to_owned())],
    });
    let master_shape = document.add_shape(master_shape);
    let mut master = Master::new(MasterId { index: 0 }, 100);
    master.name_u = "Labelled box".to_owned();
    master.shapes = vec![master_shape];
    document.masters.push(master);

    // The instance on the page: no `Text` element of its own.
    let mut instance = base_shape(SHAPE.0, SHAPE.1, 2.0, 1.0, 1);
    instance.master = Some(MasterId { index: 0 });
    let instance = document.add_shape(instance);
    page.shapes.push(instance);
    document.pages.push(page);

    let mut app = AppCtx::new(crate::seen_settings());
    app.open_document(larrygram_commands::DocumentSession::new(document, None));
    app.selection.shapes.clear();
    app.selection.anchor = None;
    app.selection.page = Some(PageId { index: 0 });
    (app, instance, master_shape)
}

/// A shell in `kind` over [`inherited_document`], nothing selected,
/// the instance centred in the viewport.
fn shell_rig(kind: LayoutKind) -> ShellRig {
    let (mut app, _, _) = inherited_document();
    app.settings.ui_layout = kind;
    app.settings.default_layout = kind;
    let mut rig = ShellRig::with_app(app);
    // One double-click window for the editor tests (the P7-19 rule).
    rig.harness().ctx.options_mut(|options| {
        options.input_options.max_double_click_delay = 10.0;
    });
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
        session.view.pan = larrygram_geom::Point::new(
            f64::from(viewport.width()) / 2.0 / k - SHAPE.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - SHAPE.1),
        );
    }
    rig.run();
    rig.run();
    rig
}

/// The instance shape of `rig`'s document.
fn instance(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    let page = session.active_page.expect("page");
    session.document.page(page).expect("page").shapes[0]
}

/// The master's shape of `rig`'s document (not on any page).
fn master_shape(app: &AppCtx) -> ShapeId {
    let session = app.active_document().expect("session");
    session.document.masters[0].shapes[0]
}

/// The screen point of a page point in `rig`'s canvas.
fn screen(rig: &ShellRig, x: f64, y: f64) -> egui::Pos2 {
    rig.shell
        .borrow()
        .page_to_screen(larrygram_geom::Point::new(x, y))
        .expect("the canvas drew")
}

/// A press and a release in one frame at `pos` (the `interaction`
/// module's click rule).
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

/// A double click at `pos` (two clicks inside one double-click
/// window; the wait first drains any earlier window).
fn double_click_at(rig: &mut ShellRig, pos: egui::Pos2) {
    for _ in 0..60 {
        rig.run();
    }
    click_at(rig, pos);
    click_at(rig, pos);
}

/// Presses one key with `modifiers`.
fn press_key(rig: &mut ShellRig, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        rig.harness().event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        rig.run();
    }
}

/// Types `text` one character at a time (the real key events).
fn type_text(rig: &mut ShellRig, text: &str) {
    for character in text.chars() {
        rig.harness()
            .event(egui::Event::Text(character.to_string()));
        rig.run();
    }
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

/// Whether the in-place editor (the "Text" multiline input) is on
/// screen.
fn editing(rig: &mut ShellRig) -> bool {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
        .next()
        .is_some()
}

/// The editor field's text (`None` when the editor is closed or egui
/// publishes no value).
fn editor_value(rig: &mut ShellRig) -> Option<String> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::MultilineTextInput, "Text")
        .next()
        .and_then(|node| node.accesskit_node().data().value().map(str::to_owned))
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

/// Every text control of the enablement check (font, size, Bold,
/// Italic, Underline and the three alignments).
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

/// Selects the fixture shape with a real canvas click.
fn select_shape(rig: &mut ShellRig) {
    let pos = screen(rig, SHAPE.0, SHAPE.1);
    click_at(rig, pos);
    let shape = instance(&rig.shell.borrow().app);
    assert_eq!(
        rig.shell.borrow().app.selection.shapes,
        vec![shape],
        "the canvas click selects the instance"
    );
}

/// Clicks the Undo button (Panel Modern names it for the last edit;
/// the split's "Undo menu" is not the button).
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

/// The plain text of `shape`'s own `Text` element (empty when it has
/// none): the cells check of the master note.
fn own_text(rig: &ShellRig, shape: ShapeId) -> String {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    entry.text.as_ref().map(literal_of).unwrap_or_default()
}

/// The plain text `shape` shows (its own, else its master's).
fn effective_text(rig: &ShellRig, shape: ShapeId) -> String {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .document
        .effective_text(shape)
        .map(literal_of)
        .unwrap_or_default()
}

/// The concatenated literal parts of `text`.
fn literal_of(text: &TextContent) -> String {
    text.parts
        .iter()
        .filter_map(|part| match part {
            TextPart::Text(literal) => Some(literal.as_str()),
            _ => None,
        })
        .collect()
}

/// The count of non-background pixels in a page-rectangle region of
/// the rendered frame (the picture check; [`crate`]'s `text_controls`
/// rule): the background is the region's most common colour.
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
// Item 4: the instance shows, enables and edits its master's text.
// ---------------------------------------------------------------------------

/// The whole flow of the task's real-input check, in both layouts:
/// the master's text draws in the picture, selecting enables the text
/// controls, the editor starts with the master's text, committing a
/// change writes the instance's own text (the master unchanged) and
/// one undo puts the instance back to inheriting.
#[test]
fn an_instance_shows_and_edits_its_masters_text() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let instance = instance(&rig.shell.borrow().app);
        let master = master_shape(&rig.shell.borrow().app);
        assert_eq!(
            own_text(&rig, instance),
            "",
            "{kind:?}: the fixture instance has no text of its own"
        );

        // The picture: the master's text draws inside the shape.
        assert!(
            ink(&mut rig, 2.15, 2.55, 3.85, 3.45) > 0,
            "{kind:?}: the master's text draws on the instance"
        );

        // The controls follow the effective text.
        select_shape(&mut rig);
        for (role, name) in text_controls(kind) {
            assert!(
                !disabled(&mut rig, role, name),
                "{kind:?}: {name} enables on the inherited text"
            );
        }

        // The editor starts with the master's text.
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the double click opens the editor"
        );
        assert_eq!(
            editor_value(&mut rig).as_deref(),
            Some(MASTER_TEXT),
            "{kind:?}: the editor starts with the master's text"
        );

        // The commit writes the instance's own text; the master's
        // text never changes.
        type_text(&mut rig, "!");
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!editing(&mut rig), "{kind:?}: Escape ends editing");
        assert_eq!(
            own_text(&rig, instance),
            "Hello!",
            "{kind:?}: the commit writes the instance's own text"
        );
        assert_eq!(
            own_text(&rig, master),
            MASTER_TEXT,
            "{kind:?}: the master's text is unchanged"
        );

        // One undo puts it back to inheriting.
        undo(&mut rig);
        assert_eq!(
            own_text(&rig, instance),
            "",
            "{kind:?}: one undo drops the instance's own text"
        );
        assert_eq!(
            effective_text(&rig, instance),
            MASTER_TEXT,
            "{kind:?}: the instance shows the master's text again"
        );
        assert_eq!(
            own_text(&rig, master),
            MASTER_TEXT,
            "{kind:?}: the master's text is still unchanged"
        );
        assert!(
            ink(&mut rig, 2.15, 2.55, 3.85, 3.45) > 0,
            "{kind:?}: the master's text draws again"
        );
    }
}

/// Committing the unchanged buffer writes nothing and pushes no undo
/// entry: the instance stays textless and the undo stack stays empty.
#[test]
fn committing_unchanged_text_writes_nothing() {
    for kind in LAYOUTS {
        let mut rig = shell_rig(kind);
        let instance = instance(&rig.shell.borrow().app);
        let master = master_shape(&rig.shell.borrow().app);
        let pos = screen(&rig, SHAPE.0, SHAPE.1);
        double_click_at(&mut rig, pos);
        assert!(
            editing(&mut rig),
            "{kind:?}: the double click opens the editor"
        );
        press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!editing(&mut rig), "{kind:?}: Escape ends editing");
        assert_eq!(
            own_text(&rig, instance),
            "",
            "{kind:?}: the unchanged commit writes nothing"
        );
        assert_eq!(
            own_text(&rig, master),
            MASTER_TEXT,
            "{kind:?}: the master's text is unchanged"
        );
        {
            let shell = rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            assert!(
                session.undo.is_empty(),
                "{kind:?}: the unchanged commit pushes no undo entry"
            );
            assert!(!session.undo.can_undo(), "{kind:?}: nothing to undo");
        }
    }
}
