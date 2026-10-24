//! What a Panel Modern control does when it is activated (ADR 0031,
//! ADR 0028).
//!
//! The widgets resolve a control's `command` through the registry and
//! render anything not dispatchable as disabled with a reason
//! tooltip. The seam is thin (`Resolve`); the tests drive the same
//! widget code with their own mapping.

use std::collections::BTreeSet;

use larrygram_commands::{AppCtx, CommandId, CommandStatus, Invocation, Registry, Source};
use larrygram_ui_spec::embedded::is_out_of_scope;
use larrygram_ui_spec::v2::Item;

use crate::{Intent, UiError};

/// Tooltip of a control whose command has no handler yet (ADR 0031).
pub(crate) const NOT_YET_IMPLEMENTED: &str = "Not yet implemented";

/// Tooltip of a control listed in `ui-spec/out-of-scope.txt`
/// (ADR 0031).
pub(crate) const OUT_OF_SCOPE: &str = "Out of scope for this run";

/// The tooltip of a dropdown arrow (ADR 0031). It doubles as the
/// arrow's accessible name: the arrow is a painted mark (egui's fonts
/// have no triangle glyph) and the tooltip text is ours.
pub(crate) const DROPDOWN_TIP: &str = "More items";

/// Recursion cap for nested menus (AGENTS.md section 6).
pub(crate) const MAX_DEPTH: u32 = 8;

/// Display-only toggle state for `check` and `toggle` controls (the
/// command state lives in `AppCtx`, ADR 0029).
pub(crate) type Toggles = BTreeSet<String>;

/// What one control does when it is activated (ADR 0031).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    /// Dispatch this intent when the widget is activated.
    Emit(Intent),
    /// Render the widget disabled with this tooltip.
    Disabled {
        /// The tooltip text (ours, never Microsoft's).
        reason: String,
    },
}

/// How the widgets resolve a control to its [`Action`].
///
/// [`crate::panel_modern::PanelModern::show`] resolves through the
/// command registry (ADR 0028); the tests drive the same widget code
/// with their own mapping.
pub(crate) type Resolve<'a> = &'a dyn Fn(&Item) -> Action;

/// The action of one item through the command registry (ADR 0031
/// control-to-command rules).
///
/// An implemented and enabled command becomes an [`Intent`] from
/// [`Source::CommandBar`]. Everything else renders disabled: the
/// `Enabled` reason when the command is implemented but disabled,
/// else "Not yet implemented". A control whose Fluent id is listed in
/// `ui-spec/out-of-scope.txt` is disabled with the out-of-scope
/// tooltip whatever its command says.
pub(crate) fn action_for_item(item: &Item, registry: &Registry, app: &AppCtx) -> Action {
    if let Some(fluent) = &item.fluent
        && is_out_of_scope(fluent.as_str())
    {
        return Action::Disabled {
            reason: OUT_OF_SCOPE.to_owned(),
        };
    }
    let stub = Action::Disabled {
        reason: NOT_YET_IMPLEMENTED.to_owned(),
    };
    let Some(command) = &item.command else {
        return stub;
    };
    let Ok(id) = CommandId::try_from(command) else {
        return stub;
    };
    let status = registry.status(&id);
    let enabled = registry.enabled(&id, app);
    if status == CommandStatus::Implemented && enabled.is_enabled() {
        return Action::Emit(Intent::new(id, Invocation::empty(Source::CommandBar)));
    }
    let reason = if status == CommandStatus::Implemented {
        enabled
            .reason
            .clone()
            .unwrap_or_else(|| NOT_YET_IMPLEMENTED.to_owned())
    } else {
        NOT_YET_IMPLEMENTED.to_owned()
    };
    Action::Disabled { reason }
}

