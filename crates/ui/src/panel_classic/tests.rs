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
use crate::lists::ListEntry;
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
                     "begin_group": true},
                    {"id": "drawing.file.long_name", "kind": "button",
                     "label": "A Command With A Rather Long Name",
                     "command": "file.open", "shortcut": "Ctrl+Shift+Alt+W"}
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
                 "command": "text.font", "width": 144},
                {"id": "toolbar.formatting.notes", "kind": "button", "label": "Notes",
                 "command": "file.open", "display": "text"},
                {"id": "toolbar.formatting.colors", "kind": "dropdown", "label": "Colors",
                 "command": "format.set.fill.color", "icon": "format.set.fill.color",
                 "begin_group": true, "items": [
                    {"id": "toolbar.formatting.colors.title", "kind": "heading",
                     "label": "Recent Colors"},
                    {"id": "toolbar.formatting.colors.custom", "kind": "submenu", "label": "Custom",
                     "items": [
                        {"id": "toolbar.formatting.colors.custom.more", "kind": "button",
                         "label": "More Colors...", "command": "format.set.fill.color"}
                     ]},
                    {"id": "toolbar.formatting.colors.none", "kind": "button", "label": "No Fill",
                     "command": "format.set.fill.color"}
                ]},
                {"id": "toolbar.formatting.recent", "kind": "split_recent", "label": "Recent",
                 "command": "file.open", "items": [
                    {"id": "toolbar.formatting.recent.a", "kind": "button", "label": "Recent A",
                     "command": "file.open"}
                ]}
             ]},
            {"id": "toolbar.format_text", "label": "Format Text", "visible": false, "dock": "top",
             "items": [
                {"id": "toolbar.format_text.bold", "kind": "button", "label": "Bold",
                 "command": "format.text.bold", "icon": "format.text.bold"}
             ]},
            {"id": "toolbar.drawing", "label": "Drawing", "visible": true, "dock": "floating",
             "items": [
                {"id": "toolbar.drawing.rectangle", "kind": "button", "label": "Rectangle",
                 "command": "shape.rectangle", "icon": "shape.rectangle"},
                {"id": "toolbar.drawing.hidden_tool", "kind": "button", "label": "Hidden Tool",
                 "command": "shape.rectangle", "hidden": true}
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

    /// A rig in a window of `size` (the tests that pin the off-screen
    /// flips).
    fn sized(size: Vec2) -> Self {
        Self::build_sized(registry(&[]), false, size, AppCtx::new(Settings::default()))
    }

    /// A rig that also right-clicks a dummy shape widget, with a
    /// selected shape (the selection commands are enabled).
    fn with_context_selection() -> Self {
        let mut app = AppCtx::new(Settings::default());
        app.selection
            .shapes
            .push(larrygram_model::ShapeId { index: 1 });
        Self::build_sized(registry(&[]), true, Vec2::new(1600.0, 900.0), app)
    }

    fn build(registry: Registry, context_trigger: bool) -> Self {
        Self::build_sized(
            registry,
            context_trigger,
            Vec2::new(1600.0, 900.0),
            AppCtx::new(Settings::default()),
        )
    }

    fn build_sized(registry: Registry, context_trigger: bool, size: Vec2, app: AppCtx) -> Self {
        let panel = Rc::new(RefCell::new(PanelClassic::new()));
        let intents = Rc::new(RefCell::new(Vec::new()));
        let layout = Rc::new(layout());
        let registry = Rc::new(registry);
        let app = Rc::new(app);
        let harness = Harness::builder().with_size(size).build_ui({
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
        self.settle();
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
        self.settle();
    }

    /// Runs the frame plus the settle frames: egui resolves a pointer
    /// press and release into a click a frame or two later, so the
    /// widgets need follow-up frames to dispatch it.
    fn settle(&mut self) {
        self.run();
        self.harness.run_steps(2);
    }

    /// Moves the pointer away from every widget and runs one frame
    /// (the submenu hover timer must not act during the test).
    fn away(&mut self) {
        self.harness.hover_at(egui::Pos2::new(1200.0, 820.0));
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
    assert_eq!(WindowMode::Stencil.id(), "stencil");
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

// --- the row resolution (unit) ------------------------------------

/// One hand-made item, decoded like the fixture.
fn item(json: serde_json::Value) -> Item {
    serde_json::from_value(json).expect("the item decodes")
}

/// A plain row for the state-machine tests.
fn row(id: &str, children: Vec<Row>) -> Row {
    Row {
        id: id.to_owned(),
        label: id.to_owned(),
        access: None,
        shortcut: None,
        icon: None,
        check: None,
        tooltip: String::new(),
        activation: if children.is_empty() {
            Activation::Disabled {
                reason: String::new(),
            }
        } else {
            Activation::Submenu
        },
        children,
        separator: false,
    }
}

/// A heading row (the non-focusable kind).
fn heading_row(id: &str) -> Row {
    Row {
        activation: Activation::Heading,
        tooltip: String::new(),
        ..row(id, Vec::new())
    }
}

#[test]
fn run_time_list_entries_without_a_command_render_disabled() {
    struct Provider;
    impl ListProvider for Provider {
        fn list(&self, name: &str, _app: &AppCtx) -> Vec<ListEntry> {
            match name {
                "windows" => vec![ListEntry::new("drawing-1")],
                _ => Vec::new(),
            }
        }
    }
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app).with_lists(&Provider);
    let items = vec![item(
        serde_json::json!({"id": "menu.windows", "kind": "list", "list": "windows"}),
    )];
    let rows = rows::rows(&items, &input, &PanelClassic::new(), Source::Menu, false, 0);
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0].activation, Activation::Disabled { .. }));
    assert_eq!(rows[0].tooltip, NOT_YET_IMPLEMENTED);
}

#[test]
fn option_rows_carry_their_value_and_a_stub_renders_disabled() {
    let layout = layout();
    let registry = registry(&["text.font"]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let items = vec![
        item(
            serde_json::json!({"id": "menu.sizes", "kind": "options", "options": ["8 pt", "10 pt"],
                    "command": "text.font"}),
        ),
        item(serde_json::json!({"id": "menu.plain", "kind": "options", "options": ["A"]})),
    ];
    let rows = rows::rows(&items, &input, &PanelClassic::new(), Source::Menu, false, 0);
    assert_eq!(rows.len(), 3);
    match &rows[0].activation {
        Activation::Intent(intent) => {
            assert_eq!(intent.command.as_str(), "text.font");
            assert_eq!(intent.invocation.args.text("value"), Some("8 pt"));
        }
        other => panic!("expected an intent, got {other:?}"),
    }
    assert_eq!(rows[0].id, "menu.sizes.0");
    assert!(matches!(rows[2].activation, Activation::Disabled { .. }));
    assert_eq!(rows[2].tooltip, NOT_YET_IMPLEMENTED);
}

#[test]
fn heading_rows_are_not_focusable_and_carry_no_tooltip() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let items = vec![item(
        serde_json::json!({"id": "menu.head", "kind": "heading", "label": "Options"}),
    )];
    let rows = rows::rows(&items, &input, &PanelClassic::new(), Source::Menu, false, 0);
    assert!(!rows[0].focusable(), "a heading takes no keyboard focus");
    assert_eq!(rows[0].activation, Activation::Heading);
    assert_eq!(rows[0].tooltip, "");
    assert_eq!(
        rows::button_tooltip("Options", None, &Activation::Heading),
        ""
    );
}

#[test]
fn an_out_of_scope_control_renders_disabled() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let items = vec![item(
        serde_json::json!({"id": "menu.coauthor", "kind": "button", "label": "Coauthor",
                                 "fluent": "CoauthorMenu", "command": "file.open"}),
    )];
    let rows = rows::rows(&items, &input, &PanelClassic::new(), Source::Menu, false, 0);
    assert!(matches!(rows[0].activation, Activation::Disabled { .. }));
    assert_eq!(rows[0].tooltip, OUT_OF_SCOPE);
}

