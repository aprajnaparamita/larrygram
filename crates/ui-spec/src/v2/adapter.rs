//! The v1-to-v2 adapter (ADR 0061 "Adapter for v1 layouts", task
//! P7-03).
//!
//! [`from_v1`] turns one v1 Fluent-data layout
//! (`ui-spec/panel-2016.yaml`, `ui-spec/panel-365.yaml`) into a
//! panel-modern [`LayoutV2`] that the v2 widgets read:
//!
//! - each v1 tab becomes a [`Tab`], and each group a [`Group`] with
//!   [`GroupLayout::Stack`] and no slot data, so controls flow in file
//!   order;
//! - galleries, menus and dropdowns become [`ItemKind::Dropdown`]
//!   items whose children come from the v1 `parent` chain;
//! - the backstage becomes the [`FileView`]: one nav entry per
//!   backstage tab (`kind: command` when the tab's controls dispatch
//!   exactly one distinct command, `kind: page` otherwise);
//! - the context menus and the quick access toolbar are carried over.
//!
//! Placement ids are built from the converted path
//! (`tab_home.group_clipboard.cut`), so they are unique per layout even
//! though the v1 data reuses Fluent ids across surfaces. Labels that
//! the v1 data carries as a bare Fluent id (`TabHome`) become display
//! labels (`Home`); a real label is kept as it is.
//!
//! Known limits, pinned by the snapshot tests:
//!
//! - a backstage tab's controls become one nav entry; the File view
//!   pages themselves are not modelled in v2 yet;
//! - a contextual tab keeps its Fluent tab-set name (trimmed to the
//!   display name) and gains `shown_in` only for the three tab sets
//!   that map to a context of `ui-spec/contexts.yaml`; the others are
//!   never shown until P7-08 maps them;
//! - items nested deeper than [`MAX_TREE_DEPTH`] levels, and items in
//!   a `parent` cycle, become top-level items (never a stack
//!   overflow).

use std::collections::BTreeMap;

use crate::v2::{
    ContextId, ContextMenuV2, Edge, FileButton, FileView, FileViewEntry, FileViewEntryKind, Group,
    GroupLayout, Item, ItemKind, LayoutKind, LayoutV2, PanelStyle, Tab, V2Error, Window,
    WindowRegion,
};
use crate::{Control, ControlType, Layout};

/// The deepest item nesting the adapter builds. The v1 data nests two
/// or three levels (menu entries under a menu); this bound keeps a
/// hostile `parent` chain from overflowing any stack downstream
/// (AGENTS.md section 6).
const MAX_TREE_DEPTH: usize = 32;

/// Fluent tab-set names that are not contextual tab sets.
const CORE_TAB_SETS: [&str; 3] = ["None (Core Tab)", "TabSetPrimary", ""];

