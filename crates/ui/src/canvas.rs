//! The drawing canvas (ADR 0033): page raster, pan, zoom, rulers, grid,
//! page tabs and the status bar.
//!
//! Owned by task P4-05. The page renders with
//! [`larrygram_render::render_raster`] into one RGBA texture (the same
//! pixels the PNG backend writes) and the texture is drawn in a viewport
//! with an egui transform. The cache rules of ADR 0033:
//!
//! - the document changed or the page switched: re-rasterize;
//! - the zoom moved more than 10% from the rasterized zoom: re-rasterize
//!   (the texture is scaled on screen in between);
//! - panning never re-rasterizes (the texture transform carries it).
//!
//! # Coordinates
//!
//! The page raster has its top-left corner at
//! `viewport.min + pan * points_per_inch(zoom)`, where `pan` is
//! [`ViewState::pan`](larrygram_commands::ViewState) in inches (x right,
//! y down, the image sense) and one page inch is `96 * zoom` points at
//! the settled zoom (ADR 0025: 96 pixels per inch at zoom 1).
//!
//! # Units
//!
//! Internal units stay inches (AGENTS.md section 6). The rulers and the
//! status bar convert at this edge only, through [`format_length`] and
//! the `units` parameter of [`show_rulers`].

use egui::{
    Align2, Color32, ColorImage, Context, FontId, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, TextureId, TextureOptions, Ui,
};
use larrygram_commands::{
    AppCtx, CommandId, DisplayUnits, DocumentSession, Invocation, SessionId, Source,
};
use larrygram_geom::{Point, Rect as PageRect};
use larrygram_model::{CellName, CellReader, Document, Page, PageId, SheetRef};
use larrygram_render::{
    BuildOptions, MAX_DEVICE_EXTENT, Overlay, OverlayMark, RasterImage, RasterOptions, build_scene,
    render_raster,
};
use larrygram_ui_spec::v2::Theme;

use crate::handles::rotated_corners;
use crate::text_edit::{self, EditorAction, TextEditor};
use crate::tools::{Modifiers, ToolEvent};
use crate::{Intent, UiError, theme};

/// Points per inch at zoom 1.0 (ADR 0025: 96 pixels per inch, 1:1).
pub const POINTS_PER_INCH: f64 = 96.0;

/// The width of the ruler bands, in points.
pub const RULER_WIDTH: f32 = 18.0;

/// The grid spacing and the ruler tick spacing, in inches (our own
/// choice: the ruler ticks sit on the grid).
const TICK_IN: f64 = 0.125;

/// A major grid line and a labelled ruler tick every 8 ticks (1 inch).
const MAJOR_EVERY: i64 = 8;

/// The zoom range is the view commands' range, so a wheel zoom and a
/// `view.zoom.*` command clamp alike.
const MIN_ZOOM: f64 = larrygram_commands::builtins::ZOOM_MIN;

/// The largest zoom (see [`MIN_ZOOM`]).
const MAX_ZOOM: f64 = larrygram_commands::builtins::ZOOM_MAX;

/// Re-rasterize when the zoom moved more than this fraction from the
/// rasterized zoom (ADR 0033).
const ZOOM_RERASTERIZE: f64 = 0.10;

/// One wheel notch changes the zoom by this factor (our own choice).
const ZOOM_PER_WHEEL_LINE: f64 = 1.1;

/// The raster byte cap of the shared rasterizer (mirrors `png.rs`,
/// AGENTS.md section 6).
const MAX_RASTER_BYTES: u64 = 256 * 1024 * 1024;

// The page size when a page has no positive size: the scene builder's
// own fallback.
use larrygram_render::FALLBACK_PAGE_SIZE;

/// The smallest raster scale in pixels per inch (keeps a raster of any
/// zoomable page non-degenerate).
const MIN_RASTER_SCALE: f64 = 0.01;

/// The most grid lines or ruler ticks drawn per frame (AGENTS.md
/// section 6: bound input-derived work). Denser views draw no grid.
const MAX_TICKS: i64 = 4096;

const GRID_MINOR: Color32 = Color32::from_rgb(190, 190, 190);
const GRID_MAJOR: Color32 = Color32::from_rgb(160, 160, 160);

/// The canvas colours read from the active theme (`docs/ui/themes.md`).
///
/// The defaults preserve the pre-theme look when no theme was applied
/// yet: a grey backdrop, a white page, and a light ruler. The canvas
/// reads `canvas.background`, `page.background`, `page.border`,
/// `page.shadow`, `ruler.background` and `ruler.ticks`; the ruler labels
/// use the tick colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasColors {
    /// The area around the page (`canvas.background`).
    pub background: Color32,
    /// The page fill behind the raster (`page.background`).
    pub page_background: Color32,
    /// The one-point outline around the page (`page.border`).
    pub page_border: Color32,
    /// The offset shadow under the page (`page.shadow`).
    pub page_shadow: Color32,
    /// The background of both ruler bands (`ruler.background`).
    pub ruler_background: Color32,
    /// The ruler ticks and labels (`ruler.ticks`).
    pub ruler_ticks: Color32,
}

impl CanvasColors {
    /// Reads the canvas tokens from `theme`, falling back per token.
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            background: crate::theme::color(theme, "canvas.background"),
            page_background: crate::theme::color(theme, "page.background"),
            page_border: crate::theme::color(theme, "page.border"),
            page_shadow: crate::theme::color(theme, "page.shadow"),
            ruler_background: crate::theme::color(theme, "ruler.background"),
            ruler_ticks: crate::theme::color(theme, "ruler.ticks"),
        }
    }

    /// The active theme's colours, or [`CanvasColors::default`] before
    /// the shell applies a theme.
    fn active(ctx: &Context) -> Self {
        theme::current(ctx).map_or_else(Self::default, |theme| Self::from_theme(&theme))
    }
}

impl Default for CanvasColors {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(204, 204, 204),
            page_background: Color32::WHITE,
            page_border: Color32::from_rgb(169, 176, 184),
            page_shadow: Color32::from_rgb(234, 234, 234),
            ruler_background: Color32::from_rgb(232, 232, 232),
            ruler_ticks: Color32::from_rgb(120, 120, 120),
        }
    }
}

/// The raster texture cache of the canvas (ADR 0033).
///
/// Holds the uploaded texture, the last [`RasterImage`] and the key it
/// was rasterized for: the session identity, the document change counter,
/// the page, the page size and the raster zoom (ADR 0040 rule 3, the
/// P4-05 deferred item). The texture handle is dropped normally (its
/// free joins the next [`egui::TexturesDelta`], which the frame applies;
/// `epaint` only panics on deltas nobody applies).
#[derive(Clone)]
pub struct CanvasCache {
    texture: Option<TextureHandle>,
    raster: Option<RasterImage>,
    session: Option<SessionId>,
    changes: u64,
    page: Option<PageId>,
    page_size: (f64, f64),
    raster_zoom: f64,
    generation: u64,
    last_warning: Option<String>,
    failed: Option<FailedKey>,
    /// Whether the primary button went down on the canvas (the tool
    /// event stream's own state, not part of the raster key).
    pointer_down: bool,
}

/// The cache key of a raster attempt that failed (P4-16 review minor
/// 11): the page is not rasterized again until the key changes, so a
/// page that cannot raster does not rebuild and re-report every frame.
#[derive(Clone, PartialEq)]
struct FailedKey {
    session: Option<SessionId>,
    changes: u64,
    page: Option<PageId>,
    zoom: f64,
}

