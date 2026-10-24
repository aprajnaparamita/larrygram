//! Where the controls of a Panel Modern group sit (ADR 0061,
//! `docs/ui/layout-format.md`, ADR 0065 placement tests).
//!
//! [`place`] turns a group's `column`, `row` and `size` slots into
//! rectangles, in logical pixels, relative to the group's content
//! top-left corner. The renderer measures the real control widths and
//! passes them in; the tests pass plain numbers and assert the
//! geometry. The same code drives both, so the placement tests cover
//! what is drawn.
//!
//! Two rules cover the adapted v1 layouts (ADR 0061): a group whose
//! controls have no slot data flows in file order, and repeats of one
//! `(id, label)` inside a container render once (the ADR 0031 display
//! dedup rule, first occurrence wins).

use std::collections::BTreeMap;

use egui::Rect;
use larrygram_ui_spec::v2::{ControlSize, Group, GroupLayout, Item, ItemKind};

use super::metrics::Metrics;

/// The controls of `group` that draw: hidden controls are not drawn
/// (they have no slot), and display repeats are dropped (ADR 0031).
///
/// A repeat is an item with the same `id` **and** `label` as an
/// earlier item of the same group; the first occurrence wins. The v2
/// curated data has unique ids, so this only bites on adapted v1 data.
pub fn visible_controls(group: &Group) -> Vec<&Item> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut out = Vec::new();
    for item in &group.controls {
        if item.hidden {
            continue;
        }
        let key = (item.id.as_str(), item.label.as_deref().unwrap_or_default());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(item);
    }
    out
}

/// True for a control that spans its column from the top of the group
/// down to the caption (`size: large`).
pub fn is_large(item: &Item) -> bool {
    item.size == Some(ControlSize::Large) && !matches!(item.kind, ItemKind::Combo | ItemKind::Check)
}

/// Where one control draws: its rectangle and whether it is large.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// The rectangle, relative to the group's content top-left.
    pub rect: Rect,
    /// True for a control that spans its whole column.
    pub large: bool,
}

/// The rectangle of every visible control of `group`, in the order of
/// [`visible_controls`], relative to the group's content top-left.
///
/// `widths[i]` is the width measured for the `i`th visible control; a
/// missing or unusable width falls back to the large-control minimum.
/// Out-of-range `row` and `column` slots are clamped, so malformed
/// data places controls but never panics (AGENTS.md section 6).
pub fn place(group: &Group, widths: &[f32], m: &Metrics) -> Vec<Placed> {
    let items = visible_controls(group);
    let widths: Vec<f32> = items
        .iter()
        .enumerate()
        .map(|(index, item)| control_width(item, widths.get(index).copied(), m))
        .collect();
    let content_height = m.content_height();
    let slotted = !items.is_empty()
        && items
            .iter()
            .all(|item| item.column.is_some() && item.row.is_some());
    let rows_per_column = group.rows.unwrap_or(m.stack_rows).max(1);

    // Where each control starts, before the column widths are known.
    let slots: Vec<Slot> = if slotted {
        items
            .iter()
            .enumerate()
            .map(|(index, item)| slotted_slot(item, index, rows_per_column))
            .collect()
    } else {
        flow_slots(group, &items, rows_per_column)
    };

    match group.layout {
        GroupLayout::Stack => stack_rects(&slots, &widths, m, rows_per_column, content_height),
        GroupLayout::Rows => rows_rects(&slots, &widths, m, content_height),
    }
}

/// One control's place in the grid, before column widths are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    /// The column index (0-based, left to right).
    column: u32,
    /// The row index inside the column or half-row.
    row: u32,
    /// True for a control that spans its whole column.
    large: bool,
    /// Draw a separator before this control (cluster gap in `rows`).
    begin_group: bool,
    /// Index of the control in [`visible_controls`] order.
    index: usize,
}

/// The slot of one control that carries `column` and `row` (clamped).
fn slotted_slot(item: &Item, index: usize, rows: u32) -> Slot {
    let column = item.column.unwrap_or(0);
    let row = item.row.unwrap_or(0).min(rows.saturating_sub(1));
    Slot {
        column,
        row,
        large: is_large(item),
        begin_group: item.begin_group,
        index,
    }
}