/// Converts one v1 Fluent-data layout into the v2 model (ADR 0061).
///
/// The result is a panel-modern layout (`kind: panel-modern`) with the
/// Panel Modern theme and frame (ADR 0063): the widgets render the two
/// adapted layouts with the Panel Modern widget set. The layout id
/// comes from the v1 source pin (`source` mentions 365 for the
/// Microsoft 365 data, 2016 otherwise).
pub fn from_v1(layout: &Layout) -> Result<LayoutV2, V2Error> {
    if layout.schema_version != 1 {
        return Err(V2Error::Convert {
            message: format!(
                "schema_version {} is not the v1 format (expected 1)",
                layout.schema_version
            ),
        });
    }
    let kind = if layout.source.contains("365") {
        LayoutKind::Panel365
    } else {
        LayoutKind::Panel2016
    };
    let mut top = Ids::default();
    let mut tabs = Vec::new();
    let mut contextual_tabs = Vec::new();
    for tab in sorted_by_order(&layout.tabs) {
        let tab_id = top.claim(&slug(tab.id.as_str()));
        let core = is_core_tab(tab.tab_set.as_deref());
        let converted = Tab {
            id: tab_id.clone(),
            label: Some(display_label(tab.id.as_str(), &tab.label)),
            fluent: Some(tab.id.clone()),
            tab_set: if core {
                None
            } else {
                Some(tab_set_display(tab.tab_set.as_deref().unwrap_or_default()))
            },
            shown_in: if core {
                None
            } else {
                tab_set_context(tab.tab_set.as_deref().unwrap_or_default())
            },
            groups: convert_groups(&tab.groups, &tab_id),
        };
        if core {
            tabs.push(converted);
        } else {
            contextual_tabs.push(converted);
        }
    }

    let qat_root = top.claim("qat");
    let quick_access_toolbar = convert_controls(&layout.quick_access_toolbar.controls, &qat_root);

    let mut nav = Vec::new();
    let mut nav_ids = Ids::default();
    for tab in sorted_by_order(&layout.backstage.tabs) {
        let id = format!("file_view.{}", nav_ids.claim(&slug(tab.id.as_str())));
        let mut commands = Vec::new();
        for control in sorted_by_order(&tab.controls) {
            if let Some(command) = &control.command
                && !commands.contains(command)
            {
                commands.push(command.clone());
            }
        }
        nav.push(FileViewEntry {
            id,
            kind: if commands.len() == 1 {
                FileViewEntryKind::Command
            } else {
                FileViewEntryKind::Page
            },
            label: display_label(tab.id.as_str(), &tab.label),
            command: commands.into_iter().next(),
        });
    }
    let default_page = nav
        .first()
        .map(|entry| entry.id.clone())
        .unwrap_or_else(|| "file_view".to_owned());

    let mut context_menus = Vec::new();
    for menu in sorted_by_order(&layout.context_menus) {
        let id = top.claim(&slug(menu.id.as_str()));
        context_menus.push(ContextMenuV2 {
            id: id.clone(),
            label: Some(display_label(menu.id.as_str(), &menu.label)),
            trigger: None,
            items: convert_controls(&menu.controls, &id),
        });
    }

    Ok(LayoutV2 {
        schema_version: 2,
        id: kind.id().to_owned(),
        name: kind.label().to_owned(),
        kind: PanelStyle::PanelModern,
        description: Some(format!(
            "Adapted from the v1 Fluent data ({}).",
            layout.source
        )),
        theme: "panel-modern".to_owned(),
        window: panel_modern_window(),
        menu_bars: Vec::new(),
        toolbars: Vec::new(),
        quick_access_toolbar,
        file_button: Some(FileButton {
            label: "File".to_owned(),
            opens: "file_view".to_owned(),
        }),
        tabs,
        contextual_tabs,
        file_view: Some(FileView {
            back_button: true,
            default_page,
            nav,
        }),
        context_menus,
    })
}

/// The Panel Modern frame regions (the curated
/// `ui-spec/layouts/panel-modern.yaml` window), which the Panel
/// Modern widgets lay out for the adapted layouts too.
fn panel_modern_window() -> Window {
    let region = |id: &str, edge: Edge, size: f64, notes: Option<&str>| WindowRegion {
        id: id.to_owned(),
        edge,
        size: Some(size),
        per_row: false,
        collapsible: false,
        notes: notes.map(str::to_owned),
    };
    Window {
        regions: vec![
            region("title_bar", Edge::Top, 25.0, Some("Quick Access Toolbar at the left, document title centred, help and window buttons at the right.")),
            region("tab_strip", Edge::Top, 25.0, Some("File button, then the tabs; contextual tab sets appear to the right of the core tabs under a coloured set caption.")),
            WindowRegion {
                collapsible: true,
                notes: Some("Groups left to right with captions along the bottom edge; a collapse button at the far right.".to_owned()),
                ..region("command_bar", Edge::Top, 93.0, None)
            },
            region("shapes_window", Edge::Left, 262.0, Some("Shapes window: title, STENCILS and SEARCH headings, then stencil sections.")),
            region("rulers", Edge::TopLeftOfDrawing, 16.0, None),
            WindowRegion {
                size: None,
                ..region("drawing", Edge::Fill, 0.0, None)
            },
            region("page_tabs", Edge::BottomOfDrawing, 40.0, Some("Page tabs, an All-pages menu and an insert-page button below the horizontal scroll bar.")),
            region("status_bar", Edge::Bottom, 22.0, Some("Page n of m and language on the left; presentation, zoom slider, zoom percent, fit-page and switch-window buttons on the right.")),
        ],
    }
}

/// Converts the groups of one tab. The group id continues the tab's
/// id, so every placement id is a path.
fn convert_groups(groups: &[crate::Group], tab_id: &str) -> Vec<Group> {
    let mut ids = Ids::default();
    sorted_by_order(groups)
        .into_iter()
        .map(|group| {
            let id = format!("{tab_id}.{}", ids.claim(&slug(group.id.as_str())));
            Group {
                label: Some(display_label(group.id.as_str(), &group.label)),
                fluent: Some(group.id.clone()),
                controls: convert_controls(&group.controls, &id),
                id,
                layout: GroupLayout::Stack,
                rows: None,
                launcher: None,
            }
        })
        .collect()
}

