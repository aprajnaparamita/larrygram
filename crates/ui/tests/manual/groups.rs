//! Items 2, 3 and 6: the group context menu and selection rules, and
//! Delete in the shape context menu (the user's manual pass).
//!
//! - Item 2: the shape context menu offers Group (two or more shapes
//!   selected) and Ungroup (a group), plus the arrange basics P7-21
//!   built (bring to front, send to back, rotate, flip).
//! - Item 3: one click on a member selects the whole group, a second
//!   click subselects the member; Cut, Copy, Delete and the context
//!   menu act on what is selected.
//! - Item 6: the shape context menu offers Delete, enabled with a
//!   selection.
//!
//! Every test drives real pointer input in both layouts.

use larrygram_model::{PageId, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{LAYOUTS, ShellRig, app_with, rectangle};

/// The two fixture rectangles and their group, nothing selected.
pub struct Grouped {
    /// The rig.
    pub rig: ShellRig,
    /// The group shape.
    pub group: ShapeId,
    /// The two members (page z order).
    pub members: [ShapeId; 2],
}

/// A page with rectangles at (3, 3) and (7, 3), grouped, nothing
/// selected.
pub fn grouped(kind: LayoutKind) -> Grouped {
    let (mut app, ids) = app_with(vec![rectangle(7.0, 3.0, 2)], kind);
    let group = {
        let session = app.active_document_mut().expect("session");
        session
            .document
            .group_shapes(PageId { index: 0 }, &ids)
            .expect("group")
    };
    Grouped {
        rig: ShellRig::ready(app),
        group,
        members: [ids[0], ids[1]],
    }
}

/// The submenu of the shape context menu that holds Group, Ungroup and
/// the arrange basics.
fn submenu(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelClassic => "Shape",
        LayoutKind::PanelModern => "Group",
        other => panic!("unexpected layout {other:?}"),
    }
}

/// The rotate row labels of `kind`'s shape context menu.
fn rotate_labels(kind: LayoutKind) -> (&'static str, &'static str) {
    match kind {
        LayoutKind::PanelClassic => ("Rotate Left", "Rotate Right"),
        LayoutKind::PanelModern => ("Rotate Left 90°", "Rotate Right 90°"),
        other => panic!("unexpected layout {other:?}"),
    }
}

/// Opens the shape context menu's arrange submenu.
fn open_arrange_submenu(rig: &mut ShellRig, kind: LayoutKind) {
    rig.click_menu_row(submenu(kind));
}

/// Clicks the Group row in the open arrange submenu. Panel Modern's
/// "Group" submenu opener shares the label, so its row is the second
/// match there.
fn click_group_row(rig: &mut ShellRig, kind: LayoutKind) {
    match kind {
        LayoutKind::PanelClassic => rig.click_menu_row("Group"),
        _ => rig.click_menu_nth("Group", 1),
    }
}

// --- item 3: the group selection rules --------------------------------

#[test]
fn a_first_click_selects_the_group_and_the_second_click_subselects_the_member() {
    for kind in LAYOUTS {
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.group],
            "{kind:?}: the first click selects the whole group"
        );
        state.rig.click_page(3.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.members[0]],
            "{kind:?}: the second click subselects the member"
        );
        state.rig.click_page(3.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.members[0]],
            "{kind:?}: a third click on the member keeps it"
        );
        state.rig.click_page(7.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.group],
            "{kind:?}: a click on the other member selects the group again"
        );
        state.rig.click_page(7.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.members[1]],
            "{kind:?}: and the next click subselects that member"
        );
    }
}

#[test]
fn cut_after_one_click_cuts_the_whole_group() {
    for kind in LAYOUTS {
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        state.rig.right_click_page(3.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.group],
            "{kind:?}: the right-click keeps the group selected"
        );
        state.rig.click_menu_row("Cut");
        assert!(
            state.rig.page_shapes().is_empty(),
            "{kind:?}: Cut cuts the whole group after one click"
        );
        assert_eq!(state.rig.undo_len(), 1, "{kind:?}: one undo step");
    }
}

#[test]
fn cut_after_the_second_click_cuts_only_the_member() {
    for kind in LAYOUTS {
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        state.rig.click_page(3.0, 3.0);
        state.rig.right_click_page(3.0, 3.0);
        assert_eq!(
            state.rig.selection(),
            vec![state.members[0]],
            "{kind:?}: the right-click keeps the subselected member"
        );
        state.rig.click_menu_row("Cut");
        assert_eq!(
            state.rig.page_shapes(),
            vec![state.group],
            "{kind:?}: Cut cuts only the subselected member"
        );
        let children = {
            let shell = state.rig.shell.borrow();
            let session = shell.app.active_document().expect("session");
            session
                .document
                .shape(state.group)
                .expect("group")
                .children
                .clone()
        };
        assert_eq!(
            children,
            vec![state.members[1]],
            "{kind:?}: the other member stays in the group"
        );
    }
}

