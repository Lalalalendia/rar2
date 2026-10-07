use crate::failure_intake::{
    FailureIntakeClass, FailureIntakeClassification, classify_failure_candidate,
};
use crate::family_classifier::classify_pub_family;
use crate::salvage_authority::{ReaderSalvageAuthority, typed_corruption_authority};
use pub_contents::ContentsFamily;
use pub_core::StreamPath;
use pub_escher::{
    BlipKind, BlipMetafileCompression, BlipUidRule, DelayedBlipPrefixGap,
    inspect_validated_delayed_blips_prefix, parse_officeart_stream, validate_blip_record,
};
use pub_quill::{QuillStoryReadError, parse_confirmed_story_catalog};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const READER_SALVAGE_PROBE_SCHEMA_V1: &str = "chaptera.reader-salvage-probe.v1";
pub const READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1: &str = "chaptera.reader-partial-source-graph.v1";

const READER_SALVAGE_MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
const READER_SALVAGE_MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;
const READER_SALVAGE_MAX_TOTAL_STREAM_BYTES: u64 = 128 * 1024 * 1024;

const CONTENTS_STREAM: &str = "/Contents";
const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const ESCHER_STREAM: &str = "/Escher/EscherStm";
const ESCHER_DELAY_STREAM: &str = "/Escher/EscherDelayStm";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSalvageTrigger {
    IntakeOnly,
    ProvenStructuralCorruption,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSalvageCorruptionEvidence {
    QuillDescriptorNodeTruncated,
    QuillStrsServiceSpanOutOfBounds,
    ExactShaTypedCorruptionAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSalvageEligibility {
    EligibleDamagedPublisher,
    EligibleKnownPublisherCorruption,
    AwaitingTypedCorruptionEvidence,
    IneligibleUnproven,
    IneligibleArchive,
    IneligibleForeign,
    IneligibleSuspicious,
    IneligibleResourceLimit,
}

impl ReaderSalvageEligibility {
    pub const fn is_eligible(self) -> bool {
        matches!(
            self,
            Self::EligibleDamagedPublisher | Self::EligibleKnownPublisherCorruption
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSalvageStreamState {
    NotAttempted,
    Readable,
    RecoveredRootRegular,
    Absent,
    PresentOverLimit,
    PresentUnreadable,
    ContainerUnavailable,
}

impl ReaderSalvageStreamState {
    const fn is_surviving(self) -> bool {
        matches!(self, Self::Readable | Self::RecoveredRootRegular)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderSalvageSubsystemProbe {
    pub contents: ReaderSalvageStreamState,
    pub quill: ReaderSalvageStreamState,
    pub escher: ReaderSalvageStreamState,
    pub escher_delay: ReaderSalvageStreamState,
}

impl ReaderSalvageSubsystemProbe {
    const fn not_attempted() -> Self {
        Self {
            contents: ReaderSalvageStreamState::NotAttempted,
            quill: ReaderSalvageStreamState::NotAttempted,
            escher: ReaderSalvageStreamState::NotAttempted,
            escher_delay: ReaderSalvageStreamState::NotAttempted,
        }
    }

    const fn container_unavailable() -> Self {
        Self {
            contents: ReaderSalvageStreamState::ContainerUnavailable,
            quill: ReaderSalvageStreamState::ContainerUnavailable,
            escher: ReaderSalvageStreamState::ContainerUnavailable,
            escher_delay: ReaderSalvageStreamState::ContainerUnavailable,
        }
    }

    pub const fn has_surviving_evidence(self) -> bool {
        self.contents.is_surviving()
            || self.quill.is_surviving()
            || self.escher.is_surviving()
            || self.escher_delay.is_surviving()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderSalvageProbe {
    pub schema_version: String,
    pub source_sha256: String,
    pub trigger: ReaderSalvageTrigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corruption_evidence: Option<ReaderSalvageCorruptionEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<ReaderSalvageAuthority>,
    pub eligibility: ReaderSalvageEligibility,
    pub intake: FailureIntakeClassification,
    pub reader_route: String,
    pub cfb_inventory_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents_family: Option<String>,
    pub subsystems: ReaderSalvageSubsystemProbe,
    pub source_modified: bool,
}

impl ReaderSalvageProbe {
    pub const fn has_surviving_evidence(&self) -> bool {
        self.subsystems.has_surviving_evidence()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReaderPartialSourceFact {
    TextRange {
        story_key: String,
        utf16_start: u32,
        utf16_end: u32,
        text: String,
    },
    VerifiedImage {
        resource_key: String,
        sha256: String,
        byte_len: u64,
    },
    GroundedGeometry {
        node_key: String,
        parent_key: Option<String>,
        x_emu: i64,
        y_emu: i64,
        width_emu: i64,
        height_emu: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderPartialSourceGap {
    TextUnavailable,
    TextSemanticAmbiguity,
    ImageFactsUnavailable,
    GeometryFactsUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderPartialSourceGraph {
    pub schema_version: String,
    pub source_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents_family: Option<String>,
    pub subsystems: ReaderSalvageSubsystemProbe,
    pub facts: Vec<ReaderPartialSourceFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovered_resources: Vec<ReaderPartialRecoveredResource>,
    pub gaps: Vec<ReaderPartialSourceGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderPartialPhysicalRange {
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderRecoveredResourcePlacementStatus {
    DetachedOwnershipNotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderRecoveredResourceRenderStatus {
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderRecoveredResourcePreviewStatus {
    NotProven,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderPartialRecoveredResource {
    pub resource_key: String,
    pub source_sha256: String,
    pub stream_sid: u32,
    pub logical_stream: String,
    pub record_offset: u64,
    pub record_len: u64,
    pub stored_payload_offset: u64,
    pub stored_payload_len: u64,
    pub stored_physical_ranges: Vec<ReaderPartialPhysicalRange>,
    pub kind: BlipKind,
    pub effective_uid_hex: String,
    pub uid_rule: BlipUidRule,
    pub stored_sha256: String,
    pub logical_sha256: String,
    pub logical_byte_len: u64,
    pub compression: BlipMetafileCompression,
    pub placement_status: ReaderRecoveredResourcePlacementStatus,
    pub render_status: ReaderRecoveredResourceRenderStatus,
    pub preview_status: ReaderRecoveredResourcePreviewStatus,
}

/// Public type namespace for source-bound recovered WMF/EMF resources.
pub mod recovered_resource {
    pub use super::{
        ReaderPartialEscherDelayMetafileEvidence, ReaderPartialPhysicalRange,
        ReaderPartialRecoveredResource, ReaderRecoveredResourcePlacementStatus,
        ReaderRecoveredResourcePreviewStatus, ReaderRecoveredResourceRenderStatus,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderPartialSourceGraphError {
    SourceIdentityMismatch,
    SourceModified,
    ProbeMismatch,
    Ineligible,
}

impl std::fmt::Display for ReaderPartialSourceGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ReaderPartialSourceGraphError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialEscherDelayImageEvidence {
    pub record_source: pub_core::RawSpan,
    pub payload_source: pub_core::RawSpan,
    pub payload_physical_ranges: Vec<pub_cfb::RootRegularStreamSourceRange>,
    pub kind: BlipKind,
    pub effective_uid_hex: String,
    pub uid_rule: BlipUidRule,
    pub payload_sha256: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderPartialEscherDelayDiscoveryMode {
    LogicalPath,
    UniqueRawCarrierNames,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialEscherDelayMetafileEvidence {
    pub record_source: pub_core::RawSpan,
    pub payload_source: pub_core::RawSpan,
    pub payload_physical_ranges: Vec<pub_cfb::RootRegularStreamSourceRange>,
    pub kind: BlipKind,
    pub effective_uid_hex: String,
    pub uid_rule: BlipUidRule,
    pub stored_sha256: String,
    pub stored_byte_len: u64,
    pub logical_sha256: String,
    pub logical_byte_len: u64,
    pub compression: BlipMetafileCompression,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderPartialEscherDelayEvidence {
    pub source_sha256: String,
    pub stream_sid: u32,
    pub discovery_mode: ReaderPartialEscherDelayDiscoveryMode,
    /// Requested logical target. It is a proven path only when
    /// logical_path_proven is true.
    pub logical_path: String,
    pub logical_path_proven: bool,
    pub physical_context_gap_count: usize,
    pub raw_directory_rejected_active_entry_count: usize,
    pub declared_len: u64,
    pub available_prefix_len: u64,
    pub prefix_sha256: String,
    pub physical_stream_status: pub_cfb::RootRegularStreamPrefixStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<pub_cfb::RootRegularStreamTruncationReason>,
    pub stream_source_ranges: Vec<pub_cfb::RootRegularStreamSourceRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_parser_gap: Option<DelayedBlipPrefixGap>,
    pub rejected_complete_blip_count: usize,
    pub validated_images: Vec<ReaderPartialEscherDelayImageEvidence>,
    pub validated_metafiles: Vec<ReaderPartialEscherDelayMetafileEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReaderPartialEscherDelayCarrier {
    stream_sid: u32,
    source_byte_len: u64,
    declared_len: u64,
    discovery_mode: ReaderPartialEscherDelayDiscoveryMode,
    logical_path: String,
    logical_path_proven: bool,
    physical_context_gap_count: usize,
    raw_directory_rejected_active_entry_count: usize,
}

fn discover_reader_partial_escherdelay_carrier(
    bytes: &[u8],
    source_sha256: &str,
    allow_raw_fallback: bool,
) -> Option<ReaderPartialEscherDelayCarrier> {
    if let Ok(discovered) =
        pub_cfb::discover_regular_stream_sid_reader(Cursor::new(bytes), ESCHER_DELAY_STREAM)
    {
        if discovered.source_sha256 != source_sha256
            || discovered.source_byte_len != bytes.len() as u64
            || discovered.logical_path != ESCHER_DELAY_STREAM
            || discovered.declared_len > READER_SALVAGE_MAX_STREAM_BYTES
        {
            return None;
        }
        return Some(ReaderPartialEscherDelayCarrier {
            stream_sid: discovered.stream_sid,
            source_byte_len: discovered.source_byte_len,
            declared_len: discovered.declared_len,
            discovery_mode: ReaderPartialEscherDelayDiscoveryMode::LogicalPath,
            logical_path: discovered.logical_path,
            logical_path_proven: true,
            physical_context_gap_count: 0,
            raw_directory_rejected_active_entry_count: 0,
        });
    }

    if !allow_raw_fallback {
        return None;
    }

    let inventory = pub_cfb::inspect_truncated_cfb_raw_directory_reader(Cursor::new(bytes)).ok()?;
    if inventory.source_sha256 != source_sha256 || inventory.source_byte_len != bytes.len() as u64 {
        return None;
    }

    let escher_storage_count = inventory
        .entries
        .iter()
        .filter(|entry| {
            entry.object_type == 1
                && entry
                    .descriptive_name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Escher"))
        })
        .count();
    let mut delay_streams = inventory.entries.iter().filter(|entry| {
        entry.object_type == 2
            && entry
                .descriptive_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("EscherDelayStm"))
    });
    let delay_stream = delay_streams.next()?;
    if escher_storage_count != 1
        || delay_streams.next().is_some()
        || delay_stream.declared_len > READER_SALVAGE_MAX_STREAM_BYTES
    {
        return None;
    }

    Some(ReaderPartialEscherDelayCarrier {
        stream_sid: delay_stream.sid,
        source_byte_len: inventory.source_byte_len,
        declared_len: delay_stream.declared_len,
        discovery_mode: ReaderPartialEscherDelayDiscoveryMode::UniqueRawCarrierNames,
        logical_path: ESCHER_DELAY_STREAM.to_owned(),
        logical_path_proven: false,
        physical_context_gap_count: inventory.gaps.len(),
        raw_directory_rejected_active_entry_count: inventory.rejected_active_entry_count,
    })
}

fn map_logical_span_to_physical_ranges(
    source_ranges: &[pub_cfb::RootRegularStreamSourceRange],
    span: &pub_core::RawSpan,
    available_prefix_len: u64,
) -> Option<Vec<pub_cfb::RootRegularStreamSourceRange>> {
    if span.stream != StreamPath(ESCHER_DELAY_STREAM.into()) {
        return None;
    }
    let span_end = span.offset.checked_add(span.len)?;
    if span_end > available_prefix_len {
        return None;
    }

    let mut logical_cursor = 0u64;
    let mut mapped_total = 0u64;
    let mut mapped = Vec::new();

    for range in source_ranges {
        let logical_end = logical_cursor.checked_add(range.len)?;
        let overlap_start = span.offset.max(logical_cursor);
        let overlap_end = span_end.min(logical_end);
        if overlap_start < overlap_end {
            let within_range = overlap_start.checked_sub(logical_cursor)?;
            let physical_offset = range.offset.checked_add(within_range)?;
            let len = overlap_end.checked_sub(overlap_start)?;
            mapped.push(pub_cfb::RootRegularStreamSourceRange {
                offset: physical_offset,
                len,
            });
            mapped_total = mapped_total.checked_add(len)?;
        }
        logical_cursor = logical_end;
        if logical_cursor >= span_end {
            break;
        }
    }

    (mapped_total == span.len).then_some(mapped)
}

fn sha256_physical_ranges(
    source: &[u8],
    ranges: &[pub_cfb::RootRegularStreamSourceRange],
) -> Option<String> {
    let mut digest = Sha256::new();
    for range in ranges {
        let start = usize::try_from(range.offset).ok()?;
        let len = usize::try_from(range.len).ok()?;
        let end = start.checked_add(len)?;
        digest.update(source.get(start..end)?);
    }
    Some(
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

/// Builds strict image evidence from a physically proven SID-bound
/// EscherDelay prefix when normal CFB access cannot provide the stream.
///
/// Stream discovery and physical recovery are independently hash/SID-bound.
/// Only complete BLIP records accepted by the strict prefix validator can
/// become Reader image facts. No BStore/page/shape/layout ownership is inferred.
pub fn build_reader_partial_escherdelay_evidence(
    bytes: &[u8],
    probe: &ReaderSalvageProbe,
) -> Option<ReaderPartialEscherDelayEvidence> {
    if !probe.eligibility.is_eligible()
        || probe.subsystems.escher_delay == ReaderSalvageStreamState::Readable
        || probe.source_sha256 != source_sha256(bytes)
        || bytes.len() > READER_SALVAGE_MAX_INPUT_BYTES
    {
        return None;
    }

    let discovered = discover_reader_partial_escherdelay_carrier(
        bytes,
        &probe.source_sha256,
        probe.subsystems.escher_delay == ReaderSalvageStreamState::ContainerUnavailable,
    )?;

    let recovered = match discovered.discovery_mode {
        ReaderPartialEscherDelayDiscoveryMode::LogicalPath => {
            pub_cfb::recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
                Cursor::new(bytes),
                discovered.stream_sid,
                &probe.source_sha256,
            )
            .ok()?
        }
        ReaderPartialEscherDelayDiscoveryMode::UniqueRawCarrierNames => {
            pub_cfb::recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
                Cursor::new(bytes),
                discovered.stream_sid,
                &probe.source_sha256,
            )
            .ok()?
        }
    };
    if recovered.source_modified
        || recovered.stream_sid != discovered.stream_sid
        || recovered.source_byte_len != discovered.source_byte_len
        || recovered.declared_len != discovered.declared_len
        || recovered.available_prefix_len > READER_SALVAGE_MAX_STREAM_BYTES
        || recovered.bytes.len() as u64 != recovered.available_prefix_len
    {
        return None;
    }

    let inventory = inspect_validated_delayed_blips_prefix(
        StreamPath(ESCHER_DELAY_STREAM.into()),
        &recovered.bytes,
    );
    if inventory.available_prefix_len != recovered.available_prefix_len {
        return None;
    }

    let terminal_parser_gap = inventory.terminal_gap.clone();
    let rejected_complete_blip_count = inventory.rejected_complete_blips.len();
    let mut validated_images = Vec::new();
    let mut validated_metafiles = Vec::new();
    for validated in inventory.records {
        if reader_partial_image_kind_admitted_v1(validated.kind) {
            let payload_physical_ranges = map_logical_span_to_physical_ranges(
                &recovered.source_ranges,
                &validated.payload_source,
                recovered.available_prefix_len,
            )?;
            if sha256_physical_ranges(bytes, &payload_physical_ranges).as_deref()
                != Some(validated.payload_sha256.as_str())
            {
                return None;
            }

            let byte_len = validated.payload_source.len;
            validated_images.push(ReaderPartialEscherDelayImageEvidence {
                record_source: validated.record_source,
                payload_source: validated.payload_source,
                payload_physical_ranges,
                kind: validated.kind,
                effective_uid_hex: validated
                    .effective_uid
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                uid_rule: validated.uid_rule,
                payload_sha256: validated.payload_sha256,
                byte_len,
            });
            continue;
        }

        if reader_partial_metafile_kind_admitted_v1(validated.kind) {
            let payload_physical_ranges = map_logical_span_to_physical_ranges(
                &recovered.source_ranges,
                &validated.payload_source,
                recovered.available_prefix_len,
            )?;
            if sha256_physical_ranges(bytes, &payload_physical_ranges).as_deref()
                != Some(validated.payload_sha256.as_str())
            {
                return None;
            }
            let logical_sha256 = validated.logical_payload_sha256?;
            let logical_byte_len = validated.logical_payload_len?;
            let compression = validated.metafile_compression?;
            validated_metafiles.push(ReaderPartialEscherDelayMetafileEvidence {
                record_source: validated.record_source,
                payload_source: validated.payload_source.clone(),
                payload_physical_ranges,
                kind: validated.kind,
                effective_uid_hex: validated
                    .effective_uid
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                uid_rule: validated.uid_rule,
                stored_sha256: validated.payload_sha256,
                stored_byte_len: validated.payload_source.len,
                logical_sha256,
                logical_byte_len,
                compression,
            });
        }
    }

    Some(ReaderPartialEscherDelayEvidence {
        source_sha256: probe.source_sha256.clone(),
        stream_sid: discovered.stream_sid,
        discovery_mode: discovered.discovery_mode,
        logical_path: discovered.logical_path,
        logical_path_proven: discovered.logical_path_proven,
        physical_context_gap_count: discovered.physical_context_gap_count,
        raw_directory_rejected_active_entry_count: discovered
            .raw_directory_rejected_active_entry_count,
        declared_len: recovered.declared_len,
        available_prefix_len: recovered.available_prefix_len,
        prefix_sha256: recovered.prefix_sha256,
        physical_stream_status: recovered.status,
        truncation_reason: recovered.truncation_reason,
        stream_source_ranges: recovered.source_ranges,
        terminal_parser_gap,
        rejected_complete_blip_count,
        validated_images,
        validated_metafiles,
    })
}

fn reader_partial_image_kind_admitted_v1(kind: BlipKind) -> bool {
    matches!(
        kind,
        BlipKind::Jpeg | BlipKind::Png | BlipKind::Gif | BlipKind::Dib | BlipKind::Tiff
    )
}

fn reader_partial_metafile_kind_admitted_v1(kind: BlipKind) -> bool {
    matches!(kind, BlipKind::Emf | BlipKind::Wmf)
}

pub fn build_reader_partial_source_graph(
    bytes: &[u8],
    probe: &ReaderSalvageProbe,
) -> Result<ReaderPartialSourceGraph, ReaderPartialSourceGraphError> {
    if probe.source_sha256 != source_sha256(bytes) {
        return Err(ReaderPartialSourceGraphError::SourceIdentityMismatch);
    }
    if probe.source_modified {
        return Err(ReaderPartialSourceGraphError::SourceModified);
    }
    let replay_trigger = if probe.corruption_evidence.is_some() {
        ReaderSalvageTrigger::IntakeOnly
    } else {
        probe.trigger
    };
    let current_probe = probe_reader_salvage_candidate_with_trigger(bytes, replay_trigger);
    if current_probe != *probe {
        return Err(ReaderPartialSourceGraphError::ProbeMismatch);
    }
    if !probe.eligibility.is_eligible() {
        return Err(ReaderPartialSourceGraphError::Ineligible);
    }

    let existing_survival = probe.has_surviving_evidence();
    let mut facts = Vec::new();
    let mut recovered_resources = Vec::new();
    let mut gaps = Vec::new();

    if probe.subsystems.quill == ReaderSalvageStreamState::Readable {
        match pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
            .ok()
            .and_then(|quill| {
                parse_confirmed_story_catalog(StreamPath(QUILL_STREAM.into()), &quill).ok()
            }) {
            Some(catalog) => {
                for story in catalog.stories {
                    let mut units = Vec::with_capacity(story.utf16le.len() / 2);
                    let mut chunks = story.utf16le.chunks_exact(2);
                    units.extend(
                        chunks
                            .by_ref()
                            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
                    );
                    if !chunks.remainder().is_empty() {
                        gaps.push(ReaderPartialSourceGap::TextSemanticAmbiguity);
                        continue;
                    }
                    let Ok(text) = String::from_utf16(&units) else {
                        gaps.push(ReaderPartialSourceGap::TextSemanticAmbiguity);
                        continue;
                    };
                    facts.push(ReaderPartialSourceFact::TextRange {
                        story_key: format!("quill-syid:{:08x}", story.syid.0),
                        utf16_start: 0,
                        utf16_end: story.utf16_code_units,
                        text,
                    });
                }
            }
            None => gaps.push(ReaderPartialSourceGap::TextSemanticAmbiguity),
        }
    } else {
        gaps.push(ReaderPartialSourceGap::TextUnavailable);
    }

    let mut verified_image_count = 0_usize;
    if probe.subsystems.escher_delay == ReaderSalvageStreamState::Readable
        && let Ok(delay) = pub_cfb::read_stream_reader(Cursor::new(bytes), ESCHER_DELAY_STREAM)
        && u64::try_from(delay.len())
            .ok()
            .is_some_and(|len| len <= READER_SALVAGE_MAX_STREAM_BYTES)
        && let Ok(parsed) = parse_officeart_stream(StreamPath(ESCHER_DELAY_STREAM.into()), &delay)
    {
        for (ordinal, record) in parsed.records.iter().enumerate() {
            let Ok(validated) = validate_blip_record(&delay, record) else {
                continue;
            };
            if !reader_partial_image_kind_admitted_v1(validated.kind) {
                continue;
            }
            facts.push(ReaderPartialSourceFact::VerifiedImage {
                resource_key: format!("escher-delay:{ordinal}:{}", validated.payload_sha256),
                sha256: validated.payload_sha256,
                byte_len: validated.payload_source.len,
            });
            verified_image_count += 1;
        }
    }
    if probe.subsystems.escher_delay != ReaderSalvageStreamState::Readable
        && let Some(evidence) = build_reader_partial_escherdelay_evidence(bytes, probe)
    {
        let stream_sid = evidence.stream_sid;
        for image in evidence.validated_images {
            if !reader_partial_image_kind_admitted_v1(image.kind) {
                continue;
            }
            facts.push(ReaderPartialSourceFact::VerifiedImage {
                resource_key: format!(
                    "escher-delay:{}:{}:{}:{}",
                    probe.source_sha256,
                    stream_sid,
                    image.record_source.offset,
                    image.payload_sha256
                ),
                sha256: image.payload_sha256,
                byte_len: image.byte_len,
            });
            verified_image_count += 1;
        }
        for metafile in evidence.validated_metafiles {
            recovered_resources.push(ReaderPartialRecoveredResource {
                resource_key: format!(
                    "escher-delay-metafile:{}:{}:{}:{}",
                    probe.source_sha256,
                    stream_sid,
                    metafile.record_source.offset,
                    metafile.stored_sha256
                ),
                source_sha256: probe.source_sha256.clone(),
                stream_sid,
                logical_stream: ESCHER_DELAY_STREAM.to_owned(),
                record_offset: metafile.record_source.offset,
                record_len: metafile.record_source.len,
                stored_payload_offset: metafile.payload_source.offset,
                stored_payload_len: metafile.stored_byte_len,
                stored_physical_ranges: metafile
                    .payload_physical_ranges
                    .into_iter()
                    .map(|range| ReaderPartialPhysicalRange {
                        offset: range.offset,
                        len: range.len,
                    })
                    .collect(),
                kind: metafile.kind,
                effective_uid_hex: metafile.effective_uid_hex,
                uid_rule: metafile.uid_rule,
                stored_sha256: metafile.stored_sha256,
                logical_sha256: metafile.logical_sha256,
                logical_byte_len: metafile.logical_byte_len,
                compression: metafile.compression,
                placement_status:
                    ReaderRecoveredResourcePlacementStatus::DetachedOwnershipNotProven,
                render_status: ReaderRecoveredResourceRenderStatus::NotProven,
                preview_status: ReaderRecoveredResourcePreviewStatus::NotProven,
            });
        }
    }
    if verified_image_count == 0 {
        gaps.push(ReaderPartialSourceGap::ImageFactsUnavailable);
    }

    if !existing_survival && verified_image_count == 0 && recovered_resources.is_empty() {
        return Err(ReaderPartialSourceGraphError::Ineligible);
    }

    // Stream survival alone is not enough to assign source-neutral page/object
    // identity or bounds. Keep geometry absent until that join is independently
    // grounded rather than promoting raw OfficeArt coordinates.
    gaps.push(ReaderPartialSourceGap::GeometryFactsUnavailable);
    gaps.sort_by_key(|gap| match gap {
        ReaderPartialSourceGap::TextUnavailable => 0,
        ReaderPartialSourceGap::TextSemanticAmbiguity => 1,
        ReaderPartialSourceGap::ImageFactsUnavailable => 2,
        ReaderPartialSourceGap::GeometryFactsUnavailable => 3,
    });
    gaps.dedup();

    Ok(ReaderPartialSourceGraph {
        schema_version: READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1.to_owned(),
        source_sha256: probe.source_sha256.clone(),
        contents_family: probe.contents_family.clone(),
        subsystems: probe.subsystems,
        facts,
        recovered_resources,
        gaps,
    })
}

pub fn probe_reader_salvage_candidate(bytes: &[u8]) -> ReaderSalvageProbe {
    probe_reader_salvage_candidate_with_trigger(bytes, ReaderSalvageTrigger::IntakeOnly)
}

pub fn probe_reader_salvage_candidate_with_trigger(
    bytes: &[u8],
    trigger: ReaderSalvageTrigger,
) -> ReaderSalvageProbe {
    let source_sha256 = source_sha256(bytes);
    let intake = classify_failure_candidate(bytes);
    let family = classify_pub_family(bytes);
    let (corruption_evidence, authority) = if bytes.len() <= READER_SALVAGE_MAX_INPUT_BYTES
        && intake.class == FailureIntakeClass::PubHighValue
        && trigger == ReaderSalvageTrigger::IntakeOnly
    {
        match detect_known_structural_corruption(bytes) {
            Some(evidence) => (Some(evidence), None),
            None => {
                let authority = typed_corruption_authority(&source_sha256);
                let evidence = authority
                    .as_ref()
                    .map(|_| ReaderSalvageCorruptionEvidence::ExactShaTypedCorruptionAuthority);
                (evidence, authority)
            }
        }
    } else {
        (None, None)
    };
    let effective_trigger = if corruption_evidence.is_some() {
        ReaderSalvageTrigger::ProvenStructuralCorruption
    } else {
        trigger
    };
    let eligibility = salvage_eligibility(bytes.len(), intake.class, effective_trigger);

    if !eligibility.is_eligible() {
        return ReaderSalvageProbe {
            schema_version: READER_SALVAGE_PROBE_SCHEMA_V1.to_owned(),
            source_sha256: source_sha256.clone(),
            trigger: effective_trigger,
            corruption_evidence,
            authority,
            eligibility,
            intake,
            reader_route: family.route.as_str().to_owned(),
            cfb_inventory_available: false,
            contents_family: None,
            subsystems: ReaderSalvageSubsystemProbe::not_attempted(),
            source_modified: false,
        };
    }

    match pub_cfb::inspect_reader(Cursor::new(bytes)) {
        Ok(inventory) => {
            let mut remaining_budget = READER_SALVAGE_MAX_TOTAL_STREAM_BYTES;
            let (contents, contents_bytes) =
                probe_inventory_stream(bytes, &inventory, CONTENTS_STREAM, &mut remaining_budget);
            let contents_family = contents_bytes
                .as_deref()
                .and_then(|contents| pub_contents::detect_family(contents).ok())
                .map(contents_family_name);
            let (quill, _) =
                probe_inventory_stream(bytes, &inventory, QUILL_STREAM, &mut remaining_budget);
            let (escher, _) =
                probe_inventory_stream(bytes, &inventory, ESCHER_STREAM, &mut remaining_budget);
            let (escher_delay, _) = probe_inventory_stream(
                bytes,
                &inventory,
                ESCHER_DELAY_STREAM,
                &mut remaining_budget,
            );

            ReaderSalvageProbe {
                schema_version: READER_SALVAGE_PROBE_SCHEMA_V1.to_owned(),
                source_sha256: source_sha256.clone(),
                trigger: effective_trigger,
                corruption_evidence,
                authority,
                eligibility,
                intake,
                reader_route: family.route.as_str().to_owned(),
                cfb_inventory_available: true,
                contents_family,
                subsystems: ReaderSalvageSubsystemProbe {
                    contents,
                    quill,
                    escher,
                    escher_delay,
                },
                source_modified: false,
            }
        }
        Err(_) => {
            let mut subsystems = ReaderSalvageSubsystemProbe::container_unavailable();
            let mut contents_family = None;

            if let Ok(recovered) =
                pub_cfb::recover_root_regular_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
            {
                if u64::try_from(recovered.bytes.len())
                    .ok()
                    .is_some_and(|len| len <= READER_SALVAGE_MAX_STREAM_BYTES)
                {
                    contents_family = pub_contents::detect_family(&recovered.bytes)
                        .ok()
                        .map(contents_family_name);
                    subsystems.contents = ReaderSalvageStreamState::RecoveredRootRegular;
                } else {
                    subsystems.contents = ReaderSalvageStreamState::PresentOverLimit;
                }
            }

            ReaderSalvageProbe {
                schema_version: READER_SALVAGE_PROBE_SCHEMA_V1.to_owned(),
                source_sha256: source_sha256.clone(),
                trigger: effective_trigger,
                corruption_evidence,
                authority,
                eligibility,
                intake,
                reader_route: family.route.as_str().to_owned(),
                cfb_inventory_available: false,
                contents_family,
                subsystems,
                source_modified: false,
            }
        }
    }
}

fn detect_known_structural_corruption(bytes: &[u8]) -> Option<ReaderSalvageCorruptionEvidence> {
    let inventory = pub_cfb::inspect_reader(Cursor::new(bytes)).ok()?;
    let entry = inventory
        .entries
        .iter()
        .find(|entry| entry.path == QUILL_STREAM)?;
    if entry.len > READER_SALVAGE_MAX_STREAM_BYTES {
        return None;
    }

    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM).ok()?;
    let quill_len = u64::try_from(quill.len()).ok()?;
    if quill_len > READER_SALVAGE_MAX_STREAM_BYTES {
        return None;
    }

    let error = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM.into()), &quill).err()?;
    known_quill_corruption_evidence(&error)
}

fn known_quill_corruption_evidence(
    error: &QuillStoryReadError,
) -> Option<ReaderSalvageCorruptionEvidence> {
    match error {
        QuillStoryReadError::DescriptorNodeTruncated { .. } => {
            Some(ReaderSalvageCorruptionEvidence::QuillDescriptorNodeTruncated)
        }
        QuillStoryReadError::StrsServiceSpanOutOfBounds { .. } => {
            Some(ReaderSalvageCorruptionEvidence::QuillStrsServiceSpanOutOfBounds)
        }
        _ => None,
    }
}

fn salvage_eligibility(
    byte_len: usize,
    class: FailureIntakeClass,
    trigger: ReaderSalvageTrigger,
) -> ReaderSalvageEligibility {
    if byte_len > READER_SALVAGE_MAX_INPUT_BYTES {
        return ReaderSalvageEligibility::IneligibleResourceLimit;
    }

    match class {
        FailureIntakeClass::PubDamaged => ReaderSalvageEligibility::EligibleDamagedPublisher,
        FailureIntakeClass::PubHighValue => match trigger {
            ReaderSalvageTrigger::IntakeOnly => {
                ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence
            }
            ReaderSalvageTrigger::ProvenStructuralCorruption => {
                ReaderSalvageEligibility::EligibleKnownPublisherCorruption
            }
        },
        FailureIntakeClass::PubPossible => ReaderSalvageEligibility::IneligibleUnproven,
        FailureIntakeClass::ArchiveWithPub => ReaderSalvageEligibility::IneligibleArchive,
        FailureIntakeClass::NotPub => ReaderSalvageEligibility::IneligibleForeign,
        FailureIntakeClass::SuspiciousPolyglot => ReaderSalvageEligibility::IneligibleSuspicious,
    }
}

fn probe_inventory_stream(
    bytes: &[u8],
    inventory: &pub_cfb::CfbInventory,
    path: &str,
    remaining_budget: &mut u64,
) -> (ReaderSalvageStreamState, Option<Vec<u8>>) {
    let Some(entry) = inventory.entries.iter().find(|entry| entry.path == path) else {
        return (ReaderSalvageStreamState::Absent, None);
    };

    if entry.len > READER_SALVAGE_MAX_STREAM_BYTES || entry.len > *remaining_budget {
        return (ReaderSalvageStreamState::PresentOverLimit, None);
    }

    match pub_cfb::read_stream_reader(Cursor::new(bytes), path) {
        Ok(stream) => {
            let actual = u64::try_from(stream.len()).unwrap_or(u64::MAX);
            if actual > READER_SALVAGE_MAX_STREAM_BYTES || actual > *remaining_budget {
                return (ReaderSalvageStreamState::PresentOverLimit, None);
            }
            *remaining_budget = remaining_budget.saturating_sub(actual);
            (ReaderSalvageStreamState::Readable, Some(stream))
        }
        Err(_) => (ReaderSalvageStreamState::PresentUnreadable, None),
    }
}

fn source_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn contents_family_name(family: ContentsFamily) -> String {
    match family {
        ContentsFamily::Family0x22 => "0x22",
        ContentsFamily::Family0x2c => "0x2c",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn partial_image_product_admission_remains_raster_only_v1() {
        for kind in [
            BlipKind::Jpeg,
            BlipKind::Png,
            BlipKind::Gif,
            BlipKind::Dib,
            BlipKind::Tiff,
        ] {
            assert!(reader_partial_image_kind_admitted_v1(kind));
        }
        for kind in [
            BlipKind::Emf,
            BlipKind::Wmf,
            BlipKind::Pict,
            BlipKind::Unknown,
        ] {
            assert!(!reader_partial_image_kind_admitted_v1(kind));
        }
    }

    #[test]
    fn logical_payload_span_maps_across_physical_ranges() {
        let ranges = vec![
            pub_cfb::RootRegularStreamSourceRange {
                offset: 1024,
                len: 8,
            },
            pub_cfb::RootRegularStreamSourceRange {
                offset: 4096,
                len: 8,
            },
        ];
        let span = pub_core::RawSpan {
            stream: StreamPath(ESCHER_DELAY_STREAM.into()),
            offset: 6,
            len: 6,
        };
        let mapped = map_logical_span_to_physical_ranges(&ranges, &span, 16)
            .expect("logical span must map exactly");
        assert_eq!(
            mapped,
            vec![
                pub_cfb::RootRegularStreamSourceRange {
                    offset: 1030,
                    len: 2,
                },
                pub_cfb::RootRegularStreamSourceRange {
                    offset: 4096,
                    len: 4,
                },
            ]
        );
    }

    #[test]
    fn logical_payload_span_from_wrong_stream_fails_closed() {
        let ranges = vec![pub_cfb::RootRegularStreamSourceRange {
            offset: 1024,
            len: 16,
        }];
        let span = pub_core::RawSpan {
            stream: StreamPath("/Escher/EscherStm".into()),
            offset: 0,
            len: 8,
        };
        assert!(map_logical_span_to_physical_ranges(&ranges, &span, 16).is_none());
    }

    #[test]
    fn logical_payload_span_crossing_missing_tail_fails_closed() {
        let ranges = vec![pub_cfb::RootRegularStreamSourceRange {
            offset: 1024,
            len: 8,
        }];
        let span = pub_core::RawSpan {
            stream: StreamPath(ESCHER_DELAY_STREAM.into()),
            offset: 6,
            len: 4,
        };
        assert!(map_logical_span_to_physical_ranges(&ranges, &span, 8).is_none());
    }

    fn synthetic_pub_cfb() -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("synthetic Publisher CFB");
        compound
            .create_storage("/Objects")
            .expect("Objects storage");
        compound
            .create_stream("/Objects/Damaged")
            .expect("small mini stream")
            .write_all(b"small")
            .expect("write mini stream");

        let mut contents = vec![0_u8; 5_000];
        contents[..4].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00]);
        compound
            .create_stream(CONTENTS_STREAM)
            .expect("Contents stream")
            .write_all(&contents)
            .expect("write Contents");
        compound.flush().expect("flush synthetic CFB");
        compound.into_inner().into_inner()
    }

    fn synthetic_pub_cfb_with_delay_png() -> Vec<u8> {
        synthetic_pub_cfb_with_delay_png_uid(true)
    }

    fn synthetic_pub_cfb_with_delay_png_uid(valid_uid: bool) -> Vec<u8> {
        synthetic_pub_cfb_with_delay_resources(valid_uid, true)
    }

    fn synthetic_pub_cfb_with_delay_metafiles_only() -> Vec<u8> {
        synthetic_pub_cfb_with_delay_resources(true, false)
    }

    fn synthetic_pub_cfb_with_delay_resources(valid_uid: bool, include_png: bool) -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("synthetic Publisher CFB");
        compound.create_storage("/Escher").expect("Escher storage");
        compound
            .create_storage("/Objects")
            .expect("Objects storage");
        compound
            .create_stream("/Objects/Damaged")
            .expect("small mini stream")
            .write_all(b"small")
            .expect("write mini stream");

        let mut contents = vec![0_u8; 5_000];
        contents[..4].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00]);
        compound
            .create_stream(CONTENTS_STREAM)
            .expect("Contents stream")
            .write_all(&contents)
            .expect("write Contents");

        use md4::{Digest, Md4};

        let mut record = Vec::new();
        if include_png {
            let mut image = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
            image.extend_from_slice(b"salvage-image");
            let digest = Md4::digest(&image);
            let mut uid = [0u8; 16];
            uid.copy_from_slice(&digest);
            if !valid_uid {
                uid[0] ^= 0x5a;
            }

            let mut payload = Vec::with_capacity(17 + image.len());
            payload.extend_from_slice(&uid);
            payload.push(0xff);
            payload.extend_from_slice(&image);
            record.extend_from_slice(&0x6e00u16.to_le_bytes());
            record.extend_from_slice(&pub_escher::OFFICE_ART_BLIP_PNG.to_le_bytes());
            record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            record.extend_from_slice(&payload);
        }

        // Include strict uncompressed + DEFLATE EMF/WMF resources in the same
        // recovered prefix. The product projection must preserve stored source
        // identity separately from logical/uncompressed identity.
        let append_metafile = |record: &mut Vec<u8>,
                               rec_type: u16,
                               rec_instance: u16,
                               logical: &[u8],
                               stored: &[u8],
                               compression: u8| {
            let metafile_uid = Md4::digest(logical);
            let mut metafile_payload = Vec::with_capacity(50 + stored.len());
            metafile_payload.extend_from_slice(&metafile_uid);
            metafile_payload.extend_from_slice(&(logical.len() as u32).to_le_bytes());
            metafile_payload.extend_from_slice(&[0u8; 16]); // rcBounds
            metafile_payload.extend_from_slice(&[0u8; 8]); // ptSize
            metafile_payload.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            metafile_payload.push(compression);
            metafile_payload.push(0xfe); // filter
            metafile_payload.extend_from_slice(stored);
            record.extend_from_slice(&(rec_instance << 4).to_le_bytes());
            record.extend_from_slice(&rec_type.to_le_bytes());
            record.extend_from_slice(&(metafile_payload.len() as u32).to_le_bytes());
            record.extend_from_slice(&metafile_payload);
        };

        let emf = b"synthetic-emf-resource";
        append_metafile(
            &mut record,
            pub_escher::OFFICE_ART_BLIP_EMF,
            0x03d4,
            emf,
            emf,
            0xfe,
        );
        let wmf = b"synthetic-wmf-resource";
        append_metafile(
            &mut record,
            pub_escher::OFFICE_ART_BLIP_WMF,
            0x0216,
            wmf,
            wmf,
            0xfe,
        );

        let emf_deflate_logical = b"synthetic-emf-deflate";
        let emf_deflate_stored: &[u8] = &[
            0x78, 0x9c, 0x2b, 0xae, 0xcc, 0x2b, 0xc9, 0x48, 0x2d, 0xc9, 0x4c, 0xd6, 0x4d, 0xcd,
            0x4d, 0xd3, 0x4d, 0x49, 0x4d, 0xcb, 0x49, 0x2c, 0x49, 0x05, 0x00, 0x5c, 0xfe, 0x08,
            0x43,
        ];
        append_metafile(
            &mut record,
            pub_escher::OFFICE_ART_BLIP_EMF,
            0x03d4,
            emf_deflate_logical,
            emf_deflate_stored,
            0x00,
        );

        let wmf_deflate_logical = b"synthetic-wmf-deflate";
        let wmf_deflate_stored: &[u8] = &[
            0x78, 0x9c, 0x2b, 0xae, 0xcc, 0x2b, 0xc9, 0x48, 0x2d, 0xc9, 0x4c, 0xd6, 0x2d, 0xcf,
            0x4d, 0xd3, 0x4d, 0x49, 0x4d, 0xcb, 0x49, 0x2c, 0x49, 0x05, 0x00, 0x5d, 0xc4, 0x08,
            0x55,
        ];
        append_metafile(
            &mut record,
            pub_escher::OFFICE_ART_BLIP_WMF,
            0x0216,
            wmf_deflate_logical,
            wmf_deflate_stored,
            0x00,
        );

        // Keep EscherDelay on the regular FAT path so the damaged-CFB
        // recovery arm exercises the exact-SID regular-stream substrate.
        record.extend_from_slice(&0x0000u16.to_le_bytes());
        record.extend_from_slice(&0xf122u16.to_le_bytes());
        record.extend_from_slice(&4096u32.to_le_bytes());
        record.extend_from_slice(&[0u8; 4096]);

        compound
            .create_stream(ESCHER_DELAY_STREAM)
            .expect("Escher delay stream")
            .write_all(&record)
            .expect("write Escher delay");

        compound.flush().expect("flush synthetic CFB");
        compound.into_inner().into_inner()
    }

    fn corrupt_contents_start_sector(mut bytes: Vec<u8>) -> Vec<u8> {
        let mut marker = Vec::new();
        for unit in "Contents".encode_utf16() {
            marker.extend_from_slice(&unit.to_le_bytes());
        }
        let offset = bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("Contents directory entry");
        assert_eq!(
            offset % 128,
            0,
            "Contents marker must begin a directory entry"
        );
        bytes[offset + 116..offset + 120].copy_from_slice(&0xffff_fffau32.to_le_bytes());
        bytes
    }

    fn corrupt_first_minifat_entry(mut bytes: Vec<u8>) -> Vec<u8> {
        let sector_shift = u16::from_le_bytes([bytes[30], bytes[31]]);
        let sector_len = 1usize << sector_shift;
        let minifat_sector = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
        let offset = (minifat_sector as usize + 1) * sector_len;
        bytes[offset..offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes
    }

    fn synthetic_pub_cfb_with_duplicate_delay_names() -> Vec<u8> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("duplicate carrier fixture");
        compound.create_storage("/Escher").expect("Escher storage");
        compound.create_storage("/Other").expect("Other storage");
        compound
            .create_stream("/Escher/EscherDelayStm")
            .expect("primary delay stream")
            .write_all(&vec![0x11; 5_000])
            .expect("primary delay bytes");
        compound
            .create_stream("/Other/EscherDelayStm")
            .expect("duplicate delay stream")
            .write_all(&vec![0x22; 5_000])
            .expect("duplicate delay bytes");

        let mut contents = vec![0_u8; 5_000];
        contents[..4].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00]);
        compound
            .create_stream(CONTENTS_STREAM)
            .expect("Contents stream")
            .write_all(&contents)
            .expect("write Contents");
        compound.flush().expect("flush duplicate carrier fixture");
        compound.into_inner().into_inner()
    }

    #[test]
    fn truncated_cfb_raw_carrier_fallback_marks_logical_path_unproven() {
        let mut bytes = synthetic_pub_cfb_with_delay_png();
        bytes.extend_from_slice(&[0xaa; 37]);
        let source_sha = source_sha256(&bytes);

        assert!(
            pub_cfb::discover_regular_stream_sid_reader(Cursor::new(&bytes), ESCHER_DELAY_STREAM,)
                .is_err(),
            "strict logical-path discovery must reject the truncated container first"
        );

        let carrier = discover_reader_partial_escherdelay_carrier(&bytes, &source_sha, true)
            .expect("unique raw carrier fallback");
        assert_eq!(
            carrier.discovery_mode,
            ReaderPartialEscherDelayDiscoveryMode::UniqueRawCarrierNames
        );
        assert!(!carrier.logical_path_proven);
        assert_eq!(carrier.logical_path, ESCHER_DELAY_STREAM);
        assert!(carrier.physical_context_gap_count > 0);
    }

    #[test]
    fn truncated_cfb_raw_carrier_fallback_rejects_duplicate_delay_names() {
        let mut bytes = synthetic_pub_cfb_with_duplicate_delay_names();
        bytes.extend_from_slice(&[0xaa; 37]);
        let source_sha = source_sha256(&bytes);

        assert!(
            pub_cfb::discover_regular_stream_sid_reader(Cursor::new(&bytes), ESCHER_DELAY_STREAM,)
                .is_err()
        );
        assert!(
            discover_reader_partial_escherdelay_carrier(&bytes, &source_sha, true).is_none(),
            "raw-name fallback must fail closed when EscherDelayStm is not unique"
        );
    }

    #[test]
    fn raw_carrier_fallback_requires_explicit_container_unavailable_gate() {
        let mut bytes = synthetic_pub_cfb_with_delay_png();
        bytes.extend_from_slice(&[0xaa; 37]);
        let source_sha = source_sha256(&bytes);

        assert!(
            discover_reader_partial_escherdelay_carrier(&bytes, &source_sha, false).is_none(),
            "raw-name carrier discovery must not activate outside the explicit container-unavailable path"
        );
    }

    #[test]
    fn damaged_cfb_recovers_sid_bound_delay_image_into_partial_graph() {
        let bytes = corrupt_first_minifat_entry(synthetic_pub_cfb_with_delay_png());
        assert!(pub_cfb::inspect_reader(Cursor::new(bytes.clone())).is_err());

        let probe = probe_reader_salvage_candidate(&bytes);
        assert_eq!(probe.intake.class, FailureIntakeClass::PubDamaged);
        assert_eq!(
            probe.eligibility,
            ReaderSalvageEligibility::EligibleDamagedPublisher
        );
        assert!(!probe.cfb_inventory_available);
        assert_eq!(
            probe.subsystems.escher_delay,
            ReaderSalvageStreamState::ContainerUnavailable
        );

        let evidence =
            build_reader_partial_escherdelay_evidence(&bytes, &probe).expect("delay evidence");
        assert_eq!(
            evidence.rejected_complete_blip_count, 0,
            "strict EMF must validate upstream rather than being rejected"
        );
        assert_eq!(
            evidence.validated_images.len(),
            1,
            "strict metafiles must not leak into raster-shaped Reader evidence"
        );
        assert_eq!(evidence.validated_metafiles.len(), 4);
        assert!(evidence.validated_metafiles.iter().any(|value| {
            value.kind == BlipKind::Emf
                && value.compression == BlipMetafileCompression::Uncompressed
        }));
        assert!(evidence.validated_metafiles.iter().any(|value| {
            value.kind == BlipKind::Emf && value.compression == BlipMetafileCompression::Deflate
        }));
        assert!(evidence.validated_metafiles.iter().any(|value| {
            value.kind == BlipKind::Wmf
                && value.compression == BlipMetafileCompression::Uncompressed
        }));
        assert!(evidence.validated_metafiles.iter().any(|value| {
            value.kind == BlipKind::Wmf && value.compression == BlipMetafileCompression::Deflate
        }));
        assert_eq!(evidence.source_sha256, source_sha256(&bytes));
        assert!(!evidence.stream_source_ranges.is_empty());
        let image = &evidence.validated_images[0];
        assert_eq!(image.kind, BlipKind::Png);
        assert!(!image.payload_physical_ranges.is_empty());
        assert_eq!(image.byte_len, 21);

        let graph = build_reader_partial_source_graph(&bytes, &probe).expect("partial graph");
        let verified = graph
            .facts
            .iter()
            .filter_map(|fact| match fact {
                ReaderPartialSourceFact::VerifiedImage {
                    sha256, byte_len, ..
                } => Some((sha256.as_str(), *byte_len)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].0, image.payload_sha256);
        assert_eq!(verified[0].1, image.byte_len);
        assert_eq!(graph.recovered_resources.len(), 4);
        assert!(graph.recovered_resources.iter().all(|resource| {
            resource.source_sha256 == source_sha256(&bytes)
                && resource.stream_sid == evidence.stream_sid
                && !resource.stored_physical_ranges.is_empty()
                && resource.stored_sha256.len() == 64
                && resource.logical_sha256.len() == 64
                && matches!(
                    resource.placement_status,
                    ReaderRecoveredResourcePlacementStatus::DetachedOwnershipNotProven
                )
                && matches!(
                    resource.render_status,
                    ReaderRecoveredResourceRenderStatus::NotProven
                )
                && matches!(
                    resource.preview_status,
                    ReaderRecoveredResourcePreviewStatus::NotProven
                )
        }));
        assert!(
            !graph
                .gaps
                .contains(&ReaderPartialSourceGap::ImageFactsUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::TextUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::GeometryFactsUnavailable)
        );
    }

    #[test]
    fn damaged_cfb_metafile_only_survival_is_product_useful_without_verified_image() {
        let bytes = corrupt_contents_start_sector(corrupt_first_minifat_entry(
            synthetic_pub_cfb_with_delay_metafiles_only(),
        ));
        assert!(pub_cfb::inspect_reader(Cursor::new(bytes.clone())).is_err());

        let probe = probe_reader_salvage_candidate(&bytes);
        assert_eq!(
            probe.eligibility,
            ReaderSalvageEligibility::EligibleDamagedPublisher
        );
        assert!(!probe.has_surviving_evidence());
        assert_eq!(
            probe.subsystems.contents,
            ReaderSalvageStreamState::ContainerUnavailable
        );

        let evidence =
            build_reader_partial_escherdelay_evidence(&bytes, &probe).expect("metafile evidence");
        assert!(evidence.validated_images.is_empty());
        assert_eq!(evidence.validated_metafiles.len(), 4);

        let graph = build_reader_partial_source_graph(&bytes, &probe)
            .expect("metafile-only recovery must be product-useful");
        assert!(
            !graph
                .facts
                .iter()
                .any(|fact| matches!(fact, ReaderPartialSourceFact::VerifiedImage { .. }))
        );
        assert_eq!(graph.recovered_resources.len(), 4);
        assert!(graph.recovered_resources.iter().all(|resource| {
            resource.source_sha256 == source_sha256(&bytes)
                && resource.stream_sid == evidence.stream_sid
        }));
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::ImageFactsUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::GeometryFactsUnavailable)
        );
    }

    #[test]
    fn damaged_cfb_can_recover_root_contents_without_relaxing_normal_cfb() {
        let bytes = corrupt_first_minifat_entry(synthetic_pub_cfb());
        assert!(pub_cfb::inspect_reader(Cursor::new(bytes.clone())).is_err());

        let probe = probe_reader_salvage_candidate(&bytes);
        assert_eq!(probe.intake.class, FailureIntakeClass::PubDamaged);
        assert_eq!(
            probe.eligibility,
            ReaderSalvageEligibility::EligibleDamagedPublisher
        );
        assert!(!probe.cfb_inventory_available);
        assert_eq!(probe.contents_family.as_deref(), Some("0x2c"));
        assert_eq!(
            probe.subsystems.contents,
            ReaderSalvageStreamState::RecoveredRootRegular
        );
        assert!(probe.has_surviving_evidence());
        assert_eq!(probe.source_sha256, source_sha256(&bytes));
        assert!(!probe.source_modified);
    }

    #[test]
    fn known_quill_corruption_mapping_is_narrow() {
        assert_eq!(
            known_quill_corruption_evidence(&QuillStoryReadError::DescriptorNodeTruncated {
                offset: 0x200,
                declared_count: 19_489,
                requested: 467_736,
                available: 504,
            }),
            Some(ReaderSalvageCorruptionEvidence::QuillDescriptorNodeTruncated)
        );
        assert_eq!(
            known_quill_corruption_evidence(&QuillStoryReadError::StrsServiceSpanOutOfBounds {
                service_span: u32::MAX,
                chunk_length: 30,
            }),
            Some(ReaderSalvageCorruptionEvidence::QuillStrsServiceSpanOutOfBounds)
        );
        assert_eq!(
            known_quill_corruption_evidence(&QuillStoryReadError::TooShort {
                offset: 8,
                requested: usize::MAX / 2,
                available: 12,
            }),
            None
        );
    }

    #[test]
    fn known_pub_failure_requires_independent_corruption_evidence() {
        let bytes = synthetic_pub_cfb();
        let intake_only = probe_reader_salvage_candidate(&bytes);
        assert_eq!(intake_only.intake.class, FailureIntakeClass::PubHighValue);
        assert_eq!(
            intake_only.eligibility,
            ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence
        );
        assert_eq!(
            intake_only.subsystems.contents,
            ReaderSalvageStreamState::NotAttempted
        );

        let proven = probe_reader_salvage_candidate_with_trigger(
            &bytes,
            ReaderSalvageTrigger::ProvenStructuralCorruption,
        );
        assert_eq!(
            proven.eligibility,
            ReaderSalvageEligibility::EligibleKnownPublisherCorruption
        );
        assert!(proven.cfb_inventory_available);
        assert_eq!(
            proven.subsystems.contents,
            ReaderSalvageStreamState::Readable
        );
        assert_eq!(proven.contents_family.as_deref(), Some("0x2c"));
        assert!(proven.has_surviving_evidence());
    }

    #[test]
    fn partial_source_graph_carries_verified_delayed_image_without_layout_join() {
        let bytes = synthetic_pub_cfb_with_delay_png();
        let probe = probe_reader_salvage_candidate_with_trigger(
            &bytes,
            ReaderSalvageTrigger::ProvenStructuralCorruption,
        );
        assert_eq!(
            probe.eligibility,
            ReaderSalvageEligibility::EligibleKnownPublisherCorruption
        );
        assert_eq!(
            probe.subsystems.escher_delay,
            ReaderSalvageStreamState::Readable
        );

        let graph =
            build_reader_partial_source_graph(&bytes, &probe).expect("partial salvage graph");
        let images = graph
            .facts
            .iter()
            .filter_map(|fact| match fact {
                ReaderPartialSourceFact::VerifiedImage {
                    resource_key,
                    sha256,
                    byte_len,
                } => Some((resource_key, sha256, *byte_len)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(images.len(), 1);
        assert!(images[0].0.starts_with("escher-delay:0:"));
        assert_eq!(images[0].1.len(), 64);
        assert!(images[0].2 > 8);
        assert!(
            !graph
                .gaps
                .contains(&ReaderPartialSourceGap::ImageFactsUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::GeometryFactsUnavailable)
        );
    }

    #[test]
    fn partial_source_graph_rejects_signature_only_image_with_bad_uid() {
        let bytes = synthetic_pub_cfb_with_delay_png_uid(false);
        let probe = probe_reader_salvage_candidate_with_trigger(
            &bytes,
            ReaderSalvageTrigger::ProvenStructuralCorruption,
        );
        assert_eq!(
            probe.subsystems.escher_delay,
            ReaderSalvageStreamState::Readable
        );

        let graph =
            build_reader_partial_source_graph(&bytes, &probe).expect("partial salvage graph");
        assert!(
            !graph
                .facts
                .iter()
                .any(|fact| matches!(fact, ReaderPartialSourceFact::VerifiedImage { .. }))
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::ImageFactsUnavailable)
        );
    }

    #[test]
    fn partial_source_graph_preserves_identity_and_explicit_gaps() {
        let bytes = synthetic_pub_cfb();
        let probe = probe_reader_salvage_candidate_with_trigger(
            &bytes,
            ReaderSalvageTrigger::ProvenStructuralCorruption,
        );
        let graph = build_reader_partial_source_graph(&bytes, &probe)
            .expect("eligible surviving evidence must project");

        assert_eq!(graph.schema_version, READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1);
        assert_eq!(graph.source_sha256, source_sha256(&bytes));
        assert!(
            graph.facts.is_empty(),
            "fixture has no Quill text or grounded graphics"
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::TextUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::ImageFactsUnavailable)
        );
        assert!(
            graph
                .gaps
                .contains(&ReaderPartialSourceGap::GeometryFactsUnavailable)
        );

        let mut changed = bytes.clone();
        changed.push(0);
        assert_eq!(
            build_reader_partial_source_graph(&changed, &probe),
            Err(ReaderPartialSourceGraphError::SourceIdentityMismatch)
        );

        let mut forged_probe = probe.clone();
        forged_probe.subsystems.quill = ReaderSalvageStreamState::Readable;
        assert_eq!(
            build_reader_partial_source_graph(&bytes, &forged_probe),
            Err(ReaderPartialSourceGraphError::ProbeMismatch)
        );
    }

    #[test]
    fn partial_source_graph_replays_damaged_publisher_probe_exactly() {
        let bytes = corrupt_first_minifat_entry(synthetic_pub_cfb());
        let probe = probe_reader_salvage_candidate(&bytes);
        assert_eq!(probe.trigger, ReaderSalvageTrigger::IntakeOnly);
        assert!(probe.corruption_evidence.is_none());
        assert_eq!(
            probe.eligibility,
            ReaderSalvageEligibility::EligibleDamagedPublisher
        );

        let graph = build_reader_partial_source_graph(&bytes, &probe)
            .expect("damaged Publisher probe must replay exactly");
        assert_eq!(graph.source_sha256, probe.source_sha256);
    }

    #[test]
    fn foreign_and_suspicious_inputs_are_never_salvage_eligible() {
        let foreign = probe_reader_salvage_candidate(b"<!DOCTYPE html><html>not pub</html>");
        assert_eq!(
            foreign.eligibility,
            ReaderSalvageEligibility::IneligibleForeign
        );

        let mut polyglot = b"<!DOCTYPE html>".to_vec();
        polyglot.extend_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        let suspicious = probe_reader_salvage_candidate(&polyglot);
        assert_eq!(
            suspicious.eligibility,
            ReaderSalvageEligibility::IneligibleSuspicious
        );
    }
}
