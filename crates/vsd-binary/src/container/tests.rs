// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tests for the compound file container and the stream layer (P8-02).

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// The size of one sector of the test files.
const SECTOR: usize = 512;

/// The size of one mini sector of the test files.
const MINI: usize = 64;

/// The mini stream cutoff of the test files.
const CUTOFF: u64 = 4096;

/// The number of FAT entries one sector holds.
const FAT_ENTRIES: usize = SECTOR / 4;

/// Where a stream of a test file lives.
#[derive(Clone, Copy)]
enum Placement {
    /// The stream lives in the mini stream.
    Mini { start: u32, size: usize },
    /// The stream lives in sectors of its own.
    Normal { start: u32, size: usize },
}

/// A tiny compound file writer, for the tests only.
///
/// It writes a version 3 file with 512 byte sectors: the FAT, the
/// directory, the mini FAT, the mini stream and the stream sectors.
/// Small streams (below the cutoff) go into the mini stream, so both
/// allocation paths are covered.
struct TestFile {
    streams: Vec<(String, Vec<u8>)>,
}

impl TestFile {
    /// A file with no streams.
    fn new() -> TestFile {
        TestFile {
            streams: Vec::new(),
        }
    }

    /// A file with one more stream.
    fn with_stream(mut self, name: &str, data: &[u8]) -> TestFile {
        self.streams.push((name.to_owned(), data.to_vec()));
        self
    }

    /// The bytes of the file.
    fn build(&self) -> Vec<u8> {
        let mut sectors: Vec<Vec<u8>> = Vec::new();
        let mut fat: Vec<u32> = Vec::new();
        let mut mini_stream: Vec<u8> = Vec::new();
        let mut mini_fat: Vec<u32> = Vec::new();
        let mut placement: Vec<Placement> = Vec::with_capacity(self.streams.len());

        // Sector 0 holds the FAT itself.
        sectors.push(vec![0u8; SECTOR]);
        fat.push(FATSECT);

        // The directory follows: the root entry and one entry per
        // stream.
        let dir_count = (1 + self.streams.len()).div_ceil(4);
        let dir_start = sectors.len() as u32;
        for _ in 0..dir_count {
            sectors.push(vec![0u8; SECTOR]);
            fat.push(ENDOFCHAIN);
        }
        chain(&mut fat, dir_start, dir_count);

        // The small streams go into the mini stream.
        for (_, data) in &self.streams {
            if (data.len() as u64) < CUTOFF {
                let start = (mini_stream.len() / MINI) as u32;
                let count = data.len().div_ceil(MINI);
                for index in 0..count {
                    mini_fat.push(if index + 1 == count {
                        ENDOFCHAIN
                    } else {
                        start + index as u32 + 1
                    });
                }
                mini_stream.extend_from_slice(data);
                while !mini_stream.len().is_multiple_of(MINI) {
                    mini_stream.push(0);
                }
                placement.push(Placement::Mini {
                    start,
                    size: data.len(),
                });
            } else {
                placement.push(Placement::Normal {
                    start: 0,
                    size: data.len(),
                });
            }
        }

        // The mini FAT.
        let mini_fat_start = if mini_fat.is_empty() {
            ENDOFCHAIN
        } else {
            sectors.len() as u32
        };
        let mini_fat_count = mini_fat.len().div_ceil(FAT_ENTRIES);
        for _ in 0..mini_fat_count {
            sectors.push(vec![0u8; SECTOR]);
            fat.push(ENDOFCHAIN);
        }
        chain(&mut fat, mini_fat_start, mini_fat_count);
        for index in 0..mini_fat_count {
            let mut sector = vec![0u8; SECTOR];
            for entry in 0..FAT_ENTRIES {
                let value = mini_fat.get(index * FAT_ENTRIES + entry).copied();
                put_u32(&mut sector, entry * 4, value.unwrap_or(FREESECT));
            }
            sectors[mini_fat_start as usize + index] = sector;
        }

        // The mini stream: the root entry's chain.
        let mini_start = if mini_stream.is_empty() {
            ENDOFCHAIN
        } else {
            sectors.len() as u32
        };
        let mini_count = mini_stream.len().div_ceil(SECTOR);
        for _ in 0..mini_count {
            sectors.push(vec![0u8; SECTOR]);
            fat.push(ENDOFCHAIN);
        }
        chain(&mut fat, mini_start, mini_count);
        for index in 0..mini_count {
            let from = index * SECTOR;
            let mut sector = vec![0u8; SECTOR];
            if from < mini_stream.len() {
                let to = (from + SECTOR).min(mini_stream.len());
                sector[..to - from].copy_from_slice(&mini_stream[from..to]);
            }
            sectors[mini_start as usize + index] = sector;
        }

        // The big streams get sectors of their own.
        for (index, (_, data)) in self.streams.iter().enumerate() {
            if (data.len() as u64) < CUTOFF {
                continue;
            }
            let count = data.len().div_ceil(SECTOR);
            let start = sectors.len() as u32;
            for _ in 0..count {
                sectors.push(vec![0u8; SECTOR]);
                fat.push(ENDOFCHAIN);
            }
            chain(&mut fat, start, count);
            for sector_index in 0..count {
                let from = sector_index * SECTOR;
                let to = (from + SECTOR).min(data.len());
                let mut sector = vec![0u8; SECTOR];
                sector[..to - from].copy_from_slice(&data[from..to]);
                sectors[start as usize + sector_index] = sector;
            }
            placement[index] = Placement::Normal {
                start,
                size: data.len(),
            };
        }

        // The FAT.
        let mut fat_sector = vec![0u8; SECTOR];
        for entry in 0..FAT_ENTRIES {
            let value = fat.get(entry).copied();
            put_u32(&mut fat_sector, entry * 4, value.unwrap_or(FREESECT));
        }
        sectors[0] = fat_sector;

        // The directory entries.
        let mut directory = vec![0u8; dir_count * SECTOR];
        write_entry(
            &mut directory,
            0,
            "Root Entry",
            5,
            mini_start,
            mini_stream.len() as u64,
        );
        for (index, (name, _)) in self.streams.iter().enumerate() {
            let (start, size) = match placement[index] {
                Placement::Mini { start, size } => (start, size),
                Placement::Normal { start, size } => (start, size),
            };
            write_entry(&mut directory, index + 1, name, 2, start, size as u64);
        }
        for index in 0..dir_count {
            sectors[dir_start as usize + index] =
                directory[index * SECTOR..(index + 1) * SECTOR].to_vec();
        }

        // The header.
        let mut header = vec![0u8; SECTOR];
        header[0..8].copy_from_slice(&SIGNATURE);
        put_u16(&mut header, 24, 0x003E);
        put_u16(&mut header, 26, 3);
        put_u16(&mut header, 28, 0xFFFE);
        put_u16(&mut header, 30, 9);
        put_u16(&mut header, 32, 6);
        put_u32(&mut header, 44, 1);
        put_u32(&mut header, 48, dir_start);
        put_u32(&mut header, 56, CUTOFF as u32);
        put_u32(&mut header, 60, mini_fat_start);
        put_u32(&mut header, 64, mini_fat_count as u32);
        put_u32(&mut header, 68, ENDOFCHAIN);
        put_u32(&mut header, 72, 0);
        put_u32(&mut header, 76, 0);
        for index in 1..HEADER_DIFAT_ENTRIES {
            put_u32(&mut header, 76 + index * 4, FREESECT);
        }

        let mut file = header;
        for sector in &sectors {
            file.extend_from_slice(sector);
        }
        file
    }
}