impl FailedKey {
    /// Whether this is the key of the given raster request.
    fn matches(
        &self,
        session: Option<SessionId>,
        changes: u64,
        page: Option<PageId>,
        zoom: f64,
    ) -> bool {
        self.session == session && self.changes == changes && self.page == page && self.zoom == zoom
    }
}

impl CanvasCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self {
            texture: None,
            raster: None,
            session: None,
            changes: 0,
            page: None,
            page_size: (0.0, 0.0),
            raster_zoom: 0.0,
            generation: 0,
            last_warning: None,
            failed: None,
            pointer_down: false,
        }
    }

    /// Drops the cached texture and key (document changed or closed).
    pub fn clear(&mut self) {
        self.texture = None;
        self.raster = None;
        self.session = None;
        self.changes = 0;
        self.page = None;
        self.page_size = (0.0, 0.0);
        self.raster_zoom = 0.0;
        self.generation = 0;
        self.last_warning = None;
        self.failed = None;
        self.pointer_down = false;
    }

    /// Whether no raster is cached.
    pub fn is_empty(&self) -> bool {
        self.raster.is_none()
    }

    /// How many times the cache rasterized (bumped on every
    /// re-rasterization, reset by [`CanvasCache::clear`]).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The last raster (the P4-09 exit-criterion test reads it).
    pub fn raster(&self) -> Option<&RasterImage> {
        self.raster.as_ref()
    }

    /// The uploaded texture, when one is cached.
    pub fn texture_id(&self) -> Option<TextureId> {
        self.texture.as_ref().map(TextureHandle::id)
    }

    /// The zoom the cached raster was rendered at (1.0 = 100%).
    pub fn raster_zoom(&self) -> f64 {
        self.raster_zoom
    }

    /// The rasterized page size, in inches.
    pub fn page_size(&self) -> (f64, f64) {
        self.page_size
    }

    /// Whether the key changed under the cache: the session, the
    /// document change counter or the page (ADR 0040 rule 3: the
    /// snapshot comparison is gone).
    fn is_stale(&self, session: &DocumentSession) -> bool {
        self.session != Some(session.id)
            || self.changes != session.changes
            || self.page != session.active_page
            || self.raster.is_none()
    }

    /// Whether the settled zoom moved more than 10% from the rasterized
    /// zoom (ADR 0033).
    fn zoom_is_far(&self, zoom: f64) -> bool {
        if self.raster.is_none() || self.raster_zoom <= 0.0 {
            return true;
        }
        ((zoom - self.raster_zoom) / self.raster_zoom).abs() > ZOOM_RERASTERIZE
    }
}

impl Default for CanvasCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for CanvasCache {
    fn eq(&self, other: &Self) -> bool {
        self.texture_id() == other.texture_id()
            && self.raster == other.raster
            && self.session == other.session
            && self.changes == other.changes
            && self.page == other.page
            && self.page_size == other.page_size
            && self.raster_zoom == other.raster_zoom
            && self.generation == other.generation
            && self.last_warning == other.last_warning
            && self.failed == other.failed
            && self.pointer_down == other.pointer_down
    }
}

impl std::fmt::Debug for CanvasCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasCache")
            .field("generation", &self.generation)
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("raster_zoom", &self.raster_zoom)
            .field(
                "raster",
                &self
                    .raster
                    .as_ref()
                    .map(|image| (image.width, image.height)),
            )
            .field("texture", &self.texture_id())
            .field("session", &self.session)
            .field("changes", &self.changes)
            .field("last_warning", &self.last_warning)
            .field("failed", &self.failed.is_some())
            .finish()
    }
}

/// What one canvas frame produced (ADR 0033).
#[derive(Debug, Default, PartialEq)]
pub struct CanvasOutput {
    /// Dispatch requests (zoom commands, page switches).
    pub intents: Vec<Intent>,
    /// Status bar text from this frame (page name, zoom, warnings).
    pub status: String,
    /// Pointer events in page inches for the active tool (ADR 0042).
    pub events: Vec<ToolEvent>,
    /// The id of the canvas widget (the shortcut focus scope reads it).
    pub focus_id: Option<egui::Id>,
}

/// The selection outline colour (our own choice).
const SELECTION: Color32 = Color32::from_rgb(31, 111, 235);
/// The handle fill colour (our own choice).
const HANDLE_FILL: Color32 = Color32::WHITE;
/// The snap guide colour (our own choice).
const SNAP_GUIDE: Color32 = Color32::from_rgb(220, 80, 60);
/// The marquee fill and stroke (our own choice).
const MARQUEE_FILL: Color32 = Color32::from_rgba_premultiplied(31, 111, 235, 24);
/// The handle square's half side, in screen points.
const HANDLE_HALF: f32 = 3.0;

/// Draws the canvas area: grid, page raster, pan and zoom input (ADR
/// 0033), and pointer events for the active tool (ADR 0042).
///
/// Middle-drag or space-drag pans; Ctrl + wheel (the platform zoom
/// modifier) zooms at the pointer; the canvas keys Ctrl + `+`, Ctrl + `-`
/// and Ctrl + `0` produce `view.zoom.in`/`view.zoom.out`/`view.zoom.fit`
/// intents (the menu items produce the same from the shell). The grid
/// draws behind the texture when [`larrygram_commands::ViewState::show_grid`].
pub fn show(
    ui: &mut Ui,
    session: &mut DocumentSession,
    cache: &mut CanvasCache,
) -> Result<CanvasOutput, UiError> {
    show_with_overlay(ui, session, cache, &Overlay::new())
}

/// [`show`] with the transient tool overlay drawn over the cached raster
/// (ADR 0042: a drag never re-rasterizes the page).
pub fn show_with_overlay(
    ui: &mut Ui,
    session: &mut DocumentSession,
    cache: &mut CanvasCache,
    overlay: &Overlay,
) -> Result<CanvasOutput, UiError> {
    let mut output = CanvasOutput::default();
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let viewport = viewport_rect(rect, session.view.show_rulers);
    output.focus_id = Some(response.id);

    // Pan: middle-drag or space-drag.
    let space_down = ui.input(|input| input.keys_down.contains(&egui::Key::Space));
    let panning = response.dragged_by(PointerButton::Middle)
        || (space_down && response.dragged_by(PointerButton::Primary));
    if panning {
        let delta = response.drag_delta();
        let k = points_per_inch(safe_zoom(session.view.zoom));
        session.view.pan = Point::new(
            session.view.pan.x + f64::from(delta.x) / k,
            session.view.pan.y + f64::from(delta.y) / k,
        );
    }

    // Zoom: Ctrl + wheel at the pointer.
    let factor = wheel_zoom_factor(ui, &response);
    if factor != 1.0 {
        let anchor = response.hover_pos().unwrap_or(rect.center());
        zoom_at_pointer(session, viewport, anchor, factor);
    }

    // Zoom commands from the canvas keys.
    if response.hovered() {
        output.intents = canvas_key_intents(ui)?;
    }

    // Rasterize on cache-key change or a settled zoom far from the
    // rasterized one (the cache rules of ADR 0033).
    let zoom = safe_zoom(session.view.zoom);
    if cache.is_stale(session) || cache.zoom_is_far(zoom) {
        rasterize(ui.ctx(), session, cache)?;
    }
    let page_height = page_height(session, cache);

    // Pointer events for the active tool, in page inches (ADR 0042).
    if !panning && !space_down {
        output.events = tool_events(ui, &response, viewport, session, cache, page_height);
    } else {
        cache.pointer_down = false;
    }

    // Paint: backdrop, then the grid, then the page, then the overlay.
    let colors = CanvasColors::active(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, colors.background);
    let k = points_per_inch(zoom);
    if session.view.show_grid {
        draw_grid(&painter, viewport, session.view.pan, k);
    }
    if let Some(texture) = cache.texture_id() {
        let origin = egui::pos2(
            viewport.min.x + (session.view.pan.x * k) as f32,
            viewport.min.y + (session.view.pan.y * k) as f32,
        );
        let size = egui::vec2(
            (cache.page_size.0 * k) as f32,
            (cache.page_size.1 * k) as f32,
        );
        let dest = Rect::from_min_size(origin, size);
        if dest.is_positive() {
            let shadow = Rect::from_min_size(dest.min + egui::vec2(3.0, 3.0), dest.size());
            painter.rect_filled(shadow, 0.0, colors.page_shadow);
            painter.rect_filled(dest, 0.0, colors.page_background);
            let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
            painter.image(texture, dest, uv, Color32::WHITE);
            painter.rect_stroke(
                dest,
                0.0,
                Stroke::new(1.0, colors.page_border),
                StrokeKind::Inside,
            );
        }
    }
    draw_overlay(
        &painter,
        viewport,
        session.view.pan,
        k,
        page_height,
        overlay,
    );

    output.status = canvas_status(session, cache);
    Ok(output)
}

