//! In-place text editing on shapes (ADR 0044).
//!
//! The editor is an egui text field over the shape's text block (the
//! canvas places it with [`editor_rect`]). On commit the buffer is
//! written as one text run ([`TextContent`], ADR 0019): run and
//! paragraph markers collapse to the first row's `IX`, which is the
//! documented Phase 5 limit until a run-aware editor lands (ADR 0044).
//! While editing, the shortcut scope is `Text` (ADR 0036). Task P5-07
//! implements the bodies.
//!
//! The editor state is deliberately independent of the command registry:
//! `crates/commands` (L4) cannot depend on egui (L6). The UI tool opens
//! the editor, and on commit dispatches `shape.set.text`; the
//! `text.begin`/`text.commit`/`text.cancel` commands validate and
//! announce the same transition for headless callers.

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Ui};
use larrygram_commands::AppCtx;
use larrygram_geom::{Point, Rect as GeoRect, Transform};
use larrygram_model::{ShapeId, TextContent, TextPart};
use larrygram_render::scene::{Node, Shape as SceneShape};
use larrygram_render::{Scene, node_bounds};

use crate::UiError;

/// The first-row markers a buffer keeps when it is written back ADR 0044.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RunMarkers {
    /// The text had a `Character` run marker.
    character: bool,
    /// The text had a `Paragraph` run marker.
    paragraph: bool,
    /// The text had a `Tabs` run marker.
    tabs: bool,
}

/// The in-place text editor state (ADR 0044).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEditor {
    /// The shape being edited, or `None` while idle (task P7-37: a new
    /// text box is a shape too, so the box's id sits here while it is
    /// typed).
    pub shape: Option<ShapeId>,
    /// The page bounds of the new text box being typed (task P7-25
    /// item 9, task P7-37): the field fills the outlined box and a
    /// click inside it is a caret move. `None` for an existing shape's
    /// edit.
    pub pending: Option<GeoRect>,
    /// The editable buffer (plain text, ADR 0044).
    pub buffer: String,
    /// The buffer as the edit started (unchanged-buffer detection).
    original: String,
    /// The run marker kinds of the original text (collapse rule).
    markers: RunMarkers,
    /// Whether the text field still needs focus on its first frame.
    focus_requested: bool,
    /// Whether the field re-requests the keyboard focus on its next
    /// frame with the caret where it is (task P7-41 item 1): a
    /// text-format control click keeps the edit open and the field
    /// takes the keys back the same frame.
    refocus_requested: bool,
}

impl TextEditor {
    /// An idle editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an edit is in progress (the `Text` shortcut scope, ADR 0036).
    pub fn is_active(&self) -> bool {
        self.shape.is_some() || self.pending.is_some()
    }

