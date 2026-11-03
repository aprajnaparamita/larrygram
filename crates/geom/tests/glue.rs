//! The endpoint resolution and the exit-aware routes (task P7-32,
//! ADR 0084 rules 3 and 4) through the public `larrygram_geom` API.
//!
//! The candidates of a target with and without connection points and
//! for a rotated shape; `resolve_pair`'s ties; point glue, its
//! missing-row fallback, free ends and non-finite inputs; and the
//! right-angle routes with exit directions: the facing Z, the
//! perpendicular L and the same-side U (no segment runs back along a
//! target's own side), a zero-length stub case, and the property that
//! every route begins and ends on its inputs and is axis-aligned.

use larrygram_geom::{
    Angle, Candidate, EndSpec, GlueEnd, GlueTarget, Point, Rect, RouteOptions, RouteStyle,
    Transform, resolve_ends, resolve_pair, route,
};
use proptest::prelude::*;

/// A 2 x 1 in box at page offset `(x, y)`, without connection points.
fn box_target(x: f64, y: f64) -> GlueTarget {
    GlueTarget {
        points: Vec::new(),
        local_box: Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0)),
        transform: Transform::translate(Point::new(x, y)),
    }
}

// ---------------------------------------------------------------------------
// Candidates (ADR 0084 rule 3)
// ---------------------------------------------------------------------------

#[test]
fn a_target_without_points_offers_its_four_side_midpoints_through_its_transform() {
    let candidates = box_target(3.0, 3.0).candidates();
    assert_eq!(
        candidates,
        vec![
            Candidate {
                point: Point::new(4.0, 4.0),
                exit: Some(Point::new(0.0, 1.0))
            },
            Candidate {
                point: Point::new(5.0, 3.5),
                exit: Some(Point::new(1.0, 0.0))
            },
            Candidate {
                point: Point::new(4.0, 3.0),
                exit: Some(Point::new(0.0, -1.0))
            },
            Candidate {
                point: Point::new(3.0, 3.5),
                exit: Some(Point::new(-1.0, 0.0))
            },
        ],
        "top, right, bottom, left"
    );
}

#[test]
fn a_target_with_points_offers_them_with_their_directions() {
    let mut target = box_target(3.0, 3.0);
    target.points = vec![
        (7, Point::new(2.0, 0.5), Some(Point::new(3.0, 0.0))),
        (9, Point::new(1.0, 0.0), None),
    ];
    let candidates = target.candidates();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].point, Point::new(5.0, 3.5));
    assert_eq!(
        candidates[0].exit,
        Some(Point::new(1.0, 0.0)),
        "row direction"
    );
    assert_eq!(candidates[1].point, Point::new(4.0, 3.0));
    assert_eq!(
        candidates[1].exit,
        Some(Point::new(0.0, -1.0)),
        "the nearest side"
    );
}

#[test]
fn a_rotated_shape_turns_its_points_and_its_exits() {
    let mut target = box_target(0.0, 0.0);
    target.transform = Transform::rotate(Angle::from_radians(std::f64::consts::FRAC_PI_2));
    let candidates = target.candidates();
    let top = candidates[0];
    assert!((top.point.x - (-1.0)).abs() < 1e-9, "{:?}", top.point);
    assert!((top.point.y - 1.0).abs() < 1e-9, "{:?}", top.point);
    let exit = top.exit.expect("exit");
    assert!((exit.x - (-1.0)).abs() < 1e-9, "{exit:?}");
    assert!(exit.y.abs() < 1e-9, "{exit:?}");
}

// ---------------------------------------------------------------------------
// resolve_pair (ADR 0084 rule 3)
// ---------------------------------------------------------------------------

#[test]
fn a_point_glue_begin_and_a_dynamic_end_resolve_together() {
    let mut fixed = box_target(0.0, 0.0);
    fixed.points = vec![(3, Point::new(1.0, 0.5), None)];
    let mut free_target = box_target(6.0, 0.0);
    free_target.points = Vec::new();
    let begin = EndSpec::Glued(&fixed, GlueEnd::Point(3));
    let end = EndSpec::Glued(&free_target, GlueEnd::Shape);
    let (begin, end, missing) = resolve_ends(&begin, &end);
    assert!(!missing);
    assert_eq!(begin.point, Point::new(1.0, 0.5), "the row's point");
    assert_eq!(
        end.point,
        Point::new(6.0, 0.5),
        "the left midpoint is nearest"
    );
}

