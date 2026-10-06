use anyhow::{Context, Result};
use serde::Serialize;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootRegularStreamPrefixStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootRegularStreamTruncationReason {
    PhysicalSectorUnavailable,
    FatEntryMissing,
    InvalidNextSector,
    UnexpectedEndOfChain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootRegularStreamSourceRange {
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveredRootRegularStreamPrefix {
    pub bytes: Vec<u8>,
    pub stream_sid: u32,
    pub declared_len: u64,
    pub available_prefix_len: u64,
    pub status: RootRegularStreamPrefixStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<RootRegularStreamTruncationReason>,
    pub root_entry_names: Vec<String>,
    pub source_ranges: Vec<RootRegularStreamSourceRange>,
}

/// Returns only physically proven prefix bytes for one direct-root regular
/// CFB stream.
///
/// Unlike the complete root regular stream reader, this primitive may return
/// a partial prefix when the stream chain becomes unavailable before the
/// directory-declared stream length. It never pads missing bytes, never
/// pretends a partial prefix is a complete stream, and still requires a fully
/// readable root directory and FAT topology up to the truncation point.
///
/// This is evidence extraction, not CFB repair.
pub fn recover_root_regular_stream_prefix_reader<R: Read + Seek>(
    mut reader: R,
    stream_path: &str,
) -> Result<RecoveredRootRegularStreamPrefix> {
    let stream_name = stream_path
        .strip_prefix('/')
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .with_context(|| {
            format!("partial recovery path must name one direct root stream: {stream_path}")
        })?;

    reader
        .seek(SeekFrom::Start(0))
        .context("failed to seek to partial CFB recovery input")?;
    let mut source = Vec::new();
    reader
        .read_to_end(&mut source)
        .context("failed to read partial CFB recovery input")?;

    recover_root_regular_stream_prefix_from_bytes(&source, stream_name)
        .with_context(|| format!("failed to recover root stream prefix {stream_path}"))
}

fn recover_root_regular_stream_prefix_from_bytes(
    source: &[u8],
    stream_name: &str,
) -> Result<RecoveredRootRegularStreamPrefix> {
    if source.len() < 512 || source.get(..8) != Some(CFB_SIGNATURE.as_slice()) {
        anyhow::bail!("not a CFB container");
    }

    let major = read_u16(source, 26)?;
    let byte_order = read_u16(source, 28)?;
    if byte_order != 0xfffe {
        anyhow::bail!("unsupported CFB byte order {byte_order:#06x}");
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
    if source.len() < sector_len || source.len() % sector_len != 0 {
        anyhow::bail!("unaligned CFB partial recovery input");
    }

    let num_sectors = source.len() / sector_len - 1;
    let num_fat_sectors = read_u32(source, 44)? as usize;
    let first_directory_sector = read_u32(source, 48)?;
    let mini_stream_cutoff = read_u32(source, 56)? as u64;
    if mini_stream_cutoff != MINI_STREAM_CUTOFF {
        anyhow::bail!("unexpected mini-stream cutoff {mini_stream_cutoff}");
    }

    let fat = read_fat(source, sector_len, num_sectors, num_fat_sectors)?;
    let directory_sector_ids =
        fat_chain_to_end(first_directory_sector, &fat, num_sectors, "directory")?;
    if directory_sector_ids.is_empty() {
        anyhow::bail!("empty CFB directory chain");
    }

    let mut directory = Vec::with_capacity(directory_sector_ids.len() * sector_len);
    for sector_id in directory_sector_ids {
        directory.extend_from_slice(read_sector(source, sector_len, sector_id)?);
    }
    if directory.len() < DIR_ENTRY_LEN {
        anyhow::bail!("missing CFB root directory entry");
    }

    let root = &directory[..DIR_ENTRY_LEN];
    if root[66] != 5 {
        anyhow::bail!("first CFB directory entry is not root");
    }

    let entry_count = directory.len() / DIR_ENTRY_LEN;
    let root_child = read_u32(root, 76)?;
    let mut pending = vec![root_child];
    let mut seen_sids = BTreeSet::new();
    let mut root_entry_names = Vec::new();
    let mut matching_stream_sid = None;

    while let Some(sid) = pending.pop() {
        if sid == NO_STREAM {
            continue;
        }
        let sid_usize = usize::try_from(sid).context("directory SID does not fit usize")?;
        if sid_usize >= entry_count {
            anyhow::bail!("root directory tree references out-of-range SID {sid}");
        }
        if !seen_sids.insert(sid) {
            anyhow::bail!("cycle in root directory sibling tree at SID {sid}");
        }
        let start = sid_usize * DIR_ENTRY_LEN;
        let entry = &directory[start..start + DIR_ENTRY_LEN];
        let obj_type = entry[66];
        if !matches!(obj_type, 1 | 2) {
            anyhow::bail!("unexpected root child object type {obj_type} at SID {sid}");
        }
        let name = directory_name(entry)?;
        root_entry_names.push(name.clone());
        if obj_type == 2
            && name.eq_ignore_ascii_case(stream_name)
            && matching_stream_sid.replace(sid).is_some()
        {
            anyhow::bail!("duplicate root stream name {stream_name}");
        }
        pending.push(read_u32(entry, 68)?);
        pending.push(read_u32(entry, 72)?);
    }
    root_entry_names.sort();

    let stream_sid =
        matching_stream_sid.with_context(|| format!("root stream {stream_name} is absent"))?;
    let start =
        usize::try_from(stream_sid).context("stream SID does not fit usize")? * DIR_ENTRY_LEN;
    let entry = &directory[start..start + DIR_ENTRY_LEN];
    let start_sector = read_u32(entry, 116)?;
    let low_len = read_u32(entry, 120)? as u64;
    let high_len = read_u32(entry, 124)? as u64;
    let stream_len = if major == 4 {
        low_len | (high_len << 32)
    } else {
        low_len
    };
    if stream_len < mini_stream_cutoff {
        anyhow::bail!(
            "root stream {stream_name} is {stream_len} bytes and therefore requires MiniFAT"
        );
    }

    let stream_len_usize =
        usize::try_from(stream_len).context("root stream length does not fit usize")?;
    let needed_sectors = stream_len_usize
        .checked_add(sector_len - 1)
        .context("root stream sector count overflow")?
        / sector_len;

    let mut bytes = Vec::with_capacity(stream_len_usize.min(source.len()));
    let mut source_ranges = Vec::new();
    let mut current = start_sector;
    let mut seen_stream = BTreeSet::new();
    let mut truncation_reason = None;

    for ordinal in 0..needed_sectors {
        if !is_regular_sector(current, num_sectors) {
            truncation_reason = Some(RootRegularStreamTruncationReason::PhysicalSectorUnavailable);
            break;
        }
        if !seen_stream.insert(current) {
            anyhow::bail!("cycle in root stream {stream_name} at sector {current}");
        }

        let Some(sector) = sector_if_available(source, sector_len, current) else {
            truncation_reason = Some(RootRegularStreamTruncationReason::PhysicalSectorUnavailable);
            break;
        };

        let remaining = stream_len_usize.saturating_sub(bytes.len());
        let take = remaining.min(sector_len);
        let source_offset =
            (usize::try_from(current).context("sector index does not fit usize")? + 1)
                .checked_mul(sector_len)
                .context("sector source offset overflow")?;
        bytes.extend_from_slice(&sector[..take]);
        source_ranges.push(RootRegularStreamSourceRange {
            offset: u64::try_from(source_offset).context("source offset does not fit u64")?,
            len: u64::try_from(take).context("source range length does not fit u64")?,
        });

        let current_usize = usize::try_from(current).context("sector index does not fit usize")?;
        let Some(next) = fat.get(current_usize).copied() else {
            truncation_reason = Some(RootRegularStreamTruncationReason::FatEntryMissing);
            break;
        };

        if ordinal + 1 < needed_sectors {
            if next == END_OF_CHAIN {
                truncation_reason = Some(RootRegularStreamTruncationReason::UnexpectedEndOfChain);
                break;
            }
            if !is_regular_sector(next, num_sectors) {
                truncation_reason = Some(RootRegularStreamTruncationReason::InvalidNextSector);
                break;
            }
            current = next;
        } else if next != END_OF_CHAIN {
            anyhow::bail!(
                "root stream {stream_name} chain continues past declared length via {next}"
            );
        }
    }

    if bytes.is_empty() {
        anyhow::bail!("no physical prefix bytes recovered for root stream {stream_name}");
    }

    let complete = bytes.len() == stream_len_usize && truncation_reason.is_none();
    Ok(RecoveredRootRegularStreamPrefix {
        stream_sid,
        available_prefix_len: u64::try_from(bytes.len())
            .context("available prefix length does not fit u64")?,
        bytes,
        declared_len: stream_len,
        status: if complete {
            RootRegularStreamPrefixStatus::Complete
        } else {
            RootRegularStreamPrefixStatus::Partial
        },
        truncation_reason,
        root_entry_names,
        source_ranges,
    })
}

fn read_fat(
    source: &[u8],
    sector_len: usize,
    num_sectors: usize,
    num_fat_sectors: usize,
) -> Result<Vec<u32>> {
    let mut fat_sector_ids = Vec::new();
    for index in 0..109usize {
        let sector = read_u32(source, 76 + index * 4)?;
        if sector != FREE_SECTOR {
            fat_sector_ids.push(sector);
        }
    }

    let mut difat_sector = read_u32(source, 68)?;
    let num_difat_sectors = read_u32(source, 72)? as usize;
    let mut seen_difat = BTreeSet::new();
    for _ in 0..num_difat_sectors {
        require_regular_sector(difat_sector, num_sectors, "DIFAT")?;
        if !seen_difat.insert(difat_sector) {
            anyhow::bail!("DIFAT cycle at sector {difat_sector}");
        }
        let sector = read_sector(source, sector_len, difat_sector)?;
        let slots = sector_len / 4;
        for index in 0..slots - 1 {
            let value = read_u32(sector, index * 4)?;
            if value != FREE_SECTOR {
                fat_sector_ids.push(value);
            }
        }
        difat_sector = read_u32(sector, sector_len - 4)?;
    }
    if num_difat_sectors > 0 && difat_sector != END_OF_CHAIN && difat_sector != FREE_SECTOR {
        anyhow::bail!("DIFAT chain did not terminate");
    }
    if fat_sector_ids.len() < num_fat_sectors {
        anyhow::bail!(
            "CFB partial recovery found {} FAT sectors, header requires {}",
            fat_sector_ids.len(),
            num_fat_sectors
        );
    }
    fat_sector_ids.truncate(num_fat_sectors);

    let mut fat = Vec::new();
    let mut seen_fat_sectors = BTreeSet::new();
    for sector_id in fat_sector_ids {
        require_regular_sector(sector_id, num_sectors, "FAT")?;
        if !seen_fat_sectors.insert(sector_id) {
            anyhow::bail!("duplicate FAT sector {sector_id}");
        }
        let sector = read_sector(source, sector_len, sector_id)?;
        for offset in (0..sector_len).step_by(4) {
            fat.push(read_u32(sector, offset)?);
        }
    }
    Ok(fat)
}

fn fat_chain_to_end(start: u32, fat: &[u32], num_sectors: usize, label: &str) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        require_regular_sector(current, num_sectors, label)?;
        if !seen.insert(current) {
            anyhow::bail!("{label} chain cycle at sector {current}");
        }
        out.push(current);
        if out.len() > num_sectors {
            anyhow::bail!("{label} chain exceeds sector count");
        }
        current = *fat
            .get(usize::try_from(current).context("sector index does not fit usize")?)
            .with_context(|| format!("FAT is missing sector entry {current}"))?;
    }
    Ok(out)
}

fn is_regular_sector(sector: u32, num_sectors: usize) -> bool {
    !matches!(
        sector,
        FREE_SECTOR | END_OF_CHAIN | FAT_SECTOR | DIFAT_SECTOR
    ) && sector <= MAX_REGULAR_SECTOR
        && usize::try_from(sector)
            .ok()
            .is_some_and(|value| value < num_sectors)
}

fn require_regular_sector(sector: u32, num_sectors: usize, label: &str) -> Result<()> {
    if !is_regular_sector(sector, num_sectors) {
        anyhow::bail!("{label} references invalid sector {sector}");
    }
    Ok(())
}

fn sector_if_available(source: &[u8], sector_len: usize, sector_id: u32) -> Option<&[u8]> {
    let sector = usize::try_from(sector_id).ok()?;
    let start = sector.checked_add(1)?.checked_mul(sector_len)?;
    let end = start.checked_add(sector_len)?;
    source.get(start..end)
}

fn read_sector(source: &[u8], sector_len: usize, sector_id: u32) -> Result<&[u8]> {
    sector_if_available(source, sector_len, sector_id)
        .with_context(|| format!("sector {sector_id} lies outside CFB"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let raw = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("u16 outside partial recovery buffer at {offset}"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let raw = bytes
        .get(offset..offset + 4)
        .with_context(|| format!("u32 outside partial recovery buffer at {offset}"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn directory_name(entry: &[u8]) -> Result<String> {
    let byte_len = read_u16(entry, 64)? as usize;
    if !(2..=64).contains(&byte_len) || byte_len % 2 != 0 {
        anyhow::bail!("invalid CFB directory name length {byte_len}");
    }
    let raw = &entry[..byte_len - 2];
    let utf16 = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&utf16).context("invalid UTF-16 CFB directory name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn regular_root_fixture() -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("partial fixture CFB");
        let expected = vec![0x5a; 9_000];
        compound
            .create_stream("/Contents")
            .expect("root Contents")
            .write_all(&expected)
            .expect("write Contents");
        compound.flush().expect("flush partial fixture");
        compound.into_inner().into_inner()
    }

    fn case_colliding_root_fixture() -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("collision fixture CFB");
        compound
            .create_stream("/Contents")
            .expect("Contents")
            .write_all(&vec![0x41; 4_096])
            .expect("write Contents");
        compound
            .create_stream("/Contentz")
            .expect("Contentz")
            .write_all(&vec![0x42; 4_096])
            .expect("write Contentz");
        compound.flush().expect("flush collision fixture");
        let mut source = compound.into_inner().into_inner();

        let from = "Contentz\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let to = "CONTENTS\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(from.len(), to.len());

        let positions = source
            .windows(from.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == from.as_slice()).then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(positions.len(), 1, "expected one Contentz directory name");
        source[positions[0]..positions[0] + to.len()].copy_from_slice(&to);
        source
    }

    fn first_fat_sector(source: &[u8]) -> u32 {
        u32::from_le_bytes([source[76], source[77], source[78], source[79]])
    }

    fn root_contents_start_sector(source: &[u8]) -> u32 {
        let full =
            recover_root_regular_stream_prefix_reader(Cursor::new(source.to_vec()), "/Contents")
                .expect("complete root Contents evidence");
        let first_range = full.source_ranges.first().expect("first source range");
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        u32::try_from((usize::try_from(first_range.offset).unwrap() / sector_len) - 1).unwrap()
    }

    fn break_contents_chain_after_first_sector(mut source: Vec<u8>) -> Vec<u8> {
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let start_sector = root_contents_start_sector(&source);
        let fat_sector = first_fat_sector(&source);
        assert_ne!(fat_sector, FREE_SECTOR);
        let fat_offset = (usize::try_from(fat_sector).unwrap() + 1) * sector_len;
        let entry_offset = fat_offset + usize::try_from(start_sector).unwrap() * 4;
        source[entry_offset..entry_offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        source
    }

    fn cycle_contents_chain_after_first_sector(mut source: Vec<u8>) -> Vec<u8> {
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let start_sector = root_contents_start_sector(&source);
        let fat_sector = first_fat_sector(&source);
        let fat_offset = (usize::try_from(fat_sector).unwrap() + 1) * sector_len;
        let entry_offset = fat_offset + usize::try_from(start_sector).unwrap() * 4;
        source[entry_offset..entry_offset + 4].copy_from_slice(&start_sector.to_le_bytes());
        source
    }

    fn cycle_root_directory_sibling(mut source: Vec<u8>) -> Vec<u8> {
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let directory_sector = u32::from_le_bytes([source[48], source[49], source[50], source[51]]);
        let directory_offset = (usize::try_from(directory_sector).unwrap() + 1) * sector_len;
        let root_child_offset = directory_offset + 76;
        let root_child = u32::from_le_bytes([
            source[root_child_offset],
            source[root_child_offset + 1],
            source[root_child_offset + 2],
            source[root_child_offset + 3],
        ]);
        assert_ne!(root_child, NO_STREAM);
        let child_offset = directory_offset + usize::try_from(root_child).unwrap() * DIR_ENTRY_LEN;
        source[child_offset + 68..child_offset + 72].copy_from_slice(&root_child.to_le_bytes());
        source
    }

    #[test]
    fn complete_prefix_matches_strict_root_reader_exactly() {
        let source = regular_root_fixture();
        let strict =
            crate::recover_root_regular_stream_reader(Cursor::new(source.clone()), "/Contents")
                .expect("strict complete Contents");
        let prefix = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect("prefix complete Contents");

        assert_eq!(prefix.status, RootRegularStreamPrefixStatus::Complete);
        assert_eq!(prefix.bytes, strict.bytes);
        assert_eq!(prefix.root_entry_names, strict.root_entry_names);
    }

    #[test]
    fn complete_control_returns_complete_without_changing_contract() {
        let source = regular_root_fixture();
        let recovered = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect("complete Contents");
        assert_eq!(recovered.status, RootRegularStreamPrefixStatus::Complete);
        assert!(recovered.stream_sid > 0);
        assert_eq!(recovered.available_prefix_len, recovered.declared_len);
        assert!(recovered.truncation_reason.is_none());
        assert_eq!(recovered.bytes.len(), 9_000);
        assert!(!recovered.source_ranges.is_empty());
    }

    #[test]
    fn broken_fat_link_returns_only_physically_proven_prefix() {
        let source = break_contents_chain_after_first_sector(regular_root_fixture());
        let recovered = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect("partial Contents evidence");

        assert_eq!(recovered.status, RootRegularStreamPrefixStatus::Partial);
        assert_eq!(
            recovered.truncation_reason,
            Some(RootRegularStreamTruncationReason::InvalidNextSector)
        );
        assert!(recovered.available_prefix_len > 0);
        assert!(recovered.available_prefix_len < recovered.declared_len);
        assert_eq!(
            usize::try_from(recovered.available_prefix_len).unwrap(),
            recovered.bytes.len()
        );
        assert_eq!(recovered.source_ranges.len(), 1);
    }

    #[test]
    fn stream_chain_cycle_fails_closed() {
        let source = cycle_contents_chain_after_first_sector(regular_root_fixture());
        let error = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect_err("stream chain cycle must fail closed");
        assert!(format!("{error:#}").contains("cycle in root stream"));
    }

    #[test]
    fn root_directory_cycle_fails_closed() {
        let source = cycle_root_directory_sibling(regular_root_fixture());
        let error = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect_err("directory sibling cycle must fail closed");
        assert!(format!("{error:#}").contains("cycle in root directory sibling tree"));
    }

    #[test]
    fn mini_stream_root_input_remains_rejected() {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("small root fixture");
        compound
            .create_stream("/Small")
            .expect("small root stream")
            .write_all(b"small")
            .expect("write small root stream");
        compound.flush().expect("flush small root fixture");

        let error = recover_root_regular_stream_prefix_reader(
            Cursor::new(compound.into_inner().into_inner()),
            "/Small",
        )
        .expect_err("MiniFAT stream must remain outside partial regular recovery");
        assert!(format!("{error:#}").contains("requires MiniFAT"));
    }

    #[test]
    fn case_colliding_root_names_fail_closed() {
        let source = case_colliding_root_fixture();
        let error = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
            .expect_err("case-colliding root stream names must fail closed");
        assert!(format!("{error:#}").contains("duplicate root stream name"));
    }

    #[test]
    fn nested_or_missing_paths_fail_closed() {
        let source = regular_root_fixture();

        let nested = recover_root_regular_stream_prefix_reader(
            Cursor::new(source.clone()),
            "/Nested/Contents",
        )
        .expect_err("nested recovery path must be rejected");
        assert!(format!("{nested:#}").contains("direct root stream"));

        let missing = recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Missing")
            .expect_err("missing root stream must be rejected");
        assert!(format!("{missing:#}").contains("is absent"));
    }
}