/// Chains `count` sectors from `start` in the FAT.
fn chain(fat: &mut [u32], start: u32, count: usize) {
    for index in 0..count {
        let value = if index + 1 == count {
            ENDOFCHAIN
        } else {
            start + index as u32 + 1
        };
        if let Some(slot) = fat.get_mut(start as usize + index) {
            *slot = value;
        }
    }
}

/// Writes one directory entry.
fn write_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    start: u32,
    size: u64,
) {
    let base = index * DIRECTORY_ENTRY_SIZE;
    let units: Vec<u16> = name.encode_utf16().collect();
    for (unit_index, unit) in units.iter().enumerate() {
        let at = base + unit_index * 2;
        directory[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }
    put_u16(directory, base + 64, ((units.len() + 1) * 2) as u16);
    if let Some(byte) = directory.get_mut(base + 66) {
        *byte = object_type;
    }
    if let Some(byte) = directory.get_mut(base + 67) {
        *byte = 1;
    }
    put_u32(directory, base + 68, FREESECT);
    put_u32(directory, base + 72, FREESECT);
    put_u32(directory, base + 76, FREESECT);
    put_u32(directory, base + 116, start);
    directory[base + 120..base + 128].copy_from_slice(&size.to_le_bytes());
}

/// Writes a little-endian `u16`.
fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// Writes a little-endian `u32`.
fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// The offset of the second directory entry in a test file: the
/// directory starts in the first sector after the header.
fn second_entry_offset() -> usize {
    SECTOR + SECTOR + DIRECTORY_ENTRY_SIZE
}

/// Builds a summary stream with the given properties.
fn build_summary(fmtid: &[u8; 16], properties: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut stream = vec![0u8; 0x30];
    put_u16(&mut stream, 0, 0xFFFE);
    put_u32(&mut stream, 4, 0x0002_0206);
    put_u32(&mut stream, 0x18, 1);
    stream[0x1C..0x2C].copy_from_slice(fmtid);
    put_u32(&mut stream, 0x2C, 0x30);
    let table_size = 8 + properties.len() * 8;
    let mut table: Vec<u8> = Vec::new();
    let mut values: Vec<u8> = Vec::new();
    for (id, value) in properties {
        table.extend_from_slice(&id.to_le_bytes());
        table.extend_from_slice(&((table_size + values.len()) as u32).to_le_bytes());
        values.extend_from_slice(value);
    }
    let size = 8 + table.len() + values.len();
    let mut section: Vec<u8> = Vec::new();
    section.extend_from_slice(&(size as u32).to_le_bytes());
    section.extend_from_slice(&(properties.len() as u32).to_le_bytes());
    section.extend_from_slice(&table);
    section.extend_from_slice(&values);
    stream.extend_from_slice(&section);
    stream
}

/// A typed property value: the type, two padding bytes, the payload.
fn lpstr_value(text: &[u8]) -> Vec<u8> {
    let mut value = vec![0x1E, 0, 0, 0];
    value.extend_from_slice(&(text.len() as u32).to_le_bytes());
    value.extend_from_slice(text);
    value
}

/// A typed property value: a 16 bit integer.
fn i2_value(number: u16) -> Vec<u8> {
    let mut value = vec![0x02, 0, 0, 0];
    value.extend_from_slice(&number.to_le_bytes());
    value
}

#[test]
fn streams_get_finds_a_stream_by_name() {
    let streams = Streams {
        streams: vec![
            Stream {
                name: VISIO_DOCUMENT.to_owned(),
                data: vec![1, 2, 3],
            },
            Stream {
                name: "\x05SummaryInformation".to_owned(),
                data: vec![],
            },
        ],
    };
    assert_eq!(streams.get(VISIO_DOCUMENT), Some([1u8, 2, 3].as_slice()));
    assert_eq!(streams.get("\x05SummaryInformation"), Some([].as_slice()));
    assert_eq!(streams.get("Missing"), None);
}

#[test]
fn visio_document_is_the_named_stream() {
    let empty = Streams::default();
    assert_eq!(empty.visio_document(), None);
    let streams = Streams {
        streams: vec![Stream {
            name: VISIO_DOCUMENT.to_owned(),
            data: vec![9],
        }],
    };
    assert_eq!(streams.visio_document(), Some([9u8].as_slice()));
    assert_eq!(streams.summary_information(), None);
    assert_eq!(streams.document_summary_information(), None);
}

#[test]
fn read_streams_reads_every_stream_of_a_built_file() {
    let big = vec![7u8; 5000];
    let file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, VISIO_MAGIC)
        .with_stream("Small", b"small")
        .with_stream("Big", &big)
        .build();
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    assert_eq!(streams.streams.len(), 3);
    assert_eq!(streams.get(VISIO_DOCUMENT), Some(VISIO_MAGIC));
    assert_eq!(streams.get("Small"), Some(b"small".as_slice()));
    assert_eq!(streams.get("Big").map(<[u8]>::len), Some(5000));
    assert_eq!(streams.get("Big").map(|data| data[4999]), Some(7));
}

