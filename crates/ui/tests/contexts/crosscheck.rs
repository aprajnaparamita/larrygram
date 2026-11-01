//! Enablement cross-check: every implemented command's predicate against
//! its `enabled_in` row (task P7-08, ADR 0064).

use larrygram_commands::builtins::registry;
use larrygram_ui_spec::v2::{ContextId, Contexts};

use super::CONTEXTS_YAML;
use super::documents;

/// A deliberate difference between a predicate and the table, with its
/// reason.
struct Allow {
    command: &'static str,
    context: ContextId,
    reason: &'static str,
}

/// Implemented commands with no `contexts.yaml` row. The table is the
/// 2007 Visio availability list; these ids are Larrygram-only switches or
/// Phase 5/7 additions made after it.
const NO_ROW: &[&str] = &[
    "app.office.center.options",
    "app.quit",
    "arrange.align.bottom",
    "arrange.align.center",
    "arrange.align.center_x",
    "arrange.align.left",
    "arrange.align.middle",
    "arrange.align.right",
    "arrange.align.top",
    "arrange.distribute.horizontal",
    "arrange.distribute.vertical",
    "arrange.distribute_shapes.distribute_center",
    "arrange.distribute_shapes.distribute_horizontal_spacing",
    "arrange.distribute_shapes.distribute_middle",
    "arrange.distribute_shapes.distribute_vertical_spacing",
    "arrange.flip_horizontal",
    "arrange.flip_vertical",
    "arrange.object.rotate90",
    "arrange.rotate90.clockwise",
    "connector.draw",
    "connector.glue_begin",
    "connector.glue_end",
    "connector.reroute_all",
    "connector.unglue",
    "data.set.property",
    "edit.clear",
    "edit.delete",
    "edit.deselect",
    "edit.duplicate",
    "edit.paste.in.place",
    "edit.redo",
    "edit.select",
    "file.close",
    "file.exit",
    "file.import",
    "file.export",
    "file.inspect",
    "file.new.blank.drawing",
    "file.new.blank.drawing.metric",
    "file.new.blank.drawing.us",
    "file.render",
    "file.save_as",
    "format.fill_color.no_fill",
    "format.fill_color.use_default",
    "format.line_color.no_line",
    "format.line_color.use_default",
    "format.line_ends.no_line_ends",
    "format.line_pattern.no_line",
    "format.set.line.ends",
    "format.text.decrease_size",
    "format.text.increase_size",
    "format.text_color.use_default",
    "format.transparency.no_transparency",
    "help.about",
    "help.welcome",
    "help.welcome.next_start",
    "insert.drawing.ellipse",
    "insert.drawing.line",
    "insert.drawing.rectangle",
    "insert.drawing.text_box",
    "insert.shape",
    "page.add",
    "page.delete",
    "page.move_left",
    "page.move_right",
    "page.next",
    "page.previous",
    "page.rename",
    "page.reorder",
    "page.reorder_pages",
    "shape.draw.zoom",
    "shape.ellipse",
    "shape.enter_group",
    "shape.exit_group",
    "shape.line",
    "shape.move",
    "shape.rectangle",
    "shape.resize",
    "shape.rotate",
    "shape.set.angle",
    "shape.set.height",
    "shape.set.pin",
    "shape.set.text",
    "shape.set.text_block",
    "shape.set.width",
    "shape.transparency",
    "shape.transparency.dlg",
    "text.begin",
    "text.cancel",
    "text.commit",
    "text.strike.through",
    "tools.options.go.to.drawing",
    "tools.options.new.page",
    "tools.options.page.setup",
    "tools.options.preferences",
    "view.custom",
    "view.fit.in.window",
    "view.full_screen_mode",
    "view.page.breaks",
    "view.toggle_guides",
    "view.ui_layout.panel_2016",
    "view.ui_layout.panel_365",
    "view.ui_layout.panel_classic",
    "view.ui_layout.panel_modern",
    "view.units",
    "view.view100",
    "view.view150",
    "view.view200",
    "view.view400",
    "view.view50",
    "view.view75",
    "view.zoom.in",
    "view.zoom.out",
];

const NO_ROW_REASON: &str = "Larrygram-only or Phase 5/7 command with no 2007 contexts.yaml row";

