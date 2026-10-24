//! Fonts: parsing, glyph outlines and the fallback stack (ADR 0024).
//!
//! Fonts are data: Carlito (for Calibri) and Selawik (for Segoe UI) are
//! bundled with their OFL licences (AGENTS.md section 4). A [`FontStack`]
//! resolves a family name to a font and falls back to its first font; a
//! missing font or glyph is a warning on the [`Layout`](crate::Layout),
//! never an error (ADR 0021).
//!
//! `skrifa` parses the faces and produces outlines; `harfrust` shapes the
//! text (in [`crate::layout`]). Both wrap the same font bytes here so the
//! crates can be swapped later (ADR 0024).
//!
//! # Family aliases
//!
//! The bundled faces substitute the Microsoft fonts we cannot ship, so
//! each face can carry substitute names ([`Font::with_alias`]): a run that
//! asks for `"Calibri"` resolves to Carlito, `"Segoe UI"` to Selawik.
//! [`default_fonts`] registers those aliases.

use std::sync::OnceLock;

use larrygram_geom::{Path, Point, Segment};
use skrifa::attribute::Style;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

use crate::error::Error;

/// A glyph index in one font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphId(pub u16);

/// One bundled font face, with its bytes.
///
/// The bytes are the unmodified upstream file (AGENTS.md section 4); the
/// row in `ATTRIBUTION.md` records its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontSource {
    /// The file name in `crates/text/fonts/`.
    pub name: &'static str,
    /// The TrueType bytes as committed.
    pub bytes: &'static [u8],
}

/// One parsed font face.
///
/// The font keeps its bytes and parses faces on demand; outlines come out
/// in em units (size 1), so callers scale by their font size (ADR 0024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    bytes: std::sync::Arc<Vec<u8>>,
    family: String,
    aliases: Vec<String>,
    weight: u16,
    italic: bool,
    upem: u16,
    /// Ascent in font units (positive up).
    ascent: i32,
    /// Descent in font units (positive down).
    descent: i32,
    /// Line gap in font units.
    line_gap: i32,
}

