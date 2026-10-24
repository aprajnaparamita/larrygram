//! The Panel Modern widget tests (task P7-07, ADR 0065).
//!
//! Two layers:
//!
//! 1. **Placement** (ADR 0065 item 4): for every group of the
//!    committed `ui-spec/layouts/panel-modern.yaml`, the rectangles
//!    [`place`] computes respect `column`, `row` and `size`.
//! 2. **`egui_kittest`**: the tab strip, the Paste split (icon part
//!    and arrow part), a launcher dispatch, a gallery opening,
//!    collapse and expand, the File view opening, closing and
//!    dispatching Save, and the contextual tabs of the three contexts.
//!
//! The tests drive [`PanelModern::show_with`] with a resolver that
//! enables every command, and assert the emitted [`Intent`]s.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::{CommandError, CommandId, Edit, Invocation, Registry, Source};
use larrygram_model::Document;
use larrygram_ui_spec::v2::{
    ControlSize, Group, Item, ItemKind, LayoutV2, PanelStyle, Tab, Window,
};

use super::common::Action;
use super::place::{self, Placed};
use super::{ListData, Metrics, PanelModern, redo_labels, show_label_rows, undo_labels};
use crate::contexts::SelectionContext;
use crate::{Intent, UiError};

/// The repository's committed Panel Modern layout.
fn committed_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui-spec/layouts/panel-modern.yaml")
}

/// The committed Panel Modern layout, parsed the way the loader will
/// (task P7-03 owns the loader; these tests read the same data).
fn committed() -> LayoutV2 {
    let text = std::fs::read_to_string(committed_path()).expect("the committed layout reads");
    let value: serde_json::Value =
        serde_saphyr::from_str(&text).expect("the committed layout parses");
    serde_json::from_value(value).expect("the committed layout decodes")
}

/// A resolver that dispatches every command the items name.
fn always(item: &Item) -> Action {
    match &item.command {
        Some(command) => match CommandId::intern(command.as_str()) {
            Ok(id) => Action::Emit(Intent::new(id, Invocation::empty(Source::CommandBar))),
            Err(error) => Action::Disabled {
                reason: error.reason.to_owned(),
            },
        },
        None => Action::Disabled {
            reason: "no command".to_owned(),
        },
    }
}

/// A resolver that dispatches every command but greys out `stub.*`
/// items (the disabled-widget paths need a disabled action).
fn stub_out(item: &Item) -> Action {
    if item.id.starts_with("stub") {
        Action::Disabled {
            reason: "not yet".to_owned(),
        }
    } else {
        always(item)
    }
}

