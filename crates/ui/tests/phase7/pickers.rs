//! The formatting pickers in their host popups (task P7-21): they
//! open from the split buttons, choose with the mouse and the keyboard,
//! and close on choice or Escape.
//!
//! The pickers' own row behaviour is unit-tested in
//! `crates/ui/src/pickers.rs`; this file drives them the way a user
//! does, through the Panel Classic toolbar splits (`ClassicRig`, whose
//! rich application state keeps the split commands enabled).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;

use super::menus::ClassicRig;

/// The names drawn in the current frame.
fn names(rig: &ClassicRig) -> Vec<String> {
    rig.named_nodes().into_iter().map(|n| n.name).collect()
}

#[test]
fn the_colour_picker_opens_and_a_mouse_choice_dispatches() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Text Color menu");
    assert!(
        names(&rig).iter().any(|n| n == "Use Default"),
        "the font colour picker opens"
    );
    assert!(
        names(&rig).iter().any(|n| n == "Accent #1F5BC4"),
        "the theme swatches draw"
    );
    rig.take_intents();
    rig.click(Role::Button, "Accent #1F5BC4");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "the swatch dispatches once");
    assert_eq!(intents[0].command.as_str(), "text.font.color");
    assert_eq!(
        intents[0].invocation.args.text("color"),
        Some("#1F5BC4"),
        "the swatch carries its value"
    );
    assert!(
        !names(&rig).iter().any(|n| n == "Accent #1F5BC4"),
        "the popup closes on choice"
    );
}

#[test]
fn the_line_picker_dispatches_a_weight() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Line Weight menu");
    assert!(names(&rig).iter().any(|n| n == "2.25 pt"), "the weights");
    rig.take_intents();
    rig.click(Role::Button, "2.25 pt");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "format.set.line.weight");
    assert_eq!(intents[0].invocation.args.number("weight"), Some(2.25));
}

#[test]
fn the_arrow_picker_dispatches_an_arrowhead() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Line Ends menu");
    assert!(names(&rig).iter().any(|n| n == "Stealth"), "the arrowheads");
    rig.take_intents();
    rig.click(Role::Button, "Stealth");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "format.set.line.ends");
}

#[test]
fn the_picker_closes_on_escape() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Text Color menu");
    assert!(names(&rig).iter().any(|n| n == "Use Default"), "it opens");
    rig.harness.key_press(egui::Key::Escape);
    rig.harness.run_steps(2);
    assert!(
        !names(&rig).iter().any(|n| n == "Use Default"),
        "Escape closes the picker"
    );
}

#[test]
fn the_picker_chooses_with_the_keyboard() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Line Weight menu");
    assert!(names(&rig).iter().any(|n| n == "No Line"), "it opens");
    rig.take_intents();
    // The arrow keys move the highlight and Enter chooses it.
    rig.harness.key_press(egui::Key::ArrowDown);
    rig.harness.run_steps(1);
    rig.harness.key_press(egui::Key::Enter);
    rig.harness.run_steps(1);
    let chosen = rig.take_intents();
    assert_eq!(chosen.len(), 1, "Enter chooses the highlighted row");
    assert_eq!(chosen[0].command.as_str(), "format.set.line.weight");
    assert_eq!(
        chosen[0].invocation.args.number("weight"),
        Some(0.25),
        "the arrow keys moved past 'No Line' to the first weight"
    );
}

#[test]
fn the_fill_picker_dispatches_a_transparency() {
    let mut rig = ClassicRig::new();
    rig.click(Role::Button, "Fill Color menu");
    assert!(names(&rig).iter().any(|n| n == "50%"), "the transparencies");
    rig.take_intents();
    rig.click(Role::Button, "50%");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "shape.transparency");
}
