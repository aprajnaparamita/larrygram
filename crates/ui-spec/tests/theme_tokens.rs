//! The theme-token contract of ADR 0063, both directions (ADR 0065
//! "Data tests"; tasks P7-10 and P7-14).
//!
//! Forward: every theme token the widgets read exists in **both**
//! committed themes. The widgets draw the same surfaces under either
//! theme (the Panel Classic menu widget draws every context menu, the
//! shell runs both metric readers against the active theme), so the
//! vocabulary is shared even where a theme's own look never draws a
//! surface. A missing token does not crash the app — the readers fall
//! back with one warning — but the fallback silently changes the look
//! or the geometry, so the data must carry every token.
//!
//! Reverse (P7-14's master ruling): every token of a committed theme
//! is read by some widget — `every_theme_token_is_read_by_a_widget`
//! scans `crates/ui/src` for the token's name and fails when no widget
//! mentions it, so dead data cannot accumulate in the YAML files.
//!
//! The lists below name every token the widgets read, with the read
//! site in a comment. `every_read_token_is_listed` scans the reader
//! seams (`theme.rs`, `canvas.rs` and the two metric readers) for
//! token-shaped string literals and fails when one is not listed here
//! (and not one of the documented non-tokens), so the lists cannot
//! drift from those seams. The widget files read their tokens through
//! `theme::color_at` with the name from these same lists; the reverse
//! test covers them.

// The never-crash lints apply to non-test code only (AGENTS.md
// section 6); these helpers are test code and use `expect` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use std::path::{Path, PathBuf};

use larrygram_ui_spec::Loader;
use larrygram_ui_spec::v2::{Metric, Theme};

/// The repository root: `crates/ui-spec/../..`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The colour tokens read directly (no fallback chain): `theme.rs`
/// `visuals`, `canvas.rs` `CanvasColors::from_theme` and the widgets'
/// `theme::color_at` reads (P7-14's wiring).
const COLOR_TOKENS: &[&str] = &[
    // theme.rs `visuals`
    "chrome.background",
    "chrome.text",
    "chrome.text_disabled",
    "button.hover.fill",
    "button.hover.border",
    "button.pressed.fill",
    "button.pressed.border",
    // canvas.rs `CanvasColors::from_theme`
    "canvas.background",
    "page.background",
    "page.border",
    "page.shadow",
    "ruler.background",
    "ruler.ticks",
    // shell.rs (title bar, status bar, page tabs, dock panes)
    "chrome.title_bar",
    "chrome.separator",
    "chrome.border",
    "pane.background",
    "pane.border",
    "pane.title.fill",
    "pane.title.text",
    "status_bar.background",
    "status_bar.text",
    "page_tab.text",
    "page_tab.active.text",
    // panel_classic/menu.rs and toolbar.rs
    "menu.separator",
    "menu.gutter",
    "menu.highlight.fill",
    "menu.highlight.border",
    "menu.bar.open.fill",
    "menu.bar.open.border",
    // panel_modern.rs (tabs, groups, contextual tab sets)
    "accent",
    "accent.text",
    "tab.text",
    "tab.selected.text",
    "group.separator",
    "group.caption.text",
    "contextual.green.band",
    "contextual.green.fill",
    "contextual.green.text",
    "contextual.orange.band",
    "contextual.orange.fill",
    "contextual.orange.text",
    // file_view.rs
    "file_view.background",
    "file_view.nav.background",
    "file_view.nav.text",
    "file_view.nav.selected",
];

/// The colour fallback chains of `theme.rs` `visuals` (`color_or`):
/// any one member serves the read, the first is the warned name.
const COLOR_CHAINS: &[&[&str]] = &[
    &["menu.background", "tooltip.background", "chrome.background"],
    &["menu.border", "tooltip.border", "chrome.border"],
    &["field.background", "page.background"],
];

/// The font roles `theme.rs` `text_styles` reads: `ui` directly, and
/// the heading chain (`title`, then `pane_title`, then `ui`).
const FONT_REQUIRED: &[&str] = &["ui"];
const FONT_CHAINS: &[&[&str]] = &[&["title", "pane_title", "ui"]];