/// A hand-built item: id, kind, label and command.
fn item(id: &str, kind: ItemKind, label: &str, command: Option<&str>) -> Item {
    Item {
        id: id.to_owned(),
        kind,
        label: Some(label.to_owned()),
        access: None,
        shortcut: None,
        command: command.map(larrygram_ui_spec::CommandId::new),
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

/// A hand-built group.
fn group(id: &str, controls: Vec<Item>) -> Group {
    Group {
        id: id.to_owned(),
        label: Some("Group".to_owned()),
        layout: larrygram_ui_spec::v2::GroupLayout::Stack,
        rows: None,
        controls,
        launcher: None,
    }
}

/// A hand-built core tab.
fn tab(id: &str, label: &str, groups: Vec<Group>) -> Tab {
    Tab {
        id: id.to_owned(),
        label: Some(label.to_owned()),
        fluent: None,
        tab_set: None,
        shown_in: None,
        groups,
    }
}

/// A hand-built layout: no File view, no contextual tabs, the tabs
/// and QAT items given (the v2 types are all public data).
fn hand_built(tabs: Vec<Tab>, quick_access_toolbar: Vec<Item>) -> LayoutV2 {
    LayoutV2 {
        schema_version: 2,
        id: "hand-built".to_owned(),
        name: "Hand-built".to_owned(),
        kind: PanelStyle::PanelModern,
        description: None,
        theme: "default".to_owned(),
        window: Window {
            regions: Vec::new(),
        },
        menu_bars: Vec::new(),
        toolbars: Vec::new(),
        quick_access_toolbar,
        file_button: None,
        tabs,
        contextual_tabs: Vec::new(),
        file_view: None,
        context_menus: Vec::new(),
    }
}

/// The width one control takes in the placement test: combos keep
/// their `width`, everything else a plain 42.
fn width_of(item: &Item) -> f32 {
    match item.kind {
        ItemKind::Combo => (item.width.unwrap_or(107.0) as f32).max(1.0),
        _ => 42.0,
    }
}

/// The placement invariants of one group (ADR 0065): same column
/// gives the same x, rows increase downwards, large controls span the
/// group height.
fn check_group(group: &Group) {
    let metrics = super::Metrics::default();
    let items = place::visible_controls(group);
    let widths: Vec<f32> = items.iter().map(|item| width_of(item)).collect();
    let placed = place::place(group, &widths, &metrics);
    assert_eq!(
        placed.len(),
        items.len(),
        "one rectangle per visible control"
    );
    for p in &placed {
        assert!(
            p.rect.width() > 0.0 && p.rect.height() > 0.0,
            "a usable rectangle"
        );
        assert!(
            p.rect.min.x.is_finite() && p.rect.min.y.is_finite(),
            "finite placement"
        );
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
        if placed[i].large {
            assert_eq!(
                ra.height(),
                metrics.content_height(),
                "a large control spans the group"
            );
            assert_eq!(ra.top(), 0.0, "a large control starts at the top");
        }
        for (j, b) in items.iter().enumerate() {
            let rb = placed[j].rect;
            match group.layout {
                larrygram_ui_spec::v2::GroupLayout::Stack => {
                    if a.column == b.column {
                        assert_eq!(ra.left(), rb.left(), "same column, same x");
                        if a.row > b.row && !(placed[i].large || placed[j].large) {
                            assert!(ra.top() > rb.top(), "rows increase downwards");
                        }
                    }
                }
                larrygram_ui_spec::v2::GroupLayout::Rows => {
                    if a.row == b.row {
                        assert_eq!(ra.top(), rb.top(), "same row, same y");
                        if a.column > b.column {
                            assert!(ra.left() > rb.left(), "columns run left to right");
                        }
                    } else if a.row > b.row {
                        assert!(ra.top() > rb.top(), "row 1 below row 0");
                    }
                }
            }
        }
    }
}

#[test]
fn placement_respects_column_row_and_size_for_every_committed_group() {
    let layout = committed();
    let tabs: Vec<&Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    assert!(!tabs.is_empty(), "the committed layout has tabs");
    let mut groups = 0;
    for tab in &tabs {
        for group in &tab.groups {
            check_group(group);
            groups += 1;
        }
    }
    assert!(groups >= 20, "the committed layout has groups ({groups})");
}

#[test]
fn adapted_fluent_groups_flow_and_dedup_like_adr_0031() {
    // A v1-adapter group: no slot data, medium buttons in file order,
    // and one exact repeat (id + label) that must render once.
    let item = |id: &str, label: &str, kind: ItemKind| Item {
        id: id.to_owned(),
        kind,
        label: Some(label.to_owned()),
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
    };
    let group = Group {
        id: "tab.home.tools".to_owned(),
        label: Some("Tools".to_owned()),
        layout: larrygram_ui_spec::v2::GroupLayout::Stack,
        rows: None,
        controls: vec![
            item("PointerTool", "Pointer Tool", ItemKind::Dropdown),
            item("Connector", "Connector", ItemKind::Button),
            item("Connector", "Connector", ItemKind::Button),
        ],
        launcher: None,
    };
    // The display dedup rule drops the repeat.
    let items = place::visible_controls(&group);
    assert_eq!(items.len(), 2);
    let widths = vec![42.0, 42.0];
    let placed = place::place(&group, &widths, &super::Metrics::default());
    // The first control of the group has a menu, so it is large.
    assert!(placed[0].large);
    assert!(!placed[1].large);
}

/// Which surface a [`Rig`] draws each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// The tab strip and the command bar ([`PanelModern::show_with`]).
    Chrome,
    /// The Quick Access Toolbar.
    QuickAccess,
}

/// How a rig resolves a control to its [`Action`] (ADR 0031).
enum Resolving {
    /// A test mapping.
    Test(Rc<dyn Fn(&Item) -> Action>),
    /// The real command registry ([`PanelModern::show`]).
    Registry(Rc<Registry>),
}

/// The Panel Modern chrome driven headlessly.
struct Rig {
    /// The widget state under test.
    panel: Rc<RefCell<PanelModern>>,
    /// The current selection context (ADR 0064).
    context: Rc<RefCell<SelectionContext>>,
    /// The intents the frames produced.
    intents: Rc<RefCell<Vec<Intent>>>,
    /// The frame clock (egui counts multi-clicks by time).
    now: f64,
    harness: Harness<'static>,
}

impl Rig {
    /// A rig on the committed layout with an empty document.
    fn new() -> Self {
        Self::build(
            Surface::Chrome,
            SelectionContext::EmptyPage,
            egui::vec2(2000.0, 600.0),
        )
    }

    /// A rig drawing one surface in one context.
    fn build(surface: Surface, context: SelectionContext, size: egui::Vec2) -> Self {
        Self::build_on(
            surface,
            context,
            size,
            committed(),
            ListData::new(),
            AppCtx::new(Settings::default()),
            Resolving::Test(Rc::new(always)),
        )
    }

    /// A rig on hand-built data with an explicit resolver.
    fn build_on(
        surface: Surface,
        context: SelectionContext,
        size: egui::Vec2,
        layout: LayoutV2,
        lists: ListData,
        app: AppCtx,
        resolving: Resolving,
    ) -> Self {
        let panel = Rc::new(RefCell::new(PanelModern::new()));
        let context = Rc::new(RefCell::new(context));
        let intents: Rc<RefCell<Vec<Intent>>> = Rc::new(RefCell::new(Vec::new()));
        let layout: Rc<LayoutV2> = Rc::new(layout);
        let lists: Rc<ListData> = Rc::new(lists);
        let app: Rc<AppCtx> = Rc::new(app);
        let harness = Harness::builder().with_size(size).build_ui({
            let panel = Rc::clone(&panel);
            let context = Rc::clone(&context);
            let intents = Rc::clone(&intents);
            let layout = Rc::clone(&layout);
            let lists = Rc::clone(&lists);
            let app = Rc::clone(&app);
            move |ui| {
                let mut panel = panel.borrow_mut();
                let produced: Result<Vec<Intent>, UiError> = match (&resolving, surface) {
                    (Resolving::Test(mapping), Surface::Chrome) => {
                        panel.show_with(ui, &layout, *context.borrow(), &app, &lists, &**mapping)
                    }
                    (Resolving::Test(mapping), Surface::QuickAccess) => {
                        panel.show_qat_with(ui, &layout, &app, &**mapping)
                    }
                    (Resolving::Registry(registry), Surface::Chrome) => {
                        panel.show(ui, &layout, *context.borrow(), &app, &lists, registry)
                    }
                    (Resolving::Registry(registry), Surface::QuickAccess) => {
                        panel.show_qat(ui, &layout, &app, registry)
                    }
                };
                intents
                    .borrow_mut()
                    .extend(produced.expect("the surface renders"));
            }
        });
        let mut rig = Self {
            panel,
            context,
            intents,
            now: 1.0,
            harness,
        };
        rig.run();
        rig
    }

    /// One frame, 1/60 s after the last one.
    fn run(&mut self) {
        self.now += 1.0 / 60.0;
        self.harness.input_mut().time = Some(self.now);
        self.harness.run();
    }

    /// Switches the selection context between frames.
    fn set_context(&mut self, context: SelectionContext) {
        *self.context.borrow_mut() = context;
        self.run();
    }

    /// The intents of every frame so far.
    fn intents(&self) -> Vec<String> {
        self.intents
            .borrow()
            .iter()
            .map(|intent| intent.command.as_str().to_owned())
            .collect()
    }

    /// Clears the recorded intents.
    fn clear(&self) {
        self.intents.borrow_mut().clear();
    }

    /// The widget names of the current frame, as a multiset.
    fn names(&self) -> BTreeMap<String, usize> {
        let root = self.harness.root();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for node in root.children_recursive() {
            let data = node.accesskit_node().data();
            let label = data.label().unwrap_or_default();
            let name = if label.is_empty() {
                data.value().unwrap_or_default()
            } else {
                label
            };
            if name.is_empty() {
                continue;
            }
            *counts.entry(name.to_owned()).or_default() += 1;
        }
        counts
    }

    /// Whether the frame draws a widget named `name`.
    fn has(&self, name: &str) -> bool {
        self.names().contains_key(name)
    }

    /// Clicks the `nth` widget with `role` and `name` and runs a frame.
    fn click_nth(&mut self, role: Role, name: &str, nth: usize) {
        let clicked = {
            let root = self.harness.root();
            match root.query_all_by_role_and_label(role, name).nth(nth) {
                Some(node) => {
                    node.click();
                    true
                }
                None => false,
            }
        };
        assert!(clicked, "no {role:?} named {name:?} in {:?}", self.names());
        self.run();
    }

    /// Clicks the first widget with `role` and `name`.
    fn click(&mut self, role: Role, name: &str) {
        self.click_nth(role, name, 0);
    }

    /// Double-clicks the first widget with `role` and `name` (two
    /// clicks 100 ms apart, as a user double-clicks).
    fn double_click(&mut self, role: Role, name: &str) {
        // Past the multi-click window of any earlier click, so the two
        // clicks below count as one double click.
        self.now += 0.5;
        for _ in 0..2 {
            let clicked = {
                let root = self.harness.root();
                match root.query_all_by_role_and_label(role, name).next() {
                    Some(node) => {
                        node.click();
                        true
                    }
                    None => false,
                }
            };
            assert!(clicked, "no {role:?} named {name:?} in {:?}", self.names());
            self.run();
        }
    }

    /// Clicks `entry`, opening `opener` first when the entry is not on
    /// screen yet (a popup may close after each activation).
    fn click_entry(&mut self, opener: &str, entry: &str) {
        if !self.has(entry) {
            self.click(Role::Button, opener);
        }
        self.click(Role::Button, entry);
    }

    /// Whether any widget label contains `needle` (a menu row's label
    /// mixes with its shortcut text).
    fn has_text(&self, needle: &str) -> bool {
        self.names().keys().any(|name| name.contains(needle))
    }

    /// Hovers the first widget whose label contains `needle` and runs
    /// two frames (a submenu opens on hover).
    fn hover_text(&mut self, needle: &str) {
        let hovered = {
            let root = self.harness.root();
            match root.children_recursive().find(|node| {
                let data = node.accesskit_node().data();
                data.label().unwrap_or_default().contains(needle)
            }) {
                Some(node) => {
                    node.hover();
                    true
                }
                None => false,
            }
        };
        assert!(
            hovered,
            "no widget label contains {needle:?} in {:?}",
            self.names()
        );
        self.run();
        self.run();
    }
}

/// The label of the first visible control of a tab (its marker).
fn marker_of(tab: &Tab) -> String {
    for group in &tab.groups {
        let items = place::visible_controls(group);
        if let Some(item) = items.first() {
            return item.label.clone().unwrap_or_default();
        }
    }
    String::new()
}

/// The first gallery of the committed layout.
fn first_gallery(layout: &LayoutV2) -> Item {
    fn walk(items: &[Item], out: &mut Vec<Item>) {
        for item in items {
            if item.kind == ItemKind::Gallery {
                out.push(item.clone());
            }
            walk(&item.items, out);
        }
    }
    let mut found = Vec::new();
    for tab in layout.tabs.iter().chain(layout.contextual_tabs.iter()) {
        for group in &tab.groups {
            walk(&group.controls, &mut found);
        }
    }
    found
        .into_iter()
        .next()
        .expect("the committed layout has a gallery")
}

#[test]
fn each_tab_shows_its_own_controls() {
    let layout = committed();
    let mut rig = Rig::new();
    assert!(rig.has("Paste"), "Home is the first tab");
    for tab in &layout.tabs {
        rig.click(Role::Button, tab.label.as_deref().unwrap_or_default());
        let marker = marker_of(tab);
        assert!(
            rig.has(&marker),
            "tab {:?} draws its control {marker:?}",
            tab.label
        );
        if tab.id != "tab.home" {
            assert!(!rig.has("Paste"), "tab {:?} hides Home", tab.label);
        }
    }
}

#[test]
fn paste_split_icon_part_dispatches_and_arrow_part_opens_the_menu() {
    let mut rig = Rig::new();
    // The icon part runs edit.paste.
    rig.clear();
    rig.click(Role::Button, "Paste");
    assert!(rig.intents().contains(&"edit.paste".to_owned()));
    // The arrow part opens the menu with the split's items.
    rig.click(Role::Button, "More items");
    for entry in [
        "Paste",
        "Paste Special...",
        "Paste as Hyperlink",
        "Duplicate",
    ] {
        assert!(rig.has(entry), "the split menu holds {entry:?}");
    }
    // And an entry dispatches its own command.
    rig.clear();
    rig.click(Role::Button, "Duplicate");
    assert!(rig.intents().contains(&"edit.paste.duplicate".to_owned()));
}

#[test]
fn a_launcher_dispatches() {
    let mut rig = Rig::new();
    // The Font group has a combo named "Font" and the launcher named
    // "Font"; the launcher is drawn after the controls.
    let count = rig.names().get("Font").copied().unwrap_or_default();
    assert!(count >= 2, "the Font combo and its launcher");
    rig.clear();
    rig.click_nth(Role::Button, "Font", count - 1);
    assert!(rig.intents().contains(&"format.all.text.props".to_owned()));
}

#[test]
fn a_gallery_opens() {
    let layout = committed();
    let gallery = first_gallery(&layout);
    let label = gallery.label.clone().unwrap_or_default();
    let mut rig = Rig::new();
    // The galleries live on the tabs beside Home; walk to the tab
    // that holds this one by clicking through until the button shows.
    for tab in &layout.tabs {
        let tab_label = tab.label.clone().unwrap_or_default();
        rig.click(Role::Button, tab_label.as_str());
        if rig.has(label.as_str()) {
            break;
        }
    }
    assert!(
        rig.has(label.as_str()),
        "the gallery button {label:?} draws"
    );
    rig.click(Role::Button, label.as_str());
    // The run-time list is empty in the tests, so the grid shows the
    // "(none)" placeholder.
    assert!(rig.has("(none)"), "an empty gallery shows the placeholder");
}

#[test]
fn collapse_and_expand_the_command_bar() {
    let mut rig = Rig::new();
    assert!(rig.has("Paste"), "the command bar starts expanded");
    // The ^ button collapses to the tab strip.
    rig.click(Role::Button, "Collapse the command bar");
    assert!(rig.panel.borrow().is_collapsed());
    assert!(!rig.has("Paste"), "the collapsed bar draws no groups");
    // A click on a tab shows the bar as an overlay.
    rig.click(Role::Button, "Home");
    assert!(rig.has("Paste"), "the overlay draws the groups");
    // A command closes the overlay again.
    rig.click(Role::Button, "Paste");
    assert!(rig.intents().contains(&"edit.paste".to_owned()));
    assert!(!rig.has("Paste"), "the overlay closes after a command runs");
    // The v button pins the bar open again.
    rig.click(Role::Button, "Expand the command bar");
    assert!(!rig.panel.borrow().is_collapsed());
    assert!(rig.has("Paste"));
    // And Ctrl+F1 toggles it.
    rig.harness
        .key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F1);
    rig.run();
    assert!(rig.panel.borrow().is_collapsed());
    rig.harness
        .key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F1);
    rig.run();
    assert!(!rig.panel.borrow().is_collapsed());
}

