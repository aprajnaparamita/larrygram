//! Snapping and drag-overlay tests through the real canvas (task P5-05,
//! ADR 0043): grid snapping, Alt disabling it, and the snap guides on the
//! overlay.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_geom::{Point, SnapKind};
use larrygram_model::{CellName, CellValue, CellWrite, ShapeId, SheetRef, stencils};
use larrygram_render::OverlayMark;
use larrygram_ui::snap::{SnapSettings, Snapper};

use crate::{CanvasRig, app_with_one_shape, screen, shape_number};

/// A Basic Shapes rectangle instance resized to 3 x 2 in and pinned at
/// (3, 3); its master's connection points must follow the new size.
fn resized_master_instance() -> (AppCtx, ShapeId) {
    let master = stencils::basic_shapes()
        .masters
        .into_iter()
        .find(|master| master.name_u == "Rectangle")
        .expect("the rectangle master");
    let mut document = larrygram_model::Document::new();
    let shape = stencils::instantiate(&mut document, &master);
    for (cell, value) in [
        ("Width", 3.0),
        ("Height", 2.0),
        ("PinX", 3.0),
        ("PinY", 3.0),
    ] {
        document
            .write_cell(
                SheetRef::Shape(shape),
                &CellName::new(cell),
                &CellWrite::new(format!("{value}"), CellValue::Number(value)),
            )
            .expect("resize the instance");
    }
    let mut app = AppCtx::new(crate::phase5_settings());
    app.open_document(DocumentSession::new(document, None));
    (app, shape)
}

#[test]
fn a_resized_master_instance_snaps_to_its_scaled_connection_points() {
    let (app, _shape) = resized_master_instance();
    let snapper = Snapper::new(SnapSettings {
        to_grid: false,
        to_shapes: false,
        to_connection_points: true,
        ..SnapSettings::default()
    });
    // The instance's local (3, 1) point is page (4.5, 3.0) after the
    // 3 x 2 resize and the pin at (3, 3).
    let result = snapper
        .snap_point(&app, None, Point::new(4.48, 2.98), 1.0, false)
        .expect("no error");
    assert_eq!(result.point, Point::new(4.5, 3.0));
    assert_eq!(result.kind, Some(SnapKind::ConnectionPoint));
}

#[test]
fn a_move_snaps_to_the_grid() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    // 0.13 in lands nearest the 1/8 in grid line at 3.125.
    rig.drag(screen(3.0, 3.0), screen(3.13, 3.0));
    let pin = shape_number(&rig.app.borrow(), id, "PinX");
    assert!((pin - 3.125).abs() < 1.0e-9, "{pin}");
}

#[test]
fn the_drag_overlay_draws_a_snap_guide() {
    let (app, _id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    rig.down(screen(3.0, 3.0));
    rig.move_to(screen(3.13, 3.0));
    {
        let overlay = rig.overlay.borrow();
        assert!(
            overlay
                .marks
                .iter()
                .any(|mark| matches!(mark, OverlayMark::SnapGuide { .. })),
            "{:?}",
            overlay.marks
        );
        assert!(
            overlay
                .marks
                .iter()
                .any(|mark| matches!(mark, OverlayMark::SelectionOutline { .. })),
            "the preview outline draws"
        );
    }
    rig.up(screen(3.13, 3.0));
}

#[test]
fn alt_keeps_the_unsnapped_delta() {
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    rig.set_alt(true);
    rig.drag(screen(3.0, 3.0), screen(3.13, 3.0));
    rig.set_alt(false);
    let pin = shape_number(&rig.app.borrow(), id, "PinX");
    assert!((pin - 3.13).abs() < 1.0e-5, "{pin}");
}

#[test]
fn the_pointer_and_snapper_share_the_page_scene() {
    // A second frame with no change reuses the pointer's scene cache and
    // keeps the selection overlay; a document change rebuilds it.
    let (app, id) = app_with_one_shape();
    let mut rig = CanvasRig::new(app);
    rig.click(screen(3.0, 3.0));
    rig.run();
    assert!(
        rig.overlay
            .borrow()
            .marks
            .iter()
            .any(|mark| matches!(mark, OverlayMark::Handles { .. })),
        "the selection handles draw while idle"
    );
    {
        let mut app = rig.app.borrow_mut();
        let session = app.active_document_mut().expect("session");
        if let Some(shape) = session.document.shape_mut(id) {
            shape
                .sheet
                .cells
                .retain(|cell| cell.name.as_str() != "Width");
        }
        session.changes = session.changes.wrapping_add(1);
    }
    rig.run();
    // The shape still has a scene node; the cache rebuilt without a
    // panic and the overlay is still a selection overlay.
    assert!(
        rig.overlay
            .borrow()
            .marks
            .iter()
            .any(|mark| matches!(mark, OverlayMark::SelectionOutline { .. })),
    );
}
