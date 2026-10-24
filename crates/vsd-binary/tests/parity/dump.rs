// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsing `vsd2raw` output into the neutral [`Doc`] (P8-08, ADR 0072).
//!
//! The dump is the librevenge raw generator's call log: one call per
//! line, indented by nesting depth, with a parenthesised property list:
//!
//! ```text
//! startDocument()
//!   setDocumentMetaData(dc:creator: Ada, dc:title: T)
//!   startPage(draw:name: Page-1, svg:height: 8.2677in, svg:width: 11.6929in)
//!     startLayer (draw:id: id1)
//!       setStyle(draw:fill: solid, draw:fill-color: #5b9bd5, ...)
//!       drawPath (svg:d: ((librevenge:path-action: M, svg:x: 1.9in, svg:y: 2.9in), ...))
//!       startTextObject (svg:x: 1.4in, svg:y: 2.1in, svg:width: 1.5in, ...)
//!         openSpan(...)
//!           insertText (Hello)
//!         closeSpan
//!       endTextObject
//!     endLayer
//!   endPage
//! endDocument
//! ```
//!
//! One `startLayer` group is one shape (the reference's raw generator
//! emits a layer per shape; nested layers are a group's sub-shapes).
//! The parser is best-effort and never panics: a line it cannot read
//! is skipped.

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use crate::neutral::{Doc, Fill, Geometry, Page, Rgb, Shape, Stroke, TextBox};

/// Parses a whole `vsd2raw` dump into a neutral document.
///
/// Malformed lines and malformed property lists are skipped; the
/// result holds everything that parsed.
pub fn parse(text: &str) -> Doc {
    let mut state = State::default();
    for line in text.lines() {
        state.line(line);
    }
    state.finish()
}

/// The parser state: one builder stack per nesting kind.
#[derive(Default)]
struct State {
    metadata: BTreeMap<String, String>,
    pages: Vec<PageBuilder>,
    /// Open shapes, outermost first, on the current page.
    stack: Vec<ShapeBuilder>,
    /// Open pages (the dump nests page calls in document order).
    page_open: bool,
}

#[derive(Default)]
struct PageBuilder {
    name: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    shapes: Vec<ShapeBuilder>,
}

#[derive(Default)]
struct ShapeBuilder {
    label: Option<String>,
    /// A text object the dump emitted outside any layer (a shape with
    /// no geometry): it becomes a shape of its own.
    synthetic: bool,
    styles: Vec<BTreeMap<String, String>>,
    /// The `svg:d` strings, in order, deduplicated (the generator
    /// emits every path twice: once filled, once stroked).
    ds: Vec<String>,
    texts: Vec<String>,
    text_box: Option<TextBox>,
    in_text: bool,
    children: Vec<ShapeBuilder>,
}

