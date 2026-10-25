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
    ctx.data_mut(|data| data.insert_temp(active_theme_id(), Arc::new(theme.clone())));
    Ok(())
}

/// The theme applied to `ctx` most recently, when one was applied.
///
/// The widgets use it to read their tokens by name (ADR 0063); the
/// handle is shared, so a widget asking for a token clones no maps.
pub fn current(ctx: &Context) -> Option<Arc<Theme>> {
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

    let small = theme.fonts.get("caption").map_or_else(
        || FontId::new((size - 2.0).max(1.0), family.clone()),
        |spec| FontId::new(font_size(spec.size_px), font_family(&spec.family)),
    );

    let mut styles = BTreeMap::new();
    styles.insert(TextStyle::Small, small);
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

/// The egui visuals for the theme tokens mapped in `docs/ui/themes.md`
/// (the design language of `docs/ui/design.md`).
///
/// Buttons are flat at rest: they take a fill only under the pointer
/// (`button.hover.*`), while held down (`button.active.fill`) or when
/// selected (`button.pressed.*`, the selected tint). The surfaces that
/// need standard framed buttons (dialogs, the wizard) wrap their
/// controls in [`raise_controls`].
fn visuals(theme: &Theme) -> Visuals {
    let token = |name: &str| color(theme, name);
    let radius = |name: &str| corner_radius(metric_or(theme, name, 0.0));
    let text = token("chrome.text");
    let control = radius("radius.control");

    let mut visuals = Visuals::light();
    visuals.panel_fill = token("chrome.background");
    visuals.faint_bg_color = token("chrome.background");
    visuals.code_bg_color = token("chrome.background");
    visuals.extreme_bg_color = token("field.background");
    visuals.override_text_color = Some(text);
    visuals.weak_text_color = Some(token("chrome.text_muted"));
    visuals.hyperlink_color = token("accent");
    visuals.warn_fg_color = token("icon.orange");
    visuals.error_fg_color = token("icon.red");
    visuals.text_cursor.stroke = Stroke::new(2.0, token("accent"));
    visuals.selection.bg_fill = token("button.pressed.fill");
    visuals.selection.stroke = Stroke::new(1.0, token("accent"));

    visuals.window_fill = token("menu.background");
    visuals.window_stroke = Stroke::new(1.0, token("menu.border"));
    visuals.window_corner_radius = radius("radius.popup");
    visuals.menu_corner_radius = radius("radius.popup");
    visuals.window_shadow.color = token("shadow");
    visuals.popup_shadow.color = token("shadow");

    let widgets = &mut visuals.widgets;
    let fg = Stroke::new(1.0, text);
    widgets.noninteractive.bg_fill = token("chrome.background");
    widgets.noninteractive.weak_bg_fill = token("chrome.background");
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, token("chrome.border"));
    widgets.noninteractive.fg_stroke = fg;

    widgets.inactive.weak_bg_fill = token("button.fill");
    // The box of a check box or radio button at rest: a neutral grey,
    // so it shows on any surface; `raise_controls` gives it the field
    // fill and a `control.border` edge instead.
    widgets.inactive.bg_fill = token("button.active.fill");
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke = fg;

    widgets.hovered.weak_bg_fill = token("button.hover.fill");
    widgets.hovered.bg_fill = token("button.hover.fill");
    widgets.hovered.bg_stroke = Stroke::new(1.0, token("button.hover.border"));
    widgets.hovered.fg_stroke = fg;

    widgets.active.weak_bg_fill = token("button.active.fill");
    widgets.active.bg_fill = token("button.active.fill");
    widgets.active.bg_stroke =
        Stroke::new(metric_or(theme, "focus.width", 2.0) as f32, token("focus"));
    widgets.active.fg_stroke = fg;

    widgets.open.weak_bg_fill = token("button.active.fill");
    widgets.open.bg_fill = token("menu.background");
    // An open combo or menu button shows the selected edge.
    widgets.open.bg_stroke = Stroke::new(1.0, token("button.pressed.border"));
    widgets.open.fg_stroke = fg;

    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.corner_radius = control;
        state.expansion = 0.0;
    }
    visuals
}

/// Gives the controls drawn inside `ui` the standard frame of
/// `docs/ui/design.md`: a raised fill (`button.raised.fill`) and a
/// visible edge (`control.border`) at rest, for dialogs, the wizard and
/// the File view tiles. Check boxes and radio buttons get the same
/// edge. The rest of the app keeps flat buttons.
pub fn raise_controls(ui: &mut egui::Ui) {
    let edge = Stroke::new(1.0, color_at(ui.ctx(), "control.border"));
    let raised = color_at(ui.ctx(), "button.raised.fill");
    let field = color_at(ui.ctx(), "field.background");
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = raised;
    widgets.inactive.bg_fill = field;
    widgets.inactive.bg_stroke = edge;
    widgets.hovered.bg_stroke = edge;
}

