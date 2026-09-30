use crate::failure_intake::{
    FailureIntakeClass, FailureIntakeClassification, classify_failure_candidate,
};
use crate::family_classifier::classify_pub_family;
use pub_contents::ContentsFamily;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const READER_SALVAGE_PROBE_SCHEMA_V1: &str = "chaptera.reader-salvage-probe.v1";

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
    let eligibility = salvage_eligibility(bytes.len(), intake.class, trigger);

    if !eligibility.is_eligible() {
        return ReaderSalvageProbe {
            schema_version: READER_SALVAGE_PROBE_SCHEMA_V1.to_owned(),
            source_sha256: source_sha256.clone(),
            trigger,
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
                trigger,
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
                trigger,
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
        compound.create_storage("/Objects").expect("Objects storage");
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
