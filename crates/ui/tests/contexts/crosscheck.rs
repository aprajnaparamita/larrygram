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
    "arrange.align.center_x",
    "arrange.align.left",
    "arrange.align.middle",
    "arrange.align.right",
    "arrange.align.top",
    "arrange.distribute.horizontal",
    "arrange.distribute.vertical",
    "connector.draw",
    "connector.glue_begin",
    "connector.glue_end",
    "connector.reroute_all",
    "connector.unglue",
    "data.set.property",
    "edit.delete",
    "edit.deselect",
    "edit.duplicate",
    "edit.redo",
    "edit.select",
    "file.close",
    "file.exit",
    "file.export",
    "file.inspect",
    "file.render",
    "file.save_as",
    "help.about",
    "help.welcome",
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
    "shape.set.width",
    "text.begin",
    "text.cancel",
    "text.commit",
    "view.ui_layout.panel_2016",
    "view.ui_layout.panel_365",
    "view.ui_layout.panel_classic",
    "view.ui_layout.panel_modern",
    "view.units",
    "view.zoom.in",
    "view.zoom.out",
];

const NO_ROW_REASON: &str = "Larrygram-only or Phase 5/7 command with no 2007 contexts.yaml row";

/// Differences accepted for P7-08. Each reason says why the table row
/// and the runtime predicate must differ.
const ALLOWED: &[Allow] = &[
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