#[test]
fn read_streams_keeps_the_directory_order() {
    let file = TestFile::new()
        .with_stream("Second", b"2")
        .with_stream(VISIO_DOCUMENT, b"1")
        .build();
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    let names: Vec<&str> = streams
        .streams
        .iter()
        .map(|stream| stream.name.as_str())
        .collect();
    assert_eq!(names, vec!["Second", VISIO_DOCUMENT]);
}

#[test]
fn read_streams_refuses_a_file_without_the_signature() {
    let err = read_streams(b"not a compound file at all", &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::NotAVisioFile(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_an_empty_file() {
    let err = read_streams(&[], &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::NotAVisioFile(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_truncated_header() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    file.truncate(200);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_wrong_byte_order() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u16(&mut file, 28, 0xFEFF);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_an_unknown_version() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u16(&mut file, 26, 7);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_fat_cycle() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The FAT lives in the first sector: make the directory chain
    // point at itself.
    put_u32(&mut file, SECTOR + 4, 1);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_fat_cycle() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The mini FAT is the sector after the directory: make its first
    // entry point at itself.
    let mini_fat_sector = 2u32;
    let offset = (mini_fat_sector as usize + 1) * SECTOR;
    put_u32(&mut file, offset, 0);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_lying_stream_size() {
    let mut file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 100])
        .build();
    // The stream claims more bytes than its chain holds.
    let entry = second_entry_offset();
    file[entry + 120..entry + 128].copy_from_slice(&100_000u64.to_le_bytes());
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_stream_over_the_cap() {
    let file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 100])
        .build();
    let limits = Limits {
        max_stream_bytes: 10,
        ..Limits::DEFAULT
    };
    let err = read_streams(&file, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_stream_bytes",
            value: 100,
            max: 10
        }
    );
}

#[test]
fn read_streams_refuses_too_many_streams() {
    let file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, b"x")
        .with_stream("Other", b"y")
        .build();
    let limits = Limits {
        max_streams: 1,
        ..Limits::DEFAULT
    };
    let err = read_streams(&file, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_streams",
            value: 2,
            max: 1
        }
    );
}

