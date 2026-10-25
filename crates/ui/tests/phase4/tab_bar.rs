//! Placement completeness (task P4-09, ADR 0031 and ADR 0037).
//!
//! ADR 0037 item 1: for each layout, the rendered surfaces contain
//! one widget per placement (tabs, groups, controls, backstage tabs,
//! context menus, QAT), by accessible name. P4-04 pins the *exact*
//! widget sets per command-bar tab, backstage entry and QAT and the
//! per-layout counts (`layout_counts` snapshot) inside `crates/ui`;
//! this module walks the same placements through the real builtin
//! registry and the public `TabBar` API (presence per placement),
//! checks the one surface P4-04 did not pin on committed data (the
//! context menus, which still carry no controls: the P4-03 note), and
//! pins the raw placement counts and unique-id counts of the
//! committed `ui-spec` data (the repeats, e.g. `TabSave` x9, show up
//! as a gap between the two numbers).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use egui_kittest::Harness;
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_ui::tab_bar::TabBar;
use larrygram_ui_spec::{Control, ControlType, FluentId, LayoutKind, LayoutSet};

use crate::{names, sets};

/// The dropdown arrow of a split button (the `DROPDOWN_TIP` of
/// `crates/ui/src/tab_bar.rs`; the string is the accessible name the
/// tests pin).
const DROPDOWN_TIP: &str = "More items";

/// Which `show` method a [`Rig`] runs each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// [`TabBar::show`].
    CommandBar,
    /// [`TabBar::show_quick_access`].
    QuickAccess,
    /// [`TabBar::show_backstage`].
    Backstage,
    /// [`TabBar::show_context_menu`].
    ContextMenu,
}

/// A command bar driven headlessly with the real builtin registry.
struct Rig {
    tab_bar: Rc<RefCell<TabBar>>,
    harness: Harness<'static>,
}

impl Rig {
    fn build(kind: LayoutKind, set: &LayoutSet, surface: Surface) -> Self {
        let tab_bar = Rc::new(RefCell::new(TabBar::new(kind)));
        let set = Rc::new(set.clone());
        let registry = larrygram_commands::builtins::registry().expect("the registry builds");
        let app = AppCtx::new(Settings::default());
        let harness = Harness::builder()
            .with_size(egui::vec2(12000.0, 4000.0))
            .build_ui({
                let tab_bar = Rc::clone(&tab_bar);
                let set = Rc::clone(&set);
                move |ui| {
                    let mut tab_bar = tab_bar.borrow_mut();
                    let produced = match surface {
                        Surface::CommandBar => tab_bar.show(ui, &set, &registry, &app),
                        Surface::QuickAccess => {
                            tab_bar.show_quick_access(ui, &set, &registry, &app)
                        }
                        Surface::Backstage => tab_bar.show_backstage(ui, &set, &registry, &app),
                        Surface::ContextMenu => {
                            tab_bar.show_context_menu(ui, &set, &registry, &app)
                        }
                    }
                    .expect("the surface renders");
                    let _ = produced;
                }
            });
        let mut rig = Self { tab_bar, harness };
        rig.run();
        rig
    }

    /// One frame.
    fn run(&mut self) {
        self.harness.run();
    }

    /// Mutates the command bar state between frames (active tab, the open
    /// context menu).
    fn tab_bar(&self) -> std::cell::RefMut<'_, TabBar> {
        self.tab_bar.borrow_mut()
    }

    /// The widget names of the current frame, as a multiset.
    fn names(&self) -> BTreeMap<String, usize> {
        names(&self.harness)
    }
}

// --- the expected widget names (ADR 0031 rules, test side) -----------

/// The kept controls of one container: the first occurrence of every
/// (id, label, order) triple (the ADR 0031 display-dedup rule).
fn expected_kept(controls: &[Control]) -> Vec<&Control> {
    let mut seen: BTreeSet<(&str, &str, u32)> = BTreeSet::new();
    controls
        .iter()
        .filter(|c| seen.insert((c.id.as_str(), c.label.as_str(), c.order)))
        .collect()
}

