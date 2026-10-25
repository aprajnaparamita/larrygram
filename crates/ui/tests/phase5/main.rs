//! The Phase 5 pointer and snapping tests (task P5-05, ADR 0049).
//!
//! One test binary with one module per area: `pointer` (selection,
//! marquee, move, resize, rotate, the focus scope) and `snap` (snapping
//! and the drag overlay). The rigs drive the real canvas, the real
//! `ToolMachine` and the real command registry through `egui_kittest`
//! (ADR 0030): a panic is the bug (AGENTS.md section 6).

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod drag_timing;
mod draw;
mod pages;
mod pointer;
mod snap;
mod text;
mod tools;

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::{Registry, builtins};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId,
};
use larrygram_render::Overlay;
use larrygram_ui::canvas::{self, CanvasCache};
use larrygram_ui::shell::dispatch_one;
use larrygram_ui::tools::ToolMachine;

/// The harness size (the canvas viewport is this minus the ruler bands).
pub const WIDTH: f32 = 800.0;
/// The harness height.
pub const HEIGHT: f32 = 600.0;
/// The zoom of every test (page 8.5 x 11 in fits the viewport).
pub const ZOOM: f64 = 0.5;
/// The ruler band width (mirrors `canvas::RULER_WIDTH`).
pub const RULER: f32 = 18.0;
/// The harness's outer frame margin (egui_kittest's central panel).
pub const MARGIN: f32 = 8.0;
/// The page height in inches (the fixture pages).
pub const PAGE_HEIGHT: f64 = 11.0;