impl Font {
    /// Parses a TrueType or OpenType font from its bytes.
    ///
    /// Returns [`Error::InvalidFont`] when the bytes are not a font this
    /// crate can parse.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Font, Error> {
        let font = font_ref(&bytes[..])?;
        let family = family_name(&font);
        let attrs = font.attributes();
        let italic = attrs.style != Style::Normal;
        let weight = attrs.weight.value().round().clamp(0.0, 1000.0) as u16;
        let metrics = font.metrics(Size::new(1.0), LocationRef::default());
        let upem = if metrics.units_per_em == 0 {
            1
        } else {
            metrics.units_per_em
        };
        // `Metrics` values are in em units (size 1); store font units so
        // the face keeps integer fields.
        let to_units = |v: f32| -> i32 {
            (f64::from(v) * f64::from(upem))
                .round()
                .clamp(-32768.0, 32767.0) as i32
        };
        let ascent = to_units(metrics.ascent);
        let descent = to_units(-metrics.descent);
        let line_gap = to_units(metrics.leading);
        Ok(Font {
            bytes: std::sync::Arc::new(bytes),
            family,
            aliases: Vec::new(),
            weight,
            italic,
            upem,
            ascent,
            descent,
            line_gap,
        })
    }

    /// The family name of the font.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The substitute family names this font answers to.
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Adds a substitute family name (for example `"Calibri"` for
    /// Carlito).
    #[must_use]
    pub fn with_alias(mut self, alias: impl Into<String>) -> Font {
        self.aliases.push(alias.into());
        self
    }

    /// The weight class of the face (100–1000, `usWeightClass`).
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Whether the face is bold (weight class 700 or more).
    pub fn bold(&self) -> bool {
        self.weight >= 700
    }

    /// Whether the face is italic or oblique.
    pub fn italic(&self) -> bool {
        self.italic
    }

    /// Whether the face answers to the family name `name`: its family or
    /// one of its aliases, compared case-insensitively.
    pub fn matches_family(&self, name: &str) -> bool {
        eq_name(&self.family, name) || self.aliases.iter().any(|a| eq_name(a, name))
    }

    /// The outline of one glyph in em units (size 1).
    ///
    /// The outline is in the font's own orientation (y up); backends scale
    /// by `size / 72` inches per point and map through the block
    /// transform (ADR 0024). A glyph without an outline (a space, or an
    /// id the font does not have) yields an empty path.
    pub fn outline(&self, glyph: GlyphId) -> Result<Path, Error> {
        let font = font_ref(&self.bytes[..])?;
        let Some(outline) = font.outline_glyphs().get(glyph.0.into()) else {
            return Ok(Path::new());
        };
        let mut pen = PathPen(Path::new());
        outline
            .draw(
                DrawSettings::unhinted(Size::new(1.0), LocationRef::default()),
                &mut pen,
            )
            .map_err(|_| Error::InvalidFont)?;
        Ok(pen.0)
    }

    /// The character-to-glyph map of the face.
    pub(crate) fn has_char(&self, ch: char) -> bool {
        font_ref(&self.bytes[..])
            .map(|f| f.charmap().map(u32::from(ch)).is_some())
            .unwrap_or(false)
    }

    /// The glyph index of `ch`, when the face has it.
    pub(crate) fn glyph_of(&self, ch: char) -> Option<GlyphId> {
        let font = font_ref(&self.bytes[..]).ok()?;
        let gid = font.charmap().map(u32::from(ch))?.to_u32();
        // Glyph ids beyond `u16` do not fit our `GlyphId`; the bundled
        // faces never produce one.
        u16::try_from(gid).ok().map(GlyphId)
    }

    /// The font bytes as a shared source, for `harfrust`.
    pub(crate) fn blob(&self) -> std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> {
        self.bytes.clone()
    }

    /// The ascent in em units (positive up).
    pub(crate) fn ascent_em(&self) -> f64 {
        f64::from(self.ascent) / f64::from(self.upem)
    }

    /// The descent in em units (positive down).
    pub(crate) fn descent_em(&self) -> f64 {
        f64::from(self.descent) / f64::from(self.upem)
    }

    /// The line gap in em units.
    pub(crate) fn line_gap_em(&self) -> f64 {
        f64::from(self.line_gap) / f64::from(self.upem)
    }

    /// The units per em of the face.
    pub(crate) fn units_per_em(&self) -> f64 {
        f64::from(self.upem).max(1.0)
    }

    /// The nominal advance width of a glyph in em units (size 1).
    pub(crate) fn advance_em(&self, glyph: GlyphId) -> f64 {
        font_ref(&self.bytes[..])
            .ok()
            .and_then(|f| {
                f.glyph_metrics(Size::new(1.0), LocationRef::default())
                    .advance_width(glyph.0.into())
            })
            .map(f64::from)
            .unwrap_or(0.0)
    }
}

/// The fonts available to one layout, in fallback order (ADR 0024).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontStack {
    fonts: Vec<Font>,
}

impl FontStack {
    /// An empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a font to the fallback chain.
    pub fn push(&mut self, font: Font) {
        self.fonts.push(font);
    }

    /// The font for `family`: the first font with that family name or
    /// alias, else the first font in the stack, else `None`.
    pub fn resolve(&self, family: &str) -> Option<&Font> {
        self.find_family(family).or_else(|| self.fonts.first())
    }

    /// The font for `family`, `bold` and `italic`, with its index in
    /// [`FontStack::fonts`].
    ///
    /// The pick, in order (ADR 0024 amendment):
    ///
    /// 1. a face of the family with the requested style, closest in
    ///    weight (400 or 700);
    /// 2. a face of the requested style from any family, closest in
    ///    weight — this is where an italic run falls back to Carlito
    ///    Italic, because Selawik ships no italics;
    /// 3. a face of the family with any style, closest in weight;
    /// 4. the first face in the stack.
    ///
    /// Steps 2–4 are fallbacks: the caller warns
    /// [`Warning::MissingFont`](crate::Warning::MissingFont) when the
    /// result does not match the family and style exactly
    /// ([`Font::matches_family`]).
    pub fn resolve_styled(&self, family: &str, bold: bool, italic: bool) -> Option<(usize, &Font)> {
        let want_weight = if bold { 700 } else { 400 };
        best_by_weight(self.matching(family, Some(italic)), want_weight)
            .or_else(|| best_by_weight(self.matching("", Some(italic)), want_weight))
            .or_else(|| best_by_weight(self.matching(family, None), want_weight))
            .or_else(|| self.fonts.first().map(|f| (0, f)))
    }