#[test]
fn a_toolbars_row_without_a_command_still_toggles() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let items = vec![item(
        serde_json::json!({"id": "view.toolbars.custom", "kind": "check", "label": "Custom"}),
    )];
    let rows = rows::rows(&items, &input, &PanelClassic::new(), Source::Menu, true, 0);
    assert!(
        matches!(
            rows[0].activation,
            Activation::ToolbarToggle { intent: None, .. }
        ),
        "a row with no command toggles without dispatching"
    );
}

#[test]
fn the_input_builders_set_the_mode_lists_and_checked_state() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app)
        .with_mode(WindowMode::Stencil)
        .with_lists(&LiveLists)
        .with_checked(&NoChecks);
    assert_eq!(input.mode.id(), "stencil");
    let check = item(
        serde_json::json!({"id": "menu.check", "kind": "check", "label": "Check",
                            "command": "file.open"}),
    );
    assert_eq!(
        rows::item_check(&check, &input, &PanelClassic::new(), false),
        None,
        "NoChecks reports no checked state"
    );
}

#[test]
fn build_levels_survives_a_stale_open_menu_and_chain() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let mut state = PanelClassic::new();
    state.open = Some(99);
    assert!(
        state.build_levels(&input).is_empty(),
        "an open index beyond the bar builds nothing"
    );
    state.open = Some(0);
    state.chain = vec![99];
    assert_eq!(
        state.build_levels(&input).len(),
        1,
        "a stale chain link stops the walk"
    );
}