impl State {
    /// Handles one dump line.
    fn line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        let (name, payload) = split_call(trimmed);
        match name {
            "setDocumentMetaData" => {
                for (key, value) in split_props(payload) {
                    self.metadata.insert(key, value);
                }
            }
            "startPage" => {
                let props = split_props(payload);
                self.pages.push(PageBuilder {
                    name: prop(&props, "draw:name"),
                    width: prop(&props, "svg:width").and_then(|v| inches(&v)),
                    height: prop(&props, "svg:height").and_then(|v| inches(&v)),
                    ..PageBuilder::default()
                });
                self.page_open = true;
            }
            "endPage" => {
                self.page_open = false;
                self.stack.clear();
            }
            "startLayer" => {
                let props = split_props(payload);
                let label = prop(&props, "draw:id");
                self.stack.push(ShapeBuilder {
                    label,
                    ..ShapeBuilder::default()
                });
            }
            "endLayer" => {
                if let Some(shape) = self.stack.pop() {
                    match self.stack.last_mut() {
                        Some(parent) => parent.children.push(shape),
                        None => {
                            if let Some(page) = self.pages.last_mut() {
                                page.shapes.push(shape);
                            }
                        }
                    }
                }
            }
            "setStyle" => {
                if let Some(shape) = self.stack.last_mut() {
                    shape.styles.push(split_props(payload));
                }
            }
            "drawPath" => {
                if let Some(shape) = self.stack.last_mut() {
                    let props = split_props(payload);
                    if let Some(d) = prop(&props, "svg:d")
                        && !shape.ds.contains(&d)
                    {
                        shape.ds.push(d);
                    }
                }
            }
            "startTextObject" => {
                // A text-only shape has no `startLayer` at all: open a
                // synthetic shape so the text and its box attach.
                if self.stack.is_empty() {
                    if !self.page_open {
                        return;
                    }
                    self.stack.push(ShapeBuilder {
                        synthetic: true,
                        ..ShapeBuilder::default()
                    });
                }
                let props = split_props(payload);
                let x = prop(&props, "svg:x").and_then(|v| inches(&v));
                let y = prop(&props, "svg:y").and_then(|v| inches(&v));
                let width = prop(&props, "svg:width").and_then(|v| inches(&v));
                let height = prop(&props, "svg:height").and_then(|v| inches(&v));
                let rotate_deg = prop(&props, "librevenge:rotate").and_then(|v| number(&v));
                if let Some(shape) = self.stack.last_mut() {
                    shape.text_box = match (x, y, width, height) {
                        (Some(x), Some(y), Some(width), Some(height)) => Some(TextBox {
                            x,
                            y,
                            width,
                            height,
                            rotate_deg: rotate_deg.unwrap_or(0.0),
                        }),
                        _ => None,
                    };
                    shape.in_text = true;
                }
            }
            "endTextObject" => {
                if let Some(shape) = self.stack.last_mut() {
                    shape.in_text = false;
                    if shape.synthetic {
                        let shape = self.stack.pop().unwrap_or_default();
                        if let Some(page) = self.pages.last_mut() {
                            page.shapes.push(shape);
                        }
                    }
                }
            }
            "insertText" => {
                if let Some(shape) = self.stack.last_mut() {
                    let text = payload.to_owned();
                    if !text.is_empty() {
                        shape.texts.push(text);
                    }
                }
            }
            // Paragraph and span calls carry formatting only; line
            // breaks are not text content for the comparison. Anything
            // else (foreign data, pictures, ...) is outside the
            // compared fields (ADR 0070 drops).
            _ => {}
        }
    }

    /// Builds the finished document out of the open builders.
    fn finish(mut self) -> Doc {
        let mut pages = Vec::new();
        for page in self.pages.drain(..) {
            pages.push(Page {
                name: page.name.map(|n| n.nfc().collect()),
                width: page.width,
                height: page.height,
                shapes: page.shapes.into_iter().map(build_shape).collect(),
            });
        }
        let mut metadata = BTreeMap::new();
        // The metadata keys map onto the compared ones (ADR 0072).
        if let Some(v) = self.metadata.get("dc:title") {
            metadata.insert("title".to_owned(), v.clone());
        }
        if let Some(v) = self.metadata.get("dc:creator") {
            metadata.insert("creator".to_owned(), v.clone());
        }
        if let Some(v) = self.metadata.get("librevenge:template") {
            metadata.insert("template".to_owned(), v.clone());
        }
        let date = self
            .metadata
            .get("meta:creation-date")
            .or_else(|| self.metadata.get("dc:date"));
        if let Some(v) = date {
            metadata.insert("date".to_owned(), v.clone());
        }
        Doc { metadata, pages }
    }
}

