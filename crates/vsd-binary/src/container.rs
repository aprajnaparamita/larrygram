// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The OLE compound file container and its streams (ADR 0068).
//!
//! A binary Visio file is an OLE compound file (MS-CFB) with a small
//! number of streams. [`read_streams`] reads the container into memory
//! with [`crate::Limits`] caps. [`decompress`] restores one compressed
//! block of the Visio stream format.
//!
//! The container reader is written from the Microsoft Open Specification
//! (MS-CFB): the header, the FAT and mini-FAT chains, the directory
//! entries and the stream bytes. Every chain walk is cycle-safe, every
//! input-derived size is checked against a cap before the allocation it
//! guards, and the arithmetic on file-derived offsets is checked
//! (ADR 0071, AGENTS.md section 6). Malformed input produces an
//! [`Error`], never a panic.
//!
//! The decompressor is derived from the libvisio reference (AGENTS.md
//! section 5); this file therefore carries the MPL-2.0 header.
//!
//! The summary streams (the OLE property set format) are read by
//! [`read_summary_properties`], which maps the well-known property ids
//! to the names the `.vsdx` reader uses, so a binary drawing and its
//! `.vsdx` twin carry the same document property names.

use std::collections::HashSet;

use crate::error::Error;
use crate::limits::Limits;

/// The stream that holds the drawing: its header (magic and version)
/// followed by the chunk records.
pub const VISIO_DOCUMENT: &str = "VisioDocument";

/// The stream that holds the document properties.
pub const SUMMARY_INFORMATION: &str = "\u{5}SummaryInformation";

/// The stream that holds the extended document properties.
pub const DOCUMENT_SUMMARY_INFORMATION: &str = "\u{5}DocumentSummaryInformation";

/// The Visio stream magic: the first bytes of a `VisioDocument` stream.
#[allow(dead_code)] // the tests build streams that open with the magic
const VISIO_MAGIC: &[u8] = b"Visio (TM) Drawing\r\n\0";

/// The compound file signature (MS-CFB 2.2).
const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The size of one directory entry (MS-CFB 2.6.1).
const DIRECTORY_ENTRY_SIZE: usize = 128;

/// The chain terminator (MS-CFB 2.3).
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
/// A free sector (MS-CFB 2.3).
const FREESECT: u32 = 0xFFFF_FFFF;
/// A sector that holds the FAT (MS-CFB 2.3).
const FATSECT: u32 = 0xFFFF_FFFD;
/// A sector that holds the DIFAT (MS-CFB 2.3).
const DIFSECT: u32 = 0xFFFF_FFFC;

/// The number of FAT sector numbers the header holds.
const HEADER_DIFAT_ENTRIES: usize = 109;

/// The number of bytes in the compound file header.
#[allow(dead_code)] // the tests size their fixtures with it
const HEADER_SIZE: usize = 512;

/// The size of the sliding window the decompressor uses.
const WINDOW_SIZE: usize = 4096;

/// One stream of the compound file, fully read and, when the format
/// compresses it, decompressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    /// The stream name, e.g. `"VisioDocument"`.
    pub name: String,
    /// The stream's bytes.
    pub data: Vec<u8>,
}

/// The streams of one compound file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Streams {
    /// The streams in directory order.
    pub streams: Vec<Stream>,
}

impl Streams {
    /// The bytes of the named stream, or `None` when the file has no
    /// such stream.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.streams
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.data.as_slice())
    }

    /// The bytes of the [`VISIO_DOCUMENT`] stream, or `None` when the
    /// file has none.
    pub fn visio_document(&self) -> Option<&[u8]> {
        self.get(VISIO_DOCUMENT)
    }

    /// The bytes of the `\u{5}SummaryInformation` stream, or `None`
    /// when the file has none.
    pub fn summary_information(&self) -> Option<&[u8]> {
        self.get(SUMMARY_INFORMATION)
    }

    /// The bytes of the `\u{5}DocumentSummaryInformation` stream, or
    /// `None` when the file has none.
    pub fn document_summary_information(&self) -> Option<&[u8]> {
        self.get(DOCUMENT_SUMMARY_INFORMATION)
    }
}

