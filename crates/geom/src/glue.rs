//! Connector endpoint resolution (ADR 0084 rule 3): one pure-geometry
//! module that resolves both ends of one connector together.
//!
//! Draw, glue, unglue and every re-route call [`resolve_ends`], so one
//! end lands on the same point in every pass. The module knows nothing
//! about documents: a caller builds each [`GlueTarget`] from its model
//! facts (connection points, local box, page transform) and passes the
//! ends as [`EndSpec`].
//!
//! All points are page inches (ADR 0011). Every function here is pure
//! and never panics: malformed input degrades to a documented fallback.
//!
//! Part of Larrygram. Alpha.

use crate::point::Point;
use crate::rect::Rect;
use crate::transform::Transform;

/// One point a target offers a glued end, with its exit direction
/// (a unit vector as a `Point`, or `None` for no preference).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// The page-space point.
    pub point: Point,
    /// The direction a route leaves the point in, page space.
    pub exit: Option<Point>,
}

/// The facts one glue target offers.
#[derive(Debug, Clone, PartialEq)]
pub struct GlueTarget {
    /// The target's connection points as `(row ix, local point, local
    /// direction)`, the direction `None` when `DirX`/`DirY` are zero.
    pub points: Vec<(u32, Point, Option<Point>)>,
    /// The target's local box (0,0 to Width,Height).
    pub local_box: Rect,
    /// Local to page.
    pub transform: Transform,
}

/// What one end is glued to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlueEnd {
    /// The connection row with this zero-based index.
    Point(u32),
    /// The shape (dynamic glue).
    Shape,
}

impl GlueTarget {
    /// The candidates for dynamic glue: the connection points, or the
    /// four side midpoints when there are none (ADR 0084 rule 3).
    ///
    /// Each candidate's exit is the row's `DirX`/`DirY` direction mapped
    /// through the transform, or, when the row has none (or one that maps
    /// to a zero-length vector), the outward normal of the nearest side
    /// of the local box, through the transform. Side midpoints take their
    /// own side's outward normal. A direction that maps to a zero-length
    /// or non-finite vector leaves `exit` `None`.
    ///
    /// Without connection points the candidates are the side midpoints
    /// of the local box in a fixed order: top, right, bottom, left.
    /// The candidates keep the connection rows' order otherwise, so a
    /// row's zero-based index is its candidate index.
    pub fn candidates(&self) -> Vec<Candidate> {
        if self.points.is_empty() {
            let Rect { min, max } = self.local_box;
            let mid_x = (min.x + max.x) / 2.0;
            let mid_y = (min.y + max.y) / 2.0;
            return [
                (Point::new(mid_x, max.y), Point::new(0.0, 1.0)),
                (Point::new(max.x, mid_y), Point::new(1.0, 0.0)),
                (Point::new(mid_x, min.y), Point::new(0.0, -1.0)),
                (Point::new(min.x, mid_y), Point::new(-1.0, 0.0)),
            ]
            .into_iter()
            .map(|(local, normal)| Candidate {
                point: self.transform.apply(local),
                exit: self.direction(normal),
            })
            .collect();
        }
        self.points
            .iter()
            .map(|(_, local, dir)| {
                let local_dir = dir
                    .as_ref()
                    .copied()
                    .filter(direction_like)
                    .unwrap_or_else(|| outward_normal(self.local_box, *local));
                Candidate {
                    point: self.transform.apply(*local),
                    exit: self.direction(local_dir),
                }
            })
            .collect()
    }

    /// The target's local box mapped through the transform: the
    /// page-space box the route's detours clear (task P7-49 goal 3).
    pub fn page_box(&self) -> Rect {
        let Rect { min, max } = self.local_box;
        let mut low = Point::new(f64::INFINITY, f64::INFINITY);
        let mut high = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for corner in [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)] {
            let point = self.transform.apply(corner);
            low = Point::new(low.x.min(point.x), low.y.min(point.y));
            high = Point::new(high.x.max(point.x), high.y.max(point.y));
        }
        Rect::new(low, high)
    }

    /// A local direction through the transform's linear part, as a unit
    /// vector, or `None` when it maps to a zero-length or non-finite
    /// vector.
    fn direction(&self, local: Point) -> Option<Point> {
        let origin = self.transform.apply(Point::new(0.0, 0.0));
        let mapped = self.transform.apply(local).sub(origin);
        let length = mapped.x.hypot(mapped.y);
        if length.is_finite() && length > 0.0 {
            Some(mapped.scale(1.0 / length))
        } else {
            None
        }
    }
}

