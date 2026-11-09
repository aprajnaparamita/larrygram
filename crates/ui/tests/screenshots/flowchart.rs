//! The Phase 5 exit-test flowchart (task P7-38 goal 1: "copy the
//! flowchart build from `apps/larrygram/tests/flowchart.rs` into the
//! test target"): ten shapes over two pages and eight glued connectors,
//! built through real command dispatches in the full shell. The labels
//! are this task's own short process labels; the Process shape at
//! (2, 7) says "Review order" (the `text-editing` shot).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_commands::{ArgValue, Args};

use crate::rig::Rig;

/// The stencil the flowchart masters come from.
const STENCIL: &str = "Basic Flowchart";

/// One dispatch through the rig with [`Source::Test`]; panics on any
/// failure and returns nothing (the outcome is the state).
fn run(rig: &mut Rig, command: &'static str, args: Args) {
    rig.dispatch_args(command, args);
}

/// The selection anchor's Visio id (`insert.shape` selects the new
/// instance; its id is never assumed).
fn anchor_visio_id(rig: &Rig) -> i32 {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("an open document");
    let anchor = shell
        .app
        .selection
        .anchor
        .expect("the new shape is selected");
    session
        .document
        .shape(anchor)
        .expect("the anchor shape exists")
        .visio_id
}

/// Inserts one catalog master at (`x`, `y`) and answers its Visio id.
fn insert_shape(rig: &mut Rig, master: &str, x: f64, y: f64) -> i32 {
    let args = Args::new()
        .with("stencil", ArgValue::Text(STENCIL.to_owned()))
        .with("master", ArgValue::Text(master.to_owned()))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    run(rig, "insert.shape", args);
    anchor_visio_id(rig)
}

/// The file-order index of each side's connection point on the
/// flowchart masters (bottom, right, top, left).
const BOTTOM: usize = 0;
const RIGHT: usize = 1;
const TOP: usize = 2;
const LEFT: usize = 3;

/// One connection-row `IX` of `target` (`index` into the file-order
/// point list, `None` = the last point): the glue `point` argument's
/// contract (P5-17).
fn point_ix(rig: &Rig, target: i32, index: Option<usize>) -> i32 {
    let shell = rig.shell.borrow();
    let session = shell.app.active_document().expect("an open document");
    let page = session.active_page.expect("an active page");
    let shape = session
        .document
        .shape_by_visio_id(page, target)
        .expect("the glue target is on the page");
    let points = session.document.connection_points_indexed(shape);
    let position = index.unwrap_or(points.len().saturating_sub(1));
    points
        .get(position)
        .map(|(ix, _)| *ix)
        .expect("the target has connection points")
}

/// One `connector.draw`: inserts the "Connector" master, glues the
/// given ends (a `None` point is outline glue) and routes it in one
/// undo step (P5-17). Answers the connector's Visio id.
fn draw_connector(
    rig: &mut Rig,
    x: f64,
    y: f64,
    begin: Option<(i32, Option<i32>)>,
    end: Option<(i32, Option<i32>)>,
) -> i32 {
    let mut args = Args::new()
        .with("stencil", ArgValue::Text(STENCIL.to_owned()))
        .with("master", ArgValue::Text("Connector".to_owned()))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    if let Some((target, point)) = begin {
        args.set("begin_target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("begin_point", ArgValue::Integer(i64::from(ix)));
        }
    }
    if let Some((target, point)) = end {
        args.set("end_target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("end_point", ArgValue::Integer(i64::from(ix)));
        }
    }
    run(rig, "connector.draw", args);
    anchor_visio_id(rig)
}

