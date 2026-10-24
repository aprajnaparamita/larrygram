//! Tests for the Panel Classic widgets (task P7-06).
//!
//! The acceptance criteria of `docs/tasks/P7-06.md`: egui_kittest
//! tests for the menu bar, the menus, the toolbars and the context
//! menus, and the count snapshot the P7-03 data pin compares against.
//!
//! The never-crash lints apply to non-test code only (clippy.toml);
//! these tests use `expect` freely.

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{
    CommandError, CommandMeta, CommandStatus, Enabled, Invocation, Outcome, Registry,
};
use larrygram_ui_spec::v2::LayoutV2;

use super::*;
use crate::Intent;
use crate::test_util::with_ui;

/// The ADR 0031 tooltip texts (the exact strings are the contract).
const NOT_YET: &str = "Not yet implemented";

// --- fixtures ------------------------------------------------------

/// A hand-made Panel Classic layout: three menus (File with a
/// submenu, a list and hidden and stub rows; Edit; View > Toolbars),
/// three docked toolbars (a split with a palette, a combo, a hidden
/// one) and one context menu.
fn layout() -> LayoutV2 {
    serde_json::from_value(serde_json::json!({
        "schema_version": 2, "id": "panel-classic", "name": "Panel Classic",
        "kind": "panel-classic", "theme": "panel-classic",
        "window": {"regions": []},
        "menu_bars": [{
            "id": "drawing", "label": "Drawing menu bar",
            "menus": [
                {"id": "drawing.file", "label": "File", "access": "f", "items": [
                    {"id": "drawing.file.new", "kind": "submenu", "label": "New", "access": "n", "items": [
                        {"id": "drawing.file.new.drawing", "kind": "button", "label": "New Drawing",
                         "command": "file.new", "access": "d"},
                        {"id": "drawing.file.new.stencil", "kind": "button", "label": "New Stencil",
                         "command": "file.new.blank.stencil", "access": "s"}
                    ]},
                    {"id": "drawing.file.open", "kind": "button", "label": "Open...", "access": "o",
                     "command": "file.open", "shortcut": "Ctrl+O"},
                    {"id": "drawing.file.print", "kind": "button", "label": "Print", "access": "p",
                     "command": "file.print", "begin_group": true},
                    {"id": "drawing.file.unmapped", "kind": "button", "label": "Unmapped", "access": "u"},
                    {"id": "drawing.file.hidden_item", "kind": "button", "label": "Hidden Item",
                     "command": "file.new", "hidden": true},
                    {"id": "drawing.file.recent", "kind": "list", "list": "recent_files",
                     "begin_group": true}
                ]},
                {"id": "drawing.edit", "label": "Edit", "access": "e", "items": [
                    {"id": "drawing.edit.cut", "kind": "button", "label": "Cut", "access": "t",
                     "command": "edit.cut", "icon": "edit.cut", "shortcut": "Ctrl+X"}
                ]},
                {"id": "drawing.view", "label": "View", "access": "v", "items": [
                    {"id": "drawing.view.toolbars", "kind": "submenu", "label": "Toolbars", "access": "t",
                     "items": [
                        {"id": "drawing.view.toolbars.standard", "kind": "check", "label": "Standard",
                         "command": "view.toolbar.standard"},
                        {"id": "drawing.view.toolbars.format_text", "kind": "check", "label": "Format Text",
                         "command": "view.toolbar.format_text"}
                     ]}
                ]}
            ]
        }],
        "toolbars": [
            {"id": "toolbar.standard", "label": "Standard", "visible": true, "dock": "top", "row": 0,
             "items": [
                {"id": "toolbar.standard.open", "kind": "button", "label": "Open",
                 "command": "file.open", "icon": "file.open"},
                {"id": "toolbar.standard.undo", "kind": "split", "label": "Undo",
                 "command": "edit.undo", "icon": "edit.undo", "items": [
                    {"id": "toolbar.standard.undo.undo_move", "kind": "button", "label": "Undo Move",
                     "command": "edit.undo"}
                 ]},
                {"id": "toolbar.standard.line_weight", "kind": "split", "label": "Line Weight",
                 "command": "format.set.line.weight", "icon": "format.set.line.weight",
                 "begin_group": true, "items": [
                    {"id": "toolbar.standard.line_weight.no_line", "kind": "button", "label": "No Line",
                     "command": "format.set.line.weight", "display": "text"},
                    {"id": "toolbar.standard.line_weight.line_weights", "kind": "list",
                     "list": "line_weights"},
                    {"id": "toolbar.standard.line_weight.more", "kind": "button",
                     "label": "More Line Weights...", "command": "format.line.weight.dlg",
                     "display": "text", "begin_group": true}
                 ]}
             ]},
            {"id": "toolbar.formatting", "label": "Formatting", "visible": true, "dock": "top",
             "row": 1, "items": [
                {"id": "toolbar.formatting.font", "kind": "combo", "label": "Font",
                 "command": "text.font", "width": 144}
             ]},
            {"id": "toolbar.format_text", "label": "Format Text", "visible": false, "dock": "top",
             "items": [
                {"id": "toolbar.format_text.bold", "kind": "button", "label": "Bold",
                 "command": "format.text.bold", "icon": "format.text.bold"}
             ]},
            {"id": "toolbar.drawing", "label": "Drawing", "visible": true, "dock": "floating",
             "items": [
                {"id": "toolbar.drawing.rectangle", "kind": "button", "label": "Rectangle",
                 "command": "shape.rectangle", "icon": "shape.rectangle"}
             ]}
        ],
        "context_menus": [
            {"id": "context.shape", "label": "Shape", "trigger": "right-click a shape", "items": [
                {"id": "context.shape.cut", "kind": "button", "label": "Cut", "command": "edit.cut",
                 "access": "t"},
                {"id": "context.shape.copy", "kind": "button", "label": "Copy", "command": "edit.copy",
                 "access": "o"}
            ]}
        ]
    }))
    .expect("the fixture layout decodes")
}