    /// The fonts, in fallback order.
    pub fn fonts(&self) -> &[Font] {
        &self.fonts
    }

    fn find_family(&self, family: &str) -> Option<&Font> {
        self.fonts.iter().find(|f| f.matches_family(family))
    }

    /// All faces matching `family` (any family when empty) and, when set,
    /// the italic flag.
    fn matching(&self, family: &str, italic: Option<bool>) -> Vec<(usize, &Font)> {
        self.fonts
            .iter()
            .enumerate()
            .filter(|(_, f)| family.is_empty() || f.matches_family(family))
            .filter(|(_, f)| italic.is_none_or(|i| f.italic == i))
            .collect()
    }
}

/// The bundled Carlito faces (for Calibri).
pub static CARLITO: &[FontSource] = &[
    FontSource {
        name: "Carlito-Regular.ttf",
        bytes: include_bytes!("../fonts/Carlito-Regular.ttf"),
    },
    FontSource {
        name: "Carlito-Bold.ttf",
        bytes: include_bytes!("../fonts/Carlito-Bold.ttf"),
    },
    FontSource {
        name: "Carlito-Italic.ttf",
        bytes: include_bytes!("../fonts/Carlito-Italic.ttf"),
    },
    FontSource {
        name: "Carlito-BoldItalic.ttf",
        bytes: include_bytes!("../fonts/Carlito-BoldItalic.ttf"),
    },
];

/// The bundled Selawik faces (for Segoe UI).
///
/// The upstream release 1.01 has Light, Semilight, Regular, Semibold and
/// Bold faces and no italics; italic runs fall back to Carlito (ADR 0024
/// amendment).
pub static SELAWIK: &[FontSource] = &[
    FontSource {
        name: "selawkl.ttf",
        bytes: include_bytes!("../fonts/selawkl.ttf"),
    },
    FontSource {
        name: "selawksl.ttf",
        bytes: include_bytes!("../fonts/selawksl.ttf"),
    },
    FontSource {
        name: "selawk.ttf",
        bytes: include_bytes!("../fonts/selawk.ttf"),
    },
    FontSource {
        name: "selawksb.ttf",
        bytes: include_bytes!("../fonts/selawksb.ttf"),
    },
    FontSource {
        name: "selawkb.ttf",
        bytes: include_bytes!("../fonts/selawkb.ttf"),
    },
];

/// Builds the default stack: Carlito (alias `Calibri`), then Selawik
/// (alias `Segoe UI`).
///
/// The faces are parsed once and cloned out. A bundled face that does not
/// parse is skipped (it cannot happen: the fonts are committed and
/// covered by tests).
pub fn default_fonts() -> FontStack {
    static STACK: OnceLock<FontStack> = OnceLock::new();
    STACK.get_or_init(build_default_stack).clone()
}

fn build_default_stack() -> FontStack {
    let mut stack = FontStack::new();
    let carlito = CARLITO.iter().map(|f| (f, "Calibri"));
    let selawik = SELAWIK.iter().map(|f| (f, "Segoe UI"));
    for (source, alias) in carlito.chain(selawik) {
        if let Ok(font) = Font::from_bytes(source.bytes.to_vec()) {
            stack.push(font.with_alias(alias));
        }
    }
    stack
}

/// A font whose bytes do not parse, for the fallback paths' tests
/// (`advance_em` and the shaper fallback in `crate::layout`).
#[cfg(test)]
pub(crate) fn unparsable_for_test() -> Font {
    Font {
        bytes: std::sync::Arc::new(b"not a font".to_vec()),
        family: String::new(),
        aliases: Vec::new(),
        weight: 400,
        italic: false,
        upem: 1000,
        ascent: 800,
        descent: 200,
        line_gap: 0,
    }
}

