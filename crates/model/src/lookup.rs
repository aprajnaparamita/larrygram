//! The inheritance lookup API (ADR 0013).
//!
//! Master/instance and stylesheet inheritance are resolved through this
//! lookup, never flattened on load. A lookup returns where the value came
//! from, so a caller can show provenance ("from master", "from style
//! `Normal`"). A cyclic style chain is cut and reported in `via`, never
//! looped on.
//!
//! Resolution order (ADR 0013): the shape's own cell (**local**), the
//! master's corresponding cell (**master**, with the sub-shape rule), the
//! style chain selected by the cell's section (**style**), the ShapeSheet
//! table's `default_value` (**section default**) and the default of the
//! cell's type (**hard default**). The style rung starts at the effective
//! style link (P6-15): the shape's own `LineStyle`/`FillStyle`/`TextStyle`,
//! or its master chain's first link of that kind when the shape sets none.
//! Every consulted source appears in [`Resolved::via`]; the entry that
//! defines the value is last.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use larrygram_geom::Unit;

use crate::cell::{Cell, CellValue, Color};
use crate::document::Document;
use crate::generated::{CELLS, CellDef};
use crate::id::{MasterId, ShapeId, StyleId};
use crate::name::CellName;
use crate::section::Section;
use crate::sheet::Sheet;

/// Where a resolved cell value came from (ADR 0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueSource {
    /// The shape's own cell.
    Local,
    /// The cell of the master shape this shape is an instance of.
    Master(MasterId),
    /// A cell of a stylesheet row in the style chain.
    Style(StyleId),
    /// The default value of the cell in the ShapeSheet table
    /// (`ui-spec/shapesheet.toml`, ADR 0008).
    SectionDefault,
    /// The hard default of the cell's type, when nothing else defines it.
    HardDefault,
}

/// The result of a lookup: the resolved value and its provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<'a> {
    /// The resolved cached value, or `None` when no source defines the cell.
    pub value: Option<&'a CellValue>,
    /// Where the value came from.
    pub source: ValueSource,
    /// The formula of the source that defines the cell, if any.
    pub formula: Option<&'a str>,
    /// Every source consulted, in resolution order (ADR 0013). The last
    /// entry is `source`. A cyclic style chain appears here as a repeated
    /// [`ValueSource`] and the chain stops there.
    pub via: Vec<ValueSource>,
}

impl<'a> Resolved<'a> {
    /// Creates a resolution with no value, no formula and no chain.
    pub fn empty(source: ValueSource) -> Self {
        Self {
            value: None,
            source,
            formula: None,
            via: Vec::new(),
        }
    }
}

/// Resolves cell values through master/instance and stylesheet inheritance
/// (ADR 0013).
pub trait CellLookup {
    /// Resolves `cell` on `shape`, following the resolution order of
    /// ADR 0013: the shape's own cell, the master's cell, the style chain
    /// (each style's own chain included), the section default, the hard
    /// default.
    ///
    /// Returns [`crate::Error::UnknownShape`] when `shape` is not an arena
    /// id of the document. A cycle never errors: the chain stops at the
    /// first repeated source (ADR 0013).
    fn resolve(&self, shape: ShapeId, cell: &CellName) -> Result<Resolved<'_>, crate::Error>;
}

impl CellLookup for Document {
    fn resolve(&self, shape: ShapeId, cell: &CellName) -> Result<Resolved<'_>, crate::Error> {
        self.resolve_with_table(shape, cell, CELLS)
    }
}

impl Document {
    /// Resolves one cell of one row of a repeated section (ADR 0019).
    ///
    /// `row` is the row's position in the section, and `section` names the
    /// section (the first section with that name when a sheet has several).
    /// The rungs are the local row's cell, the master's row at the same
    /// position in the same section (matched by `IX` when both sides have
    /// one, else by position), **no style rung** (a `Geometry` cell never
    /// comes from a style, ADR 0019 decision 5), the section default and
    /// the hard default. `Inh` does not define (ADR 0013 amendment);
    /// cycles are cut as in [`CellLookup::resolve`].
    pub fn resolve_row_cell(
        &self,
        sheet: crate::values::SheetRef,
        section: &str,
        row: usize,
        cell: &CellName,
    ) -> Result<Resolved<'_>, crate::Error> {
        self.resolve_repeated_with_table(
            sheet,
            &RepeatedCell {
                section,
                section_ix: None,
                section_pos: Some(0),
                row: Some(row),
                row_ix: None,
            },
            cell,
            CELLS,
        )
    }

    /// Resolves a page-sheet cell (ADR 0019): the page's own cell, the
    /// style chain selected by the cell's kind (through the page sheet's
    /// `LineStyle`/`FillStyle`/`TextStyle` links, which the read side keeps
    /// in [`Preserved`](crate::Preserved)), the section default and the
    /// hard default.
    pub fn resolve_page_cell(
        &self,
        page: crate::id::PageId,
        cell: &CellName,
    ) -> Result<Resolved<'_>, crate::Error> {
        self.resolve_page_cell_with_table(page, cell, CELLS)
    }

    /// The page size in inches, resolved as **one pair** (the P7-27
    /// amendment of ADR 0019): `PageWidth` and `PageHeight` come from the
    /// same sheet — the page's own sheet first, then the sheets of its
    /// style chain (the `LineStyle`/`FillStyle`/`TextStyle` links, each
    /// chain followed as in [`Document::resolve_page_cell`]) — else the
    /// [`DEFAULT_PAGE_SIZE`] pair.
    ///
    /// A rung sheet supplies the pair when at least one of the two cells
    /// is a positive finite number there; the other axis then takes that
    /// axis of [`DEFAULT_PAGE_SIZE`]. Both axes therefore never mix
    /// sheets (no width from the page and height from a style). There is
    /// no background-page rung: no allowed source ([MS-VSDX] 2.2.2.4 and
    /// 2.2.2.5, the ShapeSheet reference's `PageWidth`/`PageHeight` cells)
    /// says a foreground page takes its size from its background page.
    ///
    /// Every reader (the scene builder, the canvas, zoom-to-fit and the
    /// Page Setup dialog) calls this one helper, so they cannot disagree.
    /// An unknown page id answers the default pair (a caller bug, not
    /// file data: never an error at the size).
    pub fn page_size(&self, page: crate::id::PageId) -> (f64, f64) {
        let Some(entity) = self.page(page) else {
            return DEFAULT_PAGE_SIZE;
        };
        if let Some(size) = sheet_page_size(&entity.sheet) {
            return size;
        }
        let mut visited_styles: Vec<StyleId> = Vec::new();
        for kind in [StyleKind::Line, StyleKind::Fill, StyleKind::Text] {
            let mut link = page_style_link(entity, kind);
            while let Some(visio_id) = link {
                let Some(style) = self.style_by_visio_id(visio_id) else {
                    // A dangling style reference ends the chain.
                    break;
                };
                let id = style.id;
                if visited_styles.contains(&id) {
                    // A cyclic style chain: stop it.
                    break;
                }
                visited_styles.push(id);
                if let Some(size) = sheet_page_size(&style.sheet) {
                    return size;
                }
                link = match kind {
                    StyleKind::Line => style.line_style,
                    StyleKind::Fill => style.fill_style,
                    StyleKind::Text => style.text_style,
                };
            }
        }
        DEFAULT_PAGE_SIZE
    }
}

/// The default page size: US Letter portrait, in inches. A page that
/// defines no positive `PageWidth`/`PageHeight` on any rung of
/// [`Document::page_size`] gets this pair (files without a page sheet
/// exist in the corpus; a zero-size scene cannot be rasterized).
pub const DEFAULT_PAGE_SIZE: (f64, f64) = (8.5, 11.0);

/// The page-size pair one sheet defines ([`Document::page_size`]):
/// `Some` when at least one of `PageWidth` and `PageHeight` is a
/// positive finite number there. The axis the sheet does not supply
/// takes that axis of [`DEFAULT_PAGE_SIZE`], so both axes come from one
/// sheet.
fn sheet_page_size(sheet: &Sheet) -> Option<(f64, f64)> {
    let axis = |name: &str| -> Option<f64> {
        let (cell, _) = sheet_cell(sheet, &CellName::new(name))?;
        if !defines(cell) {
            return None;
        }
        match cell.value {
            Some(CellValue::Number(value)) if value.is_finite() && value > 0.0 => Some(value),
            _ => None,
        }
    };
    let width = axis("PageWidth");
    let height = axis("PageHeight");
    if width.is_none() && height.is_none() {
        return None;
    }
    Some((
        width.unwrap_or(DEFAULT_PAGE_SIZE.0),
        height.unwrap_or(DEFAULT_PAGE_SIZE.1),
    ))
}

/// The identity of one cell of a repeated section for a lookup: the
/// section by name, `IX` and position among same-named sections, and the
/// row by position and `IX` (ADR 0019 decision 5). `row: None, row_ix:
/// None` names a section-level cell (the `NoFill` family of `Geometry`);
/// `row: None, row_ix: Some` names a row by its `IX` alone (the text run
/// markers).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RepeatedCell<'a> {
    /// The section name as written.
    pub(crate) section: &'a str,
    /// The section's `IX`, when both sides have one.
    pub(crate) section_ix: Option<i32>,
    /// The section's position among the same-named sections of its sheet.
    pub(crate) section_pos: Option<usize>,
    /// The row's position in the section, or `None` for a section-level
    /// cell or a row named by its `IX` alone.
    pub(crate) row: Option<usize>,
    /// The row's `IX`, when both sides have one.
    pub(crate) row_ix: Option<i32>,
}

impl Document {
    /// Resolves one cell of one row of a repeated section against `table`,
    /// identifying the section by name and by its `IX` / position among
    /// same-named sections (so a sheet with several `Geometry` sections
    /// resolves each section's rows against the master's matching section).
    ///
    /// `row` is the row's position in the section and `row_ix` the row's own
    /// `IX` (used to match the master's row "by `IX` when both sides have
    /// one", ADR 0019 decision 5). `row: None, row_ix: None` resolves a
    /// section-level cell (the `NoFill` family of `Geometry`); `row: None,
    /// row_ix: Some` resolves the row with that `IX` wherever it sits (the
    /// text run markers).
    pub(crate) fn resolve_repeated_with_table(
        &self,
        sheet: crate::values::SheetRef,
        repeated: &RepeatedCell<'_>,
        cell: &CellName,
        table: &[CellDef],
    ) -> Result<Resolved<'_>, crate::Error> {
        let entity_sheet = self.sheet_of(sheet)?;
        let mut via = Vec::new();

        // The local section and the row's `IX` when the caller did not pass
        // one: the master rung matches "by `IX` when both sides have one".
        let local_section = match_repeated_section(
            entity_sheet,
            repeated.section,
            repeated.section_ix,
            repeated.section_pos,
        );
        let row_ix = match (repeated.row_ix, repeated.row) {
            (None, Some(row)) => local_section
                .and_then(|local| local.rows.get(row))
                .and_then(|local_row| local_row.ix),
            (row_ix, _) => row_ix,
        };

        // 1. Local: the row's own cell (or the section's own cell). A row
        //    is matched here as in the master rung below: by `IX` when both
        //    sides have one, else by position (ADR 0019 decision 5).
        via.push(ValueSource::Local);
        if let Some(found) =
            local_section.and_then(|local| matched_row_cell(local, repeated.row, row_ix, cell))
            && defines(found)
        {
            return Ok(Resolved {
                value: found.value.as_ref(),
                source: ValueSource::Local,
                formula: found.formula.as_deref(),
                via,
            });
        }

        // 2. Master chain, with the sub-shape rule (ADR 0013) and the
        //    position rule for masters with several top-level shapes
        //    (P2-06b decision 2). There is no style rung: a repeated-section
        //    cell like `Geometry`'s never comes from a style (ADR 0019
        //    decision 5).
        if let crate::values::SheetRef::Shape(shape) = sheet {
            let mut visited_masters: Vec<MasterId> = Vec::new();
            if let Some((first, root, mut path)) = self.instance_chain_start(shape) {
                let position = self.sibling_position(root);
                let mut current = first;
                loop {
                    if visited_masters.contains(&current) {
                        // A cyclic master chain: record the repeat and stop it.
                        via.push(ValueSource::Master(current));
                        break;
                    }
                    visited_masters.push(current);
                    via.push(ValueSource::Master(current));
                    let mut chained = None;
                    if let Some(content) = self
                        .master(current)
                        .and_then(|m| self.master_shape(m, position))
                    {
                        let target = walk_down(self, content, &path).unwrap_or(content);
                        if let Some(found) = match_repeated_section(
                            &target.sheet,
                            repeated.section,
                            repeated.section_ix,
                            repeated.section_pos,
                        )
                        .and_then(|master_section| {
                            matched_row_cell(master_section, repeated.row, row_ix, cell)
                        })
                        .filter(|found| defines(found))
                        {
                            return Ok(Resolved {
                                value: found.value.as_ref(),
                                source: ValueSource::Master(current),
                                formula: found.formula.as_deref(),
                                via,
                            });
                        }
                        // A master shape that is itself an instance continues
                        // the chain at that master's own root.
                        chained = target.master;
                        path.clear();
                    }
                    match chained {
                        Some(next) => current = next,
                        None => break,
                    }
                }
            }
        }