/// Converts one surface's control list into v2 items: sorted by
/// `order`, nested through the v1 `parent` chain, ids unique per
/// scope.
fn convert_controls(controls: &[Control], root_id: &str) -> Vec<Item> {
    let display = display_order(controls);
    let parents = parent_links(controls, &display);
    let depths = link_depths(&parents);
    let ids = placement_ids(controls, &display, &depths, root_id);
    assemble(controls, &display, &depths, &ids)
}

/// The control indices in sibling order (`order`, then file order).
fn display_order(controls: &[Control]) -> Vec<usize> {
    let mut display: Vec<usize> = (0..controls.len()).collect();
    display.sort_by_key(|&index| (controls[index].order, index));
    display
}

/// The parent of each control, in display positions: the first
/// control with the same Fluent id anywhere in the list, or `None`
/// for a root. A control is never its own parent.
fn parent_links(controls: &[Control], display: &[usize]) -> Vec<Option<usize>> {
    let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for (position, &index) in display.iter().enumerate() {
        by_id.entry(controls[index].id.as_str()).or_insert(position);
    }
    display
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let parent = controls[index].parent.as_ref()?;
            let &found = by_id.get(parent.as_str())?;
            (found != position).then_some(found)
        })
        .collect()
}

/// The effective parent and depth of each control. A chain of
/// [`MAX_TREE_DEPTH`] or more links, or a `parent` cycle, breaks at
/// the control itself (it becomes a root), so the assembled tree is
/// always bounded and acyclic.
fn link_depths(parents: &[Option<usize>]) -> Vec<(Option<usize>, usize)> {
    parents
        .iter()
        .enumerate()
        .map(|(position, &parent)| {
            let mut depth = 0;
            let mut current = position;
            while let Some(above) = parents[current] {
                depth += 1;
                if depth >= MAX_TREE_DEPTH {
                    return (None, 0);
                }
                current = above;
            }
            (parent, depth)
        })
        .collect()
}

/// The placement id of each control: `<parent id>.<slug>`, with the
/// slug de-duplicated inside its scope. Parents are claimed before
/// their children.
fn placement_ids(
    controls: &[Control],
    display: &[usize],
    depths: &[(Option<usize>, usize)],
    root_id: &str,
) -> Vec<String> {
    let mut shallowest_first: Vec<usize> = (0..display.len()).collect();
    shallowest_first.sort_by_key(|&position| (depths[position].1, position));
    let mut scopes: BTreeMap<usize, Ids> = BTreeMap::new();
    let mut root_scope = Ids::default();
    let mut ids = vec![String::new(); display.len()];
    for position in shallowest_first {
        let base = slug(controls[display[position]].id.as_str());
        ids[position] = match depths[position].0 {
            None => format!("{root_id}.{}", root_scope.claim(&base)),
            Some(above) => {
                let scope = scopes.entry(above).or_default();
                format!("{}.{}", ids[above], scope.claim(&base))
            }
        };
    }
    ids
}

/// Builds the items from the prepared links: one arena of items, then
/// children pushed into their parents in stable depth order, so
/// siblings keep their display order.
fn assemble(
    controls: &[Control],
    display: &[usize],
    depths: &[(Option<usize>, usize)],
    ids: &[String],
) -> Vec<Item> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); display.len()];
    for (position, &(effective, _)) in depths.iter().enumerate() {
        if let Some(above) = effective {
            children[above].push(position);
        }
    }
    let mut arena: Vec<Option<Item>> = display
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let control = &controls[index];
            Some(Item {
                id: ids[position].clone(),
                kind: item_kind(control.r#type, !children[position].is_empty()),
                label: Some(display_label(control.id.as_str(), &control.label)),
                access: None,
                shortcut: None,
                command: control.command.clone(),
                new_command: false,
                fluent: Some(control.id.clone()),
                visuicmd: None,
                icon: control.command.as_ref().map(|command| command.to_string()),
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
            })
        })
        .collect();
    let mut deepest_first: Vec<usize> = (0..display.len()).collect();
    deepest_first.sort_by_key(|&position| std::cmp::Reverse(depths[position].1));
    for position in deepest_first {
        if let Some(above) = depths[position].0
            && let Some(item) = arena[position].take()
            && let Some(parent) = arena[above].as_mut()
        {
            parent.items.push(item);
        }
    }
    let mut roots = Vec::new();
    for (position, slot) in arena.iter_mut().enumerate() {
        if depths[position].0.is_none()
            && let Some(item) = slot.take()
        {
            roots.push(item);
        }
    }
    roots
}

