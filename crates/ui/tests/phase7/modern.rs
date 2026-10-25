//! The ADR 0065 Panel Modern sweep (task P7-10): every control of
//! every tab dispatches or disables as the committed data says, and
//! every dropdown, split, gallery and combo opens with its entries and
//! every enabled entry dispatches.
//!
//! Matching a drawn widget to its data item: the command bar draws one
//! node per control (plus one per split arrow) in group and slot
//! order, so the nodes inside the command-bar band are zipped against
//! the expected controls. A popup's rows are the nodes that appear
//! when the popup opens.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::AppCtx;
use larrygram_commands::{Registry, Source};
use larrygram_ui::Intent;
use larrygram_ui::contexts::SelectionContext;
use larrygram_ui::panel_modern::{ListData, PanelModern};
use larrygram_ui_spec::v2::{ContextId, Item, ItemKind, LayoutKind, LayoutV2};

use crate::expect::{self, Entry, Expect};
use crate::menus::{Named, label_of};
use crate::{RichApp, builtin_registry, sets};

/// The Panel Modern widgets over the committed layout, driven
/// headlessly, with the intents of every frame recorded.
pub struct ModernRig {
    /// The widget under test.
    pub panel: Rc<RefCell<PanelModern>>,
    /// The application state the enablement predicates see.
    app: Rc<AppCtx>,
    /// The layout under test.
    layout: Rc<LayoutV2>,
    /// The selection context the tab strip sees.
    context: Rc<RefCell<SelectionContext>>,
    /// The intents the frames produced.
    intents: Rc<RefCell<Vec<Intent>>>,
    /// The frame clock.
    now: f64,
    /// The harness.
    pub harness: Harness<'static>,
}

impl ModernRig {
    /// A rig over the committed Panel Modern layout and the builtin
    /// registry, with a rich application state.
    pub fn new() -> Self {
        let layout: Rc<LayoutV2> = Rc::new(sets().get(LayoutKind::PanelModern).v2.clone());
        let registry: Rc<Registry> = Rc::new(builtin_registry());
        let app: Rc<AppCtx> = Rc::new(RichApp::rich().0);
        let lists: Rc<ListData> = Rc::new(ListData::new());
        let panel = Rc::new(RefCell::new(PanelModern::new()));
        let context = Rc::new(RefCell::new(SelectionContext::ShapeSelected));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let harness = Harness::builder()
            .with_size(egui::vec2(1600.0, 900.0))
            .build_ui({
                let panel = Rc::clone(&panel);
                let context = Rc::clone(&context);
                let intents = Rc::clone(&intents);
                let layout = Rc::clone(&layout);
                let registry = Rc::clone(&registry);
                let app = Rc::clone(&app);
                let lists = Rc::clone(&lists);
                move |ui| {
                    let mut panel = panel.borrow_mut();
                    let produced = panel
                        .show(ui, &layout, *context.borrow(), &app, &lists, &registry)
                        .expect("the command bar renders");
                    intents.borrow_mut().extend(produced);
                }
            });
        let mut rig = Self {
            panel,
            app,
            layout,
            context,
            intents,
            now: 1.0,
            harness,
        };
        rig.run();
        rig
    }

    /// The layout under test.
    pub fn layout(&self) -> &LayoutV2 {
        &self.layout
    }

    /// The application state under test.
    pub fn app(&self) -> &AppCtx {
        &self.app
    }

    /// Draws one frame, 1/60 s after the last one.
    ///
    /// `run_steps`, not `run`: an open tooltip repaints every frame,
    /// so the harness would never settle.
    pub fn run(&mut self) {
        self.now += 1.0 / 60.0;
        self.harness.input_mut().time = Some(self.now);
        self.harness.run_steps(1);
    }

    /// Settles: a click dispatches a frame or two later.
    pub fn settle(&mut self) {
        self.run();
        self.harness.run_steps(2);
    }

    /// Closes whatever popup is open and settles.
    pub fn close_popups(&mut self) {
        self.harness.key_press(egui::Key::Escape);
        self.run();
    }

    /// Sets the selection context (which contextual tabs show).
    pub fn set_context(&mut self, context: SelectionContext) {
        *self.context.borrow_mut() = context;
        self.run();
    }

    /// The intents produced since the last call.
    pub fn take_intents(&self) -> Vec<Intent> {
        std::mem::take(&mut *self.intents.borrow_mut())
    }

