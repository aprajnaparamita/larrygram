//! Commands as remote callers see them (ADR 0077): their info, their
//! argument schemas and the JSON-to-[`Args`] conversion.
//!
//! The registry is the single source (AGENTS.md section 6, P9-01 master
//! note): a command's arguments come from its own declaration in
//! `crates/commands` (task P9-03 adds `Command::args` and
//! `Command::remote`), never from a list kept here.

use larrygram_commands::app::AppCtx;
use larrygram_commands::{Args, CommandId, Registry, UiRequest};
use serde_json::{Map, Value};

use crate::error::ControlError;
use crate::limits::Limits;
use crate::ops::CommandInfo;

/// The info of one command in the current state (task P9-08): label,
/// status, the enabled predicate's answer, the remote access and, when
/// `include_args`, the arguments.
pub fn command_info(
    registry: &Registry,
    app: &AppCtx,
    id: &CommandId,
    include_args: bool,
) -> Result<CommandInfo, ControlError> {
    let _ = (registry, app, id, include_args);
    Err(ControlError::not_implemented("args::command_info"))
}

/// Converts JSON arguments into [`Args`] by the command's schema (task
/// P9-08).
///
/// A JSON string becomes `Text`, `Path` or a `choice` (checked against
/// its values) by the declared kind; an integer `Integer`; a number
/// `Number` (range-checked); a boolean `Flag`; an array of strings
/// `List`; a base64 string `Bytes`. An unknown name, a wrong type, a
/// missing required argument or a value over the [`Limits`] caps is
/// `InvalidParams` (or `LimitExceeded`), never a silent drop.
pub fn args_from_json(
    registry: &Registry,
    id: &CommandId,
    json: &Map<String, Value>,
    limits: &Limits,
) -> Result<Args, ControlError> {
    let _ = (registry, id, json, limits);
    Err(ControlError::not_implemented("args::args_from_json"))
}

/// The wire name of a UI request a command made (ADR 0077): recorded in
/// the result, never acted on for a remote caller.
pub fn ui_request_name(request: &UiRequest) -> &'static str {
    match request {
        UiRequest::ShowAbout => "about",
        UiRequest::ShowWelcome => "welcome",
        UiRequest::ShowOptions => "options",
        UiRequest::ConfirmClose { .. } => "confirm_close",
        UiRequest::Quit => "quit",
        // A request added to `UiRequest` later still has a name.
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use larrygram_commands::app::Settings;

    #[test]
    fn the_stubs_report_not_implemented() {
        let registry = Registry::new();
        let app = AppCtx::new(Settings::default());
        let id = CommandId::parse("page.add").expect("id");
        let error = command_info(&registry, &app, &id, true).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        let error =
            args_from_json(&registry, &id, &Map::new(), &Limits::default()).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }

    #[test]
    fn every_ui_request_has_a_wire_name() {
        assert_eq!(ui_request_name(&UiRequest::ShowAbout), "about");
        assert_eq!(ui_request_name(&UiRequest::ShowWelcome), "welcome");
        assert_eq!(ui_request_name(&UiRequest::ShowOptions), "options");
        assert_eq!(
            ui_request_name(&UiRequest::ConfirmClose {
                name: "Drawing1".into()
            }),
            "confirm_close"
        );
        assert_eq!(ui_request_name(&UiRequest::Quit), "quit");
    }
}
