use crate::partial_root_recovery::{
    RawDirectoryLink, RawPhysicalDirectoryEntry, RecoveredRegularStreamPrefixBySid,
    RegularStreamStorageKind, RootRegularStreamPrefixStatus, RootRegularStreamSourceRange,
    RootRegularStreamTruncationReason,
};
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};

const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
const MAX_REGULAR_SECTOR: u32 = 0xffff_fffa;
const NO_STREAM: u32 = 0xffff_ffff;
const DIR_ENTRY_LEN: usize = 128;
const MINI_STREAM_CUTOFF: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TruncatedCfbPhysicalGap {
    UnalignedTail {
        byte_len: u64,
    },
    HeaderFatSectorUnavailable {
        difat_index: usize,
        sector_id: u32,
    },
    DifatSectorUnavailable {
        ordinal: usize,
        sector_id: u32,
    },
    DifatFatSectorUnavailable {
        difat_ordinal: usize,
        slot: usize,
        sector_id: u32,
    },
    FatListIncomplete {
        declared: usize,
        available: usize,
    },
    DirectorySectorUnavailable {
        sector_id: u32,
    },
    DirectoryFatEntryMissing {
        sector_id: u32,
    },
    DirectoryInvalidNextSector {
        sector_id: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TruncatedRawPhysicalDirectoryInventory {
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub physical_sector_count: usize,
    pub fat_entry_count: usize,
    pub directory_sector_count: usize,
    pub entries: Vec<RawPhysicalDirectoryEntry>,
    pub rejected_active_entry_count: usize,
    pub gaps: Vec<TruncatedCfbPhysicalGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TruncatedPhysicalContext {
    major: u16,
    sector_len: usize,
    physical_sector_count: usize,
    mini_stream_cutoff: u64,
    fat: Vec<u32>,
    directory: Vec<u8>,
    directory_sector_count: usize,
    gaps: Vec<TruncatedCfbPhysicalGap>,
}

/// Reads only physically complete CFB sectors and returns the surviving raw
/// directory prefix without asserting that the declared FAT/DIFAT or directory
/// topology is complete.
///
/// Missing EOF sectors are explicit gaps. Directory SIDs remain exact physical
/// identities in the source bytes, but parent/child and red-black-tree topology
/// MUST NOT be inferred from this inventory when links leave the surviving
/// prefix.
pub fn inspect_truncated_cfb_raw_directory_reader<R: Read + Seek>(
    mut reader: R,
) -> Result<TruncatedRawPhysicalDirectoryInventory> {
    reader
        .seek(SeekFrom::Start(0))
        .context("failed to seek to truncated CFB input")?;
    let mut source = Vec::new();
    reader
        .read_to_end(&mut source)
        .context("failed to read truncated CFB input")?;

    let context = build_truncated_context(&source)?;
    let entry_count = context.directory.len() / DIR_ENTRY_LEN;
    let mut entries = Vec::new();
    let mut rejected_active_entry_count = 0usize;

    for sid_usize in 0..entry_count {
        let start = sid_usize
            .checked_mul(DIR_ENTRY_LEN)
            .context("directory SID offset overflow")?;
        let raw = &context.directory[start..start + DIR_ENTRY_LEN];
        let object_type = raw[66];
        if object_type == 0 {
            continue;
        }
        if sid_usize == 0 {
            if object_type != 5 {
                anyhow::bail!("CFB directory SID 0 is not root");
            }
        } else if !matches!(object_type, 1 | 2) {
            rejected_active_entry_count += 1;
            continue;
        }

        let low_len = read_u32(raw, 120)? as u64;
        let high_len = read_u32(raw, 124)? as u64;
        let declared_len = if context.major == 4 {
            low_len | (high_len << 32)
        } else {
            low_len
        };
        let sid = u32::try_from(sid_usize).context("directory SID does not fit u32")?;
        entries.push(RawPhysicalDirectoryEntry {
            sid,
            object_type,
            descriptive_name: directory_name(raw).ok(),
            left_sibling: raw_directory_link(raw, 68, entry_count)?,
            right_sibling: raw_directory_link(raw, 72, entry_count)?,
            child: raw_directory_link(raw, 76, entry_count)?,
            start_sector: read_u32(raw, 116)?,
            declared_len,
        });
    }

    Ok(TruncatedRawPhysicalDirectoryInventory {
        source_sha256: sha256_hex(&source),
        source_byte_len: source.len() as u64,
        physical_sector_count: context.physical_sector_count,
        fat_entry_count: context.fat.len(),
        directory_sector_count: context.directory_sector_count,
        entries,
        rejected_active_entry_count,
        gaps: context.gaps,
    })
}

/// Recovers only physically present regular-sector bytes for an exact directory
/// SID from a truncated CFB. Selection is SID-bound; directory names are
/// descriptive metadata only. Missing sectors never become zero-fill and the
/// returned source ranges identify every recovered byte.
pub fn recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha<R: Read + Seek>(
    mut reader: R,
    stream_sid: u32,
    expected_source_sha256: &str,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    if !is_sha256_hex(expected_source_sha256) {
        anyhow::bail!("expected source SHA-256 must be 64 hexadecimal characters");
    }

    reader
        .seek(SeekFrom::Start(0))
        .context("failed to seek to truncated SID-bound recovery input")?;
    let mut source = Vec::new();
    reader
        .read_to_end(&mut source)
        .context("failed to read truncated SID-bound recovery input")?;

    let source_sha256 = sha256_hex(&source);
    if !source_sha256.eq_ignore_ascii_case(expected_source_sha256) {
        anyhow::bail!(
            "source identity mismatch for SID {stream_sid}: expected {expected_source_sha256}, observed {source_sha256}"
        );
    }

    let context = build_truncated_context(&source)?;
    let entry_count = context.directory.len() / DIR_ENTRY_LEN;
    let sid = usize::try_from(stream_sid).context("stream SID does not fit usize")?;
    if sid >= entry_count {
        anyhow::bail!("directory SID {stream_sid} is outside surviving directory prefix");
    }
    let start = sid
        .checked_mul(DIR_ENTRY_LEN)
        .context("directory SID offset overflow")?;
    let entry = &context.directory[start..start + DIR_ENTRY_LEN];
    if entry[66] != 2 {
        anyhow::bail!("directory SID {stream_sid} is not a stream");
    }

    recover_stream_from_entry(&source, &context, stream_sid, entry, source_sha256)
}

fn build_truncated_context(source: &[u8]) -> Result<TruncatedPhysicalContext> {
    if source.len() < 512 || source.get(..8) != Some(CFB_SIGNATURE.as_slice()) {
        anyhow::bail!("not a CFB container");
    }

    let major = read_u16(source, 26)?;
    if read_u16(source, 28)? != 0xfffe {
        anyhow::bail!("unsupported CFB byte order");
    }
    let sector_shift = read_u16(source, 30)?;
    let sector_len = match (major, sector_shift) {
        (3, 9) => 512usize,
        (4, 12) => 4096usize,
        _ => anyhow::bail!("unsupported CFB major/sector pair {major}/{sector_shift}"),
    };
    if read_u16(source, 32)? != 6 {
        anyhow::bail!("unsupported CFB mini-sector size");
    }
    if source.len() < sector_len {
        anyhow::bail!("truncated CFB does not contain a complete header sector");
    }

    let physical_payload_len = source.len() - sector_len;
    let physical_sector_count = physical_payload_len / sector_len;
    let trailing = physical_payload_len % sector_len;
    let mut gaps = Vec::new();
    if trailing != 0 {
        gaps.push(TruncatedCfbPhysicalGap::UnalignedTail {
            byte_len: trailing as u64,
        });
    }

    let num_fat_sectors = read_u32(source, 44)? as usize;
    let first_directory_sector = read_u32(source, 48)?;
    let mini_stream_cutoff = read_u32(source, 56)? as u64;
    if mini_stream_cutoff != MINI_STREAM_CUTOFF {
        anyhow::bail!("unexpected mini-stream cutoff {mini_stream_cutoff}");
    }

    let fat_sector_ids = read_available_fat_sector_ids(
        source,
        sector_len,
        physical_sector_count,
        num_fat_sectors,
        &mut gaps,
    )?;
    if fat_sector_ids.is_empty() {
        anyhow::bail!("no physically available FAT sector prefix");
    }

    let mut fat = Vec::new();
    let mut seen_fat = BTreeSet::new();
    for sector_id in fat_sector_ids {
        if !seen_fat.insert(sector_id) {
            anyhow::bail!("duplicate FAT sector {sector_id}");
        }
        let sector = physical_sector(source, sector_len, physical_sector_count, sector_id)
            .with_context(|| format!("physically available FAT sector {sector_id} disappeared"))?;
        for offset in (0..sector_len).step_by(4) {
            fat.push(read_u32(sector, offset)?);
        }
    }

    let mut directory = Vec::new();
    let mut directory_sector_count = 0usize;
    let mut current = first_directory_sector;
    let mut seen_directory = BTreeSet::new();
    loop {
        if current == END_OF_CHAIN {
            break;
        }
        if !is_declared_regular_sector(current) {
            gaps.push(TruncatedCfbPhysicalGap::DirectoryInvalidNextSector { sector_id: current });
            break;
        }
        let Some(sector) = physical_sector(source, sector_len, physical_sector_count, current)
        else {
            gaps.push(TruncatedCfbPhysicalGap::DirectorySectorUnavailable { sector_id: current });
            break;
        };
        if !seen_directory.insert(current) {
            anyhow::bail!("directory chain cycle at sector {current}");
        }
        directory.extend_from_slice(sector);
        directory_sector_count += 1;

        let current_usize =
            usize::try_from(current).context("directory sector does not fit usize")?;
        let Some(next) = fat.get(current_usize).copied() else {
            gaps.push(TruncatedCfbPhysicalGap::DirectoryFatEntryMissing { sector_id: current });
            break;
        };
        if next == END_OF_CHAIN {
            break;
        }
        if !is_declared_regular_sector(next) {
            gaps.push(TruncatedCfbPhysicalGap::DirectoryInvalidNextSector { sector_id: next });
            break;
        }
        current = next;
        if directory_sector_count > physical_sector_count {
            anyhow::bail!("directory chain exceeds physical sector count");
        }
    }

    if directory.len() < DIR_ENTRY_LEN {
        anyhow::bail!("no complete CFB root directory entry survives");
    }
    if directory[66] != 5 {
        anyhow::bail!("CFB directory SID 0 is not root");
    }

    Ok(TruncatedPhysicalContext {
        major,
        sector_len,
        physical_sector_count,
        mini_stream_cutoff,
        fat,
        directory,
        directory_sector_count,
        gaps,
    })
}

fn read_available_fat_sector_ids(
    source: &[u8],
    sector_len: usize,
    physical_sector_count: usize,
    declared_fat_count: usize,
    gaps: &mut Vec<TruncatedCfbPhysicalGap>,
) -> Result<Vec<u32>> {
    let mut ids = Vec::new();
    let mut stopped = false;

    for index in 0..109usize {
        if ids.len() >= declared_fat_count {
            break;
        }
        let sector_id = read_u32(source, 76 + index * 4)?;
        if sector_id == FREE_SECTOR {
            continue;
        }
        if !is_declared_regular_sector(sector_id)
            || physical_sector(source, sector_len, physical_sector_count, sector_id).is_none()
        {
            gaps.push(TruncatedCfbPhysicalGap::HeaderFatSectorUnavailable {
                difat_index: index,
                sector_id,
            });
            stopped = true;
            break;
        }
        ids.push(sector_id);
    }

    if ids.len() < declared_fat_count && !stopped {
        let mut difat_sector = read_u32(source, 68)?;
        let declared_difat_count = read_u32(source, 72)? as usize;
        let mut seen_difat = BTreeSet::new();

        for ordinal in 0..declared_difat_count {
            if ids.len() >= declared_fat_count {
                break;
            }
            let Some(difat) =
                physical_sector(source, sector_len, physical_sector_count, difat_sector)
            else {
                gaps.push(TruncatedCfbPhysicalGap::DifatSectorUnavailable {
                    ordinal,
                    sector_id: difat_sector,
                });
                break;
            };
            if !seen_difat.insert(difat_sector) {
                anyhow::bail!("DIFAT cycle at sector {difat_sector}");
            }

            let slots = sector_len / 4;
            for slot in 0..slots - 1 {
                if ids.len() >= declared_fat_count {
                    break;
                }
                let sector_id = read_u32(difat, slot * 4)?;
                if sector_id == FREE_SECTOR {
                    continue;
                }
                if !is_declared_regular_sector(sector_id)
                    || physical_sector(source, sector_len, physical_sector_count, sector_id)
                        .is_none()
                {
                    gaps.push(TruncatedCfbPhysicalGap::DifatFatSectorUnavailable {
                        difat_ordinal: ordinal,
                        slot,
                        sector_id,
                    });
                    stopped = true;
                    break;
                }
                ids.push(sector_id);
            }
            if stopped || ids.len() >= declared_fat_count {
                break;
            }
            difat_sector = read_u32(difat, sector_len - 4)?;
            if difat_sector == END_OF_CHAIN || difat_sector == FREE_SECTOR {
                break;
            }
        }
    }

    if ids.len() < declared_fat_count {
        gaps.push(TruncatedCfbPhysicalGap::FatListIncomplete {
            declared: declared_fat_count,
            available: ids.len(),
        });
    }
    Ok(ids)
}

fn recover_stream_from_entry(
    source: &[u8],
    context: &TruncatedPhysicalContext,
    stream_sid: u32,
    entry: &[u8],
    source_sha256: String,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    let low_len = read_u32(entry, 120)? as u64;
    let high_len = read_u32(entry, 124)? as u64;
    let declared_len = if context.major == 4 {
        low_len | (high_len << 32)
    } else {
        low_len
    };
    if declared_len < context.mini_stream_cutoff {
        anyhow::bail!(
            "stream SID {stream_sid} is {declared_len} bytes and therefore requires MiniFAT"
        );
    }
    let declared_len_usize =
        usize::try_from(declared_len).context("regular stream length does not fit usize")?;
    let needed_sectors = declared_len_usize
        .checked_add(context.sector_len - 1)
        .context("regular stream sector count overflow")?
        / context.sector_len;

    let mut bytes = Vec::with_capacity(declared_len_usize.min(source.len()));
    let mut source_ranges = Vec::new();
    let mut current = read_u32(entry, 116)?;
    let mut seen = BTreeSet::new();
    let mut truncation_reason = None;

    for ordinal in 0..needed_sectors {
        if !is_declared_regular_sector(current) {
            truncation_reason = Some(RootRegularStreamTruncationReason::InvalidNextSector);
            break;
        }
        let Some(sector) = physical_sector(
            source,
            context.sector_len,
            context.physical_sector_count,
            current,
        ) else {
            truncation_reason = Some(RootRegularStreamTruncationReason::PhysicalSectorUnavailable);
            break;
        };
        if !seen.insert(current) {
            anyhow::bail!("cycle in stream SID {stream_sid} at sector {current}");
        }

        let remaining = declared_len_usize.saturating_sub(bytes.len());
        let take = remaining.min(context.sector_len);
        let source_offset =
            (usize::try_from(current).context("sector index does not fit usize")? + 1)
                .checked_mul(context.sector_len)
                .context("sector source offset overflow")?;
        bytes.extend_from_slice(&sector[..take]);
        source_ranges.push(RootRegularStreamSourceRange {
            offset: source_offset as u64,
            len: take as u64,
        });

        let current_usize = usize::try_from(current).context("sector index does not fit usize")?;
        let Some(next) = context.fat.get(current_usize).copied() else {
            truncation_reason = Some(RootRegularStreamTruncationReason::FatEntryMissing);
            break;
        };

        if ordinal + 1 < needed_sectors {
            if next == END_OF_CHAIN {
                truncation_reason = Some(RootRegularStreamTruncationReason::UnexpectedEndOfChain);
                break;
            }
            if !is_declared_regular_sector(next) {
                truncation_reason = Some(RootRegularStreamTruncationReason::InvalidNextSector);
                break;
            }
            current = next;
        } else if next != END_OF_CHAIN {
            anyhow::bail!(
                "stream SID {stream_sid} chain continues past declared length via {next}"
            );
        }
    }

    if bytes.is_empty() {
        anyhow::bail!("no physical prefix bytes recovered for stream SID {stream_sid}");
    }

    let complete = bytes.len() == declared_len_usize && truncation_reason.is_none();
    Ok(RecoveredRegularStreamPrefixBySid {
        prefix_sha256: sha256_hex(&bytes),
        available_prefix_len: bytes.len() as u64,
        bytes,
        source_sha256,
        source_byte_len: source.len() as u64,
        stream_sid,
        descriptive_name: directory_name(entry).ok(),
        storage_kind: RegularStreamStorageKind::FatRegular,
        declared_len,
        status: if complete {
            RootRegularStreamPrefixStatus::Complete
        } else {
            RootRegularStreamPrefixStatus::Partial
        },
        truncation_reason,
        source_ranges,
        source_modified: false,
    })
}

fn raw_directory_link(raw: &[u8], offset: usize, entry_count: usize) -> Result<RawDirectoryLink> {
    let sid = read_u32(raw, offset)?;
    if sid == NO_STREAM {
        return Ok(RawDirectoryLink::None);
    }
    Ok(
        if usize::try_from(sid)
            .ok()
            .is_some_and(|value| value < entry_count)
        {
            RawDirectoryLink::InRange(sid)
        } else {
            RawDirectoryLink::OutOfRange(sid)
        },
    )
}

fn physical_sector(
    source: &[u8],
    sector_len: usize,
    physical_sector_count: usize,
    sector_id: u32,
) -> Option<&[u8]> {
    let sector = usize::try_from(sector_id).ok()?;
    if sector >= physical_sector_count {
        return None;
    }
    let start = sector.checked_add(1)?.checked_mul(sector_len)?;
    let end = start.checked_add(sector_len)?;
    source.get(start..end)
}

fn is_declared_regular_sector(sector: u32) -> bool {
    !matches!(
        sector,
        FREE_SECTOR | END_OF_CHAIN | FAT_SECTOR | DIFAT_SECTOR
    ) && sector <= MAX_REGULAR_SECTOR
}

fn directory_name(entry: &[u8]) -> Result<String> {
    let byte_len = read_u16(entry, 64)? as usize;
    if !(2..=64).contains(&byte_len) || byte_len % 2 != 0 {
        anyhow::bail!("invalid CFB directory name length {byte_len}");
    }
    if entry.get(byte_len - 2..byte_len) != Some(&[0, 0][..]) {
        anyhow::bail!("CFB directory name is not NUL-terminated");
    }
    let raw = &entry[..byte_len - 2];
    let utf16 = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&utf16).context("invalid UTF-16 CFB directory name")
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let raw = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("u16 outside truncated CFB buffer at {offset}"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let raw = bytes
        .get(offset..offset + 4)
        .with_context(|| format!("u32 outside truncated CFB buffer at {offset}"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn fixture() -> Vec<u8> {
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("fixture CFB");
        compound.create_storage("/Escher").expect("Escher storage");
        compound
            .create_stream("/Escher/EscherDelayStm")
            .expect("delay stream")
            .write_all(&vec![0x5a; 9_000])
            .expect("delay bytes");
        compound.flush().expect("fixture flush");
        compound.into_inner().into_inner()
    }

    fn sha(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    #[test]
    fn directory_name_requires_declared_nul_terminator() {
        let mut entry = [0u8; DIR_ENTRY_LEN];
        let encoded = "EscherDelayStm\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        entry[..encoded.len()].copy_from_slice(&encoded);
        entry[64..66].copy_from_slice(&(encoded.len() as u16).to_le_bytes());
        assert_eq!(
            directory_name(&entry).expect("valid directory name"),
            "EscherDelayStm"
        );

        entry[encoded.len() - 2..encoded.len()].copy_from_slice(&(b'X' as u16).to_le_bytes());
        assert!(directory_name(&entry).is_err());
    }

    #[test]
    fn terminal_partial_sector_is_not_fabricated() {
        let mut source = fixture();
        source.extend_from_slice(&[0xaa; 37]);

        let inventory = inspect_truncated_cfb_raw_directory_reader(Cursor::new(&source))
            .expect("truncated raw directory");
        assert!(
            inventory
                .gaps
                .iter()
                .any(|gap| matches!(gap, TruncatedCfbPhysicalGap::UnalignedTail { byte_len: 37 }))
        );
        assert!(inventory.entries.iter().any(|entry| {
            entry.object_type == 2 && entry.descriptive_name.as_deref() == Some("EscherDelayStm")
        }));
    }

    #[test]
    fn overstated_declared_fat_count_keeps_physical_prefix() {
        let mut source = fixture();
        let declared = read_u32(&source, 44).expect("declared FAT count");
        source[44..48].copy_from_slice(&declared.saturating_add(1).to_le_bytes());

        let inventory = inspect_truncated_cfb_raw_directory_reader(Cursor::new(&source))
            .expect("physical FAT prefix must survive");
        assert!(
            inventory
                .gaps
                .iter()
                .any(|gap| matches!(gap, TruncatedCfbPhysicalGap::FatListIncomplete { .. }))
        );
        assert!(inventory.entries.iter().any(|entry| {
            entry.object_type == 2 && entry.descriptive_name.as_deref() == Some("EscherDelayStm")
        }));
    }

    #[test]
    fn sid_bound_recovery_keeps_source_ranges_and_sha_gate() {
        let source = fixture();
        let inventory = inspect_truncated_cfb_raw_directory_reader(Cursor::new(&source))
            .expect("raw inventory");
        let stream = inventory
            .entries
            .iter()
            .find(|entry| {
                entry.object_type == 2
                    && entry.descriptive_name.as_deref() == Some("EscherDelayStm")
            })
            .expect("delay SID");

        let recovered = recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
            Cursor::new(&source),
            stream.sid,
            &sha(&source),
        )
        .expect("SID-bound recovery");
        assert_eq!(recovered.available_prefix_len, recovered.declared_len);
        assert_eq!(recovered.bytes, vec![0x5a; 9_000]);
        assert_eq!(
            recovered
                .source_ranges
                .iter()
                .map(|range| range.len)
                .sum::<u64>(),
            9_000
        );

        assert!(
            recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
                Cursor::new(&source),
                stream.sid,
                &"0".repeat(64),
            )
            .is_err()
        );
    }
}
