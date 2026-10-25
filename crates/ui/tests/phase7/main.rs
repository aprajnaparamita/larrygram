//! The Phase 7 tests (tasks P7-09 and P7-10, ADR 0061, ADR 0065 and
//! ADR 0066): the four layouts switching at run time, the first-run
//! wizard and the Help entries, the ADR 0065 menu and control sweeps,
//! the placement checks and the golden images, driven through real
//! widgets with `egui_kittest`.
//!
//! One test binary with one module per acceptance area: `layouts`
//! (the four frames and the switches), `first_run` (the wizard and the
//! Help menu), `surfaces` (the per-layout page strips and status
//! bars), `menus` and `modern` (the ADR 0065 sweeps), `placement`
//! (the Panel Modern slots) and `golden` (the PNG goldens).
//!
//! The harness closure panics on a frame error, so every `run` is the
//! "each frame renders without an error" check of the acceptance
//! criteria.

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod expect;
mod first_run;
mod golden;
mod layouts;
mod menus;
mod modern;
mod placement;
mod surfaces;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::{
    ClipboardContent, ClipboardKind, CommandId, Invocation, Registry, Source,
};
use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, QName, Row, Section, SectionName,
    Shape, ShapeId, XmlNode,
};
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

/// The builtin registry of `larrygram_commands` (every implemented
/// command plus a stub per row of `commands.yaml`).
pub fn builtin_registry() -> Registry {
    larrygram_commands::builtins::registry().expect("the builtin registry builds")
}

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`) (the
/// Phase 5 fixture shape, as in `surfaces`).
fn fixture_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`).
fn fixture_shape(x: f64, y: f64, visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        fixture_cell("Width", 2.0),
        fixture_cell("Height", 1.0),
        fixture_cell("PinX", x),
        fixture_cell("PinY", y),
        fixture_cell("LocPinX", 1.0),
        fixture_cell("LocPinY", 0.5),
        fixture_cell("Angle", 0.0),
        fixture_cell("FillPattern", 1.0),
        fixture_cell("LinePattern", 1.0),
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
        row.cells = vec![fixture_cell("X", px), fixture_cell("Y", py)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

/// An application state rich enough for the enablement predicates: a
/// document with a rectangle and a picture, the rectangle selected,
/// and a shape on the clipboard (the ADR 0065 sweep state).
pub struct RichApp(
    /// The application state.
    pub AppCtx,
    /// The ordinary rectangle on the page.
    ShapeId,
    /// The picture shape on the page.
    ShapeId,
);

impl RichApp {
    /// The sweep state: a document, a selection and a clipboard.
    pub fn rich() -> Self {
        let mut document = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        let rectangle = document.add_shape(fixture_shape(3.0, 3.0, 1));
        let mut picture = fixture_shape(6.0, 3.0, 2);
        // A picture is a shape with a `ForeignData` child (ADR 0064).
        picture.preserved.children.push(XmlNode {
            name: QName::local("ForeignData"),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        });
        let picture = document.add_shape(picture);
        page.shapes.push(rectangle);
        page.shapes.push(picture);
        document.pages.push(page);
        let mut app = AppCtx::new(seen_settings());
        app.open_document(DocumentSession::new(document, None));
        app.selection.shapes = vec![rectangle];
        app.selection.anchor = Some(rectangle);
        app.selection.page = Some(PageId { index: 0 });
        app.clipboard.content = Some(ClipboardContent {
            kind: ClipboardKind::Shapes,
            shapes: vec![Shape::new(ShapeId { index: 0 }, 1)],
            connects: Vec::new(),
            source_page: PageId { index: 0 },
        });
        Self(app, rectangle, picture)
    }

    /// Selects the picture shape (one selected picture is the
    /// `PictureSelected` context, ADR 0064).
    pub fn select_picture(&mut self) {
        self.0.selection.shapes = vec![self.2];
        self.0.selection.anchor = Some(self.2);
        self.0.selection.page = Some(PageId { index: 0 });
    }
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

    /// Renders the last frame (the golden images; the `wgpu` feature
    /// of `egui_kittest`).
    pub fn render(&mut self) -> image::RgbaImage {
        self.harness.render().expect("the frame renders")
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
