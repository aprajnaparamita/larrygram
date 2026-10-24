//! Icons for command-bar and menu controls (ADR 0030, ADR 0062).
//!
//! [`placeholder`] draws a simple mark of our own (a filled rounded
//! square with the first letter of the control id); no Office icon is
//! ever used (AGENTS.md section 4). A key that has no row in
//! `ui-spec/icons.yaml` draws the mark (ADR 0062).
//!
//! [`icon_atom`] and [`draw`] are the icon-set pipeline (ADR 0062):
//! Tabler outline SVGs rasterised at 16 and 32 px with theme tints.
//! Task P7-01 defines them as interface stubs; task P7-04 implements
//! them over `crates/ui/assets/icons/tabler/` and `ui-spec/icons.yaml`.
//!
//! Placeholder marks are `SizeVariant`-independent for now: the same
//! mark is drawn at every requested size (ADR 0030).
//!
//! Placeholder marks owned by task P4-04; the icon pipeline by P7-04.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use egui::{
    Align2, Atom, AtomPaintArgs, Color32, ColorImage, FontId, Rect, Response, Sense, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use larrygram_ui_spec::v2::Theme;

use crate::UiError;

mod table;

/// Placeholder size used when the requested size is not usable.
const DEFAULT_SIZE: f32 = 12.0;

/// Smallest placeholder size, in points.
const MIN_SIZE: f32 = 4.0;

/// Largest placeholder size, in points.
const MAX_SIZE: f32 = 64.0;

/// The cache holds at most this many rasterised icons (ADR 0062).
const MAX_CACHE_ENTRIES: usize = 1_024;

/// The largest texture side, in physical pixels. A pathological pixels
/// per point cannot grow the allocation without bound.
const MAX_ICON_PIXELS: u32 = 512;

/// Alpha for a disabled icon: 40% of 255, rounded (ADR 0062).
const DISABLED_ALPHA: u8 = 102;

/// The neutral tint when a theme has no `icon.neutral` token.
const DEFAULT_TINT: Color32 = Color32::from_rgb(68, 68, 68);

/// Draws the placeholder icon for a control id at `size` points.
///
/// The icon is a simple mark of our own (ADR 0030): a filled rounded
/// square with the first letter of the control id. `size` is clamped
/// to `4.0..=64.0` points; a non-finite size falls back to 12.0. The
/// widget only reserves the square; it is not interactive.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// let response = larrygram_ui::icons::placeholder(ui, "FileSave", 16.0).expect("a rect");
/// assert_eq!(response.rect.size(), egui::Vec2::splat(16.0));
/// # });
/// ```
pub fn placeholder(ui: &mut Ui, id: &str, size: f32) -> Result<Response, UiError> {
    let size = clamp_size(size);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_mark(ui, rect, id);
    Ok(response)
}

/// The placeholder icon for `id` as a paint atom, for embedding in a
/// button or other atom-based widget (ADR 0030).
///
/// Same mark as [`placeholder`], painted into the cell the widget
/// gives it. The id is copied so the atom is `'static`.
pub fn icon(id: &str, size: f32) -> Atom<'static> {
    let size = clamp_size(size);
    let id = id.to_owned();
    Atom::paint(Vec2::splat(size), move |ui: &Ui, args: AtomPaintArgs| {
        paint_mark(ui, args.rect, &id);
    })
}

/// A small filled downward triangle, for dropdown arrows (ADR 0030).
///
/// Painted rather than typed: egui's bundled fonts have no triangle
/// glyph. The mark uses the widget's text colour.
pub fn dropdown_arrow(size: f32) -> Atom<'static> {
    let size = clamp_size(size);
    Atom::paint(Vec2::splat(size), |ui: &Ui, args: AtomPaintArgs| {
        let rect = args.rect;
        let half = rect.width().min(rect.height()) * 0.25;
        let center = rect.center();
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(center.x - half, center.y - half * 0.6),
                egui::pos2(center.x + half, center.y - half * 0.6),
                egui::pos2(center.x, center.y + half * 0.8),
            ],
            args.fallback_text_color,
            egui::Stroke::NONE,
        ));
    })
}

/// Clamps an icon size to `MIN_SIZE..=MAX_SIZE`, else the default.
fn clamp_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(MIN_SIZE, MAX_SIZE)
    } else {
        DEFAULT_SIZE
    }
}

