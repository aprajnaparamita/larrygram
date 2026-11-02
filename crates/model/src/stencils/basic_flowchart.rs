//! The "Basic Flowchart" stencil: 9 masters of our own design (ADR 0034).
//!
//! The masters are the classic flowchart symbol *families* (process,
//! decision, terminator, ...) with coordinates we wrote ourselves: a
//! rectangle, a diamond, a trapezoid, a hexagon, a parallelogram, two
//! rounded rectangles, a document box with a wave bottom edge and a
//! connector line. No shape is traced, copied or derived from Microsoft
//! material (AGENTS.md section 4).

use larrygram_geom::Unit;

use crate::cell::Cell;

use super::{
    FillDefaults, GeometryRow, LineDefaults, MasterDef, closed_polygon, edge_midpoints,
    formula_cell, number_cell, rounded_rect,
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
        shape_data: vec![("Process", name), ("Cost", "0")],
        connection_points,
        connector_1d: false,
    }
}

/// The shape cells of the 1-D "Connector" master (ADR 0084 rule 1).
///
/// The structure follows what Visio's files show (`docs/ui/connectors.md`
/// section 8: the cells are structure, the master is ours): the four
/// 1-D endpoint cells, the pin, size and lock formulas, `Angle`, the
/// routable/glue kinds, the right-angle route defaults (rule 8) and the
/// two glue triggers. No help topic, no copyright string, no `Connection`
/// section: a connector is not a glue target in the alpha.
pub(super) fn connector_cells(width: f64, height: f64) -> Vec<Cell> {
    vec![
        // 1-D Endpoints: the master's own begin and end (inches).
        number_cell("BeginX", 0.0, Unit::Inches),
        number_cell("BeginY", 0.0, Unit::Inches),
        number_cell("EndX", width, Unit::Inches),
        number_cell("EndY", 0.0, Unit::Inches),
        // The pin, size and lock formulas (the cached values a straight
        // connector of `width` computes).
        formula_cell("PinX", "GUARD((BeginX+EndX)/2)", width * 0.5, Unit::Inches),
        formula_cell("PinY", "GUARD((BeginY+EndY)/2)", 0.0, Unit::Inches),
        formula_cell("Width", "GUARD(EndX-BeginX)", width, Unit::Inches),
        formula_cell("Height", "GUARD(0.2DL)", height, Unit::Inches),
        formula_cell("LocPinX", "GUARD(Width*0.5)", width * 0.5, Unit::Inches),
        formula_cell("LocPinY", "GUARD(Height*0.5)", height * 0.5, Unit::Inches),
        formula_cell("Angle", "GUARD(0DA)", 0.0, Unit::Radians),
        // A routable 1-D shape with walking glue (ADR 0084 rule 1).
        number_cell("ObjType", 2.0, Unit::Unitsless),
        number_cell("GlueType", 2.0, Unit::Unitsless),
        // Right angle routes on straight lines (ADR 0084 rule 8).
        number_cell("ShapeRouteStyle", 1.0, Unit::Unitsless),
        number_cell("ConLineRouteExt", 1.0, Unit::Unitsless),
        // The glue triggers: unglued, each names its own sheet
        // (`docs/ui/connectors.md` section 8).
        formula_cell("BegTrigger", "_XFTRIGGER(EventXFMod)", 0.0, Unit::Unitsless),
        formula_cell("EndTrigger", "_XFTRIGGER(EventXFMod)", 0.0, Unit::Unitsless),
    ]
}