#[test]
fn double_click_on_a_tab_collapses_and_pins_the_bar() {
    let mut rig = Rig::new();
    rig.double_click(Role::Button, "Home");
    assert!(
        rig.panel.borrow().is_collapsed(),
        "a double click collapses"
    );
    rig.double_click(Role::Button, "Home");
    assert!(
        !rig.panel.borrow().is_collapsed(),
        "another double click pins it open"
    );
}

#[test]
fn the_file_view_opens_closes_and_dispatches_save() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(rig.panel.borrow().is_file_view_open());
    assert!(rig.has("Info"), "the nav column draws");
    assert!(rig.has("Save"));
    // Save runs and closes the view.
    rig.clear();
    rig.click(Role::Button, "Save");
    assert!(rig.intents().contains(&"file.save".to_owned()));
    assert!(
        !rig.panel.borrow().is_file_view_open(),
        "Save closes the view"
    );
    assert!(rig.has("Paste"), "the command bar is back");
    // Reopening and pressing Esc closes it again.
    rig.click(Role::Button, "File");
    assert!(rig.panel.borrow().is_file_view_open());
    rig.harness.key_press(egui::Key::Escape);
    rig.run();
    assert!(
        !rig.panel.borrow().is_file_view_open(),
        "Esc closes the view"
    );
    // The back button closes it too.
    rig.click(Role::Button, "File");
    rig.click(Role::Button, "Back");
    assert!(!rig.panel.borrow().is_file_view_open());
}