/// Reads every stream of the compound file `bytes`.
///
/// The file must be an OLE compound file (MS-CFB) that carries a
/// [`VISIO_DOCUMENT`] stream; anything else is
/// [`Error::NotAVisioFile`]. Caps in `limits` bound the stream count,
/// the size of one stream and the total size of all streams. A
/// structurally broken container is [`Error::Invalid`]; a container
/// that exceeds a cap is [`Error::LimitExceeded`]. Nothing panics.
pub fn read_streams(bytes: &[u8], limits: &Limits) -> Result<Streams, Error> {
    let header = Header::parse(bytes)?;
    let fat = read_fat(bytes, &header)?;
    let directory = read_directory(bytes, &header, &fat, limits)?;
    if !directory
        .streams
        .iter()
        .any(|entry| entry.name.eq_ignore_ascii_case(VISIO_DOCUMENT))
    {
        return Err(Error::NotAVisioFile(
            "the compound file has no VisioDocument stream".to_owned(),
        ));
    }
    let minifat = read_minifat(bytes, &header, &fat)?;
    let mini_stream = read_mini_stream(bytes, &header, &fat, &directory.root, limits)?;

    let mut total: u64 = 0;
    let mut streams = Vec::with_capacity(directory.streams.len());
    for entry in &directory.streams {
        // A size the platform cannot address is a cap failure too.
        if entry.size > limits.max_stream_bytes || usize::try_from(entry.size).is_err() {
            return Err(Error::LimitExceeded {
                limit: "max_stream_bytes",
                value: entry.size,
                max: limits.max_stream_bytes,
            });
        }
        // The cap below bounds the total, so the sum cannot overflow.
        total = total.saturating_add(entry.size);
        if total > limits.max_total_uncompressed {
            return Err(Error::LimitExceeded {
                limit: "max_total_uncompressed",
                value: total,
                max: limits.max_total_uncompressed,
            });
        }
        let data = read_stream(bytes, &header, &fat, &minifat, &mini_stream, entry)?;
        streams.push(Stream {
            name: entry.name.clone(),
            data,
        });
    }
    Ok(Streams { streams })
}

/// Decompresses one compressed block of the Visio stream format
/// (ADR 0068): an LZ-style scheme with a small sliding window.
///
/// `limits` caps the output size (`max_stream_bytes`) and the
/// expansion ratio (`max_compression_ratio`); the ratio is checked
/// against the size of the compressed block, so a decompression bomb is
/// refused before it can fill the memory. Uncompressed data is not
/// passed here; the caller checks the format's compression marker
/// first.
///
/// A block that ends in the middle of a token is not an error: the
/// bytes decoded so far are returned, as the format's own reader does.
pub fn decompress(data: &[u8], limits: &Limits) -> Result<Vec<u8>, Error> {
    let max_output = limits.max_stream_bytes;
    let max_ratio_bytes = (data.len() as u64).saturating_mul(limits.max_compression_ratio);
    let mut out: Vec<u8> = Vec::new();
    let mut window = [0u8; WINDOW_SIZE];
    let mut position: u64 = 0;
    let mut index = 0usize;

    while index < data.len() {
        // The loop condition keeps the index inside the block.
        let flags = data.get(index).copied().unwrap_or(0);
        index += 1;
        if index >= data.len() {
            // A flag byte without a token behind it ends the block.
            break;
        }
        let mut mask = 1u8;
        for _ in 0..8 {
            if index >= data.len() {
                break;
            }
            if flags & mask != 0 {
                let byte = data.get(index).copied().unwrap_or(0);
                index += 1;
                push_byte(
                    &mut out,
                    &mut window,
                    &mut position,
                    byte,
                    max_output,
                    max_ratio_bytes,
                )?;
            } else {
                if index + 1 >= data.len() {
                    // A back reference without its two bytes ends the
                    // block.
                    break;
                }
                let low = data.get(index).copied().unwrap_or(0);
                index += 1;
                let high = data.get(index).copied().unwrap_or(0);
                index += 1;
                let length = (high & 15) + 3;
                let mut pointer = ((u32::from(high) & 0xF0) << 4) | u32::from(low);
                if pointer > 4078 {
                    pointer -= 4078;
                } else {
                    pointer += 18;
                }
                for step in 0..length {
                    let source = ((pointer + u32::from(step)) as usize) & (WINDOW_SIZE - 1);
                    let byte = window_byte(&window, source);
                    push_byte(
                        &mut out,
                        &mut window,
                        &mut position,
                        byte,
                        max_output,
                        max_ratio_bytes,
                    )?;
                }
            }
            mask <<= 1;
        }
    }
    Ok(out)
}

/// The byte at `index` of the sliding window.
///
/// The index is always inside the window: the decompressor masks it
/// with the window size.
fn window_byte(window: &[u8; WINDOW_SIZE], index: usize) -> u8 {
    window.get(index).copied().unwrap_or(0)
}

/// Appends one byte to the output and to the sliding window, checking
/// the caps before the output grows past them.
fn push_byte(
    out: &mut Vec<u8>,
    window: &mut [u8; WINDOW_SIZE],
    position: &mut u64,
    byte: u8,
    max_output: u64,
    max_ratio_bytes: u64,
) -> Result<(), Error> {
    let produced = out.len() as u64 + 1;
    if produced > max_output {
        return Err(Error::LimitExceeded {
            limit: "max_stream_bytes",
            value: produced,
            max: max_output,
        });
    }
    if produced > max_ratio_bytes {
        return Err(Error::LimitExceeded {
            limit: "max_compression_ratio",
            value: produced,
            max: max_ratio_bytes,
        });
    }
    let slot = usize::try_from(*position).unwrap_or(0) & (WINDOW_SIZE - 1);
    // The slot is masked to the window size; the fallback keeps the
    // write infallible without an uncovered branch.
    let mut scratch = 0u8;
    *window.get_mut(slot).unwrap_or(&mut scratch) = byte;
    out.push(byte);
    *position += 1;
    Ok(())
}

