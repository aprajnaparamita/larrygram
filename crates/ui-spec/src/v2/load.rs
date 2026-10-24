//! Loading and cross-checking the layout format v2 documents (task
//! P7-03, ADR 0061).
//!
//! One staged load, the same shape as the v1 [`Loader`](crate::Loader)
//! (ADR 0007 "Validation"):
//!
//! 1. parse the document ([`LoadError::Parse`]),
//! 2. check the nesting depth (a bounded tree can never overflow the
//!    stack downstream),
//! 3. validate a layout file against `ui-spec/layout-v2.schema.json`
//!    ([`LoadError::Schema`]),
//! 4. decode into the typed model ([`LoadError::Parse`]),
//! 5. cross-check the references between the documents
//!    ([`LoadError::CrossReference`], [`LoadError::Duplicate`]).
//!
//! The cross-checks over one layout (ADR 0061, ADR 0065):
//!
//! - every `command` resolves in `commands.yaml`, unless the
//!   placement marks it as a Phase 7 proposal (`new_command`),
//! - every `icon` key has a row in `icons.yaml`,
//! - every `shown_in` names a context of `contexts.yaml` (the window
//!   modes of ADR 0064 are always known),
//! - placement ids are unique within the layout (items and File view
//!   nav entries),
//! - every `contexts.yaml` command row resolves in `commands.yaml`
//!   ([`cross_check_contexts`]),
//! - every icon row names an SVG on disk ([`check_icon_files`], the
//!   disk loader only: the embedded loader has no filesystem).
//!
//! The v1-to-v2 adapter output ([`from_v1`]) is not checked here: its
//! commands come from a v1 layout the v1 loader already cross-checked,
//! and its icon keys name the command id, which the icon map may or
//! may not carry (a key without a row draws the placeholder, ADR
//! 0062). The curated `ui-spec/layouts/*.yaml` files are the checked
//! documents.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::embedded;
use crate::error::LoadError;
use crate::v2::{Contexts, IconMap, Item, LayoutKind, LayoutV2, Theme, V2Error, from_v1};
use crate::{Commands, Layout};

/// The layout schema, compiled into the binary next to the data
/// files. Kept in sync with `ui-spec/layout-v2.schema.json` by the
/// tests (ADR 0061).
const V2_SCHEMA_JSON: &str = include_str!("../../../../ui-spec/layout-v2.schema.json");

/// Display path used in errors for the embedded v2 schema.
const V2_SCHEMA_PATH: &str = "ui-spec/layout-v2.schema.json";

/// The deepest document nesting the decoder accepts. The item tree of
/// a real layout nests three or four levels; this bound keeps a
/// hostile document from overflowing any stack downstream (AGENTS.md
/// section 6).
const MAX_VALUE_DEPTH: usize = 64;

/// Compiles a JSON Schema document. Split out so the failure paths
/// are testable.
fn compile_schema_text(text: &str, path: &str) -> Result<jsonschema::Validator, LoadError> {
    let schema: serde_json::Value = serde_json::from_str(text).map_err(|e| LoadError::Schema {
        path: PathBuf::from(path),
        message: format!("the embedded schema is not valid JSON: {e}"),
    })?;
    jsonschema::validator_for(&schema).map_err(|e| LoadError::Schema {
        path: PathBuf::from(path),
        message: format!("the embedded schema is not a valid JSON Schema: {e}"),
    })
}

/// Compiles `ui-spec/layout-v2.schema.json`. Split out so the failure
/// paths are testable.
pub(crate) fn compile_v2_schema() -> Result<jsonschema::Validator, LoadError> {
    compile_schema_text(V2_SCHEMA_JSON, V2_SCHEMA_PATH)
}

/// Validates one raw document against `validator`.
pub(crate) fn validate_value(
    validator: &jsonschema::Validator,
    value: &serde_json::Value,
    path: &Path,
) -> Result<(), LoadError> {
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LoadError::Schema {
            path: path.to_path_buf(),
            message: errors.join("; "),
        })
    }
}