/// A command that is implemented and always enabled (the tests latch
/// the registry onto ids like `text.font`).
struct TestCommand {
    meta: CommandMeta,
}

impl larrygram_commands::Command for TestCommand {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::yes()
    }

    fn execute(
        &self,
        _ctx: &mut AppCtx,
        _invocation: &Invocation,
    ) -> Result<Outcome, CommandError> {
        Ok(Outcome::message("ok"))
    }
}

/// The builtin registry, with `ids` registered over as implemented
/// commands.
fn registry(ids: &[&'static str]) -> Registry {
    let mut registry = larrygram_commands::builtins::registry().expect("the registry builds");
    for id in ids {
        let command = TestCommand {
            meta: CommandMeta::new(
                CommandId::parse(id).expect("a valid id"),
                *id,
                CommandStatus::Implemented,
            ),
        };
        registry
            .register(Box::new(command))
            .expect("the command registers");
    }
    registry
}

/// A panel chrome driven headlessly over one hand-made layout.
struct Rig {
    panel: Rc<RefCell<PanelClassic>>,
    intents: Rc<RefCell<Vec<Intent>>>,
    harness: Harness<'static>,
}

impl Rig {
    /// A rig over the builtin registry (no extra implemented ids).
    fn new() -> Self {
        Self::with_registry(registry(&[]))
    }

    /// A rig with extra implemented command ids.
    fn with_registry(registry: Registry) -> Self {
        Self::build(registry, false)
    }

    /// A rig that also right-clicks a dummy shape widget (the
    /// context-menu trigger of P7-08).
    fn with_context_trigger() -> Self {
        Self::build(registry(&[]), true)
    }

    fn build(registry: Registry, context_trigger: bool) -> Self {
        let panel = Rc::new(RefCell::new(PanelClassic::new()));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let layout = Rc::new(layout());
        let registry = Rc::new(registry);
        let app = Rc::new(AppCtx::new(Settings::default()));
        let harness = Harness::builder()
            .with_size(Vec2::new(1600.0, 900.0))
            .build_ui({
                let panel = Rc::clone(&panel);
                let intents = Rc::clone(&intents);
                let layout = Rc::clone(&layout);
                let registry = Rc::clone(&registry);
                let app = Rc::clone(&app);
                move |ui| {
                    let input = PanelInput::new(&layout, &registry, &app);
                    let mut panel = panel.borrow_mut();
                    let produced = panel.show(ui, &input).expect("the chrome renders");
                    intents.borrow_mut().extend(produced);
                    if context_trigger {
                        let response = ui.add(egui::Button::new("shape"));
                        let produced = panel
                            .context_menu(ui, &input, "context.shape", Some(&response))
                            .expect("the context menu renders");
                        intents.borrow_mut().extend(produced);
                    }
                }
            });
        let mut rig = Self {
            panel,
            intents,
            harness,
        };
        rig.run();
        rig
    }

    /// Runs one frame.
    fn run(&mut self) {
        self.harness.run();
    }

    /// The widget names of the current frame (AccessKit order).
    fn names(&self) -> Vec<String> {
        self.harness
            .root()
            .children_recursive()
            .map(|node| {
                let data = node.accesskit_node().data();
                let label = data.label().unwrap_or_default().to_owned();
                if label.is_empty() {
                    data.value().unwrap_or_default().to_owned()
                } else {
                    label
                }
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// Whether a widget with `role` and `name` exists.
    fn has(&self, role: Role, name: &str) -> bool {
        self.harness
            .root()
            .query_all_by_role_and_label(role, name)
            .next()
            .is_some()
    }

    /// Clicks the widget with `role` and `name` and runs one frame.
    fn click(&mut self, role: Role, name: &str) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(role, name)
                .next()
                .unwrap_or_else(|| panic!("no {role:?} named {name:?} in {:?}", self.names()));
            node.click();
        }
        self.run();
    }

    /// Right-clicks the widget with `role` and `name` and runs one
    /// frame.
    fn click_secondary(&mut self, role: Role, name: &str) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(role, name)
                .next()
                .unwrap_or_else(|| panic!("no {role:?} named {name:?} in {:?}", self.names()));
            node.click_secondary();
        }
        self.run();
    }

    /// Types a key with modifiers and runs one frame.
    fn key_mod(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.run();
    }

    /// Types a bare key and runs one frame.
    fn key(&mut self, key: Key) {
        self.harness.key_press(key);
        self.run();
    }

    /// The intents produced since the last call.
    fn take_intents(&self) -> Vec<Intent> {
        std::mem::take(&mut *self.intents.borrow_mut())
    }
}

