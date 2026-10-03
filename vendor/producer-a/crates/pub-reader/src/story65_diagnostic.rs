use super::{
    CONTENTS_STREAM_PATH, FIELD_STORY_ID, QUILL_STREAM_PATH, RAW_TYPE_SHAPE, RAW_TYPE_TABLE,
    ReaderPartialSourceFact, ReaderPartialSourceGap, ReaderPartialSourceGraphError,
    ReaderSalvageTrigger, build_reader_partial_source_graph, build_reference_index,
    chunk_for_reference, probe_reader_salvage_candidate_with_trigger, single_raw_type,
    unique_reference_by_raw_type, unique_u32_field,
};
use anyhow::{Context, Result};
use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, StoryCatalogReadError, parse_0x2c_header,
    parse_bounded_empty_mature_story_catalog_variant, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_mature_story_catalog,
};
use pub_core::StreamPath;
use pub_quill::{QuillStoryReadError, parse_confirmed_story_catalog};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

pub const PUB_STORY65_CONTINUATION_DIAGNOSTIC_SCHEMA_V1: &str =
    "chaptera.pub-story65-continuation-diagnostic.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PubStory65CatalogState {
    Confirmed { entry_count: usize },
    PhysicalEmpty,
    MissingCountButNotPhysicalEmpty { error_kind: String },
    Rejected { error_kind: String },
}

