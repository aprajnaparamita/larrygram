//! Text in our shapes is centred (task P7-44): a Process shape dropped
//! from the Shapes window onto a new drawing, with a typed label,
//! draws the label in the middle of its box, both ways, and the Shape
//! Data pane shows the row's whole value — in Panel Modern and Panel
//! Classic, through real input on the full shell.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_geom::Point;
use larrygram_model::{ShapeId, TextContent};
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, settings_for};

/// The two layouts the task names.
const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelModern, LayoutKind::PanelClassic];

/// The page point the master is dropped on.
const DROP: (f64, f64) = (5.0, 5.0);

/// The label typed into the dropped shape.
const LABEL: &str = "Process";

/// A shell over a new blank drawing, with the page point [`DROP`] at
/// the centre of the canvas (the text fixtures' rule).
fn rig(kind: LayoutKind) -> ShellRig {
    let mut rig = ShellRig::new(settings_for(kind));
    rig.dispatch("file.new");
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
            f64::from(viewport.width()) / 2.0 / k - DROP.0,
            f64::from(viewport.height()) / 2.0 / k - (11.0 - DROP.1),
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

/// The rect of the Shapes-window tile named `name`.
fn tile(rig: &mut ShellRig, name: &str) -> egui::Rect {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role_and_label(Role::Button, name)
        .map(|node| node.rect())
        .find(|rect| rect.center().x < 400.0 && rect.height() > 30.0)
        .unwrap_or_else(|| panic!("no Shapes-window tile named {name:?}"))
}

/// A press, four moves and a release from `from` to `to` (the text
/// fixtures' drag).
fn drag(rig: &mut ShellRig, from: egui::Pos2, to: egui::Pos2, modifiers: egui::Modifiers) {
    rig.harness().event(egui::Event::PointerMoved(from));
    rig.run();
    rig.harness().event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
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
        modifiers,
    });
    rig.run();
}

/// A press and a release in one frame at `pos`.
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

/// Holds Alt (snapping off) or releases it.
fn set_alt(rig: &mut ShellRig, alt: bool) {
    let modifiers = if alt {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::NONE
    };
    rig.harness()
        .event(egui::Event::ModifiersChanged(modifiers));
    rig.run();
}

/// One key press (the text fixtures' `press_key`).
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

/// The number value of one cell of `shape`.
fn number(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
    use larrygram_model::{CellName, CellReader, SheetRef};
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let mut reader = CellReader::new(&session.document);
    reader
        .number(SheetRef::Shape(shape), &CellName::new(name))
        .unwrap_or(f64::NAN)
}

/// The newest shape on the page.
fn last_shape(rig: &ShellRig) -> ShapeId {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    *session
        .document
        .page(page)
        .expect("page")
        .shapes
        .last()
        .expect("a shape")
}

/// The text of `shape` (`None` when it has none).
fn shape_text(rig: &ShellRig, shape: ShapeId) -> Option<String> {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    session
        .document
        .shape(shape)
        .and_then(|entity| entity.text.as_ref())
        .map(|text: &TextContent| {
            text.parts
                .iter()
                .map(|part| match part {
                    larrygram_model::TextPart::Text(literal) => literal.clone(),
                    _ => String::new(),
                })
                .collect()
        })
}

/// The bounding box of the label's ink inside `rect` (dark pixels on
/// the master's white fill, the border excluded).
fn ink_bbox(image: &image::RgbaImage, rect: egui::Rect) -> Option<egui::Rect> {
    let inner = rect.shrink(6.0);
    let (x0, y0) = (inner.left().max(0.0) as u32, inner.top().max(0.0) as u32);
    let (x1, y1) = (
        inner.right().min(image.width() as f32 - 1.0) as u32,
        inner.bottom().min(image.height() as f32 - 1.0) as u32,
    );
    let mut box_x = (f32::MAX, f32::MIN);
    let mut box_y = (f32::MAX, f32::MIN);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = image.get_pixel(x, y).0;
            if p[0] < 128 && p[1] < 128 && p[2] < 128 {
                box_x = (box_x.0.min(x as f32), box_x.1.max(x as f32 + 1.0));
                box_y = (box_y.0.min(y as f32), box_y.1.max(y as f32 + 1.0));
            }
        }
    }
    if box_x.0 > box_x.1 {
        return None;
    }
    Some(egui::Rect::from_min_max(
        egui::pos2(box_x.0, box_y.0),
        egui::pos2(box_x.1, box_y.1),
    ))
}