#[test]
fn connection_points_without_a_direction_take_the_nearest_sides_normals() {
    let mut target = box_target(0.0, 0.0);
    target.points = vec![
        (0, Point::new(0.1, 0.5), None),
        (1, Point::new(1.9, 0.5), None),
    ];
    let candidates = target.candidates();
    assert_eq!(
        candidates[0].exit,
        Some(Point::new(-1.0, 0.0)),
        "the left side"
    );
    assert_eq!(
        candidates[1].exit,
        Some(Point::new(1.0, 0.0)),
        "the right side"
    );
}

#[test]
fn a_transform_that_collapses_a_direction_offers_no_exit() {
    let mut target = box_target(0.0, 0.0);
    target.transform = Transform::scale(0.0, 0.0);
    let candidates = target.candidates();
    assert_eq!(candidates.len(), 4);
    assert!(candidates.iter().all(|candidate| candidate.exit.is_none()));
}

#[test]
fn resolve_pair_takes_the_nearest_pair_and_the_lower_index_on_a_tie() {
    let a = vec![
        Candidate {
            point: Point::new(0.0, 0.0),
            exit: None,
        },
        Candidate {
            point: Point::new(4.0, 0.0),
            exit: None,
        },
    ];
    let b = vec![
        Candidate {
            point: Point::new(2.0, 0.0),
            exit: None,
        },
        Candidate {
            point: Point::new(4.0, 0.0),
            exit: None,
        },
    ];
    assert_eq!(resolve_pair(&a, &b), Some((1, 1)));
    let tied = vec![
        Candidate {
            point: Point::new(2.0, 0.0),
            exit: None,
        },
        Candidate {
            point: Point::new(2.0, 0.0),
            exit: None,
        },
    ];
    assert_eq!(
        resolve_pair(&tied, &tied),
        Some((0, 0)),
        "ties to the lower index"
    );
    assert_eq!(resolve_pair(&[], &b), None);
    assert_eq!(resolve_pair(&a, &[]), None);
}

// ---------------------------------------------------------------------------
// resolve_ends (ADR 0084 rule 3)
// ---------------------------------------------------------------------------

#[test]
fn point_glue_lands_on_the_row_point() {
    let mut target = box_target(3.0, 3.0);
    target.points = vec![(3, Point::new(1.0, 0.5), None)];
    let begin = EndSpec::Glued(&target, GlueEnd::Point(3));
    let end = EndSpec::Free(Point::new(9.0, 3.0));
    let (begin, end, missing) = resolve_ends(&begin, &end);
    assert_eq!(begin.point, Point::new(4.0, 3.5));
    assert_eq!(end.point, Point::new(9.0, 3.0));
    assert_eq!(end.exit, None);
    assert!(!missing);
}

#[test]
fn a_missing_point_row_falls_back_to_dynamic_glue_and_is_reported() {
    let target = box_target(3.0, 3.0);
    let begin = EndSpec::Glued(&target, GlueEnd::Point(5));
    let end = EndSpec::Free(Point::new(3.0, 3.5));
    let (begin, _end, missing) = resolve_ends(&begin, &end);
    assert!(missing);
    assert_eq!(
        begin.point,
        Point::new(3.0, 3.5),
        "the left midpoint is nearest"
    );
}

#[test]
fn both_ends_on_one_shape_resolve_to_distinct_candidates() {
    let target = box_target(0.0, 0.0);
    // Two dynamic ends: the nearest distinct pair, no zero-length loop.
    let begin = EndSpec::Glued(&target, GlueEnd::Shape);
    let end = EndSpec::Glued(&target, GlueEnd::Shape);
    let (begin, end, _) = resolve_ends(&begin, &end);
    assert_ne!(begin.point, end.point, "distinct candidates");

    // A point-glued end and a dynamic one on the same shape: the
    // dynamic end takes another candidate.
    let mut pointed = box_target(0.0, 0.0);
    pointed.points = vec![
        (0, Point::new(1.0, 1.0), None),
        (1, Point::new(2.0, 0.5), None),
    ];
    let fixed = EndSpec::Glued(&pointed, GlueEnd::Point(0));
    let dynamic = EndSpec::Glued(&pointed, GlueEnd::Shape);
    let (fixed, dynamic, _) = resolve_ends(&fixed, &dynamic);
    assert_ne!(fixed.point, dynamic.point, "distinct candidates");

    // The route between them has at least two distinct points and no
    // repeated point.
    let points = route(
        begin.point,
        end.point,
        &right_angle(begin.exit, end.exit),
    )
    .expect("route");
    let mut distinct: Vec<Point> = Vec::new();
    for point in &points {
        if !distinct.contains(point) {
            distinct.push(*point);
        }
    }
    assert!(distinct.len() >= 2, "two distinct points: {points:?}");
    for pair in points.windows(2) {
        assert_ne!(pair[0], pair[1], "no repeated point: {points:?}");
    }
}