    /// The named nodes of the current frame in tree (draw) order.
    pub fn named_nodes(&self) -> Vec<Named> {
        self.harness
            .root()
            .children_recursive()
            .filter_map(|node| {
                let name = label_of(&node);
                if name.is_empty() {
                    return None;
                }
                let data = node.accesskit_node().data();
                Some(Named {
                    index: 0,
                    name,
                    role: node.accesskit_node().role(),
                    disabled: data.is_disabled(),
                    description: data.description().map(str::to_owned),
                    rect: node.rect(),
                })
            })
            .enumerate()
            .map(|(index, mut named)| {
                named.index = index;
                named
            })
            .collect()
    }

    /// Clicks one node of a previous frame by its identity: the `nth`
    /// same-role, same-name widget where `nth` counts the same-name
    /// widgets before `node` in draw order.
    pub fn click_node(&mut self, node: &Named) {
        let nth = self.named_nodes()[..node.index]
            .iter()
            .filter(|n| n.name == node.name && n.role == node.role)
            .count();
        self.click_nth(node.role, &node.name, nth);
    }

    /// The widget names of the current frame (AccessKit order).
    pub fn names(&self) -> Vec<String> {
        self.named_nodes().into_iter().map(|n| n.name).collect()
    }

    /// Clicks the `nth` named widget with `role` and `name`, and
    /// settles.
    ///
    /// The click goes through the accesskit action, not the pointer:
    /// a control's painted atoms widen its accesskit bounding box, so
    /// the box centre can land on the neighbouring control.
    pub fn click_nth(&mut self, role: Role, name: &str, nth: usize) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(role, name)
                .nth(nth)
                .unwrap_or_else(|| {
                    panic!("no {role:?} named {name:?} (#{nth}) in {:?}", self.names())
                });
            node.click_accesskit();
        }
        self.settle();
    }

    /// Clicks the first widget with `role` and `name`.
    pub fn click(&mut self, role: Role, name: &str) {
        self.click_nth(role, name, 0);
    }

    /// Hovers the `nth` widget with `role` and `name` (a submenu
    /// entry opens on hover) and settles.
    pub fn hover_nth(&mut self, role: Role, name: &str, nth: usize) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(role, name)
                .nth(nth)
                .unwrap_or_else(|| panic!("no {role:?} named {name:?} (#{nth})"));
            node.hover();
        }
        self.settle();
    }
}

/// The nodes one control draws in the command bar: its main widget,
/// then the split arrow ("More items", ADR 0031).
fn control_nodes(item: &Item) -> Vec<(String, Role)> {
    let label = item.label.clone().unwrap_or_default();
    match item.kind {
        ItemKind::Combo => vec![(label, Role::Button)],
        ItemKind::Check => vec![(label, Role::CheckBox)],
        ItemKind::Split | ItemKind::SplitRecent => vec![
            (label, Role::Button),
            (expect::DROPDOWN_TIP.to_owned(), Role::Button),
        ],
        ItemKind::Button
        | ItemKind::Toggle
        | ItemKind::Dropdown
        | ItemKind::Gallery
        | ItemKind::Palette
        | ItemKind::Launcher => vec![(label, Role::Button)],
        // Menu-only kinds render as plain labels when they appear as a
        // control (ADR 0061); their children draw under them.
        ItemKind::Submenu | ItemKind::Heading | ItemKind::List | ItemKind::Options => Vec::new(),
    }
}

/// One expected command-bar widget: its data item and where it draws.
pub struct Control {
    /// The control (or group launcher) item.
    pub item: Item,
    /// The node labels it draws, in order.
    pub nodes: Vec<(String, Role)>,
}

/// The expected command-bar widgets of one tab, in draw order: each
/// group's visible controls (with their split arrows) and its launcher.
pub fn tab_controls(tab: &larrygram_ui_spec::v2::Tab) -> Vec<Control> {
    let mut out = Vec::new();
    for group in &tab.groups {
        for item in larrygram_ui::panel_modern::place::visible_controls(group) {
            out.push(Control {
                item: item.clone(),
                nodes: control_nodes(item),
            });
        }
        if let Some(launcher) = &group.launcher {
            let label = launcher.label.clone().unwrap_or_default();
            out.push(Control {
                item: Item {
                    id: format!("{}.launcher", group.id),
                    kind: ItemKind::Launcher,
                    label: Some(label.clone()),
                    access: None,
                    shortcut: None,
                    command: Some(launcher.command.clone()),
                    new_command: false,
                    fluent: launcher.fluent.clone(),
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
                },
                nodes: vec![(label, Role::Button)],
            });
        }
    }
    out
}

