//! The ADR 0065 menu sweep (task P7-10): every menu, dropdown and
//! context menu of the committed layouts opens, and every enabled item
//! dispatches the expected command.
//!
//! The expectations come from `expect`, computed over the committed
//! `LayoutV2` and the builtin registry. The rig drives the real
//! `PanelClassic` widgets headlessly and records the intents the
//! widgets produce (the dispatch contract of ADR 0028: a widget emits
//! an `Intent`, the shell runs it through the registry).
//!
//! Matching a drawn widget to its data item: the widgets draw one node
//! per item in data order, so the nodes of an open menu chain (or a
//! docked toolbar) are zipped against the expected entries in order.
//! A test asserts the whole sequence matches, then clicks every
//! enabled entry.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::AppCtx;
use larrygram_commands::{Registry, Source};
use larrygram_ui::Intent;
use larrygram_ui::panel_classic::{PanelClassic, PanelInput, WindowMode};
use larrygram_ui_spec::v2::{ItemKind, LayoutKind, LayoutV2};

use crate::expect::{self, Entry, Expect};
use crate::{RichApp, builtin_registry, sets};

/// The run-time lists are empty in the sweep: the rows the lists
/// expand to (recent files, pages, ...) are covered by the widget
/// tests (`panel_classic::tests`), and an empty list draws one
/// disabled "(none)" row, which is what `expect` models.
pub struct EmptyLists;

impl larrygram_ui::lists::ListProvider for EmptyLists {}

/// The height of the Panel Classic menu bar in logical pixels
/// (`docs/ui/panel-classic.md`); the docked toolbar rows start below
/// it, and the toolbar sweep reads its nodes from that band.
pub const MENU_BAR_HEIGHT: f32 = 24.0;

/// One named widget of a frame.
#[derive(Clone)]
pub struct Named {
    /// The position in the frame's named-node list (tree order).
    pub index: usize,
    /// The accessible name.
    pub name: String,
    /// The accesskit role.
    pub role: Role,
    /// Whether the widget renders disabled.
    pub disabled: bool,
    /// The tooltip (accesskit description).
    pub description: Option<String>,
    /// The drawn rectangle.
    pub rect: egui::Rect,
}

/// The accessible name of one node.
pub fn label_of<'t, N: NodeT<'t>>(node: &N) -> String {
    let data = node.accesskit_node().data();
    let label = data.label().unwrap_or_default();
    if label.is_empty() {
        data.value().unwrap_or_default().to_owned()
    } else {
        label.to_owned()
    }
}

/// The Panel Classic widgets over one committed layout, driven
/// headlessly, with the intents of every frame recorded.
pub struct ClassicRig {
    /// The widget under test.
    pub panel: Rc<RefCell<PanelClassic>>,
    /// The application state the enablement predicates see.
    app: Rc<AppCtx>,
    /// The layout under test.
    layout: Rc<LayoutV2>,
    /// The intents the frames produced.
    intents: Rc<RefCell<Vec<Intent>>>,
    /// The context menu id to draw (empty: none).
    context_id: Rc<RefCell<String>>,
    /// The frame clock.
    now: f64,
    /// The harness.
    pub harness: Harness<'static>,
}

impl ClassicRig {
    /// A rig over the committed Panel Classic layout and the builtin
    /// registry, with a rich application state (so the enablement
    /// predicates see a document, a selection and a clipboard).
    pub fn new() -> Self {
        Self::with(
            LayoutKind::PanelClassic,
            WindowMode::Drawing,
            RichApp::rich(),
        )
    }