#[test]
fn hovering_a_leaf_closes_the_deeper_levels() {
    let levels = vec![vec![
        row("leaf", Vec::new()),
        row("parent", vec![row("child", Vec::new())]),
    ]];
    let mut state = PanelClassic::new();
    state.open = Some(0);
    state.chain = vec![1];
    state.highlight = vec![Some(1), Some(0)];
    with_ui(|ui| state.update_hover(ui, &levels, Some((0, 0))));
    assert!(state.chain.is_empty(), "the hover closes the submenu");
    assert!(state.hover_timer.is_none());
    // Hovering the row whose submenu is already open keeps it open.
    state.chain = vec![1, 0];
    state.hover_timer = Some((0, 1, 0.0));
    with_ui(|ui| state.update_hover(ui, &levels, Some((0, 1))));
    assert_eq!(state.chain, vec![1, 0], "the open submenu stays");
    assert!(state.hover_timer.is_none());
}

#[test]
fn move_highlight_skips_headings_and_walks_both_ways() {
    let mut state = PanelClassic::new();
    state.open = Some(0);
    let levels = vec![vec![heading_row("head")]];
    state.move_highlight(&levels, true);
    assert!(
        state.highlight.is_empty(),
        "a level without focusable rows keeps the highlight"
    );
    let levels = vec![vec![row("a", Vec::new()), row("b", Vec::new())]];
    state.highlight = Vec::new();
    state.move_highlight(&levels, false);
    assert_eq!(state.highlight, vec![Some(1)], "ArrowUp walks upwards");
    state.move_highlight(&levels, true);
    assert_eq!(state.highlight, vec![Some(0)], "and wraps around");
    // No open menu: the arrows do nothing.
    let mut closed = PanelClassic::new();
    closed.move_highlight(&levels, true);
    assert!(closed.highlight.is_empty(), "nothing is open");
}

#[test]
fn an_unknown_context_menu_clears_the_pending_target() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let input = PanelInput::new(&layout, &registry, &app);
    let mut state = PanelClassic::new();
    state.open_context_menu("context.nope");
    let produced = with_ui(|ui| state.context_menu(ui, &input, "context.nope", None))
        .expect("the chrome renders");
    assert!(produced.is_empty());
    assert!(state.context.is_none(), "the pending menu is cleared");
}

#[test]
fn a_checked_icon_row_paints_its_frame_and_an_empty_label_lays_out() {
    let mut checked_icon = row("checked", Vec::new());
    checked_icon.icon = Some("file.open".to_owned());
    checked_icon.check = Some(true);
    let empty = row("", Vec::new());
    let outcome = with_ui(|ui| menu::draw_menu(ui, &[checked_icon, empty], 200.0, None, true));
    assert_eq!(outcome.clicked, None, "a bare draw dispatches nothing");
}

// --- the toolbar widgets ------------------------------------------