/// The slots of a group whose controls carry no slot data: the
/// controls flow in file order (the adapted v1 layouts, ADR 0061).
///
/// In a `stack` group each column fills top to bottom before the next
/// one starts; a large control takes a column of its own. The first
/// control of the group that has a menu is large (ADR 0061). In a
/// `rows` group the controls flow left to right in their `row`, the
/// lower half starting a new line.
fn flow_slots(group: &Group, items: &[&Item], rows: u32) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut large_used = false;
    match group.layout {
        GroupLayout::Stack => {
            let mut column = 0;
            let mut row = 0;
            for (index, item) in items.iter().enumerate() {
                let large = is_large(item) || (!large_used && has_menu(item));
                if large {
                    large_used = large_used || has_menu(item);
                    out.push(Slot {
                        column,
                        row: 0,
                        large: true,
                        begin_group: item.begin_group,
                        index,
                    });
                    column += 1;
                    row = 0;
                } else {
                    out.push(Slot {
                        column,
                        row,
                        large: false,
                        begin_group: item.begin_group,
                        index,
                    });
                    row += 1;
                    if row >= rows {
                        row = 0;
                        column += 1;
                    }
                }
            }
        }
        GroupLayout::Rows => {
            let mut per_row: BTreeMap<u32, u32> = BTreeMap::new();
            for (index, item) in items.iter().enumerate() {
                let row = item.row.unwrap_or(0).min(1);
                let column = per_row.entry(row).or_default();
                out.push(Slot {
                    column: *column,
                    row,
                    large: false,
                    begin_group: item.begin_group,
                    index,
                });
                *column += 1;
            }
        }
    }
    out
}

/// True for a control that opens a menu (split, dropdown or gallery).
fn has_menu(item: &Item) -> bool {
    matches!(
        item.kind,
        ItemKind::Split | ItemKind::SplitRecent | ItemKind::Dropdown | ItemKind::Gallery
    )
}

/// The width one control takes: the measured width, or the large
/// minimum when nothing was measured.
fn control_width(item: &Item, width: Option<f32>, m: &Metrics) -> f32 {
    let width = width.unwrap_or(m.large_min_width);
    let width = if width.is_finite() && width > 0.0 {
        width
    } else {
        m.large_min_width
    };
    if is_large(item) {
        width.max(m.large_min_width)
    } else {
        width
    }
}

/// Column x offsets: every column starts at the same x for all of its
/// controls (ADR 0065).
fn column_x(slots: &[Slot], widths: &[f32], m: &Metrics) -> BTreeMap<u32, f32> {
    let mut column_width: BTreeMap<u32, f32> = BTreeMap::new();
    for slot in slots {
        let width = widths.get(slot.index).copied().unwrap_or(m.large_min_width);
        let entry = column_width.entry(slot.column).or_default();
        *entry = entry.max(width);
    }
    let mut x: BTreeMap<u32, f32> = BTreeMap::new();
    let mut cursor = m.group_padding_x;
    for (column, width) in column_width {
        x.insert(column, cursor);
        cursor += width + m.group_padding_x;
    }
    x
}

/// The rectangles of a `stack` group: columns share their x, rows run
/// downwards, a large control spans the whole content height.
fn stack_rects(
    slots: &[Slot],
    widths: &[f32],
    m: &Metrics,
    rows: u32,
    content_height: f32,
) -> Vec<Placed> {
    let x = column_x(slots, widths, m);
    let row_height = m.stack_row_height;
    let total = (row_height * rows as f32).min(content_height);
    let start_y = ((content_height - total) * 0.5).max(0.0);
    let mut out = vec![
        Placed {
            rect: Rect::NOTHING,
            large: false
        };
        slots.len()
    ];
    for slot in slots {
        let width = widths.get(slot.index).copied().unwrap_or(m.large_min_width);
        let left = x.get(&slot.column).copied().unwrap_or(m.group_padding_x);
        let rect = if slot.large {
            Rect::from_min_size(egui::pos2(left, 0.0), egui::vec2(width, content_height))
        } else {
            Rect::from_min_size(
                egui::pos2(left, start_y + slot.row as f32 * row_height),
                egui::vec2(width, row_height),
            )
        };
        if let Some(entry) = out.get_mut(slot.index) {
            *entry = Placed {
                rect,
                large: slot.large,
            };
        }
    }
    out
}