    /// Opens the editor on `shape` with its current text (ADR 0044).
    ///
    /// The UI reads the shape's text (and rejects a stale id) before it
    /// calls this method; `crates/ui`'s tool owns that read. The editor
    /// keeps the plain buffer and the first-row marker kinds, so a commit
    /// can collapse a multi-run text predictably.
    pub fn begin(&mut self, shape: ShapeId, text: &TextContent) -> Result<(), UiError> {
        self.shape = Some(shape);
        self.pending = None;
        self.buffer = plain_text(text);
        self.original = self.buffer.clone();
        self.markers = RunMarkers {
            character: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::CharRun { .. })),
            paragraph: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::ParaRun { .. })),
            tabs: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::TabsRun { .. })),
        };
        self.focus_requested = true;
        Ok(())
    }

    /// Opens the editor on a new text box at `rect` (its page bounds;
    /// task P7-37 item 1).
    ///
    /// The box already exists in the model (the shell inserts it when
    /// the editor opens, inside its undo group), so the editor has a
    /// real shape: `shape` is set and the text formatting predicates
    /// and format commands work as on any edited shape. The bounds keep
    /// the box-filling field and the click backstop of task P7-26
    /// item 3.
    pub fn begin_box(&mut self, shape: ShapeId, rect: GeoRect) {
        // `begin` cannot fail; it resets the buffer to the empty text
        // a new box starts with.
        self.begin(shape, &TextContent::default()).ok();
        self.pending = Some(rect);
    }

    /// Finishes the edit and returns the text to write (one run, ADR
    /// 0044), or `None` when the buffer is unchanged.
    ///
    /// The editor always returns to idle, including on the unchanged and
    /// empty-buffer cases.
    pub fn finish(&mut self) -> Result<Option<TextContent>, UiError> {
        if !self.is_active() {
            return Err(UiError::Command(larrygram_commands::CommandError::failed(
                "no text edit is active",
            )));
        }
        let changed = self.buffer != self.original;
        let text = one_run(&self.buffer, self.markers);
        *self = Self::new();
        if changed { Ok(Some(text)) } else { Ok(None) }
    }

    /// Abandons the edit without writing anything.
    pub fn cancel(&mut self) {
        *self = Self::new();
    }

    /// Re-requests the keyboard focus with the caret where it is (task
    /// P7-41 item 1): after a text-format control click the field
    /// takes the keys back without losing the typed text or the
    /// caret's place.
    pub fn refocus(&mut self) {
        self.refocus_requested = true;
    }

    /// The shape's text as the editor started with it (unchanged-buffer
    /// detection): the effective text (task P7-40), so a master's text
    /// on an instance with none of its own is the start state. A stale
    /// shape or a missing document is an error.
    pub fn original_text(&self, ctx: &AppCtx) -> Result<Option<TextContent>, UiError> {
        let Some(shape) = self.shape else {
            return Ok(None);
        };
        let Some(session) = ctx.active_document() else {
            return Err(UiError::Command(larrygram_commands::CommandError::failed(
                "no open document",
            )));
        };
        session.document.shape(shape).ok_or_else(|| {
            UiError::Command(larrygram_commands::CommandError::failed(format!(
                "unknown shape id {}",
                shape.index
            )))
        })?;
        Ok(session.document.effective_text(shape).cloned())
    }

    /// Draws the editor over the shape's drawn text (task P7-41 item
    /// 9) and reports a commit.
    ///
    /// The overlay box, the field's font size and its first baseline
    /// match the drawn text ([`EditorOverlay`]): the fill covers the
    /// drawn glyphs and the typed text sits where they sat. Escape and
    /// a click outside the block commit, matching the common Visio
    /// behaviour (ADR 0044); the `text.cancel` command is the explicit
    /// discard path. The first frame requests keyboard focus so egui's
    /// `Text` shortcut scope becomes active (ADR 0036).
    pub fn show(&mut self, ui: &mut Ui, overlay: EditorOverlay) -> EditorAction {
        if !self.is_active() {
            return EditorAction::Pending;
        }
        let rect = if finite_rect(overlay.rect) {
            overlay.rect
        } else {
            egui::Rect::from_min_size(ui.ctx().viewport_rect().min, egui::vec2(160.0, 40.0))
        };
        // The field's font matches the drawn text (task P7-41 item 9)
        // and its text sits on the drawn first baseline: the galley
        // starts one measured ascent below it. The drawn family wins
        // when the theme registered it (the same face draws then).
        let font = overlay.font.as_ref().map(|drawn| {
            let named = egui::FontFamily::Name(drawn.family.as_str().into());
            let known = ui.fonts(|fonts| fonts.definitions().families.contains_key(&named));
            let family = if known {
                named
            } else {
                egui::FontFamily::Proportional
            };
            egui::FontId::new(drawn.size_pt, family)
        });
        let baseline_shift = match (overlay.font.as_ref(), &font) {
            (Some(drawn), Some(font)) => {
                let ascent = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap("X".to_owned(), font.clone(), egui::Color32::WHITE)
                        .rows
                        .first()
                        .and_then(|row| row.glyphs.first().map(|glyph| glyph.pos.y))
                        .unwrap_or(0.0)
                });
                drawn.first_baseline_y - rect.min.y - ascent
            }
            _ => 0.0,
        };
        let open_focus = std::mem::take(&mut self.focus_requested);
        let refocus = std::mem::take(&mut self.refocus_requested);
        let request_focus = open_focus || refocus;
        // The caret goes to the end of the buffer when the editor
        // opens (task P7-29 item 2).
        let buffer_chars = self.buffer.chars().count();
        let mut commit = false;
        // The field's own rect joins the commit test (task P7-48 item
        // 1: a field that grows with typed lines never commits on a
        // press inside itself).
        let mut field = None;
        let field_width = rect.width().max(1.0);
        egui::Area::new(editor_id(self.shape))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min + egui::vec2(0.0, baseline_shift))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(field_width);
                // A new text box's field fills the outlined box
                // (task P7-26 item 3): a click anywhere inside the box
                // moves the caret and keeps editing. An existing
                // shape's edit keeps its one-row field (its click rule
                // is task P7-25 item 9's commit-first).
                let rows = if self.pending.is_some() {
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
                    ui.set_min_height(rect.height().max(row));
                    let row = row.max(1.0);
                    ((rect.height() / row).ceil() as usize).clamp(1, 64)
                } else {
                    1
                };
                let mut edit = egui::TextEdit::multiline(&mut self.buffer)
                    // The one outline of the edit is the text block's,
                    // painted below (task P7-25 item 9). The margin is
                    // zero (task P7-41 item 9): the field's text starts
                    // at the block's box, not inside an inset. The
                    // wrap width is the block width (task P7-48 item
                    // 1): the field wraps where the drawn text wraps.
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(field_width)
                    .desired_rows(rows);
                if let Some(font) = font.clone() {
                    // The font size matches the drawn text (task
                    // P7-41 item 9).
                    edit = edit.font(font);
                }
                let mut output = edit.hint_text("Text").show(ui);
                // The editor's accessible name (task P7-24: the tests
                // and screen readers find the field by it).
                output.response.response = output.response.response.clone().accessible_name("Text");
                if request_focus {
                    output.response.request_focus();
                }
                if open_focus {
                    // The caret sits at the end of the text when the
                    // editor opens (task P7-29 item 2): a stale cursor
                    // from an earlier edit of the same shape must not
                    // survive the re-open (egui keeps the field's
                    // state under its id). A refocus (task P7-41 item
                    // 1) leaves the caret alone.
                    output
                        .state
                        .cursor
                        .set_char_range(Some(CCursorRange::one(CCursor::new(buffer_chars))));
                    output.state.store(ui.ctx(), output.response.id);
                }
                // Escape commits (ADR 0044) — except when a popup
                // or menu is open (task P7-48 item 3): Escape closes
                // it and the edit stays.
                if ui.input(|input| input.key_pressed(egui::Key::Escape))
                    && !other_layer_open(ui.ctx(), self.shape)
                {
                    commit = true;
                }
                field = Some((output.response.rect, output.response.id));
            });
        // The commit-on-outside-press rule (ADR 0044, task P7-41 item
        // 1, task P7-48 item 1): a press inside the text block or the
        // field's own rect keeps the edit; a press on text-format UI
        // keeps it too and hands the keyboard back (the caret
        // untouched); any other press outside commits.
        let commit_on_press = match ui.input(|input| {
            input
                .pointer
                .any_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        }) {
            Some(pos)
                if !rect.contains(pos) && !field.is_some_and(|(field, _)| field.contains(pos)) =>
            {
                match press_rule(ui.ctx(), self.shape, pos) {
                    PressRule::Commit => true,
                    PressRule::KeepAndRefocus => {
                        if let Some((_, id)) = field {
                            ui.ctx().memory_mut(|memory| memory.request_focus(id));
                        }
                        false
                    }
                    PressRule::Keep => false,
                }
            }
            _ => false,
        };
        // Exactly one outline while editing (task P7-25 item 9): the
        // text block's, in the selection colour; the tool overlay is
        // suppressed while the editor is open.
        let colors = crate::canvas::CanvasColors::active(ui.ctx());
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, colors.selection),
            egui::StrokeKind::Inside,
        );
        if commit || commit_on_press {
            return match self.finish() {
                Ok(text) => EditorAction::Committed(text),
                // `finish` only fails when the editor is inactive, which
                // the `is_active` guard above excludes.
                Err(_) => EditorAction::Pending,
            };
        }
        EditorAction::Pending
    }
}