/// The metric tokens `panel_modern/metrics.rs` (`Metrics::from_theme`)
/// and `panel_classic/metrics.rs` (`ClassicMetrics::from_theme`) read;
/// the shell runs both against every theme, and `canvas.rs` reads
/// `ruler.size`.
const METRIC_TOKENS: &[&str] = &[
    // panel_classic/metrics.rs `ClassicMetrics::from_theme`
    "title_bar.height",
    "menu_bar.height",
    "menu_bar.item.padding_x",
    "toolbar.row.height",
    "toolbar.button",
    "toolbar.split_arrow.width",
    "toolbar.separator",
    "toolbar.gripper.width",
    "combo.font.width",
    "combo.font_size.width",
    "combo.zoom.width",
    "menu.row.height",
    "menu.separator.height",
    "menu.gutter.width",
    "menu.icon",
    "menu.padding_x",
    "menu.shortcut.gap",
    "menu.submenu_arrow.width",
    "shapes_window.width",
    "ruler.size",
    "page_tabs.height",
    "status_bar.height",
    "icon.small",
    // panel_modern/metrics.rs `Metrics::from_theme`
    "tab_strip.height",
    "tab.padding_x",
    "file_button.width",
    "command_bar.height",
    "command_bar.padding_top",
    "group.padding_x",
    "group.caption.height",
    "group.separator.width",
    "launcher",
    "button.large",
    "button.medium",
    "button.small",
    "split.arrow.width",
    "stack.rows",
    "stack.row.height",
    "rows.row.height",
    "rows.gap",
    "combo.height",
    "check.size",
    "file_view.nav.width",
    "file_view.nav.row.height",
    "qat.button",
];

/// The named fields the `Metric::Fields` tokens must carry
/// (`Metrics::from_theme`).
const METRIC_FIELDS: &[(&str, &str)] = &[
    ("toolbar.button", "width"),
    ("toolbar.button", "height"),
    ("toolbar.separator", "width"),
    ("toolbar.separator", "margin"),
    ("launcher", "width"),
    ("launcher", "height"),
    ("button.large", "min_width"),
    ("button.large", "height"),
    ("button.large", "icon"),
    ("button.medium", "height"),
    ("button.medium", "icon"),
    ("button.small", "width"),
    ("button.small", "height"),
    ("button.small", "icon"),
    ("qat.button", "width"),
    ("qat.button", "height"),
];

/// The default icon tint (`icons/mod.rs` `tint_for`).
const DEFAULT_TINT: &str = "icon.neutral";

/// Token-shaped string literals in the reader files that are not theme
/// tokens: command ids (canvas.rs page and zoom buttons) and one
/// egui memory id (theme.rs `active_theme_id`).
const NOT_TOKENS: &[&str] = &[
    "larrygram.theme.active",
    "page.delete",
    "page.move_left",
    "page.move_right",
    "page.next",
    "page.previous",
    "page.rename",
    "view.zoom.fit",
    "view.zoom.in",
    "view.zoom.out",
];

/// The reader seams the drift scan covers: the egui mapping, the
/// canvas colours and the two metric readers. (The widgets read
/// through `theme::color_at`; their names are the lists above, and
/// `every_theme_token_is_read_by_a_widget` covers them.)
const READER_SOURCES: &[&str] = &[
    "crates/ui/src/theme.rs",
    "crates/ui/src/canvas.rs",
    "crates/ui/src/panel_modern/metrics.rs",
    "crates/ui/src/panel_classic/metrics.rs",
];

/// The two committed themes, loaded from disk through the loader.
fn themes() -> Vec<(&'static str, Theme)> {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    let sets = loader.load_all().expect("the committed data loads");
    vec![
        ("panel-modern", sets.panel_modern.theme),
        ("panel-classic", sets.panel_classic.theme),
    ]
}