/// Gives the buttons drawn inside `ui` the accent fill of a default
/// button (the File button, the wizard's Next and Start): `accent` at
/// rest, `accent.hover` under the pointer, `accent.pressed` held down,
/// with `accent.text` labels.
pub fn accent_controls(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let label = color_at(&ctx, "accent.text");
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(label);
    for (state, token) in [
        (&mut visuals.widgets.inactive, "accent"),
        (&mut visuals.widgets.hovered, "accent.hover"),
        (&mut visuals.widgets.active, "accent.pressed"),
        (&mut visuals.widgets.open, "accent.pressed"),
    ] {
        let fill = color_at(&ctx, token);
        state.weak_bg_fill = fill;
        state.bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, fill);
        state.fg_stroke = Stroke::new(1.0, label);
    }
}

/// The font of the theme's type-scale `role` on `ctx` (`ui`,
/// `caption`, `strong`, `title`, `display`; `docs/ui/design.md`
/// "Type"), or the body font when the theme lacks the role.
pub fn font_at(ctx: &Context, role: &str) -> FontId {
    let style = ctx.global_style();
    style
        .text_styles
        .get(&TextStyle::Name(role.into()))
        .or_else(|| style.text_styles.get(&TextStyle::Body))
        .cloned()
        .unwrap_or_else(|| FontId::proportional(FALLBACK_FONT_SIZE))
}

/// [`font_at`] for `role`, its size multiplied by `scale` (the wizard
/// and the About window grow their text with the window).
pub fn scaled_font(ctx: &Context, role: &str, scale: f32) -> FontId {
    let font = font_at(ctx, role);
    let scale = if scale.is_finite() {
        scale.clamp(0.1, 10.0)
    } else {
        1.0
    };
    FontId::new(font.size * scale, font.family)
}

/// The scalar metric `token` on `ctx` in logical pixels, or `fallback`
/// when no theme is applied or the theme lacks the token.
pub fn metric_at(ctx: &Context, token: &str, fallback: f32) -> f32 {
    match current(ctx) {
        Some(theme) => metric_or(&theme, token, f64::from(fallback)) as f32,
        None => fallback,
    }
}

/// The scalar metric `token`, or `fallback` (with one warning) when the
/// theme lacks it.
fn metric_or(theme: &Theme, token: &str, fallback: f64) -> f64 {
    match theme.metrics.get(token) {
        Some(Metric::Px(px)) if px.is_finite() => *px,
        _ => {
            warn_once(theme, token);
            fallback
        }
    }
}

/// An egui corner radius from a logical-pixel metric, clamped to the
/// `u8` egui stores.
fn corner_radius(px: f64) -> egui::CornerRadius {
    let px = if px.is_finite() {
        px.clamp(0.0, 255.0)
    } else {
        0.0
    };
    egui::CornerRadius::same(px.round() as u8)
}

/// The colour for `token` on `ctx`, with an egui-default fallback.
///
/// The widget seam of ADR 0063: a widget asks for a token by name and
/// paints what comes back. The theme is the one [`apply`] recorded for
/// `ctx`; with none yet the fallback draws (and warns once per token
/// when the theme lacks the name).
pub fn color_at(ctx: &Context, token: &str) -> Color32 {
    match current(ctx) {
        Some(theme) => color(&theme, token),
        None => fallback_color(token),
    }
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

/// The fallback for `token`: its value in the embedded base theme
/// (`ui-spec/themes/base.yaml`, the one palette), or mid grey for a
/// name the base does not define.
fn fallback_color(token: &str) -> Color32 {
    base_colors().get(token).copied().unwrap_or(Color32::GRAY)
}

/// The colours of the embedded base theme, parsed once. A base that
/// fails to decode leaves the map empty (every fallback is then grey).
fn base_colors() -> &'static BTreeMap<String, Color32> {
    static COLORS: OnceLock<BTreeMap<String, Color32>> = OnceLock::new();
    COLORS.get_or_init(|| {
        larrygram_ui_spec::v2::base_theme()
            .map(|base| {
                base.colors
                    .iter()
                    .filter_map(|(name, value)| Some((name.clone(), parse_color(value)?)))
                    .collect()
            })
            .unwrap_or_default()
    })
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
            ("chrome.text_muted", "#445566"),
            ("button.active.fill", "#ccddee"),
            ("focus", "#ddeeff"),
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
        // The fallback is the base theme's value (task P7-18).
        assert_eq!(
            color(&bare, "canvas.background"),
            Color32::from_rgb(0xe4, 0xe4, 0xe4)
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