#[test]
fn a_label_only_toolbar_button_dispatches_its_command() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "Notes");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "one click, one intent");
    assert_eq!(intents[0].command.as_str(), "file.open");
    assert_eq!(
        intents[0].invocation.source,
        Source::Ui,
        "toolbar activations come from the UI"
    );
}

#[test]
fn a_right_click_on_a_toolbar_button_opens_the_toolbar_list() {
    let mut rig = Rig::new();
    rig.click_secondary(Role::Button, "Notes");
    assert!(rig.has(Role::MenuItem, "Standard"), "one row per toolbar");
    assert!(rig.has(Role::MenuItem, "Format Text"));
    rig.click(Role::MenuItem, "Format Text");
    assert!(
        rig.has(Role::Button, "Bold"),
        "the list toggles the visibility like View > Toolbars"
    );
}

#[test]
fn a_right_click_on_a_split_arrow_or_a_dropdown_opens_the_toolbar_list() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click_secondary(Role::Button, "Line Weight menu");
    assert!(
        rig.has(Role::MenuItem, "Standard"),
        "the split arrow opens it"
    );
    rig.key(Key::Escape);
    assert!(!rig.has(Role::MenuItem, "Standard"), "Esc closes the list");
    rig.click_secondary(Role::Button, "Colors");
    assert!(rig.has(Role::MenuItem, "Standard"), "the dropdown opens it");
}

#[test]
fn the_colour_dropdown_opens_and_picks_a_value() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click(Role::Button, "Colors");
    assert!(rig.has(Role::MenuItem, "Custom"), "the dropdown opens");
    assert!(rig.has(Role::MenuItem, "No Fill"));
    rig.click(Role::MenuItem, "No Fill");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "picking a row dispatches it");
    assert_eq!(intents[0].command.as_str(), "format.set.fill.color");
    assert!(!rig.has(Role::MenuItem, "No Fill"), "and closes the popup");
}

#[test]
fn hovering_a_popup_row_with_children_opens_its_submenu() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click(Role::Button, "Colors");
    {
        let root = rig.harness.root();
        let custom = root
            .query_all_by_role_and_label(Role::MenuItem, "Custom")
            .next()
            .expect("the Custom row");
        custom.hover();
    }
    rig.run();
    assert!(
        rig.has(Role::MenuItem, "More Colors..."),
        "the hover opens the child level"
    );
}

#[test]
fn an_outside_click_closes_the_toolbar_popup() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click(Role::Button, "Colors");
    assert!(rig.has(Role::MenuItem, "No Fill"));
    rig.click(Role::Button, "File");
    assert!(!rig.has(Role::MenuItem, "No Fill"), "the click closes it");
}

#[test]
fn clicking_the_dropdown_again_closes_its_popup() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click(Role::Button, "Colors");
    assert!(rig.has(Role::MenuItem, "No Fill"));
    rig.click(Role::Button, "Colors");
    assert!(!rig.has(Role::MenuItem, "No Fill"), "the toggle closes it");
}

#[test]
fn a_split_recent_remembers_the_last_choice() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "Recent menu");
    rig.click(Role::MenuItem, "Recent A");
    let picked = rig.take_intents();
    assert_eq!(picked.len(), 1, "the pick itself runs the child");
    assert_eq!(picked[0].command.as_str(), "file.open");
    assert!(
        rig.has(Role::Button, "Recent A"),
        "the main part shows the last choice"
    );
    rig.click(Role::Button, "Recent A");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "and runs it");
    assert_eq!(intents[0].command.as_str(), "file.open");
}

#[test]
fn escape_on_a_combo_reverts_to_the_applied_value() {
    let mut rig = Rig::with_registry(registry(&["text.font"]));
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
    {
        let root = rig.harness.root();
        let combo = root
            .query_all_by_role_and_label(Role::ComboBox, "Font")
            .next()
            .expect("the Font combo");
        combo.click();
        combo.type_text("Bogus");
    }
    rig.run();
    rig.key(Key::Escape);
    assert_eq!(
        rig.panel
            .borrow()
            .combo_text
            .get("toolbar.formatting.font")
            .map(String::as_str),
        Some("Arial"),
        "Esc reverts the buffer to the applied value"
    );
}