#[test]
fn read_streams_refuses_a_total_over_the_cap() {
    let file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 5000])
        .with_stream("Other", &[1u8; 5000])
        .build();
    let limits = Limits {
        max_total_uncompressed: 6000,
        ..Limits::DEFAULT
    };
    let err = read_streams(&file, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_total_uncompressed",
            value: 10000,
            max: 6000
        }
    );
}

#[test]
fn read_streams_refuses_a_file_without_the_drawing_stream() {
    let file = TestFile::new().with_stream("Other", b"x").build();
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::NotAVisioFile(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_fat_sector_outside_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The header names a FAT sector that is not in the file.
    put_u32(&mut file, 76, 9999);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn decompress_reads_a_block_of_literals() {
    let data = [0xFFu8, 1, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(
        decompress(&data, &Limits::DEFAULT).unwrap(),
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    );
}

#[test]
fn decompress_expands_a_back_reference() {
    // One literal "A" and a back reference of three bytes at distance
    // one: the block decodes to "AAAA".
    let data = [0x01u8, b'A', 0xEE, 0xF0];
    assert_eq!(
        decompress(&data, &Limits::DEFAULT).unwrap(),
        b"AAAA".to_vec()
    );
}

#[test]
fn decompress_reads_a_block_of_nothing() {
    assert!(decompress(&[], &Limits::DEFAULT).unwrap().is_empty());
}

#[test]
fn decompress_stops_at_a_truncated_block() {
    // A flag byte without a token behind it.
    assert!(decompress(&[0x01], &Limits::DEFAULT).unwrap().is_empty());
    // A back reference without its second byte.
    assert!(
        decompress(&[0x00, 0xEE], &Limits::DEFAULT)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn decompress_refuses_an_output_over_the_cap() {
    let data = [0xFFu8, 1, 2, 3, 4, 5, 6, 7, 8];
    let limits = Limits {
        max_stream_bytes: 4,
        ..Limits::DEFAULT
    };
    let err = decompress(&data, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_stream_bytes",
            value: 5,
            max: 4
        }
    );
}

#[test]
fn decompress_refuses_an_expansion_over_the_ratio() {
    // A back reference of eighteen bytes in a block of three: the
    // expansion is six times the input.
    let data = [0x00u8, 0xEE, 0xFF];
    let limits = Limits {
        max_compression_ratio: 2,
        ..Limits::DEFAULT
    };
    let err = decompress(&data, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_compression_ratio",
            value: 7,
            max: 6
        }
    );
}

#[test]
fn summary_properties_are_read_with_their_names() {
    let stream = build_summary(
        &FMTID_SUMMARY,
        &[
            (1, i2_value(1252)),
            (2, lpstr_value(b"Title test\0")),
            (4, lpstr_value(b"Miha Varga\0")),
        ],
    );
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(properties.len(), 2);
    assert_eq!(properties[0].name, "title");
    assert_eq!(
        properties[0].value,
        SummaryValue::Text("Title test".to_owned())
    );
    assert_eq!(properties[1].name, "creator");
    assert_eq!(
        properties[1].value,
        SummaryValue::Text("Miha Varga".to_owned())
    );
}

#[test]
fn summary_properties_of_the_document_set_use_their_own_names() {
    let stream = build_summary(
        &FMTID_DOCUMENT_SUMMARY,
        &[
            (1, i2_value(1252)),
            (2, lpstr_value(b"Category test\0")),
            (15, lpstr_value(b"Company test\0")),
        ],
    );
    let properties = read_summary_properties(&stream).unwrap();
    let names: Vec<&str> = properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(names, vec!["category", "Company"]);
}

#[test]
fn summary_strings_use_the_code_page_of_the_stream() {
    let stream = build_summary(
        &FMTID_SUMMARY,
        &[(1, i2_value(1252)), (2, lpstr_value(b"mytitle\xe9\0"))],
    );
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(
        properties[0].value,
        SummaryValue::Text("mytitleé".to_owned())
    );
}

#[test]
fn summary_strings_may_be_utf8() {
    let stream = build_summary(
        &FMTID_SUMMARY,
        &[
            (1, i2_value(65001)),
            (2, lpstr_value("mytitleé".as_bytes())),
        ],
    );
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(
        properties[0].value,
        SummaryValue::Text("mytitleé".to_owned())
    );
}

#[test]
fn summary_properties_of_an_unknown_set_are_left_out() {
    let stream = build_summary(&[0u8; 16], &[(2, lpstr_value(b"Title\0"))]);
    assert!(read_summary_properties(&stream).unwrap().is_empty());
}

#[test]
fn summary_properties_refuse_a_short_stream() {
    let err = read_summary_properties(&[0xFE, 0xFF]).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_wrong_byte_order() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[]);
    put_u16(&mut stream, 0, 0xFEFF);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

/// The corpus directory with the real `.vsd` files.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("vsd")
}

/// The `.vsd` files of the corpus, sorted for a stable order.
fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match fs::read_dir(corpus_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsd"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

#[test]
fn the_corpus_files_open_and_carry_the_visio_magic() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!("skipping: corpus/vsd is absent; run `cargo xtask corpus`");
        return;
    }
    for path in files {
        let bytes = fs::read(&path).unwrap();
        let streams = read_streams(&bytes, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let document = streams
            .visio_document()
            .unwrap_or_else(|| panic!("{}: no VisioDocument stream", path.display()));
        assert!(
            document.starts_with(VISIO_MAGIC),
            "{}: the VisioDocument stream does not start with the magic",
            path.display()
        );
    }
}

#[test]
fn the_corpus_files_carry_their_summary_streams() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!("skipping: corpus/vsd is absent; run `cargo xtask corpus`");
        return;
    }
    for path in files {
        let bytes = fs::read(&path).unwrap();
        let streams = read_streams(&bytes, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let summary = streams.summary_information();
        if let Some(summary) = summary {
            let properties = read_summary_properties(summary)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                !properties.is_empty(),
                "{}: the summary stream has no property",
                path.display()
            );
        }
    }
}

