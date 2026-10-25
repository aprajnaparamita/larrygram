//! Typed geometry rows and paths (ADR 0019).
//!
//! The `T` attribute of a `Geometry` section row names its kind ([MS-VSDX]
//! `GeometryRow_Type`); this module types a row's cached cells into a
//! [`GeometryRow`] and a section into a [`GeometryPath`]. The `Rel*` rows
//! keep their values as fractions of the shape's width and height; the
//! scene builder multiplies (ADR 0022). `NurbsTo` and `PolylineTo` decode
//! their formula-encoded cached data (`NURBS(...)` text and the A-cell
//! point list) without evaluating any formula (Phase 6).

use larrygram_geom::{Angle, Point};

use crate::cell::CellValue;
use crate::document::Document;
use crate::error::Error;
use crate::generated::CELLS;
use crate::id::ShapeId;
use crate::lookup::{match_repeated_section, section_ix_of, walk_down};
use crate::name::CellName;
use crate::values::{CellReader, CellWarning, SheetRef};

/// One control point of a [`NurbsData`] curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NurbsDataPoint {
    /// The x coordinate, a fraction of the width (`x_type` 0) or a local
    /// coordinate (`x_type` 1).
    pub x: f64,
    /// The y coordinate, a fraction of the height (`y_type` 0) or a local
    /// coordinate (`y_type` 1).
    pub y: f64,
    /// The knot of this control point.
    pub knot: f64,
    /// The weight of this control point.
    pub weight: f64,
}

/// The decoded data of a `NURBSTo` row's E-cell `NURBS()` formula (ADR 0022).
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsData {
    /// The last knot of the formula.
    pub knot_last: f64,
    /// The degree of the spline.
    pub degree: u32,
    /// The `xType` of the formula: 0 fraction of width, 1 local.
    pub x_type: u32,
    /// The `yType` of the formula: 0 fraction of height, 1 local.
    pub y_type: u32,
    /// The control points with their knots and weights, in order.
    pub points: Vec<NurbsDataPoint>,
}

/// The decoded point list of a `PolylineTo` row's A cell (ADR 0022).
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineData {
    /// The points, in order.
    pub points: Vec<Point>,
}

/// One typed geometry row (ADR 0019).
///
/// Values are as cached in the file (canonical units, ADR 0011); the
/// `Rel*` rows hold fractions of the shape's width and height (ADR 0022).
/// Cell semantics per row are documented in ADR 0022 and the ShapeSheet
/// reference.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GeometryRow {
    /// `MoveTo`: starts a subpath at (x, y).
    MoveTo {
        /// The x coordinate.
        x: f64,
        /// The y coordinate.
        y: f64,
    },
    /// `LineTo`: a straight segment to (x, y).
    LineTo {
        /// The x coordinate.
        x: f64,
        /// The y coordinate.
        y: f64,
    },
    /// `ArcTo`: a circular arc to (x, y) with the given bow.
    ArcTo {
        /// The x coordinate of the end vertex.
        x: f64,
        /// The y coordinate of the end vertex.
        y: f64,
        /// The bow (sagitta) of the arc.
        bow: f64,
    },
    /// `EllipticalArcTo`: an elliptical arc to (x, y).
    EllipticalArcTo {
        /// The x coordinate of the end vertex.
        x: f64,
        /// The y coordinate of the end vertex.
        y: f64,
        /// The control point (A, B), a point on the arc.
        ctrl: Point,
        /// The angle of the major axis (C).
        angle: Angle,
        /// The major/minor ratio (D).
        ratio: f64,
    },
    /// `NURBSTo`: a NURBS segment ending at (x, y).
    NurbsTo {
        /// The x coordinate of the last control point.
        x: f64,
        /// The y coordinate of the last control point.
        y: f64,
        /// The second-to-last knot (A).
        second_last_knot: f64,
        /// The last weight (B).
        last_weight: f64,
        /// The first knot (C).
        first_knot: f64,
        /// The first weight (D).
        first_weight: f64,
        /// The decoded E-cell formula, when present and parsable.
        data: Option<NurbsData>,
    },
    /// `PolylineTo`: a polyline ending at (x, y).
    PolylineTo {
        /// The x coordinate of the end vertex.
        x: f64,
        /// The y coordinate of the end vertex.
        y: f64,
        /// The decoded A-cell point list, when present and parsable.
        data: Option<PolylineData>,
    },
    /// `SplineStart`: opens a B-spline; (x, y) is its second control point.
    SplineStart {
        /// The x coordinate of the second control point.
        x: f64,
        /// The y coordinate of the second control point.
        y: f64,
        /// The second knot (A).
        second_knot: f64,
        /// The first knot (B).
        first_knot: f64,
        /// The last knot (C).
        last_knot: f64,
        /// The degree (D), 1 to 25.
        degree: i32,
    },
    /// `SplineKnot`: one more control point and knot of the open spline.
    SplineKnot {
        /// The x coordinate of the control point.
        x: f64,
        /// The y coordinate of the control point.
        y: f64,
        /// The knot (A).
        knot: f64,
    },
    /// `Ellipse`: an ellipse by centre and two conjugate semi-diameter
    /// endpoints (ADR 0022).
    Ellipse {
        /// The centre (X, Y).
        center: Point,
        /// The first semi-diameter endpoint (A, B).
        a: Point,
        /// The second semi-diameter endpoint (C, D).
        b: Point,
    },
    /// `InfiniteLine`: the line through (p1, p2).
    InfiniteLine {
        /// The first point (X, Y).
        p1: Point,
        /// The second point (A, B).
        p2: Point,
    },
    /// `RelMoveTo`: `MoveTo` with coordinates as width/height fractions.
    RelMoveTo {
        /// The x fraction of the width.
        x: f64,
        /// The y fraction of the height.
        y: f64,
    },
    /// `RelLineTo`: `LineTo` with coordinates as width/height fractions.
    RelLineTo {
        /// The x fraction of the width.
        x: f64,
        /// The y fraction of the height.
        y: f64,
    },
    /// `RelQuadBezTo`: a quadratic Bézier with fraction coordinates.
    RelQuadBezTo {
        /// The x fraction of the end vertex.
        x: f64,
        /// The y fraction of the end vertex.
        y: f64,
        /// The control point (A, B) as fractions.
        ctrl: Point,
    },
    /// `RelCubBezTo`: a cubic Bézier with fraction coordinates.
    RelCubBezTo {
        /// The x fraction of the end vertex.
        x: f64,
        /// The y fraction of the end vertex.
        y: f64,
        /// The first control point (A, B) as fractions.
        ctrl1: Point,
        /// The second control point (C, D) as fractions.
        ctrl2: Point,
    },
    /// `RelEllipticalArcTo`: `EllipticalArcTo` with fraction coordinates.
    RelEllipticalArcTo {
        /// The x fraction of the end vertex.
        x: f64,
        /// The y fraction of the end vertex.
        y: f64,
        /// The control point (A, B) as fractions.
        ctrl: Point,
        /// The angle of the major axis (C).
        angle: Angle,
        /// The major/minor ratio (D).
        ratio: f64,
    },
    /// A row type this model does not map; its cells stay in the row
    /// (ADR 0019).
    Unknown {
        /// The `T` attribute as written.
        row_type: String,
    },
}

/// One typed `Geometry` section (ADR 0019).
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryPath {
    /// The rows, in file order.
    pub rows: Vec<GeometryRow>,
    /// The section's `NoFill` cell.
    pub no_fill: bool,
    /// The section's `NoLine` cell.
    pub no_line: bool,
    /// The section's `NoShow` cell.
    pub no_show: bool,
}

/// The typed geometry of one shape, with row-level inheritance resolved
/// (ADR 0019) and the cached-value policy applied (ADR 0021).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapeGeometry {
    /// The `Geometry` sections, in file order.
    pub paths: Vec<GeometryPath>,
    /// The warnings recorded while reading the rows (ADR 0021).
    pub warnings: Vec<CellWarning>,
}

