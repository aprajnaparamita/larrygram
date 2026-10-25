//! Pane splitters: the draggable borders between the docked panes and
//! the drawing (task P7-19).
//!
//! egui's own panel resize zone straddles the panel edge, and the
//! canvas, registered after the panel, took the half of it that lies
//! over the drawing: the border resized only from a 3 point strip. A
//! [`Splitter`] owns its zone instead. The shell reads the splitter's
//! response at the start of the frame ([`Splitter::update`], egui
//! decides the interaction from the previous frame's rects), draws the
//! pane at the size that gives, and registers the grab zone after
//! every pane and the canvas ([`Splitter::interact`]), so the zone wins
//! the hit test on both sides of the edge.
//!
//! A drag changes the size by the drag distance within the
//! [`Limits`]; a drag past half the minimum collapses the pane to a
//! strip, a click on the strip reopens it at its remembered size, and a
//! double-click restores the default. Sizes are in egui points and
//! persist through [`PaneSize`] in the settings.

use std::collections::BTreeMap;

use egui::{Context, CursorIcon, Id, Rect, Sense, Ui};
use larrygram_commands::PaneSize;

/// Half the grab zone across the edge, in points: the zone is 8 points
/// wide, centred on the edge.
pub const GRAB_HALF: f32 = 4.0;

/// The size a collapsed pane keeps on screen, in points: the strip that
/// reopens it.
pub const COLLAPSED_SIZE: f32 = 10.0;

/// The size range of one pane, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The smallest open size.
    pub min: f32,
    /// The largest size.
    pub max: f32,
    /// The size a double-click restores (and a new pane starts at).
    pub default: f32,
}

impl Limits {
    /// The limits, made consistent: `max` at least `min` and the
    /// default inside the range. Non-finite values fall back to `min`.
    pub fn new(min: f32, max: f32, default: f32) -> Self {
        let min = if min.is_finite() { min.max(0.0) } else { 0.0 };
        let max = if max.is_finite() { max.max(min) } else { min };
        let default = if default.is_finite() {
            default.clamp(min, max)
        } else {
            min
        };
        Self { min, max, default }
    }

    /// `size` inside the range; a non-finite size is the default.
    pub fn clamp(&self, size: f32) -> f32 {
        if size.is_finite() {
            size.clamp(self.min, self.max)
        } else {
            self.default
        }
    }

    /// Below this requested size a drag collapses the pane.
    fn collapse_below(&self) -> f32 {
        self.min * 0.5
    }
}

/// Which edge of its pane a splitter moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The right edge of a pane docked on the left (the Shapes window).
    Right,
    /// The left edge of a pane docked on the right (the side panes).
    Left,
    /// The bottom edge of a pane stacked over another (Shape Data over
    /// Size & Position).
    Bottom,
}

impl Edge {
    /// The pointer axis the edge follows: 0 for x, 1 for y.
    fn axis(self) -> usize {
        match self {
            Edge::Right | Edge::Left => 0,
            Edge::Bottom => 1,
        }
    }

    /// `+1` when the pane grows as the pointer moves in the positive
    /// axis direction, `-1` otherwise.
    fn sign(self) -> f32 {
        match self {
            Edge::Right | Edge::Bottom => 1.0,
            Edge::Left => -1.0,
        }
    }

    /// The resize cursor of this edge.
    fn cursor(self) -> CursorIcon {
        match self {
            Edge::Right | Edge::Left => CursorIcon::ResizeColumn,
            Edge::Bottom => CursorIcon::ResizeRow,
        }
    }
}

/// One drag in flight: the size and the pointer position it started
/// from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    start_size: f32,
    origin: f32,
}

/// One pane border (task P7-19).
#[derive(Debug, Clone, PartialEq)]
pub struct Splitter {
    key: &'static str,
    edge: Edge,
    /// The open size, kept while collapsed; `None` until seeded.
    size: Option<f32>,
    collapsed: bool,
    drag: Option<Drag>,
    /// The grab zone of this frame, once the pane drew.
    zone: Option<Rect>,
    /// The size or collapse changed since the last [`Splitter::commit`].
    dirty: bool,
}