    /// A rig over one committed layout in one window mode.
    pub fn with(kind: LayoutKind, mode: WindowMode, app: RichApp) -> Self {
        let layout: Rc<LayoutV2> = Rc::new(sets().get(kind).v2.clone());
        let registry: Rc<Registry> = Rc::new(builtin_registry());
        let app: Rc<AppCtx> = Rc::new(app.0);
        let panel = Rc::new(RefCell::new(PanelClassic::new()));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let context_id = Rc::new(RefCell::new(String::new()));
        let harness = Harness::builder()
            .with_size(egui::vec2(1600.0, 900.0))
            .build_ui({
                let panel = Rc::clone(&panel);
                let intents = Rc::clone(&intents);
                let context_id = Rc::clone(&context_id);
                let layout = Rc::clone(&layout);
                let registry = Rc::clone(&registry);
                let app = Rc::clone(&app);
                move |ui| {
                    let lists = EmptyLists;
                    let input = PanelInput::new(&layout, &registry, &app)
                        .with_mode(mode)
                        .with_lists(&lists);
                    let mut panel = panel.borrow_mut();
                    let produced = panel.show(ui, &input).expect("the chrome renders");
                    intents.borrow_mut().extend(produced);
                    let id = context_id.borrow().clone();
                    if !id.is_empty() {
                        let produced = panel
                            .context_menu(ui, &input, &id, None)
                            .expect("the context menu renders");
                        intents.borrow_mut().extend(produced);
                    }
                }
            });
        let mut rig = Self {
            panel,
            app,
            layout,
            intents,
            context_id,
            now: 1.0,
            harness,
        };
        rig.run();
        rig
    }

    /// The application state under test.
    pub fn app(&self) -> &AppCtx {
        &self.app
    }

    /// The layout under test.
    pub fn layout(&self) -> &LayoutV2 {
        &self.layout
    }

    /// Draws one frame, 1/60 s after the last one (egui measures the
    /// submenu hover delay and multi-clicks against the frame clock).
    ///
    /// `run_steps`, not `run`: an open tooltip repaints every frame,
    /// so the harness would never settle.
    pub fn run(&mut self) {
        self.now += 1.0 / 60.0;
        self.harness.input_mut().time = Some(self.now);
        self.harness.run_steps(1);
    }

    /// Closes whatever is open: the pointer moves off the rows first
    /// (a hovered submenu row reopens its submenu as soon as the
    /// chain pops), then Escape unwinds every open level.
    pub fn close_all(&mut self) {
        self.harness.hover_at(egui::Pos2::new(1200.0, 820.0));
        self.run();
        for _ in 0..8 {
            let open = self.named_nodes().iter().any(|n| n.role == Role::MenuItem);
            if !open {
                break;
            }
            self.harness.key_press(egui::Key::Escape);
            self.run();
        }
    }

    /// Opens the context menu `id` and settles (the menu's `Area`
    /// shows its rows one frame after it opens).
    pub fn open_context(&mut self, id: &str) {
        self.panel.borrow_mut().open_context_menu(id.to_owned());
        *self.context_id.borrow_mut() = id.to_owned();
        self.settle();
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

    /// The widget names of the current frame (AccessKit order).
    pub fn names(&self) -> Vec<String> {
        self.named_nodes().into_iter().map(|n| n.name).collect()
    }

    /// Clicks the `nth` named widget with `role` and `name`, and
    /// settles (a click dispatches a frame or two later).
    pub fn click_nth(&mut self, role: Role, name: &str, nth: usize) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(role, name)
                .nth(nth)
                .unwrap_or_else(|| {
                    panic!("no {role:?} named {name:?} (#{nth}) in {:?}", self.names())
                });
            node.click();
        }
        self.settle();
    }

    /// Clicks the first widget with `role` and `name`.
    pub fn click(&mut self, role: Role, name: &str) {
        self.click_nth(role, name, 0);
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

    /// Hovers the `nth` widget with `role` and `name` (a toolbar popup
    /// submenu opens on hover) and settles.
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

    /// Runs the settle frames: egui resolves a click a frame or two
    /// later, so the widgets need follow-up frames to dispatch it.
    pub fn settle(&mut self) {
        self.run();
        self.harness.run_steps(2);
    }
}

/// The expected rows of one open menu chain: one level of entries per
/// open menu, top to bottom.
pub struct Chain {
    /// The rows of each open level (level 0 = the top menu or popup).
    pub levels: Vec<Vec<Entry>>,
    /// How to open the chain: the top-level label (a menu-bar title or
    /// a toolbar popup opener), then one (row label, occurrence) per
    /// submenu row on the way down.
    pub open: Vec<(String, usize)>,
}

impl Chain {
    /// The chain of one top-level menu or one popup.
    pub fn top(label: &str, level: Vec<Entry>) -> Self {
        Self {
            levels: vec![level],
            open: vec![(label.to_owned(), 0)],
        }
    }

    /// The chain with `children` opened below the row `(label, nth)`.
    pub fn deeper(&self, label: &str, nth: usize, children: Vec<Entry>) -> Self {
        let mut levels = self.levels.clone();
        levels.push(children);
        let mut open = self.open.clone();
        open.push((label.to_owned(), nth));
        Self { levels, open }
    }

