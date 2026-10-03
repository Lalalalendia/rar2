use super::CONTENTS_STREAM_PATH;
use anyhow::{Context, Result, bail};
use pub_contents::{Legacy0x22Directory, Legacy0x22DirectoryEntry, parse_legacy_0x22_directory};
use pub_core::StreamPath;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek, SeekFrom};

const LEGACY_DOCUMENT_TYPE: u16 = 0x0015;
const LEGACY_PAGE_TYPE: u16 = 0x0014;
const LEGACY_LIST_HEADER_SIZE: usize = 10;
const LEGACY_LIST_U16_RECORD_SIZE: u16 = 2;

pub const LEGACY22_PAGE_ROLE_OBSERVATION_SCHEMA_V1: &str =
    "chaptera.legacy22-page-role-observation.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legacy22PageListEntryObservationV1 {
    pub document_ordinal: usize,
    pub raw_type: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legacy22PageRoleObservationReceiptV1 {
    pub schema: String,
    pub document_page_list_entry_count: usize,
    pub physical_page_count: usize,
    pub page_list_entries: Vec<Legacy22PageListEntryObservationV1>,
}

/// Emits the strict old-0x22 DOCUMENT/PageList topology used by the presentation
/// layer without classifying customer-visible pages.
///
/// This deliberately does not use the bounded low-family CFB recovery path.
/// The current product profile was proven only on strict CFB + legacy directory
/// parses; recovery-only inputs must remain on the generic no-loss projection.
pub fn analyze_legacy_0x22_page_roles<R: Read + Seek>(
    mut reader: R,
) -> Result<Legacy22PageRoleObservationReceiptV1> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("strictly read {CONTENTS_STREAM_PATH}"))?;
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let directory = parse_legacy_0x22_directory(stream, &contents)
        .context("parse strict legacy 0x22 directory for PAGE-role observation")?;
    let document = unique_entry_by_type(&directory, LEGACY_DOCUMENT_TYPE, "DOCUMENT 0x0015")?;
    let page_list = parse_u16_id_list(&contents, document, "DOCUMENT PageList")?;
    if page_list.is_empty() {
        bail!("legacy DOCUMENT PageList is empty");
    }
    if page_list.iter().copied().collect::<BTreeSet<_>>().len() != page_list.len() {
        bail!("legacy DOCUMENT PageList repeats an object id");
    }

    let mut page_list_entries = Vec::with_capacity(page_list.len());
    for (document_ordinal, object_id) in page_list.into_iter().enumerate() {
        let entry = directory.entry_by_object_id(object_id).with_context(|| {
            format!("legacy DOCUMENT PageList references missing object {object_id}")
        })?;
        page_list_entries.push(Legacy22PageListEntryObservationV1 {
            document_ordinal,
            raw_type: entry.chunk_type,
        });
    }

    let physical_page_count = directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == LEGACY_PAGE_TYPE)
        .count();

    Ok(Legacy22PageRoleObservationReceiptV1 {
        schema: LEGACY22_PAGE_ROLE_OBSERVATION_SCHEMA_V1.to_owned(),
        document_page_list_entry_count: page_list_entries.len(),
        physical_page_count,
        page_list_entries,
    })
}

fn unique_entry_by_type<'a>(
    directory: &'a Legacy0x22Directory,
    chunk_type: u16,
    label: &str,
) -> Result<&'a Legacy0x22DirectoryEntry> {
    let mut matches = directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == chunk_type);
    let first = matches
        .next()
        .with_context(|| format!("missing legacy {label}"))?;
    if matches.next().is_some() {
        bail!("multiple legacy {label} objects");
    }
    Ok(first)
}

fn parse_u16_id_list(
    contents: &[u8],
    entry: &Legacy0x22DirectoryEntry,
    label: &str,
) -> Result<Vec<u16>> {
    let chunk_start = usize::try_from(entry.chunk_source.offset)
        .context("legacy chunk offset does not fit usize")?;
    let chunk_len = usize::try_from(entry.chunk_source.len)
        .context("legacy chunk length does not fit usize")?;
    let chunk_end = chunk_start
        .checked_add(chunk_len)
        .filter(|end| *end <= contents.len())
        .context("legacy chunk span exceeds Contents")?;
    let data_delta = *contents
        .get(chunk_start + 3)
        .with_context(|| format!("{label}: missing dataRelativeOffset"))?;
    let list_start = chunk_start
        .checked_add(usize::from(data_delta))
        .context("legacy list offset overflow")?;
    let header_end = list_start
        .checked_add(LEGACY_LIST_HEADER_SIZE)
        .context("legacy list header overflow")?;
    if header_end > chunk_end {
        bail!("{label}: list header exceeds bounded chunk");
    }

    let count = read_u16(contents, list_start).context("legacy list count missing")?;
    let max_count = read_u16(contents, list_start + 2).context("legacy list max_count missing")?;
    let record_size =
        read_u16(contents, list_start + 4).context("legacy list record_size missing")?;
    if max_count < count {
        bail!("{label}: max_count {max_count} is less than count {count}");
    }
    if record_size != LEGACY_LIST_U16_RECORD_SIZE {
        bail!("{label}: record size {record_size}, expected {LEGACY_LIST_U16_RECORD_SIZE}");
    }

    let payload_start = header_end;
    let payload_len = usize::from(count)
        .checked_mul(usize::from(record_size))
        .context("legacy list payload overflow")?;
    let payload_end = payload_start
        .checked_add(payload_len)
        .filter(|end| *end <= chunk_end)
        .context("legacy list payload exceeds bounded chunk")?;

    let mut ids = Vec::with_capacity(usize::from(count));
    for index in 0..usize::from(count) {
        let offset = payload_start + index * usize::from(record_size);
        ids.push(read_u16(contents, offset).context("legacy list object id missing")?);
    }
    debug_assert_eq!(payload_end, payload_start + payload_len);
    Ok(ids)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}