/// The offset of a sector in a test file.
fn sector_offset(sector: u32) -> usize {
    (sector as usize + 1) * SECTOR
}

/// Adds a DIFAT sector to a built file.
///
/// The sector names the FAT sectors in `fat_sectors` and chains to
/// `next`; the header and the FAT are patched to match.
fn add_difat(file: &mut Vec<u8>, fat_sectors: &[u32], next: u32) -> u32 {
    let sector = ((file.len() - SECTOR) / SECTOR) as u32;
    let mut data = vec![0u8; SECTOR];
    for index in 0..SECTOR / 4 {
        put_u32(&mut data, index * 4, FREESECT);
    }
    for (index, value) in fat_sectors.iter().enumerate() {
        put_u32(&mut data, index * 4, *value);
    }
    put_u32(&mut data, (SECTOR / 4 - 1) * 4, next);
    file.extend_from_slice(&data);
    put_u32(file, 68, sector);
    put_u32(file, SECTOR + sector as usize * 4, DIFSECT);
    sector
}

/// A typed property value: the type, two padding bytes, the payload.
fn typed_value(value_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut value = value_type.to_le_bytes().to_vec();
    value.extend_from_slice(&[0, 0]);
    value.extend_from_slice(payload);
    value
}

/// The payload of a vector of strings.
fn string_vector(elements: &[&[u8]]) -> Vec<u8> {
    let mut payload = (elements.len() as u32).to_le_bytes().to_vec();
    for element in elements {
        payload.extend_from_slice(&0x001Eu32.to_le_bytes());
        payload.extend_from_slice(&(element.len() as u32).to_le_bytes());
        payload.extend_from_slice(element);
    }
    payload
}

#[test]
fn read_streams_refuses_a_bad_sector_shift() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u16(&mut file, 30, 3);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_bad_mini_sector_size() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u16(&mut file, 32, 7);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_falls_back_to_the_default_cutoff() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // A cutoff of zero is nonsense: the reader uses the default.
    put_u32(&mut file, 56, 0);
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    assert_eq!(streams.get(VISIO_DOCUMENT), Some(b"x".as_slice()));
}

#[test]
fn a_version_four_entry_keeps_its_size_bytes() {
    let mut entry = vec![0u8; DIRECTORY_ENTRY_SIZE];
    entry[66] = 2;
    put_u32(&mut entry, 116, 7);
    entry[120..128].copy_from_slice(&5_000_000_000u64.to_le_bytes());
    assert_eq!(DirEntry::parse(&entry, 4).unwrap().size, 5_000_000_000);
    assert_eq!(
        DirEntry::parse(&entry, 3).unwrap().size,
        5_000_000_000 & 0xFFFF_FFFF
    );
}

#[test]
fn a_truncated_directory_entry_is_refused() {
    let err = DirEntry::parse(&[0u8; 10], 3).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_skips_a_free_fat_slot() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The header names two FAT sectors but only one exists.
    put_u32(&mut file, 44, 2);
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    assert_eq!(streams.get(VISIO_DOCUMENT), Some(b"x".as_slice()));
}

#[test]
fn read_streams_reads_the_difat_chain() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    add_difat(&mut file, &[], ENDOFCHAIN);
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    assert_eq!(streams.get(VISIO_DOCUMENT), Some(b"x".as_slice()));
}

