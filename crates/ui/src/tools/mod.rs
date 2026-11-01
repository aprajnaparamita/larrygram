//! Interaction tools (ADR 0042): the tool state machine and its event
//! seam.
//!
//! One tool is active at a time (pointer, rectangle, ellipse, line,
//! connector, text). Each tool consumes [`ToolEvent`]s and returns a
//! [`ToolEffect`]: [`Intent`]s to dispatch through the command registry
//! (never a direct model mutation, ADR 0028) plus the transient
//! [`Overlay`] marks of this frame (ADR 0042). The state machine never
//! panics on any event order (AGENTS.md section 6).
//!
//! The modules are split one per Phase 5 task so parallel tasks never
//! touch the same file (task P5-01 wires the seams):
//!
//! - [`mod@pointer`] — select, marquee, move, resize, rotate (P5-05);
//! - [`mod@draw`] — rectangle, ellipse and line drawing tools (P5-06);
//! - [`mod@stencil`] — drag from the Shapes window, drop on the page (P5-06);
//! - [`mod@text`] — the text tool and the in-place editor hand-off (P5-07);
//! - [`mod@connector`] — the dynamic connector tool (P5-08);
//! - [`mod@format_painter`] — the Format Painter's canvas mode (task
//!   P7-24 item 6).

pub mod connector;
pub mod draw;
pub mod format_painter;
pub mod pointer;
pub mod stencil;
pub mod text;

use larrygram_commands::AppCtx;
use larrygram_geom::Point;
use larrygram_render::Overlay;

use crate::{Intent, UiError};

/// The active interaction tool (ADR 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Tool {
    /// Select, move, resize, rotate (the default).
    #[default]
    Pointer,
    /// Draw a rectangle shape.
    Rectangle,
    /// Draw an ellipse shape.
    Ellipse,
    /// Draw a line shape.
    Line,
    /// Draw a dynamic connector (ADR 0045).
    Connector,
    /// Draw a text box (ADR 0044).
    Text,
}

/// The modifier keys held during an event (ADR 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift held.
    pub shift: bool,
    /// Ctrl (Cmd on macOS mapping, ADR 0036) held.
    pub ctrl: bool,
    /// Alt/Option held (disables snapping, ADR 0043).
    pub alt: bool,
}

impl Modifiers {
    /// No modifiers held.
    pub const fn none() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
        }
    }
}

/// One input event, in page coordinates and inches (the canvas converts
/// from screen points at the edge, AGENTS.md section 6).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToolEvent {
    /// A pointer button press.
    PointerDown {
        /// The pointer position, in page inches.
        pos: Point,
        /// The held modifiers.
        modifiers: Modifiers,
    },
    /// A pointer move.
    PointerMove {
        /// The pointer position, in page inches.
        pos: Point,
        /// The held modifiers.
        modifiers: Modifiers,
    },
    /// A pointer button release.
    PointerUp {
        /// The pointer position, in page inches.
        pos: Point,
        /// The held modifiers.
        modifiers: Modifiers,
    },
    /// A double click (enter a group, start text edit, ADR 0044).
    DoubleClick {
        /// The pointer position, in page inches.
        pos: Point,
    },
    /// Escape or another tool-cancelling key.
    Cancel,
}

/// What one event produced (ADR 0042).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolEffect {
    /// Commands to dispatch through the registry, in order.
    pub intents: Vec<Intent>,
    /// The overlay marks to draw this frame (ADR 0042).
    pub overlay: Overlay,
}

impl ToolEffect {
    /// An effect that does nothing.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The tool state machine (ADR 0042).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolMachine {
    /// The active tool.
    pub tool: Tool,
    /// The opaque tool state (drag anchors, draw previews); only the
    /// owning tool module reads it.
    pub(crate) state: pointer::PointerState,
    /// The drawing tools' drag (owner: `draw`).
    pub(crate) draw: draw::State,
    /// The stencil drag from the Shapes window (owner: `stencil`).
    pub(crate) stencil: stencil::State,
    /// The connector tool's drag state (P5-08).
    pub(crate) connector: connector::ConnectorState,
}

impl ToolMachine {
    /// A machine with the pointer tool active.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one event and returns what to do (ADR 0042).
    ///
    /// `ctx` is read-only: the machine computes [`Intent`]s and the
    /// caller dispatches them (ADR 0028). Task P5-05 implements the
    /// pointer tool and dispatches to the other tool modules.
    pub fn handle(&mut self, event: ToolEvent, ctx: &AppCtx) -> Result<ToolEffect, UiError> {
        // The Format Painter owns the pointer tool's events while a
        // copy is recorded (task P7-24 item 6): a click on a shape
        // applies it, Escape cancels. The other tools draw normally.
        if ctx.format_painter.is_some() && self.tool == Tool::Pointer {
            return format_painter::handle(self, &event, ctx);
        }
        match self.tool {
            Tool::Pointer => pointer::handle(self, &event, ctx),
            Tool::Rectangle | Tool::Ellipse | Tool::Line => draw::handle(self, &event, ctx),
            Tool::Connector => connector::handle(self, &event, ctx),
            Tool::Text => text::handle(self, &event, ctx),
        }
    }

    /// The selection overlay while the pointer tool is idle (the shell
    /// calls it every frame; a drag keeps its preview from the last
    /// event).
    pub fn idle_overlay(&mut self, ctx: &AppCtx) -> Result<Option<Overlay>, UiError> {
        match self.tool {
            Tool::Pointer => pointer::idle_overlay(self, ctx),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::Settings;

    #[test]
    fn every_tool_is_a_no_op_without_a_document() {
        let ctx = AppCtx::new(Settings::default());
        let event = ToolEvent::PointerDown {
            pos: Point::new(1.0, 2.0),
            modifiers: Modifiers::none(),
        };
        let mut machine = ToolMachine::new();
        assert_eq!(machine.tool, Tool::Pointer);
        // No document: the pointer reports an empty effect, not an error.
        let effect = machine.handle(event.clone(), &ctx).expect("pointer");
        assert!(effect.intents.is_empty());

        let mut drawing = machine.clone();
        drawing.tool = Tool::Rectangle;
        assert!(
            draw::handle(&mut drawing, &event, &ctx)
                .expect("drawing")
                .intents
                .is_empty()
        );
        assert_eq!(draw::anchor(&drawing), None);
        assert!(
            stencil::handle(&mut machine, &event, &ctx)
                .expect("stencil")
                .intents
                .is_empty()
        );
        assert_eq!(stencil::anchor(&machine), None);

        // P5-07 implemented the text tool: with no document its events are
        // a no-op (and it has no drag anchor).
        let effect = text::handle(&mut machine, &event, &ctx).expect("text tool");
        assert!(effect.intents.is_empty());
        assert_eq!(text::anchor(&machine), None);
        // The connector tool is implemented (P5-08): without a document
        // it is a no-op, never the stub error.
        let effect = connector::handle(&mut machine, &event, &ctx).expect("connector");
        assert!(effect.intents.is_empty());
        assert_eq!(connector::anchor(&machine), None);
        assert_eq!(ToolEffect::new(), ToolEffect::default());
        assert_eq!(Modifiers::none(), Modifiers::default());
    }
}
