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
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{CommandId, Invocation, Source};
use larrygram_ui_spec::v2::{ControlSize, Group, Item, ItemKind, LayoutV2, PanelStyle, Tab};

use super::common::Action;
use super::place::{self, Placed};
use super::{ListData, PanelModern};
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
        let panel = Rc::new(RefCell::new(PanelModern::new()));
        let context = Rc::new(RefCell::new(context));
        let intents: Rc<RefCell<Vec<Intent>>> = Rc::new(RefCell::new(Vec::new()));
        let layout: Rc<LayoutV2> = Rc::new(committed());
        let lists: Rc<ListData> = Rc::new(ListData::new());
        let app: Rc<AppCtx> = Rc::new(AppCtx::new(Settings::default()));
        let harness = Harness::builder().with_size(size).build_ui({
            let panel = Rc::clone(&panel);
            let context = Rc::clone(&context);
            let intents = Rc::clone(&intents);
            let layout = Rc::clone(&layout);
            let lists = Rc::clone(&lists);
            let app = Rc::clone(&app);
            move |ui| {
                let mut panel = panel.borrow_mut();
                let produced: Result<Vec<Intent>, UiError> = match surface {
                    Surface::Chrome => {
                        panel.show_with(ui, &layout, *context.borrow(), &app, &lists, &always)
                    }
                    Surface::QuickAccess => panel.show_qat_with(ui, &layout, &app, &always),
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