/// Differences accepted for P7-08. Each reason says why the table row
/// and the runtime predicate must differ.
const ALLOWED: &[Allow] = &[
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::EmptyPage,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::ShapesSelected,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::GroupSelected,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::SecondPage,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::PictureSelected,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "tools.picture.insert.from.file",
        context: ContextId::ContainerSelected,
        reason: "the command stops at the image pipeline boundary (task P7-22 item 5): the .vsdx media writer and the image renderer are missing",
    },
    Allow {
        command: "insert.text.box",
        context: ContextId::ShapesheetWindow,
        reason: "the menu command places a text box at the page centre and needs an open page (task P7-22 item 4); the 2007 table hides it in the ShapeSheet window",
    },
    Allow {
        command: "insert.vert.text.box",
        context: ContextId::ShapesheetWindow,
        reason: "the menu command places a text box at the page centre and needs an open page (task P7-22 item 4); the 2007 table hides it in the ShapeSheet window",
    },
    Allow {
        command: "arrange.group",
        context: ContextId::TextEditing,
        reason: "the manual-pass rule: Group is enabled with two or more shapes selected (task P7-22); the 2007 table also offers it on one group, picture or container",
    },
    Allow {
        command: "arrange.group",
        context: ContextId::GroupSelected,
        reason: "the manual-pass rule: Group is enabled with two or more shapes selected (task P7-22); the 2007 table also offers it on one group, picture or container",
    },
    Allow {
        command: "arrange.group",
        context: ContextId::PictureSelected,
        reason: "the manual-pass rule: Group is enabled with two or more shapes selected (task P7-22); the 2007 table also offers it on one group, picture or container",
    },
    Allow {
        command: "arrange.group",
        context: ContextId::ContainerSelected,
        reason: "the manual-pass rule: Group is enabled with two or more shapes selected (task P7-22); the 2007 table also offers it on one group, picture or container",
    },
    Allow {
        command: "arrange.align.box",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "arrange.position",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.set.fill.color",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.set.line.color",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.set.line.pattern",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.set.line.weight",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_bottom",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_center",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_justify",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_left",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_middle",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_right",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.align_top",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.bold",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.italic",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "format.text.underline",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.bullets",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.font",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.font.color",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.font.size",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.font.size.decrease",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "text.font.size.increase",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "tools.strikethrough",
        context: ContextId::ShapesheetWindow,
        reason: "the formatting command needs a selection; the 2007 table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "arrange.position",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "arrange.position",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.fill.color",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.fill.color",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.color",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.color",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.pattern",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.pattern",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.weight",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.set.line.weight",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_bottom",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_bottom",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_center",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_center",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_justify",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_justify",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_left",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_left",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_middle",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_middle",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_right",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_right",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_top",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.align_top",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.bold",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.bold",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.italic",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.italic",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.underline",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "format.text.underline",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.bullets",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.bullets",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.color",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.color",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size.decrease",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size.decrease",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size.increase",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "text.font.size.increase",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "tools.strikethrough",
        context: ContextId::EmptyPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "tools.strikethrough",
        context: ContextId::SecondPage,
        reason: "the formatting command needs a selection; the 2007 table lists it wherever a page is open (task P7-21)",
    },
    Allow {
        command: "edit.redo.or.repeat",
        context: ContextId::ShapesSelected,
        reason: "the repeat form is edit.redo: enabled only with a redoable edit (task P7-21)",
    },
    Allow {
        command: "edit.redo.or.repeat",
        context: ContextId::TextEditing,
        reason: "the repeat form is edit.redo: enabled only with a redoable edit (task P7-21)",
    },
    Allow {
        command: "tools.format_painter",
        context: ContextId::ShapesheetWindow,
        reason: "the painter needs a selection whatever surface is open; the table hides it in the ShapeSheet window (task P7-21)",
    },
    Allow {
        command: "arrange.bring_forward",
        context: ContextId::GroupSelected,
        reason: "z-order applies to one selected group; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_forward",
        context: ContextId::PictureSelected,
        reason: "z-order applies to one selected picture; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_forward",
        context: ContextId::TextEditing,
        reason: "z-order follows the edited shape; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_to_front",
        context: ContextId::GroupSelected,
        reason: "z-order applies to one selected group; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_to_front",
        context: ContextId::PictureSelected,
        reason: "z-order applies to one selected picture; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_to_front",
        context: ContextId::TextEditing,
        reason: "z-order follows the edited shape; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_backward",
        context: ContextId::GroupSelected,
        reason: "z-order applies to one selected group; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_backward",
        context: ContextId::PictureSelected,
        reason: "z-order applies to one selected picture; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_backward",
        context: ContextId::TextEditing,
        reason: "z-order follows the edited shape; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_to_back",
        context: ContextId::GroupSelected,
        reason: "z-order applies to one selected group; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_to_back",
        context: ContextId::PictureSelected,
        reason: "z-order applies to one selected picture; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.send_to_back",
        context: ContextId::TextEditing,
        reason: "z-order follows the edited shape; the table lists only multi/container rows",
    },
    Allow {
        command: "arrange.bring_forward",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "arrange.bring_to_front",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "arrange.group",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "arrange.send_backward",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "arrange.send_to_back",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "arrange.ungroup",
        context: ContextId::PictureSelected,
        reason: "the table lists pictures, but an imported picture has no group children to dissolve",
    },
    Allow {
        command: "connector.style",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "edit.select_all",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::EmptyPage,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::ShapesSelected,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::TextEditing,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::GroupSelected,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::SecondPage,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::PictureSelected,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::ContainerSelected,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "edit.undo",
        context: ContextId::ShapesheetWindow,
        reason: "the test document starts with no undo history",
    },
    Allow {
        command: "tools.connector",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.ellipse",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.pointer",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.rectangle",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.text",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.rulers",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.toggle_grid",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.zoom.fit",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    // The task P7-20 commands: the same window-mode rule.
    Allow {
        command: "edit.duplicate.page",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "edit.paste.duplicate",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "format.set.page.orientation",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "insert.new.background.page",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "page.size.dlg",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.backgrounds",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.blank.page.insert",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "tools.blank.page.insert.visio",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.guides",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.show.page.breaks",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.zoom.dialog",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
    Allow {
        command: "view.zoom.page.width",
        context: ContextId::ShapesheetWindow,
        reason: "the window mode is UI state that an AppCtx predicate cannot see",
    },
];

fn allowed(command: &str, context: ContextId) -> Option<&'static str> {
    ALLOWED
        .iter()
        .find(|entry| entry.command == command && entry.context == context)
        .map(|entry| entry.reason)
}

