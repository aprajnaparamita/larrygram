//! Connector routing (ADR 0045): straight and simple right-angle paths
//! between two points.
//!
//! The routing is our own, not Visio's: no obstacle avoidance in Phase 5,
//! and a deterministic elbow choice. The routing *policy* (when to
//! re-route, which endpoints are glued) lives in `crates/commands`
//! (L4); these functions are pure geometry in inches (ADR 0011).
//!
//! Part of Larrygram. Alpha.

use crate::error::Error;
use crate::point::Point;

/// How a connector path is routed (ADR 0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RouteStyle {
    /// One straight segment.
    #[default]
    Straight,
    /// Segments parallel to the axes (a simple elbow, no obstacles).
    RightAngle,
}

/// Routing options (ADR 0045).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteOptions {
    /// The routing style.
    pub style: RouteStyle,
    /// The begin end's exit direction, `None` for no preference.
    pub begin_exit: Option<Point>,
    /// The end end's exit direction.
    pub end_exit: Option<Point>,
}

impl RouteOptions {
    /// Options with the given style and no exit directions.
    pub const fn new(style: RouteStyle) -> Self {
        Self {
            style,
            begin_exit: None,
            end_exit: None,
        }
    }

    /// The options with the two exit directions (a unit vector each,
    /// page space; `None` for no preference). A non-finite or
    /// zero-length exit is no preference.
    pub const fn with_exits(mut self, begin: Option<Point>, end: Option<Point>) -> Self {
        self.begin_exit = begin;
        self.end_exit = end;
        self
    }
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self::new(RouteStyle::Straight)
    }
}

/// The path points of a connector from `start` to `end`, in inches.
///
/// The result always begins with `start` and ends with `end`, and holds
/// at least those two points. A degenerate case (identical endpoints, or
/// an elbow that coincides with an endpoint) yields the two points and
/// nothing else; it is never an error and never a panic (AGENTS.md
/// section 6).
///
/// Without exit directions the right-angle elbow is deterministic (ADR
/// 0045): horizontal first when `|dx| >= |dy|`, vertical first
/// otherwise. With them (ADR 0084 rule 4) the route leaves each glued
/// end along its exit with a short stub (0.25 in, ours) and joins the
/// stubs with axis-aligned segments — the facing Z or straight run, the
/// perpendicular L, and the same-side U (whose run-back leg sits beyond
/// both stubs, so no segment runs back along a shape's own edge). A
/// non-finite or zero-length exit is no preference. Straight routes
/// ignore exit directions.
///
/// No point is repeated.
///
/// A non-finite endpoint is [`Error::InvalidArgument`]: it has no
/// sensible page position and would poison the written geometry.
pub fn route(start: Point, end: Point, options: &RouteOptions) -> Result<Vec<Point>, Error> {
    if !finite(start) || !finite(end) {
        return Err(Error::InvalidArgument);
    }
    match options.style {
        RouteStyle::Straight => Ok(vec![start, end]),
        RouteStyle::RightAngle => {
            if same(start, end) {
                return Ok(vec![start, end]);
            }
            let begin_exit = unit_exit(options.begin_exit);
            let end_exit = unit_exit(options.end_exit);
            let mut points = right_angle_with_exits(start, end, begin_exit, end_exit);
            collapse_collinear(&mut points);
            drop_repeats(&mut points);
            Ok(points)
        }
    }
}

/// Drops any interior point the route already visited (a degenerate
/// turn can land on an earlier point): no point is repeated. The first
/// and the last point always stay.
fn drop_repeats(points: &mut Vec<Point>) {
    if points.len() < 3 {
        return;
    }
    let mut kept: Vec<Point> = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        let edge = index == 0 || index + 1 == points.len();
        if edge || !kept.contains(point) {
            kept.push(*point);
        }
    }
    *points = kept;
}

