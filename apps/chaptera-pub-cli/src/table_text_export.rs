//! Explicitly opted-in, byte-verifiable export of already-proven legacy table cells.
//!
//! This is a separate private evidence class. A table aggregate remains a
//! SourceRole::Projection; only its individual cells own original source bytes.
//! The default Publisher Story exporter is neither replaced nor weakened.

use anyhow::{Context, Result, ensure};
use chaptera_suite_handoff::AdmittedSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::{legacy_probe, table_cell_proof, text_export};

const SCHEMA: &str = "chaptera.rescue-table-cell-evidence.v1";
const CLASS: &str = "legacy_0x22_noquill_source_exact_cells_v1";
const EVIDENCE: &str = "cell_proven_projected_story_v1";
const CONTENTS: &str = "/Contents";
const MAX_MANIFEST: usize = 24 * 1024 * 1024;
const MAX_TEXT: usize = 16 * 1024 * 1024;
const MAX_STORIES: usize = 16_384;
const MAX_CELLS: usize = 4096;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CellStory {
    story_key: String,
    utf16_start: u32,
    utf16_end: u32,
    text: String,
    utf8_byte_len: usize,
    text_sha256: String,
    evidence_class: String,
    projection_carrier: String,
    projection_byte_offset: u64,
    projection_byte_len: u64,
    rows: u32,
    columns: u32,
    cells: Vec<table_cell_proof::VerifiedTableCell>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CellManifest {
    schema_version: String,
    source_sha256: String,
    source_byte_len: usize,
    extraction_class: String,
    source_immutable: bool,
    native_pub_repair_attempted: bool,
    native_pub_delivery_allowed: bool,
    page_geometry_reconstructed: bool,
    projected_story_not_semantic: bool,
    story_count: usize,
    unadmitted_projection_count: usize,
    stories_txt_sha256: String,
    stories: Vec<CellStory>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TableTextVerification {
    schema_version: &'static str,
    source_sha256: String,
    verified_table_stories: usize,
    verified_source_cells: usize,
    synthetic_cr_separators: usize,
    source_immutable: bool,
    native_pub_repair: bool,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn safe_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'_'))
}

fn render(stories: &[CellStory]) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    for story in stories {
        ensure!(safe_key(&story.story_key), "invalid_table_story_key");
        text.extend_from_slice(
            format!(
                "=== Story {} [utf16 {}..{}] ===\n",
                story.story_key, story.utf16_start, story.utf16_end
            )
            .as_bytes(),
        );
        text.extend_from_slice(story.text.as_bytes());
        text.extend_from_slice(b"\n\n");
        ensure!(
            text.len() <= MAX_TEXT + MAX_STORIES * 150,
            "table_story_text_limit"
        );
    }
    Ok(text)
}

fn exact_source_span<'a>(contents: &'a [u8], start: u64, len: u64) -> Result<&'a [u8]> {
    let end = start.checked_add(len).context("cell_carrier_overflow")?;
    let first = usize::try_from(start).context("cell_carrier_offset_limit")?;
    let last = usize::try_from(end).context("cell_carrier_length_limit")?;
    contents
        .get(first..last)
        .context("cell_carrier_outside_contents")
}

