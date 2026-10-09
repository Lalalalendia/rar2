//! Source-aware bridge from final Editor state to scoped PUB writer evidence.
//!
//! История Editor операций не равна набору native mutations. Для ordinary Story
//! последовательность A -> B -> C оценивается относительно immutable source как
//! одна mutation A -> C. Net-zero A -> B -> A не требует Story writer вообще.

use super::{
    EditOperation, EditorSession, mature_0x2c_pub_format_manifest,
    mature_0x2c_pub_persistence_target, replace_scalar_range_text_v1,
};
use pub_export::{
    PersistenceCompatibilityAssessment, PersistenceCompatibilityError, PersistenceRequirement,
    PersistenceRequirements, ScopedWriterCapability, WriterCapability, WriterCapabilityManifest,
    assess_persistence_compatibility,
};
use pub_model::{Sha256Digest, StoryId};
use pub_writer::{
    PUB_WRITER_PROBE_VERSION_V0_1, StoryTextWriteProbeRequest, probe_mature_0x2c_story_text_write,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const EDITOR_PUB_WRITER_ASSESSMENT_SCHEMA_V0_1: &str = "0.1";

pub(super) fn minimum_identity_project_schema_v1(operations: &[EditOperation]) -> &'static str {
    if operations
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