    /// Every entry of the chain, in draw order (level 0 rows, then
    /// level 1, ...).
    fn flat(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (level, entries) in self.levels.iter().enumerate() {
            for index in 0..entries.len() {
                out.push((level, index));
            }
        }
        out
    }

    /// The entry at one flat position.
    fn at(&self, (level, index): (usize, usize)) -> &Entry {
        &self.levels[level][index]
    }

    /// The occurrence of the entry at one flat position among the
    /// same-label entries before it.
    fn occurrence(&self, position: (usize, usize)) -> usize {
        let label = &self.at(position).label;
        let mut nth = 0;
        for earlier in self.flat() {
            if earlier == position {
                break;
            }
            if &self.at(earlier).label == label {
                nth += 1;
            }
        }
        nth
    }

    /// The occurrence of a submenu row inside its own level (the
    /// opener click).
    fn level_occurrence(&self, position: (usize, usize)) -> usize {
        let label = &self.at(position).label;
        let mut nth = 0;
        for index in 0..position.1 {
            if &self.levels[position.0][index].label == label {
                nth += 1;
            }
        }
        nth
    }
}

/// How one surface opens its top level (a menu-bar or popup button
/// click; or a context menu, opened through the widget API the shell
/// routes its right-clicks with).
#[derive(Clone, Copy, PartialEq)]
pub enum Opener {
    /// Click the button named by `open[0]`.
    MenuTitle,
    /// Open the context menu named by `open[0]`.
    ContextMenu,
}

/// How one surface opens its chain (`click`: the menu bars and context
/// menus; `hover`: the toolbar popups, whose submenus open on hover).
#[derive(Clone, Copy, PartialEq)]
pub enum OpenBy {
    /// Submenu rows open on click.
    Click,
    /// Submenu rows open on hover.
    Hover,
}

/// Opens the chain on a fresh menu (Escape first), using `chain.open`.
pub fn open_chain(rig: &mut ClassicRig, chain: &Chain, role: Role, by: OpenBy, opener: Opener) {
    rig.close_all();
    for (index, (label, nth)) in chain.open.iter().enumerate() {
        if index == 0 {
            // The opener: a menu-bar title (a button), a toolbar popup
            // button or a context menu.
            match opener {
                Opener::MenuTitle => rig.click(Role::Button, label),
                Opener::ContextMenu => {
                    rig.close_all();
                    rig.open_context(label);
                }
            }
        } else {
            match by {
                OpenBy::Click => rig.click_nth(role, label, *nth),
                OpenBy::Hover => rig.hover_nth(role, label, *nth),
            }
        }
    }
}