#[test]
fn free_ends_stay_on_their_points() {
    let begin = EndSpec::Free(Point::new(1.5, -2.0));
    let end = EndSpec::Free(Point::new(3.0, 4.0));
    let (begin, end, missing) = resolve_ends(&begin, &end);
    assert_eq!(begin.point, Point::new(1.5, -2.0));
    assert_eq!(end.point, Point::new(3.0, 4.0));
    assert_eq!(begin.exit, None);
    assert!(!missing);
}

#[test]
fn a_target_whose_candidates_all_overflow_still_answers() {
    let mut target = box_target(0.0, 0.0);
    target.transform = Transform::translate(Point::new(f64::NAN, 0.0));
    target.points = vec![(0, Point::new(1.0, 0.5), None)];
    let begin = EndSpec::Glued(&target, GlueEnd::Shape);
    let end = EndSpec::Free(Point::new(0.0, f64::INFINITY));
    let (begin, end, _) = resolve_ends(&begin, &end);
    assert!(
        begin.point.x.is_nan(),
        "the only candidate is unusable: {:?}",
        begin.point
    );
    assert_eq!(
        end.point,
        Point::new(0.0, f64::INFINITY),
        "a free end keeps its point"
    );
}

#[test]
fn non_finite_input_falls_back_to_the_first_candidate() {
    let target = box_target(3.0, 3.0);
    let begin = EndSpec::Glued(&target, GlueEnd::Point(5));
    let end = EndSpec::Free(Point::new(0.0, f64::INFINITY));
    let (begin, end, missing) = resolve_ends(&begin, &end);
    assert!(missing);
    assert_eq!(begin.point, Point::new(4.0, 4.0), "the first candidate");
    assert_eq!(
        end.point,
        Point::new(0.0, f64::INFINITY),
        "a free end keeps its point"
    );
}

// ---------------------------------------------------------------------------
// Routes with exit directions (ADR 0084 rule 4)
// ---------------------------------------------------------------------------

/// The options of a right-angle route with two exits.
fn right_angle(begin: Option<Point>, end: Option<Point>) -> RouteOptions {
    RouteOptions::new(RouteStyle::RightAngle).with_exits(begin, end)
}

/// Whether any route segment runs along `side` (collinear, with a
/// shared span longer than a point).
fn overlaps_side(points: &[Point], side: (Point, Point)) -> bool {
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let horizontal = a.y == b.y && side.0.y == side.1.y && a.y == side.0.y;
        let vertical = a.x == b.x && side.0.x == side.1.x && a.x == side.0.x;
        if horizontal {
            let lo = a.x.min(b.x).max(side.0.x.min(side.1.x));
            let hi = a.x.max(b.x).min(side.0.x.max(side.1.x));
            if hi - lo > 0.0 {
                return true;
            }
        } else if vertical {
            let lo = a.y.min(b.y).max(side.0.y.min(side.1.y));
            let hi = a.y.max(b.y).min(side.0.y.max(side.1.y));
            if hi - lo > 0.0 {
                return true;
            }
        }
    }
    false
}

#[test]
fn the_facing_case_zigs_once_between_the_two_stubs() {
    // A's right midpoint at (2, 0.5), C's left midpoint at (4, 2.5).
    let points = route(
        Point::new(2.0, 0.5),
        Point::new(4.0, 2.5),
        &right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(-1.0, 0.0))),
    )
    .expect("route");
    assert_eq!(
        points,
        vec![
            Point::new(2.0, 0.5),
            Point::new(3.75, 0.5),
            Point::new(3.75, 2.5),
            Point::new(4.0, 2.5),
        ],
        "one zig between the two stubs"
    );
    assert!(
        !overlaps_side(&points, (Point::new(2.0, 0.0), Point::new(2.0, 1.0))),
        "nothing runs along A's own right side"
    );
    assert!(
        !overlaps_side(&points, (Point::new(4.0, 2.0), Point::new(4.0, 3.0))),
        "nothing runs along C's own left side"
    );
}

