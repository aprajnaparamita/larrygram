//! The Phase 4 UI test harness (task P4-09, ADR 0037).
//!
//! One test binary with one module per ADR 0037 item: `tab_bar`
//! (placement completeness), `layout_switch` (ADR 0032),
//! `enabled_state` (ADR 0031) and `file_flow` (ADR 0036). The helpers
//! below drive a real [`Shell`] through `egui_kittest` (ADR 0030) and
//! walk the frame's AccessKit tree, the way assistive tech and the
//! tests see the widgets.
//!
//! Placement exactness per command-bar tab, backstage entry and QAT is
//! pinned inside `crates/ui` by P4-04's
//! `both_layouts_render_one_widget_per_placement` and its
//! `layout_counts` snapshot. These tests extend that to the committed
//! context menus and to whole shell frames; they do not duplicate it.

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod enabled_state;
mod file_flow;
mod flows;
mod interactions;
mod layout_switch;
mod tab_bar;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{CommandError, CommandId, Invocation, Outcome, Registry, Source};
use larrygram_ui::Intent;
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutSets, Loader};

/// The committed layouts, parsed once per test binary (the app loads
/// the same embedded bytes).
pub fn sets() -> &'static LayoutSets {
    use std::sync::OnceLock;
    static SETS: OnceLock<LayoutSets> = OnceLock::new();
    SETS.get_or_init(|| Loader::from_embedded().expect("the embedded ui-spec loads"))
}

/// One rendered widget node (the AccessKit view of a widget).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    /// The AccessKit role.
    pub role: Role,
    /// The accessible name (label, else value).
    pub name: String,
    /// Whether the widget is disabled.
    pub disabled: bool,
    /// The tooltip text (the `description`), when one is shown.
    pub description: Option<String>,
}

/// The roles that count as one rendered widget (ADR 0031).
fn is_widget_role(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::ComboBox
            | Role::Label
            | Role::MenuItem
            | Role::ListBoxOption
    )
}

/// Every rendered widget of a harness frame, in tree order.
pub fn widgets(harness: &Harness<'_>) -> Vec<Widget> {
    let root = harness.root();
    let mut out = Vec::new();
    for node in root.children_recursive() {
        let data = node.accesskit_node().data();
        if !is_widget_role(data.role()) {
            continue;
        }
        let label = data.label().unwrap_or_default();
        let name = if label.is_empty() {
            data.value().unwrap_or_default()
        } else {
            label
        };
        if name.is_empty() {
            continue;
        }
        out.push(Widget {
            role: data.role(),
            name: name.to_owned(),
            disabled: data.is_disabled(),
            description: data.description().map(str::to_owned),
        });
    }
    out
}

/// The widget names of a harness frame, as a multiset.
pub fn names(harness: &Harness<'_>) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for widget in widgets(harness) {
        *counts.entry(widget.name).or_default() += 1;
    }
    counts
}

/// A whole [`Shell`] driven headlessly.
///
/// The harness closure keeps running between the test's assertions,
/// so the shell is shared through `Rc<RefCell<..>>` and the test
/// drives it between frames (the P4-04 rig pattern).
pub struct ShellRig {
    /// The shell under test (the same instance across frames: a
    /// "restart" would replace it).
    pub shell: Rc<RefCell<Shell>>,
    /// The egui context of the harness, captured at build time. A
    /// restart would build a new one.
    pub egui_ctx: Rc<RefCell<Option<egui::Context>>>,
    harness: Harness<'static>,
}

/// The settings of the Phase 4 suites: the Panel (2016 data) layout
/// (the v1 widgets these tests pin) and the welcome wizard already
/// seen (task P7-09 flips both defaults: new stores start on Panel
/// Modern and open the wizard on the first launch).
pub fn phase4_settings() -> Settings {
    let mut settings = Settings::default();
    settings.ui_layout = larrygram_ui_spec::LayoutKind::Panel2016;
    settings.default_layout = larrygram_ui_spec::LayoutKind::Panel2016;
    settings.welcome_seen = true;
    settings
}

impl ShellRig {
    /// A shell over the builtin registry, the committed layouts and
    /// the Phase 4 settings.
    pub fn new() -> Self {
        Self::with_app(AppCtx::new(phase4_settings()))
    }