/// Whether a local direction is usable as given (non-zero and finite).
fn direction_like(point: &Point) -> bool {
    point.x.is_finite() && point.y.is_finite() && (point.x != 0.0 || point.y != 0.0)
}

/// The outward normal of the side of `rect` nearest to `local`, as a
/// local direction. A tie goes to the left, then the right, then the
/// bottom, then the top side. A degenerate (zero-size) box still
/// answers a unit vector.
fn outward_normal(rect: Rect, local: Point) -> Point {
    let left = (local.x - rect.min.x).abs();
    let right = (rect.max.x - local.x).abs();
    let bottom = (local.y - rect.min.y).abs();
    let top = (rect.max.y - local.y).abs();
    let smallest = left.min(right).min(bottom).min(top);
    if smallest == left {
        Point::new(-1.0, 0.0)
    } else if smallest == right {
        Point::new(1.0, 0.0)
    } else if smallest == bottom {
        Point::new(0.0, -1.0)
    } else {
        Point::new(0.0, 1.0)
    }
}

/// One end as the resolver sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum EndSpec<'a> {
    /// Glued to `target`.
    Glued(&'a GlueTarget, GlueEnd),
    /// Free at this page point.
    Free(Point),
}

/// One end's resolved place: a fixed point or a target that still
/// chooses.
enum Slot<'a> {
    /// The end lands on this candidate whatever the other end does.
    Fixed(Candidate),
    /// The end is dynamic glue on this target.
    Dynamic(&'a GlueTarget),
}

/// Resolves both ends of one connector (ADR 0084 rule 3).
///
/// - **Point glue** lands on the row's point through the target's
///   transform. A missing (or non-finite) row point falls back to
///   dynamic glue on that target and reports it in the returned flag.
/// - **One dynamic end facing a fixed end** (point glue or free) takes
///   the candidate nearest the fixed point.
/// - **Two dynamic ends** use [`resolve_pair`] (the nearest candidate
///   pair, ties to the lower index on the begin target, then on the end
///   target).
/// - **Free ends** resolve to their own point and never move; their
///   `exit` is `None`.
///
/// Never panics: a non-finite reference point falls back to the
/// target's first candidate, and a free end keeps whatever point it
/// was given (a non-finite free point is the caller's error to catch).
pub fn resolve_ends(begin: &EndSpec<'_>, end: &EndSpec<'_>) -> (Candidate, Candidate, bool) {
    let mut missing_row = false;
    let begin_slot = slot(begin, &mut missing_row);
    let end_slot = slot(end, &mut missing_row);
    let (begin, end) = match (begin_slot, end_slot) {
        (Slot::Fixed(b), Slot::Fixed(e)) => (b, e),
        (Slot::Fixed(b), Slot::Dynamic(target)) => {
            let e = choose(target, Some(b.point));
            (b, e)
        }
        (Slot::Dynamic(target), Slot::Fixed(e)) => {
            let b = choose(target, Some(e.point));
            (b, e)
        }
        (Slot::Dynamic(bt), Slot::Dynamic(et)) => pair(bt, et),
    };
    (begin, end, missing_row)
}

/// One end's [`Slot`]: fixed for a usable row point or a free end,
/// dynamic otherwise. A row that is missing or lands on a non-finite
/// point falls back to dynamic glue and sets `missing_row`.
fn slot<'a>(spec: &EndSpec<'a>, missing_row: &mut bool) -> Slot<'a> {
    match spec {
        EndSpec::Free(point) => Slot::Fixed(Candidate {
            point: *point,
            exit: None,
        }),
        EndSpec::Glued(target, GlueEnd::Shape) => Slot::Dynamic(target),
        EndSpec::Glued(target, GlueEnd::Point(ix)) => {
            let found = target
                .points
                .iter()
                .position(|(row, _, _)| row == ix)
                .and_then(|position| target.candidates().into_iter().nth(position));
            match found {
                Some(candidate) if finite(candidate.point) => Slot::Fixed(candidate),
                _ => {
                    *missing_row = true;
                    Slot::Dynamic(target)
                }
            }
        }
    }
}

/// The candidate a dynamic end takes against `reference` (the other
/// end's fixed point when there is one). A non-finite reference falls
/// back to the target's first candidate.
fn choose(target: &GlueTarget, reference: Option<Point>) -> Candidate {
    let candidates = target.candidates();
    match reference.filter(|point| finite(*point)) {
        Some(reference) => nearest(&candidates, reference),
        None => first(&candidates, target),
    }
}