impl Document {
    /// The typed geometry of one shape, with the row union resolved (ADR
    /// 0019 and its amendment): the shape's own rows merged with its master
    /// chain's rows, each row cell resolved per the rules of
    /// [`Document::resolve_row_cell`].
    ///
    /// A row's cached cells are read with the cached-value policy (ADR
    /// 0021); the recorded warnings are in [`ShapeGeometry::warnings`].
    pub fn shape_geometry(&self, shape: ShapeId) -> Result<ShapeGeometry, Error> {
        let entity = self.shape(shape).ok_or(Error::UnknownShape(shape))?;
        let mut reader = CellReader::new(self);
        let mut paths = Vec::new();
        for source in path_sources(self, entity) {
            let mut rows = Vec::new();
            for (position, merged) in source.rows.iter().enumerate() {
                let context = RowContext {
                    sheet: SheetRef::Shape(shape),
                    at: crate::lookup::RepeatedCell {
                        section: source.name,
                        section_ix: source.ix,
                        section_pos: Some(source.pos),
                        row: Some(position),
                        row_ix: merged.ix,
                    },
                };
                rows.push(type_row(&mut reader, self, &context, merged.row_type)?);
            }
            let section_context = RowContext {
                sheet: SheetRef::Shape(shape),
                at: crate::lookup::RepeatedCell {
                    section: source.name,
                    section_ix: source.ix,
                    section_pos: Some(source.pos),
                    row: None,
                    row_ix: None,
                },
            };
            let no_fill = section_context.flag(&mut reader, self, "NoFill")?;
            let no_line = section_context.flag(&mut reader, self, "NoLine")?;
            let no_show = section_context.flag(&mut reader, self, "NoShow")?;
            paths.push(GeometryPath {
                rows,
                no_fill,
                no_line,
                no_show,
            });
        }
        Ok(ShapeGeometry {
            paths,
            warnings: reader.take_warnings(),
        })
    }
}

/// One merged `Geometry` row of a path (ADR 0019 amendment): the row at
/// one position of the union of a shape's own rows and its master chain's
/// rows. The row's cells are *not* merged here: every cell resolves
/// through [`Document::resolve_row_cell`], which applies the same union
/// per cell.
struct MergedRow<'a> {
    /// The row's `IX`, when it has one.
    ix: Option<i32>,
    /// The row type (`Row/@T`) of the nearest rung that declares one.
    row_type: Option<&'a str>,
}

/// One `Geometry` section that defines a path: the section identity every
/// row cell resolves with (ADR 0019 decision 5) and the merged rows to
/// type.
struct PathSource<'a> {
    /// The section name as written (`"Geometry"`).
    name: &'a str,
    /// The section's `IX`, when it has one.
    ix: Option<i32>,
    /// The section's position among the same-named sections of its sheet.
    pos: usize,
    /// The rows to type: the union of the shape's own rows and its master
    /// chain's rows (ADR 0019 amendment).
    rows: Vec<MergedRow<'a>>,
}

/// The `Geometry` sections of one sheet, with their position among the
/// same-named sections.
fn geometry_sections(entity: &crate::shape::Shape) -> Vec<(usize, &crate::section::Section)> {
    let mut sections = Vec::new();
    let mut position = 0;
    for section in &entity.sheet.sections {
        if section.name.as_str() == "Geometry" {
            sections.push((position, section));
            position += 1;
        }
    }
    sections
}

/// The sections whose rows define one shape's geometry (ADR 0019
/// amendment): one path per `Geometry` section, its rows the union of the
/// shape's own rows and every master rung's rows of the matching section.
///
/// A shape without `Geometry` sections of its own takes the section
/// identities from the nearest master rung that carries rows; the rows of
/// the rungs further out still merge in.
fn path_sources<'a>(doc: &'a Document, entity: &'a crate::shape::Shape) -> Vec<PathSource<'a>> {
    let chain = master_chain_shapes(doc, entity.id);
    let mut sheets: Vec<&'a crate::sheet::Sheet> = Vec::with_capacity(chain.len() + 1);
    sheets.push(&entity.sheet);
    sheets.extend(chain.iter().map(|target| &target.sheet));
    let own = geometry_sections(entity);
    let identities: Vec<(usize, &crate::section::Section)> = if !own.is_empty() {
        own
    } else {
        let Some(target) = chain.iter().copied().find(|target| {
            geometry_sections(target)
                .iter()
                .any(|(_, section)| !section.rows.is_empty())
        }) else {
            return Vec::new();
        };
        geometry_sections(target)
    };
    identities
        .into_iter()
        .map(|(pos, section)| {
            let ix = section_ix_of(section);
            PathSource {
                name: section.name.as_str(),
                ix,
                pos,
                rows: merged_rows(&sheets, section.name.as_str(), ix, pos),
            }
        })
        .collect()
}

/// The merged rows of one `Geometry` section identity (ADR 0019
/// amendment): the rows of the master chain's matching sections form the
/// base (the furthest rung first), and each nearer rung — the shape's own
/// section last — applies its rows on top.
fn merged_rows<'a>(
    sheets: &[&'a crate::sheet::Sheet],
    name: &str,
    ix: Option<i32>,
    pos: usize,
) -> Vec<MergedRow<'a>> {
    let mut merged: Vec<MergedRow<'a>> = Vec::new();
    for sheet in sheets.iter().rev() {
        if let Some(section) = match_repeated_section(sheet, name, ix, Some(pos)) {
            apply_rows(&mut merged, &section.rows);
        }
    }
    merged
}

/// Applies one rung's rows to the merged rows of the rungs further out
/// (ADR 0019 amendment).
///
/// A row overrides the merged row it matches — by `IX` when both sides
/// have one, else by position (the rule of [`Document::resolve_row_cell`])
/// — and takes over its `T` when it declares one. A row with `Del`
/// ([`crate::section::Row::is_deleted`]) removes the matched row. An
/// unmatched row is inserted in `IX` order, or at its own position when it
/// has no `IX`.
fn apply_rows<'a>(merged: &mut Vec<MergedRow<'a>>, rows: &'a [crate::section::Row]) {
    for (position, row) in rows.iter().enumerate() {
        let matched = match row.ix {
            Some(ix) => merged
                .iter()
                .position(|entry| entry.ix == Some(ix))
                .or_else(|| {
                    merged
                        .get(position)
                        .filter(|entry| entry.ix.is_none())
                        .map(|_| position)
                }),
            None => merged.get(position).map(|_| position),
        };
        match matched {
            Some(at) if row.is_deleted() => {
                merged.remove(at);
            }
            Some(at) => {
                if row.row_type.is_some() {
                    merged[at].row_type = row.row_type.as_deref();
                }
            }
            None if row.is_deleted() => {}
            None => {
                let entry = MergedRow {
                    ix: row.ix,
                    row_type: row.row_type.as_deref(),
                };
                if merged.is_empty() {
                    merged.push(entry);
                    continue;
                }
                let at = match row.ix {
                    Some(ix) => merged
                        .iter()
                        .position(|entry| entry.ix.is_some_and(|other| other > ix))
                        .unwrap_or(merged.len()),
                    None => position.min(merged.len()),
                };
                merged.insert(at, entry);
            }
        }
    }
}

/// The master-chain shapes whose `Geometry` sections may carry rows for
/// `shape`, nearest rung first. The walk is the one of the lookup (ADR
/// 0013): the master of the nearest shape that is an instance, its shape
/// at the instance root's position, the sub-shape path down to `shape`,
/// and the same again when that master shape is itself an instance. The
/// walk is bounded by the visited set.
fn master_chain_shapes(doc: &Document, shape: ShapeId) -> Vec<&crate::shape::Shape> {
    let mut targets = Vec::new();
    let mut visited: Vec<crate::id::MasterId> = Vec::new();
    let Some((mut current, root, mut path)) = doc.instance_chain_start(shape) else {
        return targets;
    };
    let position = doc.sibling_position(root);
    loop {
        if visited.contains(&current) {
            break;
        }
        visited.push(current);
        let mut chained = None;
        if let Some(content) = doc
            .master(current)
            .and_then(|m| doc.master_shape(m, position))
        {
            let target = walk_down(doc, content, &path).unwrap_or(content);
            targets.push(target);
            chained = target.master;
            path.clear();
        }
        match chained {
            Some(next) => current = next,
            None => break,
        }
    }
    targets
}

/// The sheet and section/row identity one cell is read with (ADR 0019
/// decision 5): every cell of the row resolves through it.
struct RowContext<'a> {
    /// The sheet the row is read from.
    sheet: SheetRef,
    /// The section and row identity.
    at: crate::lookup::RepeatedCell<'a>,
}

