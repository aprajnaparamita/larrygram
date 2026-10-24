//! Theme tokens applied to egui (ADR 0063, `docs/ui/themes.md`).
//!
//! A widget asks for a token by name; a missing or malformed token falls
//! back to an egui default and warns once, so a theme file can never
//! crash the app. [`apply`] maps the tokens listed in `docs/ui/themes.md`
//! onto egui's [`egui::Style`] and [`egui::Visuals`], registers the bundled Selawik
//! faces as the proportional UI font and records the active theme for
//! the canvas.
//!
//! Switching layout calls [`apply`] with the new theme; the next frame
//! reads the new colours.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{
    Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};
use larrygram_ui_spec::v2::{Metric, Theme};

use crate::UiError;

/// The Selawik Regular face (OFL, `crates/text/fonts`).
const SELAWIK_REGULAR: &[u8] = include_bytes!("../../text/fonts/selawk.ttf");

/// The Selawik Light face (OFL, `crates/text/fonts`).
const SELAWIK_LIGHT: &[u8] = include_bytes!("../../text/fonts/selawkl.ttf");

/// The Selawik Semibold face (OFL, `crates/text/fonts`).
const SELAWIK_SEMIBOLD: &[u8] = include_bytes!("../../text/fonts/selawksb.ttf");

/// The fallback body size in logical pixels, matching the 9 pt UI text
/// at 96 DPI (`docs/ui/themes.md`).
const FALLBACK_FONT_SIZE: f32 = 12.0;

/// Applies `theme` to egui's `Style` and `Visuals` (ADR 0063).
///
/// The mapping is the one in `docs/ui/themes.md`: chrome fills and text,
/// the hovered and pressed button states, menus and popups, text-field
/// backgrounds, and the theme's named text styles. Selawik (Regular,
/// Light and Semibold) is the proportional UI font. The theme is also
/// recorded for [`current`], which the canvas reads.
///
/// A missing token falls back to an egui default; this function never
/// fails and never panics on theme data.
pub fn apply(ctx: &Context, theme: &Theme) -> Result<(), UiError> {
    ctx.set_fonts(font_definitions());
    ctx.set_theme(egui::Theme::Light);
    let visuals = visuals(theme);
    let text_styles = text_styles(theme);
    ctx.global_style_mut(|style| {
        style.visuals = visuals;
        style.text_styles = text_styles;
    });
    ctx.data_mut(|data| data.insert_temp(active_theme_id(), theme.clone()));
    Ok(())
}

/// The theme applied to `ctx` most recently, when one was applied.
///
/// The canvas uses it to read its background, page and ruler colours.
pub fn current(ctx: &Context) -> Option<Theme> {
    ctx.data(|data| data.get_temp(active_theme_id()))
}

/// The identity of the active-theme slot in egui's memory.
fn active_theme_id() -> egui::Id {
    egui::Id::unique("larrygram.theme.active")
}

/// The colour for `token`, with an egui-default fallback.
///
/// A missing token, or a value that is not `#rrggbb`/`#rrggbbaa`,
/// falls back and prints one warning for that token.
pub fn color(theme: &Theme, token: &str) -> Color32 {
    match theme.colors.get(token).and_then(|value| parse_color(value)) {
        Some(color) => color,
        None => {
            warn_once(theme, token);
            fallback_color(token)
        }
    }
}

/// The scalar metric for `token`, with a zero fallback.
///
/// A token that is missing, not a [`Metric::Px`], or not finite falls
/// back and prints one warning for that token. A [`Metric::Fields`]
/// token has no single scalar; use [`metric_field`].
pub fn metric(theme: &Theme, token: &str) -> f64 {
    match theme.metrics.get(token) {
        Some(Metric::Px(px)) if px.is_finite() => *px,
        Some(Metric::Px(_) | Metric::Fields(_)) | None => {
            warn_once(theme, token);
            0.0
        }
    }
}

/// One named field of a [`Metric::Fields`] token, with a zero fallback.
///
/// A missing token or field, a scalar token asked for a field, or a
/// non-finite value falls back and prints one warning for that token and
/// field.
pub fn metric_field(theme: &Theme, token: &str, field: &str) -> f64 {
    let value = match theme.metrics.get(token) {
        Some(Metric::Fields(fields)) => fields.get(field).copied(),
        Some(Metric::Px(_)) | None => None,
    };
    match value {
        Some(px) if px.is_finite() => px,
        _ => {
            warn_once(theme, &format!("{token}.{field}"));
            0.0
        }
    }
}