/// Turns a shape builder into the neutral shape.
fn build_shape(builder: ShapeBuilder) -> Shape {
    let mut fill = None;
    let mut fill_color = None;
    let mut stroke = None;
    let mut stroke_color = None;
    let mut stroke_width = None;
    let mut shadow = None;
    let mut shadow_color = None;
    for style in &builder.styles {
        if fill.is_none()
            && let Some(v) = prop(style, "draw:fill")
        {
            fill = Some(fill_kind(&v));
            fill_color = prop(style, "draw:fill-color").and_then(|v| rgb(&v));
        }
        if stroke.is_none()
            && let Some(v) = prop(style, "draw:stroke")
        {
            stroke = Some(stroke_kind(&v));
            stroke_color = prop(style, "svg:stroke-color").and_then(|v| rgb(&v));
            stroke_width = prop(style, "svg:stroke-width").and_then(|v| inches(&v));
        }
        if shadow.is_none()
            && let Some(v) = prop(style, "draw:shadow")
        {
            shadow = Some(v != "none");
            shadow_color = prop(style, "draw:shadow-color").and_then(|v| rgb(&v));
        }
    }
    // The generator states a shadow only when one is drawn.
    let shadow = shadow.or(Some(false));
    let geometry = geometry_of(&builder.ds);
    let text = if builder.texts.is_empty() {
        None
    } else {
        Some(builder.texts.join(""))
    };
    let label = builder.label.clone().unwrap_or_default();
    let text_box = if text.is_some() {
        builder.text_box
    } else {
        None
    };
    Shape {
        label,
        text,
        fill,
        fill_color,
        stroke,
        stroke_color,
        stroke_width,
        shadow,
        shadow_color,
        text_box,
        geometry,
        children: builder.children.into_iter().map(build_shape).collect(),
    }
}

/// The geometry summary of the deduplicated `svg:d` strings.
fn geometry_of(ds: &[String]) -> Option<Geometry> {
    let mut geometry = Geometry {
        polyline: true,
        ..Geometry::default()
    };
    let mut any = false;
    for d in ds {
        let Some(path) = parse_path(d) else {
            continue;
        };
        any = true;
        // Split into sub-paths at `M`, and drop the closing point the
        // generator emits before `Z` when it repeats the sub-path's
        // first point.
        let mut subpaths: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut closed = Vec::new();
        for (action, x, y) in &path {
            match action {
                'M' => {
                    subpaths.push(vec![(*x, *y)]);
                    closed.push(false);
                }
                'Z' => {
                    if let Some(last) = closed.last_mut() {
                        *last = true;
                    }
                }
                _ => {
                    if let Some(last) = subpaths.last_mut() {
                        last.push((*x, *y));
                    }
                }
            }
        }
        for (i, subpath) in subpaths.iter_mut().enumerate() {
            if closed.get(i).copied().unwrap_or(false) {
                crate::neutral::drop_closing_vertex(subpath);
            }
        }
        geometry.subpaths += subpaths.len();
        for subpath in &subpaths {
            geometry.points.extend(subpath.iter().copied());
        }
        if path
            .iter()
            .any(|(action, _, _)| !matches!(action, 'M' | 'L' | 'Z'))
        {
            geometry.polyline = false;
        }
    }
    any.then_some(geometry)
}

/// Parses one `svg:d` value into (action, x, y) triples in order.
///
/// A curve action's control points (`svg:x1`, `svg:y1`, ...) are not
/// vertices and are dropped; the action kind is what the row-kind
/// comparison reads.
fn parse_path(d: &str) -> Option<Vec<(char, f64, f64)>> {
    let mut out = Vec::new();
    for group in top_level_groups(d) {
        let props = split_props(&group);
        let action = prop(&props, "librevenge:path-action")?;
        let mut chars = action.chars();
        let kind = chars.next()?;
        if kind == 'Z' {
            // A close carries no coordinates.
            out.push((kind, 0.0, 0.0));
            continue;
        }
        let x = prop(&props, "svg:x").and_then(|v| inches(&v))?;
        let y = prop(&props, "svg:y").and_then(|v| inches(&v))?;
        out.push((kind, x, y));
    }
    Some(out)
}

/// The top-level parenthesised groups inside `text`.
fn top_level_groups(text: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '(' => {
                if depth == 1 {
                    start = i;
                }
                depth += 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    groups.push(text[start + 1..i].to_owned());
                }
            }
            _ => {}
        }
    }
    groups
}

/// Splits a call line into its name and its parenthesised payload.
fn split_call(line: &str) -> (&str, &str) {
    let Some(open) = line.find('(') else {
        return (line.trim(), "");
    };
    let name = line[..open].trim();
    let close = line.rfind(')').unwrap_or(line.len());
    let inner = if close > open {
        &line[open + 1..close]
    } else {
        ""
    };
    (name, inner)
}