/// The icon tint tokens the icon map references (`icons.yaml`), plus
/// the default tint.
fn icon_tints(sets: &larrygram_ui_spec::LayoutSets) -> Vec<String> {
    let icons = &sets.panel_modern.icons;
    let mut out = vec![DEFAULT_TINT.to_owned()];
    out.extend(icons.tints.keys().cloned());
    for entry in icons.icons.values() {
        if let Some(tint) = &entry.tint {
            out.push(tint.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every theme token the widgets read exists in both committed themes
/// (ADR 0065 "Data tests").
#[test]
fn every_theme_token_the_widgets_read_exists_in_both_themes() {
    let loader = Loader::new(workspace_root()).expect("the embedded schemas compile");
    let sets = loader.load_all().expect("the committed data loads");
    let tints = icon_tints(&sets);
    for (name, theme) in themes() {
        for token in COLOR_TOKENS {
            assert!(
                theme.colors.contains_key(*token),
                "{name} has no colour token {token:?}"
            );
        }
        for chain in COLOR_CHAINS {
            assert!(
                chain.iter().any(|token| theme.colors.contains_key(*token)),
                "{name} serves none of the colour chain {chain:?}"
            );
        }
        for token in FONT_REQUIRED {
            assert!(
                theme.fonts.contains_key(*token),
                "{name} has no font role {token:?}"
            );
        }
        for chain in FONT_CHAINS {
            assert!(
                chain.iter().any(|role| theme.fonts.contains_key(*role)),
                "{name} serves none of the font chain {chain:?}"
            );
        }
        for token in METRIC_TOKENS {
            assert!(
                theme.metrics.contains_key(*token),
                "{name} has no metric token {token:?}"
            );
        }
        for (token, field) in METRIC_FIELDS {
            let usable = match theme.metrics.get(*token) {
                Some(Metric::Fields(fields)) => fields.contains_key(*field),
                _ => false,
            };
            assert!(usable, "{name} has no metric {token:?}.{field}");
        }
        for tint in &tints {
            assert!(
                theme.colors.contains_key(tint.as_str()),
                "{name} has no icon tint {tint:?}"
            );
        }
    }
}

/// The tokens the readers use are all listed here: a new token read in
/// `crates/ui/src` fails this test until it is listed and checked
/// above (ADR 0065 keeps the lists and the code in step).
#[test]
fn every_read_token_is_listed() {
    let root = workspace_root();
    let known: Vec<&str> = COLOR_TOKENS
        .iter()
        .copied()
        .chain(COLOR_CHAINS.iter().flat_map(|chain| chain.iter().copied()))
        .chain(FONT_REQUIRED.iter().copied())
        .chain(FONT_CHAINS.iter().flat_map(|chain| chain.iter().copied()))
        .chain(METRIC_TOKENS.iter().copied())
        .chain(METRIC_FIELDS.iter().map(|(token, _)| *token))
        .chain(NOT_TOKENS.iter().copied())
        .collect();
    for source in READER_SOURCES {
        let text = std::fs::read_to_string(root.join(source))
            .unwrap_or_else(|e| panic!("read {source}: {e}"));
        for literal in token_literals(&text) {
            assert!(
                known.contains(&literal.as_str()),
                "{source} reads the token-shaped literal {literal:?}, which this test does not list"
            );
        }
    }
}

/// The token-shaped string literals of one source file: dotted
/// lowercase names like `page.shadow` (a theme token, a command id or
/// a memory id). The test modules are skipped: they name sample
/// tokens, not reads.
fn token_literals(text: &str) -> Vec<String> {
    let text = text.split("#[cfg(test)]").next().unwrap_or(text);
    // Doc and line comments name tokens in prose, not reads.
    let text: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    let mut rest: &str = &text;
    while let Some(start) = rest.find('"') {
        let Some(end) = rest[start + 1..].find('"') else {
            break;
        };
        let literal = &rest[start + 1..start + 1 + end];
        rest = &rest[start + 1 + end + 1..];
        if literal
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
            && literal.contains('.')
            && !literal.starts_with('.')
            && !literal.ends_with('.')
        {
            out.push(literal.to_owned());
        }
    }
    out
}

/// Every token of a committed theme is read by some widget (the P7-14
/// master ruling: "a test that fails when a theme token is read by no
/// widget").
///
/// The scan looks for the token's name as a quoted literal anywhere in
/// `crates/ui/src` outside the test modules, so a token no widget
/// draws fails here and must be deleted from the YAML (or wired where
/// it is drawn). Font roles are exempt: `theme.rs` `text_styles` maps
/// every role the theme carries onto an egui text style.
#[test]
fn every_theme_token_is_read_by_a_widget() {
    let sources = widget_sources(&workspace_root());
    for (name, theme) in themes() {
        for token in theme.colors.keys() {
            assert!(
                sources.contains(&format!("\"{token}\"")),
                "no widget reads the colour token {token:?} of theme {name}: \
                 wire it where it is drawn or delete it"
            );
        }
        for token in theme.metrics.keys() {
            assert!(
                sources.contains(&format!("\"{token}\"")),
                "no widget reads the metric token {token:?} of theme {name}: \
                 wire it where it is drawn or delete it"
            );
        }
    }
}

/// The concatenated non-test sources of `crates/ui/src`.
fn widget_sources(root: &Path) -> String {
    let mut out = String::new();
    collect_sources(&root.join("crates/ui/src"), &mut out);
    out
}

/// Appends every `.rs` file under `dir` (test modules stripped).
fn collect_sources(dir: &Path, out: &mut String) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {dir:?}: {e}"));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
            out.push_str(&strip_test_modules(&text));
        }
    }
}

/// The source with every `#[cfg(test)]` item removed (brace-matched,
/// so a mid-file test module does not hide the code after it).
fn strip_test_modules(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "#[cfg(test)]".len()..];
        let Some(brace) = after.find('{') else {
            return out;
        };
        let mut depth = 0_usize;
        let mut end = None;
        for (index, character) in after[brace..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 {
                end = Some(brace + index + character.len_utf8());
                break;
            }
        }
        match end {
            Some(end) => rest = &after[end..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}