/// Checks one open chain: the drawn rows must be exactly the expected
/// rows in order, and every enabled row must dispatch its command when
/// clicked. Submenu rows recurse into their children.
pub fn check_chain(
    rig: &mut ClassicRig,
    chain: &Chain,
    source: Source,
    role: Role,
    by: OpenBy,
    opener: Opener,
) {
    // The zip: every level draws its rows in order, so the named rows
    // of the open chain are exactly the flattened expected rows.
    open_chain(rig, chain, role, by, opener);
    let nodes = rig.named_nodes();
    let drawn: Vec<&Named> = nodes.iter().filter(|n| n.role == role).collect();
    let flat = chain.flat();
    assert!(
        drawn.len() >= flat.len(),
        "the open chain {:?} draws {} rows, expected {}",
        chain.open,
        drawn.len(),
        flat.len()
    );
    for (draw_index, position) in flat.iter().enumerate() {
        let entry = chain.at(*position);
        assert_eq!(
            drawn[draw_index].name, entry.label,
            "row {draw_index} of the open chain {:?} is wrong",
            chain.open
        );
    }

    // Only the deepest level is left to click: the levels above are
    // open (the zip checked their rows) and their submenus recursed
    // from their own `check_chain` call.
    let deepest = chain.levels.len().saturating_sub(1);
    for index in 0..chain.levels[deepest].len() {
        let position = (deepest, index);
        let entry = chain.at(position).clone();
        let nth = chain.occurrence(position);
        match &entry.expect {
            Expect::Heading => {
                open_chain(rig, chain, role, by, opener);
                rig.take_intents();
                rig.click_nth(role, &entry.label, nth);
                assert!(
                    rig.take_intents().is_empty(),
                    "the heading {:?} dispatches nothing",
                    entry.label
                );
            }
            Expect::Disabled { reason } => {
                open_chain(rig, chain, role, by, opener);
                let nodes = rig.named_nodes();
                let node = nodes
                    .iter()
                    .filter(|n| n.role == role && n.name == entry.label)
                    .nth(nth)
                    .unwrap_or_else(|| panic!("no row named {:?}", entry.label));
                assert!(node.disabled, "row {:?} renders disabled", entry.label);
                if !reason.is_empty() {
                    assert_eq!(
                        node.description.as_deref(),
                        Some(reason.as_str()),
                        "row {:?} carries the disabled reason",
                        entry.label
                    );
                }
            }
            Expect::Dispatch { command, value } => {
                open_chain(rig, chain, role, by, opener);
                rig.take_intents();
                rig.click_nth(role, &entry.label, nth);
                let intents = rig.take_intents();
                assert_eq!(
                    intents.len(),
                    1,
                    "row {:?} dispatches once (got {intents:?})",
                    entry.label
                );
                assert_eq!(
                    intents[0].command.as_str(),
                    command.as_str(),
                    "row {:?} dispatches its command",
                    entry.label
                );
                assert_eq!(ints_source(&intents[0]), source, "the dispatch source");
                if let Some(value) = value {
                    assert_eq!(
                        intents[0].invocation.args.text("value"),
                        Some(value.as_str()),
                        "row {:?} carries the value argument",
                        entry.label
                    );
                }
            }
            Expect::Toggle { command } => {
                open_chain(rig, chain, role, by, opener);
                rig.take_intents();
                rig.click_nth(role, &entry.label, nth);
                let intents = rig.take_intents();
                match command {
                    Some(command) => {
                        assert_eq!(
                            intents.len(),
                            1,
                            "the row {:?} dispatches once",
                            entry.label
                        );
                        assert_eq!(
                            intents[0].command.as_str(),
                            command.as_str(),
                            "the row {:?} dispatches its command",
                            entry.label
                        );
                    }
                    None => assert!(
                        intents.is_empty(),
                        "the row {:?} dispatches nothing ({intents:?})",
                        entry.label
                    ),
                }
            }
            Expect::Submenu(children) => {
                let row_nth = chain.level_occurrence(position);
                let deeper = chain.deeper(&entry.label, row_nth, children.clone());
                check_chain(rig, &deeper, source, role, by, opener);
            }
        }
    }
    rig.close_all();
}

/// The dispatch source of one intent.
fn ints_source(intent: &Intent) -> Source {
    intent.invocation.source
}

/// The menus of every menu bar: each top-level menu opens with its
/// rows, and every enabled row dispatches its command (ADR 0065).
#[test]
fn every_menu_of_every_menu_bar_opens_and_dispatches() {
    let registry = builtin_registry();
    for mode in [
        WindowMode::Drawing,
        WindowMode::Shapesheet,
        WindowMode::Stencil,
    ] {
        let mut rig = ClassicRig::with(LayoutKind::PanelClassic, mode, RichApp::rich());
        let layout = rig.layout().clone();
        let bar = layout
            .menu_bars
            .iter()
            .find(|bar| bar.shown_in == mode.context())
            .unwrap_or_else(|| panic!("a menu bar for {mode:?}"));
        assert!(!bar.menus.is_empty(), "{mode:?} has menus");
        for menu in &bar.menus {
            let level: Vec<Entry> = menu
                .items
                .iter()
                .flat_map(|item| {
                    expect::classic_level(std::slice::from_ref(item), &registry, rig.app())
                })
                .collect();
            assert!(!level.is_empty(), "the menu {:?} has rows", menu.label);
            let chain = Chain::top(&menu.label, level);
            check_chain(
                &mut rig,
                &chain,
                Source::Menu,
                Role::MenuItem,
                OpenBy::Click,
                Opener::MenuTitle,
            );
        }
    }
}