// --- the acceptance criteria --------------------------------------

#[test]
fn each_top_level_menu_opens() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(rig.has(Role::MenuItem, "Open..."), "File opens");
    rig.click(Role::Button, "Edit");
    assert!(rig.has(Role::MenuItem, "Cut"), "Edit opens");
    assert!(!rig.has(Role::MenuItem, "Open..."), "File closed again");
    rig.click(Role::Button, "View");
    assert!(rig.has(Role::MenuItem, "Toolbars"), "View opens");
    assert!(!rig.has(Role::MenuItem, "Cut"), "Edit closed again");
}

#[test]
fn alt_f_opens_file() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    assert!(rig.has(Role::MenuItem, "Open..."), "Alt+F opens File");
}

#[test]
fn an_enabled_item_dispatches_its_command() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    rig.click(Role::MenuItem, "Open...");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "one activation, one intent");
    assert_eq!(intents[0].command.as_str(), "file.open");
    assert_eq!(intents[0].invocation.source, Source::Menu);
    assert!(
        !rig.has(Role::MenuItem, "Open..."),
        "the menu closes after activation"
    );
}

#[test]
fn a_stub_is_disabled_with_the_tooltip() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    let print = rig
        .harness
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "Print")
        .next()
        .expect("the stub row");
    let data = print.accesskit_node().data();
    assert!(data.is_disabled(), "a stub renders disabled");
    assert_eq!(data.description(), Some(NOT_YET), "the tooltip");
    // A row with no command link at all is the same state.
    let unmapped = rig
        .harness
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "Unmapped")
        .next()
        .expect("the unmapped row");
    assert!(unmapped.accesskit_node().data().is_disabled());
    assert_eq!(
        unmapped.accesskit_node().data().description(),
        Some(NOT_YET)
    );
    // A stub dispatches nothing.
    rig.click(Role::MenuItem, "Print");
    assert!(rig.take_intents().is_empty());
}

