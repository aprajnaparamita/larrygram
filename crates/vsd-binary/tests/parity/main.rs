// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The oracle parity harness (P8-08, ADR 0072).
//!
//! For every corpus `.vsd` the harness runs the libvisio tools
//! (`vsd2raw` for the structure, `vsd2xhtml` for the text), maps both
//! sides into the neutral structure of [`neutral`], and prints one
//! report per file: compared fields, matching fields, the parity
//! number, one line per mismatch and the not-compared lists.
//!
//! Availability follows ADR 0010 exactly: without an oracle the run
//! skips with a clear message and the gate stays green;
//! `LARRYGRAM_REQUIRE_ORACLE=1` turns a missing oracle into a failure.
//! The corpus is optional the same way (`LARRYGRAM_REQUIRE_CORPUS=1`).
//!
//! ```sh
//! LARRYGRAM_LIBVISIO_ORACLE=/Volumes/Dara/larrygram/build/libvisio-oracle/bin \
//!   cargo test -p larrygram-vsd-binary --test parity -- --ignored --nocapture
//! ```

mod compare;
mod dump;
mod neutral;
mod oracle;
mod ours;
mod xhtml;

use std::path::{Path, PathBuf};

use compare::Report;

/// Drives the whole comparison for one file: dump text and XHTML text
/// in, report out. `import` is our side; `None` means our import is
/// not available yet and the report says so.
pub fn report_for(
    file: &str,
    dump_text: &str,
    xhtml_text: &str,
    import: Option<&larrygram_vsd_binary::Import>,
    note: Option<String>,
) -> Report {
    let oracle = dump::parse(dump_text);
    let oracle_texts = xhtml::text_strings(xhtml_text);
    let Some(import) = import else {
        let mut report = Report {
            file: file.to_owned(),
            ..Report::default()
        };
        let reason = note.unwrap_or_else(|| "our side is not available".to_owned());
        report.note = Some(format!(
            "{reason}; the oracle side parsed {} page(s) and {} text string(s)",
            oracle.pages.len(),
            oracle_texts.len()
        ));
        return report;
    };
    let ours = ours::doc(import);
    let mut report = compare::compare(file, &oracle, &ours);
    compare::compare_text(&mut report, &oracle_texts, &compare::shape_texts(&ours));
    if let Some(note) = note {
        report.note = Some(note);
    }
    report
}

/// The corpus directory when it has been fetched (ADR 0005).
fn corpus_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vsd");
    dir.is_dir().then_some(dir)
}

