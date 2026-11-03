//! Geometry primitives: points, sizes, rectangles, angles, affine transforms
//! and Visio units of measure. Canonical units are inches for lengths and
//! radians for angles (ADR 0011).
//!
//! Phase 2 needs only the types below. Path, arc, NURBS and spline types land
//! with the geometry-row work, because Phase 3 rendering is their first
//! consumer.
//!
//! Part of Larrygram. Alpha.

#![forbid(unsafe_code)]

mod angle;
mod curve;
mod error;
mod glue;
mod hit;
mod path;
mod point;
mod rect;
mod routing;
mod snap;
mod transform;
mod units;

pub use angle::Angle;
pub use curve::{Arc, Cubic, Curve, EllipticalArc, Nurbs, NurbsPoint, Polyline, Spline};
pub use error::Error;
pub use glue::{Candidate, EndSpec, GlueEnd, GlueTarget, resolve_ends, resolve_pair};
pub use hit::{
    FLATTEN_TOLERANCE, distance_point_path, distance_point_rect, distance_point_segment,
    path_contains,
};
pub use path::{Path, Segment};
pub use point::{Point, Size};
pub use rect::Rect;
pub use routing::{Obstacle, RouteOptions, RouteStyle, route};
pub use snap::{SnapCandidate, SnapKind, SnapResult, snap_axis, snap_grid, snap_point};
pub use transform::Transform;
pub use units::{Unit, UnitKind};