/// Draws the in-place text editor over the canvas (task P5-17, Goal 5).
///
/// `rect` is the canvas area of this frame; the editor's text block is
/// mapped through the canvas viewport, pan and zoom. The caller owns the
/// [`TextEditor`] and acts on the returned [`EditorAction`]. A shape
/// that is no longer in the scene draws nothing this frame.
pub fn show_text_editor(
    ui: &mut Ui,
    session: &DocumentSession,
    rect: Rect,
    editor: &mut TextEditor,
) -> Result<EditorAction, UiError> {
    let Some(shape) = editor.shape else {
        return Ok(EditorAction::Pending);
    };
    let Some(page) = session.active_page else {
        return Ok(EditorAction::Pending);
    };
    let viewport = viewport_rect(rect, session.view.show_rulers);
    let scene = build_scene(&session.document, page, &BuildOptions::new()).map_err(|error| {
        UiError::Io {
            message: error.to_string(),
        }
    })?;
    let Some(editor_rect) = text_edit::editor_rect(
        &scene,
        shape,
        viewport,
        session.view.pan,
        safe_zoom(session.view.zoom),
    ) else {
        return Ok(EditorAction::Pending);
    };
    Ok(editor.show(ui, editor_rect))
}

/// Maps a screen point over the canvas area to page inches (task P5-17:
/// the shell feeds a Shapes-window drag to the stencil tool, which needs
/// the same mapping the canvas events use). `None` when the point is
/// outside the canvas viewport or a view value is unusable.
pub fn page_position(rect: Rect, session: &DocumentSession, screen: Pos2) -> Option<Point> {
    let viewport = viewport_rect(rect, session.view.show_rulers);
    if !viewport.contains(screen) {
        return None;
    }
    let zoom = safe_zoom(session.view.zoom);
    let k = points_per_inch(zoom);
    let height = page_size(&session.document, session.active_page).1;
    let point = Point::new(
        (f64::from(screen.x) - f64::from(viewport.min.x)) / k - session.view.pan.x,
        height - ((f64::from(screen.y) - f64::from(viewport.min.y)) / k - session.view.pan.y),
    );
    if point.x.is_finite() && point.y.is_finite() {
        Some(point)
    } else {
        None
    }
}

/// The page height the pointer mapping needs: the rasterized page size,
/// else the model page size.
fn page_height(session: &DocumentSession, cache: &CanvasCache) -> f64 {
    if cache.page_size.1 > 0.0 {
        cache.page_size.1
    } else {
        page_size(&session.document, session.active_page).1
    }
}

/// The pointer events of one frame, in page inches.
fn tool_events(
    ui: &Ui,
    response: &egui::Response,
    viewport: Rect,
    session: &DocumentSession,
    cache: &mut CanvasCache,
    page_height: f64,
) -> Vec<ToolEvent> {
    let (pressed, released, down, pos, modifiers) = ui.input(|input| {
        (
            input.pointer.button_pressed(PointerButton::Primary),
            input.pointer.button_released(PointerButton::Primary),
            input.pointer.primary_down(),
            input.pointer.interact_pos(),
            input.modifiers,
        )
    });
    // Outside the `input` closure: `double_clicked_by` reads the context
    // again and would deadlock the input lock.
    let double = response.double_clicked_by(PointerButton::Primary);
    let modifiers = Modifiers {
        shift: modifiers.shift,
        ctrl: modifiers.ctrl || modifiers.command,
        alt: modifiers.alt,
    };
    let zoom = safe_zoom(session.view.zoom);
    let k = points_per_inch(zoom);
    let mut events = Vec::new();
    let to_page = |screen: Pos2| {
        Point::new(
            (f64::from(screen.x) - f64::from(viewport.min.x)) / k - session.view.pan.x,
            page_height
                - ((f64::from(screen.y) - f64::from(viewport.min.y)) / k - session.view.pan.y),
        )
    };
    if pressed {
        if let Some(screen) = pos
            && viewport.contains(screen)
        {
            cache.pointer_down = true;
            events.push(ToolEvent::PointerDown {
                pos: to_page(screen),
                modifiers,
            });
        }
    } else if cache.pointer_down
        && down
        && let Some(screen) = pos
        && viewport.contains(screen)
    {
        events.push(ToolEvent::PointerMove {
            pos: to_page(screen),
            modifiers,
        });
    }
    if double
        && let Some(screen) = pos
        && viewport.contains(screen)
    {
        events.push(ToolEvent::DoubleClick {
            pos: to_page(screen),
        });
    }
    if released && cache.pointer_down {
        cache.pointer_down = false;
        if let Some(screen) = pos {
            events.push(ToolEvent::PointerUp {
                pos: to_page(screen),
                modifiers,
            });
        }
    }
    events
}

/// Draws the transient overlay marks over the cached raster (ADR 0042).
fn draw_overlay(
    painter: &egui::Painter,
    viewport: Rect,
    pan: Point,
    k: f64,
    page_height: f64,
    overlay: &Overlay,
) {
    let to_screen = |point: Point| {
        egui::pos2(
            viewport.min.x + ((pan.x + point.x) * k) as f32,
            viewport.min.y + ((pan.y + (page_height - point.y)) * k) as f32,
        )
    };
    for mark in &overlay.marks {
        match mark {
            OverlayMark::SelectionOutline { bounds, angle } => {
                let corners = rotated_corners(*bounds, *angle);
                let points: Vec<Pos2> = corners.iter().map(|point| to_screen(*point)).collect();
                if points.len() == 4 {
                    painter.add(egui::Shape::closed_line(
                        points,
                        Stroke::new(1.0, SELECTION),
                    ));
                }
            }
            OverlayMark::Handles { bounds, angle } => {
                if let Ok(handles) = crate::handles::handles_for(*bounds, *angle) {
                    for (_, point) in handles {
                        let center = to_screen(point);
                        let rect = Rect::from_center_size(
                            center,
                            egui::vec2(HANDLE_HALF * 2.0, HANDLE_HALF * 2.0),
                        );
                        painter.rect_filled(rect, 0.0, HANDLE_FILL);
                        painter.rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0, SELECTION),
                            egui::StrokeKind::Middle,
                        );
                    }
                }
            }
            OverlayMark::RubberBand(bounds) => {
                let min = to_screen(bounds.min);
                let max = to_screen(PageRect::new(bounds.min, bounds.max).max);
                let rect = Rect::from_two_pos(min, max);
                painter.rect_filled(rect, 0.0, MARQUEE_FILL);
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, SELECTION),
                    egui::StrokeKind::Middle,
                );
            }
            OverlayMark::SnapGuide { a, b } => {
                painter.line_segment([to_screen(*a), to_screen(*b)], Stroke::new(1.0, SNAP_GUIDE));
            }
            OverlayMark::ConnectorPreview { points } => {
                let points: Vec<Pos2> = points.iter().map(|point| to_screen(*point)).collect();
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(points, Stroke::new(1.5, SELECTION)));
                }
            }
            _ => {}
        }
    }
}

