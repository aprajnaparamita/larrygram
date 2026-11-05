//! The Backgrounds gallery's designs (task P7-54): what the
//! `tools.backgrounds` command draws on a background page — a solid
//! tint, a soft vertical gradient and a light grid. Our own designs
//! and our own names (AGENTS.md section 4: no Visio background
//! designs, names or art), every design drawn as shapes with no asset
//! files (the task's master note).
//!
//! A design owns the background page named after it
//! ([`BackgroundDesign::page_name`]): the page is created on first
//! use and reused later. Every shape's colour derives from the one
//! design colour ([`design_colors`]), so the Background Color submenu
//! (`tools.background.color`) recolours the design in place by
//! rewriting `FillForegnd` down the design's shapes.

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Color, Preserved, Row, Section, SectionName, Shape, ShapeId,
};

/// One Backgrounds gallery design (task P7-54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundDesign {
    /// The gallery label ("Solid Tint").
    pub name: &'static str,
    /// The background page that holds the design ("Background Tint").
    pub page_name: &'static str,
    /// The colour the design draws with until Background Color
    /// changes it.
    pub default_color: Color,
}

/// The Backgrounds gallery's designs, in gallery order. "No
/// Background" is the gallery's clearing row and not a design.
pub const DESIGNS: &[BackgroundDesign] = &[
    BackgroundDesign {
        name: "Solid Tint",
        page_name: "Background Tint",
        default_color: Color {
            r: 0xd6,
            g: 0xe4,
            b: 0xf0,
        },
    },
    BackgroundDesign {
        name: "Soft Gradient",
        page_name: "Background Gradient",
        default_color: Color {
            r: 0x7f,
            g: 0xb2,
            b: 0xe5,
        },
    },
    BackgroundDesign {
        name: "Light Grid",
        page_name: "Background Grid",
        default_color: Color {
            r: 0x9d,
            g: 0xc3,
            b: 0xe6,
        },
    },
];

/// The design whose gallery label is `text` (ASCII case-insensitive,
/// `format.fill`'s name rule).
pub(crate) fn design_for_label(text: &str) -> Option<&'static BackgroundDesign> {
    let key = text.trim().to_ascii_lowercase();
    DESIGNS
        .iter()
        .find(|design| design.name.to_ascii_lowercase() == key)
}

/// The design the background page named `name` holds.
pub(crate) fn design_for_page(name: &str) -> Option<&'static BackgroundDesign> {
    DESIGNS
        .iter()
        .find(|design| design.page_name.eq_ignore_ascii_case(name.trim()))
}

/// The gradient's band count (the "soft" rule: 24 bands).
const BANDS: usize = 24;

/// The grid line thickness, in inches.
const GRID_LINE: f64 = 0.02;

/// One design rectangle: `(x, y, width, height)` in page inches.
type Rect = (f64, f64, f64, f64);

/// The design's rectangles and their colours, in drawing order (the
/// one table the generator and the Background Color recolour share:
/// shape `i` gets `design_colors(...)[i]`).
fn design_parts(
    design: &BackgroundDesign,
    color: Color,
    (width, height): (f64, f64),
) -> Vec<(Rect, Color)> {
    match design.name {
        // The solid tint: one full-page rectangle.
        "Solid Tint" => vec![((0.0, 0.0, width, height), color)],
        // The soft vertical gradient: 24 horizontal bands from the
        // colour at the top to white at the bottom.
        "Soft Gradient" => {
            let band = height / BANDS as f64;
            (0..BANDS)
                .map(|index| {
                    let t = index as f64 / (BANDS - 1) as f64;
                    let y = height - (index as f64 + 1.0) * band;
                    ((0.0, y, width, band), mix(color, WHITE, t))
                })
                .collect()
        }
        // The light grid: a near-white field over integer-inch grid
        // lines, first the verticals then the horizontals.
        _ => {
            let mut parts = vec![((0.0, 0.0, width, height), mix(color, WHITE, 0.82))];
            let mut x = 1.0;
            while x < width - GRID_LINE {
                parts.push((
                    (x - GRID_LINE / 2.0, 0.0, GRID_LINE, height),
                    mix(color, WHITE, 0.45),
                ));
                x += 1.0;
            }
            let mut y = 1.0;
            while y < height - GRID_LINE {
                parts.push((
                    (0.0, y - GRID_LINE / 2.0, width, GRID_LINE),
                    mix(color, WHITE, 0.45),
                ));
                y += 1.0;
            }
            parts
        }
    }
}

