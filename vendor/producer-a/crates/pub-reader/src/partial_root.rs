use anyhow::{Context, Result};
use pub_cfb::{
    RootRegularStreamPrefixStatus, RootRegularStreamSourceRange, RootRegularStreamTruncationReason,
    recover_root_regular_stream_prefix_reader,
};
use pub_contents::{
    Contents0x2cDirectorySlot, ContentsFamily, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1: &str =
    "chaptera.reader-partial-root-stream-evidence.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialRootStreamEvidence {
    pub schema_version: String,
    pub source_sha256: String,
    pub stream_sid: u32,
    pub stream_identity: String,
    pub declared_len: u64,
    pub available_prefix_len: u64,
    pub prefix_sha256: String,
    pub status: RootRegularStreamPrefixStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<RootRegularStreamTruncationReason>,
    pub source_ranges: Vec<RootRegularStreamSourceRange>,
    #[serde(skip_serializing)]
    pub prefix_bytes: Vec<u8>,
    pub source_modified: bool,
}

/// Build immutable, source-bound evidence for one direct-root regular stream
/// prefix. The returned serialized form is source-safe: it includes hashes,
/// lengths, ranges and the truncation reason, but not the recovered bytes.
pub fn build_reader_partial_root_stream_evidence(
    source: &[u8],
    stream_identity: &str,
) -> Result<ReaderPartialRootStreamEvidence> {
    let source_before = sha256_hex(source);
    let recovered = recover_root_regular_stream_prefix_reader(Cursor::new(source), stream_identity)
        .with_context(|| format!("recover partial root stream evidence {stream_identity}"))?;
    let source_after = sha256_hex(source);

    Ok(ReaderPartialRootStreamEvidence {
        schema_version: READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1.to_owned(),
        source_sha256: source_before.clone(),
        stream_sid: recovered.stream_sid,
        stream_identity: stream_identity.to_owned(),
        declared_len: recovered.declared_len,
        available_prefix_len: recovered.available_prefix_len,
        prefix_sha256: sha256_hex(&recovered.bytes),
        status: recovered.status,
        truncation_reason: recovered.truncation_reason,
        source_ranges: recovered.source_ranges,
        prefix_bytes: recovered.bytes,
        source_modified: source_before != source_after,
    })
}

