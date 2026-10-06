use anyhow::{Context, Result};
use pub_cfb::{
    RootRegularStreamPrefixStatus, RootRegularStreamSourceRange,
    RootRegularStreamTruncationReason, recover_root_regular_stream_prefix_reader,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1: &str =
    "chaptera.reader-partial-root-stream-evidence.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialRootStreamEvidence {
    pub schema_version: String,
    pub source_sha256: String,
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
    let recovered =
        recover_root_regular_stream_prefix_reader(Cursor::new(source), stream_identity)
            .with_context(|| format!("recover partial root stream evidence {stream_identity}"))?;
    let source_after = sha256_hex(source);

    Ok(ReaderPartialRootStreamEvidence {
        schema_version: READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1.to_owned(),
        source_sha256: source_before.clone(),
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
        let start_sector =
            (usize::try_from(first.offset).unwrap() / sector_len).saturating_sub(1);
        let fat_sector =
            u32::from_le_bytes([source[76], source[77], source[78], source[79]]);
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
