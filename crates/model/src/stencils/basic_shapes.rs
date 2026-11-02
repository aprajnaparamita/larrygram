//! The "Basic Shapes" stencil: 12 masters of our own design (ADR 0034).
//!
//! Every coordinate is ours: the shapes are simple polygons, one
//! ellipse, one arc and one line, authored on the unit box scaled to the
//! master's size. No shape is traced, copied or derived from Microsoft
//! material (AGENTS.md section 4).

use super::{
    FillDefaults, GeometryRow, LineDefaults, MasterDef, closed_polygon, edge_midpoints,
    regular_polygon, rounded_rect, star_points,
};

/// A master with our default line (ADR 0023) and two Shape Data rows.
fn master(
    name_u: &'static str,
    name: &'static str,
    width: f64,
    height: f64,
    geometry: Vec<GeometryRow>,
    fill: FillDefaults,
    connection_points: Vec<(f64, f64)>,
) -> MasterDef {
    MasterDef {
        name_u,
        name,
        width,
        height,
        geometry,
        line: LineDefaults::default(),
        fill,
        shape_data: vec![("Label", name), ("Cost", "0")],
        connection_points,
        connector_1d: false,
    }
}

/// The "Basic Shapes" masters, in Shapes window order.
pub(super) fn masters() -> Vec<MasterDef> {
    let filled = FillDefaults::default();
    let stroked = FillDefaults {
        pattern: 0,
        ..FillDefaults::default()
    };
    vec![
        master(
            "Rectangle",
            "Rectangle",
            1.5,
            1.0,
            closed_polygon(&[(0.0, 0.0), (1.5, 0.0), (1.5, 1.0), (0.0, 1.0)]),
            filled,
            edge_midpoints(1.5, 1.0),
        ),
        master(
            "RoundedRectangle",
            "Rounded Rectangle",
            1.5,
            1.0,
            rounded_rect(1.5, 1.0, 0.15),
            filled,
            edge_midpoints(1.5, 1.0),
        ),
        master(
            "Ellipse",
            "Ellipse",
            1.5,
            1.0,
            vec![GeometryRow::Ellipse {
                center: larrygram_geom::Point::new(0.75, 0.5),
                a: larrygram_geom::Point::new(0.75, 0.0),
                b: larrygram_geom::Point::new(0.0, 0.5),
            }],
            filled,
            edge_midpoints(1.5, 1.0),
        ),
        master(
            "Triangle",
            "Triangle",
            1.2,
            1.0,
            closed_polygon(&[(0.0, 0.0), (1.2, 0.0), (0.6, 1.0)]),
            filled,
            edge_midpoints(1.2, 1.0),
        ),
        master(
            "RightTriangle",
            "Right Triangle",
            1.2,
            1.0,
            closed_polygon(&[(0.0, 0.0), (1.2, 0.0), (0.0, 1.0)]),
            filled,
            edge_midpoints(1.2, 1.0),
        ),
        master(
            "Diamond",
            "Diamond",
            1.2,
            1.0,
            closed_polygon(&[(0.6, 0.0), (1.2, 0.5), (0.6, 1.0), (0.0, 0.5)]),
            filled,
            edge_midpoints(1.2, 1.0),
        ),
        master(
            "Pentagon",
            "Pentagon",
            1.2,
            1.2,
            closed_polygon(&regular_polygon(5, 1.2, 1.2, std::f64::consts::FRAC_PI_2)),
            filled,
            edge_midpoints(1.2, 1.2),
        ),
        master(
            "Hexagon",
            "Hexagon",
            1.2,
            1.2,
            closed_polygon(&regular_polygon(6, 1.2, 1.2, 0.0)),
            filled,
            edge_midpoints(1.2, 1.2),
        ),
        master(
            "Octagon",
            "Octagon",
            1.2,
            1.2,
            closed_polygon(&regular_polygon(8, 1.2, 1.2, std::f64::consts::FRAC_PI_8)),
            filled,
            edge_midpoints(1.2, 1.2),
        ),
        master(
            "Star5",
            "Star",
            1.2,
            1.2,
            closed_polygon(&star_points(
                5,
                1.2,
                1.2,
                0.382,
                std::f64::consts::FRAC_PI_2,
            )),
            filled,
            edge_midpoints(1.2, 1.2),
        ),
        master(
            "Arc",
            "Arc",
            1.2,
            0.6,
            vec![
                GeometryRow::MoveTo { x: 0.0, y: 0.0 },
                GeometryRow::ArcTo {
                    x: 1.2,
                    y: 0.0,
                    bow: 0.6,
                },
            ],
            stroked,
            edge_midpoints(1.2, 0.6),
        ),
        master(
            "Line",
            "Line",
            1.2,
            0.6,
            vec![
                GeometryRow::MoveTo { x: 0.0, y: 0.0 },
                GeometryRow::LineTo { x: 1.2, y: 0.6 },
            ],
            stroked,
            vec![(0.0, 0.0), (1.2, 0.6)],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stencil_has_twelve_masters() {
        let masters = masters();
        assert_eq!(masters.len(), 12);
        assert_eq!(masters[0].name_u, "Rectangle");
        assert_eq!(masters[11].name_u, "Line");
    }

    #[test]
    fn the_line_like_masters_are_not_filled() {
        let masters = masters();
        for name in ["Arc", "Line"] {
            let master = masters
                .iter()
                .find(|master| master.name_u == name)
                .expect("master");
            assert_eq!(master.fill.pattern, 0, "{name} is not filled");
        }
    }
}