/// One container's raw placements: the roots and the controls nested
/// in a sibling, repeats included (no dedup). This is what the data
/// holds; the display-dedup rule of ADR 0031 hides only exact
/// (id, label, order) repeats.
fn raw_split(controls: &[Control]) -> (usize, usize) {
    let ids: BTreeSet<&str> = controls.iter().map(|c| c.id.as_str()).collect();
    let nested = controls
        .iter()
        .filter(|c| {
            c.parent
                .as_ref()
                .is_some_and(|parent| ids.contains(parent.as_str()))
        })
        .count();
    (controls.len() - nested, nested)
}

/// The unique Fluent ids of one container's controls.
fn unique_ids(controls: &[Control]) -> usize {
    controls
        .iter()
        .map(|c| c.id.as_str())
        .collect::<BTreeSet<_>>()
        .len()
}

/// True for controls whose nested controls are dropdown items.
fn is_dropdown(r#type: ControlType) -> bool {
    matches!(
        r#type,
        ControlType::Gallery
            | ControlType::Menu
            | ControlType::DropDown
            | ControlType::ComboBox
            | ControlType::SplitButton
    )
}

/// The nested controls of `id`, in list order.
fn children_of<'c>(kept: &[&'c Control], id: &str) -> Vec<&'c Control> {
    kept.iter()
        .copied()
        .filter(|c| {
            c.parent
                .as_ref()
                .is_some_and(|parent| parent.as_str() == id)
        })
        .collect()
}

/// True when another control of the same container is this one's
/// parent (so the control renders nested, not as a root).
fn is_nested(kept: &[&Control], control: &Control) -> bool {
    control.parent.as_ref().is_some_and(|parent| {
        kept.iter()
            .any(|other| other.id.as_str() == parent.as_str())
    })
}

/// The widget names one control renders by itself: a split button
/// with items is a button plus a dropdown arrow (ADR 0031).
fn widget_names(control: &Control, children: usize) -> Vec<String> {
    if control.r#type == ControlType::SplitButton && children > 0 {
        vec![control.label.clone(), DROPDOWN_TIP.to_owned()]
    } else {
        vec![control.label.clone()]
    }
}

/// The widget names one control list renders (the dropdown items of
/// closed dropdowns stay hidden).
fn expected_names(controls: &[Control]) -> Vec<String> {
    let kept = expected_kept(controls);
    let roots: Vec<&Control> = kept
        .iter()
        .copied()
        .filter(|c| !is_nested(&kept, c))
        .collect();
    let mut out = Vec::new();
    let mut seen: BTreeSet<(&str, &str, u32)> = BTreeSet::new();
    expected_walk(&kept, &roots, &mut out, &mut seen, true);
    // Controls in a parent cycle render as roots too.
    let leftover: Vec<&Control> = kept
        .iter()
        .copied()
        .filter(|c| !seen.contains(&(c.id.as_str(), c.label.as_str(), c.order)))
        .collect();
    expected_walk(&kept, &leftover, &mut out, &mut seen, true);
    out
}

/// Walks `items`, each kept control counted once.
fn expected_walk<'c>(
    kept: &[&'c Control],
    items: &[&'c Control],
    out: &mut Vec<String>,
    seen: &mut BTreeSet<(&'c str, &'c str, u32)>,
    visible: bool,
) {
    for control in items {
        if !seen.insert((control.id.as_str(), control.label.as_str(), control.order)) {
            continue;
        }
        let children = children_of(kept, control.id.as_str());
        if visible {
            out.extend(widget_names(control, children.len()));
        }
        // A closed dropdown hides its items; the nested controls of
        // every other control render with it.
        let kids_visible = visible && !is_dropdown(control.r#type);
        expected_walk(kept, &children, out, seen, kids_visible);
    }
}

/// A name multiset of the given names.
fn counts_of(names: &[String]) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name.clone()).or_default() += 1;
    }
    counts
}

/// A synthetic context-menu control.
fn menu_control(
    id: &str,
    label: &str,
    r#type: ControlType,
    command: Option<&str>,
    parent: Option<&str>,
    order: u32,
) -> Control {
    Control {
        id: FluentId::new(id),
        r#type,
        label: label.to_owned(),
        order,
        parent: parent.map(FluentId::new),
        command: command.map(larrygram_ui_spec::CommandId::new),
        aliases: Vec::new(),
        bounds: None,
        icon_ref: None,
        keytip: None,
        size_variant: None,
        visual_notes: None,
    }
}

// --- the tests -------------------------------------------------------

