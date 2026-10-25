//! Mapping the ShapeSheet content of a sheet element (task P2-06).
//!
//! A `Shape`, `PageSheet` or `StyleSheet` element carries its cells and
//! sections as child elements: `Cell` (direct cells) and `Section` (which
//! holds `Row` children, each with its own `Cell` children). They map to
//! [`Sheet`], [`Section`], [`Row`] and [`Cell`]; everything else follows the
//! preservation rules of the module docs of [`crate::read`].
//!
//! A `Cell` or `Section` with no `N` attribute cannot be named, and a small
//! element with content the model cannot hold (text, a comment or a
//! processing instruction) has nowhere to put it: both are kept verbatim in
//! the parent's [`Preserved`] instead of being mapped.

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Preserved, Row, Section, SectionName, Sheet, XmlNode,
};

use crate::Error;
use crate::read::node::{self, elements, has_loose_content, is_namespace_declaration};
use crate::read::value;

/// Maps the children of a sheet element into `sheet`, keeping whatever is not
/// mapped in `preserved`.
///
/// `skip` names the children the caller maps itself (a shape's `Shapes`
/// wrapper); they are left alone.
pub(crate) fn map_sheet(
    node: &XmlNode,
    sheet: &mut Sheet,
    preserved: &mut Preserved,
    part: &str,
    skip: &[&str],
) -> Result<(), Error> {
    for element in elements(node) {
        if skip.contains(&element.name.local.as_str()) {
            continue;
        }
        match element.name.local.as_str() {
            "Cell" => match map_cell(element, part)? {
                Some(cell) => sheet.cells.push(cell),
                None => preserved.children.push(element.clone()),
            },
            "Section" => match map_section(element, part)? {
                Some(section) => sheet.sections.push(section),
                None => preserved.children.push(element.clone()),
            },
            _ => preserved.children.push(element.clone()),
        }
    }
    Ok(())
}

/// Whether the writer writes the `U` code `code` back unchanged for a cell
/// holding `value` in `unit` (task P6-16).
///
/// The writer re-derives the spelling of a value kind (`BOOL`, `STR`,
/// `COLOR`, `LIST`) and of a [`Unit`] (see [`crate::write::value`]); every
/// other spelling is one it cannot produce again. The reader asks the
/// writer's own decision function here, so the two rules cannot drift: a
/// code the writer reproduces needs no place in `Preserved`, and one it
/// would drop (or overwrite) does.
fn writer_spells_code(code: &str, value: Option<&CellValue>, unit: Unit) -> bool {
    match crate::write::value::unit_spelling(value, unit, Some(code)) {
        crate::write::value::UnitSpelling::Keep => true,
        crate::write::value::UnitSpelling::Code(written) => written == code,
        crate::write::value::UnitSpelling::Absent => false,
    }
}

