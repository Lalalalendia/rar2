use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

pub const CFB_INVENTORY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CfbEntry {
    pub path: String,
    pub name: String,
    pub kind: EntryKind,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Root,
    Storage,
    Stream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CfbInventory {
    pub schema_version: u32,
    pub entries: Vec<CfbEntry>,
}

pub fn inspect_path(path: impl AsRef<Path>) -> Result<CfbInventory> {
    let path = path.as_ref();
    let file =
        File::open(path).with_context(|| format!("не удалось открыть файл {}", path.display()))?;

    inspect_reader(file)
        .with_context(|| format!("не удалось разобрать CFB-файл {}", path.display()))
}

pub fn inspect_reader<R: Read + Seek>(reader: R) -> Result<CfbInventory> {
    let compound = cfb::CompoundFile::open(reader).context("не удалось разобрать CFB-контейнер")?;

    let mut entries: Vec<_> = compound
        .walk()
        .map(|entry| CfbEntry {
            path: canonical_cfb_path(entry.path()),
            name: entry.name().to_owned(),
            kind: if entry.is_root() {
                EntryKind::Root
            } else if entry.is_stream() {
                EntryKind::Stream
            } else {
                EntryKind::Storage
            },
            len: entry.len(),
        })
        .collect();

    entries.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(CfbInventory {
        schema_version: CFB_INVENTORY_SCHEMA_VERSION,
        entries,
    })
}

/// Читает один точный stream из CFB по логическому пути.
///
/// Функция ничего не знает о семантике Publisher и не нормализует имя потока.
/// Она нужна projection-парсерам, которым требуются исходные байты вместе с
/// отдельно отслеживаемым `StreamPath`.
pub fn read_stream_path(path: impl AsRef<Path>, stream_path: &str) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let file =
        File::open(path).with_context(|| format!("не удалось открыть файл {}", path.display()))?;

    read_stream_reader(file, stream_path).with_context(|| {
        format!(
            "не удалось прочитать поток {stream_path} из CFB-файла {}",
            path.display()
        )
    })
}