#[test]
fn the_perpendicular_case_turns_once() {
    // A's right midpoint at (2, 0.5), C's bottom midpoint at (5, 2).
    let points = route(
        Point::new(2.0, 0.5),
        Point::new(5.0, 2.0),
        &right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(0.0, -1.0))),
    )
    .expect("route");
    assert_eq!(
        points,
        vec![
            Point::new(2.0, 0.5),
            Point::new(5.0, 0.5),
            Point::new(5.0, 2.0),
        ]
    );
    assert!(!overlaps_side(
        &points,
        (Point::new(2.0, 0.0), Point::new(2.0, 1.0))
    ));
    assert!(
        !overlaps_side(&points, (Point::new(4.0, 2.0), Point::new(6.0, 2.0))),
        "the run into C's bottom midpoint is perpendicular to the side"
    );
}

#[test]
fn the_same_side_case_goes_around_and_never_along_the_edge() {
    // Both ends leave rightward: A's right midpoint at (2, 0.5), C's
    // right midpoint at (2, 2.5).
    let points = route(
        Point::new(2.0, 0.5),
        Point::new(2.0, 2.5),
        &right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(1.0, 0.0))),
    )
    .expect("route");
    assert_eq!(
        points,
        vec![
            Point::new(2.0, 0.5),
            Point::new(2.25, 0.5),
            Point::new(2.25, 2.5),
            Point::new(2.0, 2.5),
        ],
        "out, across and back, clear of the shapes"
    );
    assert!(!overlaps_side(
        &points,
        (Point::new(2.0, 0.0), Point::new(2.0, 1.0))
    ));
    assert!(!overlaps_side(
        &points,
        (Point::new(2.0, 2.0), Point::new(2.0, 3.0))
    ));
}

#[test]
fn two_exits_pointing_away_wrap_around() {
    // A's bottom-left corner at (0, 0) leaving leftward, C's top-right
    // corner at (4, 3) leaving rightward.
    let points = route(
        Point::new(0.0, 0.0),
        Point::new(4.0, 3.0),
        &right_angle(Some(Point::new(-1.0, 0.0)), Some(Point::new(1.0, 0.0))),
    )
    .expect("route");
    assert_eq!(
        points,
        vec![
            Point::new(0.0, 0.0),
            Point::new(-0.25, 0.0),
            Point::new(-0.25, -0.25),
            Point::new(4.25, -0.25),
            Point::new(4.25, 3.0),
            Point::new(4.0, 3.0),
        ],
        "out of the begin side, around, and back into the end side"
    );
    for pair in points.windows(2) {
        assert_ne!(pair[0], pair[1], "no repeated point: {points:?}");
    }
    assert!(!overlaps_side(
        &points,
        (Point::new(0.0, -1.0), Point::new(2.0, -1.0))
    ));
}

#[test]
fn a_zero_gap_facing_pair_gets_zero_length_stubs() {
    let points = route(
        Point::new(2.0, 0.5),
        Point::new(2.0, 2.5),
        &right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(-1.0, 0.0))),
    )
    .expect("route");
    assert_eq!(points, vec![Point::new(2.0, 0.5), Point::new(2.0, 2.5)]);
}

/// No segment of `points` crosses the interior of `box_`.
fn clears(points: &[Point], box_: larrygram_geom::Rect) -> bool {
    for pair in points.windows(2) {
        let (p, q) = (pair[0], pair[1]);
        let (min, max) = (box_.min, box_.max);
        if (p.y - q.y).abs() == 0.0
            && p.y > min.y
            && p.y < max.y
            && p.x.min(q.x) < max.x
            && p.x.max(q.x) > min.x
        {
            return false;
        }
        if (p.x - q.x).abs() == 0.0
            && p.x > min.x
            && p.x < max.x
            && p.y.min(q.y) < max.y
            && p.y.max(q.y) > min.y
        {
            return false;
        }
    }
    true
}

/// No turn of `points` reverses the previous leg (no 180 degrees).
fn no_reversal(points: &[Point]) -> bool {
    for triple in points.windows(3) {
        let first = triple[1].sub(triple[0]);
        let second = triple[2].sub(triple[1]);
        if first.x * second.x + first.y * second.y < 0.0 {
            return false;
        }
    }
    true
}

/// One probe of the review: the begin point and exit, the end point
/// and exit, and the end target's box.
type Probe = (
    Point,
    Option<Point>,
    Point,
    Option<Point>,
    larrygram_geom::Rect,
);