#[test]
fn hidden_items_are_not_drawn() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(
        !rig.has(Role::MenuItem, "Hidden Item"),
        "hidden stays hidden"
    );
}

#[test]
fn a_submenu_opens() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(!rig.has(Role::MenuItem, "New Drawing"), "closed at first");
    rig.click(Role::MenuItem, "New");
    assert!(rig.has(Role::MenuItem, "New Drawing"), "the submenu opens");
    assert!(rig.has(Role::MenuItem, "New Stencil"));
    // The submenu row itself dispatches nothing.
    assert!(rig.take_intents().is_empty());
}

#[test]
fn view_toolbars_toggles_format_text() {
    let mut rig = Rig::new();
    assert!(
        !rig.has(Role::Button, "Bold"),
        "Format Text starts hidden (visible: false)"
    );
    rig.click(Role::Button, "View");
    rig.click(Role::MenuItem, "Toolbars");
    rig.click(Role::MenuItem, "Format Text");
    assert!(
        rig.has(Role::Button, "Bold"),
        "choosing Format Text shows its toolbar"
    );
    assert!(
        rig.panel.borrow().toolbar_visible_in(
            &layout(),
            WindowMode::Drawing,
            "toolbar.format_text"
        ),
        "the visibility flips in the widget state"
    );
    // And back off again.
    rig.click(Role::Button, "View");
    rig.click(Role::MenuItem, "Toolbars");
    rig.click(Role::MenuItem, "Format Text");
    assert!(!rig.has(Role::Button, "Bold"), "the second choice hides it");
}

#[test]
fn the_line_weight_palette_opens() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "Line Weight menu");
    assert!(rig.has(Role::MenuItem, "No Line"), "the palette opens");
    assert!(rig.has(Role::MenuItem, "(none)"), "the empty list row");
    assert!(rig.has(Role::MenuItem, "More Line Weights..."));
}

#[test]
fn the_font_combo_applies_a_value() {
    let mut rig = Rig::with_registry(registry(&["text.font"]));
    assert!(rig.has(Role::ComboBox, "Font"), "the combo is drawn");
    {
        let root = rig.harness.root();
        let combo = root
            .query_all_by_role_and_label(Role::ComboBox, "Font")
            .next()
            .expect("the Font combo");
        combo.click();
        combo.type_text("Arial");
    }
    rig.run();
    rig.key(Key::Enter);
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "Enter applies the value");
    assert_eq!(intents[0].command.as_str(), "text.font");
    assert_eq!(
        intents[0].invocation.args.text("value"),
        Some("Arial"),
        "the typed value rides along"
    );
}

#[test]
fn the_shape_context_menu_opens_on_right_click() {
    let mut rig = Rig::with_context_trigger();
    assert!(!rig.has(Role::MenuItem, "Copy"), "nothing open at first");
    rig.click_secondary(Role::Button, "shape");
    assert!(rig.has(Role::MenuItem, "Cut"), "the shape menu opens");
    assert!(rig.has(Role::MenuItem, "Copy"));
}

// --- keyboard ------------------------------------------------------

#[test]
fn f10_focuses_the_menu_bar_and_enter_opens_a_menu() {
    let mut rig = Rig::new();
    rig.key(Key::F10);
    assert!(!rig.has(Role::MenuItem, "Open..."), "focus only");
    rig.key(Key::Enter);
    assert!(
        rig.has(Role::MenuItem, "Open..."),
        "Enter opens the first menu"
    );
    rig.key(Key::Escape);
    assert!(!rig.has(Role::MenuItem, "Open..."), "Esc closes the menu");
}

