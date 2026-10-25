//! The Phase 7 shell-integration tests (task P7-09, ADR 0061 and
//! ADR 0066): the four layouts switching at run time, the first-run
//! wizard and the Help entries, driven through a real [`Shell`].
//!
//! One test binary with one module per acceptance area: `layouts`
//! (the four frames and the switches) and `first_run` (the wizard and
//! the Help menu).
//!
//! The harness closure panics on a frame error, so every `run` is the
//! "each frame renders without an error" check of the acceptance
//! criteria.

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod first_run;
mod layouts;
mod surfaces;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{CommandId, Invocation, Registry, Source};
use larrygram_ui::Intent;
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutKind, LayoutSets, Loader};

/// The committed layouts, parsed once per test binary (the app loads
/// the same embedded bytes).
pub fn sets() -> &'static LayoutSets {
    use std::sync::OnceLock;
    static SETS: OnceLock<LayoutSets> = OnceLock::new();
    SETS.get_or_init(|| Loader::from_embedded().expect("the embedded ui-spec loads"))
}

/// Settings with the welcome wizard seen: the layout tests drive the
/// wizard explicitly, in `first_run`.
pub fn seen_settings() -> Settings {
    let mut settings = Settings::default();
    settings.welcome_seen = true;
    settings
}

/// Settings for one layout kind (the wizard already seen).
pub fn settings_for(kind: LayoutKind) -> Settings {
    let mut settings = seen_settings();
    settings.ui_layout = kind;
    settings.default_layout = kind;
    settings
}

/// A whole [`Shell`] driven headlessly, the P4-04 rig pattern.
pub struct ShellRig {
    /// The shell under test (the same instance across frames).
    pub shell: Rc<RefCell<Shell>>,
    harness: Harness<'static>,
}

impl ShellRig {
    /// A shell over the builtin registry, the committed layouts and
    /// `settings`, driven frame by frame.
    pub fn new(settings: Settings) -> Self {
        Self::with_app(AppCtx::new(settings))
    }

    /// A shell over caller-chosen application state (open documents,
    /// a selection, ...).
    pub fn with_app(app: AppCtx) -> Self {
        let registry: Registry =
            larrygram_commands::builtins::registry().expect("the registry builds");
        let shell = Rc::new(RefCell::new(Shell::new(registry, sets().clone(), app)));
        let harness_shell = Rc::clone(&shell);
        let harness = Harness::builder()
            .with_size(egui::vec2(1600.0, 900.0))
            .with_context_setup(|ctx| {
                // The apps install the image loaders at start-up
                // (ADR 0067); without them the brand images draw as
                // broken-image marks.
                larrygram_ui::install_image_loaders(ctx);
            })
            .build_ui(move |ui| {
                let mut shell = harness_shell.borrow_mut();
                let intents = shell.show_in(ui).expect("the frame renders");
                shell.dispatch(&intents).expect("the intents dispatch");
            });
        Self { shell, harness }
    }

    /// Draws one frame.
    pub fn run(&mut self) {
        self.harness.run();
    }

    /// One dispatch through the shell (the registry path every
    /// control uses).
    pub fn dispatch(&mut self, id: &'static str) {
        let command = CommandId::parse(id).expect("id");
        let intent = Intent::new(command, Invocation::empty(Source::Ui));
        let mut shell = self.shell.borrow_mut();
        shell.dispatch(&[intent]).expect("the command runs");
    }

    /// The widget names of the current frame, as a multiset.
    pub fn names(&mut self) -> BTreeMap<String, usize> {
        self.run();
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

    /// Whether the current frame draws a widget named `name`.
    pub fn has(&mut self, name: &str) -> bool {
        self.names().contains_key(name)
    }

    /// Clicks the first widget with `role` and `name`.
    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    pub fn click(&mut self, role: Role, name: &str) {
        self.run();
        let root = self.harness.root();
        let node = root
            .query_all_by_role_and_label(role, name)
            .next()
            .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        node.click();
    }

    /// Clicks the first widget named `name`, whatever its role.
    #[allow(clippy::expect_used)] // test helper: a failure here is the bug
    pub fn click_named(&mut self, name: &str) {
        self.run();
        let root = self.harness.root();
        let node = root
            .query_all_by_label(name)
            .next()
            .unwrap_or_else(|| panic!("no widget named {name:?}"));
        node.click();
    }
}
