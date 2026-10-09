//! Source-aware bridge from final Editor state to scoped PUB writer evidence.
//!
//! История Editor операций не равна набору native mutations. Для ordinary Story
//! последовательность A -> B -> C оценивается относительно immutable source как
//! одна mutation A -> C. Net-zero A -> B -> A не требует Story writer вообще.

use super::{
    EditOperation, EditorSession, IMAGE_CONTENT_TRANSFORM_FEATURE, mature_0x2c_pub_format_manifest,
    mature_0x2c_pub_persistence_target, replace_scalar_range_text_v1,
};
use pub_export::{
    PersistenceCompatibilityAssessment, PersistenceCompatibilityError, PersistenceRequirement,
    PersistenceRequirements, ScopedWriterCapability, WriterCapability, WriterCapabilityManifest,
    assess_persistence_compatibility,
};
use pub_model::{Sha256Digest, StoryId};
use pub_writer::{
    PUB_WRITER_PROBE_VERSION_V0_1, StoryTextPubMaterializationBlocked, StoryTextWriteProbeRequest,
    materialize_mature_0x2c_story_text_pub_candidate, probe_mature_0x2c_story_text_write,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const EDITOR_PUB_WRITER_ASSESSMENT_SCHEMA_V0_1: &str = "0.1";

pub(super) fn minimum_identity_project_schema_v1(operations: &[EditOperation]) -> &'static str {
    if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::DuplicateBlankPageV1 { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_27
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::DeleteBlankAuthoredPageV1 { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_26
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::AppendBlankPageV1 { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_25
    } else if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::RegisterAuthoredPageIdentityV1 { .. }
        )
    }) {
        super::EDITOR_PROJECT_VERSION_V0_24
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::ReorderPagesV1 { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_23
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::LinkTextFrameTail { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_22
    } else if operations
        .iter()
        .any(|operation| super::table_rowcol_history_v1(operation).is_some())
    {
        super::EDITOR_PROJECT_VERSION_V0_21
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetTableTrackExtent { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_20
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetImageCrop { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_19
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateTable { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_18
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateLine { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_17
    } else if operations
        .iter()
        .any(super::is_scoped_text_format_operation_v1)
    {
        super::EDITOR_PROJECT_VERSION_V0_16
    } else if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::SetParagraphAlignmentOverride { .. }
                | EditOperation::ClearParagraphAlignmentOverride { .. }
        )
    }) {
        super::EDITOR_PROJECT_VERSION_V0_15
    } else if operations
        .iter()
        .any(|operation| super::text_format_operation_story_id_v1(operation).is_some())
    {
        super::EDITOR_PROJECT_VERSION_V0_14
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::ReorderAuthoredStack { .. }))
    {
        super::EDITOR_PROJECT_VERSION_V0_13
    } else {
        super::EDITOR_PROJECT_VERSION_V0_12
    }
}

pub(super) fn append_blank_page_persistence_requirements_v1(
    transition: &super::AppendBlankPageTransitionV1,
) -> Vec<PersistenceRequirement> {
    vec![
        PersistenceRequirement {
            feature: "page.created_identity".into(),
            origin: Some(transition.identity.page_id.into_canonical()),
            property_path: Some("page.identity".into()),
        },
        PersistenceRequirement {
            feature: "page.geometry".into(),
            origin: Some(transition.identity.page_id.into_canonical()),
            property_path: Some("page.size".into()),
        },
        PersistenceRequirement {
            feature: "document.page_membership".into(),
            origin: Some(transition.document_id.into_canonical()),
            property_path: Some("document.pages".into()),
        },
    ]
}

pub(super) fn required_editor_asset_refs_v1(
    operations: &[EditOperation],
) -> BTreeSet<Sha256Digest> {
    operations
        .iter()
        .flat_map(EditOperation::durable_editor_asset_refs_v1)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EffectiveStoryTextMutation {
    pub story_id: StoryId,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorStoryWriterProbeState {
    Writable,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorStoryWriterProbeResult {
    pub mutation: EffectiveStoryTextMutation,
    pub requirement: PersistenceRequirement,
    pub state: EditorStoryWriterProbeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorPubWriterAssessment {
    pub schema_version: String,
    pub source_hash: Sha256Digest,
    pub manifest: WriterCapabilityManifest,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub story_probes: Vec<EditorStoryWriterProbeResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorPubPersistenceAssessment {
    pub writer: EditorPubWriterAssessment,
    pub compatibility: PersistenceCompatibilityAssessment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorPubWriterAssessmentError {
    SourceHashMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    MissingFinalStory {
        story_id: StoryId,
    },
    InvalidStoryHistory {
        story_id: StoryId,
    },
    Compatibility(PersistenceCompatibilityError),
}

impl fmt::Display for EditorPubWriterAssessmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceHashMismatch { expected, actual } => write!(
                formatter,
                "SHA-256 исходного PUB не совпадает с Editor session: ожидался {expected}, получен {actual}"
            ),
            Self::MissingFinalStory { story_id } => {
                write!(
                    formatter,
                    "финальная Story {story_id:?} отсутствует в Editor graph"
                )
            }
            Self::InvalidStoryHistory { story_id } => write!(
                formatter,
                "история Story {story_id:?} не replay-ится обратно к immutable source state"
            ),
            Self::Compatibility(error) => {
                write!(formatter, "ошибка persistence assessment: {error}")
            }
        }
    }
}

impl std::error::Error for EditorPubWriterAssessmentError {}

impl From<PersistenceCompatibilityError> for EditorPubWriterAssessmentError {
    fn from(value: PersistenceCompatibilityError) -> Self {
        Self::Compatibility(value)
    }
}

impl EditorSession {
    /// Возвращает только effective ordinary-Story mutations.
    ///
    /// TABLE-owned mutations намеренно не проходят через ordinary Story writer,
    /// даже если они физически изменяют тот же Quill Story text.
    pub fn effective_ordinary_story_text_mutations(
        &self,
    ) -> Result<Vec<EffectiveStoryTextMutation>, EditorPubWriterAssessmentError> {
        let mut ordinary_touched = BTreeSet::<StoryId>::new();
        let mut table_touched = BTreeSet::<StoryId>::new();

        for operation in self.operations() {
            match operation {
                EditOperation::ReplaceStoryRange { story_id, .. }
                | EditOperation::ReplaceStoryText { story_id, .. } => {
                    ordinary_touched.insert(*story_id);
                }
                EditOperation::ReplaceTableCellText { story_id, .. } => {
                    table_touched.insert(*story_id);
                }
                EditOperation::BreakTextFrameForwardLink { .. }
                | EditOperation::LinkTextFrameTail { .. }
                | EditOperation::ReplaceImage { .. }
                | EditOperation::SetImageCrop { .. }
                | EditOperation::MoveNode { .. }
                | EditOperation::MoveNodes { .. }
                | EditOperation::ResizeNode { .. }
                | EditOperation::ResizeNodes { .. }
                | EditOperation::CreateTextBox { .. }
                | EditOperation::CreateShape { .. }
                | EditOperation::CreateLine { .. }
                | EditOperation::CreateTable { .. }
                | EditOperation::SetTableTrackExtent { .. }
                | EditOperation::InsertTableRow { .. }
                | EditOperation::DeleteTableRow { .. }
                | EditOperation::InsertTableColumn { .. }
                | EditOperation::DeleteTableColumn { .. }
                | EditOperation::DeleteNode { .. }
                | EditOperation::ReorderAuthoredStack { .. }
                | EditOperation::ReorderPagesV1 { .. }
                | EditOperation::RegisterAuthoredPageIdentityV1 { .. }
                | EditOperation::AppendBlankPageV1 { .. }
                | EditOperation::DeleteBlankAuthoredPageV1 { .. }
                | EditOperation::DuplicateBlankPageV1 { .. }
                | EditOperation::SetTextFormatProperty { .. }
                | EditOperation::ClearTextFormatPropertyOverride { .. }
                | EditOperation::SetTextFormatPropertyScopedV1 { .. }
                | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. }
                | EditOperation::SetParagraphAlignmentOverride { .. }
                | EditOperation::ClearParagraphAlignmentOverride { .. } => {}
            }
        }

        let mut mutations = Vec::new();
        for story_id in ordinary_touched {
            if table_touched.contains(&story_id) {
                continue;
            }

            let after = self
                .graph()
                .stories
                .get(&story_id)
                .ok_or(EditorPubWriterAssessmentError::MissingFinalStory { story_id })?
                .text
                .clone();
            let mut before = after.clone();

            for operation in self.operations().iter().rev() {
                match operation {
                    EditOperation::ReplaceStoryRange {
                        story_id: op_story,
                        start_scalar,
                        expected_before,
                        replacement_text,
                        ..
                    } if *op_story == story_id => {
                        let replacement_len = u32::try_from(replacement_text.chars().count())
                            .map_err(|_| EditorPubWriterAssessmentError::InvalidStoryHistory {
                                story_id,
                            })?;
                        let inverse_end = start_scalar.checked_add(replacement_len).ok_or(
                            EditorPubWriterAssessmentError::InvalidStoryHistory { story_id },
                        )?;
                        before = replace_scalar_range_text_v1(
                            &before,
                            *start_scalar,
                            inverse_end,
                            replacement_text,
                            expected_before,
                        )
                        .ok_or(EditorPubWriterAssessmentError::InvalidStoryHistory { story_id })?;
                    }
                    EditOperation::ReplaceStoryText {
                        story_id: op_story,
                        before: operation_before,
                        after: operation_after,
                    } if *op_story == story_id => {
                        if before != *operation_after {
                            return Err(EditorPubWriterAssessmentError::InvalidStoryHistory {
                                story_id,
                            });
                        }
                        before.clone_from(operation_before);
                    }
                    _ => {}
                }
            }

            if before != after {
                mutations.push(EffectiveStoryTextMutation {
                    story_id,
                    before,
                    after,
                });
            }
        }

        Ok(mutations)
    }

    /// Requirements для native-persistence final state.
    ///
    /// Ordinary Story history заменяется source->final mutations. Остальные
    /// operation classes пока остаются консервативно operation-derived, пока
    /// для них нет собственного source-aware normalizer/writer.
    pub fn effective_pub_persistence_requirements(
        &self,
    ) -> Result<Vec<PersistenceRequirement>, EditorPubWriterAssessmentError> {
        let mut requirements = BTreeSet::<PersistenceRequirement>::new();

        for operation in self.operations() {
            match operation {
                EditOperation::ReplaceStoryRange { .. }
                | EditOperation::ReplaceStoryText { .. } => {}
                EditOperation::BreakTextFrameForwardLink { .. }
                | EditOperation::LinkTextFrameTail { .. }
                | EditOperation::ReplaceTableCellText { .. }
                | EditOperation::ReplaceImage { .. }
                | EditOperation::SetImageCrop { .. }
                | EditOperation::MoveNode { .. }
                | EditOperation::MoveNodes { .. }
                | EditOperation::ResizeNode { .. }
                | EditOperation::ResizeNodes { .. }
                | EditOperation::CreateTextBox { .. }
                | EditOperation::CreateShape { .. }
                | EditOperation::CreateLine { .. }
                | EditOperation::CreateTable { .. }
                | EditOperation::SetTableTrackExtent { .. }
                | EditOperation::InsertTableRow { .. }
                | EditOperation::DeleteTableRow { .. }
                | EditOperation::InsertTableColumn { .. }
                | EditOperation::DeleteTableColumn { .. }
                | EditOperation::DeleteNode { .. }
                | EditOperation::ReorderAuthoredStack { .. }
                | EditOperation::ReorderPagesV1 { .. }
                | EditOperation::RegisterAuthoredPageIdentityV1 { .. }
                | EditOperation::AppendBlankPageV1 { .. }
                | EditOperation::DeleteBlankAuthoredPageV1 { .. }
                | EditOperation::DuplicateBlankPageV1 { .. }
                | EditOperation::SetTextFormatProperty { .. }
                | EditOperation::ClearTextFormatPropertyOverride { .. }
                | EditOperation::SetTextFormatPropertyScopedV1 { .. }
                | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. }
                | EditOperation::SetParagraphAlignmentOverride { .. }
                | EditOperation::ClearParagraphAlignmentOverride { .. } => {
                    requirements.extend(operation.persistence_requirements());
                }
            }
        }

        for mutation in self.effective_ordinary_story_text_mutations()? {
            requirements.insert(story_text_requirement(mutation.story_id));
        }

        Ok(requirements.into_iter().collect())
    }

    /// Строит writer evidence только из exact source bytes и effective final state.
    pub fn build_mature_0x2c_pub_writer_assessment(
        &self,
        source_pub: &[u8],
    ) -> Result<EditorPubWriterAssessment, EditorPubWriterAssessmentError> {
        let actual_hash = sha256_digest(source_pub);
        if actual_hash != self.source_hash() {
            return Err(EditorPubWriterAssessmentError::SourceHashMismatch {
                expected: self.source_hash(),
                actual: actual_hash,
            });
        }

        let mut scoped = Vec::new();
        let mut story_probes = Vec::new();

        for mutation in self.effective_ordinary_story_text_mutations()? {
            let requirement = story_text_requirement(mutation.story_id);
            let request = StoryTextWriteProbeRequest {
                source_hash: self.source_hash(),
                story_id: mutation.story_id,
                before: mutation.before.clone(),
                after: mutation.after.clone(),
            };

            match probe_mature_0x2c_story_text_write(source_pub, &request) {
                Ok(_) => {
                    scoped.push(ScopedWriterCapability {
                        requirement: requirement.clone(),
                        capability: WriterCapability::Writable,
                    });
                    story_probes.push(EditorStoryWriterProbeResult {
                        mutation,
                        requirement,
                        state: EditorStoryWriterProbeState::Writable,
                        blocker_code: None,
                        blocker_detail: None,
                    });
                }
                Err(error) => {
                    scoped.push(ScopedWriterCapability {
                        requirement: requirement.clone(),
                        capability: WriterCapability::Blocked,
                    });
                    story_probes.push(EditorStoryWriterProbeResult {
                        mutation,
                        requirement,
                        state: EditorStoryWriterProbeState::Blocked,
                        blocker_code: Some(error.code().to_owned()),
                        blocker_detail: Some(error.to_string()),
                    });
                }
            }
        }

        Ok(EditorPubWriterAssessment {
            schema_version: EDITOR_PUB_WRITER_ASSESSMENT_SCHEMA_V0_1.to_owned(),
            source_hash: self.source_hash(),
            manifest: WriterCapabilityManifest {
                target: mature_0x2c_pub_persistence_target(),
                writer_version: PUB_WRITER_PROBE_VERSION_V0_1.to_owned(),
                features: BTreeMap::new(),
                scoped,
            },
            story_probes,
        })
    }

    /// End-to-end compatibility assessment для effective state.
    ///
    /// Это всё ещё не Save PUB gate: CFB materialization, opaque preservation и
    /// post-write reopen/round-trip validation остаются отдельными обязательными
    /// стадиями.
    pub fn assess_mature_0x2c_pub_effective_persistence(
        &self,
        source_pub: &[u8],
    ) -> Result<EditorPubPersistenceAssessment, EditorPubWriterAssessmentError> {
        let writer = self.build_mature_0x2c_pub_writer_assessment(source_pub)?;
        let compatibility = assess_persistence_compatibility(
            &mature_0x2c_pub_format_manifest(),
            &writer.manifest,
            self.effective_pub_persistence_requirements()?,
        )?;

        Ok(EditorPubPersistenceAssessment {
            writer,
            compatibility,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorNativePubCandidate {
    pub source_hash: Sha256Digest,
    pub output_hash: Sha256Digest,
    pub source_story_id: StoryId,
    pub output_story_id: StoryId,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorNativePubMaterializationBlocked {
    Assessment(EditorPubWriterAssessmentError),
    EffectiveStoryMutationCount {
        found: usize,
    },
    UnsupportedEffectiveState {
        requirements: Vec<PersistenceRequirement>,
    },
    Writer(StoryTextPubMaterializationBlocked),
}

impl EditorNativePubMaterializationBlocked {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Assessment(_) => "editor_pub_assessment",
            Self::EffectiveStoryMutationCount { .. } => "editor_pub_story_mutation_count",
            Self::UnsupportedEffectiveState { .. } => "editor_pub_effective_state_unsupported",
            Self::Writer(error) => error.code(),
        }
    }
}

impl fmt::Display for EditorNativePubMaterializationBlocked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assessment(error) => write!(formatter, "PUB writer assessment failed: {error}"),
            Self::EffectiveStoryMutationCount { found } => write!(
                formatter,
                "native PUB save requires exactly one effective ordinary Story mutation; found {found}"
            ),
            Self::UnsupportedEffectiveState { requirements } => write!(
                formatter,
                "native PUB save is blocked because the final Editor state requires {} persistence capabilities outside the single-Story text slice",
                requirements.len()
            ),
            Self::Writer(error) => write!(
                formatter,
                "native PUB writer blocked the final mutation: {error}"
            ),
        }
    }
}

impl std::error::Error for EditorNativePubMaterializationBlocked {}

impl From<EditorPubWriterAssessmentError> for EditorNativePubMaterializationBlocked {
    fn from(value: EditorPubWriterAssessmentError) -> Self {
        Self::Assessment(value)
    }
}

impl From<StoryTextPubMaterializationBlocked> for EditorNativePubMaterializationBlocked {
    fn from(value: StoryTextPubMaterializationBlocked) -> Self {
        Self::Writer(value)
    }
}

impl EditorSession {
    /// Materialize the current Editor final state into a whole-file PUB candidate
    /// only when the state is exactly one proven ordinary Story text mutation.
    ///
    /// The immutable source bytes are never modified. The delegated writer
    /// replaces only the bounded Quill stream in a copied CFB, then reopens the
    /// whole candidate through the current Reader and verifies the target Story.
    ///
    /// Success here proves Chaptera round-trip acceptance. Native Microsoft
    /// Publisher acceptance remains a separate validation gate.
    pub fn materialize_mature_0x2c_native_pub_candidate(
        &self,
        source_pub: &[u8],
    ) -> Result<EditorNativePubCandidate, EditorNativePubMaterializationBlocked> {
        let mutations = self.effective_ordinary_story_text_mutations()?;
        if mutations.len() != 1 {
            return Err(
                EditorNativePubMaterializationBlocked::EffectiveStoryMutationCount {
                    found: mutations.len(),
                },
            );
        }

        let requirements = self.effective_pub_persistence_requirements()?;
        let expected = vec![story_text_requirement(mutations[0].story_id)];
        if requirements != expected {
            return Err(
                EditorNativePubMaterializationBlocked::UnsupportedEffectiveState { requirements },
            );
        }

        let EffectiveStoryTextMutation {
            story_id,
            before,
            after,
        } = mutations.into_iter().next().expect("exactly one mutation");

        let request = StoryTextWriteProbeRequest {
            source_hash: self.source_hash(),
            story_id,
            before,
            after,
        };
        let candidate = materialize_mature_0x2c_story_text_pub_candidate(source_pub, &request)?;

        Ok(EditorNativePubCandidate {
            source_hash: candidate.source_hash,
            output_hash: candidate.output_hash,
            source_story_id: candidate.source_story_id,
            output_story_id: candidate.output_story_id,
            bytes: candidate.bytes,
        })
    }
}

fn story_text_requirement(story_id: StoryId) -> PersistenceRequirement {
    PersistenceRequirement {
        feature: "story.text".into(),
        origin: Some(story_id.into_canonical()),
        property_path: Some("story.text".into()),
    }
}

fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&digest);
    Sha256Digest::from_bytes(value)
}