impl PubStory65CatalogState {
    pub const fn is_physical_empty(&self) -> bool {
        matches!(self, Self::PhysicalEmpty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PubQuillStoryState {
    Confirmed {
        descriptor_node_count: usize,
        descriptor_count: usize,
        syid_count: usize,
        strs_count: usize,
        story_count: usize,
        text_byte_len: usize,
        tcd_count: usize,
        tokn_count: usize,
    },
    Rejected {
        error_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chunk: Option<String>,
    },
}

impl PubQuillStoryState {
    pub fn is_missing_required_chunk(&self, expected: &str) -> bool {
        matches!(
            self,
            Self::Rejected {
                error_kind,
                chunk: Some(chunk),
            } if error_kind == "missing_required_chunk" && chunk == expected
        )
    }

    pub fn coarse_key(&self) -> String {
        match self {
            Self::Confirmed { .. } => "confirmed".to_owned(),
            Self::Rejected { error_kind, chunk } => match chunk {
                Some(chunk) => format!("{error_kind}:{chunk}"),
                None => error_kind.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubStory65LiveStoryDemand {
    pub distinct_story_id_count: usize,
    pub shape_reference_count: usize,
    pub table_reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubStory65ForcedGraphSummary {
    pub status: String,
    pub fact_counts: BTreeMap<String, usize>,
    pub gap_counts: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubStory65ContinuationDiagnostic {
    pub schema: String,
    pub source_sha256: String,
    pub byte_len: usize,
    pub story65: PubStory65CatalogState,
    pub live_story_demand: PubStory65LiveStoryDemand,
    pub quill: PubQuillStoryState,
    pub story65_geometry_only_gate_eligible: bool,
    pub forced_partial_graph: PubStory65ForcedGraphSummary,
}

fn source_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn story_catalog_error_kind(error: &StoryCatalogReadError) -> &'static str {
    match error {
        StoryCatalogReadError::Contents(_) => "contents",
        StoryCatalogReadError::Block(_) => "block",
        StoryCatalogReadError::SpanTooLarge { .. } => "span_too_large",
        StoryCatalogReadError::MissingDeclaredCount => "missing_declared_count",
        StoryCatalogReadError::DuplicateDeclaredCount => "duplicate_declared_count",
        StoryCatalogReadError::InvalidDeclaredCount => "invalid_declared_count",
        StoryCatalogReadError::MissingEntryArray => "missing_entry_array",
        StoryCatalogReadError::DuplicateEntryArray => "duplicate_entry_array",
        StoryCatalogReadError::InvalidEntryArray => "invalid_entry_array",
        StoryCatalogReadError::UnexpectedEntryId { .. } => "unexpected_entry_id",
        StoryCatalogReadError::InvalidEntryContainer { .. } => "invalid_entry_container",
        StoryCatalogReadError::MissingTextId { .. } => "missing_text_id",
        StoryCatalogReadError::DuplicateTextId { .. } => "duplicate_text_id",
        StoryCatalogReadError::InvalidTextId { .. } => "invalid_text_id",
        StoryCatalogReadError::MissingLayoutKey { .. } => "missing_layout_key",
        StoryCatalogReadError::DuplicateLayoutKey { .. } => "duplicate_layout_key",
        StoryCatalogReadError::InvalidLayoutKey { .. } => "invalid_layout_key",
        StoryCatalogReadError::DuplicateTextIdentity { .. } => "duplicate_text_identity",
        StoryCatalogReadError::EntryCountMismatch { .. } => "entry_count_mismatch",
        StoryCatalogReadError::PhysicalEmptyChunkUnexpectedLength { .. } => {
            "physical_empty_chunk_unexpected_length"
        }
        StoryCatalogReadError::PhysicalEmptyChunkHasFields { .. } => {
            "physical_empty_chunk_has_fields"
        }
        StoryCatalogReadError::PhysicalEmptyChunkAmbiguousTail { .. } => {
            "physical_empty_chunk_ambiguous_tail"
        }
    }
}

fn fourcc(value: &[u8; 4]) -> String {
    String::from_utf8_lossy(value).trim_end().to_owned()
}

fn quill_error(error: &QuillStoryReadError) -> (String, Option<String>) {
    let (kind, chunk) = match error {
        QuillStoryReadError::TooShort { .. } => ("too_short", None),
        QuillStoryReadError::DescriptorNodeTruncated { .. } => ("descriptor_node_truncated", None),
        QuillStoryReadError::DescriptorListPointerOutOfBounds { .. } => {
            ("descriptor_list_pointer_out_of_bounds", None)
        }
        QuillStoryReadError::DescriptorListCycle { .. } => ("descriptor_list_cycle", None),
        QuillStoryReadError::UnexpectedDescriptorPresenceMarker { .. } => {
            ("unexpected_descriptor_presence_marker", None)
        }
        QuillStoryReadError::ChunkOutOfBounds { name, .. } => {
            ("chunk_out_of_bounds", Some(fourcc(name)))
        }
        QuillStoryReadError::MissingRequiredChunk { name } => {
            ("missing_required_chunk", Some(fourcc(name)))
        }
        QuillStoryReadError::DuplicateRequiredChunk { name } => {
            ("duplicate_required_chunk", Some(fourcc(name)))
        }
        QuillStoryReadError::StrsServiceSpanOutOfBounds { .. } => {
            ("strs_service_span_out_of_bounds", None)
        }
        QuillStoryReadError::StoryCountMismatch { .. } => ("story_count_mismatch", None),
        QuillStoryReadError::TextLengthOverflow => ("text_length_overflow", None),
        QuillStoryReadError::TextLengthMismatch { .. } => ("text_length_mismatch", None),
        QuillStoryReadError::TcdStoryOrdinalOutOfBounds { .. } => {
            ("tcd_story_ordinal_out_of_bounds", None)
        }
        QuillStoryReadError::TcdCellCountOverflow { .. } => ("tcd_cell_count_overflow", None),
        QuillStoryReadError::ToknStoryOrdinalOutOfBounds { .. } => {
            ("tokn_story_ordinal_out_of_bounds", None)
        }
        QuillStoryReadError::ToknUnexpectedPlcType { .. } => ("tokn_unexpected_plc_type", None),
        QuillStoryReadError::ToknCountOverflow { .. } => ("tokn_count_overflow", None),
        QuillStoryReadError::ToknNonMonotonicBoundary { .. } => {
            ("tokn_non_monotonic_boundary", None)
        }
        QuillStoryReadError::ToknInvalidBlockLength { .. } => ("tokn_invalid_block_length", None),
        QuillStoryReadError::ToknTokenSpanOverflow { .. } => ("tokn_token_span_overflow", None),
        QuillStoryReadError::ToknTokenLengthExceedsBoundary { .. } => {
            ("tokn_token_length_exceeds_boundary", None)
        }
        QuillStoryReadError::ToknTargetSectionOverflow => ("tokn_target_section_overflow", None),
    };
    (kind.to_owned(), chunk)
}

fn gap_key(gap: ReaderPartialSourceGap) -> &'static str {
    match gap {
        ReaderPartialSourceGap::TextUnavailable => "text_unavailable",
        ReaderPartialSourceGap::TextSemanticAmbiguity => "text_semantic_ambiguity",
        ReaderPartialSourceGap::ImageFactsUnavailable => "image_facts_unavailable",
        ReaderPartialSourceGap::GeometryFactsUnavailable => "geometry_facts_unavailable",
    }
}

fn fact_key(fact: &ReaderPartialSourceFact) -> &'static str {
    match fact {
        ReaderPartialSourceFact::TextRange { .. } => "text_range",
        ReaderPartialSourceFact::VerifiedImage { .. } => "verified_image",
        ReaderPartialSourceFact::GroundedGeometry { .. } => "grounded_geometry",
    }
}

fn graph_error_key(error: ReaderPartialSourceGraphError) -> &'static str {
    match error {
        ReaderPartialSourceGraphError::SourceIdentityMismatch => "source_identity_mismatch",
        ReaderPartialSourceGraphError::SourceModified => "source_modified",
        ReaderPartialSourceGraphError::ProbeMismatch => "probe_mismatch",
        ReaderPartialSourceGraphError::Ineligible => "ineligible",
    }
}

pub fn build_story65_continuation_diagnostic(
    bytes: &[u8],
) -> Result<PubStory65ContinuationDiagnostic> {
    let source_sha256 = source_sha256(bytes);
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM_PATH)
        .context("read mature Contents for Story65 continuation diagnostic")?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM_PATH)
        .context("read mature Quill for Story65 continuation diagnostic")?;

    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse mature Contents header for Story65 continuation diagnostic")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature Contents trailer for Story65 continuation diagnostic")?;
    let references = build_reference_index(&contents, &trailer.directory)
        .context("index Contents references for Story65 continuation diagnostic")?;

    let story_catalog_reference = unique_reference_by_raw_type(
        &references,
        CONTENTS_RAW_TYPE_STORY_CATALOG,
        "Story catalog 0x65",
    )
    .context("locate Story65 for continuation diagnostic")?;
    let story_catalog_chunk =
        chunk_for_reference(contents_stream.clone(), &contents, story_catalog_reference)
            .context("bound Story65 for continuation diagnostic")?;

    let story65 = match parse_confirmed_mature_story_catalog(&contents, &story_catalog_chunk) {
        Ok(catalog) => PubStory65CatalogState::Confirmed {
            entry_count: catalog.entries.len(),
        },
        Err(StoryCatalogReadError::MissingDeclaredCount) => {
            match parse_bounded_empty_mature_story_catalog_variant(&contents, &story_catalog_chunk)
            {
                Ok(_) => PubStory65CatalogState::PhysicalEmpty,
                Err(error) => PubStory65CatalogState::MissingCountButNotPhysicalEmpty {
                    error_kind: story_catalog_error_kind(&error).to_owned(),
                },
            }
        }
        Err(error) => PubStory65CatalogState::Rejected {
            error_kind: story_catalog_error_kind(&error).to_owned(),
        },
    };

    let mut distinct_story_ids = BTreeSet::new();
    let mut shape_reference_count = 0usize;
    let mut table_reference_count = 0usize;
    for reference in references.values() {
        let raw_type = single_raw_type(reference);
        if !matches!(raw_type, Some(RAW_TYPE_SHAPE) | Some(RAW_TYPE_TABLE)) {
            continue;
        }
        let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)
            .context("bound Story-bearing reference for continuation diagnostic")?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)
            .context("read Story id for continuation diagnostic")?
        else {
            continue;
        };
        distinct_story_ids.insert(text_id);
        match raw_type {
            Some(RAW_TYPE_SHAPE) => shape_reference_count += 1,
            Some(RAW_TYPE_TABLE) => table_reference_count += 1,
            _ => {}
        }
    }
    let live_story_demand = PubStory65LiveStoryDemand {
        distinct_story_id_count: distinct_story_ids.len(),
        shape_reference_count,
        table_reference_count,
    };