/// The "Basic Flowchart" masters, in Shapes window order.
pub(super) fn masters() -> Vec<MasterDef> {
    let filled = FillDefaults::default();
    let stroked = FillDefaults {
        pattern: 0,
        ..FillDefaults::default()
    };
    vec![
        master(
            "Process",
            "Process",
            1.5,
            0.75,
            closed_polygon(&[(0.0, 0.0), (1.5, 0.0), (1.5, 0.75), (0.0, 0.75)]),
            filled,
            edge_midpoints(1.5, 0.75),
        ),
        master(
            "AlternateProcess",
            "Alternate Process",
            1.5,
            0.75,
            rounded_rect(1.5, 0.75, 0.12),
            filled,
            edge_midpoints(1.5, 0.75),
        ),
        master(
            "Decision",
            "Decision",
            1.2,
            0.9,
            closed_polygon(&[(0.6, 0.0), (1.2, 0.45), (0.6, 0.9), (0.0, 0.45)]),
            filled,
            edge_midpoints(1.2, 0.9),
        ),
        master(
            "Terminator",
            "Terminator",
            1.5,
            0.6,
            rounded_rect(1.5, 0.6, 0.3),
            filled,
            edge_midpoints(1.5, 0.6),
        ),
        master(
            "Data",
            "Data",
            1.5,
            0.75,
            closed_polygon(&[(0.3, 0.0), (1.5, 0.0), (1.2, 0.75), (0.0, 0.75)]),
            filled,
            edge_midpoints(1.5, 0.75),
        ),
        master(
            "Document",
            "Document",
            1.5,
            0.8,
            vec![
                GeometryRow::MoveTo { x: 0.0, y: 0.2 },
                GeometryRow::ArcTo {
                    x: 0.75,
                    y: 0.2,
                    bow: -0.08,
                },
                GeometryRow::ArcTo {
                    x: 1.5,
                    y: 0.2,
                    bow: 0.08,
                },
                GeometryRow::LineTo { x: 1.5, y: 0.8 },
                GeometryRow::LineTo { x: 0.0, y: 0.8 },
                GeometryRow::LineTo { x: 0.0, y: 0.2 },
            ],
            filled,
            edge_midpoints(1.5, 0.8),
        ),
        master(
            "ManualOperation",
            "Manual Operation",
            1.5,
            0.75,
            closed_polygon(&[(0.3, 0.0), (1.2, 0.0), (1.5, 0.75), (0.0, 0.75)]),
            filled,
            edge_midpoints(1.5, 0.75),
        ),
        master(
            "Preparation",
            "Preparation",
            1.5,
            0.75,
            closed_polygon(&[
                (0.3, 0.0),
                (1.2, 0.0),
                (1.5, 0.375),
                (1.2, 0.75),
                (0.3, 0.75),
                (0.0, 0.375),
            ]),
            filled,
            edge_midpoints(1.5, 0.75),
        ),
        {
            // The 1-D connector (ADR 0084 rule 1): endpoint cells and
            // glue triggers in [`connector_cells`], one straight
            // geometry path and no connection points. The path runs
            // through the middle of the 0.2 in tall box (`Height` is
            // `GUARD(0.2DL)`).
            MasterDef {
                name_u: "Connector",
                name: "Connector",
                width: 1.2,
                height: 0.2,
                geometry: vec![
                    GeometryRow::MoveTo { x: 0.0, y: 0.1 },
                    GeometryRow::LineTo { x: 1.2, y: 0.1 },
                ],
                line: LineDefaults::default(),
                fill: stroked,
                shape_data: Vec::new(),
                connection_points: Vec::new(),
                connector_1d: true,
            }
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellValue;

    #[test]
    fn the_stencil_has_nine_masters() {
        let masters = masters();
        assert_eq!(masters.len(), 9);
        assert_eq!(masters[0].name_u, "Process");
        assert_eq!(masters[8].name_u, "Connector");
    }

    #[test]
    fn the_connector_is_a_1d_master() {
        let masters = masters();
        let connector = masters
            .iter()
            .find(|master| master.name_u == "Connector")
            .expect("connector");
        assert!(connector.connector_1d, "ADR 0084 rule 1: a 1-D shape");
        assert_eq!((connector.width, connector.height), (1.2, 0.2));
        assert!(
            connector.connection_points.is_empty(),
            "a connector is not a glue target in the alpha"
        );
        assert!(connector.shape_data.is_empty());
        assert_eq!(connector.geometry.len(), 2, "one straight geometry path");
    }

    #[test]
    fn the_connector_cells_carry_the_1d_structure() {
        let cells = connector_cells(1.2, 0.2);
        let find = |name: &str| {
            cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .expect("the named cell exists")
        };
        // The endpoint cells are literal values (the master's own ends).
        assert_eq!(find("BeginX").formula, None);
        assert_eq!(find("BeginX").value, Some(CellValue::Number(0.0)));
        assert_eq!(find("EndX").value, Some(CellValue::Number(1.2)));
        assert_eq!(find("EndY").value, Some(CellValue::Number(0.0)));
        // The pin, size and lock formulas (ADR 0084 rule 1).
        assert_eq!(
            find("PinX").formula.as_deref(),
            Some("GUARD((BeginX+EndX)/2)")
        );
        assert_eq!(
            find("PinY").formula.as_deref(),
            Some("GUARD((BeginY+EndY)/2)")
        );
        assert_eq!(find("Width").formula.as_deref(), Some("GUARD(EndX-BeginX)"));
        assert_eq!(find("Height").formula.as_deref(), Some("GUARD(0.2DL)"));
        assert_eq!(find("LocPinX").formula.as_deref(), Some("GUARD(Width*0.5)"));
        assert_eq!(
            find("LocPinY").formula.as_deref(),
            Some("GUARD(Height*0.5)")
        );
        assert_eq!(find("Angle").formula.as_deref(), Some("GUARD(0DA)"));
        // The kinds and the right-angle defaults.
        assert_eq!(find("ObjType").value, Some(CellValue::Number(2.0)));
        assert_eq!(find("GlueType").value, Some(CellValue::Number(2.0)));
        assert_eq!(find("ShapeRouteStyle").value, Some(CellValue::Number(1.0)));
        assert_eq!(find("ConLineRouteExt").value, Some(CellValue::Number(1.0)));
        // The two triggers, unglued: each names its own sheet.
        assert_eq!(
            find("BegTrigger").formula.as_deref(),
            Some("_XFTRIGGER(EventXFMod)")
        );
        assert_eq!(
            find("EndTrigger").formula.as_deref(),
            Some("_XFTRIGGER(EventXFMod)")
        );
    }
}