/// Parses document text and rejects a nesting deeper than
/// [`MAX_VALUE_DEPTH`].
fn bounded_value(text: &str, path: &Path) -> Result<serde_json::Value, LoadError> {
    let value = crate::loader::parse_document(text, path)?;
    if value_depth(&value) > MAX_VALUE_DEPTH {
        return Err(LoadError::Parse {
            path: path.to_path_buf(),
            message: format!("document nests deeper than {MAX_VALUE_DEPTH} levels"),
        });
    }
    Ok(value)
}

/// The nesting depth of a parsed document (iterative, so measuring it
/// can never overflow the stack itself).
fn value_depth(value: &serde_json::Value) -> usize {
    let mut deepest = 0;
    let mut stack: Vec<(usize, &serde_json::Value)> = vec![(1, value)];
    while let Some((depth, node)) = stack.pop() {
        deepest = deepest.max(depth);
        match node {
            serde_json::Value::Array(items) => {
                stack.extend(items.iter().map(|item| (depth + 1, item)));
            }
            serde_json::Value::Object(map) => {
                stack.extend(map.values().map(|item| (depth + 1, item)));
            }
            _ => {}
        }
    }
    deepest
}

/// Parses and decodes one v2 document that has no schema of its own
/// (themes, the icon map, the contexts table): its typed model is the
/// shape check.
pub(crate) fn decode_v2<T: DeserializeOwned>(text: &str, path: &Path) -> Result<T, LoadError> {
    let value = bounded_value(text, path)?;
    serde_json::from_value(value).map_err(|e| LoadError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Parses, validates and decodes one `ui-spec/layouts/*.yaml` file.
pub(crate) fn decode_layout(
    text: &str,
    path: &Path,
    validator: &jsonschema::Validator,
) -> Result<LayoutV2, LoadError> {
    let value = bounded_value(text, path)?;
    validate_value(validator, &value, path)?;
    serde_json::from_value(value).map_err(|e| LoadError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// The command ids of `commands.yaml`, the set every `command` field
/// must resolve in.
pub(crate) fn command_ids(commands: &Commands) -> BTreeSet<&str> {
    commands.entries.iter().map(|e| e.id.as_str()).collect()
}

/// A placement that carries a `command`: an item, a group launcher or
/// a File view nav entry. `new_command` marks a Phase 7 proposal
/// (ADRs 0061, 0062), which the check accepts without a row in
/// `commands.yaml`.
fn check_command(
    command: &str,
    new_command: bool,
    referenced_by: &str,
    known: &BTreeSet<&str>,
) -> Result<(), LoadError> {
    if known.contains(command) || new_command {
        Ok(())
    } else {
        Err(LoadError::CrossReference {
            kind: "item_command",
            id: command.to_owned(),
            referenced_by: referenced_by.to_owned(),
        })
    }
}

/// The cross-check state over one layout: what is known, and which
/// placement ids have been seen.
struct Checker<'a> {
    /// The layout file, for error paths.
    path: &'a Path,
    /// The command ids of `commands.yaml`.
    known_commands: &'a BTreeSet<&'a str>,
    /// The icon map.
    icons: &'a IconMap,
    /// The placement ids seen so far.
    seen: BTreeSet<&'a str>,
}

impl<'a> Checker<'a> {
    /// Checks one item: unique id, known command, known icon key.
    fn item(&mut self, item: &'a Item) -> Result<(), LoadError> {
        if !self.seen.insert(item.id.as_str()) {
            return Err(LoadError::Duplicate {
                path: self.path.to_path_buf(),
                kind: "item_id",
                id: item.id.clone(),
            });
        }
        if let Some(command) = &item.command {
            check_command(
                command.as_str(),
                item.new_command,
                &item.id,
                self.known_commands,
            )?;
        }
        if let Some(key) = &item.icon
            && !self.icons.icons.contains_key(key)
        {
            return Err(LoadError::CrossReference {
                kind: "icon_key",
                id: key.clone(),
                referenced_by: item.id.clone(),
            });
        }
        Ok(())
    }

    /// Checks one surface's item tree iteratively (preorder), so a
    /// deep tree can never overflow the stack.
    fn items(&mut self, items: &'a [Item]) -> Result<(), LoadError> {
        let mut stack: Vec<&'a Item> = items.iter().rev().collect();
        while let Some(item) = stack.pop() {
            self.item(item)?;
            stack.extend(item.items.iter().rev());
        }
        Ok(())
    }

    /// Checks one File view nav entry: unique id, known command.
    fn nav_entry(&mut self, entry: &'a crate::v2::FileViewEntry) -> Result<(), LoadError> {
        if !self.seen.insert(entry.id.as_str()) {
            return Err(LoadError::Duplicate {
                path: self.path.to_path_buf(),
                kind: "item_id",
                id: entry.id.clone(),
            });
        }
        if let Some(command) = &entry.command {
            check_command(command.as_str(), false, &entry.id, self.known_commands)?;
        }
        Ok(())
    }

    /// Checks a container's `shown_in`: a known context.
    fn shown_in(
        &self,
        shown_in: Option<crate::v2::ContextId>,
        referenced_by: &str,
        known_contexts: &BTreeSet<crate::v2::ContextId>,
    ) -> Result<(), LoadError> {
        if let Some(context) = shown_in
            && !known_contexts.contains(&context)
        {
            return Err(LoadError::CrossReference {
                kind: "shown_in",
                id: context.as_str().to_owned(),
                referenced_by: referenced_by.to_owned(),
            });
        }
        Ok(())
    }

    /// Checks a group launcher: a known command.
    fn launcher(&self, group: &crate::v2::Group) -> Result<(), LoadError> {
        if let Some(launcher) = &group.launcher {
            check_command(
                launcher.command.as_str(),
                launcher.new_command,
                &format!("{}.launcher", group.id),
                self.known_commands,
            )?;
        }
        Ok(())
    }
}

/// Cross-checks one v2 layout against `commands.yaml`, the icon map
/// and the contexts table. Returns the first violation in document
/// order.
pub(crate) fn cross_check_layout(
    layout: &LayoutV2,
    path: &Path,
    known_commands: &BTreeSet<&str>,
    icons: &IconMap,
    contexts: &Contexts,
) -> Result<(), LoadError> {
    let mut checker = Checker {
        path,
        known_commands,
        icons,
        seen: BTreeSet::new(),
    };
    let known_contexts = known_contexts(contexts);
    checker.items(&layout.quick_access_toolbar)?;
    for tab in layout.tabs.iter().chain(layout.contextual_tabs.iter()) {
        checker.shown_in(tab.shown_in, &tab.id, &known_contexts)?;
        for group in &tab.groups {
            checker.launcher(group)?;
            checker.items(&group.controls)?;
        }
    }
    for bar in &layout.menu_bars {
        checker.shown_in(bar.shown_in, &bar.id, &known_contexts)?;
        for menu in &bar.menus {
            checker.items(&menu.items)?;
        }
    }
    for toolbar in &layout.toolbars {
        checker.shown_in(toolbar.shown_in, &toolbar.id, &known_contexts)?;
        checker.items(&toolbar.items)?;
    }
    if let Some(view) = &layout.file_view {
        for entry in &view.nav {
            checker.nav_entry(entry)?;
        }
    }
    for menu in &layout.context_menus {
        checker.items(&menu.items)?;
    }
    Ok(())
}

/// The contexts `shown_in` may name: everything `contexts.yaml`
/// defines, plus the window modes of ADR 0064.
fn known_contexts(contexts: &Contexts) -> BTreeSet<crate::v2::ContextId> {
    let mut known: BTreeSet<crate::v2::ContextId> =
        contexts.contexts.iter().map(|def| def.id).collect();
    known.insert(crate::v2::ContextId::StencilEdit);
    known
}

/// Cross-checks `contexts.yaml`: every command row must name a
/// command of `commands.yaml`.
pub(crate) fn cross_check_contexts(
    contexts: &Contexts,
    known_commands: &BTreeSet<&str>,
) -> Result<(), LoadError> {
    for id in contexts.commands.keys() {
        if !known_commands.contains(id.as_str()) {
            return Err(LoadError::CrossReference {
                kind: "context_command",
                id: id.clone(),
                referenced_by: "contexts.yaml".to_owned(),
            });
        }
    }
    Ok(())
}

/// Cross-checks the icon map against the filesystem: every row names
/// `<root>/<set.path>/<icon>.svg`. The disk loader runs this; the
/// embedded loader has no filesystem.
pub(crate) fn check_icon_files(icons: &IconMap, root: &Path) -> Result<(), LoadError> {
    for (key, entry) in &icons.icons {
        let file = root
            .join(&icons.set.path)
            .join(format!("{}.svg", entry.icon));
        if !file.is_file() {
            return Err(LoadError::CrossReference {
                kind: "icon_file",
                id: entry.icon.clone(),
                referenced_by: key.clone(),
            });
        }
    }
    Ok(())
}

/// Loads one committed v2 layout (ADR 0061).
///
/// Panel Modern and Panel Classic come from the embedded copies of
/// `ui-spec/layouts/*.yaml` (schema validation and cross-checks
/// included); Panel (2016 data) and Panel (365 data) are the embedded
/// v1 Fluent data converted by [`from_v1`]. Use
/// [`Loader::load`](crate::Loader::load) to read the same documents
/// from disk.
pub fn load_layout(kind: LayoutKind) -> Result<LayoutV2, V2Error> {
    let layout_file = |name: &str| PathBuf::from("ui-spec/layouts").join(name);
    let p = |name: &str| PathBuf::from("ui-spec").join(name);
    let commands: Commands = decode_v2(embedded::COMMANDS_YAML, &p("commands.yaml"))?;
    let known = command_ids(&commands);
    let icons: IconMap = decode_v2(embedded::ICONS_YAML, &p("icons.yaml"))?;
    let contexts: Contexts = decode_v2(embedded::CONTEXTS_YAML, &p("contexts.yaml"))?;
    cross_check_contexts(&contexts, &known)?;
    let curated = |text: &'static str, name: &str| -> Result<LayoutV2, V2Error> {
        let file = layout_file(name);
        let validator = compile_v2_schema()?;
        let layout = decode_layout(text, &file, &validator)?;
        cross_check_layout(&layout, &file, &known, &icons, &contexts)?;
        Ok(layout)
    };
    match kind {
        LayoutKind::PanelModern => curated(embedded::LAYOUT_PANEL_MODERN_YAML, "panel-modern.yaml"),
        LayoutKind::PanelClassic => {
            curated(embedded::LAYOUT_PANEL_CLASSIC_YAML, "panel-classic.yaml")
        }
        LayoutKind::Panel2016 => {
            let v1: Layout = decode_v2(embedded::PANEL_2016_YAML, &p("panel-2016.yaml"))?;
            from_v1(&v1)
        }
        LayoutKind::Panel365 => {
            let v1: Layout = decode_v2(embedded::PANEL_365_YAML, &p("panel-365.yaml"))?;
            from_v1(&v1)
        }
    }
}

/// The embedded documents, assembled into the shared parts of a
/// [`LayoutSet`](crate::LayoutSet): the icon map, the contexts and
/// both themes. Used by [`Loader::from_embedded`](crate::Loader::from_embedded).
pub(crate) fn embedded_shared() -> Result<Shared, LoadError> {
    let p = |name: &str| PathBuf::from("ui-spec").join(name);
    let icons: IconMap = decode_v2(embedded::ICONS_YAML, &p("icons.yaml"))?;
    let contexts: Contexts = decode_v2(embedded::CONTEXTS_YAML, &p("contexts.yaml"))?;
    let tm = embedded::THEME_PANEL_MODERN_YAML;
    let tc = embedded::THEME_PANEL_CLASSIC_YAML;
    let theme_modern: Theme = decode_v2(tm, &p("themes/panel-modern.yaml"))?;
    let theme_classic: Theme = decode_v2(tc, &p("themes/panel-classic.yaml"))?;
    Ok(Shared {
        icons,
        contexts,
        theme_modern,
        theme_classic,
    })
}

/// The documents shared by every layout: the icon map, the contexts
/// and the two themes.
pub(crate) struct Shared {
    /// `ui-spec/icons.yaml`.
    pub(crate) icons: IconMap,
    /// `ui-spec/contexts.yaml`.
    pub(crate) contexts: Contexts,
    /// `ui-spec/themes/panel-modern.yaml` (also the theme of the two
    /// adapted layouts, ADR 0063).
    pub(crate) theme_modern: Theme,
    /// `ui-spec/themes/panel-classic.yaml`.
    pub(crate) theme_classic: Theme,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::v2::{ContextDef, ContextId, ItemKind};

    fn validator() -> jsonschema::Validator {
        compile_v2_schema().expect("the embedded schema compiles")
    }

    fn item(id: &str) -> Item {
        serde_json::from_value(serde_json::json!({"id": id, "kind": "button"}))
            .expect("an item decodes")
    }

    fn layout_with_items(items: Vec<Item>) -> LayoutV2 {
        serde_json::from_value(serde_json::json!({
            "schema_version": 2,
            "id": "panel-modern",
            "name": "Panel Modern",
            "kind": "panel-modern",
            "theme": "panel-modern",
            "window": {"regions": []},
            "quick_access_toolbar": items,
            "file_button": {"label": "File", "opens": "file_view"},
            "tabs": [],
            "contextual_tabs": [],
            "file_view": {"back_button": true, "default_page": "file_view", "nav": []},
            "context_menus": []
        }))
        .expect("a layout decodes")
    }

    fn icons_with(keys: &[&str]) -> IconMap {
        let mut icons: IconMap = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "set": {"name": "Tabler Icons", "version": "3.49.0", "license": "MIT",
                    "style": "outline", "source": "s", "path": "icons"},
            "fallback": "placeholder",
            "icons": {}
        }))
        .expect("an icon map decodes");
        for key in keys {
            icons.icons.insert(
                (*key).to_owned(),
                crate::v2::IconEntry {
                    icon: "save".to_owned(),
                    tint: None,
                },
            );
        }
        icons
    }

    fn contexts_with(ids: &[ContextId]) -> Contexts {
        Contexts {
            schema_version: 1,
            contexts: ids
                .iter()
                .map(|id| ContextDef {
                    id: *id,
                    description: "test".to_owned(),
                    status: None,
                })
                .collect(),
            notes: Vec::new(),
            commands: BTreeMap::new(),
        }
    }

    fn empty() -> BTreeSet<&'static str> {
        BTreeSet::new()
    }

    #[test]
    fn compile_schema_text_reports_why_it_fails() {
        let broken = compile_schema_text("{not json", "ui-spec/layout-v2.schema.json");
        assert!(matches!(
            broken,
            Err(LoadError::Schema { ref message, .. }) if message.contains("not valid JSON")
        ));
        let not_a_schema = compile_schema_text(r#"{"type": 42}"#, "ui-spec/layout-v2.schema.json");
        assert!(matches!(
            not_a_schema,
            Err(LoadError::Schema { ref message, .. }) if message.contains("not a valid JSON Schema")
        ));
        assert!(compile_schema_text("{}", "ui-spec/layout-v2.schema.json").is_ok());
    }

    #[test]
    fn value_depth_measures_nesting_without_recursion() {
        assert_eq!(value_depth(&serde_json::json!(1)), 1);
        assert_eq!(value_depth(&serde_json::json!([1, [2]])), 3);
        assert_eq!(value_depth(&serde_json::json!({"a": {"b": []}})), 3);
        // Deep nesting is measured iteratively: no stack growth.
        let mut deep = serde_json::json!(0);
        for _ in 0..10_000 {
            deep = serde_json::Value::Array(vec![deep]);
        }
        assert_eq!(value_depth(&deep), 10_001);
    }

    #[test]
    fn bounded_value_rejects_deep_nesting() {
        let mut deep = serde_json::json!(0);
        for _ in 0..MAX_VALUE_DEPTH {
            deep = serde_json::Value::Array(vec![deep]);
        }
        let text = deep.to_string();
        let error = decode_v2::<serde_json::Value>(&text, Path::new("deep.json"))
            .expect_err("past the depth bound");
        assert!(
            matches!(&error, LoadError::Parse { message, .. } if message.contains("nests deeper")),
            "got {error}"
        );
        // Just under the bound decodes.
        let mut shallow = serde_json::json!(0);
        for _ in 0..MAX_VALUE_DEPTH - 2 {
            shallow = serde_json::Value::Array(vec![shallow]);
        }
        let shallow = shallow.to_string();
        assert!(decode_v2::<serde_json::Value>(&shallow, Path::new("shallow.json")).is_ok());
    }

    #[test]
    fn decode_v2_rejects_broken_yaml_and_the_wrong_shape() {
        let broken =
            decode_v2::<Theme>("a: [1\n", Path::new("themes/x.yaml")).expect_err("broken yaml");
        assert!(matches!(broken, LoadError::Parse { .. }));
        // A theme without the required fields decodes as YAML but not
        // as a Theme.
        let wrong = decode_v2::<Theme>("schema_version: 1\n", Path::new("t.yaml"))
            .expect_err("the wrong shape");
        assert!(matches!(wrong, LoadError::Parse { .. }));
    }

    #[test]
    fn decode_layout_validates_against_the_v2_schema() {
        let good = r#"schema_version: 2
id: panel-modern
name: Panel Modern
kind: panel-modern
theme: panel-modern
window: {regions: []}
quick_access_toolbar: []
file_button: {label: File, opens: file_view}
tabs: []
contextual_tabs: []
file_view: {back_button: true, default_page: file_view, nav: []}
context_menus: []
"#;
        let layout = decode_layout(good, Path::new("layouts/x.yaml"), &validator())
            .expect("the minimal layout decodes");
        assert_eq!(layout.id, "panel-modern");

        let error = decode_layout(
            "schema_version: 2\n",
            Path::new("layouts/x.yaml"),
            &validator(),
        )
        .expect_err("a missing name fails the schema");
        assert!(matches!(
            error,
            LoadError::Schema { ref message, .. } if !message.is_empty()
        ));
    }

    #[test]
    fn decode_layout_rejects_values_the_types_cannot_represent() {
        // Schema-valid (`column` is any integer >= 0) but wider than
        // the typed model's `u32`: decoding must fail, not wrap.
        let text = r#"schema_version: 2
id: panel-modern
name: Panel Modern
kind: panel-modern
theme: panel-modern
window: {regions: []}
quick_access_toolbar:
- {id: qat.save, kind: button, label: Save, command: file.save, column: 4294967296}
file_button: {label: File, opens: file_view}
tabs: []
contextual_tabs: []
file_view: {back_button: true, default_page: file_view, nav: []}
context_menus: []
"#;
        let error = decode_layout(text, Path::new("layouts/x.yaml"), &validator())
            .expect_err("wider than u32");
        assert!(matches!(error, LoadError::Parse { .. }));
    }

    #[test]
    fn cross_check_accepts_a_consistent_layout() {
        let mut item = item("qat.save");
        item.command = Some(crate::CommandId::new("file.save"));
        item.icon = Some("file.save".to_owned());
        let layout = layout_with_items(vec![item]);
        let icons = icons_with(&["file.save"]);
        let contexts = contexts_with(&[ContextId::EmptyPage]);
        let known: BTreeSet<&str> = ["file.save"].into();
        assert!(
            cross_check_layout(&layout, Path::new("l.yaml"), &known, &icons, &contexts).is_ok()
        );
    }

    #[test]
    fn cross_check_rejects_an_unknown_command() {
        let mut item = item("qat.save");
        item.command = Some(crate::CommandId::new("file.save"));
        let layout = layout_with_items(vec![item]);
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("no row for file.save");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "item_command", id, referenced_by }
                if id == "file.save" && referenced_by == "qat.save"
        ));
    }

    #[test]
    fn cross_check_accepts_a_new_command_proposal() {
        let mut item = item("qat.save");
        item.command = Some(crate::CommandId::new("file.save"));
        item.new_command = true;
        let layout = layout_with_items(vec![item]);
        assert!(
            cross_check_layout(
                &layout,
                Path::new("l.yaml"),
                &empty(),
                &icons_with(&[]),
                &contexts_with(&[])
            )
            .is_ok()
        );
    }

    #[test]
    fn cross_check_rejects_an_unknown_icon_key() {
        let mut item = item("qat.save");
        item.icon = Some("nope".to_owned());
        let layout = layout_with_items(vec![item]);
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("no row for the key");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "icon_key", id, referenced_by }
                if id == "nope" && referenced_by == "qat.save"
        ));
    }

    #[test]
    fn cross_check_rejects_duplicate_item_ids() {
        let layout = layout_with_items(vec![item("qat.save"), item("qat.save")]);
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("one id twice");
        assert!(matches!(
            &error,
            LoadError::Duplicate { kind: "item_id", id, .. } if id == "qat.save"
        ));

        // The same id across two surfaces is a duplicate too.
        let mut layout = layout_with_items(vec![item("qat.save")]);
        layout.context_menus = vec![
            serde_json::from_value(serde_json::json!({
                "id": "context.shape",
                "items": [{"id": "qat.save", "kind": "button"}]
            }))
            .expect("a context menu decodes"),
        ];
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("one id twice, on two surfaces");
        assert!(matches!(error, LoadError::Duplicate { .. }));
    }

    #[test]
    fn cross_check_rejects_an_unknown_shown_in_context() {
        let mut layout = layout_with_items(vec![]);
        layout.contextual_tabs = vec![
            serde_json::from_value(serde_json::json!({
                "id": "tab.picture_tools.format",
                "tab_set": "Picture Tools",
                "shown_in": "picture_selected",
                "groups": []
            }))
            .expect("a tab decodes"),
        ];
        let contexts = contexts_with(&[ContextId::EmptyPage]);
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts,
        )
        .expect_err("picture_selected is not defined");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "shown_in", id, referenced_by }
                if id == "picture_selected" && referenced_by == "tab.picture_tools.format"
        ));
        // The window mode of ADR 0064 is known without a row.
        let mut stencil = layout.clone();
        stencil.contextual_tabs[0].shown_in = Some(ContextId::StencilEdit);
        assert!(
            cross_check_layout(
                &stencil,
                Path::new("l.yaml"),
                &empty(),
                &icons_with(&[]),
                &contexts
            )
            .is_ok()
        );
    }

    #[test]
    fn cross_check_rejects_an_unknown_launcher_command() {
        let mut layout = layout_with_items(vec![]);
        layout.tabs = vec![
            serde_json::from_value(serde_json::json!({
                "id": "tab.home",
                "label": "Home",
                "groups": [{
                    "id": "tab.home.font",
                    "label": "Font",
                    "layout": "stack",
                    "controls": [],
                    "launcher": {"command": "nope.launcher"}
                }]
            }))
            .expect("a tab decodes"),
        ];
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("the launcher command is unknown");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "item_command", id, referenced_by }
                if id == "nope.launcher" && referenced_by == "tab.home.font.launcher"
        ));
        // A launcher marked as a proposal passes.
        layout.tabs[0].groups[0]
            .launcher
            .as_mut()
            .expect("launcher")
            .new_command = true;
        assert!(
            cross_check_layout(
                &layout,
                Path::new("l.yaml"),
                &empty(),
                &icons_with(&[]),
                &contexts_with(&[])
            )
            .is_ok()
        );
    }

    #[test]
    fn cross_check_checks_the_file_view_nav() {
        let mut layout = layout_with_items(vec![]);
        layout.file_view = Some(
            serde_json::from_value(serde_json::json!({
                "back_button": true,
                "default_page": "file_view.save",
                "nav": [{"id": "file_view.save", "kind": "command", "label": "Save",
                         "command": "nope.nav"}]
            }))
            .expect("a file view decodes"),
        );
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("the nav command is unknown");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "item_command", id, .. } if id == "nope.nav"
        ));

        // A nav id shares the placement id space.
        let mut layout = layout_with_items(vec![item("file_view.save")]);
        layout.file_view = Some(
            serde_json::from_value(serde_json::json!({
                "back_button": true,
                "default_page": "file_view.save",
                "nav": [{"id": "file_view.save", "kind": "command", "label": "Save"}]
            }))
            .expect("a file view decodes"),
        );
        let error = cross_check_layout(
            &layout,
            Path::new("l.yaml"),
            &empty(),
            &icons_with(&[]),
            &contexts_with(&[]),
        )
        .expect_err("one id twice");
        assert!(matches!(error, LoadError::Duplicate { .. }));
    }

    #[test]
    fn cross_check_contexts_rejects_an_unknown_context_command() {
        let mut contexts = contexts_with(&[]);
        contexts.commands.insert(
            "edit.cut".to_owned(),
            crate::v2::CommandAvailability {
                enabled_in: vec![],
                on_by_default: None,
            },
        );
        let error = cross_check_contexts(&contexts, &empty()).expect_err("edit.cut is unknown");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "context_command", id, referenced_by }
                if id == "edit.cut" && referenced_by == "contexts.yaml"
        ));
        let known: BTreeSet<&str> = ["edit.cut"].into();
        assert!(cross_check_contexts(&contexts, &known).is_ok());
    }

    #[test]
    fn check_icon_files_reports_the_missing_svg() {
        let dir =
            std::env::temp_dir().join(format!("larrygram-ui-spec-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("icons")).expect("temp icons dir");
        let icons = icons_with(&["file.save"]);
        // The named SVG is missing.
        let error = check_icon_files(&icons, &dir).expect_err("the svg is missing");
        assert!(matches!(
            &error,
            LoadError::CrossReference { kind: "icon_file", id, referenced_by }
                if id == "save" && referenced_by == "file.save"
        ));
        std::fs::write(dir.join("icons/save.svg"), "<svg/>").expect("svg");
        assert!(check_icon_files(&icons, &dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_layout_loads_all_four_kinds() {
        for kind in [
            LayoutKind::PanelModern,
            LayoutKind::PanelClassic,
            LayoutKind::Panel2016,
            LayoutKind::Panel365,
        ] {
            let layout = load_layout(kind).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert!(!layout.window.regions.is_empty(), "{kind:?}");
        }
        // The curated kinds decode their own files; the adapted ones
        // are panel-modern by construction (ADR 0061).
        let modern = load_layout(LayoutKind::PanelModern).expect("loads");
        assert_eq!(modern.id, "panel-modern");
        assert_eq!(modern.kind, crate::v2::PanelStyle::PanelModern);
        assert_eq!(modern.tabs[0].id, "tab.home");
        let classic = load_layout(LayoutKind::PanelClassic).expect("loads");
        assert_eq!(classic.kind, crate::v2::PanelStyle::PanelClassic);
        let adapted = load_layout(LayoutKind::Panel2016).expect("loads");
        assert_eq!(adapted.id, "panel-2016");
        assert_eq!(adapted.kind, crate::v2::PanelStyle::PanelModern);
    }

    #[test]
    fn embedded_shared_holds_both_themes_and_the_maps() {
        let shared = embedded_shared().expect("the embedded documents decode");
        assert_eq!(shared.theme_modern.id, "panel-modern");
        assert_eq!(shared.theme_classic.id, "panel-classic");
        assert!(!shared.icons.icons.is_empty());
        assert!(!shared.contexts.contexts.is_empty());
    }

    #[test]
    fn command_ids_collects_the_table() {
        let mapping = |id: &str| crate::CommandMapping {
            id: crate::CommandId::new(id),
            label: "Label".to_owned(),
            visuicmd: None,
            fluent_ids: vec![],
            mapping_confidence: crate::MappingConfidence::Unknown,
        };
        let commands = Commands {
            entries: vec![mapping("file.save"), mapping("edit.cut")],
        };
        assert_eq!(
            command_ids(&commands),
            BTreeSet::from(["file.save", "edit.cut"])
        );
        assert!(command_ids(&Commands::default()).is_empty());
    }

    #[test]
    fn item_kinds_decode_in_the_tests() {
        // Sanity for the helpers above (coverage of the test inputs).
        assert_eq!(item("qat.save").kind, ItemKind::Button);
    }
}
