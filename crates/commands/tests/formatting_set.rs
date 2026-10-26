//! Acceptance tests for task P7-21: the formatting set (text, fill,
//! line, arrange and the Format Painter) is reachable from both
//! committed layouts, every command has a real handler with undo, and
//! the formatting changes render — the cells and the picture agree.
//!
//! Reachability is data-level (the P7-20 rule): the Panel Modern and
//! Panel Classic layouts link their controls to command ids and the
//! registry resolves every id to `CommandStatus::Implemented`. The
//! picture check builds one page's scene (`larrygram-render`) and reads
//! the fill, stroke and text run back out of it.

use std::collections::BTreeSet;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandId, CommandStatus, Invocation, Outcome, Source};
use larrygram_model::{
    Cell, CellName, CellValue, Preserved, Row, Section, SectionName, Shape, ShapeId,
};
use larrygram_ui_spec::{LayoutKind, Loader};

/// The formatting set of the task: the ids that must be implemented.
const FORMAT_SET: &[&str] = &[
    // Text
    "text.font",
    "text.font.size",
    "text.font.size.increase",
    "text.font.size.decrease",
    "format.text.increase_size",
    "format.text.decrease_size",
    "format.text.bold",
    "format.text.italic",
    "format.text.underline",
    "tools.strikethrough",
    "text.strike.through",
    "text.font.color",
    "format.text_color.use_default",
    "format.text.align_left",
    "format.text.align_center",
    "format.text.align_right",
    "format.text.align_justify",
    "format.text.align_top",
    "format.text.align_middle",
    "format.text.align_bottom",
    "text.bullets",
    // Fill
    "format.set.fill.color",
    "format.fill_color.no_fill",
    "format.fill_color.use_default",
    "shape.transparency",
    "shape.transparency.dlg",
    "format.transparency.no_transparency",
    // Line
    "format.set.line.color",
    "format.line_color.use_default",
    "format.line_color.no_line",
    "format.set.line.weight",
    "format.set.line.pattern",
    "format.line_pattern.no_line",
    "format.set.line.ends",
    "format.line_ends.no_line_ends",
    // Arrange
    "arrange.align.box",
    "arrange.align.left",
    "arrange.align.center",
    "arrange.align.center_x",
    "arrange.align.right",
    "arrange.align.top",
    "arrange.align.middle",
    "arrange.align.bottom",
    "arrange.position",
    "arrange.distribute.horizontal",
    "arrange.distribute.vertical",
    "arrange.distribute_shapes.distribute_horizontal_spacing",
    "arrange.distribute_shapes.distribute_vertical_spacing",
    "arrange.distribute_shapes.distribute_center",
    "arrange.distribute_shapes.distribute_middle",
    "arrange.group",
    "arrange.ungroup",
    "arrange.bring_forward",
    "arrange.bring_to_front",
    "arrange.send_backward",
    "arrange.send_to_back",
    "arrange.rotate90.clockwise",
    "arrange.object.rotate90",
    "arrange.flip_horizontal",
    "arrange.flip_vertical",
    // Format Painter
    "tools.format_painter",
];

/// The controls the Panel Modern layout must link to an implemented
/// command of the set.
const MODERN_CONTROLS: &[&str] = &[
    "text.font",
    "text.font.size",
    "text.font.size.increase",
    "text.font.size.decrease",
    "format.text.bold",
    "format.text.italic",
    "format.text.underline",
    "tools.strikethrough",
    "text.font.color",
    "format.text.align_left",
    "format.text.align_center",
    "format.text.align_right",
    "format.text.align_justify",
    "format.text.align_top",
    "format.text.align_middle",
    "format.text.align_bottom",
    "text.bullets",
    "format.set.fill.color",
    "format.set.line.color",
    "format.set.line.weight",
    "format.set.line.pattern",
    "format.set.line.ends",
    "tools.format_painter",
    "arrange.align.box",
    "arrange.position",
    "arrange.group",
    "arrange.ungroup",
    "arrange.bring_forward",
    "arrange.bring_to_front",
    "arrange.send_to_back",
    "arrange.rotate90.clockwise",
    "arrange.object.rotate90",
    "arrange.flip_horizontal",
    "arrange.flip_vertical",
];

