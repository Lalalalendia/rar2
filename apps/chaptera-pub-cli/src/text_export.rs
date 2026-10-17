//! Source-bound text evidence export. This is not a repaired Publisher file.
//! The canonical reader remains the sole Quill/FDPP decoder.

use anyhow::{Context, Result, ensure};
use chaptera_suite_handoff::AdmittedSource;
use pub_model::{AuthorityClass, ReadConfidence, Sha256Digest, SourceRole};
use pub_reader::{PubSourceGraph, ReaderPartialSourceFact};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = "chaptera.rescue-story-evidence.v1";
const MAX_RECORDS: usize = 16_384;
const MAX_TOTAL_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 24 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
struct StoryEvidence {
    story_key: String,
    utf16_start: u32,
    utf16_end: u32,
    text: String,
    utf8_byte_len: usize,
    text_sha256: String,
    evidence_class: &'static str,
    carrier: Option<String>,
    carrier_byte_offset: Option<u64>,
    carrier_byte_len: Option<u64>,
}

#[derive(Debug, Serialize)]
struct TextEvidence {
    schema_version: &'static str,
    source_sha256: String,
    extraction_class: &'static str,
    source_immutable: bool,
    native_pub_repair_attempted: bool,
    native_pub_delivery_allowed: bool,
    page_geometry_reconstructed: bool,
    story_count: usize,
    stories_txt_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    unadmitted_story_count: Option<usize>,
    stories: Vec<StoryEvidence>,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn add_record(
    records: &mut Vec<StoryEvidence>,
    total_text: &mut usize,
    record: StoryEvidence,
) -> Result<()> {
    ensure!(!record.text.is_empty(), "empty_story_not_promoted");
    ensure!(
        !record.story_key.is_empty()
            && record.story_key.len() <= 96
            && record
                .story_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b':' | b'_')),
        "unsafe_story_key"
    );
    ensure!(records.len() < MAX_RECORDS, "story_count_limit");
    let utf16_len =
        u32::try_from(record.text.encode_utf16().count()).context("text_utf16_limit")?;
    ensure!(
        record.utf16_start.checked_add(utf16_len) == Some(record.utf16_end),
        "unproven_utf16_story_range"
    );
    ensure!(
        record.utf8_byte_len == record.text.len()
            && record.text_sha256 == sha256(record.text.as_bytes()),
        "story_payload_digest_mismatch"
    );
    *total_text = total_text
        .checked_add(record.text.len())
        .context("text_length_overflow")?;
    ensure!(*total_text <= MAX_TOTAL_TEXT_BYTES, "text_total_limit");
    if let (Some(offset), Some(len)) = (record.carrier_byte_offset, record.carrier_byte_len) {
        ensure!(
            offset.checked_add(len).is_some(),
            "invalid_source_carrier_range"
        );
    }
    records.push(record);
    Ok(())
}

fn mature_story_records(
    graph: &PubSourceGraph,
    expected_sha: Sha256Digest,
) -> Result<Vec<StoryEvidence>> {
    ensure!(
        graph.source.source_hash == expected_sha,
        "mature_source_sha_mismatch"
    );
    let mut records = Vec::new();
    let mut total = 0;
    for story in graph.stories.values() {
        if story.text.is_empty() {
            continue;
        }
        let exact_text = story
            .source_refs
            .iter()
            .find(|source_ref| {
                source_ref.role == SourceRole::Semantic
                    && source_ref.authority == AuthorityClass::Authoritative
                    && source_ref.confidence == Some(ReadConfidence::Exact)
                    && source_ref.validate_primary_source(&graph.source).is_ok()
                    && source_ref.byte_range.is_some()
            })
            .context("mature_story_missing_exact_semantic_provenance")?;
        let span = exact_text
            .byte_range
            .context("mature_story_missing_carrier_range")?;
        ensure!(
            span.length > 0 && span.end().is_some(),
            "mature_story_carrier_range_invalid"
        );
        let end = u32::try_from(story.text.encode_utf16().count()).context("mature_utf16_limit")?;
        add_record(
            &mut records,
            &mut total,
            StoryEvidence {
                story_key: story.id.as_canonical().to_string(),
                utf16_start: 0,
                utf16_end: end,
                utf8_byte_len: story.text.len(),
                text_sha256: sha256(story.text.as_bytes()),
                text: story.text.clone(),
                evidence_class: "mature_authoritative_exact",
                carrier: Some(exact_text.carrier.clone()),
                carrier_byte_offset: Some(span.offset),
                carrier_byte_len: Some(span.length),
            },
        )?;
    }
    Ok(records)
}

