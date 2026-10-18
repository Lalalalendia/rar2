//! Source-bound diagnostic for legacy no-Quill table cell text.
//!
//! Reuses the one canonical Reader SourceGraph and the one pub-cfb reader.
//! This module never changes Story admission or prints private text.

use anyhow::{Result, ensure};
use chaptera_suite_handoff::AdmittedSource;
use pub_model::{AuthorityClass, NodeKind, ReadConfidence, SourceRef, SourceRole, Story};
use pub_reader::{PubSourceGraph, PubTableSource};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::Path;

use crate::legacy_probe;

const SCHEMA: &str = "chaptera.rescue-table-cell-proof.v1";
const CONTENTS: &str = "/Contents";
const MAX_CELLS_PER_STORY: usize = 4096;
const MAX_STORY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Default, Serialize)]
struct RefusalCounts {
    missing_or_ambiguous_owner: usize,
    incomplete_or_merged_grid: usize,
    unverified_cell_carrier: usize,
    mismatch_in_cell_projection: usize,
    source_stream_unavailable: usize,
}

impl RefusalCounts {
    fn total(&self) -> usize {
        self.missing_or_ambiguous_owner
            + self.incomplete_or_merged_grid
            + self.unverified_cell_carrier
            + self.mismatch_in_cell_projection
            + self.source_stream_unavailable
    }

    fn record(&mut self, reason: Refusal) {
        match reason {
            Refusal::Owner => self.missing_or_ambiguous_owner += 1,
            Refusal::Grid => self.incomplete_or_merged_grid += 1,
            Refusal::Carrier => self.unverified_cell_carrier += 1,
            Refusal::Projection => self.mismatch_in_cell_projection += 1,
            Refusal::Stream => self.source_stream_unavailable += 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Owner,
    Grid,
    Carrier,
    Projection,
    Stream,
}

#[derive(Debug, Serialize)]
pub(crate) struct TableCellProofReceipt {
    schema_version: &'static str,
    source_sha256: String,
    source_byte_len: usize,
    family: &'static str,
    previously_admitted_stories: usize,
    projection_only_candidates: usize,
    proven_table_stories: usize,
    proven_cell_count: usize,
    proven_source_cell_bytes: usize,
    virtual_separator_count: usize,
    refused: RefusalCounts,
    source_immutable: bool,
    semantic_aggregate_exported: bool,
    native_pub_repair_attempted: bool,
    private_text_exposed: bool,
}

#[derive(Debug, Clone, Copy)]
struct VerifiedCell<'a> {
    source_bytes: &'a [u8],
    source_start: usize,
    source_end: usize,
    utf16_start: u32,
    utf16_end: u32,
}

#[derive(Debug)]
struct Proof {
    cells: usize,
    bytes: usize,
    separators: usize,
}

// This is a second check of the *already-decoded*, strict ASCII cell text,
// not a second 0x22/Quill/CFB parser. No Unicode codepage is inferred.
fn checked_join(cells: &[VerifiedCell<'_>], expected_story: &str) -> Result<Proof, Refusal> {
    if cells.is_empty() || cells.len() > MAX_CELLS_PER_STORY {
        return Err(Refusal::Grid);
    }
    let mut assembled = String::new();
    let mut cursor = 0_u32;
    let mut prev_end = 0usize;
    let mut total_bytes = 0usize;
    for (index, cell) in cells.iter().enumerate() {
        if cell.source_end < cell.source_start
            || (index > 0 && cell.source_start < prev_end)
            || cell.source_end - cell.source_start != cell.source_bytes.len()
        {
            return Err(Refusal::Carrier);
        }
        if cell.source_bytes.iter().any(|byte| *byte >= 0x80) {
            return Err(Refusal::Carrier);
        }
        if cell.utf16_start != cursor {
            return Err(Refusal::Projection);
        }
        if index != 0 {
            assembled.push('\r');
            cursor = cursor.checked_add(1).ok_or(Refusal::Projection)?;
        }
        // The canonical legacy Reader admitted only 7-bit ASCII bytes.
        // UTF-8 decoding therefore cannot be a codepage interpretation.
        let text = std::str::from_utf8(cell.source_bytes).map_err(|_| Refusal::Carrier)?;
        assembled.push_str(text);
        cursor = cursor
            .checked_add(u32::try_from(text.len()).map_err(|_| Refusal::Projection)?)
            .ok_or(Refusal::Projection)?;
        if cell.utf16_end != cursor {
            return Err(Refusal::Projection);
        }
        total_bytes = total_bytes
            .checked_add(text.len())
            .ok_or(Refusal::Projection)?;
        if assembled.len() > MAX_STORY_BYTES || total_bytes > MAX_STORY_BYTES {
            return Err(Refusal::Grid);
        }
        prev_end = cell.source_end;
    }
    if assembled != expected_story || cursor as usize != expected_story.encode_utf16().count() {
        return Err(Refusal::Projection);
    }
    Ok(Proof {
        cells: cells.len(),
        bytes: total_bytes,
        separators: cells.len() - 1,
    })
}

fn exact_source_range<'a>(
    reference: &SourceRef,
    graph: &PubSourceGraph,
    contents: &'a [u8],
    role: SourceRole,
    path: &str,
) -> Option<(&'a [u8], usize, usize)> {
    if reference.role != role
        || reference.path.as_deref() != Some(path)
        || reference.carrier != CONTENTS
        || reference.authority != AuthorityClass::Authoritative
        || reference.confidence != Some(ReadConfidence::Exact)
        || reference.validate_primary_source(&graph.source).is_err()
    {
        return None;
    }
    let span = reference.byte_range?;
    let start = usize::try_from(span.offset).ok()?;
    let end = usize::try_from(span.end()?).ok()?;
    Some((contents.get(start..end)?, start, end))
}

fn unique_source_range<'a>(
    references: &[SourceRef],
    graph: &PubSourceGraph,
    contents: &'a [u8],
    role: SourceRole,
    path: &str,
) -> Option<(&'a [u8], usize, usize)> {
    let mut declared = references
        .iter()
        .filter(|reference| reference.role == role && reference.path.as_deref() == Some(path));
    let single = declared.next()?;
    if declared.next().is_some() {
        return None;
    }
    exact_source_range(single, graph, contents, role, path)
}