/// The label a control draws today: the layout's label, replaced by
/// the run-time one for `dynamic_label` items (`docs/ui/panel-modern.md`).
///
/// Undo reads "Undo `<action>`" and Redo "Repeat `<action>`" or
/// "Can't Redo"; with nothing to undo the label is "Can't Undo".
pub(crate) fn display_label(item: &Item, app: &AppCtx) -> String {
    let label = item.label.clone().unwrap_or_default();
    if !item.dynamic_label {
        return label;
    }
    let Some(command) = item.command.as_ref().map(|id| id.as_str()) else {
        return label;
    };
    let Some(session) = app.active_document() else {
        return fallback_label(command, &label);
    };
    match command {
        "edit.undo" => match session.undo.undo_labels().pop() {
            Some(action) => format!("Undo {action}"),
            None => "Can't Undo".to_owned(),
        },
        "edit.redo.or.repeat" => match session.undo.redo_labels().first() {
            Some(action) => format!("Repeat {action}"),
            None => "Can't Redo".to_owned(),
        },
        _ => label,
    }
}

/// The dynamic label of a command with no document open.
fn fallback_label(command: &str, label: &str) -> String {
    match command {
        "edit.undo" => "Can't Undo".to_owned(),
        "edit.redo.or.repeat" => "Can't Redo".to_owned(),
        _ => label.to_owned(),
    }
}

/// The tooltip of an enabled control: the label, then the shortcut in
/// brackets (`docs/ui/panel-modern.md`). The one-sentence description
/// of the spec needs a per-command description, which the command
/// metadata does not carry yet (task P7-07 Result, open issues).
pub(crate) fn tooltip_text(label: &str, item: &Item) -> String {
    match &item.shortcut {
        Some(shortcut) => format!("{label} ({shortcut})"),
        None => label.to_owned(),
    }
}

/// Finishes one widget: dispatch its intent on activation, or grey it
/// out with its tooltip (ADR 0031).
pub(crate) fn finish(response: egui::Response, action: &Action, tip: &str, out: &mut Vec<Intent>) {
    match action {
        Action::Emit(intent) => {
            let response = response.on_hover_text(tip);
            if response.clicked() {
                out.push(intent.clone());
            }
        }
        Action::Disabled { reason } => {
            response.on_disabled_hover_text(reason.as_str());
        }
    }
}

/// Which way a painted arrow points (egui's bundled fonts have no
/// triangle or arrow glyph; ADR 0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrow {
    /// Points left (the File view back button).
    Left,
    /// Points right (a submenu, a launcher).
    Right,
    /// Points down (a dropdown or split arrow).
    Down,
}

/// A small filled triangle, for the split arrows and the back button.
pub(crate) fn arrow_atom(size: f32, direction: Arrow) -> egui::Atom<'static> {
    egui::Atom::paint(egui::Vec2::splat(size), move |ui: &egui::Ui, args| {
        let rect = args.rect;
        let half = rect.width().min(rect.height()) * 0.25;
        let center = rect.center();
        let points = match direction {
            Arrow::Left => vec![
                egui::pos2(center.x + half * 0.6, center.y - half),
                egui::pos2(center.x - half * 0.8, center.y),
                egui::pos2(center.x + half * 0.6, center.y + half),
            ],
            Arrow::Right => vec![
                egui::pos2(center.x - half * 0.6, center.y - half),
                egui::pos2(center.x + half * 0.8, center.y),
                egui::pos2(center.x - half * 0.6, center.y + half),
            ],
            Arrow::Down => vec![
                egui::pos2(center.x - half, center.y - half * 0.6),
                egui::pos2(center.x + half, center.y - half * 0.6),
                egui::pos2(center.x, center.y + half * 0.8),
            ],
        };
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            args.fallback_text_color,
            egui::Stroke::NONE,
        ));
    })
}

/// An empty atom of `width`, for menu gutters and spacers.
pub(crate) fn spacer(width: f32) -> egui::Atom<'static> {
    egui::Atom::paint(egui::Vec2::new(width, 1.0), |_, _| {})
}

/// A recursion boundary: deeper entries render as plain labels so no
/// input can recurse without bound (AGENTS.md section 6).
pub(crate) fn too_deep(ui: &mut egui::Ui, item: &Item) {
    let label = item.label.clone().unwrap_or_default();
    ui.label(label);
}