/// Both ends dynamic: the nearest candidate pair, ties to the lower
/// index on the begin target, then on the end target.
fn pair(begin: &GlueTarget, end: &GlueTarget) -> (Candidate, Candidate) {
    let begin_candidates = begin.candidates();
    let end_candidates = end.candidates();
    let (i, j) = resolve_pair(&begin_candidates, &end_candidates).unwrap_or((0, 0));
    // Both slices are never empty and `resolve_pair` never answers
    // outside them, so the fallbacks only guard the no-panic contract.
    (
        begin_candidates
            .get(i)
            .copied()
            .unwrap_or(first(&begin_candidates, begin)),
        end_candidates
            .get(j)
            .copied()
            .unwrap_or(first(&end_candidates, end)),
    )
}

/// A target's first usable candidate (the first finite one, else the
/// first, else a degenerate one at its origin: never a panic).
fn first(candidates: &[Candidate], target: &GlueTarget) -> Candidate {
    candidates
        .iter()
        .copied()
        .find(|candidate| finite(candidate.point))
        .or(candidates.first().copied())
        .unwrap_or(Candidate {
            point: target.transform.apply(Point::new(0.0, 0.0)),
            exit: None,
        })
}

/// The candidate nearest `reference` (ties to the lower index). A
/// non-finite candidate is never nearer than a finite one, so a target
/// with only non-finite candidates answers its first (the documented
/// fallback).
fn nearest(candidates: &[Candidate], reference: Point) -> Candidate {
    let mut best: Option<(f64, Candidate)> = None;
    for candidate in candidates {
        let delta = candidate.point.sub(reference);
        let distance = distance_or_infinite(delta);
        if best.as_ref().is_none_or(|(best, _)| distance < *best) {
            best = Some((distance, *candidate));
        }
    }
    best.map(|(_, candidate)| candidate).unwrap_or(Candidate {
        point: reference,
        exit: None,
    })
}

/// The nearest candidate pair, one from each slice; ties to the lower
/// index in `a`, then in `b`. `None` when either slice is empty.
///
/// The scan is a double loop in index order with a strict `<`, so a
/// tie keeps the pair found first: the lower index in `a`, then the
/// lower index in `b`. A non-finite distance never wins over a finite
/// one (it counts as infinite).
pub fn resolve_pair(a: &[Candidate], b: &[Candidate]) -> Option<(usize, usize)> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut best: Option<(f64, (usize, usize))> = None;
    for (i, first) in a.iter().enumerate() {
        for (j, second) in b.iter().enumerate() {
            let delta = first.point.sub(second.point);
            let distance = distance_or_infinite(delta);
            if best.as_ref().is_none_or(|(best, _)| distance < *best) {
                best = Some((distance, (i, j)));
            }
        }
    }
    best.map(|(_, pair)| pair)
}