/// Drops interior points that only continue the previous direction, so
/// a straight run stays `start`, `end` and the stub points remain only
/// where the route turns.
fn collapse_collinear(points: &mut Vec<Point>) {
    if points.len() < 3 {
        return;
    }
    let mut kept: Vec<Point> = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        let turn = match (
            index.checked_sub(1).and_then(|i| points.get(i)),
            points.get(index + 1),
        ) {
            (Some(previous), Some(next)) => {
                let in_leg = point.sub(*previous);
                let out_leg = next.sub(*point);
                let cross = in_leg.x * out_leg.y - in_leg.y * out_leg.x;
                let dot = in_leg.x * out_leg.x + in_leg.y * out_leg.y;
                cross != 0.0 || dot <= 0.0
            }
            _ => true,
        };
        if turn {
            kept.push(*point);
        }
    }
    if kept.len() >= 2 {
        *points = kept;
    }
}

/// The right-angle path with exit directions (ADR 0084 rule 4): a stub
/// out of each glued end along its exit, and the join between them.
fn right_angle_with_exits(
    start: Point,
    end: Point,
    begin_exit: Option<Point>,
    end_exit: Option<Point>,
) -> Vec<Point> {
    let (begin_stub, end_stub) = stub_lengths(start, end, begin_exit, end_exit);
    let out = exit_point(start, begin_exit, begin_stub);
    let back = exit_point(end, end_exit, end_stub);
    let mut points = vec![start];
    push_unique(&mut points, out);
    for corner in join(out, back, begin_exit, end_exit) {
        push_unique(&mut points, corner);
    }
    push_unique(&mut points, back);
    push_unique(&mut points, end);
    // `start` and `end` differ here (`route` answers the degenerate
    // case itself) and both always stay, so the path has at least two
    // points.
    points
}

/// The stub length of each glued end, in inches: 0.25 in, halved to a
/// quarter of the gap each when two facing exits would run past each
/// other along their shared axis (a zero gap gives zero-length stubs).
fn stub_lengths(
    start: Point,
    end: Point,
    begin_exit: Option<Point>,
    end_exit: Option<Point>,
) -> (f64, f64) {
    const STUB: f64 = 0.25;
    let (Some(begin_exit), Some(end_exit)) = (begin_exit, end_exit) else {
        return (STUB, STUB);
    };
    if let Some(axis) = shared_axis(begin_exit, end_exit) {
        let begin_sign = axis_sign(begin_exit, axis);
        let end_sign = axis_sign(end_exit, axis);
        if begin_sign != 0.0 && begin_sign == -end_sign {
            let gap = (axis_value(end, axis) - axis_value(start, axis)).abs();
            if gap < 2.0 * STUB {
                let stub = gap / 4.0;
                return (stub, stub);
            }
        }
    }
    (STUB, STUB)
}

/// The point one stub away from `point` along `exit`, or `point` itself
/// without an exit.
fn exit_point(point: Point, exit: Option<Point>, stub: f64) -> Point {
    match exit {
        Some(exit) => point.add(exit.scale(stub)),
        None => point,
    }
}

/// The axis-aligned corners that join `out` to `back`, honouring the
/// exits (ADR 0084 rule 4): each side leaves along its own exit's axis,
/// facing exits make the Z, same-side exits make the U (with the return
/// leg pushed beyond both stubs), and two exits pointing away from each
/// other wrap around. The route never runs back along a shape's own
/// edge. Free ends take the plain elbow.
fn join(out: Point, back: Point, begin_exit: Option<Point>, end_exit: Option<Point>) -> Vec<Point> {
    const PUSH: f64 = 0.25;
    let begin_axis = begin_exit.map(axis);
    let end_axis = end_exit.map(axis);
    match (begin_axis, end_axis) {
        (Some(a), Some(b)) if a == b => {
            let begin_sign = axis_sign(begin_exit.unwrap_or(Point::new(0.0, 0.0)), a);
            let end_sign = axis_sign(end_exit.unwrap_or(Point::new(0.0, 0.0)), a);
            let gap = axis_value(back, a) - axis_value(out, a);
            let gap_sign = if gap > 0.0 {
                1.0
            } else if gap < 0.0 {
                -1.0
            } else {
                0.0
            };
            if begin_sign == end_sign {
                // The same side: out, across and back, with the return
                // leg beyond both stubs (never along the shape's edge).
                let across = if begin_sign > 0.0 {
                    axis_value(out, a).max(axis_value(back, a)) + PUSH
                } else {
                    axis_value(out, a).min(axis_value(back, a)) - PUSH
                };
                let first = with_axis(out, a, across);
                let second = with_axis(back, a, across);
                let mut corners = vec![first];
                push_unique(&mut corners, second);
                corners
            } else if begin_sign == gap_sign || gap_sign == 0.0 {
                // Facing: one turn right after the begin stub, so no
                // long leg runs along the begin shape's own side.
                vec![cross(out, back, other(a))]
            } else {
                // Both exits point away from the gap: wrap around,
                // out of the begin side and back into the end side,
                // clear of both shapes.
                let wrap = shorter_side(out, back, other(a));
                let return_at = if end_sign > 0.0 {
                    axis_value(out, a).max(axis_value(back, a)) + PUSH
                } else {
                    axis_value(out, a).min(axis_value(back, a)) - PUSH
                };
                let middle = with_axis(with_axis(out, a, return_at), other(a), wrap);
                vec![
                    with_axis(out, other(a), wrap),
                    middle,
                    with_axis(back, a, return_at),
                ]
            }
        }
        (Some(a), None) => vec![cross(out, back, a)],
        (Some(a), Some(_)) => vec![cross(out, back, a)],
        (None, Some(b)) => vec![cross(out, back, other(b))],
        (None, None) => {
            // The plain elbow between the two points.
            let dx = (back.x - out.x).abs();
            let dy = (back.y - out.y).abs();
            vec![if dx >= dy {
                Point::new(back.x, out.y)
            } else {
                Point::new(out.x, back.y)
            }]
        }
    }
}