/// The colour of every shape of the design, in drawing order (the
/// Background Color recolour's table).
pub(crate) fn design_colors(
    design: &BackgroundDesign,
    color: Color,
    size: (f64, f64),
) -> Vec<Color> {
    design_parts(design, color, size)
        .into_iter()
        .map(|(_, color)| color)
        .collect()
}

/// The design's shapes, in drawing order (rectangle shapes, no
/// outlines; `Shape::name` marks them "Background").
pub(crate) fn design_shapes(
    design: &BackgroundDesign,
    color: Color,
    size: (f64, f64),
) -> Vec<Shape> {
    design_parts(design, color, size)
        .into_iter()
        .map(|((x, y, width, height), color)| rect_shape(x, y, width, height, color))
        .collect()
}

/// White, the gradient's and the grid's fade target.
const WHITE: Color = Color {
    r: 0xff,
    g: 0xff,
    b: 0xff,
};

/// The `t`-interpolation between two colours (the designs' fades).
fn mix(a: Color, b: Color, t: f64) -> Color {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let blend = |from: u8, to: u8| -> u8 {
        let value = f64::from(from) + (f64::from(to) - f64::from(from)) * t;
        value.round().clamp(0.0, 255.0) as u8
    };
    Color {
        r: blend(a.r, b.r),
        g: blend(a.g, b.g),
        b: blend(a.b, b.b),
    }
}

/// One filled rectangle shape (the design's only shape kind): the
/// transform and fill cells plus the closed rectangle geometry, no
/// outline (`LinePattern` 0).
fn rect_shape(x: f64, y: f64, width: f64, height: f64, color: Color) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    shape.name = "Background".to_owned();
    shape.name_u = "Background".to_owned();
    shape.sheet.cells = vec![
        number_cell("Width", width, Unit::Inches),
        number_cell("Height", height, Unit::Inches),
        number_cell("PinX", x + width / 2.0, Unit::Inches),
        number_cell("PinY", y + height / 2.0, Unit::Inches),
        number_cell("LocPinX", width / 2.0, Unit::Inches),
        number_cell("LocPinY", height / 2.0, Unit::Inches),
        number_cell("Angle", 0.0, Unit::Radians),
        number_cell("FillPattern", 1.0, Unit::Unitsless),
        color_cell("FillForegnd", color),
        number_cell("LinePattern", 0.0, Unit::Unitsless),
    ];
    shape.sheet.sections.push(rect_geometry(width, height));
    shape
}

/// One numeric cell with its literal formula (the `insert` shapes'
/// spelling).
fn number_cell(name: &str, value: f64, unit: Unit) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(super::edit::number_literal(value)),
        value: Some(CellValue::Number(value)),
        unit,
        preserved: Preserved::new(),
    }
}

/// One colour cell with its `RGB(r,g,b)` formula (the colour
/// commands' spelling, `format::color_write`).
fn color_cell(name: &str, color: Color) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("RGB({},{},{})", color.r, color.g, color.b)),
        value: Some(CellValue::Color(color)),
        unit: Unit::Unitsless,
        preserved: Preserved::new(),
    }
}