/// The compound file header (MS-CFB 2.2).
struct Header {
    /// The size of one sector, in bytes.
    sector_size: usize,
    /// The size of one mini sector, in bytes.
    mini_sector_size: usize,
    /// The size below which a stream lives in the mini stream.
    mini_cutoff: u64,
    /// The first sector of the directory chain.
    first_dir_sector: u32,
    /// The first sector of the mini FAT chain.
    first_minifat_sector: u32,
    /// The number of FAT sectors the header declares.
    num_fat_sectors: u32,
    /// The first sector of the DIFAT chain.
    first_difat_sector: u32,
    /// The FAT sector numbers the header holds.
    difat: [u32; HEADER_DIFAT_ENTRIES],
    /// The major version of the file format (3 or 4).
    major_version: u16,
}

impl Header {
    /// Reads the header of the compound file `bytes`.
    fn parse(bytes: &[u8]) -> Result<Header, Error> {
        let signature = bytes.get(..SIGNATURE.len()).ok_or_else(|| {
            Error::NotAVisioFile("the file is too short to be a compound file".to_owned())
        })?;
        if signature != SIGNATURE {
            return Err(Error::NotAVisioFile(
                "the file does not start with the compound file signature".to_owned(),
            ));
        }
        let major_version = read_u16(bytes, 26)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        if !matches!(major_version, 3 | 4) {
            return Err(Error::Invalid(format!(
                "unsupported compound file version {major_version}"
            )));
        }
        let byte_order = read_u16(bytes, 28)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        if byte_order != 0xFFFE {
            return Err(Error::Invalid(
                "the compound file is not little endian".to_owned(),
            ));
        }
        let sector_shift = read_u16(bytes, 30)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        // The specification names 512 byte sectors for version 3 and
        // 4096 byte sectors for version 4; the reader takes the size
        // from the header and refuses anything that is not a sane
        // sector size.
        if !(9..=12).contains(&sector_shift) {
            return Err(Error::Invalid(format!(
                "unsupported compound file sector shift {sector_shift}"
            )));
        }
        let sector_size = 1usize << sector_shift;
        let mini_shift = read_u16(bytes, 32)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        if mini_shift != 6 {
            return Err(Error::Invalid(
                "unsupported mini sector size in the compound file".to_owned(),
            ));
        }
        let mini_cutoff = read_u32(bytes, 56).unwrap_or(4096) as u64;
        let mini_cutoff = if (1..=65536).contains(&mini_cutoff) {
            mini_cutoff
        } else {
            4096
        };
        let num_fat_sectors = read_u32(bytes, 44)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        let first_dir_sector = read_u32(bytes, 48)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        let first_minifat_sector = read_u32(bytes, 60)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        let first_difat_sector = read_u32(bytes, 68)
            .ok_or_else(|| Error::Invalid("the compound file header is truncated".to_owned()))?;
        let mut difat = [FREESECT; HEADER_DIFAT_ENTRIES];
        for (index, slot) in difat.iter_mut().enumerate() {
            let offset = 76 + index * 4;
            if let Some(value) = read_u32(bytes, offset) {
                *slot = value;
            }
        }
        Ok(Header {
            sector_size,
            mini_sector_size: 64,
            mini_cutoff,
            first_dir_sector,
            first_minifat_sector,
            num_fat_sectors,
            first_difat_sector,
            difat,
            major_version,
        })
    }
}

/// One directory entry of the compound file (MS-CFB 2.6.1).
#[derive(Debug)]
struct DirEntry {
    /// The object's name.
    name: String,
    /// The object type: 1 storage, 2 stream, 5 root.
    object_type: u8,
    /// The first sector of the object's chain.
    start_sector: u32,
    /// The object's size, in bytes.
    size: u64,
}