/// A page point in screen points (page y is up; the raster is y down).
pub fn screen(x: f64, y: f64) -> egui::Pos2 {
    let k = 96.0 * ZOOM;
    egui::pos2(
        MARGIN + RULER + (x * k) as f32,
        MARGIN + RULER + ((PAGE_HEIGHT - y) * k) as f32,
    )
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

/// One 2 x 1 in filled rectangle with its pin at (`x`, `y`).
fn shape(x: f64, y: f64, visio_id: i32) -> Shape {
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

/// The settings of the Phase 5 suites: the Panel (2016 data) layout
/// (the Phase 4 frame they pin: `canvas::show_page_tabs` and the
/// status bar) and the welcome wizard already seen (task P7-09 flips
/// both defaults).
pub fn phase5_settings() -> Settings {
    let mut settings = Settings::default();
    settings.ui_layout = larrygram_ui_spec::LayoutKind::Panel2016;
    settings.default_layout = larrygram_ui_spec::LayoutKind::Panel2016;
    settings.welcome_seen = true;
    settings
}

/// An app with one page and one filled shape at (3, 3).
pub fn app_with_one_shape() -> (AppCtx, ShapeId) {
    let mut doc = Document::new();
    let id = doc.add_shape(shape(3.0, 3.0, 1));
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    page.shapes.push(id);
    page.sheet.cells = vec![cell("PageWidth", 8.5), cell("PageHeight", PAGE_HEIGHT)];
    doc.pages.push(page);
    let mut app = AppCtx::new(phase5_settings());
    app.open_document(DocumentSession::new(doc, None));
    let session = app.active_document_mut().expect("session");
    session.view.zoom = ZOOM;
    (app, id)
}

/// A canvas rig: the real canvas, tool machine and registry over one
/// `AppCtx`, driven by `egui_kittest`.
pub struct CanvasRig {
    /// The application state.
    pub app: Rc<RefCell<AppCtx>>,
    /// The canvas raster cache (its generation counts re-rasterizations).
    pub cache: Rc<RefCell<CanvasCache>>,
    /// The tool state machine.
    pub tools: Rc<RefCell<ToolMachine>>,
    /// The overlay of the last frame.
    pub overlay: Rc<RefCell<Overlay>>,
    harness: Harness<'static>,
}

impl CanvasRig {
    /// A rig over `app` and the builtin registry.
    pub fn new(app: AppCtx) -> Self {
        Self::with_registry(app, builtins::registry().expect("the registry builds"))
    }

    /// A rig over `app` and a caller-chosen registry.
    pub fn with_registry(app: AppCtx, registry: Registry) -> Self {
        let app = Rc::new(RefCell::new(app));
        let cache = Rc::new(RefCell::new(CanvasCache::new()));
        let tools = Rc::new(RefCell::new(ToolMachine::new()));
        let overlay = Rc::new(RefCell::new(Overlay::new()));
        let registry = Rc::new(registry);
        let harness = Harness::builder()
            .with_size(egui::vec2(WIDTH, HEIGHT))
            .build_ui({
                let app = Rc::clone(&app);
                let cache = Rc::clone(&cache);
                let tools = Rc::clone(&tools);
                let overlay = Rc::clone(&overlay);
                let registry = Rc::clone(&registry);
                move |ui| {
                    let mut app = app.borrow_mut();
                    let mut cache = cache.borrow_mut();
                    let mut tools = tools.borrow_mut();
                    let mut overlay = overlay.borrow_mut();
                    let events;
                    {
                        let Some(session) = app.active_document_mut() else {
                            return;
                        };
                        let output = canvas::show_with_overlay(ui, session, &mut cache, &overlay)
                            .expect("the canvas draws");
                        events = output.events;
                    }
                    let had_events = !events.is_empty();
                    {
                        for event in events {
                            let effect = tools.handle(event, &app).expect("the tool handles it");
                            *overlay = effect.overlay;
                            for intent in effect.intents {
                                let _ = dispatch_one(
                                    &registry,
                                    &mut app,
                                    &intent.command,
                                    intent.invocation,
                                );
                            }
                        }
                        if !had_events
                            && let Some(idle) = tools.idle_overlay(&app).expect("idle overlay")
                        {
                            *overlay = idle;
                        }
                    }
                }
            });
        let mut rig = Self {
            app,
            cache,
            tools,
            overlay,
            harness,
        };
        rig.run();
        rig
    }

    /// Runs one frame.
    pub fn run(&mut self) -> u64 {
        self.harness.run()
    }

    /// Presses the primary button at a screen point and runs a frame.
    /// Answers the frames the harness settled (one in the usual case).
    pub fn down(&mut self, pos: egui::Pos2) -> u64 {
        self.harness.drag_at(pos);
        self.harness.run()
    }

    /// Moves the pointer and runs a frame (module docs of
    /// `drag_timing`: the frames it settled).
    pub fn move_to(&mut self, pos: egui::Pos2) -> u64 {
        self.harness.hover_at(pos);
        self.harness.run()
    }

    /// Releases the primary button at a screen point and runs a frame.
    pub fn up(&mut self, pos: egui::Pos2) -> u64 {
        self.harness.drop_at(pos);
        self.harness.run()
    }

    /// A click: down and up at the same point.
    pub fn click(&mut self, pos: egui::Pos2) {
        self.down(pos);
        self.up(pos);
    }

    /// A drag: down, one move and up.
    pub fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.down(from);
        self.move_to(to);
        self.up(to);
    }

    /// Sets the Alt modifier for the next frames (snapping off).
    pub fn set_alt(&mut self, alt: bool) {
        let modifiers = if alt {
            egui::Modifiers {
                alt: true,
                ..egui::Modifiers::default()
            }
        } else {
            egui::Modifiers::default()
        };
        self.harness.event(egui::Event::ModifiersChanged(modifiers));
    }

    /// A shift-click (add or remove from the selection).
    pub fn shift_click(&mut self, pos: egui::Pos2) {
        self.harness
            .event(egui::Event::ModifiersChanged(egui::Modifiers {
                shift: true,
                ..egui::Modifiers::default()
            }));
        self.click(pos);
        self.harness
            .event(egui::Event::ModifiersChanged(egui::Modifiers::default()));
        self.harness.run();
    }

    /// The raster generation (how many times the page was re-rasterized).
    pub fn generation(&self) -> u64 {
        self.cache.borrow().generation()
    }
}

/// One cached number cell of a shape.
pub fn shape_number(app: &AppCtx, shape: ShapeId, name: &str) -> f64 {
    let session = app.active_document().expect("a document");
    let entry = session.document.shape(shape).expect("a shape");
    entry
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
        .expect("the cell exists")
}