fn legacy_story_records(
    graph: &PubSourceGraph,
    expected_sha: Sha256Digest,
    evidence_class: &'static str,
) -> Result<(Vec<StoryEvidence>, usize)> {
    ensure!(
        graph.source.source_hash == expected_sha,
        "legacy_source_sha_mismatch"
    );
    let mut records = Vec::new();
    let mut accepted_bytes = 0usize;
    let mut observed_bytes = 0usize;
    let mut observed_stories = 0usize;
    let mut omitted = 0usize;
    for story in graph.stories.values() {
        if story.text.is_empty() {
            continue;
        }
        observed_bytes = observed_bytes.saturating_add(story.text.len());
        let within_limits = observed_stories < MAX_RECORDS
            && observed_bytes <= MAX_TOTAL_TEXT_BYTES;
        observed_stories += 1;
        let exact_text = story.source_refs.iter().find(|source_ref| {
            source_ref.role == SourceRole::Semantic
                && source_ref.authority == AuthorityClass::Authoritative
                && source_ref.confidence == Some(ReadConfidence::Exact)
                && source_ref.validate_primary_source(&graph.source).is_ok()
                && source_ref
                    .byte_range
                    .is_some_and(|span| span.length > 0 && span.end().is_some())
        });
        let (Some(exact_text), Ok(end)) = (
            exact_text,
            u32::try_from(story.text.encode_utf16().count()),
        ) else {
            omitted += 1;
            continue;
        };
        if !within_limits {
            omitted += 1;
            continue;
        }
        let Some(span) = exact_text.byte_range else {
            omitted += 1;
            continue;
        };
        let record = StoryEvidence {
            story_key: story.id.as_canonical().to_string(),
            utf16_start: 0,
            utf16_end: end,
            utf8_byte_len: story.text.len(),
            text_sha256: sha256(story.text.as_bytes()),
            text: story.text.clone(),
            evidence_class,
            carrier: Some(exact_text.carrier.clone()),
            carrier_byte_offset: Some(span.offset),
            carrier_byte_len: Some(span.length),
        };
        // An ambiguous/overlarge record is omitted, never converted to a
        // fabricated Story. The private manifest records this partial gap.
        if add_record(&mut records, &mut accepted_bytes, record).is_err() {
            omitted += 1;
        }
    }
    Ok((records, omitted))
}

fn salvage_story_records(bytes: &[u8]) -> Result<Vec<StoryEvidence>> {
    let probe = pub_reader::probe_reader_salvage_candidate(bytes);
    ensure!(
        probe.eligibility.is_eligible(),
        "salvage_source_not_eligible"
    );
    let graph = pub_reader::build_reader_partial_source_graph(bytes, &probe)
        .map_err(|error| anyhow::anyhow!("salvage_graph_not_admitted: {error}"))?;
    ensure!(
        graph.source_sha256 == sha256(bytes),
        "salvage_source_sha_mismatch"
    );
    let mut records = Vec::new();
    let mut total = 0;
    for fact in graph.facts {
        if let ReaderPartialSourceFact::TextRange {
            story_key,
            utf16_start,
            utf16_end,
            text,
        } = fact
        {
            if text.is_empty() {
                continue;
            }
            add_record(
                &mut records,
                &mut total,
                StoryEvidence {
                    story_key,
                    utf16_start,
                    utf16_end,
                    utf8_byte_len: text.len(),
                    text_sha256: sha256(text.as_bytes()),
                    text,
                    evidence_class: "salvage_confirmed_quill_text_range",
                    carrier: None,
                    carrier_byte_offset: None,
                    carrier_byte_len: None,
                },
            )?;
        }
    }
    Ok(records)
}

