//! Sizes of the Panel Classic widgets, in logical pixels (ADR 0063).
//!
//! The defaults are the values of `ui-spec/themes/panel-classic.yaml`;
//! [`ClassicMetrics::from_theme`] reads them from any [`Theme`] so the
//! look can be tuned from the theme file without touching widget code
//! (ADR 0063: "a widget asks for a token by name"). A token that is
//! missing, non-finite or non-positive keeps its default: a theme file
//! can never crash the app (AGENTS.md section 6). Colours are not read
//! here; the widgets ask [`crate::theme::color_at`] for those.

use egui::Vec2;
use larrygram_ui_spec::v2::{Metric, Theme};

/// Widget sizes of one Panel Classic chrome, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassicMetrics {
    /// Height of the window title strip (`title_bar.height`).
    pub title_bar_height: f32,
    /// Height of the menu bar (`menu_bar.height`).
    pub menu_bar_height: f32,
    /// Side padding of a menu-bar title (`menu_bar.item.padding_x`).
    pub menu_bar_item_padding_x: f32,
    /// Height of one docked toolbar row (`toolbar.row.height`).
    pub toolbar_row_height: f32,
    /// Size of a toolbar button (`toolbar.button`).
    pub toolbar_button: Vec2,
    /// Width of a toolbar split button's arrow part
    /// (`toolbar.split_arrow.width`).
    pub toolbar_split_arrow_width: f32,
    /// Width of the line between toolbar groups (`toolbar.separator`).
    pub toolbar_separator_width: f32,
    /// Margin around that line (`toolbar.separator`).
    pub toolbar_separator_margin: f32,
    /// Width of a docked toolbar's gripper (`toolbar.gripper.width`).
    pub toolbar_gripper_width: f32,
    /// Width of the Font combo (`combo.font.width`).
    pub combo_font_width: f32,
    /// Width of the Font Size combo (`combo.font_size.width`).
    pub combo_font_size_width: f32,
    /// Width of the Zoom combo (`combo.zoom.width`).
    pub combo_zoom_width: f32,
    /// Height of one menu row (`menu.row.height`).
    pub menu_row_height: f32,
    /// Height of the separator block between menu rows
    /// (`menu.separator.height`).
    pub menu_separator_height: f32,
    /// Width of the menu icon gutter (`menu.gutter.width`).
    pub menu_gutter_width: f32,
    /// Menu icon size (`menu.icon`).
    pub menu_icon: f32,
    /// Side padding inside a menu (`menu.padding_x`).
    pub menu_padding_x: f32,
    /// Smallest gap between a row's label and its shortcut
    /// (`menu.shortcut.gap`).
    pub menu_shortcut_gap: f32,
    /// Width reserved for the submenu arrow (`menu.submenu_arrow.width`).
    pub menu_submenu_arrow_width: f32,
    /// Width of the Shapes window (`shapes_window.width`).
    pub shapes_window_width: f32,
    /// Width of the ruler bands (`ruler.size`).
    pub ruler_size: f32,
    /// Height of the page-tab strip (`page_tabs.height`).
    pub page_tabs_height: f32,
    /// Height of the status bar (`status_bar.height`).
    pub status_bar_height: f32,
    /// Small icon size (`icon.small`).
    pub icon_small: f32,
}

impl Default for ClassicMetrics {
    /// The metrics of `ui-spec/themes/panel-classic.yaml`.
    fn default() -> Self {
        Self {
            title_bar_height: 23.0,
            menu_bar_height: 24.0,
            menu_bar_item_padding_x: 7.0,
            toolbar_row_height: 26.0,
            toolbar_button: Vec2::new(23.0, 22.0),
            toolbar_split_arrow_width: 11.0,
            toolbar_separator_width: 1.0,
            toolbar_separator_margin: 3.0,
            toolbar_gripper_width: 7.0,
            combo_font_width: 144.0,
            combo_font_size_width: 86.0,
            combo_zoom_width: 65.0,
            menu_row_height: 23.0,
            menu_separator_height: 3.0,
            menu_gutter_width: 24.0,
            menu_icon: 16.0,
            menu_padding_x: 8.0,
            menu_shortcut_gap: 24.0,
            menu_submenu_arrow_width: 12.0,
            shapes_window_width: 190.0,
            ruler_size: 16.0,
            page_tabs_height: 18.0,
            status_bar_height: 20.0,
            icon_small: 16.0,
        }
    }
}