/// ADR 0037: the rendered command bar contains one widget per placement,
/// by accessible name, for both layouts and every surface.
///
/// Exact widget sets per command-bar tab, backstage entry and QAT are
/// pinned by P4-04's
/// `both_layouts_render_one_widget_per_placement`; this walk covers
/// the same placements through the real registry (plus the context
/// menus) and asserts presence, so it complements rather than
/// duplicates that test.
#[test]
fn every_placement_of_both_layouts_renders_by_accessible_name() {
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let set = sets().get(kind).clone();

        // Every command-bar tab: the tab labels, the group labels and the
        // placements of the active tab's groups.
        let mut rig = Rig::build(kind, &set, Surface::CommandBar);
        for (index, tab) in set.layout.tabs.iter().enumerate() {
            rig.tab_bar().set_active_tab(index);
            rig.run();
            let names = rig.names();
            for label in tab_labels(&set) {
                assert!(names.contains_key(&label), "{kind:?} tab {label:?} renders");
            }
            for group in &tab.groups {
                assert!(
                    names.contains_key(&group.label),
                    "{kind:?} group {:?} renders",
                    group.label
                );
                for name in expected_names(&group.controls) {
                    assert!(
                        names.contains_key(&name),
                        "{kind:?} placement {name:?} of {} renders",
                        tab.label
                    );
                }
            }
        }

        // The quick access toolbar.
        let rig = Rig::build(kind, &set, Surface::QuickAccess);
        let names = rig.names();
        for name in expected_names(&set.layout.quick_access_toolbar.controls) {
            assert!(names.contains_key(&name), "{kind:?} QAT placement {name:?}");
        }

        // Every backstage entry.
        let mut rig = Rig::build(kind, &set, Surface::Backstage);
        rig.tab_bar().set_backstage_open(true);
        rig.run();
        for (index, tab) in set.layout.backstage.tabs.iter().enumerate() {
            rig.tab_bar().set_active_backstage_tab(index);
            rig.run();
            let names = rig.names();
            for entry in &set.layout.backstage.tabs {
                assert!(
                    names.contains_key(&entry.label),
                    "{kind:?} entry {:?}",
                    entry.label
                );
            }
            for name in expected_names(&tab.controls) {
                assert!(
                    names.contains_key(&name),
                    "{kind:?} backstage placement {name:?} of {}",
                    tab.label
                );
            }
        }

        // Every context menu.
        for menu in &set.layout.context_menus {
            let mut rig = Rig::build(kind, &set, Surface::ContextMenu);
            rig.tab_bar().set_context_menu(Some(menu.id.clone()));
            rig.run();
            let names = rig.names();
            for name in expected_names(&menu.controls) {
                assert!(
                    names.contains_key(&name),
                    "{kind:?} context menu placement {name:?} of {}",
                    menu.id.as_str()
                );
            }
        }
    }
}

/// A context menu renders exactly one widget per placement.
///
/// The committed context menus carry no controls yet (the P4-03
/// note), so the exactness runs on a synthetic menu (the P4-04 rules,
/// through the real registry) and the committed menus are walked too:
/// each must render exactly its placements, which today is nothing.
#[test]
fn a_context_menu_renders_exactly_its_placements() {
    // A synthetic menu: a button, a split button with items, a
    // dropdown with items and a nested item.
    let menu_controls = vec![
        menu_control(
            "MenuCopy",
            "Copy",
            ControlType::Button,
            Some("edit.copy"),
            None,
            0,
        ),
        menu_control(
            "MenuSplit",
            "Split",
            ControlType::SplitButton,
            Some("file.new"),
            None,
            1,
        ),
        menu_control(
            "MenuSplitItem",
            "Split item",
            ControlType::Button,
            Some("file.new"),
            Some("MenuSplit"),
            2,
        ),
        menu_control("MenuList", "List", ControlType::Gallery, None, None, 3),
        menu_control(
            "MenuListItem",
            "List item",
            ControlType::Button,
            None,
            Some("MenuList"),
            4,
        ),
    ];
    let mut set = LayoutSet::default();
    set.layout
        .context_menus
        .push(larrygram_ui_spec::ContextMenu {
            id: FluentId::new("TestMenu"),
            label: "TestMenu".to_owned(),
            order: 0,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            controls: menu_controls,
        });

    let mut rig = Rig::build(LayoutKind::Panel2016, &set, Surface::ContextMenu);
    rig.tab_bar()
        .set_context_menu(Some(FluentId::new("TestMenu")));
    rig.run();
    assert_eq!(
        rig.names(),
        counts_of(&expected_names(&set.layout.context_menus[0].controls)),
        "a context menu renders one widget per placement"
    );

    // The committed menus (currently empty, P4-03): exactly their
    // placements, nothing else.
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let set = sets().get(kind).clone();
        assert!(
            !set.layout.context_menus.is_empty(),
            "{kind:?} has context menus to walk"
        );
        for menu in &set.layout.context_menus {
            let mut rig = Rig::build(kind, &set, Surface::ContextMenu);
            rig.tab_bar().set_context_menu(Some(menu.id.clone()));
            rig.run();
            assert_eq!(
                rig.names(),
                counts_of(&expected_names(&menu.controls)),
                "{kind:?} context menu {} renders one widget per placement",
                menu.id.as_str()
            );
        }
    }
}