/// Every context menu of both committed layouts opens with its rows,
/// and every enabled row dispatches its command (ADR 0065).
#[test]
fn every_context_menu_opens_and_dispatches() {
    let registry = builtin_registry();
    for kind in [LayoutKind::PanelClassic, LayoutKind::PanelModern] {
        let mut rig = ClassicRig::with(kind, WindowMode::Drawing, RichApp::rich());
        hide_floating(&mut rig);
        let layout = rig.layout().clone();
        assert!(
            !layout.context_menus.is_empty(),
            "{kind:?} has context menus"
        );
        for menu in &layout.context_menus {
            let level = expect::classic_level(&menu.items, &registry, rig.app());
            assert!(!level.is_empty(), "the menu {:?} has rows", menu.id);
            let chain = Chain::top(&menu.id, level);
            check_chain(
                &mut rig,
                &chain,
                Source::Menu,
                Role::MenuItem,
                OpenBy::Click,
                Opener::ContextMenu,
            );
        }
    }
}

/// Shows every docked toolbar in the drawing mode (the sweep covers
/// all of them, not only the two the data shows by default). The
/// floating toolbars hide: their tool windows land on top of the
/// docked rows and eat the clicks.
fn show_all_docked(rig: &mut ClassicRig) {
    for toolbar in &rig.layout().clone().toolbars {
        let visible = toolbar.dock != larrygram_ui_spec::v2::Dock::Floating;
        rig.panel
            .borrow_mut()
            .set_toolbar_visible_in(WindowMode::Drawing, &toolbar.id, visible);
    }
    rig.run();
}

/// Hides the floating toolbars: their tool windows land on top of the
/// menus and eat the clicks.
fn hide_floating(rig: &mut ClassicRig) {
    for toolbar in &rig.layout().clone().toolbars {
        if toolbar.dock == larrygram_ui_spec::v2::Dock::Floating {
            rig.panel
                .borrow_mut()
                .set_toolbar_visible_in(WindowMode::Drawing, &toolbar.id, false);
        }
    }
    rig.run();
}

/// The drawn toolbar items of one mode, in draw order (rows top to
/// bottom, toolbars in data order, items in data order).
pub fn drawn_toolbar_items(
    rig: &ClassicRig,
    mode: WindowMode,
) -> Vec<&larrygram_ui_spec::v2::Item> {
    let layout = rig.layout();
    let visible = |toolbar: &larrygram_ui_spec::v2::Toolbar| {
        toolbar.dock != larrygram_ui_spec::v2::Dock::Floating
            && (toolbar.shown_in.is_none() || toolbar.shown_in == mode.context())
            && rig
                .panel
                .borrow()
                .toolbar_visible_in(layout, mode, &toolbar.id)
    };
    let mut rows: Vec<u32> = layout
        .toolbars
        .iter()
        .filter(|t| visible(t))
        .map(|t| t.row.unwrap_or(0))
        .collect();
    rows.sort_unstable();
    rows.dedup();
    let mut out = Vec::new();
    for row in rows {
        for toolbar in layout.toolbars.iter().filter(|t| visible(t)) {
            if toolbar.row.unwrap_or(0) != row {
                continue;
            }
            out.extend(toolbar.items.iter().filter(|item| !item.hidden));
        }
    }
    out
}

/// The node sequence one toolbar item draws: its main button (or
/// combo), then the split arrow ("`label` menu") when it has one.
fn toolbar_node_labels(item: &larrygram_ui_spec::v2::Item) -> Vec<(String, Role)> {
    let label = item.label.clone().unwrap_or_default();
    let mut out = Vec::new();
    match item.kind {
        ItemKind::Combo => out.push((label, Role::ComboBox)),
        ItemKind::Split | ItemKind::SplitRecent => {
            out.push((label.clone(), Role::Button));
            out.push((format!("{label} menu"), Role::Button));
        }
        _ => out.push((label, Role::Button)),
    }
    out
}