/// The v2 item kind of one v1 control type (ADR 0061: galleries,
/// menus and dropdowns all become dropdown items).
fn item_kind(control_type: ControlType, has_children: bool) -> ItemKind {
    use ControlType as C;
    use ItemKind as K;
    match control_type {
        C::Button | C::Task | C::Control => K::Button,
        C::ToggleButton => K::Toggle,
        C::CheckBox => K::Check,
        C::SplitButton => K::Split,
        C::Gallery | C::Menu | C::DropDown => K::Dropdown,
        C::ComboBox => K::Combo,
        C::LabelControl => K::Heading,
        C::ButtonDialogBoxLauncher => K::Launcher,
        // Structural Fluent types that only appear inside a parent
        // chain: a container becomes a submenu when it has children,
        // a heading when it does not.
        C::Tab
        | C::TabSet
        | C::Group
        | C::ContextMenu
        | C::Category
        | C::TaskFormGroup
        | C::None => {
            if has_children {
                K::Submenu
            } else {
                K::Heading
            }
        }
    }
}

/// True for a core tab: no tab set, the extractor's "None (Core Tab)"
/// marker, or the primary tab set.
fn is_core_tab(tab_set: Option<&str>) -> bool {
    tab_set.is_none_or(|name| CORE_TAB_SETS.contains(&name))
}

/// The display name of a Fluent tab-set name: `TabSetPictureTools`
/// becomes `Picture Tools`.
fn tab_set_display(name: &str) -> String {
    let stripped = name.strip_prefix("TabSet").unwrap_or(name);
    words(stripped).join(" ")
}

/// The context a contextual tab set appears in (ADR 0064). Tab sets
/// with no context in `ui-spec/contexts.yaml` stay hidden until
/// P7-08 maps them.
fn tab_set_context(name: &str) -> Option<ContextId> {
    match name {
        "TabSetPictureTools" => Some(ContextId::PictureSelected),
        "TabSetContainerTools" => Some(ContextId::ContainerSelected),
        "TabSetShapeSheetTools" => Some(ContextId::ShapesheetWindow),
        _ => None,
    }
}

/// The display label of a v1 record. The v1 data often carries the
/// Fluent id as the label (`TabHome`); that becomes the display words
/// with the structural prefix stripped (`Home`). A real label is kept.
fn display_label(id: &str, label: &str) -> String {
    if label != id {
        return label.to_owned();
    }
    for prefix in [
        "PlaceTab",
        "ContextMenu",
        "TaskFormGroup",
        "Button",
        "Category",
        "Group",
        "Label",
        "Menu",
        "Tab",
    ] {
        if let Some(rest) = label.strip_prefix(prefix)
            && rest.starts_with(|c: char| c.is_ascii_uppercase())
        {
            return words(rest).join(" ");
        }
    }
    words(label).join(" ")
}

/// The `ItemId`-safe slug of a Fluent id: camel-case words joined by
/// `_`, lowercased (`TabHome` becomes `tab_home`).
fn slug(name: &str) -> String {
    let mut out = words(name).join("_").to_ascii_lowercase();
    if out.is_empty() {
        return "item".to_owned();
    }
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert_str(0, "x_");
    }
    out
}