/// This verifier independently reconstructs text from physical Contents bytes.
/// It deliberately does not call the canonical Reader, proof admission, or
/// exporter projection function; only the common CFB stream reader is reused.
fn validate_private_manifest(
    source_bytes: &[u8],
    manifest: &CellManifest,
    txt: &[u8],
) -> Result<(usize, usize)> {
    ensure!(
        manifest.schema_version == SCHEMA
            && manifest.source_sha256 == sha(source_bytes)
            && manifest.source_byte_len == source_bytes.len()
            && manifest.extraction_class == CLASS
            && manifest.source_immutable
            && !manifest.native_pub_repair_attempted
            && !manifest.native_pub_delivery_allowed
            && !manifest.page_geometry_reconstructed
            && manifest.projected_story_not_semantic
            && manifest.stories_txt_sha256 == sha(txt),
        "table_story_manifest_authority_mismatch"
    );
    ensure!(
        manifest.story_count == manifest.stories.len()
            && (1..=MAX_STORIES).contains(&manifest.story_count)
            && manifest.unadmitted_projection_count <= MAX_STORIES,
        "invalid_table_story_count"
    );
    let contents = table_cell_proof::source_contents(source_bytes)
        .context("independent_table_source_stream_unavailable")?;
    let mut previous_key: Option<&str> = None;
    let mut total_bytes = 0usize;
    let mut total_cells = 0usize;
    let mut separators = 0usize;
    for story in &manifest.stories {
        ensure!(
            safe_key(&story.story_key)
                && previous_key.is_none_or(|key| story.story_key.as_str() > key),
            "unsafe_or_duplicate_table_story_key"
        );
        previous_key = Some(&story.story_key);
        ensure!(
            story.evidence_class == EVIDENCE
                && story.projection_carrier == CONTENTS
                && story.utf16_start == 0
                && story.utf16_end > 0
                && story.utf16_end as usize == story.text.encode_utf16().count()
                && story.utf8_byte_len == story.text.len()
                && story.text_sha256 == sha(story.text.as_bytes()),
            "table_story_content_or_role_mismatch"
        );
        ensure!(
            story.rows > 0
                && story.columns > 0
                && story
                    .rows
                    .checked_mul(story.columns)
                    .is_some_and(|n| usize::try_from(n).ok() == Some(story.cells.len()))
                && (1..=MAX_CELLS).contains(&story.cells.len()),
            "table_story_grid_not_rectangular"
        );
        let projection_end = story
            .projection_byte_offset
            .checked_add(story.projection_byte_len)
            .context("table_projection_overflow")?;
        exact_source_span(
            &contents,
            story.projection_byte_offset,
            story.projection_byte_len,
        )?;
        let mut restored = String::new();
        let mut utf16 = 0_u32;
        let mut prev_carrier_end = 0_u64;
        for (index, cell) in story.cells.iter().enumerate() {
            let index_u32 = u32::try_from(index)?;
            ensure!(
                cell.row == index_u32 / story.columns
                    && cell.column == index_u32 % story.columns
                    && cell.virtual_cr_before == (index > 0)
                    && cell.utf16_start == utf16,
                "table_cell_order_or_utf16_start_mismatch"
            );
            let carrier_end = cell
                .carrier_byte_offset
                .checked_add(cell.carrier_byte_len)
                .context("cell_source_range_overflow")?;
            ensure!(
                cell.carrier_byte_offset >= story.projection_byte_offset
                    && carrier_end <= projection_end
                    && (index == 0 || cell.carrier_byte_offset >= prev_carrier_end),
                "table_cell_nonmonotone_or_outside_projection"
            );
            let bytes =
                exact_source_span(&contents, cell.carrier_byte_offset, cell.carrier_byte_len)?;
            ensure!(
                bytes.iter().all(u8::is_ascii) && sha(bytes) == cell.cell_text_sha256,
                "table_cell_original_bytes_mismatch"
            );
            let boundary =
                exact_source_span(&contents, cell.boundary_byte_offset, cell.boundary_byte_len)?;
            ensure!(
                cell.boundary_byte_len > 0 && sha(boundary) == cell.boundary_sha256,
                "table_cell_boundary_changed"
            );
            if index > 0 {
                restored.push('\r');
                utf16 = utf16.checked_add(1).context("table_separator_overflow")?;
                separators += 1;
            }
            let source_text = std::str::from_utf8(bytes).context("non_ascii_cell_bytes")?;
            restored.push_str(source_text);
            utf16 = utf16
                .checked_add(u32::try_from(source_text.len())?)
                .context("cell_utf16_limit")?;
            ensure!(cell.utf16_end == utf16, "table_cell_end_mismatch");
            prev_carrier_end = carrier_end;
            total_cells += 1;
            total_bytes = total_bytes
                .checked_add(source_text.len())
                .context("table_total_text_overflow")?;
            ensure!(total_bytes <= MAX_TEXT, "table_total_text_limit");
        }
        ensure!(
            restored == story.text && utf16 == story.utf16_end,
            "table_story_not_reconstructed_from_original_bytes"
        );
    }
    ensure!(
        render(&manifest.stories)? == txt && separators == total_cells - manifest.story_count,
        "table_stories_txt_or_separator_mismatch"
    );
    Ok((total_cells, separators))
}

