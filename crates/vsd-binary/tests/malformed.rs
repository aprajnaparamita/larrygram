// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The hand-authored malformed fixtures (task P8-10, ADR 0071): a
//! truncated container, a bad FAT, lying record lengths, a pointer
//! cycle and absurd counts. Every fixture is our own work (MIT,
//! `corpus/PROVENANCE.md`), built byte for byte by the builders here
//! and in [`common::cfb`]; a test proves the committed files equal the
//! builder output. Each case must error or warn, never panic (AGENTS.md
//! section 6).

mod common;

use std::fs;
use std::path::PathBuf;

use common::cfb::{Defect, SECTOR, compound};
use larrygram_vsd_binary::{Error, Limits, WarningKind, import};

/// The Visio magic that opens the `VisioDocument` stream.
const MAGIC: &[u8] = b"Visio (TM) Drawing\r\n\0";

/// The byte offset of the version inside the stream header.
const VERSION_OFFSET: usize = 0x1A;

/// The byte offset of the trailer pointer inside the stream header.
const TRAILER_POINTER_OFFSET: usize = 0x24;

/// The length of the stream header up to and including the trailer
/// pointer: the first region follows it.
const STREAM_HEADER_LEN: usize = 0x36;

/// The length of one pointer record.
const POINTER_LEN: usize = 18;

/// The length of one chunk record header.
const CHUNK_HEADER_LEN: usize = 19;

/// `corpus/fixtures/` in the repository root (committed, hand-authored).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("fixtures")
}