fn prove_story(
    graph: &PubSourceGraph,
    story: &Story,
    contents: &[u8],
) -> Result<Proof, Refusal> {
    let owners = graph
        .nodes
        .values()
        .filter(|node| {
            node.payload
                .table
                .as_ref()
                .is_some_and(|table| table.story_id == Some(story.id))
        })
        .collect::<Vec<_>>();
    if owners.len() != 1 {
        return Err(Refusal::Owner);
    }
    let owner = owners[0];
    let table: &PubTableSource = owner.payload.table.as_ref().ok_or(Refusal::Owner)?;
    let ownership = owner.payload.table_story.as_ref().ok_or(Refusal::Owner)?;
    if owner.kind != NodeKind::Table
        || ownership.story_id != Some(story.id)
        || ownership.text_id != table.text_id
    {
        return Err(Refusal::Owner);
    }

    let expected = table
        .rows
        .checked_mul(table.columns)
        .and_then(|x| usize::try_from(x).ok())
        .ok_or(Refusal::Grid)?;
    if expected == 0
        || expected > MAX_CELLS_PER_STORY
        || table.cells.len() != expected
        || !table
            .simple_table
            .as_ref()
            .is_some_and(|grid| grid.rows == table.rows
                && grid.columns == table.columns
                && grid.cells_are_row_major())
        || pub_reader::materialize_bounded_simple_table_cells(table, story).is_err()
    {
        return Err(Refusal::Grid);
    }

    let (_, projection_start, projection_end) = unique_source_range(
        &story.source_refs,
        graph,
        contents,
        SourceRole::Projection,
        "legacy_table_text_projection",
    )
    .ok_or(Refusal::Projection)?;

    let mut identities = BTreeSet::new();
    let mut spans = Vec::with_capacity(expected);
    for (index, cell) in table.cells.iter().enumerate() {
        let idx = u32::try_from(index).map_err(|_| Refusal::Grid)?;
        let (row, column) = (idx / table.columns, idx % table.columns);
        let coords = cell.coordinates.ok_or(Refusal::Grid)?;
        if coords.start_row != row
            || coords.end_row != row
            || coords.start_column != column
            || coords.end_column != column
            || !identities.insert(cell.id)
        {
            return Err(Refusal::Grid);
        }
        let (bytes, start, end) = unique_source_range(
            &cell.source_refs,
            graph,
            contents,
            SourceRole::Semantic,
            "legacy_table_cell_text",
        )
        .ok_or(Refusal::Carrier)?;
        if start < projection_start || end > projection_end {
            return Err(Refusal::Projection);
        }
        // The source boundary is itself a separately persisted relation.
        // It is *not* claimed to contain a literal CR byte.
        unique_source_range(
            &cell.source_refs,
            graph,
            contents,
            SourceRole::Relation,
            "legacy_table_cell_boundary",
        )
        .ok_or(Refusal::Carrier)?;
        spans.push(VerifiedCell {
            source_bytes: bytes,
            source_start: start,
            source_end: end,
            utf16_start: cell.utf16_start,
            utf16_end: cell.utf16_end,
        });
    }
    checked_join(&spans, &story.text)
}

// Exactly the same one source-authority stream as canonical pub-reader,
// including its constrained no-Quill direct-root fallback.
fn source_contents(bytes: &[u8]) -> Option<Vec<u8>> {
    if let Ok(contents) = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS) {
        return Some(contents);
    }
    let recovered =
        pub_cfb::recover_root_regular_stream_reader(Cursor::new(bytes), CONTENTS).ok()?;
    if recovered.root_entry_names.iter().any(|name| {
        name.eq_ignore_ascii_case("Quill") || name.eq_ignore_ascii_case("Escher")
    }) {
        return None;
    }
    Some(recovered.bytes)
}