/// The closed rectangle geometry of a `width` x `height` box (the
/// `insert` shapes' rule).
fn rect_geometry(width: f64, height: f64) -> Section {
    let mut section = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, x, y)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", width, 0.0),
        ("LineTo", width, height),
        ("LineTo", 0.0, height),
        ("LineTo", 0.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![
            number_cell("X", x, Unit::Inches),
            number_cell("Y", y, Unit::Inches),
        ];
        section.rows.push(row);
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tint of the design's default colour.
    fn tint() -> &'static BackgroundDesign {
        &DESIGNS[0]
    }

    /// The grid of the design's default colour.
    fn grid() -> &'static BackgroundDesign {
        &DESIGNS[2]
    }

    #[test]
    fn the_designs_have_our_own_names_and_pages() {
        let names: Vec<&str> = DESIGNS.iter().map(|design| design.name).collect();
        assert_eq!(names, vec!["Solid Tint", "Soft Gradient", "Light Grid"]);
        for design in DESIGNS {
            assert!(design.page_name.starts_with("Background "));
            assert_eq!(design_for_label(design.name), Some(design));
            assert_eq!(
                design_for_label(&design.name.to_ascii_lowercase()),
                Some(design),
                "the label match is case-insensitive"
            );
            assert_eq!(design_for_page(design.page_name), Some(design));
        }
        assert_eq!(design_for_label("No Background"), None);
        assert_eq!(design_for_label("Nope"), None);
        assert_eq!(design_for_page("Page-1"), None);
    }

    #[test]
    fn the_tint_is_one_full_page_rectangle() {
        let shapes = design_shapes(tint(), Color::new(1, 2, 3), (8.5, 11.0));
        assert_eq!(shapes.len(), 1);
        let cell = shapes[0]
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "PinX")
            .expect("PinX");
        assert_eq!(cell.value, Some(CellValue::Number(4.25)));
        assert_eq!(
            design_colors(tint(), Color::new(1, 2, 3), (8.5, 11.0)).len(),
            1
        );
    }

    #[test]
    fn the_gradient_fades_from_the_colour_to_white() {
        let colors = design_colors(&DESIGNS[1], Color::new(0x7f, 0xb2, 0xe5), (8.5, 11.0));
        assert_eq!(colors.len(), 24);
        assert_eq!(colors.first(), Some(&Color::new(0x7f, 0xb2, 0xe5)), "top");
        assert_eq!(colors.last(), Some(&Color::new(0xff, 0xff, 0xff)), "bottom");
    }

    #[test]
    fn the_grid_has_a_field_and_integer_inch_lines() {
        let size = (8.5, 11.0);
        let shapes = design_shapes(grid(), Color::new(0x9d, 0xc3, 0xe6), size);
        // 1 field + 8 verticals (x = 1..8) + 10 horizontals (y = 1..10).
        assert_eq!(shapes.len(), 1 + 8 + 10);
        let colors = design_colors(grid(), Color::new(0x9d, 0xc3, 0xe6), size);
        assert_eq!(colors.len(), shapes.len(), "one colour per shape");
        // The field is very light; the lines are the stronger tint.
        assert_eq!(colors[0], mix(Color::new(0x9d, 0xc3, 0xe6), WHITE, 0.82));
        assert_eq!(colors[1], mix(Color::new(0x9d, 0xc3, 0xe6), WHITE, 0.45));
    }

    #[test]
    fn mix_interpolates_and_clamps() {
        assert_eq!(mix(Color::new(0, 0, 0), WHITE, 0.0), Color::new(0, 0, 0));
        assert_eq!(mix(Color::new(0, 0, 0), WHITE, 1.0), WHITE);
        assert_eq!(
            mix(Color::new(0, 0, 0), WHITE, 0.5),
            Color::new(128, 128, 128)
        );
        assert_eq!(mix(Color::new(0, 0, 0), WHITE, 9.0), WHITE, "clamped");
        assert_eq!(
            mix(Color::new(0, 0, 0), WHITE, f64::NAN),
            Color::new(0, 0, 0),
            "a non-finite t is zero"
        );
    }

    #[test]
    fn every_shape_carries_the_colour_cell() {
        // The tint draws in the exact colour; every shape's
        // `FillForegnd` spells its colour as `RGB(r,g,b)`.
        for shape in design_shapes(tint(), Color::new(1, 2, 3), (8.5, 11.0)) {
            let cell = shape
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == "FillForegnd")
                .expect("FillForegnd");
            assert_eq!(cell.formula.as_deref(), Some("RGB(1,2,3)"));
            assert_eq!(cell.value, Some(CellValue::Color(Color::new(1, 2, 3))));
        }
    }
}
