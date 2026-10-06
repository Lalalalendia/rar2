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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootRegularStreamPrefixStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegularStreamStorageKind {
    FatRegular,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveredRegularStreamPrefixBySid {
    pub bytes: Vec<u8>,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub stream_sid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptive_name: Option<String>,
    pub storage_kind: RegularStreamStorageKind,
    pub declared_len: u64,
    pub available_prefix_len: u64,
    pub prefix_sha256: String,
    pub status: RootRegularStreamPrefixStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<RootRegularStreamTruncationReason>,
    pub source_ranges: Vec<RootRegularStreamSourceRange>,
    pub source_modified: bool,
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

/// Returns only physically proven prefix bytes for the exact CFB directory
/// stream ID.
///
/// Stream identity is the directory-entry SID inside these exact source bytes.
/// The directory name is descriptive metadata only and is never used to select
/// the stream. V1 is intentionally limited to regular-sector/FAT-backed
/// streams; MiniFAT-backed streams fail closed.
///
/// This is physical evidence extraction, not logical-path reconstruction and
/// not CFB repair.
pub fn recover_regular_stream_prefix_by_sid_reader<R: Read + Seek>(
    reader: R,
    stream_sid: u32,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    recover_regular_stream_prefix_by_sid_reader_inner(reader, stream_sid, None)
}

/// Same physical recovery primitive with an explicit expected source SHA-256
/// admission gate. The expected digest is identity evidence only; it never
/// changes stream selection or parsing.
pub fn recover_regular_stream_prefix_by_sid_reader_with_expected_sha<R: Read + Seek>(
    reader: R,
    stream_sid: u32,
    expected_source_sha256: &str,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    if !is_sha256_hex(expected_source_sha256) {
        anyhow::bail!("expected source SHA-256 must be 64 hexadecimal characters");
    }
    recover_regular_stream_prefix_by_sid_reader_inner(
        reader,
        stream_sid,
        Some(expected_source_sha256),
    )
}

fn recover_regular_stream_prefix_by_sid_reader_inner<R: Read + Seek>(
    mut reader: R,
    stream_sid: u32,
    expected_source_sha256: Option<&str>,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    reader
        .seek(SeekFrom::Start(0))
        .context("failed to seek to SID-bound partial CFB recovery input")?;
    let mut source = Vec::new();
    reader
        .read_to_end(&mut source)
        .context("failed to read SID-bound partial CFB recovery input")?;

    let recovered = recover_regular_stream_prefix_by_sid_from_bytes(&source, stream_sid)
        .with_context(|| format!("failed to recover regular stream prefix for SID {stream_sid}"))?;
    if expected_source_sha256
        .is_some_and(|expected| !recovered.source_sha256.eq_ignore_ascii_case(expected))
    {
        anyhow::bail!(
            "source identity mismatch for SID {stream_sid}: expected {}, observed {}",
            expected_source_sha256.unwrap_or_default(),
            recovered.source_sha256
        );
    }
    Ok(recovered)
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
    let start = usize::try_from(stream_sid).context("stream SID does not fit usize")?
        * DIR_ENTRY_LEN;
    let entry = &directory[start..start + DIR_ENTRY_LEN];
    let recovered = recover_regular_stream_prefix_from_entry(
        source,
        major,
        sector_len,
        num_sectors,
        mini_stream_cutoff,
        &fat,
        stream_sid,
        entry,
        &format!("root stream {stream_name}"),
    )?;

    Ok(RecoveredRootRegularStreamPrefix {
        bytes: recovered.bytes,
        stream_sid: recovered.stream_sid,
        declared_len: recovered.declared_len,
        available_prefix_len: recovered.available_prefix_len,
        status: recovered.status,
        truncation_reason: recovered.truncation_reason,
        root_entry_names,
        source_ranges: recovered.source_ranges,
    })
}

fn recover_regular_stream_prefix_by_sid_from_bytes(
    source: &[u8],
    stream_sid: u32,
) -> Result<RecoveredRegularStreamPrefixBySid> {
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
    if directory[66] != 5 {
        anyhow::bail!("first CFB directory entry is not root");
    }

    let entry_count = directory.len() / DIR_ENTRY_LEN;
    let sid = usize::try_from(stream_sid).context("stream SID does not fit usize")?;
    if sid >= entry_count {
        anyhow::bail!("directory SID {stream_sid} is out of range");
    }
    let start = sid
        .checked_mul(DIR_ENTRY_LEN)
        .context("directory SID offset overflow")?;
    let entry = &directory[start..start + DIR_ENTRY_LEN];
    if entry[66] != 2 {
        anyhow::bail!("directory SID {stream_sid} is not a stream");
    }

    recover_regular_stream_prefix_from_entry(
        source,
        major,
        sector_len,
        num_sectors,
        mini_stream_cutoff,
        &fat,
        stream_sid,
        entry,
        &format!("stream SID {stream_sid}"),
    )
}

#[allow(clippy::too_many_arguments)]
fn recover_regular_stream_prefix_from_entry(
    source: &[u8],
    major: u16,
    sector_len: usize,
    num_sectors: usize,
    mini_stream_cutoff: u64,
    fat: &[u32],
    stream_sid: u32,
    entry: &[u8],
    label: &str,
) -> Result<RecoveredRegularStreamPrefixBySid> {
    let start_sector = read_u32(entry, 116)?;
    let low_len = read_u32(entry, 120)? as u64;
    let high_len = read_u32(entry, 124)? as u64;
    let stream_len = if major == 4 {
        low_len | (high_len << 32)
    } else {
        low_len
    };
    if stream_len < mini_stream_cutoff {
        anyhow::bail!("{label} is {stream_len} bytes and therefore requires MiniFAT");
    }

    let stream_len_usize =
        usize::try_from(stream_len).context("regular stream length does not fit usize")?;
    let needed_sectors = stream_len_usize
        .checked_add(sector_len - 1)
        .context("regular stream sector count overflow")?
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
            anyhow::bail!("cycle in {label} at sector {current}");
        }

        let Some(sector) = sector_if_available(source, sector_len, current) else {
            truncation_reason = Some(RootRegularStreamTruncationReason::PhysicalSectorUnavailable);
            break;
        };

        let remaining = stream_len_usize.saturating_sub(bytes.len());
        let take = remaining.min(sector_len);
        let source_offset = (usize::try_from(current).context("sector index does not fit usize")? + 1)
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
            anyhow::bail!("{label} chain continues past declared length via {next}");
        }
    }

    if bytes.is_empty() {
        anyhow::bail!("no physical prefix bytes recovered for {label}");
    }

    let complete = bytes.len() == stream_len_usize && truncation_reason.is_none();
    let prefix_sha256 = sha256_hex(&bytes);
    Ok(RecoveredRegularStreamPrefixBySid {
        source_sha256: sha256_hex(source),
        source_byte_len: u64::try_from(source.len())
            .context("source byte length does not fit u64")?,
        stream_sid,
        descriptive_name: directory_name(entry).ok(),
        storage_kind: RegularStreamStorageKind::FatRegular,
        available_prefix_len: u64::try_from(bytes.len())
            .context("available prefix length does not fit u64")?,
        prefix_sha256,
        bytes,
        declared_len: stream_len,
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
    if num_difat_sectors > 0
        && difat_sector != END_OF_CHAIN
        && difat_sector != FREE_SECTOR
    {
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

fn fat_chain_to_end(
    start: u32,
    fat: &[u32],
    num_sectors: usize,
    label: &str,
) -> Result<Vec<u32>> {
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
    !matches!(sector, FREE_SECTOR | END_OF_CHAIN | FAT_SECTOR | DIFAT_SECTOR)
        && sector <= MAX_REGULAR_SECTOR
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

    fn directory_bytes(source: &[u8]) -> (usize, Vec<u32>, Vec<u8>) {
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let num_sectors = source.len() / sector_len - 1;
        let num_fat_sectors = u32::from_le_bytes([
            source[44], source[45], source[46], source[47],
        ]) as usize;
        let first_directory_sector =
            u32::from_le_bytes([source[48], source[49], source[50], source[51]]);
        let fat =
            read_fat(source, sector_len, num_sectors, num_fat_sectors).expect("fixture FAT");
        let directory_sector_ids =
            fat_chain_to_end(first_directory_sector, &fat, num_sectors, "fixture directory")
                .expect("fixture directory chain");
        let mut directory = Vec::new();
        for sector_id in &directory_sector_ids {
            directory.extend_from_slice(
                read_sector(source, sector_len, *sector_id).expect("fixture directory sector"),
            );
        }
        (sector_len, directory_sector_ids, directory)
    }

    fn stream_sid_by_name(source: &[u8], expected_name: &str) -> u32 {
        let (_, _, directory) = directory_bytes(source);
        for sid in 1..directory.len() / DIR_ENTRY_LEN {
            let start = sid * DIR_ENTRY_LEN;
            let entry = &directory[start..start + DIR_ENTRY_LEN];
            if entry[66] == 2
                && directory_name(entry).ok().as_deref() == Some(expected_name)
            {
                return u32::try_from(sid).expect("fixture SID");
            }
        }
        panic!("stream {expected_name} missing from fixture directory");
    }

    fn patch_directory_name(source: &mut [u8], sid: u32, new_name: &str) {
        let (sector_len, directory_sector_ids, _) = directory_bytes(source);
        let sid = usize::try_from(sid).expect("fixture SID usize");
        let logical_offset = sid * DIR_ENTRY_LEN;
        let directory_sector_ordinal = logical_offset / sector_len;
        let within_sector = logical_offset % sector_len;
        let directory_sector = directory_sector_ids[directory_sector_ordinal];
        let raw_offset =
            (usize::try_from(directory_sector).expect("directory sector") + 1) * sector_len
                + within_sector;
        let entry = &mut source[raw_offset..raw_offset + DIR_ENTRY_LEN];

        let mut encoded = new_name.encode_utf16().collect::<Vec<_>>();
        assert!(encoded.len() <= 31);
        encoded.push(0);
        entry[..64].fill(0);
        for (index, unit) in encoded.iter().enumerate() {
            entry[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let byte_len = u16::try_from(encoded.len() * 2).expect("directory name length");
        entry[64..66].copy_from_slice(&byte_len.to_le_bytes());
    }

    fn nested_regular_fixture() -> (Vec<u8>, u32, Vec<u8>) {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("nested fixture CFB");
        compound.create_storage("/Escher").expect("Escher storage");
        let expected = vec![0x6b; 9_000];
        compound
            .create_stream("/Escher/EscherDelayStm")
            .expect("nested delay stream")
            .write_all(&expected)
            .expect("write nested delay stream");
        compound.flush().expect("flush nested fixture");
        let source = compound.into_inner().into_inner();
        let sid = stream_sid_by_name(&source, "EscherDelayStm");
        (source, sid, expected)
    }

    fn root_contents_start_sector(source: &[u8]) -> u32 {
        let full = recover_root_regular_stream_prefix_reader(Cursor::new(source.to_vec()), "/Contents")
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
        let directory_sector =
            u32::from_le_bytes([source[48], source[49], source[50], source[51]]);
        let directory_offset = (usize::try_from(directory_sector).unwrap() + 1) * sector_len;
        let root_child_offset = directory_offset + 76;
        let root_child = u32::from_le_bytes([
            source[root_child_offset],
            source[root_child_offset + 1],
            source[root_child_offset + 2],
            source[root_child_offset + 3],
        ]);
        assert_ne!(root_child, NO_STREAM);
        let child_offset =
            directory_offset + usize::try_from(root_child).unwrap() * DIR_ENTRY_LEN;
        source[child_offset + 68..child_offset + 72]
            .copy_from_slice(&root_child.to_le_bytes());
        source
    }

    #[test]
    fn complete_prefix_matches_strict_root_reader_exactly() {
        let source = regular_root_fixture();
        let strict = crate::recover_root_regular_stream_reader(
            Cursor::new(source.clone()),
            "/Contents",
        )
        .expect("strict complete Contents");
        let prefix = recover_root_regular_stream_prefix_reader(
            Cursor::new(source),
            "/Contents",
        )
        .expect("prefix complete Contents");

        assert_eq!(prefix.status, RootRegularStreamPrefixStatus::Complete);
        assert_eq!(prefix.bytes, strict.bytes);
        assert_eq!(prefix.root_entry_names, strict.root_entry_names);
    }

    #[test]
    fn complete_control_returns_complete_without_changing_contract() {
        let source = regular_root_fixture();
        let recovered =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
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
        let recovered =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
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
        let error =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
                .expect_err("stream chain cycle must fail closed");
        assert!(format!("{error:#}").contains("cycle in root stream"));
    }

    #[test]
    fn root_directory_cycle_fails_closed() {
        let source = cycle_root_directory_sibling(regular_root_fixture());
        let error =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
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
        let error =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Contents")
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

        let missing =
            recover_root_regular_stream_prefix_reader(Cursor::new(source), "/Missing")
                .expect_err("missing root stream must be rejected");
        assert!(format!("{missing:#}").contains("is absent"));
    }

    #[test]
    fn exact_sid_recovers_nested_regular_stream_without_path_selection() {
        let (source, sid, expected) = nested_regular_fixture();
        let recovered =
            recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), sid)
                .expect("recover nested regular stream by SID");
        assert_eq!(recovered.stream_sid, sid);
        assert_eq!(recovered.source_sha256, sha256_hex(&source));
        assert_eq!(recovered.source_byte_len, source.len() as u64);
        assert_eq!(recovered.descriptive_name.as_deref(), Some("EscherDelayStm"));
        assert_eq!(recovered.storage_kind, RegularStreamStorageKind::FatRegular);
        assert_eq!(recovered.status, RootRegularStreamPrefixStatus::Complete);
        assert_eq!(recovered.prefix_sha256, sha256_hex(&expected));
        assert_eq!(recovered.bytes, expected);
        assert_eq!(recovered.available_prefix_len, recovered.declared_len);
        assert!(!recovered.source_modified);
    }

    #[test]
    fn expected_source_sha_gate_fails_closed_on_mismatch() {
        let (source, sid, _) = nested_regular_fixture();
        let expected = sha256_hex(&source);
        let recovered = recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
            Cursor::new(source.clone()),
            sid,
            &expected,
        )
        .expect("matching source identity");
        assert_eq!(recovered.source_sha256, expected);

        let wrong = "00".repeat(32);
        let error = recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
            Cursor::new(source),
            sid,
            &wrong,
        )
        .expect_err("mismatched source identity must fail closed");
        assert!(format!("{error:#}").contains("source identity mismatch"));
    }

    #[test]
    fn non_stream_sid_fails_closed() {
        let (source, _, _) = nested_regular_fixture();
        let error = recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), 0)
            .expect_err("root SID must not be recoverable as a stream");
        assert!(format!("{error:#}").contains("is not a stream"));
    }

    #[test]
    fn sid_bound_recovery_rejects_minifat_stream() {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("small SID fixture");
        compound
            .create_stream("/Small")
            .expect("small stream")
            .write_all(b"small")
            .expect("write small stream");
        compound.flush().expect("flush small SID fixture");
        let source = compound.into_inner().into_inner();
        let sid = stream_sid_by_name(&source, "Small");

        let error = recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), sid)
            .expect_err("MiniFAT stream must remain outside V1");
        assert!(format!("{error:#}").contains("requires MiniFAT"));
    }

    #[test]
    fn sid_bound_stream_cycle_fails_closed() {
        let (mut source, sid, _) = nested_regular_fixture();
        let recovered =
            recover_regular_stream_prefix_by_sid_reader(Cursor::new(source.clone()), sid)
                .expect("complete SID-bound stream");
        let first_range = recovered.source_ranges.first().expect("first source range");
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let start_sector =
            u32::try_from(usize::try_from(first_range.offset).unwrap() / sector_len - 1)
                .expect("start sector");
        let fat_sector = first_fat_sector(&source);
        let fat_offset = (usize::try_from(fat_sector).unwrap() + 1) * sector_len;
        let entry_offset = fat_offset + usize::try_from(start_sector).unwrap() * 4;
        source[entry_offset..entry_offset + 4].copy_from_slice(&start_sector.to_le_bytes());

        let error = recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), sid)
            .expect_err("SID-bound stream cycle must fail closed");
        assert!(format!("{error:#}").contains("cycle in stream SID"));
    }

    #[test]
    fn case_colliding_names_remain_distinct_by_sid() {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("collision fixture");
        let lower_payload = vec![0x31; 9_000];
        let upper_payload = vec![0x72; 9_000];
        compound
            .create_stream("/contents")
            .expect("lower stream")
            .write_all(&lower_payload)
            .expect("write lower");
        compound
            .create_stream("/CONTENt2")
            .expect("upper staging stream")
            .write_all(&upper_payload)
            .expect("write upper");
        compound.flush().expect("flush collision fixture");
        let mut source = compound.into_inner().into_inner();

        let lower_sid = stream_sid_by_name(&source, "contents");
        let upper_sid = stream_sid_by_name(&source, "CONTENt2");
        assert_ne!(lower_sid, upper_sid);
        patch_directory_name(&mut source, upper_sid, "CONTENTS");

        let lower =
            recover_regular_stream_prefix_by_sid_reader(Cursor::new(source.clone()), lower_sid)
                .expect("lower by SID");
        let upper =
            recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), upper_sid)
                .expect("upper by SID");

        assert_eq!(lower.descriptive_name.as_deref(), Some("contents"));
        assert_eq!(upper.descriptive_name.as_deref(), Some("CONTENTS"));
        assert_eq!(lower.bytes, lower_payload);
        assert_eq!(upper.bytes, upper_payload);
        assert_ne!(lower.bytes, upper.bytes);
    }
}