impl EditOperation {
    /// Exact editor-owned asset identities required to retain this canonical
    /// operation in a durable EditorProject.
    ///
    /// Keep this match exhaustive: every future asset-bearing operation must
    /// make an explicit reachability decision here.
    pub fn durable_editor_asset_refs_v1(&self) -> Vec<Sha256Digest> {
        match self {
            Self::ReplaceImage {
                before_asset,
                after_asset,
                ..
            } => {
                let mut refs = Vec::with_capacity(2);
                if let Some(before_asset) = before_asset {
                    refs.push(*before_asset);
                }
                refs.push(*after_asset);
                refs.sort_unstable();
                refs.dedup();
                refs
            }
            Self::ReplaceStoryRange { .. }
            | Self::ReplaceStoryText { .. }
            | Self::BreakTextFrameForwardLink { .. }
            | Self::LinkTextFrameTail { .. }
            | Self::ReplaceTableCellText { .. }
            | Self::SetImageCrop { .. }
            | Self::MoveNode { .. }
            | Self::MoveNodes { .. }
            | Self::ResizeNode { .. }
            | Self::ResizeNodes { .. }
            | Self::CreateTextBox { .. }
            | Self::CreateShape { .. }
            | Self::CreateLine { .. }
            | Self::CreateTable { .. }
            | Self::SetTableTrackExtent { .. }
            | Self::InsertTableRow { .. }
            | Self::DeleteTableRow { .. }
            | Self::InsertTableColumn { .. }
            | Self::DeleteTableColumn { .. }
            | Self::DeleteNode { .. }
            | Self::ReorderAuthoredStack { .. }
            | Self::ReorderPagesV1 { .. }
            | Self::RegisterAuthoredPageIdentityV1 { .. }
            | Self::AppendBlankPageV1 { .. }
            | Self::DeleteBlankAuthoredPageV1 { .. }
            | Self::DuplicateBlankPageV1 { .. }
            | Self::SetTextFormatProperty { .. }
            | Self::ClearTextFormatPropertyOverride { .. }
            | Self::SetTextFormatPropertyScopedV1 { .. }
            | Self::ClearTextFormatPropertyOverrideScopedV1 { .. }
            | Self::SetParagraphAlignmentOverride { .. }
            | Self::ClearParagraphAlignmentOverride { .. } => Vec::new(),
        }
    }
}

