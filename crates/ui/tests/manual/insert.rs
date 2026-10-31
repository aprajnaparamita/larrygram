//! Items 4 and 5: Insert > Text Box and Insert > Picture (the user's
//! manual pass).
//!
//! - Item 4: `insert.text.box` and `insert.vert.text.box` place a text
//!   box at the page centre (the placement choice recorded in the
//!   task's Result) and select it.
//! - Item 5: Insert > Picture stops at the pipeline boundary: the
//!   rows are disabled with the reason naming the missing `.vsdx`
//!   image writer and image renderer (the model carries the image;
//!   the escape clause of the task's goal 5).

use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_model::{CellValue, ShapeId};
use larrygram_ui_spec::LayoutKind;

use crate::{LAYOUTS, ShellRig, rig_with_rect};

/// The number value of one cell of `shape`.
fn number(rig: &ShellRig, shape: ShapeId, name: &str) -> f64 {
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
        .unwrap_or_else(|| panic!("no {name} on the shape"))
}

/// The shape the last insertion added (the page's last shape).
fn last_shape(rig: &ShellRig) -> ShapeId {
    *rig.page_shapes().last().expect("a new shape")
}

/// Opens the Insert surface of one layout.
fn open_insert(rig: &mut ShellRig, kind: LayoutKind) {
    let _ = kind;
    rig.click_named("Insert");
    rig.run();
}

/// The accesskit state of the first widget named `label`: (disabled,
/// description).
fn widget_state(rig: &mut ShellRig, label: &str) -> (bool, String) {
    rig.run();
    rig.harness()
        .root()
        .query_all_by_label(label)
        .next()
        .map(|node| {
            let data = node.accesskit_node().data();
            (
                data.is_disabled(),
                data.description().unwrap_or_default().to_owned(),
            )
        })
        .unwrap_or_else(|| panic!("no widget named {label:?}"))
}

// --- item 4: Insert > Text Box ----------------------------------------

#[test]
fn insert_text_box_places_a_box_at_the_page_centre_in_both_layouts() {
    for kind in LAYOUTS {
        let (mut rig, _id) = rig_with_rect(kind);
        let undo = rig.undo_len();
        open_insert(&mut rig, kind);
        match kind {
            LayoutKind::PanelClassic => rig.click_menu_nth("Text Box", 0),
            _ => rig.click_named("Text Box"),
        }
        let shape = last_shape(&rig);
        assert_eq!(
            rig.selection(),
            vec![shape],
            "{kind:?}: the box is selected"
        );
        let (x, y) = (number(&rig, shape, "PinX"), number(&rig, shape, "PinY"));
        assert!(
            (x - 4.25).abs() < 1e-9 && (y - 5.5).abs() < 1e-9,
            "{kind:?}: the box sits at the page centre ({x}, {y})"
        );
        let (w, h) = (number(&rig, shape, "Width"), number(&rig, shape, "Height"));
        assert!(
            (w - 2.0).abs() < 1e-9 && (h - 0.5).abs() < 1e-9,
            "{kind:?}: the box is 2 x 0.5 in ({w}, {h})"
        );
        assert_eq!(rig.undo_len(), undo + 1, "{kind:?}: one undo step");
    }
}

#[test]
fn insert_vertical_text_box_places_an_upright_box_in_both_layouts() {
    for kind in LAYOUTS {
        let (mut rig, _id) = rig_with_rect(kind);
        open_insert(&mut rig, kind);
        match kind {
            LayoutKind::PanelClassic => {
                rig.click_menu_nth("Text Box", 1);
                rig.click_menu_row("Vertical");
            }
            _ => {
                // The Text Box split button's dropdown holds the two
                // directions; the drop-down opener is the "More items"
                // button beside it.
                // The split's drop-down arrow: "Text Box menu" on the
                // Classic toolbars, "More items" beside the button on
                // the Panel Modern command bar.
                rig.run();
                let arrow = {
                    let root = rig.harness().root();
                    let by_name = root.query_all_by_label("Text Box menu").next();
                    by_name.or_else(|| {
                        let textbox = root.query_all_by_label("Text Box").next()?;
                        let box_rect = textbox.rect();
                        root.query_all_by_label("More items")
                            .filter(|node| {
                                let rect = node.rect();
                                (rect.center().y - box_rect.center().y).abs() < 8.0
                                    && rect.center().x > box_rect.max.x
                            })
                            .min_by(|a, b| {
                                (a.rect().center().x - box_rect.max.x)
                                    .total_cmp(&(b.rect().center().x - box_rect.max.x))
                            })
                    })
                };
                arrow.expect("the Text Box drop-down arrow").click();
                rig.run();
                rig.run();
                rig.click_named("Vertical Text Box");
            }
        }
        let shape = last_shape(&rig);
        let angle = number(&rig, shape, "Angle");
        assert!(
            (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
            "{kind:?}: the vertical box stands upright (Angle {angle})"
        );
    }
}

// --- item 5: Insert > Picture -----------------------------------------

#[test]
fn insert_picture_is_disabled_with_the_pipeline_reason_in_both_layouts() {
    for kind in LAYOUTS {
        let (mut rig, _id) = rig_with_rect(kind);
        open_insert(&mut rig, kind);
        let row = match kind {
            LayoutKind::PanelClassic => {
                rig.click_menu_row("Picture");
                "From File..."
            }
            _ => "Pictures...",
        };
        let (disabled, description) = widget_state(&mut rig, row);
        assert!(disabled, "{kind:?}: {row:?} is disabled ({description:?})");
        // The reason the row carries (the tooltip where the layout
        // plumbs it) is the registry's enabled reason.
        let reason = {
            use larrygram_commands::{CommandId, Registry};
            let shell = rig.shell.borrow();
            let registry: Registry =
                larrygram_commands::builtins::registry().expect("the registry builds");
            let id = CommandId::parse("file.import").expect("id");
            registry
                .enabled(&id, &shell.app)
                .reason
                .expect("a disabled reason")
        };
        assert!(
            reason.contains("image writer") && reason.contains("image renderer"),
            "{kind:?}: the reason names the missing writer and renderer: {reason:?}"
        );
        if !description.is_empty() {
            assert!(
                description.contains("image writer"),
                "{kind:?}: the tooltip carries the reason: {description:?}"
            );
        }
    }
}
