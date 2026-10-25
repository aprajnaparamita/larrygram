//! Every theme token the widgets read exists in both committed themes
//! (ADR 0065 "Data tests", task P7-10).
//!
//! The readers are `crates/ui/src/theme.rs` (the egui mapping in
//! `docs/ui/themes.md`), `crates/ui/src/canvas.rs` (the drawing-area
//! colours) and `crates/ui/src/panel_modern/metrics.rs`
//! (`Metrics::from_theme`, which the shell runs against every theme).
//! A missing token does not crash the app — the readers fall back with
//! one warning — but the fallback silently changes the look or the
//! geometry, so the data must carry every token.
//!
//! The lists below name every token the widgets read, with the read
//! site in a comment. `every_read_token_is_listed` scans the three
//! reader files for token-shaped string literals and fails when one is
//! not listed here (and not one of the documented non-tokens), so the
//! lists cannot drift from the code.

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
/// `visuals` (the single-token `color_or` calls) and `canvas.rs`
/// `CanvasColors::from_theme`.
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

/// The metric tokens `panel_modern/metrics.rs` `Metrics::from_theme`
/// reads (the shell runs it against every theme).
const METRIC_TOKENS: &[&str] = &[
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

/// The reader source files the scan covers.
const READER_SOURCES: &[&str] = &[
    "crates/ui/src/theme.rs",
    "crates/ui/src/canvas.rs",
    "crates/ui/src/panel_modern/metrics.rs",
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
