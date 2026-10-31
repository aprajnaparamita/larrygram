//! Item 7: the flip draws (the user's manual pass: "Flip does not
//! draw"). The tests flip through the shape context menu in both
//! layouts and check the rendered scene mirrors — on a plain shape,
//! inside a group and on text.
//!
//! The finding came from the stale first-pass build (the master's
//! correction of 2026-10-31); the scene builder has composed
//! `FlipX`/`FlipY` into the shape transform since Phase 3
//! (`build.rs` `shape_transform`), and both backends compose that
//! transform into path and glyph painting (`png.rs` `draw_text`).

use larrygram_geom::{Point, Transform};
use larrygram_model::{ShapeId, TextContent, TextPart};
use larrygram_render::scene::Node;
use larrygram_render::{BuildOptions, Scene, build_scene};
use larrygram_ui_spec::LayoutKind;

use crate::groups::grouped;
use crate::{LAYOUTS, ShellRig, app_with};

/// The rendered scene of the rig's active page.
fn scene_of(rig: &ShellRig) -> Scene {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("session");
    let page = session.active_page.expect("page");
    build_scene(&session.document, page, &BuildOptions::new()).expect("scene")
}

/// Every shape node's absolute origin (the shape-local zero mapped to
/// the page) with its model id, in draw order.
fn origins(scene: &Scene) -> Vec<(Option<ShapeId>, Point)> {
    fn walk(nodes: &[Node], parent: Transform, out: &mut Vec<(Option<ShapeId>, Point)>) {
        for node in nodes {
            match node {
                Node::Group(group) => {
                    walk(&group.children, group.transform.then(parent), out);
                }
                Node::Shape(shape) => {
                    let total = shape.transform.then(parent);
                    out.push((shape.shape_id, total.apply(Point::new(0.0, 0.0))));
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&scene.nodes, Transform::identity(), &mut out);
    out
}

/// The origin of the node built from `id`.
fn origin_of(scene: &Scene, id: ShapeId) -> Point {
    origins(scene)
        .into_iter()
        .find(|(shape_id, _)| *shape_id == Some(id))
        .map(|(_, origin)| origin)
        .unwrap_or_else(|| panic!("no node for shape {id:?}"))
}

/// Opens the shape context menu's arrange submenu.
fn open_arrange_submenu(rig: &mut ShellRig, kind: LayoutKind) {
    let label = match kind {
        LayoutKind::PanelClassic => "Shape",
        _ => "Group",
    };
    rig.click_menu_row(label);
}

/// Flips the selection through the shape context menu.
fn flip(rig: &mut ShellRig, kind: LayoutKind, row: &str) {
    rig.right_click_page(3.0, 3.0);
    open_arrange_submenu(rig, kind);
    rig.click_menu_row(row);
}

#[test]
fn flip_horizontal_and_vertical_mirror_the_shape_in_both_layouts() {
    for kind in LAYOUTS {
        let (mut rig, id) = crate::rig_with_rect(kind);
        // The fixture rectangle: 2 x 1 in pinned at (3, 3) with its
        // local pin at (1, 0.5), so the local origin lands on (2, 2.5).
        let before = origin_of(&scene_of(&rig), id);
        assert!(
            (before.x - 2.0).abs() < 1e-9 && (before.y - 2.5).abs() < 1e-9,
            "{kind:?}: unmirrored origin {before:?}"
        );
        rig.click_page(3.0, 3.0);
        flip(&mut rig, kind, "Flip Horizontal");
        let mirrored_x = origin_of(&scene_of(&rig), id);
        assert!(
            (mirrored_x.x - 4.0).abs() < 1e-9 && (mirrored_x.y - 2.5).abs() < 1e-9,
            "{kind:?}: Flip Horizontal mirrors the shape; origin {mirrored_x:?}"
        );
        flip(&mut rig, kind, "Flip Vertical");
        let mirrored_xy = origin_of(&scene_of(&rig), id);
        assert!(
            (mirrored_xy.x - 4.0).abs() < 1e-9 && (mirrored_xy.y - 3.5).abs() < 1e-9,
            "{kind:?}: Flip Vertical mirrors the shape; origin {mirrored_xy:?}"
        );
    }
}

#[test]
fn a_flip_inside_a_group_mirrors_the_member_and_the_group() {
    for kind in LAYOUTS {
        let mut state = grouped(kind);
        // One click selects the group; a second click subselects the
        // first member (the item 3 rules), and the flip acts on it.
        state.rig.click_page(3.0, 3.0);
        state.rig.click_page(3.0, 3.0);
        let before = origin_of(&scene_of(&state.rig), state.members[0]);
        flip(&mut state.rig, kind, "Flip Horizontal");
        let after = origin_of(&scene_of(&state.rig), state.members[0]);
        assert!(
            (after.x - (before.x + 2.0)).abs() < 1e-9,
            "{kind:?}: the member mirrors inside its group: {before:?} -> {after:?}"
        );
        // The other member does not move.
        let other_before = origin_of(&scene_of(&state.rig), state.members[1]);
        assert!(
            (other_before.x - 6.0).abs() < 1e-9,
            "{kind:?}: the other member is untouched: {other_before:?}"
        );
        // And flipping the whole group mirrors every member about the
        // group's pin: one click on the other member selects the
        // group again (the item 3 rules).
        state.rig.click_page(6.0, 3.0);
        assert_eq!(state.rig.selection(), vec![state.group]);
        let group_before = origin_of(&scene_of(&state.rig), state.members[1]);
        flip(&mut state.rig, kind, "Flip Horizontal");
        let group_after = origin_of(&scene_of(&state.rig), state.members[1]);
        assert!(
            group_after.x != group_before.x,
            "{kind:?}: the flipped group mirrors its members: {group_before:?} -> {group_after:?}"
        );
    }
}

#[test]
fn a_flip_mirrors_the_text_with_the_shape() {
    for kind in LAYOUTS {
        let (mut app, ids) = app_with(Vec::new(), kind);
        let id = ids[0];
        {
            let session = app.active_document_mut().expect("session");
            session
                .document
                .set_shape_text(
                    id,
                    TextContent {
                        parts: vec![TextPart::Text("flip".to_owned())],
                    },
                )
                .expect("text");
        }
        let mut rig = ShellRig::ready(app);
        // The text rides the shape node's transform, so the flip that
        // mirrors the shape mirrors the glyphs with it (the backends
        // paint them under that transform).
        let before = origin_of(&scene_of(&rig), id);
        rig.click_page(3.0, 3.0);
        flip(&mut rig, kind, "Flip Horizontal");
        let scene = scene_of(&rig);
        let after = origin_of(&scene, id);
        assert!(
            (after.x - (before.x + 2.0)).abs() < 1e-9,
            "{kind:?}: the flipped shape carrying the text mirrors: {before:?} -> {after:?}"
        );
        let has_text = scene.nodes.iter().any(|node| match node {
            Node::Shape(shape) => shape.text.is_some(),
            _ => false,
        });
        assert!(has_text, "{kind:?}: the scene still carries the text");
    }
}