#[test]
fn the_file_view_pages_switch() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    rig.click(Role::Button, "New");
    assert!(rig.has("Blank Drawing"), "the New page draws its tiles");
    rig.click(Role::Button, "Open");
    assert!(rig.has("Browse…"), "the Open page draws Browse");
    rig.click(Role::Button, "Export");
    assert!(rig.has("PNG"), "the Export page draws its formats");
    rig.click(Role::Button, "Print");
    assert!(rig.has("Copies: 1"), "the Print page draws its settings");
}

#[test]
fn contextual_tabs_appear_for_the_three_contexts_and_leave() {
    let layout = committed();
    let mut rig = Rig::new();
    assert!(
        !rig.has("Format"),
        "no contextual tab with nothing selected"
    );
    // Picture Tools > Format.
    rig.set_context(SelectionContext::PictureSelected);
    assert!(rig.has("Format"), "Picture Tools shows Format");
    // Container Tools > Format.
    rig.set_context(SelectionContext::ContainerSelected);
    assert!(rig.has("Format"), "Container Tools shows Format");
    // ShapeSheet Tools > Design (a second Design beside the core tab).
    rig.set_context(SelectionContext::ShapesheetWindow);
    assert_eq!(
        rig.names().get("Design").copied().unwrap_or_default(),
        2,
        "ShapeSheet Tools shows its Design beside the core tab"
    );
    // Contexts without a tab set show nothing contextual.
    rig.set_context(SelectionContext::ShapeSelected);
    assert!(!rig.has("Format"));
    assert_eq!(
        rig.names().get("Design").copied().unwrap_or_default(),
        1,
        "only the core Design remains"
    );
    // The tab sets come from the layout data.
    let sets: Vec<&str> = layout
        .contextual_tabs
        .iter()
        .filter(|tab| tab.shown_in.is_some())
        .map(|tab| tab.tab_set.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        sets,
        vec!["Picture Tools", "Container Tools", "ShapeSheet Tools"]
    );
}

#[test]
fn a_contextual_tab_returns_to_the_last_core_tab_when_its_context_ends() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "Design");
    assert!(rig.has("Themes") || rig.names().len() > 1);
    rig.set_context(SelectionContext::PictureSelected);
    rig.click(Role::Button, "Format");
    assert!(rig.has("Adjust") || rig.has("Brightness"));
    // The context ends: the bar returns to Design, the last core tab.
    rig.set_context(SelectionContext::EmptyPage);
    assert!(!rig.has("Format"));
    let layout = committed();
    let design = layout
        .tabs
        .iter()
        .find(|tab| tab.id == "tab.design")
        .expect("the Design tab");
    let design_marker = marker_of(design);
    assert!(
        rig.has(&design_marker),
        "the command bar returns to the last core tab"
    );
}

