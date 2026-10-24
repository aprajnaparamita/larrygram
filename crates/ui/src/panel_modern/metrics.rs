//! Sizes of the Panel Modern widgets, in logical pixels (ADR 0061).
//!
//! The defaults are the values of `ui-spec/themes/panel-modern.yaml`;
//! [`Metrics::from_theme`] reads them from any [`Theme`] so task P7-09
//! can pass the loaded theme in. A token that is missing, non-finite or
//! non-positive keeps its default: a theme file can never crash the
//! app (AGENTS.md section 6). Colours are not read here: task P7-05
//! owns the colour tokens and the widgets use egui defaults until it
//! merges.

use egui::Vec2;
use larrygram_ui_spec::v2::{Metric, Theme};

/// Widget sizes of one Panel Modern layout, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// Height of the tab strip (`tab_strip.height`).
    pub tab_strip_height: f32,
    /// Horizontal padding of a tab caption (`tab.padding_x`).
    pub tab_padding_x: f32,
    /// Width of the File button (`file_button.width`).
    pub file_button_width: f32,
    /// Height of the command bar (`command_bar.height`).
    pub command_bar_height: f32,
    /// Padding above the groups (`command_bar.padding_top`).
    pub command_bar_padding_top: f32,
    /// Horizontal padding inside a group (`group.padding_x`).
    pub group_padding_x: f32,
    /// Height of the group caption row (`group.caption.height`).
    pub group_caption_height: f32,
    /// Width of the line between groups (`group.separator.width`).
    pub group_separator_width: f32,
    /// Size of a group launcher (`launcher`).
    pub launcher_size: Vec2,
    /// Minimum width of a large control (`button.large.min_width`).
    pub large_min_width: f32,
    /// Height of a large control (`button.large.height`).
    pub large_height: f32,
    /// Icon size of a large control (`button.large.icon`).
    pub large_icon: f32,
    /// Height of a medium control (`button.medium.height`).
    pub medium_height: f32,
    /// Icon size of a medium control (`button.medium.icon`).
    pub medium_icon: f32,
    /// Width of a small control (`button.small.width`).
    pub small_width: f32,
    /// Height of a small control (`button.small.height`).
    pub small_height: f32,
    /// Icon size of a small control (`button.small.icon`).
    pub small_icon: f32,
    /// Width of a split button's arrow part (`split.arrow.width`).
    pub split_arrow_width: f32,
    /// Rows per column in a `stack` group (`stack.rows`).
    pub stack_rows: u32,
    /// Height of one row in a `stack` group (`stack.row.height`).
    pub stack_row_height: f32,
    /// Height of one row in a `rows` group (`rows.row.height`).
    pub rows_row_height: f32,
    /// Gap between the clusters of a `rows` group (`rows.gap`).
    pub rows_gap: f32,
    /// Height of a combo box (`combo.height`).
    pub combo_height: f32,
    /// Side of a check box (`check.size`).
    pub check_size: f32,
    /// Width of the File view navigation column (`file_view.nav.width`).
    pub file_view_nav_width: f32,
    /// Height of one File view navigation row
    /// (`file_view.nav.row.height`).
    pub file_view_nav_row_height: f32,
    /// Side of a Quick Access Toolbar button (`qat.button`).
    pub qat_button: Vec2,
}

impl Default for Metrics {
    /// The metrics of `ui-spec/themes/panel-modern.yaml`.
    fn default() -> Self {
        Self {
            tab_strip_height: 25.0,
            tab_padding_x: 12.0,
            file_button_width: 59.0,
            command_bar_height: 92.0,
            command_bar_padding_top: 2.0,
            group_padding_x: 4.0,
            group_caption_height: 17.0,
            group_separator_width: 1.0,
            launcher_size: Vec2::new(16.0, 17.0),
            large_min_width: 42.0,
            large_height: 69.0,
            large_icon: 32.0,
            medium_height: 23.0,
            medium_icon: 16.0,
            small_width: 22.0,
            small_height: 23.0,
            small_icon: 16.0,
            split_arrow_width: 12.0,
            stack_rows: 3,
            stack_row_height: 23.0,
            rows_row_height: 23.0,
            rows_gap: 8.0,
            combo_height: 23.0,
            check_size: 13.0,
            file_view_nav_width: 123.0,
            file_view_nav_row_height: 38.0,
            qat_button: Vec2::new(22.0, 22.0),
        }
    }
}