impl DirEntry {
    /// Reads one directory entry.
    fn parse(bytes: &[u8], major_version: u16) -> Result<DirEntry, Error> {
        let object_type = bytes
            .get(66)
            .copied()
            .ok_or_else(|| Error::Invalid("a directory entry is truncated".to_owned()))?;
        let name_length = read_u16(bytes, 64).unwrap_or(0) as usize;
        let name_length = name_length.min(64) & !1;
        let mut units: Vec<u16> = Vec::with_capacity(name_length / 2);
        for index in 0..name_length / 2 {
            let unit = read_u16(bytes, index * 2).unwrap_or(0);
            units.push(unit);
        }
        // The name length counts the terminating null.
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        let name = String::from_utf16_lossy(units.get(..end).unwrap_or_default());
        let start_sector = read_u32(bytes, 116)
            .ok_or_else(|| Error::Invalid("a directory entry is truncated".to_owned()))?;
        let size = read_u64(bytes, 120)
            .ok_or_else(|| Error::Invalid("a directory entry is truncated".to_owned()))?;
        // A version 3 file only uses the low four bytes of the size;
        // older writers left the high bytes uninitialised.
        let size = if major_version == 3 {
            size & 0xFFFF_FFFF
        } else {
            size
        };
        Ok(DirEntry {
            name,
            object_type,
            start_sector,
            size,
        })
    }
}

/// The directory of a compound file: the root entry and the streams.
struct Directory {
    /// The root entry, which carries the mini stream.
    root: Option<DirEntry>,
    /// The stream entries, in directory order.
    streams: Vec<DirEntry>,
}

/// Reads the FAT: the array that chains the sectors of the file.
fn read_fat(bytes: &[u8], header: &Header) -> Result<Vec<u32>, Error> {
    let max_sectors = file_sector_count(bytes, header.sector_size);
    // The FAT sectors: the ones the header names, then the ones the
    // DIFAT chain names.
    let mut fat_sectors: Vec<&[u8]> = Vec::new();
    for slot in header.difat.iter() {
        if fat_sectors.len() >= header.num_fat_sectors as usize {
            break;
        }
        if *slot == FREESECT {
            continue;
        }
        let data = sector_bytes(bytes, *slot, header.sector_size).ok_or_else(|| {
            Error::Invalid("a FAT sector of the compound file lies outside the file".to_owned())
        })?;
        fat_sectors.push(data);
    }
    // The rest of the FAT sectors live in the DIFAT chain.
    let mut visited: HashSet<u32> = HashSet::new();
    let mut difat_sector = header.first_difat_sector;
    while difat_sector != ENDOFCHAIN && difat_sector != FREESECT {
        if !visited.insert(difat_sector) {
            return Err(Error::Invalid(
                "the DIFAT chain of the compound file cycles".to_owned(),
            ));
        }
        let data = sector_bytes(bytes, difat_sector, header.sector_size).ok_or_else(|| {
            Error::Invalid("a DIFAT sector of the compound file lies outside the file".to_owned())
        })?;
        let entries = header.sector_size / 4;
        for index in 0..entries.saturating_sub(1) {
            let sector = read_u32(data, index * 4).unwrap_or(FREESECT);
            if sector == FREESECT {
                continue;
            }
            let sector_data = sector_bytes(bytes, sector, header.sector_size).ok_or_else(|| {
                Error::Invalid("a FAT sector of the compound file lies outside the file".to_owned())
            })?;
            if fat_sectors.len() >= max_sectors {
                return Err(Error::Invalid(
                    "the compound file names more FAT sectors than it has".to_owned(),
                ));
            }
            fat_sectors.push(sector_data);
        }
        difat_sector = read_u32(data, (entries - 1) * 4).unwrap_or(ENDOFCHAIN);
    }

    let entries_per_sector = header.sector_size / 4;
    let mut fat = Vec::with_capacity(fat_sectors.len() * entries_per_sector);
    for data in &fat_sectors {
        for index in 0..entries_per_sector {
            fat.push(read_u32(data, index * 4).unwrap_or(FREESECT));
        }
    }
    Ok(fat)
}

/// Reads the mini FAT: the array that chains the mini sectors.
fn read_minifat(bytes: &[u8], header: &Header, fat: &[u32]) -> Result<Vec<u32>, Error> {
    if header.first_minifat_sector == ENDOFCHAIN || header.first_minifat_sector == FREESECT {
        return Ok(Vec::new());
    }
    let max_sectors = file_sector_count(bytes, header.sector_size);
    let chain = sector_chain(fat, header.first_minifat_sector, max_sectors)?;
    let entries_per_sector = header.sector_size / 4;
    let mut minifat = Vec::with_capacity(chain.len() * entries_per_sector);
    for sector in &chain {
        let data = sector_bytes(bytes, *sector, header.sector_size).ok_or_else(|| {
            Error::Invalid(
                "a mini FAT sector of the compound file lies outside the file".to_owned(),
            )
        })?;
        for index in 0..entries_per_sector {
            minifat.push(read_u32(data, index * 4).unwrap_or(FREESECT));
        }
    }
    Ok(minifat)
}