        // 3. No style rung (ADR 0019 decision 5).
        Ok(fall_through(
            table,
            cell.as_str(),
            Some(repeated.section),
            via,
        ))
    }

    /// Resolves a page-sheet cell against `table` (ADR 0019): the page's own
    /// cell, the style chain selected by the cell's kind, the section
    /// default, the hard default.
    pub(crate) fn resolve_page_cell_with_table(
        &self,
        page: crate::id::PageId,
        cell: &CellName,
        table: &[CellDef],
    ) -> Result<Resolved<'_>, crate::Error> {
        let entity = self.page(page).ok_or(crate::Error::UnknownPage(page))?;
        let mut via = Vec::new();
        // The section the cell lives in, when a find saw it; it selects the
        // style and disambiguates table entries.
        let mut section_hint: Option<&str> = None;

        // 1. Local: the cell on the page sheet itself.
        via.push(ValueSource::Local);
        if let Some((found, hint)) = sheet_cell(&entity.sheet, cell) {
            section_hint = hint;
            if defines(found) {
                return Ok(Resolved {
                    value: found.value.as_ref(),
                    source: ValueSource::Local,
                    formula: found.formula.as_deref(),
                    via,
                });
            }
        }

        // 2. Style chain from the page sheet's style links (P2-06b decision
        //    3); the links are read from the preserved `PageSheet`
        //    attributes (ADR 0019 decision 1, read-side only).
        let mut visited_styles: Vec<StyleId> = Vec::new();
        if let Some(kind) = style_kind_for(table, cell.as_str(), section_hint) {
            let mut link = page_style_link(entity, kind);
            while let Some(visio_id) = link {
                let Some(style) = self.style_by_visio_id(visio_id) else {
                    // A dangling style reference ends the chain.
                    break;
                };
                let id = style.id;
                if visited_styles.contains(&id) {
                    // A cyclic style chain: record the repeat and stop it.
                    via.push(ValueSource::Style(id));
                    break;
                }
                visited_styles.push(id);
                via.push(ValueSource::Style(id));
                if let Some((found, hint)) = sheet_cell(&style.sheet, cell) {
                    section_hint = hint;
                    if defines(found) {
                        return Ok(Resolved {
                            value: found.value.as_ref(),
                            source: ValueSource::Style(id),
                            formula: found.formula.as_deref(),
                            via,
                        });
                    }
                }
                link = match kind {
                    StyleKind::Line => style.line_style,
                    StyleKind::Fill => style.fill_style,
                    StyleKind::Text => style.text_style,
                };
            }
        }

        // 3. Section default from the ShapeSheet table, 4. hard default.
        Ok(fall_through(table, cell.as_str(), section_hint, via))
    }

    /// The sheet of a [`SheetRef`](crate::values::SheetRef), or the id error
    /// of a stale reference (a caller bug, not file data).
    pub(crate) fn sheet_of(&self, sheet: crate::values::SheetRef) -> Result<&Sheet, crate::Error> {
        match sheet {
            crate::values::SheetRef::Shape(shape) => self
                .shape(shape)
                .map(|entity| &entity.sheet)
                .ok_or(crate::Error::UnknownShape(shape)),
            crate::values::SheetRef::Page(page) => self
                .page(page)
                .map(|entity| &entity.sheet)
                .ok_or(crate::Error::UnknownPage(page)),
        }
    }
}

impl Document {
    /// Resolves `cell` on `shape` against `table`, the ShapeSheet cell
    /// catalogue (the generated table in production).
    ///
    /// The table is a parameter so the section-default rung can be tested
    /// with a table that records `default_value` strings; the shipped
    /// catalogue (`ui-spec/shapesheet.toml`) records none yet.
    pub(crate) fn resolve_with_table(
        &self,
        shape: ShapeId,
        cell: &CellName,
        table: &[CellDef],
    ) -> Result<Resolved<'_>, crate::Error> {
        let entity = self.shape(shape).ok_or(crate::Error::UnknownShape(shape))?;
        let mut via = Vec::new();
        // The section the cell lives in, when a find saw it; it selects the
        // style and disambiguates table entries.
        let mut section_hint: Option<&str> = None;

        // 1. Local: the cell on the shape itself.
        via.push(ValueSource::Local);
        if let Some((found, hint)) = sheet_cell(&entity.sheet, cell) {
            section_hint = hint;
            if defines(found) {
                return Ok(Resolved {
                    value: found.value.as_ref(),
                    source: ValueSource::Local,
                    formula: found.formula.as_deref(),
                    via,
                });
            }
        }

        // 2. Master chain, with the sub-shape rule (ADR 0013) and the
        // position rule for masters with several top-level shapes (P2-06b
        // decision 2).
        let mut visited_masters: Vec<MasterId> = Vec::new();
        if let Some((first, root, mut path)) = self.instance_chain_start(shape) {
            let position = self.sibling_position(root);
            let mut current = first;
            loop {
                if visited_masters.contains(&current) {
                    // A cyclic master chain: record the repeat and stop it.
                    via.push(ValueSource::Master(current));
                    break;
                }
                visited_masters.push(current);
                via.push(ValueSource::Master(current));
                let mut chained = None;
                if let Some(content) = self
                    .master(current)
                    .and_then(|m| self.master_shape(m, position))
                {
                    let target = walk_down(self, content, &path).unwrap_or(content);
                    if let Some((found, hint)) = sheet_cell(&target.sheet, cell) {
                        section_hint = hint;
                        if defines(found) {
                            return Ok(Resolved {
                                value: found.value.as_ref(),
                                source: ValueSource::Master(current),
                                formula: found.formula.as_deref(),
                                via,
                            });
                        }
                    }
                    // A master shape that is itself an instance continues
                    // the chain at that master's own root.
                    chained = target.master;
                    path.clear();
                }
                match chained {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }

        // 3. Style chain selected by the cell's section (a Geometry cell
        // never comes from a style). A style reference is a Visio style id
        // (P2-06b decision 3) and each style continues the chain through
        // its own link of the same kind. The first link is the **effective**
        // one (P6-15): the shape's own link, or the master chain's first
        // link of this kind when the shape sets none.
        let mut visited_styles: Vec<StyleId> = Vec::new();
        if let Some(kind) = style_kind_for(table, cell.as_str(), section_hint) {
            let mut link = self.effective_style_link(shape, kind);
            while let Some(visio_id) = link {
                let Some(style) = self.style_by_visio_id(visio_id) else {
                    // A dangling style reference ends the chain.
                    break;
                };
                let id = style.id;
                if visited_styles.contains(&id) {
                    // A cyclic style chain: record the repeat and stop it.
                    via.push(ValueSource::Style(id));
                    break;
                }
                visited_styles.push(id);
                via.push(ValueSource::Style(id));
                if let Some((found, hint)) = sheet_cell(&style.sheet, cell) {
                    section_hint = hint;
                    if defines(found) {
                        return Ok(Resolved {
                            value: found.value.as_ref(),
                            source: ValueSource::Style(id),
                            formula: found.formula.as_deref(),
                            via,
                        });
                    }
                }
                link = match kind {
                    StyleKind::Line => style.line_style,
                    StyleKind::Fill => style.fill_style,
                    StyleKind::Text => style.text_style,
                };
            }
        }

        // 4. Section default from the ShapeSheet table, 5. hard default.
        Ok(fall_through(table, cell.as_str(), section_hint, via))
    }

    /// The master chain that applies to `shape`: the master of the nearest
    /// shape that is an instance (the shape itself or an ancestor), that
    /// instance root, and the child positions from the root down to `shape`.
    ///
    /// Returns `None` when no ancestor is a master instance, and when the
    /// parent chain is missing or cyclic (a malformed file must not loop).
    pub(crate) fn instance_chain_start(
        &self,
        shape: ShapeId,
    ) -> Option<(MasterId, ShapeId, Vec<usize>)> {
        let mut path = Vec::new();
        let mut visited = Vec::new();
        let mut current = shape;
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let shape = self.shape(current)?;
            if let Some(master) = shape.master {
                path.reverse();
                return Some((master, current, path));
            }
            let parent = shape.parent?;
            let position = self
                .shape(parent)?
                .children
                .iter()
                .position(|c| *c == current)?;
            path.push(position);
            current = parent;
        }
    }

    /// The position of `shape` among its siblings: its parent's children, or
    /// the page's top-level shapes when it has no parent. `None` when the
    /// shape is unreachable from any page (a malformed file).
    pub(crate) fn sibling_position(&self, shape: ShapeId) -> Option<usize> {
        let node = self.shape(shape)?;
        if let Some(parent) = node.parent {
            return self
                .shape(parent)?
                .children
                .iter()
                .position(|c| *c == shape);
        }
        self.pages
            .iter()
            .find_map(|page| page.shapes.iter().position(|s| *s == shape))
    }

    /// The master shape an instance at `position` matches (P2-06b decision
    /// 2): the master's only shape when it has exactly one top-level shape,
    /// the shape at `position` otherwise. A position the master does not
    /// have degrades to the master's first shape.
    pub(crate) fn master_shape(
        &self,
        master: &crate::master::Master,
        position: Option<usize>,
    ) -> Option<&crate::shape::Shape> {
        match master.shapes.as_slice() {
            [] => None,
            [only] => self.shape(*only),
            many => {
                let picked = position
                    .and_then(|p| many.get(p))
                    .or_else(|| many.first())?;
                self.shape(*picked)
            }
        }
    }

    /// The style link the style rung starts from (P6-15, ADR 0013 rule 3):
    /// the shape's own `LineStyle`/`FillStyle`/`TextStyle` when it sets one,
    /// else the first link of that kind on its master chain: the same
    /// master shapes the master rung consults (the sub-shape rule and the
    /// position rule included), in rung order, so a master that is itself
    /// an instance continues the search at its own master.
    ///
    /// A link the shape sets wins, even one that names no stylesheet row
    /// (the style chain then ends where it starts, as for any dangling
    /// link). A cyclic master chain is cut as in [`CellLookup::resolve`]:
    /// no link is taken from a master already visited.
    fn effective_style_link(&self, shape: ShapeId, kind: StyleKind) -> Option<u32> {
        if let Some(link) = self
            .shape(shape)
            .and_then(|entity| shape_style_link(entity, kind))
        {
            return Some(link);
        }
        let (first, root, mut path) = self.instance_chain_start(shape)?;
        let position = self.sibling_position(root);
        let mut visited: Vec<MasterId> = Vec::new();
        let mut current = first;
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let mut chained = None;
            if let Some(content) = self
                .master(current)
                .and_then(|m| self.master_shape(m, position))
            {
                let target = walk_down(self, content, &path).unwrap_or(content);
                if let Some(link) = shape_style_link(target, kind) {
                    return Some(link);
                }
                chained = target.master;
                path.clear();
            }
            match chained {
                Some(next) => current = next,
                None => return None,
            }
        }
    }

    /// The shapes the master rungs of the lookup of
    /// [`Document::resolve_row_cell`] visit, in rung order (ADR 0013): the
    /// sub-shape the instance's path walks down to (or the master's own
    /// shape), once per rung. A cyclic master chain is cut as in the
    /// lookup, and at most `max_depth` rungs are visited (the caller
    /// passes its own recursion cap).
    ///
    /// The first rung is the instance's own master; a nested master
    /// continues the chain (P2-06b decision 2). An instance of no master
    /// yields no shapes.
    pub fn master_chain_shapes(&self, shape: ShapeId, max_depth: usize) -> Vec<ShapeId> {
        let mut targets = Vec::new();
        let Some((first, root, mut path)) = self.instance_chain_start(shape) else {
            return targets;
        };
        let position = self.sibling_position(root);
        let mut visited: Vec<MasterId> = Vec::new();
        let mut current = first;
        loop {
            if visited.contains(&current) || targets.len() >= max_depth {
                break;
            }
            visited.push(current);
            let mut chained = None;
            if let Some(content) = self
                .master(current)
                .and_then(|m| self.master_shape(m, position))
            {
                let target = walk_down(self, content, &path).unwrap_or(content);
                targets.push(target.id);
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
}

/// The corresponding shape of `root` at the child positions `path`, or
/// `None` when the structure does not match. The walk is bounded by
/// `path.len()`.
pub(crate) fn walk_down<'a>(
    doc: &'a Document,
    root: &'a crate::shape::Shape,
    path: &[usize],
) -> Option<&'a crate::shape::Shape> {
    let mut current = root;
    for position in path {
        let child = *current.children.get(*position)?;
        current = doc.shape(child)?;
    }
    Some(current)
}

/// The formula `.vsdx` writes on a cell whose value is inherited. Its cached
/// value is only a snapshot of the inherited one.
const INHERIT_FORMULA: &str = "Inh";

/// The last two rungs of every lookup (ADR 0013): the ShapeSheet table's
/// `default_value` (rung 4) and the default of the cell's type (rung 5).
fn fall_through(
    table: &[CellDef],
    name: &str,
    section_hint: Option<&str>,
    mut via: Vec<ValueSource>,
) -> Resolved<'static> {
    via.push(ValueSource::SectionDefault);
    let def = find_def(table, name, section_hint);
    if let Some(value) = def
        .and_then(|d| d.default_value.map(|text| (text, d)))
        .and_then(|(text, d)| interned_default(text, d.default_unit, d.kind))
    {
        return Resolved {
            value: Some(value),
            source: ValueSource::SectionDefault,
            formula: None,
            via,
        };
    }
    via.push(ValueSource::HardDefault);
    let value = def.and_then(|d| hard_default(d.kind));
    Resolved {
        value,
        source: ValueSource::HardDefault,
        formula: None,
        via,
    }
}

/// The section of `sheet` that matches a repeated section of the same name,
/// matched by `IX` when both sides have one, else by position among the
/// same-named sections (ADR 0019 decision 5).
pub(crate) fn match_repeated_section<'a>(
    sheet: &'a Sheet,
    name: &str,
    section_ix: Option<i32>,
    section_pos: Option<usize>,
) -> Option<&'a Section> {
    let same_name = || {
        sheet
            .sections
            .iter()
            .filter(|section| section.name.as_str() == name)
    };
    if let Some(section_ix) = section_ix
        && let Some(found) = same_name().find(|section| section_ix_of(section) == Some(section_ix))
    {
        return Some(found);
    }
    match section_pos {
        Some(position) => same_name().nth(position),
        None => same_name().next(),
    }
}

/// The `IX` attribute of a section, kept in its preserved attributes
/// (`Section` has no field for it).
pub(crate) fn section_ix_of(section: &Section) -> Option<i32> {
    section
        .preserved
        .attributes
        .iter()
        .find(|attribute| attribute.name.local == "IX")
        .and_then(|attribute| attribute.value.trim().parse::<i32>().ok())
}