/// The controls the Panel Classic layout must link to an implemented
/// command of the set.
const CLASSIC_CONTROLS: &[&str] = &[
    "text.font",
    "text.font.size",
    "format.text.increase_size",
    "format.text.decrease_size",
    "format.text.bold",
    "format.text.italic",
    "format.text.underline",
    "text.strike.through",
    "text.font.color",
    "format.text.align_left",
    "format.text.align_center",
    "format.text.align_right",
    "text.bullets",
    "format.text.align_top",
    "format.text.align_middle",
    "format.text.align_bottom",
    "format.set.fill.color",
    "format.fill_color.no_fill",
    "format.text_color.use_default",
    "format.line_color.use_default",
    "format.line_color.no_line",
    "format.set.line.color",
    "format.set.line.weight",
    "format.set.line.pattern",
    "format.line_pattern.no_line",
    "format.set.line.ends",
    "format.line_ends.no_line_ends",
    "shape.transparency.dlg",
    "format.transparency.no_transparency",
    "tools.format_painter",
    "arrange.align.left",
    "arrange.align.center",
    "arrange.align.right",
    "arrange.align.top",
    "arrange.align.middle",
    "arrange.align.bottom",
    "arrange.distribute_shapes.distribute_horizontal_spacing",
    "arrange.distribute_shapes.distribute_vertical_spacing",
    "arrange.distribute_shapes.distribute_center",
    "arrange.distribute_shapes.distribute_middle",
    "arrange.group",
    "arrange.ungroup",
    "arrange.bring_forward",
    "arrange.bring_to_front",
    "arrange.send_backward",
    "arrange.send_to_back",
    "arrange.rotate90.clockwise",
    "arrange.object.rotate90",
    "arrange.flip_horizontal",
    "arrange.flip_vertical",
];

/// Every command id the two layouts link (`command` on a control),
/// collected over the whole v2 layout tree.
#[allow(clippy::expect_used)] // test helper: the embedded data must load
fn linked_commands(kind: LayoutKind) -> BTreeSet<String> {
    let sets = Loader::from_embedded().expect("the embedded data loads");
    let value = serde_json::to_value(sets.get(kind).v2.clone()).expect("the layout serialises");
    let mut out = BTreeSet::new();
    collect_commands(&value, &mut out);
    out
}

/// Recursing over the serialized layout: every `command` string.
fn collect_commands(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(command)) = map.get("command") {
                out.insert(command.clone());
            }
            for entry in map.values() {
                collect_commands(entry, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_commands(item, out);
            }
        }
        _ => {}
    }
}

/// Every formatting-set id resolves to an implemented command.
#[test]
fn every_formatting_command_is_implemented() {
    let registry = registry().expect("registry");
    for id in FORMAT_SET {
        let id = CommandId::parse(id).expect("formatting-set id");
        assert_eq!(
            registry.status(&id),
            CommandStatus::Implemented,
            "{id} must have a real handler"
        );
    }
}