/// Reads the directory: the root entry and the stream entries.
fn read_directory(
    bytes: &[u8],
    header: &Header,
    fat: &[u32],
    limits: &Limits,
) -> Result<Directory, Error> {
    let max_sectors = file_sector_count(bytes, header.sector_size);
    let chain = sector_chain(fat, header.first_dir_sector, max_sectors)?;
    let mut root = None;
    let mut streams: Vec<DirEntry> = Vec::new();
    for sector in &chain {
        let data = sector_bytes(bytes, *sector, header.sector_size).ok_or_else(|| {
            Error::Invalid(
                "a directory sector of the compound file lies outside the file".to_owned(),
            )
        })?;
        for index in 0..header.sector_size / DIRECTORY_ENTRY_SIZE {
            let start = index * DIRECTORY_ENTRY_SIZE;
            let entry = data
                .get(start..start + DIRECTORY_ENTRY_SIZE)
                .unwrap_or_default();
            let entry = DirEntry::parse(entry, header.major_version)?;
            match entry.object_type {
                5 => root = Some(entry),
                2 => {
                    if streams.len() >= limits.max_streams as usize {
                        return Err(Error::LimitExceeded {
                            limit: "max_streams",
                            value: streams.len() as u64 + 1,
                            max: limits.max_streams as u64,
                        });
                    }
                    streams.push(entry);
                }
                // Storages and unallocated entries hold no stream bytes.
                _ => {}
            }
        }
    }
    Ok(Directory { root, streams })
}

/// Reads the mini stream: the bytes every small stream lives in.
fn read_mini_stream(
    bytes: &[u8],
    header: &Header,
    fat: &[u32],
    root: &Option<DirEntry>,
    limits: &Limits,
) -> Result<Vec<u8>, Error> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    if root.size == 0 {
        return Ok(Vec::new());
    }
    // A size the platform cannot address is a cap failure too.
    if root.size > limits.max_total_uncompressed || usize::try_from(root.size).is_err() {
        return Err(Error::LimitExceeded {
            limit: "max_total_uncompressed",
            value: root.size,
            max: limits.max_total_uncompressed,
        });
    }
    // The cap above makes the size fit a `usize`.
    let size = root.size as usize;
    let max_sectors = file_sector_count(bytes, header.sector_size);
    let chain = sector_chain(fat, root.start_sector, max_sectors)?;
    let needed = size.div_ceil(header.sector_size);
    if chain.len() < needed {
        return Err(Error::Invalid(
            "the mini stream is shorter than its size claims".to_owned(),
        ));
    }
    let mut out = Vec::with_capacity(size);
    for sector in chain.iter().take(needed) {
        let data = sector_bytes(bytes, *sector, header.sector_size).ok_or_else(|| {
            Error::Invalid("a sector of the compound file lies outside the file".to_owned())
        })?;
        out.extend_from_slice(data);
    }
    out.truncate(size);
    Ok(out)
}

/// Reads the bytes of one stream.
fn read_stream(
    bytes: &[u8],
    header: &Header,
    fat: &[u32],
    minifat: &[u32],
    mini_stream: &[u8],
    entry: &DirEntry,
) -> Result<Vec<u8>, Error> {
    if entry.size == 0 {
        return Ok(Vec::new());
    }
    // The caller caps the size, so it fits a `usize`.
    let size = entry.size as usize;
    let mut out = Vec::with_capacity(size);
    if entry.size < header.mini_cutoff {
        let max_mini_sectors = mini_stream.len() / header.mini_sector_size;
        let chain = sector_chain(minifat, entry.start_sector, max_mini_sectors)?;
        let needed = size.div_ceil(header.mini_sector_size);
        if chain.len() < needed {
            return Err(Error::Invalid(
                "a stream is shorter than its size claims".to_owned(),
            ));
        }
        for sector in chain.iter().take(needed) {
            let offset = (*sector as usize).saturating_mul(header.mini_sector_size);
            let data = mini_stream
                .get(offset..offset + header.mini_sector_size)
                .ok_or_else(|| {
                    Error::Invalid("a mini sector lies outside the mini stream".to_owned())
                })?;
            out.extend_from_slice(data);
        }
    } else {
        let max_sectors = file_sector_count(bytes, header.sector_size);
        let chain = sector_chain(fat, entry.start_sector, max_sectors)?;
        let needed = size.div_ceil(header.sector_size);
        if chain.len() < needed {
            return Err(Error::Invalid(
                "a stream is shorter than its size claims".to_owned(),
            ));
        }
        for sector in chain.iter().take(needed) {
            let data = sector_bytes(bytes, *sector, header.sector_size).ok_or_else(|| {
                Error::Invalid("a sector of the compound file lies outside the file".to_owned())
            })?;
            out.extend_from_slice(data);
        }
    }
    out.truncate(size);
    Ok(out)
}