/// Whether both coordinates are finite.
fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// The length of a delta, counting a non-finite one as infinite so it
/// never wins a "nearest" comparison.
fn distance_or_infinite(delta: Point) -> f64 {
    let distance = delta.x.hypot(delta.y);
    if distance.is_finite() {
        distance
    } else {
        f64::INFINITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Angle;

    fn box_target() -> GlueTarget {
        GlueTarget {
            points: Vec::new(),
            local_box: Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0)),
            transform: Transform::translate(Point::new(3.0, 3.0)),
        }
    }

    #[test]
    fn a_target_without_points_offers_its_four_side_midpoints() {
        let candidates = box_target().candidates();
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0].point, Point::new(4.0, 4.0), "top");
        assert_eq!(candidates[1].point, Point::new(5.0, 3.5), "right");
        assert_eq!(candidates[2].point, Point::new(4.0, 3.0), "bottom");
        assert_eq!(candidates[3].point, Point::new(3.0, 3.5), "left");
        assert_eq!(candidates[0].exit, Some(Point::new(0.0, 1.0)));
        assert_eq!(candidates[1].exit, Some(Point::new(1.0, 0.0)));
        assert_eq!(candidates[2].exit, Some(Point::new(0.0, -1.0)));
        assert_eq!(candidates[3].exit, Some(Point::new(-1.0, 0.0)));
    }

    #[test]
    fn a_rotation_turns_the_points_and_the_exits() {
        let mut target = box_target();
        target.transform = Transform::rotate(Angle::from_radians(std::f64::consts::FRAC_PI_2));
        let candidates = target.candidates();
        let top = candidates[0];
        assert!((top.point.x - (-1.0)).abs() < 1e-9, "{:?}", top.point);
        assert!((top.point.y - 1.0).abs() < 1e-9, "{:?}", top.point);
        let exit = top.exit.expect("exit");
        assert!((exit.x - (-1.0)).abs() < 1e-9, "{exit:?}");
        assert!(exit.y.abs() < 1e-9, "{exit:?}");
    }

    #[test]
    fn connection_points_carry_their_direction_or_the_nearest_side() {
        let mut target = box_target();
        target.points = vec![
            (7, Point::new(2.0, 0.5), Some(Point::new(3.0, 0.0))),
            (9, Point::new(1.0, 0.0), None),
            (11, Point::new(1.0, 1.0), Some(Point::new(0.0, 0.0))),
        ];
        let candidates = target.candidates();
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].point, Point::new(5.0, 3.5));
        assert_eq!(
            candidates[0].exit,
            Some(Point::new(1.0, 0.0)),
            "row direction"
        );
        assert_eq!(
            candidates[1].exit,
            Some(Point::new(0.0, -1.0)),
            "nearest side"
        );
        assert_eq!(candidates[2].point, Point::new(4.0, 4.0));
        assert_eq!(
            candidates[2].exit,
            Some(Point::new(0.0, 1.0)),
            "zero direction"
        );
    }

    #[test]
    fn resolve_pair_takes_the_nearest_pair_and_lower_index_on_a_tie() {
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
        assert_eq!(resolve_pair(&tied, &tied), Some((0, 0)));
        assert_eq!(resolve_pair(&[], &b), None);
        assert_eq!(resolve_pair(&a, &[]), None);
    }

    #[test]
    fn point_glue_lands_on_the_row_point() {
        let mut target = box_target();
        target.points = vec![(3, Point::new(1.0, 0.5), None)];
        let begin = EndSpec::Glued(&target, GlueEnd::Point(3));
        let end = EndSpec::Free(Point::new(9.0, 3.0));
        let (begin, end, missing) = resolve_ends(&begin, &end);
        assert_eq!(begin.point, Point::new(4.0, 3.5));
        assert_eq!(begin.exit, Some(Point::new(0.0, -1.0)), "the nearest side");
        assert_eq!(end.point, Point::new(9.0, 3.0));
        assert_eq!(end.exit, None);
        assert!(!missing);
    }

    #[test]
    fn a_missing_point_row_falls_back_to_dynamic_glue_and_is_reported() {
        let target = box_target();
        let begin = EndSpec::Glued(&target, GlueEnd::Point(5));
        let end = EndSpec::Free(Point::new(3.0, 3.0));
        let (begin, _end, missing) = resolve_ends(&begin, &end);
        assert!(missing);
        assert_eq!(
            begin.point,
            Point::new(3.0, 3.5),
            "the left midpoint is nearest"
        );
    }

    #[test]
    fn one_dynamic_end_takes_the_candidate_nearest_the_fixed_end() {
        let target = box_target();
        let begin = EndSpec::Glued(&target, GlueEnd::Shape);
        let end = EndSpec::Free(Point::new(4.0, 10.0));
        let (begin, _end, missing) = resolve_ends(&begin, &end);
        assert!(!missing);
        assert_eq!(begin.point, Point::new(4.0, 4.0), "the top midpoint");
    }

    #[test]
    fn two_dynamic_ends_take_the_nearest_pair() {
        let left = box_target();
        let mut right = box_target();
        right.transform = Transform::translate(Point::new(7.0, 3.0));
        let begin = EndSpec::Glued(&left, GlueEnd::Shape);
        let end = EndSpec::Glued(&right, GlueEnd::Shape);
        let (begin, end, missing) = resolve_ends(&begin, &end);
        assert!(!missing);
        assert_eq!(
            begin.point,
            Point::new(5.0, 3.5),
            "the facing right midpoint"
        );
        assert_eq!(end.point, Point::new(7.0, 3.5), "the facing left midpoint");
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
    fn a_non_finite_distance_counts_as_infinite() {
        assert_eq!(distance_or_infinite(Point::new(3.0, 4.0)), 5.0);
        assert_eq!(
            distance_or_infinite(Point::new(f64::NAN, 1.0)),
            f64::INFINITY
        );
    }

    #[test]
    fn non_finite_input_falls_back_to_the_first_candidate() {
        let target = box_target();
        // A missing row against a non-finite free point: dynamic glue
        // falls back to the target's first candidate.
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

        let dynamic = EndSpec::Glued(&target, GlueEnd::Shape);
        let free = EndSpec::Free(Point::new(0.0, f64::INFINITY));
        let (begin, _end, _) = resolve_ends(&dynamic, &free);
        assert_eq!(
            begin.point,
            Point::new(4.0, 4.0),
            "a non-finite reference picks the first"
        );
    }
}