/// The review's three probes (task P7-49 goal 3): the L must not run
/// back through the begin shape or through the target, so each route
/// clears both boxes and turns no 180 degrees.
#[test]
fn the_review_probes_clear_their_shapes_without_a_reversal() {
    let a = larrygram_geom::Rect::new(Point::new(3.0, 3.0), Point::new(5.0, 4.0));
    let probes: [Probe; 3] = [
        // Leaving A's right side towards a shape below and left of A.
        (
            Point::new(5.0, 3.5),
            Some(Point::new(1.0, 0.0)),
            Point::new(4.0, 1.0),
            Some(Point::new(0.0, 1.0)),
            larrygram_geom::Rect::new(Point::new(3.0, 0.0), Point::new(5.0, 1.0)),
        ),
        // Leaving A's top side towards a shape above and left of A.
        (
            Point::new(4.0, 4.0),
            Some(Point::new(0.0, 1.0)),
            Point::new(2.0, 6.0),
            Some(Point::new(-1.0, 0.0)),
            larrygram_geom::Rect::new(Point::new(2.0, 5.0), Point::new(4.0, 7.0)),
        ),
        // Leaving a box's right side towards a shape below and left.
        (
            Point::new(0.0, 0.0),
            Some(Point::new(1.0, 0.0)),
            Point::new(-3.0, -3.0),
            Some(Point::new(0.0, -1.0)),
            larrygram_geom::Rect::new(Point::new(-4.0, -3.0), Point::new(-2.0, -1.0)),
        ),
    ];
    for (begin, begin_exit, end, end_exit, end_box) in probes {
        let begin_box = if begin == Point::new(5.0, 3.5) || begin == Point::new(4.0, 4.0) {
            a
        } else {
            larrygram_geom::Rect::new(Point::new(-2.0, -0.5), Point::new(0.0, 0.5))
        };
        let points = route(
            begin,
            end,
            &right_angle(begin_exit, end_exit).with_boxes(Some(begin_box), Some(end_box)),
        )
        .expect("route");
        assert!(
            clears(&points, begin_box),
            "enters the begin box: {points:?}"
        );
        assert!(clears(&points, end_box), "enters the end box: {points:?}");
        assert!(no_reversal(&points), "a 180 turn: {points:?}");
    }
}

#[test]
fn a_zero_length_or_non_finite_exit_is_no_preference() {
    let start = Point::new(2.0, 0.5);
    let end = Point::new(5.0, 2.0);
    let none = route(start, end, &right_angle(None, None)).expect("route");
    for exits in [
        (Some(Point::new(0.0, 0.0)), None),
        (
            Some(Point::new(f64::NAN, 0.0)),
            Some(Point::new(0.0, f64::INFINITY)),
        ),
    ] {
        let treated = route(start, end, &right_angle(exits.0, exits.1)).expect("route");
        assert_eq!(treated, none, "{exits:?} routes like no preference");
    }
}

#[test]
fn a_degenerate_route_with_exits_keeps_the_two_points() {
    let p = Point::new(-1.5, 4.0);
    let points = route(
        p,
        p,
        &right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(0.0, -1.0))),
    )
    .expect("route");
    assert_eq!(points, vec![p, p]);
}

#[test]
fn exits_never_break_the_route_contract() {
    let options = right_angle(Some(Point::new(1.0, 0.0)), Some(Point::new(0.0, 1.0)));
    let start = Point::new(-2.0, 3.0);
    let end = Point::new(7.0, -1.0);
    let points = route(start, end, &options).expect("route");
    assert_eq!(points.first(), Some(&start));
    assert_eq!(points.last(), Some(&end));
    for pair in points.windows(2) {
        assert_ne!(pair[0], pair[1], "no repeated point: {points:?}");
    }
}