/// The coordinate on `axis` that clears both points by the push
/// distance on the shorter side (a tie goes to the lower side).
fn shorter_side(out: Point, back: Point, axis: Axis) -> f64 {
    const PUSH: f64 = 0.25;
    let low = axis_value(out, axis).min(axis_value(back, axis)) - PUSH;
    let high = axis_value(out, axis).max(axis_value(back, axis)) + PUSH;
    let travel_low = (axis_value(out, axis) - low).abs() + (axis_value(back, axis) - low).abs();
    let travel_high = (axis_value(out, axis) - high).abs() + (axis_value(back, axis) - high).abs();
    if travel_high < travel_low { high } else { low }
}

/// The one turn between `out` and `back` whose first leg runs along
/// `first_axis`.
fn cross(out: Point, back: Point, first_axis: Axis) -> Point {
    match first_axis {
        Axis::Horizontal => Point::new(back.x, out.y),
        Axis::Vertical => Point::new(out.x, back.y),
    }
}

/// The axis one exit runs along (its dominant component).
fn axis(exit: Point) -> Axis {
    if exit.x.abs() >= exit.y.abs() {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// The two exits' shared axis when both run along the same one.
fn shared_axis(begin_exit: Point, end_exit: Point) -> Option<Axis> {
    let begin = axis(begin_exit);
    let end = axis(end_exit);
    (begin == end).then_some(begin)
}

/// The other axis.
fn other(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

/// The exit's signed component on `axis` (`-1` or `1`; a unit exit
/// always runs along its dominant axis).
fn axis_sign(exit: Point, axis: Axis) -> f64 {
    if axis_value(exit, axis) > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// A point's coordinate on `axis`.
fn axis_value(point: Point, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => point.x,
        Axis::Vertical => point.y,
    }
}

/// The point `p` with its `axis` coordinate replaced.
fn with_axis(p: Point, axis: Axis, value: f64) -> Point {
    match axis {
        Axis::Horizontal => Point::new(value, p.y),
        Axis::Vertical => Point::new(p.x, value),
    }
}

/// One axis of the page plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// The x axis.
    Horizontal,
    /// The y axis.
    Vertical,
}

/// A unit exit vector, or `None` for no preference (a non-finite or
/// zero-length exit).
fn unit_exit(exit: Option<Point>) -> Option<Point> {
    let exit = exit?;
    if !finite(exit) {
        return None;
    }
    let length = exit.x.hypot(exit.y);
    if length > 0.0 && length.is_finite() {
        Some(exit.scale(1.0 / length))
    } else {
        None
    }
}

/// Pushes `point` unless it repeats the last point.
fn push_unique(points: &mut Vec<Point>, point: Point) {
    if points.last().is_none_or(|last| !same(*last, point)) {
        points.push(point);
    }
}

/// Whether both coordinates are finite.
fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// Exact point equality (the caller's coordinates are not rounded).
fn same(a: Point, b: Point) -> bool {
    a.x == b.x && a.y == b.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_hold_their_style() {
        assert_eq!(RouteOptions::default().style, RouteStyle::Straight);
        let o = RouteOptions::new(RouteStyle::RightAngle);
        assert_eq!(o.style, RouteStyle::RightAngle);
    }

    #[test]
    fn a_straight_route_is_the_two_endpoints() {
        let start = Point::new(1.0, 2.0);
        let end = Point::new(4.0, -1.0);
        let points = route(start, end, &RouteOptions::default()).expect("route");
        assert_eq!(points, vec![start, end]);
    }

    #[test]
    fn a_right_angle_route_goes_horizontal_first_when_dx_is_larger() {
        let start = Point::new(0.0, 0.0);
        let end = Point::new(3.0, 1.0);
        let points = route(start, end, &RouteOptions::new(RouteStyle::RightAngle)).expect("route");
        assert_eq!(
            points,
            vec![start, Point::new(3.0, 0.0), end],
            "the elbow keeps the end x and the start y"
        );
    }

    #[test]
    fn a_right_angle_route_goes_vertical_first_when_dy_is_larger() {
        let start = Point::new(0.0, 0.0);
        let end = Point::new(1.0, 4.0);
        let points = route(start, end, &RouteOptions::new(RouteStyle::RightAngle)).expect("route");
        assert_eq!(
            points,
            vec![start, Point::new(0.0, 4.0), end],
            "the elbow keeps the start x and the end y"
        );
    }

    #[test]
    fn the_tie_goes_horizontal_first() {
        let start = Point::new(-2.0, 5.0);
        let end = Point::new(2.0, 1.0);
        let points = route(start, end, &RouteOptions::new(RouteStyle::RightAngle)).expect("route");
        assert_eq!(
            points,
            vec![start, Point::new(2.0, 5.0), end],
            "|dx| == |dy| uses the horizontal-first rule"
        );
    }

    #[test]
    fn a_degenerate_right_angle_route_has_no_repeated_elbow() {
        let options = RouteOptions::new(RouteStyle::RightAngle);
        let p = Point::new(1.5, -2.0);
        assert_eq!(route(p, p, &options).expect("route"), vec![p, p]);

        // A purely horizontal end makes the elbow the end point.
        let start = Point::new(0.0, 0.0);
        let end = Point::new(3.0, 0.0);
        assert_eq!(
            route(start, end, &options).expect("route"),
            vec![start, end]
        );

        // A purely vertical start/end makes the vertical-first elbow the
        // end point too.
        let end = Point::new(0.0, 3.0);
        assert_eq!(
            route(start, end, &options).expect("route"),
            vec![start, end]
        );
    }

    #[test]
    fn a_straight_degenerate_route_keeps_the_two_points() {
        let p = Point::new(2.0, 3.0);
        assert_eq!(
            route(p, p, &RouteOptions::default()).expect("route"),
            vec![p, p]
        );
    }

    #[test]
    fn a_non_finite_endpoint_is_invalid() {
        let fine = Point::new(0.0, 0.0);
        for bad in [
            Point::new(f64::NAN, 0.0),
            Point::new(0.0, f64::INFINITY),
            Point::new(f64::NEG_INFINITY, 0.0),
        ] {
            assert_eq!(
                route(fine, bad, &RouteOptions::default()),
                Err(Error::InvalidArgument)
            );
            assert_eq!(
                route(bad, fine, &RouteOptions::new(RouteStyle::RightAngle)),
                Err(Error::InvalidArgument)
            );
        }
    }

    #[test]
    fn every_route_starts_and_ends_at_its_endpoints() {
        let options = [RouteStyle::Straight, RouteStyle::RightAngle];
        for style in options {
            let start = Point::new(-1.25, 0.5);
            let end = Point::new(0.75, -2.5);
            let points = route(start, end, &RouteOptions::new(style)).expect("route");
            assert_eq!(points.first(), Some(&start));
            assert_eq!(points.last(), Some(&end));
            assert!(points.len() >= 2);
        }
    }
}
