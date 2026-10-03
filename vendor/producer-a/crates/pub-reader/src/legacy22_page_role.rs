use super::CONTENTS_STREAM_PATH;
use anyhow::{Context, Result, bail};
use pub_contents::{Legacy0x22Directory, Legacy0x22DirectoryEntry, parse_legacy_0x22_directory};
use pub_core::StreamPath;
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek, SeekFrom};

const LEGACY_DOCUMENT_TYPE: u16 = 0x0015;
const LEGACY_PAGE_TYPE: u16 = 0x0014;
const LEGACY_PAGE_LIST_SPECIAL_TYPE: u16 = 0x0041;
const LEGACY_LIST_HEADER_SIZE: usize = 10;
const LEGACY_LIST_U16_RECORD_SIZE: u16 = 2;

pub const LEGACY22_NOQUILL_PAGE_PROFILE_ID_V1: &str =
    "publisher-legacy22/noquill-middle-pages/v1";
pub const LEGACY22_QUILL_PAGE_PROFILE_ID_V1: &str =
    "publisher-legacy22/quill-middle-pages/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legacy22PageListDialectV1 {
    NoQuill,
    Quill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy22PageListPresentationSelectionV1 {
    pub profile_id: &'static str,
    pub raw_page_list_entry_count: usize,
    pub materialized_page_count: usize,
    pub customer_page_indices: Vec<usize>,
}

pub fn select_legacy_0x22_page_list_presentation_v1<R: Read + Seek>(
    mut reader: R,
    dialect: Legacy22PageListDialectV1,
) -> Result<Option<Legacy22PageListPresentationSelectionV1>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    // Product admission is intentionally stricter than the low-family SourceGraph
    // recovery path. The 650-file recurrence authority was established only on
    // strict CFB + legacy directory/PageList parses; a recovery-only file must
    // retain the generic no-loss Viewer projection until independently proven.
    let contents = match pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        CONTENTS_STREAM_PATH,
    ) {
        Ok(contents) => contents,
        Err(_) => return Ok(None),
    };

    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let directory = parse_legacy_0x22_directory(stream, &contents)
        .context("parse legacy 0x22 Contents directory for PAGE presentation")?;
    let document = unique_entry_by_type(&directory, LEGACY_DOCUMENT_TYPE, "DOCUMENT 0x0015")?;
    let page_list = parse_u16_id_list(&contents, document, "DOCUMENT PageList")?;
    if page_list.is_empty() {
        return Ok(None);
    }
    if page_list.iter().copied().collect::<BTreeSet<_>>().len() != page_list.len() {
        return Ok(None);
    }

    let entry_types = page_list
        .iter()
        .map(|object_id| {
            directory
                .entry_by_object_id(*object_id)
                .map(|entry| entry.chunk_type)
        })
        .collect::<Option<Vec<_>>>();
    let Some(entry_types) = entry_types else {
        return Ok(None);
    };
    let physical_page_count = directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == LEGACY_PAGE_TYPE)
        .count();

    Ok(select_from_entry_types(
        dialect,
        physical_page_count,
        &entry_types,
    ))
}