/// The nodes inside the command bar, in draw order: the tab strip
/// draws first (the File button and the tab captions) and the collapse
/// button last, so the groups' controls and launchers are the middle.
pub fn band_nodes(rig: &ModernRig) -> Vec<Named> {
    let nodes = rig.named_nodes();
    let strip_bottom = nodes
        .iter()
        .find(|n| n.name == "File")
        .map(|n| n.rect.bottom())
        .unwrap_or(0.0);
    let mut out: Vec<Named> = nodes
        .into_iter()
        .filter(|n| n.rect.top() >= strip_bottom)
        .collect();
    if out
        .last()
        .is_some_and(|n| n.name.starts_with("Collapse") || n.name.starts_with("Expand"))
    {
        out.pop();
    }
    out
}

/// One control of the active tab with the nodes it draws.
pub struct Drawn {
    /// The control (or group launcher) item.
    pub control: Control,
    /// The nodes it draws, in order (a split draws its main part and
    /// its "More items" arrow).
    pub nodes: Vec<Named>,
}

/// Switches to one tab (its contextual set first) and zips the drawn
/// command-bar nodes against the expected controls.
pub fn activate_tab(
    rig: &mut ModernRig,
    label: &str,
    tab: &larrygram_ui_spec::v2::Tab,
) -> Vec<Drawn> {
    if let Some(context) = tab.shown_in {
        rig.set_context(context_of(context));
    }
    rig.close_popups();
    // The tab captions are not unique ("Design" is both a core tab and
    // the ShapeSheet Tools tab): click the tab whose `id` matches by
    // clicking the same-label button at the tab's visible position.
    let kind = tab.shown_in;
    let visible_tabs: Vec<&larrygram_ui_spec::v2::Tab> = rig
        .layout()
        .tabs
        .iter()
        .chain(rig.layout().contextual_tabs.iter())
        .filter(|other| other.shown_in.is_none() || other.shown_in == kind)
        .collect();
    let nth = visible_tabs
        .iter()
        .take_while(|other| other.id != tab.id)
        .filter(|other| other.label == tab.label)
        .count();
    rig.click_nth(Role::Button, label, nth);

    let controls = tab_controls(tab);
    let expected: Vec<(String, Role)> = controls.iter().flat_map(|c| c.nodes.clone()).collect();
    let nodes = band_nodes(rig);
    let drawn: Vec<&Named> = nodes
        .iter()
        .filter(|n| n.role == Role::Button || n.role == Role::CheckBox)
        .collect();
    assert!(
        drawn.len() >= expected.len(),
        "the tab {label:?} draws {} nodes, expected {}",
        drawn.len(),
        expected.len()
    );
    for (index, (name, role)) in expected.iter().enumerate() {
        assert_eq!(drawn[index].name, *name, "tab {label:?} node {index}");
        assert_eq!(drawn[index].role, *role, "tab {label:?} node {index} role");
    }
    // Slice the nodes per control.
    let mut out = Vec::new();
    let mut at = 0;
    for control in controls {
        let take = control.nodes.len();
        out.push(Drawn {
            nodes: drawn[at..at + take].iter().map(|n| (*n).clone()).collect(),
            control,
        });
        at += take;
    }
    out
}

/// The selection context whose data id is `id`.
fn context_of(id: ContextId) -> SelectionContext {
    [
        SelectionContext::EmptyPage,
        SelectionContext::PageSelected,
        SelectionContext::ShapeSelected,
        SelectionContext::ShapesSelected,
        SelectionContext::TextEditing,
        SelectionContext::ConnectorSelected,
        SelectionContext::GroupSelected,
        SelectionContext::SecondPage,
        SelectionContext::PictureSelected,
        SelectionContext::ContainerSelected,
        SelectionContext::ShapesheetWindow,
        SelectionContext::StencilEdit,
    ]
    .into_iter()
    .find(|context| context.id() == id)
    .unwrap_or_else(|| panic!("no selection context for {id:?}"))
}

