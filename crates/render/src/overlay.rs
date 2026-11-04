//! Drag and selection overlays (ADR 0042).
//!
//! The overlay is transient vector art drawn on top of the cached canvas
//! raster: the raster is re-rasterized only when the document change
//! counter moves, so a drag redraws marks, never a full page. The marks
//! are plain data in page inches; `crates/ui` (L6) draws them as egui
//! shapes.
//!
//! A [`OverlayMark::SelectionOutline`] or [`OverlayMark::Handles`] mark
//! carries the node's bounds rectangle and its rotation: the UI draws
//! `bounds` turned by `angle` about `bounds`' centre. The rectangle is
//! the node's own bounding box positioned at its transformed centre and
//! scaled by the transform's axes, so one shape's outline follows its
//! rotation and the union of several shapes is the axis-aligned union of
//! their transformed bounds.

use larrygram_geom::{Angle, Point, Rect};
use larrygram_model::ConnectorEnd;

use crate::error::Error;
use crate::hit::{NodePath, node_bounds, oriented_bounds, placement};
use crate::scene::Scene;

/// One connector end's handle mark (task P7-33 item 1): the end's
/// page point and whether the end is glued (the UI draws a filled
/// accent marker) or free (a hollow one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndMark {
    /// Which connector end the handle moves.
    pub end: ConnectorEnd,
    /// The handle's anchor, in page inches.
    pub at: Point,
    /// Whether the end is glued.
    pub glued: bool,
}

/// What a dragged connector end will glue to, and the points to show
/// (task P7-33 item 3): the target's outline for dynamic glue, the
/// single highlighted point for point glue, and the connection points
/// of the shape under the pointer as markers. P7-34 reuses it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlueFeedback {
    /// The target's outline (page space) for dynamic glue.
    pub outline: Option<Vec<Point>>,
    /// The single highlighted point for point glue.
    pub point: Option<Point>,
    /// Connection points to show as markers (page space).
    pub markers: Vec<Point>,
}

/// One overlay mark (ADR 0042).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OverlayMark {
    /// The selection outline around a shape's bounds, rotated with it.
    SelectionOutline {
        /// The shape's bounds, in page inches.
        bounds: Rect,
        /// The shape's rotation.
        angle: Angle,
    },
    /// The resize and rotate handles of the selection, at their anchor
    /// points (the UI draws the glyphs). With `ends` the selection is a
    /// 1-D connector: its begin and end handles draw instead of the
    /// resize and rotate ones (task P7-33 item 1).
    Handles {
        /// The selection bounds, in page inches.
        bounds: Rect,
        /// The selection rotation.
        angle: Angle,
        /// The connector end handles of a 1-D selection.
        ends: Option<[EndMark; 2]>,
    },
    /// The marquee rectangle while it is dragged.
    RubberBand(Rect),
    /// A snap guide line (two points, in page inches).
    SnapGuide {
        /// One end of the guide.
        a: Point,
        /// The other end.
        b: Point,
    },
    /// The live preview of a connector being drawn or re-routed.
    ConnectorPreview {
        /// The routed points, in page inches (ADR 0045).
        points: Vec<Point>,
    },
    /// The glue feedback under the pointer while a connector end is
    /// drawn or dragged (task P7-33 item 3).
    GlueFeedback(GlueFeedback),
    /// A shape's connection points as markers (task P7-34 items 1 and
    /// 2): P7-33's marker style for every point, and the selected
    /// point's own style. The view check shows every shape's points
    /// (`selected` `None`); the Connection Point tool shows the
    /// selected shape's points and marks the selected one.
    ConnectionPoints {
        /// The points, in page inches.
        points: Vec<Point>,
        /// The selected point, in page inches, or `None`.
        selected: Option<Point>,
    },
}

/// The transient marks of one frame (ADR 0042).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Overlay {
    /// The marks, in draw order.
    pub marks: Vec<OverlayMark>,
}

impl Overlay {
    /// An empty overlay.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }
}

