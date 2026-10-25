// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A tiny compound-file writer for the hand-authored `.vsd` fixtures
//! (task P8-10, ADR 0005).
//!
//! The fixtures under `corpus/fixtures/` are our own work, committed
//! and MIT (see `corpus/PROVENANCE.md`); this writer builds the OLE
//! compound container (MS-CFB) around one or more streams, and the
//! `malformed` test file builds the `VisioDocument` stream payloads.
//! The layout follows MS-CFB: a 512-byte header, a FAT sector, the
//! directory, the mini FAT and the stream sectors. Small streams (below
//! the mini-stream cutoff) go into the mini stream, so both allocation
//! paths are covered.
//!
//! [`Defect`] lets a fixture break one part of the container on
//! purpose; the byte layout stays otherwise identical, so a malformed
//! fixture differs from the valid one in exactly the field it tests.

/// The size of one sector.
pub const SECTOR: usize = 512;

/// The size of one mini sector.
const MINI: usize = 64;

/// The mini stream cutoff.
const CUTOFF: u64 = 4096;

/// The number of FAT entries one sector holds.
const FAT_ENTRIES: usize = SECTOR / 4;

/// The chain terminator (MS-CFB 2.3).
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
/// A free sector (MS-CFB 2.3).
const FREESECT: u32 = 0xFFFF_FFFF;
/// A sector that holds the FAT (MS-CFB 2.3).
const FATSECT: u32 = 0xFFFF_FFFD;

/// The size of one directory entry (MS-CFB 2.6.1).
const DIRECTORY_ENTRY_SIZE: usize = 128;

/// The compound file signature (MS-CFB 2.2).
const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// One deliberate container defect a fixture carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    /// A sound container.
    None,
    /// The FAT chain of the first stream loops back to its own start.
    FatCycle,
}

/// Where a stream of a fixture lives.
#[derive(Clone, Copy)]
enum Placement {
    /// The stream lives in the mini stream.
    Mini { start: u32, size: usize },
    /// The stream lives in sectors of its own.
    Normal { start: u32, size: usize },
}

/// Builds one compound file around the given streams, in directory
/// order. The first stream is the `VisioDocument` stream of a fixture.
pub fn compound(streams: &[(&str, &[u8])], defect: Defect) -> Vec<u8> {
    let mut sectors: Vec<Vec<u8>> = Vec::new();
    let mut fat: Vec<u32> = Vec::new();
    let mut mini_stream: Vec<u8> = Vec::new();
    let mut mini_fat: Vec<u32> = Vec::new();
    let mut placement: Vec<Placement> = Vec::with_capacity(streams.len());

    // Sector 0 holds the FAT itself.
    sectors.push(vec![0u8; SECTOR]);
    fat.push(FATSECT);

    // The directory: the root entry and one entry per stream.
    let dir_count = (1 + streams.len()).div_ceil(4);
    let dir_start = sectors.len() as u32;
    for _ in 0..dir_count {
        sectors.push(vec![0u8; SECTOR]);
        fat.push(ENDOFCHAIN);
    }
    chain(&mut fat, dir_start, dir_count);

    // The small streams go into the mini stream.
    for (stream_index, (_, data)) in streams.iter().enumerate() {
        if (data.len() as u64) < CUTOFF {
            let start = (mini_stream.len() / MINI) as u32;
            let count = data.len().div_ceil(MINI);
            for index in 0..count {
                let value = if index + 1 == count {
                    ENDOFCHAIN
                } else {
                    start + index as u32 + 1
                };
                // The defect: the first stream's mini-FAT chain loops
                // back to its own start.
                let looping = defect == Defect::FatCycle && stream_index == 0 && index == 0;
                mini_fat.push(if looping { start } else { value });
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
    for (index, (_, data)) in streams.iter().enumerate() {
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

    // The defect: the first stream's FAT chain loops back to its start.
    if let (Defect::FatCycle, Some(Placement::Normal { start, .. })) =
        (defect, placement.first().copied())
        && let Some(slot) = fat.get_mut(start as usize)
    {
        *slot = start;
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
    for (index, (name, _)) in streams.iter().enumerate() {
        let (start, size) = match placement[index] {
            Placement::Mini { start, size } | Placement::Normal { start, size } => (start, size),
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
    for index in 1..109 {
        put_u32(&mut header, 76 + index * 4, FREESECT);
    }

    let mut file = header;
    for sector in &sectors {
        file.extend_from_slice(sector);
    }
    file
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