pub const READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1: &str =
    "chaptera.reader-partial-contents-semantic-evidence.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderPartialContentsClass {
    UsefulSemanticPrefix,
    ForensicOnly,
    NoSafeFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderPartialContentsBoundary {
    EvidenceInvalid,
    FamilyUnrecognized,
    Legacy22SeparateGrammarRequired,
    HeaderOnlyTrailerUnavailable,
    TrailerIncomplete,
    DirectoryComplete,
    DirectoryWithCompleteReferencedChunks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialContentsChunkFact {
    pub seq_num: usize,
    pub raw_type: u16,
    pub chunk_offset: u32,
    pub chunk_declared_len: u32,
    pub fully_decoded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_seq_num: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialContentsSemanticEvidence {
    pub schema_version: String,
    pub source_sha256: String,
    pub stream_sid: u32,
    pub prefix_sha256: String,
    pub available_prefix_len: u64,
    pub declared_len: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serialization_revision: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailer_offset: Option<u32>,
    pub boundary: ReaderPartialContentsBoundary,
    pub class: ReaderPartialContentsClass,
    pub complete_chunk_facts: Vec<ReaderPartialContentsChunkFact>,
    pub ambiguous_reference_count: usize,
    pub referenced_chunk_unavailable_count: usize,
    pub chunk_parse_failure_count: usize,
    pub chunk_crosses_trailer_count: usize,
    pub missing_tail_len: u64,
}

/// Classify only semantic facts that are fully contained in a physically
/// proven /Contents prefix. No string carving, padding, sibling inference or
/// ownership/layout inference is performed.
pub fn analyze_reader_partial_contents_prefix(
    evidence: &ReaderPartialRootStreamEvidence,
) -> ReaderPartialContentsSemanticEvidence {
    let prefix = evidence.prefix_bytes.as_slice();
    let missing_tail_len = evidence
        .declared_len
        .saturating_sub(evidence.available_prefix_len);

    if !partial_contents_evidence_is_consistent(evidence) {
        return ReaderPartialContentsSemanticEvidence {
            schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
            source_sha256: evidence.source_sha256.clone(),
            stream_sid: evidence.stream_sid,
            prefix_sha256: evidence.prefix_sha256.clone(),
            available_prefix_len: evidence.available_prefix_len,
            declared_len: evidence.declared_len,
            family: None,
            serialization_revision: None,
            trailer_offset: None,
            boundary: ReaderPartialContentsBoundary::EvidenceInvalid,
            class: ReaderPartialContentsClass::NoSafeFact,
            complete_chunk_facts: Vec::new(),
            ambiguous_reference_count: 0,
            referenced_chunk_unavailable_count: 0,
            chunk_parse_failure_count: 0,
            chunk_crosses_trailer_count: 0,
            missing_tail_len,
        };
    }

    let family = match pub_contents::detect_family(prefix) {
        Ok(family) => family,
        Err(_) => {
            return ReaderPartialContentsSemanticEvidence {
                schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
                source_sha256: evidence.source_sha256.clone(),
                stream_sid: evidence.stream_sid,
                prefix_sha256: evidence.prefix_sha256.clone(),
                available_prefix_len: evidence.available_prefix_len,
                declared_len: evidence.declared_len,
                family: None,
                serialization_revision: None,
                trailer_offset: None,
                boundary: ReaderPartialContentsBoundary::FamilyUnrecognized,
                class: ReaderPartialContentsClass::NoSafeFact,
                complete_chunk_facts: Vec::new(),
                ambiguous_reference_count: 0,
                referenced_chunk_unavailable_count: 0,
                chunk_parse_failure_count: 0,
                chunk_crosses_trailer_count: 0,
                missing_tail_len,
            };
        }
    };

    let family_name = match family {
        ContentsFamily::Family0x22 => "0x22",
        ContentsFamily::Family0x2c => "0x2c",
    }
    .to_owned();

    let preamble = match pub_contents::parse_preamble(StreamPath("/Contents".into()), prefix) {
        Ok(preamble) => preamble,
        Err(_) => {
            return ReaderPartialContentsSemanticEvidence {
                schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
                source_sha256: evidence.source_sha256.clone(),
                stream_sid: evidence.stream_sid,
                prefix_sha256: evidence.prefix_sha256.clone(),
                available_prefix_len: evidence.available_prefix_len,
                declared_len: evidence.declared_len,
                family: Some(family_name),
                serialization_revision: None,
                trailer_offset: None,
                boundary: ReaderPartialContentsBoundary::FamilyUnrecognized,
                class: ReaderPartialContentsClass::NoSafeFact,
                complete_chunk_facts: Vec::new(),
                ambiguous_reference_count: 0,
                referenced_chunk_unavailable_count: 0,
                chunk_parse_failure_count: 0,
                chunk_crosses_trailer_count: 0,
                missing_tail_len,
            };
        }
    };

    if family == ContentsFamily::Family0x22 {
        return ReaderPartialContentsSemanticEvidence {
            schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
            source_sha256: evidence.source_sha256.clone(),
            stream_sid: evidence.stream_sid,
            prefix_sha256: evidence.prefix_sha256.clone(),
            available_prefix_len: evidence.available_prefix_len,
            declared_len: evidence.declared_len,
            family: Some(family_name),
            serialization_revision: Some(preamble.serialization_revision),
            trailer_offset: None,
            boundary: ReaderPartialContentsBoundary::Legacy22SeparateGrammarRequired,
            class: ReaderPartialContentsClass::ForensicOnly,
            complete_chunk_facts: Vec::new(),
            ambiguous_reference_count: 0,
            referenced_chunk_unavailable_count: 0,
            chunk_parse_failure_count: 0,
            chunk_crosses_trailer_count: 0,
            missing_tail_len,
        };
    }

    let header_prefix =
        match pub_contents::parse_0x2c_header_prefix(StreamPath("/Contents".into()), prefix) {
            Ok(header) => header,
            Err(_) => {
                return ReaderPartialContentsSemanticEvidence {
                    schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
                    source_sha256: evidence.source_sha256.clone(),
                    stream_sid: evidence.stream_sid,
                    prefix_sha256: evidence.prefix_sha256.clone(),
                    available_prefix_len: evidence.available_prefix_len,
                    declared_len: evidence.declared_len,
                    family: Some(family_name),
                    serialization_revision: Some(preamble.serialization_revision),
                    trailer_offset: None,
                    boundary: ReaderPartialContentsBoundary::HeaderOnlyTrailerUnavailable,
                    class: ReaderPartialContentsClass::ForensicOnly,
                    complete_chunk_facts: Vec::new(),
                    ambiguous_reference_count: 0,
                    referenced_chunk_unavailable_count: 0,
                    chunk_parse_failure_count: 0,
                    chunk_crosses_trailer_count: 0,
                    missing_tail_len,
                };
            }
        };

    let trailer_offset = header_prefix.trailer_offset;
    let Ok(header) = parse_0x2c_header(StreamPath("/Contents".into()), prefix) else {
        return ReaderPartialContentsSemanticEvidence {
            schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
            source_sha256: evidence.source_sha256.clone(),
            stream_sid: evidence.stream_sid,
            prefix_sha256: evidence.prefix_sha256.clone(),
            available_prefix_len: evidence.available_prefix_len,
            declared_len: evidence.declared_len,
            family: Some(family_name),
            serialization_revision: Some(preamble.serialization_revision),
            trailer_offset: Some(trailer_offset),
            boundary: ReaderPartialContentsBoundary::HeaderOnlyTrailerUnavailable,
            class: ReaderPartialContentsClass::ForensicOnly,
            complete_chunk_facts: Vec::new(),
            ambiguous_reference_count: 0,
            referenced_chunk_unavailable_count: 0,
            chunk_parse_failure_count: 0,
            chunk_crosses_trailer_count: 0,
            missing_tail_len,
        };
    };

    let Ok(trailer) = parse_confirmed_0x2c_trailer_root(prefix, &header) else {
        return ReaderPartialContentsSemanticEvidence {
            schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
            source_sha256: evidence.source_sha256.clone(),
            stream_sid: evidence.stream_sid,
            prefix_sha256: evidence.prefix_sha256.clone(),
            available_prefix_len: evidence.available_prefix_len,
            declared_len: evidence.declared_len,
            family: Some(family_name),
            serialization_revision: Some(preamble.serialization_revision),
            trailer_offset: Some(trailer_offset),
            boundary: ReaderPartialContentsBoundary::TrailerIncomplete,
            class: ReaderPartialContentsClass::ForensicOnly,
            complete_chunk_facts: Vec::new(),
            ambiguous_reference_count: 0,
            referenced_chunk_unavailable_count: 0,
            chunk_parse_failure_count: 0,
            chunk_crosses_trailer_count: 0,
            missing_tail_len,
        };
    };

    let mut complete_chunk_facts = Vec::new();
    let mut ambiguous_reference_count = 0usize;
    let mut referenced_chunk_unavailable_count = 0usize;
    let mut chunk_parse_failure_count = 0usize;
    let mut chunk_crosses_trailer_count = 0usize;

    for seq_num in 0..trailer.directory.slots.len() {
        if matches!(
            trailer.directory.slot(seq_num),
            Some(Contents0x2cDirectorySlot::Empty { .. })
        ) {
            continue;
        }

        let reference = match parse_confirmed_chunk_reference(prefix, &trailer.directory, seq_num) {
            Ok(Some(reference)) => reference,
            Ok(None) => continue,
            Err(_) => {
                ambiguous_reference_count += 1;
                continue;
            }
        };

        if reference.raw_types.len() != 1 || reference.chunk_offsets.len() != 1 {
            ambiguous_reference_count += 1;
            continue;
        }

        let raw_type = reference.raw_types[0].value;
        let chunk_offset = reference.chunk_offsets[0].value;
        if chunk_offset >= trailer_offset {
            chunk_crosses_trailer_count += 1;
            continue;
        }
        let chunk = match parse_confirmed_0x2c_chunk(
            StreamPath("/Contents".into()),
            prefix,
            chunk_offset,
        ) {
            Ok(chunk) => chunk,
            Err(pub_contents::ChunkReadError::DeclaredRangeOutOfBounds { .. }) => {
                referenced_chunk_unavailable_count += 1;
                continue;
            }
            Err(_) => {
                chunk_parse_failure_count += 1;
                continue;
            }
        };

        let chunk_end = u64::from(chunk_offset).saturating_add(u64::from(chunk.declared_length));
        if chunk_end > u64::from(trailer_offset) {
            chunk_crosses_trailer_count += 1;
            continue;
        }

        let parent_seq_num = if reference.parent_seq_nums.len() == 1 {
            Some(reference.parent_seq_nums[0].value)
        } else {
            None
        };
        complete_chunk_facts.push(ReaderPartialContentsChunkFact {
            seq_num,
            raw_type,
            chunk_offset,
            chunk_declared_len: chunk.declared_length,
            fully_decoded: chunk.is_fully_decoded(),
            parent_seq_num,
        });
    }

    let useful = !complete_chunk_facts.is_empty();
    ReaderPartialContentsSemanticEvidence {
        schema_version: READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1.to_owned(),
        source_sha256: evidence.source_sha256.clone(),
        stream_sid: evidence.stream_sid,
        prefix_sha256: evidence.prefix_sha256.clone(),
        available_prefix_len: evidence.available_prefix_len,
        declared_len: evidence.declared_len,
        family: Some(family_name),
        serialization_revision: Some(preamble.serialization_revision),
        trailer_offset: Some(trailer_offset),
        boundary: if useful {
            ReaderPartialContentsBoundary::DirectoryWithCompleteReferencedChunks
        } else {
            ReaderPartialContentsBoundary::DirectoryComplete
        },
        class: if useful {
            ReaderPartialContentsClass::UsefulSemanticPrefix
        } else {
            ReaderPartialContentsClass::ForensicOnly
        },
        complete_chunk_facts,
        ambiguous_reference_count,
        referenced_chunk_unavailable_count,
        chunk_parse_failure_count,
        chunk_crosses_trailer_count,
        missing_tail_len,
    }
}

fn partial_contents_evidence_is_consistent(evidence: &ReaderPartialRootStreamEvidence) -> bool {
    if evidence.stream_identity != "/Contents"
        || evidence.stream_sid == 0
        || evidence.source_modified
        || evidence.status != RootRegularStreamPrefixStatus::Partial
        || evidence.available_prefix_len > evidence.declared_len
        || usize::try_from(evidence.available_prefix_len).ok() != Some(evidence.prefix_bytes.len())
        || sha256_hex(&evidence.prefix_bytes) != evidence.prefix_sha256
        || evidence.source_ranges.is_empty()
    {
        return false;
    }

    let mut ranges = evidence.source_ranges.clone();
    ranges.sort_by_key(|range| range.offset);
    let mut total = 0u64;
    let mut previous_end = None;
    for range in ranges {
        if range.len == 0 {
            return false;
        }
        let Some(end) = range.offset.checked_add(range.len) else {
            return false;
        };
        if previous_end.is_some_and(|previous| range.offset < previous) {
            return false;
        }
        let Some(next_total) = total.checked_add(range.len) else {
            return false;
        };
        total = next_total;
        previous_end = Some(end);
    }

    total == evidence.available_prefix_len
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const FREE_SECTOR: u32 = 0xffff_ffff;

    fn regular_root_fixture() -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("partial reader fixture");
        compound
            .create_stream("/Contents")
            .expect("root Contents")
            .write_all(&vec![0x42; 9_000])
            .expect("write Contents");
        compound.flush().expect("flush fixture");
        compound.into_inner().into_inner()
    }

    fn break_contents_chain_after_first_sector(mut source: Vec<u8>) -> Vec<u8> {
        let evidence =
            build_reader_partial_root_stream_evidence(&source, "/Contents").expect("full evidence");
        let first = evidence.source_ranges.first().expect("first source range");
        let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
        let start_sector = (usize::try_from(first.offset).unwrap() / sector_len).saturating_sub(1);
        let fat_sector = u32::from_le_bytes([source[76], source[77], source[78], source[79]]);
        assert_ne!(fat_sector, FREE_SECTOR);
        let fat_offset = (usize::try_from(fat_sector).unwrap() + 1) * sector_len;
        let entry_offset = fat_offset + start_sector * 4;
        source[entry_offset..entry_offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        source
    }

    #[test]
    fn evidence_binds_source_prefix_and_stream_identity() {
        let source = regular_root_fixture();
        let evidence =
            build_reader_partial_root_stream_evidence(&source, "/Contents").expect("evidence");

        assert_eq!(
            evidence.schema_version,
            READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1
        );
        assert_eq!(evidence.stream_identity, "/Contents");
        assert!(evidence.stream_sid > 0);
        assert_eq!(evidence.status, RootRegularStreamPrefixStatus::Complete);
        assert_eq!(evidence.available_prefix_len, evidence.declared_len);
        assert_eq!(evidence.prefix_bytes.len(), 9_000);
        assert!(!evidence.source_modified);
        assert_eq!(evidence.source_sha256, sha256_hex(&source));
        assert_eq!(evidence.prefix_sha256, sha256_hex(&evidence.prefix_bytes));
    }

    #[test]
    fn partial_evidence_keeps_missing_tail_explicit() {
        let source = break_contents_chain_after_first_sector(regular_root_fixture());
        let evidence =
            build_reader_partial_root_stream_evidence(&source, "/Contents").expect("partial");

        assert_eq!(evidence.status, RootRegularStreamPrefixStatus::Partial);
        assert_eq!(
            evidence.truncation_reason,
            Some(RootRegularStreamTruncationReason::InvalidNextSector)
        );
        assert!(evidence.available_prefix_len < evidence.declared_len);
        assert_eq!(
            usize::try_from(evidence.available_prefix_len).unwrap(),
            evidence.prefix_bytes.len()
        );
        assert!(!evidence.source_modified);
    }

    fn partial_evidence_for_prefix(
        prefix: Vec<u8>,
        declared_len: u64,
    ) -> ReaderPartialRootStreamEvidence {
        ReaderPartialRootStreamEvidence {
            schema_version: READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1.to_owned(),
            source_sha256: "11".repeat(32),
            stream_sid: 1,
            stream_identity: "/Contents".to_owned(),
            declared_len,
            available_prefix_len: prefix.len() as u64,
            prefix_sha256: sha256_hex(&prefix),
            status: RootRegularStreamPrefixStatus::Partial,
            truncation_reason: Some(RootRegularStreamTruncationReason::UnexpectedEndOfChain),
            source_ranges: vec![RootRegularStreamSourceRange {
                offset: 512,
                len: prefix.len() as u64,
            }],
            prefix_bytes: prefix,
            source_modified: false,
        }
    }

    fn mature_prefix_without_trailer() -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(&pub_contents::CONTENTS_0X2C_MAGIC);
        bytes[12..14].copy_from_slice(&0x0015u16.to_le_bytes());
        bytes[0x1A..0x1E].copy_from_slice(&512u32.to_le_bytes());
        bytes
    }

    fn mature_prefix_with_one_complete_chunk() -> Vec<u8> {
        let mut bytes = vec![0u8; 256];
        bytes[0..4].copy_from_slice(&pub_contents::CONTENTS_0X2C_MAGIC);
        bytes[12..14].copy_from_slice(&0x0015u16.to_le_bytes());
        bytes[0x1A..0x1E].copy_from_slice(&128u32.to_le_bytes());

        let chunk = [0x0A, 0x00, 0x00, 0x00, 0x27, 0x20, 0x16, 0x00, 0x00, 0x00];
        bytes[64..64 + chunk.len()].copy_from_slice(&chunk);

        let directory = [
            0x00, 0x88, 0x0E, 0x00, 0x00, 0x00, 0x02, 0x18, 0x01, 0x00, 0x04, 0xB8, 0x40, 0x00,
            0x00, 0x00,
        ];
        let trailer_len = 4 + 6 + 6 + 6 + directory.len();
        let mut cursor = 128usize;
        bytes[cursor..cursor + 4].copy_from_slice(&(trailer_len as u32).to_le_bytes());
        cursor += 4;
        bytes[cursor..cursor + 6].copy_from_slice(&[0x01, 0x20, 0x01, 0x00, 0x00, 0x00]);
        cursor += 6;
        bytes[cursor..cursor + 6].copy_from_slice(&[0x02, 0x20, 0x00, 0x00, 0x00, 0x00]);
        cursor += 6;
        let directory_declared_len = 4 + directory.len();
        bytes[cursor..cursor + 2].copy_from_slice(&[0x03, 0x90]);
        bytes[cursor + 2..cursor + 6]
            .copy_from_slice(&(directory_declared_len as u32).to_le_bytes());
        cursor += 6;
        bytes[cursor..cursor + directory.len()].copy_from_slice(&directory);
        bytes
    }

    #[test]
    fn forged_partial_evidence_fails_closed() {
        let prefix = mature_prefix_without_trailer();
        let mut evidence = partial_evidence_for_prefix(prefix, 1024);
        evidence.prefix_sha256 = "00".repeat(32);

        let semantic = analyze_reader_partial_contents_prefix(&evidence);
        assert_eq!(
            semantic.boundary,
            ReaderPartialContentsBoundary::EvidenceInvalid
        );
        assert_eq!(semantic.class, ReaderPartialContentsClass::NoSafeFact);
        assert!(semantic.complete_chunk_facts.is_empty());
    }

    #[test]
    fn wrong_stream_identity_fails_closed() {
        let prefix = mature_prefix_without_trailer();
        let mut evidence = partial_evidence_for_prefix(prefix, 1024);
        evidence.stream_identity = "/NotContents".to_owned();

        let semantic = analyze_reader_partial_contents_prefix(&evidence);
        assert_eq!(
            semantic.boundary,
            ReaderPartialContentsBoundary::EvidenceInvalid
        );
        assert_eq!(semantic.class, ReaderPartialContentsClass::NoSafeFact);
    }

    #[test]
    fn prefix_without_trailer_is_forensic_only() {
        let prefix = mature_prefix_without_trailer();
        let evidence = partial_evidence_for_prefix(prefix, 1024);
        let semantic = analyze_reader_partial_contents_prefix(&evidence);

        assert_eq!(semantic.family.as_deref(), Some("0x2c"));
        assert_eq!(
            semantic.boundary,
            ReaderPartialContentsBoundary::HeaderOnlyTrailerUnavailable
        );
        assert_eq!(semantic.class, ReaderPartialContentsClass::ForensicOnly);
        assert!(semantic.complete_chunk_facts.is_empty());
    }

    #[test]
    fn complete_directory_and_referenced_chunk_make_prefix_useful() {
        let prefix = mature_prefix_with_one_complete_chunk();
        let evidence = partial_evidence_for_prefix(prefix, 512);
        let semantic = analyze_reader_partial_contents_prefix(&evidence);

        assert_eq!(
            semantic.boundary,
            ReaderPartialContentsBoundary::DirectoryWithCompleteReferencedChunks
        );
        assert_eq!(
            semantic.class,
            ReaderPartialContentsClass::UsefulSemanticPrefix
        );
        assert_eq!(semantic.complete_chunk_facts.len(), 1);
        assert_eq!(semantic.complete_chunk_facts[0].seq_num, 0);
        assert_eq!(semantic.complete_chunk_facts[0].raw_type, 0x01);
        assert_eq!(semantic.complete_chunk_facts[0].chunk_offset, 64);
        assert_eq!(semantic.complete_chunk_facts[0].chunk_declared_len, 10);
    }

    #[test]
    fn referenced_chunk_that_crosses_trailer_is_not_promoted() {
        let mut prefix = mature_prefix_with_one_complete_chunk();
        prefix[64..68].copy_from_slice(&80u32.to_le_bytes());
        let evidence = partial_evidence_for_prefix(prefix, 512);
        let semantic = analyze_reader_partial_contents_prefix(&evidence);

        assert_eq!(semantic.class, ReaderPartialContentsClass::ForensicOnly);
        assert!(semantic.complete_chunk_facts.is_empty());
        assert_eq!(semantic.chunk_crosses_trailer_count, 1);
    }

    #[test]
    fn old_family_stays_on_separate_partial_grammar_path() {
        let mut prefix = vec![0u8; 128];
        prefix[0..4].copy_from_slice(&pub_contents::CONTENTS_0X22_MAGIC);
        prefix[12..14].copy_from_slice(&0x02CDu16.to_le_bytes());
        let evidence = partial_evidence_for_prefix(prefix, 512);
        let semantic = analyze_reader_partial_contents_prefix(&evidence);

        assert_eq!(semantic.family.as_deref(), Some("0x22"));
        assert_eq!(
            semantic.boundary,
            ReaderPartialContentsBoundary::Legacy22SeparateGrammarRequired
        );
        assert_eq!(semantic.class, ReaderPartialContentsClass::ForensicOnly);
    }

    #[test]
    fn serialized_receipt_does_not_emit_prefix_bytes() {
        let source = break_contents_chain_after_first_sector(regular_root_fixture());
        let evidence =
            build_reader_partial_root_stream_evidence(&source, "/Contents").expect("partial");
        let value = serde_json::to_value(&evidence).expect("serialize evidence");

        assert!(value.get("prefix_bytes").is_none());
        assert_eq!(
            value.get("prefix_sha256").and_then(|v| v.as_str()),
            Some(evidence.prefix_sha256.as_str())
        );
        assert_eq!(
            value.get("available_prefix_len").and_then(|v| v.as_u64()),
            Some(evidence.available_prefix_len)
        );
    }
}