/// Walks a sector chain, refusing a cycle and a chain longer than the
/// file can hold.
fn sector_chain(fat: &[u32], start: u32, max_len: usize) -> Result<Vec<u32>, Error> {
    let mut chain: Vec<u32> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut current = start;
    while current != ENDOFCHAIN {
        if current == FREESECT || current == FATSECT || current == DIFSECT {
            return Err(Error::Invalid(
                "a sector chain ends in a reserved sector".to_owned(),
            ));
        }
        if !visited.insert(current) {
            return Err(Error::Invalid("a sector chain cycles".to_owned()));
        }
        chain.push(current);
        if chain.len() > max_len {
            return Err(Error::Invalid(
                "a sector chain is longer than the file".to_owned(),
            ));
        }
        let next = fat.get(current as usize).copied().ok_or_else(|| {
            Error::Invalid("a sector lies outside the allocation table".to_owned())
        })?;
        current = next;
    }
    Ok(chain)
}

/// The number of sectors the file holds after its header.
fn file_sector_count(bytes: &[u8], sector_size: usize) -> usize {
    bytes.len().saturating_sub(sector_size) / sector_size
}

/// The bytes of one sector, or `None` when the sector lies outside the
/// file.
fn sector_bytes(bytes: &[u8], sector: u32, sector_size: usize) -> Option<&[u8]> {
    let offset = (sector as usize).checked_add(1)?.checked_mul(sector_size)?;
    bytes.get(offset..offset.checked_add(sector_size)?)
}

/// Reads a little-endian `u16` at `offset`.
///
/// The `get` above fixes the length of the slice, so the chunk always
/// fits; the default only keeps the read free of a branch that cannot
/// be taken.
fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(
        slice.first_chunk::<2>().copied().unwrap_or([0; 2]),
    ))
}

/// Reads a little-endian `u32` at `offset`.
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(
        slice.first_chunk::<4>().copied().unwrap_or([0; 4]),
    ))
}

/// Reads a little-endian `u64` at `offset`.
fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes(
        slice.first_chunk::<8>().copied().unwrap_or([0; 8]),
    ))
}

/// The property id of the code page in a summary stream.
const CODE_PAGE_ID: u32 = 1;

/// The code page a summary stream uses when it does not name one.
const DEFAULT_CODE_PAGE: u16 = 1252;

/// The code page of UTF-8.
const UTF8_CODE_PAGE: u16 = 65001;

/// The maximum number of property sets one summary stream may declare.
const MAX_PROPERTY_SETS: usize = 16;

/// The maximum number of properties one property set may declare.
const MAX_PROPERTIES: usize = 4096;

/// The maximum number of elements one property vector may hold.
const MAX_VECTOR_ELEMENTS: usize = 4096;

/// The format id of the `SummaryInformation` property set.
const FMTID_SUMMARY: [u8; 16] = [
    0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9,
];

/// The format id of the `DocumentSummaryInformation` property set.
const FMTID_DOCUMENT_SUMMARY: [u8; 16] = [
    0x02, 0xD5, 0xCD, 0xD5, 0x9C, 0x2E, 0x1B, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2B, 0x2C, 0xF9, 0xAE,
];

/// The well-known properties of the `SummaryInformation` set, with the
/// names the `.vsdx` reader gives the same properties.
const SUMMARY_PROPERTIES: &[(u32, &str)] = &[
    (2, "title"),
    (3, "subject"),
    (4, "creator"),
    (5, "keywords"),
    (6, "description"),
    (7, "Template"),
    (8, "lastModifiedBy"),
    (9, "revision"),
    (10, "TotalTime"),
    (11, "lastPrinted"),
    (12, "created"),
    (13, "modified"),
    (14, "Pages"),
    (15, "Words"),
    (16, "Characters"),
    (18, "Application"),
    (19, "DocSecurity"),
];

/// The well-known properties of the `DocumentSummaryInformation` set.
const DOCUMENT_SUMMARY_PROPERTIES: &[(u32, &str)] = &[
    (2, "category"),
    (5, "Lines"),
    (6, "Paragraphs"),
    (7, "Slides"),
    (8, "Notes"),
    (9, "HiddenSlides"),
    (10, "MMClips"),
    (11, "ScaleCrop"),
    (12, "HeadingPairs"),
    (13, "TitlesOfParts"),
    (14, "Manager"),
    (15, "Company"),
    (16, "LinksUpToDate"),
    (17, "CharactersWithSpaces"),
    (19, "SharedDoc"),
    (21, "HyperlinksChanged"),
    (22, "AppVersion"),
    (25, "contentType"),
    (26, "contentStatus"),
    (27, "language"),
    (28, "version"),
];

/// One property of a summary stream.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryProperty {
    /// The property's name, e.g. `"title"`.
    pub name: String,
    /// The property's value.
    pub value: SummaryValue,
}

