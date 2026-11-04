//! Curved connector routes (ADR 0084 rule 8): the NURBS control data
//! of a route's points.
//!
//! A curved connector is one [`Nurbs`] whose control points are the
//! route's points (the right-angle route's points, per rule 8), of
//! degree `min(3, n - 1)`, weights 1, on a clamped uniform knot
//! vector. The knot vector follows the stored-knot convention of
//! [`Nurbs`](crate::Nurbs) (ADR 0022): one knot per control point plus
//! the final knot, padded with copies of that knot at evaluation time.
//!
//! Part of Larrygram. Alpha.

use crate::curve::{Nurbs, NurbsPoint};
use crate::point::Point;

/// The clamped uniform B-spline through a route's points as control
/// points (degree `min(3, n - 1)`, weights 1). `None` for fewer than
/// three points. Never panics.
///
/// The knot vector is the clamped uniform one over `0..=1`: `0` with
/// multiplicity `degree + 1`, `1` likewise, and the interior knots
/// evenly spaced. In [`Nurbs`]'s stored form the point knots are the
/// first `n` knots of that vector and [`Nurbs::knot_last`] is `1` (the
/// last knot), so the padding of ADR 0022 rebuilds the full vector.
/// With clamped ends the curve starts at the first route point and
/// ends at the last; the first control segment is the route's exit
/// stub, so the curve leaves each end tangent to its exit direction
/// (ADR 0084 rule 8).
pub fn curve_through(route: &[Point]) -> Option<Nurbs> {
    let n = route.len();
    if n < 3 {
        return None;
    }
    // `n >= 3` and `n` is `usize`, so `n - 1` fits a `u32` here.
    let degree = std::cmp::min(3, u32::try_from(n - 1).unwrap_or(3));
    let span = f64::from(degree);
    let width = n as f64 - span;
    let points = route
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let knot = if width <= 0.0 {
                0.0
            } else {
                ((index as f64 - span) / width).clamp(0.0, 1.0)
            };
            NurbsPoint {
                x: point.x,
                y: point.y,
                knot,
                weight: 1.0,
            }
        })
        .collect();
    Some(Nurbs {
        degree,
        x_type: 1,
        y_type: 1,
        knot_last: 1.0,
        points,
    })
}