#[test]
fn the_quick_access_toolbar_carries_the_dynamic_labels() {
    let mut rig = Rig::build(
        Surface::QuickAccess,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 100.0),
    );
    // With no document the labels read "Can't Undo" and "Can't Redo".
    assert!(rig.has("Save"));
    assert!(rig.has("Can't Undo"));
    assert!(rig.has("Can't Redo"));
    // The customise dropdown lists the current items as checked rows.
    rig.click(Role::Button, "Customize Quick Access Toolbar");
    assert!(rig.has("Undo"), "the customise menu lists Undo");
    assert!(rig.has("Redo"));
}

#[test]
fn placement_uses_the_real_control_sizes() {
    // A hand-built group: a large control, mediums and a combo.
    let item = |id: &str, size: Option<ControlSize>, width: Option<f64>| Item {
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
        size,
        column: Some(0),
        row: Some(0),
        width,
        begin_group: false,
        hidden: false,
        dynamic_label: false,
        list: None,
        options: Vec::new(),
        items: Vec::new(),
    };
    let group = Group {
        id: "g".to_owned(),
        label: Some("G".to_owned()),
        layout: larrygram_ui_spec::v2::GroupLayout::Stack,
        rows: None,
        controls: vec![
            item("big", Some(ControlSize::Large), None),
            item("medium", Some(ControlSize::Medium), None),
            item("small", Some(ControlSize::Small), None),
        ],
        launcher: None,
    };
    check_group(&group);
    let metrics = super::Metrics::default();
    let widths: Vec<f32> = vec![
        metrics.large_min_width,
        metrics.small_width,
        metrics.small_width,
    ];
    let placed: Vec<Placed> = place::place(&group, &widths, &metrics);
    assert!(placed[0].large);
    assert_eq!(placed[0].rect.height(), metrics.content_height());
    assert_eq!(placed[1].rect.height(), metrics.stack_row_height);
}

#[test]
fn panel_style_of_the_committed_layout_is_panel_modern() {
    assert_eq!(committed().kind, PanelStyle::PanelModern);
}

/// A no-op edit with a label, for the undo-stack tests.
struct Labelled(&'static str);

impl Edit for Labelled {
    fn label(&self) -> &str {
        self.0
    }

    fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
        Ok(())
    }

    fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
        Ok(())
    }
}

#[test]
fn list_data_holds_entries_and_marks_empty_lists() {
    let mut lists = ListData::new();
    lists.insert("colors", vec!["Red".to_owned(), "Blue".to_owned()]);
    assert_eq!(
        lists.entries("colors"),
        super::ListEntries::Some(&["Red".to_owned(), "Blue".to_owned()][..])
    );
    // An unknown name and an empty list both read as empty.
    assert_eq!(lists.entries("themes"), super::ListEntries::Empty);
    lists.insert("themes", Vec::new());
    assert_eq!(lists.entries("themes"), super::ListEntries::Empty);
}

#[test]
fn panel_modern_default_and_accessors_round_trip() {
    let mut panel = PanelModern::default();
    assert_eq!(panel.metrics().stack_rows, Metrics::default().stack_rows);
    let bigger = Metrics::default();
    panel.set_metrics(bigger);
    assert!(!panel.is_collapsed());
    panel.set_collapsed(true);
    assert!(panel.is_collapsed());
    panel.set_collapsed(false);
    assert!(!panel.is_collapsed());
    assert!(!panel.is_file_view_open());
    panel.set_file_view_open(true);
    assert!(panel.is_file_view_open());
    assert_eq!(panel.file_view().page(), "file_view.info");
    panel.set_file_view_open(false);
    assert!(!panel.is_file_view_open());
}

#[test]
fn cycle_tab_walks_the_visible_tabs_and_wraps() {
    let layout = committed();
    let tabs: Vec<&Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    let contextual = tabs
        .iter()
        .position(|tab| tab.shown_in.is_some())
        .expect("the committed layout has contextual tabs");
    // No tabs at all: nothing to cycle to.
    let mut panel = PanelModern::new();
    panel.cycle_tab(&[], 1);
    assert_eq!(panel.active_core, 0);
    // A stale core index finds no current tab and stops.
    panel.active_core = 99;
    panel.cycle_tab(&tabs, 1);
    assert_eq!(panel.active_core, 99);
    // One step from the last core tab lands on the contextual tab.
    panel.active_core = contextual - 1;
    panel.cycle_tab(&tabs, 1);
    assert_eq!(
        panel.active_contextual.as_deref(),
        Some(tabs[contextual].id.as_str())
    );
    // One step past the last tab wraps to the first core tab.
    panel.active_contextual = Some(tabs[tabs.len() - 1].id.clone());
    panel.cycle_tab(&tabs, 1);
    assert_eq!(panel.active_contextual, None);
    assert_eq!(panel.active_core, 0);
}

#[test]
fn reconcile_tabs_clamps_an_out_of_range_core_tab() {
    let layout = committed();
    let tabs: Vec<&Tab> = layout
        .tabs
        .iter()
        .chain(layout.contextual_tabs.iter())
        .collect();
    let core_len = layout.tabs.len();
    let mut panel = PanelModern::new();
    panel.active_core = core_len + 5;
    panel.reconcile_tabs(&tabs);
    assert_eq!(panel.active_core, core_len - 1);
    // A contextual tab whose context ended returns to the core tabs.
    panel.active_contextual = Some("tab.gone".to_owned());
    panel.reconcile_tabs(&tabs);
    assert_eq!(panel.active_contextual, None);
}

