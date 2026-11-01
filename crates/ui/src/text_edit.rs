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
    /// The shape being edited, or `None` while idle (and while a
    /// pending text box is being typed, task P7-25 item 9).
    pub shape: Option<ShapeId>,
    /// The new text box being typed into (task P7-25 item 9): its
    /// page bounds while the box is not yet in the model. The box and
    /// its text are inserted as one undo step on commit.
    pub pending: Option<GeoRect>,
    /// The editable buffer (plain text, ADR 0044).
    pub buffer: String,
    /// The buffer as the edit started (unchanged-buffer detection).
    original: String,
    /// The run marker kinds of the original text (collapse rule).
    markers: RunMarkers,
    /// Whether the text field still needs focus on its first frame.
    focus_requested: bool,
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
    /// task P7-25 item 9).
    ///
    /// The box is not in the model yet: the typed text commits through
    /// one `insert.drawing.text_box` with its `text` argument (one undo
    /// step, ADR 0040).
    pub fn begin_new(&mut self, rect: GeoRect) {
        *self = Self {
            pending: Some(rect),
            focus_requested: true,
            ..Self::default()
        };
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

    /// The shape's text as the editor started with it (unchanged-buffer
    /// detection). A stale shape or a missing document is an error.
    pub fn original_text(&self, ctx: &AppCtx) -> Result<Option<TextContent>, UiError> {
        let Some(shape) = self.shape else {
            return Ok(None);
        };
        let Some(session) = ctx.active_document() else {
            return Err(UiError::Command(larrygram_commands::CommandError::failed(
                "no open document",
            )));
        };
        let entry = session.document.shape(shape).ok_or_else(|| {
            UiError::Command(larrygram_commands::CommandError::failed(format!(
                "unknown shape id {}",
                shape.index
            )))
        })?;
        Ok(entry.text.clone())
    }

    /// Draws the editor over `rect` (the shape's text block in screen
    /// points) and reports a commit.
    ///
    /// Escape and a click outside the block commit, matching the common
    /// Visio behaviour (ADR 0044); the `text.cancel` command is the
    /// explicit discard path. The first frame requests keyboard focus so
    /// egui's `Text` shortcut scope becomes active (ADR 0036).
    pub fn show(&mut self, ui: &mut Ui, rect: egui::Rect) -> EditorAction {
        if !self.is_active() {
            return EditorAction::Pending;
        }
        let rect = if finite_rect(rect) {
            rect
        } else {
            egui::Rect::from_min_size(ui.ctx().viewport_rect().min, egui::vec2(160.0, 40.0))
        };
        let request_focus = self.focus_requested;
        let mut commit = false;
        egui::Area::new(editor_id(self.shape))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(rect.width().max(80.0));
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
                let mut output = egui::TextEdit::multiline(&mut self.buffer)
                    // The one outline of the edit is the text block's,
                    // painted below (task P7-25 item 9).
                    .frame(egui::Frame::NONE)
                    .desired_width(rect.width().max(80.0))
                    .desired_rows(rows)
                    .hint_text("Text")
                    .show(ui);
                // The editor's accessible name (task P7-24: the tests
                // and screen readers find the field by it).
                output.response.response = output.response.response.clone().accessible_name("Text");
                if request_focus {
                    output.response.request_focus();
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    commit = true;
                }
            });
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
        if request_focus {
            self.focus_requested = false;
        }
        let outside_click = ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !rect.contains(pos))
        });
        if commit || outside_click {
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

/// The screen rectangle of a shape's text block (or its shape bounds for
/// an empty shape such as a fresh text box), or `None` when the shape is
/// not in the scene or the view values are unusable.
///
/// The canvas passes its viewport, pan and zoom. Text is laid out in the
/// block box, so the editor sits exactly over the text; the block's
/// rotation is not applied to the egui field (egui text edits are
/// axis-aligned), whose rectangle is the axis-aligned bounds of the
/// rotated block.
pub fn editor_rect(
    scene: &Scene,
    shape: ShapeId,
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
) -> Option<egui::Rect> {
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
    let page = node
        .text
        .as_ref()
        .and_then(|text| text_block_bounds(text, transform))
        .or_else(|| node_bounds(scene, &path).ok())?;
    let k = 96.0 * zoom;
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
    Some(egui::Rect::from_min_max(min, max))
}

/// The screen rectangle of a pending text box (task P7-25 item 9):
/// `bounds` in page inches mapped through the canvas viewport, pan and
/// zoom — the same mapping as [`editor_rect`].
pub fn pending_editor_rect(
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
    page_height: f64,
    bounds: GeoRect,
) -> Option<egui::Rect> {
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
    Some(egui::Rect::from_min_max(min, max))
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

/// The text block's bounds in page inches: the layout box turned about
/// its centre and placed at [`larrygram_render::scene::TextNode::origin`]
/// (`origin + (x, -y)`), then mapped through the shape transform.
fn text_block_bounds(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<GeoRect> {
    let width = text.layout.width;
    let height = text.layout.height;
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
            origin: Point::new(1.0, 3.0),
        };
        text_node.layout.width = 2.0;
        text_node.layout.height = 1.0;
        node.text = Some(text_node);
        scene.nodes.push(Node::Shape(Box::new(node)));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let rect = editor_rect(
            &scene,
            ShapeId { index: 2 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
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
        let rect = editor_rect(
            &nested,
            ShapeId { index: 7 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("grouped shape");
        assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
    }

    #[test]
    fn text_block_bounds_reject_bad_boxes_and_rotation() {
        let mut node = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            origin: Point::new(0.0, 0.0),
        };
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
        node.layout.width = 1.0;
        node.layout.height = 1.0;
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

    /// Task P7-25 item 9: a pending text box starts empty and its
    /// commit carries the typed text (the shell inserts the box with
    /// it, one undo step).
    #[test]
    fn a_pending_box_finishes_with_the_typed_text() {
        let mut editor = TextEditor::new();
        editor.begin_new(GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)));
        assert!(editor.is_active());
        assert_eq!(editor.shape, None);
        assert_eq!(
            editor.pending,
            Some(GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)))
        );
        assert_eq!(editor.buffer, "");
        // Nothing typed: no text, no text object.
        assert_eq!(editor.finish().expect("finish"), None);
        assert!(!editor.is_active());

        editor.begin_new(GeoRect::new(Point::new(0.0, 0.0), Point::new(2.0, 0.5)));
        editor.buffer = "Hi".to_owned();
        let written = editor.finish().expect("finish").expect("changed");
        assert_eq!(
            written.parts,
            vec![TextPart::Text("Hi".to_owned())],
            "a new box keeps no run markers"
        );
        assert_eq!(editor, TextEditor::new());
    }

    /// `begin` and `begin_new` replace each other's target.
    #[test]
    fn begin_and_begin_new_replace_the_target() {
        let mut editor = TextEditor::new();
        editor.begin_new(GeoRect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)));
        editor
            .begin(
                ShapeId { index: 2 },
                &text(vec![TextPart::Text("x".to_owned())]),
            )
            .expect("begin");
        assert_eq!(editor.pending, None);
        assert_eq!(editor.shape, Some(ShapeId { index: 2 }));
        editor.begin_new(GeoRect::new(Point::new(1.0, 1.0), Point::new(2.0, 2.0)));
        assert_eq!(editor.shape, None);
        assert!(editor.pending.is_some());
        editor.cancel();
        assert_eq!(editor, TextEditor::new());
    }

    /// Task P7-25 item 9: the pending box's page rect maps to the
    /// screen like an edited shape's text block.
    #[test]
    fn a_pending_box_maps_to_a_screen_rectangle() {
        let bounds = GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let rect =
            pending_editor_rect(viewport, Point::new(0.0, 0.0), 1.0, 11.0, bounds).expect("rect");
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
