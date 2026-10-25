//! The [`Loader`]: read, validate, decode and cross-check `ui-spec/`
//! data.
//!
//! Loading is deliberately staged (ADR 0007 "Validation"):
//!
//! 1. read the file ([`LoadError::Io`]),
//! 2. parse YAML (or JSON) into a raw document ([`LoadError::Parse`]),
//! 3. validate against `ui-spec/schema.json`
//!    ([`LoadError::Schema`]),
//! 4. check the document is the kind of document this file should
//!    hold ([`LoadError::Unknown`]),
//! 5. decode into the typed model ([`LoadError::Parse`]),
//! 6. reject a `commands.yaml` id used twice ([`LoadError::Duplicate`]),
//! 7. cross-check references between the layouts, `commands.yaml` and
//!    `shortcuts.yaml` ([`LoadError::CrossReference`]).
//!
//! `commands.yaml` and `shortcuts.yaml` are shared by both layouts,
//! so every load reads *both* layout files: the `fluent_id` check
//! runs against the union of the Fluent ids of the two layouts (ADR
//! 0007 "Cross-references" 1). [`Loader::load`] returns one
//! [`LayoutSet`], [`Loader::load_all`] returns both together as
//! [`LayoutSets`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::error::LoadError;
use crate::lookup::all_controls;
use crate::v2::load::{
    Shared, check_icon_files, command_ids, cross_check_contexts, cross_check_layout, decode_layout,
    decode_v2, embedded_shared,
};
use crate::v2::{Contexts, IconMap, LayoutV2, Theme};
use crate::{
    CommandId, CommandMapping, Commands, FluentId, Layout, LayoutKind, LayoutSet, LayoutSets,
    Shortcut, Shortcuts,
};

/// The JSON Schema, compiled into the binary so the loader does not
/// depend on a runtime path lookup. Kept in sync with
/// `ui-spec/schema.json` by the gate (the file is committed next to
/// the data files; ADR 0007).
const SCHEMA_JSON: &str = include_str!("../../../ui-spec/schema.json");

/// Display path used in errors for the embedded schema.
const SCHEMA_PATH: &str = "ui-spec/schema.json";

/// Subdirectory of the workspace root holding the data files.
const DATA_DIR: &str = "ui-spec";

/// The command mappings, shared by both layouts.
const COMMANDS_FILE: &str = "commands.yaml";

/// The keyboard shortcuts, shared by both layouts.
const SHORTCUTS_FILE: &str = "shortcuts.yaml";

/// The Panel Modern theme, shared by the two adapted layouts (ADR
/// 0063).
const THEME_MODERN_FILE: &str = "themes/panel-modern.yaml";

/// The Panel Classic theme.
const THEME_CLASSIC_FILE: &str = "themes/panel-classic.yaml";

/// The icon map (ADR 0062).
const ICONS_FILE: &str = "icons.yaml";

/// The selection contexts (ADR 0064).
const CONTEXTS_FILE: &str = "contexts.yaml";

/// The kind of document a data file holds. The schema's `oneOf`
/// allows any of the three at any path; the loader knows what each
/// file is supposed to contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocKind {
    /// `panel-2016.yaml` / `panel-365.yaml`: a Panel layout.
    Layout,
    /// `commands.yaml`: the command mapping table.
    Commands,
    /// `shortcuts.yaml`: the keyboard shortcut table.
    Shortcuts,
}

impl DocKind {
    /// Short name used in error messages.
    fn noun(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Commands => "commands",
            Self::Shortcuts => "shortcuts",
        }
    }
}

/// Loads the `ui-spec/` data files from a workspace root.
///
/// The schema is compiled once in [`Loader::new`] and shared by every
/// load call, so one `Loader` can serve both layouts:
///
/// ```no_run
/// use larrygram_ui_spec::{FluentId, CommandId, LayoutKind, Loader};
///
/// let loader = Loader::new(".")?;
/// let both = loader.load_all()?;
/// let set_2016 = both.get(LayoutKind::Panel2016);
/// let lookup = set_2016.command_lookup();
/// assert_eq!(
///     lookup.commands.get(&FluentId::new("FileSave")),
///     Some(&CommandId::new("file.save")),
/// );
/// # Ok::<(), larrygram_ui_spec::LoadError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Loader {
    /// Directory containing the `ui-spec/` data files.
    workspace_root: PathBuf,
    /// The compiled v1 schema validator (`Send + Sync`, cheap to
    /// clone).
    validator: jsonschema::Validator,
    /// The compiled layout format v2 schema validator.
    v2_validator: jsonschema::Validator,
}