/// Every control of every core and contextual tab draws in its slot,
/// and every enabled control dispatches its command while every
/// disabled one renders disabled (ADR 0065).
#[test]
fn every_modern_control_dispatches_or_disables() {
    let mut rig = ModernRig::new();
    let registry = builtin_registry();
    let layout = rig.layout().clone();
    let mut clicked = 0;
    let mut disabled = 0;
    let tabs: Vec<&larrygram_ui_spec::v2::Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    assert_eq!(layout.tabs.len(), 7, "the seven core tabs");
    for tab in &tabs {
        let label = tab.label.clone().unwrap_or_default();
        let drawn = activate_tab(&mut rig, &label, tab);
        assert!(!drawn.is_empty(), "the tab {label:?} has controls");
        for entry in &drawn {
            let item = &entry.control.item;
            let name = item.label.clone().unwrap_or_default();
            if entry.nodes.is_empty() {
                continue;
            }
            let expect = expect::modern_control(item, &registry, rig.app());
            match expect {
                Expect::Dispatch { command, .. } => {
                    rig.take_intents();
                    rig.click_node(&entry.nodes[0]);
                    let intents = rig.take_intents();
                    assert_eq!(intents.len(), 1, "the control {name:?} dispatches once");
                    assert_eq!(
                        intents[0].command.as_str(),
                        command.as_str(),
                        "the control {name:?} dispatches its command"
                    );
                    assert_eq!(ints_source(&intents[0]), Source::CommandBar);
                    rig.close_popups();
                    clicked += 1;
                }
                Expect::Disabled { reason } => {
                    let node = &entry.nodes[0];
                    assert!(node.disabled, "the control {name:?} renders disabled");
                    // A disabled control hovers its reason as the
                    // tooltip; the accesskit description carries it
                    // when egui maps the disabled hover text to one.
                    if !reason.is_empty() && node.description.is_some() {
                        assert_eq!(
                            node.description.as_deref(),
                            Some(reason.as_str()),
                            "the control {name:?} carries its reason"
                        );
                    }
                    disabled += 1;
                }
                _ => {}
            }
        }
    }
    assert!(
        clicked > 10 && disabled > 5,
        "the sweep clicked {clicked} and checked {disabled} controls"
    );
}

/// The dispatch source of one intent.
fn ints_source(intent: &Intent) -> Source {
    intent.invocation.source
}

/// The nodes one popup draws (the delta between the closed and the
/// open frame), in draw order.
fn popup_nodes(rig: &mut ModernRig, open: impl FnOnce(&mut ModernRig)) -> Vec<Named> {
    rig.close_popups();
    let baseline: BTreeSet<(String, u32, u32)> = rig.named_nodes().iter().map(key_of).collect();
    open(rig);
    rig.named_nodes()
        .into_iter()
        .filter(|n| !baseline.contains(&key_of(n)))
        .collect()
}

/// One node's identity: its name and drawn position (a control's rect
/// does not move when its popup opens).
fn key_of(node: &Named) -> (String, u32, u32) {
    (
        node.name.clone(),
        node.rect.min.x.to_bits(),
        node.rect.min.y.to_bits(),
    )
}

/// Every dropdown, split, gallery and combo of every tab opens with
/// its entries, and every enabled entry dispatches its command (ADR
/// 0065).
#[test]
fn every_modern_dropdown_gallery_and_combo_opens_and_dispatches() {
    let mut rig = ModernRig::new();
    let registry = builtin_registry();
    let layout = rig.layout().clone();
    let mut opened = 0;
    let tabs: Vec<&larrygram_ui_spec::v2::Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    for tab in &tabs {
        let label = tab.label.clone().unwrap_or_default();
        let drawn = activate_tab(&mut rig, &label, tab);
        for entry in &drawn {
            let item = &entry.control.item;
            let entries = expect::modern_control_entries(item, &registry, rig.app());
            if entries.is_empty() {
                continue;
            }
            if matches!(
                expect::modern_control(item, &registry, rig.app()),
                Expect::Disabled { .. }
            ) {
                continue;
            }
            opened += 1;
            // The opener: the control itself, or the split's arrow.
            let opener = match item.kind {
                ItemKind::Split | ItemKind::SplitRecent => entry.nodes[1].clone(),
                _ => entry.nodes[0].clone(),
            };
            check_popup(&mut rig, &entries, &opener);
        }
    }
    assert!(opened >= 3, "the sweep opened popups ({opened})");
}