/// The cell of the row that matches a row at `row` with `IX` `row_ix`: by
/// `IX` when both sides have one, else by position (ADR 0019 decision 5).
///
/// A row whose `IX` differs from `row_ix` never matches by position: both
/// sides have an `IX` and they disagree. A positional match applies only
/// when the row at `row` has no `IX` of its own.
///
/// `row: None, row_ix: Some` names a row by its `IX` alone (the text run
/// markers of `CellReader::row_*`): the row with that `IX` matches and
/// nothing else. `row: None, row_ix: None` names the section itself (a
/// section-level cell like `NoFill`).
fn matched_row_cell<'a>(
    section: &'a Section,
    row: Option<usize>,
    row_ix: Option<i32>,
    cell: &CellName,
) -> Option<&'a Cell> {
    let matched = match (row, row_ix) {
        (None, None) => {
            return section.cells.iter().find(|c| c.name == *cell);
        }
        (None, Some(row_ix)) => section
            .rows
            .iter()
            .find(|candidate| candidate.ix == Some(row_ix)),
        (Some(row), None) => section.rows.get(row),
        (Some(row), Some(row_ix)) => {
            match section
                .rows
                .iter()
                .find(|candidate| candidate.ix == Some(row_ix))
            {
                Some(found) => Some(found),
                None => section
                    .rows
                    .get(row)
                    .filter(|candidate| candidate.ix.is_none()),
            }
        }
    };
    matched?.cells.iter().find(|c| c.name == *cell)
}

/// The style link of a shape for one style kind (P2-06b decision 3): the
/// `LineStyle`/`FillStyle`/`TextStyle` attribute of the shape, the Visio
/// `ID` of the stylesheet row it names.
fn shape_style_link(shape: &crate::shape::Shape, kind: StyleKind) -> Option<u32> {
    match kind {
        StyleKind::Line => shape.line_style,
        StyleKind::Fill => shape.fill_style,
        StyleKind::Text => shape.text_style,
    }
}

/// The style link of a page sheet for one style kind (ADR 0019): the
/// `LineStyle`/`FillStyle`/`TextStyle` attribute of the `PageSheet`
/// element, which the read side keeps in the page's preserved attributes.
fn page_style_link(page: &crate::page::Page, kind: StyleKind) -> Option<u32> {
    let wanted = match kind {
        StyleKind::Line => "LineStyle",
        StyleKind::Fill => "FillStyle",
        StyleKind::Text => "TextStyle",
    };
    page.preserved
        .attributes
        .iter()
        .find(|attribute| attribute.name.local == wanted)
        .and_then(|attribute| attribute.value.trim().parse::<u32>().ok())
}

/// Whether a cell defines the value of its source: it carries a formula or
/// a cached value (ADR 0013, "the first that defines the cell"). A cell
/// whose formula is `Inh` does not define it; the lookup continues to the
/// master, style and default sources.
fn defines(cell: &Cell) -> bool {
    if cell.formula.as_deref() == Some(INHERIT_FORMULA) {
        return false;
    }
    cell.value.is_some() || cell.formula.is_some()
}

/// The cell named `name` in `sheet` and the section it lives in (`None` for
/// a direct cell). Direct cells come first, then each section's own cells
/// before its row cells, in file order (P2-06b decision 1).
fn sheet_cell<'a>(sheet: &'a Sheet, name: &CellName) -> Option<(&'a Cell, Option<&'a str>)> {
    if let Some(found) = sheet.cells.iter().find(|c| c.name == *name) {
        return Some((found, None));
    }
    for section in &sheet.sections {
        if let Some(found) = section.cells.iter().find(|c| c.name == *name) {
            return Some((found, Some(section.name.as_str())));
        }
        for row in &section.rows {
            if let Some(found) = row.cells.iter().find(|c| c.name == *name) {
                return Some((found, Some(section.name.as_str())));
            }
        }
    }
    None
}

/// Which stylesheet applies to a cell of one section (ADR 0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    /// `LineStyle` selects the style for `Line Format` cells.
    Line,
    /// `FillStyle` selects the style for `Fill Format` cells.
    Fill,
    /// `TextStyle` selects the style for `Character` and `Paragraph` cells.
    Text,
}

/// The style kind of the cells of one section, or `None` when that
/// section's cells never come from a style (Geometry, Shape Transform, ...).
fn style_kind_of_section(section: &str) -> Option<StyleKind> {
    match section {
        "Line Format" | "Line" => Some(StyleKind::Line),
        "Fill Format" | "Fill" => Some(StyleKind::Fill),
        "Character" | "Paragraph" => Some(StyleKind::Text),
        _ => None,
    }
}

/// The style kind for one cell: its section when known, otherwise the kind
/// of the table sections the cell name appears in (a name shared by several
/// kinds has no style).
///
/// Names are translated from their `.vsdx` spelling to their ShapeSheet-table
/// spelling first (crate::aliases), so file spellings find the same entries.
fn style_kind_for(table: &[CellDef], name: &str, section_hint: Option<&str>) -> Option<StyleKind> {
    let name = crate::aliases::table_name(name);
    if let Some(hint) = section_hint {
        return style_kind_of_section(crate::aliases::table_name(hint));
    }
    let mut found = None;
    for def in table {
        if def.name != name {
            continue;
        }
        if let Some(kind) = style_kind_of_section(def.section) {
            if found.is_some_and(|seen| seen != kind) {
                return None;
            }
            found = Some(kind);
        }
    }
    found
}

/// The catalogue entry of one cell: its section when known, otherwise the
/// first entry with this name (the table is sorted, so the choice is
/// deterministic).
///
/// Names are translated from their `.vsdx` spelling to their ShapeSheet-table
/// spelling first (crate::aliases), so file spellings find the same entries.
fn find_def<'t>(
    table: &'t [CellDef],
    name: &str,
    section_hint: Option<&str>,
) -> Option<&'t CellDef> {
    let name = crate::aliases::table_name(name);
    let section_hint = section_hint.map(crate::aliases::table_name);
    let mut fallback = None;
    for def in table {
        if def.name != name {
            continue;
        }
        if Some(def.section) == section_hint {
            return Some(def);
        }
        if fallback.is_none() {
            fallback = Some(def);
        }
    }
    fallback
}

/// The hard default of a cell type (ADR 0013: 0, empty string, false,
/// black). A list's hard default is the empty list; a type the table does
/// not know has none.
///
/// The values are interned once so a lookup can hand out `&'static`
/// references without allocating per call.
fn hard_default(kind: &str) -> Option<&'static CellValue> {
    static NUMBER: OnceLock<CellValue> = OnceLock::new();
    static STRING: OnceLock<CellValue> = OnceLock::new();
    static BOOL: OnceLock<CellValue> = OnceLock::new();
    static COLOR: OnceLock<CellValue> = OnceLock::new();
    static LIST: OnceLock<CellValue> = OnceLock::new();
    let value = match kind {
        "Number" => NUMBER.get_or_init(|| CellValue::Number(0.0)),
        "String" => STRING.get_or_init(|| CellValue::Str(String::new())),
        "Bool" => BOOL.get_or_init(|| CellValue::Bool(false)),
        "Color" => COLOR.get_or_init(|| CellValue::Color(Color::new(0, 0, 0))),
        "List" => LIST.get_or_init(|| CellValue::List(String::new())),
        _ => return None,
    };
    Some(value)
}

/// Parses a table `default_value` and interns the result, so a lookup can
/// hand out a `&'static` reference (the borrowed value outlives the lookup).
///
/// The cache is keyed by the catalogue strings it was parsed from and is
/// bounded by their number (the table is static), so the interning leak is
/// bounded and happens once per distinct default.
fn interned_default(
    text: &'static str,
    default_unit: &'static str,
    kind: &'static str,
) -> Option<&'static CellValue> {
    static CACHE: OnceLock<DefaultCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    interned_in(cache, text, default_unit, kind)
}

/// The key of the default-value cache: the catalogue strings parsed.
type DefaultKey = (&'static str, &'static str, &'static str);

/// The default-value cache of [`interned_default`].
type DefaultCache = Mutex<HashMap<DefaultKey, &'static CellValue>>;

/// The insert-or-reuse half of [`interned_default`], over a caller-supplied
/// cache so the lock handling is testable.
///
/// One critical section: a poisoned lock is recovered with `into_inner`, and
/// the value is parsed, inserted or reused under the lock, so no parse is
/// leaked per call and concurrent first calls share one value.
fn interned_in(
    cache: &DefaultCache,
    text: &'static str,
    default_unit: &'static str,
    kind: &'static str,
) -> Option<&'static CellValue> {
    let key: DefaultKey = (text, default_unit, kind);
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = guard.get(&key) {
        return Some(*value);
    }
    let parsed = parse_default_value(text, default_unit, kind)?;
    let value = guard
        .entry(key)
        .or_insert_with(|| Box::leak(Box::new(parsed)));
    Some(*value)
}

/// Parses a `default_value` string of the ShapeSheet table into a cached
/// value in canonical units (ADR 0011), using the cell type and its
/// `default_unit` (ADR 0008).
///
/// - `"String"`: the text, without its quotes when quoted.
/// - `"Bool"`: `TRUE` or `FALSE`, case-insensitive.
/// - `"Color"`: `#RRGGBB` or `#RGB`.
/// - `"List"`: the raw text.
/// - `"Number"` (and any other type): a number literal with an optional
///   unit suffix (`"0 in"`, `"2.5"`, `"50"` for a percent cell), converted
///   to the canonical unit of the cell's `default_unit` kind.
///
/// Returns `None` when the text does not fit the type.
fn parse_default_value(text: &str, default_unit: &str, kind: &str) -> Option<CellValue> {
    let trimmed = text.trim();
    match kind {
        "String" => Some(CellValue::Str(unquote(trimmed))),
        "Bool" => match trimmed.to_ascii_uppercase().as_str() {
            "TRUE" => Some(CellValue::Bool(true)),
            "FALSE" => Some(CellValue::Bool(false)),
            _ => None,
        },
        "Color" => parse_color(trimmed),
        "List" => Some(CellValue::List(trimmed.to_owned())),
        _ => parse_number_default(trimmed, default_unit),
    }
}

/// Removes one layer of matching quotes.
pub(crate) fn unquote(text: &str) -> String {
    for quote in ['"', '\''] {
        if text.len() >= 2
            && text.starts_with(quote)
            && text.ends_with(quote)
            && let Some(inner) = text.get(1..text.len() - 1)
        {
            return inner.to_owned();
        }
    }
    text.to_owned()
}

/// Parses `#RRGGBB` or `#RGB`.
fn parse_color(text: &str) -> Option<CellValue> {
    let hex = text.strip_prefix('#')?;
    let (r, g, b) = match hex.len() {
        6 => (
            u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
            u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
            u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
        ),
        3 => (
            u8::from_str_radix(hex.get(0..1)?, 16).ok()? * 17,
            u8::from_str_radix(hex.get(1..2)?, 16).ok()? * 17,
            u8::from_str_radix(hex.get(2..3)?, 16).ok()? * 17,
        ),
        _ => return None,
    };
    Some(CellValue::Color(Color::new(r, g, b)))
}

/// Parses a numeric default: a number literal with an optional unit suffix,
/// converted to the canonical unit of the cell's `default_unit` kind
/// (`larrygram_geom::Unit` and `UnitKind`). A suffix must measure the same
/// kind as the cell's default unit.
fn parse_number_default(text: &str, default_unit: &str) -> Option<CellValue> {
    let (value, suffix) = crate::raw::parse_number_value(text)?;
    let base = unit_from_name(default_unit).unwrap_or(Unit::Unitsless);
    let unit = if suffix.trim().is_empty() {
        base
    } else {
        unit_from_name(suffix)?
    };
    let canonical = base.kind().to_canonical(unit, value).ok()?;
    Some(CellValue::Number(canonical))
}

/// The [`Unit`] a name or suffix spells, case-insensitively. The table's
/// unit-less spellings (`NoCast`, `Number`, `String`, `Color`) are plain
/// numbers ([`Unit::Unitsless`]).
fn unit_from_name(name: &str) -> Option<Unit> {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" | "nocast" | "number" | "string" | "color" | "unitsless" | "unitless" | "no" => {
            Some(Unit::Unitsless)
        }
        "in" | "inch" | "inches" => Some(Unit::Inches),
        "ft" | "foot" | "feet" => Some(Unit::Feet),
        "yd" | "yard" | "yards" => Some(Unit::Yards),
        "mi" | "mile" | "miles" => Some(Unit::Miles),
        "mm" | "millimeter" | "millimeters" => Some(Unit::Millimeters),
        "cm" | "centimeter" | "centimeters" => Some(Unit::Centimeters),
        "m" | "meter" | "meters" => Some(Unit::Meters),
        "km" | "kilometer" | "kilometers" => Some(Unit::Kilometers),
        "pt" | "point" | "points" => Some(Unit::Points),
        "pica" | "picas" => Some(Unit::Picas),
        "cicero" | "ciceros" => Some(Unit::Ciceros),
        "ang" | "angstrom" | "angstroms" => Some(Unit::Angstroms),
        "nm" | "nanometer" | "nanometers" => Some(Unit::Nanometers),
        "micron" | "microns" | "micrometer" | "micrometers" => Some(Unit::Microns),
        "millipoint" | "millipoints" => Some(Unit::Millipoints),
        "deg" | "degree" | "degrees" => Some(Unit::Degrees),
        "rad" | "radian" | "radians" => Some(Unit::Radians),
        "%" | "percent" => Some(Unit::Percent),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shape Data (`Property`) rows: the row union of a shape and its masters
// (task P6-18, the P6-09 note).
// ---------------------------------------------------------------------------

/// The section name of a shape's Shape Data rows.
pub(crate) const PROPERTY_SECTION: &str = "Property";

/// The deepest master chain [`Document::property_rows`] walks.
const MAX_PROPERTY_CHAIN: usize = 32;

/// One Shape Data (`Property`) row of a shape: its own row, or one it
/// inherits from its master chain (ADR 0019's row union at the row
/// granularity, task P6-18).
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRow {
    /// The row's key (`Prop.<row>.Value`): its `N` name, or `Row_<n>`
    /// for the row at 1-based position `n` when it has no name.
    pub key: String,
    /// The row itself (a copy: own and master rows are different shapes).
    pub row: crate::section::Row,
    /// The row's position in its section's rows.
    pub position: usize,
    /// Whether the master chain defines the row (an edit writes a local
    /// row on the instance, the P6-09 note).
    pub inherited: bool,
}

