//! The Shapes window: the stencil catalog of original masters
//! (ADR 0034).
//!
//! Owned by task P4-06. The window lists [`stencils::catalog`]: one
//! collapsible section per stencil, one tile per master with a small
//! preview. The preview is [`render_raster`] of a one-shape
//! scene built with [`stencils::instantiate`] (ADR 0033: the same pixels
//! as the PNG backends).
//!
//! Task P7-19: the whole tile (preview and label) is the hit target.
//! A click selects the master, a double-click or Enter asks the shell
//! to drop it at the page centre, and a drag starts a stencil drag
//! onto the page. The arrow keys move the selection while the window
//! is active, the whole header row collapses and expands a section,
//! and the search box filters the tiles as the user types.

use std::collections::{BTreeSet, HashMap};

use egui::{ColorImage, Context, Sense, TextureHandle, TextureId, TextureOptions, Ui};
use larrygram_commands::AppCtx;
use larrygram_model::stencils::{self, MasterDef, StencilDef};
use larrygram_model::{Cell, CellName, CellValue, Document};
use larrygram_render::{BuildOptions, RasterOptions, build_scene, render_raster};

use crate::UiError;

/// One preview box edge, in egui points.
const PREVIEW_PX: f32 = 40.0;

/// The space around a tile's preview, in points.
const TILE_PAD: f32 = 3.0;

/// The height of a stencil header row, in points.
const HEADER_HEIGHT: f32 = 24.0;

/// The Shapes window header, per layout (task P7-09 owns the header
/// and look).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Header {
    /// Panel Classic: a "Shapes" title row with a close button, then
    /// "Search for Shapes:" (docs/ui/panel-classic.md).
    #[default]
    Classic,
    /// Panel Modern: a "Shapes" title with the Stencils and Search
    /// headings (docs/ui/panel-modern.md).
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
    /// The master whose tile drag started this frame (task P5-17): the
    /// shell takes it and calls `stencil::begin_drag`.
    drag_started: Option<StencilPick>,
    /// The master a double-click or Enter asked to drop at the page
    /// centre (task P7-19): the shell takes it and dispatches
    /// `insert.shape`.
    drop_requested: Option<StencilPick>,
    /// The close button was pressed (Panel Classic): the shell
    /// collapses the window.
    close_requested: bool,
    /// The search box text: the tiles whose name contains it show.
    search: String,
    /// The stencils whose section the user collapsed.
    collapsed: BTreeSet<&'static str>,
    /// Whether the window has the keyboard (the user pressed in it
    /// last; the shell keeps it).
    active: bool,
    /// The keyboard moved the selection: scroll its tile into view.
    scroll_to_selected: bool,
    /// The search box id of the last frame (the keys stay with the box
    /// while it has the focus).
    search_id: Option<egui::Id>,
    /// The catalog, built once (it builds every master's geometry).
    catalog: Option<Vec<StencilDef>>,
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
            .field("drop_requested", &self.drop_requested)
            .field("search", &self.search)
            .field("collapsed", &self.collapsed)
            .field("active", &self.active)
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