/// Both layouts link their formatting controls to implemented commands
/// (task P7-21 goal 2: reachable from both layouts).
#[test]
fn the_layouts_link_the_formatting_set() {
    let registry = registry().expect("registry");
    for (kind, controls) in [
        (LayoutKind::PanelClassic, CLASSIC_CONTROLS),
        (LayoutKind::PanelModern, MODERN_CONTROLS),
    ] {
        let linked = linked_commands(kind);
        for id in controls {
            assert!(linked.contains(*id), "{kind:?} must link {id} to a control");
            let id = CommandId::parse(id).expect("control id");
            assert_eq!(
                registry.status(&id),
                CommandStatus::Implemented,
                "{kind:?} control {id} must be implemented"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The smoke run: every formatting command dispatches, undoes and redoes.
// ---------------------------------------------------------------------------

/// Dispatches one command over the registry.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn run(
    ctx: &mut AppCtx,
    id: &'static str,
    args: Args,
) -> Result<Outcome, larrygram_commands::CommandError> {
    let registry = registry().expect("registry");
    let id = CommandId::parse(id).expect("id");
    let invocation = Invocation::new(args, Source::Test);
    larrygram_commands::dispatch(&registry, ctx, &id, invocation)
}

/// One text argument.
fn text(name: &str, value: &str) -> Args {
    Args::new().with(name, ArgValue::Text(value.to_owned()))
}

/// One number argument.
fn number(name: &str, value: f64) -> Args {
    Args::new().with(name, ArgValue::Number(value))
}

/// A session with three drawn shapes, the middle one selected (so
/// every z-order command has somewhere to move and the format commands
/// have real cells).
#[allow(clippy::expect_used)] // test helper
fn prepared() -> AppCtx {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new.blank.drawing", Args::new()).expect("new drawing");
    let session = ctx.active_document_mut().expect("session");
    let page = session.active_page.expect("page");
    let mut selected = None;
    for (index, x) in [1.0_f64, 3.0, 5.0].into_iter().enumerate() {
        let mut shape = Shape::new(ShapeId { index: 0 }, i32::try_from(index).unwrap_or(0) + 1);
        for (name, value) in [
            ("PinX", x),
            ("PinY", 1.0),
            ("Width", 2.0),
            ("Height", 1.0),
            ("LocPinX", 1.0),
            ("LocPinY", 0.5),
            ("Angle", 0.0),
        ] {
            shape.sheet.cells.push(Cell {
                name: CellName::new(name),
                formula: Some(format!("{value}")),
                value: Some(CellValue::Number(value)),
                unit: larrygram_geom::Unit::Inches,
                preserved: Preserved::new(),
            });
        }
        shape.sheet.sections.push(geometry());
        let id = session
            .document
            .insert_shape(page, shape, None)
            .expect("shape");
        if index == 1 {
            selected = Some(id);
        }
    }
    let shape = selected.expect("the middle shape");
    ctx.selection.shapes = vec![shape];
    ctx.selection.anchor = Some(shape);
    ctx.selection.page = Some(page);
    ctx
}

/// A two-inch by one-inch rectangle geometry section.
fn geometry() -> Section {
    let mut section = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, x, y)) in [
        ("MoveTo", 0.0, 0.0),
        ("LineTo", 2.0, 0.0),
        ("LineTo", 2.0, 1.0),
        ("LineTo", 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![
            Cell {
                name: CellName::new("X"),
                formula: Some(format!("{x}")),
                value: Some(CellValue::Number(x)),
                unit: larrygram_geom::Unit::Inches,
                preserved: Preserved::new(),
            },
            Cell {
                name: CellName::new("Y"),
                formula: Some(format!("{y}")),
                value: Some(CellValue::Number(y)),
                unit: larrygram_geom::Unit::Inches,
                preserved: Preserved::new(),
            },
        ];
        section.rows.push(row);
    }
    section
}

/// Every command of the formatting set runs over a prepared session,
/// then undoes and redoes (task P7-21 acceptance: "every command of the
/// basic set runs from both layouts with undo").
#[test]
fn every_formatting_command_runs_with_undo_and_redo() {
    let cases: &[(&str, Args)] = &[
        ("text.font", text("value", "Selawik")),
        ("text.font.size", number("size", 14.0)),
        ("text.font.size.increase", Args::new()),
        ("text.font.size.decrease", Args::new()),
        ("format.text.increase_size", Args::new()),
        ("format.text.decrease_size", Args::new()),
        ("format.text.bold", Args::new()),
        ("format.text.italic", Args::new()),
        ("format.text.underline", Args::new()),
        ("tools.strikethrough", Args::new()),
        ("text.strike.through", Args::new()),
        ("text.font.color", text("value", "#1F5BC4")),
        ("format.text_color.use_default", Args::new()),
        ("format.text.align_left", Args::new()),
        ("format.text.align_center", Args::new()),
        ("format.text.align_right", Args::new()),
        ("format.text.align_justify", Args::new()),
        ("format.text.align_top", Args::new()),
        ("format.text.align_middle", Args::new()),
        ("format.text.align_bottom", Args::new()),
        ("text.bullets", Args::new()),
        ("format.set.fill.color", text("color", "red")),
        ("format.fill_color.no_fill", Args::new()),
        ("format.fill_color.use_default", Args::new()),
        ("shape.transparency", number("transparency", 40.0)),
        ("shape.transparency.dlg", number("value", 25.0)),
        ("format.transparency.no_transparency", Args::new()),
        ("format.set.line.color", text("value", "blue")),
        ("format.line_color.use_default", Args::new()),
        ("format.line_color.no_line", Args::new()),
        ("format.set.line.weight", number("weight", 2.0)),
        ("format.set.line.pattern", number("pattern", 2.0)),
        ("format.line_pattern.no_line", Args::new()),
        ("format.set.line.ends", number("value", 1.0)),
        ("format.line_ends.no_line_ends", Args::new()),
        ("arrange.align.box", text("value", "left")),
        ("arrange.align.left", Args::new()),
        ("arrange.align.center", Args::new()),
        ("arrange.align.center_x", Args::new()),
        ("arrange.align.right", Args::new()),
        ("arrange.align.top", Args::new()),
        ("arrange.align.middle", Args::new()),
        ("arrange.align.bottom", Args::new()),
        ("arrange.position", text("value", "distribute horizontal")),
        ("arrange.distribute.horizontal", Args::new()),
        ("arrange.distribute.vertical", Args::new()),
        (
            "arrange.distribute_shapes.distribute_horizontal_spacing",
            Args::new(),
        ),
        (
            "arrange.distribute_shapes.distribute_vertical_spacing",
            Args::new(),
        ),
        ("arrange.distribute_shapes.distribute_center", Args::new()),
        ("arrange.distribute_shapes.distribute_middle", Args::new()),
        ("arrange.bring_forward", Args::new()),
        ("arrange.bring_to_front", Args::new()),
        ("arrange.send_backward", Args::new()),
        ("arrange.send_to_back", Args::new()),
        ("arrange.rotate90.clockwise", Args::new()),
        ("arrange.object.rotate90", Args::new()),
        ("arrange.flip_horizontal", Args::new()),
        ("arrange.flip_vertical", Args::new()),
    ];
    for (id, args) in cases {
        // A fresh selection per case: the commands run in isolation,
        // like a user running one of them.
        let mut ctx = prepared();
        let before = ctx
            .active_document()
            .map(|session| session.undo.len())
            .unwrap_or(0);
        run(&mut ctx, id, args.clone()).unwrap_or_else(|error| panic!("{id}: {error}"));
        let after = ctx
            .active_document()
            .map(|session| session.undo.len())
            .unwrap_or(0);
        assert_eq!(after, before + 1, "{id} pushes exactly one undo step");
        run(&mut ctx, "edit.undo", Args::new()).expect("undo");
        run(&mut ctx, "edit.redo", Args::new()).expect("redo");
    }

    // Group and ungroup run as a pair (ungroup needs the group).
    let mut ctx = prepared();
    ctx.selection
        .shapes
        .push(ctx.selection.anchor.expect("anchor"));
    run(&mut ctx, "arrange.group", Args::new()).expect("arrange.group");
    run(&mut ctx, "edit.undo", Args::new()).expect("undo");
    run(&mut ctx, "edit.redo", Args::new()).expect("redo");
    run(&mut ctx, "arrange.ungroup", Args::new()).expect("arrange.ungroup");
    run(&mut ctx, "edit.undo", Args::new()).expect("undo");
    run(&mut ctx, "edit.redo", Args::new()).expect("redo");
}

/// `tools.format_painter` runs from the selection: the copy form, then
/// the apply form over a second shape (its one undo step).
#[test]
fn the_format_painter_copies_and_applies() {
    let mut ctx = prepared();
    run(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
    let args = Args::new().with("apply", ArgValue::Flag(true));
    run(&mut ctx, "tools.format_painter", args).expect("apply");
    run(&mut ctx, "edit.undo", Args::new()).expect("undo");
    run(&mut ctx, "edit.redo", Args::new()).expect("redo");
}

/// The `apply` flag form of the painter is a real flag as well as a
/// text spelling.
#[test]
fn the_format_painter_takes_a_flag() {
    let mut ctx = prepared();
    run(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
    let args = Args::new().with("apply", ArgValue::Flag(true));
    run(&mut ctx, "tools.format_painter", args).expect("apply");
}

// ---------------------------------------------------------------------------
// The render check: the cells and the picture agree.
// ---------------------------------------------------------------------------

/// Formats one shape and reads the change back out of the sheet and out
/// of the rendered scene (task P7-21 acceptance: "changes render: a test
/// renders a formatted shape and checks the cells and the picture").
#[test]
fn a_formatted_shape_renders_its_cells_and_its_picture() {
    let mut ctx = prepared();
    run(&mut ctx, "format.set.fill.color", text("color", "#FF0000")).expect("fill");
    run(&mut ctx, "shape.transparency", number("transparency", 50.0)).expect("transparency");
    run(&mut ctx, "format.set.line.color", text("value", "#0000FF")).expect("line color");
    run(&mut ctx, "format.set.line.weight", number("weight", 2.0)).expect("line weight");
    run(&mut ctx, "format.set.line.pattern", number("pattern", 2.0)).expect("line pattern");
    run(&mut ctx, "format.set.line.ends", number("value", 1.0)).expect("line ends");
    run(&mut ctx, "text.font", text("value", "Selawik")).expect("font");
    run(&mut ctx, "text.font.size", number("size", 12.0)).expect("size");
    run(&mut ctx, "format.text.bold", Args::new()).expect("bold");
    run(&mut ctx, "format.text.italic", Args::new()).expect("italic");
    run(&mut ctx, "format.text.underline", Args::new()).expect("underline");
    run(&mut ctx, "tools.strikethrough", Args::new()).expect("strikethrough");
    run(&mut ctx, "text.font.color", text("value", "#008000")).expect("font color");
    run(&mut ctx, "shape.set.text", text("text", "Hello")).expect("text");

    // The cells the renderer reads.
    let session = ctx.active_document().expect("session");
    let shape = ctx.selection.shapes.first().copied().expect("shape");
    let entry = session.document.shape(shape).expect("shape entry");
    let direct = |name: &str| -> Option<CellValue> {
        entry
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.clone())
    };
    let row_value = |section: &str, name: &str| -> Option<CellValue> {
        entry
            .sheet
            .sections
            .iter()
            .find(|block| block.name.as_str() == section)
            .and_then(|block| block.rows.first())
            .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
            .and_then(|cell| cell.value.clone())
    };
    assert_eq!(
        direct("FillForegnd"),
        Some(CellValue::Color(larrygram_model::Color::new(255, 0, 0)))
    );
    assert_eq!(direct("FillPattern"), Some(CellValue::Number(1.0)));
    assert_eq!(direct("FillForegndTrans"), Some(CellValue::Number(0.5)));
    assert_eq!(
        direct("LineColor"),
        Some(CellValue::Color(larrygram_model::Color::new(0, 0, 255)))
    );
    assert_eq!(direct("LineWeight"), Some(CellValue::Number(2.0 / 72.0)));
    assert_eq!(direct("LinePattern"), Some(CellValue::Number(2.0)));
    assert_eq!(direct("EndArrow"), Some(CellValue::Number(1.0)));
    assert_eq!(
        row_value("Character", "Font"),
        Some(CellValue::Str("Selawik".to_owned()))
    );
    assert_eq!(
        row_value("Character", "Size"),
        Some(CellValue::Number(1.0 / 6.0))
    );
    assert_eq!(
        row_value("Character", "Style"),
        Some(CellValue::Number(1.0 + 2.0 + 4.0))
    );
    assert_eq!(
        row_value("Character", "Strikethru"),
        Some(CellValue::Bool(true))
    );
    assert_eq!(
        row_value("Character", "Color"),
        Some(CellValue::Color(larrygram_model::Color::new(0, 0x80, 0)))
    );

    // The picture (the builder draws one node per path: a filled node
    // and a stroked node, both of the shape).
    let page = session.active_page.expect("page");
    let scene = larrygram_render::build_scene(
        &session.document,
        page,
        &larrygram_render::BuildOptions::default(),
    )
    .expect("scene");
    let mut bodies = Vec::new();
    let mut stack: Vec<&larrygram_render::scene::Node> = scene.nodes.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            larrygram_render::scene::Node::Shape(body) => bodies.push(body.as_ref()),
            larrygram_render::scene::Node::Group(group) => {
                stack.extend(group.children.iter());
            }
            _ => {}
        }
    }
    bodies.retain(|body| body.shape_id == Some(shape));
    assert_eq!(bodies.len(), 2, "one filled and one stroked node");
    let filled = bodies
        .iter()
        .find(|body| body.fill.is_some())
        .expect("the filled node");
    let stroked = bodies
        .iter()
        .find(|body| body.stroke.is_some())
        .expect("the stroked node");
    let body = *filled;
    let fill = body.fill.as_ref().expect("the fill renders");
    let larrygram_render::scene::Paint::Solid(color) = fill else {
        panic!("the fill is solid");
    };
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (255, 0, 0, 128),
        "the fill colour at half transparency"
    );
    let stroke = stroked.stroke.as_ref().expect("the stroke renders");
    assert_eq!(stroke.width, 2.0 / 72.0, "the weight in inches");
    assert!(!stroke.dash.is_empty(), "pattern 2 dashes the stroke");
    let larrygram_render::scene::Paint::Solid(line) = &stroke.paint else {
        panic!("the stroke is solid");
    };
    assert_eq!((line.r, line.g, line.b), (0, 0, 255));
    assert!(stroke.end.is_some(), "the arrowhead renders");
    let text_node = bodies
        .iter()
        .find_map(|body| body.text.as_ref())
        .expect("the text renders");
    let run = text_node.runs.first().expect("one run");
    assert!(run.underline, "the run is underlined");
    assert!(run.strikethrough, "the run is struck through");
    assert_eq!((run.color.r, run.color.g, run.color.b), (0, 0x80, 0));
}