pub fn read_stream_reader<R: Read + Seek>(reader: R, stream_path: &str) -> Result<Vec<u8>> {
    let mut compound =
        cfb::CompoundFile::open(reader).context("не удалось разобрать CFB-контейнер")?;
    let mut stream = compound
        .open_stream(stream_path)
        .with_context(|| format!("в CFB отсутствует поток {stream_path}"))?;

    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .with_context(|| format!("не удалось прочитать поток {stream_path}"))?;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRootRegularStream {
    pub bytes: Vec<u8>,
    pub root_entry_names: Vec<String>,
}

/// Bounded recovery reader for one regular-sector stream directly under the
/// CFB root storage.
///
/// This deliberately does not parse or validate the MiniFAT. It exists for
/// files whose publication-critical root stream is intact while unrelated
/// embedded-object mini streams are malformed. The requested stream must be a
/// direct root child and must be at least the CFB mini-stream cutoff, so this
/// helper cannot be used as a generic malformed-CFB reader.
pub fn recover_root_regular_stream_reader<R: Read + Seek>(
    mut reader: R,
    stream_path: &str,
) -> Result<RecoveredRootRegularStream> {
    let stream_name = stream_path
        .strip_prefix('/')
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .with_context(|| {
            format!("recovery path must name one direct root stream: {stream_path}")
        })?;

    reader
        .seek(SeekFrom::Start(0))
        .context("не удалось перейти к началу CFB recovery input")?;
    let mut source = Vec::new();
    reader
        .read_to_end(&mut source)
        .context("не удалось прочитать CFB recovery input")?;

    recover_root_regular_stream_from_bytes(&source, stream_name)
        .with_context(|| format!("не удалось восстановить root stream {stream_path}"))
}

const RECOVERY_CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const RECOVERY_FREE_SECTOR: u32 = 0xffff_ffff;
const RECOVERY_END_OF_CHAIN: u32 = 0xffff_fffe;
const RECOVERY_FAT_SECTOR: u32 = 0xffff_fffd;
const RECOVERY_DIFAT_SECTOR: u32 = 0xffff_fffc;
const RECOVERY_MAX_REGULAR_SECTOR: u32 = 0xffff_fffa;
const RECOVERY_NO_STREAM: u32 = 0xffff_ffff;
const RECOVERY_DIR_ENTRY_LEN: usize = 128;
const RECOVERY_MINI_STREAM_CUTOFF: u64 = 4096;

fn recover_root_regular_stream_from_bytes(
    source: &[u8],
    stream_name: &str,
) -> Result<RecoveredRootRegularStream> {
    if source.len() < 512 || source.get(..8) != Some(RECOVERY_CFB_SIGNATURE.as_slice()) {
        anyhow::bail!("not a CFB container");
    }

    let major = recovery_u16(source, 26)?;
    let byte_order = recovery_u16(source, 28)?;
    if byte_order != 0xfffe {
        anyhow::bail!("unsupported CFB byte order {byte_order:#06x}");
    }
    let sector_shift = recovery_u16(source, 30)?;
    let sector_len = match (major, sector_shift) {
        (3, 9) => 512usize,
        (4, 12) => 4096usize,
        _ => anyhow::bail!("unsupported CFB major/sector pair {major}/{sector_shift}"),
    };
    if recovery_u16(source, 32)? != 6 {
        anyhow::bail!("unsupported CFB mini-sector size");
    }
    if source.len() < sector_len || source.len() % sector_len != 0 {
        anyhow::bail!("unaligned CFB recovery input");
    }
    let num_sectors = source.len() / sector_len - 1;
    let num_fat_sectors = recovery_u32(source, 44)? as usize;
    let first_directory_sector = recovery_u32(source, 48)?;
    let mini_stream_cutoff = recovery_u32(source, 56)? as u64;
    if mini_stream_cutoff != RECOVERY_MINI_STREAM_CUTOFF {
        anyhow::bail!("unexpected mini-stream cutoff {mini_stream_cutoff}");
    }

    let mut fat_sector_ids = Vec::new();
    for index in 0..109usize {
        let sector = recovery_u32(source, 76 + index * 4)?;
        if sector != RECOVERY_FREE_SECTOR {
            fat_sector_ids.push(sector);
        }
    }

    let mut difat_sector = recovery_u32(source, 68)?;
    let num_difat_sectors = recovery_u32(source, 72)? as usize;
    let mut seen_difat = std::collections::BTreeSet::new();
    for _ in 0..num_difat_sectors {
        recovery_require_regular_sector(difat_sector, num_sectors, "DIFAT")?;
        if !seen_difat.insert(difat_sector) {
            anyhow::bail!("DIFAT cycle at sector {difat_sector}");
        }
        let sector = recovery_sector(source, sector_len, difat_sector)?;
        let slots = sector_len / 4;
        for index in 0..slots - 1 {
            let value = recovery_u32(sector, index * 4)?;
            if value != RECOVERY_FREE_SECTOR {
                fat_sector_ids.push(value);
            }
        }
        difat_sector = recovery_u32(sector, sector_len - 4)?;
    }
    if num_difat_sectors > 0
        && difat_sector != RECOVERY_END_OF_CHAIN
        && difat_sector != RECOVERY_FREE_SECTOR
    {
        anyhow::bail!("DIFAT chain did not terminate");
    }
    if fat_sector_ids.len() < num_fat_sectors {
        anyhow::bail!(
            "CFB recovery found {} FAT sectors, header requires {}",
            fat_sector_ids.len(),
            num_fat_sectors
        );
    }
    fat_sector_ids.truncate(num_fat_sectors);

    let mut fat = Vec::new();
    let mut seen_fat_sectors = std::collections::BTreeSet::new();
    for sector_id in fat_sector_ids {
        recovery_require_regular_sector(sector_id, num_sectors, "FAT")?;
        if !seen_fat_sectors.insert(sector_id) {
            anyhow::bail!("duplicate FAT sector {sector_id}");
        }
        let sector = recovery_sector(source, sector_len, sector_id)?;
        for offset in (0..sector_len).step_by(4) {
            fat.push(recovery_u32(sector, offset)?);
        }
    }

    let directory_sector_ids =
        recovery_fat_chain_to_end(first_directory_sector, &fat, num_sectors, "directory")?;
    if directory_sector_ids.is_empty() {
        anyhow::bail!("empty CFB directory chain");
    }
    let mut directory = Vec::with_capacity(directory_sector_ids.len() * sector_len);
    for sector_id in directory_sector_ids {
        directory.extend_from_slice(recovery_sector(source, sector_len, sector_id)?);
    }
    if directory.len() < RECOVERY_DIR_ENTRY_LEN {
        anyhow::bail!("missing CFB root directory entry");
    }
    let root = &directory[..RECOVERY_DIR_ENTRY_LEN];
    if root[66] != 5 {
        anyhow::bail!("first CFB directory entry is not root");
    }

    let entry_count = directory.len() / RECOVERY_DIR_ENTRY_LEN;
    let root_child = recovery_u32(root, 76)?;
    let mut pending = vec![root_child];
    let mut seen_sids = std::collections::BTreeSet::new();
    let mut root_entry_names = Vec::new();
    let mut matching_stream_sid = None;

    while let Some(sid) = pending.pop() {
        if sid == RECOVERY_NO_STREAM {
            continue;
        }
        let sid_usize = usize::try_from(sid).context("directory SID does not fit usize")?;
        if sid_usize >= entry_count {
            anyhow::bail!("root directory tree references out-of-range SID {sid}");
        }
        if !seen_sids.insert(sid) {
            anyhow::bail!("cycle in root directory sibling tree at SID {sid}");
        }
        let start = sid_usize * RECOVERY_DIR_ENTRY_LEN;
        let entry = &directory[start..start + RECOVERY_DIR_ENTRY_LEN];
        let obj_type = entry[66];
        if !matches!(obj_type, 1 | 2) {
            anyhow::bail!("unexpected root child object type {obj_type} at SID {sid}");
        }
        let name = recovery_directory_name(entry)?;
        root_entry_names.push(name.clone());
        if obj_type == 2
            && name.eq_ignore_ascii_case(stream_name)
            && matching_stream_sid.replace(sid).is_some()
        {
            anyhow::bail!("duplicate root stream name {stream_name}");
        }
        pending.push(recovery_u32(entry, 68)?);
        pending.push(recovery_u32(entry, 72)?);
    }
    root_entry_names.sort();

    let stream_sid =
        matching_stream_sid.with_context(|| format!("root stream {stream_name} is absent"))?;
    let start = usize::try_from(stream_sid).context("stream SID does not fit usize")?
        * RECOVERY_DIR_ENTRY_LEN;
    let entry = &directory[start..start + RECOVERY_DIR_ENTRY_LEN];
    let start_sector = recovery_u32(entry, 116)?;
    let low_len = recovery_u32(entry, 120)? as u64;
    let high_len = recovery_u32(entry, 124)? as u64;
    let stream_len = if major == 4 {
        low_len | (high_len << 32)
    } else {
        low_len
    };
    if stream_len < mini_stream_cutoff {
        anyhow::bail!(
            "root stream {stream_name} is {} bytes and therefore requires MiniFAT",
            stream_len
        );
    }
    let stream_len_usize =
        usize::try_from(stream_len).context("root stream length does not fit usize")?;
    let needed_sectors = stream_len_usize
        .checked_add(sector_len - 1)
        .context("root stream sector count overflow")?
        / sector_len;

    let mut bytes = Vec::with_capacity(stream_len_usize);
    let mut current = start_sector;
    let mut seen_stream = std::collections::BTreeSet::new();
    for ordinal in 0..needed_sectors {
        recovery_require_regular_sector(current, num_sectors, stream_name)?;
        if !seen_stream.insert(current) {
            anyhow::bail!("cycle in root stream {stream_name} at sector {current}");
        }
        let sector = recovery_sector(source, sector_len, current)?;
        let remaining = stream_len_usize.saturating_sub(bytes.len());
        bytes.extend_from_slice(&sector[..remaining.min(sector_len)]);

        let current_usize = usize::try_from(current).context("sector index does not fit usize")?;
        let next = *fat
            .get(current_usize)
            .with_context(|| format!("FAT is missing sector entry {current}"))?;
        if ordinal + 1 < needed_sectors {
            recovery_require_regular_sector(next, num_sectors, stream_name)?;
            current = next;
        } else if next != RECOVERY_END_OF_CHAIN {
            anyhow::bail!(
                "root stream {stream_name} chain continues past declared length via {next}"
            );
        }
    }
    if bytes.len() != stream_len_usize {
        anyhow::bail!(
            "root stream {stream_name} recovered {} of {} bytes",
            bytes.len(),
            stream_len_usize
        );
    }

    Ok(RecoveredRootRegularStream {
        bytes,
        root_entry_names,
    })
}

fn recovery_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let raw = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("u16 outside recovery buffer at {offset}"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn recovery_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let raw = bytes
        .get(offset..offset + 4)
        .with_context(|| format!("u32 outside recovery buffer at {offset}"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn recovery_sector(source: &[u8], sector_len: usize, sector_id: u32) -> Result<&[u8]> {
    let sector = usize::try_from(sector_id).context("sector index does not fit usize")?;
    let start = sector
        .checked_add(1)
        .and_then(|value| value.checked_mul(sector_len))
        .context("sector offset overflow")?;
    let end = start
        .checked_add(sector_len)
        .context("sector end overflow")?;
    source
        .get(start..end)
        .with_context(|| format!("sector {sector_id} lies outside CFB"))
}

fn recovery_require_regular_sector(sector: u32, num_sectors: usize, label: &str) -> Result<()> {
    if matches!(
        sector,
        RECOVERY_FREE_SECTOR | RECOVERY_END_OF_CHAIN | RECOVERY_FAT_SECTOR | RECOVERY_DIFAT_SECTOR
    ) || sector > RECOVERY_MAX_REGULAR_SECTOR
        || usize::try_from(sector)
            .ok()
            .is_none_or(|value| value >= num_sectors)
    {
        anyhow::bail!("{label} references invalid sector {sector}");
    }
    Ok(())
}

fn recovery_fat_chain_to_end(
    start: u32,
    fat: &[u32],
    num_sectors: usize,
    label: &str,
) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut current = start;
    while current != RECOVERY_END_OF_CHAIN {
        recovery_require_regular_sector(current, num_sectors, label)?;
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

fn recovery_directory_name(entry: &[u8]) -> Result<String> {
    let byte_len = recovery_u16(entry, 64)? as usize;
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

#[derive(Debug, Clone)]
struct CfbRewriteEntry {
    path: PathBuf,
    kind: EntryKind,
    clsid: Uuid,
    state_bits: u32,
    created: SystemTime,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
}

/// Создаёт копию исходного CFB и заменяет в ней ровно один существующий stream.
///
/// В отличие от fresh repack, функция открывает byte-for-byte копию исходного
/// контейнера в read-write режиме. Поэтому source directory topology и прочее
/// неэкспонированное CFB-состояние не пересоздаётся без необходимости.
///
/// Все untouched logical stream payloads должны остаться byte-for-byte равны
/// source. Exposed entry metadata также проверяется после mutation.
///
/// Функция не утверждает, что конкретное приложение примет изменённый CFB.
/// Это preservation-first container primitive; native application validation
/// остаётся обязанностью вызывающего writer.
pub fn replace_stream_reader<R: Read + Seek>(
    mut reader: R,
    stream_path: &str,
    replacement: &[u8],
) -> Result<Vec<u8>> {
    reader
        .seek(SeekFrom::Start(0))
        .context("не удалось перейти к началу исходного CFB")?;
    let mut source_bytes = Vec::new();
    reader
        .read_to_end(&mut source_bytes)
        .context("не удалось прочитать исходные CFB-байты")?;

    let mut source = cfb::CompoundFile::open(std::io::Cursor::new(source_bytes.as_slice()))
        .context("не удалось разобрать исходный CFB-контейнер")?;
    let source_version = source.version();

    let mut entries = source
        .walk()
        .map(|entry| CfbRewriteEntry {
            path: entry.path().to_path_buf(),
            kind: if entry.is_root() {
                EntryKind::Root
            } else if entry.is_stream() {
                EntryKind::Stream
            } else {
                EntryKind::Storage
            },
            clsid: entry.clsid().to_owned(),
            state_bits: entry.state_bits(),
            created: entry.created(),
            modified: entry.modified(),
            bytes: None,
        })
        .collect::<Vec<_>>();

    let target_path = Path::new(stream_path);
    let target = entries
        .iter()
        .find(|entry| entry.path == target_path)
        .with_context(|| format!("в CFB отсутствует target stream {stream_path}"))?;
    if target.kind != EntryKind::Stream {
        anyhow::bail!("target path {stream_path} не является CFB stream");
    }

    for entry in &mut entries {
        if entry.kind != EntryKind::Stream {
            continue;
        }
        let mut stream = source.open_stream(&entry.path).with_context(|| {
            format!("не удалось открыть source stream {}", entry.path.display())
        })?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).with_context(|| {
            format!(
                "не удалось прочитать source stream {}",
                entry.path.display()
            )
        })?;
        entry.bytes = Some(bytes);
    }
    drop(source);

    let mut output = cfb::CompoundFile::open(std::io::Cursor::new(source_bytes))
        .context("не удалось открыть копию CFB в read-write режиме")?;
    {
        let mut stream = output.open_stream(target_path).with_context(|| {
            format!("не удалось открыть target stream {stream_path} для записи")
        })?;
        let replacement_len = u64::try_from(replacement.len())
            .context("размер replacement stream не помещается в u64")?;
        stream
            .set_len(replacement_len)
            .with_context(|| format!("не удалось изменить размер target stream {stream_path}"))?;
        stream
            .seek(SeekFrom::Start(0))
            .with_context(|| format!("не удалось перейти к началу target stream {stream_path}"))?;
        stream
            .write_all(replacement)
            .with_context(|| format!("не удалось записать target stream {stream_path}"))?;
        stream
            .flush()
            .with_context(|| format!("не удалось flush target stream {stream_path}"))?;
    }
    output.flush().context("не удалось flush output CFB")?;

    let bytes = output.into_inner().into_inner();
    validate_rewritten_cfb(&entries, target_path, replacement, source_version, &bytes)?;
    Ok(bytes)
}

fn validate_rewritten_cfb(
    source_entries: &[CfbRewriteEntry],
    target_path: &Path,
    replacement: &[u8],
    source_version: cfb::Version,
    output_bytes: &[u8],
) -> Result<()> {
    let mut output = cfb::CompoundFile::open(std::io::Cursor::new(output_bytes))
        .context("изменённый CFB не открывается")?;
    if output.version() != source_version {
        anyhow::bail!(
            "CFB version changed during stream mutation: source={source_version:?}, output={:?}",
            output.version()
        );
    }
    let output_entries = output
        .walk()
        .map(|entry| {
            let kind = if entry.is_root() {
                EntryKind::Root
            } else if entry.is_stream() {
                EntryKind::Stream
            } else {
                EntryKind::Storage
            };
            (
                entry.path().to_path_buf(),
                (
                    kind,
                    entry.clsid().to_owned(),
                    entry.state_bits(),
                    entry.created(),
                    entry.modified(),
                ),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    if output_entries.len() != source_entries.len() {
        anyhow::bail!(
            "CFB entry count changed during rewrite: source={}, output={}",
            source_entries.len(),
            output_entries.len()
        );
    }

    for source in source_entries {
        let (kind, clsid, state_bits, created, modified) = output_entries
            .get(&source.path)
            .with_context(|| format!("output CFB потерял entry {}", source.path.display()))?;
        if *kind != source.kind {
            anyhow::bail!("CFB entry kind changed for {}", source.path.display());
        }
        if *state_bits != source.state_bits {
            anyhow::bail!("CFB state bits changed for {}", source.path.display());
        }
        if matches!(source.kind, EntryKind::Root | EntryKind::Storage) {
            if clsid != &source.clsid {
                anyhow::bail!("CFB CLSID changed for {}", source.path.display());
            }
            if created != &source.created || modified != &source.modified {
                anyhow::bail!(
                    "CFB storage timestamps changed for {}",
                    source.path.display()
                );
            }
        }

        if source.kind == EntryKind::Stream {
            let mut stream = output.open_stream(&source.path).with_context(|| {
                format!(
                    "не удалось повторно открыть stream {}",
                    source.path.display()
                )
            })?;
            let mut actual = Vec::new();
            stream.read_to_end(&mut actual).with_context(|| {
                format!(
                    "не удалось повторно прочитать stream {}",
                    source.path.display()
                )
            })?;
            let expected = if source.path == target_path {
                replacement
            } else {
                source
                    .bytes
                    .as_deref()
                    .expect("source stream bytes were collected")
            };
            if actual != expected {
                anyhow::bail!(
                    "logical stream payload changed unexpectedly for {}",
                    source.path.display()
                );
            }
        }
    }

    Ok(())
}

fn canonical_cfb_path(path: &Path) -> String {
    let names: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy()),
            _ => None,
        })
        .collect();

    if names.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", names.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::time::{Duration, UNIX_EPOCH};

    fn synthetic_cfb() -> Cursor<Vec<u8>> {
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new()))
            .expect("синтетический CFB должен создаваться");

        compound
            .create_storage("/Zoo")
            .expect("хранилище Zoo должно создаваться");
        compound
            .create_storage("/Alpha")
            .expect("хранилище Alpha должно создаваться");

        compound
            .create_stream("/Zoo/last")
            .expect("поток Zoo/last должен создаваться")
            .write_all(b"1234")
            .expect("данные Zoo/last должны записываться");

        compound
            .create_stream("/Alpha/first")
            .expect("поток Alpha/first должен создаваться")
            .write_all(b"12")
            .expect("данные Alpha/first должны записываться");

        compound
            .set_storage_clsid(
                "/Alpha",
                Uuid::parse_str("12345678-1234-5678-9abc-def012345678").unwrap(),
            )
            .expect("CLSID Alpha должен устанавливаться");
        compound
            .set_state_bits("/Alpha", 0x1234_5678)
            .expect("state bits Alpha должны устанавливаться");
        compound
            .set_state_bits("/Zoo/last", 0x55aa_00ff)
            .expect("state bits stream должны устанавливаться");
        compound
            .set_created_time("/Alpha", UNIX_EPOCH + Duration::from_secs(1_000))
            .expect("created time Alpha должен устанавливаться");
        compound
            .set_modified_time("/Alpha", UNIX_EPOCH + Duration::from_secs(2_000))
            .expect("modified time Alpha должен устанавливаться");

        compound
            .flush()
            .expect("синтетический CFB должен сбрасываться в память");

        let mut cursor = compound.into_inner();
        cursor.set_position(0);
        cursor
    }

    #[test]
    fn inventory_is_sorted_by_canonical_path() {
        let inventory = inspect_reader(synthetic_cfb()).expect("CFB должен разбираться");

        let paths: Vec<_> = inventory
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();

        assert_eq!(
            paths,
            vec!["/", "/Alpha", "/Alpha/first", "/Zoo", "/Zoo/last"]
        );
        assert_eq!(inventory.schema_version, CFB_INVENTORY_SCHEMA_VERSION);
    }

    #[test]
    fn inventory_preserves_kind_and_stream_length() {
        let inventory = inspect_reader(synthetic_cfb()).expect("CFB должен разбираться");

        let first = inventory
            .entries
            .iter()
            .find(|entry| entry.path == "/Alpha/first")
            .expect("поток Alpha/first должен присутствовать");

        assert_eq!(first.kind, EntryKind::Stream);
        assert_eq!(first.len, 2);

        let alpha = inventory
            .entries
            .iter()
            .find(|entry| entry.path == "/Alpha")
            .expect("хранилище Alpha должно присутствовать");

        assert_eq!(alpha.kind, EntryKind::Storage);
        assert_eq!(alpha.len, 0);
    }

    #[test]
    fn reads_exact_stream_bytes() {
        let bytes =
            read_stream_reader(synthetic_cfb(), "/Alpha/first").expect("поток должен читаться");
        assert_eq!(bytes, b"12");
    }

    fn corrupt_first_minifat_entry(mut bytes: Vec<u8>) -> Vec<u8> {
        let sector_shift = u16::from_le_bytes([bytes[30], bytes[31]]);
        let sector_len = 1usize << sector_shift;
        let minifat_sector = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
        assert_ne!(minifat_sector, RECOVERY_END_OF_CHAIN);
        let offset = (minifat_sector as usize + 1) * sector_len;
        bytes[offset..offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes
    }

    #[test]
    fn recovery_reads_intact_root_regular_stream_without_validating_minifat() {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("synthetic recovery CFB");
        compound
            .create_storage("/Objects")
            .expect("Objects storage");
        compound
            .create_stream("/Objects/Damaged")
            .expect("small damaged stream")
            .write_all(b"small")
            .expect("write small stream");
        let expected = vec![0x5a; 5_000];
        compound
            .create_stream("/Contents")
            .expect("root Contents")
            .write_all(&expected)
            .expect("write Contents");
        compound.flush().expect("flush recovery CFB");
        let bytes = corrupt_first_minifat_entry(compound.into_inner().into_inner());

        assert!(inspect_reader(Cursor::new(bytes.clone())).is_err());

        let recovered = recover_root_regular_stream_reader(Cursor::new(bytes), "/Contents")
            .expect("regular root Contents should recover");
        assert_eq!(recovered.bytes, expected);
        assert!(
            recovered
                .root_entry_names
                .iter()
                .any(|name| name == "Contents")
        );
        assert!(
            recovered
                .root_entry_names
                .iter()
                .any(|name| name == "Objects")
        );
    }

    #[test]
    fn recovery_refuses_small_root_streams_that_require_minifat() {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("small root fixture");
        compound
            .create_stream("/Small")
            .expect("small root stream")
            .write_all(b"small")
            .expect("write small root stream");
        compound.flush().expect("flush small root fixture");
        let bytes = compound.into_inner().into_inner();

        let error = recover_root_regular_stream_reader(Cursor::new(bytes), "/Small")
            .expect_err("small root stream must not use recovery path");
        assert!(format!("{error:#}").contains("requires MiniFAT"));
    }

    #[test]
    fn missing_stream_is_explicit_error() {
        let error = read_stream_reader(synthetic_cfb(), "/Missing")
            .expect_err("несуществующий stream должен быть ошибкой");
        assert!(error.to_string().contains("/Missing"));
    }

    #[test]
    fn in_copy_mutation_replaces_one_stream_and_preserves_logical_state() {
        let source = synthetic_cfb().into_inner();
        let rewritten =
            replace_stream_reader(Cursor::new(source.clone()), "/Alpha/first", b"replacement")
                .expect("bounded CFB in-copy mutation должна пройти");

        assert_eq!(
            read_stream_reader(Cursor::new(rewritten.clone()), "/Alpha/first")
                .expect("target stream должен читаться"),
            b"replacement"
        );
        assert_eq!(
            read_stream_reader(Cursor::new(rewritten.clone()), "/Zoo/last")
                .expect("untouched stream должен читаться"),
            b"1234"
        );

        let original =
            cfb::CompoundFile::open(Cursor::new(source)).expect("source CFB должен открываться");
        let output =
            cfb::CompoundFile::open(Cursor::new(rewritten)).expect("output CFB должен открываться");
        assert_eq!(original.version(), output.version());

        for path in ["/Alpha", "/Zoo/last"] {
            let before = original.entry(path).expect("source entry");
            let after = output.entry(path).expect("output entry");
            assert_eq!(before.clsid(), after.clsid());
            assert_eq!(before.state_bits(), after.state_bits());
            if before.is_storage() {
                assert_eq!(before.created(), after.created());
                assert_eq!(before.modified(), after.modified());
            }
        }
    }

    #[test]
    fn in_copy_mutation_rejects_missing_or_non_stream_target() {
        let source = synthetic_cfb().into_inner();

        let missing = replace_stream_reader(Cursor::new(source.clone()), "/Missing", b"x")
            .expect_err("missing stream должен блокировать rewrite");
        assert!(missing.to_string().contains("/Missing"));

        let storage = replace_stream_reader(Cursor::new(source), "/Alpha", b"x")
            .expect_err("storage path не должен приниматься как stream");
        assert!(storage.to_string().contains("не является CFB stream"));
    }

    #[test]
    fn invalid_input_returns_cfb_error() {
        let error = inspect_reader(Cursor::new(b"not a cfb".to_vec()))
            .expect_err("произвольные байты не должны считаться CFB");

        assert!(
            error
                .to_string()
                .contains("не удалось разобрать CFB-контейнер"),
            "неожиданная ошибка: {error:#}"
        );
    }
}