/// The second connector form (P5-17): `insert.shape` plus one
/// `connector.glue_begin`/`connector.glue_end` per end.
fn glued_connector(
    rig: &mut Rig,
    x: f64,
    y: f64,
    begin: (i32, Option<i32>),
    end: (i32, Option<i32>),
) -> i32 {
    let connector = insert_shape(rig, "Connector", x, y);
    for (command, (target, point)) in [("connector.glue_begin", begin), ("connector.glue_end", end)]
    {
        let mut args = Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("target", ArgValue::Integer(i64::from(target)));
        if let Some(ix) = point {
            args.set("point", ArgValue::Integer(i64::from(ix)));
        }
        run(rig, command, args);
    }
    connector
}

/// One flowchart shape (the shots find their shapes by Visio id).
#[derive(Debug, Clone, Copy)]
pub struct ShapeRef {
    /// The shape's Visio `ID` (per page).
    pub visio_id: i32,
}

/// What the build step created.
pub struct Flowchart {
    /// The ten shapes in build order.
    pub shapes: Vec<ShapeRef>,
}

/// Builds the 10-shape, two-page, eight-connector flowchart (the
/// `apps/larrygram/tests/flowchart.rs` build, minus that test's
/// save/reopen assertions, plus the connectors that join every
/// shape), then labels every shape with this task's own short
/// process labels.
///
/// Page one: Terminator -> Process -> Decision -> Process ->
/// Terminator down the middle, the Decision's second exit to the
/// right; page two: Process -> Decision -> Terminator, the
/// Decision's second exit to the right. The flow runs bottom point to
/// top point and the side exits right point to left point, so no
/// route doubles back around a shape. Five connectors are drawn with
/// `connector.draw` and three with `insert.shape` +
/// `connector.glue_begin`/`connector.glue_end`; the first and last of
/// page one use dynamic glue.
pub fn build_flowchart(rig: &mut Rig) -> Flowchart {
    // Every x sits `DX` right of the exit-test drawing's, so the flow
    // stands near the middle of the page (the shots crop around its
    // shapes without showing the grey beyond the page edge).
    const DX: f64 = 1.5;
    run(rig, "file.new", Args::new());
    run(
        rig,
        "page.rename",
        Args::new().with("name", ArgValue::Text("Flow".to_owned())),
    );

    // Page one: six shapes, each labelled as it is inserted (this
    // task's own short labels; a shape's Visio id is per-page, so the
    // label must be written while its page is active).
    let mut shapes = Vec::new();
    for ((master, x, y), label) in [
        (("Terminator", 2.0 + DX, 9.0), "Start order"),
        (("Process", 2.0 + DX, 7.0), "Review order"),
        (("Decision", 2.0 + DX, 5.0), "Order valid?"),
        (("Process", 5.5 + DX, 5.0), "Fix order"),
        (("Process", 2.0 + DX, 3.0), "Fill order"),
        (("Terminator", 2.0 + DX, 1.0), "Done"),
    ] {
        let visio_id = insert_shape(rig, master, x, y);
        label_shape(rig, visio_id, label);
        shapes.push(ShapeRef { visio_id });
    }

    // Page one: five connectors.
    draw_connector(
        rig,
        4.0 + DX,
        8.0,
        Some((shapes[0].visio_id, None)),
        Some((shapes[1].visio_id, None)),
    );
    let begin_point = point_ix(rig, shapes[1].visio_id, Some(BOTTOM));
    let end_point = point_ix(rig, shapes[2].visio_id, Some(TOP));
    draw_connector(
        rig,
        4.0 + DX,
        6.0,
        Some((shapes[1].visio_id, Some(begin_point))),
        Some((shapes[2].visio_id, Some(end_point))),
    );
    glued_connector(
        rig,
        4.0 + DX,
        4.0,
        (
            shapes[2].visio_id,
            Some(point_ix(rig, shapes[2].visio_id, Some(RIGHT))),
        ),
        (
            shapes[3].visio_id,
            Some(point_ix(rig, shapes[3].visio_id, Some(LEFT))),
        ),
    );
    let begin_point = point_ix(rig, shapes[2].visio_id, Some(BOTTOM));
    let end_point = point_ix(rig, shapes[4].visio_id, Some(TOP));
    draw_connector(
        rig,
        2.0 + DX,
        4.0,
        Some((shapes[2].visio_id, Some(begin_point))),
        Some((shapes[4].visio_id, Some(end_point))),
    );
    glued_connector(
        rig,
        2.0 + DX,
        2.0,
        (shapes[4].visio_id, None),
        (shapes[5].visio_id, None),
    );

    // Page two: four shapes and three connectors.
    run(
        rig,
        "page.add",
        Args::new().with("name", ArgValue::Text("Detail".to_owned())),
    );
    for ((master, x, y), label) in [
        (("Process", 2.0 + DX, 7.0), "Check stock"),
        (("Decision", 2.0 + DX, 5.0), "In stock?"),
        (("Process", 5.5 + DX, 5.0), "Order part"),
        (("Terminator", 2.0 + DX, 3.0), "Close"),
    ] {
        let visio_id = insert_shape(rig, master, x, y);
        label_shape(rig, visio_id, label);
        shapes.push(ShapeRef { visio_id });
    }

    draw_connector(
        rig,
        4.0 + DX,
        6.0,
        Some((
            shapes[6].visio_id,
            Some(point_ix(rig, shapes[6].visio_id, Some(BOTTOM))),
        )),
        Some((
            shapes[7].visio_id,
            Some(point_ix(rig, shapes[7].visio_id, Some(TOP))),
        )),
    );
    glued_connector(
        rig,
        4.0 + DX,
        4.0,
        (
            shapes[7].visio_id,
            Some(point_ix(rig, shapes[7].visio_id, Some(RIGHT))),
        ),
        (
            shapes[8].visio_id,
            Some(point_ix(rig, shapes[8].visio_id, Some(LEFT))),
        ),
    );
    draw_connector(
        rig,
        2.0 + DX,
        4.0,
        Some((
            shapes[7].visio_id,
            Some(point_ix(rig, shapes[7].visio_id, Some(BOTTOM))),
        )),
        Some((
            shapes[9].visio_id,
            Some(point_ix(rig, shapes[9].visio_id, Some(TOP))),
        )),
    );

    Flowchart { shapes }
}