fn no_row_reason(command: &str) -> Option<&'static str> {
    NO_ROW.contains(&command).then_some(NO_ROW_REASON)
}

fn contexts() -> Contexts {
    serde_saphyr::from_str(CONTEXTS_YAML).expect("the committed contexts table decodes")
}

#[test]
fn every_implemented_command_matches_its_enabled_in_row() {
    let table = contexts();
    let registry = registry().expect("the builtin registry builds");
    let pending: Vec<ContextId> = table
        .contexts
        .iter()
        .filter(|context| context.status.as_deref() == Some("pending"))
        .map(|context| context.id)
        .collect();

    let mut failures: Vec<String> = Vec::new();
    for definition in &table.contexts {
        if definition.status.as_deref() == Some("pending") {
            continue;
        }
        let case = documents::for_id(definition.id).unwrap_or_else(|| {
            panic!(
                "no test document for non-pending context {:?}",
                definition.id
            )
        });
        for id in registry.ids() {
            if registry.status(&id) != larrygram_commands::CommandStatus::Implemented {
                continue;
            }
            let Some(availability) = table.commands.get(id.as_str()) else {
                if no_row_reason(id.as_str()).is_none() {
                    failures.push(format!(
                        "{} in {:?}: implemented but not in contexts.yaml",
                        id, definition.id
                    ));
                }
                continue;
            };
            let expected = availability.enabled_in.contains(&definition.id);
            let actual = registry.enabled(&id, &case.app).is_enabled();
            if expected != actual && allowed(id.as_str(), definition.id).is_none() {
                failures.push(format!(
                    "{} in {:?}: table {}, predicate {}",
                    id,
                    definition.id,
                    if expected { "enabled" } else { "disabled" },
                    if actual { "enabled" } else { "disabled" }
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} enablement difference(s); add each to ALLOWED with a reason or fix the predicate:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        !pending.is_empty(),
        "the pending contexts are skipped, not silently absent"
    );
}