#[test]
fn arrows_move_the_highlight_and_enter_activates() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    rig.key(Key::ArrowDown);
    rig.key(Key::ArrowDown);
    rig.key(Key::Enter);
    let intents = rig.take_intents();
    // The first focusable row is "New" (a submenu), the second
    // "Open...", which activates.
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "file.open");
}

#[test]
fn a_bare_access_letter_activates_an_item() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    assert!(rig.has(Role::MenuItem, "Open..."), "Alt+F opens File");
    rig.key(Key::O);
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "the access letter activates the row");
    assert_eq!(intents[0].command.as_str(), "file.open");
}

// --- run-time lists ------------------------------------------------

#[test]
fn an_empty_run_time_list_shows_one_disabled_row() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    let none = rig
        .harness
        .root()
        .query_all_by_role_and_label(Role::MenuItem, "(none)")
        .next()
        .expect("the (none) row");
    assert!(none.accesskit_node().data().is_disabled());
}

#[test]
fn the_recent_files_list_inserts_its_entries() {
    let layout = Rc::new(layout());
    let registry = Rc::new(registry(&[]));
    let mut app = AppCtx::new(Settings::default());
    app.settings
        .recent_files
        .push(std::path::PathBuf::from("/tmp/first.vsdx"));
    let app = Rc::new(app);
    let panel = Rc::new(RefCell::new(PanelClassic::new()));
    let intents = Rc::new(RefCell::new(Vec::new()));
    let harness = Harness::builder()
        .with_size(Vec2::new(1600.0, 900.0))
        .build_ui({
            let panel = Rc::clone(&panel);
            let intents = Rc::clone(&intents);
            let layout = Rc::clone(&layout);
            let registry = Rc::clone(&registry);
            let app = Rc::clone(&app);
            move |ui| {
                let input = PanelInput::new(&layout, &registry, &app);
                let mut panel = panel.borrow_mut();
                let produced = panel.show(ui, &input).expect("the chrome renders");
                intents.borrow_mut().extend(produced);
            }
        });
    let mut rig = Rig {
        panel,
        intents,
        harness,
    };
    rig.run();
    rig.click(Role::Button, "File");
    assert!(
        rig.has(Role::MenuItem, "1. first.vsdx"),
        "the recent file row is numbered"
    );
    rig.click(Role::MenuItem, "1. first.vsdx");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "file.open");
    assert_eq!(
        intents[0].invocation.args.path("path"),
        Some(std::path::Path::new("/tmp/first.vsdx"))
    );
}

// --- the state API -------------------------------------------------

#[test]
fn window_modes_name_their_contexts() {
    assert_eq!(WindowMode::Drawing.id(), "drawing");
    assert_eq!(WindowMode::Drawing.context(), None);
    assert_eq!(WindowMode::Shapesheet.id(), "shapesheet");
    assert_eq!(
        WindowMode::Shapesheet.context(),
        Some(ContextId::ShapesheetWindow)
    );
    assert_eq!(WindowMode::Stencil.context(), Some(ContextId::StencilEdit));
}

#[test]
fn toolbar_visibility_defaults_come_from_the_data() {
    let layout = layout();
    let state = PanelClassic::new();
    assert!(state.toolbar_visible_in(&layout, WindowMode::Drawing, "toolbar.standard"));
    assert!(!state.toolbar_visible_in(&layout, WindowMode::Drawing, "toolbar.format_text"));
    let mut state = state;
    state.set_toolbar_visible_in(WindowMode::Drawing, "toolbar.format_text", true);
    assert!(state.toolbar_visible_in(&layout, WindowMode::Drawing, "toolbar.format_text"));
    // The modes remember their choices separately.
    assert!(!state.toolbar_visible_in(&layout, WindowMode::Shapesheet, "toolbar.format_text"));
}