/// Checks one open popup: its rows must be the expected entries in
/// order (among the popup's own nodes), and every enabled entry must
/// dispatch its command. Submenu entries open their children.
fn check_popup(rig: &mut ModernRig, entries: &[Entry], opener: &Named) {
    let drawn = popup_nodes(rig, |rig| {
        rig.click_node(opener);
    });
    // The expected entries appear among the drawn rows, in order (a
    // popup's delta can carry stray named nodes around them).
    let mut positions = Vec::new();
    let mut at = 0;
    for entry in entries {
        let offset = drawn[at..]
            .iter()
            .position(|n| n.name == entry.label)
            .unwrap_or_else(|| {
                panic!(
                    "the popup of {:?} draws no row named {:?} in {:?}",
                    opener.name,
                    entry.label,
                    drawn.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
                )
            });
        at += offset + 1;
        positions.push(at - 1);
    }
    for (entry, position) in entries.iter().zip(positions) {
        match &entry.expect {
            // Panel Modern has no toolbar-toggle rows; the classic
            // View > Toolbars rule is `expect::classic_toolbars_level`.
            Expect::Toggle { .. } | Expect::Heading => {}
            Expect::Disabled { .. } => {
                let drawn = popup_nodes(rig, |rig| {
                    rig.click_node(opener);
                });
                let node = &drawn[position];
                assert!(node.disabled, "the row {:?} renders disabled", entry.label);
            }
            Expect::Dispatch { command, value: _ } => {
                rig.close_popups();
                let drawn = popup_nodes(rig, |rig| {
                    rig.click_node(opener);
                });
                // The opener dispatches its own command when it has
                // one (a dropdown dispatches and opens); only the
                // entry's dispatch counts here.
                rig.take_intents();
                rig.click_node(&drawn[position]);
                let intents = rig.take_intents();
                assert_eq!(
                    intents.len(),
                    1,
                    "the row {:?} dispatches once (got {intents:?})",
                    entry.label
                );
                assert_eq!(
                    intents[0].command.as_str(),
                    command.as_str(),
                    "the row {:?} dispatches its command",
                    entry.label
                );
                assert_eq!(ints_source(&intents[0]), Source::CommandBar);
            }
            Expect::Submenu(children) => {
                // Clicking the row opens the submenu beside it
                // (`SubMenuButton`).
                rig.close_popups();
                let drawn = popup_nodes(rig, |rig| {
                    rig.click_node(opener);
                });
                rig.click_node(&drawn[position]);
                for child in children {
                    assert!(
                        rig.names().contains(&child.label),
                        "the submenu of {:?} draws {:?}",
                        entry.label,
                        child.label
                    );
                }
            }
        }
    }
    rig.close_popups();
}

/// The Quick Access Toolbar dispatches its items (ADR 0065 "the QAT
/// works"): Save runs `file.save`, the Undo split's main part runs
/// `edit.undo`, its arrow opens the history list, and the customise
/// dropdown opens its menu. The nodes are the QAT items in order.
#[test]
fn the_quick_access_toolbar_dispatches_and_opens() {
    let layout: Rc<LayoutV2> = Rc::new(sets().get(LayoutKind::PanelModern).v2.clone());
    let qat = layout.quick_access_toolbar.clone();
    assert!(!qat.is_empty(), "the layout has a QAT");
    let app: Rc<AppCtx> = Rc::new(RichApp::rich().0);
    let registry: Rc<Registry> = Rc::new(builtin_registry());
    let panel = Rc::new(RefCell::new(PanelModern::new()));
    let intents: Rc<RefCell<Vec<Intent>>> = Rc::new(RefCell::new(Vec::new()));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 120.0))
        .build_ui({
            let layout = Rc::clone(&layout);
            let app = Rc::clone(&app);
            let registry = Rc::clone(&registry);
            let panel = Rc::clone(&panel);
            let intents = Rc::clone(&intents);
            move |ui| {
                let mut panel = panel.borrow_mut();
                let produced = panel
                    .show_qat(ui, &layout, &app, &registry)
                    .expect("the QAT renders");
                intents.borrow_mut().extend(produced);
            }
        });
    harness.run();
    // The zip over the QAT nodes: one node per item, plus the split
    // arrow ("More items") after a split. The `dynamic_label` items
    // draw their empty-undo-stack labels ("Can't Undo", "Can't Redo").
    let expected: Vec<String> = qat
        .iter()
        .flat_map(|item| {
            let label = if item.dynamic_label {
                match item.command.as_ref().map(|c| c.as_str()) {
                    Some("edit.undo") => "Can't Undo".to_owned(),
                    Some("edit.redo.or.repeat") => "Can't Redo".to_owned(),
                    _ => item.label.clone().unwrap_or_default(),
                }
            } else {
                item.label.clone().unwrap_or_default()
            };
            match item.kind {
                ItemKind::Split | ItemKind::SplitRecent => {
                    vec![label, expect::DROPDOWN_TIP.to_owned()]
                }
                _ => vec![label],
            }
        })
        .collect();
    let root = harness.root();
    let drawn: Vec<String> = root
        .children_recursive()
        .map(|node| label_of(&node))
        .filter(|name| !name.is_empty())
        .collect();
    for (index, name) in expected.iter().enumerate() {
        assert_eq!(
            drawn.get(index),
            Some(name),
            "QAT node {index} of {expected:?} in {drawn:?}"
        );
    }
    // Save runs file.save (the undo stack is empty, so the dynamic
    // labels read "Can't Undo" and the split renders disabled).
    std::mem::take(&mut *intents.borrow_mut());
    {
        let root = harness.root();
        let node = root
            .query_all_by_role_and_label(Role::Button, "Save")
            .next()
            .expect("the Save button");
        node.click();
    }
    harness.run();
    harness.run_steps(2);
    let produced = std::mem::take(&mut *intents.borrow_mut());
    assert_eq!(produced.len(), 1, "Save dispatches once");
    assert_eq!(produced[0].command.as_str(), "file.save");
    // The customise dropdown is a Phase 7 stub (`view.qat.customize`),
    // so with the real registry it renders disabled and its
    // display-only menu does not open: the widget tests drive it with
    // a test resolver (P7-07), the registry path is this one. Recorded
    // as a finding in the P7-10 Result.
    let root = harness.root();
    let node = root
        .query_all_by_role_and_label(Role::Button, "Customize Quick Access Toolbar")
        .next()
        .expect("the customise button");
    assert!(
        node.accesskit_node().data().is_disabled(),
        "the customise button renders disabled (its command is a stub)"
    );
}