/// The camel-case words of a name, split at non-alphanumerics and at
/// camel-case boundaries (`TabSetPictureTools` becomes
/// `["Tab", "Set", "Picture", "Tools"]`).
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = if index == 0 {
            None
        } else {
            chars.get(index - 1)
        };
        let next = chars.get(index + 1);
        let starts_word = c.is_ascii_uppercase()
            && !current.is_empty()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
        if starts_word {
            words.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// A scope of placement ids: the first claim keeps the slug, the next
/// ones gain `_2`, `_3`, ...
#[derive(Default)]
struct Ids {
    used: BTreeMap<String, u32>,
}

impl Ids {
    fn claim(&mut self, base: &str) -> String {
        let count = self.used.entry(base.to_owned()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base.to_owned()
        } else {
            format!("{base}_{count}")
        }
    }
}

/// Sibling order, for [`sorted_by_order`].
trait HasOrder {
    fn order_value(&self) -> u32;
}

impl HasOrder for Control {
    fn order_value(&self) -> u32 {
        self.order
    }
}

impl HasOrder for crate::Group {
    fn order_value(&self) -> u32 {
        self.order
    }
}

impl HasOrder for crate::Tab {
    fn order_value(&self) -> u32 {
        self.order
    }
}

impl HasOrder for crate::BackstageTab {
    fn order_value(&self) -> u32 {
        self.order
    }
}

impl HasOrder for crate::ContextMenu {
    fn order_value(&self) -> u32 {
        self.order
    }
}

/// The items in `order` (stable: file order breaks ties).
fn sorted_by_order<T: HasOrder>(items: &[T]) -> Vec<&T> {
    let mut indexed: Vec<(usize, &T)> = items.iter().enumerate().collect();
    indexed.sort_by_key(|(index, item)| (item.order_value(), *index));
    indexed.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(json: serde_json::Value) -> Control {
        serde_json::from_value(json).expect("a v1 control decodes")
    }

    fn v1_layout(tabs: Vec<crate::Tab>) -> Layout {
        Layout {
            schema_version: 1,
            tabs,
            ..Layout::default()
        }
    }

    fn one_tab(groups: Vec<crate::Group>) -> crate::Tab {
        serde_json::from_value(serde_json::json!({
            "id": "TabHome", "label": "TabHome", "order": 0,
            "tab_set": "None (Core Tab)",
            "groups": []
        }))
        .map(|mut tab: crate::Tab| {
            tab.groups = groups;
            tab
        })
        .expect("a v1 tab decodes")
    }

    fn one_group(controls: Vec<Control>) -> crate::Group {
        serde_json::from_value(serde_json::json!({
            "id": "GroupClipboard", "label": "GroupClipboard", "order": 0,
            "controls": []
        }))
        .map(|mut group: crate::Group| {
            group.controls = controls;
            group
        })
        .expect("a v1 group decodes")
    }

    fn simple(id: &str, label: &str, order: u32) -> serde_json::Value {
        serde_json::json!({"id": id, "type": "button", "label": label, "order": order})
    }

    #[test]
    fn rejects_a_layout_that_is_not_v1() {
        let error = from_v1(&Layout {
            schema_version: 2,
            ..Layout::default()
        })
        .expect_err("must fail");
        assert!(
            error.to_string().contains("schema_version 2"),
            "got {error}"
        );
    }

    #[test]
    fn an_empty_layout_converts_to_an_empty_panel_modern_layout() {
        let layout = from_v1(&Layout::default()).expect("converts");
        assert_eq!(layout.kind, PanelStyle::PanelModern);
        assert_eq!(layout.theme, "panel-modern");
        assert_eq!(layout.schema_version, 2);
        assert_eq!(layout.id, "panel-2016");
        assert_eq!(layout.name, "Panel (2016 data)");
        assert!(layout.tabs.is_empty());
        assert_eq!(
            layout.file_button.as_ref().expect("file button").opens,
            "file_view"
        );
        assert!(!layout.window.regions.is_empty());
    }

    #[test]
    fn a_365_source_pins_the_365_ids() {
        let layout = from_v1(&Layout {
            source: "Microsoft 365, Current Channel".to_owned(),
            ..Layout::default()
        })
        .expect("converts");
        assert_eq!(layout.id, "panel-365");
        assert_eq!(layout.name, "Panel (365 data)");
    }

    #[test]
    fn ids_are_paths_and_slugs_of_the_fluent_ids() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![control(
            simple("Cut", "Cut", 0),
        )])])]))
        .expect("converts");
        let tab = &layout.tabs[0];
        assert_eq!(tab.id, "tab_home");
        assert_eq!(tab.groups[0].id, "tab_home.group_clipboard");
        assert_eq!(tab.groups[0].controls[0].id, "tab_home.group_clipboard.cut");
    }

    #[test]
    fn duplicate_slugs_get_a_suffix_within_their_scope() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![
            control(simple("Cut", "Cut", 0)),
            control(simple("cut", "cut", 1)),
            control(simple("Cut", "Cut again", 2)),
        ])])]))
        .expect("converts");
        let ids: Vec<&str> = layout.tabs[0].groups[0]
            .controls
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "tab_home.group_clipboard.cut",
                "tab_home.group_clipboard.cut_2",
                "tab_home.group_clipboard.cut_3"
            ]
        );
    }

    #[test]
    fn controls_flow_in_order_field_then_file_order() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![
            control(simple("Second", "Second", 5)),
            control(simple("First", "First", 1)),
            control(simple("Tie", "Tie", 1)),
        ])])]))
        .expect("converts");
        let labels: Vec<&str> = layout.tabs[0].groups[0]
            .controls
            .iter()
            .map(|item| item.label.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(labels, vec!["First", "Tie", "Second"]);
    }

    #[test]
    fn the_parent_chain_nests_children_under_their_menu() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![
            control(serde_json::json!({
                "id": "PasteMenu", "type": "splitButton", "label": "Paste",
                "order": 7, "command": "edit.paste"
            })),
            control(serde_json::json!({
                "id": "PasteText", "type": "button", "label": "Keep Text",
                "order": 2, "parent": "PasteMenu", "command": "edit.paste.text"
            })),
            control(serde_json::json!({
                "id": "PasteSub", "type": "menu", "label": "Paste Special",
                "order": 3, "parent": "PasteMenu"
            })),
            control(serde_json::json!({
                "id": "PasteWord", "type": "button", "label": "Word",
                "order": 1, "parent": "PasteSub", "command": "edit.paste.word"
            })),
        ])])]))
        .expect("converts");
        let controls = &layout.tabs[0].groups[0].controls;
        assert_eq!(controls.len(), 1);
        let menu = &controls[0];
        assert_eq!(menu.kind, ItemKind::Split);
        assert_eq!(menu.items.len(), 2);
        assert_eq!(
            menu.items[0].id,
            "tab_home.group_clipboard.paste_menu.paste_text"
        );
        let sub = &menu.items[1];
        assert_eq!(sub.kind, ItemKind::Dropdown);
        assert_eq!(sub.items.len(), 1);
        assert_eq!(
            sub.items[0].id,
            "tab_home.group_clipboard.paste_menu.paste_sub.paste_word"
        );
    }

    #[test]
    fn a_parent_cycle_becomes_roots_without_hanging() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![
            control(serde_json::json!({
                "id": "A", "type": "button", "label": "A", "order": 0, "parent": "B"
            })),
            control(serde_json::json!({
                "id": "B", "type": "button", "label": "B", "order": 1, "parent": "A"
            })),
        ])])]))
        .expect("converts");
        let controls = &layout.tabs[0].groups[0].controls;
        assert_eq!(controls.len(), 2, "both items survive");
        assert!(controls.iter().all(|item| item.items.is_empty()));
    }

    #[test]
    fn a_chain_deeper_than_the_bound_becomes_roots() {
        let mut chain = Vec::new();
        for level in 0..MAX_TREE_DEPTH + 2 {
            chain.push(control(serde_json::json!({
                "id": format!("Level{level}"),
                "type": "button",
                "label": format!("Level {level}"),
                "order": level,
                "parent": if level == 0 { serde_json::Value::Null } else { format!("Level{}", level - 1).into() }
            })));
        }
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(chain)])])).expect("converts");
        let controls = &layout.tabs[0].groups[0].controls;
        // Every item is kept; the nesting is bounded.
        assert_eq!(count_items(controls), MAX_TREE_DEPTH + 2);
        assert!(max_depth(controls) <= MAX_TREE_DEPTH);
    }

    fn count_items(items: &[Item]) -> usize {
        items.iter().map(|item| 1 + count_items(&item.items)).sum()
    }

    fn max_depth(items: &[Item]) -> usize {
        items
            .iter()
            .map(|item| 1 + max_depth(&item.items))
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn every_control_type_maps_to_an_item_kind() {
        let cases = [
            ("button", ItemKind::Button),
            ("task", ItemKind::Button),
            ("control", ItemKind::Button),
            ("toggleButton", ItemKind::Toggle),
            ("checkBox", ItemKind::Check),
            ("splitButton", ItemKind::Split),
            ("gallery", ItemKind::Dropdown),
            ("menu", ItemKind::Dropdown),
            ("dropDown", ItemKind::Dropdown),
            ("comboBox", ItemKind::Combo),
            ("labelControl", ItemKind::Heading),
            ("buttonDialogBoxLauncher", ItemKind::Launcher),
            ("category", ItemKind::Heading),
            ("taskFormGroup", ItemKind::Heading),
            ("group", ItemKind::Heading),
            ("none", ItemKind::Heading),
        ];
        for (type_name, expected) in cases {
            let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![control(
                serde_json::json!({"id": "Item", "type": type_name, "label": "Item", "order": 0}),
            )])])]))
            .expect("converts");
            assert_eq!(
                layout.tabs[0].groups[0].controls[0].kind, expected,
                "for {type_name}"
            );
        }
    }

    #[test]
    fn a_structural_type_with_children_is_a_submenu() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![
            control(serde_json::json!({
                "id": "CategorySave", "type": "category", "label": "Save", "order": 0
            })),
            control(serde_json::json!({
                "id": "TaskSave", "type": "task", "label": "Save As",
                "order": 1, "parent": "CategorySave", "command": "file.save_as"
            })),
        ])])]))
        .expect("converts");
        assert_eq!(layout.tabs[0].groups[0].controls[0].kind, ItemKind::Submenu);
    }

    #[test]
    fn tab_sets_split_core_from_contextual_tabs() {
        let mut core = one_tab(vec![]);
        core.tab_set = Some("TabSetPrimary".to_owned());
        let mut contextual = one_tab(vec![]);
        contextual.id = crate::FluentId::new("TabPictureToolsFormat");
        contextual.label = "TabPictureToolsFormat".to_owned();
        contextual.tab_set = Some("TabSetPictureTools".to_owned());
        let mut ink = one_tab(vec![]);
        ink.id = crate::FluentId::new("TabInkToolsPens");
        ink.label = "TabInkToolsPens".to_owned();
        ink.tab_set = Some("TabSetInkTools".to_owned());
        let layout = from_v1(&v1_layout(vec![core, contextual, ink])).expect("converts");
        assert_eq!(layout.tabs.len(), 1, "one core tab");
        assert_eq!(layout.contextual_tabs.len(), 2, "picture and ink tools");
        let picture = &layout.contextual_tabs[0];
        assert_eq!(picture.tab_set.as_deref(), Some("Picture Tools"));
        assert_eq!(picture.shown_in, Some(ContextId::PictureSelected));
        let ink_tools = &layout.contextual_tabs[1];
        assert_eq!(ink_tools.tab_set.as_deref(), Some("Ink Tools"));
        assert_eq!(ink_tools.shown_in, None);
        // The core tabs carry no tab set and never show conditionally.
        assert!(layout.tabs.iter().all(|tab| tab.shown_in.is_none()));
    }

    #[test]
    fn contextual_tab_sets_without_a_context_stay_hidden() {
        let mut ink = one_tab(vec![]);
        ink.tab_set = Some("TabSetInkTools".to_owned());
        let layout = from_v1(&v1_layout(vec![ink])).expect("converts");
        let tab = &layout.contextual_tabs[0];
        assert_eq!(tab.tab_set.as_deref(), Some("Ink Tools"));
        assert_eq!(tab.shown_in, None);
    }

    #[test]
    fn labels_become_display_words_only_when_they_are_fluent_ids() {
        assert_eq!(display_label("TabHome", "TabHome"), "Home");
        assert_eq!(
            display_label("GroupClipboard", "GroupClipboard"),
            "Clipboard"
        );
        assert_eq!(
            display_label("ContextMenuDrawingPage", "ContextMenuDrawingPage"),
            "Drawing Page"
        );
        assert_eq!(display_label("Undo", "Undo"), "Undo");
        // A real label is kept.
        assert_eq!(display_label("Cut", "Cut X"), "Cut X");
        // No prefix is stripped when it leaves a lowercase rest.
        assert_eq!(display_label("Tabby", "Tabby"), "Tabby");
    }

    #[test]
    fn slugs_follow_the_item_id_pattern() {
        assert_eq!(slug("TabHome"), "tab_home");
        assert_eq!(slug("FileSaveQuickAccess"), "file_save_quick_access");
        assert_eq!(slug("2Fast"), "x_2_fast");
        assert_eq!(slug("A-B"), "a_b");
        assert_eq!(slug("!!!"), "item");
    }

    #[test]
    fn the_backstage_becomes_the_file_view() {
        let mut layout = v1_layout(vec![]);
        layout.backstage.tabs = vec![
            serde_json::from_value(serde_json::json!({
                "id": "TabSave", "label": "TabSave", "order": 1, "controls": [
                    {"id": "SaveAs", "type": "button", "label": "Save As", "order": 0,
                     "command": "file.save_as"}
                ]
            }))
            .expect("a backstage tab decodes"),
            serde_json::from_value(serde_json::json!({
                "id": "TabInfo", "label": "TabInfo", "order": 0, "controls": [
                    {"id": "Permissions", "type": "button", "label": "Permissions", "order": 0,
                     "command": "tools.permissions"},
                    {"id": "Version", "type": "button", "label": "Version", "order": 1,
                     "command": "file.version"}
                ]
            }))
            .expect("a backstage tab decodes"),
        ];
        let converted = from_v1(&layout).expect("converts");
        let view = converted.file_view.expect("a file view");
        // Entries are in `order`: TabInfo (0) first.
        assert_eq!(view.default_page, "file_view.tab_info");
        assert_eq!(view.nav.len(), 2);
        assert_eq!(view.nav[0].id, "file_view.tab_info");
        assert_eq!(view.nav[0].kind, FileViewEntryKind::Page);
        assert_eq!(
            view.nav[0].command.as_ref().map(|c| c.as_str()),
            Some("tools.permissions")
        );
        // One distinct command makes the entry a command entry.
        assert_eq!(view.nav[1].id, "file_view.tab_save");
        assert_eq!(view.nav[1].kind, FileViewEntryKind::Command);
        assert_eq!(
            view.nav[1].command.as_ref().map(|c| c.as_str()),
            Some("file.save_as")
        );
    }

    #[test]
    fn the_backstage_of_an_empty_layout_has_an_empty_file_view() {
        let converted = from_v1(&Layout::default()).expect("converts");
        let view = converted.file_view.expect("a file view");
        assert!(view.nav.is_empty());
        assert_eq!(view.default_page, "file_view");
    }

    #[test]
    fn context_menus_are_carried_over_with_their_items() {
        let mut layout = v1_layout(vec![]);
        layout.context_menus = vec![
            serde_json::from_value(serde_json::json!({
                "id": "ContextMenuDrawingPage", "label": "ContextMenuDrawingPage", "order": 0,
                "controls": [
                    {"id": "Paste", "type": "button", "label": "Paste", "order": 0,
                     "command": "edit.paste"}
                ]
            }))
            .expect("a context menu decodes"),
        ];
        let converted = from_v1(&layout).expect("converts");
        assert_eq!(converted.context_menus.len(), 1);
        let menu = &converted.context_menus[0];
        assert_eq!(menu.id, "context_menu_drawing_page");
        assert_eq!(menu.label.as_deref(), Some("Drawing Page"));
        assert_eq!(menu.items.len(), 1);
    }

    #[test]
    fn the_quick_access_toolbar_is_carried_over_under_the_qat_root() {
        let mut layout = v1_layout(vec![]);
        layout.quick_access_toolbar.controls = vec![control(serde_json::json!({
            "id": "FileSave", "type": "button", "label": "Save", "order": 0,
            "command": "file.save"
        }))];
        let converted = from_v1(&layout).expect("converts");
        assert_eq!(converted.quick_access_toolbar.len(), 1);
        assert_eq!(converted.quick_access_toolbar[0].id, "qat.file_save");
    }

    #[test]
    fn items_carry_their_fluent_id_and_command_as_the_icon_key() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![control(
            serde_json::json!({
                "id": "Cut", "type": "button", "label": "Cut", "order": 0,
                "command": "edit.cut"
            }),
        )])])]))
        .expect("converts");
        let item = &layout.tabs[0].groups[0].controls[0];
        assert_eq!(item.fluent.as_ref().map(|f| f.as_str()), Some("Cut"));
        assert_eq!(item.command.as_ref().map(|c| c.as_str()), Some("edit.cut"));
        assert_eq!(item.icon.as_deref(), Some("edit.cut"));
        assert!(!item.new_command);
        // No slot data: the group stacks in file order (ADR 0061).
        assert_eq!(item.column, None);
        assert_eq!(item.row, None);
        assert_eq!(item.size, None);
    }

    #[test]
    fn groups_and_tabs_keep_their_fluent_ids() {
        let layout = from_v1(&v1_layout(vec![one_tab(vec![one_group(vec![])])])).expect("converts");
        let tab = &layout.tabs[0];
        assert_eq!(tab.fluent.as_ref().map(|f| f.as_str()), Some("TabHome"));
        assert_eq!(
            tab.groups[0].fluent.as_ref().map(|f| f.as_str()),
            Some("GroupClipboard")
        );
    }
}
