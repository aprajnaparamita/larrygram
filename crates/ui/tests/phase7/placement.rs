//! The ADR 0065 placement test (task P7-10): the placement rectangles
//! of every Panel Modern group's controls respect `column`, `row` and
//! `size` — same column gives the same x, rows run downwards and large
//! controls span the group height.
//!
//! The rectangles come from the placement model the widgets render
//! (`panel_modern::place::place`, with the committed theme's metrics
//! and the data's slot rules), for every committed group of every core
//! and contextual tab. The widget sweep (`modern`) pins the rendered
//! side: one widget draws per placed control, in placement order. The
//! accesskit node rectangles cannot check the exact positions: a
//! control's node reports the transformed bounds of its painted atoms,
//! which are wider than the widget itself.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_ui::panel_modern::metrics::Metrics;
use larrygram_ui::panel_modern::place;
use larrygram_ui_spec::v2::{Group, GroupLayout, LayoutKind, Tab};

use crate::modern::{ModernRig, activate_tab, tab_controls};
use crate::sets;

/// The width one control takes in the check: the layout's combo width,
/// the large minimum otherwise (the exact width does not move the
/// invariants: the column x is the max over the column, and the row
/// heights come from the metrics).
fn width_of(item: &larrygram_ui_spec::v2::Item) -> f32 {
    match item.kind {
        larrygram_ui_spec::v2::ItemKind::Combo => (item.width.unwrap_or(107.0) as f32).max(1.0),
        _ => 42.0,
    }
}

/// The placement invariants of one group (ADR 0065): same column
/// gives the same x, rows increase downwards, large controls span the
/// group height.
fn check_group(group: &Group, metrics: &Metrics) {
    let items = place::visible_controls(group);
    assert!(!items.is_empty() || group.controls.iter().all(|c| c.hidden));
    let widths: Vec<f32> = items.iter().map(|item| width_of(item)).collect();
    let placed = place::place(group, &widths, metrics);
    assert_eq!(
        placed.len(),
        items.len(),
        "one rectangle per visible control"
    );
    for (item, p) in items.iter().zip(&placed) {
        assert!(
            p.rect.width() > 0.0 && p.rect.height() > 0.0,
            "{:?} has a usable rectangle",
            item.id
        );
        assert!(
            p.rect.min.x.is_finite() && p.rect.min.y.is_finite(),
            "{:?} has a finite placement",
            item.id
        );
    }
    // A large control spans the group height: the content area from
    // the top down to the caption (ADR 0065).
    for (item, p) in items.iter().zip(&placed) {
        if place::is_large(item) {
            assert_eq!(
                p.rect.height(),
                metrics.content_height(),
                "{:?} spans the group height",
                item.id
            );
            assert_eq!(p.rect.top(), 0.0, "{:?} starts at the top", item.id);
        }
    }
    let slotted = !items.is_empty()
        && items
            .iter()
            .all(|item| item.column.is_some() && item.row.is_some());
    if !slotted {
        return;
    }
    for (i, a) in items.iter().enumerate() {
        let ra = placed[i].rect;
        for (j, b) in items.iter().enumerate() {
            let rb = placed[j].rect;
            match group.layout {
                GroupLayout::Stack => {
                    if a.column == b.column {
                        assert_eq!(
                            ra.left(),
                            rb.left(),
                            "{:?} and {:?} share a column x",
                            a.id,
                            b.id
                        );
                        if a.row > b.row && !(placed[i].large || placed[j].large) {
                            assert!(ra.top() > rb.top(), "{:?} sits below {:?}", a.id, b.id);
                        }
                    }
                }
                GroupLayout::Rows => {
                    if a.row == b.row {
                        assert_eq!(
                            ra.top(),
                            rb.top(),
                            "{:?} and {:?} share a row y",
                            a.id,
                            b.id
                        );
                        if a.column > b.column {
                            assert!(ra.left() > rb.left(), "{:?} sits right of {:?}", a.id, b.id);
                        }
                    } else if a.row > b.row {
                        assert!(ra.top() > rb.top(), "{:?} sits below {:?}", a.id, b.id);
                    }
                }
            }
        }
    }
}

/// The placement rectangles of every Panel Modern group respect their
/// `column`, `row` and `size` slots (ADR 0065 item 4).
#[test]
fn every_group_control_places_in_its_slot() {
    let layout = sets().get(LayoutKind::PanelModern).v2.clone();
    // The metrics the widgets render with: the committed theme's.
    let theme = &sets().get(LayoutKind::PanelModern).theme;
    let metrics = Metrics::from_theme(theme);
    let tabs: Vec<&Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    assert_eq!(layout.tabs.len(), 7, "the seven core tabs");
    let mut groups = 0;
    let mut controls = 0;
    for tab in &tabs {
        for group in &tab.groups {
            check_group(group, &metrics);
            groups += 1;
            controls += place::visible_controls(group).len();
        }
    }
    assert!(
        groups >= 20 && controls >= 100,
        "the sweep covered {groups} groups and {controls} controls"
    );
}

/// Every group of every tab draws one widget per visible control, in
/// placement order (the rendered side of ADR 0065 item 4: the widgets
/// draw at the placed rectangles).
#[test]
fn every_group_control_draws_one_widget_in_order() {
    let mut rig = ModernRig::new();
    let layout = sets().get(LayoutKind::PanelModern).v2.clone();
    let tabs: Vec<&Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    let mut controls = 0;
    for tab in &tabs {
        let label = tab.label.clone().unwrap_or_default();
        let drawn = activate_tab(&mut rig, &label, tab);
        let expected = tab_controls(tab);
        assert_eq!(
            drawn.len(),
            expected.len(),
            "the tab {label:?} draws one widget per control"
        );
        for (entry, control) in drawn.iter().zip(&expected) {
            assert_eq!(
                entry.control.item.id, control.item.id,
                "the tab {label:?} draws its controls in placement order"
            );
            controls += 1;
        }
    }
    assert!(controls >= 100, "the sweep covered {controls} controls");
}