/// What one editor frame produced (ADR 0044).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EditorAction {
    /// The edit is still in progress.
    Pending,
    /// The edit ended; the payload is the text to write, or `None` when
    /// the buffer did not change.
    Committed(Option<TextContent>),
}

/// The stable egui id of the editor area for one shape.
fn editor_id(shape: Option<ShapeId>) -> Id {
    Id::unique(("larrygram-text-editor", shape.map(|shape| shape.index)))
}

/// The key of the per-frame keep-editing rects (task P7-41 item 1).
fn keep_rects_id() -> Id {
    Id::unique("larrygram-keep-editing-rects")
}

/// One recorded keep-editing rect (task P7-41 item 1).
#[derive(Debug, Clone, Copy)]
struct KeepRect {
    /// The control's widget rect.
    rect: egui::Rect,
    /// Whether the editor takes the keyboard back after the press.
    editor_keeps_keyboard: bool,
}

/// The keep-editing rects recorded in one frame (task P7-41 item 1):
/// the pass number they were drawn in, so a stale list never silences
/// a press.
#[derive(Debug, Clone, Default)]
struct KeepRects {
    /// The pass the rects were drawn in.
    pass: u64,
    /// The widget rects of text-format controls.
    rects: Vec<KeepRect>,
}

/// Records a text-format control's widget rect (task P7-41 item 1): a
/// press on such a control keeps the in-place editor open. The
/// controls call this as they draw with the command they dispatch;
/// anything that is not a text-format command records nothing.
/// `editor_keeps_keyboard` says whether the editor takes the keyboard
/// back after the press: it does after a button or a list row, but not
/// after the classic font and size boxes, which are text fields of
/// their own. The list follows the frame and [`TextEditor::show`]
/// reads it the same frame (the controls draw before the canvas).
pub fn keep_editing_rect(
    ctx: &egui::Context,
    command: Option<&str>,
    rect: egui::Rect,
    editor_keeps_keyboard: bool,
) {
    // A text-format control's press keeps the edit (task P7-41 item
    // 1), and so does an Undo or Redo press (task P7-48 item 5): their
    // command commits the open edit — cancelling an untyped box,
    // undoing a typed one whole — when it runs.
    let keep = command.is_some_and(|command| {
        larrygram_commands::builtins::is_text_format(command)
            || matches!(command, "edit.undo" | "edit.redo" | "edit.redo.or.repeat")
    });
    if !keep {
        return;
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let mut kept: KeepRects = data.get_temp(keep_rects_id()).unwrap_or_default();
        if kept.pass != pass {
            kept.rects.clear();
            kept.pass = pass;
        }
        kept.rects.push(KeepRect {
            rect,
            editor_keeps_keyboard,
        });
        data.insert_temp(keep_rects_id(), kept);
    });
}

/// Whether a popup, menu or palette layer other than the editor's own
/// is open (task P7-48 item 3): Escape belongs to that layer — it
/// closes and the open edit stays.
pub fn other_layer_open(ctx: &egui::Context, shape: Option<ShapeId>) -> bool {
    let own = editor_id(shape);
    ctx.memory(|memory| {
        memory
            .areas()
            .visible_layer_ids()
            .iter()
            .any(|layer| layer.order == egui::Order::Foreground && layer.id != own)
    })
}

