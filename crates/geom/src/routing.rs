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
use crate::transform::Transform;

/// How a connector path is routed (ADR 0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RouteStyle {
    /// One straight segment.
    #[default]
    Straight,
    /// Segments parallel to the axes (a simple elbow, no obstacles).
    RightAngle,
    /// The right-angle route bent into a curve
    /// ([`curve_through`](crate::curve_through), ADR 0084 rule 8):
    /// `route` returns the right-angle points and the caller bends
    /// them.
    Curved,
}

/// A glue target's box as the router clears it: the rectangle in its
/// own frame and the transform into the page (an **oriented** box in
/// the page frame, task P7-50 goal 3). The route never enters the
/// box's interior; a boundary touch does not count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    /// The box in the target's own frame.
    pub local: Rect,
    /// The target's own frame to the page.
    pub transform: Transform,
}

impl Obstacle {
    /// The page-space bounding box of the oriented box (the route's
    /// channels clear it).
    pub fn bounds(&self) -> Rect {
        let Rect { min, max } = self.local;
        let mut low = Point::new(f64::INFINITY, f64::INFINITY);
        let mut high = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for corner in [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)] {
            let point = self.transform.apply(corner);
            low = Point::new(low.x.min(point.x), low.y.min(point.y));
            high = Point::new(high.x.max(point.x), high.y.max(point.y));
        }
        Rect::new(low, high)
    }

    /// Whether the segment from `p` to `q` crosses the box's interior:
    /// the segment is mapped into the box's own frame and tested for a
    /// strictly interior point there (a boundary touch does not cross,
    /// task P7-50 goal 3). A transform that cannot be inverted leaves
    /// no interior to cross.
    pub fn crossed_by(&self, p: Point, q: Point) -> bool {
        let Ok(inverse) = self.transform.invert() else {
            return false;
        };
        interior_crossing(inverse.apply(p), inverse.apply(q), self.local)
    }
}

/// Whether the segment `a`..`b` has a point strictly inside `rect`.
fn interior_crossing(a: Point, b: Point, rect: Rect) -> bool {
    let x = strictly_inside_1d(a.x, b.x, rect.min.x, rect.max.x);
    let y = strictly_inside_1d(a.y, b.y, rect.min.y, rect.max.y);
    match (x, y) {
        (Some((x_lo, x_hi)), Some((y_lo, y_hi))) => x_lo.max(y_lo) < x_hi.min(y_hi),
        _ => false,
    }
}

/// The margin a point must sit inside a box's side to count as
/// strictly interior (inches). An end lands exactly on its box's
/// boundary and a round trip through the transform can land it a few
/// ULP inside; without the margin its own stub leg would count as a
/// crossing (task P7-50 goal 3).
const INTERIOR_EPS: f64 = 1e-9;