/// Maps one `Cell` element, or returns `None` when the element cannot be
/// modeled and must be kept verbatim by the caller.
///
/// `N` is the name, `F` the formula, `V` the cached value, `U` the declared
/// unit and `E` the cached error (see [`value`]). When `E` is present the
/// value is the error and `V` and `U` stay preserved; a `V` the model
/// cannot hold (a non-finite number, task P8-19 cause 3) leaves the value
/// slot empty and keeps `V` and `U` preserved the same way. A `U` code
/// stays preserved only when the writer does not spell it back for this
/// cell ([`writer_spells_code`]); codes it re-derives are consumed here.
pub(crate) fn map_cell(node: &XmlNode, part: &str) -> Result<Option<Cell>, Error> {
    let Some(name) = node::attribute(node, "N") else {
        return Ok(None);
    };
    if has_loose_content(node) {
        return Ok(None);
    }
    let name = name.to_owned();
    let error = node::attribute(node, "E").map(str::to_owned);
    let v = node::attribute(node, "V").map(str::to_owned);
    let u = node::attribute(node, "U").map(str::to_owned);
    let mut cell = Cell::new(CellName::new(name.clone()));
    // Whether `V` and `U` stay in `Preserved`: the error takes the value
    // slot, and so does a literal with no model value.
    let mut verbatim_value = error.is_some();
    cell.value = match (&error, &v) {
        (Some(error), _) => Some(CellValue::Error(error.clone())),
        (None, Some(literal)) => match value::value(literal, u.as_deref(), part, &name)? {
            Some(value) => Some(value),
            None => {
                verbatim_value = true;
                None
            }
        },
        (None, None) => None,
    };
    cell.unit = if verbatim_value {
        Unit::Unitsless
    } else {
        value::unit(u.as_deref()).unwrap_or(Unit::Unitsless)
    };
    for attribute in &node.attributes {
        if is_namespace_declaration(attribute) {
            continue;
        }
        match attribute.name.local.as_str() {
            "N" | "E" => {}
            "F" => cell.formula = Some(attribute.value.clone()),
            "V" => {
                if verbatim_value {
                    // The error or the absent value leaves the written value alone.
                    cell.preserved.attributes.push(attribute.clone());
                }
            }
            "U" => {
                if verbatim_value
                    || !writer_spells_code(&attribute.value, cell.value.as_ref(), cell.unit)
                {
                    cell.preserved.attributes.push(attribute.clone());
                }
            }
            _ => cell.preserved.attributes.push(attribute.clone()),
        }
    }
    for element in elements(node) {
        cell.preserved.children.push(element.clone());
    }
    Ok(Some(cell))
}

/// Maps one `Section` element, or returns `None` when the element cannot be
/// modeled and must be kept verbatim by the caller.
///
/// The `Cell` children a section writes outside its rows map to
/// [`Section::cells`](larrygram_model::Section) (P2-06b decision 1), in file
/// order.
pub(crate) fn map_section(node: &XmlNode, part: &str) -> Result<Option<Section>, Error> {
    let Some(name) = node::attribute(node, "N") else {
        return Ok(None);
    };
    if has_loose_content(node) {
        return Ok(None);
    }
    let mut section = Section::new(SectionName::new(name));
    for attribute in &node.attributes {
        if is_namespace_declaration(attribute) || attribute.name.local == "N" {
            continue;
        }
        // `IX` and `Del` have no field on `Section`; they stay preserved.
        section.preserved.attributes.push(attribute.clone());
    }
    for element in elements(node) {
        match element.name.local.as_str() {
            "Cell" => match map_cell(element, part)? {
                Some(cell) => section.cells.push(cell),
                None => section.preserved.children.push(element.clone()),
            },
            "Row" => match map_row(element, part)? {
                Some(row) => section.rows.push(row),
                None => section.preserved.children.push(element.clone()),
            },
            _ => section.preserved.children.push(element.clone()),
        }
    }
    Ok(Some(section))
}

