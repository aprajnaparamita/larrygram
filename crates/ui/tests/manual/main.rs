//! The manual-pass fixes (task P7-22): the Delete key, the group
//! context-menu and selection rules, Insert > Text Box and Picture,
//! Delete in the context menu and the flip rendering, driven through
//! the real [`Shell`](larrygram_ui::shell::Shell) with key presses,
//! right-clicks and menu clicks in **both** layouts (`egui_kittest`,
//! offscreen).
//!
//! One test binary with one module per goal item: `keys` (item 1),
//! `groups` (items 2, 3 and 6) and `insert` (items 4 and 5). Every
//! test drives real input events, never a direct command dispatch
//! (the master note of `docs/tasks/P7-22.md`).

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod flip;
mod groups;
mod insert;
mod keys;

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::{CommandId, Invocation, Registry, Source};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId,
};
use larrygram_ui::Intent;
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutKind, LayoutSets, Loader};

/// The two layouts every test runs in (the master note).
pub const LAYOUTS: [LayoutKind; 2] = [LayoutKind::PanelClassic, LayoutKind::PanelModern];

/// The window of every test, in points.
pub const WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// The committed layouts, parsed once per test binary (the app loads
/// the same embedded bytes).
pub fn sets() -> &'static LayoutSets {
    use std::sync::OnceLock;
    static SETS: OnceLock<LayoutSets> = OnceLock::new();
    SETS.get_or_init(|| Loader::from_embedded().expect("the embedded ui-spec loads"))
}