/// Writes `text` as the shape's one text run (`shape.set.text` on the
/// selection, ADR 0044).
pub fn label_shape(rig: &mut Rig, visio_id: i32, text: &str) {
    run(
        rig,
        "edit.select",
        Args::new().with("shapes", ArgValue::List(vec![visio_id.to_string()])),
    );
    run(
        rig,
        "shape.set.text",
        Args::new().with("text", ArgValue::Text(text.to_owned())),
    );
}

/// The three-shape, two-connector drawing of the `connectors-reroute`
/// shot: Process -> Process -> Process in a row on one page, joined by
/// two glued dynamic connectors.
pub struct ThreeShapes {
    /// The middle shape's Visio id (the one the shot drags).
    pub middle: i32,
    /// The connector into the middle shape and the one out of it.
    pub connectors: [i32; 2],
}

/// Builds the drawing on the active document's first page.
pub fn build_three_connected(rig: &mut Rig) -> ThreeShapes {
    run(rig, "file.new", Args::new());
    run(
        rig,
        "page.rename",
        Args::new().with("name", ArgValue::Text("Flow".to_owned())),
    );
    let left = insert_shape(rig, "Process", 1.5, 6.0);
    let middle = insert_shape(rig, "Process", 4.25, 6.0);
    let right = insert_shape(rig, "Process", 7.0, 6.0);
    let into = draw_connector(rig, 2.875, 6.0, Some((left, None)), Some((middle, None)));
    let out = draw_connector(rig, 5.625, 6.0, Some((middle, None)), Some((right, None)));
    label_shape(rig, left, "Get order");
    label_shape(rig, middle, "Check stock");
    label_shape(rig, right, "Ship order");
    ThreeShapes {
        middle,
        connectors: [into, out],
    }
}
