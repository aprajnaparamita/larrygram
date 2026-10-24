//! The committed `ui-spec/` data embedded in the binary (ADR 0031).
//!
//! The command bar is built from the committed YAML at runtime. Wasm has no
//! filesystem, so the four data documents are embedded here with
//! `include_str!` and parsed by
//! [`Loader::from_embedded`](crate::Loader::from_embedded), the same
//! validation path the tests use on disk. Nothing but the committed
//! files under `ui-spec/` is embedded; raw Microsoft data never lands
//! in the repository (AGENTS.md section 4).
//!
//! [`OUT_OF_SCOPE_TXT`] and [`is_out_of_scope`] carry the ADR 0031
//! out-of-scope rule: a control listed in `ui-spec/out-of-scope.txt`
//! renders disabled with an "out of scope" tooltip.

use std::collections::BTreeSet;
use std::sync::OnceLock;

/// The committed `panel-2016.yaml` as embedded text.
pub const PANEL_2016_YAML: &str = include_str!("../../../ui-spec/panel-2016.yaml");

/// The committed `panel-365.yaml` as embedded text.
pub const PANEL_365_YAML: &str = include_str!("../../../ui-spec/panel-365.yaml");

/// The committed `commands.yaml` as embedded text.
pub const COMMANDS_YAML: &str = include_str!("../../../ui-spec/commands.yaml");

/// The committed `shortcuts.yaml` as embedded text.
pub const SHORTCUTS_YAML: &str = include_str!("../../../ui-spec/shortcuts.yaml");

/// The committed `ui-spec/out-of-scope.txt` as embedded text (ADR 0031).
pub const OUT_OF_SCOPE_TXT: &str = include_str!("../../../ui-spec/out-of-scope.txt");

/// The committed `ui-spec/layouts/panel-modern.yaml` as embedded text
/// (ADR 0061).
pub const LAYOUT_PANEL_MODERN_YAML: &str =
    include_str!("../../../ui-spec/layouts/panel-modern.yaml");

/// The committed `ui-spec/layouts/panel-classic.yaml` as embedded
/// text (ADR 0061).
pub const LAYOUT_PANEL_CLASSIC_YAML: &str =
    include_str!("../../../ui-spec/layouts/panel-classic.yaml");

/// The committed `ui-spec/themes/panel-modern.yaml` as embedded text
/// (ADR 0063).
pub const THEME_PANEL_MODERN_YAML: &str = include_str!("../../../ui-spec/themes/panel-modern.yaml");

/// The committed `ui-spec/themes/panel-classic.yaml` as embedded text
/// (ADR 0063).
pub const THEME_PANEL_CLASSIC_YAML: &str =
    include_str!("../../../ui-spec/themes/panel-classic.yaml");

/// The committed `ui-spec/icons.yaml` as embedded text (ADR 0062).
pub const ICONS_YAML: &str = include_str!("../../../ui-spec/icons.yaml");

/// The committed `ui-spec/contexts.yaml` as embedded text (ADR 0064).
pub const CONTEXTS_YAML: &str = include_str!("../../../ui-spec/contexts.yaml");

/// True when `id` is listed in `ui-spec/out-of-scope.txt` (ADR 0031).
///
/// A line is either an exact Fluent id or a glob in which `*` stands
/// for any run of characters; lines starting with `#` and blank lines
/// are ignored. A control matched here renders disabled with the
/// tooltip "Out of scope for this run".
///
/// ```
/// use larrygram_ui_spec::embedded::is_out_of_scope;
/// assert!(is_out_of_scope("CoauthorMenu"));
/// assert!(is_out_of_scope("SomeMacroThing"));
/// assert!(!is_out_of_scope("FileSave"));
/// ```
pub fn is_out_of_scope(id: &str) -> bool {
    scope_list().contains(id)
}

/// The parsed out-of-scope list, built once per process.
fn scope_list() -> &'static OutOfScope {
    static LIST: OnceLock<OutOfScope> = OnceLock::new();
    LIST.get_or_init(|| OutOfScope::parse(OUT_OF_SCOPE_TXT))
}

/// The parsed `out-of-scope.txt` entries.
#[derive(Debug, Default, PartialEq, Eq)]
struct OutOfScope {
    /// Exact Fluent ids.
    exact: BTreeSet<String>,
    /// Glob patterns (lines containing `*`).
    globs: Vec<String>,
}

impl OutOfScope {
    /// Parses the file text: one entry per line, `#` comments and
    /// blank lines ignored.
    fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.contains('*') {
                out.globs.push(trimmed.to_owned());
            } else {
                out.exact.insert(trimmed.to_owned());
            }
        }
        out
    }

    /// True when `id` matches an exact entry or a glob.
    fn contains(&self, id: &str) -> bool {
        if self.exact.contains(id) {
            return true;
        }
        self.globs.iter().any(|glob| glob_match(glob, id))
    }
}

