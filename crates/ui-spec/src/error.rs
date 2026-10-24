//! Errors produced while loading `ui-spec/` data.
//!
//! Every failure of [`Loader::new`](crate::Loader::new) and
//! [`Loader::load`](crate::Loader::load) is one of the [`LoadError`]
//! variants. [`IdError`] is the failure of the id parsers
//! ([`CommandId::parse`](crate::CommandId::parse),
//! [`FluentId::parse`](crate::FluentId::parse)). There is no
//! catch-all panic path: malformed input is an error, never a crash
//! (AGENTS.md section 6).

use std::path::PathBuf;

/// Everything that can go wrong while loading a [`LayoutSet`](crate::LayoutSet).
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// A data file could not be read (missing file, permission denied,
    /// ...).
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The embedded `ui-spec/schema.json` is broken, or a document
    /// failed validation against it.
    #[error("schema error for {}: {message}", path.display())]
    Schema {
        /// `ui-spec/schema.json` when the schema itself failed to
        /// compile, otherwise the data file that failed validation.
        path: PathBuf,
        /// The validator's error list, joined by `; `. Never empty for
        /// a validation failure.
        message: String,
    },

    /// A document could not be parsed into the typed model: broken
    /// YAML/JSON, or data that does not fit the public types.
    #[error("cannot parse {}: {message}", path.display())]
    Parse {
        /// The file that failed to parse.
        path: PathBuf,
        /// The parser's error message.
        message: String,
    },

    /// A cross-reference between the data documents is broken (ADR
    /// 0007 "Cross-references", ADR 0061, task P7-03).
    #[error("{}", cross_reference_message(kind, id, referenced_by))]
    CrossReference {
        /// What was violated:
        ///
        /// - `"fluent_id"`: an entry in `commands.yaml` lists a Fluent
        ///   id that appears in no layout (the check runs against the
        ///   union of both layouts, per ADR 0007).
        /// - `"control_command"`: a layout control dispatches a
        ///   `CommandId` that `commands.yaml` does not define.
        /// - `"command_id"`: a shortcut in `shortcuts.yaml` dispatches
        ///   a `CommandId` that `commands.yaml` does not define.
        /// - `"item_command"`: a v2 item (or group launcher, or File
        ///   view nav entry) dispatches a command that
        ///   `commands.yaml` does not define and the placement does
        ///   not mark as a proposal (`new_command`).
        /// - `"icon_key"`: a v2 item's `icon` key has no row in
        ///   `icons.yaml`.
        /// - `"icon_file"`: an `icons.yaml` row names an SVG that is
        ///   not on disk (the disk loader only; the embedded loader
        ///   has no filesystem).
        /// - `"shown_in"`: a v2 `shown_in` names a context that
        ///   `contexts.yaml` does not define (the window modes of ADR
        ///   0064 are always known).
        /// - `"context_command"`: a `contexts.yaml` command row names
        ///   a command that `commands.yaml` does not define.
        kind: &'static str,
        /// The unknown id (Fluent id, command id, icon key or
        /// context id).
        id: String,
        /// The id of the referring entry: the command id for
        /// `"fluent_id"`, the control's Fluent id for
        /// `"control_command"`, the rendered key sequence for
        /// `"command_id"`, the placement id of the item, launcher or
        /// container for the v2 checks, and `"contexts.yaml"` for
        /// `"context_command"`.
        referenced_by: String,
    },

    /// One id is used twice where it must be unique.
    #[error("duplicate {kind}: {id} (in {})", path.display())]
    Duplicate {
        /// The file that uses the id twice.
        path: PathBuf,
        /// Which id space is duplicated:
        ///
        /// - `"command_id"`: a command `id` appears twice in
        ///   `commands.yaml` (the table is one entry per command id,
        ///   ADR 0006).
        /// - `"item_id"`: a v2 placement `id` (an item or a File view
        ///   nav entry) appears twice in one layout (ADR 0061).
        kind: &'static str,
        /// The duplicated id.
        id: String,
    },

    /// The document is well-formed and schema-valid, but it is not the
    /// kind of document expected at this path (e.g. `commands.yaml`
    /// holding a shortcuts list).
    #[error("{}: {message}", path.display())]
    Unknown {
        /// The file that holds the wrong kind of document.
        path: PathBuf,
        /// What was expected and what was found.
        message: String,
    },
}