impl PersistenceRequirements for EditOperation {
    fn persistence_requirements(&self) -> Vec<PersistenceRequirement> {
        match self {
            Self::ReplaceStoryRange { story_id, .. } | Self::ReplaceStoryText { story_id, .. } => {
                vec![PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                }]
            }
            Self::BreakTextFrameForwardLink {
                story_id,
                new_story_id,
                ..
            } => vec![
                PersistenceRequirement {
                    feature: "story.linked_frames".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.frames".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(new_story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
            ],
            Self::LinkTextFrameTail { transition } => vec![
                PersistenceRequirement {
                    feature: "story.linked_frames".into(),
                    origin: Some(transition.story_id.into_canonical()),
                    property_path: Some("story.frames".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(transition.target_empty_story.id.into_canonical()),
                    property_path: Some("story.inverse_empty_target".into()),
                },
            ],
            Self::ReplaceTableCellText {
                story_id, cell_id, ..
            } => vec![
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
                PersistenceRequirement {
                    feature: "table.cell_text".into(),
                    origin: Some(cell_id.into_canonical()),
                    property_path: Some("table.cell.text".into()),
                },
            ],
            Self::ReplaceImage { node_id, .. } => vec![PersistenceRequirement {
                feature: "image.replacement".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.image.resource".into()),
            }],
            Self::SetImageCrop { node_id, .. } => vec![PersistenceRequirement {
                feature: IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.image.crop".into()),
            }],
            Self::MoveNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.geometry.position".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds.position".into()),
            }],
            Self::MoveNodes { entries, .. } => entries
                .iter()
                .map(|entry| PersistenceRequirement {
                    feature: "node.geometry.position".into(),
                    origin: Some(entry.node_id.into_canonical()),
                    property_path: Some("node.bounds.position".into()),
                })
                .collect(),
            Self::ResizeNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.geometry.bounds".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds".into()),
            }],
            Self::ResizeNodes { entries, .. } => entries
                .iter()
                .map(|entry| PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(entry.node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                })
                .collect(),
            Self::CreateTextBox {
                node_id, story_id, ..
            } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
            ],
            Self::CreateShape { node_id, .. } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
                PersistenceRequirement {
                    feature: "shape.paint".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.paint".into()),
                },
            ],
            Self::CreateLine { node_id, .. } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "line.geometry.endpoints".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.line.geometry".into()),
                },
                PersistenceRequirement {
                    feature: "line.stroke".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.line.stroke".into()),
                },
            ],
            Self::CreateTable { table } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(table.story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
                PersistenceRequirement {
                    feature: "table.grid".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("table.grid".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::SetTableTrackExtent { history } => vec![
                PersistenceRequirement {
                    feature: "table.track_extent".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("table.grid.track.extent".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::InsertTableRow { history }
            | Self::DeleteTableRow { history }
            | Self::InsertTableColumn { history }
            | Self::DeleteTableColumn { history } => vec![
                PersistenceRequirement {
                    feature: "table.structure".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("table.grid".into()),
                },
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(history.story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::DeleteNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.deleted_identity".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node".into()),
            }],
            Self::ReorderAuthoredStack { transition } => vec![PersistenceRequirement {
                feature: "node.authored_stack_order".into(),
                origin: Some(transition.node_id.into_canonical()),
                property_path: Some("page.authored_stack".into()),
            }],
            Self::ReorderPagesV1 { transition } => vec![PersistenceRequirement {
                feature: "document.page_order".into(),
                origin: Some(transition.document_id.into_canonical()),
                property_path: Some("document.pages".into()),
            }],
            Self::RegisterAuthoredPageIdentityV1 { identity } => vec![PersistenceRequirement {
                feature: "page.created_identity".into(),
                origin: Some(identity.page_id.into_canonical()),
                property_path: Some("page.identity".into()),
            }],
            Self::AppendBlankPageV1 { transition } => {
                append_blank_page_persistence_requirements_v1(transition)
            }
            Self::DeleteBlankAuthoredPageV1 { transition } => vec![
                PersistenceRequirement {
                    feature: "page.created_identity".into(),
                    origin: Some(transition.identity.page_id.into_canonical()),
                    property_path: Some("page.identity".into()),
                },
                PersistenceRequirement {
                    feature: "document.page_membership".into(),
                    origin: Some(transition.document_id.into_canonical()),
                    property_path: Some("document.pages".into()),
                },
            ],
            Self::DuplicateBlankPageV1 { transition } => vec![
                PersistenceRequirement {
                    feature: "page.created_identity".into(),
                    origin: Some(transition.destination_identity.page_id.into_canonical()),
                    property_path: Some("page.identity".into()),
                },
                PersistenceRequirement {
                    feature: "document.page_membership".into(),
                    origin: Some(transition.document_id.into_canonical()),
                    property_path: Some("document.pages".into()),
                },
                PersistenceRequirement {
                    feature: "page.geometry".into(),
                    origin: Some(transition.destination_identity.page_id.into_canonical()),
                    property_path: Some("page.size".into()),
                },
            ],
            Self::SetTextFormatProperty { story_id, .. }
            | Self::ClearTextFormatPropertyOverride { story_id, .. }
            | Self::SetTextFormatPropertyScopedV1 { story_id, .. }
            | Self::ClearTextFormatPropertyOverrideScopedV1 { story_id, .. } => {
                vec![PersistenceRequirement {
                    feature: "story.character_format_overlay".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.character_format".into()),
                }]
            }
            Self::SetParagraphAlignmentOverride { paragraph_ids, .. }
            | Self::ClearParagraphAlignmentOverride { paragraph_ids, .. } => paragraph_ids
                .iter()
                .map(|paragraph_id| PersistenceRequirement {
                    feature: "story.paragraph_alignment".into(),
                    origin: Some(paragraph_id.into_canonical()),
                    property_path: Some("paragraph.alignment".into()),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod asset_reachability_tests {
    use super::super::*;

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::from_bytes([byte; 32])
    }

    fn replace(before_asset: Option<Sha256Digest>, after_asset: Sha256Digest) -> EditOperation {
        EditOperation::ReplaceImage {
            node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
                .expect("canonical NodeId"),
            before_asset,
            after_asset,
        }
    }

    #[test]
    fn operation_asset_refs_are_exact_and_deterministic() {
        let a = digest(0x11);
        let b = digest(0x22);

        assert_eq!(replace(None, a).durable_editor_asset_refs_v1(), vec![a]);
        assert_eq!(
            replace(Some(a), b).durable_editor_asset_refs_v1(),
            vec![a, b]
        );

        let refs = required_editor_asset_refs_v1(&[
            replace(None, a),
            replace(Some(a), b),
            replace(Some(b), a),
        ])
        .into_iter()
        .collect::<Vec<_>>();
        assert_eq!(refs, vec![a, b]);
    }

    #[test]
    fn current_image_resources_replace_source_bytes_without_fallback() {
        let node_id: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
            .expect("canonical NodeId");
        let source_resource: ResourceId =
            serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                .expect("canonical ResourceId");
        let replacement_sha = digest(0x55);
        let source_assets = BTreeMap::from([(
            source_resource,
            EditorSourceImageAsset {
                mime: "image/png".into(),
                bytes: vec![1, 2, 3],
            },
        )]);
        let source_nodes = BTreeMap::from([(node_id, source_resource)]);
        let replacement_assets = BTreeMap::from([(
            replacement_sha,
            EditorReplacementAsset {
                sha256: replacement_sha,
                mime: "image/jpeg".into(),
                bytes: vec![9, 8, 7, 6],
            },
        )]);
        let replacements = BTreeMap::from([(node_id, replacement_sha)]);

        let resources = current_image_resources_v1(
            &source_assets,
            &source_nodes,
            &replacement_assets,
            &replacements,
        )
        .expect("current image resources");

        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].node_ids, vec![node_id]);
        assert_eq!(resources[0].mime, "image/jpeg");
        assert_eq!(resources[0].bytes, vec![9, 8, 7, 6]);
        assert_ne!(resources[0].resource_id, source_resource);
    }

    #[test]
    fn current_image_resources_group_shared_source_and_replacement_assets() {
        let node_a: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
            .expect("canonical NodeId");
        let node_b: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000002\"")
            .expect("canonical NodeId");
        let node_c: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000003\"")
            .expect("canonical NodeId");
        let node_d: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000004\"")
            .expect("canonical NodeId");
        let source_resource: ResourceId =
            serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                .expect("canonical ResourceId");
        let replacement_sha = digest(0x66);

        let resources = current_image_resources_v1(
            &BTreeMap::from([(
                source_resource,
                EditorSourceImageAsset {
                    mime: "image/png".into(),
                    bytes: vec![1, 2, 3],
                },
            )]),
            &BTreeMap::from([(node_a, source_resource), (node_b, source_resource)]),
            &BTreeMap::from([(
                replacement_sha,
                EditorReplacementAsset {
                    sha256: replacement_sha,
                    mime: "image/jpeg".into(),
                    bytes: vec![4, 5, 6],
                },
            )]),
            &BTreeMap::from([(node_c, replacement_sha), (node_d, replacement_sha)]),
        )
        .expect("current image resources");

        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].resource_id, source_resource);
        assert_eq!(resources[0].node_ids, vec![node_a, node_b]);
        let replacement_resource = replacement_asset_resource_id(replacement_sha);
        let replacement = resources
            .iter()
            .find(|resource| resource.resource_id == replacement_resource)
            .expect("replacement resource");
        assert_eq!(replacement.node_ids, vec![node_c, node_d]);
        assert_eq!(replacement.bytes, vec![4, 5, 6]);
    }

    #[test]
    fn current_image_resources_fail_closed_when_replacement_bytes_are_missing() {
        let node_id: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
            .expect("canonical NodeId");
        let replacement_sha = digest(0x77);
        let error = current_image_resources_v1(
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([(node_id, replacement_sha)]),
        )
        .expect_err("missing replacement bytes must fail");

        assert_eq!(
            error,
            EditorCurrentImageResourceError::MissingReplacementAsset {
                sha256: replacement_sha,
            }
        );
    }

    #[test]
    fn source_text_format_base_preserves_effective_bools_without_inventing_defaults() {
        let source_hash: Sha256Digest =
            "1111111111111111111111111111111111111111111111111111111111111111"
                .parse()
                .expect("test source hash");
        let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x51; 16]));
        let run = PubTypographyRun {
            story_id,
            story_utf16_start: 0,
            story_utf16_end: 3,
            story_scalar_start: 0,
            story_scalar_end: 3,
            source_font_index: 4,
            source_font_name: "Montserrat".to_owned(),
            text_size_emu: 304_800,
            font_inherited: true,
            size_inherited: true,
            color_rgb: Some([0x11, 0x22, 0x33]),
            color_scheme_slot: None,
            color_inherited: true,
            bold: Some(pub_reader::PubTypographyBooleanV1 {
                local_toggle: true,
                inherited_value: true,
                effective_value: false,
            }),
            italic: Some(pub_reader::PubTypographyBooleanV1 {
                local_toggle: false,
                inherited_value: true,
                effective_value: true,
            }),
        };

        let state = source_text_format_overlay_from_runs_v1(
            source_hash,
            story_id,
            "sha256:source-story",
            3,
            &[run],
        )
        .expect("complete bounded source format");
        assert_eq!(state.base_runs.len(), 1);
        let format = &state.base_runs[0].format;
        assert!(!format.bold);
        assert!(format.italic);
        assert_eq!(format.font_size_emu, 304_800);
        assert_eq!(format.text_color_rgb, "#112233");
        assert!(format.font_resource_id.starts_with("pub-source-font:"));
        assert!(state.overrides.is_empty());
    }

    #[test]
    fn source_text_format_base_refuses_to_invent_missing_color() {
        let source_hash: Sha256Digest =
            "2222222222222222222222222222222222222222222222222222222222222222"
                .parse()
                .expect("test source hash");
        let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x52; 16]));
        let run = PubTypographyRun {
            story_id,
            story_utf16_start: 0,
            story_utf16_end: 1,
            story_scalar_start: 0,
            story_scalar_end: 1,
            source_font_index: 0,
            source_font_name: "Arial".to_owned(),
            text_size_emu: 152_400,
            font_inherited: false,
            size_inherited: false,
            color_rgb: None,
            color_scheme_slot: None,
            color_inherited: false,
            bold: Some(pub_reader::PubTypographyBooleanV1 {
                local_toggle: false,
                inherited_value: false,
                effective_value: false,
            }),
            italic: Some(pub_reader::PubTypographyBooleanV1 {
                local_toggle: false,
                inherited_value: false,
                effective_value: false,
            }),
        };

        let error = source_text_format_overlay_from_runs_v1(
            source_hash,
            story_id,
            "sha256:source-story",
            1,
            &[run],
        )
        .expect_err("missing color must fail closed");
        assert!(matches!(
            error,
            EditorTextFormatBaseErrorV1::UnsupportedBase { .. }
        ));
        assert!(error.to_string().contains("text color"));
    }

    #[test]
    fn consumer_proven_typography_override_is_montserrat_only() {
        let montserrat_story =
            StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x31; 16]));
        let arial_story = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x32; 16]));
        let typography = vec![
            FullStoryTypographyV1 {
                story_id: montserrat_story,
                font_family: "Montserrat".into(),
                font_size_emu: LengthEmu::new(304_800),
            },
            FullStoryTypographyV1 {
                story_id: arial_story,
                font_family: "Arial".into(),
                font_size_emu: LengthEmu::new(152_400),
            },
        ];

        for target in [EditorEditableTarget::Idml, EditorEditableTarget::Odg] {
            let overrides = consumer_proven_typography_overrides_v1(target, &typography);
            assert_eq!(overrides.len(), 2);
            assert!(overrides.iter().all(|item| {
                item.origin == montserrat_story.into_canonical()
                    && matches!(
                        item.feature.as_str(),
                        STORY_FONT_FAMILY_FEATURE | STORY_FONT_SIZE_FEATURE
                    )
                    && item.disposition == CapabilityLevel::Preserved
            }));
            assert!(
                overrides
                    .iter()
                    .all(|item| { item.origin != arial_story.into_canonical() })
            );
        }
    }

    #[test]
    fn scoped_text_format_wire_has_distinct_kind_and_schema_floor() {
        let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x61; 16]));
        let legacy = EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar: 0,
            end_scalar: 3,
            property: FormatPropertyV1::Bold,
            value: FormatValueV1::Bool(true),
            before_state_hash: "legacy-before".to_owned(),
            after_state_hash: "legacy-after".to_owned(),
        };
        let scoped = EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            start_scalar: 0,
            end_scalar: 3,
            property: FormatPropertyV1::Bold,
            value: FormatValueV1::Bool(true),
            before_state_hash: "sha256:scoped-before".to_owned(),
            after_state_hash: "sha256:scoped-after".to_owned(),
        };

        let legacy_json = serde_json::to_value(&legacy).expect("legacy format JSON");
        let scoped_json = serde_json::to_value(&scoped).expect("scoped format JSON");
        assert_eq!(legacy_json["kind"], "set_text_format_property");
        assert_eq!(scoped_json["kind"], "set_text_format_property_scoped_v1");
        assert!(legacy_json.get("state_domain").is_none());
        assert!(scoped_json.get("state_domain").is_none());
        assert_eq!(
            serde_json::from_value::<EditOperation>(legacy_json)
                .expect("legacy JSON remains readable"),
            legacy
        );
        assert_eq!(
            serde_json::from_value::<EditOperation>(scoped_json)
                .expect("scoped JSON is readable by v0.16"),
            scoped
        );

        assert_eq!(
            minimum_identity_project_schema_v1(&[legacy]),
            EDITOR_PROJECT_VERSION_V0_14
        );
        assert_eq!(
            minimum_identity_project_schema_v1(&[scoped]),
            EDITOR_PROJECT_VERSION_V0_16
        );
        assert_eq!(
            minimum_identity_project_schema_v1(&[]),
            EDITOR_PROJECT_VERSION_V0_12
        );
    }

    #[test]
    fn create_line_requires_v017_schema_and_round_trips_exact_wire() {
        let operation = EditOperation::CreateLine {
            node_id: serde_json::from_str("\"01890f47-0c00-7abc-8def-0123456789ab\"")
                .expect("canonical editor UUIDv7 NodeId"),
            page_id: serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"")
                .expect("canonical PageId"),
            parent_id: serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"")
                .expect("canonical PageId"),
            geometry: LineGeometryV1 {
                begin: PointEmuV1 { x: 100, y: 200 },
                end: PointEmuV1 { x: 400, y: 500 },
            },
            stroke: AuthoredSolidStrokeV1 {
                visible: true,
                color: Srgb8V1 { r: 4, g: 5, b: 6 },
                width_emu: 25_400,
            },
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };

        assert_eq!(
            minimum_identity_project_schema_v1(std::slice::from_ref(&operation)),
            EDITOR_PROJECT_VERSION_V0_17
        );
        assert!(operation.durable_editor_asset_refs_v1().is_empty());

        let json = serde_json::to_value(&operation).expect("CreateLine JSON");
        assert_eq!(json["kind"], "create_line");
        assert_eq!(
            serde_json::from_value::<EditOperation>(json).expect("CreateLine JSON round-trip"),
            operation
        );
    }

    #[test]
    fn non_asset_operations_emit_no_durable_asset_refs() {
        let operation = EditOperation::MoveNode {
            node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
                .expect("canonical NodeId"),
            before: RectEmu::new(
                LengthEmu::ZERO,
                LengthEmu::ZERO,
                LengthEmu::new(10),
                LengthEmu::new(10),
            ),
            after: RectEmu::new(
                LengthEmu::new(1),
                LengthEmu::new(2),
                LengthEmu::new(10),
                LengthEmu::new(10),
            ),
        };
        assert!(operation.durable_editor_asset_refs_v1().is_empty());
    }
}