fn font_ref(bytes: &[u8]) -> Result<FontRef<'_>, Error> {
    FontRef::new(bytes).map_err(|_| Error::InvalidFont)
}

/// The family name: the typographic family (name id 16) when present,
/// else the family (name id 1). English first, else the first record.
fn family_name(font: &FontRef<'_>) -> String {
    pick_name(font, StringId::TYPOGRAPHIC_FAMILY_NAME)
        .or_else(|| pick_name(font, StringId::FAMILY_NAME))
        .unwrap_or_default()
}

fn pick_name(font: &FontRef<'_>, id: StringId) -> Option<String> {
    let mut first: Option<String> = None;
    for s in font.localized_strings(id) {
        let value = s.to_string();
        if value.is_empty() {
            continue;
        }
        let english = s.language().is_some_and(|l| l.starts_with("en"));
        if english {
            return Some(value);
        }
        if first.is_none() {
            first = Some(value);
        }
    }
    first
}

fn weight_distance(a: u16, b: u16) -> u32 {
    let (a, b) = (i32::from(a), i32::from(b));
    a.abs_diff(b)
}

/// The candidate closest in weight to `want`; the first on ties.
fn best_by_weight<'f>(candidates: Vec<(usize, &'f Font)>, want: u16) -> Option<(usize, &'f Font)> {
    let mut best: Option<(usize, &'f Font)> = None;
    for candidate in candidates {
        let better = match best {
            None => true,
            Some((_, b)) => {
                weight_distance(candidate.1.weight, want) < weight_distance(b.weight, want)
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best
}

/// Family names compare case-insensitively and ignore ASCII spaces and
/// hyphens (`"Segoe UI"`, `"SegoeUI"`).
fn eq_name(a: &str, b: &str) -> bool {
    let fold = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_ascii_whitespace() && *c != '-')
            .flat_map(char::to_lowercase)
            .collect()
    };
    fold(a) == fold(b)
}

/// Collects a glyph outline into a [`Path`].
struct PathPen(Path);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Segment::MoveTo(point(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Segment::LineTo(point(x, y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.push(Segment::QuadTo {
            ctrl: point(cx0, cy0),
            to: point(x, y),
        });
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.push(Segment::CubicTo {
            ctrl1: point(cx0, cy0),
            ctrl2: point(cx1, cy1),
            to: point(x, y),
        });
    }

    fn close(&mut self) {
        self.0.push(Segment::Close);
    }
}

fn point(x: f32, y: f32) -> Point {
    Point::new(f64::from(x), f64::from(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(family: &str) -> Font {
        Font {
            bytes: std::sync::Arc::new(Vec::new()),
            family: family.to_owned(),
            aliases: Vec::new(),
            weight: 400,
            italic: false,
            upem: 1000,
            ascent: 800,
            descent: 200,
            line_gap: 0,
        }
    }

    #[test]
    fn family_accessor_returns_the_name() {
        assert_eq!(font("Carlito").family(), "Carlito");
    }

    #[test]
    fn from_bytes_rejects_garbage() {
        assert_eq!(Font::from_bytes(Vec::new()), Err(Error::InvalidFont));
        assert_eq!(
            Font::from_bytes(b"not a font".to_vec()),
            Err(Error::InvalidFont)
        );
    }

    #[test]
    fn from_bytes_reads_the_bundled_carlito_regular() {
        let f = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        assert!(f.matches_family("Carlito"));
        assert!(f.matches_family("carlito"));
        assert_eq!(f.weight(), 400);
        assert!(!f.bold());
        assert!(!f.italic());
        assert_eq!(f.aliases(), &[] as &[String]);
        assert_eq!(f.units_per_em(), 2048.0);
        // Line metrics of Carlito: ascent 1950, descent 550 per 2048 em.
        assert!((f.ascent_em() - 1950.0 / 2048.0).abs() < 1e-12);
        assert!((f.descent_em() - 550.0 / 2048.0).abs() < 1e-12);
        assert_eq!(f.line_gap_em(), 0.0);
    }

    #[test]
    fn aliases_add_substitute_names() {
        let f = font("Carlito").with_alias("Calibri");
        assert_eq!(f.aliases(), &["Calibri".to_owned()]);
        assert!(f.matches_family("Calibri"));
        assert!(f.matches_family("CALIBRI"));
        assert!(!f.matches_family("Carlito Jr"));
        // Hyphens and spaces are not significant.
        let s = font("Selawik").with_alias("Segoe UI");
        assert!(s.matches_family("SegoeUI"));
        assert!(s.matches_family("se goe ui"));
    }

    #[test]
    fn bundled_faces_parse_with_expected_styles() {
        for source in CARLITO {
            Font::from_bytes(source.bytes.to_vec()).unwrap();
        }
        let styles: Vec<(u16, bool)> = CARLITO
            .iter()
            .chain(SELAWIK)
            .map(|s| {
                let f = Font::from_bytes(s.bytes.to_vec()).unwrap();
                (f.weight(), f.italic())
            })
            .collect();
        assert_eq!(
            styles,
            vec![
                (400, false),
                (700, false),
                (400, true),
                (700, true),
                (300, false),
                (300, false),
                (400, false),
                (600, false),
                (700, false),
            ]
        );
    }

    #[test]
    fn bold_uses_the_weight_class() {
        let mut f = font("Carlito");
        f.weight = 700;
        assert!(f.bold());
        f.weight = 600;
        assert!(!f.bold());
    }

    #[test]
    fn outline_returns_em_unit_paths() {
        let f = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        // 'A' is a well-known glyph with straight sides.
        let glyph = f.glyph_of('A').unwrap();
        let path = f.outline(glyph).unwrap();
        assert!(!path.is_empty());
        // Em units: every point, control points included, stays within
        // ±1 for a capital letter.
        let bounds = path.bounds().unwrap();
        let max = [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
            .into_iter()
            .fold(0.0_f64, |max, v| max.max(v.abs()));
        assert!(max > 0.0 && max <= 1.0, "outline left em units: {max}");
    }

    #[test]
    fn outline_of_a_missing_glyph_is_empty() {
        let f = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        assert!(f.outline(GlyphId(65535)).unwrap().is_empty());
        // A space has no ink.
        let space = f.glyph_of(' ').unwrap();
        assert!(f.outline(space).unwrap().is_empty());
    }

    #[test]
    fn outline_of_a_broken_font_is_an_error() {
        let f = Font {
            bytes: std::sync::Arc::new(b"broken".to_vec()),
            ..font("Carlito")
        };
        assert_eq!(f.outline(GlyphId(1)), Err(Error::InvalidFont));
    }

    #[test]
    fn glyph_lookup_reports_missing_characters() {
        let f = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        assert!(f.has_char('A'));
        assert!(f.glyph_of('A').is_some());
        // Carlito has no CJK.
        assert!(!f.has_char('中'));
        assert_eq!(f.glyph_of('中'), None);
    }

    #[test]
    fn stack_resolves_by_family_then_first() {
        let mut stack = FontStack::new();
        assert_eq!(stack.resolve("Carlito"), None);
        stack.push(font("Selawik"));
        stack.push(font("Carlito"));
        assert_eq!(stack.resolve("Carlito").map(Font::family), Some("Carlito"));
        assert_eq!(stack.resolve("Missing").map(Font::family), Some("Selawik"));
        assert_eq!(stack.fonts().len(), 2);
    }

    #[test]
    fn resolve_styled_picks_the_exact_face() {
        let stack = default_fonts();
        let (i, f) = stack.resolve_styled("Carlito", false, false).unwrap();
        assert_eq!(f.family(), "Carlito");
        assert_eq!(&stack.fonts()[i] as *const Font, f as *const Font);
        assert!(!f.italic());
        assert_eq!(f.weight(), 400);
        let (_, bold) = stack.resolve_styled("Calibri", true, false).unwrap();
        assert_eq!(bold.weight(), 700);
        let (_, bi) = stack.resolve_styled("Calibri", true, true).unwrap();
        assert!(bi.italic());
        assert_eq!(bi.weight(), 700);
    }

    #[test]
    fn resolve_styled_falls_back_to_carlito_italic_for_segoe() {
        // Selawik ships no italics (ADR 0024 amendment): the italic run
        // falls to Carlito Italic, closest weight.
        let stack = default_fonts();
        let (_, f) = stack.resolve_styled("Segoe UI", false, true).unwrap();
        assert_eq!(f.family(), "Carlito");
        assert!(f.italic());
        assert_eq!(f.weight(), 400);
        let (_, fb) = stack.resolve_styled("Segoe UI", true, true).unwrap();
        assert_eq!(fb.family(), "Carlito");
        assert!(fb.italic());
        assert_eq!(fb.weight(), 700);
    }

    #[test]
    fn resolve_styled_prefers_the_closest_weight() {
        // "Segoe UI" regular is Selawik Regular (400), not Light or Bold.
        let stack = default_fonts();
        let (_, f) = stack.resolve_styled("Segoe UI", false, false).unwrap();
        assert_eq!(f.weight(), 400);
        let (_, b) = stack.resolve_styled("Segoe UI", true, false).unwrap();
        assert_eq!(b.weight(), 700);
    }

    #[test]
    fn resolve_styled_on_an_empty_stack_is_none() {
        assert_eq!(
            FontStack::new().resolve_styled("Carlito", false, false),
            None
        );
    }

    #[test]
    fn default_fonts_are_carlito_then_selawik_with_aliases() {
        let stack = default_fonts();
        assert_eq!(stack.fonts().len(), 9);
        assert_eq!(stack.fonts()[0].family(), "Carlito");
        assert_eq!(stack.fonts()[4].family(), "Selawik");
        assert!(stack.resolve("Calibri").unwrap().matches_family("Carlito"));
        assert!(stack.resolve("Segoe UI").unwrap().matches_family("Selawik"));
    }

    #[test]
    fn eq_name_ignores_case_space_and_hyphen() {
        assert!(eq_name("Segoe UI", "segoe-ui"));
        assert!(!eq_name("Carlito", "Calibri"));
        assert!(eq_name("", ""));
    }

    #[test]
    fn weight_distance_is_symmetric() {
        assert_eq!(weight_distance(400, 700), 300);
        assert_eq!(weight_distance(700, 400), 300);
        assert_eq!(weight_distance(400, 400), 0);
    }

    #[test]
    fn pick_name_skips_empty_strings() {
        let f = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        assert!(!f.family().is_empty());
    }

    // --- edge cases (P3-17, coverage finding minor 2) -----------------

    #[test]
    fn advance_em_of_an_unparsable_font_is_zero() {
        let f = unparsable_for_test();
        assert_eq!(f.advance_em(GlyphId(1)), 0.0);
    }

    #[test]
    fn path_pen_records_cubic_curves() {
        // The bundled faces are TrueType (quadratic); a CFF glyph would
        // come out through `curve_to`, so the pen is exercised directly.
        let mut pen = PathPen(Path::new());
        pen.move_to(0.0, 0.0);
        pen.curve_to(1.0, 0.0, 1.0, 1.0, 0.0, 1.0);
        pen.close();
        assert_eq!(
            pen.0.segments(),
            &[
                Segment::MoveTo(Point::new(0.0, 0.0)),
                Segment::CubicTo {
                    ctrl1: Point::new(1.0, 0.0),
                    ctrl2: Point::new(1.0, 1.0),
                    to: Point::new(0.0, 1.0),
                },
                Segment::Close,
            ]
        );
    }

    #[test]
    fn family_names_come_from_the_name_table() {
        let carlito = Font::from_bytes(CARLITO[0].bytes.to_vec()).unwrap();
        assert_eq!(carlito.family(), "Carlito");
        let selawk = Font::from_bytes(SELAWIK[0].bytes.to_vec()).unwrap();
        assert_eq!(selawk.family(), "Selawik");
    }
}