impl RowContext<'_> {
    /// Resolves one cell of the row (ADR 0019 decision 5), with the
    /// ADR 0054/0057 snapshot rule: the row's own cached value (an `Inh`
    /// snapshot included) is what the reader returns.
    fn resolve<'d>(
        &self,
        doc: &'d Document,
        cell: &CellName,
    ) -> Result<crate::lookup::Resolved<'d>, Error> {
        let mut resolved = doc.resolve_repeated_with_table(self.sheet, &self.at, cell, CELLS)?;
        crate::values::prefer_row(doc, self.sheet, &self.at, cell, &mut resolved);
        Ok(resolved)
    }

    /// Reads one number of the row with the cached-value policy (ADR 0021).
    fn number(
        &self,
        reader: &mut CellReader<'_>,
        doc: &Document,
        name: &str,
    ) -> Result<f64, Error> {
        let cell = CellName::new(name);
        let resolved = self.resolve(doc, &cell)?;
        Ok(reader.number_from(self.sheet, &cell, resolved))
    }

    /// Reads one optional number of the row: a cell nothing defines gives
    /// the row default `default` without a warning (ADR 0022).
    fn optional_number(
        &self,
        reader: &mut CellReader<'_>,
        doc: &Document,
        name: &str,
        default: f64,
    ) -> Result<f64, Error> {
        let cell = CellName::new(name);
        let resolved = self.resolve(doc, &cell)?;
        Ok(reader.number_from_or(self.sheet, &cell, resolved, default))
    }

    /// Reads one section-level boolean (`NoFill` and friends, `row: None`).
    /// A section without the cell keeps the rendering default `false`,
    /// without a warning: the flags are switches every missing section
    /// would otherwise warn about three times per path.
    fn flag(&self, reader: &mut CellReader<'_>, doc: &Document, name: &str) -> Result<bool, Error> {
        let cell = CellName::new(name);
        let resolved = self.resolve(doc, &cell)?;
        Ok(reader.boolean_from_or(self.sheet, &cell, resolved, false))
    }

    /// The formula-encoded cached data of one cell: its formula text first,
    /// then its cached string value (ADR 0019 decision 2).
    fn encoded(
        &self,
        doc: &Document,
        name: &str,
    ) -> Result<(Option<String>, Option<String>), Error> {
        let cell = CellName::new(name);
        let resolved = self.resolve(doc, &cell)?;
        let formula = resolved.formula.map(str::to_owned);
        let value = match resolved.value {
            Some(CellValue::Str(text)) => Some(text.clone()),
            _ => None,
        };
        Ok((formula, value))
    }
}

/// Types one `Geometry` row from its cached cells (ADR 0019, ADR 0022).
fn type_row(
    reader: &mut CellReader<'_>,
    doc: &Document,
    context: &RowContext<'_>,
    row_type: Option<&str>,
) -> Result<GeometryRow, Error> {
    let x = |reader: &mut CellReader<'_>| context.number(reader, doc, "X");
    let y = |reader: &mut CellReader<'_>| context.number(reader, doc, "Y");
    let a = |reader: &mut CellReader<'_>| context.number(reader, doc, "A");
    let b = |reader: &mut CellReader<'_>| context.number(reader, doc, "B");
    let c = |reader: &mut CellReader<'_>| context.number(reader, doc, "C");
    let d = |reader: &mut CellReader<'_>| context.number(reader, doc, "D");
    Ok(match row_type.unwrap_or("") {
        "MoveTo" => GeometryRow::MoveTo {
            x: x(reader)?,
            y: y(reader)?,
        },
        "LineTo" => GeometryRow::LineTo {
            x: x(reader)?,
            y: y(reader)?,
        },
        "ArcTo" => GeometryRow::ArcTo {
            x: x(reader)?,
            y: y(reader)?,
            bow: a(reader)?,
        },
        "EllipticalArcTo" => GeometryRow::EllipticalArcTo {
            x: x(reader)?,
            y: y(reader)?,
            ctrl: Point::new(a(reader)?, b(reader)?),
            angle: Angle::from_radians(context.optional_number(reader, doc, "C", 0.0)?),
            ratio: context.optional_number(reader, doc, "D", 1.0)?,
        },
        "NURBSTo" | "NurbsTo" => {
            let (formula, value) = context.encoded(doc, "E")?;
            let data = formula
                .as_deref()
                .and_then(parse_nurbs_data)
                .or_else(|| value.as_deref().and_then(parse_nurbs_data));
            GeometryRow::NurbsTo {
                x: x(reader)?,
                y: y(reader)?,
                second_last_knot: a(reader)?,
                last_weight: b(reader)?,
                first_knot: c(reader)?,
                first_weight: d(reader)?,
                data,
            }
        }
        "PolylineTo" => {
            let (formula, value) = context.encoded(doc, "A")?;
            let data = formula
                .as_deref()
                .and_then(parse_polyline_data)
                .or_else(|| value.as_deref().and_then(parse_polyline_data));
            GeometryRow::PolylineTo {
                x: x(reader)?,
                y: y(reader)?,
                data,
            }
        }
        "SplineStart" => GeometryRow::SplineStart {
            x: x(reader)?,
            y: y(reader)?,
            second_knot: a(reader)?,
            first_knot: b(reader)?,
            last_knot: c(reader)?,
            degree: d(reader)? as i32,
        },
        "SplineKnot" => GeometryRow::SplineKnot {
            x: x(reader)?,
            y: y(reader)?,
            knot: a(reader)?,
        },
        "Ellipse" => GeometryRow::Ellipse {
            center: Point::new(x(reader)?, y(reader)?),
            a: Point::new(a(reader)?, b(reader)?),
            b: Point::new(c(reader)?, d(reader)?),
        },
        "InfiniteLine" => GeometryRow::InfiniteLine {
            p1: Point::new(x(reader)?, y(reader)?),
            p2: Point::new(a(reader)?, b(reader)?),
        },
        "RelMoveTo" => GeometryRow::RelMoveTo {
            x: x(reader)?,
            y: y(reader)?,
        },
        "RelLineTo" => GeometryRow::RelLineTo {
            x: x(reader)?,
            y: y(reader)?,
        },
        "RelQuadBezTo" => GeometryRow::RelQuadBezTo {
            x: x(reader)?,
            y: y(reader)?,
            ctrl: Point::new(a(reader)?, b(reader)?),
        },
        "RelCubBezTo" => GeometryRow::RelCubBezTo {
            x: x(reader)?,
            y: y(reader)?,
            ctrl1: Point::new(a(reader)?, b(reader)?),
            ctrl2: Point::new(c(reader)?, d(reader)?),
        },
        "RelEllipticalArcTo" => GeometryRow::RelEllipticalArcTo {
            x: x(reader)?,
            y: y(reader)?,
            ctrl: Point::new(a(reader)?, b(reader)?),
            angle: Angle::from_radians(context.optional_number(reader, doc, "C", 0.0)?),
            ratio: context.optional_number(reader, doc, "D", 1.0)?,
        },
        other => GeometryRow::Unknown {
            row_type: other.to_owned(),
        },
    })
}

/// The inner argument text of a `NAME(...)` formula-encoded value, or `None`
/// when the text is not a call of `name` (case-insensitive).
fn function_args(text: &str, name: &str) -> Option<String> {
    let trimmed = text.trim().strip_prefix('=').unwrap_or(text).trim();
    let (head, rest) = trimmed.split_once('(')?;
    if !head.trim().eq_ignore_ascii_case(name) {
        return None;
    }
    Some(rest.trim().strip_suffix(')')?.trim().to_owned())
}

/// The comma-separated arguments of a formula-encoded value, with one layer
/// of quotes removed from each.
fn split_args(inner: &str) -> Vec<String> {
    inner
        .split(',')
        .map(|argument| crate::lookup::unquote(argument.trim()))
        .collect()
}

/// Parses one numeric argument of a formula-encoded value.
fn number_arg(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok()
}

/// Parses one unsigned integer argument (`degree`, `xType`, `yType`).
fn index_arg(text: &str) -> Option<u32> {
    let value = number_arg(text)?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64 {
        return None;
    }
    Some(value as u32)
}

/// Parses the E cell's `NURBS(knotLast, degree, xType, yType, x1, y1,
/// knot1, weight1, ...)` formula text into [`NurbsData`] (ADR 0019
/// decision 2). Returns `None` when the text does not parse; the row then
/// degrades to its `X`, `Y`, `A`..`D` cells (ADR 0022).
fn parse_nurbs_data(text: &str) -> Option<NurbsData> {
    let inner = function_args(text, "NURBS")?;
    let args = split_args(&inner);
    // knotLast, degree, xType, yType, then x/y/knot/weight groups.
    if args.len() < 8 || !(args.len() - 4).is_multiple_of(4) {
        return None;
    }
    let knot_last = number_arg(args.first()?)?;
    let degree = index_arg(args.get(1)?)?;
    let x_type = index_arg(args.get(2)?)?;
    let y_type = index_arg(args.get(3)?)?;
    let mut points = Vec::new();
    for group in args.get(4..)?.chunks_exact(4) {
        points.push(NurbsDataPoint {
            x: number_arg(group.first()?)?,
            y: number_arg(group.get(1)?)?,
            knot: number_arg(group.get(2)?)?,
            weight: number_arg(group.get(3)?)?,
        });
    }
    Some(NurbsData {
        knot_last,
        degree,
        x_type,
        y_type,
        points,
    })
}

/// Parses the A cell's `POLYLINE(xType, yType, x1, y1, ...)` formula text —
/// or a bare point list — into [`PolylineData`] (ADR 0019 decision 2).
/// Returns `None` when the text does not parse; the row then degrades to
/// its `X`/`Y` cells (ADR 0022).
///
/// `PolylineData` keeps the points as written: the formula's `xType` and
/// `yType` flags (fraction of width/height vs local coordinates) have no
/// slot in the type (ADR 0019) and are dropped here.
fn parse_polyline_data(text: &str) -> Option<PolylineData> {
    if let Some(inner) = function_args(text, "POLYLINE") {
        let args = split_args(&inner);
        return points_from_tokens(args.get(2..)?);
    }
    let tokens: Vec<String> = text
        .split([';', ',', '\t', '\n', '\r', ' '])
        .filter(|token| !token.trim().is_empty())
        .map(|token| token.to_owned())
        .collect();
    points_from_tokens(&tokens)
}