/// The selection outline and handle marks for the given nodes (the
/// selection's shapes as scene nodes).
///
/// Handles are laid out around the union bounds of the nodes (one shape:
/// its rotated bounds; several: the axis-aligned union). One
/// [`OverlayMark::SelectionOutline`] comes first per node, in the given
/// order, then one [`OverlayMark::Handles`] mark for the whole selection.
/// An empty `nodes` list yields an empty overlay. A node path that does
/// not resolve is [`Error::InvalidNode`] (the caller passes paths from
/// `hit_test`); a node that draws nothing contributes its zero bounds at
/// the transformed origin.
pub fn build_selection_overlay(scene: &Scene, nodes: &[NodePath]) -> Result<Overlay, Error> {
    let mut overlay = Overlay::new();
    let mut single: Option<(Rect, Angle)> = None;
    let mut union: Option<Rect> = None;
    for path in nodes {
        let (bounds, radians) = oriented_bounds(&placement(scene, path)?);
        let angle = Angle::from_radians(radians);
        overlay
            .marks
            .push(OverlayMark::SelectionOutline { bounds, angle });
        if single.is_none() {
            single = Some((bounds, angle));
        }
        let page = node_bounds(scene, path)?;
        union = Some(match union {
            Some(union) => union.union(page),
            None => page,
        });
    }
    let handles = match nodes.len() {
        0 => None,
        1 => single,
        _ => Some((
            union.unwrap_or_else(|| Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))),
            Angle::from_radians(0.0),
        )),
    };
    if let Some((bounds, angle)) = handles {
        overlay.marks.push(OverlayMark::Handles {
            bounds,
            angle,
            ends: None,
        });
    }
    Ok(overlay)
}

#[cfg(test)]
mod tests {
    use core::f64::consts::FRAC_PI_2;

    use larrygram_geom::{Path, Transform};

    use super::*;
    use crate::scene::{Node, Rgba, Shape};

    fn shape(transform: Transform, path: Path) -> Node {
        Node::Shape(Box::new(Shape {
            shape_id: None,
            transform,
            path,
            fill: Some(crate::scene::Paint::Solid(Rgba::opaque(0, 0, 0))),
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        }))
    }

    fn box_path(width: f64, height: f64) -> Path {
        let mut path = Path::new();
        path.push(larrygram_geom::Segment::MoveTo(Point::new(0.0, 0.0)));
        path.push(larrygram_geom::Segment::LineTo(Point::new(width, 0.0)));
        path.push(larrygram_geom::Segment::LineTo(Point::new(width, height)));
        path.push(larrygram_geom::Segment::LineTo(Point::new(0.0, height)));
        path.push(larrygram_geom::Segment::Close);
        path
    }

    #[test]
    fn overlays_hold_their_marks() {
        let overlay = Overlay::new();
        assert!(overlay.is_empty());
        let mut overlay = Overlay::new();
        overlay.marks.push(OverlayMark::RubberBand(Rect::new(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
        )));
        assert!(!overlay.is_empty());
        assert_eq!(overlay.marks.len(), 1);
    }

    #[test]
    fn empty_selection_draws_nothing() {
        let scene = Scene::new(8.5, 11.0);
        let overlay = build_selection_overlay(&scene, &[]).expect("no error");
        assert!(overlay.is_empty());
    }

    #[test]
    fn one_shape_gets_its_rotated_bounds() {
        let mut scene = Scene::new(8.5, 11.0);
        // A 2 by 1 rectangle turned 90 degrees about the origin.
        scene.nodes.push(shape(
            Transform::rotate(Angle::from_radians(FRAC_PI_2)),
            box_path(2.0, 1.0),
        ));
        let overlay =
            build_selection_overlay(&scene, &[NodePath::new().child(0)]).expect("no error");
        assert_eq!(overlay.marks.len(), 2);
        match overlay.marks.first() {
            Some(OverlayMark::SelectionOutline { bounds, angle }) => {
                assert_eq!(bounds.width(), 2.0);
                assert_eq!(bounds.height(), 1.0);
                assert_eq!((bounds.min.x + bounds.max.x) / 2.0, -0.5);
                assert_eq!((bounds.min.y + bounds.max.y) / 2.0, 1.0);
                assert_eq!(angle.radians(), FRAC_PI_2);
            }
            other => panic!("expected a selection outline, got {other:?}"),
        }
        match overlay.marks.get(1) {
            Some(OverlayMark::Handles {
                bounds,
                angle,
                ends,
            }) => {
                assert_eq!(bounds.width(), 2.0);
                assert_eq!(angle.radians(), FRAC_PI_2);
                assert_eq!(*ends, None, "no connector end handles");
            }
            other => panic!("expected handles, got {other:?}"),
        }
    }