/// A little-endian `u32`.
fn le32(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// One pointer record (`Type`, reserved, `Offset`, `Length`, `Format`).
fn pointer(kind: u32, offset: u32, length: u32, format: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(POINTER_LEN);
    out.extend(le32(kind));
    out.extend(le32(0));
    out.extend(le32(offset));
    out.extend(le32(length));
    out.extend_from_slice(&format.to_le_bytes());
    out
}

/// One pointer list: the offset field, the list size (one: no order
/// array), the pointer count, a skipped u32 and the pointers (the
/// layout the chunk walk's pointer-list reader reads).
fn pointer_list(count: u32, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(le32(8)); // the table sits at 8 - 4 = 4
    out.extend(le32(1));
    out.extend(le32(count));
    out.extend(le32(0));
    for entry in entries {
        out.extend(entry);
    }
    out
}

/// One chunk record: the 19-byte header followed by its payload. The
/// header states `declared` as the payload length, which the lying
/// fixture overstates.
fn chunk_record(
    kind: u32,
    id: u32,
    list: u32,
    level: u16,
    declared: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(CHUNK_HEADER_LEN + payload.len());
    out.extend(le32(kind));
    out.extend(le32(id));
    out.extend(le32(list));
    out.extend(le32(declared));
    out.extend_from_slice(&level.to_le_bytes());
    out.push(0);
    out.extend_from_slice(payload);
    out
}

/// One region of the stream: the target of one pointer.
struct Region {
    /// The pointer's `Type`.
    kind: u32,
    /// The pointer's `Format`.
    format: u16,
    /// The region's bytes.
    bytes: Vec<u8>,
}

/// A version 11 `VisioDocument` stream whose trailer list names
/// `regions`, in order.
fn stream(regions: &[Region]) -> Vec<u8> {
    stream_with_count(regions, None)
}

/// [`stream`] with the trailer list's pointer count overstated by
/// `count`: the absurd-counts fixture.
fn stream_with_count(regions: &[Region], count: Option<u32>) -> Vec<u8> {
    let trailer_len = 16 + POINTER_LEN * regions.len();
    let mut offset = STREAM_HEADER_LEN + trailer_len;
    let mut pointers = Vec::new();
    for region in regions {
        pointers.push(pointer(
            region.kind,
            offset as u32,
            region.bytes.len() as u32,
            region.format,
        ));
        offset += region.bytes.len();
    }
    let count = count.unwrap_or(pointers.len() as u32);
    let trailer = pointer_list(count, &pointers);
    let mut out = vec![0u8; STREAM_HEADER_LEN];
    out[0..MAGIC.len()].copy_from_slice(MAGIC);
    out[VERSION_OFFSET] = 11;
    out[TRAILER_POINTER_OFFSET..STREAM_HEADER_LEN].copy_from_slice(&pointer(
        0x14,
        STREAM_HEADER_LEN as u32,
        trailer_len as u32,
        0,
    ));
    out.extend(trailer);
    for region in regions {
        out.extend_from_slice(&region.bytes);
    }
    out
}

/// The region offsets of [`stream_with_count`] for two 34-byte regions:
/// the pointer cycle fixture names them inside its regions.
fn cycle_region_offsets() -> (u32, u32) {
    let trailer_len = 16 + POINTER_LEN * 2;
    let first = (STREAM_HEADER_LEN + trailer_len) as u32;
    (first, first + 34)
}

/// The valid base: a version 11 stream with one chunk-stream region
/// holding a page record and a page-sheet record.
fn minimal_stream() -> Vec<u8> {
    let mut records = Vec::new();
    records.extend(chunk_record(0x15, 1, 0, 1, 8, &[0; 8]));
    records.extend(chunk_record(0x46, 0, 0, 2, 8, &[0; 8]));
    stream(&[Region {
        kind: 0x1a,
        format: 0x80,
        bytes: records,
    }])
}

/// The base with the second record's length lying beyond the region.
fn lying_length_stream() -> Vec<u8> {
    let mut records = Vec::new();
    records.extend(chunk_record(0x15, 1, 0, 1, 8, &[0; 8]));
    records.extend(chunk_record(0x46, 0, 0, 2, 0x0010_0000, &[0; 8]));
    stream(&[Region {
        kind: 0x1a,
        format: 0x80,
        bytes: records,
    }])
}

/// Two blob regions whose nested pointer lists name each other: a
/// pointer cycle the walk must terminate on.
fn pointer_cycle_stream() -> Vec<u8> {
    let (first, second) = cycle_region_offsets();
    stream(&[
        Region {
            kind: 0x15,
            format: 0x50,
            bytes: pointer_list(1, &[pointer(0x15, second, 34, 0x50)]),
        },
        Region {
            kind: 0x15,
            format: 0x50,
            bytes: pointer_list(1, &[pointer(0x15, first, 34, 0x50)]),
        },
    ])
}

/// A trailer list that claims 1073741824 pointers and holds one.
fn absurd_count_stream() -> Vec<u8> {
    let mut records = Vec::new();
    records.extend(chunk_record(0x15, 1, 0, 1, 8, &[0; 8]));
    stream_with_count(
        &[Region {
            kind: 0x1a,
            format: 0x80,
            bytes: records,
        }],
        Some(0x3FFF_FFFF),
    )
}

/// The committed fixtures, named as `corpus/fixtures/` holds them and
/// the bytes the builders produce.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let base = minimal_stream();
    vec![
        (
            "minimal-drawing.vsd",
            compound(&[("VisioDocument", base.as_slice())], Defect::None),
        ),
        (
            "malformed-truncated-container.vsd",
            compound(&[("VisioDocument", base.as_slice())], Defect::None)[..SECTOR + 128].to_vec(),
        ),
        (
            "malformed-bad-fat.vsd",
            compound(&[("VisioDocument", base.as_slice())], Defect::FatCycle),
        ),
        (
            "malformed-lying-record-length.vsd",
            compound(
                &[("VisioDocument", lying_length_stream().as_slice())],
                Defect::None,
            ),
        ),
        (
            "malformed-pointer-cycle.vsd",
            compound(
                &[("VisioDocument", pointer_cycle_stream().as_slice())],
                Defect::None,
            ),
        ),
        (
            "malformed-absurd-count.vsd",
            compound(
                &[("VisioDocument", absurd_count_stream().as_slice())],
                Defect::None,
            ),
        ),
    ]
}

/// The committed fixture bytes, or `None` when the file is absent.
fn committed(name: &str) -> Option<Vec<u8>> {
    fs::read(fixtures_dir().join(name)).ok()
}