    let quill_state =
        match parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), &quill) {
            Ok(catalog) => PubQuillStoryState::Confirmed {
                descriptor_node_count: catalog.descriptor_nodes.len(),
                descriptor_count: catalog
                    .descriptor_nodes
                    .iter()
                    .map(|node| node.descriptors.len())
                    .sum(),
                syid_count: catalog.syid.ids.len(),
                strs_count: catalog.strs.lengths.len(),
                story_count: catalog.stories.len(),
                text_byte_len: catalog.text.bytes.len(),
                tcd_count: catalog.tcd.len(),
                tokn_count: catalog.tokn.len(),
            },
            Err(error) => {
                let (error_kind, chunk) = quill_error(&error);
                PubQuillStoryState::Rejected { error_kind, chunk }
            }
        };

    let story65_geometry_only_gate_eligible = story65.is_physical_empty()
        && live_story_demand.distinct_story_id_count == 0
        && quill_state.is_missing_required_chunk("STRS");

    let forced_probe = probe_reader_salvage_candidate_with_trigger(
        bytes,
        ReaderSalvageTrigger::ProvenStructuralCorruption,
    );
    let forced_partial_graph = match build_reader_partial_source_graph(bytes, &forced_probe) {
        Ok(graph) => {
            let mut fact_counts = BTreeMap::new();
            for fact in &graph.facts {
                *fact_counts.entry(fact_key(fact).to_owned()).or_insert(0) += 1;
            }
            let mut gap_counts = BTreeMap::new();
            for gap in graph.gaps {
                *gap_counts.entry(gap_key(gap).to_owned()).or_insert(0) += 1;
            }
            PubStory65ForcedGraphSummary {
                status: "constructed".to_owned(),
                fact_counts,
                gap_counts,
                error_kind: None,
            }
        }
        Err(error) => PubStory65ForcedGraphSummary {
            status: "error".to_owned(),
            fact_counts: BTreeMap::new(),
            gap_counts: BTreeMap::new(),
            error_kind: Some(graph_error_key(error).to_owned()),
        },
    };

    Ok(PubStory65ContinuationDiagnostic {
        schema: PUB_STORY65_CONTINUATION_DIAGNOSTIC_SCHEMA_V1.to_owned(),
        source_sha256,
        byte_len: bytes.len(),
        story65,
        live_story_demand,
        quill: quill_state,
        story65_geometry_only_gate_eligible,
        forced_partial_graph,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quill_error_codes_are_source_safe_and_stable() {
        let (kind, chunk) =
            quill_error(&QuillStoryReadError::MissingRequiredChunk { name: *b"STRS" });
        assert_eq!(kind, "missing_required_chunk");
        assert_eq!(chunk.as_deref(), Some("STRS"));

        let (kind, chunk) = quill_error(&QuillStoryReadError::TextLengthMismatch {
            expected_bytes: 8,
            actual_bytes: 6,
        });
        assert_eq!(kind, "text_length_mismatch");
        assert_eq!(chunk, None);
    }
}
