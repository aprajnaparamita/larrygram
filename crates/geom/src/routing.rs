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
use crate::rect::Rect;

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
    /// The begin glue target's page-space box: the route clears it
    /// (task P7-49 goal 3).
    pub begin_box: Option<Rect>,
    /// The end glue target's page-space box.
    pub end_box: Option<Rect>,
}

impl RouteOptions {
    /// Options with the given style and no exits or boxes.
    pub const fn new(style: RouteStyle) -> Self {
        Self {
            style,
            begin_exit: None,
            end_exit: None,
            begin_box: None,
            end_box: None,
        }
    }

    /// The options with the two exit directions (a unit vector each,
    /// page space; `None` for no preference). A non-finite or
    /// zero-length exit is no preference; a diagonal one is snapped to
    /// its dominant axis (task P7-49 goal 4).
    pub const fn with_exits(mut self, begin: Option<Point>, end: Option<Point>) -> Self {
        self.begin_exit = begin;
        self.end_exit = end;
        self
    }

    /// The options with the two glue targets' page-space boxes: the
    /// route's turns clear their interiors (task P7-49 goal 3).
    pub const fn with_boxes(mut self, begin: Option<Rect>, end: Option<Rect>) -> Self {
        self.begin_box = begin;
        self.end_box = end;
        self
    }
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self::new(RouteStyle::Straight)
    }
}

/// The stub length of a glued end, in inches.
const STUB: f64 = 0.25;

/// The path points of a connector from `start` to `end`, in inches.
///
/// The result always begins with `start` and ends with `end`, and holds
/// at least those two points. A degenerate case (identical endpoints)
/// yields the two points and nothing else; it is never an error and
/// never a panic (AGENTS.md section 6).
///
/// Without exit directions the right-angle elbow is deterministic
/// (ADR 0045): horizontal first when `|dx| >= |dy|`, vertical first
/// otherwise. With them (ADR 0084 rule 4) the route leaves each glued
/// end along its exit with a short stub and joins the stubs with
/// axis-aligned segments. The join never turns 180 degrees (it never
/// runs back along a shape's own edge or spikes back into an end) and,
/// when the options give the glue targets' boxes, never enters either
/// box. Straight routes ignore exit directions. Every exit is snapped
/// to its dominant axis first (task P7-49 goal 4), so every segment of
/// a right-angle route is horizontal or vertical.
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
            let boxes = [options.begin_box, options.end_box];
            let mut points = right_angle_with_exits(start, end, begin_exit, end_exit, boxes);
            collapse_collinear(&mut points);
            drop_repeats(&mut points);
            Ok(points)
        }
    }
}