/// Splits a property list at the commas that separate entries.
///
/// A comma inside a value (a person's name, a path, the text of an
/// `insertText`) does not separate entries unless the next token
/// looks like a `key:` or `ns:key:` prefix. Parenthesised values
/// (`svg:d: ((...))`) never split at their inner commas.
pub fn split_props(payload: &str) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let bytes = payload.as_bytes();
    let mut i = 0usize;
    while i < payload.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                let rest = payload[i + 1..].trim_start();
                if looks_like_key(rest) {
                    insert_prop(&mut props, &payload[start..i]);
                    start = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    insert_prop(&mut props, &payload[start..]);
    props
}

/// Whether `text` starts with a `key:` or `ns:key:` prefix.
fn looks_like_key(text: &str) -> bool {
    let mut seen_colon = false;
    for ch in text.chars() {
        match ch {
            ':' => seen_colon = true,
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {
                if seen_colon {
                    return true;
                }
            }
            _ => return seen_colon,
        }
    }
    seen_colon
}

/// Inserts one `key: value` entry.
///
/// The key is `name` or `ns:name` (`dc:creator`, `svg:x`); the value
/// is everything after the colon that terminates the key.
fn insert_prop(props: &mut BTreeMap<String, String>, entry: &str) {
    let entry = entry.trim();
    if entry.is_empty() {
        return;
    }
    let Some(key_end) = key_end(entry) else {
        return;
    };
    let key = entry[..key_end].trim().to_owned();
    let value = entry[key_end + 1..].trim().to_owned();
    if !key.is_empty() {
        props.insert(key, value);
    }
}

/// The index of the colon that terminates the entry's key.
fn key_end(entry: &str) -> Option<usize> {
    let is_key_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let run1 = entry.find(|c: char| !is_key_char(c)).unwrap_or(entry.len());
    if run1 == 0 {
        return None;
    }
    let rest = &entry[run1..];
    if let Some(after) = rest.strip_prefix(':') {
        let run2 = after.find(|c: char| !is_key_char(c)).unwrap_or(after.len());
        if run2 > 0 && after[run2..].starts_with(':') {
            // `ns:name:` — the second colon terminates the key.
            return Some(run1 + 1 + run2);
        }
    }
    rest.starts_with(':').then_some(run1)
}

/// One property by key.
fn prop(props: &BTreeMap<String, String>, key: &str) -> Option<String> {
    props.get(key).cloned()
}

/// The numeric part of a `3.0433in` style value.
pub fn number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let end = trimmed
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(trimmed.len());
    trimmed[..end].parse::<f64>().ok()
}

/// The unit suffix of a `3.0433in` style value.
pub fn unit(value: &str) -> &str {
    let trimmed = value.trim();
    let end = trimmed
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(trimmed.len());
    trimmed[end..].trim()
}

/// A length in inches (`in`, `pt` or no suffix).
pub fn inches(value: &str) -> Option<f64> {
    let n = number(value)?;
    match unit(value) {
        "in" | "" => Some(n),
        "pt" => Some(n / 72.0),
        _ => None,
    }
}

/// An `#rrggbb` colour.
pub fn rgb(value: &str) -> Option<Rgb> {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
    Some(Rgb::new(r, g, b))
}

/// The fill kind of a `draw:fill` value.
fn fill_kind(value: &str) -> Fill {
    match value {
        "none" => Fill::None,
        "solid" => Fill::Solid,
        "hatch" => Fill::Hatch,
        "gradient" => Fill::Gradient,
        _ => Fill::Other,
    }
}