#[test]
fn read_streams_refuses_a_difat_cycle() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    let sector = add_difat(&mut file, &[], ENDOFCHAIN);
    // The DIFAT sector chains to itself.
    let last = (SECTOR / 4 - 1) * 4;
    put_u32(&mut file, sector_offset(sector) + last, sector);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_fat_sector_outside_the_file_in_the_difat() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    add_difat(&mut file, &[9999], ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_more_fat_sectors_than_the_file_has() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The same sector again and again: the file cannot hold that
    // many FAT sectors.
    add_difat(&mut file, &[1, 1, 1, 1, 1, 1], ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_file_without_a_root_entry() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The root entry is not a root any more.
    file[SECTOR + SECTOR + 66] = 0;
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_stream_over_the_cap() {
    let file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    let limits = Limits {
        max_total_uncompressed: 10,
        ..Limits::DEFAULT
    };
    let err = read_streams(&file, &limits).unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: "max_total_uncompressed",
            value: 64,
            max: 10
        }
    );
}

#[test]
fn read_streams_refuses_a_mini_stream_shorter_than_its_size() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The root entry claims a mini stream of ten kilobytes.
    let root = SECTOR + SECTOR;
    file[root + 120..root + 128].copy_from_slice(&10_000u64.to_le_bytes());
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_reads_an_empty_stream() {
    let file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, b"")
        .with_stream("Other", b"x")
        .build();
    let streams = read_streams(&file, &Limits::DEFAULT).unwrap();
    assert_eq!(streams.get(VISIO_DOCUMENT), Some([].as_slice()));
    assert_eq!(streams.get("Other"), Some(b"x".as_slice()));
}

#[test]
fn read_streams_refuses_a_big_stream_shorter_than_its_size() {
    let mut file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 5000])
        .build();
    // The stream claims more bytes than its sectors hold.
    let entry = second_entry_offset();
    file[entry + 120..entry + 128].copy_from_slice(&100_000u64.to_le_bytes());
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_chain_that_ends_in_a_reserved_sector() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u32(&mut file, SECTOR + 4, FATSECT);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_chain_longer_than_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // A chain of ten sectors in a file that has four.
    for index in 1..10u32 {
        put_u32(&mut file, SECTOR + index as usize * 4, index + 1);
    }
    put_u32(&mut file, SECTOR + 10 * 4, ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_sector_outside_the_allocation_table() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u32(&mut file, SECTOR + 4, 1000);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_property_values_are_read_by_their_type() {
    let cases: Vec<(u16, Vec<u8>, SummaryValue)> = vec![
        (
            0x0002,
            1252u16.to_le_bytes().to_vec(),
            SummaryValue::Integer(1252),
        ),
        (
            0x0003,
            (-5i32).to_le_bytes().to_vec(),
            SummaryValue::Integer(-5),
        ),
        (
            0x0004,
            1.5f32.to_le_bytes().to_vec(),
            SummaryValue::Real(1.5),
        ),
        (
            0x0005,
            2.5f64.to_le_bytes().to_vec(),
            SummaryValue::Real(2.5),
        ),
        (0x0006, vec![0u8; 8], SummaryValue::Binary(vec![0u8; 8])),
        (
            0x000B,
            1u16.to_le_bytes().to_vec(),
            SummaryValue::Boolean(true),
        ),
        (
            0x000B,
            0u16.to_le_bytes().to_vec(),
            SummaryValue::Boolean(false),
        ),
        (
            0x0014,
            (-7i64).to_le_bytes().to_vec(),
            SummaryValue::Integer(-7),
        ),
        (
            0x0015,
            7u64.to_le_bytes().to_vec(),
            SummaryValue::Integer(7),
        ),
        (
            0x0016,
            (-3i32).to_le_bytes().to_vec(),
            SummaryValue::Integer(-3),
        ),
        (
            0x0017,
            3u32.to_le_bytes().to_vec(),
            SummaryValue::Integer(3),
        ),
        (
            0x001E,
            {
                let mut payload = 3u32.to_le_bytes().to_vec();
                payload.extend_from_slice(b"hi\0");
                payload
            },
            SummaryValue::Text("hi".to_owned()),
        ),
        (
            0x001F,
            {
                let mut payload = 2u32.to_le_bytes().to_vec();
                payload.extend_from_slice(&[b'h', 0, b'i', 0]);
                payload
            },
            SummaryValue::Text("hi".to_owned()),
        ),
        (
            0x0040,
            130_000_000_000u64.to_le_bytes().to_vec(),
            SummaryValue::DateTime(130_000_000_000),
        ),
        (
            0x101E,
            string_vector(&[b"one\0", b"two\0"]),
            SummaryValue::Text("one\ntwo".to_owned()),
        ),
        (
            0x100C,
            string_vector(&[b"one\0"]),
            SummaryValue::Binary(Vec::new()),
        ),
        (0x0099, vec![1, 2, 3], SummaryValue::Binary(Vec::new())),
    ];
    for (value_type, payload, expected) in cases {
        let stream = build_summary(&FMTID_SUMMARY, &[(14, typed_value(value_type, &payload))]);
        let properties = read_summary_properties(&stream).unwrap();
        assert_eq!(properties.len(), 1, "type {value_type:#06x}");
        assert_eq!(properties[0].name, "Pages");
        assert_eq!(properties[0].value, expected, "type {value_type:#06x}");
    }
}

#[test]
fn the_code_page_reads_the_special_characters() {
    let bytes: Vec<u8> = (0x80u8..=0x9F).collect();
    let stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(&bytes))]);
    let properties = read_summary_properties(&stream).unwrap();
    let expected = concat!(
        "\u{20AC}\u{0081}\u{201A}\u{0192}\u{201E}\u{2026}\u{2020}\u{2021}",
        "\u{02C6}\u{2030}\u{0160}\u{2039}\u{0152}\u{008D}\u{017D}\u{008F}",
        "\u{0090}\u{2018}\u{2019}\u{201C}\u{201D}\u{2022}\u{2013}\u{2014}",
        "\u{02DC}\u{2122}\u{0161}\u{203A}\u{0153}\u{009D}\u{017E}\u{0178}",
    );
    assert_eq!(properties[0].value, SummaryValue::Text(expected.to_owned()));
}