/// The `.vsd` files of the corpus, sorted by name.
fn corpus_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().map(|e| e == "vsd").unwrap_or(false))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// Runs the harness over every corpus file and prints the reports
/// (P8-08 goals 1 to 3).
///
/// Ignored by default: it needs the libvisio oracle (ADR 0010) and the
/// fetched corpus (ADR 0005).
#[test]
#[ignore = "needs the libvisio oracle and the corpus; see ADR 0010"]
fn parity_reports_over_the_corpus() {
    let oracle_dir = match oracle::availability() {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            eprintln!(
                "skipping the parity run: no oracle; set LARRYGRAM_LIBVISIO_ORACLE (ADR 0010)"
            );
            return;
        }
        Err(err) => panic!("{err}"),
    };
    let Some(dir) = corpus_dir() else {
        assert!(
            std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_none(),
            "corpus/vsd is missing and LARRYGRAM_REQUIRE_CORPUS=1 is set"
        );
        eprintln!("skipping the parity run: corpus/vsd is absent; run `cargo xtask corpus`");
        return;
    };

    let mut reports = Vec::new();
    for file in corpus_files(&dir) {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dump_text = match oracle::run_tool(&oracle_dir, "vsd2raw", &file) {
            Ok(text) => text,
            Err(err) => panic!("vsd2raw failed on {name}: {err}"),
        };
        let xhtml_text = match oracle::run_tool(&oracle_dir, "vsd2xhtml", &file) {
            Ok(text) => text,
            Err(err) => panic!("vsd2xhtml failed on {name}: {err}"),
        };
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(err) => panic!("cannot read {name}: {err}"),
        };
        let (import, note) =
            match larrygram_vsd_binary::import(&bytes, &larrygram_vsd_binary::Limits::DEFAULT) {
                Ok(import) => (Some(import), None),
                Err(larrygram_vsd_binary::Error::NotImplemented) => (
                    None,
                    Some("our import is still the P8-01 stub (P8-07 assembles it)".to_owned()),
                ),
                Err(err) => (None, Some(format!("our import failed: {err}"))),
            };
        reports.push(report_for(
            &name,
            &dump_text,
            &xhtml_text,
            import.as_ref(),
            note,
        ));
    }

    let mut compared = 0usize;
    let mut matching = 0usize;
    for report in &reports {
        println!("{}", report.render());
        compared += report.compared;
        matching += report.matching;
    }
    if compared > 0 {
        println!(
            "total parity over {} file(s): {:.1}% ({}/{})",
            reports.len(),
            100.0 * matching as f64 / compared as f64,
            matching,
            compared
        );
    } else {
        println!(
            "total parity over {} file(s): nothing compared yet (see the notes)",
            reports.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built dump and a hand-built import that agree.
    const DUMP: &str = "\
startDocument()
  setDocumentMetaData(dc:creator: Ada, dc:title: T)
  startPage(draw:name: Page-1, svg:height: 10.0000in, svg:width: 8.0000in)
    startLayer (draw:id: id1)
      setStyle(draw:fill: solid, draw:fill-color: #5b9bd5, draw:stroke: none, svg:stroke-color: #000000, svg:stroke-width: 0.0139in)
      drawPath (svg:d: ((librevenge:path-action: M, svg:x: 1.0000in, svg:y: 8.0000in), (librevenge:path-action: L, svg:x: 3.0000in, svg:y: 6.0000in), (librevenge:path-action: Z)))
      startTextObject (svg:x: 0.5000in, svg:y: 4.0000in, svg:width: 3.0000in, svg:height: 4.0000in, librevenge:rotate: 0.0000)
        openSpan()
          insertText (Hi)
        closeSpan
      endTextObject
    endLayer
  endPage
endDocument
";

    const XHTML: &str = r#"<html><body>
<svg:g id="Layer1"><svg:text><svg:tspan>Hi</svg:tspan></svg:text></svg:g>
</body></html>"#;

    /// The import matching [`DUMP`]: one page (8 x 10 in) with one
    /// shape whose geometry and text box land where the dump says.
    fn matching_import() -> larrygram_vsd_binary::Import {
        use larrygram_vsd_binary::model::{
            Document, Page as ModelPage, PageId, Property, Row, Section, SectionName,
            Shape as ModelShape, ShapeId, TextContent, TextPart,
        };
        use ours::{bool_cell, color_cell, number_cell};

        let mut document = Document::new();
        document.properties.push(Property::new("title", "T"));
        document.properties.push(Property::new("creator", "Ada"));

        let mut shape = ModelShape::new(ShapeId { index: 0 }, 7);
        shape.sheet.cells.extend([
            number_cell("PinX", 1.5),
            number_cell("PinY", 2.0),
            number_cell("LocPinX", 1.5),
            number_cell("LocPinY", 2.0),
            number_cell("Width", 3.0),
            number_cell("Height", 4.0),
            number_cell("Angle", 0.0),
            bool_cell("FlipX", false),
            bool_cell("FlipY", false),
            number_cell("FillPattern", 1.0),
            color_cell("FillForegnd", 0x5b, 0x9b, 0xd5),
            number_cell("LinePattern", 0.0),
            color_cell("LineColor", 0, 0, 0),
            number_cell("LineWeight", 0.0139),
            number_cell("ShadowPattern", 0.0),
        ]);
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text("Hi".to_owned())],
        });
        // The text block pins where the dump's text object is.
        shape.sheet.cells.extend([
            number_cell("TxtPinX", 2.0),
            number_cell("TxtPinY", 2.0),
            number_cell("TxtLocPinX", 1.5),
            number_cell("TxtLocPinY", 0.0),
            number_cell("TxtWidth", 3.0),
            number_cell("TxtHeight", 4.0),
            number_cell("TxtAngle", 0.0),
        ]);
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut move_to = Row::new();
        move_to.row_type = Some("MoveTo".to_owned());
        move_to.cells.push(number_cell("X", 1.0));
        move_to.cells.push(number_cell("Y", 2.0));
        let mut line_to = Row::new();
        line_to.row_type = Some("LineTo".to_owned());
        line_to.cells.push(number_cell("X", 3.0));
        line_to.cells.push(number_cell("Y", 4.0));
        section.rows.push(move_to);
        section.rows.push(line_to);
        shape.sheet.sections.push(section);
        let id = document.add_shape(shape);

        let mut page = ModelPage::new(PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        page.sheet.cells.extend([
            number_cell("PageWidth", 8.0),
            number_cell("PageHeight", 10.0),
        ]);
        page.shapes.push(id);
        document.pages.push(page);

        larrygram_vsd_binary::Import {
            document,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn agreeing_hand_built_sides_report_full_parity() {
        let import = matching_import();
        let report = report_for("fixture.vsd", DUMP, XHTML, Some(&import), None);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        assert!(report.compared > 20);
        assert_eq!(report.parity(), 100.0);
    }

    #[test]
    fn the_xhtml_text_order_is_compared() {
        let import = matching_import();
        let other = "<svg:tspan>Hi</svg:tspan><svg:tspan>Extra</svg:tspan>";
        let report = report_for("fixture.vsd", DUMP, other, Some(&import), None);
        assert!(
            report
                .mismatches
                .iter()
                .any(|m| m.field.starts_with("xhtml.text"))
        );
    }

    #[test]
    fn a_missing_import_reports_the_oracle_side_only() {
        let report = report_for(
            "fixture.vsd",
            DUMP,
            XHTML,
            None,
            Some("our import is still the P8-01 stub".to_owned()),
        );
        let rendered = report.render();
        assert!(rendered.contains("P8-01 stub"), "{rendered}");
        assert!(rendered.contains("page(s)"), "{rendered}");
        assert_eq!(report.compared, 0);
    }

    #[test]
    fn corpus_files_are_sorted_and_filtered() {
        let dir = std::env::temp_dir().join(format!(
            "larrygram-parity-corpus-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.vsd"), b"").unwrap();
        std::fs::write(dir.join("a.vsd"), b"").unwrap();
        std::fs::write(dir.join("c.txt"), b"").unwrap();
        let names: Vec<String> = corpus_files(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.vsd".to_owned(), "b.vsd".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_corpus_directory_is_optional() {
        if let Some(dir) = corpus_dir() {
            assert!(dir.is_dir());
        }
    }
}