    /// A shell over the builtin registry and the committed layouts,
    /// with caller-chosen state.
    pub fn with_app(app: AppCtx) -> Self {
        let registry = larrygram_commands::builtins::registry().expect("the registry builds");
        Self::build(Shell::new(registry, sets().clone(), app))
    }

    /// A shell over a caller-chosen registry (the file-flow tests
    /// inject commands).
    pub fn with_registry(registry: Registry, app: AppCtx) -> Self {
        Self::build(Shell::new(registry, sets().clone(), app))
    }

    fn build(shell: Shell) -> Self {
        Self::build_sized(shell, egui::vec2(1600.0, 900.0))
    }

    /// The same rig with a window the whole committed command bar fits in
    /// (the P4-04 note: the layouts are about 5000 points wide, and a
    /// click outside the screen rect never reaches a widget).
    pub fn large() -> Self {
        Self::build_sized(
            Shell::new(
                larrygram_commands::builtins::registry().expect("the registry builds"),
                sets().clone(),
                AppCtx::new(phase4_settings()),
            ),
            egui::vec2(12000.0, 4000.0),
        )
    }

    fn build_sized(shell: Shell, size: egui::Vec2) -> Self {
        let shell = Rc::new(RefCell::new(shell));
        let egui_ctx: Rc<RefCell<Option<egui::Context>>> = Rc::new(RefCell::new(None));
        let harness = Harness::builder()
            .with_size(size)
            .with_context_setup({
                let egui_ctx = Rc::clone(&egui_ctx);
                move |ctx| {
                    // The apps install the image loaders at start-up;
                    // the rig does the same (ADR 0067), or the brand
                    // images draw as broken-image marks.
                    larrygram_ui::install_image_loaders(ctx);
                    *egui_ctx.borrow_mut() = Some(ctx.clone());
                }
            })
            .build_ui({
                let shell = Rc::clone(&shell);
                move |ui| {
                    let mut shell = shell.borrow_mut();
                    let intents = shell.show_in(ui).expect("the frame renders");
                    let results = shell.dispatch(&intents).expect("the intents dispatch");
                    // A command failure is a finding for the caller
                    // to assert on, never a panic here.
                    let _ = results;
                }
            });
        let mut rig = Self {
            shell,
            egui_ctx,
            harness,
        };
        rig.run();
        rig
    }

    /// Runs one frame.
    pub fn run(&mut self) {
        self.harness.run();
    }

    /// Dispatches one command through the shell between frames.
    pub fn dispatch(&self, id: &'static str, source: Source) -> Result<Outcome, CommandError> {
        let command = CommandId::parse(id).expect("a valid command id");
        let intent = Intent::new(command, Invocation::empty(source));
        let mut shell = self.shell.borrow_mut();
        let mut results = shell.dispatch(&[intent]).expect("the intent dispatches");
        results.pop().expect("one result per intent")
    }

    /// The widgets of the current frame.
    pub fn widgets(&self) -> Vec<Widget> {
        widgets(&self.harness)
    }

    /// Clicks the first widget with `role` and `name` and runs one
    /// frame. Panics when no such widget exists (the test's finding).
    pub fn click(&mut self, role: Role, name: &str) {
        assert!(
            self.try_click(role, name),
            "no {role:?} named {name:?} in {:?}",
            self.names().keys().collect::<Vec<_>>()
        );
    }

    /// Clicks the first widget with `role` and `name` and runs one
    /// frame; `false` (and no frame) when there is no such widget.
    pub fn try_click(&mut self, role: Role, name: &str) -> bool {
        self.try_click_nth(role, name, 0)
    }

    /// Like [`ShellRig::try_click`], but the `nth` matching widget
    /// (the window has two "File" buttons: menu bar and command bar).
    pub fn try_click_nth(&mut self, role: Role, name: &str, nth: usize) -> bool {
        let clicked = {
            let root = self.harness.root();
            let node = root.query_all_by_role_and_label(role, name).nth(nth);
            match node {
                Some(node) => {
                    node.click();
                    true
                }
                None => false,
            }
        };
        if clicked {
            self.harness.run();
        }
        clicked
    }

    /// The widget names of the current frame, as a multiset.
    pub fn names(&self) -> BTreeMap<String, usize> {
        names(&self.harness)
    }
}

impl Default for ShellRig {
    fn default() -> Self {
        Self::new()
    }
}