/// The points of a flat coordinate token list: `x1 y1 x2 y2 ...`.
fn points_from_tokens(tokens: &[String]) -> Option<PolylineData> {
    if tokens.len() < 2 || !tokens.len().is_multiple_of(2) {
        return None;
    }
    let mut points = Vec::new();
    // `chunks_exact(2)` guarantees both indices (AGENTS.md section 6
    // allows direct indexing in `model`; the parser crates deny it).
    for pair in tokens.chunks_exact(2) {
        points.push(Point::new(number_arg(&pair[0])?, number_arg(&pair[1])?));
    }
    Some(PolylineData { points })
}

#[cfg(test)]
mod tests {
    use crate::cell::{Cell, CellValue};
    use crate::document::Document;
    use crate::id::{MasterId, ShapeId};
    use crate::master::Master;
    use crate::name::{CellName, SectionName};
    use crate::section::{Row, Section};
    use crate::shape::Shape;

    use super::*;

    /// A cell with a cached number.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A cell with a formula and no cached value.
    fn formula_cell(name: &str, formula: &str) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = Some(formula.to_owned());
        cell
    }

    /// A geometry row of `row_type` with numeric `cells` (name, value).
    fn row(row_type: &str, ix: i32, cells: &[(&str, f64)]) -> Row {
        let mut row = Row::new();
        row.row_type = Some(row_type.to_owned());
        row.ix = Some(ix);
        for (name, value) in cells {
            row.cells.push(number_cell(name, *value));
        }
        row
    }

    /// The NURBS data of a typed row and its end point, or `None` for any
    /// other row kind.
    fn nurbs(row: &GeometryRow) -> Option<(Option<&NurbsData>, f64, f64)> {
        match row {
            GeometryRow::NurbsTo { data, x, y, .. } => Some((data.as_ref(), *x, *y)),
            _ => None,
        }
    }

    /// The polyline data of a typed row and its end point, or `None` for
    /// any other row kind.
    fn polyline(row: &GeometryRow) -> Option<(Option<&PolylineData>, f64, f64)> {
        match row {
            GeometryRow::PolylineTo { data, x, y } => Some((data.as_ref(), *x, *y)),
            _ => None,
        }
    }

    /// A `Geometry` section with `rows`.
    fn geometry(rows: Vec<Row>) -> Section {
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section.rows = rows;
        section
    }

    /// Adds a shape with one `Geometry` section to the document.
    fn with_geometry(doc: &mut Document, rows: Vec<Row>) -> ShapeId {
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        if let Some(shape) = doc.shape_mut(id) {
            shape.sheet.sections.push(geometry(rows));
        }
        id
    }

    #[test]
    fn row_variants_hold_their_data() {
        let r = GeometryRow::ArcTo {
            x: 1.0,
            y: 2.0,
            bow: 0.5,
        };
        assert!(matches!(r, GeometryRow::ArcTo { bow, .. } if bow == 0.5));
        let e = GeometryRow::Ellipse {
            center: Point::new(0.0, 0.0),
            a: Point::new(1.0, 0.0),
            b: Point::new(0.0, 1.0),
        };
        assert!(matches!(
            e,
            GeometryRow::Ellipse {
                a: Point { x: 1.0, y: 0.0 },
                ..
            }
        ));
        assert_eq!(
            GeometryRow::Unknown {
                row_type: "Whatever".to_owned()
            },
            GeometryRow::Unknown {
                row_type: "Whatever".to_owned()
            }
        );
    }

    #[test]
    fn an_unknown_shape_id_is_an_error() {
        let doc = Document::new();
        assert_eq!(
            doc.shape_geometry(ShapeId { index: 0 }),
            Err(Error::UnknownShape(ShapeId { index: 0 }))
        );
    }

    #[test]
    fn a_shape_without_geometry_has_no_paths() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let geometry = doc.shape_geometry(id).expect("reads");
        assert!(geometry.paths.is_empty());
        assert!(geometry.warnings.is_empty());
    }

    #[test]
    fn every_row_type_is_typed_from_its_cells() {
        let mut doc = Document::new();
        let id = with_geometry(
            &mut doc,
            vec![
                row("MoveTo", 1, &[("X", 1.0), ("Y", 2.0)]),
                row("LineTo", 2, &[("X", 3.0), ("Y", 4.0)]),
                row("ArcTo", 3, &[("X", 5.0), ("Y", 6.0), ("A", 0.5)]),
                row(
                    "EllipticalArcTo",
                    4,
                    &[
                        ("X", 7.0),
                        ("Y", 8.0),
                        ("A", 9.0),
                        ("B", 10.0),
                        ("C", 1.0),
                        ("D", 2.0),
                    ],
                ),
                row(
                    "SplineStart",
                    5,
                    &[
                        ("X", 1.0),
                        ("Y", 2.0),
                        ("A", 3.0),
                        ("B", 4.0),
                        ("C", 5.0),
                        ("D", 3.0),
                    ],
                ),
                row("SplineKnot", 6, &[("X", 1.0), ("Y", 2.0), ("A", 0.5)]),
                row(
                    "Ellipse",
                    7,
                    &[
                        ("X", 0.0),
                        ("Y", 0.0),
                        ("A", 1.0),
                        ("B", 0.0),
                        ("C", 0.0),
                        ("D", 2.0),
                    ],
                ),
                row(
                    "InfiniteLine",
                    8,
                    &[("X", 0.0), ("Y", 0.0), ("A", 1.0), ("B", 1.0)],
                ),
                row("RelMoveTo", 9, &[("X", 0.25), ("Y", 0.75)]),
                row("RelLineTo", 10, &[("X", 1.0), ("Y", 1.0)]),
                row(
                    "RelQuadBezTo",
                    11,
                    &[("X", 1.0), ("Y", 1.0), ("A", 0.5), ("B", 0.5)],
                ),
                row(
                    "RelCubBezTo",
                    12,
                    &[
                        ("X", 1.0),
                        ("Y", 1.0),
                        ("A", 0.1),
                        ("B", 0.2),
                        ("C", 0.3),
                        ("D", 0.4),
                    ],
                ),
                row(
                    "RelEllipticalArcTo",
                    13,
                    &[
                        ("X", 1.0),
                        ("Y", 1.0),
                        ("A", 0.5),
                        ("B", 0.5),
                        ("C", 0.0),
                        ("D", 1.0),
                    ],
                ),
                row("Whatever", 14, &[("X", 1.0), ("Y", 1.0)]),
                Row::new(),
            ],
        );
        let geometry = doc.shape_geometry(id).expect("reads");
        assert!(geometry.warnings.is_empty(), "{:?}", geometry.warnings);
        let path = geometry.paths.first().expect("one path");
        assert_eq!(path.rows.len(), 15);
        assert_eq!(path.rows[0], GeometryRow::MoveTo { x: 1.0, y: 2.0 });
        assert_eq!(path.rows[1], GeometryRow::LineTo { x: 3.0, y: 4.0 });
        assert_eq!(
            path.rows[2],
            GeometryRow::ArcTo {
                x: 5.0,
                y: 6.0,
                bow: 0.5
            }
        );
        assert_eq!(
            path.rows[3],
            GeometryRow::EllipticalArcTo {
                x: 7.0,
                y: 8.0,
                ctrl: Point::new(9.0, 10.0),
                angle: Angle::from_radians(1.0),
                ratio: 2.0,
            }
        );
        assert_eq!(
            path.rows[4],
            GeometryRow::SplineStart {
                x: 1.0,
                y: 2.0,
                second_knot: 3.0,
                first_knot: 4.0,
                last_knot: 5.0,
                degree: 3,
            }
        );
        assert_eq!(
            path.rows[5],
            GeometryRow::SplineKnot {
                x: 1.0,
                y: 2.0,
                knot: 0.5
            }
        );
        assert_eq!(
            path.rows[6],
            GeometryRow::Ellipse {
                center: Point::new(0.0, 0.0),
                a: Point::new(1.0, 0.0),
                b: Point::new(0.0, 2.0),
            }
        );
        assert_eq!(
            path.rows[7],
            GeometryRow::InfiniteLine {
                p1: Point::new(0.0, 0.0),
                p2: Point::new(1.0, 1.0),
            }
        );
        assert_eq!(path.rows[8], GeometryRow::RelMoveTo { x: 0.25, y: 0.75 });
        assert_eq!(path.rows[9], GeometryRow::RelLineTo { x: 1.0, y: 1.0 });
        assert_eq!(
            path.rows[10],
            GeometryRow::RelQuadBezTo {
                x: 1.0,
                y: 1.0,
                ctrl: Point::new(0.5, 0.5),
            }
        );
        assert_eq!(
            path.rows[11],
            GeometryRow::RelCubBezTo {
                x: 1.0,
                y: 1.0,
                ctrl1: Point::new(0.1, 0.2),
                ctrl2: Point::new(0.3, 0.4),
            }
        );
        assert_eq!(
            path.rows[12],
            GeometryRow::RelEllipticalArcTo {
                x: 1.0,
                y: 1.0,
                ctrl: Point::new(0.5, 0.5),
                angle: Angle::from_radians(0.0),
                ratio: 1.0,
            }
        );
        assert_eq!(
            path.rows[13],
            GeometryRow::Unknown {
                row_type: "Whatever".to_owned()
            }
        );
        assert_eq!(
            path.rows[14],
            GeometryRow::Unknown {
                row_type: String::new()
            }
        );
    }

    #[test]
    fn an_elliptical_arc_without_c_and_d_uses_the_row_defaults() {
        let mut doc = Document::new();
        let id = with_geometry(
            &mut doc,
            vec![row(
                "EllipticalArcTo",
                1,
                &[("X", 1.0), ("Y", 2.0), ("A", 3.0), ("B", 4.0)],
            )],
        );
        let geometry = doc.shape_geometry(id).expect("reads");
        // The corpus omits C and D (ADR 0022): C = 0, D = 1, no warning.
        assert!(geometry.warnings.is_empty(), "{:?}", geometry.warnings);
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::EllipticalArcTo {
                x: 1.0,
                y: 2.0,
                ctrl: Point::new(3.0, 4.0),
                angle: Angle::from_radians(0.0),
                ratio: 1.0,
            }
        );
    }

    #[test]
    fn a_bad_optional_cell_substitutes_the_row_default_with_one_warning() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(Vec::new());
            let mut row = row(
                "EllipticalArcTo",
                1,
                &[("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 1.0)],
            );
            let mut bad = Cell::new(CellName::from_static("D"));
            bad.value = Some(CellValue::Error("#VALUE!".to_owned()));
            row.cells.push(bad);
            section.rows.push(row);
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        assert_eq!(geometry.warnings.len(), 1);
        assert_eq!(
            geometry.warnings[0].kind,
            crate::values::WarningKind::ErrorValue("#VALUE!".to_owned())
        );
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::EllipticalArcTo {
                x: 1.0,
                y: 1.0,
                ctrl: Point::new(1.0, 1.0),
                angle: Angle::from_radians(0.0),
                ratio: 1.0,
            }
        );
    }

    #[test]
    fn a_missing_row_cell_substitutes_zero_with_one_warning() {
        let mut doc = Document::new();
        let id = with_geometry(&mut doc, vec![row("MoveTo", 1, &[("X", 1.0)])]);
        let geometry = doc.shape_geometry(id).expect("reads");
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::MoveTo { x: 1.0, y: 0.0 }
        );
        assert_eq!(geometry.warnings.len(), 1);
        assert_eq!(
            geometry.warnings[0].kind,
            crate::values::WarningKind::Missing
        );
    }

    #[test]
    fn nurbs_rows_decode_their_e_cell_formula() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(Vec::new());
            let mut good = row(
                "NURBSTo",
                1,
                &[
                    ("X", 1.0),
                    ("Y", 1.0),
                    ("A", 0.75),
                    ("B", 1.0),
                    ("C", 0.0),
                    ("D", 1.0),
                ],
            );
            good.cells.push(formula_cell(
                "E",
                "NURBS(1, 3, 0, 1, 0.25, 0.5, 0, 1, 1, 1, 1, 1)",
            ));
            let mut bad = row("NURBSTo", 2, &[("X", 2.0), ("Y", 2.0)]);
            bad.cells.push(formula_cell("E", "NURBS(1, 3)"));
            section.rows.push(good);
            section.rows.push(bad);
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        let rows = &geometry.paths[0].rows;
        let (data, x, y) = nurbs(&rows[0]).expect("a NURBSTo row");
        assert_eq!((x, y), (1.0, 1.0));
        let data = data.expect("parses");
        assert_eq!(data.knot_last, 1.0);
        assert_eq!(data.degree, 3);
        assert_eq!(data.x_type, 0);
        assert_eq!(data.y_type, 1);
        assert_eq!(data.points.len(), 2);
        assert_eq!(data.points[0].x, 0.25);
        assert_eq!(data.points[0].y, 0.5);
        assert_eq!(data.points[0].knot, 0.0);
        assert_eq!(data.points[0].weight, 1.0);
        assert_eq!(data.points[1].knot, 1.0);
        // Unparsable E: the row degrades to its cells (ADR 0022).
        let (data, x, y) = nurbs(&rows[1]).expect("a NURBSTo row");
        assert_eq!(data, None);
        assert_eq!((x, y), (2.0, 2.0));
    }

    #[test]
    fn nurbs_rows_fall_back_to_a_cached_string_value() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(Vec::new());
            let mut with_value = row("NurbsTo", 1, &[("X", 1.0), ("Y", 1.0)]);
            let mut e = Cell::new(CellName::from_static("E"));
            e.value = Some(CellValue::Str("NURBS(1, 2, 1, 1, 0, 0, 0, 1)".to_owned()));
            with_value.cells.push(e);
            section.rows.push(with_value);
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        let (data, _, _) = nurbs(&geometry.paths[0].rows[0]).expect("a NURBSTo row");
        let data = data.expect("parses");
        assert_eq!(data.degree, 2);
        assert_eq!(data.x_type, 1);
        assert_eq!(data.points.len(), 1);
    }

    #[test]
    fn polyline_rows_decode_their_a_cell_point_list() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(Vec::new());
            let mut call = row("PolylineTo", 1, &[("X", 1.0), ("Y", 1.0)]);
            call.cells
                .push(formula_cell("A", "POLYLINE(0, 0, 0, 0, 0, 1, 1, 1)"));
            let mut bare = row("PolylineTo", 2, &[("X", 1.0), ("Y", 1.0)]);
            bare.cells.push(formula_cell("A", "0 0;1 1;0.5 0.25"));
            let mut odd = row("PolylineTo", 3, &[("X", 1.0), ("Y", 1.0)]);
            odd.cells.push(formula_cell("A", "0 0;1"));
            section.rows.push(call);
            section.rows.push(bare);
            section.rows.push(odd);
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        let rows = &geometry.paths[0].rows;
        let (data, _, _) = polyline(&rows[0]).expect("a PolylineTo row");
        let data = data.expect("parses");
        assert_eq!(
            data.points,
            vec![
                Point::new(0.0, 0.0),
                Point::new(0.0, 1.0),
                Point::new(1.0, 1.0)
            ]
        );
        let (data, _, _) = polyline(&rows[1]).expect("a PolylineTo row");
        let data = data.expect("parses");
        assert_eq!(data.points.len(), 3);
        assert_eq!(data.points[2], Point::new(0.5, 0.25));
        // An odd token count is unparsable: no data (ADR 0022).
        let (data, _, _) = polyline(&rows[2]).expect("a PolylineTo row");
        assert_eq!(data, None);
    }

    #[test]
    fn section_flags_come_from_the_sections_own_cells() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(vec![row("LineTo", 1, &[("X", 1.0), ("Y", 1.0)])]);
            section.cells.push(number_cell("NoFill", 1.0));
            section.cells.push(number_cell("NoShow", 1.0));
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        let path = &geometry.paths[0];
        assert!(path.no_fill);
        // `NoLine` is missing: the rendering default is false, without a
        // warning (the flags are switches).
        assert!(!path.no_line);
        assert!(path.no_show);
        assert!(geometry.warnings.is_empty(), "{:?}", geometry.warnings);
    }

    #[test]
    fn section_flags_resolve_per_section_not_per_name() {
        // P8-15: the value policy's name-only "own entry" search
        // answered for the first same-named section, so one section's
        // `NoLine` suppressed the stroke of every section of a shape
        // with several `Geometry` sections (the box outline of
        // `no-bgcolor.vsd` did not draw).
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut first = geometry(vec![row("LineTo", 1, &[("X", 1.0), ("Y", 1.0)])]);
            first.cells.push(number_cell("NoLine", 1.0));
            shape.sheet.sections.push(first);
            let second = geometry(vec![row("LineTo", 1, &[("X", 2.0), ("Y", 2.0)])]);
            shape.sheet.sections.push(second);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        assert!(geometry.paths[0].no_line);
        assert!(
            !geometry.paths[1].no_line,
            "each section resolves its own flags"
        );
    }

    #[test]
    fn an_instance_without_rows_resolves_the_masters_rows() {
        // A master with a LineTo row; its instance has a Geometry section
        // with no rows (ADR 0019).
        let mut doc = Document::new();
        let master_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            shape
                .sheet
                .sections
                .push(geometry(vec![row("LineTo", 1, &[("X", 4.0), ("Y", 5.0)])]));
        }
        doc.masters.push(Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.master = Some(MasterId { index: 0 });
            shape.sheet.sections.push(geometry(Vec::new()));
        }
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);

        let geometry = doc.shape_geometry(instance).expect("reads");
        assert_eq!(geometry.paths.len(), 1);
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::LineTo { x: 4.0, y: 5.0 }
        );
    }

    #[test]
    fn row_cells_inherit_from_the_master_row_when_the_local_cell_is_inh() {
        // The local row's X is `Inh` (does not define): X comes from the
        // master's row at the same position, Y stays local.
        let mut doc = Document::new();
        let master_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            shape
                .sheet
                .sections
                .push(geometry(vec![row("MoveTo", 1, &[("X", 7.0), ("Y", 8.0)])]));
        }
        doc.masters.push(Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.master = Some(MasterId { index: 0 });
            let mut section = geometry(Vec::new());
            let mut local = row("MoveTo", 1, &[("Y", 9.0)]);
            local.cells.push(formula_cell("X", "Inh"));
            section.rows.push(local);
            shape.sheet.sections.push(section);
        }
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);

        let geometry = doc.shape_geometry(instance).expect("reads");
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::MoveTo { x: 7.0, y: 9.0 }
        );
    }

    #[test]
    fn several_geometry_sections_type_one_path_each() {
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let first = geometry(vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])]);
            let mut second = geometry(vec![row("LineTo", 1, &[("X", 2.0), ("Y", 2.0)])]);
            second.preserved.attributes.push(crate::raw::XmlAttribute {
                name: crate::raw::QName {
                    prefix: None,
                    uri: None,
                    local: "IX".to_owned(),
                },
                value: "1".to_owned(),
            });
            shape.sheet.sections.push(first);
            shape.sheet.sections.push(second);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        assert_eq!(geometry.paths.len(), 2);
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::MoveTo { x: 1.0, y: 1.0 }
        );
        assert_eq!(
            geometry.paths[1].rows[0],
            GeometryRow::LineTo { x: 2.0, y: 2.0 }
        );
    }

    // --- formula-encoded data parsing ---------------------------------

    #[test]
    fn parse_nurbs_data_reads_the_documented_call() {
        let data = parse_nurbs_data("NURBS(2, 3, 0, 0, 0.5, 0.5, 0.25, 2, 1, 1, 0.75, 1)")
            .expect("parses");
        assert_eq!(data.knot_last, 2.0);
        assert_eq!(data.degree, 3);
        assert_eq!(data.x_type, 0);
        assert_eq!(data.y_type, 0);
        assert_eq!(data.points.len(), 2);
        assert_eq!(data.points[0].weight, 2.0);
        // A leading '=' and quotes around arguments are accepted.
        let quoted = parse_nurbs_data("= nurbs(\"1\", 3, 1, 1, \"0\", \"0\", \"0\", \"1\")")
            .expect("parses");
        assert_eq!(quoted.x_type, 1);
        assert_eq!(quoted.points.len(), 1);
    }

    #[test]
    fn parse_nurbs_data_rejects_bad_calls() {
        assert_eq!(parse_nurbs_data("POLYLINE(0, 0, 1, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0, 0, 0, 0)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, -3, 0, 0, 0, 0, 0, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3.5, 0, 0, 0, 0, 0, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(x, 3, 0, 0, 0, 0, 0, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0, 0, x, 0, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0, 0, 0, 0, 1"), None);
    }

    #[test]
    fn parse_polyline_data_reads_a_call_or_a_bare_list() {
        let data = parse_polyline_data("POLYLINE(1, 1, 0, 0, 1, 1)").expect("parses");
        assert_eq!(
            data.points,
            vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]
        );
        let bare = parse_polyline_data("0,0;1,1;2 2").expect("parses");
        assert_eq!(
            bare.points,
            vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0)
            ]
        );
    }

    #[test]
    fn parse_polyline_data_rejects_bad_lists() {
        assert_eq!(parse_polyline_data("POLYLINE(0, 0, 1)"), None);
        assert_eq!(parse_polyline_data("POLYLINE(0)"), None);
        assert_eq!(parse_polyline_data("0 0;1"), None);
        assert_eq!(parse_polyline_data("0 0;1 x"), None);
        assert_eq!(parse_polyline_data(""), None);
        assert_eq!(parse_polyline_data("NURBS(1, 3, 0, 0, 0, 0, 0, 1)"), None);
    }

    #[test]
    fn parse_nurbs_data_edge_spellings() {
        // A space before the parenthesis and a leading `=` are accepted.
        let spaced = parse_nurbs_data("NURBS (1, 3, 0, 0, 0, 0, 0, 1)").expect("parses");
        assert_eq!(spaced.degree, 3);
        // A trailing comma, a stray closing parenthesis and a quoted
        // non-number make the argument list unparsable.
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0, 0, 0, 0, 1,)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 0, 0, 0, 0, 1))"), None);
        assert_eq!(parse_nurbs_data("NURBS(\"x\", 3, 0, 0, 0, 0, 0, 1)"), None);
        // An infinite degree is not an index; an infinite knot is kept
        // as written (numbers are not validated here).
        assert_eq!(parse_nurbs_data("NURBS(1, inf, 0, 0, 0, 0, 0, 1)"), None);
        let infinite = parse_nurbs_data("NURBS(1e999, 3, 0, 0, 0, 0, 0, 1)").expect("parses");
        assert!(infinite.knot_last.is_infinite());
    }

    #[test]
    fn parse_polyline_data_edge_spellings() {
        // A `POLYLINE` call never falls back to the bare-list form.
        assert_eq!(parse_polyline_data("POLYLINE(1, 1)"), None);
        assert_eq!(parse_polyline_data("POLYLINE(1, 1, 0, 0, 1 x)"), None);
        // Every separator of the bare list form splits.
        let bare = parse_polyline_data("\t0\t1\n2,3\r4;5").expect("parses");
        assert_eq!(
            bare.points,
            vec![
                Point::new(0.0, 1.0),
                Point::new(2.0, 3.0),
                Point::new(4.0, 5.0)
            ]
        );
        // Odd and empty token lists do not parse.
        assert_eq!(parse_polyline_data("5"), None);
        assert_eq!(parse_polyline_data("0 0 0"), None);
        assert_eq!(parse_polyline_data("   "), None);
        // An overflowing coordinate is kept as written.
        let infinite = parse_polyline_data("1e999 0").expect("parses");
        assert_eq!(infinite.points, vec![Point::new(f64::INFINITY, 0.0)]);
    }

    // --- row sources and degrading values -----------------------------

    #[test]
    fn row_accessors_return_none_for_other_rows() {
        assert!(nurbs(&GeometryRow::MoveTo { x: 0.0, y: 0.0 }).is_none());
        assert!(polyline(&GeometryRow::MoveTo { x: 0.0, y: 0.0 }).is_none());
    }

    #[test]
    fn index_arguments_must_fit_u32() {
        assert_eq!(
            parse_nurbs_data("NURBS(1, 4294967296, 0, 0, 0, 0, 0, 1)"),
            None
        );
        assert_eq!(parse_nurbs_data("NURBS(1, 3, -1, 0, 0, 0, 0, 1)"), None);
        assert_eq!(parse_nurbs_data("NURBS(1, 3, 0, 1.5, 0, 0, 0, 1)"), None);
    }

    /// A document with one master (`master_cells` on its shape) and one
    /// instance of it on a page.
    fn instance_of(master_cells: &[(&str, f64)]) -> (Document, ShapeId) {
        let mut doc = Document::new();
        let master_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        if let Some(shape) = doc.shape_mut(master_shape) {
            shape
                .sheet
                .sections
                .push(geometry(vec![row("MoveTo", 1, master_cells)]));
        }
        doc.masters.push(crate::master::Master::new(
            crate::id::MasterId { index: 0 },
            1,
        ));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        if let Some(shape) = doc.shape_mut(instance) {
            shape.master = Some(crate::id::MasterId { index: 0 });
        }
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);
        (doc, instance)
    }

    #[test]
    fn rows_come_from_the_master_when_the_shape_has_no_geometry_section() {
        let (doc, instance) = instance_of(&[("X", 3.0), ("Y", 4.0)]);
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert_eq!(geometry.paths.len(), 1);
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::MoveTo { x: 3.0, y: 4.0 }
        );
    }

    #[test]
    fn a_chained_master_supplies_the_rows_of_a_section_without_any() {
        // Master B carries the rows; master A is an instance of B with an
        // empty Geometry section; the instance is an instance of A.
        let mut doc = Document::new();
        let b_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(b_shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(vec![row("LineTo", 1, &[("X", 5.0), ("Y", 6.0)])]));
        doc.masters.push(crate::master::Master::new(
            crate::id::MasterId { index: 0 },
            2,
        ));
        doc.masters[0].shapes.push(b_shape);
        let a_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(a_shape).expect("exists");
            shape.master = Some(crate::id::MasterId { index: 0 });
            shape.sheet.sections.push(geometry(Vec::new()));
        }
        doc.masters.push(crate::master::Master::new(
            crate::id::MasterId { index: 1 },
            1,
        ));
        doc.masters[1].shapes.push(a_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 3));
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.master = Some(crate::id::MasterId { index: 1 });
            shape.sheet.sections.push(geometry(Vec::new()));
        }
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);

        let geometry = doc.shape_geometry(instance).expect("reads");
        assert_eq!(geometry.paths.len(), 1);
        assert_eq!(
            geometry.paths[0].rows[0],
            GeometryRow::LineTo { x: 5.0, y: 6.0 }
        );
    }

    #[test]
    fn a_master_chain_without_rows_takes_no_paths() {
        let (mut doc, instance) = instance_of(&[]);
        // The master's section exists but has no rows and no further rung.
        let master_shape = doc.masters[0].shapes[0];
        doc.shape_mut(master_shape).expect("exists").sheet.sections[0]
            .rows
            .clear();
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert!(geometry.paths.is_empty());
    }

    #[test]
    fn a_dangling_master_reference_takes_no_paths() {
        let mut doc = Document::new();
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(instance).expect("exists").master = Some(crate::id::MasterId { index: 5 });
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert!(geometry.paths.is_empty());
    }

    #[test]
    fn a_cyclic_master_chain_takes_no_paths() {
        let (mut doc, instance) = instance_of(&[]);
        let master_shape = doc.masters[0].shapes[0];
        // The master's shape is itself an instance of the same master.
        doc.shape_mut(master_shape).expect("exists").master =
            Some(crate::id::MasterId { index: 0 });
        doc.shape_mut(master_shape).expect("exists").sheet.sections[0]
            .rows
            .clear();
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert!(geometry.paths.is_empty());
    }

    #[test]
    fn optional_row_cells_degrade_by_kind() {
        // The D cell of an `EllipticalArcTo` is optional (ADR 0022): a
        // `Themed` marker, a wrong kind and a formula-only cell each
        // substitute the row default, warning only for the bad ones.
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(id).expect("exists");
            let mut section = geometry(Vec::new());
            let mut themed = row(
                "EllipticalArcTo",
                1,
                &[("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 1.0)],
            );
            let mut cell = Cell::new(CellName::from_static("D"));
            cell.value = Some(CellValue::Str("Themed".to_owned()));
            themed.cells.push(cell);
            let mut wrong = row(
                "EllipticalArcTo",
                2,
                &[("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 1.0)],
            );
            let mut cell = Cell::new(CellName::from_static("D"));
            cell.value = Some(CellValue::Str("x".to_owned()));
            wrong.cells.push(cell);
            let mut formula_only = row(
                "EllipticalArcTo",
                3,
                &[("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 1.0)],
            );
            formula_only.cells.push(formula_cell("D", "Width*2"));
            section.rows.push(themed);
            section.rows.push(wrong);
            section.rows.push(formula_only);
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(id).expect("reads");
        let rows = &geometry.paths[0].rows;
        // The themed marker is unresolvable: 0 with `UnresolvedTheme`.
        assert_eq!(
            rows[0],
            GeometryRow::EllipticalArcTo {
                x: 1.0,
                y: 1.0,
                ctrl: Point::new(1.0, 1.0),
                angle: Angle::from_radians(0.0),
                ratio: 0.0,
            }
        );
        // The wrong kind and the formula-only cell give the row default.
        for row in &rows[1..] {
            assert_eq!(
                *row,
                GeometryRow::EllipticalArcTo {
                    x: 1.0,
                    y: 1.0,
                    ctrl: Point::new(1.0, 1.0),
                    angle: Angle::from_radians(0.0),
                    ratio: 1.0,
                }
            );
        }
        assert_eq!(geometry.warnings.len(), 2);
        assert_eq!(
            geometry.warnings[0].kind,
            crate::values::WarningKind::UnresolvedTheme
        );
        assert_eq!(
            geometry.warnings[1].kind,
            crate::values::WarningKind::KindMismatch
        );
    }

    #[test]
    fn section_flags_read_every_written_kind() {
        let mut doc = Document::new();
        let first = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(first).expect("exists");
            let mut section = geometry(Vec::new());
            let mut no_fill = Cell::new(CellName::from_static("NoFill"));
            no_fill.value = Some(CellValue::Bool(true));
            section.cells.push(no_fill);
            let mut no_line = Cell::new(CellName::from_static("NoLine"));
            no_line.value = Some(CellValue::Error("#VALUE!".to_owned()));
            section.cells.push(no_line);
            let mut no_show = Cell::new(CellName::from_static("NoShow"));
            no_show.value = Some(CellValue::Str("x".to_owned()));
            section.cells.push(no_show);
            shape.sheet.sections.push(section);
        }
        let second = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(second).expect("exists");
            let mut section = geometry(Vec::new());
            let mut no_fill = Cell::new(CellName::from_static("NoFill"));
            no_fill.value = Some(CellValue::Str("Themed".to_owned()));
            section.cells.push(no_fill);
            section.cells.push(formula_cell("NoLine", "TRUE"));
            shape.sheet.sections.push(section);
        }
        let geometry = doc.shape_geometry(first).expect("reads");
        let path = &geometry.paths[0];
        assert!(path.no_fill);
        assert!(!path.no_line);
        assert!(!path.no_show);
        assert_eq!(geometry.warnings.len(), 2);
        assert_eq!(
            geometry.warnings[0].kind,
            crate::values::WarningKind::ErrorValue("#VALUE!".to_owned())
        );
        assert_eq!(
            geometry.warnings[1].kind,
            crate::values::WarningKind::KindMismatch
        );
        let geometry = doc.shape_geometry(second).expect("reads");
        let path = &geometry.paths[0];
        assert!(!path.no_fill);
        assert!(!path.no_line);
        assert_eq!(geometry.warnings.len(), 1);
        assert_eq!(
            geometry.warnings[0].kind,
            crate::values::WarningKind::UnresolvedTheme
        );
    }
}

#[cfg(test)]
mod row_union_tests {
    use crate::cell::{Cell, CellValue};
    use crate::document::Document;
    use crate::id::{MasterId, ShapeId};
    use crate::lookup::{CellLookup, ValueSource};
    use crate::master::Master;
    use crate::name::{CellName, SectionName};
    use crate::raw::{QName, XmlAttribute};
    use crate::section::{Row, Section};
    use crate::shape::Shape;

    use super::*;

    /// A cell with a cached number.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A cell with a cached boolean.
    fn bool_cell(name: &str, value: bool) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Bool(value));
        cell
    }

    /// A cell whose formula is `Inh`: it defines nothing (ADR 0013), but
    /// its cached snapshot is this sheet's own value (ADR 0054/0057).
    fn inherited_cell(name: &str) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = Some("Inh".to_owned());
        cell.value = Some(CellValue::Number(99.0));
        cell
    }

    /// A row with `T = row_type`, `IX = ix` and numeric `cells`.
    fn row(row_type: &str, ix: i32, cells: &[(&str, f64)]) -> Row {
        let mut row = Row::new();
        row.row_type = Some(row_type.to_owned());
        row.ix = Some(ix);
        for (name, value) in cells {
            row.cells.push(number_cell(name, *value));
        }
        row
    }

    /// A row with `Del = 1` (kept verbatim in `preserved`, ADR 0012).
    fn deleted_row(row_type: &str, ix: i32) -> Row {
        let mut row = Row::new();
        row.row_type = Some(row_type.to_owned());
        row.ix = Some(ix);
        row.preserved.attributes.push(XmlAttribute {
            name: QName {
                prefix: None,
                uri: None,
                local: "Del".to_owned(),
            },
            value: "1".to_owned(),
        });
        row
    }

    /// A `Geometry` section with `rows` and section-level `flags`.
    fn geometry(rows: Vec<Row>, flags: &[(&str, bool)]) -> Section {
        let mut sec = Section::new(SectionName::from_static("Geometry"));
        sec.rows = rows;
        for (name, value) in flags {
            sec.cells.push(bool_cell(name, *value));
        }
        sec
    }

    /// A document with one master whose shape carries `master_rows` and
    /// one instance of it whose shape carries `instance_rows` (in a
    /// `Geometry` section each).
    fn two_rung(master_rows: Vec<Row>, instance_rows: Vec<Row>) -> (Document, ShapeId) {
        let mut doc = Document::new();
        let master_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(master_shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(master_rows, &[]));
        doc.masters.push(Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(instance).expect("exists").master = Some(MasterId { index: 0 });
        doc.shape_mut(instance)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(instance_rows, &[]));
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);
        (doc, instance)
    }

    /// The row kinds and X values of one path, for order assertions.
    fn kinds_and_x(rows: &[GeometryRow]) -> Vec<(&'static str, f64)> {
        rows.iter()
            .map(|row| match row {
                GeometryRow::MoveTo { x, .. } => ("MoveTo", *x),
                GeometryRow::LineTo { x, .. } => ("LineTo", *x),
                _ => ("other", f64::NAN),
            })
            .collect()
    }

    #[test]
    fn sparse_instance_rows_merge_with_the_master_rows() {
        // Acceptance: the instance carries only `LineTo` rows at `IX` 2..3
        // (some cells `Inh`); the master holds the rest. The union is the
        // master's row list with the instance's rows applied by `IX`.
        let master_rows = vec![
            row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)]),
            row("LineTo", 2, &[("X", 2.0), ("Y", 2.0)]),
            row("LineTo", 3, &[("X", 3.0), ("Y", 3.0)]),
            row("LineTo", 4, &[("X", 4.0), ("Y", 4.0)]),
            row("LineTo", 5, &[("X", 5.0), ("Y", 5.0)]),
        ];
        let mut overridden = row("LineTo", 2, &[("X", 20.0)]);
        overridden.cells.push(inherited_cell("Y"));
        let mut partial = row("LineTo", 3, &[("Y", 30.0)]);
        partial.cells.push(inherited_cell("X"));
        let (doc, instance) = two_rung(master_rows, vec![overridden, partial]);

        let geometry = doc.shape_geometry(instance).expect("reads");
        let rows = &geometry.paths[0].rows;
        assert_eq!(rows.len(), 5);
        assert_eq!(
            kinds_and_x(rows),
            vec![
                ("MoveTo", 1.0),
                ("LineTo", 20.0),
                ("LineTo", 99.0),
                ("LineTo", 4.0),
                ("LineTo", 5.0)
            ]
        );
        // Partial cell override: the `Inh` cells carry this sheet's own
        // snapshot (ADR 0054/0057, value 99), and the master's rows fill
        // the rest.
        assert_eq!(rows[1], GeometryRow::LineTo { x: 20.0, y: 99.0 });
        assert_eq!(rows[2], GeometryRow::LineTo { x: 99.0, y: 30.0 });
    }

    #[test]
    fn an_instance_row_overrides_by_ix_not_by_position() {
        // The instance's only row sits at position 0 but names `IX` 2: it
        // overrides the master's row 2 and leaves row 1 in place.
        let (doc, instance) = two_rung(
            vec![
                row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)]),
                row("LineTo", 2, &[("X", 2.0), ("Y", 2.0)]),
            ],
            vec![row("LineTo", 2, &[("X", 20.0), ("Y", 20.0)])],
        );
        let geometry = doc.shape_geometry(instance).expect("reads");
        let rows = &geometry.paths[0].rows;
        assert_eq!(kinds_and_x(rows), vec![("MoveTo", 1.0), ("LineTo", 20.0)]);
    }

    #[test]
    fn a_deleted_row_removes_the_inherited_row() {
        let (doc, instance) = two_rung(
            vec![
                row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)]),
                row("LineTo", 2, &[("X", 2.0), ("Y", 2.0)]),
                row("LineTo", 3, &[("X", 3.0), ("Y", 3.0)]),
            ],
            vec![deleted_row("LineTo", 2)],
        );
        let geometry = doc.shape_geometry(instance).expect("reads");
        let rows = &geometry.paths[0].rows;
        assert_eq!(kinds_and_x(rows), vec![("MoveTo", 1.0), ("LineTo", 3.0)]);
    }

    #[test]
    fn a_deleted_row_that_matches_nothing_removes_nothing() {
        let (doc, instance) = two_rung(
            vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])],
            vec![deleted_row("LineTo", 9)],
        );
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert_eq!(kinds_and_x(&geometry.paths[0].rows), vec![("MoveTo", 1.0)]);
    }

    #[test]
    fn a_deleted_row_drops_its_own_row_too() {
        // A shape with no master: `Del` removes the row it sits on.
        let mut doc = Document::new();
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(id)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(vec![deleted_row("MoveTo", 1)], &[]));
        let geometry = doc.shape_geometry(id).expect("reads");
        assert!(geometry.paths[0].rows.is_empty());
    }

    #[test]
    fn section_level_cells_merge_across_rungs() {
        // The master sets NoFill, NoShow and NoSnap; the instance turns
        // NoFill off and inherits the rest (`NoQuickDrag` is nowhere: the
        // rendering default `false`, without a warning).
        let (mut doc, instance) = two_rung(
            vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])],
            vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])],
        );
        {
            let master_shape = doc.masters[0].shapes[0];
            let sec = &mut doc.shape_mut(master_shape).expect("exists").sheet.sections[0];
            sec.cells.push(bool_cell("NoFill", true));
            sec.cells.push(bool_cell("NoShow", true));
            sec.cells.push(bool_cell("NoSnap", true));
        }
        {
            let sec = &mut doc.shape_mut(instance).expect("exists").sheet.sections[0];
            sec.cells.push(bool_cell("NoFill", false));
        }
        let geometry = doc.shape_geometry(instance).expect("reads");
        let path = &geometry.paths[0];
        assert!(!path.no_fill, "the instance's NoFill overrides");
        assert!(path.no_show, "NoShow comes from the master");
        assert!(geometry.warnings.is_empty(), "{:?}", geometry.warnings);
        // NoSnap is a section-level cell with no slot on `GeometryPath`:
        // the lookup resolves it the same way.
        let resolved = doc
            .resolve(instance, &CellName::from_static("NoSnap"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Master(MasterId { index: 0 }));
        assert_eq!(resolved.value, Some(&CellValue::Bool(true)));
    }

    #[test]
    fn a_section_level_inh_cell_keeps_its_snapshot() {
        // ADR 0054/0057: an `Inh` cell's cached snapshot is this sheet's
        // own value at save time, so it wins over the master's rung for
        // the reader; `Document::resolve` still reports the master as the
        // source (pinned in `values` tests).
        let (mut doc, instance) = two_rung(
            vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])],
            vec![row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)])],
        );
        {
            let master_shape = doc.masters[0].shapes[0];
            let sec = &mut doc.shape_mut(master_shape).expect("exists").sheet.sections[0];
            sec.cells.push(bool_cell("NoLine", true));
        }
        {
            let sec = &mut doc.shape_mut(instance).expect("exists").sheet.sections[0];
            let mut cell = Cell::new(CellName::from_static("NoLine"));
            cell.formula = Some("Inh".to_owned());
            cell.value = Some(CellValue::Bool(false));
            sec.cells.push(cell);
        }
        let geometry = doc.shape_geometry(instance).expect("reads");
        assert!(!geometry.paths[0].no_line, "the snapshot is the value");
    }

    #[test]
    fn the_row_union_applies_through_every_master_rung() {
        // Instance -> master A -> master B: B holds rows 1..2, A overrides
        // row 2, the instance adds row 3.
        let mut doc = Document::new();
        let b_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(b_shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(
                vec![
                    row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)]),
                    row("LineTo", 2, &[("X", 2.0), ("Y", 2.0)]),
                ],
                &[],
            ));
        doc.masters.push(Master::new(MasterId { index: 0 }, 2));
        doc.masters[0].shapes.push(b_shape);
        let a_shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(a_shape).expect("exists");
            shape.master = Some(MasterId { index: 0 });
            shape.sheet.sections.push(geometry(
                vec![row("LineTo", 2, &[("X", 20.0), ("Y", 20.0)])],
                &[],
            ));
        }
        doc.masters.push(Master::new(MasterId { index: 1 }, 1));
        doc.masters[1].shapes.push(a_shape);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 3));
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.master = Some(MasterId { index: 1 });
            shape.sheet.sections.push(geometry(
                vec![row("LineTo", 3, &[("X", 30.0), ("Y", 30.0)])],
                &[],
            ));
        }
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);

        let geometry = doc.shape_geometry(instance).expect("reads");
        let rows = &geometry.paths[0].rows;
        assert_eq!(rows.len(), 3);
        assert_eq!(
            kinds_and_x(rows),
            vec![("MoveTo", 1.0), ("LineTo", 20.0), ("LineTo", 30.0)]
        );
        assert_eq!(rows[1], GeometryRow::LineTo { x: 20.0, y: 20.0 });
    }

    #[test]
    fn an_instance_row_with_no_ix_matches_by_position() {
        // Both sides have rows without `IX`: position 0 matches position 0.
        let mut master_row = row("MoveTo", 1, &[("X", 1.0), ("Y", 1.0)]);
        master_row.ix = None;
        let mut local_row = row("LineTo", 1, &[("X", 9.0), ("Y", 9.0)]);
        local_row.ix = None;
        let (doc, instance) = two_rung(vec![master_row], vec![local_row]);
        let geometry = doc.shape_geometry(instance).expect("reads");
        let rows = &geometry.paths[0].rows;
        assert_eq!(kinds_and_x(rows), vec![("LineTo", 9.0)]);
    }
}