/// The error path used by every widget seam (kept for the `Result`
/// the public API returns).
#[allow(dead_code)]
pub(crate) fn no_error() -> Result<(), UiError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
    use larrygram_commands::{CommandError, Edit, Registry};
    use larrygram_model::Document;
    use larrygram_ui_spec::v2::{Item, ItemKind};

    use super::*;

    /// A no-op edit with a label, for the undo-stack tests.
    struct NoopEdit(&'static str);

    impl Edit for NoopEdit {
        fn label(&self) -> &str {
            self.0
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
    }

    /// A minimal item.
    fn item(id: &str, command: Option<&str>) -> Item {
        Item {
            id: id.to_owned(),
            kind: ItemKind::Button,
            label: Some("Label".to_owned()),
            access: None,
            shortcut: Some("Ctrl+X".to_owned()),
            command: command.map(larrygram_ui_spec::CommandId::new),
            new_command: false,
            fluent: None,
            visuicmd: None,
            icon: None,
            display: None,
            size: None,
            column: None,
            row: None,
            width: None,
            begin_group: false,
            hidden: false,
            dynamic_label: false,
            list: None,
            options: Vec::new(),
            items: Vec::new(),
        }
    }

    /// An app with one document.
    fn app_with_document() -> AppCtx {
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(Document::new(), None));
        app
    }

    #[test]
    fn tooltips_show_the_label_and_the_shortcut() {
        let plain = item("a", None);
        assert_eq!(tooltip_text("Label", &plain), "Label (Ctrl+X)");
        let mut no_shortcut = item("a", None);
        no_shortcut.shortcut = None;
        assert_eq!(tooltip_text("Label", &no_shortcut), "Label");
    }

    #[test]
    fn dynamic_labels_read_the_undo_stack() {
        let mut app = app_with_document();
        app.documents[0].undo.push(Box::new(NoopEdit("Move shape")));
        app.documents[0]
            .undo
            .push(Box::new(NoopEdit("Resize shape")));

        let mut undo = item("qat.undo", Some("edit.undo"));
        undo.dynamic_label = true;
        assert_eq!(display_label(&undo, &app), "Undo Resize shape");

        let mut redo = item("qat.redo", Some("edit.redo.or.repeat"));
        redo.dynamic_label = true;
        assert_eq!(display_label(&redo, &app), "Can't Redo");

        // Move one edit to the redo tail: take the stack out the way
        // the dispatcher does.
        let mut stack = std::mem::take(&mut app.documents[0].undo);
        let reverted = stack.undo(&mut app).expect("the edit reverts");
        assert_eq!(reverted.as_deref(), Some("Resize shape"));
        app.documents[0].undo = stack;

        assert_eq!(display_label(&undo, &app), "Undo Move shape");
        assert_eq!(display_label(&redo, &app), "Repeat Resize shape");
    }

    #[test]
    fn dynamic_labels_without_a_document() {
        let app = AppCtx::new(Settings::default());
        let mut undo = item("qat.undo", Some("edit.undo"));
        undo.dynamic_label = true;
        assert_eq!(display_label(&undo, &app), "Can't Undo");
        let mut redo = item("qat.redo", Some("edit.redo.or.repeat"));
        redo.dynamic_label = true;
        assert_eq!(display_label(&redo, &app), "Can't Redo");
        // A dynamic label for any other command keeps the label.
        let mut other = item("qat.save", Some("file.save"));
        other.dynamic_label = true;
        assert_eq!(display_label(&other, &app), "Label");
        // A dynamic item with no command keeps the label.
        let mut no_command = item("qat.thing", None);
        no_command.dynamic_label = true;
        assert_eq!(display_label(&no_command, &app), "Label");
    }

    #[test]
    fn actions_resolve_through_the_registry() {
        let registry = Registry::default();
        let app = AppCtx::new(Settings::default());
        // A control with no command is a stub.
        let plain = item("a", None);
        assert!(matches!(
            action_for_item(&plain, &registry, &app),
            Action::Disabled { reason } if reason == NOT_YET_IMPLEMENTED
        ));
        // A command id that does not even parse is a stub.
        let broken = item("a", Some("Not A Command Id"));
        assert!(matches!(
            action_for_item(&broken, &registry, &app),
            Action::Disabled { .. }
        ));
        // An unknown command id is a stub.
        let unknown = item("a", Some("file.unknown_thing"));
        assert!(matches!(
            action_for_item(&unknown, &registry, &app),
            Action::Disabled { reason } if reason == NOT_YET_IMPLEMENTED
        ));
        // A Fluent id listed as out of scope is out of scope.
        let mut oos = item("a", Some("file.save"));
        oos.fluent = Some(larrygram_ui_spec::FluentId::new("CoauthorMenu"));
        assert!(matches!(
            action_for_item(&oos, &registry, &app),
            Action::Disabled { reason } if reason == OUT_OF_SCOPE
        ));
    }

    #[test]
    fn implemented_and_enabled_commands_emit_intents() {
        let registry = larrygram_commands::builtins::registry().expect("the registry builds");
        let app = AppCtx::new(Settings::default());
        // `view.toggle_grid` is a toggle that is enabled with no
        // document; `file.save` has no document to save.
        let toggle = item("a", Some("view.toggle_grid"));
        let action = action_for_item(&toggle, &registry, &app);
        assert!(matches!(action, Action::Emit(_)), "got {action:?}");
        let save = item("a", Some("file.save"));
        assert!(matches!(
            action_for_item(&save, &registry, &app),
            Action::Disabled { .. }
        ));
    }

    #[test]
    fn arrows_and_spacers_paint_without_panicking() {
        use crate::test_util::with_ui;
        with_ui(|ui| {
            for direction in [Arrow::Left, Arrow::Right, Arrow::Down] {
                let response = ui.add(egui::Button::new(arrow_atom(10.0, direction)));
                assert!(response.rect.width() > 0.0);
            }
            let response = ui.add(egui::Button::new(spacer(4.0)));
            assert!(response.rect.width() > 0.0);
            too_deep(ui, &item("a", None));
        });
    }

    #[test]
    fn dynamic_labels_with_an_empty_undo_stack_fall_back() {
        let app = app_with_document();
        let mut undo = item("qat.undo", Some("edit.undo"));
        undo.dynamic_label = true;
        assert_eq!(display_label(&undo, &app), "Can't Undo");
        let mut redo = item("qat.redo", Some("edit.redo.or.repeat"));
        redo.dynamic_label = true;
        assert_eq!(display_label(&redo, &app), "Can't Redo");
        // Any other command with a document keeps its label.
        let mut other = item("qat.save", Some("file.save"));
        other.dynamic_label = true;
        assert_eq!(display_label(&other, &app), "Label");
        // And a plain item keeps its label.
        let plain = item("qat.save", Some("file.save"));
        assert_eq!(display_label(&plain, &app), "Label");
    }

    #[test]
    fn finish_greys_out_a_disabled_action() {
        use crate::test_util::with_ui;
        with_ui(|ui| {
            let response = ui.add_enabled_ui(false, |ui| ui.button("Grey")).inner;
            let mut out = Vec::new();
            let disabled = Action::Disabled {
                reason: "why".to_owned(),
            };
            finish(response, &disabled, "tip", &mut out);
            assert!(out.is_empty(), "a disabled action never dispatches");
        });
    }

    #[test]
    fn the_error_seam_is_always_ok() {
        assert!(no_error().is_ok());
    }

    #[test]
    fn the_noop_edit_applies_and_reverts() {
        let mut app = AppCtx::new(Settings::default());
        let mut edit = NoopEdit("Move shape");
        assert_eq!(edit.label(), "Move shape");
        assert!(edit.apply(&mut app).is_ok());
        assert!(edit.revert(&mut app).is_ok());
    }
}