/// Draws the ruler bands along the top and left edges of the canvas
/// (ADR 0033).
///
/// Ticks sit at 1/8 inch in page space; the labels are in `units`
/// (AGENTS.md section 6: conversion at this edge only). The bands are
/// laid out around the same viewport [`show`] uses, so the shell can
/// give both functions the same area. **Deviation (P4-05):** the stub
/// signature carried no display units; `units` is added so the ruler
/// labels honour `Settings::units`.
pub fn show_rulers(
    ui: &mut Ui,
    session: &DocumentSession,
    units: DisplayUnits,
) -> Result<(), UiError> {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    if !session.view.show_rulers {
        return Ok(());
    }
    let viewport = viewport_rect(rect, true);
    let colors = CanvasColors::active(ui.ctx());
    let painter = ui.painter_at(rect);
    let zoom = safe_zoom(session.view.zoom);
    let k = points_per_inch(zoom);
    let pan = session.view.pan;

    let top = Rect::from_min_max(rect.min, egui::pos2(rect.max.x, viewport.min.y));
    let left = Rect::from_min_max(rect.min, egui::pos2(viewport.min.x, rect.max.y));
    painter.rect_filled(top, 0.0, colors.ruler_background);
    painter.rect_filled(left, 0.0, colors.ruler_background);

    for (x, major) in tick_positions(viewport.min.x, viewport.width(), pan.x, k) {
        let length = if major {
            RULER_WIDTH * 0.6
        } else {
            RULER_WIDTH * 0.25
        };
        painter.line_segment(
            [
                egui::pos2(x, viewport.min.y),
                egui::pos2(x, viewport.min.y - length),
            ],
            Stroke::new(1.0, colors.ruler_ticks),
        );
        if major {
            let inches = (f64::from(x) - f64::from(viewport.min.x)) / k - pan.x;
            painter.text(
                egui::pos2(x + 2.0, viewport.min.y - RULER_WIDTH),
                Align2::LEFT_TOP,
                format_number(converted(inches, units)),
                FontId::proportional(9.0),
                colors.ruler_ticks,
            );
        }
    }
    for (y, major) in tick_positions(viewport.min.y, viewport.height(), pan.y, k) {
        let length = if major {
            RULER_WIDTH * 0.6
        } else {
            RULER_WIDTH * 0.25
        };
        painter.line_segment(
            [
                egui::pos2(viewport.min.x, y),
                egui::pos2(viewport.min.x - length, y),
            ],
            Stroke::new(1.0, colors.ruler_ticks),
        );
        if major {
            let inches = (f64::from(y) - f64::from(viewport.min.y)) / k - pan.y;
            painter.text(
                egui::pos2(viewport.min.x - 2.0, y),
                Align2::RIGHT_CENTER,
                format_number(converted(inches, units)),
                FontId::proportional(9.0),
                colors.ruler_ticks,
            );
        }
    }
    Ok(())
}

/// Draws the page tab strip under the canvas (ADR 0033).
///
/// Clicking a tab switches `active_page` directly; the strip's prev and
/// next buttons produce `page.previous`/`page.next` intents for the
/// command registry (the same change the shortcuts and the control
/// channel run).
pub fn show_page_tabs(ui: &mut Ui, session: &mut DocumentSession) -> Result<Vec<Intent>, UiError> {
    let pages: Vec<(PageId, String)> = session
        .document
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.id, page_label(page, index)))
        .collect();
    let active = session.active_page;
    let mut previous = false;
    let mut next = false;
    let mut clicked = None;
    let mut context_action: Option<(PageId, &'static str)> = None;
    ui.horizontal_wrapped(|ui| {
        previous = ui.button("<").on_hover_text("Previous page").clicked();
        for (id, label) in &pages {
            let response = ui
                .selectable_label(active == Some(*id), label)
                .on_hover_text("Show this page");
            if response.clicked() {
                clicked = Some(*id);
            }
            response.context_menu(|ui| {
                for (label, command) in [
                    ("Rename Page", "page.rename"),
                    ("Delete Page", "page.delete"),
                    ("Move Page Left", "page.move_left"),
                    ("Move Page Right", "page.move_right"),
                ] {
                    if ui.button(label).clicked() {
                        context_action = Some((*id, command));
                        ui.close();
                    }
                }
            });
        }
        next = ui.button(">").on_hover_text("Next page").clicked();
    });
    if let Some(id) = clicked {
        session.active_page = Some(id);
    }
    let mut intents = Vec::new();
    if let Some((id, command)) = context_action {
        session.active_page = Some(id);
        intents.push(intent(command)?);
    }
    if previous {
        intents.push(intent("page.previous")?);
    }
    if next {
        intents.push(intent("page.next")?);
    }
    Ok(intents)
}

/// Draws the status bar (page, units, zoom, last warning; ADR 0033).
///
/// Shows the active page's name and size (the size formatted in
/// [`Settings::units`](larrygram_commands::Settings)), the zoom percent
/// and the last [`AppCtx::warnings`] entry.
pub fn show_status_bar(ui: &mut Ui, app: &AppCtx) -> Result<(), UiError> {
    let units = app.settings.units;
    let mut text = match app.active_document() {
        Some(session) => {
            let name = active_page_label(session);
            let (width, height) = page_size(&session.document, session.active_page);
            format!(
                "{name}  {} x {}  {}",
                format_length(width, units),
                format_length(height, units),
                zoom_percent(safe_zoom(session.view.zoom)),
            )
        }
        None => "No document".to_owned(),
    };
    if let Some(warning) = app.warnings.last() {
        text.push_str("  ");
        text.push_str(warning);
    }
    ui.horizontal(|ui| {
        ui.label(text);
    });
    Ok(())
}

/// Formats a length in inches for display in `units` (AGENTS.md section
/// 6: conversion at this edge only).
///
/// ```
/// # use larrygram_commands::DisplayUnits;
/// # use larrygram_ui::canvas::format_length;
/// assert_eq!(format_length(8.5, DisplayUnits::Inches), "8.5 in");
/// assert_eq!(format_length(8.5, DisplayUnits::Millimeters), "215.9 mm");
/// ```
pub fn format_length(inches: f64, units: DisplayUnits) -> String {
    let suffix = match units {
        DisplayUnits::Millimeters => "mm",
        DisplayUnits::Centimeters => "cm",
        DisplayUnits::Meters => "m",
        DisplayUnits::Feet => "ft",
        _ => "in",
    };
    format!("{} {}", format_number(converted(inches, units)), suffix)
}

/// Formats a number with at most three decimals and no trailing zeros.
/// A non-finite value shows as `?`.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "?".to_owned();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.is_finite() {
        format!("{rounded}")
    } else {
        format!("{value}")
    }
}

/// The display value of a length in inches, in `units`.
fn converted(inches: f64, units: DisplayUnits) -> f64 {
    match units {
        DisplayUnits::Millimeters => inches * 25.4,
        DisplayUnits::Centimeters => inches * 2.54,
        DisplayUnits::Meters => inches * 0.0254,
        DisplayUnits::Feet => inches / 12.0,
        _ => inches,
    }
}