/// What a press at `pos` does to the open editor (task P7-41 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressRule {
    /// The press is not on text-format UI: the edit commits (the P7-29
    /// rule).
    Commit,
    /// The press is on text-format UI that hands the keyboard back: the
    /// edit stays and the field re-takes the keyboard and the caret.
    KeepAndRefocus,
    /// The press is on text-format UI that takes the keyboard for its
    /// own text (the classic font and size boxes): the edit stays.
    Keep,
}

/// The press rule at `pos` (task P7-41 item 1): a press in a popup,
/// menu or palette layer (the format popups and dropdowns choose
/// values with the editor open) or on a recorded text-format control
/// ([`keep_editing_rect`]) keeps the edit. The editor's own area is
/// not one of them: a press on it but off the text block commits, like
/// any press outside.
fn press_rule(ctx: &egui::Context, shape: Option<ShapeId>, pos: egui::Pos2) -> PressRule {
    let own = editor_id(shape);
    if ctx
        .layer_id_at(pos)
        .is_some_and(|layer| layer.order == egui::Order::Foreground && layer.id != own)
    {
        return PressRule::KeepAndRefocus;
    }
    let pass = ctx.cumulative_pass_nr();
    let recorded = ctx.data(|data| {
        data.get_temp::<KeepRects>(keep_rects_id())
            .filter(|kept| kept.pass == pass)
            .and_then(|kept| {
                kept.rects
                    .iter()
                    .find(|kept| kept.rect.contains(pos))
                    .copied()
            })
    });
    match recorded {
        Some(kept) if kept.editor_keeps_keyboard => PressRule::KeepAndRefocus,
        Some(_) => PressRule::Keep,
        None => PressRule::Commit,
    }
}

/// The plain text of a [`TextContent`]: literal parts concatenated, field
/// markers dropped (their cached display text follows as a literal).
fn plain_text(text: &TextContent) -> String {
    let mut buffer = String::new();
    for part in &text.parts {
        if let TextPart::Text(literal) = part {
            buffer.push_str(literal);
        }
    }
    buffer
}

/// The one-run content ADR 0044 writes: the first-row markers the
/// original text had, then the literal buffer. An empty buffer is an
/// empty text (no markers).
fn one_run(buffer: &str, markers: RunMarkers) -> TextContent {
    if buffer.is_empty() {
        return TextContent::new();
    }
    let mut parts = Vec::new();
    if markers.character {
        parts.push(TextPart::CharRun { ix: 0 });
    }
    if markers.paragraph {
        parts.push(TextPart::ParaRun { ix: 0 });
    }
    if markers.tabs {
        parts.push(TextPart::TabsRun { ix: 0 });
    }
    parts.push(TextPart::Text(buffer.to_owned()));
    TextContent { parts }
}

/// Where the in-place editor draws over the shape's drawn text (task
/// P7-41 item 9): the box and font of the overlay follow the glyphs
/// the scene draws, at any display scale.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorOverlay {
    /// The drawn text's screen box: the fill and outline of the edit.
    pub rect: egui::Rect,
    /// The drawn text's font metrics, or `None` for a plain field over
    /// a box with no drawn text (a pending text box, or a shape whose
    /// text draws no ink).
    pub font: Option<EditorFont>,
}

/// The font of the drawn text the overlay matches (task P7-41 item 9).
#[derive(Debug, Clone, PartialEq)]
pub struct EditorFont {
    /// The first line's font size, in egui points (the drawn points
    /// scaled by the canvas zoom).
    pub size_pt: f32,
    /// The first baseline's screen y: the field's text sits on it.
    pub first_baseline_y: f32,
    /// The drawn face's family name. The field uses it when the theme
    /// registered that family (the same face draws then), else the
    /// proportional UI font.
    pub family: String,
}

/// The overlay of a shape's drawn text: the screen box of the glyphs
/// the scene draws and the first line's font metrics (task P7-41 item
/// 9), or `None` when the shape is not in the scene or the view values
/// are unusable.
///
/// The canvas passes its viewport, pan and zoom. The block's rotation
/// is not applied to the egui field (egui text edits are axis-aligned),
/// whose rectangle is the axis-aligned bounds of the rotated text; the
/// font metrics are `None` then as well (its first baseline is no
/// axis' y).
pub fn editor_rect(
    scene: &Scene,
    shape: ShapeId,
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
) -> Option<EditorOverlay> {
    if !zoom.is_finite() || zoom <= 0.0 || !finite_rect(viewport) {
        return None;
    }
    let (path, node, transform) = find_shape(
        &scene.nodes,
        shape,
        &Default::default(),
        Transform::identity(),
        0,
    )?;
    let k = 96.0 * zoom;
    // The box is the text block (task P7-48 item 1: the review's M1 —
    // the commit test, the fill and the wrap width follow the block,
    // not the laid-out glyphs); the font size and the first baseline
    // come from the drawn first glyph (task P7-41 item 9). The metrics
    // drop for rotated text (a tilted first baseline is no axis' y;
    // egui text edits are axis-aligned).
    let page = node
        .text
        .as_ref()
        .and_then(|text| text_block_bounds(text, transform))
        .or_else(|| node_bounds(scene, &path).ok())?;
    let font = node
        .text
        .as_ref()
        .filter(|text| text.layout.rotation.radians() == 0.0)
        .and_then(|text| drawn_font(text, transform))
        .and_then(|(size_pt, baseline_page_y, family)| {
            let size_pt = (size_pt * k / 72.0) as f32;
            let baseline_y =
                (f64::from(viewport.min.y) + ((pan.y + scene.height - baseline_page_y) * k)) as f32;
            (size_pt.is_finite() && size_pt > 0.0 && baseline_y.is_finite()).then_some(EditorFont {
                size_pt,
                first_baseline_y: baseline_y,
                family,
            })
        });
    let min = egui::pos2(
        viewport.min.x + ((pan.x + page.min.x) * k) as f32,
        viewport.min.y + ((pan.y + scene.height - page.max.y) * k) as f32,
    );
    let max = egui::pos2(
        viewport.min.x + ((pan.x + page.max.x) * k) as f32,
        viewport.min.y + ((pan.y + scene.height - page.min.y) * k) as f32,
    );
    if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
        return None;
    }
    Some(EditorOverlay {
        rect: egui::Rect::from_min_max(min, max),
        font,
    })
}