/// The open parameter interval of `t` in `[0, 1]` where the lerp of
/// `a` to `b` is strictly between `low` and `high`, or `None` when the
/// axis is never strictly inside (a degenerate side is never inside).
fn strictly_inside_1d(a: f64, b: f64, low: f64, high: f64) -> Option<(f64, f64)> {
    let (low, high) = (low + INTERIOR_EPS, high - INTERIOR_EPS);
    let Some(std::cmp::Ordering::Less) = low.partial_cmp(&high) else {
        return None;
    };
    if a == b {
        return (a > low && a < high).then_some((0.0, 1.0));
    }
    let (t0, t1) = ((low - a) / (b - a), (high - a) / (b - a));
    let lo = t0.min(t1).max(0.0);
    let hi = t0.max(t1).min(1.0);
    (lo < hi).then_some((lo, hi))
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
    /// The begin glue target's box: the route clears its interior
    /// (task P7-50 goal 3).
    pub begin_box: Option<Obstacle>,
    /// The end glue target's box.
    pub end_box: Option<Obstacle>,
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

    /// The options with the two glue targets' boxes: the route's turns
    /// clear their interiors (task P7-50 goal 3). The boxes are
    /// oriented (their own frame and transform), so a rotated target's
    /// own stub leg leaves its box instead of crossing its
    /// axis-aligned bounds.
    pub const fn with_boxes(mut self, begin: Option<Obstacle>, end: Option<Obstacle>) -> Self {
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
        RouteStyle::RightAngle | RouteStyle::Curved => {
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
    boxes: [Option<Obstacle>; 2],
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
/// three-corner wraps and four-corner staples. Without a qualifying
/// candidate the fallback is the first with no 180-degree turn, and
/// the first elbow only when none qualifies (task P7-50 goal 3, never
/// an error, never a panic).
fn join(
    start: Point,
    out: Point,
    back: Point,
    end: Point,
    begin_exit: Option<Point>,
    end_exit: Option<Point>,
    boxes: [Option<Obstacle>; 2],
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
    // The fallback (task P7-50 goal 3): the first candidate with no
    // 180-degree turn, ignoring the box check (a check that fails
    // every candidate must not answer a route that turns back through
    // a shape); the first elbow only when none qualifies.
    candidates
        .into_iter()
        .find(|corners| turns_no_180(start, out, corners, back, end))
        .unwrap_or_default()
}

/// The full route of one candidate: start, stub out, corners, stub
/// back and end.
fn full_route(start: Point, out: Point, corners: &[Point], back: Point, end: Point) -> Vec<Point> {
    let mut full: Vec<Point> = Vec::with_capacity(corners.len() + 4);
    full.push(start);
    full.push(out);
    full.extend(corners.iter().copied());
    full.push(back);
    full.push(end);
    full
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
    boxes: [Option<Obstacle>; 2],
) -> bool {
    turns_no_180(start, out, corners, back, end)
        && clears_boxes(start, out, corners, back, end, boxes)
}

/// Whether one candidate's full route turns no 180 degrees (the two
/// stubs frame the join legs).
fn turns_no_180(start: Point, out: Point, corners: &[Point], back: Point, end: Point) -> bool {
    let full = full_route(start, out, corners, back, end);
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
    }
    true
}

/// Whether no segment of one candidate's full route (stubs included)
/// crosses the interior of either glue target's oriented box (a
/// boundary touch does not count, task P7-50 goal 3).
fn clears_boxes(
    start: Point,
    out: Point,
    corners: &[Point],
    back: Point,
    end: Point,
    boxes: [Option<Obstacle>; 2],
) -> bool {
    let full = full_route(start, out, corners, back, end);
    for pair in full.windows(2) {
        for box_ in boxes.into_iter().flatten() {
            if box_.crossed_by(pair[0], pair[1]) {
                return false;
            }
        }
    }
    true
}

/// The coordinate on `axis` that clears both points and both boxes on
/// `beyond`, by the push distance (the boxes' page-space bounds, which
/// contain the oriented boxes).
fn channel(
    out: Point,
    back: Point,
    boxes: [Option<Obstacle>; 2],
    axis: Axis,
    beyond: Beyond,
) -> f64 {
    let mut low = axis_value(out, axis).min(axis_value(back, axis));
    let mut high = axis_value(out, axis).max(axis_value(back, axis));
    for box_ in boxes.into_iter().flatten() {
        let bounds = box_.bounds();
        low = low.min(axis_value(bounds.min, axis));
        high = high.max(axis_value(bounds.max, axis));
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
    // The first and the last points always turn, so `kept` has at
    // least two.
    *points = kept;
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
    use crate::Angle;

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
    fn drop_repeats_keeps_the_first_visit() {
        let mut points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
        ];
        drop_repeats(&mut points);
        assert_eq!(
            points,
            vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 0.0),
            ]
        );
    }

    #[test]
    fn the_routing_types_keep_their_derives() {
        // The options' derives are exercised (Debug, Clone, Copy,
        // PartialEq, Eq, Hash).
        let options = RouteOptions::new(RouteStyle::RightAngle);
        assert_eq!(options, options.clone());
        assert!(format!("{options:?}").contains("RightAngle"));
        assert_eq!(Beyond::Low, Beyond::Low);
        assert!(format!("{:?}", Beyond::High).contains("High"));
        let mut set = std::collections::HashSet::new();
        set.insert(RouteStyle::Straight);
        assert!(set.contains(&RouteStyle::Straight));
    }

    #[test]
    fn interior_crossing_detects_the_interior_and_the_boundary_touch() {
        let rect = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        assert!(interior_crossing(
            Point::new(-1.0, 1.0),
            Point::new(3.0, 1.0),
            rect
        ));
        assert!(!interior_crossing(
            Point::new(-1.0, 3.0),
            Point::new(3.0, 3.0),
            rect
        ));
        assert!(
            !interior_crossing(Point::new(0.0, 1.0), Point::new(0.0, 3.0), rect),
            "the boundary touches but does not cross"
        );
        assert!(interior_crossing(
            Point::new(-1.0, -1.0),
            Point::new(3.0, 3.0),
            rect
        ));
        assert!(
            !interior_crossing(Point::new(-1.0, 1.0), Point::new(1.0, -1.0), rect),
            "a corner point is a touch, not a crossing"
        );
        let flat = Rect::new(Point::new(0.0, 1.0), Point::new(2.0, 1.0));
        assert!(!interior_crossing(
            Point::new(-1.0, 1.0),
            Point::new(3.0, 1.0),
            flat
        ));
    }

    #[test]
    fn an_obstacle_tests_crossings_in_its_own_frame() {
        // A 2 x 1 box rotated 30 degrees about the origin.
        let obstacle = Obstacle {
            local: Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0)),
            transform: Transform::rotate(Angle::from_radians(std::f64::consts::FRAC_PI_6)),
        };
        // The top side's midpoint and its outward normal (the stub
        // leg): on the boundary and outwards, never a crossing.
        let midpoint = obstacle.transform.apply(Point::new(1.0, 1.0));
        let outward = obstacle.transform.apply(Point::new(1.0, 1.25));
        assert!(!obstacle.crossed_by(midpoint, outward), "the stub leaves");
        // Straight through the middle crosses.
        let through = (
            obstacle.transform.apply(Point::new(-1.0, 0.5)),
            obstacle.transform.apply(Point::new(3.0, 0.5)),
        );
        assert!(obstacle.crossed_by(through.0, through.1), "through it");
        // A collapsed transform leaves no interior to cross.
        let flat = Obstacle {
            local: obstacle.local,
            transform: Transform::scale(0.0, 0.0),
        };
        assert!(!flat.crossed_by(through.0, through.1));
        // The bounds are the axis-aligned box of the rotated corners:
        // max x = 2*cos(30) and max y = 1 + 2*sin(30)*... = 1 + cos(30).
        let bounds = obstacle.bounds();
        let cos30 = 3.0f64.sqrt() / 2.0;
        assert!((bounds.max.x - 2.0 * cos30).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.max.y - (1.0 + cos30)).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn a_route_from_inside_a_box_still_answers() {
        // Every candidate enters the box the start sits in: the first
        // elbow is the deterministic fallback, never an error.
        let box_ = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        let points = route(
            Point::new(0.5, 0.5),
            Point::new(5.0, 0.5),
            &RouteOptions::new(RouteStyle::RightAngle).with_boxes(
                Some(Obstacle {
                    local: box_,
                    transform: Transform::identity(),
                }),
                None,
            ),
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