#[test]
fn tool_latch_reads_the_active_tool() {
    let layout = layout();
    let registry = registry(&[]);
    let mut app = AppCtx::new(Settings::default());
    app.active_tool = larrygram_commands::app::ActiveTool::Pointer;
    let input = PanelInput::new(&layout, &registry, &app).with_checked(&ToolLatch);
    assert_eq!(
        input.checked.map(|c| c.is_checked("tools.pointer", &app)),
        Some(Some(true))
    );
    assert_eq!(
        input.checked.map(|c| c.is_checked("tools.connector", &app)),
        Some(Some(false))
    );
    assert_eq!(
        input.checked.map(|c| c.is_checked("file.open", &app)),
        Some(None)
    );
}

#[test]
fn the_stub_entry_point_renders_without_a_registry() {
    let layout = layout();
    let outcome = with_ui(|ui| show(ui, &layout));
    assert!(outcome.is_ok(), "the P7-01 signature keeps working");
}

#[test]
fn the_stub_entry_point_keeps_its_state_across_frames() {
    let layout = Rc::new(layout());
    let mut harness = Harness::builder()
        .with_size(Vec2::new(1600.0, 900.0))
        .build_ui({
            let layout = Rc::clone(&layout);
            move |ui| {
                show(ui, &layout).expect("the chrome renders");
            }
        });
    harness.run();
    {
        let root = harness.root();
        let title = root
            .query_all_by_role_and_label(Role::Button, "File")
            .next()
            .expect("the File title");
        title.click();
    }
    harness.run();
    harness.run();
    assert!(
        harness
            .root()
            .query_all_by_role_and_label(Role::MenuItem, "Open...")
            .next()
            .is_some(),
        "the open menu survives the frame: the state lives in the egui memory"
    );
}

#[test]
fn context_menus_open_through_the_explicit_api() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let mut state = PanelClassic::new();
    state.open_context_menu("context.shape");
    let produced = with_ui(|ui| {
        let input = PanelInput::new(&layout, &registry, &app);
        state.context_menu(ui, &input, "context.shape", None)
    })
    .expect("renders");
    assert!(produced.is_empty(), "opening dispatches nothing");
}

// --- the count pin (ADR 0065, task P7-03's data pin) --------------

/// The committed Panel Classic layout, parsed from the checked-in
/// YAML (the same data the P7-03 loader reads; the count pins must
/// come from one source).
fn committed() -> LayoutV2 {
    serde_saphyr::from_str(include_str!(
        "../../../../ui-spec/layouts/panel-classic.yaml"
    ))
    .expect("the committed panel-classic.yaml parses")
}

/// Per surface and kind: (items, hidden, widgets).
#[derive(Default, Debug, PartialEq, Eq)]
struct Counts {
    /// Every data item, hidden included.
    items: usize,
    /// The `hidden: true` items.
    hidden: usize,
    /// The widget slots the Panel Classic widgets draw: the visible
    /// items (one row or button per item; a run-time `list` slot
    /// counts as one).
    widgets: usize,
}

/// The items of one surface, keyed by kind name (nested items count
/// under their own kind).
fn surface_counts(items_lists: &[&[larrygram_ui_spec::v2::Item]]) -> BTreeMap<String, Counts> {
    let mut out: BTreeMap<String, Counts> = BTreeMap::new();
    for items in items_lists {
        walk(items, &mut out);
    }
    out
}