/// The stroke kind of a `draw:stroke` value.
fn stroke_kind(value: &str) -> Stroke {
    match value {
        "none" => Stroke::None,
        "solid" => Stroke::Solid,
        "dash" => Stroke::Dash,
        _ => Stroke::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built dump: one page, two shapes (one a group with a
    /// sub-shape), styles, geometry and text.
    const DUMP: &str = "\
startDocument()
  setDocumentMetaData(dc:creator: Jurukov, Anton, dc:date: 1601-01-01T06:42:04Z, meta:creation-date: 1601-01-01T06:42:04Z, dc:title: Title test, librevenge:template: BASICD_M.VSTX)
  startPage(draw:name: Page-1, svg:height: 8.2677in, svg:width: 11.6929in)
    startLayer (draw:id: id1)
      setStyle(draw:fill: solid, draw:fill-color: #5b9bd5, draw:stroke: none, svg:stroke-color: #000000, svg:stroke-width: 0.0139in)
      drawPath (svg:d: ((librevenge:path-action: M, svg:x: 1.9685in, svg:y: 2.9838in), (librevenge:path-action: L, svg:x: 3.5433in, svg:y: 2.9838in), (librevenge:path-action: L, svg:x: 1.9685in, svg:y: 2.9838in), (librevenge:path-action: Z)))
    endLayer
    startLayer (draw:id: id2)
      startLayer (draw:id: id3)
        setStyle(draw:fill: none, draw:fill-color: #ffffff, draw:stroke: solid, svg:stroke-color: #c8c8c8, svg:stroke-width: 0.0035in, draw:shadow: visible, draw:shadow-color: #5b9bd5)
        drawPath (svg:d: ((librevenge:path-action: M, svg:x: 5.9in, svg:y: 2.1in), (librevenge:path-action: L, svg:x: 7.4in, svg:y: 2.1in), (librevenge:path-action: Z)))
        drawPath (svg:d: ((librevenge:path-action: M, svg:x: 5.9in, svg:y: 2.1in), (librevenge:path-action: L, svg:x: 7.4in, svg:y: 2.1in), (librevenge:path-action: Z)))
        startTextObject (svg:x: 5.5in, svg:y: 1.9in, svg:width: 1.5in, svg:height: 0.25in, librevenge:rotate: 90.0000)
          openSpan()
            insertText (Hello, world (really))
          closeSpan
        endTextObject
      endLayer
    endLayer
  endPage
endDocument
";

    #[test]
    fn parses_metadata_pages_and_shapes() {
        let doc = parse(DUMP);
        assert_eq!(
            doc.metadata.get("title").map(String::as_str),
            Some("Title test")
        );
        assert_eq!(
            doc.metadata.get("creator").map(String::as_str),
            Some("Jurukov, Anton")
        );
        assert_eq!(
            doc.metadata.get("template").map(String::as_str),
            Some("BASICD_M.VSTX")
        );
        assert_eq!(
            doc.metadata.get("date").map(String::as_str),
            Some("1601-01-01T06:42:04Z")
        );
        assert_eq!(doc.pages.len(), 1);
        let page = &doc.pages[0];
        assert_eq!(page.name.as_deref(), Some("Page-1"));
        assert_eq!(page.width, Some(11.6929));
        assert_eq!(page.height, Some(8.2677));
        assert_eq!(page.shapes.len(), 2);
    }

    #[test]
    fn parses_styles_geometry_and_text() {
        let doc = parse(DUMP);
        let first = &doc.pages[0].shapes[0];
        assert_eq!(first.label, "id1");
        assert_eq!(first.fill, Some(Fill::Solid));
        assert_eq!(first.fill_color, Some(Rgb::new(0x5b, 0x9b, 0xd5)));
        assert_eq!(first.stroke, Some(Stroke::None));
        assert_eq!(first.stroke_color, Some(Rgb::new(0, 0, 0)));
        assert_eq!(first.stroke_width, Some(0.0139));
        assert_eq!(first.shadow, Some(false));
        assert_eq!(first.text, None);
        let geometry = first.geometry.as_ref().unwrap();
        assert!(geometry.polyline);
        assert_eq!(geometry.subpaths, 1);
        assert_eq!(geometry.points, vec![(1.9685, 2.9838), (3.5433, 2.9838)]);
    }

    #[test]
    fn nests_sub_shapes_and_keeps_their_own_text() {
        let doc = parse(DUMP);
        let group = &doc.pages[0].shapes[1];
        assert_eq!(group.label, "id2");
        assert_eq!(group.children.len(), 1);
        let child = &group.children[0];
        assert_eq!(child.label, "id3");
        assert_eq!(child.fill, Some(Fill::None));
        assert_eq!(child.stroke, Some(Stroke::Solid));
        assert_eq!(child.stroke_color, Some(Rgb::new(0xc8, 0xc8, 0xc8)));
        assert_eq!(child.shadow, Some(true));
        assert_eq!(child.shadow_color, Some(Rgb::new(0x5b, 0x9b, 0xd5)));
        assert_eq!(child.text.as_deref(), Some("Hello, world (really)"));
        let text_box = child.text_box.unwrap();
        assert_eq!(text_box.x, 5.5);
        assert_eq!(text_box.y, 1.9);
        assert_eq!(text_box.width, 1.5);
        assert_eq!(text_box.height, 0.25);
        assert_eq!(text_box.rotate_deg, 90.0);
    }

    #[test]
    fn drops_the_closing_point_that_repeats_the_first() {
        let doc = parse(DUMP);
        let geometry = doc.pages[0].shapes[0].geometry.as_ref().unwrap();
        // The dump's third vertex repeats the first before `Z`; the
        // neutral structure keeps the two distinct vertices.
        assert_eq!(geometry.points.len(), 2);
    }

    #[test]
    fn split_props_keeps_commas_inside_values() {
        let props = split_props("dc:creator: Jurukov, Anton, dc:title: A, B");
        assert_eq!(
            props.get("dc:creator").map(String::as_str),
            Some("Jurukov, Anton")
        );
        assert_eq!(props.get("dc:title").map(String::as_str), Some("A, B"));
    }

    #[test]
    fn split_props_keeps_parenthesised_values_whole() {
        let props = split_props(
            "svg:d: ((librevenge:path-action: M, svg:x: 1.0in, svg:y: 2.0in), \
             (librevenge:path-action: L, svg:x: 3.0in, svg:y: 4.0in))",
        );
        let d = props.get("svg:d").unwrap();
        let path = parse_path(d).unwrap();
        assert_eq!(path, vec![('M', 1.0, 2.0), ('L', 3.0, 4.0)]);
    }

    #[test]
    fn measures_convert_to_inches() {
        assert_eq!(inches("3.0433in"), Some(3.0433));
        assert_eq!(inches("72pt"), Some(1.0));
        assert_eq!(inches("0.5"), Some(0.5));
        assert_eq!(inches("40%"), None);
        assert_eq!(number("100.0000%"), Some(100.0));
        assert_eq!(unit("0.0139in"), "in");
    }

    #[test]
    fn colours_parse_as_rgb_triples() {
        assert_eq!(rgb("#5b9bd5"), Some(Rgb::new(0x5b, 0x9b, 0xd5)));
        assert_eq!(rgb("#fff"), None);
        assert_eq!(rgb("none"), None);
    }

    #[test]
    fn empty_and_malformed_dumps_do_not_panic() {
        for text in [
            "",
            "garbage",
            "startLayer (",
            "drawPath (svg:d: (())",
            "insertText (",
        ] {
            let _ = parse(text);
        }
        let doc = parse("startLayer (draw:id: id1)\n  setStyle(draw:fill: weird)\nendLayer\n");
        assert_eq!(doc.pages.len(), 0);
        assert!(doc.metadata.is_empty());
    }

    #[test]
    fn a_shape_outside_a_page_is_dropped() {
        let doc = parse(
            "startLayer ()\n  setStyle(draw:fill: solid, draw:fill-color: #ffffff)\nendLayer\n",
        );
        assert!(doc.pages.is_empty());
    }

    #[test]
    fn a_shape_without_an_id_gets_a_positional_label() {
        let doc = parse("startPage(draw:name: P)\n  startLayer ()\n  endLayer\nendPage\n");
        let page = doc.pages.first().unwrap();
        assert_eq!(page.shapes[0].label, "");
    }
}
