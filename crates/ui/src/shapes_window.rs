//! The Shapes window: the stencil catalog of original masters
//! (ADR 0034).
//!
//! Owned by task P4-06. The window lists [`stencils::catalog`]: one
//! collapsible section per stencil, one row per master with a small
//! preview. The preview is [`render_raster`] of a one-shape
//! scene built with [`stencils::instantiate`] (ADR 0033: the same pixels
//! as the PNG backends). Clicking a row selects the master;
//! dragging a master onto the page is Phase 5.

use std::collections::HashMap;

use egui::{ColorImage, TextureHandle, TextureOptions, Ui};
use larrygram_commands::AppCtx;
use larrygram_model::stencils::{self, MasterDef};
use larrygram_model::{Cell, CellName, CellValue, Document};
use larrygram_render::{BuildOptions, RasterOptions, build_scene, render_raster};

use crate::UiError;

/// One preview box edge, in egui points.
const PREVIEW_PX: f32 = 40.0;

/// The Shapes window header, per layout (task P7-09 owns the header
/// and look).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Header {
    /// Panel Classic: a "Shapes" title row with a close button, then
    /// "Search for Shapes:" (docs/ui/panel-classic.md).
    #[default]
    Classic,
    /// Panel Modern: a large "Shapes" title with the STENCILS and
    /// SEARCH headings (docs/ui/panel-modern.md).
    Modern,
}

/// A picked stencil master (ADR 0034).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StencilPick {
    /// The stencil's display name (e.g. "Basic Shapes").
    pub stencil: &'static str,
    /// The master's unique name (`NameU`, e.g. "Rectangle").
    pub master: &'static str,
}

/// One cached preview: built on first use and kept as a texture.
///
/// [`TextureHandle`] has no [`std::fmt::Debug`], so the debug output
/// reports only the state (see [`ShapesWindow`]'s `Debug`).
enum Preview {
    /// A rasterized preview.
    Ready(TextureHandle),
    /// The scene or the raster could not be built; the window draws a
    /// placeholder.
    Failed,
}

/// The Shapes window widget state (ADR 0034).
#[derive(Default)]
pub struct ShapesWindow {
    selected: Option<StencilPick>,
    previews: HashMap<(&'static str, &'static str), Preview>,
    /// The master whose row drag started this frame (task P5-17): the
    /// shell takes it and calls `stencil::begin_drag`.
    drag_started: Option<StencilPick>,
    /// The search box text (look only; task P7-09).
    search: String,
}

impl std::fmt::Debug for ShapesWindow {
    /// The selection and the preview states ("ready" or "failed"), not
    /// the texture handles.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let previews: Vec<(&'static str, &'static str, &'static str)> = self
            .previews
            .iter()
            .map(|((stencil, master), state)| {
                let state = match state {
                    Preview::Ready(_) => "ready",
                    Preview::Failed => "failed",
                };
                (*stencil, *master, state)
            })
            .collect();
        f.debug_struct("ShapesWindow")
            .field("selected", &self.selected)
            .field("drag_started", &self.drag_started)
            .field("previews", &previews)
            .finish()
    }
}

impl PartialEq for ShapesWindow {
    /// The window's identity is its selection; the preview cache is
    /// rendering state (texture handles), not widget state.
    fn eq(&self, other: &Self) -> bool {
        self.selected == other.selected
    }
}

impl ShapesWindow {
    /// An empty window with nothing selected.
    pub fn new() -> Self {
        Self::default()
    }

    /// The currently selected master, if any.
    pub fn selected(&self) -> Option<&StencilPick> {
        self.selected.as_ref()
    }

    /// Takes the master whose row drag started since the last call
    /// (task P5-17). The shell starts a stencil drag with it.
    pub fn take_drag_started(&mut self) -> Option<StencilPick> {
        self.drag_started.take()
    }