#[test]
fn the_history_labels_read_the_undo_stack() {
    let mut app = AppCtx::new(Settings::default());
    assert!(undo_labels(&app).is_empty(), "no document, no history");
    assert!(redo_labels(&app).is_empty());
    app.open_document(DocumentSession::new(Document::new(), None));
    app.documents[0].undo.push(Box::new(Labelled("Move shape")));
    app.documents[0]
        .undo
        .push(Box::new(Labelled("Resize shape")));
    assert_eq!(
        undo_labels(&app),
        vec!["Move shape".to_owned(), "Resize shape".to_owned()]
    );
    assert!(redo_labels(&app).is_empty(), "nothing to repeat yet");
    // Undo one edit: it moves to the redo tail.
    let mut stack = std::mem::take(&mut app.documents[0].undo);
    let reverted = stack.undo(&mut app).expect("the edit reverts");
    assert_eq!(reverted.as_deref(), Some("Resize shape"));
    app.documents[0].undo = stack;
    assert_eq!(undo_labels(&app), vec!["Move shape".to_owned()]);
    assert_eq!(redo_labels(&app), vec!["Resize shape".to_owned()]);
}

#[test]
fn history_rows_draw_rows_or_the_placeholder() {
    use crate::test_util::with_ui;
    let action = Action::Emit(Intent::new(
        CommandId::intern("file.save").expect("a valid id"),
        Invocation::empty(Source::CommandBar),
    ));
    let mut out = Vec::new();
    with_ui(|ui| {
        // With no rows the popup draws the "(none)" placeholder.
        show_label_rows(ui, &[], &action, &mut out);
        // With rows the popup draws one button per row.
        show_label_rows(ui, &["Move shape".to_owned()], &action, &mut out);
    });
    assert!(out.is_empty(), "a draw-only frame dispatches nothing");
    // A disabled action greys the rows out.
    let disabled = Action::Disabled {
        reason: "no history".to_owned(),
    };
    with_ui(|ui| {
        show_label_rows(ui, &["Move shape".to_owned()], &disabled, &mut out);
    });
}

#[test]
fn show_resolves_controls_through_the_command_registry() {
    let registry = Rc::new(larrygram_commands::builtins::registry().expect("the registry builds"));
    let layout = hand_built(
        vec![tab(
            "tab.grid",
            "View",
            vec![group(
                "group.grid",
                vec![item(
                    "view.grid",
                    ItemKind::Button,
                    "Grid",
                    Some("view.toggle_grid"),
                )],
            )],
        )],
        Vec::new(),
    );
    let mut rig = Rig::build_on(
        Surface::Chrome,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 300.0),
        layout,
        ListData::new(),
        AppCtx::new(Settings::default()),
        Resolving::Registry(registry),
    );
    assert!(rig.has("Grid"), "the registry path draws the controls");
    rig.clear();
    rig.click(Role::Button, "Grid");
    assert!(rig.intents().contains(&"view.toggle_grid".to_owned()));
}

#[test]
fn show_qat_resolves_through_the_command_registry() {
    let registry = Rc::new(larrygram_commands::builtins::registry().expect("the registry builds"));
    let mut save = item("qat.save", ItemKind::Button, "Save", Some("file.save"));
    save.dynamic_label = true;
    let mut undo = item("qat.undo", ItemKind::Split, "Undo", Some("edit.undo"));
    undo.dynamic_label = true;
    let layout = hand_built(Vec::new(), vec![save, undo]);
    let rig = Rig::build_on(
        Surface::QuickAccess,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 100.0),
        layout,
        ListData::new(),
        AppCtx::new(Settings::default()),
        Resolving::Registry(registry),
    );
    assert!(rig.has("Save"));
    assert!(
        rig.has("Can't Undo"),
        "the dynamic label falls back with no document"
    );
}

#[test]
fn the_file_view_closes_when_the_layout_has_none() {
    let layout = hand_built(
        vec![tab(
            "tab.one",
            "One",
            vec![group(
                "group.one",
                vec![item(
                    "one.solo",
                    ItemKind::Button,
                    "Solo",
                    Some("file.save"),
                )],
            )],
        )],
        Vec::new(),
    );
    let mut rig = Rig::build_on(
        Surface::Chrome,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 300.0),
        layout,
        ListData::new(),
        AppCtx::new(Settings::default()),
        Resolving::Test(Rc::new(always)),
    );
    rig.panel.borrow_mut().set_file_view_open(true);
    rig.run();
    assert!(
        !rig.panel.borrow().is_file_view_open(),
        "a layout with no File view closes it"
    );
}

#[test]
fn the_qat_split_menu_lists_the_history_or_its_items() {
    // The committed Undo arrow lists the undo stack: empty with no
    // document, so the "(none)" placeholder draws.
    let mut rig = Rig::build(
        Surface::QuickAccess,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 100.0),
    );
    rig.click(Role::Button, "More items");
    assert!(rig.has("(none)"), "an empty history shows the placeholder");
    // A split with child items lists the items instead.
    let mut split = item(
        "qat.split",
        ItemKind::Split,
        "Split Menu",
        Some("file.save"),
    );
    split.items = vec![item(
        "qat.child",
        ItemKind::Button,
        "Child One",
        Some("file.save"),
    )];
    let layout = hand_built(Vec::new(), vec![split]);
    let mut rig = Rig::build_on(
        Surface::QuickAccess,
        SelectionContext::EmptyPage,
        egui::vec2(800.0, 100.0),
        layout,
        ListData::new(),
        AppCtx::new(Settings::default()),
        Resolving::Test(Rc::new(always)),
    );
    rig.click(Role::Button, "More items");
    assert!(rig.has("Child One"), "the split lists its items");
    rig.clear();
    rig.click(Role::Button, "Child One");
    assert!(rig.intents().contains(&"file.save".to_owned()));
}