/// A rejected id string, from [`CommandId::parse`](crate::CommandId::parse)
/// or [`FluentId::parse`](crate::FluentId::parse).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}: {value:?}")]
pub struct IdError {
    /// What is wrong with the id.
    reason: &'static str,
    /// The rejected string.
    value: String,
}

impl IdError {
    /// Builds an error naming `value` and the rule it broke.
    pub(crate) fn new(reason: &'static str, value: &str) -> Self {
        Self {
            reason,
            value: value.to_owned(),
        }
    }

    /// The rejected string.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// What is wrong with the id.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// The `Display` text of a [`LoadError::CrossReference`].
///
/// The `fluent_id` case names the union check of ADR 0007: the id
/// appears in *no* layout, not merely in the layout being loaded.
/// Every other violation is an id no document defines, except the
/// v2 checks of task P7-03, which name what is missing.
fn cross_reference_message(kind: &str, id: &str, referenced_by: &str) -> String {
    let problem = match kind {
        "fluent_id" => "is not in any layout",
        "item_command" => "is not in commands.yaml (and is not marked new_command)",
        "icon_key" => "is not in the icon map",
        "icon_file" => "is not on disk",
        "shown_in" => "is not a known context",
        "context_command" => "is not in commands.yaml",
        _ => "is not known",
    };
    format!("cross-reference ({kind}): {id} (referenced by {referenced_by}) {problem}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fluent_id_violation_says_not_in_any_layout() {
        let err = LoadError::CrossReference {
            kind: "fluent_id",
            id: "GhostControl".to_owned(),
            referenced_by: "file.save".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "cross-reference (fluent_id): GhostControl \
             (referenced by file.save) is not in any layout"
        );
    }

    #[test]
    fn duplicate_names_the_kind_id_and_file() {
        let err = LoadError::Duplicate {
            path: PathBuf::from("ui-spec/commands.yaml"),
            kind: "command_id",
            id: "file.save".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "duplicate command_id: file.save (in ui-spec/commands.yaml)"
        );
    }

    #[test]
    fn unknown_names_the_file_and_the_problem() {
        let err = LoadError::Unknown {
            path: PathBuf::from("commands.yaml"),
            message: "expected a commands document".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "commands.yaml: expected a commands document"
        );
    }

    #[test]
    fn id_error_exposes_value_and_reason() {
        let err = IdError::new("id is empty", "");
        assert_eq!(err.value(), "");
        assert_eq!(err.reason(), "id is empty");
        assert_eq!(err.to_string(), "id is empty: \"\"");
    }

    #[test]
    fn other_violations_say_not_known() {
        for kind in ["control_command", "command_id"] {
            let err = LoadError::CrossReference {
                kind,
                id: "edit.undo".to_owned(),
                referenced_by: "Ctrl+Z".to_owned(),
            };
            assert_eq!(
                err.to_string(),
                format!("cross-reference ({kind}): edit.undo (referenced by Ctrl+Z) is not known")
            );
        }
    }

    #[test]
    fn v2_violations_name_what_is_missing() {
        let cases = [
            (
                "item_command",
                "edit.paste",
                "tab.home.clipboard.paste",
                "is not in commands.yaml (and is not marked new_command)",
            ),
            (
                "icon_key",
                "edit.cut",
                "tab.home.clipboard.cut",
                "is not in the icon map",
            ),
            ("icon_file", "scissors", "edit.cut", "is not on disk"),
            (
                "shown_in",
                "picture_selected",
                "tab.picture_tools.format",
                "is not a known context",
            ),
            (
                "context_command",
                "edit.cut",
                "contexts.yaml",
                "is not in commands.yaml",
            ),
        ];
        for (kind, id, referenced_by, problem) in cases {
            let err = LoadError::CrossReference {
                kind,
                id: id.to_owned(),
                referenced_by: referenced_by.to_owned(),
            };
            assert_eq!(
                err.to_string(),
                format!("cross-reference ({kind}): {id} (referenced by {referenced_by}) {problem}")
            );
        }
    }

    #[test]
    fn duplicate_names_the_item_id_space() {
        let err = LoadError::Duplicate {
            path: PathBuf::from("ui-spec/layouts/panel-modern.yaml"),
            kind: "item_id",
            id: "tab.home.clipboard.paste".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "duplicate item_id: tab.home.clipboard.paste \
             (in ui-spec/layouts/panel-modern.yaml)"
        );
    }
}