    /// Draws the header, the stencil sections and the master list
    /// with previews (ADR 0034; the header per layout, task P7-09).
    ///
    /// Returns the picks the user made this frame. A master whose
    /// preview cannot be rasterized draws a placeholder box and still
    /// lists and selects.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        app: &AppCtx,
        header: Header,
    ) -> Result<Vec<StencilPick>, UiError> {
        // `app` is reserved for Phase 5 (dropping a master into the
        // active document); the listing and previews need no state.
        let _ = app;
        self.show_header(ui, header);
        let mut picks = Vec::new();
        for stencil in stencils::catalog() {
            egui::CollapsingHeader::new(stencil.name)
                .default_open(true)
                .show(ui, |ui| {
                    for master in &stencil.masters {
                        let pick = StencilPick {
                            stencil: stencil.name,
                            master: master.name_u,
                        };
                        ui.horizontal(|ui| {
                            self.preview(ui, stencil.name, master);
                            let selected = self.selected == Some(pick);
                            let response = ui.add(
                                egui::Button::selectable(selected, master.name)
                                    .sense(egui::Sense::click_and_drag()),
                            );
                            if response.clicked() {
                                self.select(pick);
                                picks.push(pick);
                            }
                            if response.drag_started() {
                                self.select(pick);
                                self.drag_started = Some(pick);
                            }
                        });
                    }
                });
        }
        Ok(picks)
    }

    /// Selects one master.
    fn select(&mut self, pick: StencilPick) {
        self.selected = Some(pick);
    }

    /// The window header of the active layout (task P7-09): the
    /// classic title row and search box, or the modern titles. The
    /// close button and the search box are look only (hiding the
    /// window and searching the catalog are later work); the button
    /// draws disabled so it never pretends to act.
    fn show_header(&mut self, ui: &mut Ui, header: Header) {
        match header {
            Header::Classic => {
                ui.horizontal(|ui| {
                    ui.label("Shapes");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(false, |ui| ui.small_button("x"))
                            .inner
                            .on_disabled_hover_text("Not yet implemented");
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Search for Shapes:");
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("type a word"));
                    ui.add_enabled_ui(false, |ui| ui.small_button("Go"))
                        .inner
                        .on_disabled_hover_text("Not yet implemented");
                });
            }
            Header::Modern => {
                ui.heading("Shapes");
                ui.label("STENCILS");
                ui.label("SEARCH");
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search for shapes"));
            }
        }
        ui.separator();
    }

    /// Draws the preview of one master, rasterizing and caching it on
    /// first use.
    fn preview(&mut self, ui: &mut Ui, stencil: &'static str, master: &MasterDef) {
        let key = (stencil, master.name_u);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.previews.entry(key) {
            let state = match preview_image(master) {
                Some(image) => {
                    let name = format!("stencil-preview-{stencil}-{}", master.name_u);
                    Preview::Ready(ui.ctx().load_texture(name, image, TextureOptions::LINEAR))
                }
                None => Preview::Failed,
            };
            entry.insert(state);
        }
        let size = egui::vec2(PREVIEW_PX, PREVIEW_PX);
        match self.previews.get(&key) {
            Some(Preview::Ready(texture)) => {
                ui.add(egui::Image::new((texture.id(), size)));
            }
            _ => {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
}

/// The one-master document a preview rasterizes: the instance
/// [`stencils::instantiate`] places, on a page the master's size so the
/// raster is a close crop (ADR 0034).
fn preview_document(master: &MasterDef) -> Document {
    let mut document = Document::new();
    stencils::instantiate(&mut document, master);
    let mut width = Cell::new(CellName::from_static("PageWidth"));
    width.value = Some(CellValue::Number(master.width));
    let mut height = Cell::new(CellName::from_static("PageHeight"));
    height.value = Some(CellValue::Number(master.height));
    if let Some(page) = document.pages.first_mut() {
        page.sheet.cells.push(width);
        page.sheet.cells.push(height);
    }
    document
}

/// The preview of one master as raw RGBA pixels, or `None` when the
/// scene or the raster cannot be built.
fn preview_image(master: &MasterDef) -> Option<ColorImage> {
    let document = preview_document(master);
    let page = larrygram_model::PageId { index: 0 };
    let scale = f64::from(PREVIEW_PX) / master.width.max(master.height);
    let mut options = BuildOptions::new();
    options.scale = scale;
    let scene = build_scene(&document, page, &options).ok()?;
    let raster = render_raster(
        &scene,
        &RasterOptions {
            scale,
            antialias: true,
        },
    )
    .ok()?;
    if raster.width == 0 || raster.height == 0 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied(
        [raster.width as usize, raster.height as usize],
        &raster.rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;

    /// Runs `f` with a scratch [`egui::Ui`] and returns the strings the
    /// frame painted (the labels of the widgets it drew).
    #[allow(clippy::expect_used, clippy::panic)] // test helper
    fn painted_text(f: impl FnOnce(&mut Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut f = Some(f);
        let mut output = ctx.run_ui(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(func) = f.take() {
                    func(ui);
                }
            });
        });
        // The scratch harness draws nothing; drop the texture deltas on
        // purpose (epaint would otherwise panic on drop).
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        texts
    }

    #[test]
    fn the_window_starts_with_nothing_selected() {
        let window = ShapesWindow::new();
        assert!(window.selected().is_none());
    }

    #[test]
    fn a_pick_names_stencil_and_master() {
        let pick = StencilPick {
            stencil: "Basic Shapes",
            master: "Rectangle",
        };
        assert_eq!(pick.stencil, "Basic Shapes");
        assert_eq!(pick.master, "Rectangle");
    }

    #[test]
    fn the_window_lists_both_stencils_and_every_master() {
        let texts = painted_text(|ui| {
            let mut window = ShapesWindow::new();
            let app = AppCtx::new(larrygram_commands::Settings::default());
            let picks = window.show(ui, &app, Header::Classic).expect("show");
            assert!(picks.is_empty(), "no clicks in a scratch frame");
        });
        for stencil in stencils::catalog() {
            assert!(
                texts.iter().any(|text| text == stencil.name),
                "{}",
                stencil.name
            );
            for master in &stencil.masters {
                assert!(
                    texts.iter().any(|text| text == master.name),
                    "{} listed",
                    master.name
                );
            }
        }
    }

    #[test]
    fn a_master_row_selects_its_master() {
        let mut window = ShapesWindow::new();
        let pick = StencilPick {
            stencil: "Basic Shapes",
            master: "Rectangle",
        };
        with_ui(|ui| {
            let app = AppCtx::new(larrygram_commands::Settings::default());
            window.show(ui, &app, Header::Classic).expect("show");
        });
        window.select(pick);
        assert_eq!(window.selected(), Some(&pick));
        // The two states compare by selection, not by preview cache.
        let mut other = ShapesWindow::new();
        other.select(pick);
        assert_eq!(window, other);
    }

    #[test]
    fn every_master_gets_a_preview_slot() {
        let mut window = ShapesWindow::new();
        with_ui(|ui| {
            let app = AppCtx::new(larrygram_commands::Settings::default());
            window.show(ui, &app, Header::Classic).expect("show");
            // A second frame reuses the cached slots (or the failed
            // placeholder) instead of rasterizing again.
            window.show(ui, &app, Header::Classic).expect("show again");
        });
        let expected: usize = stencils::catalog()
            .iter()
            .map(|stencil| stencil.masters.len())
            .sum();
        assert_eq!(window.previews.len(), expected);
    }

    #[test]
    fn a_preview_document_holds_the_instance_on_a_page_the_master_size() {
        let master = stencils::basic_shapes().masters[0].clone();
        let document = preview_document(&master);
        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.pages[0].shapes.len(), 1);
        let cells = |name: &str| {
            document.pages[0]
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .and_then(|cell| match cell.value {
                    Some(CellValue::Number(value)) => Some(value),
                    _ => None,
                })
                .expect("page size cell")
        };
        assert_eq!(cells("PageWidth"), master.width);
        assert_eq!(cells("PageHeight"), master.height);
    }

    #[test]
    fn a_preview_raster_fits_the_master_box() {
        // Every master of both stencils rasterizes into one preview box.
        for stencil in stencils::catalog() {
            for master in &stencil.masters {
                let image = preview_image(master).expect("a preview raster");
                assert!(image.size[0] > 0 && image.size[1] > 0, "{}", master.name);
                assert!(
                    image.size[0] <= 41 && image.size[1] <= 41,
                    "one preview box: {}",
                    master.name
                );
            }
        }
    }
}
