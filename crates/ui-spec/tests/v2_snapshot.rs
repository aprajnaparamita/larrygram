//! Snapshot pins for the layout format v2 (ADR 0065, task P7-03):
//!
//! - the full output of [`from_v1`] for the two v1 Fluent-data
//!   layouts (2016 and 365), rendered one placement per line;
//! - the count pins: the items per surface and per kind for each of
//!   the four layouts.
//!
//! A data or adapter change that moves a placement changes the
//! snapshot in the same commit. Regenerate with `INSTA_UPDATE=1
//! cargo test -p larrygram-ui-spec --test v2_snapshot`.

use std::path::{Path, PathBuf};

use larrygram_ui_spec::Layout;
use larrygram_ui_spec::v2::{Item, ItemKind, LayoutKind, LayoutV2, from_v1, load_layout};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One committed v1 layout, decoded from disk.
#[allow(clippy::expect_used)] // test helper; the committed data loads
fn v1_layout(name: &str) -> Layout {
    let text = std::fs::read_to_string(workspace_root().join("ui-spec").join(name))
        .expect("the committed v1 file");
    serde_saphyr::from_str(&text).expect("the committed v1 file decodes")
}

/// One line per item, with every set field. The nesting is the
/// leading indent.
fn render_items(items: &[Item], depth: usize, out: &mut String) {
    for item in items {
        let pad = "  ".repeat(depth);
        out.push_str(&format!("{pad}{} {}", item.id, kind_name(item.kind)));
        if let Some(label) = &item.label {
            out.push_str(&format!(" label={label:?}"));
        }
        if let Some(command) = &item.command {
            out.push_str(&format!(" cmd={command}"));
        }
        if item.new_command {
            out.push_str(" new");
        }
        if let Some(fluent) = &item.fluent {
            out.push_str(&format!(" fluent={fluent}"));
        }
        if let Some(icon) = &item.icon {
            out.push_str(&format!(" icon={icon}"));
        }
        if let Some(access) = item.access {
            out.push_str(&format!(" access={access}"));
        }
        if let Some(shortcut) = &item.shortcut {
            out.push_str(&format!(" shortcut={shortcut:?}"));
        }
        if let Some(size) = item.size {
            out.push_str(&format!(" size={size:?}"));
        }
        if let Some(column) = item.column {
            out.push_str(&format!(" column={column}"));
        }
        if let Some(row) = item.row {
            out.push_str(&format!(" row={row}"));
        }
        if item.begin_group {
            out.push_str(" begin_group");
        }
        if item.hidden {
            out.push_str(" hidden");
        }
        if item.dynamic_label {
            out.push_str(" dynamic_label");
        }
        if let Some(list) = &item.list {
            out.push_str(&format!(" list={list}"));
        }
        if !item.options.is_empty() {
            out.push_str(&format!(" options={:?}", item.options));
        }
        out.push('\n');
        render_items(&item.items, depth + 1, out);
    }
}

/// The name of an item kind, as in the data.
fn kind_name(kind: ItemKind) -> &'static str {
    use ItemKind as K;
    match kind {
        K::Button => "button",
        K::Toggle => "toggle",
        K::Check => "check",
        K::Split => "split",
        K::SplitRecent => "split_recent",
        K::Dropdown => "dropdown",
        K::Submenu => "submenu",
        K::Gallery => "gallery",
        K::Combo => "combo",
        K::Palette => "palette",
        K::Launcher => "launcher",
        K::Heading => "heading",
        K::List => "list",
        K::Options => "options",
    }
}

