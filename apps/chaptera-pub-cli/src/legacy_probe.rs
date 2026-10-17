//! Read-only, source-bound diagnostic for old Publisher Story admission.
//!
//! This probe composes *existing* pub-reader graphs; it is not a second
//! Contents, CFB or Quill decoder. No story payload or source path is emitted.
use anyhow::{Result, ensure};
use chaptera_suite_handoff::AdmittedSource;
use pub_model::{AuthorityClass, ReadConfidence, Sha256Digest, SourceRole};
use pub_reader::{PubSourceGraph, PubSourceGraphBuild};
use serde::Serialize;
use std::io::Cursor;
use std::path::Path;

const SCHEMA: &str = "chaptera.rescue-legacy-story-probe.v1";
const MAX_STORIES: usize = 16_384;
const MAX_TOTAL_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct LegacyProbeReceipt {
    schema_version: &'static str,
    source_sha256: String,
    source_byte_len: usize,
    family: &'static str,
    status: &'static str,
    confirmed_story_ranges: usize,
    ambiguous_story_ranges: usize,
    graph_diagnostic_count: usize,
    source_immutable: bool,
    native_pub_repair_attempted: bool,
    semantic_text_exposed: bool,
}

fn safe_story_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 96
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b':' | b'_'))
}

fn count_proven(graph: &PubSourceGraph) -> (usize, usize) {
    let mut proven = 0usize;
    let mut ambiguous = 0usize;
    let mut total_text_bytes = 0usize;
    for story in graph.stories.values() {
        if story.text.is_empty() {
            continue;
        }
        let text_bytes = story.text.len();
        total_text_bytes = total_text_bytes.saturating_add(text_bytes);
        let key = story.id.as_canonical();
        let length_ok = story.text.encode_utf16().count() <= u32::MAX as usize;
        let ref_ok = story.source_refs.iter().any(|source_ref| {
            source_ref.role == SourceRole::Semantic
                && source_ref.authority == AuthorityClass::Authoritative
                && source_ref.confidence == Some(ReadConfidence::Exact)
                && source_ref.validate_primary_source(&graph.source).is_ok()
                && source_ref
                    .byte_range
                    .is_some_and(|span| span.length > 0 && span.end().is_some())
        });
        if safe_story_key(key)
            && length_ok
            && total_text_bytes <= MAX_TOTAL_TEXT_BYTES
            && proven + ambiguous < MAX_STORIES
            && ref_ok
        {
            proven += 1;
        } else {
            ambiguous += 1;
        }
    }
    (proven, ambiguous)
}

fn receipt(
    sha: Sha256Digest,
    byte_len: usize,
    family: &'static str,
    build: Option<PubSourceGraphBuild>,
) -> LegacyProbeReceipt {
    let (proven, ambiguous, diagnostics) = if let Some(build) = build {
        let (a, b) = count_proven(&build.graph);
        (a, b, build.diagnostics.len().min(MAX_STORIES))
    } else {
        (0, 0, 0)
    };
    let status = if proven > 0 {
        if ambiguous > 0 {
            "verified_partial"
        } else {
            "verified"
        }
    } else if ambiguous > 0 {
        "ambiguous"
    } else {
        "no_proven_story"
    };
    LegacyProbeReceipt {
        schema_version: SCHEMA,
        source_sha256: sha.to_string(),
        source_byte_len: byte_len,
        family,
        status: if family == "unclassified" {
            "ineligible"
        } else {
            status
        },
        confirmed_story_ranges: proven,
        ambiguous_story_ranges: ambiguous,
        graph_diagnostic_count: diagnostics,
        source_immutable: true,
        native_pub_repair_attempted: false,
        semantic_text_exposed: false,
    }
}

pub(crate) fn probe(file: &Path) -> Result<LegacyProbeReceipt> {
    let admitted = AdmittedSource::open(file)
        .map_err(|_| anyhow::anyhow!("legacy_probe_source_unavailable"))?;
    let bytes = admitted.bytes();
    let sha = super::source_sha256(bytes);
    ensure!(
        sha.to_string() == admitted.sha256(),
        "source_snapshot_sha_mismatch"
    );
    let len = bytes.len();
    // Mature acceptance is authoritative. Never reinterpret an admitted
    // mature file as an older family just to obtain another Story count.
    if let Ok(build) = pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes), sha) {
        return Ok(receipt(sha, len, "mature_0x2c", Some(build)));
    }
    // The Quill-bearing old family takes precedence over the Contents-only
    // path. Each builder has its own strict historical version grammar.
    if let Ok(build) = pub_reader::build_legacy_0x22_quill_source_graph(Cursor::new(bytes), sha) {
        return Ok(receipt(sha, len, "legacy_0x22_quill", Some(build)));
    }
    if let Ok(build) = pub_reader::build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), sha) {
        return Ok(receipt(sha, len, "legacy_0x22_noquill", Some(build)));
    }
    Ok(receipt(sha, len, "unclassified", None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_story_key_is_bounded_and_filename_inert() {
        assert!(safe_story_key("legacy-noquill:01:02"));
        assert!(!safe_story_key("../source.pub"));
        assert!(!safe_story_key("private name"));
        assert!(!safe_story_key(&"a".repeat(97)));
    }

    #[test]
    fn no_admission_does_not_claim_text() {
        let sha = super::super::source_sha256(b"synthetic-negative");
        let value = receipt(sha, 18, "unclassified", None);
        assert_eq!(value.status, "ineligible");
        assert_eq!(value.confirmed_story_ranges, 0);
        assert!(!value.semantic_text_exposed);
    }
}
