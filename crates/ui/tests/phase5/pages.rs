//! The page tab strip through the real [`Shell`] (task P5-20, ADR 0033,
//! ADR 0047).
//!
//! Tab clicks, the strip's `<` / `>` buttons (the `page.previous` /
//! `page.next` intents) and the tab context menu (Rename Page, Delete
//! Page, Move Page Left, Move Page Right) are driven through
//! `egui_kittest` (ADR 0030). Today's behaviour is pinned, including
//! `Rename Page`: the command needs a name and the shell has no dialog
//! kind for text, so the ask reaches the status bar as one line and the
//! page keeps its name.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use larrygram_commands::app::{AppCtx, DocumentSession};
use larrygram_commands::{
    Command, CommandError, CommandId, CommandMeta, CommandStatus, Enabled, Invocation, Outcome,
    Registry, builtins,
};
use larrygram_model::{Document, PageId};
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::Loader;

/// A document with two named pages and nothing else.
fn app_with_two_pages() -> AppCtx {
    let mut doc = Document::new();
    doc.add_page("Page-1").expect("page one");
    doc.add_page("Page-2").expect("page two");
    let mut app = AppCtx::new(crate::phase5_settings());
    app.open_document(DocumentSession::new(doc, None));
    app
}

/// The page list of `app`: id and tab label in order.
fn page_names(app: &AppCtx) -> Vec<(PageId, String)> {
    let session = app.active_document().expect("a document");
    session
        .document
        .pages
        .iter()
        .map(|page| (page.id, page.name.clone()))
        .collect()
}

/// The active page of `app`.
fn active(app: &AppCtx) -> Option<PageId> {
    app.active_document()
        .and_then(|session| session.active_page)
}

/// A [`Shell`] driven by `egui_kittest` with the real registry and
/// layouts (the `tools.rs` rig).
struct PagesRig {
    shell: Rc<RefCell<Shell>>,
    harness: Harness<'static>,
}

impl PagesRig {
    fn new(app: AppCtx) -> Self {
        Self::with_registry(builtins::registry().expect("registry"), app)
    }

    fn with_registry(registry: Registry, app: AppCtx) -> Self {
        let shell = Rc::new(RefCell::new(Shell::new(
            registry,
            Loader::from_embedded().expect("layouts"),
            app,
        )));
        let harness_shell = Rc::clone(&shell);
        let harness = Harness::builder()
            .with_size(egui::vec2(800.0, 600.0))
            .with_accessibility_check(false)
            .build_ui(move |ui| {
                let mut shell = harness_shell.borrow_mut();
                let intents = shell.show_in(ui).expect("the frame renders");
                let _ = shell.dispatch(&intents).expect("the intents dispatch");
            });
        let mut rig = Self { shell, harness };
        rig.run();
        rig
    }

    fn run(&mut self) {
        self.harness.run();
    }

    /// The widget named `label`, skipping its plain `Label` node (an
    /// egui widget is labelled by a separate label node).
    fn find(&self, label: &'static str) -> Node<'_> {
        self.harness
            .root()
            .query_all_by(|node| {
                node.label().as_deref() == Some(label) && node.role() != Role::Label
            })
            .next()
            .unwrap_or_else(|| panic!("no widget named {label:?} in {:?}", self.names()))
    }

    /// Clicks the widget named `label` and runs one frame.
    fn click(&mut self, label: &'static str) {
        self.find(label).click();
        self.run();
    }

    /// Opens the context menu of the widget named `label` (the
    /// secondary click of `Response::context_menu`) and runs the frames
    /// the menu needs to appear.
    fn open_context_menu(&mut self, label: &'static str) {
        self.find(label).click_secondary();
        for _ in 0..2 {
            self.run();
        }
    }

    /// Every widget name (label, else value) of the current frame (for
    /// failure messages and the status-bar check).
    fn names(&self) -> Vec<String> {
        self.harness
            .root()
            .children_recursive()
            .filter_map(|node| {
                let data = node.accesskit_node().data();
                let label = data.label().unwrap_or_default();
                let name = if label.is_empty() {
                    data.value().unwrap_or_default()
                } else {
                    label
                };
                (!name.is_empty()).then(|| name.to_owned())
            })
            .collect()
    }

    fn app(&self) -> std::cell::Ref<'_, AppCtx> {
        std::cell::Ref::map(self.shell.borrow(), |shell| &shell.app)
    }
}