impl Document {
    /// The `Property` rows of `shape`: its own rows in file order, then
    /// the rows of its master chain (ADR 0013's rung order) that no own
    /// row claims. The shape's own row wins on a key clash; a deleted own
    /// row claims its key and hides the inherited row (the ADR 0019 row
    /// union rule). A cyclic master chain is cut as in [`CellLookup`]
    /// and the walk is bounded (32 rungs).
    pub fn property_rows(&self, shape: ShapeId) -> Vec<PropertyRow> {
        let mut rows = Vec::new();
        let mut claimed: Vec<String> = Vec::new();
        if let Some(entity) = self.shape(shape) {
            for section in &entity.sheet.sections {
                if section.name.as_str() != PROPERTY_SECTION {
                    continue;
                }
                for (position, row) in section.rows.iter().enumerate() {
                    let key = property_row_key(row, position);
                    claimed.push(key.clone());
                    if row.is_deleted() {
                        continue;
                    }
                    rows.push(PropertyRow {
                        key,
                        row: row.clone(),
                        position,
                        inherited: false,
                    });
                }
            }
        }
        for rung in self.master_chain_shapes(shape, MAX_PROPERTY_CHAIN) {
            let Some(entity) = self.shape(rung) else {
                continue;
            };
            for section in &entity.sheet.sections {
                if section.name.as_str() != PROPERTY_SECTION {
                    continue;
                }
                for (position, row) in section.rows.iter().enumerate() {
                    let key = property_row_key(row, position);
                    if row.is_deleted()
                        || claimed.iter().any(|seen| seen.eq_ignore_ascii_case(&key))
                    {
                        continue;
                    }
                    claimed.push(key.clone());
                    rows.push(PropertyRow {
                        key,
                        row: row.clone(),
                        position,
                        inherited: true,
                    });
                }
            }
        }
        rows
    }
}

/// The key of one Shape Data row: its `N` name, or `Row_<n>` for the row
/// at 1-based position `n` when it has no name.
pub(crate) fn property_row_key(row: &crate::section::Row, position: usize) -> String {
    match row.name.as_deref() {
        Some(name) => name.to_owned(),
        None => format!("Row_{}", position.saturating_add(1)),
    }
}