/// The values of every text field on screen (the pane fields).
fn field_values(rig: &mut ShellRig) -> Vec<String> {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_role(Role::TextInput)
        .map(|node| node.value().unwrap_or_default())
        .collect()
}

/// One layout's flow: drop the Process master on a new drawing, type
/// a label, commit it, and check the picture draws the label centred
/// and the Shape Data pane shows the whole value.
fn a_dropped_process_draws_its_label_centred(kind: LayoutKind) {
    let mut rig = rig(kind);
    // The Shapes window search brings the Process tile up (the same
    // list the real search box filters).
    rig.shell
        .borrow_mut()
        .shapes_window_mut()
        .set_search("process");
    let rect = tile(&mut rig, "Process");
    let preview = egui::pos2(rect.min.x + 12.0, rect.center().y);
    set_alt(&mut rig, true);
    let target = screen(&rig, DROP.0, DROP.1);
    drag(&mut rig, preview, target, egui::Modifiers::ALT);
    set_alt(&mut rig, false);
    let shape = last_shape(&rig);
    let (pin_x, pin_y) = (number(&rig, shape, "PinX"), number(&rig, shape, "PinY"));
    assert!(
        (pin_x - DROP.0).abs() < 0.05 && (pin_y - DROP.1).abs() < 0.05,
        "{kind:?}: the master drops where it is released, pin ({pin_x}, {pin_y})"
    );

    // Select it with a real canvas click and type the label.
    click_at(&mut rig, target);
    press_key(&mut rig, egui::Key::F2, egui::Modifiers::NONE);
    type_text(&mut rig, LABEL);
    press_key(&mut rig, egui::Key::Escape, egui::Modifiers::NONE);
    assert_eq!(
        shape_text(&rig, shape).as_deref(),
        Some(LABEL),
        "{kind:?}: the label commits"
    );

    // The picture: the label's ink sits in the middle of the shape's
    // box, both ways (a top-left label would sit in its corner).
    let image = rig.render();
    let left_bottom = screen(&rig, DROP.0 - 0.75, DROP.1 - 0.375);
    let right_top = screen(&rig, DROP.0 + 0.75, DROP.1 + 0.375);
    let rect = egui::Rect::from_two_pos(left_bottom, right_top);
    let ink = ink_bbox(&image, rect).expect("the label draws ink");
    let (dx, dy) = (
        ink.center().x - rect.center().x,
        ink.center().y - rect.center().y,
    );
    assert!(
        dx.abs() < rect.width() * 0.12 && dy.abs() < rect.height() * 0.2,
        "{kind:?}: the label is centred (ink {ink:?} in {rect:?}, offset {dx:.1},{dy:.1})"
    );

    // The Shape Data pane shows the master's whole value.
    let values = field_values(&mut rig);
    assert!(
        values.iter().any(|value| value == "Process"),
        "{kind:?}: the pane shows the whole value, got {values:?}"
    );
    assert!(
        values.iter().all(|value| !value.ends_with('\u{2026}')),
        "{kind:?}: no value is cut, got {values:?}"
    );
}

#[test]
fn a_dropped_process_draws_its_label_centred_in_panel_modern() {
    a_dropped_process_draws_its_label_centred(LayoutKind::PanelModern);
}

#[test]
fn a_dropped_process_draws_its_label_centred_in_panel_classic() {
    a_dropped_process_draws_its_label_centred(LayoutKind::PanelClassic);
}