/// Maps one `Row` element, or returns `None` when the element cannot be
/// modeled and must be kept verbatim by the caller.
pub(crate) fn map_row(node: &XmlNode, part: &str) -> Result<Option<Row>, Error> {
    if has_loose_content(node) {
        return Ok(None);
    }
    let mut row = Row::new();
    for attribute in &node.attributes {
        if is_namespace_declaration(attribute) {
            continue;
        }
        match attribute.name.local.as_str() {
            "N" => row.name = Some(attribute.value.clone()),
            "T" => row.row_type = Some(attribute.value.clone()),
            "IX" => {
                row.ix = Some(super::parse_attr_int(&attribute.value, part, "IX")?);
            }
            _ => row.preserved.attributes.push(attribute.clone()),
        }
    }
    for element in elements(node) {
        match element.name.local.as_str() {
            "Cell" => match map_cell(element, part)? {
                Some(cell) => row.cells.push(cell),
                None => row.preserved.children.push(element.clone()),
            },
            _ => row.preserved.children.push(element.clone()),
        }
    }
    Ok(Some(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::{Color, QName, XmlAttribute, XmlChild};

    fn qname(name: &str) -> QName {
        match name.split_once(':') {
            Some((prefix, local)) => QName {
                prefix: Some(prefix.to_owned()),
                uri: None,
                local: local.to_owned(),
            },
            None => QName::local(name),
        }
    }

    fn element(name: &str, attrs: &[(&str, &str)], children: Vec<XmlChild>) -> XmlNode {
        XmlNode {
            name: qname(name),
            attributes: attrs
                .iter()
                .map(|(name, value)| XmlAttribute {
                    name: qname(name),
                    value: (*value).to_owned(),
                })
                .collect(),
            children,
            self_closing: true,
        }
    }

    fn cell(attrs: &[(&str, &str)]) -> XmlNode {
        element("Cell", attrs, Vec::new())
    }

    #[test]
    fn a_cell_maps_name_formula_value_and_unit() {
        let cell = map_cell(
            &cell(&[("N", "PinX"), ("U", "MM"), ("V", "2"), ("F", "Width*0.5")]),
            "/p",
        )
        .expect("cell")
        .expect("modeled");
        assert_eq!(cell.name.as_str(), "PinX");
        assert_eq!(cell.formula.as_deref(), Some("Width*0.5"));
        assert_eq!(cell.value, Some(CellValue::Number(2.0)));
        assert_eq!(cell.unit, Unit::Millimeters);
        assert!(cell.preserved.attributes.is_empty());
    }

    #[test]
    fn a_cell_without_a_name_is_kept_verbatim() {
        assert!(
            map_cell(&cell(&[("V", "2")]), "/p")
                .expect("cell")
                .is_none()
        );
    }

    #[test]
    fn a_re_derived_u_code_is_consumed() {
        // The writer spells `BOOL` back from the boolean (task P6-16), so
        // the code needs no place in `Preserved`.
        let cell = map_cell(&cell(&[("N", "Value"), ("U", "BOOL"), ("V", "1")]), "/p")
            .expect("cell")
            .expect("modeled");
        assert_eq!(cell.value, Some(CellValue::Bool(true)));
        assert_eq!(cell.unit, Unit::Unitsless);
        assert!(cell.preserved.attributes.is_empty());
    }

    #[test]
    fn a_str_code_is_consumed_only_when_the_writer_spells_it_back() {
        // A number-like string reads back the `STR` the writer would spell
        // for it; a plain string is not re-derived (`kind_code` fires only
        // for a value whose shape would misread), so its code stays.
        let number_like = map_cell(&cell(&[("N", "Value"), ("U", "STR"), ("V", "2")]), "/p")
            .expect("cell")
            .expect("modeled");
        assert_eq!(number_like.value, Some(CellValue::Str("2".to_owned())));
        assert!(number_like.preserved.attributes.is_empty());
        let plain = map_cell(
            &cell(&[("N", "Value"), ("U", "STR"), ("V", "Bottom")]),
            "/p",
        )
        .expect("cell")
        .expect("modeled");
        assert_eq!(plain.value, Some(CellValue::Str("Bottom".to_owned())));
        assert_eq!(plain.preserved.attributes.len(), 1);
        assert_eq!(plain.preserved.attributes[0].name.local, "U");
    }

    #[test]
    fn a_color_code_is_kept_for_a_color_without_a_unit() {
        // `COLOR` is re-derived only for a colour that carries a unit
        // (`write::value::kind_code`); a read `COLOR` cell has none, and the
        // corpus spells four such cells (`U='COLOR' V='#ffffff'`).
        let cell = map_cell(
            &cell(&[("N", "Fill"), ("U", "COLOR"), ("V", "#ffffff")]),
            "/p",
        )
        .expect("cell")
        .expect("modeled");
        assert_eq!(
            cell.value,
            Some(CellValue::Color(Color::new(255, 255, 255)))
        );
        assert_eq!(cell.preserved.attributes.len(), 1);
        assert_eq!(cell.preserved.attributes[0].name.local, "U");
    }

    #[test]
    fn a_code_the_writer_would_overwrite_stays_preserved() {
        // `U='MM' V='Themed'`: the theme marker reads as a string and the
        // writer would spell `STR` over the `MM`, so the file's code is
        // kept (`writer_spells_code` only consumes a code the writer
        // writes back unchanged).
        let cell = map_cell(&cell(&[("N", "Fill"), ("U", "MM"), ("V", "Themed")]), "/p")
            .expect("cell")
            .expect("modeled");
        assert_eq!(cell.value, Some(CellValue::Str("Themed".to_owned())));
        assert_eq!(cell.preserved.attributes.len(), 1);
        assert_eq!(cell.preserved.attributes[0].value, "MM");
    }

    #[test]
    fn a_list_code_is_consumed() {
        let cell = map_cell(
            &cell(&[("N", "LayerMember"), ("U", "LIST"), ("V", "1;0")]),
            "/p",
        )
        .expect("cell")
        .expect("modeled");
        assert_eq!(cell.value, Some(CellValue::List("1;0".to_owned())));
        assert!(cell.preserved.attributes.is_empty());
    }

    #[test]
    fn a_unit_code_the_writer_re_derives_is_consumed() {
        let cell = map_cell(&cell(&[("N", "PinX"), ("U", "MM"), ("V", "2")]), "/p")
            .expect("cell")
            .expect("modeled");
        assert_eq!(cell.unit, Unit::Millimeters);
        assert!(cell.preserved.attributes.is_empty());
    }

    #[test]
    fn a_code_the_writer_never_spells_stays_preserved() {
        // `DATE`, `GUID`, `PNT`, `DT`, `DL` and any unknown code have no
        // spelling in `write::value`; the code is the only place that keeps
        // them (task P6-16).
        for (code, v) in [
            ("DATE", "2015-07-06T11:05:49"),
            ("GUID", "{EFB4EACD-BEFB-4ECD-A226-1282E88A7BE7}"),
            ("PNT", "2;3"),
            ("DT", "0.01041666666666667"),
            ("DL", "1.082677165354331"),
            ("XYZ", "word"),
        ] {
            let cell = map_cell(&cell(&[("N", "Value"), ("U", code), ("V", v)]), "/p")
                .expect("cell")
                .expect("modeled");
            assert_eq!(cell.preserved.attributes.len(), 1, "code {code}");
            assert_eq!(cell.preserved.attributes[0].name.local, "U");
            assert_eq!(cell.preserved.attributes[0].value, code);
        }
    }

    #[test]
    fn an_error_takes_the_value_slot_and_keeps_v_and_u() {
        let cell = map_cell(
            &cell(&[("N", "Value"), ("U", "STR"), ("V", ""), ("E", "#VALUE!")]),
            "/p",
        )
        .expect("cell")
        .expect("modeled");
        assert_eq!(cell.value, Some(CellValue::Error("#VALUE!".to_owned())));
        assert_eq!(cell.unit, Unit::Unitsless);
        let kept: Vec<&str> = cell
            .preserved
            .attributes
            .iter()
            .map(|attribute| attribute.name.local.as_str())
            .collect();
        assert_eq!(kept, ["U", "V"]);
    }

    #[test]
    fn cell_children_are_preserved() {
        let node = element(
            "Cell",
            &[("N", "Value")],
            vec![XmlChild::Element(element(
                "RefBy",
                &[("ID", "1")],
                Vec::new(),
            ))],
        );
        let cell = map_cell(&node, "/p").expect("cell").expect("modeled");
        assert_eq!(cell.preserved.children.len(), 1);
    }

    #[test]
    fn a_cell_with_loose_content_is_kept_verbatim() {
        let node = element(
            "Cell",
            &[("N", "Value")],
            vec![XmlChild::Comment("note".to_owned())],
        );
        assert!(map_cell(&node, "/p").expect("cell").is_none());
    }

    #[test]
    fn a_bad_value_names_the_part_and_the_cell() {
        let error = map_cell(
            &cell(&[("N", "PinX"), ("U", "MM"), ("V", "x")]),
            "/visio/p.xml",
        );
        assert_eq!(
            error,
            Err(Error::InvalidPackage(
                "/visio/p.xml: cell `PinX`: numeric value does not parse".to_owned()
            ))
        );
    }

    #[test]
    fn a_section_maps_its_level_cells_and_rows() {
        let node = element(
            "Section",
            &[("N", "Geometry"), ("IX", "0")],
            vec![
                XmlChild::Element(cell(&[("N", "NoFill"), ("V", "0")])),
                XmlChild::Element(element(
                    "Row",
                    &[("T", "RelMoveTo"), ("IX", "1")],
                    vec![XmlChild::Element(cell(&[("N", "X"), ("V", "0")]))],
                )),
            ],
        );
        let section = map_section(&node, "/p").expect("section").expect("modeled");
        assert_eq!(section.name.as_str(), "Geometry");
        // The section-level `NoFill` cell maps to `Section::cells`.
        assert_eq!(section.cells.len(), 1);
        assert_eq!(section.cells[0].name.as_str(), "NoFill");
        assert_eq!(section.rows.len(), 1);
        assert_eq!(section.rows[0].row_type.as_deref(), Some("RelMoveTo"));
        assert_eq!(section.rows[0].ix, Some(1));
        assert_eq!(section.rows[0].cells.len(), 1);
        // `IX` has no field on `Section`; it stays preserved.
        assert_eq!(section.preserved.attributes.len(), 1);
        assert_eq!(section.preserved.attributes[0].name.local, "IX");
        assert!(section.preserved.children.is_empty());
    }

    #[test]
    fn a_section_without_a_name_is_kept_verbatim() {
        assert!(
            map_section(&element("Section", &[], Vec::new()), "/p")
                .expect("section")
                .is_none()
        );
    }

    #[test]
    fn a_row_without_an_index_keeps_none() {
        let row = map_row(&element("Row", &[], Vec::new()), "/p")
            .expect("row")
            .expect("modeled");
        assert_eq!(row.name, None);
        assert_eq!(row.ix, None);
        assert_eq!(row.cells.len(), 0);
    }

    #[test]
    fn a_bad_row_index_is_an_error() {
        assert!(matches!(
            map_row(&element("Row", &[("IX", "x")], Vec::new()), "/p"),
            Err(Error::InvalidPackage(_))
        ));
    }

    #[test]
    fn map_sheet_puts_unknown_children_in_preserved() {
        let node = element(
            "Shape",
            &[],
            vec![
                XmlChild::Element(cell(&[("N", "PinX"), ("V", "2")])),
                XmlChild::Element(element("Text", &[], Vec::new())),
                XmlChild::Element(element("Section", &[], Vec::new())),
                XmlChild::Element(element("Shapes", &[], Vec::new())),
            ],
        );
        let mut sheet = Sheet::new();
        let mut preserved = Preserved::new();
        map_sheet(&node, &mut sheet, &mut preserved, "/p", &["Shapes"]).expect("sheet");
        assert_eq!(sheet.cells.len(), 1);
        assert_eq!(sheet.sections.len(), 0);
        let names: Vec<&str> = preserved
            .children
            .iter()
            .map(|child| child.name.local.as_str())
            .collect();
        // `Shapes` is mapped by the caller and left alone.
        assert_eq!(names, ["Text", "Section"]);
    }

    #[test]
    fn values_map_by_declared_kind() {
        let cell = map_cell(&cell(&[("N", "Fill"), ("V", "#4d4d4d")]), "/p")
            .expect("cell")
            .expect("modeled");
        assert_eq!(cell.value, Some(CellValue::Color(Color::new(77, 77, 77))));
    }
}