/// The position of the live `Property` row of `sheet` whose key is
/// `key`: a row-name match first, then the `Row_<n>` position form.
/// Names match case-insensitively (ADR 0054); a deleted row never
/// matches (it hides the inherited row instead).
pub(crate) fn find_property_row(sheet: &Sheet, key: &str) -> Option<usize> {
    let section = sheet
        .sections
        .iter()
        .find(|entry| entry.name.as_str() == PROPERTY_SECTION)?;
    let mut by_position = None;
    for (position, row) in section.rows.iter().enumerate() {
        if row.is_deleted() {
            continue;
        }
        if row
            .name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(key))
        {
            return Some(position);
        }
        if by_position.is_none()
            && row.name.is_none()
            && property_row_key(row, position).eq_ignore_ascii_case(key)
        {
            by_position = Some(position);
        }
    }
    by_position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    #[test]
    fn empty_resolution_has_no_value() {
        let r = Resolved::empty(ValueSource::HardDefault);
        assert_eq!(r.value, None);
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(r.formula, None);
        assert!(r.via.is_empty());
    }

    #[test]
    fn value_sources_carry_their_ids() {
        assert_ne!(
            ValueSource::Master(MasterId { index: 1 }),
            ValueSource::Master(MasterId { index: 2 })
        );
        assert_ne!(ValueSource::Style(StyleId { index: 1 }), ValueSource::Local);
    }

    // --- fixtures ------------------------------------------------------

    /// A cell with a cached number value.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A table whose `PinX` has a section default of `2 in`.
    const DEFAULT_TABLE: &[CellDef] = &[CellDef {
        name: "PinX",
        section: "Shape Transform",
        cell_index: Some(0),
        kind: "Number",
        default_unit: "Inches",
        default_value: Some("2 in"),
    }];

    /// A table with one cell per type and no defaults.
    const KIND_TABLE: &[CellDef] = &[
        CellDef {
            name: "Num",
            section: "Shape Transform",
            cell_index: Some(0),
            kind: "Number",
            default_unit: "Inches",
            default_value: None,
        },
        CellDef {
            name: "Str",
            section: "Character",
            cell_index: Some(0),
            kind: "String",
            default_unit: "NoCast",
            default_value: None,
        },
        CellDef {
            name: "Bool",
            section: "Protection",
            cell_index: Some(0),
            kind: "Bool",
            default_unit: "NoCast",
            default_value: None,
        },
        CellDef {
            name: "Col",
            section: "Fill Format",
            cell_index: Some(0),
            kind: "Color",
            default_unit: "Color",
            default_value: None,
        },
        CellDef {
            name: "Lst",
            section: "Shape Data",
            cell_index: Some(0),
            kind: "List",
            default_unit: "NoCast",
            default_value: None,
        },
        CellDef {
            name: "Mystery",
            section: "Miscellaneous",
            cell_index: Some(0),
            kind: "Something",
            default_unit: "NoCast",
            default_value: None,
        },
    ];

    /// Builds a document with one page and one top-level shape.
    fn document_with_shape() -> (Document, ShapeId) {
        let mut doc = Document::new();
        let id = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.pages
            .push(crate::page::Page::new(crate::id::PageId { index: 0 }, 1));
        doc.pages[0].shapes.push(id);
        (doc, id)
    }

    #[test]
    fn a_master_parent_cycle_ends_the_walk_instead_of_looping() {
        // Regression (coverage audit minor 5): the master-path walk guards
        // against a cycle in the parent chain of a malformed file.
        let (mut doc, a) = document_with_shape();
        let b = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 1 }, 2));
        // `a`'s parent is `b` and `b`'s parent is `a`: a cycle no well-formed
        // file produces.
        doc.shape_mut(a).expect("a").parent = Some(b);
        doc.shape_mut(a).expect("a").children.push(b);
        doc.shape_mut(b).expect("b").parent = Some(a);
        doc.shape_mut(b).expect("b").children.push(a);
        let r = doc
            .resolve_with_table(a, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .expect("resolves without hanging");
        // No master rung applies; the walk ends at the section default.
        assert_eq!(r.source, ValueSource::SectionDefault);
    }

    // --- the five sources, in order ------------------------------------

    #[test]
    fn local_cell_resolves_first() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id)
            .expect("shape exists")
            .sheet
            .cells
            .push(number_cell("PinX", 1.5));
        let r = doc.resolve(id, &CellName::from_static("PinX")).unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(1.5)));
        assert_eq!(r.source, ValueSource::Local);
        assert_eq!(r.via, vec![ValueSource::Local]);
    }

    #[test]
    fn local_formula_is_reported_without_a_value() {
        let (mut doc, id) = document_with_shape();
        let mut cell = Cell::new(CellName::from_static("LocPinX"));
        cell.set_formula(Some("Width*0.5".to_owned()));
        doc.shape_mut(id)
            .expect("shape exists")
            .sheet
            .cells
            .push(cell);
        let r = doc.resolve(id, &CellName::from_static("LocPinX")).unwrap();
        assert_eq!(r.value, None);
        assert_eq!(r.source, ValueSource::Local);
        assert_eq!(r.formula, Some("Width*0.5"));
    }

    #[test]
    fn empty_local_cell_does_not_define() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id)
            .expect("shape exists")
            .sheet
            .cells
            .push(Cell::new(CellName::from_static("PinX")));
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));
        assert_eq!(r.source, ValueSource::SectionDefault);
        assert_eq!(r.via, vec![ValueSource::Local, ValueSource::SectionDefault]);
    }

    #[test]
    fn inherited_local_cell_does_not_define() {
        let (mut doc, id) = document_with_shape();
        let mut cell = number_cell("PinX", 9.0);
        cell.set_formula(Some("Inh".to_owned()));
        doc.shape_mut(id)
            .expect("shape exists")
            .sheet
            .cells
            .push(cell);
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));
        assert_eq!(r.source, ValueSource::SectionDefault);
    }

    #[test]
    fn inherited_local_cell_resolves_from_the_master() {
        let (mut doc, id) = document_with_shape();
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let content = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(content)
            .expect("content exists")
            .sheet
            .cells
            .push(number_cell("PinX", 7.0));
        master.shapes = vec![content];
        doc.masters.push(master);
        let shape = doc.shape_mut(id).expect("shape exists");
        shape.master = Some(MasterId { index: mid as u32 });
        let mut cell = number_cell("PinX", 9.0);
        cell.set_formula(Some("Inh".to_owned()));
        shape.sheet.cells.push(cell);

        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(7.0)));
        assert_eq!(
            r.source,
            ValueSource::Master(MasterId { index: mid as u32 })
        );
    }

    #[test]
    fn master_cell_resolves_for_an_instance() {
        let (mut doc, id) = document_with_shape();
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let content = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(content)
            .expect("content exists")
            .sheet
            .cells
            .push(number_cell("PinX", 7.0));
        master.shapes = vec![content];
        master.name_u = "Box".to_owned();
        doc.masters.push(master);
        doc.shape_mut(id).expect("shape exists").master = Some(MasterId { index: mid as u32 });

        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(7.0)));
        assert_eq!(
            r.source,
            ValueSource::Master(MasterId { index: mid as u32 })
        );
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: mid as u32 })
            ]
        );
    }

    #[test]
    fn sub_shape_resolves_against_the_master_sub_shape_at_the_same_position() {
        let (mut doc, root) = document_with_shape();
        let sibling = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 3));
        let sub = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(root)
            .expect("root exists")
            .children
            .extend([sibling, sub]);
        doc.shape_mut(sibling).expect("sibling exists").parent = Some(root);
        doc.shape_mut(sub).expect("sub exists").parent = Some(root);

        // Master content: root with two children; only the second defines
        // PinX.
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let m_root = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let m_first = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        let m_second = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 3));
        doc.shape_mut(m_root).expect("exists").children = vec![m_first, m_second];
        doc.shape_mut(m_first).expect("exists").parent = Some(m_root);
        doc.shape_mut(m_second).expect("exists").parent = Some(m_root);
        doc.shape_mut(m_first)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 1.0));
        doc.shape_mut(m_second)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 2.0));
        master.shapes = vec![m_root];
        doc.masters.push(master);
        doc.shape_mut(root).expect("exists").master = Some(MasterId { index: mid as u32 });

        // The instance's own second child resolves to the master's second
        // child (position 1).
        let r = doc
            .resolve_with_table(sub, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));
        assert_eq!(
            r.source,
            ValueSource::Master(MasterId { index: mid as u32 })
        );

        // The instance root resolves to the master root (no definition
        // there): the section default shows the master rung was consulted.
        let r = doc
            .resolve_with_table(root, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::SectionDefault);
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: mid as u32 }),
                ValueSource::SectionDefault
            ]
        );
    }

    #[test]
    fn structure_mismatch_falls_back_to_the_master_own_sheet() {
        let (mut doc, root) = document_with_shape();
        let sibling = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 3));
        let sub = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(root)
            .expect("exists")
            .children
            .extend([sibling, sub]);
        doc.shape_mut(sibling).expect("exists").parent = Some(root);
        doc.shape_mut(sub).expect("exists").parent = Some(root);

        // The master has only one child, so the position 1 does not exist.
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let m_root = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let m_only = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(m_root).expect("exists").children = vec![m_only];
        doc.shape_mut(m_only).expect("exists").parent = Some(m_root);
        doc.shape_mut(m_root)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 9.0));
        master.shapes = vec![m_root];
        doc.masters.push(master);
        doc.shape_mut(root).expect("exists").master = Some(MasterId { index: mid as u32 });

        let r = doc
            .resolve_with_table(sub, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(9.0)));
        assert_eq!(
            r.source,
            ValueSource::Master(MasterId { index: mid as u32 })
        );
    }

    #[test]
    fn a_master_with_one_shape_matches_any_instance_position() {
        // The first page shape is a plain shape; the instance sits at
        // position 1 and still matches the master's only shape.
        let (mut doc, plain) = document_with_shape();
        let id = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.pages[0].shapes.push(id);
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let content = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(content)
            .expect("content exists")
            .sheet
            .cells
            .push(number_cell("PinX", 7.0));
        master.shapes = vec![content];
        doc.masters.push(master);
        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: mid as u32 });
        assert_eq!(doc.sibling_position(id), Some(1));
        assert_eq!(doc.sibling_position(plain), Some(0));

        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(7.0)));
        assert_eq!(
            r.source,
            ValueSource::Master(MasterId { index: mid as u32 })
        );
    }

    #[test]
    fn a_master_with_several_shapes_matches_by_position() {
        let (mut doc, first) = document_with_shape();
        let second = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.pages[0].shapes.push(second);
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let m_first = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let m_second = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(m_first)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 1.0));
        doc.shape_mut(m_second)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 2.0));
        master.shapes = vec![m_first, m_second];
        doc.masters.push(master);
        doc.shape_mut(first).expect("exists").master = Some(MasterId { index: mid as u32 });
        doc.shape_mut(second).expect("exists").master = Some(MasterId { index: mid as u32 });

        let r = doc
            .resolve_with_table(first, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(1.0)));
        let r = doc
            .resolve_with_table(second, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));
    }

    #[test]
    fn an_out_of_range_position_degrades_to_the_first_master_shape() {
        let (mut doc, _) = document_with_shape();
        let id = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.pages[0].shapes.push(id);
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(MasterId { index: mid as u32 }, 1);
        let m_first = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let m_second = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(m_first)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 1.0));
        doc.shape_mut(m_second)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 2.0));
        master.shapes = vec![m_first, m_second];
        doc.masters.push(master);
        // The instance sits at position 1, the master has a shape there;
        // a master with one shape keeps its single shape for any position.
        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: mid as u32 });
        assert_eq!(doc.sibling_position(id), Some(1));

        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));

        // A shape no page reaches has no position: the master's first shape.
        let orphan = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 3));
        doc.shape_mut(orphan).expect("exists").master = Some(MasterId { index: mid as u32 });
        assert_eq!(doc.sibling_position(orphan), None);
        let r = doc
            .resolve_with_table(orphan, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(1.0)));
    }

    #[test]
    fn nested_instance_continues_the_master_chain() {
        let (mut doc, id) = document_with_shape();
        // Master 1 content is itself an instance of master 2.
        let m2 = doc.masters.len();
        let m2_content = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(m2_content)
            .expect("exists")
            .sheet
            .cells
            .push(number_cell("PinX", 4.0));
        let mut master2 = crate::master::Master::new(MasterId { index: m2 as u32 }, 2);
        master2.shapes = vec![m2_content];
        doc.masters.push(master2);

        let m1 = doc.masters.len();
        let m1_content = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(m1_content).expect("exists").master = Some(MasterId { index: m2 as u32 });
        let mut master1 = crate::master::Master::new(MasterId { index: m1 as u32 }, 3);
        master1.shapes = vec![m1_content];
        doc.masters.push(master1);

        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: m1 as u32 });
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(4.0)));
        assert_eq!(r.source, ValueSource::Master(MasterId { index: m2 as u32 }));
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: m1 as u32 }),
                ValueSource::Master(MasterId { index: m2 as u32 })
            ]
        );
    }

    #[test]
    fn cyclic_master_chain_terminates_and_shows_the_repeat() {
        let (mut doc, id) = document_with_shape();
        // Master 0 content is an instance of master 1 and back.
        let m0 = doc.masters.len();
        let c0 = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let m1 = m0 + 1;
        let c1 = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(c0).expect("exists").master = Some(MasterId { index: m1 as u32 });
        doc.shape_mut(c1).expect("exists").master = Some(MasterId { index: m0 as u32 });
        let mut master0 = crate::master::Master::new(MasterId { index: m0 as u32 }, 1);
        master0.shapes = vec![c0];
        let mut master1 = crate::master::Master::new(MasterId { index: m1 as u32 }, 2);
        master1.shapes = vec![c1];
        doc.masters.push(master0);
        doc.masters.push(master1);
        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: m0 as u32 });

        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::SectionDefault);
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: m0 as u32 }),
                ValueSource::Master(MasterId { index: m1 as u32 }),
                ValueSource::Master(MasterId { index: m0 as u32 }),
                ValueSource::SectionDefault
            ]
        );
    }

    #[test]
    fn style_chain_resolves_through_the_style_links() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").line_style = Some(1);

        let mut red = Style::new(StyleId { index: 0 });
        red.visio_id = 1;
        red.name = "Red".to_owned();
        red.line_style = Some(0);
        doc.stylesheet.styles.push(red);

        let mut normal = Style::new(StyleId { index: 1 });
        normal.visio_id = 0;
        normal.name = "Normal".to_owned();
        normal.sheet.cells.push(number_cell("LineWeight", 0.05));
        doc.stylesheet.styles.push(normal);

        let r = doc
            .resolve(id, &CellName::from_static("LineWeight"))
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 1 }));
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::Style(StyleId { index: 1 })
            ]
        );
    }

    #[test]
    fn the_chain_follows_the_style_link_of_the_cells_kind() {
        // Style 0 links line cells to style 1 and text cells to style 2:
        // one chain per kind (P2-06b decision 3).
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").line_style = Some(0);
        doc.shape_mut(id).expect("exists").text_style = Some(0);
        let mut head = Style::new(StyleId { index: 0 });
        head.visio_id = 0;
        head.line_style = Some(1);
        head.text_style = Some(2);
        doc.stylesheet.styles.push(head);
        let mut line = Style::new(StyleId { index: 1 });
        line.visio_id = 1;
        line.sheet.cells.push(number_cell("LineWeight", 0.05));
        doc.stylesheet.styles.push(line);
        let mut text = Style::new(StyleId { index: 2 });
        text.visio_id = 2;
        text.sheet.cells.push(number_cell("Size", 12.0));
        doc.stylesheet.styles.push(text);

        let r = doc
            .resolve(id, &CellName::from_static("LineWeight"))
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 1 }));
        let r = doc.resolve(id, &CellName::from_static("Size")).unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(12.0)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 2 }));
    }

    #[test]
    fn fill_and_text_cells_use_their_own_styles() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").fill_style = Some(1);
        doc.shape_mut(id).expect("exists").text_style = Some(2);
        let mut fill = Style::new(StyleId { index: 0 });
        fill.visio_id = 1;
        fill.name = "Fill".to_owned();
        fill.sheet.cells.push(number_cell("FillForegnd", 1.0));
        doc.stylesheet.styles.push(fill);
        let mut text = Style::new(StyleId { index: 1 });
        text.visio_id = 2;
        text.name = "TextStyle".to_owned();
        text.sheet.cells.push(number_cell("Size", 12.0));
        doc.stylesheet.styles.push(text);

        let r = doc
            .resolve(id, &CellName::from_static("FillForegnd"))
            .unwrap();
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 0 }));
        let r = doc.resolve(id, &CellName::from_static("Size")).unwrap();
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 1 }));
    }

    #[test]
    fn geometry_cells_never_come_from_a_style() {
        // `X` is a Geometry cell here; the same name also appears in
        // sections with no style kind.
        const GEO_TABLE: &[CellDef] = &[
            CellDef {
                name: "X",
                section: "Geometry",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "Inches",
                default_value: None,
            },
            CellDef {
                name: "X",
                section: "Scratch",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "Inches",
                default_value: None,
            },
        ];
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").line_style = Some(1);
        let mut red = Style::new(StyleId { index: 0 });
        red.visio_id = 1;
        red.name = "Red".to_owned();
        red.sheet.cells.push(number_cell("X", 3.0));
        doc.stylesheet.styles.push(red);

        // No style is consulted: `via` holds no `Style` entry and the value
        // falls through to the hard default.
        let r = doc
            .resolve_with_table(id, &CellName::from_static("X"), GEO_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(r.value, Some(&CellValue::Number(0.0)));
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::SectionDefault,
                ValueSource::HardDefault
            ]
        );
    }

    #[test]
    fn row_cells_carry_their_section_into_the_style_lookup() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").text_style = Some(1);
        let mut text = Style::new(StyleId { index: 0 });
        text.visio_id = 1;
        text.name = "TextStyle".to_owned();
        text.sheet.cells.push(number_cell("Size", 12.0));
        doc.stylesheet.styles.push(text);

        // The local cell exists but defines nothing; its section (`Character`)
        // selects `TextStyle`.
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Character"));
        let mut row = crate::section::Row::new();
        row.cells.push(Cell::new(CellName::from_static("Size")));
        section.rows.push(row);
        doc.shape_mut(id)
            .expect("exists")
            .sheet
            .sections
            .push(section);

        let r = doc.resolve(id, &CellName::from_static("Size")).unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(12.0)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 0 }));
    }

    #[test]
    fn cyclic_style_chain_terminates_and_falls_through_to_defaults() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").line_style = Some(0);
        let mut a = Style::new(StyleId { index: 0 });
        a.visio_id = 0;
        a.name = "A".to_owned();
        a.line_style = Some(1);
        doc.stylesheet.styles.push(a);
        let mut b = Style::new(StyleId { index: 1 });
        b.visio_id = 1;
        b.name = "B".to_owned();
        b.line_style = Some(0);
        doc.stylesheet.styles.push(b);

        let r = doc
            .resolve(id, &CellName::from_static("LineWeight"))
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::Style(StyleId { index: 1 }),
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault
            ]
        );
    }

    #[test]
    fn self_referencing_style_terminates() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").fill_style = Some(0);
        let mut solo = Style::new(StyleId { index: 0 });
        solo.visio_id = 0;
        solo.name = "Solo".to_owned();
        solo.fill_style = Some(0);
        doc.stylesheet.styles.push(solo);

        let r = doc
            .resolve(id, &CellName::from_static("FillForegnd"))
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault
            ]
        );
    }

    #[test]
    fn dangling_master_and_style_references_do_not_error() {
        let (mut doc, id) = document_with_shape();
        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: 9 });
        doc.shape_mut(id).expect("exists").line_style = Some(42);
        let r = doc
            .resolve(id, &CellName::from_static("LineWeight"))
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 9 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault
            ]
        );
    }

    #[test]
    fn master_without_content_shape_is_consulted_but_defines_nothing() {
        let (mut doc, id) = document_with_shape();
        let mid = doc.masters.len();
        doc.masters.push(crate::master::Master::new(
            MasterId { index: mid as u32 },
            1,
        ));
        doc.shape_mut(id).expect("exists").master = Some(MasterId { index: mid as u32 });
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::SectionDefault);
    }

    #[test]
    fn unknown_shape_is_an_error() {
        let doc = Document::new();
        let err = doc
            .resolve(ShapeId { index: 3 }, &CellName::from_static("PinX"))
            .unwrap_err();
        assert_eq!(err, crate::Error::UnknownShape(ShapeId { index: 3 }));
    }

    #[test]
    fn cyclic_parent_chain_skips_the_master_rung() {
        let mut doc = Document::new();
        let a = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let b = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(a).expect("exists").parent = Some(b);
        doc.shape_mut(b).expect("exists").parent = Some(a);
        let r = doc
            .resolve_with_table(a, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.via, vec![ValueSource::Local, ValueSource::SectionDefault]);
    }

    #[test]
    fn broken_parent_link_skips_the_master_rung() {
        let mut doc = Document::new();
        let a = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        // The parent does not list the shape among its children.
        let p = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(a).expect("exists").parent = Some(p);
        let r = doc
            .resolve_with_table(a, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.via, vec![ValueSource::Local, ValueSource::SectionDefault]);
    }

    // --- the default rungs ---------------------------------------------

    #[test]
    fn section_default_parses_the_table_value() {
        let (doc, id) = document_with_shape();
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.value, Some(&CellValue::Number(2.0)));
        assert_eq!(r.source, ValueSource::SectionDefault);
        assert_eq!(r.formula, None);
        assert_eq!(r.via, vec![ValueSource::Local, ValueSource::SectionDefault]);
    }

    #[test]
    fn section_default_of_a_bad_literal_falls_through() {
        const BAD: &[CellDef] = &[CellDef {
            name: "PinX",
            section: "Shape Transform",
            cell_index: Some(0),
            kind: "Number",
            default_unit: "Inches",
            default_value: Some("not a number"),
        }];
        let (doc, id) = document_with_shape();
        let r = doc
            .resolve_with_table(id, &CellName::from_static("PinX"), BAD)
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(r.value, Some(&CellValue::Number(0.0)));
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::SectionDefault,
                ValueSource::HardDefault
            ]
        );
    }

    #[test]
    fn hard_defaults_match_the_cell_type() {
        let (doc, id) = document_with_shape();
        for (name, want) in [
            ("Num", CellValue::Number(0.0)),
            ("Str", CellValue::Str(String::new())),
            ("Bool", CellValue::Bool(false)),
            ("Col", CellValue::Color(Color::new(0, 0, 0))),
            ("Lst", CellValue::List(String::new())),
        ] {
            let r = doc
                .resolve_with_table(id, &CellName::new(name), KIND_TABLE)
                .unwrap();
            assert_eq!(r.source, ValueSource::HardDefault);
            assert_eq!(r.value, Some(&want), "{name}");
        }
    }

    #[test]
    fn unknown_cell_type_has_no_hard_default() {
        let (doc, id) = document_with_shape();
        let r = doc
            .resolve_with_table(id, &CellName::from_static("Mystery"), KIND_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(r.value, None);
        assert_eq!(r.formula, None);
    }

    #[test]
    fn unknown_cell_name_has_no_table_default() {
        let (doc, id) = document_with_shape();
        let r = doc
            .resolve_with_table(id, &CellName::from_static("NoSuchCell"), DEFAULT_TABLE)
            .unwrap();
        assert_eq!(r.source, ValueSource::HardDefault);
        assert_eq!(r.value, None);
    }

    // --- default parsing ------------------------------------------------

    #[test]
    fn parse_default_value_by_kind() {
        assert_eq!(
            parse_default_value("hello", "NoCast", "String"),
            Some(CellValue::Str("hello".to_owned()))
        );
        assert_eq!(
            parse_default_value("\"q\"", "NoCast", "String"),
            Some(CellValue::Str("q".to_owned()))
        );
        assert_eq!(
            parse_default_value("TRUE", "NoCast", "Bool"),
            Some(CellValue::Bool(true))
        );
        assert_eq!(
            parse_default_value("false", "NoCast", "Bool"),
            Some(CellValue::Bool(false))
        );
        assert_eq!(
            parse_default_value("#ff8000", "Color", "Color"),
            Some(CellValue::Color(Color::new(255, 128, 0)))
        );
        assert_eq!(
            parse_default_value("#f80", "Color", "Color"),
            Some(CellValue::Color(Color::new(255, 136, 0)))
        );
        assert_eq!(
            parse_default_value("1;2", "NoCast", "List"),
            Some(CellValue::List("1;2".to_owned()))
        );
    }

    #[test]
    fn parse_default_numbers_convert_to_canonical_units() {
        assert_eq!(
            parse_default_value("0.5", "Inches", "Number"),
            Some(CellValue::Number(0.5))
        );
        assert_eq!(
            parse_default_value("0.5 ft", "Inches", "Number"),
            Some(CellValue::Number(6.0))
        );
        assert_eq!(
            parse_default_value("50", "Percent", "Number"),
            Some(CellValue::Number(0.5))
        );
        assert_eq!(
            parse_default_value("2", "NoCast", "Number"),
            Some(CellValue::Number(2.0))
        );
    }

    #[test]
    fn parse_default_angle_values_are_radians() {
        // The canonical angle unit is radians (ADR 0011), so a `Degrees`
        // default converts to radians and a `Radians` default stays as
        // written. `parse_default_value` itself does not change.
        let quarter_turn = parse_default_value("90", "Degrees", "Number");
        assert!(
            matches!(
                quarter_turn,
                Some(CellValue::Number(value))
                    if (value - core::f64::consts::FRAC_PI_2).abs() <= 1.0e-12
            ),
            "{quarter_turn:?}"
        );
        assert_eq!(
            parse_default_value("1.5", "Radians", "Number"),
            Some(CellValue::Number(1.5))
        );
    }

    #[test]
    fn parse_default_rejects_bad_literals() {
        assert_eq!(parse_default_value("maybe", "NoCast", "Bool"), None);
        assert_eq!(parse_default_value("#12", "Color", "Color"), None);
        assert_eq!(parse_default_value("#xyzxyz", "Color", "Color"), None);
        assert_eq!(parse_default_value("word", "Inches", "Number"), None);
        // A suffix of a different kind than the cell's unit is rejected.
        assert_eq!(parse_default_value("1 deg", "Inches", "Number"), None);
        assert_eq!(parse_default_value("1 furlong", "Inches", "Number"), None);
    }

    #[test]
    fn unit_names_cover_the_table_and_suffix_spellings() {
        assert_eq!(unit_from_name("Inches"), Some(Unit::Inches));
        assert_eq!(unit_from_name("in"), Some(Unit::Inches));
        assert_eq!(unit_from_name("NoCast"), Some(Unit::Unitsless));
        assert_eq!(unit_from_name("Number"), Some(Unit::Unitsless));
        assert_eq!(unit_from_name("%"), Some(Unit::Percent));
        assert_eq!(unit_from_name("deg"), Some(Unit::Degrees));
        assert_eq!(unit_from_name("furlong"), None);
    }

    #[test]
    fn unquote_strips_one_layer_of_quotes() {
        assert_eq!(unquote("\"a\""), "a");
        assert_eq!(unquote("'a'"), "a");
        assert_eq!(unquote("\"\""), "");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(unquote("'\""), "'\"");
        assert_eq!(unquote("plain"), "plain");
    }

    #[test]
    fn parse_color_rejects_bad_hex() {
        assert_eq!(parse_color("nope"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn interned_defaults_are_stable_references() {
        let first = interned_default("2 in", "Inches", "Number").expect("parses");
        let second = interned_default("2 in", "Inches", "Number").expect("parses");
        assert!(std::ptr::eq(first, second));
        assert_eq!(interned_default("word", "Inches", "Number"), None);
    }

    #[test]
    fn interned_defaults_reuse_one_value_per_key_on_a_poisoned_lock() {
        // Regression (review finding 4): the old cache handed out a freshly
        // leaked value per call after a poisoned lock. The panic below is the
        // only way to poison one; its message is silenced.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let cache: DefaultCache = Mutex::new(HashMap::new());
        let first = interned_in(&cache, "2 in", "Inches", "Number").expect("parses");
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panic!("poison the cache");
        }));
        std::panic::set_hook(previous_hook);
        assert!(panicked.is_err());
        let second = interned_in(&cache, "2 in", "Inches", "Number").expect("parses");
        let third = interned_in(&cache, "2 in", "Inches", "Number").expect("parses");
        assert!(std::ptr::eq(first, second));
        assert!(std::ptr::eq(second, third));
        // A key that does not parse leaks nothing and returns none.
        assert_eq!(interned_in(&cache, "word", "Inches", "Number"), None);
        assert_eq!(interned_in(&cache, "word", "Inches", "Number"), None);
    }

    // --- sheet and style helpers ---------------------------------------

    #[test]
    fn sheet_cell_prefers_direct_cells_and_reports_row_sections() {
        let mut sheet = Sheet::new();
        sheet.cells.push(number_cell("PinX", 1.0));
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
        let mut row = crate::section::Row::new();
        row.cells.push(number_cell("X", 2.0));
        section.rows.push(row);
        sheet.sections.push(section);

        let (found, hint) = sheet_cell(&sheet, &CellName::from_static("PinX")).expect("found");
        assert_eq!(found.value.as_ref(), Some(&CellValue::Number(1.0)));
        assert_eq!(hint, None);
        let (found, hint) = sheet_cell(&sheet, &CellName::from_static("X")).expect("found");
        assert_eq!(found.value.as_ref(), Some(&CellValue::Number(2.0)));
        assert_eq!(hint, Some("Geometry"));
        assert!(sheet_cell(&sheet, &CellName::from_static("Y")).is_none());
    }

    #[test]
    fn section_level_cells_match_before_row_cells() {
        let mut sheet = Sheet::new();
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
        section.cells.push(number_cell("X", 5.0));
        let mut row = crate::section::Row::new();
        row.cells.push(number_cell("X", 2.0));
        section.rows.push(row);
        sheet.sections.push(section);

        let (found, hint) = sheet_cell(&sheet, &CellName::from_static("X")).expect("found");
        assert_eq!(found.value.as_ref(), Some(&CellValue::Number(5.0)));
        assert_eq!(hint, Some("Geometry"));
    }

    #[test]
    fn a_section_level_cell_resolves_as_a_local_cell() {
        let (mut doc, id) = document_with_shape();
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
        section.cells.push(number_cell("NoShow", 1.0));
        let mut row = crate::section::Row::new();
        row.cells.push(number_cell("NoShow", 0.0));
        section.rows.push(row);
        doc.shape_mut(id)
            .expect("exists")
            .sheet
            .sections
            .push(section);

        let r = doc
            .resolve(id, &CellName::from_static("NoShow"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(1.0)));
        assert_eq!(r.source, ValueSource::Local);
    }

    #[test]
    fn style_kind_covers_the_four_sections() {
        assert_eq!(style_kind_of_section("Line Format"), Some(StyleKind::Line));
        assert_eq!(style_kind_of_section("Fill Format"), Some(StyleKind::Fill));
        assert_eq!(style_kind_of_section("Character"), Some(StyleKind::Text));
        assert_eq!(style_kind_of_section("Paragraph"), Some(StyleKind::Text));
        assert_eq!(style_kind_of_section("Geometry"), None);
    }

    #[test]
    fn style_kind_of_a_name_shared_by_kinds_is_none() {
        // `ColorTrans` is a Character cell and something else: a name that
        // maps to two kinds selects no style.
        const TWO: &[CellDef] = &[
            CellDef {
                name: "Shared",
                section: "Character",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "NoCast",
                default_value: None,
            },
            CellDef {
                name: "Shared",
                section: "Fill Format",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "NoCast",
                default_value: None,
            },
        ];
        assert_eq!(style_kind_for(TWO, "Shared", None), None);
        assert_eq!(
            style_kind_for(TWO, "Shared", Some("Character")),
            Some(StyleKind::Text)
        );
        assert_eq!(style_kind_for(TWO, "NoSuch", None), None);
    }

    #[test]
    fn find_def_prefers_the_hinted_section() {
        const TWO: &[CellDef] = &[
            CellDef {
                name: "Shared",
                section: "Character",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "NoCast",
                default_value: Some("1"),
            },
            CellDef {
                name: "Shared",
                section: "Fill Format",
                cell_index: Some(0),
                kind: "Number",
                default_unit: "NoCast",
                default_value: Some("2"),
            },
        ];
        assert_eq!(
            find_def(TWO, "Shared", Some("Fill Format")).and_then(|d| d.default_value),
            Some("2")
        );
        assert_eq!(
            find_def(TWO, "Shared", None).and_then(|d| d.default_value),
            Some("1")
        );
        assert_eq!(find_def(TWO, "Other", None), None);
    }
}