#[test]
fn summary_properties_refuse_a_truncated_property_set_table() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    // The property set offset points past the end of the stream.
    put_u32(&mut stream, 0x2C, 0xFFFF);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_too_many_property_sets() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[]);
    put_u32(&mut stream, 0x18, 100);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_too_many_properties() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[]);
    put_u32(&mut stream, 0x34, 100_000);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_truncated_value() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    // The value of the property points past the end of the stream.
    put_u32(&mut stream, 0x3C, 0xFFFF);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_without_a_code_page_use_the_default_one() {
    let stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(properties[0].value, SummaryValue::Text("Title".to_owned()));
}

#[test]
fn summary_properties_with_a_code_page_that_is_not_a_number_use_the_default() {
    let stream = build_summary(
        &FMTID_SUMMARY,
        &[
            (1, lpstr_value(b"nonsense\0")),
            (2, lpstr_value(b"Title\0")),
        ],
    );
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].value, SummaryValue::Text("Title".to_owned()));
}

#[test]
fn decompress_reads_a_back_reference_with_a_far_pointer() {
    // A pointer above the window wraps around to the start of it.
    let data = [0x01u8, b'A', 0xFF, 0xF0];
    assert_eq!(
        decompress(&data, &Limits::DEFAULT).unwrap(),
        vec![b'A', 0, 0, 0]
    );
}