#[test]
fn closing_the_floating_toolbar_hides_it() {
    let mut rig = Rig::new();
    assert!(rig.has(Role::Button, "Rectangle"), "the floating toolbar");
    assert!(
        !rig.has(Role::Button, "Hidden Tool"),
        "its hidden item is not drawn"
    );
    rig.click(Role::Button, "Close window");
    assert!(!rig.has(Role::Button, "Rectangle"), "closing hides it");
    assert!(
        !rig.panel
            .borrow()
            .toolbar_visible_in(&layout(), WindowMode::Drawing, "toolbar.drawing"),
        "and the visibility flips in the state"
    );
}

// --- the menu bar keyboard ----------------------------------------

#[test]
fn a_bare_alt_tap_focuses_the_menu_bar() {
    let mut rig = Rig::new();
    rig.harness
        .event(egui::Event::ModifiersChanged(Modifiers::ALT));
    rig.harness.event(egui::Event::Key {
        key: Key::AltLeft,
        pressed: true,
        modifiers: Modifiers::ALT,
        repeat: false,
        physical_key: None,
    });
    rig.run();
    rig.harness.event(egui::Event::Key {
        key: Key::AltLeft,
        pressed: false,
        modifiers: Modifiers::NONE,
        repeat: false,
        physical_key: None,
    });
    rig.harness
        .event(egui::Event::ModifiersChanged(Modifiers::NONE));
    rig.run();
    rig.key(Key::Enter);
    assert!(
        rig.has(Role::MenuItem, "Open..."),
        "the tap focused the menu bar"
    );
}

#[test]
fn shift_f10_opens_the_mapped_context_menu() {
    let mut rig = Rig::with_context_trigger();
    rig.panel.borrow_mut().set_context_target("context.shape");
    rig.key_mod(Modifiers::SHIFT, Key::F10);
    assert!(
        rig.has(Role::MenuItem, "Cut"),
        "Shift+F10 opens the mapped menu"
    );
}

#[test]
fn escape_closes_the_deepest_open_level_first() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    rig.click(Role::MenuItem, "New");
    assert!(rig.has(Role::MenuItem, "New Drawing"));
    rig.away();
    assert!(rig.has(Role::MenuItem, "New Drawing"), "the hover keeps it");
    rig.key(Key::Escape);
    assert!(
        !rig.has(Role::MenuItem, "New Drawing"),
        "the submenu closes"
    );
    assert!(rig.has(Role::MenuItem, "Open..."), "the menu stays open");
}

#[test]
fn escape_leaves_the_menu_bar() {
    let mut rig = Rig::new();
    rig.key(Key::F10);
    rig.key(Key::Escape);
    rig.key(Key::Enter);
    assert!(
        !rig.has(Role::MenuItem, "Open..."),
        "Enter opens nothing once the bar is unfocused"
    );
}

#[test]
fn arrow_left_closes_a_submenu_then_cycles_the_titles() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    rig.click(Role::MenuItem, "New");
    rig.away();
    rig.key(Key::ArrowLeft);
    assert!(
        !rig.has(Role::MenuItem, "New Drawing"),
        "the submenu closes"
    );
    assert!(rig.has(Role::MenuItem, "Open..."), "the menu stays open");
    // With no submenu open, ArrowLeft moves to the previous title.
    rig.key(Key::ArrowLeft);
    assert!(
        rig.has(Role::MenuItem, "Toolbars"),
        "the previous title is View"
    );
}

#[test]
fn arrows_move_between_the_titles_when_the_bar_is_focused() {
    let mut rig = Rig::new();
    rig.key(Key::F10);
    rig.key(Key::ArrowRight);
    rig.key(Key::Enter);
    assert!(
        rig.has(Role::MenuItem, "Cut"),
        "the next title is Edit and Enter opens it"
    );
}

#[test]
fn arrow_right_opens_the_highlighted_submenu() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    rig.key(Key::ArrowDown);
    rig.key(Key::ArrowRight);
    assert!(
        rig.has(Role::MenuItem, "New Drawing"),
        "ArrowRight opens the highlighted submenu"
    );
}