#[cfg(test)]
mod native_pub_candidate_tests {
    use super::*;
    use crate::{LengthEmu, open_mature_0x2c_editor};
    use pub_reader::derive_pub_story_id;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn sample3_pub() -> Vec<u8> {
        decode_base64(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../pub-quill/tests/fixtures/Sample3.pub.b64"
        )))
    }

    fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
        let digest = Sha256::digest(bytes);
        let mut value = [0_u8; 32];
        value.copy_from_slice(&digest);
        Sha256Digest::from_bytes(value)
    }

    fn controlled_story(session: &EditorSession, source_hash: Sha256Digest) -> (StoryId, String) {
        let story_id = derive_pub_story_id(&source_hash, 4).expect("source StoryId");
        let text = session.graph().stories.get(&story_id).expect("controlled Story").text.clone();
        (story_id, text)
    }

    #[test]
    fn single_effective_story_mutation_materializes_reopenable_pub() {
        let source = sample3_pub();
        let source_before = source.clone();
        let source_hash = sha256_digest(&source);
        let mut session =
            open_mature_0x2c_editor(&source, source_hash).expect("open bounded editor");
        let (story_id, before) = controlled_story(&session, source_hash);
        assert!(before.contains("345678"), "controlled marker must exist");
        let after = before.replacen("345678", "", 1);
        session
            .replace_story_text(story_id, after.clone())
            .expect("ordinary Story edit");

        let candidate = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect("native PUB candidate");

        assert_eq!(source, source_before, "source bytes are immutable");
        assert_eq!(candidate.source_hash, source_hash);
        assert_ne!(candidate.output_hash, source_hash);
        assert_eq!(candidate.source_story_id, story_id);

        let reopened = pub_reader::build_mature_0x2c_source_graph(
            Cursor::new(&candidate.bytes),
            candidate.output_hash,
        )
        .expect("candidate SourceGraph");
        let resolved =
            pub_reader::resolve_pub_source_graph(&reopened.graph).expect("candidate resolve");
        assert_eq!(
            resolved.graph.stories[&candidate.output_story_id].text,
            after
        );
    }

    #[test]
    fn no_effective_story_mutation_is_never_a_native_pub_save() {
        let source = sample3_pub();
        let source_hash = sha256_digest(&source);
        let session = open_mature_0x2c_editor(&source, source_hash).expect("bounded Editor");
        let error = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect_err("no-op source remains Project-only");
        assert_eq!(error.code(), "editor_pub_story_mutation_count");
    }

    #[test]
    fn additional_geometry_requirement_blocks_native_pub_save() {
        let source = sample3_pub();
        let source_hash = sha256_digest(&source);
        let mut session =
            open_mature_0x2c_editor(&source, source_hash).expect("open bounded editor");
        let (story_id, before) = controlled_story(&session, source_hash);
        let after = before.replacen("345678", "", 1);
        session
            .replace_story_text(story_id, after)
            .expect("ordinary Story edit");

        let node_id = session
            .graph()
            .nodes
            .iter()
            .find_map(|(node_id, node)| {
                let next_x = LengthEmu::new(node.header.bounds.x.get().checked_add(1)?);
                session
                    .can_move_node_to(*node_id, next_x, node.header.bounds.y)
                    .ok()
                    .map(|_| *node_id)
            })
            .expect("movable geometry candidate");
        let before_bounds = session.graph().nodes[&node_id].header.bounds;
        session
            .move_node_to(
                node_id,
                LengthEmu::new(before_bounds.x.get() + 1),
                before_bounds.y,
            )
            .expect("bounded move");

        let error = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect_err("mixed final state must remain Project-only");
        assert_eq!(error.code(), "editor_pub_effective_state_unsupported");
    }

    fn decode_base64(text: &str) -> Vec<u8> {
        let cleaned = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        assert_eq!(cleaned.len() % 4, 0, "base64 fixture length");

        let mut output = Vec::with_capacity(cleaned.len() / 4 * 3);
        for quartet in cleaned.chunks_exact(4) {
            let a = base64_value(quartet[0]);
            let b = base64_value(quartet[1]);
            let c = if quartet[2] == b'=' {
                0
            } else {
                base64_value(quartet[2])
            };
            let d = if quartet[3] == b'=' {
                0
            } else {
                base64_value(quartet[3])
            };
            output.push((a << 2) | (b >> 4));
            if quartet[2] != b'=' {
                output.push((b << 4) | (c >> 2));
            }
            if quartet[3] != b'=' {
                output.push((c << 6) | d);
            }
        }
        output
    }

    fn base64_value(byte: u8) -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("invalid base64 byte {other:#x}"),
        }
    }
}