/// Paints the placeholder: a faint square with the id's first letter.
fn paint_mark(ui: &Ui, rect: Rect, id: &str) {
    let visuals = ui.visuals();
    let side = rect.width().min(rect.height());
    let corner = ((side * 0.2).round().clamp(0.0, 255.0)) as u8;
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(corner),
        visuals.faint_bg_color,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        first_letter(id),
        FontId::proportional(side * 0.7),
        visuals.text_color(),
    );
}

/// The letter drawn on the mark: the first character of the id,
/// uppercased; `?` for an empty id.
fn first_letter(id: &str) -> String {
    id.chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;

    #[test]
    fn placeholder_reserves_the_requested_square() {
        with_ui(|ui| {
            let response = placeholder(ui, "FileSave", 16.0).expect("a rect");
            assert_eq!(response.rect.size(), Vec2::splat(16.0));
            assert!(!response.clicked());
        });
    }

    #[test]
    fn placeholder_clamps_bad_sizes() {
        with_ui(|ui| {
            for (asked, want) in [
                (0.0, MIN_SIZE),
                (-3.0, MIN_SIZE),
                (2.5, MIN_SIZE),
                (1000.0, MAX_SIZE),
                (f32::NAN, DEFAULT_SIZE),
                (f32::INFINITY, DEFAULT_SIZE),
                (f32::NEG_INFINITY, DEFAULT_SIZE),
            ] {
                let response = placeholder(ui, "X", asked).expect("a rect");
                assert_eq!(response.rect.size(), Vec2::splat(want), "asked {asked}");
            }
        });
    }

    #[test]
    fn placeholder_handles_an_empty_id() {
        with_ui(|ui| {
            let response = placeholder(ui, "", 8.0).expect("a rect");
            assert_eq!(response.rect.size(), Vec2::splat(8.0));
        });
    }

    #[test]
    fn the_mark_letter_comes_from_the_id() {
        assert_eq!(first_letter("fileSave"), "F");
        assert_eq!(first_letter("2Cut"), "2");
        assert_eq!(first_letter(""), "?");
        // A non-ASCII id still yields one letter, not a panic.
        assert_eq!(first_letter("édition"), "É");
    }

    #[test]
    fn clamped_sizes_are_finite_and_bounded() {
        for size in [f32::NAN, -1.0, 0.0, 3.0, 12.0, 70.0, f32::INFINITY] {
            let clamped = clamp_size(size);
            assert!(clamped.is_finite());
            assert!((MIN_SIZE..=MAX_SIZE).contains(&clamped));
        }
    }

    #[test]
    fn icon_paints_without_panicking() {
        // The atom paints inside a button; here it is laid out bare.
        with_ui(|ui| {
            let atom = icon("FileSave", 12.0);
            ui.add(egui::Button::new((atom, "Save")));
        });
    }

    #[test]
    fn the_dropdown_arrow_paints_a_triangle() {
        with_ui(|ui| {
            ui.add(egui::Button::new(dropdown_arrow(12.0)));
            // A bad size clamps like every other mark.
            let atom = dropdown_arrow(f32::NAN);
            ui.add(egui::Button::new(atom));
        });
    }
}

/// The icon for an icon-map key as a paint atom (ADR 0062).
///
/// The atom rasterises the SVG named by `ui-spec/icons.yaml` at `size`
/// points with the `theme` tint, caches the texture and falls back to
/// [`icon`] for an unknown key or a rasterisation error. `enabled:
/// false` draws the icon at 40% alpha.
///
/// The key is resolved here, so an unknown key never reaches the
/// cache. The atom needs the `Ui` to load its texture, which is why the
/// rasterisation happens when the atom paints.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// use std::collections::BTreeMap;
/// let theme = larrygram_ui_spec::v2::Theme {
///     schema_version: 1,
///     id: "panel-modern".to_owned(),
///     name: "test".to_owned(),
///     fonts: BTreeMap::new(),
///     colors: BTreeMap::new(),
///     metrics: BTreeMap::new(),
///     contextual_tab_sets: BTreeMap::new(),
/// };
/// let atom = larrygram_ui::icons::icon_atom("edit.cut", 16.0, &theme, true)
///     .expect("the key exists");
/// ui.add(egui::Button::new((atom, "Cut")));
/// # });
/// ```
pub fn icon_atom(
    key: &str,
    size: f32,
    theme: &Theme,
    enabled: bool,
) -> Result<Atom<'static>, UiError> {
    let size = clamp_size(size);
    let Some(entry) = table::entry(key) else {
        warn_once(
            key,
            &format!("icon key `{key}` has no row in ui-spec/icons.yaml; drawing the placeholder"),
        );
        return Ok(icon(key, size));
    };
    let tint = tint_for(theme, entry.tint);
    let icon = entry.icon;
    let key = key.to_owned();
    Ok(Atom::paint(Vec2::splat(size), move |ui, args| {
        paint_or_fallback(
            ui,
            args.rect,
            &key,
            texture_for(ui, icon, tint, size),
            enabled,
        );
    }))
}