/// The whole layout, rendered for the pin.
fn render_layout(layout: &LayoutV2) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} {:?} theme={} kind={:?}\n",
        layout.id, layout.name, layout.theme, layout.kind
    ));
    if let Some(description) = &layout.description {
        out.push_str(&format!("description: {description}\n"));
    }
    for region in &layout.window.regions {
        out.push_str(&format!(
            "window {} {:?} size={:?}\n",
            region.id, region.edge, region.size
        ));
    }
    out.push_str(&format!(
        "quick_access_toolbar ({}):\n",
        layout.quick_access_toolbar.len()
    ));
    render_items(&layout.quick_access_toolbar, 1, &mut out);
    for bar in &layout.menu_bars {
        out.push_str(&format!(
            "menu_bar {} shown_in={:?}\n",
            bar.id,
            bar.shown_in.map(|c| c.as_str())
        ));
        for menu in &bar.menus {
            out.push_str(&format!("menu {} {:?}\n", menu.id, menu.label));
            render_items(&menu.items, 1, &mut out);
        }
    }
    for toolbar in &layout.toolbars {
        out.push_str(&format!(
            "toolbar {} shown_in={:?} dock={:?} row={:?}\n",
            toolbar.id,
            toolbar.shown_in.map(|c| c.as_str()),
            toolbar.dock,
            toolbar.row
        ));
        render_items(&toolbar.items, 1, &mut out);
    }
    for tab in &layout.tabs {
        out.push_str(&format!("tab {} {:?}\n", tab.id, tab.label));
        for group in &tab.groups {
            out.push_str(&format!(
                "group {} {:?} layout={:?} rows={:?}\n",
                group.id, group.label, group.layout, group.rows
            ));
            render_items(&group.controls, 1, &mut out);
            if let Some(launcher) = &group.launcher {
                out.push_str(&format!("launcher {} cmd={}\n", group.id, launcher.command));
            }
        }
    }
    for tab in &layout.contextual_tabs {
        out.push_str(&format!(
            "contextual_tab {} {:?} tab_set={:?} shown_in={:?}\n",
            tab.id,
            tab.label,
            tab.tab_set,
            tab.shown_in.map(|c| c.as_str())
        ));
        for group in &tab.groups {
            out.push_str(&format!("group {} {:?}\n", group.id, group.label));
            render_items(&group.controls, 1, &mut out);
        }
    }
    if let Some(view) = &layout.file_view {
        out.push_str(&format!(
            "file_view back_button={} default_page={}\n",
            view.back_button, view.default_page
        ));
        for entry in &view.nav {
            out.push_str(&format!(
                "nav {} {:?} {:?} cmd={:?}\n",
                entry.id,
                entry.kind,
                entry.label,
                entry.command.as_ref().map(|c| c.to_string())
            ));
        }
    }
    for menu in &layout.context_menus {
        out.push_str(&format!(
            "context_menu {} {:?} trigger={:?}\n",
            menu.id, menu.label, menu.trigger
        ));
        render_items(&menu.items, 1, &mut out);
    }
    out
}

/// Every item of the layout, across every surface (for the counts).
fn all_items(layout: &LayoutV2) -> Vec<&Item> {
    fn walk<'a>(items: &'a [Item], out: &mut Vec<&'a Item>) {
        for item in items {
            out.push(item);
            walk(&item.items, out);
        }
    }
    let mut out = Vec::new();
    walk(&layout.quick_access_toolbar, &mut out);
    for tab in layout.tabs.iter().chain(layout.contextual_tabs.iter()) {
        for group in &tab.groups {
            walk(&group.controls, &mut out);
        }
    }
    for bar in &layout.menu_bars {
        for menu in &bar.menus {
            walk(&menu.items, &mut out);
        }
    }
    for toolbar in &layout.toolbars {
        walk(&toolbar.items, &mut out);
    }
    for menu in &layout.context_menus {
        walk(&menu.items, &mut out);
    }
    out
}