fn select_story_records(
    bytes: &[u8],
    source_sha: Sha256Digest,
    include_legacy: bool,
) -> Result<(&'static str, Vec<StoryEvidence>, Option<usize>)> {
    let mature = pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes), source_sha);
    let (mode, mut records, omitted) = match mature {
        Ok(build) => (
            "mature_0x2c_authoritative_story",
            mature_story_records(&build.graph, source_sha)?,
            None,
        ),
        Err(_) if include_legacy => {
            if let Ok(build) =
                pub_reader::build_legacy_0x22_quill_source_graph(Cursor::new(bytes), source_sha)
            {
                let (records, omitted) = legacy_story_records(
                    &build.graph,
                    source_sha,
                    "legacy_0x22_quill_authoritative_exact",
                )?;
                ("legacy_0x22_quill_authoritative_story", records, Some(omitted))
            } else if let Ok(build) =
                pub_reader::build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_sha)
            {
                let (records, omitted) = legacy_story_records(
                    &build.graph,
                    source_sha,
                    "legacy_0x22_noquill_authoritative_exact",
                )?;
                ("legacy_0x22_noquill_authoritative_story", records, Some(omitted))
            } else {
                (
                    "eligible_corrupt_source_confirmed_quill",
                    salvage_story_records(bytes)?,
                    None,
                )
            }
        }
        Err(_) => (
            "eligible_corrupt_source_confirmed_quill",
            salvage_story_records(bytes)?,
            None,
        ),
    };
    ensure!(!records.is_empty(), "no_confirmed_story_text");
    records.sort_by(|a, b| {
        (&a.story_key, a.utf16_start, a.utf16_end).cmp(&(&b.story_key, b.utf16_start, b.utf16_end))
    });
    let mut previous: Option<&StoryEvidence> = None;
    let mut seen = BTreeSet::new();
    for record in &records {
        ensure!(
            seen.insert((
                record.story_key.as_str(),
                record.utf16_start,
                record.utf16_end
            )),
            "duplicate_story_text_range"
        );
        if let Some(last) = previous
            && last.story_key == record.story_key
        {
            ensure!(
                last.utf16_end <= record.utf16_start,
                "overlapping_story_text_ranges"
            );
        }
        previous = Some(record);
    }
    Ok((mode, records, omitted))
}

fn render_text(records: &[StoryEvidence]) -> Vec<u8> {
    let mut text = String::new();
    for record in records {
        text.push_str("=== Story ");
        text.push_str(&record.story_key);
        text.push_str(" [utf16 ");
        text.push_str(&record.utf16_start.to_string());
        text.push_str("..");
        text.push_str(&record.utf16_end.to_string());
        text.push_str("] ===\n");
        text.push_str(&record.text);
        text.push_str("\n\n");
    }
    text.into_bytes()
}

fn assert_matching_existing(
    output: &Path,
    expected_json: &[u8],
    expected_txt: &[u8],
) -> Result<()> {
    ensure!(
        output.is_dir() && !output.is_symlink(),
        "untrusted_existing_story_directory"
    );
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(output)? {
        let item = entry?;
        let ty = item.file_type()?;
        ensure!(
            ty.is_file() && !ty.is_symlink(),
            "untrusted_existing_story_artifact"
        );
        names.insert(item.file_name());
    }
    ensure!(
        names == BTreeSet::from(["stories.json".into(), "stories.txt".into()]),
        "unexpected_story_artifact"
    );
    for (name, bytes) in [
        ("stories.json", expected_json),
        ("stories.txt", expected_txt),
    ] {
        let path = output.join(name);
        let actual_size = fs::metadata(&path)?.len();
        ensure!(
            actual_size == u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            "story_artifact_size_mismatch"
        );
        ensure!(
            fs::read(&path)? == bytes,
            "story_artifact_integrity_mismatch"
        );
    }
    Ok(())
}