impl Splitter {
    /// A splitter for the pane stored under `key`.
    pub fn new(key: &'static str, edge: Edge) -> Self {
        Self {
            key,
            edge,
            size: None,
            collapsed: false,
            drag: None,
            zone: None,
            dirty: false,
        }
    }

    /// The settings key of the pane.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The egui id of the grab zone.
    pub fn id(&self) -> Id {
        Id::unique(("larrygram.splitter", self.key))
    }

    /// The open size, or `None` before the first frame.
    pub fn size(&self) -> Option<f32> {
        self.size
    }

    /// Whether the pane is collapsed to its strip.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// The grab zone of the last frame.
    pub fn zone(&self) -> Option<Rect> {
        self.zone
    }

    /// Whether a drag of this splitter is in flight.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Seeds the size from the settings the first time, then keeps its
    /// own: a stored size outside the limits is clamped, a missing or
    /// non-finite one is the default.
    fn seed(&mut self, stored: &BTreeMap<String, PaneSize>, limits: Limits) -> f32 {
        if let Some(size) = self.size {
            return size;
        }
        let (size, collapsed) = match stored.get(self.key) {
            Some(entry) => (limits.clamp(entry.size), entry.collapsed),
            None => (limits.default, false),
        };
        self.size = Some(size);
        self.collapsed = collapsed;
        size
    }