/// The right-angle path with exit directions (ADR 0084 rule 4): a stub
/// out of each glued end along its exit, and the join between them.
fn right_angle_with_exits(
    start: Point,
    end: Point,
    begin_exit: Option<Point>,
    end_exit: Option<Point>,
    boxes: [Option<Rect>; 2],
) -> Vec<Point> {
    let (begin_stub, end_stub) = stub_lengths(start, end, begin_exit, end_exit);
    let out = exit_point(start, begin_exit, begin_stub);
    let back = exit_point(end, end_exit, end_stub);
    let mut points = vec![start];
    push_unique(&mut points, out);
    for corner in join(start, out, back, end, begin_exit, end_exit, boxes) {
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
    let (Some(begin_exit), Some(end_exit)) = (begin_exit, end_exit) else {
        return (STUB, STUB);
    };
    if let Some(axis) = shared_axis(begin_exit, end_exit) {
        let begin_out = axis_value(begin_exit, axis) > 0.0;
        let end_out = axis_value(end_exit, axis) > 0.0;
        if begin_out != end_out {
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

/// The corners that join `out` to `back` (task P7-49 goal 3): the
/// first candidate whose full route turns no 180 degrees and enters
/// neither glue target's box wins, so a corner that falls behind a
/// stub or runs through a shape is replaced by a detour the way the
/// wrap is. The candidates are the two one-corner elbows in the exit
/// order, then the two-corner channels beyond both boxes, then the
/// three-corner wraps. Without a valid candidate the first elbow is
/// the answer (never an error, never a panic).
fn join(
    start: Point,
    out: Point,
    back: Point,
    end: Point,
    begin_exit: Option<Point>,
    end_exit: Option<Point>,
    boxes: [Option<Rect>; 2],
) -> Vec<Point> {
    let horizontal_first = match (begin_exit.map(axis), end_exit.map(axis)) {
        (Some(Axis::Horizontal), _) => true,
        (Some(Axis::Vertical), _) => false,
        (None, Some(Axis::Vertical)) => true,
        (None, Some(Axis::Horizontal)) => false,
        (None, None) => (back.x - out.x).abs() >= (back.y - out.y).abs(),
    };
    let c1 = vec![Point::new(back.x, out.y)];
    let c2 = vec![Point::new(out.x, back.y)];
    let mut candidates: Vec<Vec<Point>> = Vec::new();
    if horizontal_first {
        candidates.push(c1.clone());
        candidates.push(c2.clone());
    } else {
        candidates.push(c2.clone());
        candidates.push(c1.clone());
    }
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for beyond in [Beyond::Low, Beyond::High] {
            let across = channel(out, back, boxes, axis, beyond);
            candidates.push(vec![
                with_axis(out, axis, across),
                with_axis(back, axis, across),
            ]);
        }
    }
    for travel in [Axis::Horizontal, Axis::Vertical] {
        let cross_axis = other(travel);
        for across_beyond in [Beyond::Low, Beyond::High] {
            let across = channel(out, back, boxes, cross_axis, across_beyond);
            for return_beyond in [Beyond::Low, Beyond::High] {
                let back_at = channel(out, back, boxes, travel, return_beyond);
                candidates.push(vec![
                    with_axis(out, cross_axis, across),
                    with_axis(with_axis(out, travel, back_at), cross_axis, across),
                    with_axis(back, travel, back_at),
                ]);
            }
        }
    }
    // The four-corner staples: out along one axis, around the far
    // channel and back in along the return channel, for the cases the
    // three-corner wraps cannot clear both boxes.
    for travel in [Axis::Horizontal, Axis::Vertical] {
        let cross_axis = other(travel);
        for out_beyond in [Beyond::Low, Beyond::High] {
            let out_at = channel(out, back, boxes, travel, out_beyond);
            for across_beyond in [Beyond::Low, Beyond::High] {
                let across = channel(out, back, boxes, cross_axis, across_beyond);
                for return_beyond in [Beyond::Low, Beyond::High] {
                    let back_at = channel(out, back, boxes, travel, return_beyond);
                    let c1 = with_axis(out, travel, out_at);
                    let c2 = with_axis(c1, cross_axis, across);
                    let c3 = with_axis(c2, travel, back_at);
                    let c4 = with_axis(c3, cross_axis, axis_value(back, cross_axis));
                    candidates.push(vec![c1, c2, c3, c4]);
                }
            }
        }
    }
    for corners in &candidates {
        if valid(start, out, corners, back, end, boxes) {
            return corners.clone();
        }
    }
    candidates.remove(0)
}

/// Whether one candidate's full route is sound: it turns no 180
/// degrees (including against the two stubs) and no segment of it,
/// stubs included, enters either glue target's box.
fn valid(
    start: Point,
    out: Point,
    corners: &[Point],
    back: Point,
    end: Point,
    boxes: [Option<Rect>; 2],
) -> bool {
    let mut full: Vec<Point> = Vec::with_capacity(corners.len() + 4);
    full.push(start);
    full.push(out);
    full.extend(corners.iter().copied());
    full.push(back);
    full.push(end);
    // The stub directions at the two ends frame the join legs.
    let mut previous: Option<Point> = None;
    for pair in full.windows(2) {
        let leg = pair[1].sub(pair[0]);
        if leg.x == 0.0 && leg.y == 0.0 {
            continue;
        }
        if let Some(previous) = previous
            && previous.x * leg.x + previous.y * leg.y < 0.0
        {
            return false;
        }
        previous = Some(leg);
        for box_ in boxes {
            if let Some(box_) = box_
                && crosses(pair[0], pair[1], box_)
            {
                return false;
            }
        }
    }
    true
}

/// Whether the segment from `p` to `q` crosses the interior of `rect`.
/// A segment along the boundary touches but does not cross.
fn crosses(p: Point, q: Point, rect: Rect) -> bool {
    let (Rect { min, max }, _ignore) = (rect, ());
    if (p.y - q.y).abs() == 0.0 {
        // Horizontal: strictly inside the y range and across the x range.
        p.y > min.y && p.y < max.y && p.x.min(q.x) < max.x && p.x.max(q.x) > min.x
    } else if (p.x - q.x).abs() == 0.0 {
        p.x > min.x && p.x < max.x && p.y.min(q.y) < max.y && p.y.max(q.y) > min.y
    } else {
        // A non-axis-aligned leg: treat it as crossing when it meets
        // the interior (the exits are snapped, so this is only the
        // no-exit elbow's corner case).
        p.x.min(q.x) < max.x && p.x.max(q.x) > min.x && p.y.min(q.y) < max.y && p.y.max(q.y) > min.y
    }
}

/// The coordinate on `axis` that clears both points and both boxes on
/// `beyond`, by the push distance.
fn channel(out: Point, back: Point, boxes: [Option<Rect>; 2], axis: Axis, beyond: Beyond) -> f64 {
    let mut low = axis_value(out, axis).min(axis_value(back, axis));
    let mut high = axis_value(out, axis).max(axis_value(back, axis));
    for box_ in boxes.into_iter().flatten() {
        low = low.min(axis_value(box_.min, axis));
        high = high.max(axis_value(box_.max, axis));
    }
    match beyond {
        Beyond::Low => low - STUB,
        Beyond::High => high + STUB,
    }
}

/// Which side of an axis a channel clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Beyond {
    /// Below or left of everything.
    Low,
    /// Above or right of everything.
    High,
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

/// A unit exit vector snapped to its dominant axis (task P7-49 goal
/// 4), or `None` for no preference (a non-finite or zero-length exit).
fn unit_exit(exit: Option<Point>) -> Option<Point> {
    let exit = exit?;
    if !finite(exit) {
        return None;
    }
    let length = exit.x.hypot(exit.y);
    if !(length > 0.0 && length.is_finite()) {
        return None;
    }
    if exit.x.abs() >= exit.y.abs() {
        Some(Point::new(if exit.x >= 0.0 { 1.0 } else { -1.0 }, 0.0))
    } else {
        Some(Point::new(0.0, if exit.y >= 0.0 { 1.0 } else { -1.0 }))
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
    fn crosses_detects_the_interior_and_the_diagonal() {
        let rect = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        assert!(crosses(Point::new(-1.0, 1.0), Point::new(3.0, 1.0), rect));
        assert!(!crosses(Point::new(-1.0, 3.0), Point::new(3.0, 3.0), rect));
        assert!(
            !crosses(Point::new(0.0, 1.0), Point::new(0.0, 3.0), rect),
            "the boundary touches but does not cross"
        );
        assert!(crosses(Point::new(-1.0, -1.0), Point::new(3.0, 3.0), rect));
    }

    #[test]
    fn a_route_from_inside_a_box_still_answers() {
        // Every candidate enters the box the start sits in: the first
        // elbow is the deterministic fallback, never an error.
        let box_ = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        let points = route(
            Point::new(0.5, 0.5),
            Point::new(5.0, 0.5),
            &RouteOptions::new(RouteStyle::RightAngle).with_boxes(Some(box_), None),
        )
        .expect("route");
        assert_eq!(points.first(), Some(&Point::new(0.5, 0.5)));
        assert_eq!(points.last(), Some(&Point::new(5.0, 0.5)));
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