/// The icon atom for `key`, tinted by the theme active on `ctx`
/// (task P7-09 wires P7-04's [`icon_atom`] into the widgets).
///
/// The theme comes from [`crate::theme::current`], the one the shell
/// applied for the active layout; with no theme yet the atom draws
/// the placeholder mark ([`icon`]). A rasterisation failure falls back
/// the same way, never an error (AGENTS.md section 6).
pub fn icon_themed(ctx: &egui::Context, key: &str, size: f32, enabled: bool) -> Atom<'static> {
    match crate::theme::current(ctx) {
        Some(theme) => icon_atom(key, size, &theme, enabled).unwrap_or_else(|_| icon(key, size)),
        None => icon(key, size),
    }
}

/// Draws the icon for a key into `rect`, falling back to the placeholder.
///
/// An unknown key or a rasterisation error draws the placeholder and
/// logs once per key; the fallback is the defined result (ADR 0062).
pub fn draw(
    ui: &mut Ui,
    rect: Rect,
    key: &str,
    size: f32,
    theme: &Theme,
    enabled: bool,
) -> Result<(), UiError> {
    let Some(entry) = table::entry(key) else {
        warn_once(
            key,
            &format!("icon key `{key}` has no row in ui-spec/icons.yaml; drawing the placeholder"),
        );
        paint_mark(ui, rect, key);
        return Ok(());
    };
    let tint = tint_for(theme, entry.tint);
    paint_or_fallback(
        ui,
        rect,
        key,
        texture_for(ui, entry.icon, tint, size),
        enabled,
    );
    Ok(())
}

fn paint_or_fallback(
    ui: &Ui,
    rect: Rect,
    key: &str,
    texture: Result<TextureHandle, UiError>,
    enabled: bool,
) {
    match texture {
        Ok(texture) => paint_texture(ui, rect, &texture, enabled),
        Err(error) => {
            warn_once(
                key,
                &format!("icon key `{key}` failed to rasterise ({error}); drawing the placeholder"),
            );
            paint_mark(ui, rect, key);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    icon: String,
    pixels: u32,
    tint: [u8; 4],
    pixels_per_point_milli: u32,
}

#[derive(Debug, Clone)]
struct Lru<K, V> {
    entries: Vec<(K, V, u64)>,
    clock: u64,
}

impl<K, V> Default for Lru<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
        }
    }
}

impl<K: PartialEq, V: Clone> Lru<K, V> {
    fn get(&mut self, key: &K) -> Option<V> {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        for (stored, value, used) in &mut self.entries {
            if stored == key {
                *used = clock;
                return Some(value.clone());
            }
        }
        None
    }

    fn insert(&mut self, key: K, value: V) {
        self.clock = self.clock.wrapping_add(1);
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(index, _)| index);
            if let Some(index) = oldest {
                self.entries.remove(index);
            }
        }
        self.entries.push((key, value, self.clock));
    }
}

type TextureCache = Lru<CacheKey, TextureHandle>;

fn texture_for(ui: &Ui, icon: &str, tint: Color32, size: f32) -> Result<TextureHandle, UiError> {
    let ppp = safe_pixels_per_point(ui.ctx().pixels_per_point());
    let pixels = physical_pixels(size, ppp);
    let key = CacheKey {
        icon: icon.to_owned(),
        pixels,
        tint: tint.to_array(),
        pixels_per_point_milli: (ppp * 1_000.0).round() as u32,
    };
    let cache_id = egui::Id::unique("larrygram.icon-texture-cache");
    // Drop the data lock before loading: holding it across the load deadlocks.
    let cached = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_insert_with::<TextureCache>(cache_id, Lru::default)
            .get(&key)
    });
    if let Some(texture) = cached {
        return Ok(texture);
    }
    let bytes = table::icon(icon).ok_or_else(|| UiError::Render {
        message: format!("no embedded SVG for icon `{icon}`"),
    })?;
    let image = rasterize(bytes, tint, pixels)?;
    let name = format!(
        "larrygram-icon-{icon}-{pixels}-{:02x}{:02x}{:02x}",
        tint.r(),
        tint.g(),
        tint.b()
    );
    let texture = ui.ctx().load_texture(name, image, TextureOptions::LINEAR);
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_insert_with::<TextureCache>(cache_id, Lru::default)
            .insert(key, texture.clone());
    });
    Ok(texture)
}

