// Temporary debug dump (not for hand-off).
#![allow(missing_docs)]
use larrygram_vsd_binary::model::{CellValue, Document, ShapeId};
use larrygram_vsd_binary::{Limits, import};

fn cell_text(v: &CellValue) -> String {
    match v {
        CellValue::Number(n) => format!("{n:.4}"),
        CellValue::Bool(b) => format!("{b}"),
        CellValue::Str(s) => s.clone(),
        _ => "?".to_owned(),
    }
}

fn dump(doc: &Document, what: &str) {
    for master in &doc.masters {
        println!(
            "== master visio_id={} name={:?} ({})",
            master.visio_id, master.name, what
        );
        for id in &master.shapes {
            dump_shape(doc, *id, 0);
        }
    }
    for page in &doc.pages {
        println!("== page {} ({})", page.name, what);
        for id in &page.shapes {
            dump_shape(doc, *id, 0);
        }
    }
}

fn dump_shape(doc: &Document, id: ShapeId, depth: usize) {
    let Some(shape) = doc.shape(id) else {
        return;
    };
    let pad = "  ".repeat(depth + 1);
    println!(
        "{}shape visio_id={} name={:?} master={:?} children={:?}",
        pad, shape.visio_id, shape.name, shape.master, shape.children
    );
    for section in &shape.sheet.sections {
        if section.name.as_str() != "Geometry" {
            continue;
        }
        let flags: Vec<String> = section
            .cells
            .iter()
            .map(|c| {
                format!(
                    "{}={}",
                    c.name.as_str(),
                    c.value.as_ref().map(cell_text).unwrap_or_default()
                )
            })
            .collect();
        println!("{}  Geometry cells {:?}", pad, flags);
        for (i, row) in section.rows.iter().enumerate() {
            let cells: Vec<String> = row
                .cells
                .iter()
                .map(|c| {
                    format!(
                        "{}={}",
                        c.name.as_str(),
                        c.value.as_ref().map(cell_text).unwrap_or_default()
                    )
                })
                .collect();
            println!(
                "{}   row[{i}] {:?} ix={:?} {}",
                pad,
                row.row_type,
                row.ix,
                cells.join(" ")
            );
        }
    }
    match doc.shape_geometry(id) {
        Ok(geo) => {
            for (i, path) in geo.paths.iter().enumerate() {
                println!(
                    "{}  path[{i}] no_fill={} no_line={} no_show={} rows={}",
                    pad,
                    path.no_fill,
                    path.no_line,
                    path.no_show,
                    path.rows.len()
                );
                for (j, row) in path.rows.iter().enumerate() {
                    println!("{}    row[{j}] {:?}", pad, row);
                }
            }
        }
        Err(err) => println!("{}  shape_geometry error {err}", pad),
    }
    for child in &shape.children {
        dump_shape(doc, *child, depth + 1);
    }
}

#[test]
fn scratch_chunks() {
    let name = std::env::var("SCRATCH_FILE").unwrap_or_else(|_| "no-bgcolor.vsd".to_owned());
    let path = format!("{}/../../corpus/vsd/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap();
    let streams = larrygram_vsd_binary::read_streams(&bytes, &larrygram_vsd_binary::Limits::DEFAULT).unwrap();
    let data = streams.visio_document().unwrap_or_default();
    let version = larrygram_vsd_binary::version(data).unwrap();
    let mut blocks = Vec::new();
    let (chunks, _warnings) = larrygram_vsd_binary::read_chunks(
        data,
        version,
        &larrygram_vsd_binary::Limits::DEFAULT,
        &mut blocks,
    )
    .unwrap();
    let want = std::env::var("SCRATCH_REC").ok();
    for (index, chunk) in chunks.iter().enumerate() {
        let mut extra = String::new();
        {
            // parse X/Y cells of row payloads
            if matches!(
                chunk.record.name(),
                "MOVE_TO" | "LINE_TO" | "ARC_TO" | "ELLIPTICAL_ARC_TO" | "ELLIPSE" | "SPLINE_START" | "SPLINE_KNOT" | "INFINITE_LINE"
            ) {
                let mut cells = Vec::new();
                let mut at = 0usize;
                while at + 9 <= chunk.data.len() && cells.len() < 4 {
                    let tag = chunk.data[at];
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&chunk.data[at + 1..at + 9]);
                    cells.push(format!("tag{tag}={}", f64::from_le_bytes(buf)));
                    at += 9;
                }
                extra = format!(" cells {}", cells.join(" "));
            }
        }
        let skip = match &want {
            Some(rec) => chunk.record.name() != rec,
            None => false,
        };
        if skip {
            continue;
        }
        println!(
            "chunk #{index} id={} level={} {} len={}{}{}",
            chunk.id,
            chunk.level,
            chunk.record.name(),
            chunk.data.len(),
            extra,
            if std::env::var_os("SCRATCH_HEX").is_some() {
                format!(
                    " hex={}",
                    chunk
                        .data
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            } else {
                String::new()
            }
        );
    }
}

#[test]
fn scratch_counts() {
    let name = std::env::var("SCRATCH_FILE").unwrap_or_else(|_| "no-bgcolor.vsd".to_owned());
    let path = format!("{}/../../corpus/vsd/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap();
    let import = import(&bytes, &Limits::DEFAULT).unwrap();
    let doc = &import.document;
    println!(
        "pages={} shapes={} masters={}",
        doc.pages.len(),
        doc.shapes.len(),
        doc.masters.len()
    );
    for page in &doc.pages {
        let mut reader = larrygram_vsd_binary::model::CellReader::new(doc);
        let sheet = larrygram_vsd_binary::model::SheetRef::Page(page.id);
        let w = reader
            .number(sheet, &larrygram_vsd_binary::model::CellName::from_static("PageWidth"))
            .unwrap_or(-1.0);
        let h = reader
            .number(sheet, &larrygram_vsd_binary::model::CellName::from_static("PageHeight"))
            .unwrap_or(-1.0);
        println!("page {:?} shapes={} size={}x{}", page.name, page.shapes.len(), w, h);
    }
    let mut reader = larrygram_vsd_binary::model::CellReader::new(doc);
    for shape in &doc.shapes {
        let sheet = larrygram_vsd_binary::model::SheetRef::Shape(shape.id);
        let lp = reader
            .number(sheet, &larrygram_vsd_binary::model::CellName::new("LinePattern"))
            .unwrap_or(-1.0);
        let lw = reader
            .number(sheet, &larrygram_vsd_binary::model::CellName::new("LineWeight"))
            .unwrap_or(-1.0);
        let fp = reader
            .number(sheet, &larrygram_vsd_binary::model::CellName::new("FillPattern"))
            .unwrap_or(-1.0);
        println!(
            "shape {} visio={} LinePattern={} LineWeight={} FillPattern={}",
            shape.id.index, shape.visio_id, lp, lw, fp
        );
    }
}

#[test]
fn scratch_dump() {
    let name = std::env::var("SCRATCH_FILE").unwrap_or_else(|_| "no-bgcolor.vsd".to_owned());
    let path = format!("{}/../../corpus/vsd/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap();
    let import = import(&bytes, &Limits::DEFAULT).unwrap();
    dump(&import.document, "vsd");
    for w in &import.warnings {
        println!("warning: {w:?}");
    }
}