/// The zoom percentage, e.g. `"100%"`.
fn zoom_percent(zoom: f64) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// The label of a page: its `Name`, else its `NameU`, else `Page N`.
pub(crate) fn page_label(page: &Page, index: usize) -> String {
    if !page.name.is_empty() {
        page.name.clone()
    } else if !page.name_u.is_empty() {
        page.name_u.clone()
    } else {
        format!("Page {}", index + 1)
    }
}

/// The label of the active page of a session.
fn active_page_label(session: &DocumentSession) -> String {
    session
        .active_page
        .and_then(|id| {
            session
                .document
                .pages
                .iter()
                .enumerate()
                .find(|(_, page)| page.id == id)
        })
        .map(|(index, page)| page_label(page, index))
        .unwrap_or_else(|| "No page".to_owned())
}

/// The page size in inches: `PageWidth`/`PageHeight` when positive, the
/// scene builder's fallback otherwise.
fn page_size(doc: &Document, page: Option<PageId>) -> (f64, f64) {
    let Some(page) = page else {
        return FALLBACK_PAGE_SIZE;
    };
    let mut reader = CellReader::new(doc);
    let width = positive(
        reader
            .number(SheetRef::Page(page), &CellName::new("PageWidth"))
            .ok(),
        FALLBACK_PAGE_SIZE.0,
    );
    let height = positive(
        reader
            .number(SheetRef::Page(page), &CellName::new("PageHeight"))
            .ok(),
        FALLBACK_PAGE_SIZE.1,
    );
    (width, height)
}

/// The value when it is finite and positive, `fallback` otherwise.
fn positive(value: Option<f64>, fallback: f64) -> f64 {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => value,
        _ => fallback,
    }
}

/// The canvas viewport inside the widget rect: inset by the ruler bands
/// when the rulers show.
fn viewport_rect(rect: Rect, show_rulers: bool) -> Rect {
    if show_rulers {
        Rect::from_min_max(rect.min + egui::vec2(RULER_WIDTH, RULER_WIDTH), rect.max)
    } else {
        rect
    }
}

/// Points per one page inch at `zoom` (ADR 0025: 96 at zoom 1).
fn points_per_inch(zoom: f64) -> f64 {
    POINTS_PER_INCH * zoom
}

/// The zoom as a finite, positive number in `[MIN_ZOOM, MAX_ZOOM]`.
fn safe_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() && zoom > 0.0 {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// The zoom factor from Ctrl + wheel and pinch events over the canvas
/// (1.0 when the pointer is elsewhere or nothing zooms).
fn wheel_zoom_factor(ui: &Ui, response: &egui::Response) -> f64 {
    if !response.hovered() {
        return 1.0;
    }
    ui.input(|input| {
        let mut factor = 1.0;
        for event in &input.events {
            match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if modifiers.ctrl || modifiers.command => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => f64::from(delta.x + delta.y),
                        egui::MouseWheelUnit::Page => f64::from(delta.x + delta.y) * 8.0,
                        _ => f64::from(delta.x + delta.y) / 24.0,
                    };
                    factor *= ZOOM_PER_WHEEL_LINE.powf(lines);
                }
                egui::Event::Zoom(zoom) => factor *= f64::from(*zoom),
                _ => {}
            }
        }
        factor
    })
}

/// Zooms the view by `factor`, keeping the page point under `anchor`
/// fixed on screen (ADR 0033: zoom at the pointer).
fn zoom_at_pointer(session: &mut DocumentSession, viewport: Rect, anchor: Pos2, factor: f64) {
    let old_zoom = safe_zoom(session.view.zoom);
    let zoom = safe_zoom(old_zoom * factor);
    let old_k = points_per_inch(old_zoom);
    let new_k = points_per_inch(zoom);
    let dx = f64::from(anchor.x) - f64::from(viewport.min.x);
    let dy = f64::from(anchor.y) - f64::from(viewport.min.y);
    let page_x = dx / old_k - session.view.pan.x;
    let page_y = dy / old_k - session.view.pan.y;
    session.view.zoom = zoom;
    session.view.pan = Point::new(dx / new_k - page_x, dy / new_k - page_y);
}

/// The zoom command intents of the canvas keys (Ctrl + `+`/`=`, Ctrl +
/// `-`, Ctrl + `0`; our own binding for `view.zoom.fit`, `shortcuts.yaml`
/// has none). The keys are consumed, so the shortcut layer never
/// double-dispatches them.
fn canvas_key_intents(ui: &mut Ui) -> Result<Vec<Intent>, UiError> {
    let (zoom_in, zoom_out, zoom_fit) = ui.input_mut(|input| {
        let zoom_in = input.consume_key(egui::Modifiers::CTRL, egui::Key::Plus)
            | input.consume_key(egui::Modifiers::CTRL, egui::Key::Equals);
        let zoom_out = input.consume_key(egui::Modifiers::CTRL, egui::Key::Minus);
        let zoom_fit = input.consume_key(egui::Modifiers::CTRL, egui::Key::Num0);
        (zoom_in, zoom_out, zoom_fit)
    });
    let mut intents = Vec::new();
    if zoom_in {
        intents.push(intent("view.zoom.in")?);
    }
    if zoom_out {
        intents.push(intent("view.zoom.out")?);
    }
    if zoom_fit {
        intents.push(intent("view.zoom.fit")?);
    }
    Ok(intents)
}

/// One dispatch request for a static command id.
fn intent(id: &'static str) -> Result<Intent, UiError> {
    let command = CommandId::parse(id).map_err(|error| UiError::Io {
        message: format!("the canvas command id {id} is not valid: {error}"),
    })?;
    Ok(Intent::new(command, Invocation::empty(Source::Ui)))
}

/// Rasterizes the active page into the cache (ADR 0033: `render_raster`
/// at `zoom * 96` pixels per inch).
///
/// A page whose raster failed is not retried every frame: the failed
/// key is remembered and the attempt runs again only when the key
/// (document, page, zoom) changes (P4-16 review minor 11), so the
/// failure is reported once, not once per frame.
fn rasterize(
    ctx: &Context,
    session: &DocumentSession,
    cache: &mut CanvasCache,
) -> Result<(), UiError> {
    let Some(page) = session.active_page else {
        cache.clear();
        return Ok(());
    };
    let zoom = safe_zoom(session.view.zoom);
    if cache
        .failed
        .as_ref()
        .is_some_and(|failed| failed.matches(Some(session.id), session.changes, Some(page), zoom))
    {
        return Ok(());
    }
    match rasterize_at(ctx, session, cache) {
        Ok(()) => {
            cache.failed = None;
            Ok(())
        }
        Err(error) => {
            cache.failed = Some(FailedKey {
                session: Some(session.id),
                changes: session.changes,
                page: Some(page),
                zoom,
            });
            Err(error)
        }
    }
}