/// The rectangles of a `rows` group: two half-rows, the controls of
/// each flowing left to right in `column` order, with a `rows.gap` at
/// every cluster jump (the columns jump by more than one, or the item
/// asks for a separator).
fn rows_rects(slots: &[Slot], widths: &[f32], m: &Metrics, content_height: f32) -> Vec<Placed> {
    let half = content_height * 0.5;
    let row_height = m.rows_row_height.min(half);
    let base_y = ((half - row_height) * 0.5).max(0.0);
    let mut out = vec![
        Placed {
            rect: Rect::NOTHING,
            large: false
        };
        slots.len()
    ];
    for half_row in 0..2u32 {
        let mut row: Vec<&Slot> = slots
            .iter()
            .filter(|slot| slot.row.min(1) == half_row)
            .collect();
        row.sort_by_key(|slot| (slot.column, slot.index));
        let mut cursor_x = m.group_padding_x;
        let mut previous_column: Option<u32> = None;
        for slot in row {
            let width = widths.get(slot.index).copied().unwrap_or(m.large_min_width);
            let jump = match previous_column {
                Some(previous) => slot.column > previous + 1,
                None => false,
            };
            if jump || slot.begin_group {
                cursor_x += m.rows_gap;
            }
            let rect = Rect::from_min_size(
                egui::pos2(cursor_x, base_y + half_row as f32 * half),
                egui::vec2(width, row_height),
            );
            if let Some(entry) = out.get_mut(slot.index) {
                *entry = Placed {
                    rect,
                    large: slot.large,
                };
            }
            cursor_x += width;
            previous_column = Some(slot.column);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use larrygram_ui_spec::v2::{Item, ItemKind};

    use super::*;

    /// One control with the defaults the tests do not care about.
    fn item(id: &str) -> Item {
        Item {
            id: id.to_owned(),
            kind: ItemKind::Button,
            label: Some(id.to_owned()),
            access: None,
            shortcut: None,
            command: None,
            new_command: false,
            fluent: None,
            visuicmd: None,
            icon: None,
            display: None,
            size: None,
            column: None,
            row: None,
            width: None,
            begin_group: false,
            hidden: false,
            dynamic_label: false,
            list: None,
            options: Vec::new(),
            items: Vec::new(),
        }
    }

    /// One control with a slot.
    fn slotted(id: &str, size: Option<ControlSize>, column: u32, row: u32) -> Item {
        let mut control = item(id);
        control.size = size;
        control.column = Some(column);
        control.row = Some(row);
        control
    }

    /// One control without a slot.
    fn flowing(id: &str) -> Item {
        item(id)
    }

    /// The rectangles of `group`, for the geometry assertions.
    fn rects_of(group: &Group, widths: &[f32], m: &Metrics) -> Vec<Rect> {
        place(group, widths, m)
            .into_iter()
            .map(|p| p.rect)
            .collect()
    }

    /// A stack group of `controls`.
    fn stack(controls: Vec<Item>) -> Group {
        Group {
            fluent: None,
            id: "group".to_owned(),
            label: Some("Group".to_owned()),
            layout: GroupLayout::Stack,
            rows: None,
            controls,
            launcher: None,
        }
    }

    #[test]
    fn same_column_gives_the_same_x_and_rows_run_downwards() {
        let group = stack(vec![
            slotted("a", Some(ControlSize::Medium), 0, 0),
            slotted("b", Some(ControlSize::Medium), 0, 1),
            slotted("c", Some(ControlSize::Small), 1, 0),
            slotted("d", Some(ControlSize::Small), 1, 2),
        ]);
        let rects = rects_of(&group, &[22.0, 22.0, 22.0, 22.0], &Metrics::default());
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0].left(), rects[1].left());
        assert_eq!(rects[2].left(), rects[3].left());
        assert!(rects[2].left() > rects[0].left());
        assert!(rects[1].top() > rects[0].top());
        assert!(rects[3].top() > rects[2].top());
        assert_eq!(rects[0].height(), 23.0);
    }

    #[test]
    fn large_controls_span_the_group_height() {
        let m = Metrics::default();
        let group = stack(vec![
            slotted("big", Some(ControlSize::Large), 0, 0),
            slotted("a", Some(ControlSize::Medium), 1, 0),
            slotted("b", Some(ControlSize::Medium), 1, 1),
        ]);
        let rects = rects_of(&group, &[42.0, 22.0, 22.0], &m);
        assert_eq!(rects[0].height(), m.content_height());
        assert_eq!(rects[0].top(), 0.0);
        assert!(rects[1].height() < m.content_height());
    }

    #[test]
    fn two_rows_are_centred_vertically() {
        let mut group = stack(vec![
            slotted("a", Some(ControlSize::Medium), 0, 0),
            slotted("b", Some(ControlSize::Medium), 0, 1),
        ]);
        group.rows = Some(2);
        let rects = rects_of(&group, &[22.0, 22.0], &Metrics::default());
        assert_eq!(rects[0].top(), rects[1].top() - 23.0);
        // Centred: the same space above and below.
        assert_eq!(rects[0].top(), 69.0 - rects[1].bottom());
    }

    #[test]
    fn rows_groups_flow_per_half_row_with_cluster_gaps() {
        let mut group = stack(vec![
            slotted("a", Some(ControlSize::Small), 0, 0),
            slotted("b", Some(ControlSize::Small), 1, 0),
            slotted("c", Some(ControlSize::Small), 4, 0),
            slotted("d", Some(ControlSize::Small), 0, 1),
            slotted("e", Some(ControlSize::Small), 1, 1),
        ]);
        group.layout = GroupLayout::Rows;
        let rects = rects_of(&group, &[22.0, 22.0, 22.0, 22.0, 22.0], &Metrics::default());
        // Row 0 sits above row 1.
        assert!(rects[0].top() < rects[3].top());
        assert_eq!(rects[0].top(), rects[1].top());
        assert_eq!(rects[3].top(), rects[4].top());
        // Columns flow left to right inside the row.
        assert_eq!(rects[0].left() + 22.0, rects[1].left());
        // The column jump 1 to 4 inserts one cluster gap.
        assert_eq!(
            rects[2].left(),
            rects[1].right() + Metrics::default().rows_gap
        );
    }

    #[test]
    fn flow_placement_fills_columns_and_sizes_the_first_menu_large() {
        let mut group = stack(vec![
            flowing("menu"),
            flowing("a"),
            flowing("b"),
            flowing("c"),
        ]);
        group.controls[0].kind = ItemKind::Dropdown;
        group.controls[0].items = vec![flowing("menu.child")];
        let rects = rects_of(&group, &[42.0, 22.0, 22.0, 22.0], &Metrics::default());
        // The first control with a menu is large and takes column 0.
        assert_eq!(rects[0].height(), Metrics::default().content_height());
        assert_eq!(rects[0].top(), 0.0);
        // The rest stack three per column beside it.
        assert_eq!(rects[1].left(), rects[2].left());
        assert_eq!(rects[2].left(), rects[3].left());
        assert_eq!(rects[1].top(), 0.0);
        assert_eq!(rects[2].top(), 23.0);
        assert_eq!(rects[3].top(), 46.0);
    }

    #[test]
    fn out_of_range_slots_are_clamped() {
        let group = stack(vec![
            slotted("a", Some(ControlSize::Medium), 0, 99),
            slotted("b", Some(ControlSize::Medium), 7, 0),
        ]);
        let rects = rects_of(&group, &[22.0, 22.0], &Metrics::default());
        assert!(rects[0].top() <= Metrics::default().content_height());
        assert!(rects[1].left() > rects[0].left());
    }

    #[test]
    fn missing_widths_fall_back_to_the_large_minimum() {
        let group = stack(vec![slotted("a", Some(ControlSize::Medium), 0, 0)]);
        let rects = rects_of(&group, &[], &Metrics::default());
        assert_eq!(rects[0].width(), Metrics::default().large_min_width);
        let rects = rects_of(&group, &[f32::NAN], &Metrics::default());
        assert_eq!(rects[0].width(), Metrics::default().large_min_width);
    }

    #[test]
    fn hidden_controls_have_no_slot_and_repeats_render_once() {
        let mut group = stack(vec![
            slotted("a", Some(ControlSize::Medium), 0, 0),
            slotted("a", Some(ControlSize::Medium), 0, 1),
            slotted("b", Some(ControlSize::Medium), 0, 2),
        ]);
        group.controls[2].hidden = true;
        let items = visible_controls(&group);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "a");
        let rects = rects_of(&group, &[22.0], &Metrics::default());
        assert_eq!(rects.len(), 1);
    }
}