/// Every committed fixture equals the builder output byte for byte, so
/// the committed files stay reproducible from this source.
#[test]
fn committed_fixtures_match_the_builder_output() {
    for (name, built) in fixtures() {
        let Some(saved) = committed(name) else {
            panic!("missing fixture {name}; run the builder and commit the file");
        };
        assert_eq!(saved, built, "fixture {name} drifted from the builder");
    }
}

/// Rewrites the committed fixtures from the builders. Ignored: run it
/// only when a fixture changes, then review the diff (`--ignored`).
#[test]
#[ignore = "regenerates the committed fixtures"]
fn write_the_committed_fixtures() {
    for (name, built) in fixtures() {
        fs::create_dir_all(fixtures_dir()).expect("creates corpus/fixtures");
        fs::write(fixtures_dir().join(name), &built).expect("writes the fixture");
        println!("wrote {name} ({} bytes)", built.len());
    }
}

/// The valid base imports into a document without a panic.
#[test]
fn minimal_drawing_imports_as_a_document() {
    let (name, bytes) = &fixtures()[0];
    let imported = import(bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    assert!(imported.document.pages.len() <= 1, "{name}");
}

/// A truncated container is an error, never a panic (ADR 0071).
#[test]
fn a_truncated_container_errors() {
    let bytes = committed("malformed-truncated-container.vsd").expect("fixture is committed");
    let err = import(&bytes, &Limits::DEFAULT).expect_err("truncated container must error");
    assert!(
        matches!(err, Error::NotAVisioFile(_) | Error::Invalid(_)),
        "{err:?}"
    );
}

/// A FAT chain that loops back to its own start is an error, never a
/// panic (the walk is cycle-safe).
#[test]
fn a_bad_fat_errors() {
    let bytes = committed("malformed-bad-fat.vsd").expect("fixture is committed");
    let err = import(&bytes, &Limits::DEFAULT).expect_err("a FAT cycle must error");
    assert!(
        matches!(err, Error::NotAVisioFile(_) | Error::Invalid(_)),
        "{err:?}"
    );
}

/// A record whose header lies about its length warns and keeps the
/// records before it (ADR 0071: skip at the next recoverable boundary).
#[test]
fn a_lying_record_length_warns_and_keeps_earlier_records() {
    let bytes = committed("malformed-lying-record-length.vsd").expect("fixture is committed");
    let imported =
        import(&bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("must import: {e:?}"));
    assert!(
        imported.warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "chunk record length"
            }),
        "{:?}",
        imported.warnings
    );
}

/// A pointer cycle terminates with a warning about the repeated target.
#[test]
fn a_pointer_cycle_terminates_with_a_warning() {
    let bytes = committed("malformed-pointer-cycle.vsd").expect("fixture is committed");
    let imported =
        import(&bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("must import: {e:?}"));
    assert!(
        imported.warnings.iter().any(|w| w.kind
            == WarningKind::Dropped {
                what: "repeated pointer target"
            }),
        "{:?}",
        imported.warnings
    );
}

/// A pointer list that claims a absurd number of pointers reads what is
/// there and warns, within the caps (no huge allocation).
#[test]
fn an_absurd_pointer_count_degrades_with_a_warning() {
    let bytes = committed("malformed-absurd-count.vsd").expect("fixture is committed");
    let imported =
        import(&bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("must import: {e:?}"));
    assert!(
        imported.warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "pointer list"
            }),
        "{:?}",
        imported.warnings
    );
}

/// Every fixture survives truncation and byte damage without a panic
/// (ADR 0071): the input either errors or imports with warnings.
#[test]
fn damaged_fixtures_never_panic() {
    for (name, bytes) in fixtures() {
        for cut in [0, 1, 7, 64, 512, bytes.len() / 2, bytes.len() - 1] {
            let _ = import(&bytes[..cut], &Limits::DEFAULT);
        }
        for at in [0, 1, 512, 1024, 4096] {
            if let Some(byte) = bytes.get(at) {
                let mut damaged = bytes.clone();
                damaged[at] = byte.wrapping_add(1);
                let _ = import(&damaged, &Limits::DEFAULT);
            }
        }
        assert!(!bytes.is_empty(), "{name}");
    }
}