#[cfg(test)]
mod style_link_tests {
    //! The effective style link (P6-15): an instance without style links
    //! of its own resolves style-selected cells through its master's links.

    use super::*;
    use crate::master::Master;
    use crate::page::Page;
    use crate::style::Style;

    /// A cell with a cached number value.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A stylesheet row with arena id `index`, Visio id `visio_id`, a
    /// `LineStyle` link and, when `weight` is set, a `LineWeight` of its
    /// own.
    fn line_style(index: u32, visio_id: u32, link: Option<u32>, weight: Option<f64>) -> Style {
        let mut style = Style::new(StyleId { index });
        style.visio_id = visio_id;
        style.name = format!("Style{visio_id}");
        style.line_style = link;
        if let Some(weight) = weight {
            style.sheet.cells.push(number_cell("LineWeight", weight));
        }
        style
    }

    /// A document with one master (Visio id 1) whose content shape carries
    /// `master_link`, and one instance (Visio id 2) of it on a page.
    /// Returns the document, the master's content shape and the instance.
    fn instance_of_master(master_link: Option<u32>) -> (Document, ShapeId, ShapeId) {
        let mut doc = Document::new();
        let master_shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(master_shape).expect("exists").line_style = master_link;
        doc.masters.push(Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(instance).expect("exists").master = Some(MasterId { index: 0 });
        let mut page = Page::new(crate::id::PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);
        (doc, master_shape, instance)
    }

    #[test]
    fn an_instance_inherits_the_style_link_of_its_master() {
        // Acceptance: an instance without style links resolves `LineWeight`
        // through its master's `LineStyle`.
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 0 }));
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 0 }),
                ValueSource::Style(StyleId { index: 0 })
            ]
        );
    }

    #[test]
    fn an_instance_with_its_own_link_uses_it() {
        // Acceptance: a link the instance sets wins over the master's.
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        doc.stylesheet
            .styles
            .push(line_style(1, 2, None, Some(0.1)));
        doc.shape_mut(instance).expect("exists").line_style = Some(2);
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(0.1)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 1 }));
    }

    #[test]
    fn a_dangling_link_of_the_instance_wins_over_the_masters() {
        // A link the instance sets wins even when it names no stylesheet
        // row: the style rung ends where it starts (pinned choice, P6-15).
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        doc.shape_mut(instance).expect("exists").line_style = Some(99);
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert!(
            !r.via.iter().any(|s| matches!(s, ValueSource::Style(_))),
            "the master's link is not used: {:?}",
            r.via
        );
        assert!(matches!(
            r.source,
            ValueSource::SectionDefault | ValueSource::HardDefault
        ));
    }

    #[test]
    fn the_link_search_walks_a_chained_master() {
        // A master that is itself an instance with no link of its own: the
        // search continues at the master's own master.
        let (mut doc, master_shape, instance) = instance_of_master(None);
        doc.shape_mut(master_shape).expect("exists").master = Some(MasterId { index: 1 });
        doc.masters.push(Master::new(MasterId { index: 1 }, 3));
        let outer = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 1 }, 4));
        doc.shape_mut(outer).expect("exists").line_style = Some(1);
        doc.masters[1].shapes.push(outer);
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 0 }));
    }

    #[test]
    fn a_structure_mismatch_still_reads_the_master_shapes_link() {
        // The sub-shape rule: a child resolves against the master's shape
        // at the same position; a structure mismatch degrades to the
        // master's own shape, whose link still applies.
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        let child = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 1 }, 3));
        doc.shape_mut(instance)
            .expect("exists")
            .children
            .push(child);
        doc.shape_mut(child).expect("exists").parent = Some(instance);
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        let r = doc
            .resolve(child, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 0 }));
    }

    #[test]
    fn a_cyclic_master_chain_finds_no_style_link() {
        // The link search walks the master chain: a cycle is cut and no
        // style rung applies (never a hang, P6-15).
        let (mut doc, master_shape, instance) = instance_of_master(None);
        doc.shape_mut(master_shape).expect("exists").master = Some(MasterId { index: 0 });
        doc.stylesheet
            .styles
            .push(line_style(0, 1, None, Some(0.05)));
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves without hanging");
        assert!(
            !r.via.iter().any(|s| matches!(s, ValueSource::Style(_))),
            "no style rung applies: {:?}",
            r.via
        );
        assert!(matches!(
            r.source,
            ValueSource::SectionDefault | ValueSource::HardDefault
        ));
    }

    #[test]
    fn a_master_style_chain_with_a_cycle_terminates() {
        // Acceptance: a style chain entered through the master's link and
        // cycling (1 -> 2 -> 1) stops at the repeat and falls through.
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        doc.stylesheet.styles.push(line_style(0, 1, Some(2), None));
        doc.stylesheet.styles.push(line_style(1, 2, Some(1), None));
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves without hanging");
        assert_eq!(
            r.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 0 }),
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::Style(StyleId { index: 1 }),
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
        assert!(matches!(
            r.source,
            ValueSource::SectionDefault | ValueSource::HardDefault
        ));
    }

    #[test]
    fn the_style_chain_continues_from_the_masters_link() {
        // The chain runs from the effective link through each style's own
        // link of the same kind, as it does from a shape's own link.
        let (mut doc, _master, instance) = instance_of_master(Some(1));
        doc.stylesheet.styles.push(line_style(0, 1, Some(2), None));
        doc.stylesheet
            .styles
            .push(line_style(1, 2, None, Some(0.05)));
        let r = doc
            .resolve(instance, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(r.value, Some(&CellValue::Number(0.05)));
        assert_eq!(r.source, ValueSource::Style(StyleId { index: 1 }));
    }
}

#[cfg(test)]
mod phase3_tests {
    use crate::generated::CellDef;
    use crate::id::PageId;
    use crate::page::Page;
    use crate::raw::{QName, XmlAttribute};
    use crate::section::{Row, Section};
    use crate::values::SheetRef;

    use super::*;