/// The overlay of a pending text box (task P7-25 item 9): `bounds` in
/// page inches mapped through the canvas viewport, pan and zoom — the
/// same mapping as [`editor_rect`]. The box has no drawn text to
/// match, so its field stays plain (task P7-41 item 9).
pub fn pending_editor_rect(
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
    page_height: f64,
    bounds: GeoRect,
) -> Option<EditorOverlay> {
    if !zoom.is_finite() || zoom <= 0.0 || !finite_rect(viewport) || !page_height.is_finite() {
        return None;
    }
    let k = 96.0 * zoom;
    let min = egui::pos2(
        viewport.min.x + ((pan.x + bounds.min.x) * k) as f32,
        viewport.min.y + ((pan.y + page_height - bounds.max.y) * k) as f32,
    );
    let max = egui::pos2(
        viewport.min.x + ((pan.x + bounds.max.x) * k) as f32,
        viewport.min.y + ((pan.y + page_height - bounds.min.y) * k) as f32,
    );
    if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
        return None;
    }
    Some(EditorOverlay {
        rect: egui::Rect::from_min_max(min, max),
        font: None,
    })
}

/// The drawn text's font metrics (task P7-41 item 9): the first
/// line's first glyph's size in points, the first baseline's page y
/// and the drawn family. Task P7-48 item 1 keeps only the size and
/// the first baseline of the drawn glyphs — the box is the text
/// block. `None` when the text draws no glyph or the values are
/// unusable.
fn drawn_font(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<(f64, f64, String)> {
    let origin = text.origin;
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return None;
    }
    let line = text.layout.lines.first()?;
    let glyph = line.glyphs.first()?;
    if glyph.size <= 0.0 || !glyph.size.is_finite() || !glyph.x.is_finite() {
        return None;
    }
    if !line.baseline.is_finite() {
        return None;
    }
    let font = text.fonts.get(glyph.font)?;
    // The first baseline's point, in the text-block convention
    // `origin + (x, -y)`, through the shape transform (the
    // `text_block_bounds` mapping).
    let baseline = transform.apply(Point::new(origin.x + glyph.x, origin.y - line.baseline));
    if !baseline.x.is_finite() || !baseline.y.is_finite() {
        return None;
    }
    Some((glyph.size, baseline.y, font.family().to_owned()))
}

/// Finds one model shape's node, turning its ancestor transforms into
/// the shape-local to page transform (the text block tool reads the
/// transform through it, task P7-25 item 4).
pub(crate) fn find_shape<'a>(
    nodes: &'a [Node],
    shape: ShapeId,
    path: &larrygram_render::NodePath,
    parent: Transform,
    depth: usize,
) -> Option<(larrygram_render::NodePath, &'a SceneShape, Transform)> {
    const MAX_DEPTH: usize = 128;
    if depth >= MAX_DEPTH {
        return None;
    }
    for (index, node) in nodes.iter().enumerate() {
        let child = path.child(index);
        match node {
            Node::Group(group) => {
                if let Some(found) = find_shape(
                    &group.children,
                    shape,
                    &child,
                    group.transform.then(parent),
                    depth + 1,
                ) {
                    return Some(found);
                }
            }
            Node::Shape(candidate) if candidate.shape_id == Some(shape) => {
                return Some((child, candidate, candidate.transform.then(parent)));
            }
            _ => {}
        }
    }
    None
}

/// The text block's bounds in page inches (task P7-48 item 1): the
/// block box ([`larrygram_render::scene::TextNode::block`]) turned
/// about its centre and placed at the node's origin (`origin + (x,
/// -y)`), then mapped through the shape transform.
fn text_block_bounds(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<GeoRect> {
    let width = text.block.width;
    let height = text.block.height;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let origin = text.origin;
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return None;
    }
    let (sin, cos) = text.layout.rotation.radians().sin_cos();
    if !sin.is_finite() || !cos.is_finite() {
        return None;
    }
    let (cx, cy) = (width / 2.0, height / 2.0);
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, height),
        Point::new(0.0, height),
    ];
    let mut bounds: Option<GeoRect> = None;
    for corner in corners {
        let (dx, dy) = (corner.x - cx, corner.y - cy);
        let local = Point::new(
            origin.x + cx + dx * cos - dy * sin,
            origin.y - (cy + dx * sin + dy * cos),
        );
        let page = transform.apply(local);
        if !page.x.is_finite() || !page.y.is_finite() {
            return None;
        }
        let point = GeoRect::new(page, page);
        bounds = Some(match bounds {
            Some(bounds) => bounds.union(point),
            None => point,
        });
    }
    bounds
}

