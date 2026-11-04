//! `curve_through` (task P7-36 item 2): the clamped uniform B-spline
//! through a route's points as control points.

use larrygram_geom::{Curve, NurbsPoint, Point, curve_through};
use proptest::prelude::*;

/// The points, knots and weights of the control data.
fn control(nurbs: &larrygram_geom::Nurbs) -> (Vec<(f64, f64)>, Vec<f64>, Vec<f64>) {
    (
        nurbs.points.iter().map(|p| (p.x, p.y)).collect(),
        nurbs.points.iter().map(|p| p.knot).collect(),
        nurbs.points.iter().map(|p| p.weight).collect(),
    )
}

#[test]
fn fewer_than_three_points_is_none() {
    assert_eq!(curve_through(&[]), None);
    assert_eq!(curve_through(&[Point::new(0.0, 0.0)]), None);
    assert_eq!(
        curve_through(&[Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
        None
    );
}

#[test]
fn three_points_take_degree_two_and_all_zero_point_knots() {
    let route = [
        Point::new(0.0, 0.0),
        Point::new(1.0, 2.0),
        Point::new(3.0, 1.0),
    ];
    let nurbs = curve_through(&route).expect("a curve");
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.x_type, 1);
    assert_eq!(nurbs.y_type, 1);
    assert_eq!(nurbs.knot_last, 1.0);
    let (xy, knots, weights) = control(&nurbs);
    assert_eq!(xy, vec![(0.0, 0.0), (1.0, 2.0), (3.0, 1.0)]);
    assert_eq!(knots, vec![0.0, 0.0, 0.0]);
    assert_eq!(weights, vec![1.0, 1.0, 1.0]);
}

#[test]
fn four_points_take_degree_three_and_one_bezier_span() {
    let route = [
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(0.0, 1.0),
    ];
    let nurbs = curve_through(&route).expect("a curve");
    assert_eq!(nurbs.degree, 3);
    let (_, knots, _) = control(&nurbs);
    // The clamped uniform vector is 0, 0, 0, 0, 1, 1, 1, 1: the stored
    // knots are its first four and `knot_last` is its fifth.
    assert_eq!(knots, vec![0.0, 0.0, 0.0, 0.0]);
    assert_eq!(nurbs.knot_last, 1.0);
}

#[test]
fn five_points_space_the_one_interior_knot() {
    let route: Vec<Point> = (0..5).map(|i| Point::new(i as f64, 0.0)).collect();
    let nurbs = curve_through(&route).expect("a curve");
    assert_eq!(nurbs.degree, 3);
    let (_, knots, _) = control(&nurbs);
    // The full vector is 0, 0, 0, 0, 0.5, 1, 1, 1, 1.
    assert_eq!(knots, vec![0.0, 0.0, 0.0, 0.0, 0.5]);
    assert_eq!(nurbs.knot_last, 1.0);
}

#[test]
fn many_points_keep_the_ends_and_space_the_interior_knots() {
    let route: Vec<Point> = (0..10).map(|i| Point::new(i as f64, (i % 3) as f64)).collect();
    let nurbs = curve_through(&route).expect("a curve");
    assert_eq!(nurbs.degree, 3);
    let (xy, knots, _) = control(&nurbs);
    assert_eq!(xy.first(), Some(&(0.0, 0.0)));
    assert_eq!(xy.last(), Some(&(9.0, 0.0)));
    assert_eq!(knots.len(), 10);
    // U[i] = (i - 3) / 7 for i in 3..10.
    for (index, knot) in knots.iter().enumerate() {
        let expected = ((index as f64 - 3.0) / 7.0).clamp(0.0, 1.0);
        assert!((knot - expected).abs() < 1e-12, "knot {index}: {knot}");
    }
    assert_eq!(nurbs.knot_last, 1.0);
}

#[test]
fn the_flattened_curve_starts_and_ends_on_the_route() {
    let route = [
        Point::new(-2.0, 1.0),
        Point::new(0.0, 4.0),
        Point::new(3.0, -1.0),
        Point::new(5.0, 2.0),
        Point::new(8.0, 0.0),
    ];
    let nurbs = curve_through(&route).expect("a curve");
    let cubics = Curve::Nurbs(nurbs)
        .to_cubics(0.001)
        .expect("flattens");
    let first = cubics.first().expect("a segment");
    let last = cubics.last().expect("a segment");
    assert_eq!(first.from, route[0]);
    assert_eq!(last.to, route[4]);
}

proptest::proptest! {
    /// `curve_through` never panics on any input.
    #[test]
    fn never_panics(points in proptest::collection::vec(
        (any::<f64>(), any::<f64>()), 0..20
    )) {
        let route: Vec<Point> = points
            .iter()
            .map(|(x, y)| Point::new(*x, *y))
            .collect();
        let _ = curve_through(&route);
    }

    /// On finite input the curve starts and ends on the route (the
    /// clamped ends interpolate the first and last control points).
    #[test]
    fn starts_and_ends_on_the_route(points in proptest::collection::vec(
        (any::<f64>(), any::<f64>()), 3..12
    )) {
        let route: Vec<Point> = points
            .iter()
            .map(|(x, y)| Point::new(*x, *y))
            .collect();
        prop_assume!(route.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        let nurbs = curve_through(&route).expect("a curve");
        let cubics = Curve::Nurbs(nurbs).to_cubics(0.001).expect("flattens");
        if let Some(first) = cubics.first() {
            prop_assert_eq!(first.from, route[0]);
        }
        if let Some(last) = cubics.last() {
            prop_assert_eq!(last.to, route[route.len() - 1]);
        }
    }
}

/// The [`NurbsPoint`] spelling the write path reads back (a smoke
/// check that the struct fields stay as the row writer expects).
#[test]
fn control_points_carry_knots_and_weights() {
    let nurbs = curve_through(&[
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(2.0, 0.0),
    ])
    .expect("a curve");
    assert_eq!(
        nurbs.points[1],
        NurbsPoint {
            x: 1.0,
            y: 1.0,
            knot: 0.0,
            weight: 1.0
        }
    );
}