/// One glue target of the proptest: a box and one of its side points
/// with that side's outward normal, jittered (task P7-49 goal 4: the
/// router snaps it back to the dominant axis).
fn target(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    side: u8,
    along: f64,
    jitter: (f64, f64),
) -> (Point, Option<Point>, larrygram_geom::Rect) {
    let box_ = larrygram_geom::Rect::new(Point::new(x, y), Point::new(x + width, y + height));
    let (point, normal) = match side % 4 {
        0 => (Point::new(x + width * along, y), Point::new(0.0, -1.0)),
        1 => (
            Point::new(x + width, y + height * along),
            Point::new(1.0, 0.0),
        ),
        2 => (
            Point::new(x + width * along, y + height),
            Point::new(0.0, 1.0),
        ),
        _ => (Point::new(x, y + height * along), Point::new(-1.0, 0.0)),
    };
    // The wobble keeps the outward sign on the normal's axis and adds
    // a parallel component: the snapped exit may run along the side
    // but never into the box.
    let parallel = Point::new(normal.y, -normal.x);
    let exit = Point::new(
        normal.x * (0.5 + jitter.0.abs()) + parallel.x * jitter.1,
        normal.y * (0.5 + jitter.0.abs()) + parallel.y * jitter.1,
    );
    (point, Some(exit), box_)
}

proptest! {
    /// Every right-angle route with axis-aligned exits begins and ends
    /// on its inputs, repeats no point and is axis-aligned.
    #[test]
    fn every_route_begins_and_ends_on_the_inputs_and_is_axis_aligned(
        start_x in -100.0f64..100.0,
        start_y in -100.0f64..100.0,
        end_x in -100.0f64..100.0,
        end_y in -100.0f64..100.0,
        begin in prop_oneof![
            Just(None),
            Just(Some(Point::new(1.0, 0.0))),
            Just(Some(Point::new(-1.0, 0.0))),
            Just(Some(Point::new(0.0, 1.0))),
            Just(Some(Point::new(0.0, -1.0))),
        ],
        end in prop_oneof![
            Just(None),
            Just(Some(Point::new(1.0, 0.0))),
            Just(Some(Point::new(-1.0, 0.0))),
            Just(Some(Point::new(0.0, 1.0))),
            Just(Some(Point::new(0.0, -1.0))),
        ],
    ) {
        let start = Point::new(start_x, start_y);
        let finish = Point::new(end_x, end_y);
        let points = route(start, finish, &right_angle(begin, end)).expect("route");
        prop_assert_eq!(points.first(), Some(&start));
        prop_assert_eq!(points.last(), Some(&finish));
        prop_assert!(points.len() >= 2);
        for pair in points.windows(2) {
            prop_assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "not axis-aligned: {points:?}");
            prop_assert!(pair[0] != pair[1], "repeated point: {points:?}");
        }
    }

    /// Every route between two glue targets clears both boxes and
    /// turns no 180 degrees, even with jittered (non-axis-aligned)
    /// exits: every segment is horizontal or vertical (task P7-49
    /// goals 3 and 4).
    #[test]
    fn every_route_clears_its_shapes_and_turns_no_180(
        x in -20.0f64..20.0,
        y in -20.0f64..20.0,
        width in 0.5f64..3.0,
        height in 0.5f64..2.0,
        side_a in 0u8..4,
        side_b in 0u8..4,
        along_a in 0.1f64..0.9,
        along_b in 0.1f64..0.9,
        dx in -12.0f64..12.0,
        dy in -12.0f64..12.0,
        jitter_a in (-0.4f64..0.4, -1.2f64..1.2),
        jitter_b in (-0.4f64..0.4, -1.2f64..1.2),
    ) {
        let (begin, begin_exit, begin_box) =
            target(x, y, width, height, side_a, along_a, jitter_a);
        // The end target sits clear of the begin one on one axis (two
        // glue targets never overlap).
        let (bx, by) = if dx.abs() > dy.abs() {
            (x + width + 1.0 + dx.abs(), y + dy)
        } else {
            (x + dx, y + height + 1.0 + dy.abs())
        };
        let (end, end_exit, end_box) = target(bx, by, width, height, side_b, along_b, jitter_b);
        let points = route(
            begin,
            end,
            &right_angle(begin_exit, end_exit).with_boxes(Some(begin_box), Some(end_box)),
        )
        .expect("route");
        prop_assert_eq!(points.first(), Some(&begin));
        prop_assert_eq!(points.last(), Some(&end));
        for pair in points.windows(2) {
            prop_assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "not axis-aligned: {points:?}");
            prop_assert!(pair[0] != pair[1], "repeated point: {points:?}");
        }
        for triple in points.windows(3) {
            let first = triple[1].sub(triple[0]);
            let second = triple[2].sub(triple[1]);
            prop_assert!(first.x * second.x + first.y * second.y >= 0.0, "a 180 turn: {points:?}");
        }
        prop_assert!(clears(&points, begin_box), "enters the begin box: {points:?}");
        prop_assert!(clears(&points, end_box), "enters the end box: {points:?}");
    }
}