/// Every toolbar button, dropdown, palette, split and combo behaves as
/// the data says: enabled items dispatch (or open their popup and
/// dispatch from it), disabled ones render disabled with their reason
/// (ADR 0065).
#[test]
fn every_toolbar_item_behaves() {
    let mut rig = ClassicRig::new();
    let registry = builtin_registry();
    // Every docked toolbar visible: the sweep covers all of them, not
    // only the two the data shows by default.
    show_all_docked(&mut rig);
    let items = drawn_toolbar_items(&rig, WindowMode::Drawing)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        items.len() > 100,
        "the toolbars draw items ({})",
        items.len()
    );
    // The zip: the drawn toolbar nodes are exactly the item nodes, in
    // order (the menu bar is above the band, so it cannot collide).
    let expected: Vec<(String, Role)> = items
        .iter()
        .flat_map(toolbar_node_labels)
        .collect::<Vec<_>>();
    let nodes = rig.named_nodes();
    let drawn: Vec<&Named> = nodes
        .iter()
        .filter(|n| n.rect.top() >= MENU_BAR_HEIGHT)
        .filter(|n| n.role == Role::Button || n.role == Role::ComboBox)
        .collect();
    assert!(
        drawn.len() >= expected.len(),
        "the toolbars draw {} nodes, expected {}",
        drawn.len(),
        expected.len()
    );
    for (index, (label, role)) in expected.iter().enumerate() {
        assert_eq!(drawn[index].name, *label, "toolbar node {index}");
        assert_eq!(drawn[index].role, *role, "toolbar node {index} role");
    }

    let mut clicked = 0;
    let mut disabled = 0;
    let mut popups = 0;
    for item in &items {
        let label = item.label.clone().unwrap_or_default();
        match item.kind {
            ItemKind::Combo => {
                let expect = expect::resolve(item, &registry, rig.app(), None);
                let Expect::Dispatch { command, .. } = expect else {
                    disabled += 1;
                    continue;
                };
                rig.take_intents();
                {
                    let root = rig.harness.root();
                    let combo = root
                        .query_all_by_role_and_label(Role::ComboBox, &label)
                        .next()
                        .unwrap_or_else(|| panic!("no combo named {label:?}"));
                    combo.click();
                    combo.type_text("Arial");
                }
                rig.run();
                rig.harness.key_press(egui::Key::Enter);
                rig.settle();
                let intents = rig.take_intents();
                assert_eq!(intents.len(), 1, "the combo {label:?} applies once");
                assert_eq!(
                    intents[0].command.as_str(),
                    command.as_str(),
                    "the combo {label:?} dispatches its command"
                );
                assert_eq!(
                    intents[0].invocation.args.text("value"),
                    Some("Arial"),
                    "the combo {label:?} carries the typed value"
                );
                clicked += 1;
            }
            ItemKind::Split | ItemKind::SplitRecent | ItemKind::Dropdown | ItemKind::Palette => {
                let expect = expect::resolve(item, &registry, rig.app(), None);
                // The main part of a split dispatches; a dropdown
                // button only opens its popup.
                if matches!(item.kind, ItemKind::Split | ItemKind::SplitRecent) {
                    match &expect {
                        Expect::Dispatch { command, .. } => {
                            rig.take_intents();
                            rig.click(Role::Button, &label);
                            let intents = rig.take_intents();
                            assert_eq!(intents.len(), 1, "{label:?} dispatches once");
                            assert_eq!(
                                intents[0].command.as_str(),
                                command.as_str(),
                                "{label:?} dispatches its command"
                            );
                            clicked += 1;
                        }
                        Expect::Disabled { reason } => {
                            assert_node(&mut rig, Role::Button, &label, reason.as_str());
                            disabled += 1;
                        }
                        _ => {}
                    }
                }
                // The popup: its rows dispatch or disable as the data
                // says (a disabled control's popup never opens).
                if matches!(expect, Expect::Disabled { .. }) {
                    continue;
                }
                let level = expect::classic_level(&item.items, &registry, rig.app());
                if level.is_empty() {
                    continue;
                }
                popups += 1;
                let opener = match item.kind {
                    ItemKind::Split | ItemKind::SplitRecent => format!("{label} menu"),
                    _ => label.clone(),
                };
                let chain = Chain::top(&opener, level);
                check_chain(
                    &mut rig,
                    &chain,
                    Source::Ui,
                    Role::MenuItem,
                    OpenBy::Hover,
                    Opener::MenuTitle,
                );
            }
            _ => {
                let expect = expect::resolve(item, &registry, rig.app(), None);
                match expect {
                    Expect::Dispatch { command, .. } => {
                        rig.take_intents();
                        rig.click(Role::Button, &label);
                        let intents = rig.take_intents();
                        assert_eq!(intents.len(), 1, "toolbar item {label:?} dispatches once");
                        assert_eq!(
                            intents[0].command.as_str(),
                            command.as_str(),
                            "toolbar item {label:?} dispatches its command"
                        );
                        assert_eq!(ints_source(&intents[0]), Source::Ui);
                        clicked += 1;
                    }
                    Expect::Disabled { reason } => {
                        assert_node(&mut rig, Role::Button, &label, reason.as_str());
                        disabled += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        clicked > 5 && disabled > 5 && popups >= 3,
        "the sweep clicked {clicked}, checked {disabled} and opened {popups} toolbar items"
    );
}

/// The `nth` widget with `role` and `name` renders disabled with
/// `reason` as its tooltip (an empty reason: no tooltip).
pub fn assert_node(rig: &mut ClassicRig, role: Role, name: &str, reason: &str) {
    let nodes = rig.named_nodes();
    let node = nodes
        .iter()
        .find(|n| n.role == role && n.name == name)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    assert!(node.disabled, "{name:?} renders disabled");
    if !reason.is_empty() {
        assert_eq!(
            node.description.as_deref(),
            Some(reason),
            "{name:?} carries the disabled reason"
        );
    }
}

/// View > Toolbars lists every named toolbar and shows and hides each
/// one (Phase 7 exit criterion 2).
#[test]
fn the_view_toolbars_menu_toggles_every_toolbar() {
    let mut rig = ClassicRig::new();
    let layout = rig.layout().clone();
    // The View > Toolbars rows name the toolbars the submenu lists.
    let listed: Vec<String> = layout
        .menu_bars
        .iter()
        .flat_map(|bar| &bar.menus)
        .find(|menu| menu.label == "View")
        .and_then(|menu| {
            menu.items
                .iter()
                .find(|item| item.label.as_deref() == Some("Toolbars"))
        })
        .map(|item| {
            item.items
                .iter()
                .map(|child| child.label.clone().unwrap_or_default())
                .collect()
        })
        .expect("the View > Toolbars submenu");
    let named: Vec<(String, String)> = listed
        .iter()
        .map(|label| {
            let id = rig.panel.borrow().toolbar_id_for_label(&layout, label);
            (label.clone(), id)
        })
        .collect();
    assert!(
        named.len() >= 16,
        "View > Toolbars lists the toolbars ({})",
        named.len()
    );
    for (label, id) in &named {
        // Choose the row twice: each choice flips the visibility, so
        // the second one restores the data default.
        for _ in 0..2 {
            open_chain(
                &mut rig,
                &Chain::top("View", Vec::new()),
                Role::MenuItem,
                OpenBy::Click,
                Opener::MenuTitle,
            );
            rig.click(Role::MenuItem, "Toolbars");
            let before = rig
                .panel
                .borrow()
                .toolbar_visible_in(&layout, WindowMode::Drawing, id);
            rig.click(Role::MenuItem, label);
            let after = rig
                .panel
                .borrow()
                .toolbar_visible_in(&layout, WindowMode::Drawing, id);
            assert_ne!(before, after, "the choice toggles {label:?} ({id})");
        }
    }
}

/// The drawing menu bar shows its 10 menus with access keys, and Alt+F
/// opens File (Phase 7 exit criterion 2).
#[test]
fn the_drawing_menu_bar_has_its_ten_menus_with_access_keys() {
    let mut rig = ClassicRig::new();
    let layout = rig.layout().clone();
    let bar = layout
        .menu_bars
        .iter()
        .find(|bar| bar.shown_in.is_none())
        .expect("the drawing menu bar");
    assert_eq!(bar.menus.len(), 10, "the drawing menu bar has 10 menus");
    for menu in &bar.menus {
        assert!(
            menu.access.is_some(),
            "the menu {:?} has an access key",
            menu.label
        );
        assert!(
            rig.names().contains(&menu.label),
            "the menu bar draws {:?}",
            menu.label
        );
    }
    rig.harness
        .key_press_modifiers(egui::Modifiers::ALT, egui::Key::F);
    rig.run();
    assert!(
        rig.names().iter().any(|n| n.starts_with("New")),
        "Alt+F opens File"
    );
}

/// The shape and page context menus open on a right-click trigger (the
/// widget seam the shell routes its right-clicks through; exit
/// criterion 2). The page-tab target is not routed yet (P7-09 open
/// issue 3), so this drives the same seam the shell uses.
#[test]
fn the_shape_and_page_context_menus_open_on_right_click() {
    use larrygram_ui::contexts::{HitTarget, menu_for};
    let mut rig = ClassicRig::new();
    let layout = rig.layout().clone();
    for hit in [
        HitTarget::Shape,
        HitTarget::Page,
        HitTarget::PageTab,
        HitTarget::TextEditing,
    ] {
        let Some(menu) = menu_for(&layout, hit) else {
            // The layout carries no menu for this target (the classic
            // data has no page menu; P7-09 open issue 3 records the
            // unrouted targets).
            continue;
        };
        let id = menu.id.as_str();
        rig.close_all();
        rig.open_context(&menu.id);
        let names = rig.names();
        for item in menu.items.iter().filter(|item| !item.hidden) {
            let Some(label) = item.label.clone() else {
                // A nameless item draws a row with no accessible name.
                continue;
            };
            assert!(names.contains(&label), "{id} draws {label:?}");
        }
    }
}

/// A context menu's submenu rows open their children and the chain
/// stays open while the pointer moves into them (task P7-10, the
/// `context_menu` seam fix): a click opens at once, a hover after the
/// submenu delay.
#[test]
fn a_context_menu_submenu_opens_its_children() {
    let mut rig = ClassicRig::new();
    hide_floating(&mut rig);
    let layout = rig.layout().clone();
    let menu = layout
        .context_menus
        .iter()
        .find(|menu| {
            menu.items
                .iter()
                .any(|item| !item.items.is_empty() && item.label.is_some())
        })
        .expect("a context menu with a submenu row");
    let submenu = menu
        .items
        .iter()
        .find(|item| !item.items.is_empty() && item.label.is_some())
        .expect("the submenu row");
    let child = submenu
        .items
        .iter()
        .find(|item| item.label.is_some())
        .expect("a named child");
    let label = submenu.label.clone().unwrap_or_default();
    let child_label = child.label.clone().unwrap_or_default();
    // A click opens the submenu at once.
    rig.open_context(&menu.id);
    assert!(
        !rig.names().contains(&child_label),
        "the submenu is closed at first"
    );
    rig.click(Role::MenuItem, &label);
    assert!(
        rig.names().contains(&child_label),
        "clicking {label:?} opens its children"
    );
    // The chain stays open while the pointer is inside the child
    // level, and a click there dispatches the child's command.
    let expect = expect::resolve(child, &builtin_registry(), rig.app(), None);
    if let expect::Expect::Dispatch { command, .. } = expect {
        rig.take_intents();
        rig.click(Role::MenuItem, &child_label);
        let intents = rig.take_intents();
        assert_eq!(
            intents.len(),
            1,
            "the child {child_label:?} dispatches once"
        );
        assert_eq!(intents[0].command.as_str(), command.as_str());
    }
    // A hover opens the submenu too, after the delay.
    rig.close_all();
    rig.open_context(&menu.id);
    rig.hover_nth(Role::MenuItem, &label, 0);
    for _ in 0..30 {
        rig.run();
    }
    assert!(
        rig.names().contains(&child_label),
        "hovering {label:?} opens its children after the delay"
    );
}

/// Standard and Formatting dock on rows 0 and 1 (Phase 7 exit
/// criterion 2): the data's dock rows and the drawn order (the
/// toolbars draw row by row, top to bottom).
#[test]
fn standard_and_formatting_dock_on_rows_0_and_1() {
    let rig = ClassicRig::new();
    let layout = rig.layout().clone();
    let row_of = |id: &str| {
        layout
            .toolbars
            .iter()
            .find(|toolbar| toolbar.id == id)
            .unwrap_or_else(|| panic!("the {id} toolbar"))
            .row
    };
    assert_eq!(row_of("toolbar.standard"), Some(0), "Standard on row 0");
    assert_eq!(row_of("toolbar.formatting"), Some(1), "Formatting on row 1");
    // And they are the two the data shows by default.
    for id in ["toolbar.standard", "toolbar.formatting"] {
        assert!(
            rig.panel
                .borrow()
                .toolbar_visible_in(&layout, WindowMode::Drawing, id),
            "{id} is visible by default"
        );
    }
    // Every other toolbar starts hidden except the data's own
    // defaults; the sweep above covers the visible ones' items.
    let drawn = drawn_toolbar_items(&rig, WindowMode::Drawing);
    assert!(
        drawn.len() > 10,
        "the two default toolbars draw their items ({})",
        drawn.len()
    );
}