    #[test]
    fn a_non_finite_transform_degrades_to_the_placement_bounds() {
        let mut scene = Scene::new(8.5, 11.0);
        let transform = Transform {
            m11: f64::NAN,
            ..Transform::identity()
        };
        scene.nodes.push(shape(transform, box_path(1.0, 1.0)));
        let overlay =
            build_selection_overlay(&scene, &[NodePath::new().child(0)]).expect("no error");
        match overlay.marks.first() {
            Some(OverlayMark::SelectionOutline { bounds, angle }) => {
                assert_eq!(
                    *bounds,
                    Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))
                );
                assert_eq!(angle.radians(), 0.0);
            }
            other => panic!("expected a selection outline, got {other:?}"),
        }
    }

    #[test]
    fn several_shapes_get_one_axis_aligned_handle_frame() {
        let mut scene = Scene::new(8.5, 11.0);
        scene
            .nodes
            .push(shape(Transform::identity(), box_path(1.0, 1.0)));
        scene.nodes.push(shape(
            Transform::translate(Point::new(3.0, 3.0)),
            box_path(1.0, 1.0),
        ));
        let paths = [NodePath::new().child(0), NodePath::new().child(1)];
        let overlay = build_selection_overlay(&scene, &paths).expect("no error");
        assert_eq!(overlay.marks.len(), 3);
        match overlay.marks.get(2) {
            Some(OverlayMark::Handles {
                bounds,
                angle,
                ends,
            }) => {
                assert_eq!(
                    *bounds,
                    Rect::new(Point::new(0.0, 0.0), Point::new(4.0, 4.0))
                );
                assert_eq!(angle.radians(), 0.0);
                assert_eq!(*ends, None);
            }
            other => panic!("expected handles, got {other:?}"),
        }
    }

    /// Task P7-33 item 1 and 3: the end-handle marks and the glue
    /// feedback are plain marks with the fields the UI draws.
    #[test]
    fn end_handles_and_glue_feedback_carry_their_marks() {
        let ends = [
            EndMark {
                end: ConnectorEnd::Begin,
                at: Point::new(1.0, 2.0),
                glued: true,
            },
            EndMark {
                end: ConnectorEnd::End,
                at: Point::new(3.0, 4.0),
                glued: false,
            },
        ];
        let mut overlay = Overlay::new();
        overlay.marks.push(OverlayMark::Handles {
            bounds: Rect::new(Point::new(1.0, 2.0), Point::new(3.0, 4.0)),
            angle: Angle::default(),
            ends: Some(ends),
        });
        overlay.marks.push(OverlayMark::GlueFeedback(GlueFeedback {
            outline: Some(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
            point: Some(Point::new(0.5, 0.5)),
            markers: vec![Point::new(0.25, 0.25)],
        }));
        assert_eq!(overlay.marks.len(), 2);
        match overlay.marks.first() {
            Some(OverlayMark::Handles {
                ends: Some(ends), ..
            }) => {
                assert!(ends[0].glued, "the begin is glued");
                assert_eq!(ends[1].end, ConnectorEnd::End);
                assert!(!ends[1].glued);
            }
            other => panic!("expected end handles, got {other:?}"),
        }
        assert!(matches!(
            overlay.marks.get(1),
            Some(OverlayMark::GlueFeedback(GlueFeedback { .. }))
        ));
        assert_eq!(GlueFeedback::default().outline, None);
    }

    /// Task P7-34: the connection-point markers are plain points with
    /// the selected one's own style slot.
    #[test]
    fn connection_point_markers_carry_their_points_and_selection() {
        let mut overlay = Overlay::new();
        overlay.marks.push(OverlayMark::ConnectionPoints {
            points: vec![Point::new(1.0, 1.0), Point::new(2.0, 2.0)],
            selected: Some(Point::new(2.0, 2.0)),
        });
        match overlay.marks.first() {
            Some(OverlayMark::ConnectionPoints { points, selected }) => {
                assert_eq!(points.len(), 2);
                assert_eq!(*selected, Some(Point::new(2.0, 2.0)));
            }
            other => panic!("expected connection points, got {other:?}"),
        }
        // The view check marks every point and selects none.
        overlay.marks.clear();
        overlay.marks.push(OverlayMark::ConnectionPoints {
            points: vec![Point::new(1.0, 1.0)],
            selected: None,
        });
        assert!(!overlay.is_empty());
    }

    #[test]
    fn a_stale_node_path_is_a_caller_bug() {
        let scene = Scene::new(8.5, 11.0);
        assert_eq!(
            build_selection_overlay(&scene, &[NodePath::new().child(2)]),
            Err(Error::InvalidNode)
        );
    }

    #[test]
    fn a_node_with_nothing_to_draw_degrades_to_zero_bounds() {
        let mut scene = Scene::new(8.5, 11.0);
        scene.nodes.push(shape(
            Transform::translate(Point::new(2.0, 3.0)),
            Path::new(),
        ));
        let overlay =
            build_selection_overlay(&scene, &[NodePath::new().child(0)]).expect("no error");
        match overlay.marks.first() {
            Some(OverlayMark::SelectionOutline { bounds, .. }) => {
                assert_eq!(
                    *bounds,
                    Rect::new(Point::new(2.0, 3.0), Point::new(2.0, 3.0))
                );
            }
            other => panic!("expected a selection outline, got {other:?}"),
        }
    }
}