#[test]
fn a_tab_click_switches_the_active_page() {
    let mut rig = PagesRig::new(app_with_two_pages());
    let pages = page_names(&rig.app());
    assert_eq!(
        active(&rig.app()),
        Some(pages[0].0),
        "the first page starts"
    );
    rig.click("Page-2");
    assert_eq!(active(&rig.app()), Some(pages[1].0), "the tab switch");
    rig.click("Page-1");
    assert_eq!(active(&rig.app()), Some(pages[0].0), "and back");
}

/// A `page.previous` / `page.next` command that counts its runs, to pin
/// the intent identity of the strip's buttons.
struct Counts(&'static str, Arc<AtomicUsize>);

impl Command for Counts {
    fn meta(&self) -> &CommandMeta {
        use std::sync::OnceLock;
        static NEXT: OnceLock<CommandMeta> = OnceLock::new();
        static PREVIOUS: OnceLock<CommandMeta> = OnceLock::new();
        match self.0 {
            "page.next" => NEXT.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("page.next").expect("id"),
                    "Next Page",
                    CommandStatus::Implemented,
                )
            }),
            _ => PREVIOUS.get_or_init(|| {
                CommandMeta::new(
                    CommandId::parse("page.previous").expect("id"),
                    "Previous Page",
                    CommandStatus::Implemented,
                )
            }),
        }
    }
    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::yes()
    }
    fn execute(&self, _ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome::message("counted"))
    }
}

#[test]
fn the_strip_buttons_dispatch_page_previous_and_page_next() {
    let next = Arc::new(AtomicUsize::new(0));
    let previous = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::new();
    registry
        .register(Box::new(Counts("page.next", Arc::clone(&next))))
        .expect("register");
    registry
        .register(Box::new(Counts("page.previous", Arc::clone(&previous))))
        .expect("register");
    let mut rig = PagesRig::with_registry(registry, app_with_two_pages());
    rig.click(">");
    assert_eq!(
        next.load(Ordering::SeqCst),
        1,
        "the `>` button is page.next"
    );
    assert_eq!(previous.load(Ordering::SeqCst), 0);
    rig.click("<");
    assert_eq!(
        previous.load(Ordering::SeqCst),
        1,
        "the `<` button is page.previous"
    );
}

#[test]
fn the_strip_buttons_move_through_the_pages_and_wrap() {
    let mut rig = PagesRig::new(app_with_two_pages());
    let pages = page_names(&rig.app());
    rig.click(">");
    assert_eq!(active(&rig.app()), Some(pages[1].0), "next");
    rig.click(">");
    assert_eq!(active(&rig.app()), Some(pages[0].0), "next wraps");
    rig.click("<");
    assert_eq!(active(&rig.app()), Some(pages[1].0), "previous wraps");
}

#[test]
fn the_page_tab_menu_deletes_the_page() {
    let mut rig = PagesRig::new(app_with_two_pages());
    rig.open_context_menu("Page-2");
    rig.click("Delete Page");
    let pages = page_names(&rig.app());
    assert_eq!(
        pages
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Page-1"],
        "the menu deleted the tab's page"
    );
    assert_eq!(active(&rig.app()), Some(pages[0].0), "and activated it");
}

#[test]
fn the_page_tab_menu_moves_the_page_left_and_right() {
    let mut rig = PagesRig::new(app_with_two_pages());
    rig.open_context_menu("Page-2");
    rig.click("Move Page Left");
    assert_eq!(
        page_names(&rig.app())
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Page-2", "Page-1"],
        "the tab's page moved left"
    );
    rig.open_context_menu("Page-2");
    rig.click("Move Page Right");
    assert_eq!(
        page_names(&rig.app())
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Page-1", "Page-2"],
        "and back right"
    );
}

#[test]
fn the_page_tab_menu_rename_asks_for_a_name_in_one_status_line() {
    // Today's behaviour (a black-box pin): `page.rename` without a name
    // is a `NeedsInput::Text`, the shell has no dialog kind for text
    // (ADR 0036: only file paths open a dialog), so the ask reaches the
    // status bar as one line and nothing is renamed.
    let mut rig = PagesRig::new(app_with_two_pages());
    rig.open_context_menu("Page-1");
    rig.click("Rename Page");
    assert_eq!(
        page_names(&rig.app())
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Page-1", "Page-2"],
        "the page keeps its name"
    );
    let app = rig.app();
    let warning = app
        .warnings
        .last()
        .expect("the ask reached the status bar")
        .clone();
    assert!(
        warning.contains("page name"),
        "the status line names what is needed: {warning}"
    );
    assert!(
        !warning.contains('\n'),
        "the status line is one line: {warning:?}"
    );
    drop(app);
    assert!(
        rig.names()
            .iter()
            .any(|label| label.contains(warning.as_str())),
        "the status bar shows the warning: {:?}",
        rig.names()
    );
}