/// Builds the egui font list with Selawik as the proportional UI font.
fn font_definitions() -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    let faces = [
        ("selawik-regular", SELAWIK_REGULAR),
        ("selawik-light", SELAWIK_LIGHT),
        ("selawik-semibold", SELAWIK_SEMIBOLD),
    ];
    for (name, bytes) in faces {
        definitions
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    let mut proportional = vec![
        "selawik-regular".to_owned(),
        "selawik-light".to_owned(),
        "selawik-semibold".to_owned(),
    ];
    if let Some(fallback) = definitions.families.get(&FontFamily::Proportional) {
        proportional.extend(fallback.iter().cloned());
    }
    definitions
        .families
        .insert(FontFamily::Proportional, proportional);

    for (family, face) in [
        ("Selawik", "selawik-regular"),
        ("Selawik Light", "selawik-light"),
        ("Selawik Semibold", "selawik-semibold"),
    ] {
        definitions.families.insert(
            FontFamily::Name(family.into()),
            vec![face.to_owned(), "selawik-regular".to_owned()],
        );
    }
    definitions
}

/// The egui family for a theme font family.
///
/// Selawik and unknown families use the proportional font; the Light and
/// Semibold faces have their own named families for the pane title.
fn font_family(family: &str) -> FontFamily {
    match family {
        "Selawik Light" => FontFamily::Name("Selawik Light".into()),
        "Selawik Semibold" => FontFamily::Name("Selawik Semibold".into()),
        _ => FontFamily::Proportional,
    }
}

/// The egui text styles for the theme's font roles.
fn text_styles(theme: &Theme) -> BTreeMap<TextStyle, FontId> {
    let ui = theme.fonts.get("ui");
    let size = ui.map_or(FALLBACK_FONT_SIZE, |spec| font_size(spec.size_px));
    let family = ui.map_or(FontFamily::Proportional, |spec| font_family(&spec.family));
    let heading = theme
        .fonts
        .get("title")
        .or_else(|| theme.fonts.get("pane_title"))
        .or(ui);
    let heading_size = heading.map_or(size * 1.5, |spec| font_size(spec.size_px));
    let heading_family = heading.map_or_else(|| family.clone(), |spec| font_family(&spec.family));

    let mut styles = BTreeMap::new();
    styles.insert(
        TextStyle::Small,
        FontId::new((size - 2.0).max(1.0), family.clone()),
    );
    styles.insert(TextStyle::Body, FontId::new(size, family.clone()));
    styles.insert(TextStyle::Button, FontId::new(size, family.clone()));
    styles.insert(TextStyle::Monospace, FontId::monospace(size));
    styles.insert(
        TextStyle::Heading,
        FontId::new(heading_size, heading_family),
    );
    for (role, spec) in &theme.fonts {
        styles.insert(
            TextStyle::Name(role.clone().into()),
            FontId::new(font_size(spec.size_px), font_family(&spec.family)),
        );
    }
    styles
}

/// A finite font size in logical pixels, capped so malformed theme data
/// cannot make egui allocate without bound.
fn font_size(size_px: f64) -> f32 {
    if size_px.is_finite() {
        size_px.clamp(1.0, 256.0) as f32
    } else {
        FALLBACK_FONT_SIZE
    }
}

/// The egui visuals for the theme tokens mapped in `docs/ui/themes.md`.
fn visuals(theme: &Theme) -> Visuals {
    let mut visuals = Visuals::light();
    visuals.panel_fill = color_or(
        theme,
        &["chrome.background"],
        fallback_color("chrome.background"),
    );
    visuals.override_text_color = Some(color_or(
        theme,
        &["chrome.text"],
        fallback_color("chrome.text"),
    ));
    visuals.weak_text_color = Some(color_or(
        theme,
        &["chrome.text_disabled"],
        fallback_color("chrome.text_disabled"),
    ));

    visuals.widgets.hovered.weak_bg_fill = color_or(
        theme,
        &["button.hover.fill"],
        fallback_color("button.hover.fill"),
    );
    visuals.widgets.hovered.bg_stroke = Stroke::new(
        1.0,
        color_or(
            theme,
            &["button.hover.border"],
            fallback_color("button.hover.border"),
        ),
    );
    let pressed = color_or(
        theme,
        &["button.pressed.fill"],
        fallback_color("button.pressed.fill"),
    );
    visuals.widgets.active.weak_bg_fill = pressed;
    visuals.widgets.active.bg_fill = pressed;
    visuals.widgets.active.bg_stroke = Stroke::new(
        1.0,
        color_or(
            theme,
            &["button.pressed.border"],
            fallback_color("button.pressed.border"),
        ),
    );
    visuals.selection.bg_fill = pressed;

    visuals.window_fill = color_or(
        theme,
        &["menu.background", "tooltip.background", "chrome.background"],
        fallback_color("menu.background"),
    );
    visuals.window_stroke = Stroke::new(
        1.0,
        color_or(
            theme,
            &["menu.border", "tooltip.border", "chrome.border"],
            fallback_color("menu.border"),
        ),
    );
    visuals.extreme_bg_color = color_or(
        theme,
        &["field.background", "page.background"],
        Color32::WHITE,
    );
    visuals
}