/// The raw placement counts of the committed `ui-spec` data, pinned
/// (ADR 0037: an extractor change that adds a control shows up as a
/// snapshot diff).
///
/// These are the *raw* rows (repeats included, no ADR 0031 dedup):
/// the data-side view. P4-04's `layout_counts` snapshot pins the
/// rendered-side counts (kept, roots, nested) of the same layouts.
#[test]
fn the_raw_placement_counts_are_pinned() {
    let line = |label: &str, value: usize| format!("{label}: {value}");
    let mut report = String::new();
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let layout = &sets().get(kind).layout;
        let mut tab_bar_controls = 0usize;
        let mut tab_bar_nested = 0usize;
        let mut tab_bar_ids: BTreeSet<&str> = BTreeSet::new();
        for tab in &layout.tabs {
            for group in &tab.groups {
                let (roots, items) = raw_split(&group.controls);
                tab_bar_controls += roots;
                tab_bar_nested += items;
                tab_bar_ids.extend(group.controls.iter().map(|c| c.id.as_str()));
            }
        }
        let mut backstage_controls = 0usize;
        let mut backstage_nested = 0usize;
        let mut backstage_ids: BTreeSet<&str> = BTreeSet::new();
        for tab in &layout.backstage.tabs {
            let (roots, items) = raw_split(&tab.controls);
            backstage_controls += roots;
            backstage_nested += items;
            backstage_ids.extend(tab.controls.iter().map(|c| c.id.as_str()));
        }
        let mut menu_controls = 0usize;
        let mut menu_nested = 0usize;
        let mut menu_ids: BTreeSet<&str> = BTreeSet::new();
        for menu in &layout.context_menus {
            let (roots, items) = raw_split(&menu.controls);
            menu_controls += roots;
            menu_nested += items;
            menu_ids.extend(menu.controls.iter().map(|c| c.id.as_str()));
        }
        let (qat_controls, qat_nested) = raw_split(&layout.quick_access_toolbar.controls);
        report.push_str(&format!("layout: {kind:?}\n"));
        for (label, value) in [
            ("command bar tabs", layout.tabs.len()),
            (
                "command bar groups",
                layout.tabs.iter().map(|t| t.groups.len()).sum(),
            ),
            ("command bar controls (raw roots)", tab_bar_controls),
            ("command bar controls (raw nested)", tab_bar_nested),
            ("command bar controls (unique ids)", tab_bar_ids.len()),
            ("backstage tabs", layout.backstage.tabs.len()),
            ("backstage controls (raw roots)", backstage_controls),
            ("backstage controls (raw nested)", backstage_nested),
            ("backstage controls (unique ids)", backstage_ids.len()),
            ("context menus", layout.context_menus.len()),
            ("context menu controls (raw roots)", menu_controls),
            ("context menu controls (raw nested)", menu_nested),
            ("context menu controls (unique ids)", menu_ids.len()),
            ("quick access controls (raw roots)", qat_controls),
            ("quick access controls (raw nested)", qat_nested),
            (
                "quick access controls (unique ids)",
                unique_ids(&layout.quick_access_toolbar.controls),
            ),
        ] {
            report.push_str(&line(label, value));
            report.push('\n');
        }
    }
    insta::assert_snapshot!("phase4_placement_counts", report);
}

/// The tab labels of one layout.
fn tab_labels(set: &LayoutSet) -> Vec<String> {
    set.layout.tabs.iter().map(|t| t.label.clone()).collect()
}