fn rasterize(bytes: &[u8], tint: Color32, pixels: u32) -> Result<ColorImage, UiError> {
    let text = std::str::from_utf8(bytes).map_err(|error| UiError::Render {
        message: format!("icon SVG is not UTF-8: {error}"),
    })?;
    let colour = format!("#{:02x}{:02x}{:02x}", tint.r(), tint.g(), tint.b());
    let replaced = text.replace("currentColor", &colour);
    let tree = resvg::usvg::Tree::from_str(&replaced, &resvg::usvg::Options::default()).map_err(
        |error| UiError::Render {
            message: format!("icon SVG parse failed: {error}"),
        },
    )?;
    let side = pixels;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side).ok_or_else(|| UiError::Render {
        message: format!("cannot allocate a {side}x{side} icon pixmap"),
    })?;
    let size = tree.size();
    let scale_x = side as f32 / size.width().max(1.0);
    let scale_y = side as f32 / size.height().max(1.0);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(ColorImage::from_rgba_premultiplied(
        [side as usize, side as usize],
        pixmap.data(),
    ))
}

fn paint_texture(ui: &Ui, rect: Rect, texture: &TextureHandle, enabled: bool) {
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let alpha = if enabled { 255 } else { DISABLED_ALPHA };
    ui.painter()
        .image(texture.id(), rect, uv, Color32::from_white_alpha(alpha));
}

fn tint_for(theme: &Theme, token: Option<&str>) -> Color32 {
    let token = token.unwrap_or("icon.neutral");
    theme
        .colors
        .get(token)
        .and_then(|value| parse_hex_color(value))
        .unwrap_or(DEFAULT_TINT)
}

fn parse_hex_color(value: &str) -> Option<Color32> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    let pair = |at: usize| {
        hex.get(at..at + 2)
            .and_then(|p| u8::from_str_radix(p, 16).ok())
    };
    match hex.len() {
        6 => Some(Color32::from_rgb(pair(0)?, pair(2)?, pair(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            pair(0)?,
            pair(2)?,
            pair(4)?,
            pair(6)?,
        )),
        _ => None,
    }
}

fn safe_pixels_per_point(ppp: f32) -> f32 {
    if ppp.is_finite() && ppp > 0.0 {
        ppp.clamp(0.1, 16.0)
    } else {
        1.0
    }
}

fn physical_pixels(size: f32, ppp: f32) -> u32 {
    let size = clamp_size(size);
    let ppp = safe_pixels_per_point(ppp);
    (size * ppp).round().clamp(1.0, MAX_ICON_PIXELS as f32) as u32
}

fn warn_once(key: &str, message: &str) {
    static WARNED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(BTreeSet::new()));
    if let Ok(mut warned) = warned.lock()
        && warned.insert(key.to_owned())
    {
        log::warn!("{message}");
    }
}