/// The first present token in `tokens`, else `fallback` with one warning.
fn color_or(theme: &Theme, tokens: &[&str], fallback: Color32) -> Color32 {
    for token in tokens {
        if let Some(color) = theme
            .colors
            .get(*token)
            .and_then(|value| parse_color(value))
        {
            return color;
        }
    }
    if let Some(token) = tokens.first() {
        warn_once(theme, token);
    }
    fallback
}

/// Parses `#rrggbb`, `#rrggbbaa` or the short `#rgb`/`#rgba` spellings.
fn parse_color(value: &str) -> Option<Color32> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    match hex.len() {
        3 | 4 => {
            let mut channels = [0_u8; 4];
            for (index, digit) in hex.chars().enumerate() {
                let nibble = digit.to_digit(16)?;
                let byte = u8::try_from(nibble * 17).ok()?;
                *channels.get_mut(index)? = byte;
            }
            let alpha = if hex.len() == 4 { channels[3] } else { 255 };
            Some(Color32::from_rgba_unmultiplied(
                channels[0],
                channels[1],
                channels[2],
                alpha,
            ))
        }
        6 | 8 => {
            let alpha = if hex.len() == 8 {
                u8::from_str_radix(hex.get(6..8)?, 16).ok()?
            } else {
                255
            };
            Some(Color32::from_rgba_unmultiplied(
                u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
                u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
                u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
                alpha,
            ))
        }
        _ => None,
    }
}

/// The egui-light fallback for a known token, or mid grey for an unknown
/// one.
fn fallback_color(token: &str) -> Color32 {
    match token {
        "chrome.background" => Color32::from_gray(248),
        "chrome.text" => Color32::from_gray(80),
        "chrome.text_disabled" => Color32::from_gray(140),
        "chrome.border" => Color32::from_gray(190),
        "button.hover.fill" => Color32::from_gray(230),
        "button.hover.border" => Color32::TRANSPARENT,
        "button.pressed.fill" => Color32::from_gray(210),
        "button.pressed.border" => Color32::from_gray(190),
        "menu.background" | "tooltip.background" => Color32::from_gray(248),
        "menu.border" | "tooltip.border" => Color32::from_gray(190),
        "field.background" | "page.background" => Color32::WHITE,
        "canvas.background" => Color32::from_rgb(204, 204, 204),
        "page.border" => Color32::from_rgb(169, 176, 184),
        "page.shadow" => Color32::from_rgb(234, 234, 234),
        "ruler.background" => Color32::from_rgb(232, 232, 232),
        "ruler.ticks" => Color32::from_rgb(120, 120, 120),
        _ => Color32::GRAY,
    }
}

/// The tokens that already warned, so a missing token warns once.
fn warnings() -> &'static Mutex<BTreeSet<String>> {
    static WARNINGS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    WARNINGS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Prints one warning per token (and remembers it for the tests).