    /// A cell with a cached number.
    fn number_cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A cell whose formula is `Inh`: it does not define (ADR 0013).
    fn inherited_cell(name: &str) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = Some(INHERIT_FORMULA.to_owned());
        cell.value = Some(CellValue::Number(99.0));
        cell
    }

    /// A `Geometry` section with one row of `cells`.
    fn geometry(cells: Vec<Cell>) -> Section {
        let mut section = Section::new(crate::name::SectionName::from_static("Geometry"));
        let mut row = Row::new();
        row.row_type = Some("LineTo".to_owned());
        row.ix = Some(1);
        row.cells = cells;
        section.rows.push(row);
        section
    }

    /// A document with one master (`Box`) whose only shape carries
    /// `master_cells`, and one instance of it.
    fn instance_of_master(master_cells: Vec<Cell>) -> (Document, ShapeId) {
        let mut doc = Document::new();
        let master_shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(master_shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(master_cells));
        doc.masters
            .push(crate::master::Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.master = Some(MasterId { index: 0 });
        }
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);
        (doc, instance)
    }

    #[test]
    fn row_cells_resolve_from_the_local_row_first() {
        let mut doc = Document::new();
        let shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(vec![number_cell("X", 1.5)]));
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(shape),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(1.5)));
        assert_eq!(resolved.source, ValueSource::Local);
        assert_eq!(resolved.via, vec![ValueSource::Local]);
    }

    #[test]
    fn an_instance_with_no_rows_resolves_the_masters_rows() {
        // Acceptance: an instance with no geometry rows resolves the
        // master's rows.
        let (mut doc, instance) = instance_of_master(vec![number_cell("X", 4.0)]);
        // The instance gets an empty Geometry section: no local rows.
        doc.shape_mut(instance)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(Vec::new()));
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(4.0)));
        assert_eq!(resolved.source, ValueSource::Master(MasterId { index: 0 }));
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 0 })
            ]
        );
    }

    #[test]
    fn an_inh_cell_does_not_define() {
        // Acceptance: `Inh` cells do not define; the cached snapshot (99)
        // is never used.
        let (mut doc, instance) = instance_of_master(vec![number_cell("X", 4.0)]);
        doc.shape_mut(instance)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(vec![inherited_cell("X")]));
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(4.0)));
        assert_eq!(resolved.source, ValueSource::Master(MasterId { index: 0 }));
    }

    #[test]
    fn a_style_rung_never_applies_to_geometry_cells() {
        // Acceptance: a style rung never applies to `Geometry` cells (ADR
        // 0019 decision 5), even when the shape's links reach a style that
        // defines the cell.
        let (mut doc, instance) = instance_of_master(Vec::new());
        {
            let shape = doc.shape_mut(instance).expect("exists");
            shape.line_style = Some(7);
            shape.fill_style = Some(7);
            shape.text_style = Some(7);
            shape
                .sheet
                .sections
                .push(geometry(vec![inherited_cell("X")]));
        }
        let mut style = crate::style::Style::new(StyleId { index: 0 });
        style.visio_id = 7;
        style
            .sheet
            .sections
            .push(geometry(vec![number_cell("X", 5.0)]));
        doc.stylesheet.styles.push(style);
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(resolved.value, Some(&CellValue::Number(0.0)));
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
    }

    #[test]
    fn rows_match_by_ix_when_both_sides_have_one() {
        // The master's row carries `IX = 5` at position 0; the instance's
        // row is at position 0 with `IX = 1`. The master rung matches by
        // `IX` first (no match here) and degrades to position.
        let (mut doc, instance) = instance_of_master(vec![number_cell("X", 4.0)]);
        {
            let shape = doc.shape_mut(instance).expect("exists");
            let mut section = geometry(Vec::new());
            section.rows[0].ix = Some(1);
            section.rows[0].cells.push(inherited_cell("X"));
            shape.sheet.sections.push(section);
        }
        // No master row with IX 1: position 0 matches.
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(4.0)));

        // Now give the master row IX 1 and add a second master row.
        let master_shape = doc.masters[0].shapes[0];
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            let section = &mut shape.sheet.sections[0];
            section.rows[0].ix = Some(5);
            section.rows[0].cells[0].value = Some(CellValue::Number(4.0));
            let mut second = Row::new();
            second.ix = Some(1);
            second.cells.push(number_cell("X", 6.0));
            section.rows.push(second);
        }
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(6.0)));
    }

    #[test]
    fn several_geometry_sections_match_by_ix() {
        // Two `Geometry` sections on both sides, disambiguated by `IX`.
        let mut doc = Document::new();
        let master_shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            shape
                .sheet
                .sections
                .push(geometry(vec![number_cell("X", 1.0)]));
            let mut second = geometry(vec![number_cell("X", 2.0)]);
            second.preserved.attributes.push(XmlAttribute {
                name: QName {
                    prefix: None,
                    uri: None,
                    local: "IX".to_owned(),
                },
                value: "1".to_owned(),
            });
            shape.sheet.sections.push(second);
        }
        doc.masters
            .push(crate::master::Master::new(MasterId { index: 0 }, 1));
        doc.masters[0].shapes.push(master_shape);
        let instance = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        doc.shape_mut(instance).expect("exists").master = Some(MasterId { index: 0 });
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(instance);
        doc.pages.push(page);

        let first = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: None,
                    section_pos: Some(0),
                    row: Some(0),
                    row_ix: None,
                },
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(first.value, Some(&CellValue::Number(1.0)));
        let second = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: Some(1),
                    section_pos: Some(1),
                    row: Some(0),
                    row_ix: None,
                },
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(second.value, Some(&CellValue::Number(2.0)));
    }

    #[test]
    fn row_cells_fall_through_to_the_table_default() {
        // A table that records a `default_value` for the row cell.
        const TWO: &[CellDef] = &[
            CellDef {
                name: "X",
                section: "Geometry",
                cell_index: None,
                kind: "Number",
                default_unit: "Inches",
                default_value: Some("2 in"),
            },
            CellDef {
                name: "Other",
                section: "Geometry",
                cell_index: None,
                kind: "Number",
                default_unit: "Number",
                default_value: None,
            },
        ];
        let doc = Document::new();
        let resolved = doc.resolve_repeated_with_table(
            SheetRef::Shape(ShapeId { index: 9 }),
            &RepeatedCell {
                section: "Geometry",
                section_ix: None,
                section_pos: Some(0),
                row: Some(0),
                row_ix: None,
            },
            &CellName::from_static("X"),
            TWO,
        );
        // A stale shape id is a caller error even with a table.
        assert_eq!(
            resolved,
            Err(crate::Error::UnknownShape(ShapeId { index: 9 }))
        );
        let (mut doc, shape) = instance_of_master(Vec::new());
        doc.shape_mut(shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(Vec::new()));
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(shape),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: None,
                    section_pos: Some(0),
                    row: Some(0),
                    row_ix: None,
                },
                &CellName::from_static("X"),
                TWO,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::SectionDefault);
        assert_eq!(resolved.value, Some(&CellValue::Number(2.0)));
        // A cell the table knows without a default gets the hard default.
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(shape),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: None,
                    section_pos: Some(0),
                    row: Some(0),
                    row_ix: None,
                },
                &CellName::from_static("Other"),
                TWO,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(resolved.value, Some(&CellValue::Number(0.0)));
    }

    #[test]
    fn a_cyclic_master_chain_terminates() {
        let (mut doc, instance) = instance_of_master(Vec::new());
        // The master's shape is itself an instance of the same master.
        let master_shape = doc.masters[0].shapes[0];
        doc.shape_mut(master_shape).expect("exists").master = Some(MasterId { index: 0 });
        doc.shape_mut(instance)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(Vec::new()));
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(instance),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 0 }),
                ValueSource::Master(MasterId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
    }

    #[test]
    fn section_level_cells_resolve_without_a_row() {
        // `NoFill` is a section-level cell (row `None`).
        let mut doc = Document::new();
        let shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(shape).expect("exists");
            let mut section = geometry(Vec::new());
            section.cells.push({
                let mut cell = Cell::new(CellName::from_static("NoFill"));
                cell.value = Some(CellValue::Bool(true));
                cell
            });
            shape.sheet.sections.push(section);
        }
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(shape),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: None,
                    section_pos: Some(0),
                    row: None,
                    row_ix: None,
                },
                &CellName::from_static("NoFill"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Local);
        assert_eq!(resolved.value, Some(&CellValue::Bool(true)));
    }

    // --- page sheets ---------------------------------------------------

    /// A document with one page whose sheet has `cells`, and one style
    /// (`ID = 7`) with `style_cells`.
    fn page_with_style(cells: Vec<Cell>, style_cells: Vec<Cell>) -> (Document, PageId) {
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.sheet.cells = cells;
        page.preserved.attributes.push(XmlAttribute {
            name: QName {
                prefix: None,
                uri: None,
                local: "LineStyle".to_owned(),
            },
            value: "7".to_owned(),
        });
        doc.pages.push(page);
        let mut style = crate::style::Style::new(StyleId { index: 0 });
        style.visio_id = 7;
        style.sheet.cells = style_cells;
        doc.stylesheet.styles.push(style);
        (doc, PageId { index: 0 })
    }

    #[test]
    fn resolve_page_cell_reads_the_page_cell_first() {
        let (doc, page) = page_with_style(vec![number_cell("PageWidth", 11.0)], Vec::new());
        let resolved = doc
            .resolve_page_cell(page, &CellName::from_static("PageWidth"))
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(11.0)));
        assert_eq!(resolved.source, ValueSource::Local);
    }

    #[test]
    fn resolve_page_cell_follows_the_page_style_links() {
        // A `Line Format` cell the page sheet does not define comes from
        // the style named by the page sheet's `LineStyle` link.
        let (doc, page) = page_with_style(
            vec![inherited_cell("LineWeight")],
            vec![number_cell("LineWeight", 0.02)],
        );
        let resolved = doc
            .resolve_page_cell(page, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(0.02)));
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 0 }));
    }

    #[test]
    fn resolve_page_cell_stops_a_cyclic_or_dangling_style_chain() {
        // The style links to itself: the chain stops at the repeat.
        let (mut doc, page) = page_with_style(Vec::new(), vec![inherited_cell("LineWeight")]);
        doc.stylesheet.styles[0].line_style = Some(7);
        let resolved = doc
            .resolve_page_cell(page, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::Style(StyleId { index: 0 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
        // A link that names no style ends the chain quietly.
        let (mut doc, page) = page_with_style(Vec::new(), Vec::new());
        doc.preserved = doc.preserved.clone();
        doc.pages[0].preserved.attributes[0].value = "8".to_owned();
        let resolved = doc
            .resolve_page_cell(page, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
    }

    #[test]
    fn page_size_reads_the_page_sheet() {
        let (doc, page) = page_with_style(
            vec![
                number_cell("PageWidth", 11.0),
                number_cell("PageHeight", 8.5),
            ],
            Vec::new(),
        );
        assert_eq!(doc.page_size(page), (11.0, 8.5));
    }

    #[test]
    fn page_size_comes_from_the_style_chain() {
        // The page sheet defines no size; the style it links does.
        let (doc, page) = page_with_style(
            Vec::new(),
            vec![
                number_cell("PageWidth", 10.0),
                number_cell("PageHeight", 6.0),
            ],
        );
        assert_eq!(doc.page_size(page), (10.0, 6.0));
    }

    #[test]
    fn page_size_takes_both_axes_from_one_sheet() {
        // The page defines only its width: the height is the default's,
        // never the style sheet's (width and height resolve together).
        let (doc, page) = page_with_style(
            vec![number_cell("PageWidth", 3.0)],
            vec![
                number_cell("PageWidth", 10.0),
                number_cell("PageHeight", 6.0),
            ],
        );
        assert_eq!(doc.page_size(page), (3.0, DEFAULT_PAGE_SIZE.1));
    }

    #[test]
    fn page_size_skips_a_non_positive_size() {
        // Neither axis is positive on the page sheet, so the pair falls
        // through to the style chain.
        let (doc, page) = page_with_style(
            vec![
                number_cell("PageWidth", 0.0),
                number_cell("PageHeight", -1.0),
            ],
            vec![
                number_cell("PageWidth", 10.0),
                number_cell("PageHeight", 6.0),
            ],
        );
        assert_eq!(doc.page_size(page), (10.0, 6.0));
    }

    #[test]
    fn page_size_falls_back_to_letter() {
        let (doc, page) = page_with_style(Vec::new(), Vec::new());
        assert_eq!(doc.page_size(page), DEFAULT_PAGE_SIZE);
        // A stale page id: the default pair, never an error.
        assert_eq!(
            doc.page_size(PageId { index: 3 }),
            DEFAULT_PAGE_SIZE,
            "an unknown page gets the default pair"
        );
    }

    #[test]
    fn page_size_stops_a_cyclic_style_chain() {
        // The style links to itself: the chain stops at the repeat and
        // the default pair answers.
        let (mut doc, page) = page_with_style(
            Vec::new(),
            vec![
                number_cell("PageWidth", 10.0),
                number_cell("PageHeight", 6.0),
            ],
        );
        doc.stylesheet.styles[0].line_style = Some(7);
        assert_eq!(doc.page_size(page), (10.0, 6.0));
        let (mut doc, page) = page_with_style(Vec::new(), vec![inherited_cell("LineWeight")]);
        doc.stylesheet.styles[0].line_style = Some(7);
        assert_eq!(doc.page_size(page), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn resolve_page_cell_reads_the_section_default_with_a_table() {
        const TWO: &[CellDef] = &[CellDef {
            name: "PageWidth",
            section: "Page Sheet",
            cell_index: None,
            kind: "Number",
            default_unit: "Inches",
            default_value: Some("8.5 in"),
        }];
        let mut doc = Document::new();
        doc.pages.push(Page::new(PageId { index: 0 }, 1));
        let resolved = doc
            .resolve_page_cell_with_table(
                PageId { index: 0 },
                &CellName::from_static("PageWidth"),
                TWO,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::SectionDefault);
        assert_eq!(resolved.value, Some(&CellValue::Number(8.5)));
        assert_eq!(
            doc.resolve_page_cell_with_table(
                PageId { index: 1 },
                &CellName::from_static("PageWidth"),
                TWO
            ),
            Err(crate::Error::UnknownPage(PageId { index: 1 }))
        );
    }

    #[test]
    fn resolve_row_cell_reports_a_stale_shape() {
        let doc = Document::new();
        assert_eq!(
            doc.resolve_row_cell(
                SheetRef::Shape(ShapeId { index: 2 }),
                "Geometry",
                0,
                &CellName::from_static("X")
            ),
            Err(crate::Error::UnknownShape(ShapeId { index: 2 }))
        );
        assert_eq!(
            doc.resolve_repeated_with_table(
                SheetRef::Page(PageId { index: 2 }),
                &RepeatedCell {
                    section: "Geometry",
                    section_ix: None,
                    section_pos: Some(0),
                    row: Some(0),
                    row_ix: None,
                },
                &CellName::from_static("X"),
                CELLS,
            ),
            Err(crate::Error::UnknownPage(PageId { index: 2 }))
        );
    }

    #[test]
    fn match_repeated_section_falls_back_to_the_first_named() {
        let mut sheet = Sheet::new();
        let mut first = Section::new(crate::name::SectionName::from_static("Geometry"));
        first.rows.push(Row::new());
        let mut other = Section::new(crate::name::SectionName::from_static("Connection"));
        other.rows.push(Row::new());
        let mut second = Section::new(crate::name::SectionName::from_static("Geometry"));
        second.rows.push(Row::new());
        sheet.sections.push(first);
        sheet.sections.push(other);
        sheet.sections.push(second);
        // No IX on either side: the position among same-named sections wins.
        assert!(std::ptr::eq(
            match_repeated_section(&sheet, "Geometry", None, Some(1)).expect("found"),
            &sheet.sections[2]
        ));
        assert!(match_repeated_section(&sheet, "Geometry", None, Some(9)).is_none());
        assert!(match_repeated_section(&sheet, "Missing", None, Some(0)).is_none());
    }

    #[test]
    fn row_cells_fall_to_defaults_without_a_master() {
        // A shape that is not an instance skips the master rung entirely.
        let mut doc = Document::new();
        let shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(shape)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(Vec::new()));
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(shape),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
    }

    #[test]
    fn style_chains_continue_when_the_style_does_not_define() {
        // Fill and text chains: the first style links onward without
        // defining the cell, the second defines it.
        let mut doc = Document::new();
        let shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(shape).expect("exists");
            shape.fill_style = Some(1);
            shape.text_style = Some(2);
        }
        let mut first = crate::style::Style::new(StyleId { index: 0 });
        first.visio_id = 1;
        first.fill_style = Some(3);
        doc.stylesheet.styles.push(first);
        let mut second = crate::style::Style::new(StyleId { index: 1 });
        second.visio_id = 2;
        second.text_style = Some(4);
        doc.stylesheet.styles.push(second);
        let mut third = crate::style::Style::new(StyleId { index: 2 });
        third.visio_id = 3;
        third.sheet.cells.push(number_cell("FillForegnd", 0.5));
        doc.stylesheet.styles.push(third);
        let mut fourth = crate::style::Style::new(StyleId { index: 3 });
        fourth.visio_id = 4;
        let mut font = Cell::new(CellName::from_static("Font"));
        font.value = Some(CellValue::Str("Calibri".to_owned()));
        fourth.sheet.cells.push(font);
        doc.stylesheet.styles.push(fourth);

        let resolved = doc
            .resolve(shape, &CellName::from_static("FillForegnd"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 2 }));
        let resolved = doc
            .resolve(shape, &CellName::from_static("Font"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 3 }));
    }

    #[test]
    fn page_style_chains_continue_through_every_kind() {
        // The page sheet's links cover Line, Fill and Text, and the chain
        // continues through a style that does not define the cell.
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        for (name, value) in [("LineStyle", "1"), ("FillStyle", "2"), ("TextStyle", "3")] {
            page.preserved.attributes.push(XmlAttribute {
                name: QName {
                    prefix: None,
                    uri: None,
                    local: name.to_owned(),
                },
                value: value.to_owned(),
            });
        }
        doc.pages.push(page);
        let style = |index: u32, id: StyleId, link: (&str, u32)| {
            let mut style = crate::style::Style::new(id);
            style.visio_id = index;
            match link.0 {
                "line" => style.line_style = Some(link.1),
                "fill" => style.fill_style = Some(link.1),
                _ => style.text_style = Some(link.1),
            }
            style
        };
        let mut first = style(1, StyleId { index: 0 }, ("line", 4));
        first.sheet.cells.push(number_cell("LinePattern", 1.0));
        let mut inherited = Cell::new(CellName::from_static("LineWeight"));
        inherited.formula = Some(INHERIT_FORMULA.to_owned());
        first.sheet.cells.push(inherited);
        let mut second = style(2, StyleId { index: 1 }, ("fill", 5));
        second.sheet.cells.push(number_cell("FillPattern", 1.0));
        let mut third = style(3, StyleId { index: 2 }, ("text", 6));
        third.sheet.cells.push(number_cell("Size", 1.0));
        let mut fourth = style(4, StyleId { index: 3 }, ("line", 4));
        fourth.sheet.cells.push(number_cell("LineWeight", 0.02));
        let mut fifth = style(5, StyleId { index: 4 }, ("fill", 5));
        fifth.sheet.cells.push(number_cell("FillForegnd", 7.0));
        let mut sixth = style(6, StyleId { index: 5 }, ("text", 6));
        let mut font = Cell::new(CellName::from_static("Font"));
        font.value = Some(CellValue::Str("Calibri".to_owned()));
        sixth.sheet.cells.push(font);
        for entry in [first, second, third, fourth, fifth, sixth] {
            doc.stylesheet.styles.push(entry);
        }

        let resolved = doc
            .resolve_page_cell(PageId { index: 0 }, &CellName::from_static("LineWeight"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 3 }));
        let resolved = doc
            .resolve_page_cell(PageId { index: 0 }, &CellName::from_static("FillForegnd"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 4 }));
        let resolved = doc
            .resolve_page_cell(PageId { index: 0 }, &CellName::from_static("Font"))
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::Style(StyleId { index: 5 }));
    }

    #[test]
    fn match_repeated_section_without_a_position_takes_the_first() {
        let mut sheet = Sheet::new();
        sheet
            .sections
            .push(Section::new(crate::name::SectionName::from_static(
                "Geometry",
            )));
        sheet
            .sections
            .push(Section::new(crate::name::SectionName::from_static(
                "Geometry",
            )));
        assert!(std::ptr::eq(
            match_repeated_section(&sheet, "Geometry", None, None).expect("found"),
            &sheet.sections[0]
        ));
    }

    #[test]
    fn page_sheet_rows_resolve_without_a_master_rung() {
        // A `SheetRef::Page` row lookup: the page sheet's own row, then the
        // defaults (a page has no master rung).
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.sheet.sections.push(geometry(Vec::new()));
        doc.pages.push(page);
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Page(PageId { index: 0 }),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
    }

    #[test]
    fn a_dangling_master_reference_skips_the_master_rung() {
        let mut doc = Document::new();
        let shape = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        {
            let shape = doc.shape_mut(shape).expect("exists");
            shape.master = Some(MasterId { index: 5 });
            shape.sheet.sections.push(geometry(Vec::new()));
        }
        let resolved = doc
            .resolve_row_cell(
                SheetRef::Shape(shape),
                "Geometry",
                0,
                &CellName::from_static("X"),
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
        assert_eq!(
            resolved.via,
            vec![
                ValueSource::Local,
                ValueSource::Master(MasterId { index: 5 }),
                ValueSource::SectionDefault,
                ValueSource::HardDefault,
            ]
        );
    }

    #[test]
    fn a_row_matches_by_position_only_when_the_other_side_has_no_ix() {
        // The caller names `IX` 5. A master row without `IX` at the same
        // position matches by position ("by `IX` when both sides have one,
        // else by position", ADR 0019 decision 5); a master row with
        // another `IX` at that position does not match at all.
        let (mut doc, instance) = instance_of_master(Vec::new());
        let master_shape = doc.masters[0].shapes[0];
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            let sec = &mut shape.sheet.sections[0];
            sec.rows.clear();
            let mut no_ix = Row::new();
            no_ix.row_type = Some("LineTo".to_owned());
            no_ix.cells.push(number_cell("X", 1.0));
            sec.rows.push(no_ix);
        }
        doc.shape_mut(instance)
            .expect("exists")
            .sheet
            .sections
            .push(geometry(Vec::new()));
        let target = RepeatedCell {
            section: "Geometry",
            section_ix: None,
            section_pos: Some(0),
            row: Some(0),
            row_ix: Some(5),
        };
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &target,
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(1.0)));

        // Now the master's row at position 0 has `IX` 2: no match.
        let master_shape = doc.masters[0].shapes[0];
        doc.shape_mut(master_shape).expect("exists").sheet.sections[0].rows[0].ix = Some(2);
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &target,
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
    }

    #[test]
    fn a_row_named_by_ix_alone_matches_only_that_ix() {
        // A text run marker names its row by `IX` alone (`row: None,
        // `row_ix: Some`): the master row with that `IX` matches wherever
        // it sits, and no other row does.
        let (mut doc, instance) = instance_of_master(Vec::new());
        let master_shape = doc.masters[0].shapes[0];
        {
            let shape = doc.shape_mut(master_shape).expect("exists");
            let sec = &mut shape.sheet.sections[0];
            sec.rows.clear();
            let mut first = Row::new();
            first.row_type = Some("LineTo".to_owned());
            first.ix = Some(0);
            first.cells.push(number_cell("X", 1.0));
            let mut second = Row::new();
            second.row_type = Some("LineTo".to_owned());
            second.ix = Some(5);
            second.cells.push(number_cell("X", 5.0));
            sec.rows.push(first);
            sec.rows.push(second);
        }
        let target = RepeatedCell {
            section: "Geometry",
            section_ix: None,
            section_pos: Some(0),
            row: None,
            row_ix: Some(5),
        };
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &target,
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(resolved.value, Some(&CellValue::Number(5.0)));

        // An `IX` no row has matches no row at all: the hard default, not
        // the row at that position.
        let target = RepeatedCell {
            row_ix: Some(1),
            ..target
        };
        let resolved = doc
            .resolve_repeated_with_table(
                SheetRef::Shape(instance),
                &target,
                &CellName::from_static("X"),
                CELLS,
            )
            .expect("resolves");
        assert_eq!(resolved.source, ValueSource::HardDefault);
    }

    #[test]
    fn master_chain_shapes_follow_the_master_rungs() {
        // A master with two top-level shapes: the instance at page
        // position 1 resolves through the second one (P2-06b decision 2),
        // the same shape the row cells resolve through.
        let mut doc = Document::new();
        let filler = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        let instance = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 2));
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(filler);
        page.shapes.push(instance);
        doc.pages.push(page);
        let many = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 3));
        let one = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 4));
        let mut master = crate::master::Master::new(MasterId { index: 0 }, 1);
        master.shapes = vec![many, one];
        doc.masters.push(master);
        doc.shape_mut(instance).expect("shape").master = Some(MasterId { index: 0 });
        assert_eq!(doc.master_chain_shapes(instance, 128), vec![one]);

        // A nested master that chains back to itself: one rung, no loop.
        doc.shape_mut(one).expect("shape").master = Some(MasterId { index: 0 });
        assert_eq!(doc.master_chain_shapes(instance, 128), vec![one]);

        // The depth cap bounds the walk.
        assert!(doc.master_chain_shapes(instance, 0).is_empty());

        // A shape of no master has no rungs.
        assert!(doc.master_chain_shapes(many, 128).is_empty());
    }

    /// A `Property` row with the given name (or none) and cells.
    fn property_row(name: Option<&str>, cells: Vec<Cell>) -> crate::section::Row {
        let mut row = crate::section::Row::new();
        row.name = name.map(str::to_owned);
        row.cells = cells;
        row
    }

    /// A `Property` section holding `rows`.
    fn property_section(rows: Vec<crate::section::Row>) -> Section {
        let mut section = Section::new(crate::name::SectionName::from_static("Property"));
        section.rows = rows;
        section
    }

    /// A shape with `rows` as its `Property` section.
    fn shape_with_properties(doc: &mut Document, rows: Vec<crate::section::Row>) -> ShapeId {
        let id = doc.add_shape(crate::shape::Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(id)
            .expect("shape")
            .sheet
            .sections
            .push(property_section(rows));
        id
    }

    #[test]
    fn property_rows_carry_keys_and_skip_deleted_rows() {
        let mut doc = Document::new();
        let named = property_row(Some("Cost"), vec![number_cell("Value", 2.0)]);
        let unnamed = property_row(None, vec![number_cell("Value", 3.0)]);
        let mut gone = property_row(Some("Gone"), Vec::new());
        gone.preserved.attributes.push(crate::raw::XmlAttribute {
            name: crate::raw::QName::local("Del"),
            value: "1".to_owned(),
        });
        let shape = shape_with_properties(&mut doc, vec![named, unnamed, gone]);
        let rows = doc.property_rows(shape);
        let keys: Vec<&str> = rows.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, vec!["Cost", "Row_2"]);
        assert!(rows.iter().all(|entry| !entry.inherited));
    }

    #[test]
    fn property_rows_add_the_master_chain_rows_the_shape_does_not_claim() {
        let mut doc = Document::new();
        let own = property_row(Some("Cost"), vec![number_cell("Value", 2.0)]);
        let mut gone = property_row(Some("Gone"), Vec::new());
        gone.preserved.attributes.push(crate::raw::XmlAttribute {
            name: crate::raw::QName::local("Del"),
            value: "1".to_owned(),
        });
        let instance = shape_with_properties(&mut doc, vec![own, gone]);
        let rung = shape_with_properties(
            &mut doc,
            vec![
                property_row(Some("Cost"), vec![number_cell("Value", 9.0)]),
                property_row(Some("Label"), Vec::new()),
                property_row(Some("Gone"), Vec::new()),
            ],
        );
        let mut master = crate::master::Master::new(MasterId { index: 0 }, 1);
        master.shapes = vec![rung];
        doc.masters.push(master);
        doc.shape_mut(instance).expect("shape").master = Some(MasterId { index: 0 });

        let rows = doc.property_rows(instance);
        let keys: Vec<&str> = rows.iter().map(|entry| entry.key.as_str()).collect();
        // The own `Cost` wins the clash and the deleted own `Gone`
        // hides the master's row (the ADR 0019 row union rule).
        assert_eq!(keys, vec!["Cost", "Label"]);
        assert!(!rows[0].inherited);
        assert!(rows[1].inherited);
        assert_eq!(
            rows[0].row.cells.first().map(|cell| cell.value.clone()),
            Some(Some(CellValue::Number(2.0)))
        );

        // A stale shape id lists nothing.
        assert!(doc.property_rows(ShapeId { index: 999 }).is_empty());
    }

    #[test]
    fn find_property_row_matches_a_name_before_a_position_key() {
        let mut doc = Document::new();
        let named = property_row(Some("Cost"), Vec::new());
        let first = property_row(None, Vec::new());
        let second = property_row(None, Vec::new());
        let shape = shape_with_properties(&mut doc, vec![named, first, second]);
        let sheet = &doc.shape(shape).expect("shape").sheet;
        // A named row is keyed by its name, an unnamed row by its
        // 1-based position (`Row_<n>`).
        assert_eq!(find_property_row(sheet, "Cost"), Some(0));
        assert_eq!(find_property_row(sheet, "Row_2"), Some(1));
        assert_eq!(find_property_row(sheet, "Row_3"), Some(2));
        assert_eq!(find_property_row(sheet, "Row_1"), None);
        assert_eq!(find_property_row(sheet, "Nope"), None);

        // A row named `Row_2` claims that key over the unnamed row at
        // position 2: a name beats the synthesized key.
        let mut doc = Document::new();
        let first = property_row(None, Vec::new());
        let named = property_row(Some("Row_1"), Vec::new());
        let shape = shape_with_properties(&mut doc, vec![first, named]);
        let sheet = &doc.shape(shape).expect("shape").sheet;
        assert_eq!(find_property_row(sheet, "Row_1"), Some(1));

        // A sheet without the section resolves nothing.
        let empty = crate::sheet::Sheet::new();
        assert_eq!(find_property_row(&empty, "Row_1"), None);
    }
}
