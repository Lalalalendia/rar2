use crate::failure_intake::{
    FailureIntakeClass, FailureIntakeClassification, classify_failure_candidate,
};
use crate::family_classifier::classify_pub_family;
use pub_contents::ContentsFamily;
use pub_core::StreamPath;
use pub_escher::inspect_delayed_blips;
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
    pub gaps: Vec<ReaderPartialSourceGap>,
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
    let current_probe = probe_reader_salvage_candidate_with_trigger(bytes, probe.trigger);
    if current_probe != *probe {
        return Err(ReaderPartialSourceGraphError::ProbeMismatch);
    }
    if !probe.eligibility.is_eligible() || !probe.has_surviving_evidence() {
        return Err(ReaderPartialSourceGraphError::Ineligible);
    }

    let mut facts = Vec::new();
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
        && let Ok(inventory) =
            inspect_delayed_blips(StreamPath(ESCHER_DELAY_STREAM.into()), &delay)
    {
        for record in inventory.records {
            let Some(source) = record.image_payload_source else {
                continue;
            };
            let Some(start) = usize::try_from(source.offset).ok() else {
                continue;
            };
            let Some(len) = usize::try_from(source.len).ok() else {
                continue;
            };
            let Some(end) = start.checked_add(len) else {
                continue;
            };
            let Some(payload) = delay.get(start..end) else {
                continue;
            };
            let sha256 = source_sha256(payload);
            facts.push(ReaderPartialSourceFact::VerifiedImage {
                resource_key: format!("escher-delay:{}:{sha256}", record.ordinal),
                sha256,
                byte_len: payload.len() as u64,
            });
            verified_image_count += 1;
        }
    }
    if verified_image_count == 0 {
        gaps.push(ReaderPartialSourceGap::ImageFactsUnavailable);
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
    let corruption_evidence = if bytes.len() <= READER_SALVAGE_MAX_INPUT_BYTES
        && intake.class == FailureIntakeClass::PubHighValue
        && trigger == ReaderSalvageTrigger::IntakeOnly
    {
        detect_known_structural_corruption(bytes)
    } else {
        None
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
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("synthetic Publisher CFB");
        compound.create_storage("/Escher").expect("Escher storage");

        let mut contents = vec![0_u8; 5_000];
        contents[..4].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00]);
        compound
            .create_stream(CONTENTS_STREAM)
            .expect("Contents stream")
            .write_all(&contents)
            .expect("write Contents");

        let mut payload = vec![0_u8; 17];
        payload.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        payload.extend_from_slice(b"salvage-image");
        let mut record = Vec::new();
        record.extend_from_slice(&0x6e00u16.to_le_bytes());
        record.extend_from_slice(&pub_escher::OFFICE_ART_BLIP_PNG.to_le_bytes());
        record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        record.extend_from_slice(&payload);
        compound
            .create_stream(ESCHER_DELAY_STREAM)
            .expect("Escher delay stream")
            .write_all(&record)
            .expect("write Escher delay");

        compound.flush().expect("flush synthetic CFB");
        compound.into_inner().into_inner()
    }

    fn corrupt_first_minifat_entry(mut bytes: Vec<u8>) -> Vec<u8> {
        let sector_shift = u16::from_le_bytes([bytes[30], bytes[31]]);
        let sector_len = 1usize << sector_shift;
        let minifat_sector = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
        let offset = (minifat_sector as usize + 1) * sector_len;
        bytes[offset..offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes
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