impl Metrics {
    /// The metrics of `theme`, with the defaults for every token the
    /// theme does not carry (or carries unusably).
    pub fn from_theme(theme: &Theme) -> Self {
        let mut m = Self::default();
        let t = ThemeTokens { theme };
        m.tab_strip_height = t.get("tab_strip.height", m.tab_strip_height);
        m.tab_padding_x = t.get("tab.padding_x", m.tab_padding_x);
        m.file_button_width = t.get("file_button.width", m.file_button_width);
        m.command_bar_height = t.get("command_bar.height", m.command_bar_height);
        m.command_bar_padding_top = t.get("command_bar.padding_top", m.command_bar_padding_top);
        m.group_padding_x = t.get("group.padding_x", m.group_padding_x);
        m.group_caption_height = t.get("group.caption.height", m.group_caption_height);
        m.group_separator_width = t.get("group.separator.width", m.group_separator_width);
        m.launcher_size = Vec2::new(
            t.field("launcher", "width", m.launcher_size.x),
            t.field("launcher", "height", m.launcher_size.y),
        );
        m.large_min_width = t.field("button.large", "min_width", m.large_min_width);
        m.large_height = t.field("button.large", "height", m.large_height);
        m.large_icon = t.field("button.large", "icon", m.large_icon);
        m.medium_height = t.field("button.medium", "height", m.medium_height);
        m.medium_icon = t.field("button.medium", "icon", m.medium_icon);
        m.small_width = t.field("button.small", "width", m.small_width);
        m.small_height = t.field("button.small", "height", m.small_height);
        m.small_icon = t.field("button.small", "icon", m.small_icon);
        m.split_arrow_width = t.get("split.arrow.width", m.split_arrow_width);
        if let Some(rows) = t.number("stack.rows") {
            let rows = rows.round().clamp(1.0, 16.0) as u32;
            m.stack_rows = rows;
        }
        m.stack_row_height = t.get("stack.row.height", m.stack_row_height);
        m.rows_row_height = t.get("rows.row.height", m.rows_row_height);
        m.rows_gap = t.get("rows.gap", m.rows_gap);
        m.combo_height = t.get("combo.height", m.combo_height);
        m.check_size = t.get("check.size", m.check_size);
        m.file_view_nav_width = t.get("file_view.nav.width", m.file_view_nav_width);
        m.file_view_nav_row_height = t.get("file_view.nav.row.height", m.file_view_nav_row_height);
        m.qat_button = Vec2::new(
            t.field("qat.button", "width", m.qat_button.x),
            t.field("qat.button", "height", m.qat_button.y),
        );
        m
    }

    /// The height of one group's content area: the rows of a `stack`
    /// group. Large controls span it (ADR 0065).
    pub fn content_height(&self) -> f32 {
        self.stack_row_height * (self.stack_rows.max(1) as f32)
    }
}

/// A theme's metric table, read with fallbacks.
struct ThemeTokens<'a> {
    /// The theme under test.
    theme: &'a Theme,
}

impl ThemeTokens<'_> {
    /// The `key` metric as a number, when the theme carries one.
    fn number(&self, key: &str) -> Option<f32> {
        match self.theme.metrics.get(key) {
            Some(Metric::Px(value)) => Some(*value as f32),
            Some(Metric::Fields(_)) | None => None,
        }
    }

    /// The `field` of the `key` metric table, when present.
    fn field_number(&self, key: &str, field: &str) -> Option<f32> {
        match self.theme.metrics.get(key) {
            Some(Metric::Fields(fields)) => fields.get(field).map(|value| *value as f32),
            Some(Metric::Px(_)) | None => None,
        }
    }

    /// The `key` metric, `fallback` when the theme has no usable one.
    fn get(&self, key: &str, fallback: f32) -> f32 {
        usable(self.number(key), fallback)
    }

    /// The `field` of the `key` metric table, `fallback` when the
    /// theme has no usable one.
    fn field(&self, key: &str, field: &str, fallback: f32) -> f32 {
        usable(self.field_number(key, field), fallback)
    }
}

/// `value` when it is finite and positive, else `fallback`.
fn usable(value: Option<f32>, fallback: f32) -> f32 {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => value,
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A theme with the tokens under test.
    fn theme() -> Theme {
        let mut metrics: BTreeMap<String, Metric> = BTreeMap::new();
        metrics.insert("tab_strip.height".to_owned(), Metric::Px(30.0));
        metrics.insert("command_bar.height".to_owned(), Metric::Px(-4.0));
        metrics.insert("stack.rows".to_owned(), Metric::Px(2.0));
        metrics.insert(
            "button.large".to_owned(),
            Metric::Fields(BTreeMap::from([
                ("min_width".to_owned(), 50.0),
                ("height".to_owned(), 60.0),
            ])),
        );
        Theme {
            schema_version: 1,
            id: "test".to_owned(),
            name: "Test".to_owned(),
            fonts: BTreeMap::new(),
            colors: BTreeMap::new(),
            metrics,
            contextual_tab_sets: BTreeMap::new(),
        }
    }

    #[test]
    fn defaults_match_the_committed_theme() {
        let m = Metrics::default();
        assert_eq!(m.tab_strip_height, 25.0);
        assert_eq!(m.file_button_width, 59.0);
        assert_eq!(m.command_bar_height, 92.0);
        assert_eq!(m.large_min_width, 42.0);
        assert_eq!(m.large_height, 69.0);
        assert_eq!(m.stack_rows, 3);
        assert_eq!(m.stack_row_height, 23.0);
        assert_eq!(m.content_height(), 69.0);
    }

    #[test]
    fn from_theme_reads_tokens_and_keeps_defaults() {
        let m = Metrics::from_theme(&theme());
        // Read from the theme.
        assert_eq!(m.tab_strip_height, 30.0);
        assert_eq!(m.large_min_width, 50.0);
        assert_eq!(m.large_height, 60.0);
        assert_eq!(m.stack_rows, 2);
        // Fields the theme does not carry keep their default.
        assert_eq!(m.large_icon, 32.0);
        assert_eq!(m.stack_row_height, 23.0);
        // A non-positive token is unusable and keeps its default.
        assert_eq!(m.command_bar_height, 92.0);
    }

    #[test]
    fn unusable_values_keep_the_default() {
        assert_eq!(usable(None, 7.0), 7.0);
        assert_eq!(usable(Some(f32::NAN), 7.0), 7.0);
        assert_eq!(usable(Some(0.0), 7.0), 7.0);
        assert_eq!(usable(Some(9.0), 7.0), 9.0);
    }

    #[test]
    fn content_height_never_scales_with_zero_rows() {
        let m = Metrics {
            stack_rows: 0,
            ..Metrics::default()
        };
        assert_eq!(m.content_height(), 23.0);
    }
}