fn select_from_entry_types(
    dialect: Legacy22PageListDialectV1,
    physical_page_count: usize,
    entry_types: &[u16],
) -> Option<Legacy22PageListPresentationSelectionV1> {
    let materialized_page_count = entry_types
        .iter()
        .filter(|raw_type| **raw_type == LEGACY_PAGE_TYPE)
        .count();
    if physical_page_count != materialized_page_count.checked_add(1)? {
        return None;
    }

    match dialect {
        Legacy22PageListDialectV1::NoQuill => {
            if entry_types.len() < 4
                || entry_types
                    .iter()
                    .any(|raw_type| *raw_type != LEGACY_PAGE_TYPE)
            {
                return None;
            }
            let customer_end = entry_types.len().checked_sub(1)?;
            if customer_end <= 2 {
                return None;
            }
            Some(Legacy22PageListPresentationSelectionV1 {
                profile_id: LEGACY22_NOQUILL_PAGE_PROFILE_ID_V1,
                raw_page_list_entry_count: entry_types.len(),
                materialized_page_count,
                customer_page_indices: (2..customer_end).collect(),
            })
        }
        Legacy22PageListDialectV1::Quill => {
            if entry_types.len() < 6
                || entry_types[0] != LEGACY_PAGE_TYPE
                || entry_types[1] != LEGACY_PAGE_TYPE
            {
                return None;
            }
            let tail_start = entry_types.len().checked_sub(3)?;
            if tail_start <= 2
                || entry_types[2..tail_start]
                    .iter()
                    .any(|raw_type| *raw_type != LEGACY_PAGE_TYPE)
            {
                return None;
            }
            let tail = &entry_types[tail_start..];
            let all_page_tail = tail == [LEGACY_PAGE_TYPE, LEGACY_PAGE_TYPE, LEGACY_PAGE_TYPE];
            let special_tail = tail
                == [
                    LEGACY_PAGE_TYPE,
                    LEGACY_PAGE_LIST_SPECIAL_TYPE,
                    LEGACY_PAGE_TYPE,
                ];
            if !all_page_tail && !special_tail {
                return None;
            }
            Some(Legacy22PageListPresentationSelectionV1 {
                profile_id: LEGACY22_QUILL_PAGE_PROFILE_ID_V1,
                raw_page_list_entry_count: entry_types.len(),
                materialized_page_count,
                customer_page_indices: (2..tail_start).collect(),
            })
        }
    }
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
    if max_count < count || record_size != LEGACY_LIST_U16_RECORD_SIZE {
        return Ok(Vec::new());
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
        let object_id = read_u16(contents, offset).context("legacy list object id missing")?;
        ids.push(object_id);
    }
    debug_assert_eq!(payload_end, payload_start + payload_len);
    Ok(ids)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noquill_profile_selects_only_middle_pages() {
        let selection = select_from_entry_types(
            Legacy22PageListDialectV1::NoQuill,
            5,
            &[LEGACY_PAGE_TYPE; 4],
        )
        .expect("bounded no-Quill envelope should select");
        assert_eq!(selection.customer_page_indices, vec![2]);
        assert_eq!(selection.materialized_page_count, 4);
    }

    #[test]
    fn noquill_profile_fails_open_on_physical_page_drift() {
        assert_eq!(
            select_from_entry_types(
                Legacy22PageListDialectV1::NoQuill,
                4,
                &[LEGACY_PAGE_TYPE; 4],
            ),
            None
        );
    }

    #[test]
    fn quill_profile_accepts_current_special_tail() {
        let selection = select_from_entry_types(
            Legacy22PageListDialectV1::Quill,
            6,
            &[
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_LIST_SPECIAL_TYPE,
                LEGACY_PAGE_TYPE,
            ],
        )
        .expect("bounded Quill special-tail envelope should select");
        assert_eq!(selection.customer_page_indices, vec![2]);
        assert_eq!(selection.materialized_page_count, 5);
    }

    #[test]
    fn quill_profile_accepts_historical_all_page_tail() {
        let selection = select_from_entry_types(
            Legacy22PageListDialectV1::Quill,
            8,
            &[
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
                LEGACY_PAGE_TYPE,
            ],
        )
        .expect("bounded historical Quill envelope should select");
        assert_eq!(selection.customer_page_indices, vec![2, 3]);
        assert_eq!(selection.materialized_page_count, 7);
    }

    #[test]
    fn quill_profile_rejects_unproven_tail_shape() {
        assert_eq!(
            select_from_entry_types(
                Legacy22PageListDialectV1::Quill,
                6,
                &[
                    LEGACY_PAGE_TYPE,
                    LEGACY_PAGE_TYPE,
                    LEGACY_PAGE_TYPE,
                    LEGACY_PAGE_LIST_SPECIAL_TYPE,
                    LEGACY_PAGE_TYPE,
                    LEGACY_PAGE_TYPE,
                ],
            ),
            None
        );
    }
}
