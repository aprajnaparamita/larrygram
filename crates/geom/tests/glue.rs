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
            Candidate { point: Point::new(4.0, 4.0), exit: Some(Point::new(0.0, 1.0)) },
            Candidate { point: Point::new(5.0, 3.5), exit: Some(Point::new(1.0, 0.0)) },
            Candidate { point: Point::new(4.0, 3.0), exit: Some(Point::new(0.0, -1.0)) },
            Candidate { point: Point::new(3.0, 3.5), exit: Some(Point::new(-1.0, 0.0)) },
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
    assert_eq!(candidates[0].exit, Some(Point::new(1.0, 0.0)), "row direction");
    assert_eq!(candidates[1].point, Point::new(4.0, 3.0));
    assert_eq!(candidates[1].exit, Some(Point::new(0.0, -1.0)), "the nearest side");
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
fn resolve_pair_takes_the_nearest_pair_and_the_lower_index_on_a_tie() {
    let a = vec![
        Candidate { point: Point::new(0.0, 0.0), exit: None },
        Candidate { point: Point::new(4.0, 0.0), exit: None },
    ];
    let b = vec![
        Candidate { point: Point::new(2.0, 0.0), exit: None },
        Candidate { point: Point::new(4.0, 0.0), exit: None },
    ];
    assert_eq!(resolve_pair(&a, &b), Some((1, 1)));
    let tied = vec![
        Candidate { point: Point::new(2.0, 0.0), exit: None },
        Candidate { point: Point::new(2.0, 0.0), exit: None },
    ];
    assert_eq!(resolve_pair(&tied, &tied), Some((0, 0)), "ties to the lower index");
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
    assert_eq!(begin.point, Point::new(3.0, 3.5), "the left midpoint is nearest");
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
fn non_finite_input_falls_back_to_the_first_candidate() {
    let target = box_target(3.0, 3.0);
    let begin = EndSpec::Glued(&target, GlueEnd::Point(5));
    let end = EndSpec::Free(Point::new(0.0, f64::INFINITY));
    let (begin, end, missing) = resolve_ends(&begin, &end);
    assert!(missing);
    assert_eq!(begin.point, Point::new(4.0, 4.0), "the first candidate");
    assert_eq!(end.point, Point::new(0.0, f64::INFINITY), "a free end keeps its point");
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
            Point::new(2.25, 0.5),
            Point::new(3.75, 0.5),
            Point::new(3.75, 2.5),
            Point::new(4.0, 2.5),
        ]
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
            Point::new(2.25, 0.5),
            Point::new(5.0, 0.5),
            Point::new(5.0, 1.75),
            Point::new(5.0, 2.0),
        ]
    );
    assert!(!overlaps_side(&points, (Point::new(2.0, 0.0), Point::new(2.0, 1.0))));
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
            Point::new(2.5, 0.5),
            Point::new(2.5, 2.5),
            Point::new(2.25, 2.5),
            Point::new(2.0, 2.5),
        ],
        "the U runs back beyond both stubs"
    );
    assert!(!overlaps_side(&points, (Point::new(2.0, 0.0), Point::new(2.0, 1.0))));
    assert!(!overlaps_side(&points, (Point::new(2.0, 2.0), Point::new(2.0, 3.0))));
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
}