/// Builds the scene of the active page and uploads it as a texture
/// (the raster attempt behind [`rasterize`]).
fn rasterize_at(
    ctx: &Context,
    session: &DocumentSession,
    cache: &mut CanvasCache,
) -> Result<(), UiError> {
    let Some(page) = session.active_page else {
        cache.clear();
        return Ok(());
    };
    let scene = build_scene(&session.document, page, &BuildOptions::new()).map_err(render_error)?;
    let wanted = POINTS_PER_INCH * safe_zoom(session.view.zoom);
    let scale = clamp_scale(ctx, scene.width, scene.height, wanted);
    let raster = render_raster(
        &scene,
        &RasterOptions {
            scale,
            antialias: true,
        },
    )
    .map_err(render_error)?;
    let width = raster.width as usize;
    let height = raster.height as usize;
    if width.checked_mul(height).and_then(|px| px.checked_mul(4)) != Some(raster.rgba.len()) {
        return Err(UiError::Io {
            message: "the canvas raster has an inconsistent size".to_owned(),
        });
    }
    let image = ColorImage::from_rgba_unmultiplied([width, height], &raster.rgba);
    let texture = ctx.load_texture("larrygram-canvas-page", image, TextureOptions::LINEAR);
    cache.texture = Some(texture);
    cache.page_size = (scene.width, scene.height);
    cache.last_warning = scene
        .warnings
        .last()
        .map(|warning| format!("{}: {:?}", warning.cell.as_str(), warning.kind));
    cache.raster = Some(raster);
    cache.session = Some(session.id);
    cache.changes = session.changes;
    cache.page = Some(page);
    cache.raster_zoom = safe_zoom(session.view.zoom);
    cache.generation = cache.generation.saturating_add(1);
    Ok(())
}

/// The raster scale under the ADR 0025 caps and the backend texture
/// limit: `wanted` pixels per inch, clamped so the page always fits.
fn clamp_scale(ctx: &Context, width_in: f64, height_in: f64, wanted: f64) -> f64 {
    let width = positive(Some(width_in), FALLBACK_PAGE_SIZE.0);
    let height = positive(Some(height_in), FALLBACK_PAGE_SIZE.1);
    let longest = width.max(height);
    let side_cap = f64::from(MAX_DEVICE_EXTENT) / longest;
    let area_cap = ((MAX_RASTER_BYTES / 4) as f64 / (width * height)).sqrt();
    let texture_side = ctx.input(|input| input.max_texture_side) as f64;
    let texture_cap = texture_side.max(1.0) / longest;
    let cap = side_cap.min(area_cap).min(texture_cap);
    let wanted = positive(Some(wanted), POINTS_PER_INCH);
    let floor = MIN_RASTER_SCALE.min(cap);
    wanted.clamp(floor, cap)
}

/// The grid behind the texture (ADR 0033): lines at [`TICK_IN`] inches,
/// a stronger line every inch.
fn draw_grid(painter: &egui::Painter, viewport: Rect, pan: Point, k: f64) {
    for (x, major) in tick_positions(viewport.min.x, viewport.width(), pan.x, k) {
        painter.line_segment(
            [egui::pos2(x, viewport.min.y), egui::pos2(x, viewport.max.y)],
            Stroke::new(1.0, if major { GRID_MAJOR } else { GRID_MINOR }),
        );
    }
    for (y, major) in tick_positions(viewport.min.y, viewport.height(), pan.y, k) {
        painter.line_segment(
            [egui::pos2(viewport.min.x, y), egui::pos2(viewport.max.x, y)],
            Stroke::new(1.0, if major { GRID_MAJOR } else { GRID_MINOR }),
        );
    }
}

/// The visible tick positions along one axis: the screen coordinate and
/// whether the tick is major (every [`MAJOR_EVERY`] ticks, one inch).
/// Draws nothing when the ticks would be denser than [`MAX_TICKS`].
fn tick_positions(start: f32, length: f32, pan: f64, k: f64) -> Vec<(f32, bool)> {
    let mut positions = Vec::new();
    if !(k.is_finite() && k > 0.0 && length > 0.0) {
        return positions;
    }
    let first = -pan / TICK_IN;
    let last = (f64::from(length) / k - pan) / TICK_IN;
    if !(first.is_finite() && last.is_finite()) {
        return positions;
    }
    let first = first.floor() as i64;
    let last = last.ceil() as i64;
    let total = last.saturating_sub(first).saturating_add(1);
    if total <= 0 || total > MAX_TICKS {
        return positions;
    }
    for tick in first..=last {
        let pos = f64::from(start) + (pan + (tick as f64) * TICK_IN) * k;
        if pos.is_finite() {
            positions.push((pos as f32, tick.rem_euclid(MAJOR_EVERY) == 0));
        }
    }
    positions
}

/// The status text of one canvas frame (page name, zoom, last warning).
fn canvas_status(session: &DocumentSession, cache: &CanvasCache) -> String {
    let mut status = active_page_label(session);
    status.push_str("  ");
    status.push_str(&zoom_percent(safe_zoom(session.view.zoom)));
    if let Some(warning) = &cache.last_warning {
        status.push_str("  ");
        status.push_str(warning);
    }
    status
}