/// Whether `master` matches the search `query` (case-insensitive, on
/// the display name or the unique name; an empty query matches all).
fn matches(master: &MasterDef, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || master.name.to_lowercase().contains(&query)
        || master.name_u.to_lowercase().contains(&query)
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

    /// Takes the master whose tile drag started since the last call
    /// (task P5-17). The shell starts a stencil drag with it.
    pub fn take_drag_started(&mut self) -> Option<StencilPick> {
        self.drag_started.take()
    }

    /// Takes the master a double-click or Enter asked to drop at the
    /// page centre (task P7-19).
    pub fn take_drop_requested(&mut self) -> Option<StencilPick> {
        self.drop_requested.take()
    }

    /// Takes the close button's press (Panel Classic, task P7-19).
    pub fn take_close_requested(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The search box text.
    pub fn search(&self) -> &str {
        &self.search
    }

    /// Replaces the search box text (the tiles filter at once).
    pub fn set_search(&mut self, text: &str) {
        text.clone_into(&mut self.search);
    }

    /// Whether the window has the keyboard.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Gives the window the keyboard, or takes it away (the shell calls
    /// this on every press: inside the window or elsewhere).
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Whether the section of `stencil` is collapsed.
    pub fn is_collapsed(&self, stencil: &str) -> bool {
        self.collapsed.contains(stencil)
    }

    /// The catalog, built on first use.
    fn catalog(&mut self) -> &[StencilDef] {
        self.catalog.get_or_insert_with(stencils::catalog)
    }

    /// The masters the window shows, in order: every master of each
    /// open section, or every match of the search (its sections open).
    pub fn visible(&mut self) -> Vec<StencilPick> {
        let query = self.search.clone();
        let searching = !query.trim().is_empty();
        let collapsed = self.collapsed.clone();
        self.catalog()
            .iter()
            .filter(|stencil| searching || !collapsed.contains(stencil.name))
            .flat_map(|stencil| {
                stencil
                    .masters
                    .iter()
                    .filter(|master| matches(master, &query))
                    .map(|master| StencilPick {
                        stencil: stencil.name,
                        master: master.name_u,
                    })
            })
            .collect()
    }

    /// Handles the window's keys while it is active and the search box
    /// does not have the focus: Up and Down move the selection through
    /// the visible tiles, Home and End jump to the first and last, and
    /// Enter asks to drop the selection at the page centre. The keys it
    /// uses are removed from the frame's input, so the shortcut table
    /// (which nudges shapes with the arrows) does not see them.
    pub fn handle_keys(&mut self, ctx: &Context) {
        if !self.active {
            return;
        }
        if self
            .search_id
            .is_some_and(|id| ctx.memory(|memory| memory.has_focus(id)))
        {
            return;
        }
        let keys = [
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::Home,
            egui::Key::End,
            egui::Key::Enter,
        ];
        let pressed: Vec<egui::Key> = ctx.input_mut(|input| {
            let mut pressed = Vec::new();
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: down,
                    modifiers,
                    ..
                } if keys.contains(key) && modifiers.is_none() => {
                    if *down {
                        pressed.push(*key);
                    }
                    false
                }
                _ => true,
            });
            pressed
        });
        for key in pressed {
            self.key(key);
        }
    }

    /// One key of [`ShapesWindow::handle_keys`].
    fn key(&mut self, key: egui::Key) {
        let visible = self.visible();
        if key == egui::Key::Enter {
            if let Some(pick) = self.selected.filter(|pick| visible.contains(pick)) {
                self.drop_requested = Some(pick);
            }
            return;
        }
        let Some(last) = visible.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected
            .and_then(|pick| visible.iter().position(|entry| *entry == pick));
        let next = match (key, current) {
            (egui::Key::Home, _) | (egui::Key::ArrowDown, None) => 0,
            (egui::Key::End, _) | (egui::Key::ArrowUp, None) => last,
            (egui::Key::ArrowDown, Some(index)) => (index + 1).min(last),
            (egui::Key::ArrowUp, Some(index)) => index.saturating_sub(1),
            _ => return,
        };
        if let Some(pick) = visible.get(next) {
            self.selected = Some(*pick);
            self.scroll_to_selected = true;
        }
    }

    /// Draws the header, the stencil sections and the master tiles
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
        // `app` is reserved (the shell drops the masters); the listing
        // and previews need no state.
        let _ = app;
        self.show_header(ui, header);
        let mut picks = Vec::new();
        let query = self.search.clone();
        let searching = !query.trim().is_empty();
        let stencils: Vec<(&'static str, Vec<MasterDef>)> = self
            .catalog()
            .iter()
            .map(|stencil| {
                let masters = stencil
                    .masters
                    .iter()
                    .filter(|master| matches(master, &query))
                    .cloned()
                    .collect();
                (stencil.name, masters)
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("shapes-window-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut shown = 0usize;
                for (stencil, masters) in &stencils {
                    let stencil: &'static str = stencil;
                    if searching && masters.is_empty() {
                        continue;
                    }
                    let open = searching || !self.collapsed.contains(stencil);
                    if self.section_header(ui, stencil, open, searching) {
                        if self.collapsed.contains(stencil) {
                            self.collapsed.remove(stencil);
                        } else {
                            self.collapsed.insert(stencil);
                        }
                        self.active = true;
                    }
                    if !open {
                        continue;
                    }
                    for master in masters {
                        shown += 1;
                        let pick = StencilPick {
                            stencil,
                            master: master.name_u,
                        };
                        let response = self.tile(ui, stencil, master, pick);
                        if response.clicked() {
                            self.select(pick);
                            self.active = true;
                            picks.push(pick);
                        }
                        if response.double_clicked() {
                            self.select(pick);
                            self.drop_requested = Some(pick);
                        }
                        if response.drag_started() {
                            self.select(pick);
                            self.active = true;
                            self.drag_started = Some(pick);
                        }
                    }
                }
                if searching && shown == 0 {
                    ui.add_space(6.0);
                    ui.weak("No shapes match the search.");
                }
            });
        Ok(picks)
    }

    /// Selects one master.
    fn select(&mut self, pick: StencilPick) {
        self.selected = Some(pick);
    }

    /// One stencil header row: the whole row toggles the section.
    /// Returns whether it was clicked (never while searching: the
    /// matches always show).
    fn section_header(&self, ui: &mut Ui, stencil: &str, open: bool, searching: bool) -> bool {
        let size = egui::vec2(ui.available_width(), HEADER_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, stencil)
        });
        let painter = ui.painter_at(rect);
        let fill = crate::theme::color_at(ui.ctx(), "pane.title.fill");
        painter.rect_filled(rect, 0.0, fill);
        if response.hovered() && !searching {
            painter.rect_filled(
                rect,
                2.0,
                crate::theme::color_at(ui.ctx(), "button.hover.fill"),
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let text = crate::theme::color_at(ui.ctx(), "pane.title.text");
        // The open/closed triangle, then the stencil name.
        let centre = egui::pos2(rect.min.x + 10.0, rect.center().y);
        let half = 4.0;
        let points = if open {
            vec![
                centre + egui::vec2(-half, -half * 0.6),
                centre + egui::vec2(half, -half * 0.6),
                centre + egui::vec2(0.0, half * 0.8),
            ]
        } else {
            vec![
                centre + egui::vec2(-half * 0.6, -half),
                centre + egui::vec2(half * 0.8, 0.0),
                centre + egui::vec2(-half * 0.6, half),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            text,
            egui::Stroke::NONE,
        ));
        painter.text(
            egui::pos2(rect.min.x + 20.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            stencil,
            egui::TextStyle::Body.resolve(ui.style()),
            text,
        );
        response.clicked() && !searching
    }

    /// One master tile: the preview and the label in one hit target,
    /// with the hover and selected looks (task P7-19).
    fn tile(
        &mut self,
        ui: &mut Ui,
        stencil: &'static str,
        master: &MasterDef,
        pick: StencilPick,
    ) -> egui::Response {
        let texture = self.preview_texture(ui.ctx(), stencil, master);
        let size = egui::vec2(ui.available_width(), PREVIEW_PX + 2.0 * TILE_PAD);
        // Click and drag, not focusable: egui's own arrow-key focus
        // moves would fight the window's arrow keys.
        let (rect, response) = ui.allocate_exact_size(size, Sense::CLICK | Sense::DRAG);
        let selected = self.selected == Some(pick);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, selected, master.name)
        });
        if selected && std::mem::take(&mut self.scroll_to_selected) {
            response.scroll_to_me(None);
        }
        let painter = ui.painter_at(rect);
        let hovered = response.hovered() || response.dragged();
        let look = if selected {
            Some(("button.pressed.fill", "button.pressed.border"))
        } else if hovered {
            Some(("button.hover.fill", "button.hover.border"))
        } else {
            None
        };
        if let Some((fill, border)) = look {
            let shape = rect.shrink(0.5);
            painter.rect_filled(shape, 2.0, crate::theme::color_at(ui.ctx(), fill));
            let width = if selected && self.active { 2.0 } else { 1.0 };
            painter.rect_stroke(
                shape,
                2.0,
                egui::Stroke::new(width, crate::theme::color_at(ui.ctx(), border)),
                egui::StrokeKind::Inside,
            );
        }
        if hovered {
            ui.ctx().set_cursor_icon(if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        let preview = egui::Rect::from_min_size(
            rect.min + egui::vec2(TILE_PAD + 2.0, TILE_PAD),
            egui::vec2(PREVIEW_PX, PREVIEW_PX),
        );
        match texture {
            Some(texture) => {
                let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                painter.image(texture, preview, uv, egui::Color32::WHITE);
            }
            None => {
                painter.rect_stroke(
                    preview,
                    0.0,
                    egui::Stroke::new(1.0, crate::theme::color_at(ui.ctx(), "chrome.border")),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let text = ui.visuals().text_color();
        painter.text(
            egui::pos2(preview.max.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            master.name,
            egui::TextStyle::Body.resolve(ui.style()),
            text,
        );
        response
    }

    /// The window header of the active layout (task P7-09): the
    /// classic title row and search box, or the modern titles. The
    /// close button collapses the window (task P7-19); the search box
    /// filters the tiles as the user types.
    fn show_header(&mut self, ui: &mut Ui, header: Header) {
        let title_fill = crate::theme::color_at(ui.ctx(), "pane.title.fill");
        let title_text = crate::theme::color_at(ui.ctx(), "pane.title.text");
        let band =
            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 24.0));
        ui.painter().rect_filled(band, 0.0, title_fill);
        let hint = match header {
            Header::Classic => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Shapes").color(title_text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("x")
                            .on_hover_text("Close the Shapes window")
                            .clicked()
                        {
                            self.close_requested = true;
                        }
                    });
                });
                // The label above the box: on one row they pushed the
                // window to 440 px, past its 190 px (task P7-17).
                ui.label("Search for Shapes:");
                "type a word"
            }
            Header::Modern => {
                // The pane title in the `strong` type role, the two
                // headings in title case (task P7-19, after the P7-18
                // design pass).
                let strong = crate::theme::font_at(ui.ctx(), "strong");
                ui.label(egui::RichText::new("Shapes").font(strong).color(title_text));
                ui.label(egui::RichText::new("Stencils").color(title_text));
                ui.label(egui::RichText::new("Search").color(title_text));
                "Search for shapes"
            }
        };
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        self.search_id = Some(response.id);
        if response.gained_focus() || response.clicked() {
            self.active = true;
        }
        ui.separator();
    }

    /// The preview texture of one master, rasterizing and caching it on
    /// first use; `None` when the preview could not be built.
    fn preview_texture(
        &mut self,
        ctx: &Context,
        stencil: &'static str,
        master: &MasterDef,
    ) -> Option<TextureId> {
        let key = (stencil, master.name_u);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.previews.entry(key) {
            let state = match preview_image(master) {
                Some(image) => {
                    let name = format!("stencil-preview-{stencil}-{}", master.name_u);
                    Preview::Ready(ctx.load_texture(name, image, TextureOptions::LINEAR))
                }
                None => Preview::Failed,
            };
            entry.insert(state);
        }
        match self.previews.get(&key) {
            Some(Preview::Ready(texture)) => Some(texture.id()),
            _ => None,
        }
    }
}