impl Loader {
    /// Creates a loader for the workspace rooted at `workspace_root`.
    ///
    /// Compiles `ui-spec/schema.json` and
    /// `ui-spec/layout-v2.schema.json` (both embedded in the binary)
    /// once. The only failure is [`LoadError::Schema`] when an
    /// embedded schema itself is broken.
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, LoadError> {
        Ok(Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
            validator: compile_schema(SCHEMA_JSON)?,
            v2_validator: crate::v2::load::compile_v2_schema()?,
        })
    }

    /// Loads the full data set for one layout kind (all four kinds have
    /// one: [`LayoutKind::PanelModern`] and [`LayoutKind::PanelClassic`]
    /// come from the hand-curated v2 files, the other two from the v1
    /// Fluent data, ADR 0061).
    ///
    /// Reads `ui-spec/` for **both** v1 layouts, the shared tables and
    /// the v2 documents (layouts, themes, `icons.yaml`, `contexts.yaml`),
    /// validates each against its schema, decodes them, and cross-checks
    /// the references: the three of ADR 0007 over the v1 documents
    ///
    /// 1. every Fluent id listed in `commands.yaml` appears in at
    ///    least one of the two layouts (`kind: "fluent_id"`; the
    ///    shared table may legitimately name a 365-only or a
    ///    2016-only control),
    /// 2. every `command` on a control of the *requested* layout is
    ///    defined in `commands.yaml` (`kind: "control_command"`),
    /// 3. every `command` in `shortcuts.yaml` is defined in
    ///    `commands.yaml` (`kind: "command_id"`),
    ///
    /// and the v2 cross-checks of ADR 0061 (task P7-03: commands, icon
    /// keys and files, contexts, unique placement ids). Only the
    /// requested layout's [`LayoutSet`] is returned; use
    /// [`Self::load_all`] to get all four at once. The data files may be
    /// YAML or JSON: a missing `.yaml` file is looked up as `.json` with
    /// the same stem.
    ///
    /// ```no_run
    /// use larrygram_ui_spec::{LayoutKind, Loader};
    ///
    /// let loader = Loader::new(".")?;
    /// let modern = loader.load(LayoutKind::PanelModern)?;
    /// assert_eq!(modern.v2.id, "panel-modern");
    /// # Ok::<(), larrygram_ui_spec::LoadError>(())
    /// ```
    pub fn load(&self, kind: LayoutKind) -> Result<LayoutSet, LoadError> {
        let docs = self.read_documents()?;
        let mut checked: Vec<&Layout> = Vec::new();
        match kind {
            LayoutKind::Panel2016 => checked.push(&docs.panel_2016),
            LayoutKind::Panel365 => checked.push(&docs.panel_365),
            LayoutKind::PanelModern | LayoutKind::PanelClassic => {}
        }
        check_references(
            &checked,
            &docs.fluent_union(),
            &docs.commands,
            &docs.shortcuts,
        )?;
        let v2 = self.read_v2_documents(&docs.commands)?;
        let sets = build_sets(docs, v2)?;
        Ok(match kind {
            LayoutKind::PanelModern => sets.panel_modern,
            LayoutKind::PanelClassic => sets.panel_classic,
            LayoutKind::Panel2016 => sets.panel_2016,
            LayoutKind::Panel365 => sets.panel_365,
        })
    }

    /// Loads the data sets for **all four** layouts in one pass.
    ///
    /// Reads every `ui-spec/` file exactly once and runs the ADR 0007
    /// cross-references over the pair of v1 layouts (`fluent_id`
    /// against the union of both layouts' Fluent ids,
    /// `control_command` against each layout's controls,
    /// `command_id` against the shared shortcut table) plus the v2
    /// cross-checks of ADR 0061 over the curated layouts and the
    /// shared documents.
    ///
    /// ```no_run
    /// # use larrygram_ui_spec::{LayoutKind, Loader};
    /// let loader = Loader::new(".")?;
    /// let sets = loader.load_all()?;
    /// let set_365 = sets.get(LayoutKind::Panel365);
    /// # Ok::<(), larrygram_ui_spec::LoadError>(())
    /// ```
    pub fn load_all(&self) -> Result<LayoutSets, LoadError> {
        let docs = self.read_documents()?;
        check_references(
            &[&docs.panel_2016, &docs.panel_365],
            &docs.fluent_union(),
            &docs.commands,
            &docs.shortcuts,
        )?;
        let v2 = self.read_v2_documents(&docs.commands)?;
        build_sets(docs, v2)
    }

    /// Reads and validates all four data documents, in file order:
    /// both layouts, then the shared command and shortcut tables.
    fn read_documents(&self) -> Result<Documents, LoadError> {
        let panel_2016: Layout = self.load_document(
            &self.data_file(LayoutKind::Panel2016.file_name())?,
            DocKind::Layout,
        )?;
        let panel_365: Layout = self.load_document(
            &self.data_file(LayoutKind::Panel365.file_name())?,
            DocKind::Layout,
        )?;
        let commands_path = self.data_file(COMMANDS_FILE)?;
        let entries: Vec<CommandMapping> = self.load_document(&commands_path, DocKind::Commands)?;
        check_duplicate_command_ids(&entries, &commands_path)?;
        let commands = Commands { entries };
        let entries: Vec<Shortcut> =
            self.load_document(&self.data_file(SHORTCUTS_FILE)?, DocKind::Shortcuts)?;
        let shortcuts = Shortcuts { entries };
        Ok(Documents {
            panel_2016,
            panel_365,
            commands,
            shortcuts,
        })
    }

    /// Resolves one data file under `<workspace_root>/ui-spec/`,
    /// falling back from `<name>.yaml` to `<name>.json`.
    fn data_file(&self, name: &str) -> Result<PathBuf, LoadError> {
        let dir = self.workspace_root.join(DATA_DIR);
        let primary = dir.join(name);
        if primary.is_file() {
            return Ok(primary);
        }
        if let Some(stem) = name.strip_suffix(".yaml") {
            let alternative = dir.join(format!("{stem}.json"));
            if alternative.is_file() {
                return Ok(alternative);
            }
        }
        Err(LoadError::Io {
            path: primary,
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "ui-spec data file not found (expected YAML or JSON)",
            ),
        })
    }

    /// Reads and processes one document.
    fn load_document<T: DeserializeOwned>(
        &self,
        path: &Path,
        kind: DocKind,
    ) -> Result<T, LoadError> {
        let text = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.load_text(&text, path, kind)
    }

    /// Runs the staged load on document text already in memory.
    fn load_text<T: DeserializeOwned>(
        &self,
        text: &str,
        path: &Path,
        kind: DocKind,
    ) -> Result<T, LoadError> {
        let value = parse_document(text, path)?;
        self.validate(&value, path)?;
        if !document_matches(&value, kind) {
            return Err(LoadError::Unknown {
                path: path.to_path_buf(),
                message: format!("expected a {} document", kind.noun()),
            });
        }
        serde_json::from_value(value).map_err(|e| LoadError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Validates one raw document against the schema.
    fn validate(&self, value: &serde_json::Value, path: &Path) -> Result<(), LoadError> {
        let errors: Vec<String> = self
            .validator
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

    /// Reads one v2 document from the workspace.
    fn read_v2_text(&self, name: &str) -> Result<(String, PathBuf), LoadError> {
        let path = self.data_file(name)?;
        let text = std::fs::read_to_string(&path).map_err(|e| io(path.clone(), e))?;
        Ok((text, path))
    }

    /// Reads and decodes one v2 document that has no schema of its
    /// own (a theme, the icon map, the contexts table).
    fn read_v2_document<T: DeserializeOwned>(&self, name: &str) -> Result<T, LoadError> {
        let (text, path) = self.read_v2_text(name)?;
        decode_v2(&text, &path)
    }

    /// Reads the layout format v2 documents from disk: the two
    /// curated layouts, the two themes, `icons.yaml` and
    /// `contexts.yaml`, schema-validated and cross-checked (ADR 0061,
    /// task P7-03). The icon-file check runs here: this loader has a
    /// filesystem.
    fn read_v2_documents(&self, commands: &Commands) -> Result<V2Documents, LoadError> {
        let name_modern = format!("layouts/{}", LayoutKind::PanelModern.file_name());
        let name_classic = format!("layouts/{}", LayoutKind::PanelClassic.file_name());
        let (text, modern_path) = self.read_v2_text(&name_modern)?;
        let panel_modern = decode_layout(&text, &modern_path, &self.v2_validator)?;
        let (text, classic_path) = self.read_v2_text(&name_classic)?;
        let panel_classic = decode_layout(&text, &classic_path, &self.v2_validator)?;
        let theme_modern: Theme = self.read_v2_document(THEME_MODERN_FILE)?;
        let theme_classic: Theme = self.read_v2_document(THEME_CLASSIC_FILE)?;
        let icons: IconMap = self.read_v2_document(ICONS_FILE)?;
        let contexts: Contexts = self.read_v2_document(CONTEXTS_FILE)?;
        let known = command_ids(commands);
        cross_check_contexts(&contexts, &known)?;
        let check = |layout: &LayoutV2, path: &Path| {
            cross_check_layout(layout, path, &known, &icons, &contexts)
        };
        check(&panel_modern, &modern_path)?;
        check(&panel_classic, &classic_path)?;
        check_icon_files(&icons, &self.workspace_root)?;
        Ok(V2Documents {
            panel_modern,
            panel_classic,
            shared: Shared {
                icons,
                contexts,
                theme_modern,
                theme_classic,
            },
        })
    }

    /// Loads all four layout sets from the embedded documents (ADR
    /// 0031).
    ///
    /// Same validation path as [`Self::load_all`], minus the file
    /// reads and the icon-file check (no filesystem): the schemas and
    /// every data document are compiled into the binary (the schemas
    /// by `include_str!`, the documents by [`crate::embedded`]), so
    /// this works on wasm. Every document still goes through the full
    /// staged load (parse, schema validation, decode) and the
    /// cross-references run exactly as on disk.
    pub fn from_embedded() -> Result<LayoutSets, LoadError> {
        build_embedded()
    }
}

/// Builds a [`LoadError::Io`] from the read error of a data file.
fn io(path: PathBuf, source: std::io::Error) -> LoadError {
    LoadError::Io { path, source }
}

/// Builds the [`LoadError::Parse`] of a v1 layout the adapter cannot
/// convert.
fn adapt_error(name: &str, message: String) -> LoadError {
    LoadError::Parse {
        path: PathBuf::from(DATA_DIR).join(name),
        message,
    }
}

/// The one pass that parses every embedded document: v1 and v2,
/// staged and cross-checked exactly as [`Loader::load_all`] does on
/// disk.
fn build_embedded() -> Result<LayoutSets, LoadError> {
    use crate::embedded::{
        COMMANDS_YAML, LAYOUT_PANEL_CLASSIC_YAML, LAYOUT_PANEL_MODERN_YAML, PANEL_365_YAML,
        PANEL_2016_YAML, SHORTCUTS_YAML,
    };
    let loader = Loader {
        workspace_root: PathBuf::from(DATA_DIR),
        validator: compile_schema(SCHEMA_JSON)?,
        v2_validator: crate::v2::load::compile_v2_schema()?,
    };
    let path = |name: &str| PathBuf::from(DATA_DIR).join(name);
    let name_2016 = LayoutKind::Panel2016.file_name();
    let name_365 = LayoutKind::Panel365.file_name();
    let panel_2016 = loader.load_text(PANEL_2016_YAML, &path(name_2016), DocKind::Layout)?;
    let panel_365 = loader.load_text(PANEL_365_YAML, &path(name_365), DocKind::Layout)?;
    let commands_path = path(COMMANDS_FILE);
    let entries: Vec<CommandMapping> =
        loader.load_text(COMMANDS_YAML, &commands_path, DocKind::Commands)?;
    check_duplicate_command_ids(&entries, &commands_path)?;
    let commands = Commands { entries };
    let entries: Vec<Shortcut> =
        loader.load_text(SHORTCUTS_YAML, &path(SHORTCUTS_FILE), DocKind::Shortcuts)?;
    let shortcuts = Shortcuts { entries };
    let docs = Documents {
        panel_2016,
        panel_365,
        commands,
        shortcuts,
    };
    let union = docs.fluent_union();
    let layouts = [&docs.panel_2016, &docs.panel_365];
    check_references(&layouts, &union, &docs.commands, &docs.shortcuts)?;
    let known = command_ids(&docs.commands);
    let modern_path = path("layouts/panel-modern.yaml");
    let classic_path = path("layouts/panel-classic.yaml");
    let v2 = &loader.v2_validator;
    let panel_modern = decode_layout(LAYOUT_PANEL_MODERN_YAML, &modern_path, v2)?;
    let panel_classic = decode_layout(LAYOUT_PANEL_CLASSIC_YAML, &classic_path, v2)?;
    let Shared {
        icons,
        contexts,
        theme_modern,
        theme_classic,
    } = embedded_shared()?;
    cross_check_contexts(&contexts, &known)?;
    let check = |layout: &LayoutV2, path: &Path| {
        cross_check_layout(layout, path, &known, &icons, &contexts)
    };
    check(&panel_modern, &modern_path)?;
    check(&panel_classic, &classic_path)?;
    build_sets(
        docs,
        V2Documents {
            panel_modern,
            panel_classic,
            shared: Shared {
                icons,
                contexts,
                theme_modern,
                theme_classic,
            },
        },
    )
}

/// The v2 documents of one workspace, read and cross-checked together.
struct V2Documents {
    /// `ui-spec/layouts/panel-modern.yaml`.
    panel_modern: LayoutV2,
    /// `ui-spec/layouts/panel-classic.yaml`.
    panel_classic: LayoutV2,
    /// The icon map, the contexts and both themes.
    shared: crate::v2::load::Shared,
}

/// Builds the four [`LayoutSet`]s: the two curated v2 layouts, and
/// the two adapted v1 layouts (ADR 0061 "Adapter for v1 layouts").
/// The adapted layouts use the Panel Modern theme (ADR 0063); the
/// icon map and the contexts are shared by all four.
fn build_sets(docs: Documents, v2: V2Documents) -> Result<LayoutSets, LoadError> {
    let Documents {
        panel_2016,
        panel_365,
        commands,
        shortcuts,
    } = docs;
    let V2Documents {
        panel_modern,
        panel_classic,
        shared,
    } = v2;
    let Shared {
        icons,
        contexts,
        theme_modern,
        theme_classic,
    } = shared;
    let adapt = |layout: &Layout, name: &str| -> Result<LayoutV2, LoadError> {
        crate::v2::from_v1(layout).map_err(|e| adapt_error(name, e.to_string()))
    };
    let adapted_2016 = adapt(&panel_2016, LayoutKind::Panel2016.file_name())?;
    let adapted_365 = adapt(&panel_365, LayoutKind::Panel365.file_name())?;
    let set = |layout, v2, theme| LayoutSet {
        layout,
        v2,
        commands: commands.clone(),
        shortcuts: shortcuts.clone(),
        theme,
        icons: icons.clone(),
        contexts: contexts.clone(),
    };
    Ok(LayoutSets {
        panel_modern: set(Layout::default(), panel_modern, theme_modern.clone()),
        panel_classic: set(Layout::default(), panel_classic, theme_classic),
        panel_2016: set(panel_2016, adapted_2016, theme_modern.clone()),
        panel_365: set(panel_365, adapted_365, theme_modern),
    })
}

/// The v1 data documents of one workspace, read and validated
/// together so the cross-references can see the whole data set.
struct Documents {
    /// `ui-spec/panel-2016.yaml`.
    panel_2016: Layout,
    /// `ui-spec/panel-365.yaml`.
    panel_365: Layout,
    /// `ui-spec/commands.yaml`, shared by both layouts.
    commands: Commands,
    /// `ui-spec/shortcuts.yaml`, shared by both layouts.
    shortcuts: Shortcuts,
}

impl Documents {
    /// The union of both layouts' Fluent ids (ADR 0007
    /// cross-reference 1: "must appear in at least one layout").
    fn fluent_union(&self) -> BTreeSet<FluentId> {
        collect_fluent_ids(&self.panel_2016)
            .union(&collect_fluent_ids(&self.panel_365))
            .cloned()
            .collect()
    }
}

/// Compiles the schema document. Split out of [`Loader::new`] so the
/// failure paths are testable.
fn compile_schema(text: &str) -> Result<jsonschema::Validator, LoadError> {
    let schema: serde_json::Value = serde_json::from_str(text).map_err(|e| LoadError::Schema {
        path: PathBuf::from(SCHEMA_PATH),
        message: format!("the embedded schema is not valid JSON: {e}"),
    })?;
    jsonschema::validator_for(&schema).map_err(|e| LoadError::Schema {
        path: PathBuf::from(SCHEMA_PATH),
        message: format!("the embedded schema is not a valid JSON Schema: {e}"),
    })
}

/// Parses document text as JSON (for `.json` paths) or YAML
/// (everything else; YAML 1.2 is a superset of JSON).
pub(crate) fn parse_document(text: &str, path: &Path) -> Result<serde_json::Value, LoadError> {
    let is_json = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    let parsed: Result<serde_json::Value, String> = if is_json {
        serde_json::from_str(text).map_err(|e| e.to_string())
    } else {
        serde_saphyr::from_str(text).map_err(|e| e.to_string())
    };
    parsed.map_err(|message| LoadError::Parse {
        path: path.to_path_buf(),
        message,
    })
}

/// Checks that a schema-valid document is the kind of document the
/// file is supposed to hold. An empty list matches both tables (it
/// carries no markers).
fn document_matches(value: &serde_json::Value, kind: DocKind) -> bool {
    match kind {
        DocKind::Layout => value.get("schema_version").is_some(),
        DocKind::Commands => value.as_array().is_some_and(|entries| {
            entries
                .iter()
                .all(|entry| entry.get("fluent_ids").is_some())
        }),
        DocKind::Shortcuts => value
            .as_array()
            .is_some_and(|entries| entries.iter().all(|entry| entry.get("sequence").is_some())),
    }
}

/// Every Fluent id used by the layout: tab, group, backstage tab and
/// context-menu ids, every control id, and every control alias.
fn collect_fluent_ids(layout: &Layout) -> BTreeSet<FluentId> {
    let mut ids = BTreeSet::new();
    for tab in &layout.tabs {
        ids.insert(tab.id.clone());
        for group in &tab.groups {
            ids.insert(group.id.clone());
        }
    }
    for tab in &layout.backstage.tabs {
        ids.insert(tab.id.clone());
    }
    for menu in &layout.context_menus {
        ids.insert(menu.id.clone());
    }
    for control in all_controls(layout) {
        ids.insert(control.id.clone());
        for alias in &control.aliases {
            ids.insert(alias.clone());
        }
    }
    ids
}

/// Rejects a `commands.yaml` id used twice.
///
/// The table is one entry per Larrygram command id (ADR 0006), so a
/// duplicate would silently merge two entries in the cross-reference
/// checks and in `CommandLookup::build`. The layout id space is
/// *not* checked here: the committed data reuses Fluent ids across
/// surfaces (see `docs/tasks/P1-14.md`, Open issues), so the master
/// decides that policy.
fn check_duplicate_command_ids(entries: &[CommandMapping], path: &Path) -> Result<(), LoadError> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.id.as_str()) {
            return Err(LoadError::Duplicate {
                path: path.to_path_buf(),
                kind: "command_id",
                id: entry.id.to_string(),
            });
        }
    }
    Ok(())
}