    /// Applies this frame's pointer interaction and returns the size
    /// the pane draws: [`COLLAPSED_SIZE`] while collapsed, else the
    /// open size within `limits`.
    ///
    /// Call it before the pane draws; the response is the one egui
    /// computed at the start of the frame from last frame's zone.
    pub fn update(
        &mut self,
        ctx: &Context,
        stored: &BTreeMap<String, PaneSize>,
        limits: Limits,
    ) -> f32 {
        let size = self.seed(stored, limits);
        let response = ctx.read_response(self.id());
        let axis = self.edge.axis();
        if let Some(response) = response {
            if response.drag_started() {
                let origin = ctx
                    .input(|input| input.pointer.press_origin())
                    .or(response.interact_pointer_pos())
                    .map(|pos| pos[axis]);
                if let Some(origin) = origin {
                    let start_size = if self.collapsed {
                        COLLAPSED_SIZE
                    } else {
                        limits.clamp(size)
                    };
                    self.drag = Some(Drag { start_size, origin });
                }
            }
            if let Some(drag) = self.drag
                && (response.dragged() || response.drag_stopped())
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let wanted = drag.start_size + self.edge.sign() * (pointer[axis] - drag.origin);
                self.apply_drag(wanted, limits);
            }
            if response.drag_stopped() {
                self.drag = None;
            }
            if response.double_clicked() {
                self.drag = None;
                self.restore_default(limits);
            } else if response.clicked() && self.collapsed {
                self.reopen();
            }
        } else {
            self.drag = None;
        }
        self.drawn_size(limits)
    }

    /// The size the pane draws with the current state.
    pub fn drawn_size(&self, limits: Limits) -> f32 {
        if self.collapsed {
            COLLAPSED_SIZE
        } else {
            limits.clamp(self.size.unwrap_or(limits.default))
        }
    }

    /// One drag step to the `wanted` size: collapse below half the
    /// minimum, else the size within the limits.
    fn apply_drag(&mut self, wanted: f32, limits: Limits) {
        if !wanted.is_finite() {
            return;
        }
        if wanted < limits.collapse_below() {
            if !self.collapsed {
                self.collapsed = true;
                self.dirty = true;
            }
            return;
        }
        let size = limits.clamp(wanted);
        if self.collapsed || self.size != Some(size) {
            self.collapsed = false;
            self.size = Some(size);
            self.dirty = true;
        }
    }

    /// Restores the default size and opens the pane.
    pub fn restore_default(&mut self, limits: Limits) {
        self.size = Some(limits.default);
        self.collapsed = false;
        self.dirty = true;
    }

    /// Collapses the pane to its strip; it keeps its size.
    pub fn collapse(&mut self) {
        if !self.collapsed {
            self.collapsed = true;
            self.dirty = true;
        }
    }

    /// Reopens a collapsed pane at its remembered size.
    pub fn reopen(&mut self) {
        if self.collapsed {
            self.collapsed = false;
            self.dirty = true;
        }
    }

    /// Records the pane's drawn rect: the grab zone lies across the
    /// moving edge, [`GRAB_HALF`] points to each side.
    pub fn place(&mut self, pane: Rect) {
        let zone = match self.edge {
            Edge::Right => Rect::from_min_max(
                egui::pos2(pane.max.x - GRAB_HALF, pane.min.y),
                egui::pos2(pane.max.x + GRAB_HALF, pane.max.y),
            ),
            Edge::Left => Rect::from_min_max(
                egui::pos2(pane.min.x - GRAB_HALF, pane.min.y),
                egui::pos2(pane.min.x + GRAB_HALF, pane.max.y),
            ),
            Edge::Bottom => Rect::from_min_max(
                egui::pos2(pane.min.x, pane.max.y - GRAB_HALF),
                egui::pos2(pane.max.x, pane.max.y + GRAB_HALF),
            ),
        };
        self.zone = zone.is_positive().then_some(zone);
    }

    /// Registers the grab zone over everything drawn so far, shows the
    /// resize cursor and paints the hover line in `accent`. Call it
    /// after the canvas so the zone wins the hit test on both sides of
    /// the edge.
    pub fn interact(&self, ui: &Ui, accent: egui::Color32) {
        let Some(zone) = self.zone else {
            return;
        };
        let response = ui.interact(zone, self.id(), Sense::click_and_drag());
        let label = format!("{} splitter", self.key);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::accesskit::Role::Splitter, ui.is_enabled(), &label)
        });
        let active = response.hovered() || response.dragged() || self.drag.is_some();
        if active {
            ui.ctx().set_cursor_icon(self.edge.cursor());
            let stroke = egui::Stroke::new(2.0, accent);
            let painter = ui.painter_at(zone);
            match self.edge {
                Edge::Right | Edge::Left => {
                    painter.vline(zone.center().x, zone.y_range(), stroke);
                }
                Edge::Bottom => {
                    painter.hline(zone.x_range(), zone.center().y, stroke);
                }
            }
        }
    }

    /// Whether the pointer is over the zone or dragging it this frame
    /// (the canvas then ignores the press).
    pub fn is_busy(&self, ctx: &Context) -> bool {
        self.drag.is_some()
            || ctx
                .read_response(self.id())
                .is_some_and(|response| response.hovered() || response.dragged())
    }

    /// Writes the size into `stored` when it changed and no drag is in
    /// flight (one settings write per gesture).
    pub fn commit(&mut self, stored: &mut BTreeMap<String, PaneSize>) {
        if !self.dirty || self.drag.is_some() {
            return;
        }
        if let Some(size) = self.size {
            stored.insert(self.key.to_owned(), PaneSize::new(size, self.collapsed));
        }
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits::new(100.0, 400.0, 200.0)
    }

    #[test]
    fn limits_are_made_consistent() {
        let limits = Limits::new(100.0, 50.0, 300.0);
        assert_eq!(limits.max, 100.0);
        assert_eq!(limits.default, 100.0);
        let limits = Limits::new(f32::NAN, f32::INFINITY, f32::NAN);
        assert_eq!((limits.min, limits.max, limits.default), (0.0, 0.0, 0.0));
        assert_eq!(Limits::new(-5.0, 10.0, 5.0).min, 0.0);
        assert_eq!(super::tests::limits().clamp(f32::NAN), 200.0);
        assert_eq!(super::tests::limits().clamp(1000.0), 400.0);
        assert_eq!(super::tests::limits().clamp(10.0), 100.0);
    }

    #[test]
    fn a_splitter_seeds_from_the_settings_once() {
        let mut stored = BTreeMap::new();
        let mut splitter = Splitter::new("shapes", Edge::Right);
        assert_eq!(splitter.key(), "shapes");
        assert_eq!(splitter.seed(&stored, limits()), 200.0, "the default");
        stored.insert("shapes".to_owned(), PaneSize::new(300.0, true));
        assert_eq!(splitter.seed(&stored, limits()), 200.0, "seeded once");

        let mut fresh = Splitter::new("shapes", Edge::Right);
        assert_eq!(fresh.seed(&stored, limits()), 300.0);
        assert!(fresh.is_collapsed());
        assert_eq!(fresh.drawn_size(limits()), COLLAPSED_SIZE);

        stored.insert("shapes".to_owned(), PaneSize::new(f32::NAN, false));
        let mut garbage = Splitter::new("shapes", Edge::Right);
        assert_eq!(garbage.seed(&stored, limits()), 200.0, "NaN is the default");
        stored.insert("shapes".to_owned(), PaneSize::new(9000.0, false));
        let mut wide = Splitter::new("shapes", Edge::Right);
        assert_eq!(wide.seed(&stored, limits()), 400.0, "clamped");
    }

    #[test]
    fn a_drag_clamps_collapses_and_reopens() {
        let mut splitter = Splitter::new("panes", Edge::Left);
        splitter.seed(&BTreeMap::new(), limits());
        splitter.apply_drag(250.0, limits());
        assert_eq!(splitter.size(), Some(250.0));
        splitter.apply_drag(1000.0, limits());
        assert_eq!(splitter.size(), Some(400.0), "the maximum");
        splitter.apply_drag(70.0, limits());
        assert_eq!(splitter.size(), Some(100.0), "the minimum, still open");
        assert!(!splitter.is_collapsed());
        splitter.apply_drag(f32::NAN, limits());
        assert_eq!(splitter.size(), Some(100.0), "a NaN step is ignored");
        splitter.apply_drag(20.0, limits());
        assert!(splitter.is_collapsed(), "below half the minimum");
        assert_eq!(splitter.size(), Some(100.0), "the size is kept");
        splitter.apply_drag(20.0, limits());
        splitter.reopen();
        assert!(!splitter.is_collapsed());
        splitter.reopen();
        splitter.collapse();
        splitter.collapse();
        assert!(splitter.is_collapsed());
        splitter.restore_default(limits());
        assert_eq!(splitter.size(), Some(200.0));
        assert!(!splitter.is_collapsed());
    }

    #[test]
    fn a_commit_writes_once_per_change() {
        let mut stored = BTreeMap::new();
        let mut splitter = Splitter::new("shape_data", Edge::Bottom);
        splitter.commit(&mut stored);
        assert!(stored.is_empty(), "nothing changed");
        splitter.seed(&stored, limits());
        splitter.apply_drag(300.0, limits());
        splitter.drag = Some(Drag {
            start_size: 200.0,
            origin: 0.0,
        });
        assert!(splitter.is_dragging());
        splitter.commit(&mut stored);
        assert!(stored.is_empty(), "no write during a drag");
        splitter.drag = None;
        splitter.commit(&mut stored);
        assert_eq!(stored.get("shape_data"), Some(&PaneSize::new(300.0, false)));
        stored.clear();
        splitter.commit(&mut stored);
        assert!(stored.is_empty(), "written once");
    }

    #[test]
    fn the_zone_straddles_the_moving_edge() {
        let pane = Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(110.0, 220.0));
        for (edge, expected) in [
            (
                Edge::Right,
                Rect::from_min_max(egui::pos2(106.0, 20.0), egui::pos2(114.0, 220.0)),
            ),
            (
                Edge::Left,
                Rect::from_min_max(egui::pos2(6.0, 20.0), egui::pos2(14.0, 220.0)),
            ),
            (
                Edge::Bottom,
                Rect::from_min_max(egui::pos2(10.0, 216.0), egui::pos2(110.0, 224.0)),
            ),
        ] {
            let mut splitter = Splitter::new("pane", edge);
            assert!(splitter.zone().is_none());
            splitter.place(pane);
            assert_eq!(splitter.zone(), Some(expected), "{edge:?}");
            assert!(
                splitter
                    .zone()
                    .is_some_and(|zone| zone.width().min(zone.height()) >= 6.0)
            );
        }
        let mut splitter = Splitter::new("pane", Edge::Bottom);
        splitter.place(Rect::NOTHING);
        assert!(splitter.zone().is_none(), "an empty pane has no zone");
        assert_eq!(Edge::Bottom.cursor(), CursorIcon::ResizeRow);
        assert_eq!(Edge::Left.cursor(), CursorIcon::ResizeColumn);
    }
}
