// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extracting the text strings from `vsd2xhtml` output (P8-08, ADR
//! 0072: "compared against `vsd2xhtml`: the text strings and their
//! order").
//!
//! The XHTML rendering wraps every shape in an `<svg:g>` group and
//! every text run in an `<svg:tspan>`. The extraction returns one
//! string per `svg:tspan`, in document order, with markup and
//! entities resolved. The document's comment block (a disabled SVG
//! prologue) is skipped.

use unicode_normalization::UnicodeNormalization;

/// The text strings of a `vsd2xhtml` document, in document order.
pub fn text_strings(xhtml: &str) -> Vec<String> {
    let body = strip_comments(xhtml);
    let mut out = Vec::new();
    let mut rest = body.as_str();
    while let Some(start) = rest.find("<svg:tspan") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let Some(close) = after.find("</svg:tspan>") else {
            // An unterminated span: take the rest of the document.
            let text = unescape(&after[open_end + 1..]);
            push_text(&mut out, &text);
            break;
        };
        let text = unescape(&after[open_end + 1..close]);
        push_text(&mut out, &text);
        rest = &after[close + "</svg:tspan>".len()..];
    }
    out
}

/// One string per text object: the ordered strings joined the way the
/// comparison reads a shape's text.
pub fn joined_strings(strings: &[String]) -> Option<String> {
    if strings.is_empty() {
        None
    } else {
        Some(strings.join(""))
    }
}

/// Appends a normalised string when it has content after
/// normalisation.
fn push_text(out: &mut Vec<String>, text: &str) {
    let normalised: String = text.trim().nfc().collect();
    if !normalised.is_empty() {
        out.push(normalised);
    }
}

/// Removes `<!-- ... -->` comment blocks.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Resolves the text of a span: removes any markup first (a span
/// nests no elements in the oracle output; this is defensive), then
/// resolves XML entities.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let mut decoded = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        match rest.find(';') {
            Some(end) if end <= 10 => {
                let entity = &rest[1..end];
                match decode_entity(entity) {
                    Some(ch) => decoded.push(ch),
                    None => {
                        decoded.push('&');
                        decoded.push_str(entity);
                        decoded.push(';');
                    }
                }
                rest = &rest[end + 1..];
            }
            _ => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// One XML entity (`amp`, `lt`, `gt`, `quot`, `apos`, `#nn`, `#xnn`).
fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = entity.strip_prefix('#')?;
            let code = if let Some(hex) = digits
                .strip_prefix('x')
                .or_else(|| digits.strip_prefix('X'))
            {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                digits.parse::<u32>().ok()?
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built XHTML rendering: a comment block (the disabled SVG
    /// prologue the oracle writes), two shape groups and two tspans.
    const XHTML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
<!--
<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<svg:svg version="1.1">
</svg:svg>
 -->
<svg:svg version="1.1">
<svg:g id="Layer1000">
<svg:text x="108.5984" y="156.4724">
<svg:tspan font-family="Calibri" font-size="8.0000" fill="#000000">
My hovercraft is full of eels.
</svg:tspan>
</svg:text>
</svg:g>
<svg:g id="Layer1001">
<svg:text x="1.0" y="2.0">
<svg:tspan font-family="Calibri">
A &amp; B &lt;tag&gt;
</svg:tspan>
<svg:tspan font-family="Calibri">
&#65;&#x42;
</svg:tspan>
</svg:text>
</svg:g>
</svg:svg>
</body>
</html>
"##;

    #[test]
    fn extracts_strings_in_document_order() {
        let strings = text_strings(XHTML);
        assert_eq!(
            strings,
            vec!["My hovercraft is full of eels.", "A & B <tag>", "AB"]
        );
    }

    #[test]
    fn the_commented_prologue_contributes_no_strings() {
        let strings = text_strings(XHTML);
        assert_eq!(strings.len(), 3);
        assert!(!strings.iter().any(|s| s.contains("svg:svg")));
    }

    #[test]
    fn empty_and_malformed_documents_do_not_panic() {
        for text in [
            "",
            "<svg:tspan>",
            "<svg:tspan>unterminated",
            "<svg:tspan>&bogus;</svg:tspan>",
            "<svg:tspan>&#xzz;</svg:tspan>",
            "<!-- unterminated comment <svg:tspan>x</svg:tspan>",
        ] {
            let _ = text_strings(text);
        }
    }

    #[test]
    fn empty_spans_are_dropped() {
        let strings = text_strings("<svg:tspan>  </svg:tspan><svg:tspan>x</svg:tspan>");
        assert_eq!(strings, vec!["x"]);
    }

    #[test]
    fn joined_strings_concatenates_in_order() {
        let strings = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(joined_strings(&strings).as_deref(), Some("ab"));
        assert_eq!(joined_strings(&[]), None);
    }

    #[test]
    fn names_compare_after_nfc() {
        // A decomposed `e` + combining acute comes out precomposed.
        let strings = text_strings("<svg:tspan>cafe\u{301}</svg:tspan>");
        assert_eq!(strings, vec!["caf\u{e9}"]);
    }
}