pub(crate) fn probe(file: &Path) -> Result<TableCellProofReceipt> {
    let admitted = AdmittedSource::open(file)
        .map_err(|_| anyhow::anyhow!("table_cell_source_unavailable"))?;
    let bytes = admitted.bytes();
    let hash = super::source_sha256(bytes);
    ensure!(hash.to_string() == admitted.sha256(), "table_cell_source_sha_drift");
    let (family, build) = legacy_probe::build_provenance_graph(bytes, hash);
    let mut receipt = TableCellProofReceipt {
        schema_version: SCHEMA,
        source_sha256: hash.to_string(),
        source_byte_len: bytes.len(),
        family,
        previously_admitted_stories: 0,
        projection_only_candidates: 0,
        proven_table_stories: 0,
        proven_cell_count: 0,
        proven_source_cell_bytes: 0,
        virtual_separator_count: 0,
        refused: RefusalCounts::default(),
        source_immutable: true,
        semantic_aggregate_exported: false,
        native_pub_repair_attempted: false,
        private_text_exposed: false,
    };
    let Some(build) = build else {
        return Ok(receipt);
    };
    let graph = &build.graph;
    let (existing, _, _, reasons) = legacy_probe::count_proven(graph);
    receipt.previously_admitted_stories = existing;
    if family != "legacy_0x22_noquill" {
        return Ok(receipt);
    }
    receipt.projection_only_candidates = reasons.projection_only_exact;
    let contents = source_contents(bytes);
    let mut counted = 0usize;
    for story in graph.stories.values().filter(|story| {
        !story.text.is_empty()
            && legacy_probe::safe_story_key(&story.id.as_canonical().to_string())
            && legacy_probe::is_projection_only_exact(graph, story)
    }) {
        counted += 1;
        let verified = if let Some(ref contents) = contents {
            prove_story(graph, story, contents)
        } else {
            Err(Refusal::Stream)
        };
        match verified {
            Ok(proof) => {
                receipt.proven_table_stories += 1;
                receipt.proven_cell_count += proof.cells;
                receipt.proven_source_cell_bytes += proof.bytes;
                receipt.virtual_separator_count += proof.separators;
            }
            Err(reason) => receipt.refused.record(reason),
        }
    }
    ensure!(
        counted == receipt.projection_only_candidates
            && counted == receipt.proven_table_stories + receipt.refused.total(),
        "table_cell_proof_candidate_partition"
    );
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell<'a>(bytes: &'a [u8], offset: usize, start: u32, end: u32) -> VerifiedCell<'a> {
        VerifiedCell {
            source_bytes: bytes,
            source_start: offset,
            source_end: offset + bytes.len(),
            utf16_start: start,
            utf16_end: end,
        }
    }

    #[test]
    fn synthetic_separator_is_not_mistaken_for_source_bytes() {
        let proof = checked_join(
            &[cell(b"one", 0, 0, 3), cell(b"two", 10, 3, 7)],
            "one\rtwo",
        )
        .expect("exact ASCII cells and one virtual separator");
        assert_eq!((proof.cells, proof.bytes, proof.separators), (2, 6, 1));
    }

    #[test]
    fn corrupt_separator_or_utf16_position_is_not_admitted() {
        assert_eq!(
            checked_join(&[cell(b"a", 0, 0, 1), cell(b"b", 4, 1, 3)], "a b")
                .unwrap_err(),
            Refusal::Projection
        );
        assert_eq!(
            checked_join(&[cell(b"a", 0, 0, 1), cell(b"b", 4, 2, 3)], "a\rb")
                .unwrap_err(),
            Refusal::Projection
        );
    }

    #[test]
    fn non_ascii_and_overlapping_raw_carriers_fail_closed() {
        assert_eq!(
            checked_join(&[cell(&[0x80], 0, 0, 1)], "\u{0080}").unwrap_err(),
            Refusal::Carrier
        );
        assert_eq!(
            checked_join(&[cell(b"ab", 0, 0, 2), cell(b"c", 1, 2, 4)], "ab\rc")
                .unwrap_err(),
            Refusal::Carrier
        );
    }

    #[test]
    fn missing_or_excess_cell_content_is_not_a_semantic_story() {
        assert_eq!(checked_join(&[], "").unwrap_err(), Refusal::Grid);
        assert_eq!(
            checked_join(&[cell(b"abc", 0, 0, 3)], "abd").unwrap_err(),
            Refusal::Projection
        );
        assert_eq!(
            checked_join(&[cell(b"a", 0, 0, 1)], "a\r").unwrap_err(),
            Refusal::Projection
        );
    }

    #[test]
    fn empty_cell_still_requires_its_source_owned_position() {
        let proof = checked_join(
            &[cell(b"", 0, 0, 0), cell(b"x", 1, 0, 2)],
            "\rx",
        )
        .expect("source-owned empty first cell");
        assert_eq!(proof.bytes, 1);
    }
}
