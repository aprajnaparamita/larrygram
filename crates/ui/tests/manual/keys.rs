//! Item 1: the Delete key deletes the selection (the user's manual
//! pass: "the Delete key does nothing on any shape, a text object
//! included"). The tests press the real key events in both layouts.

use crate::{LAYOUTS, ShellRig, rig_with_rect};

/// Selects the rectangle with a real click.
fn select(rig: &mut ShellRig) {
    rig.click_page(3.0, 3.0);
    assert_eq!(rig.selection().len(), 1, "the click selects the shape");
}

#[test]
fn the_delete_key_deletes_the_selection() {
    for kind in LAYOUTS {
        let (mut rig, _shape) = rig_with_rect(kind);
        select(&mut rig);
        let undo = rig.undo_len();
        rig.key(egui::Key::Delete);
        assert!(
            rig.page_shapes().is_empty(),
            "{kind:?}: the Delete key deletes the selection"
        );
        assert_eq!(rig.undo_len(), undo + 1, "{kind:?}: one undo step");
    }
}

#[test]
fn the_macos_delete_key_backspace_deletes_the_selection() {
    for kind in LAYOUTS {
        let (mut rig, _shape) = rig_with_rect(kind);
        select(&mut rig);
        rig.key(egui::Key::Backspace);
        assert!(
            rig.page_shapes().is_empty(),
            "{kind:?}: the Backspace key deletes the selection"
        );
    }
}