/// Settings for one layout kind (the welcome wizard already seen).
pub fn settings_for(kind: LayoutKind) -> Settings {
    let mut settings = Settings::default();
    settings.welcome_seen = true;
    settings.ui_layout = kind;
    settings.default_layout = kind;
    settings
}

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`) and a
/// rectangular geometry (the Phase 5/7 fixture shape).
pub fn rectangle(x: f64, y: f64, visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
        cell("FillPattern", 1.0),
        cell("LinePattern", 1.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", 2.0, 0.0),
        ("LineTo", 2.0, 1.0),
        ("LineTo", 0.0, 1.0),
        ("LineTo", 0.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

/// The fixture document: a rectangle at (3, 3) and `extra` shapes
/// added after it, on one page.
pub fn fixture(extra: Vec<Shape>) -> (Document, Vec<ShapeId>) {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    let mut ids = Vec::new();
    let first = document.add_shape(rectangle(3.0, 3.0, 1));
    page.shapes.push(first);
    ids.push(first);
    for shape in extra {
        let id = document.add_shape(shape);
        page.shapes.push(id);
        ids.push(id);
    }
    document.pages.push(page);
    (document, ids)
}

/// A whole [`Shell`] driven headlessly (the P4-04 rig pattern).
pub struct ShellRig {
    /// The shell under test (the same instance across frames).
    pub shell: Rc<RefCell<Shell>>,
    harness: Harness<'static>,
}

/// Zooms the active session out until the whole 8.5 x 11 in page fits
/// the canvas (the fixture points span the page).
fn fit_page(app: &mut AppCtx) {
    if let Some(session) = app.active_document_mut() {
        session.view.zoom = 0.5;
    }
}

impl ShellRig {
    /// A shell over caller-chosen application state in one layout.
    pub fn with_app(app: AppCtx) -> Self {
        let registry: Registry =
            larrygram_commands::builtins::registry().expect("the registry builds");
        let shell = Rc::new(RefCell::new(Shell::new(registry, sets().clone(), app)));
        let harness_shell = Rc::clone(&shell);
        let harness = Harness::builder()
            .with_size(WINDOW)
            .with_pixels_per_point(1.0)
            .with_context_setup(|ctx| {
                larrygram_ui::install_image_loaders(ctx);
            })
            .build_ui(move |ui| {
                let mut shell = harness_shell.borrow_mut();
                let intents = shell.show_in(ui).expect("the frame renders");
                shell.dispatch(&intents).expect("the intents dispatch");
            });
        Self { shell, harness }
    }

    /// A shell over `app`, one frame drawn.
    pub fn ready(app: AppCtx) -> Self {
        let mut rig = Self::with_app(app);
        rig.run();
        rig
    }

    /// Draws one frame.
    pub fn run(&mut self) {
        self.harness.run();
    }

    /// The harness itself (pointer and key events).
    pub fn harness(&mut self) -> &mut Harness<'static> {
        &mut self.harness
    }

    /// Presses and releases one key (a real key event).
    pub fn key(&mut self, key: egui::Key) {
        self.harness.key_press(key);
        self.run();
    }

    /// The screen point of a page point in the canvas.
    pub fn screen(&self, x: f64, y: f64) -> egui::Pos2 {
        self.shell
            .borrow()
            .page_to_screen(Point::new(x, y))
            .expect("the canvas drew")
    }

    /// A press and a release in one frame at a page point.
    pub fn click_page(&mut self, x: f64, y: f64) {
        self.click_at(self.screen(x, y));
    }

    /// A press and a release in one frame at a screen point.
    pub fn click_at(&mut self, pos: egui::Pos2) {
        self.click_at_mods(pos, egui::Modifiers::NONE);
    }

    /// A press and a release in one frame at a screen point with
    /// `modifiers` (Shift+click toggles the selection). The modifiers
    /// are set with `ModifiersChanged` (egui keeps the frame's
    /// modifier state there, not on the button events).
    pub fn click_at_mods(&mut self, pos: egui::Pos2, modifiers: egui::Modifiers) {
        self.harness()
            .event(egui::Event::ModifiersChanged(modifiers));
        self.harness().event(egui::Event::PointerMoved(pos));
        self.run();
        for pressed in [true, false] {
            self.harness().event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            });
        }
        self.run();
        self.harness()
            .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        self.run();
    }

    /// A right-click (press and release of the secondary button) at a
    /// page point.
    pub fn right_click_page(&mut self, x: f64, y: f64) {
        let pos = self.screen(x, y);
        self.harness().event(egui::Event::PointerMoved(pos));
        self.run();
        for pressed in [true, false] {
            self.harness().event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        self.run();
    }

    /// One dispatch through the shell (test plumbing only; the tests
    /// that count drive real input instead).
    pub fn dispatch(&mut self, id: &'static str) {
        let command = CommandId::parse(id).expect("id");
        let intent = Intent::new(command, Invocation::empty(Source::Ui));
        let mut shell = self.shell.borrow_mut();
        shell.dispatch(&[intent]).expect("the command runs");
    }

    /// The selected shapes.
    pub fn selection(&self) -> Vec<ShapeId> {
        self.shell.borrow().app.selection.shapes.clone()
    }

    /// The shapes on the active page, in z order.
    pub fn page_shapes(&self) -> Vec<ShapeId> {
        let shell = self.shell.borrow();
        let session = shell.app.active_document().expect("session");
        let page = session.active_page.expect("page");
        session.document.page(page).expect("page").shapes.clone()
    }

    /// The undo depth of the active document.
    pub fn undo_len(&self) -> usize {
        self.shell
            .borrow()
            .app
            .active_document()
            .expect("session")
            .undo
            .len()
    }

    /// Clicks the first menu row named `label` (the context menu is
    /// open; a submenu row opens its children on click).
    pub fn click_menu_row(&mut self, label: &str) {
        self.click_menu_nth(label, 0);
    }

    /// Clicks the first widget named `label` (a menu-bar title or a
    /// tab), whatever its role.
    pub fn click_named(&mut self, label: &str) {
        use egui_kittest::kittest::Queryable;
        self.run();
        let root = self.harness.root();
        let node = root
            .query_all_by_label(label)
            .next()
            .unwrap_or_else(|| panic!("no widget named {label:?}"));
        node.click();
        self.run();
    }

    /// Clicks the `nth` menu row named `label` (a submenu and its item
    /// can share a label).
    pub fn click_menu_nth(&mut self, label: &str, nth: usize) {
        use egui::accesskit::Role;
        use egui_kittest::kittest::Queryable;
        self.run();
        let root = self.harness.root();
        let node = root
            .query_all_by_role_and_label(Role::MenuItem, label)
            .nth(nth)
            .unwrap_or_else(|| panic!("no menu row named {label:?} (#{nth})"));
        node.click();
        self.run();
    }

    /// The labels of the open context menu's rows (all levels).
    pub fn menu_rows(&mut self) -> Vec<String> {
        use egui::accesskit::Role;
        use egui_kittest::kittest::NodeT;
        self.run();
        self.harness()
            .root()
            .children_recursive()
            .filter(|node| node.accesskit_node().data().role() == Role::MenuItem)
            .map(|node| {
                node.accesskit_node()
                    .data()
                    .label()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }
}

/// An application with one page and the given shapes, nothing
/// selected.
pub fn app_with(shapes: Vec<Shape>, kind: LayoutKind) -> (AppCtx, Vec<ShapeId>) {
    let (document, ids) = fixture(shapes);
    let mut app = AppCtx::new(settings_for(kind));
    app.open_document(DocumentSession::new(document, None));
    fit_page(&mut app);
    (app, ids)
}

/// The one-rectangle application of each layout.
pub fn rig_with_rect(kind: LayoutKind) -> (ShellRig, ShapeId) {
    let (app, ids) = app_with(Vec::new(), kind);
    (ShellRig::ready(app), ids[0])
}