/// True when every side of the rectangle is finite.
fn finite_rect(rect: egui::Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.x.is_finite()
        && rect.max.y.is_finite()
        && rect.width() >= 0.0
        && rect.height() >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::Settings;
    use larrygram_geom::Angle;
    use larrygram_render::Rgba;
    use larrygram_render::scene::{Group, Shape as SceneShape, TextNode};

    fn text(parts: Vec<TextPart>) -> TextContent {
        TextContent { parts }
    }

    fn document_with(text: Option<TextContent>) -> (AppCtx, ShapeId) {
        use larrygram_model::Document;
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 1);
        shape.text = text.clone();
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        if let Some(text) = text {
            doc.set_shape_text(shape, text).expect("text");
        }
        let mut app = AppCtx::new(Settings::default());
        app.open_document(larrygram_commands::DocumentSession::new(doc, None));
        (app, shape)
    }

    #[test]
    fn begin_plain_text_and_finish_a_changed_buffer() {
        let mut editor = TextEditor::new();
        let content = text(vec![
            TextPart::CharRun { ix: 3 },
            TextPart::Text("a".to_owned()),
            TextPart::ParaRun { ix: 2 },
            TextPart::Text("b".to_owned()),
            TextPart::TabsRun { ix: 1 },
            TextPart::Text("c".to_owned()),
        ]);
        editor.begin(ShapeId { index: 4 }, &content).expect("begin");
        assert!(editor.is_active());
        assert_eq!(editor.buffer, "abc");
        editor.buffer.push('!');
        let written = editor.finish().expect("finish").expect("changed");
        assert_eq!(
            written.parts,
            vec![
                TextPart::CharRun { ix: 0 },
                TextPart::ParaRun { ix: 0 },
                TextPart::TabsRun { ix: 0 },
                TextPart::Text("abc!".to_owned()),
            ]
        );
        assert!(!editor.is_active());
        assert_eq!(editor, TextEditor::new());
    }

    #[test]
    fn an_unchanged_buffer_writes_nothing_and_cancel_discards() {
        let mut editor = TextEditor::new();
        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![TextPart::Text("same".to_owned())]),
            )
            .expect("begin");
        assert_eq!(editor.finish().expect("finish"), None);
        assert!(!editor.is_active());

        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![TextPart::Text("old".to_owned())]),
            )
            .expect("begin");
        editor.buffer = "new".to_owned();
        editor.cancel();
        assert_eq!(editor, TextEditor::new());
    }

    #[test]
    fn finish_requires_an_active_edit() {
        let mut editor = TextEditor::new();
        assert!(matches!(
            editor.finish(),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));
    }

    #[test]
    fn an_empty_buffer_writes_an_empty_text() {
        let mut editor = TextEditor::new();
        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![
                    TextPart::CharRun { ix: 0 },
                    TextPart::Text("gone".to_owned()),
                ]),
            )
            .expect("begin");
        editor.buffer.clear();
        let written = editor.finish().expect("finish").expect("changed");
        assert!(written.is_empty());
    }

    #[test]
    fn original_text_reads_the_model_and_rejects_stale_ids() {
        let (app, shape) = document_with(Some(text(vec![TextPart::Text("old".to_owned())])));
        let mut editor = TextEditor::new();
        assert_eq!(editor.original_text(&app).expect("idle"), None);
        editor
            .begin(shape, &text(vec![TextPart::Text("old".to_owned())]))
            .expect("begin");
        assert_eq!(
            editor.original_text(&app).expect("text"),
            Some(text(vec![TextPart::Text("old".to_owned())]))
        );

        let mut stale = TextEditor::new();
        stale
            .begin(ShapeId { index: 99 }, &TextContent::new())
            .expect("begin");
        assert!(matches!(
            stale.original_text(&app),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));

        let mut no_document = TextEditor::new();
        no_document
            .begin(ShapeId { index: 0 }, &TextContent::new())
            .expect("begin");
        assert!(matches!(
            no_document.original_text(&AppCtx::new(Settings::default())),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));
    }

    #[test]
    fn original_text_follows_the_master_text() {
        // Task P7-40: the editor starts from the effective text, the
        // master's on an instance with no text of its own.
        use larrygram_model::{Master, MasterId};
        let (mut app, shape) = document_with(None);
        let session = app.active_document_mut().expect("session");
        let master_shape = session
            .document
            .add_shape(larrygram_model::Shape::new(ShapeId { index: 0 }, 2));
        session
            .document
            .shape_mut(master_shape)
            .expect("shape")
            .text = Some(text(vec![TextPart::Text("master".to_owned())]));
        let mut master = Master::new(MasterId { index: 0 }, 2);
        master.shapes = vec![master_shape];
        session.document.masters.push(master);
        session.document.shape_mut(shape).expect("shape").master = Some(MasterId { index: 0 });

        let mut editor = TextEditor::new();
        editor
            .begin(shape, &text(vec![TextPart::Text("master".to_owned())]))
            .expect("begin");
        assert_eq!(
            editor.original_text(&app).expect("text"),
            Some(text(vec![TextPart::Text("master".to_owned())]))
        );
    }

    #[test]
    fn plain_text_drops_markers_and_keeps_field_display_text() {
        let content = text(vec![
            TextPart::CharRun { ix: 1 },
            TextPart::Text("left".to_owned()),
            TextPart::Field { ix: 4 },
            TextPart::Text("42".to_owned()),
            TextPart::Text(" right".to_owned()),
        ]);
        assert_eq!(plain_text(&content), "left42 right");
    }

    #[test]
    fn a_scene_text_block_maps_to_a_screen_rectangle() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 2 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        let mut text_node = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(2.0, 1.0),
            origin: Point::new(1.0, 3.0),
        };
        text_node.layout.width = 2.0;
        text_node.layout.height = 1.0;
        node.text = Some(text_node);
        scene.nodes.push(Node::Shape(Box::new(node)));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 2 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        // The text draws no glyphs (no lines): the block box, plain.
        assert!(overlay.font.is_none(), "{overlay:?}");
        let rect = overlay.rect;
        // origin (1, 3), block 2 x 1 in: page y 2..3 maps down, x 1..3.
        assert!((rect.min.x - 106.0).abs() < 0.01, "{rect:?}");
        assert!((rect.min.y - (20.0 + 8.0 * 96.0)).abs() < 0.01, "{rect:?}");
        assert!((rect.width() - 192.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 96.0).abs() < 0.01, "{rect:?}");
    }

    #[test]
    fn editor_rect_handles_groups_missing_shapes_and_bad_views() {
        let scene = Scene::new(8.5, 11.0);
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        assert_eq!(
            editor_rect(
                &scene,
                ShapeId { index: 0 },
                viewport,
                Point::new(0.0, 0.0),
                1.0
            ),
            None
        );
        assert_eq!(
            editor_rect(
                &scene,
                ShapeId { index: 0 },
                viewport,
                Point::new(0.0, 0.0),
                0.0
            ),
            None
        );

        let mut nested = Scene::new(8.5, 11.0);
        let group = Group {
            transform: Transform::translate(Point::new(1.0, 1.0)),
            clip: None,
            children: vec![Node::Shape(Box::new(SceneShape {
                shape_id: Some(ShapeId { index: 7 }),
                transform: Transform::identity(),
                path: Default::default(),
                fill: None,
                stroke: None,
                shadow: None,
                clip: None,
                text: None,
            }))],
        };
        nested.nodes.push(Node::Group(group));
        let overlay = editor_rect(
            &nested,
            ShapeId { index: 7 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("grouped shape");
        let rect = overlay.rect;
        assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
    }

    /// A text node with one glyph at (0.5, 0.2) layout inches over the
    /// bundled Carlito face (task P7-41 item 9's fixture), in a 2 x 1
    /// inch block.
    fn font_node() -> TextNode {
        let font = larrygram_text::Font::from_bytes(larrygram_text::CARLITO[0].bytes.to_vec())
            .expect("the bundled face parses");
        let mut text_node = TextNode {
            fonts: vec![font],
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(2.0, 1.0),
            origin: Point::new(1.0, 3.0),
        };
        text_node.layout.lines.push(larrygram_text::Line {
            glyphs: vec![larrygram_text::GlyphPos {
                glyph: larrygram_text::GlyphId(0),
                font: 0,
                x: 0.5,
                y: 0.2,
                run: 0,
                size: 72.0,
            }],
            baseline: 0.2,
            width: 0.5,
            paragraph: 0,
        });
        text_node
    }

    /// Task P7-41 item 9, task P7-48 item 1: the drawn font data is
    /// the first glyph's size and the first baseline (the box is the
    /// text block, not the glyphs).
    #[test]
    fn drawn_font_measures_the_first_baseline_and_the_family() {
        let node = font_node();
        let (size_pt, baseline, family) = drawn_font(&node, Transform::identity()).expect("font");
        assert!((size_pt - 72.0).abs() < 1.0e-9, "{size_pt}");
        // The first line's baseline sits at layout y 0.2: page y 2.8.
        assert!((baseline - 2.8).abs() < 1.0e-9, "{baseline}");
        assert_eq!(family, "Carlito");
    }

    /// The unusable cases answer `None` (task P7-41 item 9).
    #[test]
    fn drawn_font_rejects_unusable_values_and_inkless_text() {
        let empty = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(0.0, 0.0),
        };
        assert_eq!(drawn_font(&empty, Transform::identity()), None);

        // A glyph with no font in the list: nothing measurable.
        let mut node = font_node();
        node.fonts.clear();
        assert_eq!(drawn_font(&node, Transform::identity()), None);

        // A non-finite origin is unusable.
        let mut node = font_node();
        node.origin = Point::new(f64::NAN, 0.0);
        assert_eq!(drawn_font(&node, Transform::identity()), None);

        // A non-finite baseline or glyph x, or a non-positive size,
        // draws no usable metric.
        let mut node = font_node();
        node.layout.lines[0].baseline = f64::NAN;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
        let mut node = font_node();
        node.layout.lines[0].glyphs[0].x = f64::NAN;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
        let mut node = font_node();
        node.layout.lines[0].glyphs[0].size = 0.0;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
    }

    /// Task P7-48 item 1: the box is the text block whatever the
    /// glyphs measure, and the metrics drop for rotated text (a tilted
    /// first baseline is no axis' y).
    #[test]
    fn editor_rect_uses_the_block_box_and_drops_the_metrics_for_rotation() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 3 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(font_node()),
        };
        scene.nodes.push(Node::Shape(Box::new(node.clone())));
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        assert!(overlay.font.is_some(), "{overlay:?}");
        // The block box, not the one-glyph content.
        assert!(
            (overlay.rect.width() - 2.0 * 96.0).abs() < 0.01,
            "{:?}",
            overlay.rect
        );
        assert!(
            (overlay.rect.height() - 96.0).abs() < 0.01,
            "{:?}",
            overlay.rect
        );

        // Rotated text keeps the box and drops the metrics.
        let mut rotated = font_node();
        rotated.layout.rotation = Angle::from_radians(0.1);
        node.text = Some(rotated);
        scene.nodes = vec![Node::Shape(Box::new(node))];
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        assert!(overlay.font.is_none(), "{overlay:?}");
        assert!(overlay.rect.width() > 0.0, "{overlay:?}");
    }

    #[test]
    fn text_block_bounds_reject_bad_boxes_and_rotation() {
        let mut node = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(0.0, 0.0),
            origin: Point::new(0.0, 0.0),
        };
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
        node.block = larrygram_geom::Size::new(1.0, 1.0);
        assert!(text_block_bounds(&node, Transform::identity()).is_some());
        node.layout.rotation = Angle::from_radians(f64::NAN);
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
        node.layout.rotation = Angle::from_radians(0.0);
        node.origin = Point::new(f64::INFINITY, 0.0);
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
    }

    #[test]
    fn editor_id_is_stable_per_shape() {
        assert_eq!(
            editor_id(Some(ShapeId { index: 3 })),
            editor_id(Some(ShapeId { index: 3 }))
        );
        assert_ne!(
            editor_id(Some(ShapeId { index: 3 })),
            editor_id(Some(ShapeId { index: 4 }))
        );
        assert_ne!(editor_id(None), editor_id(Some(ShapeId { index: 3 })));
    }

    /// Task P7-37 item 1: a new text box starts empty and its commit
    /// carries the typed text (the shell writes the text and ends the
    /// box's undo group, one step).
    #[test]
    fn a_new_box_finishes_with_the_typed_text() {
        let shape = ShapeId { index: 7 };
        let mut editor = TextEditor::new();
        editor.begin_box(
            shape,
            GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)),
        );
        assert!(editor.is_active());
        assert_eq!(editor.shape, Some(shape), "the box is a shape while typed");
        assert_eq!(
            editor.pending,
            Some(GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)))
        );
        assert_eq!(editor.buffer, "");
        // Nothing typed: no text to write.
        assert_eq!(editor.finish().expect("finish"), None);
        assert!(!editor.is_active());

        editor.begin_box(
            shape,
            GeoRect::new(Point::new(0.0, 0.0), Point::new(2.0, 0.5)),
        );
        editor.buffer = "Hi".to_owned();
        let written = editor.finish().expect("finish").expect("changed");
        assert_eq!(
            written.parts,
            vec![TextPart::Text("Hi".to_owned())],
            "a new box keeps no run markers"
        );
        assert_eq!(editor, TextEditor::new());
    }

    /// `begin` and `begin_box` replace each other's target.
    #[test]
    fn begin_and_begin_box_replace_the_target() {
        let mut editor = TextEditor::new();
        editor.begin_box(
            ShapeId { index: 1 },
            GeoRect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
        );
        editor
            .begin(
                ShapeId { index: 2 },
                &text(vec![TextPart::Text("x".to_owned())]),
            )
            .expect("begin");
        assert_eq!(editor.pending, None);
        assert_eq!(editor.shape, Some(ShapeId { index: 2 }));
        editor.begin_box(
            ShapeId { index: 3 },
            GeoRect::new(Point::new(1.0, 1.0), Point::new(2.0, 2.0)),
        );
        assert_eq!(
            editor.shape,
            Some(ShapeId { index: 3 }),
            "the box has a shape"
        );
        assert!(editor.pending.is_some(), "and its bounds");
        editor.cancel();
        assert_eq!(editor, TextEditor::new());
    }

    /// Task P7-25 item 9: the pending box's page rect maps to the
    /// screen like an edited shape's text block.
    #[test]
    fn a_pending_box_maps_to_a_screen_rectangle() {
        let bounds = GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let rect = pending_editor_rect(viewport, Point::new(0.0, 0.0), 1.0, 11.0, bounds)
            .expect("rect")
            .rect;
        // x 1..3 maps right; page y 2..2.5 maps down from height 11.
        assert!((rect.min.x - 106.0).abs() < 0.01, "{rect:?}");
        assert!((rect.min.y - (20.0 + 8.5 * 96.0)).abs() < 0.01, "{rect:?}");
        assert!((rect.width() - 192.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 48.0).abs() < 0.01, "{rect:?}");
        assert_eq!(
            pending_editor_rect(viewport, Point::new(0.0, 0.0), 0.0, 11.0, bounds),
            None
        );
        assert_eq!(
            pending_editor_rect(egui::Rect::NOTHING, Point::new(0.0, 0.0), 1.0, 11.0, bounds),
            None
        );
    }
}