/// The kitchen-sink tab: every control kind and every menu-entry kind.
fn sink_tab() -> Tab {
    let check_item = item(
        "sink.check",
        ItemKind::Check,
        "Check It",
        Some("view.toggle_grid"),
    );
    let mut combo_options = item(
        "sink.combo.options",
        ItemKind::Combo,
        "Combo Options",
        Some("file.save"),
    );
    combo_options.options = vec!["Alpha".to_owned(), "Beta".to_owned()];
    let mut combo_items = item(
        "sink.combo.items",
        ItemKind::Combo,
        "Combo Items",
        Some("file.save"),
    );
    let mut menu_heading = item("sink.menu.heading", ItemKind::Heading, "Section", None);
    menu_heading.items = vec![item(
        "sink.menu.heading.child",
        ItemKind::Button,
        "Heading Child",
        None,
    )];
    let mut list_rows = item(
        "sink.menu.list",
        ItemKind::List,
        "Color List",
        Some("file.save"),
    );
    list_rows.list = Some("colors".to_owned());
    let mut list_empty = item(
        "sink.menu.list.empty",
        ItemKind::List,
        "Missing List",
        Some("file.save"),
    );
    list_empty.list = Some("missing".to_owned());
    let mut menu_options = item(
        "sink.menu.options",
        ItemKind::Options,
        "Options Entry",
        Some("file.save"),
    );
    menu_options.options = vec!["One".to_owned(), "Two".to_owned()];
    let mut fallback_options = item(
        "sink.menu.options.fallback",
        ItemKind::Options,
        "Fallback Options",
        Some("file.save"),
    );
    fallback_options.items = vec![item(
        "sink.menu.options.fallback.child",
        ItemKind::Button,
        "Fallback Child",
        Some("file.save"),
    )];
    let mut submenu = item("sink.menu.submenu", ItemKind::Submenu, "More", None);
    submenu.icon = Some("edit.copy".to_owned());
    submenu.shortcut = Some("Ctrl+M".to_owned());
    submenu.items = vec![item(
        "sink.menu.submenu.child",
        ItemKind::Button,
        "Deep One",
        Some("file.save"),
    )];
    let check_entry = item(
        "sink.menu.check",
        ItemKind::Check,
        "Check Entry",
        Some("view.toggle_grid"),
    );
    let toggle_entry = item(
        "sink.menu.toggle",
        ItemKind::Toggle,
        "Toggle Entry",
        Some("file.save"),
    );
    let mut menu_button = item(
        "sink.menu.button",
        ItemKind::Button,
        "Menu Button",
        Some("file.save"),
    );
    menu_button.icon = Some("edit.paste".to_owned());
    menu_button.shortcut = Some("Ctrl+V".to_owned());
    let stub_entry = item(
        "stub.entry",
        ItemKind::Button,
        "Stub Entry",
        Some("file.save"),
    );
    combo_items.items = vec![
        menu_heading,
        list_rows,
        list_empty,
        menu_options,
        fallback_options,
        submenu,
        check_entry,
        toggle_entry,
        menu_button,
        stub_entry,
    ];
    let mut gallery = item(
        "sink.gallery",
        ItemKind::Gallery,
        "Gallery One",
        Some("file.save"),
    );
    gallery.list = Some("colors".to_owned());
    gallery.items = vec![item(
        "sink.gallery.item",
        ItemKind::Button,
        "Gallery Item",
        Some("file.save"),
    )];
    let mut palette_item = item(
        "sink.palette",
        ItemKind::Palette,
        "Palette One",
        Some("file.save"),
    );
    palette_item.items = vec![item(
        "sink.palette.child",
        ItemKind::Button,
        "Palette Child",
        Some("file.save"),
    )];
    let mut control_heading = item(
        "sink.control.heading",
        ItemKind::Heading,
        "Control Heading",
        None,
    );
    control_heading.items = vec![item(
        "sink.control.heading.child",
        ItemKind::Button,
        "Control Child",
        Some("file.save"),
    )];
    // A nested chain that runs past the menu recursion cap.
    let mut deep = item("sink.deep.8", ItemKind::Heading, "Deep 8", None);
    for level in (0..8).rev() {
        let mut parent = item(
            &format!("sink.deep.{level}"),
            ItemKind::Heading,
            &format!("Deep {level}"),
            None,
        );
        parent.items = vec![deep];
        deep = parent;
    }
    // One control per column, so no rectangle covers another.
    let mut controls = vec![
        check_item,
        combo_options,
        combo_items,
        gallery,
        palette_item,
        control_heading,
        deep,
    ];
    for (column, control) in controls.iter_mut().enumerate() {
        control.column = Some(column as u32);
        control.row = Some(0);
    }
    tab("tab.sink", "Sink", vec![group("group.sink", controls)])
}

/// The run-time lists the kitchen-sink controls read.
fn sink_lists() -> ListData {
    let mut lists = ListData::new();
    lists.insert("colors", vec!["Red".to_owned(), "Blue".to_owned()]);
    lists
}

/// A rig drawing the kitchen-sink tab (a popup left open can cover
/// later widgets, so each test drives its own rig).
fn sink_rig() -> Rig {
    Rig::build_on(
        Surface::Chrome,
        SelectionContext::EmptyPage,
        egui::vec2(2000.0, 600.0),
        hand_built(vec![sink_tab()], Vec::new()),
        sink_lists(),
        AppCtx::new(Settings::default()),
        Resolving::Test(Rc::new(stub_out)),
    )
}

#[test]
fn display_only_and_menu_only_controls_draw() {
    let mut rig = sink_rig();
    // A check control toggles and dispatches.
    rig.clear();
    rig.click(Role::CheckBox, "Check It");
    assert!(rig.intents().contains(&"view.toggle_grid".to_owned()));
    rig.click(Role::CheckBox, "Check It");
    // The menu-only control kinds draw as labels with their children.
    assert!(rig.has("Control Child"), "a heading control draws children");
    assert!(rig.has("Deep 8"), "the recursion cap stops the chain");
}

#[test]
fn a_combo_popup_lists_its_fixed_options() {
    let mut rig = sink_rig();
    rig.click(Role::Button, "Combo Options");
    assert!(
        rig.has("Alpha"),
        "the combo lists its options in {:?}",
        rig.names()
    );
    assert!(rig.has("Beta"));
    rig.clear();
    rig.click_entry("Combo Options", "Alpha");
    assert!(rig.intents().contains(&"file.save".to_owned()));
}