fn commit_story_outputs(
    admitted: &AdmittedSource,
    output: &Path,
    json: &[u8],
    txt: &[u8],
) -> Result<()> {
    if output.exists() || output.is_symlink() {
        return assert_matching_existing(output, json, txt);
    }
    let parent = output.parent().context("missing_output_parent")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("clock_before_epoch")?
        .as_nanos();
    let stage = parent.join(format!(".chaptera-story-{}-{tick}", std::process::id()));
    fs::create_dir(&stage).context("create_story_staging_directory")?;

    let write_result: Result<()> = (|| {
        admitted
            .commit_derived_output(&stage.join("stories.json"), json)
            .context("commit_private_story_manifest")?;
        admitted
            .commit_derived_output(&stage.join("stories.txt"), txt)
            .context("commit_private_story_text")?;
        assert_matching_existing(&stage, json, txt)?;
        ensure!(
            !output.exists() && !output.is_symlink(),
            "story_output_collision"
        );
        fs::rename(&stage, output).context("commit_story_directory")?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    write_result
}

pub(crate) fn extract_text(file: &Path, output: &Path, include_legacy: bool) -> Result<usize> {
    let admitted = AdmittedSource::open(file).map_err(anyhow::Error::msg)?;
    let bytes = admitted.bytes();
    let source_hash = super::source_sha256(bytes);
    ensure!(
        source_hash.to_string() == admitted.sha256(),
        "source_snapshot_sha_mismatch"
    );
    let (mode, records, unadmitted_story_count) =
        select_story_records(bytes, source_hash, include_legacy)?;

    // No private filename/path is recorded; these artifacts live only under
    // the operator-selected local output directory.
    let text_bytes = render_text(&records);
    ensure!(
        text_bytes.len() <= MAX_TOTAL_TEXT_BYTES + MAX_RECORDS * 150,
        "story_txt_limit"
    );
    let manifest = TextEvidence {
        schema_version: SCHEMA,
        source_sha256: source_hash.to_string(),
        extraction_class: mode,
        source_immutable: true,
        native_pub_repair_attempted: false,
        native_pub_delivery_allowed: false,
        page_geometry_reconstructed: false,
        story_count: records.len(),
        stories_txt_sha256: sha256(&text_bytes),
        unadmitted_story_count,
        stories: records,
    };
    let mut json_bytes =
        serde_json::to_vec_pretty(&manifest).context("serialize_story_manifest")?;
    json_bytes.push(b'\n');
    ensure!(
        json_bytes.len() <= MAX_JSON_BYTES,
        "story_manifest_size_limit"
    );
    commit_story_outputs(&admitted, output, &json_bytes, &text_bytes)?;
    Ok(manifest.story_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admitted_story(story_key: &str, text: &str, start: u32, end: u32) -> Result<StoryEvidence> {
        let mut rows = Vec::new();
        add_record(
            &mut rows,
            &mut 0_usize,
            StoryEvidence {
                story_key: story_key.to_owned(),
                utf16_start: start,
                utf16_end: end,
                utf8_byte_len: text.len(),
                text_sha256: sha256(text.as_bytes()),
                text: text.to_owned(),
                evidence_class: "salvage_confirmed_quill_text_range",
                carrier: None,
                carrier_byte_offset: None,
                carrier_byte_len: None,
            },
        )?;
        Ok(rows.remove(0))
    }

    #[test]
    fn story_record_enforces_utf16_boundary_for_surrogate_pairs() {
        let item = admitted_story("quill-syid:00000001", "A😀Z", 3, 7).unwrap();
        assert_eq!(item.utf16_end - item.utf16_start, 4);
        assert_eq!(item.text_sha256, sha256("A😀Z".as_bytes()));
        assert_eq!(item.utf8_byte_len, "A😀Z".len());
        assert!(admitted_story("quill-syid:00000001", "A😀Z", 3, 6).is_err());
    }

    #[test]
    fn refuses_unknown_story_key_and_empty_payload() {
        assert!(admitted_story("confidential/name", "hello", 0, 5).is_err());
        assert!(admitted_story("some-story", "", 0, 0).is_err());
    }

    #[test]
    fn record_with_tampered_payload_digest_is_rejected() {
        let mut record = admitted_story("quill-syid:00000002", "hello", 0, 5).unwrap();
        record.text_sha256 = "0".repeat(64);
        assert!(add_record(&mut Vec::new(), &mut 0_usize, record).is_err());
    }

    #[test]
    fn unicode_rendering_preserves_exact_content() {
        let item = admitted_story("quill-syid:00000001", "Добрый день", 0, 11).unwrap();
        let bytes = render_text(&[item]);
        let rendered = String::from_utf8(bytes).unwrap();
        assert!(rendered.contains("Добрый день"));
        assert!(!rendered.contains("repaired PUB"));
    }

    #[test]
    fn matching_existing_artifact_is_idempotent_and_tampering_is_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "chaptera-story-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("stories.json"), b"{}\n").unwrap();
        fs::write(dir.join("stories.txt"), b"text").unwrap();
        assert!(assert_matching_existing(&dir, b"{}\n", b"text").is_ok());
        fs::write(dir.join("stories.txt"), b"fake").unwrap();
        assert!(assert_matching_existing(&dir, b"{}\n", b"text").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