fn warn_once(theme: &Theme, token: &str) {
    let mut warnings = warnings()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if warnings.insert(token.to_owned()) {
        eprintln!(
            "warning: theme {:?} has no usable token {token:?}; using a fallback",
            theme.id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_ui_spec::v2::FontSpec;

    /// A theme with the mapped tokens and one font role.
    fn theme(id: &str, background: &str) -> Theme {
        let mut colors = BTreeMap::new();
        for (token, value) in [
            ("chrome.background", background),
            ("chrome.text", "#112233"),
            ("chrome.text_disabled", "#445566"),
            ("chrome.border", "#778899"),
            ("button.hover.fill", "#aabbcc"),
            ("button.hover.border", "#bbccdd"),
            ("button.pressed.fill", "#ccddee"),
            ("button.pressed.border", "#ddeeff"),
            ("menu.background", "#f0f1f2"),
            ("menu.border", "#010203"),
            ("field.background", "#fafbfc"),
            ("canvas.background", "#101112"),
            ("page.background", "#ffffff"),
            ("page.border", "#202122"),
            ("page.shadow", "#303132"),
            ("ruler.background", "#404142"),
            ("ruler.ticks", "#505152"),
        ] {
            colors.insert(token.to_owned(), value.to_owned());
        }
        let mut fonts = BTreeMap::new();
        fonts.insert(
            "ui".to_owned(),
            FontSpec {
                family: "Selawik".to_owned(),
                size_px: 13.0,
                transform: None,
            },
        );
        fonts.insert(
            "pane_title".to_owned(),
            FontSpec {
                family: "Selawik Light".to_owned(),
                size_px: 22.0,
                transform: None,
            },
        );
        let mut metrics = BTreeMap::new();
        metrics.insert("ruler.size".to_owned(), Metric::Px(16.0));
        metrics.insert(
            "button.large".to_owned(),
            Metric::Fields(BTreeMap::from([
                ("width".to_owned(), 42.0),
                ("height".to_owned(), 69.0),
            ])),
        );
        Theme {
            schema_version: 1,
            id: id.to_owned(),
            name: id.to_owned(),
            fonts,
            colors,
            metrics,
            contextual_tab_sets: BTreeMap::new(),
        }
    }

    #[test]
    fn both_themes_apply_and_the_visuals_read_back() {
        let ctx = Context::default();
        let modern = theme("panel-modern", "#fbfbfb");
        apply(&ctx, &modern).expect("the modern theme applies");
        let visuals = ctx.global_style().visuals.clone();
        assert_eq!(visuals.panel_fill, Color32::from_rgb(251, 251, 251));
        assert_eq!(
            visuals.override_text_color,
            Some(Color32::from_rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            visuals.weak_text_color,
            Some(Color32::from_rgb(0x44, 0x55, 0x66))
        );
        assert_eq!(
            visuals.widgets.hovered.weak_bg_fill,
            Color32::from_rgb(0xaa, 0xbb, 0xcc)
        );
        assert_eq!(
            visuals.widgets.hovered.bg_stroke.color,
            Color32::from_rgb(0xbb, 0xcc, 0xdd)
        );
        assert_eq!(
            visuals.widgets.active.weak_bg_fill,
            Color32::from_rgb(0xcc, 0xdd, 0xee)
        );
        assert_eq!(
            visuals.widgets.active.bg_stroke.color,
            Color32::from_rgb(0xdd, 0xee, 0xff)
        );
        assert_eq!(visuals.window_fill, Color32::from_rgb(0xf0, 0xf1, 0xf2));
        assert_eq!(
            visuals.window_stroke,
            Stroke::new(1.0, Color32::from_rgb(0x01, 0x02, 0x03))
        );
        assert_eq!(
            visuals.extreme_bg_color,
            Color32::from_rgb(0xfa, 0xfb, 0xfc)
        );
        assert_eq!(
            ctx.global_style()
                .text_styles
                .get(&TextStyle::Body)
                .cloned(),
            Some(FontId::new(13.0, FontFamily::Proportional))
        );
        assert_eq!(
            ctx.global_style()
                .text_styles
                .get(&TextStyle::Name("pane_title".into()))
                .cloned(),
            Some(FontId::new(22.0, FontFamily::Name("Selawik Light".into())))
        );

        let classic = theme("panel-classic", "#bfdbff");
        apply(&ctx, &classic).expect("the classic theme applies");
        let visuals = ctx.global_style().visuals.clone();
        assert_eq!(visuals.panel_fill, Color32::from_rgb(0xbf, 0xdb, 0xff));
        assert_eq!(
            current(&ctx).as_ref().map(|theme| theme.id.as_str()),
            Some("panel-classic")
        );
    }

    #[test]
    fn font_definitions_register_the_three_selawik_faces() {
        let definitions = font_definitions();
        for face in ["selawik-regular", "selawik-light", "selawik-semibold"] {
            assert!(definitions.font_data.contains_key(face), "{face}");
        }
        let proportional = definitions
            .families
            .get(&FontFamily::Proportional)
            .expect("the proportional family exists");
        for face in ["selawik-regular", "selawik-light", "selawik-semibold"] {
            assert!(proportional.iter().any(|name| name == face), "{face}");
        }
        for family in ["Selawik", "Selawik Light", "Selawik Semibold"] {
            assert!(
                definitions
                    .families
                    .contains_key(&FontFamily::Name(family.into())),
                "{family}"
            );
        }
        assert_eq!(
            font_family("Selawik Semibold"),
            FontFamily::Name("Selawik Semibold".into())
        );
        assert_eq!(font_family("Carlito"), FontFamily::Proportional);
    }

    #[test]
    fn apply_survives_a_theme_with_no_tokens() {
        let ctx = Context::default();
        let bare = Theme {
            schema_version: 1,
            id: "bare".to_owned(),
            name: "Bare".to_owned(),
            fonts: BTreeMap::new(),
            colors: BTreeMap::new(),
            metrics: BTreeMap::new(),
            contextual_tab_sets: BTreeMap::new(),
        };
        apply(&ctx, &bare).expect("an empty theme still applies");
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            fallback_color("chrome.background")
        );
        assert_eq!(
            ctx.global_style()
                .text_styles
                .get(&TextStyle::Body)
                .map(|font| font.size),
            Some(FALLBACK_FONT_SIZE)
        );
    }

    #[test]
    fn a_missing_colour_token_falls_back_and_warns_once() {
        let bare = theme("no-canvas", "#ffffff");
        let token = "test.missing.colour";
        assert_eq!(color(&bare, token), Color32::GRAY);
        assert_eq!(color(&bare, token), Color32::GRAY);
        assert!(
            warnings()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(token)
        );
    }

    #[test]
    fn a_malformed_colour_falls_back() {
        let mut bare = theme("bad-colour", "#ffffff");
        bare.colors
            .insert("canvas.background".to_owned(), "not-a-colour".to_owned());
        assert_eq!(
            color(&bare, "canvas.background"),
            fallback_color("canvas.background")
        );
        assert_eq!(
            color(&bare, "canvas.background"),
            Color32::from_rgb(204, 204, 204)
        );
    }

    #[test]
    fn colour_spellings_parse() {
        assert_eq!(
            parse_color("#abc"),
            Some(Color32::from_rgb(0xaa, 0xbb, 0xcc))
        );
        assert_eq!(
            parse_color("#abcd"),
            Some(Color32::from_rgba_unmultiplied(0xaa, 0xbb, 0xcc, 0xdd))
        );
        assert_eq!(
            parse_color("  #112233  "),
            Some(Color32::from_rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            parse_color("#11223344"),
            Some(Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0x44))
        );
        assert_eq!(
            parse_color("112233"),
            Some(Color32::from_rgb(0x11, 0x22, 0x33))
        );
        for bad in ["", "#12", "#12345", "#gggggg", "#1234567"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn metrics_read_scalars_and_fields() {
        let theme = theme("metrics", "#ffffff");
        assert_eq!(metric(&theme, "ruler.size"), 16.0);
        assert_eq!(metric_field(&theme, "button.large", "height"), 69.0);
        assert_eq!(metric_field(&theme, "button.large", "missing"), 0.0);
        assert_eq!(metric(&theme, "button.large"), 0.0);
        assert_eq!(metric(&theme, "missing.metric"), 0.0);
        assert_eq!(metric_field(&theme, "ruler.size", "width"), 0.0);
        assert_eq!(metric_field(&theme, "missing.metric", "width"), 0.0);
    }

    #[test]
    fn a_non_finite_metric_falls_back() {
        let mut theme = theme("nan", "#ffffff");
        theme
            .metrics
            .insert("ruler.size".to_owned(), Metric::Px(f64::NAN));
        assert_eq!(metric(&theme, "ruler.size"), 0.0);
        theme
            .metrics
            .insert("ruler.size".to_owned(), Metric::Px(f64::INFINITY));
        assert_eq!(metric(&theme, "ruler.size"), 0.0);
        theme.metrics.insert(
            "button.large".to_owned(),
            Metric::Fields(BTreeMap::from([("width".to_owned(), f64::NEG_INFINITY)])),
        );
        assert_eq!(metric_field(&theme, "button.large", "width"), 0.0);
    }

    #[test]
    fn font_sizes_are_finite_and_capped() {
        assert_eq!(font_size(12.5), 12.5);
        assert_eq!(font_size(0.0), 1.0);
        assert_eq!(font_size(-5.0), 1.0);
        assert_eq!(font_size(1_000.0), 256.0);
        assert_eq!(font_size(f64::NAN), FALLBACK_FONT_SIZE);
        assert_eq!(font_size(f64::INFINITY), FALLBACK_FONT_SIZE);
    }
}