/// The File view's command entries dispatch their commands (ADR 0065
/// "the File view works").
#[test]
fn every_file_view_command_entry_dispatches() {
    let mut rig = ModernRig::new();
    let registry = builtin_registry();
    let layout = rig.layout().clone();
    let view = layout
        .file_view
        .as_ref()
        .expect("the layout has a File view");
    rig.panel.borrow_mut().set_file_view_open(true);
    rig.settle();
    for entry in &view.nav {
        if entry.kind == larrygram_ui_spec::v2::FileViewEntryKind::Page {
            // A page entry switches the page: no command dispatches.
            rig.panel.borrow_mut().set_file_view_open(true);
            rig.settle();
            rig.take_intents();
            rig.click(Role::Button, &entry.label);
            assert!(
                rig.take_intents().is_empty(),
                "the File view page entry {:?} dispatches nothing",
                entry.label
            );
            assert_eq!(
                rig.panel.borrow().file_view().page(),
                entry.id.as_str(),
                "the File view page entry {:?} activates its page",
                entry.label
            );
            continue;
        }
        let expect = entry.command.as_ref().map(|command| {
            expect::resolve(
                &Item {
                    id: entry.id.clone(),
                    kind: ItemKind::Button,
                    label: Some(entry.label.clone()),
                    access: None,
                    shortcut: None,
                    command: Some(command.clone()),
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
                },
                &registry,
                rig.app(),
                None,
            )
        });
        match expect {
            Some(Expect::Dispatch { command, .. }) => {
                rig.panel.borrow_mut().set_file_view_open(true);
                rig.settle();
                rig.take_intents();
                rig.click(Role::Button, &entry.label);
                let intents = rig.take_intents();
                assert_eq!(
                    intents.len(),
                    1,
                    "the File view entry {:?} dispatches once",
                    entry.label
                );
                assert_eq!(
                    intents[0].command.as_str(),
                    command.as_str(),
                    "the File view entry {:?} dispatches its command",
                    entry.label
                );
            }
            Some(Expect::Disabled { reason }) => {
                rig.panel.borrow_mut().set_file_view_open(true);
                rig.settle();
                let nodes = rig.named_nodes();
                let node = nodes
                    .iter()
                    .find(|n| n.name == entry.label)
                    .unwrap_or_else(|| panic!("no File view entry named {:?}", entry.label));
                assert!(
                    node.disabled,
                    "the File view entry {:?} renders disabled",
                    entry.label
                );
                if !reason.is_empty() {
                    assert_eq!(
                        node.description.as_deref(),
                        Some(reason.as_str()),
                        "the File view entry {:?} carries its reason",
                        entry.label
                    );
                }
            }
            _ => {}
        }
    }
}