fn check_output_directory(output: &Path) -> Result<()> {
    for parent in output.ancestors() {
        ensure!(!parent.is_symlink(), "symlink_table_story_parent");
    }
    ensure!(output.is_dir(), "missing_table_story_directory");
    let entries = fs::read_dir(output)?.collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(entries.len() == 2, "unexpected_table_story_artifact");
    let names = entries
        .iter()
        .map(|entry| {
            let ty = entry.file_type()?;
            ensure!(ty.is_file() && !ty.is_symlink(), "unsafe_table_story_file");
            Ok(entry.file_name())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        names == BTreeSet::from(["stories.json".into(), "stories.txt".into()]),
        "unexpected_table_story_file"
    );
    Ok(())
}

pub(crate) fn extract_table_text(source: &Path, output: &Path) -> Result<usize> {
    let admitted = AdmittedSource::open(source).map_err(anyhow::Error::msg)?;
    let bytes = admitted.bytes();
    let digest = super::source_sha256(bytes);
    ensure!(
        digest.to_string() == admitted.sha256(),
        "table_source_sha_mismatch"
    );
    let (family, built) = legacy_probe::build_provenance_graph(bytes, digest);
    ensure!(
        family == "legacy_0x22_noquill",
        "no_eligible_legacy_table_source"
    );
    let graph = built.context("no_canonical_legacy_table_graph")?.graph;
    let (proven, refused) = table_cell_proof::collect_verified_tables(&graph, bytes)?;
    ensure!(!proven.is_empty(), "no_cell_proven_table_story");
    let mut records = Vec::new();
    for story in proven {
        let utf16_end =
            u32::try_from(story.text.encode_utf16().count()).context("table_story_utf16_limit")?;
        let record = CellStory {
            story_key: story.story_key,
            utf16_start: 0,
            utf16_end,
            utf8_byte_len: story.text.len(),
            text_sha256: sha(story.text.as_bytes()),
            text: story.text,
            evidence_class: EVIDENCE.to_owned(),
            projection_carrier: CONTENTS.to_owned(),
            projection_byte_offset: story.projection_byte_offset,
            projection_byte_len: story.projection_byte_len,
            rows: story.rows,
            columns: story.columns,
            cells: story.cells,
        };
        records.push(record);
    }
    records.sort_by(|a, b| a.story_key.cmp(&b.story_key));
    let txt = render(&records)?;
    let manifest = CellManifest {
        schema_version: SCHEMA.to_owned(),
        source_sha256: digest.to_string(),
        source_byte_len: bytes.len(),
        extraction_class: CLASS.to_owned(),
        source_immutable: true,
        native_pub_repair_attempted: false,
        native_pub_delivery_allowed: false,
        page_geometry_reconstructed: false,
        projected_story_not_semantic: true,
        story_count: records.len(),
        unadmitted_projection_count: refused,
        stories_txt_sha256: sha(&txt),
        stories: records,
    };
    validate_private_manifest(bytes, &manifest, &txt)?;
    let mut json =
        serde_json::to_vec_pretty(&manifest).context("serialize_table_story_manifest")?;
    json.push(b'\n');
    ensure!(json.len() <= MAX_MANIFEST, "table_story_manifest_limit");
    check_safe_output_parent(output)?;
    text_export::commit_story_outputs(&admitted, output, &json, &txt)?;
    verify_table_text(source, output)?;
    Ok(manifest.story_count)
}

fn check_safe_output_parent(output: &Path) -> Result<()> {
    for ancestor in output.ancestors() {
        ensure!(!ancestor.is_symlink(), "unsafe_table_story_path");
    }
    Ok(())
}

pub(crate) fn verify_table_text(source: &Path, output: &Path) -> Result<TableTextVerification> {
    let admitted = AdmittedSource::open(source).map_err(anyhow::Error::msg)?;
    let bytes = admitted.bytes();
    let digest = super::source_sha256(bytes);
    ensure!(
        digest.to_string() == admitted.sha256(),
        "table_verify_source_drift"
    );
    check_output_directory(output)?;
    let json_path = output.join("stories.json");
    let text_path = output.join("stories.txt");
    ensure!(
        fs::metadata(&json_path)?.len() <= MAX_MANIFEST as u64,
        "table_json_limit"
    );
    ensure!(
        fs::metadata(&text_path)?.len() <= (MAX_TEXT + MAX_STORIES * 150) as u64,
        "table_txt_limit"
    );
    let json = fs::read(json_path).context("read_private_table_json")?;
    let txt = fs::read(text_path).context("read_private_table_text")?;
    let manifest: CellManifest =
        serde_json::from_slice(&json).context("parse_private_table_manifest")?;
    let (cells, separators) = validate_private_manifest(bytes, &manifest, &txt)?;
    // Byte-verification alone is insufficient: a forged private manifest could
    // otherwise select unrelated ASCII Contents slices and recompute hashes.
    // Cross-check every source range, relation, table owner and row identity
    // against the independently rebuilt canonical Reader SourceGraph.
    let (family, build) = legacy_probe::build_provenance_graph(bytes, digest);
    ensure!(family == "legacy_0x22_noquill", "table_verify_wrong_family");
    let graph = build.context("table_verify_missing_source_graph")?.graph;
    let (mut expected, refused) = table_cell_proof::collect_verified_tables(&graph, bytes)?;
    expected.sort_by(|a, b| a.story_key.cmp(&b.story_key));
    ensure!(
        expected.len() == manifest.stories.len() && refused == manifest.unadmitted_projection_count,
        "table_verify_admission_set_mismatch"
    );
    for (witness, entry) in expected.iter().zip(&manifest.stories) {
        ensure!(
            witness.story_key == entry.story_key
                && witness.text == entry.text
                && witness.projection_byte_offset == entry.projection_byte_offset
                && witness.projection_byte_len == entry.projection_byte_len
                && witness.rows == entry.rows
                && witness.columns == entry.columns
                && serde_json::to_value(&witness.cells)? == serde_json::to_value(&entry.cells)?,
            "table_verify_source_graph_provenance_mismatch"
        );
    }
    ensure!(
        super::source_sha256(admitted.bytes()) == digest,
        "source_changed_after_table_verify"
    );
    Ok(TableTextVerification {
        schema_version: "chaptera.rescue-table-cell-verified.v1",
        source_sha256: digest.to_string(),
        verified_table_stories: manifest.story_count,
        verified_source_cells: cells,
        synthetic_cr_separators: separators,
        source_immutable: true,
        native_pub_repair: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_key_or_untrusted_extraction_class_is_rejected() {
        assert!(safe_key("legacy-table:0001"));
        assert!(!safe_key("../private.pub"));
        assert!(!safe_key("not a key"));
        assert!(!safe_key(""));
    }

    #[test]
    fn private_table_txt_projection_uses_exact_cr_format() {
        let record = CellStory {
            story_key: "tab:1".into(),
            utf16_start: 0,
            utf16_end: 3,
            text: "a\rb".into(),
            utf8_byte_len: 3,
            text_sha256: sha(b"a\rb"),
            evidence_class: EVIDENCE.into(),
            projection_carrier: CONTENTS.into(),
            projection_byte_offset: 0,
            projection_byte_len: 3,
            rows: 1,
            columns: 2,
            cells: Vec::new(),
        };
        let rendered = String::from_utf8(render(&[record]).unwrap()).unwrap();
        assert_eq!(rendered, "=== Story tab:1 [utf16 0..3] ===\na\rb\n\n");
    }
}