/// The margin around a preview's master, as a share of its larger
/// side: the outline's stroke straddles the shape's edge, and on a
/// page exactly the master's size its outer half was cut off (task
/// P7-17: Ellipse and Arc lost their top edge).
const PREVIEW_MARGIN: f64 = 0.05;

/// The margin of one master's preview, in inches.
fn preview_margin(master: &MasterDef) -> f64 {
    master.width.max(master.height) * PREVIEW_MARGIN
}

/// The one-master document a preview rasterizes: the instance
/// [`stencils::instantiate`] places, moved in by [`preview_margin`], on
/// a page the master's size plus the margin on each side, so the
/// raster is a close crop with the whole outline (ADR 0034).
fn preview_document(master: &MasterDef) -> Document {
    let mut document = Document::new();
    let id = stencils::instantiate(&mut document, master);
    let margin = preview_margin(master);
    if let Some(shape) = document.shape_mut(id) {
        for cell in &mut shape.sheet.cells {
            if matches!(cell.name.as_str(), "PinX" | "PinY")
                && let Some(CellValue::Number(value)) = cell.value
            {
                cell.value = Some(CellValue::Number(value + margin));
                cell.formula = None;
            }
        }
    }
    let mut width = Cell::new(CellName::from_static("PageWidth"));
    width.value = Some(CellValue::Number(master.width + 2.0 * margin));
    let mut height = Cell::new(CellName::from_static("PageHeight"));
    height.value = Some(CellValue::Number(master.height + 2.0 * margin));
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
    let scale =
        f64::from(PREVIEW_PX) / (master.width.max(master.height) + 2.0 * preview_margin(master));
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
    fn a_preview_document_holds_the_instance_on_a_page_the_master_size_plus_its_margin() {
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
        // The master plus the margin on each side (task P7-17), the
        // instance moved in by the margin.
        let margin = preview_margin(&master);
        assert!(margin > 0.0);
        assert_eq!(cells("PageWidth"), master.width + 2.0 * margin);
        assert_eq!(cells("PageHeight"), master.height + 2.0 * margin);
        let shape = document
            .shape(document.pages[0].shapes[0])
            .expect("the instance");
        let pin = |name: &str| {
            shape
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .and_then(|cell| match cell.value {
                    Some(CellValue::Number(value)) => Some(value),
                    _ => None,
                })
                .expect("pin cell")
        };
        assert_eq!(pin("PinX"), master.width * 0.5 + margin);
        assert_eq!(pin("PinY"), master.height * 0.5 + margin);
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
    fn pick(stencil: &'static str, master: &'static str) -> StencilPick {
        StencilPick { stencil, master }
    }

    #[test]
    fn the_search_matches_either_name_and_ignores_case() {
        let master = stencils::basic_shapes().masters[0].clone();
        assert!(matches(&master, ""));
        assert!(matches(&master, "   "));
        assert!(matches(&master, &master.name.to_uppercase()));
        assert!(matches(&master, &master.name_u[..2].to_lowercase()));
        assert!(!matches(&master, "no such master anywhere"));
    }

    #[test]
    fn the_visible_tiles_follow_the_sections_and_the_search() {
        let mut window = ShapesWindow::new();
        let all: usize = stencils::catalog()
            .iter()
            .map(|stencil| stencil.masters.len())
            .sum();
        assert_eq!(window.visible().len(), all);
        let first = stencils::catalog()[0].clone();
        window.collapsed.insert(first.name);
        assert!(window.is_collapsed(first.name));
        assert_eq!(window.visible().len(), all - first.masters.len());
        // A search opens every section and keeps the matches only.
        window.set_search("rect");
        assert_eq!(window.search(), "rect");
        let found = window.visible();
        assert!(!found.is_empty());
        assert!(found.iter().any(|entry| entry.stencil == first.name));
        assert!(found.len() < all);
        window.set_search("no such master anywhere");
        assert!(window.visible().is_empty());
    }

    #[test]
    fn the_keys_move_the_selection_and_enter_asks_for_a_drop() {
        let mut window = ShapesWindow::new();
        let visible = window.visible();
        let last = *visible.last().expect("masters");
        window.key(egui::Key::Enter);
        assert!(window.take_drop_requested().is_none(), "nothing selected");
        window.key(egui::Key::ArrowDown);
        assert_eq!(window.selected(), Some(&visible[0]));
        window.key(egui::Key::ArrowDown);
        assert_eq!(window.selected(), Some(&visible[1]));
        window.key(egui::Key::ArrowUp);
        window.key(egui::Key::ArrowUp);
        assert_eq!(window.selected(), Some(&visible[0]), "stops at the top");
        window.key(egui::Key::End);
        assert_eq!(window.selected(), Some(&last));
        window.key(egui::Key::ArrowDown);
        assert_eq!(window.selected(), Some(&last), "stops at the bottom");
        window.key(egui::Key::Home);
        assert_eq!(window.selected(), Some(&visible[0]));
        window.key(egui::Key::Enter);
        assert_eq!(window.take_drop_requested(), Some(visible[0]));
        assert!(window.take_drop_requested().is_none(), "taken once");
        // Up with nothing selected starts at the bottom; a key the
        // window does not use changes nothing.
        let mut window = ShapesWindow::new();
        window.key(egui::Key::ArrowUp);
        assert_eq!(window.selected(), Some(&last));
        window.key(egui::Key::A);
        assert_eq!(window.selected(), Some(&last));
        // With nothing visible the keys do nothing.
        let mut window = ShapesWindow::new();
        window.set_search("no such master anywhere");
        window.key(egui::Key::ArrowDown);
        assert!(window.selected().is_none());
        // A selection hidden by the search is not dropped.
        let mut window = ShapesWindow::new();
        window.select(pick("Basic Shapes", "Rectangle"));
        window.set_search("no such master anywhere");
        window.key(egui::Key::Enter);
        assert!(window.take_drop_requested().is_none());
    }

    #[test]
    fn handle_keys_takes_its_keys_only_while_active() {
        let ctx = egui::Context::default();
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut window = ShapesWindow::new();
        let run = |window: &mut ShapesWindow, events: Vec<egui::Event>| -> usize {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut left = 0;
            let mut output = ctx.run_ui(input, |ui| {
                window.handle_keys(ui.ctx());
                left = ui.input(|input| input.events.len());
            });
            output.textures_delta.clear();
            left
        };
        assert_eq!(
            run(&mut window, vec![key(egui::Key::ArrowDown)]),
            1,
            "inactive"
        );
        assert!(window.selected().is_none());
        window.set_active(true);
        assert!(window.is_active());
        let left = run(
            &mut window,
            vec![
                key(egui::Key::ArrowDown),
                key(egui::Key::Enter),
                key(egui::Key::A),
            ],
        );
        assert_eq!(left, 1, "the arrow and Enter are taken, A stays");
        assert!(window.selected().is_some());
        assert!(window.take_drop_requested().is_some());
    }

    #[test]
    fn the_close_button_request_is_taken_once() {
        let mut window = ShapesWindow::new();
        assert!(!window.take_close_requested());
        window.close_requested = true;
        assert!(window.take_close_requested());
        assert!(!window.take_close_requested());
        let text = format!("{window:?}");
        assert!(text.contains("ShapesWindow"));
    }

    #[test]
    fn the_modern_header_and_a_search_draw_the_matches_only() {
        let texts = painted_text(|ui| {
            let mut window = ShapesWindow::new();
            window.set_search("rect");
            let app = AppCtx::new(larrygram_commands::Settings::default());
            window.show(ui, &app, Header::Modern).expect("show");
        });
        assert!(texts.iter().any(|text| text == "Rectangle"));
        assert!(!texts.iter().any(|text| text == "Ellipse"), "filtered out");
        let texts = painted_text(|ui| {
            let mut window = ShapesWindow::new();
            window.set_search("no such master anywhere");
            let app = AppCtx::new(larrygram_commands::Settings::default());
            window.show(ui, &app, Header::Classic).expect("show");
        });
        assert!(texts.iter().any(|text| text.starts_with("No shapes match")));
        let texts = painted_text(|ui| {
            let mut window = ShapesWindow::new();
            for stencil in stencils::catalog() {
                window.collapsed.insert(stencil.name);
            }
            let app = AppCtx::new(larrygram_commands::Settings::default());
            window.show(ui, &app, Header::Classic).expect("show");
        });
        assert!(!texts.iter().any(|text| text == "Rectangle"), "collapsed");
        assert!(
            texts.iter().any(|text| text == "Basic Shapes"),
            "headers stay"
        );
    }
}