#[test]
fn a_combo_menu_draws_every_entry_kind() {
    let mut rig = sink_rig();
    // The combo items menu draws every entry kind (menu-only kinds
    // render their rows, not their own label).
    rig.click(Role::Button, "Combo Items");
    for name in [
        "Section",
        "Red",
        "Blue",
        "(none)",
        "One",
        "Two",
        "Fallback Child",
        "Check Entry",
        "Toggle Entry",
        "Menu Button",
        "Stub Entry",
    ] {
        assert!(rig.has(name), "the menu draws {name:?}");
    }
    // The options entry with no options lists its children instead.
    rig.clear();
    rig.click_entry("Combo Items", "Fallback Child");
    assert!(rig.intents().contains(&"file.save".to_owned()));
    // A check entry latches on and off again.
    rig.click_entry("Combo Items", "Check Entry");
    rig.click_entry("Combo Items", "Check Entry");
    // A plain menu entry dispatches its command.
    rig.clear();
    rig.click_entry("Combo Items", "Menu Button");
    assert!(rig.intents().contains(&"file.save".to_owned()));
    // The submenu entry draws with its icon and shortcut, and opens on
    // hover.
    if !rig.has_text("More") {
        rig.click(Role::Button, "Combo Items");
    }
    assert!(rig.has_text("More"), "the submenu row draws");
    rig.hover_text("More");
    assert!(
        rig.has("Deep One"),
        "the submenu opens in {:?}",
        rig.names()
    );
}

#[test]
fn galleries_and_palettes_open_with_their_children() {
    let mut rig = sink_rig();
    rig.click(Role::Button, "Gallery One");
    assert!(
        rig.has("Red"),
        "the gallery lists its entries in {:?}",
        rig.names()
    );
    assert!(
        rig.has("Gallery Item"),
        "the gallery draws its menu children"
    );
    // A fresh rig: the gallery popup can cover the palette.
    let mut rig = sink_rig();
    rig.click(Role::Button, "Palette One");
    assert!(
        rig.has("Palette Child"),
        "the palette draws its children in {:?}",
        rig.names()
    );
}

#[test]
fn rows_groups_flow_left_to_right_in_their_half_row() {
    // A v1-adapter `rows` group: no slot data, so the controls flow
    // in their half row.
    let mut top = item("rows.a", ItemKind::Button, "A", None);
    top.begin_group = true;
    let controls = vec![top, item("rows.b", ItemKind::Button, "B", None), {
        let mut lower = item("rows.c", ItemKind::Button, "C", None);
        lower.row = Some(1);
        lower
    }];
    let group = Group {
        id: "rows".to_owned(),
        label: Some("Rows".to_owned()),
        layout: larrygram_ui_spec::v2::GroupLayout::Rows,
        rows: None,
        controls,
        launcher: None,
    };
    let metrics = Metrics::default();
    let placed = place::place(&group, &[42.0, 42.0, 42.0], &metrics);
    assert_eq!(placed.len(), 3);
    // The first two controls share half-row 0, left to right.
    assert_eq!(placed[0].rect.top(), placed[1].rect.top());
    assert!(placed[1].rect.left() > placed[0].rect.left());
    // The third sits in half-row 1, below.
    assert!(placed[2].rect.top() > placed[0].rect.top());
    // A rows group never makes a control large.
    assert!(!placed.iter().any(|p| p.large));
}

#[test]
fn the_mouse_wheel_over_the_tab_strip_cycles_tabs() {
    let mut rig = Rig::new();
    // The pointer rests on the tab strip and the wheel turns one
    // notch. A hover opens tooltips that keep repainting, so these
    // frames are stepped, not run.
    rig.harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(1950.0, 12.0)));
    rig.harness
        .input_mut()
        .events
        .push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
    rig.harness.step();
    assert_eq!(rig.panel.borrow().active_core, 1, "one notch moves one tab");
}

#[test]
fn the_overlay_closes_on_escape_and_on_a_click_outside() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "Collapse the command bar");
    // A click on a tab shows the groups as an overlay.
    rig.click(Role::Button, "Home");
    assert!(rig.has("Paste"), "the overlay draws the groups");
    rig.harness.key_press(egui::Key::Escape);
    rig.run();
    assert!(!rig.has("Paste"), "Esc closes the overlay");
    // A click outside the overlay and the tab strip closes it too.
    rig.click(Role::Button, "Home");
    assert!(rig.has("Paste"), "the overlay reopens");
    let far = egui::pos2(1900.0, 580.0);
    rig.harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(far));
    rig.harness
        .input_mut()
        .events
        .push(egui::Event::PointerButton {
            pos: far,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
    rig.harness
        .input_mut()
        .events
        .push(egui::Event::PointerButton {
            pos: far,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
    rig.run();
    assert!(!rig.has("Paste"), "a click outside closes the overlay");
}

#[test]
fn the_file_view_pages_draw_with_live_data() {
    let mut app = AppCtx::new(Settings::default());
    app.open_document(DocumentSession::new(Document::new(), None));
    app.settings
        .recent_files
        .push(PathBuf::from("/drawings/floor plan.vsdx"));
    let mut lists = ListData::new();
    lists.insert(
        "template_categories",
        vec!["Basic".to_owned(), "Flow".to_owned()],
    );
    let mut rig = Rig::build_on(
        Surface::Chrome,
        SelectionContext::EmptyPage,
        egui::vec2(2000.0, 600.0),
        committed(),
        lists,
        app,
        Resolving::Test(Rc::new(always)),
    );
    rig.click(Role::Button, "File");
    // Info reads the open document.
    rig.click(Role::Button, "Info");
    let names = rig.names();
    assert!(
        names.keys().any(|name| name.starts_with("Name: ")),
        "the Info page names the document"
    );
    assert!(
        names.keys().any(|name| name.starts_with("Pages: ")),
        "the Info page counts the pages"
    );
    // New lists the template tiles from the run-time list.
    rig.click(Role::Button, "New");
    assert!(rig.has("Basic"), "the New page lists the templates");
    // Open lists the recent files.
    rig.click(Role::Button, "Open");
    assert!(
        rig.has("1  floor plan.vsdx"),
        "the Open page lists the recent files"
    );
    // Save As draws its Browse entry.
    rig.click(Role::Button, "Save As");
    assert!(rig.has("Save As"), "the Save As page draws");
}