#[test]
fn read_streams_refuses_a_difat_sector_outside_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u32(&mut file, 68, 9999);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_fat_sector_outside_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The mini FAT sector is inside the allocation table but not in
    // the file; its chain ends there.
    put_u32(&mut file, SECTOR + 8, 100);
    put_u32(&mut file, SECTOR + 100 * 4, ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_directory_sector_outside_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u32(&mut file, SECTOR + 4, 100);
    put_u32(&mut file, SECTOR + 100 * 4, ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_stream_sector_outside_the_file() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    put_u32(&mut file, SECTOR + 12, 100);
    put_u32(&mut file, SECTOR + 100 * 4, ENDOFCHAIN);
    // The mini stream needs two sectors, so the second one is read.
    let root = SECTOR + SECTOR;
    file[root + 120..root + 128].copy_from_slice(&1024u64.to_le_bytes());
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_stream_sector_outside_the_file() {
    let mut file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 5000])
        .build();
    // The chain of the stream is long enough, but its second sector
    // lies outside the file.
    put_u32(&mut file, SECTOR + 16, 100);
    for sector in 5..12u32 {
        put_u32(&mut file, SECTOR + sector as usize * 4, sector + 1);
    }
    put_u32(&mut file, SECTOR + 100 * 4, 5);
    put_u32(&mut file, SECTOR + 12 * 4, ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_lying_mini_stream_size() {
    let mut file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 3000])
        .build();
    // The stream claims more than its mini chain holds, but stays
    // below the cutoff.
    let entry = second_entry_offset();
    file[entry + 120..entry + 128].copy_from_slice(&4095u64.to_le_bytes());
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_sector_outside_the_mini_stream() {
    let mut file = TestFile::new()
        .with_stream(VISIO_DOCUMENT, &[0u8; 128])
        .build();
    // The mini stream is two sectors long, but the chain names a
    // mini sector beyond it.
    let root = SECTOR + SECTOR;
    file[root + 120..root + 128].copy_from_slice(&128u64.to_le_bytes());
    let mini_fat = (2 + 1) * SECTOR;
    put_u32(&mut file, mini_fat, 5);
    put_u32(&mut file, mini_fat + 5 * 4, ENDOFCHAIN);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_an_empty_stream() {
    let err = read_summary_properties(&[]).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_truncated_format_id() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    stream.truncate(30);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_truncated_property_set_offset() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    stream.truncate(46);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_truncated_value_of_every_type() {
    for value_type in [
        0x0002u16, 0x0003, 0x0004, 0x0005, 0x000B, 0x0014, 0x0015, 0x0016, 0x0017, 0x001E, 0x001F,
        0x0040,
    ] {
        let mut stream =
            build_summary(&FMTID_SUMMARY, &[(14, typed_value(value_type, &[1, 2, 3]))]);
        // The value keeps its type but loses its payload.
        stream.truncate(0x44);
        let err = read_summary_properties(&stream).unwrap_err();
        assert!(
            matches!(err, Error::Invalid(_)),
            "type {value_type:#06x}: {err:?}"
        );
    }
}

#[test]
fn summary_properties_refuse_a_truncated_string_value() {
    for value_type in [0x001Eu16, 0x001F] {
        let mut stream = build_summary(
            &FMTID_SUMMARY,
            &[(14, typed_value(value_type, &[3, 0, 0, 0]))],
        );
        // The value keeps its length but loses its text.
        stream.truncate(0x48);
        let err = read_summary_properties(&stream).unwrap_err();
        assert!(
            matches!(err, Error::Invalid(_)),
            "type {value_type:#06x}: {err:?}"
        );
    }
}

#[test]
fn summary_properties_refuse_a_vector_with_too_many_elements() {
    let mut payload = (MAX_VECTOR_ELEMENTS as u32 + 1).to_le_bytes().to_vec();
    payload.extend_from_slice(&[0u8; 8]);
    let stream = build_summary(&FMTID_SUMMARY, &[(14, typed_value(0x101E, &payload))]);
    let err = read_summary_properties(&stream).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_read_a_vector_of_wide_strings() {
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&0x001Fu32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&[b'h', 0, b'i', 0]);
    let stream = build_summary(&FMTID_SUMMARY, &[(14, typed_value(0x101F, &payload))]);
    let properties = read_summary_properties(&stream).unwrap();
    assert_eq!(properties[0].value, SummaryValue::Text("hi".to_owned()));
}

#[test]
fn read_streams_refuses_every_truncated_header() {
    let file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    for length in 8..=80 {
        let mut short = file.clone();
        short.truncate(length);
        let err = read_streams(&short, &Limits::DEFAULT).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "length {length}: {err:?}");
    }
}

#[test]
fn a_directory_entry_without_a_start_sector_is_refused() {
    let entry = vec![0u8; 67];
    let err = DirEntry::parse(&entry, 3).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn a_directory_entry_without_a_size_is_refused() {
    let entry = vec![0u8; 119];
    let err = DirEntry::parse(&entry, 3).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn read_streams_refuses_a_mini_fat_sector_cycle() {
    let mut file = TestFile::new().with_stream(VISIO_DOCUMENT, b"x").build();
    // The FAT chains the mini FAT sector to itself.
    put_u32(&mut file, SECTOR + 8, 2);
    let err = read_streams(&file, &Limits::DEFAULT).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[test]
fn summary_properties_refuse_a_truncated_property_table() {
    let mut stream = build_summary(&FMTID_SUMMARY, &[(2, lpstr_value(b"Title\0"))]);
    let mut missing_id = stream.clone();
    missing_id.truncate(0x3A);
    assert!(matches!(
        read_summary_properties(&missing_id),
        Err(Error::Invalid(_))
    ));
    let mut missing_offset = stream.clone();
    missing_offset.truncate(0x3E);
    assert!(matches!(
        read_summary_properties(&missing_offset),
        Err(Error::Invalid(_))
    ));
    stream.truncate(0x34);
    assert!(matches!(
        read_summary_properties(&stream),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn summary_properties_refuse_a_truncated_vector() {
    let stream = build_summary(
        &FMTID_SUMMARY,
        &[(14, typed_value(0x101E, &1u32.to_le_bytes()))],
    );
    let mut missing_count = stream.clone();
    missing_count.truncate(0x44);
    assert!(matches!(
        read_summary_properties(&missing_count),
        Err(Error::Invalid(_))
    ));
    let mut missing_element = stream.clone();
    missing_element.truncate(0x48);
    assert!(matches!(
        read_summary_properties(&missing_element),
        Err(Error::Invalid(_))
    ));
    let mut missing_length = stream.clone();
    missing_length.truncate(0x4C);
    assert!(matches!(
        read_summary_properties(&missing_length),
        Err(Error::Invalid(_))
    ));
    let mut missing_text = stream.clone();
    missing_text.truncate(0x50);
    assert!(matches!(
        read_summary_properties(&missing_text),
        Err(Error::Invalid(_))
    ));
}
