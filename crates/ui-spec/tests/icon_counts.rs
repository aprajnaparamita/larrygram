//! The icon count per layout is pinned (Phase 7 exit criterion 4,
//! task P7-10): every item with an `icon` key draws its Tabler icon,
//! never the placeholder.
//!
//! The chain that proves "never the placeholder" has three links, and
//! this test pins the first: every icon key the layouts use has a row
//! in `ui-spec/icons.yaml` (the loader's cross-check is the same
//! rule), so the icon map resolves it. The second link is the codegen
//! table (`cargo xtask codegen --check` keeps `crates/ui/src/icons/
//! table.rs` in step with `icons.yaml`), and the third is the
//! rasterisation tests in `crates/ui/src/icons/mod.rs`
//! (`every_map_name_rasterises_at_16_and_32` and
//! `every_entry_and_icon_resolves`). The counts below are the pin: a
//! data change that adds or drops an icon shows up here.

// The never-crash lints apply to non-test code only (AGENTS.md
// section 6); these helpers are test code and use `expect` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use std::path::{Path, PathBuf};

use larrygram_ui_spec::Loader;
use larrygram_ui_spec::v2::{IconMap, Item, LayoutKind, LayoutV2};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The pinned count of items with an `icon` key, per layout.
const ICON_ITEMS: &[(LayoutKind, usize)] = &[
    (LayoutKind::PanelModern, 246),
    (LayoutKind::PanelClassic, 511),
    (LayoutKind::Panel2016, 409),
    (LayoutKind::Panel365, 449),
];

/// The pinned count of distinct icon keys, per layout.
const ICON_KEYS: &[(LayoutKind, usize)] = &[
    (LayoutKind::PanelModern, 212),
    (LayoutKind::PanelClassic, 270),
    (LayoutKind::Panel2016, 355),
    (LayoutKind::Panel365, 386),
];

/// Walks one item tree, calling `visit` on every item (nested items
/// included).
fn walk_items(items: &[Item], visit: &mut impl FnMut(&Item)) {
    for item in items {
        visit(item);
        walk_items(&item.items, visit);
    }
}

/// Every item of one layout, in data order.
fn layout_items(layout: &LayoutV2, out: &mut Vec<Item>) {
    let mut collect = |item: &Item| out.push(item.clone());
    walk_items(&layout.quick_access_toolbar, &mut collect);
    for tab in layout.tabs.iter().chain(layout.contextual_tabs.iter()) {
        for group in &tab.groups {
            walk_items(&group.controls, &mut collect);
        }
    }
    for bar in &layout.menu_bars {
        for menu in &bar.menus {
            walk_items(&menu.items, &mut collect);
        }
    }
    for toolbar in &layout.toolbars {
        walk_items(&toolbar.items, &mut collect);
    }
    for menu in &layout.context_menus {
        walk_items(&menu.items, &mut collect);
    }
}

/// The icon keys one layout uses and how many items carry one.
fn icon_use(layout: &LayoutV2) -> (usize, Vec<String>) {
    let mut items = Vec::new();
    layout_items(layout, &mut items);
    let mut keys = Vec::new();
    let mut count = 0;
    for item in &items {
        if let Some(icon) = &item.icon {
            count += 1;
            keys.push(icon.clone());
        }
    }
    keys.sort();
    keys.dedup();
    (count, keys)
}

/// The icon count per layout is pinned (Phase 7 exit criterion 4).
#[test]
fn the_icon_count_per_layout_is_pinned() {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    let sets = loader.load_all().expect("the committed data loads");
    for (kind, pinned_items) in ICON_ITEMS {
        let layout = &sets.get(*kind).v2;
        let (count, keys) = icon_use(layout);
        let pinned_keys = ICON_KEYS
            .iter()
            .find(|(other, _)| other == kind)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        assert_eq!(
            count, *pinned_items,
            "{kind:?}: the number of items with an icon key moved"
        );
        assert_eq!(
            keys.len(),
            pinned_keys,
            "{kind:?}: the number of distinct icon keys moved ({keys:?})"
        );
    }
}

/// The pinned count of icon keys without an icon-map row (they draw
/// the placeholder, ADR 0062). Panel Modern and Panel Classic have
/// none: every item with an `icon` key draws its Tabler icon. The two
/// adapted Fluent layouts name command ids as icon keys
/// (`v2::from_v1`), and one without a row draws the placeholder by
/// documented rule.
const UNRESOLVED_KEYS: &[(LayoutKind, usize)] = &[
    (LayoutKind::PanelModern, 0),
    (LayoutKind::PanelClassic, 0),
    (LayoutKind::Panel2016, 144),
    (LayoutKind::Panel365, 174),
];

/// Every icon key the curated layouts use resolves in the icon map,
/// so the icon pipeline draws the Tabler icon, never the placeholder
/// (Phase 7 exit criterion 4).
#[test]
fn every_curated_layout_icon_key_resolves_in_the_icon_map() {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    let sets = loader.load_all().expect("the committed data loads");
    let icons: &IconMap = &sets.panel_modern.icons;
    for (kind, pinned_unresolved) in UNRESOLVED_KEYS {
        let (_count, keys) = icon_use(&sets.get(*kind).v2);
        let unresolved: Vec<&String> = keys
            .iter()
            .filter(|key| !icons.icons.contains_key(key.as_str()))
            .collect();
        assert_eq!(
            unresolved.len(),
            *pinned_unresolved,
            "{kind:?}: the number of icon keys without a map row moved"
        );
        if *pinned_unresolved == 0 {
            for key in &keys {
                let entry = icons
                    .icons
                    .get(key.as_str())
                    .unwrap_or_else(|| panic!("{kind:?}: no icon row for {key:?}"));
                assert!(
                    !entry.icon.is_empty(),
                    "{kind:?}: the icon row for {key:?} names no SVG"
                );
            }
        }
    }
}

/// The icon map itself: every row names a non-empty icon and every
/// tint token it uses is non-empty (the theme test checks the tints
/// resolve).
#[test]
fn every_icon_map_row_is_complete() {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    let sets = loader.load_all().expect("the committed data loads");
    let icons = &sets.panel_modern.icons;
    assert!(!icons.icons.is_empty(), "the icon map has rows");
    for (key, entry) in &icons.icons {
        assert!(!entry.icon.is_empty(), "the row {key:?} names no icon");
        if let Some(tint) = &entry.tint {
            assert!(
                icons.tints.contains_key(tint.as_str()),
                "the row {key:?} names an unknown tint {tint:?}"
            );
        }
    }
}