/// Renders a shortcut's chord sequence for error messages
/// (`[[Ctrl, Z]]` becomes `Ctrl+Z`).
fn render_sequence(sequence: &[Vec<String>]) -> String {
    sequence
        .iter()
        .map(|chord| chord.join("+"))
        .collect::<Vec<String>>()
        .join(", ")
}

/// Runs the three cross-references of ADR 0007. Returns the first
/// violation in document order.
///
/// - `layouts` are the layouts being returned by the load; each one's
///   controls must dispatch a defined command (`control_command`).
///   [`Loader::load`] passes only the requested layout,
///   [`Loader::load_all`] passes both.
/// - `fluent_ids` is the union over **both** layout files: a command
///   may name a 365-only or a 2016-only control
///   (`fluent_id` fails only when the id is in no layout at all).
fn check_references(
    layouts: &[&Layout],
    fluent_ids: &BTreeSet<FluentId>,
    commands: &Commands,
    shortcuts: &Shortcuts,
) -> Result<(), LoadError> {
    // 1. Every Fluent id in commands.yaml is in at least one layout.
    for entry in &commands.entries {
        for fluent_id in &entry.fluent_ids {
            if !fluent_ids.contains(fluent_id) {
                return Err(LoadError::CrossReference {
                    kind: "fluent_id",
                    id: fluent_id.to_string(),
                    referenced_by: entry.id.to_string(),
                });
            }
        }
    }

    // 2. Every checked layout control's command is in commands.yaml.
    let command_ids: BTreeSet<&CommandId> =
        commands.entries.iter().map(|entry| &entry.id).collect();
    for layout in layouts {
        for control in all_controls(layout) {
            if let Some(command) = &control.command
                && !command_ids.contains(command)
            {
                return Err(LoadError::CrossReference {
                    kind: "control_command",
                    id: command.to_string(),
                    referenced_by: control.id.to_string(),
                });
            }
        }
    }

    // 3. Every shortcut's command is in commands.yaml.
    for shortcut in &shortcuts.entries {
        if !command_ids.contains(&shortcut.command) {
            return Err(LoadError::CrossReference {
                kind: "command_id",
                id: shortcut.command.to_string(),
                referenced_by: render_sequence(&shortcut.sequence),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Backstage, BackstageTab, ContextMenu, Control, ControlType, Group, MappingConfidence,
        QuickAccessToolbar, ShortcutScope, Tab,
    };

    fn empty_layout() -> Layout {
        Layout::default()
    }

    fn control(id: &str, command: Option<&str>) -> Control {
        Control {
            id: FluentId::new(id),
            r#type: ControlType::Button,
            label: "Label".to_owned(),
            order: 0,
            parent: None,
            command: command.map(CommandId::new),
            aliases: Vec::new(),
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
        }
    }

    fn command(id: &str, fluent_ids: &[&str]) -> CommandMapping {
        CommandMapping {
            id: CommandId::new(id),
            label: "Label".to_owned(),
            visuicmd: None,
            fluent_ids: fluent_ids.iter().map(|s| FluentId::new(*s)).collect(),
            mapping_confidence: MappingConfidence::Unknown,
        }
    }

    fn shortcut(sequence: &[&str], command: &str) -> Shortcut {
        Shortcut {
            sequence: sequence
                .iter()
                .map(|chord| chord.split('+').map(str::to_owned).collect())
                .collect(),
            command: CommandId::new(command),
            scope: ShortcutScope::Global,
            section: None,
        }
    }

    fn loader() -> Loader {
        Loader::new(".").expect("the embedded schema always compiles")
    }

    /// The good fixture workspace (used by the `load_all` unit test).
    fn good_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/good")
    }

    #[test]
    fn compile_schema_accepts_the_embedded_schema() {
        assert!(compile_schema(SCHEMA_JSON).is_ok());
    }

    #[test]
    fn compile_schema_rejects_broken_json() {
        let err = compile_schema("{not json").expect_err("must fail");
        match err {
            LoadError::Schema { path, message } => {
                assert_eq!(path, PathBuf::from(SCHEMA_PATH));
                assert!(message.contains("not valid JSON"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn compile_schema_rejects_a_non_schema() {
        // Valid JSON, but `type` is not a string: not a valid schema.
        let err = compile_schema(r#"{"type": 42}"#).expect_err("must fail");
        assert!(matches!(err, LoadError::Schema { .. }));
    }

    #[test]
    fn parse_document_reads_yaml() {
        let value = parse_document("a: 1\n", Path::new("x.yaml")).expect("valid yaml");
        assert_eq!(value["a"], 1);
    }

    #[test]
    fn parse_document_reads_json_paths_as_json() {
        let value = parse_document("{\"a\": [1, 2]}", Path::new("x.json")).expect("valid json");
        assert_eq!(value["a"][1], 2);
    }

    #[test]
    fn parse_document_rejects_broken_yaml() {
        let err = parse_document("a: [1\n", Path::new("x.yaml")).expect_err("must fail");
        assert!(matches!(err, LoadError::Parse { .. }));
    }

    #[test]
    fn parse_document_rejects_broken_json() {
        let err = parse_document("{oops}", Path::new("x.json")).expect_err("must fail");
        assert!(matches!(err, LoadError::Parse { .. }));
    }

    #[test]
    fn document_matches_kinds() {
        let layout = serde_json::json!({"schema_version": 1});
        let commands = serde_json::json!([{"fluent_ids": []}]);
        let shortcuts = serde_json::json!([{"sequence": [["Ctrl", "Z"]]}]);
        let empty = serde_json::json!([]);
        let scalar = serde_json::json!("nope");

        assert!(document_matches(&layout, DocKind::Layout));
        assert!(!document_matches(&layout, DocKind::Commands));
        assert!(!document_matches(&commands, DocKind::Layout));
        assert!(document_matches(&commands, DocKind::Commands));
        assert!(!document_matches(&commands, DocKind::Shortcuts));
        assert!(document_matches(&shortcuts, DocKind::Shortcuts));
        assert!(!document_matches(&shortcuts, DocKind::Commands));
        // An empty list is a valid (empty) commands or shortcuts table.
        assert!(document_matches(&empty, DocKind::Commands));
        assert!(document_matches(&empty, DocKind::Shortcuts));
        assert!(!document_matches(&empty, DocKind::Layout));
        assert!(!document_matches(&scalar, DocKind::Layout));
        assert!(!document_matches(&scalar, DocKind::Commands));
        assert!(!document_matches(&scalar, DocKind::Shortcuts));
    }

    #[test]
    fn render_sequence_formats_chords() {
        assert_eq!(
            render_sequence(&[vec!["Ctrl".into(), "Z".into()]]),
            "Ctrl+Z"
        );
        assert_eq!(
            render_sequence(&[vec!["Alt".into(), "H".into()], vec!["F".into(), "O".into()]]),
            "Alt+H, F+O"
        );
        assert_eq!(render_sequence(&[]), "");
    }

    #[test]
    fn collect_fluent_ids_covers_every_surface() {
        let layout = Layout {
            tabs: vec![Tab {
                id: FluentId::new("TabHome"),
                label: "Home".to_owned(),
                order: 0,
                tab_set: None,
                bounds: None,
                icon_ref: None,
                keytip: None,
                size_variant: None,
                visual_notes: None,
                groups: vec![Group {
                    id: FluentId::new("GroupClipboard"),
                    label: "Clipboard".to_owned(),
                    order: 0,
                    bounds: None,
                    icon_ref: None,
                    keytip: None,
                    size_variant: None,
                    visual_notes: None,
                    controls: vec![{
                        let mut paste = control("PasteGallery", Some("edit.paste"));
                        paste.aliases = vec![FluentId::new("PasteMenu")];
                        paste
                    }],
                }],
            }],
            backstage: Backstage {
                tabs: vec![BackstageTab {
                    id: FluentId::new("FileNew"),
                    label: "New".to_owned(),
                    order: 0,
                    bounds: None,
                    icon_ref: None,
                    keytip: None,
                    size_variant: None,
                    visual_notes: None,
                    controls: vec![control("FileSave", Some("file.save"))],
                }],
            },
            context_menus: vec![ContextMenu {
                id: FluentId::new("ShapeShortcutMenu"),
                label: "Menu".to_owned(),
                order: 0,
                bounds: None,
                icon_ref: None,
                keytip: None,
                size_variant: None,
                visual_notes: None,
                controls: vec![control("Cut", Some("edit.cut"))],
            }],
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("UndoQuickAccess", Some("edit.undo"))],
            },
            ..Layout::default()
        };
        let ids: Vec<String> = collect_fluent_ids(&layout)
            .iter()
            .map(|id| id.to_string())
            .collect();
        assert_eq!(
            ids,
            vec![
                "Cut",
                "FileNew",
                "FileSave",
                "GroupClipboard",
                "PasteGallery",
                "PasteMenu",
                "ShapeShortcutMenu",
                "TabHome",
                "UndoQuickAccess",
            ]
        );
    }

    #[test]
    fn check_references_accepts_consistent_data() {
        let layout = Layout {
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("FileSave", Some("file.save"))],
            },
            ..empty_layout()
        };
        let commands = Commands {
            entries: vec![command("file.save", &["FileSave"])],
        };
        let shortcuts = Shortcuts {
            entries: vec![shortcut(&["Ctrl+S"], "file.save")],
        };
        let fluent_ids = collect_fluent_ids(&layout);
        assert!(check_references(&[&layout], &fluent_ids, &commands, &shortcuts).is_ok());
    }

    #[test]
    fn check_references_accepts_an_id_only_the_other_layout_has() {
        // `commands.yaml` is shared: a command may name a control of
        // the layout being *returned* only (union reading of ADR
        // 0007 cross-reference 1).
        let checked = Layout {
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("FileSave", Some("file.save"))],
            },
            ..empty_layout()
        };
        let other = Layout {
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("RedoButton", Some("edit.redo"))],
            },
            ..empty_layout()
        };
        let fluent_ids: BTreeSet<FluentId> = collect_fluent_ids(&checked)
            .union(&collect_fluent_ids(&other))
            .cloned()
            .collect();
        let commands = Commands {
            entries: vec![
                command("file.save", &["FileSave"]),
                command("edit.redo", &["RedoButton"]),
            ],
        };
        assert!(
            check_references(&[&checked], &fluent_ids, &commands, &Shortcuts::default()).is_ok()
        );
    }

    #[test]
    fn check_references_rejects_unknown_fluent_id() {
        let layout = Layout {
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("FileSave", Some("file.save"))],
            },
            ..empty_layout()
        };
        let commands = Commands {
            entries: vec![command("file.save", &["FileSave", "Ghost"])],
        };
        let fluent_ids = collect_fluent_ids(&layout);
        let err = check_references(&[&layout], &fluent_ids, &commands, &Shortcuts::default())
            .expect_err("must fail");
        // The union reading: the id is in *no* layout.
        let message = err.to_string();
        assert!(message.contains("is not in any layout"), "got {message}");
        match err {
            LoadError::CrossReference {
                kind,
                id,
                referenced_by,
            } => {
                assert_eq!(kind, "fluent_id");
                assert_eq!(id, "Ghost");
                assert_eq!(referenced_by, "file.save");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn check_references_rejects_unknown_control_command() {
        let layout = Layout {
            quick_access_toolbar: QuickAccessToolbar {
                controls: vec![control("FileSave", Some("file.save"))],
            },
            ..empty_layout()
        };
        let commands = Commands {
            entries: vec![command("edit.paste", &[])],
        };
        let fluent_ids = collect_fluent_ids(&layout);
        let err = check_references(&[&layout], &fluent_ids, &commands, &Shortcuts::default())
            .expect_err("must fail");
        match err {
            LoadError::CrossReference {
                kind,
                id,
                referenced_by,
            } => {
                assert_eq!(kind, "control_command");
                assert_eq!(id, "file.save");
                assert_eq!(referenced_by, "FileSave");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn check_references_rejects_unknown_shortcut_command() {
        let commands = Commands {
            entries: vec![command("file.save", &[])],
        };
        let shortcuts = Shortcuts {
            entries: vec![shortcut(&["Ctrl+Z"], "edit.undo")],
        };
        let layout = empty_layout();
        let fluent_ids = collect_fluent_ids(&layout);
        let err = check_references(&[&layout], &fluent_ids, &commands, &shortcuts)
            .expect_err("must fail");
        match err {
            LoadError::CrossReference {
                kind,
                id,
                referenced_by,
            } => {
                assert_eq!(kind, "command_id");
                assert_eq!(id, "edit.undo");
                assert_eq!(referenced_by, "Ctrl+Z");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_returns_the_curated_v2_layouts_for_the_curated_kinds() {
        // Panel Modern and Panel Classic have no v1 data file: the
        // loader reads the hand-curated v2 files for them (ADR 0061).
        let loader = Loader::new(good_fixture()).expect("the fixture workspace loads");
        for kind in [LayoutKind::PanelModern, LayoutKind::PanelClassic] {
            let set = loader
                .load(kind)
                .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert_eq!(set.v2.id, kind.id(), "{kind:?}");
            assert!(set.layout.tabs.is_empty(), "{kind:?} has no v1 data");
            assert!(!set.theme.id.is_empty(), "{kind:?} has a theme");
            assert!(!set.icons.icons.is_empty(), "{kind:?} has the icon map");
            assert!(
                !set.contexts.contexts.is_empty(),
                "{kind:?} has the contexts"
            );
        }
    }

    #[test]
    fn load_all_returns_both_layouts_and_get_picks_one() {
        let loader = Loader::new(good_fixture()).expect("the fixture workspace loads");
        let sets = loader.load_all().expect("the good fixture is consistent");
        // Same data as loading each kind on its own.
        assert_eq!(
            sets.get(LayoutKind::Panel2016),
            &loader.load(LayoutKind::Panel2016).expect("2016 loads")
        );
        assert_eq!(
            sets.get(LayoutKind::Panel365),
            &loader.load(LayoutKind::Panel365).expect("365 loads")
        );
        // Distinct layouts, shared tables.
        assert_ne!(sets.panel_2016.layout, sets.panel_365.layout);
        assert_eq!(sets.panel_2016.commands, sets.panel_365.commands);
        assert_eq!(sets.panel_2016.shortcuts, sets.panel_365.shortcuts);
    }

    #[test]
    fn load_all_reports_the_same_fluent_id_failure_as_load() {
        let loader =
            Loader::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xref-fluent"))
                .expect("the fixture workspace loads");
        match loader.load_all() {
            Err(LoadError::CrossReference {
                kind,
                id,
                referenced_by,
            }) => {
                assert_eq!(kind, "fluent_id");
                assert_eq!(id, "GhostControl");
                assert_eq!(referenced_by, "file.save");
            }
            other => panic!("expected a fluent_id cross-reference error, got {other:?}"),
        }
    }

    #[test]
    fn load_text_rejects_schema_violations() {
        let err = loader()
            .load_text::<Vec<Shortcut>>("{}", Path::new("shortcuts.yaml"), DocKind::Shortcuts)
            .expect_err("must fail");
        match err {
            LoadError::Schema { path, message } => {
                assert_eq!(path, PathBuf::from("shortcuts.yaml"));
                assert!(!message.is_empty());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_text_rejects_the_wrong_document_kind() {
        // A shortcuts table in commands.yaml: schema-valid (both are
        // root alternatives) but not what this file should hold.
        let text = "- sequence: [[Ctrl, Z]]\n  command: edit.undo\n  scope: global\n";
        let err = loader()
            .load_text::<Vec<CommandMapping>>(text, Path::new("commands.yaml"), DocKind::Commands)
            .expect_err("must fail");
        match err {
            LoadError::Unknown { path, message } => {
                assert_eq!(path, PathBuf::from("commands.yaml"));
                assert!(message.contains("commands"), "got {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_text_names_layout_when_a_layout_file_holds_a_table() {
        // A commands/shortcuts table in panel-2016.yaml: the error
        // says "layout" (DocKind::noun).
        let text = "[]\n";
        let err = loader()
            .load_text::<Layout>(text, Path::new("panel-2016.yaml"), DocKind::Layout)
            .expect_err("must fail");
        match err {
            LoadError::Unknown { path, message } => {
                assert_eq!(path, PathBuf::from("panel-2016.yaml"));
                assert_eq!(message, "expected a layout document");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_text_names_shortcuts_when_a_shortcut_file_holds_commands() {
        // A commands table in shortcuts.yaml: the error says
        // "shortcuts" (DocKind::noun).
        let text = "- id: file.save\n  label: Save\n  visuicmd: null\n  fluent_ids: [FileSave]\n  mapping_confidence: exact\n";
        let err = loader()
            .load_text::<Vec<Shortcut>>(text, Path::new("shortcuts.yaml"), DocKind::Shortcuts)
            .expect_err("must fail");
        match err {
            LoadError::Unknown { path, message } => {
                assert_eq!(path, PathBuf::from("shortcuts.yaml"));
                assert_eq!(message, "expected a shortcuts document");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn data_file_falls_through_a_missing_non_yaml_name() {
        // A name that does not end in `.yaml` has no `.json`
        // alternative to try; the miss reports the primary path.
        let err = loader().data_file("commands.toml").expect_err("must fail");
        match err {
            LoadError::Io { path, source } => {
                assert!(path.ends_with("ui-spec/commands.toml"), "got {path:?}");
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn data_file_prefers_yaml_and_falls_back_to_json() {
        let dir = std::env::temp_dir().join(format!(
            "larrygram-ui-spec-data-file-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ui-spec")).expect("temp workspace");
        let loader = Loader::new(&dir).expect("the embedded schema always compiles");
        // Missing both: the error names the .yaml path.
        assert!(loader.data_file("commands.yaml").is_err());
        // Only the .json alternative exists: the fallback finds it.
        std::fs::write(dir.join("ui-spec/commands.json"), "[]").expect("json file");
        assert!(
            loader
                .data_file("commands.yaml")
                .expect("the .json fallback")
                .ends_with("ui-spec/commands.json")
        );
        // With both present the primary .yaml path wins.
        std::fs::write(dir.join("ui-spec/commands.yaml"), "[]").expect("yaml file");
        assert!(
            loader
                .data_file("commands.yaml")
                .expect("the .yaml primary")
                .ends_with("ui-spec/commands.yaml")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_command_ids_are_a_load_error() {
        let path = Path::new("ui-spec/commands.yaml");
        let entries = vec![command("file.save", &[]), command("edit.undo", &[])];
        assert!(check_duplicate_command_ids(&entries, path).is_ok());
        let dup = vec![
            command("file.save", &[]),
            command("edit.undo", &[]),
            command("file.save", &["FileSave"]),
        ];
        let err = check_duplicate_command_ids(&dup, path).expect_err("must fail");
        match err {
            LoadError::Duplicate { path: p, kind, id } => {
                assert_eq!(p, PathBuf::from("ui-spec/commands.yaml"));
                assert_eq!(kind, "command_id");
                assert_eq!(id, "file.save");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_text_rejects_values_the_model_cannot_represent() {
        // Schema-valid (`order` is any integer >= 0) but wider than
        // the typed model's `u32`: decoding must fail, not wrap.
        let layout = r#"
schema_version: 1
product: visio
source: s
source_url: https://example.com/x
source_revision: r
tabs: []
backstage:
  kind: backstage
  tabs: []
context_menus: []
quick_access_toolbar:
  id: qat
  controls:
    - id: FileSave
      type: button
      label: Save
      order: 4294967296
      command: file.save
      bounds: null
      icon_ref: null
      keytip: null
      size_variant: null
      visual_notes: null
"#;
        let err = loader()
            .load_text::<Layout>(layout, Path::new("panel-2016.yaml"), DocKind::Layout)
            .expect_err("must fail");
        assert!(matches!(err, LoadError::Parse { .. }), "got {err:?}");
    }

    #[test]
    fn io_wraps_the_read_error_of_a_data_file() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let error = io(PathBuf::from("ui-spec/icons.yaml"), source);
        assert!(
            matches!(&error, LoadError::Io { path, .. } if path.ends_with("ui-spec/icons.yaml"))
        );
        assert!(error.to_string().contains("cannot read"));
    }

    #[test]
    fn adapt_error_names_the_v1_file() {
        let error = adapt_error("panel-2016.yaml", "nope".to_owned());
        assert!(matches!(&error, LoadError::Parse { message, .. } if message == "nope"));
        assert!(error.to_string().contains("ui-spec/panel-2016.yaml"));
    }

    #[test]
    fn build_sets_reports_a_v1_layout_it_cannot_convert() {
        let docs = Documents {
            panel_2016: Layout {
                schema_version: 2,
                ..Layout::default()
            },
            panel_365: Layout::default(),
            commands: Commands::default(),
            shortcuts: Shortcuts::default(),
        };
        let v2 = V2Documents {
            panel_modern: LayoutV2::default(),
            panel_classic: LayoutV2::default(),
            shared: Shared {
                icons: Default::default(),
                contexts: Default::default(),
                theme_modern: Default::default(),
                theme_classic: Default::default(),
            },
        };
        let error = build_sets(docs, v2).expect_err("schema_version 2 cannot convert");
        assert!(
            matches!(&error, LoadError::Parse { message, .. } if message.contains("schema_version 2"))
        );
    }
}
