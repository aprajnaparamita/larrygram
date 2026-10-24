//! Validates the committed Panel Modern and Panel Classic layouts.
//!
//! Task P7-01 adds this test with the imported data (ADR 0061):
//!
//! 1. each layout file validates against
//!    `ui-spec/layout-v2.schema.json` with the crate's own
//!    `jsonschema` dependency;
//! 2. each layout decodes into [`LayoutV2`], so the Rust types and the
//!    files agree;
//! 3. every `command` id resolves in `ui-spec/commands.yaml` (task
//!    P7-02 merged the Phase 7 ids into `OUR_COMMANDS` and deleted the
//!    proposal file).
//!
//! Task P7-03 replaces the ad-hoc walk with the loader's own
//! cross-checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use larrygram_ui_spec::v2::LayoutV2;

/// A test failure with a formatted message.
type Error = Box<dyn std::error::Error>;

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Parses a YAML or JSON document into a generic value, the same way
/// the crate's loader does.
fn parse(path: &Path) -> Result<serde_json::Value, Error> {
    let text = std::fs::read_to_string(path)?;
    serde_saphyr::from_str(&text).map_err(|error| format!("{}: {error}", path.display()).into())
}

/// Every `command` id the item tree uses, in file order.
fn command_ids(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "command" {
                    if let Some(id) = child.as_str() {
                        out.push(id.to_owned());
                    }
                } else {
                    command_ids(child, out);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                command_ids(item, out);
            }
        }
        _ => {}
    }
}

/// The ids of the `commands.yaml` table (a list of entries).
fn table_ids(path: &Path) -> Result<BTreeSet<String>, Error> {
    let value = parse(path)?;
    let entries = value.as_array().cloned().unwrap_or_default();
    Ok(entries
        .iter()
        .filter_map(|entry| entry.get("id")?.as_str().map(str::to_owned))
        .collect())
}

#[test]
fn both_layouts_validate_against_the_v2_schema() -> Result<(), Error> {
    let root = workspace_root();
    let schema = parse(&root.join("ui-spec/layout-v2.schema.json"))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| format!("the schema is not valid: {error}"))?;

    for file in ["panel-classic.yaml", "panel-modern.yaml"] {
        let layout = parse(&root.join("ui-spec/layouts").join(file))?;
        let errors: Vec<String> = validator
            .iter_errors(&layout)
            .map(|error| format!("{}: {error}", error.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{file} failed schema validation: {}",
            errors.join("; ")
        );
    }
    Ok(())
}

#[test]
fn both_layouts_decode_into_the_v2_types() -> Result<(), Error> {
    let root = workspace_root();
    for file in ["panel-classic.yaml", "panel-modern.yaml"] {
        let value = parse(&root.join("ui-spec/layouts").join(file))?;
        let layout: LayoutV2 = serde_json::from_value(value).map_err(|error| {
            format!("{file} does not decode into ui_spec::v2::LayoutV2: {error}")
        })?;
        assert_eq!(layout.schema_version, 2);
        assert!(!layout.context_menus.is_empty(), "{file} has context menus");
    }
    Ok(())
}

#[test]
fn every_command_resolves_in_commands_yaml() -> Result<(), Error> {
    let root = workspace_root();
    let known = table_ids(&root.join("ui-spec/commands.yaml"))?;
    assert!(!known.is_empty(), "commands.yaml has ids");

    let mut missing = BTreeSet::new();
    for file in ["panel-classic.yaml", "panel-modern.yaml"] {
        let value = parse(&root.join("ui-spec/layouts").join(file))?;
        let mut ids = Vec::new();
        command_ids(&value, &mut ids);
        assert!(!ids.is_empty(), "{file} links commands");
        for id in ids {
            if !known.contains(&id) {
                missing.insert(format!("{file}: {id}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "command ids missing from commands.yaml: {missing:?}"
    );
    Ok(())
}