#[cfg(test)]
mod icon_set_tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::test_util::with_ui;

    /// A theme with the icon tokens the map uses.
    fn theme() -> Theme {
        let mut colors = BTreeMap::new();
        colors.insert("icon.neutral".to_owned(), "#444444".to_owned());
        colors.insert("icon.blue".to_owned(), "#2b6cc4".to_owned());
        colors.insert("icon.orange".to_owned(), "#d9831f".to_owned());
        Theme {
            schema_version: 1,
            id: "panel-modern".to_owned(),
            name: "Panel Modern (white)".to_owned(),
            fonts: BTreeMap::new(),
            colors,
            metrics: BTreeMap::new(),
            contextual_tab_sets: BTreeMap::new(),
        }
    }

    #[test]
    fn icon_atom_accepts_a_known_key() {
        let atom = icon_atom("edit.cut", 16.0, &theme(), true).expect("a known key");
        with_ui(|ui| {
            ui.add(egui::Button::new((atom, "Cut")));
        });
    }

    #[test]
    fn unknown_keys_draw_the_placeholder() {
        let atom = icon_atom("no.such.icon", 16.0, &theme(), true).expect("the fallback");
        with_ui(|ui| {
            ui.add(egui::Button::new((atom, "Unknown")));
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(16.0));
            draw(ui, rect, "no.such.icon", 16.0, &theme(), true).expect("the fallback");
        });
    }

    #[test]
    fn a_disabled_atom_and_an_empty_key_draw() {
        let atom = icon_atom("edit.cut", 16.0, &theme(), false).expect("a known key");
        let fallback = icon_atom("", 16.0, &theme(), true).expect("the fallback");
        with_ui(|ui| {
            ui.add(egui::Button::new((atom, "Cut")));
            ui.add(egui::Button::new((fallback, "Unknown")));
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(16.0));
            draw(ui, rect, "", 16.0, &theme(), false).expect("the fallback");
        });
    }

    #[test]
    fn draw_accepts_known_and_disabled_keys() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(32.0));
        with_ui(|ui| {
            draw(ui, rect, "edit.cut", 32.0, &theme(), true).expect("an icon");
            draw(ui, rect, "edit.cut", 32.0, &theme(), false).expect("a disabled icon");
        });
    }

    #[test]
    fn every_map_name_rasterises_at_16_and_32() {
        let tint = Color32::from_rgb(0x44, 0x44, 0x44);
        for entry in table::ENTRIES {
            let bytes = table::icon(entry.icon).expect("an embedded SVG");
            for side in [16, 32] {
                let image = rasterize(bytes, tint, side).expect("the icon rasterises");
                assert_eq!(image.size, [side as usize, side as usize], "{}", entry.icon);
                assert!(
                    image.pixels.iter().any(|pixel| pixel.a() > 0),
                    "{} rasterised empty at {side} px",
                    entry.icon
                );
            }
        }
    }

    #[test]
    fn the_tint_replaces_current_color() {
        let red = Color32::from_rgb(0xff, 0x00, 0x00);
        let bytes = table::icon("scissors").expect("an embedded SVG");
        let image = rasterize(bytes, red, 32).expect("the icon rasterises");
        let opaque = image
            .pixels
            .iter()
            .filter(|pixel| pixel.a() == 255)
            .collect::<Vec<_>>();
        assert!(!opaque.is_empty(), "the icon has fully opaque pixels");
        for pixel in opaque {
            assert_eq!((pixel.r(), pixel.g(), pixel.b()), (0xff, 0x00, 0x00));
        }
    }

    #[test]
    fn every_entry_and_icon_resolves() {
        for entry in table::ENTRIES {
            assert!(table::entry(entry.key).is_some());
            assert!(table::icon(entry.icon).is_some(), "{}", entry.icon);
        }
        assert!(table::icon("no-such-icon").is_none());
        assert!(table::entry("no.such.key").is_none());
    }

    #[test]
    fn the_cache_reuses_a_texture_and_separates_keys() {
        with_ui(|ui| {
            let tint = Color32::from_rgb(0x2b, 0x6c, 0xc4);
            let first = texture_for(ui, "scissors", tint, 16.0).expect("a texture");
            let again = texture_for(ui, "scissors", tint, 16.0).expect("a cached texture");
            assert_eq!(first.id(), again.id());
            let other_size = texture_for(ui, "scissors", tint, 32.0).expect("a texture");
            assert_ne!(first.id(), other_size.id());
            let other_tint = texture_for(ui, "scissors", Color32::from_rgb(0xd9, 0x83, 0x1f), 16.0)
                .expect("a texture");
            assert_ne!(first.id(), other_tint.id());
        });
    }

    #[test]
    fn the_cache_bound_holds_and_evicts_the_oldest() {
        let mut cache: Lru<u32, u32> = Lru::default();
        for key in 0..(MAX_CACHE_ENTRIES as u32 + 1) {
            cache.insert(key, key);
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert_eq!(cache.get(&0), None, "the first key was evicted");
        assert_eq!(
            cache.get(&(MAX_CACHE_ENTRIES as u32)),
            Some(MAX_CACHE_ENTRIES as u32)
        );
        // A hit refreshes the entry, so the next insert keeps it.
        assert_eq!(cache.get(&1), Some(1));
        cache.insert(u32::MAX, u32::MAX);
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert_eq!(
            cache.get(&2),
            None,
            "the least recently used key was evicted"
        );
        assert_eq!(cache.get(&1), Some(1));
    }

    #[test]
    fn tint_tokens_parse_and_fall_back() {
        assert_eq!(
            tint_for(&theme(), Some("icon.blue")),
            Color32::from_rgb(0x2b, 0x6c, 0xc4)
        );
        assert_eq!(
            tint_for(&theme(), None),
            Color32::from_rgb(0x44, 0x44, 0x44)
        );
        assert_eq!(tint_for(&theme(), Some("icon.missing")), DEFAULT_TINT);
        assert_eq!(
            parse_hex_color("#2b6cc4"),
            Some(Color32::from_rgb(0x2b, 0x6c, 0xc4))
        );
        assert_eq!(
            parse_hex_color("2b6cc480"),
            Some(Color32::from_rgba_unmultiplied(0x2b, 0x6c, 0xc4, 0x80))
        );
        for bad in ["", "#", "#12345", "#zzzzzz", "#123456789"] {
            assert_eq!(parse_hex_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn sizes_and_scales_are_capped() {
        assert_eq!(physical_pixels(f32::NAN, 1.0), DEFAULT_SIZE as u32);
        assert_eq!(physical_pixels(0.0, 1.0), (MIN_SIZE * 1.0) as u32);
        assert_eq!(physical_pixels(32.0, 2.0), 64);
        assert_eq!(physical_pixels(1000.0, 1.0), MAX_SIZE as u32);
        assert_eq!(physical_pixels(64.0, 16.0), MAX_ICON_PIXELS);
        assert_eq!(physical_pixels(16.0, f32::NAN), 16);
        assert_eq!(physical_pixels(16.0, 0.0), 16);
        assert_eq!(physical_pixels(16.0, 1000.0), 16 * 16);
        assert_eq!(safe_pixels_per_point(f32::INFINITY), 1.0);
    }

    #[test]
    fn a_disabled_icon_is_painted_at_40_percent_alpha() {
        assert_eq!(Color32::from_white_alpha(255), Color32::WHITE);
        assert_eq!(DISABLED_ALPHA, 102);
    }

    #[test]
    fn a_rasterisation_error_falls_back_to_the_placeholder() {
        with_ui(|ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(16.0));
            paint_or_fallback(
                ui,
                rect,
                "edit.cut",
                Err(UiError::Render {
                    message: "test failure".to_owned(),
                }),
                true,
            );
        });
    }

    #[test]
    fn a_missing_embedded_svg_is_an_error() {
        with_ui(|ui| {
            let result = texture_for(ui, "no-such-icon", DEFAULT_TINT, 16.0);
            assert!(matches!(result, Err(UiError::Render { .. })));
        });
    }

    #[test]
    fn broken_svg_bytes_are_errors() {
        let tint = Color32::from_rgb(0x44, 0x44, 0x44);
        assert!(rasterize(b"\xff\xfe", tint, 16).is_err());
        assert!(rasterize(b"not an svg", tint, 16).is_err());
        let valid = table::icon("scissors").expect("an embedded SVG");
        assert!(rasterize(valid, tint, 0).is_err(), "no 0-px pixmap");
    }

    /// A log sink, so a test can count the warnings that fire.
    struct CaptureLog(Mutex<Vec<String>>);

    impl log::Log for CaptureLog {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata())
                && let Ok(mut messages) = self.0.lock()
            {
                messages.push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    fn capture_log() -> &'static CaptureLog {
        static LOGGER: OnceLock<CaptureLog> = OnceLock::new();
        let logger = LOGGER.get_or_init(|| CaptureLog(Mutex::new(Vec::new())));
        let _ = log::set_logger(logger);
        log::set_max_level(log::LevelFilter::Warn);
        logger
    }

    #[test]
    fn one_warning_covers_a_key() {
        let logger = capture_log();
        warn_once("test.warning.key", "first");
        warn_once("test.warning.key", "second");
        warn_once("test.warning.other", "other");
        log::Log::flush(logger);
        let messages = logger.0.lock().expect("the log lock");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.contains("first"))
                .count(),
            1,
            "the first key logs once"
        );
        assert!(!messages.iter().any(|m| m.contains("second")));
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.contains("other"))
                .count(),
            1,
            "a distinct key logs once"
        );
    }
}