#[test]
fn enter_on_a_highlighted_submenu_row_opens_it() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    rig.key(Key::ArrowDown);
    rig.key(Key::Enter);
    assert!(
        rig.has(Role::MenuItem, "New Drawing"),
        "Enter opens the highlighted submenu"
    );
}

#[test]
fn a_typed_access_letter_activates_a_row() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    rig.harness.event(egui::Event::Text("o".to_owned()));
    rig.run();
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "file.open");
}

#[test]
fn a_typed_access_letter_opens_a_submenu() {
    let mut rig = Rig::new();
    rig.key_mod(Modifiers::ALT, Key::F);
    rig.harness.event(egui::Event::Text("n".to_owned()));
    rig.run();
    assert!(
        rig.has(Role::MenuItem, "New Drawing"),
        "the access letter opens the submenu"
    );
}

#[test]
fn a_typed_access_letter_with_no_open_menu_does_nothing() {
    let mut rig = Rig::new();
    rig.harness.event(egui::Event::Text("o".to_owned()));
    rig.run();
    assert!(rig.take_intents().is_empty());
    assert!(!rig.has(Role::MenuItem, "Open..."), "no menu opens");
}

#[test]
fn clicking_an_open_title_closes_the_menu() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(rig.has(Role::MenuItem, "Open..."));
    rig.click(Role::Button, "File");
    assert!(
        !rig.has(Role::MenuItem, "Open..."),
        "clicking the open title closes it"
    );
}

#[test]
fn the_help_search_box_runs_help_search() {
    let mut rig = Rig::new();
    {
        let root = rig.harness.root();
        let search = root
            .query_all_by_role_and_label(Role::ComboBox, "Help search")
            .next()
            .expect("the help search box");
        search.click();
        search.type_text("shape data");
    }
    rig.run();
    rig.key(Key::Enter);
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "Enter runs help.search");
    assert_eq!(intents[0].command.as_str(), "help.search");
    assert_eq!(
        intents[0].invocation.args.text("text"),
        Some("shape data"),
        "the typed text rides along"
    );
}

#[test]
fn a_toolbars_row_with_an_implemented_command_dispatches_it() {
    let mut rig = Rig::with_registry(registry(&["view.toolbar.format_text"]));
    rig.click(Role::Button, "View");
    rig.click(Role::MenuItem, "Toolbars");
    rig.click(Role::MenuItem, "Format Text");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1, "the toggle also dispatches its command");
    assert_eq!(intents[0].command.as_str(), "view.toolbar.format_text");
    assert!(rig.has(Role::Button, "Bold"), "and flips the visibility");
}

// --- the context menus --------------------------------------------

#[test]
fn escape_closes_the_context_menu() {
    let mut rig = Rig::with_context_trigger();
    rig.click_secondary(Role::Button, "shape");
    assert!(rig.has(Role::MenuItem, "Cut"));
    rig.key(Key::Escape);
    assert!(!rig.has(Role::MenuItem, "Cut"), "Esc closes it");
}

#[test]
fn a_click_outside_closes_the_context_menu() {
    let mut rig = Rig::with_context_trigger();
    rig.click_secondary(Role::Button, "shape");
    assert!(rig.has(Role::MenuItem, "Cut"));
    rig.click(Role::Button, "File");
    assert!(!rig.has(Role::MenuItem, "Cut"), "the click closes it");
}

#[test]
fn picking_a_context_menu_item_dispatches_and_closes() {
    let mut rig = Rig::with_context_selection();
    rig.click_secondary(Role::Button, "shape");
    rig.click(Role::MenuItem, "Copy");
    let intents = rig.take_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].command.as_str(), "edit.copy");
    assert!(!rig.has(Role::MenuItem, "Copy"), "and the menu closes");
}