/// The count pin: items per surface and per kind (ADR 0065).
fn render_counts(layout: &LayoutV2) -> String {
    fn count(items: &[Item]) -> usize {
        items.iter().map(|item| 1 + count(&item.items)).sum()
    }
    let mut out = format!("{}:\n", layout.id);
    out.push_str(&format!(
        "  quick_access_toolbar: {}\n",
        count(&layout.quick_access_toolbar)
    ));
    let tab_items: usize = layout
        .tabs
        .iter()
        .map(|tab| {
            tab.groups
                .iter()
                .map(|group| count(&group.controls))
                .sum::<usize>()
        })
        .sum();
    out.push_str(&format!(
        "  tabs: {} (items: {})\n",
        layout.tabs.len(),
        tab_items
    ));
    let contextual_items: usize = layout
        .contextual_tabs
        .iter()
        .map(|tab| {
            tab.groups
                .iter()
                .map(|group| count(&group.controls))
                .sum::<usize>()
        })
        .sum();
    out.push_str(&format!(
        "  contextual_tabs: {} (items: {})\n",
        layout.contextual_tabs.len(),
        contextual_items
    ));
    let menu_items: usize = layout
        .menu_bars
        .iter()
        .flat_map(|bar| bar.menus.iter())
        .map(|menu| count(&menu.items))
        .sum();
    out.push_str(&format!(
        "  menu_bars: {} (items: {})\n",
        layout.menu_bars.len(),
        menu_items
    ));
    let toolbar_items: usize = layout
        .toolbars
        .iter()
        .map(|toolbar| count(&toolbar.items))
        .sum();
    out.push_str(&format!(
        "  toolbars: {} (items: {})\n",
        layout.toolbars.len(),
        toolbar_items
    ));
    out.push_str(&format!(
        "  file_view nav: {}\n",
        layout
            .file_view
            .as_ref()
            .map(|view| view.nav.len())
            .unwrap_or(0)
    ));
    let menu_count: usize = layout
        .context_menus
        .iter()
        .map(|menu| count(&menu.items))
        .sum();
    out.push_str(&format!(
        "  context_menus: {} (items: {})\n",
        layout.context_menus.len(),
        menu_count
    ));
    let mut kinds_of: Vec<(&'static str, usize)> = Vec::new();
    for item in all_items(layout) {
        let name = kind_name(item.kind);
        match kinds_of.iter_mut().find(|(kind, _)| *kind == name) {
            Some((_, n)) => *n += 1,
            None => kinds_of.push((name, 1)),
        }
    }
    kinds_of.sort();
    out.push_str("  items by kind:");
    for (kind, n) in &kinds_of {
        out.push_str(&format!(" {kind}={}", n));
    }
    out.push('\n');
    out
}

#[test]
fn panel_2016_adapts_to_the_pinned_v2_layout() {
    let v1 = v1_layout("panel-2016.yaml");
    let layout = from_v1(&v1).expect("the 2016 layout converts");
    insta::assert_snapshot!(render_layout(&layout));
}

#[test]
fn panel_365_adapts_to_the_pinned_v2_layout() {
    let v1 = v1_layout("panel-365.yaml");
    let layout = from_v1(&v1).expect("the 365 layout converts");
    insta::assert_snapshot!(render_layout(&layout));
}

#[test]
fn panel_modern_items_per_surface_and_kind_are_pinned() {
    insta::assert_snapshot!(render_counts(
        &load_layout(LayoutKind::PanelModern).expect("the layout loads")
    ));
}

#[test]
fn panel_classic_items_per_surface_and_kind_are_pinned() {
    insta::assert_snapshot!(render_counts(
        &load_layout(LayoutKind::PanelClassic).expect("the layout loads")
    ));
}

#[test]
fn panel_2016_items_per_surface_and_kind_are_pinned() {
    insta::assert_snapshot!(render_counts(
        &load_layout(LayoutKind::Panel2016).expect("the layout loads")
    ));
}

#[test]
fn panel_365_items_per_surface_and_kind_are_pinned() {
    insta::assert_snapshot!(render_counts(
        &load_layout(LayoutKind::Panel365).expect("the layout loads")
    ));
}