impl ClassicMetrics {
    /// The metrics of `theme`, with the defaults for every token the
    /// theme does not carry (or carries unusably).
    pub fn from_theme(theme: &Theme) -> Self {
        let mut m = Self::default();
        let t = ThemeTokens { theme };
        m.title_bar_height = t.get("title_bar.height", m.title_bar_height);
        m.menu_bar_height = t.get("menu_bar.height", m.menu_bar_height);
        m.menu_bar_item_padding_x = t.get("menu_bar.item.padding_x", m.menu_bar_item_padding_x);
        m.toolbar_row_height = t.get("toolbar.row.height", m.toolbar_row_height);
        m.toolbar_button = Vec2::new(
            t.field("toolbar.button", "width", m.toolbar_button.x),
            t.field("toolbar.button", "height", m.toolbar_button.y),
        );
        m.toolbar_split_arrow_width =
            t.get("toolbar.split_arrow.width", m.toolbar_split_arrow_width);
        m.toolbar_separator_width =
            t.field("toolbar.separator", "width", m.toolbar_separator_width);
        m.toolbar_separator_margin =
            t.field("toolbar.separator", "margin", m.toolbar_separator_margin);
        m.toolbar_gripper_width = t.get("toolbar.gripper.width", m.toolbar_gripper_width);
        m.combo_font_width = t.get("combo.font.width", m.combo_font_width);
        m.combo_font_size_width = t.get("combo.font_size.width", m.combo_font_size_width);
        m.combo_zoom_width = t.get("combo.zoom.width", m.combo_zoom_width);
        m.menu_row_height = t.get("menu.row.height", m.menu_row_height);
        m.menu_separator_height = t.get("menu.separator.height", m.menu_separator_height);
        m.menu_gutter_width = t.get("menu.gutter.width", m.menu_gutter_width);
        m.menu_icon = t.get("menu.icon", m.menu_icon);
        m.menu_padding_x = t.get("menu.padding_x", m.menu_padding_x);
        m.menu_shortcut_gap = t.get("menu.shortcut.gap", m.menu_shortcut_gap);
        m.menu_submenu_arrow_width = t.get("menu.submenu_arrow.width", m.menu_submenu_arrow_width);
        m.shapes_window_width = t.get("shapes_window.width", m.shapes_window_width);
        m.ruler_size = t.get("ruler.size", m.ruler_size);
        m.page_tabs_height = t.get("page_tabs.height", m.page_tabs_height);
        m.status_bar_height = t.get("status_bar.height", m.status_bar_height);
        m.icon_small = t.get("icon.small", m.icon_small);
        m
    }

    /// The metrics of the theme applied to `ctx`, or the defaults
    /// when no theme was applied yet.
    pub fn for_ctx(ctx: &egui::Context) -> Self {
        match crate::theme::current(ctx) {
            Some(theme) => Self::from_theme(&theme),
            None => Self::default(),
        }
    }
}

/// A theme's metric table, read with fallbacks (the Panel Modern
/// `Metrics` rules, kept identical on purpose).
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
        metrics.insert("menu.row.height".to_owned(), Metric::Px(30.0));
        metrics.insert("toolbar.row.height".to_owned(), Metric::Px(-4.0));
        metrics.insert(
            "toolbar.button".to_owned(),
            Metric::Fields(BTreeMap::from([
                ("width".to_owned(), 40.0),
                ("height".to_owned(), 50.0),
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
        let m = ClassicMetrics::default();
        assert_eq!(m.title_bar_height, 23.0);
        assert_eq!(m.menu_bar_height, 24.0);
        assert_eq!(m.menu_row_height, 23.0);
        assert_eq!(m.menu_gutter_width, 24.0);
        assert_eq!(m.menu_icon, 16.0);
        assert_eq!(m.toolbar_button, Vec2::new(23.0, 22.0));
        assert_eq!(m.shapes_window_width, 190.0);
        assert_eq!(m.status_bar_height, 20.0);
    }

    #[test]
    fn from_theme_reads_tokens_and_keeps_defaults() {
        let m = ClassicMetrics::from_theme(&theme());
        // Read from the theme.
        assert_eq!(m.menu_row_height, 30.0);
        assert_eq!(m.toolbar_button, Vec2::new(40.0, 50.0));
        // Fields the theme does not carry keep their default.
        assert_eq!(m.menu_gutter_width, 24.0);
        // A non-positive token is unusable and keeps its default.
        assert_eq!(m.toolbar_row_height, 26.0);
    }

    #[test]
    fn for_ctx_falls_back_without_a_theme() {
        let ctx = egui::Context::default();
        assert_eq!(ClassicMetrics::for_ctx(&ctx), ClassicMetrics::default());
    }

    #[test]
    fn unusable_values_keep_the_default() {
        assert_eq!(usable(None, 7.0), 7.0);
        assert_eq!(usable(Some(f32::NAN), 7.0), 7.0);
        assert_eq!(usable(Some(0.0), 7.0), 7.0);
        assert_eq!(usable(Some(9.0), 7.0), 9.0);
    }
}