/// Matches `text` against a `*`-glob `pattern` (the only wildcard).
///
/// The pattern is anchored at both ends unless it starts or ends
/// with `*`. Segments between `*` must appear in order.
fn glob_match(pattern: &str, text: &str) -> bool {
    let segments: Vec<&str> = pattern.split('*').collect();
    let head_anchored = !pattern.starts_with('*');
    let tail_anchored = !pattern.ends_with('*');
    let mut rest = text;
    let mut first = true;
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        let last = segments
            .get(index + 1..)
            .is_some_and(|tail| tail.iter().all(|part| part.is_empty()));
        if first && head_anchored && last && tail_anchored {
            // A pattern without `*`: the whole text must match.
            return rest == *segment;
        }
        if first && head_anchored {
            let Some(tail) = rest.strip_prefix(segment) else {
                return false;
            };
            rest = tail;
        } else if last && tail_anchored {
            let Some(tail) = rest.strip_suffix(segment) else {
                return false;
            };
            rest = tail;
        } else {
            let Some(found) = rest.find(segment) else {
                return false;
            };
            rest = &rest[found + segment.len()..];
        }
        first = false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("ui-spec")
            .join(name);
        std::fs::read_to_string(path).expect("the committed data file")
    }

    #[test]
    fn the_constants_hold_the_committed_files() {
        assert_eq!(PANEL_2016_YAML, read("panel-2016.yaml"));
        assert_eq!(PANEL_365_YAML, read("panel-365.yaml"));
        assert_eq!(COMMANDS_YAML, read("commands.yaml"));
        assert_eq!(SHORTCUTS_YAML, read("shortcuts.yaml"));
        assert_eq!(OUT_OF_SCOPE_TXT, read("out-of-scope.txt"));
        assert_eq!(LAYOUT_PANEL_MODERN_YAML, read("layouts/panel-modern.yaml"));
        assert_eq!(
            LAYOUT_PANEL_CLASSIC_YAML,
            read("layouts/panel-classic.yaml")
        );
        assert_eq!(THEME_PANEL_MODERN_YAML, read("themes/panel-modern.yaml"));
        assert_eq!(THEME_PANEL_CLASSIC_YAML, read("themes/panel-classic.yaml"));
        assert_eq!(ICONS_YAML, read("icons.yaml"));
        assert_eq!(CONTEXTS_YAML, read("contexts.yaml"));
    }

    #[test]
    fn the_embedded_documents_look_like_their_documents() {
        // A cheap guard against an include_str! pointing at the wrong
        // file: each document has its own marker.
        assert!(PANEL_2016_YAML.contains("source: Office Fluent UI 2016"));
        assert!(PANEL_365_YAML.contains("Microsoft 365"));
        assert!(COMMANDS_YAML.contains("mapping_confidence"));
        assert!(SHORTCUTS_YAML.contains("sequence"));
        assert!(LAYOUT_PANEL_MODERN_YAML.contains("kind: panel-modern"));
        assert!(LAYOUT_PANEL_CLASSIC_YAML.contains("kind: panel-classic"));
        assert!(THEME_PANEL_MODERN_YAML.contains("id: panel-modern"));
        assert!(THEME_PANEL_CLASSIC_YAML.contains("id: panel-classic"));
        assert!(ICONS_YAML.contains("fallback: placeholder"));
        assert!(CONTEXTS_YAML.contains("enabled_in"));
    }

    #[test]
    fn out_of_scope_matches_exact_ids_and_globs() {
        let list = OutOfScope::parse("# comment\n\nCoauthorMenu\n*Macro*\nExact_Only\n");
        assert_eq!(
            list.exact,
            BTreeSet::from(["CoauthorMenu".to_owned(), "Exact_Only".to_owned()])
        );
        assert_eq!(list.globs, vec!["*Macro*".to_owned()]);
        assert!(list.contains("CoauthorMenu"));
        assert!(list.contains("Exact_Only"));
        assert!(list.contains("RunMyMacroNow"));
        assert!(list.contains("Macro"));
        assert!(!list.contains("CoauthorMen"));
        assert!(!list.contains("CoauthorMenuExtra"));
        assert!(!list.contains("Makro"));
        assert!(!list.contains("Exact_Only_Too"));
    }

    #[test]
    fn glob_matching_is_anchored_at_the_pattern_ends() {
        // Anchored head: the segment must start the text.
        assert!(glob_match("Pre*", "PreFix"));
        assert!(glob_match("Pre*", "Pre"));
        assert!(!glob_match("Pre*", "xPreFix"));
        // Anchored tail: the segment must end the text.
        assert!(glob_match("*Fix", "PreFix"));
        assert!(glob_match("*Fix", "Fix"));
        assert!(!glob_match("*Fix", "Fixed"));
        // Anchored both ends.
        assert!(glob_match("Whole", "Whole"));
        assert!(!glob_match("Whole", "Wholes"));
        // Unanchored both ends.
        assert!(glob_match("*mid*", "a mid b"));
        assert!(!glob_match("*mid*", "a mi d b"));
        // Several segments appear in order.
        assert!(glob_match("*a*b*", "xaybz"));
        assert!(!glob_match("*a*b*", "xbyaz"));
        // A pattern of only stars matches everything.
        assert!(glob_match("*", "anything"));
        assert!(glob_match("**", ""));
    }

    #[test]
    fn is_out_of_scope_reads_the_committed_list() {
        // The committed list marks the collaboration surface and
        // macros out of scope (ui-spec/out-of-scope.txt).
        assert!(is_out_of_scope("CoauthorMenu"));
        assert!(is_out_of_scope("RunMyMacro"));
        assert!(is_out_of_scope("SomeAddInThing"));
        assert!(!is_out_of_scope("FileSave"));
        assert!(!is_out_of_scope("TabHome"));
    }
}
