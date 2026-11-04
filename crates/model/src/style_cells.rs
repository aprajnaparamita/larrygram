//! The connector style cells (ADR 0084 rule 8): the cell names and
//! the values the alpha reads and writes.
//!
//! A style choice is one pair of cells, on the connector or on the
//! page sheet:
//!
//! | Choice | `ShapeRouteStyle` | `ConLineRouteExt` |
//! |---|---|---|
//! | Right Angle | 1 | 1 |
//! | Straight Lines | 2 | 1 |
//! | Curved Lines | 1 | 2 |
//!
//! [`connector_style`] reads a connector's style from its own cells,
//! then the page's, then right angle; [`style_values`] gives the pair
//! a choice writes. A value that is only read is never rewritten.
//!
//! Part of Larrygram. Alpha.

use larrygram_geom::RouteStyle;

/// The connector's own route style (Shape Layout section): 0 or
/// missing means the page default (ADR 0084 rule 8).
pub const SHAPE_ROUTE_STYLE: &str = "ShapeRouteStyle";

/// The connector's own straight/curved switch (Shape Layout section):
/// 0 or missing means the page default.
pub const CON_LINE_ROUTE_EXT: &str = "ConLineRouteExt";

/// The page's default route style (Page Layout section).
pub const PAGE_ROUTE_STYLE: &str = "RouteStyle";

/// The page's default straight/curved switch (Page Layout section).
pub const PAGE_LINE_ROUTE_EXT: &str = "LineRouteExt";

/// Maps the shape's and the page's cell values (`None` when missing)
/// to the connector's style (ADR 0084 rule 8).
///
/// Each cell falls through from the shape to the page when it is
/// missing or 0, and a missing page value means right angle. A
/// `ShapeRouteStyle` of 2 is straight lines; 1 with a
/// `ConLineRouteExt` of 2 is curved lines; 1 with anything else is
/// right angle. Value 16, the directed values (3–15, 17–22) and
/// unknown values route as right angle in the alpha (open questions
/// 11 and 12).
pub fn connector_style(
    shape_route_style: Option<f64>,
    shape_line_route_ext: Option<f64>,
    page_route_style: Option<f64>,
    page_line_route_ext: Option<f64>,
) -> RouteStyle {
    let route_style = nonzero(shape_route_style).or_else(|| nonzero(page_route_style));
    let line_route_ext = nonzero(shape_line_route_ext).or_else(|| nonzero(page_line_route_ext));
    match route_style {
        Some(2.0) => RouteStyle::Straight,
        Some(1.0) if line_route_ext == Some(2.0) => RouteStyle::Curved,
        _ => RouteStyle::RightAngle,
    }
}

/// The (`ShapeRouteStyle`, `ConLineRouteExt`) values a choice writes
/// (ADR 0084 rule 8).
pub fn style_values(style: RouteStyle) -> (f64, f64) {
    match style {
        RouteStyle::Straight => (2.0, 1.0),
        RouteStyle::Curved => (1.0, 2.0),
        _ => (1.0, 1.0),
    }
}

/// The finite non-zero cell value: 0 and missing fall through to the
/// page (ADR 0084 rule 8), and a non-number counts as missing.
fn nonzero(value: Option<f64>) -> Option<f64> {
    match value {
        Some(value) if value.is_finite() && value != 0.0 => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three choices write the documented pairs and read back.
    #[test]
    fn the_table_round_trips() {
        for style in [
            RouteStyle::RightAngle,
            RouteStyle::Straight,
            RouteStyle::Curved,
        ] {
            let (route, ext) = style_values(style);
            assert_eq!(
                connector_style(Some(route), Some(ext), Some(9.0), Some(9.0)),
                style,
                "{style:?}"
            );
        }
        assert_eq!(style_values(RouteStyle::RightAngle), (1.0, 1.0));
        assert_eq!(style_values(RouteStyle::Straight), (2.0, 1.0));
        assert_eq!(style_values(RouteStyle::Curved), (1.0, 2.0));
    }

    /// The shape's cells win over the page's.
    #[test]
    fn the_shape_cells_come_first() {
        assert_eq!(
            connector_style(Some(2.0), Some(1.0), Some(1.0), Some(2.0)),
            RouteStyle::Straight
        );
        assert_eq!(
            connector_style(Some(1.0), Some(2.0), Some(2.0), Some(1.0)),
            RouteStyle::Curved
        );
    }

    /// A missing or 0 shape value falls through to the page.
    #[test]
    fn missing_and_zero_fall_through_to_the_page() {
        for missing in [None, Some(0.0)] {
            assert_eq!(
                connector_style(missing, missing, Some(2.0), Some(1.0)),
                RouteStyle::Straight
            );
            assert_eq!(
                connector_style(missing, missing, Some(1.0), Some(2.0)),
                RouteStyle::Curved
            );
            assert_eq!(
                connector_style(missing, Some(2.0), Some(1.0), None),
                RouteStyle::Curved
            );
        }
    }

    /// With no page value the style is right angle.
    #[test]
    fn a_missing_page_value_means_right_angle() {
        assert_eq!(connector_style(None, None, None, None), RouteStyle::RightAngle);
        assert_eq!(
            connector_style(None, Some(2.0), None, None),
            RouteStyle::RightAngle
        );
        assert_eq!(
            connector_style(Some(1.0), None, None, None),
            RouteStyle::RightAngle
        );
    }

    /// Value 16, the directed values and unknown values route as
    /// right angle (ADR 0084 rule 8).
    #[test]
    fn sixteen_directed_and_unknown_values_route_as_right_angle() {
        for value in [16.0, 3.0, 6.0, 15.0, 17.0, 22.0, -1.0, 99.0, f64::INFINITY, f64::NAN] {
            assert_eq!(
                connector_style(Some(value), Some(2.0), None, None),
                RouteStyle::RightAngle,
                "{value}"
            );
        }
    }

    /// A non-number cell value counts as missing.
    #[test]
    fn non_finite_values_count_as_missing() {
        assert_eq!(
            connector_style(Some(f64::NAN), None, Some(2.0), Some(1.0)),
            RouteStyle::Straight
        );
        assert_eq!(
            connector_style(Some(f64::INFINITY), None, Some(2.0), Some(1.0)),
            RouteStyle::Straight
        );
    }
}