/// The value of a summary-stream property.
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryValue {
    /// Text, decoded with the code page of the stream.
    Text(String),
    /// A whole number.
    Integer(i64),
    /// A floating-point number.
    Real(f64),
    /// A boolean.
    Boolean(bool),
    /// A Windows timestamp, as the file stores it: 100 nanoseconds
    /// since 1601.
    DateTime(u64),
    /// Bytes of a type the importer does not interpret. The value is
    /// empty for a type it cannot even measure.
    Binary(Vec<u8>),
}

/// Reads the properties of a summary stream.
///
/// `stream` is the bytes of a [`SUMMARY_INFORMATION`] or a
/// [`DOCUMENT_SUMMARY_INFORMATION`] stream, in the OLE property set
/// format. The well-known property ids are mapped to the names the
/// `.vsdx` reader uses, so a binary drawing and its `.vsdx` twin carry
/// the same document property names. A property the importer does not
/// know, or whose value it cannot measure, is left out.
///
/// The strings of a summary stream are in the code page its code page
/// property names; UTF-8 and Windows-1252 are read exactly, every other
/// code page is read as Latin-1.
pub fn read_summary_properties(stream: &[u8]) -> Result<Vec<SummaryProperty>, Error> {
    let byte_order = read_u16(stream, 0).ok_or_else(|| {
        Error::Invalid("the summary stream is shorter than its header".to_owned())
    })?;
    if byte_order != 0xFFFE {
        return Err(Error::Invalid(
            "the summary stream is not little endian".to_owned(),
        ));
    }
    let count = read_u32(stream, 24)
        .ok_or_else(|| Error::Invalid("the summary stream is shorter than its header".to_owned()))?
        as usize;
    if count > MAX_PROPERTY_SETS {
        return Err(Error::Invalid(
            "the summary stream declares too many property sets".to_owned(),
        ));
    }
    let mut properties: Vec<SummaryProperty> = Vec::new();
    for index in 0..count {
        let base = 28 + index * 20;
        let fmtid = stream.get(base..base + 16).ok_or_else(|| {
            Error::Invalid("the summary stream's property set table is truncated".to_owned())
        })?;
        let offset = read_u32(stream, base + 16).ok_or_else(|| {
            Error::Invalid("the summary stream's property set table is truncated".to_owned())
        })? as usize;
        properties.extend(read_property_set(stream, offset, fmtid)?);
    }
    Ok(properties)
}

/// Reads one property set of a summary stream.
fn read_property_set(
    stream: &[u8],
    offset: usize,
    fmtid: &[u8],
) -> Result<Vec<SummaryProperty>, Error> {
    let count = read_u32(stream, offset + 4)
        .ok_or_else(|| Error::Invalid("a summary property set is truncated".to_owned()))?
        as usize;
    if count > MAX_PROPERTIES {
        return Err(Error::Invalid(
            "a summary property set declares too many properties".to_owned(),
        ));
    }
    let mut table: Vec<(u32, usize)> = Vec::with_capacity(count);
    for index in 0..count {
        let id = read_u32(stream, offset + 8 + index * 8)
            .ok_or_else(|| Error::Invalid("a summary property set is truncated".to_owned()))?;
        let value_offset = read_u32(stream, offset + 12 + index * 8)
            .ok_or_else(|| Error::Invalid("a summary property set is truncated".to_owned()))?
            as usize;
        table.push((id, value_offset));
    }
    // The code page property is read first: it says how the strings of
    // the other properties are encoded.
    let code_page = table
        .iter()
        .find(|(id, _)| *id == CODE_PAGE_ID)
        .and_then(|(_, value_offset)| {
            read_value(stream, offset + value_offset, DEFAULT_CODE_PAGE).ok()
        })
        .and_then(|value| match value {
            SummaryValue::Integer(code_page) => u16::try_from(code_page).ok(),
            _ => None,
        })
        .unwrap_or(DEFAULT_CODE_PAGE);
    let mut properties: Vec<SummaryProperty> = Vec::new();
    for (id, value_offset) in table {
        let Some(name) = property_name(fmtid, id) else {
            continue;
        };
        let value = read_value(stream, offset + value_offset, code_page)?;
        properties.push(SummaryProperty {
            name: name.to_owned(),
            value,
        });
    }
    Ok(properties)
}

/// The name of a well-known property, or `None` when the property set
/// or the id is unknown.
fn property_name(fmtid: &[u8], id: u32) -> Option<&'static str> {
    let table: &[(u32, &str)] = if fmtid == FMTID_SUMMARY {
        SUMMARY_PROPERTIES
    } else if fmtid == FMTID_DOCUMENT_SUMMARY {
        DOCUMENT_SUMMARY_PROPERTIES
    } else {
        return None;
    };
    table
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, name)| *name)
}