/// A build or raster failure as a [`UiError`] (task P4-03 added the
/// render variant the P4-05 Result asked for).
fn render_error(error: larrygram_render::Error) -> UiError {
    UiError::Render {
        message: format!("the canvas raster failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::{Cell, CellValue, Row, Section, SectionName, Shape, ShapeId};

    /// Runs one frame over `f` on `ctx` and drops the texture deltas
    /// safely (epaint panics on unapplied deltas).
    fn frame<R>(ctx: &Context, input: egui::RawInput, f: impl FnOnce(&mut Ui) -> R) -> R {
        let mut out = None;
        let mut f = Some(f);
        let mut output = ctx.run_ui(input, |ui| {
            if let Some(func) = f.take() {
                out = Some(func(ui));
            }
        });
        output.textures_delta.clear();
        out.expect("the frame did not run")
    }

    /// A frame input with a working viewport and the pointer at `pos`.
    fn input_with_pointer(pos: Pos2) -> egui::RawInput {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 400.0));
        let mut input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        if let Some(info) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            info.inner_rect = Some(rect);
        }
        input.events.push(egui::Event::PointerMoved(pos));
        input
    }

    /// One cell with a cached value.
    fn cell(name: &str, value: CellValue) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(value);
        cell
    }

    /// A document with one page (8.5 x 11 in) and one red filled
    /// rectangle (2 x 1 in at the page origin); `FillForegnd` is the
    /// palette index 2 (red), the cached form colours take.
    fn filled_document() -> Document {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 0);
        shape.sheet.cells = vec![
            cell("Width", CellValue::Number(2.0)),
            cell("Height", CellValue::Number(1.0)),
            cell("PinX", CellValue::Number(1.0)),
            cell("PinY", CellValue::Number(0.5)),
            cell("LocPinX", CellValue::Number(1.0)),
            cell("LocPinY", CellValue::Number(0.5)),
            cell("FillPattern", CellValue::Number(1.0)),
            cell("FillForegnd", CellValue::Number(2.0)),
            cell("LinePattern", CellValue::Number(0.0)),
        ];
        let mut geometry = Section::new(SectionName::new("Geometry"));
        for (index, (x, y)) in [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            let mut row = Row::new();
            row.row_type = Some(if index == 0 {
                "MoveTo".to_owned()
            } else {
                "LineTo".to_owned()
            });
            row.ix = i32::try_from(index).ok();
            row.cells = vec![
                cell("X", CellValue::Number(x)),
                cell("Y", CellValue::Number(y)),
            ];
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        let id = doc.add_shape(shape);

        let mut page = Page::new(PageId { index: 0 }, 0);
        page.name = "Page-1".to_owned();
        page.sheet.cells = vec![
            cell("PageWidth", CellValue::Number(8.5)),
            cell("PageHeight", CellValue::Number(11.0)),
        ];
        page.shapes.push(id);
        doc.pages.push(page);
        doc
    }

    #[test]
    fn the_cache_starts_empty_and_clears() {
        let mut cache = CanvasCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.generation(), 0);
        assert_eq!(cache.raster(), None);
        assert_eq!(cache.texture_id(), None);
        assert_eq!(cache.raster_zoom(), 0.0);
        assert_eq!(cache.page_size(), (0.0, 0.0));
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache, CanvasCache::default());
    }

    #[test]
    fn show_rasterizes_a_filled_shape_and_stays_cached() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        frame(&ctx, egui::RawInput::default(), |ui| {
            let output = show(ui, &mut session, &mut cache).expect("canvas");
            assert!(output.status.starts_with("Page-1  100%"));
        });
        assert_eq!(cache.generation(), 1);
        let raster = cache.raster().expect("a raster");
        assert_eq!((raster.width, raster.height), (816, 1056));
        let lit = raster
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(lit > 1000, "the shape draws ({} lit pixels)", lit);
        let red = raster
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 200 && pixel[3] > 200)
            .count();
        assert!(red > 1000, "the fill is red ({} red pixels)", red);

        // A second frame changes nothing: no re-rasterization.
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 1);
    }

    #[test]
    fn zoom_beyond_ten_percent_re_rasterizes_and_pan_does_not() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 1);
        assert_eq!(cache.raster_zoom(), 1.0);

        // Panning never re-rasterizes (ADR 0033).
        session.view.pan = Point::new(1.5, -0.25);
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 1);

        // A 5% zoom stays inside the 10% band: scaled on screen only.
        session.view.zoom = 1.05;
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 1);

        // 20% from the rasterized zoom re-rasterizes.
        session.view.zoom = 1.2;
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 2);
        assert_eq!(cache.raster_zoom(), 1.2);

        // A page switch re-rasterizes.
        let mut second = Page::new(PageId { index: 1 }, 1);
        second.name = "Page-2".to_owned();
        session.document.pages.push(second);
        session.active_page = Some(PageId { index: 1 });
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert_eq!(cache.generation(), 3);
    }

    #[test]
    fn ctrl_wheel_zooms_at_the_pointer() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        // One frame registers the canvas widget for hover (egui 0.37
        // hit-tests against the previous pass).
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        let anchor = Pos2::new(300.0, 250.0);
        let mut input = input_with_pointer(anchor);
        input.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            // One wheel notch up: zoom in (winit reports up as +y).
            delta: egui::vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::CTRL,
        });
        let (before_x, before_y) = {
            let k = points_per_inch(1.0);
            (
                (f64::from(anchor.x) - f64::from(RULER_WIDTH)) / k - session.view.pan.x,
                (f64::from(anchor.y) - f64::from(RULER_WIDTH)) / k - session.view.pan.y,
            )
        };
        frame(&ctx, input, |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });
        assert!(session.view.zoom > 1.0, "the wheel zoomed in");
        // The page point under the pointer is still under the pointer.
        let k = points_per_inch(session.view.zoom);
        let after_x = (f64::from(anchor.x) - f64::from(RULER_WIDTH)) / k - session.view.pan.x;
        let after_y = (f64::from(anchor.y) - f64::from(RULER_WIDTH)) / k - session.view.pan.y;
        assert!((after_x - before_x).abs() < 1.0e-9);
        assert!((after_y - before_y).abs() < 1.0e-9);
    }

    #[test]
    fn canvas_keys_produce_zoom_intents() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        // One frame registers the canvas widget for hover (egui 0.37
        // hit-tests against the previous pass).
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
        });

        for (key, expected) in [
            (egui::Key::Plus, "view.zoom.in"),
            (egui::Key::Minus, "view.zoom.out"),
            (egui::Key::Num0, "view.zoom.fit"),
        ] {
            let mut input = input_with_pointer(Pos2::new(100.0, 100.0));
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            });
            let intents = frame(&ctx, input, |ui| {
                show(ui, &mut session, &mut cache).expect("canvas").intents
            });
            assert_eq!(intents.len(), 1, "one intent for {expected}");
            assert_eq!(intents[0].command.as_str(), expected);
        }
    }

    #[test]
    fn the_canvas_draws_rulers_grid_page_tabs_and_status_bar() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        let mut app = AppCtx::new(larrygram_commands::Settings::default());
        app.open_document(DocumentSession::new(filled_document(), None));
        app.record_warning("last warning");
        frame(&ctx, egui::RawInput::default(), |ui| {
            show(ui, &mut session, &mut cache).expect("canvas");
            show_rulers(ui, &session, DisplayUnits::Millimeters).expect("rulers");
            show_page_tabs(ui, &mut session).expect("page tabs");
            show_status_bar(ui, &app).expect("status bar");
        });
    }

    #[test]
    fn rulers_and_tabs_skip_without_the_toggle_or_the_pages() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        session.view.show_rulers = false;
        frame(&ctx, egui::RawInput::default(), |ui| {
            show_rulers(ui, &session, DisplayUnits::Inches).expect("rulers");
            let intents = show_page_tabs(ui, &mut session).expect("page tabs");
            assert!(intents.is_empty());
        });

        let mut empty = DocumentSession::new(Document::new(), None);
        let mut cache = CanvasCache::new();
        frame(&ctx, egui::RawInput::default(), |ui| {
            let output = show(ui, &mut empty, &mut cache).expect("canvas");
            assert_eq!(output.status, "No page  100%");
        });
        assert!(cache.is_empty());
    }

    #[test]
    fn lengths_format_in_the_display_units() {
        assert_eq!(format_length(8.5, DisplayUnits::Inches), "8.5 in");
        assert_eq!(format_length(8.5, DisplayUnits::Millimeters), "215.9 mm");
        assert_eq!(format_length(8.5, DisplayUnits::Centimeters), "21.59 cm");
        assert_eq!(format_length(11.0, DisplayUnits::Meters), "0.279 m");
        assert_eq!(format_length(12.0, DisplayUnits::Feet), "1 ft");
        assert_eq!(format_length(0.125, DisplayUnits::Inches), "0.125 in");
        assert_eq!(format_number(-3.0), "-3");
        assert_eq!(format_number(f64::NAN), "?");
        assert_eq!(
            format_length(8.5, DisplayUnits::Inches),
            format_length(8.5, DisplayUnits::Inches)
        );
    }

    #[test]
    fn page_size_falls_back_and_zoom_percent_rounds() {
        let doc = filled_document();
        assert_eq!(page_size(&doc, Some(PageId { index: 0 })), (8.5, 11.0));
        assert_eq!(page_size(&doc, None), FALLBACK_PAGE_SIZE);
        assert_eq!(
            page_size(&Document::new(), Some(PageId { index: 0 })),
            FALLBACK_PAGE_SIZE
        );
        assert_eq!(zoom_percent(1.0), "100%");
        assert_eq!(zoom_percent(0.5), "50%");
    }

    #[test]
    fn safe_zoom_clamps_garbage() {
        assert_eq!(safe_zoom(f64::NAN), 1.0);
        assert_eq!(safe_zoom(-2.0), 1.0);
        assert_eq!(safe_zoom(0.01), MIN_ZOOM);
        assert_eq!(safe_zoom(1000.0), MAX_ZOOM);
        assert_eq!(safe_zoom(2.0), 2.0);
    }

    #[test]
    fn the_cache_debug_and_empty_zoom_key_are_covered() {
        let cache = CanvasCache::new();
        let text = format!("{cache:?}");
        assert!(text.contains("generation"), "{text}");
        assert!(cache.zoom_is_far(1.0), "an empty cache is always far");
        assert_eq!(format_number(f64::MAX), f64::MAX.to_string());
        assert_eq!(format_number(f64::NAN), "?");
    }

    #[test]
    fn page_labels_fall_back_to_name_u_then_position() {
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.name = "Named".to_owned();
        assert_eq!(page_label(&page, 0), "Named");
        page.name.clear();
        page.name_u = "NameU".to_owned();
        assert_eq!(page_label(&page, 0), "NameU");
        page.name_u.clear();
        assert_eq!(page_label(&page, 4), "Page 5");
        assert_eq!(
            viewport_rect(
                Rect::from_min_size(Pos2::ZERO, egui::vec2(10.0, 10.0)),
                false
            ),
            Rect::from_min_size(Pos2::ZERO, egui::vec2(10.0, 10.0))
        );
        assert!(tick_positions(0.0, 100.0, f64::NAN, 96.0).is_empty());
        assert!(tick_positions(0.0, 100.0, 0.0, f64::NAN).is_empty());
    }

    #[test]
    fn an_invalid_command_id_is_a_ui_error() {
        assert!(intent("").is_err());
    }

    #[test]
    fn a_document_without_a_page_clears_the_cache() {
        let ctx = Context::default();
        let mut cache = CanvasCache::new();
        let mut session = DocumentSession::new(filled_document(), None);
        session.active_page = None;
        rasterize(&ctx, &session, &mut cache).expect("no page is not an error");
        assert!(cache.is_empty());
    }

    #[test]
    fn every_overlay_mark_kind_draws() {
        let ctx = Context::default();
        let mut session = DocumentSession::new(filled_document(), None);
        let mut cache = CanvasCache::new();
        let mut overlay = Overlay::new();
        overlay.marks.push(OverlayMark::SelectionOutline {
            bounds: PageRect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
            angle: larrygram_geom::Angle::default(),
        });
        overlay.marks.push(OverlayMark::Handles {
            bounds: PageRect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
            angle: larrygram_geom::Angle::default(),
        });
        overlay.marks.push(OverlayMark::RubberBand(PageRect::new(
            Point::new(0.0, 0.0),
            Point::new(2.0, 2.0),
        )));
        overlay.marks.push(OverlayMark::SnapGuide {
            a: Point::new(0.0, 0.0),
            b: Point::new(1.0, 1.0),
        });
        overlay.marks.push(OverlayMark::ConnectorPreview {
            points: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
        });
        frame(&ctx, egui::RawInput::default(), |ui| {
            show_with_overlay(ui, &mut session, &mut cache, &overlay).expect("canvas");
        });
        assert_eq!(cache.generation(), 1);
    }

    #[test]
    fn render_errors_map_into_ui_errors() {
        // The `UiError::Render` mapping (P4-12 coverage major 4).
        let error = render_error(larrygram_render::Error::TooLarge);
        assert!(matches!(error, UiError::Render { .. }), "{error:?}");
        assert_eq!(
            error.to_string(),
            "render error: the canvas raster failed: output is larger than the allowed caps"
        );
    }

    #[test]
    fn a_page_that_cannot_raster_reports_once_and_is_not_retried() {
        // P4-16 review minor 11: a failed raster remembers its cache
        // key and is retried only when the key changes. The error is
        // reported once, not once per frame.
        let ctx = Context::default();
        let mut cache = CanvasCache::new();
        let mut session = DocumentSession::new(filled_document(), None);
        // A page id the document does not hold: `build_scene` rejects
        // it and `rasterize` maps the failure to `UiError::Render`.
        session.active_page = Some(PageId { index: 42 });
        let error = rasterize(&ctx, &session, &mut cache).expect_err("the raster fails");
        assert!(matches!(error, UiError::Render { .. }), "{error:?}");
        for _ in 0..3 {
            rasterize(&ctx, &session, &mut cache).expect("the failed key is not retried");
        }
        assert!(cache.is_empty());
        assert_eq!(cache.generation(), 0, "no rasterization ran again");

        // A different page is a different key: retried (and fails).
        session.active_page = Some(PageId { index: 43 });
        rasterize(&ctx, &session, &mut cache).expect_err("retried at a new key");
        // A different zoom is a different key too.
        session.view.zoom = 2.0;
        rasterize(&ctx, &session, &mut cache).expect_err("retried at a new zoom");
        // And so is a changed document (the change counter is the key).
        session.document = Document::new();
        session.changes = session.changes.wrapping_add(1);
        rasterize(&ctx, &session, &mut cache).expect_err("retried at a changed document");
    }

    #[test]
    fn a_successful_raster_clears_the_failed_key() {
        let ctx = Context::default();
        let mut cache = CanvasCache::new();
        let mut session = DocumentSession::new(filled_document(), None);
        session.active_page = Some(PageId { index: 42 });
        rasterize(&ctx, &session, &mut cache).expect_err("the raster fails");
        // The key that failed rasterizes fine once the page is valid.
        session.active_page = Some(PageId { index: 0 });
        rasterize(&ctx, &session, &mut cache).expect("the page rasterizes");
        assert_eq!(cache.generation(), 1);
        assert!(!cache.is_empty());
        // The failed key is forgotten after a success: a new failure
        // at the old key is reported again.
        session.active_page = Some(PageId { index: 42 });
        rasterize(&ctx, &session, &mut cache).expect_err("reported again");
    }

    /// A theme with the canvas tokens and their expected colours.
    fn canvas_theme(id: &str, background: &str) -> Theme {
        let mut colors = std::collections::BTreeMap::new();
        for (token, value) in [
            ("canvas.background", background),
            ("page.background", "#fefefe"),
            ("page.border", "#010203"),
            ("page.shadow", "#040506"),
            ("ruler.background", "#070809"),
            ("ruler.ticks", "#0a0b0c"),
        ] {
            colors.insert(token.to_owned(), value.to_owned());
        }
        Theme {
            schema_version: 1,
            id: id.to_owned(),
            name: id.to_owned(),
            fonts: std::collections::BTreeMap::new(),
            colors,
            metrics: std::collections::BTreeMap::new(),
            contextual_tab_sets: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn canvas_colours_are_read_from_the_theme() {
        let theme = canvas_theme("canvas", "#101112");
        let colors = CanvasColors::from_theme(&theme);
        assert_eq!(colors.background, Color32::from_rgb(0x10, 0x11, 0x12));
        assert_eq!(colors.page_background, Color32::from_rgb(0xfe, 0xfe, 0xfe));
        assert_eq!(colors.page_border, Color32::from_rgb(0x01, 0x02, 0x03));
        assert_eq!(colors.page_shadow, Color32::from_rgb(0x04, 0x05, 0x06));
        assert_eq!(colors.ruler_background, Color32::from_rgb(0x07, 0x08, 0x09));
        assert_eq!(colors.ruler_ticks, Color32::from_rgb(0x0a, 0x0b, 0x0c));
    }

    #[test]
    fn canvas_colours_fall_back_before_a_theme_is_applied() {
        let ctx = Context::default();
        assert_eq!(CanvasColors::active(&ctx), CanvasColors::default());
        assert_eq!(
            CanvasColors::default().background,
            Color32::from_rgb(204, 204, 204)
        );
    }

    #[test]
    fn applying_a_theme_changes_the_active_canvas_colours() {
        let ctx = Context::default();
        let modern = canvas_theme("panel-modern", "#fbfbfb");
        theme::apply(&ctx, &modern).expect("the theme applies");
        assert_eq!(
            CanvasColors::active(&ctx),
            CanvasColors::from_theme(&modern)
        );

        let classic = canvas_theme("panel-classic", "#9b9ab3");
        theme::apply(&ctx, &classic).expect("the second theme applies");
        assert_eq!(
            CanvasColors::active(&ctx),
            CanvasColors::from_theme(&classic)
        );
        assert_ne!(
            CanvasColors::active(&ctx).background,
            CanvasColors::from_theme(&modern).background
        );
    }
}
