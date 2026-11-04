//! Segment moves (task P7-35 item 2) through the public
//! `larrygram_geom` API: moving a horizontal and a vertical segment,
//! an end segment getting a bend so its glued end stays, the refused
//! inputs, and the property that a successful move keeps the path's
//! ends, stays axis-aligned and repeats no point.

use larrygram_geom::{Point, move_segment};
use proptest::prelude::*;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// The five-point staircase of the deterministic cases:
/// `0` and `2` horizontal, `1` and `3` vertical.
fn staircase() -> Vec<Point> {
    vec![
        p(0.0, 0.0),
        p(2.0, 0.0),
        p(2.0, 2.0),
        p(4.0, 2.0),
        p(4.0, 4.0),
    ]
}

// ---------------------------------------------------------------------------
// Moving one segment
// ---------------------------------------------------------------------------

#[test]
fn a_horizontal_segment_moves_along_y_and_stretches_its_neighbours() {
    let moved = move_segment(&staircase(), 2, 3.0).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![
            p(0.0, 0.0),
            p(2.0, 0.0),
            p(2.0, 3.0),
            p(4.0, 3.0),
            p(4.0, 4.0)
        ]
    );
}

#[test]
fn a_vertical_segment_moves_along_x_and_stretches_its_neighbours() {
    let moved = move_segment(&staircase(), 1, -1.0).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![
            p(0.0, 0.0),
            p(-1.0, 0.0),
            p(-1.0, 2.0),
            p(4.0, 2.0),
            p(4.0, 4.0)
        ]
    );
}

// ---------------------------------------------------------------------------
// End segments get a bend
// ---------------------------------------------------------------------------

#[test]
fn a_moved_end_segment_gets_a_bend_so_the_end_stays() {
    let moved = move_segment(&staircase(), 0, 1.5).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![
            p(0.0, 0.0),
            p(0.0, 1.5),
            p(2.0, 1.5),
            p(2.0, 2.0),
            p(4.0, 2.0),
            p(4.0, 4.0)
        ]
    );
}

#[test]
fn the_last_end_segment_gets_a_bend_at_the_far_end() {
    let moved = move_segment(&staircase(), 3, 5.0).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![
            p(0.0, 0.0),
            p(2.0, 0.0),
            p(2.0, 2.0),
            p(5.0, 2.0),
            p(5.0, 4.0),
            p(4.0, 4.0)
        ]
    );
}

#[test]
fn a_single_segment_path_gets_a_bend_at_both_ends() {
    let moved = move_segment(&[p(0.0, 0.0), p(3.0, 0.0)], 0, 1.0).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![p(0.0, 0.0), p(0.0, 1.0), p(3.0, 1.0), p(3.0, 0.0)]
    );
}

// ---------------------------------------------------------------------------
// Degenerate neighbours and refused inputs
// ---------------------------------------------------------------------------

#[test]
fn a_neighbour_that_runs_along_the_segment_takes_a_jog() {
    // A collinear run `(0,0)-(2,0)-(4,0)`: the neighbour cannot
    // stretch, so a jog connects it to the moved line.
    let path = [p(0.0, 0.0), p(2.0, 0.0), p(4.0, 0.0), p(4.0, 2.0)];
    let moved = move_segment(&path, 1, 1.0).expect("the move succeeds");
    assert_eq!(
        moved,
        vec![
            p(0.0, 0.0),
            p(2.0, 0.0),
            p(2.0, 1.0),
            p(4.0, 1.0),
            p(4.0, 2.0)
        ]
    );
}

#[test]
fn a_move_to_the_current_position_changes_nothing() {
    assert_eq!(move_segment(&staircase(), 2, 2.0).unwrap(), staircase());
    assert_eq!(move_segment(&staircase(), 1, 2.0).unwrap(), staircase());
}

#[test]
fn an_out_of_range_segment_is_an_error() {
    let path = staircase();
    assert_eq!(
        move_segment(&path, 4, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
    assert_eq!(
        move_segment(&path, 99, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
    assert_eq!(
        move_segment(&[p(0.0, 0.0)], 0, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
    assert_eq!(
        move_segment(&[], 0, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
}

#[test]
fn a_non_axis_aligned_segment_is_an_error() {
    assert_eq!(
        move_segment(&[p(0.0, 0.0), p(1.0, 1.0)], 0, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
    // A zero-length segment has no axis.
    assert_eq!(
        move_segment(&[p(0.0, 0.0), p(0.0, 0.0)], 0, 1.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
}

#[test]
fn a_non_finite_target_is_an_error() {
    assert_eq!(
        move_segment(&staircase(), 2, f64::NAN),
        Err(larrygram_geom::Error::InvalidArgument)
    );
}

#[test]
fn a_move_that_repeats_a_point_is_an_error() {
    // The segment `(2,2)-(0,2)` moved onto `y = 0` lands its end on
    // the path's first point: no axis-aligned path keeps both ends and
    // repeats nothing there.
    let path = [p(0.0, 0.0), p(2.0, 0.0), p(2.0, 2.0), p(0.0, 2.0)];
    assert_eq!(
        move_segment(&path, 2, 0.0),
        Err(larrygram_geom::Error::InvalidArgument)
    );
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// An alternating axis-aligned path: horizontal and vertical steps of
/// a real extent from a random start.
fn axis_path() -> impl Strategy<Value = Vec<Point>> {
    (
        -5.0f64..5.0,
        -5.0f64..5.0,
        proptest::collection::vec(-3.0f64..3.0, 1..13),
    )
        .prop_map(|(x, y, lengths)| {
            let mut points = vec![p(x, y)];
            let mut at = points[0];
            for (step, length) in lengths.into_iter().enumerate() {
                let length = if length.abs() < 0.5 { 0.5 } else { length };
                at = if step % 2 == 0 {
                    p(at.x + length, at.y)
                } else {
                    p(at.x, at.y + length)
                };
                points.push(at);
            }
            points
        })
}

proptest! {
    /// A successful move keeps the path's ends, stays axis-aligned and
    /// repeats no point (task P7-35 item 2).
    #[test]
    fn a_moved_route_keeps_its_ends_and_shape(
        path in axis_path(),
        index in 0usize..12,
        to in -10.0f64..10.0,
    ) {
        let Ok(moved) = move_segment(&path, index, to) else {
            return Ok(());
        };
        prop_assert_eq!(moved.first().copied(), path.first().copied(), "the begin stays");
        prop_assert_eq!(moved.last().copied(), path.last().copied(), "the end stays");
        for pair in moved.windows(2) {
            let dx = (pair[0].x - pair[1].x).abs();
            let dy = (pair[0].y - pair[1].y).abs();
            prop_assert!((dx == 0.0) != (dy == 0.0), "segment {:?} is not axis-aligned", pair);
        }
        for (position, point) in moved.iter().enumerate() {
            for other in moved.iter().skip(position + 1) {
                prop_assert!(point != other, "point {point:?} repeats");
            }
        }
    }

    /// The same for any axis-aligned input, including collinear runs
    /// and zero-length neighbours (the function never panics).
    #[test]
    fn move_segment_never_panics_on_any_axis_aligned_input(
        points in proptest::collection::vec(-10.0f64..10.0, 0..48),
        index in 0usize..24,
        to in -20.0f64..20.0,
    ) {
        let path: Vec<Point> = points.chunks(2)
            .map(|pair| p(pair[0], *pair.get(1).unwrap_or(&0.0)))
            .collect();
        let _ = move_segment(&path, index, to);
    }
}