/// Reads one typed property value.
fn read_value(stream: &[u8], at: usize, code_page: u16) -> Result<SummaryValue, Error> {
    let value_type = read_u16(stream, at)
        .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?;
    let body = at + 4;
    match value_type {
        0x0002 => Ok(SummaryValue::Integer(i64::from(
            read_u16(stream, body).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?,
        ))),
        0x0003 => Ok(SummaryValue::Integer(i64::from(
            read_u32(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as i32,
        ))),
        0x0004 => Ok(SummaryValue::Real(f64::from(f32::from_bits(
            read_u32(stream, body).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?,
        )))),
        0x0005 => Ok(SummaryValue::Real(f64::from_bits(
            read_u64(stream, body).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?,
        ))),
        0x0006 => Ok(SummaryValue::Binary(
            stream
                .get(body..body + 8)
                .map(<[u8]>::to_vec)
                .unwrap_or_default(),
        )),
        0x000B => {
            let value = read_u16(stream, body).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?;
            Ok(SummaryValue::Boolean(value != 0))
        }
        0x0014 => Ok(SummaryValue::Integer(
            read_u64(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as i64,
        )),
        0x0015 => Ok(SummaryValue::Integer(
            read_u64(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as i64,
        )),
        0x0016 => Ok(SummaryValue::Integer(i64::from(
            read_u32(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as i32,
        ))),
        0x0017 => Ok(SummaryValue::Integer(i64::from(
            read_u32(stream, body).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?,
        ))),
        0x001E => {
            let length = read_u32(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as usize;
            let bytes = stream.get(body + 4..body + 4 + length).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?;
            Ok(SummaryValue::Text(decode_text(bytes, code_page)))
        }
        0x001F => {
            let length = read_u32(stream, body)
                .ok_or_else(|| Error::Invalid("a summary property value is truncated".to_owned()))?
                as usize;
            let bytes = stream.get(body + 4..body + 4 + length * 2).ok_or_else(|| {
                Error::Invalid("a summary property value is truncated".to_owned())
            })?;
            Ok(SummaryValue::Text(decode_wide_text(bytes)))
        }
        0x0040 => Ok(SummaryValue::DateTime(read_u64(stream, body).ok_or_else(
            || Error::Invalid("a summary property value is truncated".to_owned()),
        )?)),
        0x1000..=0x101F => read_vector(stream, body, value_type & 0x1F, code_page),
        _ => Ok(SummaryValue::Binary(Vec::new())),
    }
}

/// Reads a vector of property values.
fn read_vector(
    stream: &[u8],
    body: usize,
    element_type: u16,
    code_page: u16,
) -> Result<SummaryValue, Error> {
    let count = read_u32(stream, body)
        .ok_or_else(|| Error::Invalid("a summary property vector is truncated".to_owned()))?
        as usize;
    if count > MAX_VECTOR_ELEMENTS {
        return Err(Error::Invalid(
            "a summary property vector holds too many elements".to_owned(),
        ));
    }
    if element_type != 0x001E && element_type != 0x001F {
        // A vector of variants or of numbers has no place in the
        // document properties; the importer leaves it out.
        return Ok(SummaryValue::Binary(Vec::new()));
    }
    let mut texts: Vec<String> = Vec::with_capacity(count);
    let mut at = body + 4;
    for _ in 0..count {
        let _element = read_u32(stream, at)
            .ok_or_else(|| Error::Invalid("a summary property vector is truncated".to_owned()))?;
        at += 4;
        let length = read_u32(stream, at)
            .ok_or_else(|| Error::Invalid("a summary property vector is truncated".to_owned()))?
            as usize;
        at += 4;
        // A wide string counts its characters, a code page string its
        // bytes.
        let bytes = if element_type == 0x001F {
            length * 2
        } else {
            length
        };
        let data = stream
            .get(at..at + bytes)
            .ok_or_else(|| Error::Invalid("a summary property vector is truncated".to_owned()))?;
        texts.push(if element_type == 0x001E {
            decode_text(data, code_page)
        } else {
            decode_wide_text(data)
        });
        at += bytes;
    }
    Ok(SummaryValue::Text(texts.join("\n")))
}

/// Decodes a code page string: the bytes end at the first null.
fn decode_text(bytes: &[u8], code_page: u16) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let bytes = bytes.get(..end).unwrap_or_default();
    if code_page == UTF8_CODE_PAGE {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes.iter().map(|byte| code_page_char(*byte)).collect()
}

/// Decodes a wide (UTF-16) string: the bytes end at the first null.
fn decode_wide_text(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for index in 0..bytes.len() / 2 {
        units.push(read_u16(bytes, index * 2).unwrap_or(0));
    }
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(units.get(..end).unwrap_or_default())
}

/// The character a byte stands for in the Windows-1252 code page.
///
/// The code page is Latin-1 with 32 characters in the `0x80..=0x9F`
/// range; every other code page the importer meets is read as
/// Latin-1.
fn code_page_char(byte: u8) -> char {
    match byte {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        other => char::from(other),
    }
}

#[cfg(test)]
mod tests;