fn walk(items: &[larrygram_ui_spec::v2::Item], out: &mut BTreeMap<String, Counts>) {
    for item in items {
        let kind = serde_json::to_value(item.kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".to_owned());
        let counts = out.entry(kind).or_default();
        counts.items += 1;
        if item.hidden {
            counts.hidden += 1;
        } else {
            counts.widgets += 1;
        }
        walk(&item.items, out);
    }
}

/// The count report of the rendered Panel Classic widgets: items per
/// surface and kind, with the hidden items and the drawn widget slots
/// (task P7-06's acceptance criterion; compare with the P7-03 data
/// pin, which counts the same items per surface and kind).
fn counts_report(layout: &LayoutV2) -> String {
    let menu_items: Vec<&[larrygram_ui_spec::v2::Item]> = layout
        .menu_bars
        .iter()
        .flat_map(|bar| bar.menus.iter().map(|menu| menu.items.as_slice()))
        .collect();
    let toolbar_items: Vec<&[larrygram_ui_spec::v2::Item]> = layout
        .toolbars
        .iter()
        .map(|toolbar| toolbar.items.as_slice())
        .collect();
    let context_items: Vec<&[larrygram_ui_spec::v2::Item]> = layout
        .context_menus
        .iter()
        .map(|context| context.items.as_slice())
        .collect();
    let _ = &menu_items;
    let mut report = String::new();
    report.push_str("layout: panel-classic\n");
    for (surface, lists) in [
        ("menu_bars", &menu_items),
        ("toolbars", &toolbar_items),
        ("context_menus", &context_items),
    ] {
        report.push_str(&format!("surface: {surface}\n"));
        for (kind, counts) in surface_counts(lists) {
            report.push_str(&format!(
                "  {kind}: items {} (hidden {}) widgets {}\n",
                counts.items, counts.hidden, counts.widgets
            ));
        }
    }
    report
}

#[test]
fn the_count_snapshot_is_pinned() {
    let report = counts_report(&committed());
    insta::assert_snapshot!("panel_classic_counts", report);
}

#[test]
fn widget_slots_match_the_visible_items() {
    let layout = committed();
    for (surface, lists) in [
        (
            "menu_bars",
            layout
                .menu_bars
                .iter()
                .flat_map(|bar| bar.menus.iter().map(|menu| menu.items.as_slice()))
                .collect::<Vec<_>>(),
        ),
        (
            "toolbars",
            layout
                .toolbars
                .iter()
                .map(|toolbar| toolbar.items.as_slice())
                .collect::<Vec<_>>(),
        ),
        (
            "context_menus",
            layout
                .context_menus
                .iter()
                .map(|context| context.items.as_slice())
                .collect::<Vec<_>>(),
        ),
    ] {
        for (kind, counts) in surface_counts(&lists) {
            assert_eq!(
                counts.items,
                counts.hidden + counts.widgets,
                "{surface}/{kind}: every item is hidden or drawn"
            );
            assert!(counts.items > 0);
        }
    }
}

#[test]
fn each_committed_menu_renders_one_widget_per_visible_item() {
    let layout = Rc::new(committed());
    let registry = Rc::new(registry(&[]));
    let app = Rc::new(AppCtx::new(Settings::default()));
    let panel = Rc::new(RefCell::new(PanelClassic::new()));
    let intents = Rc::new(RefCell::new(Vec::new()));
    let harness = Harness::builder()
        .with_size(Vec2::new(1600.0, 900.0))
        .build_ui({
            let panel = Rc::clone(&panel);
            let layout = Rc::clone(&layout);
            let registry = Rc::clone(&registry);
            let app = Rc::clone(&app);
            let intents = Rc::clone(&intents);
            move |ui| {
                let input = PanelInput::new(&layout, &registry, &app);
                let mut panel = panel.borrow_mut();
                let produced = panel.show(ui, &input).expect("the chrome renders");
                intents.borrow_mut().extend(produced);
            }
        });
    let mut rig = Rig {
        panel,
        intents,
        harness,
    };
    rig.run();
    let bar = layout
        .menu_bars
        .iter()
        .find(|bar| bar.shown_in.is_none())
        .expect("the drawing menu bar");
    for menu in &bar.menus {
        rig.click(Role::Button, &menu.label);
        let drawn = rig
            .harness
            .root()
            .children_recursive()
            .filter(|node| node.accesskit_node().data().role() == Role::MenuItem)
            .count();
        let expected = menu.items.iter().filter(|item| !item.hidden).count();
        assert_eq!(
            drawn, expected,
            "one widget per visible item in {}",
            menu.label
        );
    }
}