// --- item 2: the shape context menu ----------------------------------

#[test]
fn the_shape_context_menu_offers_group_with_two_or_more_shapes() {
    for kind in LAYOUTS {
        // With one shape selected the row is greyed out: nothing
        // happens when it is clicked.
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        state.rig.right_click_page(3.0, 3.0);
        open_arrange_submenu(&mut state.rig, kind);
        click_group_row(&mut state.rig, kind);
        assert_eq!(
            state.rig.page_shapes(),
            vec![state.group],
            "{kind:?}: one shape selected: no group is made"
        );

        // Two shapes selected: the same row groups them.
        let (app, ids) = app_with(vec![rectangle(7.0, 3.0, 2)], kind);
        let mut rig = ShellRig::ready(app);
        let first = rig.screen(3.0, 3.0);
        let second = rig.screen(7.0, 3.0);
        rig.click_at_mods(first, egui::Modifiers::NONE);
        rig.click_at_mods(second, egui::Modifiers::SHIFT);
        assert_eq!(rig.selection(), ids, "{kind:?}: two shapes selected");
        rig.right_click_page(3.0, 3.0);
        open_arrange_submenu(&mut rig, kind);
        click_group_row(&mut rig, kind);
        assert_eq!(
            rig.page_shapes().len(),
            1,
            "{kind:?}: Group groups the two selected shapes"
        );
    }
}

#[test]
fn the_shape_context_menu_offers_ungroup_on_a_group() {
    for kind in LAYOUTS {
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        assert_eq!(state.rig.selection(), vec![state.group]);
        state.rig.right_click_page(3.0, 3.0);
        open_arrange_submenu(&mut state.rig, kind);
        state.rig.click_menu_row("Ungroup");
        assert_eq!(
            state.rig.page_shapes().len(),
            2,
            "{kind:?}: Ungroup dissolves the selected group"
        );
    }
}

#[test]
fn the_shape_context_menu_offers_the_arrange_basics() {
    for kind in LAYOUTS {
        let (left, right) = rotate_labels(kind);
        let mut state = grouped(kind);
        state.rig.click_page(3.0, 3.0);
        state.rig.right_click_page(3.0, 3.0);
        open_arrange_submenu(&mut state.rig, kind);
        state.rig.run();
        let rows = state.rig.menu_rows();
        for label in [
            "Group",
            "Ungroup",
            "Bring to Front",
            "Send to Back",
            left,
            right,
            "Flip Horizontal",
            "Flip Vertical",
        ] {
            assert!(
                rows.iter().any(|row| row == label),
                "{kind:?}: the shape context menu offers {label:?}; rows: {rows:?}"
            );
        }
    }
}

#[test]
fn an_arrange_basic_is_enabled_and_moves_the_shape() {
    for kind in LAYOUTS {
        // Two shapes on the page, the back one selected: Bring to
        // Front moves it to the front (the row is really enabled).
        let (app, ids) = app_with(vec![rectangle(7.0, 3.0, 2)], kind);
        let mut rig = ShellRig::ready(app);
        let first = rig.screen(3.0, 3.0);
        rig.click_at_mods(first, egui::Modifiers::NONE);
        assert_eq!(rig.selection(), vec![ids[0]]);
        rig.right_click_page(3.0, 3.0);
        open_arrange_submenu(&mut rig, kind);
        rig.click_menu_row("Bring to Front");
        assert_eq!(
            rig.page_shapes(),
            vec![ids[1], ids[0]],
            "{kind:?}: Bring to Front is enabled and moves the shape to the front"
        );
    }
}

// --- item 6: Delete in the context menu ------------------------------

#[test]
fn the_shape_context_menu_offers_delete() {
    for kind in LAYOUTS {
        let (mut rig, _id) = crate::rig_with_rect(kind);
        rig.click_page(3.0, 3.0);
        rig.right_click_page(3.0, 3.0);
        let rows = rig.menu_rows();
        assert!(
            rows.iter().any(|row| row == "Delete"),
            "{kind:?}: the shape context menu offers Delete; rows: {rows:?}"
        );
        let undo = rig.undo_len();
        rig.click_menu_row("Delete");
        assert!(
            rig.page_shapes().is_empty(),
            "{kind:?}: Delete deletes the selection"
        );
        assert_eq!(rig.undo_len(), undo + 1, "{kind:?}: one undo step");
    }
}