#[test]
fn a_submenu_flips_left_when_it_would_leave_the_screen() {
    // The File menu is wide (the long row), so the New submenu would
    // start past the right edge of a 700 px window.
    let mut rig = Rig::sized(Vec2::new(700.0, 500.0));
    rig.click(Role::Button, "File");
    rig.click(Role::MenuItem, "New");
    rig.run();
    let root = rig.harness.root();
    let parent = root
        .query_all_by_role_and_label(Role::MenuItem, "New")
        .next()
        .expect("the New row");
    let child = root
        .query_all_by_role_and_label(Role::MenuItem, "New Drawing")
        .next()
        .expect("the submenu flips left and stays usable");
    assert!(
        child.rect().left() < parent.rect().left(),
        "the child level flips to the left of its parent row"
    );
}

#[test]
fn the_state_machine_guards_its_empty_inputs() {
    let layout = layout();
    let registry = registry(&[]);
    let app = AppCtx::new(Settings::default());
    let mut state = PanelClassic::new();
    // No open menu: the arrows do nothing.
    state.move_highlight(&[], true);
    assert!(state.highlight.is_empty(), "nothing is open");
    // An open menu whose level was not built (a stale chain).
    state.open = Some(0);
    state.move_highlight(&[], true);
    assert!(state.highlight.is_empty(), "a missing level moves nothing");
    // A window mode with no menu bar cycles nothing.
    let shapesheet = PanelInput::new(&layout, &registry, &app).with_mode(WindowMode::Shapesheet);
    state.cycle_title(&shapesheet, 1);
    assert_eq!(state.open, Some(0), "no bar: the title does not move");
    // A frame with no pointer closes nothing.
    with_ui(|ui| state.close_on_outside_click(ui));
    assert_eq!(state.open, Some(0));
}

#[test]
fn find_item_walks_into_the_children_and_key_char_types_letters() {
    let nested = item(
        serde_json::json!({"id": "a", "kind": "submenu", "label": "A", "items": [
            {"id": "a.b", "kind": "button", "label": "B"}
        ]}),
    );
    assert_eq!(
        find_item(&[nested], "a.b").map(|found| found.id.as_str()),
        Some("a.b"),
        "the search walks into the children"
    );
    assert_eq!(key_char(Key::A), Some('a'));
    assert_eq!(key_char(Key::Num1), Some('1'));
    assert_eq!(
        key_char(Key::Escape),
        None,
        "only letter and digit keys type"
    );
}

#[test]
fn a_click_outside_closes_the_open_menu() {
    let mut rig = Rig::new();
    rig.click(Role::Button, "File");
    assert!(rig.has(Role::MenuItem, "Open..."));
    rig.click(Role::ComboBox, "Help search");
    assert!(!rig.has(Role::MenuItem, "Open..."), "the click closes it");
}

#[test]
fn a_popup_can_open_from_any_item_and_a_stale_one_clears() {
    let mut rig = Rig::new();
    let anchor = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(34.0, 22.0));
    // A menu-bar item's children pop up (the lookup walks the menus).
    rig.panel.borrow_mut().open_popup = Some("drawing.file.new".to_owned());
    rig.panel.borrow_mut().popup_rect = anchor;
    rig.run();
    assert!(
        rig.has(Role::MenuItem, "New Drawing"),
        "a menu item's children pop up"
    );
    // A nested item is found too (its own children are empty).
    rig.panel.borrow_mut().open_popup = Some("toolbar.standard.undo.undo_move".to_owned());
    rig.panel.borrow_mut().popup_rect = anchor;
    rig.run();
    assert_eq!(
        rig.panel.borrow().open_popup.as_deref(),
        Some("toolbar.standard.undo.undo_move"),
        "the nested item is found"
    );
    // A stale id clears the popup.
    rig.panel.borrow_mut().open_popup = Some("does.not.exist".to_owned());
    rig.run();
    assert!(
        rig.panel.borrow().open_popup.is_none(),
        "a stale popup clears"
    );
}

#[test]
fn clicking_a_heading_in_a_popup_dispatches_nothing() {
    let mut rig = Rig::with_registry(registry(&["format.set.fill.color"]));
    rig.click(Role::Button, "Colors");
    rig.click(Role::MenuItem, "Recent Colors");
    assert!(rig.take_intents().is_empty(), "a heading is not a command");
    assert!(
        rig.has(Role::MenuItem, "No Fill"),
        "and the popup stays open"
    );
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
