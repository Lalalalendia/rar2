//! Bounded authoring session for Publisher migration workflows.
//!
//! This crate does not make a general "editable PUB" claim. Every edit
//! operation is capability-gated and preserves the immutable source identity.
//! The first operation is a bounded ordinary-story text replacement over the
//! resolved authoring graph. Native PUB materialization remains a separate
//! writer gate.

mod authored_paragraph_alignment_v1;
mod duplicate_authored_rectangle_v1;
mod imported_paragraph_alignment_v1;
mod imported_paragraph_flow_v1;
mod imported_paragraphs_v1;
mod link_text_frame_tail_v1;
mod session_geometry;
mod session_image;
mod session_table;
mod session_text;
use session_geometry::{validate_move_nodes_transition, validate_resize_nodes_transition};
use session_image::{
    apply_crop_forward, apply_crop_inverse, apply_image_forward, apply_image_inverse,
};
mod table_rowcol_graph_v1;
mod table_rowcol_history_v1;
mod text_format_property_base_v1;
mod writer_assessment;

pub use authored_paragraph_alignment_v1::{
    AuthoredParagraphAlignmentValueV1, EffectiveParagraphAlignmentV1,
    EffectiveParagraphAlignmentValueV1, ParagraphAlignmentAuthorityV1,
    ParagraphAlignmentOverrideSnapshotV1, ParagraphAlignmentTransitionErrorV1,
};
pub use pub_editor_authoring_core::{
    AUTHORED_STACK_PROTOCOL_V1, AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1,
    AUTHORED_TABLE_SENTINEL_TEXT_ID_V1, AuthoredEntityProvenanceV1, AuthoredLineRuntimeV1,
    AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1, AuthoredShapeTransformV1,
    AuthoredSolidFillV1, AuthoredSolidStrokeV1, AuthoredStackLifecycleErrorV1,
    AuthoredStackLifecycleKindV1, AuthoredStackLifecycleTransitionV1, AuthoredStackReorderErrorV1,
    AuthoredStackReorderModeV1, AuthoredStackReorderTransitionV1, AuthoredStackV1,
    AuthoredTableStoryRangesV1, CreateLineRuntimeValidationError,
    CreateShapeRuntimeValidationError, CreateTablePlanV1, CreateTableRuntimeV1,
    CreateTableRuntimeValidationError, LineGeometryV1, PointEmuV1, Srgb8V1,
    apply_authored_stack_reorder_forward_v1, apply_authored_stack_reorder_inverse_v1,
    apply_authored_stack_transition_forward_v1, apply_authored_stack_transition_inverse_v1,
    apply_create_table_forward_v1, apply_create_table_inverse_v1, authored_stack_state_id_v1,
    build_create_table_plan_v1, line_bounds_v1, plan_create_line_append_v1,
    plan_create_shape_append_v1, plan_create_table_append_v1, plan_delete_shape_remove_v1,
    plan_reorder_authored_stack_v1, rebuild_authored_table_story_v1,
    validate_authored_line_runtime_v1, validate_authored_shape_runtime_v1,
    validate_authored_stack_v1, validate_create_table_runtime_v1,
};

pub use duplicate_authored_rectangle_v1::{
    DUPLICATE_OFFSET_EMU_V1, DUPLICATE_PLACEMENT_POLICY_V1, DuplicateAuthoredRectangleErrorV1,
    DuplicateAuthoredRectanglePlanV1, plan_duplicate_authored_rectangle_v1,
    validate_duplicate_authored_rectangle_source_v1,
};
pub use imported_paragraph_alignment_v1::{
    ImportedParagraphAlignmentValueV1, ImportedParagraphBaseAlignmentErrorV1,
    ImportedParagraphBaseAlignmentV1,
};
pub use imported_paragraph_flow_v1::{
    ImportedParagraphFlowConstraintBindingV1, ImportedParagraphFlowConstraintV1,
    ImportedParagraphFlowErrorV1,
};
pub use imported_paragraphs_v1::{ImportedParagraphProjectionErrorV1, ImportedParagraphV1};
pub use link_text_frame_tail_v1::TextFrameLinkTransitionV1;
pub use pub_editor_geometry_core::{
    MAX_MOVE_NODES_V1, MAX_RESIZE_NODES_V1, MoveNodeBatchEntry, ResizeNodeBatchEntry,
};
pub use pub_editor_table_core::{
    SetTableTrackExtentErrorV1, SetTableTrackExtentHistoryV1, TABLE_TRACK_EXTENT_HISTORY_V1,
    TableTrackExtentHistoryErrorV1, TableTrackExtentPlanV1, TableTrackTargetV1,
    apply_table_track_extent_history_forward_v1, apply_table_track_extent_history_inverse_v1,
    canonical_table_track_extent_history_v1, plan_table_track_extent_v1, set_table_track_extent_v1,
};
pub use table_rowcol_graph_v1::{
    TableRowColGraphErrorV1, apply_table_structure_snapshot_to_graph_v1,
    table_structure_snapshot_from_graph_v1,
};
pub use table_rowcol_history_v1::{
    TABLE_ROWCOL_HISTORY_V1, TableCellContentSnapshotV1, TableRowColHistoryErrorV1,
    TableRowColHistoryV1, TableRowColMutationV1, TableStructureSnapshotV1,
    apply_table_rowcol_history_forward_v1, apply_table_rowcol_history_inverse_v1,
    canonical_table_rowcol_history_v1, plan_table_rowcol_mutation_v1,
    validate_table_structure_snapshot_v1,
};
pub use text_format_property_base_v1::{
    TEXT_FORMAT_PROPERTY_STATE_V1, TextFormatPropertyBaseRunV1, TextFormatPropertyOverrideRunV1,
    TextFormatPropertyStateV1, apply_text_format_property_operation_checked_v1,
    apply_text_format_property_operation_semantic_v1, build_source_text_format_property_state_v1,
    clear_text_format_property_state_v1, effective_text_format_property_segments_v1,
    fold_text_format_property_history_v1, set_text_format_property_state_v1,
    text_format_property_state_hash_v1,
};
pub use writer_assessment::{
    EDITOR_PUB_WRITER_ASSESSMENT_SCHEMA_V0_1, EditorPubPersistenceAssessment,
    EditorPubWriterAssessment, EditorPubWriterAssessmentError, EditorStoryWriterProbeResult,
    EditorStoryWriterProbeState, EffectiveStoryTextMutation,
};

use chaptera_text_format_overlay::{
    BaseCharacterFormatV1, BaseFormatRunV1, TextFormatOverlayStateV1,
    build_text_format_overlay_state_v1,
    clear_text_format_property_override_v1 as overlay_clear_text_format_property_override_v1,
    set_text_format_property_v1 as overlay_set_text_format_property_v1, state_hash_v1,
};
pub use chaptera_text_format_overlay::{
    EffectivePropertySegmentV1, EffectivePropertySourceV1, FormatPropertyV1, FormatValueV1,
};
use pub_export::{
    CapabilityLevel, ExportPlan, ExportReport, ExportReportSource, FormatCompatibilityManifest,
    FormatRepresentability, FullStoryParagraphAlignmentV1, FullStoryTypographyV1, LossItem,
    LossKind, LossSeverity, ParagraphAlignmentV1, ParagraphScopedAlignmentV1,
    PersistenceCompatibilityAssessment, PersistenceCompatibilityError, PersistenceRequirement,
    PersistenceRequirements, PersistenceTargetProfile, STORY_FONT_FAMILY_FEATURE,
    STORY_FONT_SIZE_FEATURE, STORY_PARAGRAPH_ALIGNMENT_FEATURE, STORY_TEXT_COLOR_FEATURE,
    ScopedCapabilityError, ScopedCapabilityOverride, SemanticFeatureRequest,
    TargetCapabilityManifest, TargetProfile, WriterCapabilityManifest,
    assess_persistence_compatibility, build_export_report, plan_export_with_scoped_capabilities,
    render_human_summary,
};
use pub_idml::{
    IDML_ADAPTER_VERSION_V0_1, IDML_SCHEMA_FENCE_LEGACY_DOM_7, IMAGE_BYTES_FEATURE,
    IMAGE_CONTENT_TRANSFORM_FEATURE, IMAGE_FRAME_GEOMETRY_FEATURE, IdmlEmbeddedImagePlacement,
    IdmlParagraphScopedAlignmentPlacement, IdmlWireProfile, add_embedded_images_to_idml,
    add_full_story_paragraph_alignment_to_idml, add_full_story_typography_to_idml,
    add_paragraph_scoped_alignment_to_idml, project_resolved_graph_to_idml, write_idml_ucf,
};
use pub_model::{
    Affine2D, EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1,
    EffectiveTableTrackV1, Node, NodeHeader, NodeKind, ResourceId, SourceDerivedIdInput, Story,
    StoryFrame, TableColumnId, TableRowId, derive_source_canonical_id, validate_story_frames,
};
pub use pub_model::{
    LengthEmu, NodeId, PageId, ParagraphId, RectEmu, Sha256Digest, StoryId, TableCellId,
};
use pub_odg::{
    ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgEmbeddedImagePlacement,
    OdgFullStoryParagraphAlignmentPlacement, OdgFullStoryTypographyPlacement,
    OdgParagraphScopedAlignmentPlacement, add_embedded_images_to_odg,
    add_full_story_paragraph_alignment_to_odg, add_full_story_typography_to_odg,
    add_paragraph_scoped_alignment_to_odg, project_resolved_graph_to_odg, write_odg,
};
use pub_reader::{
    PubAssetExportBundle, PubParagraphAlignmentRun, PubParagraphFlowRun, PubResolvedGraph,
    PubResolvedNodePayload, PubResolvedStoryFrame, PubTypographyRun, PubTypographySizeRun,
    build_mature_0x2c_asset_export_bundle_from_bytes, build_mature_0x2c_source_graph,
    materialize_bounded_simple_table_cells, resolve_pub_source_graph,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Cursor;
use uuid::Uuid;

pub const EDITOR_PROJECT_VERSION_V0_1: &str = "pub-editor-v0.1";
pub const EDITOR_PROJECT_VERSION_V0_2: &str = "pub-editor-v0.2";
pub const EDITOR_PROJECT_VERSION_V0_3: &str = "pub-editor-v0.3";
pub const EDITOR_PROJECT_VERSION_V0_4: &str = "pub-editor-v0.4";
pub const EDITOR_PROJECT_VERSION_V0_5: &str = "pub-editor-v0.5";
pub const EDITOR_PROJECT_VERSION_V0_6: &str = "pub-editor-v0.6";
pub const EDITOR_PROJECT_VERSION_V0_7: &str = "pub-editor-v0.7";
pub const EDITOR_PROJECT_VERSION_V0_8: &str = "pub-editor-v0.8";
pub const EDITOR_PROJECT_VERSION_V0_9: &str = "pub-editor-v0.9";
pub const EDITOR_PROJECT_VERSION_V0_10: &str = "pub-editor-v0.10";
pub const EDITOR_PROJECT_VERSION_V0_11: &str = "pub-editor-v0.11";
pub const EDITOR_PROJECT_VERSION_V0_12: &str = "pub-editor-v0.12";
pub const EDITOR_PROJECT_VERSION_V0_13: &str = "pub-editor-v0.13";
pub const EDITOR_PROJECT_VERSION_V0_14: &str = "pub-editor-v0.14";
pub const EDITOR_PROJECT_VERSION_V0_15: &str = "pub-editor-v0.15";
pub const EDITOR_PROJECT_VERSION_V0_16: &str = "pub-editor-v0.16";
pub const EDITOR_PROJECT_VERSION_V0_17: &str = "pub-editor-v0.17";
pub const EDITOR_PROJECT_VERSION_V0_18: &str = "pub-editor-v0.18";
pub const EDITOR_PROJECT_VERSION_V0_19: &str = "pub-editor-v0.19";
pub const EDITOR_PROJECT_VERSION_V0_20: &str = "pub-editor-v0.20";
pub const EDITOR_PROJECT_VERSION_V0_21: &str = "pub-editor-v0.21";
pub const EDITOR_PROJECT_VERSION_V0_22: &str = "pub-editor-v0.22";
pub const EDITOR_PROJECT_VERSION_CURRENT: &str = EDITOR_PROJECT_VERSION_V0_22;
pub const PUB_MATURE_0X2C_PERSISTENCE_PROFILE: &str = "mature-0x2c";
pub const PUB_MATURE_0X2C_SCHEMA_FENCE: &str = "pub-family-0x2c";

/// Canonical Story-state identity shared with services/editor-api/story_range_v1.py.
pub fn story_state_id_v1(story_id: StoryId, text: &str) -> String {
    let payload = serde_json::json!({
        "protocol_version": "chaptera.story-state.v1",
        "story_id": story_id.as_canonical().to_string(),
        "text": text,
    });
    let bytes =
        serde_json::to_vec(&payload).expect("canonical Story state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

/// Canonical authored-shape state identity used by persisted DeleteNode V1.
pub fn authored_shape_state_id_v1(shape: &AuthoredShapeRuntimeV1) -> String {
    let payload = serde_json::json!({
        "protocol_version": "chaptera.authored-shape-state.v1",
        "shape": shape,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical authored-shape state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

fn scalar_byte_offset(text: &str, scalar_index: u32) -> Option<usize> {
    let target = usize::try_from(scalar_index).ok()?;
    if target == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(target).map(|(offset, _)| offset)
}

fn replace_scalar_range_text(
    text: &str,
    start_scalar: u32,
    end_scalar: u32,
    expected_before: &str,
    replacement_text: &str,
) -> Option<String> {
    if end_scalar < start_scalar {
        return None;
    }
    let start = scalar_byte_offset(text, start_scalar)?;
    let end = scalar_byte_offset(text, end_scalar)?;
    if text.get(start..end)? != expected_before {
        return None;
    }
    let mut after = String::with_capacity(
        text.len()
            .saturating_sub(end.saturating_sub(start))
            .saturating_add(replacement_text.len()),
    );
    after.push_str(&text[..start]);
    after.push_str(replacement_text);
    after.push_str(&text[end..]);
    Some(after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageCropStateV1 {
    pub top_raw: Option<u32>,
    pub bottom_raw: Option<u32>,
    pub left_raw: Option<u32>,
    pub right_raw: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringTextPresetV1 {
    pub resource_id: String,
    pub font_fingerprint_sha256: String,
    pub face_index: u32,
    pub font_size_emu: LengthEmu,
    pub line_height_emu: LengthEmu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenAuthorCreatedStoryV1 {
    pub story_id: StoryId,
    pub frame_id: NodeId,
    pub page_id: PageId,
    pub text_preset: AuthoringTextPresetV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorCreatedStoryProofError {
    pub code: &'static str,
    pub message: String,
}

impl AuthorCreatedStoryProofError {
    fn unproven(message: impl Into<String>) -> Self {
        Self {
            code: "author_created_story_unproven",
            message: message.into(),
        }
    }
}

impl fmt::Display for AuthorCreatedStoryProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AuthorCreatedStoryProofError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditOperation {
    ReplaceStoryRange {
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        expected_before: String,
        replacement_text: String,
        before_story_state_id: String,
        after_story_state_id: String,
    },
    ReplaceStoryText {
        story_id: StoryId,
        before: String,
        after: String,
    },
    BreakTextFrameForwardLink {
        story_id: StoryId,
        upstream_frame_id: NodeId,
        downstream_frame_id: NodeId,
        new_story_id: StoryId,
        before_frames: Vec<StoryFrame<StoryId, NodeId>>,
        after_frames: Vec<StoryFrame<StoryId, NodeId>>,
    },
    LinkTextFrameTail {
        transition: TextFrameLinkTransitionV1,
    },
    ReplaceTableCellText {
        node_id: NodeId,
        story_id: StoryId,
        cell_id: TableCellId,
        before_story: String,
        after_story: String,
        before_ranges: Vec<TableCellRangeSnapshot>,
        after_ranges: Vec<TableCellRangeSnapshot>,
    },
    ReplaceImage {
        node_id: NodeId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_asset: Option<Sha256Digest>,
        after_asset: Sha256Digest,
    },
    SetImageCrop {
        node_id: NodeId,
        before: ImageCropStateV1,
        after: ImageCropStateV1,
    },
    MoveNode {
        node_id: NodeId,
        before: RectEmu,
        after: RectEmu,
    },
    MoveNodes {
        page_id: PageId,
        entries: Vec<MoveNodeBatchEntry>,
    },
    ResizeNode {
        node_id: NodeId,
        before: RectEmu,
        after: RectEmu,
    },
    ResizeNodes {
        page_id: PageId,
        entries: Vec<ResizeNodeBatchEntry>,
    },
    CreateTextBox {
        node_id: NodeId,
        story_id: StoryId,
        page_id: PageId,
        bounds: RectEmu,
        text_preset: AuthoringTextPresetV1,
    },
    CreateShape {
        node_id: NodeId,
        page_id: PageId,
        parent_id: PageId,
        shape_kind: AuthoredShapeKindV1,
        bounds: RectEmu,
        transform: AuthoredShapeTransformV1,
        paint: AuthoredShapePaintV1,
        provenance: AuthoredEntityProvenanceV1,
    },
    CreateLine {
        node_id: NodeId,
        page_id: PageId,
        parent_id: PageId,
        geometry: LineGeometryV1,
        stroke: AuthoredSolidStrokeV1,
        provenance: AuthoredEntityProvenanceV1,
    },
    CreateTable {
        table: CreateTableRuntimeV1,
    },
    SetTableTrackExtent {
        history: SetTableTrackExtentHistoryV1,
    },
    InsertTableRow {
        history: TableRowColHistoryV1,
    },
    DeleteTableRow {
        history: TableRowColHistoryV1,
    },
    InsertTableColumn {
        history: TableRowColHistoryV1,
    },
    DeleteTableColumn {
        history: TableRowColHistoryV1,
    },
    DeleteNode {
        node_id: NodeId,
        page_id: PageId,
        before: AuthoredShapeRuntimeV1,
        before_state_id: String,
    },
    ReorderAuthoredStack {
        transition: AuthoredStackReorderTransitionV1,
    },
    SetTextFormatProperty {
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        value: FormatValueV1,
        before_state_hash: String,
        after_state_hash: String,
    },
    ClearTextFormatPropertyOverride {
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        before_state_hash: String,
        after_state_hash: String,
    },
    SetTextFormatPropertyScopedV1 {
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        value: FormatValueV1,
        before_state_hash: String,
        after_state_hash: String,
    },
    ClearTextFormatPropertyOverrideScopedV1 {
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        before_state_hash: String,
        after_state_hash: String,
    },
    SetParagraphAlignmentOverride {
        paragraph_ids: Vec<ParagraphId>,
        value: AuthoredParagraphAlignmentValueV1,
        before: Vec<ParagraphAlignmentOverrideSnapshotV1>,
        after: Vec<ParagraphAlignmentOverrideSnapshotV1>,
    },
    ClearParagraphAlignmentOverride {
        paragraph_ids: Vec<ParagraphId>,
        before: Vec<ParagraphAlignmentOverrideSnapshotV1>,
        after: Vec<ParagraphAlignmentOverrideSnapshotV1>,
    },
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
            | Self::SetTextFormatProperty { .. }
            | Self::ClearTextFormatPropertyOverride { .. }
            | Self::SetTextFormatPropertyScopedV1 { .. }
            | Self::ClearTextFormatPropertyOverrideScopedV1 { .. }
            | Self::SetParagraphAlignmentOverride { .. }
            | Self::ClearParagraphAlignmentOverride { .. } => Vec::new(),
        }
    }
}

fn required_editor_asset_refs_v1(operations: &[EditOperation]) -> BTreeSet<Sha256Digest> {
    operations
        .iter()
        .flat_map(EditOperation::durable_editor_asset_refs_v1)
        .collect()
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCellRangeSnapshot {
    pub cell_id: TableCellId,
    pub utf16_start: u32,
    pub utf16_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorProjectAsset {
    pub sha256: Sha256Digest,
    pub mime: String,
    pub byte_len: u64,
}

impl EditorProjectAsset {
    pub fn file_name(&self) -> Result<String, EditorAssetError> {
        editor_asset_file_name(self.sha256, &self.mime)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorReplacementAsset {
    pub sha256: Sha256Digest,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorCurrentImageResourceV1 {
    pub resource_id: ResourceId,
    pub mime: String,
    pub node_ids: Vec<NodeId>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCurrentImageResourceError {
    MissingSourceAsset { resource_id: ResourceId },
    MissingReplacementAsset { sha256: Sha256Digest },
    ResourceIdentityCollision { resource_id: ResourceId },
}

impl fmt::Display for EditorCurrentImageResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSourceAsset { resource_id } => write!(
                formatter,
                "current image source resource {} has no exact byte backing",
                resource_id.as_canonical()
            ),
            Self::MissingReplacementAsset { sha256 } => write!(
                formatter,
                "current image replacement asset {sha256} has no exact byte backing"
            ),
            Self::ResourceIdentityCollision { resource_id } => write!(
                formatter,
                "current image resource {} resolved to conflicting byte payloads",
                resource_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for EditorCurrentImageResourceError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorSourceImageAsset {
    mime: String,
    bytes: Vec<u8>,
}

fn current_image_resources_v1(
    source_image_assets: &BTreeMap<ResourceId, EditorSourceImageAsset>,
    source_image_nodes: &BTreeMap<NodeId, ResourceId>,
    replacement_assets: &BTreeMap<Sha256Digest, EditorReplacementAsset>,
    image_replacements: &BTreeMap<NodeId, Sha256Digest>,
) -> Result<Vec<EditorCurrentImageResourceV1>, EditorCurrentImageResourceError> {
    let mut resources = BTreeMap::<ResourceId, (String, Vec<u8>, BTreeSet<NodeId>)>::new();

    for (node_id, resource_id) in source_image_nodes {
        if image_replacements.contains_key(node_id) {
            continue;
        }
        let asset = source_image_assets.get(resource_id).ok_or(
            EditorCurrentImageResourceError::MissingSourceAsset {
                resource_id: *resource_id,
            },
        )?;
        match resources.entry(*resource_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((
                    asset.mime.clone(),
                    asset.bytes.clone(),
                    BTreeSet::from([*node_id]),
                ));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (mime, bytes, node_ids) = entry.get_mut();
                if mime != &asset.mime || bytes != &asset.bytes {
                    return Err(EditorCurrentImageResourceError::ResourceIdentityCollision {
                        resource_id: *resource_id,
                    });
                }
                node_ids.insert(*node_id);
            }
        }
    }

    for (node_id, asset_sha) in image_replacements {
        let asset = replacement_assets.get(asset_sha).ok_or(
            EditorCurrentImageResourceError::MissingReplacementAsset { sha256: *asset_sha },
        )?;
        let resource_id = replacement_asset_resource_id(*asset_sha);
        match resources.entry(resource_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((
                    asset.mime.clone(),
                    asset.bytes.clone(),
                    BTreeSet::from([*node_id]),
                ));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (mime, bytes, node_ids) = entry.get_mut();
                if mime != &asset.mime || bytes != &asset.bytes {
                    return Err(EditorCurrentImageResourceError::ResourceIdentityCollision {
                        resource_id,
                    });
                }
                node_ids.insert(*node_id);
            }
        }
    }

    Ok(resources
        .into_iter()
        .map(
            |(resource_id, (mime, bytes, node_ids))| EditorCurrentImageResourceV1 {
                resource_id,
                mime,
                node_ids: node_ids.into_iter().collect(),
                bytes,
            },
        )
        .collect())
}

#[derive(Debug, Clone, Copy)]
struct EditableExportImageState<'a> {
    replacements: &'a BTreeMap<NodeId, Sha256Digest>,
    crop_overrides: &'a BTreeMap<NodeId, ImageCropStateV1>,
    source_nodes: &'a BTreeMap<NodeId, ResourceId>,
}

fn source_image_context_from_bundle(
    bundle: PubAssetExportBundle,
) -> (
    BTreeMap<ResourceId, EditorSourceImageAsset>,
    BTreeMap<NodeId, ResourceId>,
) {
    let mut files = bundle
        .files
        .into_iter()
        .map(|file| (file.resource_id, file.bytes))
        .collect::<BTreeMap<_, _>>();
    let mut assets = BTreeMap::new();
    let mut nodes = BTreeMap::new();

    for entry in bundle.manifest.assets {
        if !matches!(entry.mime.as_str(), "image/png" | "image/jpeg") {
            continue;
        }
        let Some(bytes) = files.remove(&entry.resource_id) else {
            continue;
        };
        assets.insert(
            entry.resource_id,
            EditorSourceImageAsset {
                mime: entry.mime,
                bytes,
            },
        );
        for usage in entry.uses {
            nodes.insert(usage.node_id, entry.resource_id);
        }
    }

    (assets, nodes)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorProjectForkProvenance {
    pub project_id: String,
    pub document_id: String,
    pub history_id: String,
    pub state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorProjectIdentity {
    pub project_id: String,
    pub document_id: String,
    pub history_id: String,
    pub genesis_revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<EditorProjectForkProvenance>,
}

fn new_project_identity() -> EditorProjectIdentity {
    EditorProjectIdentity {
        project_id: Uuid::now_v7().to_string(),
        document_id: Uuid::now_v7().to_string(),
        history_id: Uuid::now_v7().to_string(),
        genesis_revision_id: Uuid::now_v7().to_string(),
        forked_from: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorProject {
    pub schema_version: String,
    pub source_hash: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<EditorProjectIdentity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<EditorProjectAsset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table_grids: Vec<EffectiveTableGridV1>,
    pub operations: Vec<EditOperation>,
}

impl EditorProject {
    pub fn state_id_v1(&self) -> String {
        let payload = serde_json::json!({
            "protocol_version": "chaptera.editor-project-state.v1",
            "source_hash": self.source_hash,
            "assets": self.assets,
            "table_grids": self.table_grids,
            "operations": self.operations,
        });
        let bytes = serde_json::to_vec(&payload)
            .expect("canonical EditorProject state JSON serialization cannot fail");
        let digest = Sha256::digest(bytes);
        let mut encoded = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut encoded, "{byte:02x}")
                .expect("writing lowercase hex into String cannot fail");
        }
        format!("sha256:{encoded}")
    }

    pub fn fork_next_issue(&self) -> Result<Self, EditorProjectForkError> {
        let parent = self
            .identity
            .as_ref()
            .ok_or(EditorProjectForkError::MissingProjectIdentity)?;
        let state_id = self.state_id_v1();
        let mut identity = new_project_identity();
        identity.forked_from = Some(EditorProjectForkProvenance {
            project_id: parent.project_id.clone(),
            document_id: parent.document_id.clone(),
            history_id: parent.history_id.clone(),
            state_id,
        });

        Ok(Self {
            schema_version: self.schema_version.clone(),
            source_hash: self.source_hash,
            identity: Some(identity),
            assets: self.assets.clone(),
            table_grids: self.table_grids.clone(),
            operations: self.operations.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorProjectForkError {
    MissingProjectIdentity,
}

impl fmt::Display for EditorProjectForkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProjectIdentity => {
                formatter.write_str("editor project has no durable project identity")
            }
        }
    }
}

impl std::error::Error for EditorProjectForkError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAssetError {
    EmptyBytes,
    UnsupportedMime {
        mime: String,
    },
    SignatureMismatch {
        mime: String,
    },
    ByteLengthOverflow,
    MimeConflict {
        sha256: Sha256Digest,
        existing: String,
        requested: String,
    },
}

impl fmt::Display for EditorAssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBytes => formatter.write_str("replacement asset is empty"),
            Self::UnsupportedMime { mime } => {
                write!(formatter, "replacement asset MIME {mime:?} is unsupported")
            }
            Self::SignatureMismatch { mime } => write!(
                formatter,
                "replacement asset bytes do not match declared MIME {mime:?}"
            ),
            Self::ByteLengthOverflow => {
                formatter.write_str("replacement asset length does not fit u64")
            }
            Self::MimeConflict {
                sha256,
                existing,
                requested,
            } => write!(
                formatter,
                "replacement asset {sha256} is already registered as {existing:?}, not {requested:?}"
            ),
        }
    }
}

impl std::error::Error for EditorAssetError {}

impl PersistenceRequirements for EditorProject {
    fn persistence_requirements(&self) -> Vec<PersistenceRequirement> {
        let mut requirements = self
            .operations
            .iter()
            .flat_map(PersistenceRequirements::persistence_requirements)
            .collect::<Vec<_>>();
        requirements.sort();
        requirements.dedup();
        requirements
    }
}

pub fn mature_0x2c_pub_persistence_target() -> PersistenceTargetProfile {
    PersistenceTargetProfile {
        format: "pub".into(),
        profile: PUB_MATURE_0X2C_PERSISTENCE_PROFILE.into(),
        schema_fence: Some(PUB_MATURE_0X2C_SCHEMA_FENCE.into()),
    }
}

/// Ограниченный профиль представимости semantic-классов, уже материализованных
/// mature-0x2C моделью Publisher.
///
/// Это не утверждает, что текущий Rust native writer способен записать каждый
/// конкретный instance. Writer readiness намеренно передаётся отдельным manifest.
pub fn mature_0x2c_pub_format_manifest() -> FormatCompatibilityManifest {
    let mut features = BTreeMap::new();
    features.insert("story.text".into(), FormatRepresentability::Lossless);
    features.insert("table.cell_text".into(), FormatRepresentability::Lossless);
    features.insert("image.replacement".into(), FormatRepresentability::Lossless);
    features.insert(
        "node.geometry.position".into(),
        FormatRepresentability::Lossless,
    );
    features.insert(
        "node.geometry.bounds".into(),
        FormatRepresentability::Lossless,
    );
    // CreateShape now emits explicit requirements for created identity and
    // authored paint, but mature-0x2C representability for those semantics is
    // not yet proven. Leaving the feature keys absent deliberately yields
    // NotEvaluated rather than silently upgrading source-format authority.

    FormatCompatibilityManifest {
        target: mature_0x2c_pub_persistence_target(),
        features,
        scoped: Vec::new(),
    }
}

pub fn assess_mature_0x2c_pub_project_persistence(
    project: &EditorProject,
    writer: &WriterCapabilityManifest,
) -> Result<PersistenceCompatibilityAssessment, PersistenceCompatibilityError> {
    assess_persistence_compatibility(
        &mature_0x2c_pub_format_manifest(),
        writer,
        project.persistence_requirements(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorError {
    SourceIdentityChanged,
    MissingStory {
        story_id: StoryId,
    },
    RichStoryUnsupported {
        story_id: StoryId,
    },
    TableStoryUnsupported {
        story_id: StoryId,
    },
    FrameCountUnsupported {
        story_id: StoryId,
        found: usize,
    },
    FrameTopologyUnsupported {
        story_id: StoryId,
        errors: usize,
    },
    BreakLinkUnsupported {
        upstream_frame_id: NodeId,
        downstream_frame_id: NodeId,
    },
    LinkTextFrameUnsupported {
        source_frame_id: NodeId,
        target_frame_id: NodeId,
        reason: String,
    },
    NewStoryIdInvalid {
        story_id: StoryId,
    },
    NewStoryIdConflict {
        story_id: StoryId,
    },
    StaleFrameTopology {
        story_id: StoryId,
    },
    TableEditUnsupported {
        node_id: NodeId,
    },
    MissingTableCell {
        node_id: NodeId,
        cell_id: TableCellId,
    },
    TableCellNoChange {
        node_id: NodeId,
        cell_id: TableCellId,
    },
    TableTrackResizeUnsupported {
        node_id: NodeId,
    },
    StaleTableTrackResize {
        node_id: NodeId,
    },
    TableRowColUnsupported {
        node_id: NodeId,
    },
    StaleTableRowCol {
        node_id: NodeId,
    },
    ImageReplaceUnsupported {
        node_id: NodeId,
    },
    MissingReplacementAsset {
        sha256: Sha256Digest,
    },
    ImageReplacementNoChange {
        node_id: NodeId,
        sha256: Sha256Digest,
    },
    StaleImageOperation {
        node_id: NodeId,
    },
    ImageCropUnsupported {
        node_id: NodeId,
    },
    ImageCropNoChange {
        node_id: NodeId,
    },
    StaleImageCrop {
        node_id: NodeId,
    },
    CreateTextBoxInvalidNodeId {
        node_id: NodeId,
    },
    CreateTextBoxInvalidStoryId {
        story_id: StoryId,
    },
    CreateTextBoxPageMissing {
        page_id: PageId,
    },
    CreateTextBoxNodeIdCollision {
        node_id: NodeId,
    },
    CreateTextBoxStoryIdCollision {
        story_id: StoryId,
    },
    CreateTextBoxInvalidBounds {
        node_id: NodeId,
    },
    CreateTextBoxInvalidTextPreset,
    StaleCreateTextBox {
        node_id: NodeId,
        story_id: StoryId,
    },
    CreateShapeInvalidNodeId {
        node_id: NodeId,
    },
    CreateShapePageMissing {
        page_id: PageId,
    },
    CreateShapeIdCollision {
        node_id: NodeId,
    },
    CreateShapeInvalidBounds {
        node_id: NodeId,
    },
    CreateShapeInvalidPaint {
        node_id: NodeId,
    },
    CreateShapeInvalidProvenance {
        node_id: NodeId,
    },
    CreateShapeMalformed {
        node_id: NodeId,
    },
    CreateLineInvalidNodeId {
        node_id: NodeId,
    },
    CreateLinePageMissing {
        page_id: PageId,
    },
    CreateLineIdCollision {
        node_id: NodeId,
    },
    CreateLineInvalidGeometry {
        node_id: NodeId,
    },
    CreateLineInvalidStroke {
        node_id: NodeId,
    },
    CreateLineInvalidProvenance {
        node_id: NodeId,
    },
    CreateLineMalformed {
        node_id: NodeId,
    },
    NodeDeleteUnsupported {
        node_id: NodeId,
    },
    NodeDeletePageMismatch {
        node_id: NodeId,
        page_id: PageId,
    },
    StaleNodeDelete {
        node_id: NodeId,
    },
    AuthoredStackReorderUnsupported {
        node_id: NodeId,
    },
    AuthoredStackReorderNoChange {
        node_id: NodeId,
    },
    StaleAuthoredStack {
        page_id: PageId,
    },
    NodeMoveUnsupported {
        node_id: NodeId,
    },
    NodeMoveNoChange {
        node_id: NodeId,
    },
    NodeMoveOverflow {
        node_id: NodeId,
    },
    StaleNodeMove {
        node_id: NodeId,
    },
    MoveNodesEmpty,
    MoveNodesTooLarge {
        found: usize,
    },
    MoveNodesDuplicate {
        node_id: NodeId,
    },
    MoveNodesSizeChanged {
        node_id: NodeId,
    },
    MoveNodesPageMismatch {
        node_id: NodeId,
        page_id: PageId,
    },
    ResizeNodesInvalidCount {
        found: usize,
    },
    ResizeNodesDuplicate {
        node_id: NodeId,
    },
    ResizeNodesNotCanonical {
        node_id: NodeId,
    },
    ResizeNodesPageMismatch {
        node_id: NodeId,
        page_id: PageId,
    },
    ResizeNodesNoSizeChange,
    NodeResizeUnsupported {
        node_id: NodeId,
    },
    NodeResizeNoChange {
        node_id: NodeId,
    },
    NodeResizeNoSizeChange {
        node_id: NodeId,
    },
    NodeResizeNonPositive {
        node_id: NodeId,
    },
    NodeResizeOverflow {
        node_id: NodeId,
    },
    StaleNodeResize {
        node_id: NodeId,
    },
    NoChange {
        story_id: StoryId,
    },
    StaleOperation {
        story_id: StoryId,
    },
    TextFormatUnsupported {
        story_id: StoryId,
        reason: String,
    },
    TextFormatStateInvalid {
        story_id: StoryId,
        message: String,
    },
    TextFormatTextMutationConflict {
        story_id: StoryId,
    },
    ParagraphAlignmentTargetsEmpty,
    ParagraphAlignmentProjectionUnavailable,
    ParagraphAlignmentParagraphUnavailable {
        paragraph_id: ParagraphId,
    },
    ParagraphAlignmentNoChange,
    StaleParagraphAlignmentOverride {
        paragraph_id: ParagraphId,
    },
    ParagraphAlignmentTransitionInvalid,
    ParagraphAlignmentLifecycleUnsupported {
        story_id: StoryId,
    },
    NothingToUndo,
    NothingToRedo,
}

impl fmt::Display for EditorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceIdentityChanged => {
                formatter.write_str("editor source identity changed after session creation")
            }
            Self::MissingStory { story_id } => write!(
                formatter,
                "story {} is not present in the resolved authoring graph",
                story_id.as_canonical()
            ),
            Self::RichStoryUnsupported { story_id } => write!(
                formatter,
                "story {} carries paragraph/run/field/hyperlink semantics that ReplaceStoryText does not remap",
                story_id.as_canonical()
            ),
            Self::TableStoryUnsupported { story_id } => write!(
                formatter,
                "story {} is owned by a table and is outside the first editor slice",
                story_id.as_canonical()
            ),
            Self::FrameCountUnsupported { story_id, found } => write!(
                formatter,
                "story {} is referenced by {found} ordinary frames; ReplaceStoryText requires at least one materialized frame",
                story_id.as_canonical()
            ),
            Self::FrameTopologyUnsupported { story_id, errors } => write!(
                formatter,
                "story {} has unsupported or inconsistent multi-frame topology ({errors} validation errors)",
                story_id.as_canonical()
            ),
            Self::BreakLinkUnsupported {
                upstream_frame_id,
                downstream_frame_id,
            } => write!(
                formatter,
                "text-frame edge {} -> {} is not an admitted explicit Story chain edge",
                upstream_frame_id.as_canonical(),
                downstream_frame_id.as_canonical()
            ),
            Self::NewStoryIdInvalid { story_id } => write!(
                formatter,
                "new Story identity {} is not an editor-created UUIDv7",
                story_id.as_canonical()
            ),
            Self::LinkTextFrameUnsupported { reason, .. } => formatter.write_str(reason),
            Self::NewStoryIdConflict { story_id } => write!(
                formatter,
                "new Story identity {} already exists",
                story_id.as_canonical()
            ),
            Self::StaleFrameTopology { story_id } => write!(
                formatter,
                "story {} frame topology changed since the persisted operation",
                story_id.as_canonical()
            ),
            Self::TableEditUnsupported { node_id } => write!(
                formatter,
                "table node {} is outside the bounded simple-cell text edit slice",
                node_id.as_canonical()
            ),
            Self::MissingTableCell { node_id, cell_id } => write!(
                formatter,
                "table node {} does not contain cell {}",
                node_id.as_canonical(),
                cell_id.as_canonical()
            ),
            Self::TableCellNoChange { node_id, cell_id } => write!(
                formatter,
                "replacement text for table node {} cell {} is identical to current text",
                node_id.as_canonical(),
                cell_id.as_canonical()
            ),
            Self::TableTrackResizeUnsupported { node_id } => write!(
                formatter,
                "table node {} is outside the bounded track-resize slice",
                node_id.as_canonical()
            ),
            Self::StaleTableTrackResize { node_id } => write!(
                formatter,
                "table node {} track-resize state no longer matches persisted history",
                node_id.as_canonical()
            ),
            Self::TableRowColUnsupported { node_id } => write!(
                formatter,
                "table node {} is outside the bounded row/column lifecycle slice",
                node_id.as_canonical()
            ),
            Self::StaleTableRowCol { node_id } => write!(
                formatter,
                "table node {} row/column state no longer matches persisted history",
                node_id.as_canonical()
            ),
            Self::ImageReplaceUnsupported { node_id } => write!(
                formatter,
                "image node {} is outside the bounded image replacement slice",
                node_id.as_canonical()
            ),
            Self::MissingReplacementAsset { sha256 } => write!(
                formatter,
                "replacement image asset {sha256} is not registered in this editor session"
            ),
            Self::ImageReplacementNoChange { node_id, sha256 } => write!(
                formatter,
                "image node {} already uses replacement asset {sha256}",
                node_id.as_canonical()
            ),
            Self::StaleImageOperation { node_id } => write!(
                formatter,
                "image node {} no longer matches the replacement operation precondition",
                node_id.as_canonical()
            ),
            Self::ImageCropUnsupported { node_id } => write!(
                formatter,
                "image node {} is outside the bounded source-backed crop slice",
                node_id.as_canonical()
            ),
            Self::ImageCropNoChange { node_id } => write!(
                formatter,
                "image node {} already has the requested crop state",
                node_id.as_canonical()
            ),
            Self::StaleImageCrop { node_id } => write!(
                formatter,
                "image node {} no longer matches the expected crop state",
                node_id.as_canonical()
            ),
            Self::CreateTextBoxInvalidNodeId { node_id } => write!(
                formatter,
                "CreateTextBox node {} is not an editor-created UUIDv7",
                node_id.as_canonical()
            ),
            Self::CreateTextBoxInvalidStoryId { story_id } => write!(
                formatter,
                "CreateTextBox Story {} is not an editor-created UUIDv7",
                story_id.as_canonical()
            ),
            Self::CreateTextBoxPageMissing { page_id } => write!(
                formatter,
                "CreateTextBox page {} is not present in the opened document",
                page_id.as_canonical()
            ),
            Self::CreateTextBoxNodeIdCollision { node_id } => write!(
                formatter,
                "CreateTextBox node {} collides with an existing visual node",
                node_id.as_canonical()
            ),
            Self::CreateTextBoxStoryIdCollision { story_id } => write!(
                formatter,
                "CreateTextBox Story {} already exists",
                story_id.as_canonical()
            ),
            Self::CreateTextBoxInvalidBounds { node_id } => write!(
                formatter,
                "CreateTextBox node {} has invalid or unsafe bounds",
                node_id.as_canonical()
            ),
            Self::CreateTextBoxInvalidTextPreset => formatter.write_str(
                "CreateTextBox requires a non-empty fingerprinted deterministic text preset with positive font metrics",
            ),
            Self::StaleCreateTextBox { node_id, story_id } => write!(
                formatter,
                "CreateTextBox node {} / Story {} no longer matches its atomic replay precondition",
                node_id.as_canonical(),
                story_id.as_canonical()
            ),
            Self::CreateShapeInvalidNodeId { node_id } => write!(
                formatter,
                "CreateShape node {} is not an editor-created UUIDv7",
                node_id.as_canonical()
            ),
            Self::CreateShapePageMissing { page_id } => write!(
                formatter,
                "CreateShape page {} is not present in the opened document",
                page_id.as_canonical()
            ),
            Self::CreateShapeIdCollision { node_id } => write!(
                formatter,
                "CreateShape node {} collides with an existing visual node",
                node_id.as_canonical()
            ),
            Self::CreateShapeInvalidBounds { node_id } => write!(
                formatter,
                "CreateShape node {} has invalid or unsafe bounds",
                node_id.as_canonical()
            ),
            Self::CreateShapeInvalidPaint { node_id } => write!(
                formatter,
                "CreateShape node {} has invalid explicit fill/stroke paint",
                node_id.as_canonical()
            ),
            Self::CreateShapeInvalidProvenance { node_id } => write!(
                formatter,
                "CreateShape node {} is not explicitly author-created",
                node_id.as_canonical()
            ),
            Self::CreateShapeMalformed { node_id } => write!(
                formatter,
                "CreateShape node {} violates the bounded rectangle/identity/page contract",
                node_id.as_canonical()
            ),
            Self::CreateLineInvalidNodeId { node_id } => write!(
                formatter,
                "CreateLine node {} is not an editor-created UUIDv7",
                node_id.as_canonical()
            ),
            Self::CreateLinePageMissing { page_id } => write!(
                formatter,
                "CreateLine page {} is not present in the opened document",
                page_id.as_canonical()
            ),
            Self::CreateLineIdCollision { node_id } => write!(
                formatter,
                "CreateLine node {} collides with an existing visual node",
                node_id.as_canonical()
            ),
            Self::CreateLineInvalidGeometry { node_id } => write!(
                formatter,
                "CreateLine node {} has invalid or unsafe ordered endpoint geometry",
                node_id.as_canonical()
            ),
            Self::CreateLineInvalidStroke { node_id } => write!(
                formatter,
                "CreateLine node {} has invalid explicit stroke",
                node_id.as_canonical()
            ),
            Self::CreateLineInvalidProvenance { node_id } => write!(
                formatter,
                "CreateLine node {} is not explicitly author-created",
                node_id.as_canonical()
            ),
            Self::CreateLineMalformed { node_id } => write!(
                formatter,
                "CreateLine node {} violates the bounded identity/page contract",
                node_id.as_canonical()
            ),
            Self::NodeDeleteUnsupported { node_id } => write!(
                formatter,
                "node {} is not an admitted author-created direct page-owned Rectangle",
                node_id.as_canonical()
            ),
            Self::NodeDeletePageMismatch { node_id, page_id } => write!(
                formatter,
                "DeleteNode node {} does not belong to persisted page {}",
                node_id.as_canonical(),
                page_id.as_canonical()
            ),
            Self::StaleNodeDelete { node_id } => write!(
                formatter,
                "node {} no longer matches the DeleteNode authored-state precondition",
                node_id.as_canonical()
            ),
            Self::AuthoredStackReorderUnsupported { node_id } => write!(
                formatter,
                "node {} is not an admitted author-created member of the authored stack",
                node_id.as_canonical()
            ),
            Self::AuthoredStackReorderNoChange { node_id } => write!(
                formatter,
                "node {} is already at the requested authored-stack position",
                node_id.as_canonical()
            ),
            Self::StaleAuthoredStack { page_id } => write!(
                formatter,
                "authored stack for page {} no longer matches the persisted transition precondition",
                page_id.as_canonical()
            ),
            Self::NodeMoveUnsupported { node_id } => write!(
                formatter,
                "node {} is outside the bounded directly-page-owned move slice",
                node_id.as_canonical()
            ),
            Self::NodeMoveNoChange { node_id } => write!(
                formatter,
                "node {} already has the requested authored position",
                node_id.as_canonical()
            ),
            Self::NodeMoveOverflow { node_id } => write!(
                formatter,
                "node {} move would overflow canonical EMU bounds",
                node_id.as_canonical()
            ),
            Self::StaleNodeMove { node_id } => write!(
                formatter,
                "node {} no longer matches the move operation precondition",
                node_id.as_canonical()
            ),
            Self::MoveNodesEmpty => formatter.write_str("MoveNodes requires at least one entry"),
            Self::MoveNodesTooLarge { found } => write!(
                formatter,
                "MoveNodes contains {found} entries; maximum is {MAX_MOVE_NODES_V1}"
            ),
            Self::MoveNodesDuplicate { node_id } => write!(
                formatter,
                "MoveNodes contains duplicate node {}",
                node_id.as_canonical()
            ),
            Self::MoveNodesSizeChanged { node_id } => write!(
                formatter,
                "MoveNodes entry for node {} changes width or height",
                node_id.as_canonical()
            ),
            Self::MoveNodesPageMismatch { node_id, page_id } => write!(
                formatter,
                "MoveNodes node {} is not directly owned by page {}",
                node_id.as_canonical(),
                page_id.as_canonical()
            ),
            Self::ResizeNodesInvalidCount { found } => write!(
                formatter,
                "ResizeNodes contains {found} entries; expected 2..={MAX_RESIZE_NODES_V1}"
            ),
            Self::ResizeNodesDuplicate { node_id } => write!(
                formatter,
                "ResizeNodes contains duplicate node {}",
                node_id.as_canonical()
            ),
            Self::ResizeNodesNotCanonical { node_id } => write!(
                formatter,
                "ResizeNodes entries are not in canonical NodeId order at node {}",
                node_id.as_canonical()
            ),
            Self::ResizeNodesPageMismatch { node_id, page_id } => write!(
                formatter,
                "ResizeNodes node {} is not directly owned by page {}",
                node_id.as_canonical(),
                page_id.as_canonical()
            ),
            Self::ResizeNodesNoSizeChange => formatter
                .write_str("ResizeNodes must include at least one genuine width or height change"),
            Self::NodeResizeUnsupported { node_id } => write!(
                formatter,
                "node {} is outside the bounded directly-page-owned resize slice",
                node_id.as_canonical()
            ),
            Self::NodeResizeNoChange { node_id } => write!(
                formatter,
                "node {} already has the requested authored bounds",
                node_id.as_canonical()
            ),
            Self::NodeResizeNoSizeChange { node_id } => write!(
                formatter,
                "node {} ResizeNode request does not change width or height",
                node_id.as_canonical()
            ),
            Self::NodeResizeNonPositive { node_id } => write!(
                formatter,
                "node {} resize requires strictly positive width and height",
                node_id.as_canonical()
            ),
            Self::NodeResizeOverflow { node_id } => write!(
                formatter,
                "node {} resize would overflow canonical EMU bounds",
                node_id.as_canonical()
            ),
            Self::StaleNodeResize { node_id } => write!(
                formatter,
                "node {} no longer matches the resize operation precondition",
                node_id.as_canonical()
            ),
            Self::NoChange { story_id } => write!(
                formatter,
                "replacement text for story {} is identical to the current text",
                story_id.as_canonical()
            ),
            Self::StaleOperation { story_id } => write!(
                formatter,
                "story {} no longer matches the edit operation precondition",
                story_id.as_canonical()
            ),
            Self::TextFormatUnsupported { story_id, reason } => write!(
                formatter,
                "story {} is outside the bounded text-format authoring slice: {reason}",
                story_id.as_canonical()
            ),
            Self::TextFormatStateInvalid { story_id, message } => write!(
                formatter,
                "story {} text-format state is invalid: {message}",
                story_id.as_canonical()
            ),
            Self::TextFormatTextMutationConflict { story_id } => write!(
                formatter,
                "story {} has active character-format history and cannot change text until range rebasing is implemented",
                story_id.as_canonical()
            ),
            Self::ParagraphAlignmentTargetsEmpty => {
                formatter.write_str("paragraph alignment operation requires at least one ParagraphId")
            }
            Self::ParagraphAlignmentProjectionUnavailable => formatter.write_str(
                "current Story state does not expose a canonical imported Paragraph projection",
            ),
            Self::ParagraphAlignmentParagraphUnavailable { paragraph_id } => write!(
                formatter,
                "paragraph {} is not a current imported Paragraph target",
                paragraph_id.as_canonical()
            ),
            Self::ParagraphAlignmentNoChange => formatter
                .write_str("paragraph alignment operation produces no canonical state change"),
            Self::StaleParagraphAlignmentOverride { paragraph_id } => write!(
                formatter,
                "paragraph alignment override for {} changed since the operation was recorded",
                paragraph_id.as_canonical()
            ),
            Self::ParagraphAlignmentTransitionInvalid => formatter
                .write_str("paragraph alignment override transition is not canonical"),
            Self::ParagraphAlignmentLifecycleUnsupported { story_id } => write!(
                formatter,
                "story {} has applied paragraph alignment history; Story text/topology mutation is fenced until ParagraphId lifecycle rebasing is implemented",
                story_id.as_canonical()
            ),
            Self::NothingToUndo => formatter.write_str("editor session has nothing to undo"),
            Self::NothingToRedo => formatter.write_str("editor session has nothing to redo"),
        }
    }
}

impl EditorError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SourceIdentityChanged => "source_identity_changed",
            Self::MissingStory { .. } => "missing_story",
            Self::RichStoryUnsupported { .. } => "rich_story_unsupported",
            Self::TableStoryUnsupported { .. } => "table_story_unsupported",
            Self::FrameCountUnsupported { .. } => "frame_count_unsupported",
            Self::FrameTopologyUnsupported { .. } => "frame_topology_unsupported",
            Self::BreakLinkUnsupported { .. } => "break_link_unsupported",
            Self::LinkTextFrameUnsupported { .. } => "link_text_frame_unsupported",
            Self::NewStoryIdInvalid { .. } => "new_story_id_invalid",
            Self::NewStoryIdConflict { .. } => "new_story_id_conflict",
            Self::StaleFrameTopology { .. } => "stale_frame_topology",
            Self::TableEditUnsupported { .. } => "table_edit_unsupported",
            Self::MissingTableCell { .. } => "missing_table_cell",
            Self::TableCellNoChange { .. } => "table_cell_no_change",
            Self::ImageReplaceUnsupported { .. } => "image_replace_unsupported",
            Self::MissingReplacementAsset { .. } => "missing_replacement_asset",
            Self::ImageReplacementNoChange { .. } => "image_replacement_no_change",
            Self::StaleImageOperation { .. } => "stale_image_operation",
            Self::ImageCropUnsupported { .. } => "image_crop_unsupported",
            Self::ImageCropNoChange { .. } => "image_crop_no_change",
            Self::StaleImageCrop { .. } => "stale_image_crop",
            Self::CreateTextBoxInvalidNodeId { .. } => "create_text_box_invalid_node_id",
            Self::CreateTextBoxInvalidStoryId { .. } => "create_text_box_invalid_story_id",
            Self::CreateTextBoxPageMissing { .. } => "create_text_box_page_missing",
            Self::CreateTextBoxNodeIdCollision { .. } => "create_text_box_node_id_collision",
            Self::CreateTextBoxStoryIdCollision { .. } => "create_text_box_story_id_collision",
            Self::CreateTextBoxInvalidBounds { .. } => "create_text_box_invalid_bounds",
            Self::CreateTextBoxInvalidTextPreset => "create_text_box_invalid_text_preset",
            Self::StaleCreateTextBox { .. } => "stale_create_text_box",
            Self::CreateShapeInvalidNodeId { .. } => "create_shape_invalid_node_id",
            Self::CreateShapePageMissing { .. } => "create_shape_page_missing",
            Self::CreateShapeIdCollision { .. } => "create_shape_id_collision",
            Self::CreateShapeInvalidBounds { .. } => "create_shape_invalid_bounds",
            Self::CreateShapeInvalidPaint { .. } => "create_shape_invalid_paint",
            Self::CreateShapeInvalidProvenance { .. } => "create_shape_invalid_provenance",
            Self::CreateShapeMalformed { .. } => "create_shape_malformed",
            Self::CreateLineInvalidNodeId { .. } => "create_line_invalid_node_id",
            Self::CreateLinePageMissing { .. } => "create_line_page_missing",
            Self::CreateLineIdCollision { .. } => "create_line_id_collision",
            Self::CreateLineInvalidGeometry { .. } => "create_line_invalid_geometry",
            Self::CreateLineInvalidStroke { .. } => "create_line_invalid_stroke",
            Self::CreateLineInvalidProvenance { .. } => "create_line_invalid_provenance",
            Self::CreateLineMalformed { .. } => "create_line_malformed",
            Self::NodeDeleteUnsupported { .. } => "node_delete_unsupported",
            Self::NodeDeletePageMismatch { .. } => "node_delete_page_mismatch",
            Self::StaleNodeDelete { .. } => "stale_node_delete",
            Self::AuthoredStackReorderUnsupported { .. } => "authored_stack_reorder_unsupported",
            Self::AuthoredStackReorderNoChange { .. } => "authored_stack_reorder_no_change",
            Self::StaleAuthoredStack { .. } => "stale_authored_stack",
            Self::NodeMoveUnsupported { .. } => "node_move_unsupported",
            Self::NodeMoveNoChange { .. } => "node_move_no_change",
            Self::NodeMoveOverflow { .. } => "node_move_overflow",
            Self::StaleNodeMove { .. } => "stale_node_move",
            Self::MoveNodesEmpty => "move_nodes_empty",
            Self::MoveNodesTooLarge { .. } => "move_nodes_too_large",
            Self::MoveNodesDuplicate { .. } => "move_nodes_duplicate",
            Self::MoveNodesSizeChanged { .. } => "move_nodes_size_changed",
            Self::MoveNodesPageMismatch { .. } => "move_nodes_page_mismatch",
            Self::ResizeNodesInvalidCount { .. } => "resize_nodes_invalid_count",
            Self::ResizeNodesDuplicate { .. } => "resize_nodes_duplicate",
            Self::ResizeNodesNotCanonical { .. } => "resize_nodes_not_canonical",
            Self::ResizeNodesPageMismatch { .. } => "resize_nodes_page_mismatch",
            Self::ResizeNodesNoSizeChange => "resize_nodes_no_size_change",
            Self::NodeResizeUnsupported { .. } => "node_resize_unsupported",
            Self::NodeResizeNoChange { .. } => "node_resize_no_change",
            Self::NodeResizeNoSizeChange { .. } => "node_resize_no_size_change",
            Self::NodeResizeNonPositive { .. } => "node_resize_non_positive",
            Self::NodeResizeOverflow { .. } => "node_resize_overflow",
            Self::StaleNodeResize { .. } => "stale_node_resize",
            Self::TableTrackResizeUnsupported { .. } => "table_track_resize_unsupported",
            Self::StaleTableTrackResize { .. } => "stale_table_track_resize",
            Self::TableRowColUnsupported { .. } => "table_rowcol_unsupported",
            Self::StaleTableRowCol { .. } => "stale_table_rowcol",
            Self::NoChange { .. } => "no_change",
            Self::StaleOperation { .. } => "stale_operation",
            Self::TextFormatUnsupported { .. } => "text_format_unsupported",
            Self::TextFormatStateInvalid { .. } => "text_format_state_invalid",
            Self::TextFormatTextMutationConflict { .. } => "text_format_text_mutation_conflict",
            Self::ParagraphAlignmentTargetsEmpty => "paragraph_alignment_targets_empty",
            Self::ParagraphAlignmentProjectionUnavailable => {
                "paragraph_alignment_projection_unavailable"
            }
            Self::ParagraphAlignmentParagraphUnavailable { .. } => {
                "paragraph_alignment_paragraph_unavailable"
            }
            Self::ParagraphAlignmentNoChange => "paragraph_alignment_no_change",
            Self::StaleParagraphAlignmentOverride { .. } => "stale_paragraph_alignment_override",
            Self::ParagraphAlignmentTransitionInvalid => "paragraph_alignment_transition_invalid",
            Self::ParagraphAlignmentLifecycleUnsupported { .. } => {
                "paragraph_alignment_lifecycle_unsupported"
            }
            Self::NothingToUndo => "nothing_to_undo",
            Self::NothingToRedo => "nothing_to_redo",
        }
    }
}

impl std::error::Error for EditorError {}

#[derive(Debug)]
pub enum EditorOpenError {
    SourceGraph(String),
    Resolve(String),
    Session(EditorError),
}

impl fmt::Display for EditorOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceGraph(message) => write!(
                formatter,
                "could not build the mature-0x2C source graph for the editor: {message}"
            ),
            Self::Resolve(message) => write!(
                formatter,
                "could not resolve the mature-0x2C authoring graph for the editor: {message}"
            ),
            Self::Session(error) => write!(formatter, "could not create editor session: {error}"),
        }
    }
}

impl std::error::Error for EditorOpenError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorTextFormatBaseErrorV1 {
    SourceIdentityChanged,
    MissingStory { story_id: StoryId },
    StoryChanged { story_id: StoryId },
    UnsupportedBase { story_id: StoryId, reason: String },
    Overlay { story_id: StoryId, message: String },
}

impl fmt::Display for EditorTextFormatBaseErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceIdentityChanged => formatter
                .write_str("editor source identity changed before text-format base projection"),
            Self::MissingStory { story_id } => write!(
                formatter,
                "text-format base Story {} is missing",
                story_id.as_canonical()
            ),
            Self::StoryChanged { story_id } => write!(
                formatter,
                "text-format base Story {} no longer matches its immutable source text revision",
                story_id.as_canonical()
            ),
            Self::UnsupportedBase { story_id, reason } => write!(
                formatter,
                "text-format base for Story {} is unsupported: {reason}",
                story_id.as_canonical()
            ),
            Self::Overlay { story_id, message } => write!(
                formatter,
                "text-format overlay rejected Story {} base: {message}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for EditorTextFormatBaseErrorV1 {}

fn source_font_binding_id_v1(
    source_hash: Sha256Digest,
    source_font_index: u32,
    source_font_name: &str,
) -> String {
    let source_object_key = format!("quill-font-index:{source_font_index}:{source_font_name}");
    let identity = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash: &source_hash,
        adapter_id: "pub-editor",
        source_object_key: &source_object_key,
        semantic_role: "chaptera.text-format.base-font-binding",
    })
    .expect("bounded source font binding uses fixed valid identity namespaces");
    format!("pub-source-font:{identity}")
}

fn source_text_format_overlay_from_runs_v1(
    source_hash: Sha256Digest,
    story_id: StoryId,
    base_revision_id: &str,
    story_scalar_len: u32,
    source_runs: &[PubTypographyRun],
) -> Result<TextFormatOverlayStateV1, EditorTextFormatBaseErrorV1> {
    if story_scalar_len == 0 {
        return build_text_format_overlay_state_v1(
            story_id.as_canonical().to_string(),
            base_revision_id,
            0,
            Vec::new(),
            Vec::new(),
        )
        .map_err(|error| EditorTextFormatBaseErrorV1::Overlay {
            story_id,
            message: error.to_string(),
        });
    }

    let mut runs = source_runs
        .iter()
        .filter(|run| run.story_id == story_id)
        .collect::<Vec<_>>();
    runs.sort_by_key(|run| (run.story_scalar_start, run.story_scalar_end));

    if runs.is_empty() {
        return Err(EditorTextFormatBaseErrorV1::UnsupportedBase {
            story_id,
            reason: "no source effective typography runs".to_owned(),
        });
    }

    let mut cursor = 0_u32;
    let mut base_runs = Vec::with_capacity(runs.len());
    for run in runs {
        if run.story_scalar_start != cursor
            || run.story_scalar_end <= run.story_scalar_start
            || run.story_scalar_end > story_scalar_len
        {
            return Err(EditorTextFormatBaseErrorV1::UnsupportedBase {
                story_id,
                reason: "source effective typography does not form one contiguous scalar partition"
                    .to_owned(),
            });
        }
        if run.source_font_name.trim().is_empty() || run.text_size_emu == 0 {
            return Err(EditorTextFormatBaseErrorV1::UnsupportedBase {
                story_id,
                reason: "source font binding or effective font size is unavailable".to_owned(),
            });
        }

        let bold =
            run.bold
                .as_ref()
                .ok_or_else(|| EditorTextFormatBaseErrorV1::UnsupportedBase {
                    story_id,
                    reason: "bounded effective bold is unavailable".to_owned(),
                })?;
        let italic =
            run.italic
                .as_ref()
                .ok_or_else(|| EditorTextFormatBaseErrorV1::UnsupportedBase {
                    story_id,
                    reason: "bounded effective italic is unavailable".to_owned(),
                })?;
        if bold.effective_value != (bold.inherited_value ^ bold.local_toggle)
            || italic.effective_value != (italic.inherited_value ^ italic.local_toggle)
        {
            return Err(EditorTextFormatBaseErrorV1::UnsupportedBase {
                story_id,
                reason: "Reader boolean projection violates the admitted XOR invariant".to_owned(),
            });
        }

        let [red, green, blue] =
            run.color_rgb
                .ok_or_else(|| EditorTextFormatBaseErrorV1::UnsupportedBase {
                    story_id,
                    reason: "bounded effective direct-RGB text color is unavailable".to_owned(),
                })?;

        base_runs.push(BaseFormatRunV1 {
            start_scalar: run.story_scalar_start,
            end_scalar: run.story_scalar_end,
            format: BaseCharacterFormatV1 {
                font_resource_id: source_font_binding_id_v1(
                    source_hash,
                    run.source_font_index,
                    &run.source_font_name,
                ),
                font_size_emu: u64::from(run.text_size_emu),
                bold: bold.effective_value,
                italic: italic.effective_value,
                text_color_rgb: format!("#{red:02X}{green:02X}{blue:02X}"),
            },
        });
        cursor = run.story_scalar_end;
    }

    if cursor != story_scalar_len {
        return Err(EditorTextFormatBaseErrorV1::UnsupportedBase {
            story_id,
            reason: "source effective typography does not cover the full Story".to_owned(),
        });
    }

    build_text_format_overlay_state_v1(
        story_id.as_canonical().to_string(),
        base_revision_id,
        story_scalar_len,
        base_runs,
        Vec::new(),
    )
    .map_err(|error| EditorTextFormatBaseErrorV1::Overlay {
        story_id,
        message: error.to_string(),
    })
}

fn text_format_base_error_to_editor_v1(error: EditorTextFormatBaseErrorV1) -> EditorError {
    match error {
        EditorTextFormatBaseErrorV1::SourceIdentityChanged => EditorError::SourceIdentityChanged,
        EditorTextFormatBaseErrorV1::MissingStory { story_id } => {
            EditorError::MissingStory { story_id }
        }
        EditorTextFormatBaseErrorV1::StoryChanged { story_id } => {
            EditorError::TextFormatTextMutationConflict { story_id }
        }
        EditorTextFormatBaseErrorV1::UnsupportedBase { story_id, reason } => {
            EditorError::TextFormatUnsupported { story_id, reason }
        }
        EditorTextFormatBaseErrorV1::Overlay { story_id, message } => {
            EditorError::TextFormatStateInvalid { story_id, message }
        }
    }
}

fn paragraph_alignment_transition_error_to_editor_v1(
    error: ParagraphAlignmentTransitionErrorV1,
) -> EditorError {
    match error {
        ParagraphAlignmentTransitionErrorV1::Stale { paragraph_id, .. } => {
            EditorError::StaleParagraphAlignmentOverride { paragraph_id }
        }
        ParagraphAlignmentTransitionErrorV1::NonCanonicalTargets
        | ParagraphAlignmentTransitionErrorV1::SnapshotShapeMismatch => {
            EditorError::ParagraphAlignmentTransitionInvalid
        }
    }
}

fn text_format_operation_story_id_v1(operation: &EditOperation) -> Option<StoryId> {
    match operation {
        EditOperation::SetTextFormatProperty { story_id, .. }
        | EditOperation::ClearTextFormatPropertyOverride { story_id, .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { story_id, .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { story_id, .. } => {
            Some(*story_id)
        }
        _ => None,
    }
}

fn text_format_operation_property_v1(operation: &EditOperation) -> Option<FormatPropertyV1> {
    match operation {
        EditOperation::SetTextFormatProperty { property, .. }
        | EditOperation::ClearTextFormatPropertyOverride { property, .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { property, .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { property, .. } => {
            Some(*property)
        }
        _ => None,
    }
}

fn is_scoped_text_format_operation_v1(operation: &EditOperation) -> bool {
    matches!(
        operation,
        EditOperation::SetTextFormatPropertyScopedV1 { .. }
            | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. }
    )
}

fn minimum_identity_project_schema_v1(operations: &[EditOperation]) -> &'static str {
    if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::LinkTextFrameTail { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_22
    } else if operations
        .iter()
        .any(|operation| table_rowcol_history_v1(operation).is_some())
    {
        EDITOR_PROJECT_VERSION_V0_21
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetTableTrackExtent { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_20
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetImageCrop { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_19
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateTable { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_18
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateLine { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_17
    } else if operations.iter().any(is_scoped_text_format_operation_v1) {
        EDITOR_PROJECT_VERSION_V0_16
    } else if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::SetParagraphAlignmentOverride { .. }
                | EditOperation::ClearParagraphAlignmentOverride { .. }
        )
    }) {
        EDITOR_PROJECT_VERSION_V0_15
    } else if operations
        .iter()
        .any(|operation| text_format_operation_story_id_v1(operation).is_some())
    {
        EDITOR_PROJECT_VERSION_V0_14
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::ReorderAuthoredStack { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_13
    } else {
        EDITOR_PROJECT_VERSION_V0_12
    }
}

fn apply_text_format_history_operation_semantic_v1(
    state: &TextFormatOverlayStateV1,
    operation: &EditOperation,
) -> Result<TextFormatOverlayStateV1, EditorError> {
    let story_id = text_format_operation_story_id_v1(operation)
        .expect("semantic text-format replay receives only text-format operations");
    if state.story_id != story_id.as_canonical().to_string() {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "format operation Story does not match overlay Story".to_owned(),
        });
    }
    let current_hash =
        state_hash_v1(state).map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
    let receipt = match operation {
        EditOperation::SetTextFormatProperty {
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        }
        | EditOperation::SetTextFormatPropertyScopedV1 {
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        } => overlay_set_text_format_property_v1(
            state,
            *start_scalar,
            *end_scalar,
            *property,
            value.clone(),
            &current_hash,
        ),
        EditOperation::ClearTextFormatPropertyOverride {
            start_scalar,
            end_scalar,
            property,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            start_scalar,
            end_scalar,
            property,
            ..
        } => overlay_clear_text_format_property_override_v1(
            state,
            *start_scalar,
            *end_scalar,
            *property,
            &current_hash,
        ),
        _ => unreachable!("semantic text-format replay receives only text-format operations"),
    }
    .map_err(|error| EditorError::TextFormatStateInvalid {
        story_id,
        message: error.to_string(),
    })?;
    Ok(receipt.after_state)
}

fn apply_text_format_history_operation_v1(
    state: &TextFormatOverlayStateV1,
    operation: &EditOperation,
) -> Result<TextFormatOverlayStateV1, EditorError> {
    if is_scoped_text_format_operation_v1(operation) {
        let story_id = text_format_operation_story_id_v1(operation)
            .expect("scoped format operation has Story");
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "property-scoped text-format operation cannot be validated in complete-overlay hash domain".to_owned(),
        });
    }

    let (story_id, before_state_hash, after_state_hash, receipt) = match operation {
        EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            after_state_hash,
        } => (
            *story_id,
            before_state_hash,
            after_state_hash,
            overlay_set_text_format_property_v1(
                state,
                *start_scalar,
                *end_scalar,
                *property,
                value.clone(),
                before_state_hash,
            ),
        ),
        EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            after_state_hash,
        } => (
            *story_id,
            before_state_hash,
            after_state_hash,
            overlay_clear_text_format_property_override_v1(
                state,
                *start_scalar,
                *end_scalar,
                *property,
                before_state_hash,
            ),
        ),
        _ => unreachable!(
            "checked complete-overlay replay admits only legacy text-format operations"
        ),
    };

    if state.story_id != story_id.as_canonical().to_string() {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "format operation Story does not match overlay Story".to_owned(),
        });
    }

    let receipt = receipt.map_err(|error| EditorError::TextFormatStateInvalid {
        story_id,
        message: error.to_string(),
    })?;
    if receipt.command.before_state_hash != *before_state_hash
        || receipt.command.after_state_hash != *after_state_hash
    {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "persisted format operation hashes do not match deterministic replay"
                .to_owned(),
        });
    }
    Ok(receipt.after_state)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorProjectError {
    UnsupportedSchema {
        found: String,
    },
    SourceHashMismatch {
        expected: Sha256Digest,
        found: Sha256Digest,
    },
    SessionNotEmpty,
    LegacyProjectCarriesAssets,
    LegacyProjectCarriesImageOperation {
        index: usize,
    },
    LegacyProjectCarriesGeometryOperation {
        index: usize,
    },
    LegacyProjectCarriesResizeOperation {
        index: usize,
    },
    LegacyProjectCarriesBreakLinkOperation {
        index: usize,
    },
    LegacyProjectCarriesLinkTextFrameOperation {
        index: usize,
    },
    LegacyProjectCarriesMoveNodesOperation {
        index: usize,
    },
    LegacyProjectCarriesResizeNodesOperation {
        index: usize,
    },
    LegacyProjectCarriesCreateShapeOperation {
        index: usize,
    },
    LegacyProjectCarriesCreateLineOperation {
        index: usize,
    },
    LegacyProjectCarriesCreateTableOperation {
        index: usize,
    },
    LegacyProjectCarriesTableTrackExtentOperation {
        index: usize,
    },
    LegacyProjectCarriesTableRowColOperation {
        index: usize,
    },
    LegacyProjectCarriesCreateTextBoxOperation {
        index: usize,
    },
    LegacyProjectCarriesDeleteNodeOperation {
        index: usize,
    },
    LegacyProjectCarriesReorderAuthoredStackOperation {
        index: usize,
    },
    LegacyProjectCarriesTextFormatOperation {
        index: usize,
    },
    LegacyProjectCarriesScopedTextFormatOperation {
        index: usize,
    },
    LegacyProjectCarriesParagraphAlignmentOperation {
        index: usize,
    },
    LegacyProjectCarriesImageCropOperation {
        index: usize,
    },
    LegacyProjectCarriesTableGrids,
    LegacyProjectCarriesIdentity,
    MissingProjectIdentity,
    TableGridMismatch,
    RequiredAssetUnavailable {
        sha256: Sha256Digest,
    },
    AssetReachabilityMismatch {
        expected: Vec<Sha256Digest>,
        found: Vec<Sha256Digest>,
    },
    MissingAssetBytes {
        sha256: Sha256Digest,
    },
    AssetLengthMismatch {
        sha256: Sha256Digest,
        expected: u64,
        found: u64,
    },
    AssetHashMismatch {
        expected: Sha256Digest,
        found: Sha256Digest,
    },
    AssetMetadataNonCanonical,
    Asset {
        index: usize,
        error: EditorAssetError,
    },
    Session(EditorError),
    Operation {
        index: usize,
        error: EditorError,
    },
    OperationMismatch {
        index: usize,
    },
    InvalidTableAfterState {
        index: usize,
        node_id: NodeId,
        cell_id: TableCellId,
    },
}

impl fmt::Display for EditorProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { found } => write!(
                formatter,
                "editor project schema {found:?} is unsupported; expected {EDITOR_PROJECT_VERSION_V0_1:?}, {EDITOR_PROJECT_VERSION_V0_2:?}, {EDITOR_PROJECT_VERSION_V0_3:?}, {EDITOR_PROJECT_VERSION_V0_4:?}, {EDITOR_PROJECT_VERSION_V0_5:?}, {EDITOR_PROJECT_VERSION_V0_6:?}, {EDITOR_PROJECT_VERSION_V0_7:?}, {EDITOR_PROJECT_VERSION_V0_8:?}, {EDITOR_PROJECT_VERSION_V0_9:?}, {EDITOR_PROJECT_VERSION_V0_10:?}, {EDITOR_PROJECT_VERSION_V0_11:?}, {EDITOR_PROJECT_VERSION_V0_12:?}, {EDITOR_PROJECT_VERSION_V0_13:?}, {EDITOR_PROJECT_VERSION_V0_14:?}, {EDITOR_PROJECT_VERSION_V0_15:?}, {EDITOR_PROJECT_VERSION_V0_16:?}, {EDITOR_PROJECT_VERSION_V0_17:?}, {EDITOR_PROJECT_VERSION_V0_18:?}, {EDITOR_PROJECT_VERSION_V0_19:?}, {EDITOR_PROJECT_VERSION_V0_20:?}, {EDITOR_PROJECT_VERSION_V0_21:?}, or {EDITOR_PROJECT_VERSION_V0_22:?}"
            ),
            Self::SourceHashMismatch { expected, found } => write!(
                formatter,
                "editor project source hash {found} does not match opened PUB {expected}"
            ),
            Self::SessionNotEmpty => formatter.write_str(
                "editor project replay requires a fresh session with no existing edits or replacement assets",
            ),
            Self::LegacyProjectCarriesAssets => {
                formatter.write_str("pub-editor-v0.1 projects cannot carry replacement assets")
            }
            Self::LegacyProjectCarriesImageOperation { index } => write!(
                formatter,
                "editor project operation {index} uses ReplaceImage but the project schema predates pub-editor-v0.3"
            ),
            Self::LegacyProjectCarriesGeometryOperation { index } => write!(
                formatter,
                "editor project operation {index} uses MoveNode but the project schema predates pub-editor-v0.4"
            ),
            Self::LegacyProjectCarriesResizeOperation { index } => write!(
                formatter,
                "editor project operation {index} uses ResizeNode but the project schema predates pub-editor-v0.5"
            ),
            Self::LegacyProjectCarriesBreakLinkOperation { index } => write!(
                formatter,
                "editor project operation {index} uses BreakTextFrameForwardLink but the project schema predates pub-editor-v0.7"
            ),
            Self::LegacyProjectCarriesLinkTextFrameOperation { index } => write!(
                formatter,
                "editor project operation {index} uses LinkTextFrameTail but the project schema predates pub-editor-v0.22"
            ),
            Self::LegacyProjectCarriesMoveNodesOperation { index } => write!(
                formatter,
                "editor project operation {index} uses MoveNodes but the project schema predates pub-editor-v0.8"
            ),
            Self::LegacyProjectCarriesResizeNodesOperation { index } => write!(
                formatter,
                "editor project operation {index} uses ResizeNodes but the project schema predates pub-editor-v0.9"
            ),
            Self::LegacyProjectCarriesCreateShapeOperation { index } => write!(
                formatter,
                "editor project operation {index} uses CreateShape but the project schema predates pub-editor-v0.10"
            ),
            Self::LegacyProjectCarriesCreateLineOperation { index } => write!(
                formatter,
                "editor project operation {index} uses CreateLine but the project schema predates pub-editor-v0.17"
            ),
            Self::LegacyProjectCarriesCreateTableOperation { index } => write!(
                formatter,
                "editor project operation {index} uses CreateTable but the project schema predates pub-editor-v0.18"
            ),
            Self::LegacyProjectCarriesTableTrackExtentOperation { index } => write!(
                formatter,
                "editor project operation {index} uses SetTableTrackExtent but the project schema predates pub-editor-v0.20"
            ),
            Self::LegacyProjectCarriesTableRowColOperation { index } => write!(
                formatter,
                "editor project operation {index} uses table row/column lifecycle but the project schema predates pub-editor-v0.21"
            ),
            Self::LegacyProjectCarriesCreateTextBoxOperation { index } => write!(
                formatter,
                "editor project operation {index} uses CreateTextBox but the project schema predates pub-editor-v0.11"
            ),
            Self::LegacyProjectCarriesDeleteNodeOperation { index } => write!(
                formatter,
                "editor project operation {index} uses DeleteNode but the project schema predates pub-editor-v0.12"
            ),
            Self::LegacyProjectCarriesReorderAuthoredStackOperation { index } => write!(
                formatter,
                "editor project operation {index} uses ReorderAuthoredStack but the project schema predates pub-editor-v0.13"
            ),
            Self::LegacyProjectCarriesTextFormatOperation { index } => write!(
                formatter,
                "editor project operation {index} uses text-format overrides but the project schema predates pub-editor-v0.14"
            ),
            Self::LegacyProjectCarriesScopedTextFormatOperation { index } => write!(
                formatter,
                "editor project operation {index} uses property-scoped text-format history but the project schema predates pub-editor-v0.16"
            ),
            Self::LegacyProjectCarriesParagraphAlignmentOperation { index } => write!(
                formatter,
                "editor project operation {index} uses paragraph alignment overrides but the project schema predates pub-editor-v0.15"
            ),
            Self::LegacyProjectCarriesImageCropOperation { index } => write!(
                formatter,
                "editor project operation {index} uses SetImageCrop but the project schema predates pub-editor-v0.19"
            ),
            Self::LegacyProjectCarriesTableGrids => formatter.write_str(
                "editor projects before pub-editor-v0.6 cannot carry EffectiveTableGridV1 state",
            ),
            Self::LegacyProjectCarriesIdentity => formatter.write_str(
                "editor projects before pub-editor-v0.11 cannot carry durable project identity",
            ),
            Self::MissingProjectIdentity => formatter.write_str(
                "pub-editor-v0.11+ requires durable project identity",
            ),
            Self::TableGridMismatch => formatter.write_str(
                "editor project EffectiveTableGridV1 state does not match deterministic replay",
            ),
            Self::RequiredAssetUnavailable { sha256 } => write!(
                formatter,
                "editor operation history requires asset {sha256}, but exact runtime bytes are unavailable"
            ),
            Self::AssetReachabilityMismatch { expected, found } => write!(
                formatter,
                "editor project asset metadata does not match operation-reachable identities; expected {expected:?}, found {found:?}"
            ),
            Self::MissingAssetBytes { sha256 } => {
                write!(
                    formatter,
                    "editor project replacement asset {sha256} is missing"
                )
            }
            Self::AssetLengthMismatch {
                sha256,
                expected,
                found,
            } => write!(
                formatter,
                "editor project replacement asset {sha256} has {found} bytes; expected {expected}"
            ),
            Self::AssetHashMismatch { expected, found } => write!(
                formatter,
                "editor project replacement asset hash {found} does not match metadata {expected}"
            ),
            Self::AssetMetadataNonCanonical => formatter.write_str(
                "editor project replacement asset metadata is not in canonical content-addressed order",
            ),
            Self::Asset { index, error } => {
                write!(
                    formatter,
                    "editor project asset {index} was rejected: {error}"
                )
            }
            Self::Session(error) => write!(formatter, "editor session is invalid: {error}"),
            Self::Operation { index, error } => {
                write!(
                    formatter,
                    "editor project operation {index} was rejected: {error}"
                )
            }
            Self::OperationMismatch { index } => write!(
                formatter,
                "editor project operation {index} does not match the canonical operation generated by the current editor"
            ),
            Self::InvalidTableAfterState {
                index,
                node_id,
                cell_id,
            } => write!(
                formatter,
                "editor project operation {index} cannot derive a bounded replacement for table node {} cell {}",
                node_id.as_canonical(),
                cell_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for EditorProjectError {}

pub fn open_mature_0x2c_editor(
    bytes: &[u8],
    source_hash: Sha256Digest,
) -> Result<EditorSession, EditorOpenError> {
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .map_err(|error| EditorOpenError::SourceGraph(error.to_string()))?;
    let source_images = build_mature_0x2c_asset_export_bundle_from_bytes(bytes, &source.graph).ok();
    let resolved = resolve_pub_source_graph(&source.graph)
        .map_err(|error| EditorOpenError::Resolve(error.to_string()))?;
    let mut session = EditorSession::new(resolved.graph).map_err(EditorOpenError::Session)?;
    session.source_typography_runs = source.typography_runs;
    session.source_typography_size_runs = source.typography_size_runs;
    session.source_paragraph_alignments = source.paragraph_alignments;
    session.source_paragraph_flow_runs = source.paragraph_flow_runs;
    if let Some(bundle) = source_images {
        let (assets, nodes) = source_image_context_from_bundle(bundle);
        session.source_image_assets = assets;
        session.source_image_nodes = nodes;
    }
    Ok(session)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorEditableTarget {
    Idml,
    Odg,
}

impl EditorEditableTarget {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Idml => "idml",
            Self::Odg => "odg",
        }
    }

    const fn format(self) -> &'static str {
        match self {
            Self::Idml => "idml",
            Self::Odg => "odg",
        }
    }

    const fn adapter_version(self) -> &'static str {
        match self {
            Self::Idml => IDML_ADAPTER_VERSION_V0_1,
            Self::Odg => ODG_ADAPTER_VERSION_V0_1,
        }
    }

    const fn schema_fence(self) -> &'static str {
        match self {
            Self::Idml => IDML_SCHEMA_FENCE_LEGACY_DOM_7,
            Self::Odg => ODG_SCHEMA_FENCE_ODF_1_4,
        }
    }
}

impl fmt::Display for EditorEditableTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idml => "IDML",
            Self::Odg => "ODG",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEditableExportPreview {
    pub target: EditorEditableTarget,
    pub report: ExportReport,
    pub human_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEditableTableCell {
    pub node_id: NodeId,
    pub story_id: StoryId,
    pub cell_id: TableCellId,
    pub row: u32,
    pub column: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEditableExport {
    pub target: EditorEditableTarget,
    pub report: ExportReport,
    pub human_summary: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorExportError {
    Session(EditorError),
    Report(String),
    Blocked {
        target: EditorEditableTarget,
        blockers: u64,
    },
    Projection {
        target: EditorEditableTarget,
        message: String,
    },
    Write {
        target: EditorEditableTarget,
        message: String,
    },
}

impl fmt::Display for EditorExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
            Self::Report(message) => write!(formatter, "could not build export report: {message}"),
            Self::Blocked { target, blockers } => write!(
                formatter,
                "{target} export is blocked by {blockers} required semantic losses"
            ),
            Self::Projection { target, message } => {
                write!(formatter, "{target} semantic projection failed: {message}")
            }
            Self::Write { target, message } => {
                write!(
                    formatter,
                    "{target} package serialization failed: {message}"
                )
            }
        }
    }
}

impl std::error::Error for EditorExportError {}

#[derive(Debug, Clone)]
pub struct EditorSession {
    source_hash: Sha256Digest,
    graph: PubResolvedGraph,
    project_identity: Option<EditorProjectIdentity>,
    source_image_assets: BTreeMap<ResourceId, EditorSourceImageAsset>,
    source_image_nodes: BTreeMap<NodeId, ResourceId>,
    image_crop_overrides: BTreeMap<NodeId, ImageCropStateV1>,
    source_story_state_ids: BTreeMap<StoryId, String>,
    source_typography_runs: Vec<PubTypographyRun>,
    source_typography_size_runs: Vec<PubTypographySizeRun>,
    source_paragraph_alignments: Vec<PubParagraphAlignmentRun>,
    source_paragraph_flow_runs: Vec<PubParagraphFlowRun>,
    replacement_assets: BTreeMap<Sha256Digest, EditorReplacementAsset>,
    image_replacements: BTreeMap<NodeId, Sha256Digest>,
    authored_shapes: BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    authored_lines: BTreeMap<NodeId, AuthoredLineRuntimeV1>,
    authored_stacks: BTreeMap<PageId, AuthoredStackV1>,
    undo: Vec<EditOperation>,
    redo: Vec<EditOperation>,
}

impl EditorSession {
    pub fn new(graph: PubResolvedGraph) -> Result<Self, EditorError> {
        let source_hash = graph.source.source_hash;
        if graph.document.source_hash != source_hash {
            return Err(EditorError::SourceIdentityChanged);
        }

        let source_story_state_ids = graph
            .stories
            .iter()
            .map(|(story_id, story)| (*story_id, story_state_id_v1(*story_id, &story.text)))
            .collect();

        Ok(Self {
            source_hash,
            graph,
            project_identity: Some(new_project_identity()),
            source_image_assets: BTreeMap::new(),
            source_image_nodes: BTreeMap::new(),
            image_crop_overrides: BTreeMap::new(),
            source_story_state_ids,
            source_typography_runs: Vec::new(),
            source_typography_size_runs: Vec::new(),
            source_paragraph_alignments: Vec::new(),
            source_paragraph_flow_runs: Vec::new(),
            replacement_assets: BTreeMap::new(),
            image_replacements: BTreeMap::new(),
            authored_shapes: BTreeMap::new(),
            authored_lines: BTreeMap::new(),
            authored_stacks: BTreeMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn source_hash(&self) -> Sha256Digest {
        self.source_hash
    }

    pub fn graph(&self) -> &PubResolvedGraph {
        &self.graph
    }

    pub fn operations(&self) -> &[EditOperation] {
        &self.undo
    }

    pub fn source_text_format_overlay_v1(
        &self,
        story_id: StoryId,
    ) -> Result<TextFormatOverlayStateV1, EditorTextFormatBaseErrorV1> {
        self.validate_source_identity()
            .map_err(|_| EditorTextFormatBaseErrorV1::SourceIdentityChanged)?;
        let story = self
            .graph
            .stories
            .get(&story_id)
            .ok_or(EditorTextFormatBaseErrorV1::MissingStory { story_id })?;
        let source_revision_id = self
            .source_story_state_ids
            .get(&story_id)
            .ok_or(EditorTextFormatBaseErrorV1::MissingStory { story_id })?;
        if story_state_id_v1(story_id, &story.text) != *source_revision_id {
            return Err(EditorTextFormatBaseErrorV1::StoryChanged { story_id });
        }
        let story_scalar_len = u32::try_from(story.text.chars().count()).map_err(|_| {
            EditorTextFormatBaseErrorV1::UnsupportedBase {
                story_id,
                reason: "Story scalar length exceeds u32".to_owned(),
            }
        })?;

        source_text_format_overlay_from_runs_v1(
            self.source_hash,
            story_id,
            source_revision_id,
            story_scalar_len,
            &self.source_typography_runs,
        )
    }

    pub fn source_text_format_property_state_v1(
        &self,
        story_id: StoryId,
        property: FormatPropertyV1,
    ) -> Result<TextFormatPropertyStateV1, EditorTextFormatBaseErrorV1> {
        self.validate_source_identity()
            .map_err(|_| EditorTextFormatBaseErrorV1::SourceIdentityChanged)?;
        let story = self
            .graph
            .stories
            .get(&story_id)
            .ok_or(EditorTextFormatBaseErrorV1::MissingStory { story_id })?;
        let source_revision_id = self
            .source_story_state_ids
            .get(&story_id)
            .ok_or(EditorTextFormatBaseErrorV1::MissingStory { story_id })?;
        if story_state_id_v1(story_id, &story.text) != *source_revision_id {
            return Err(EditorTextFormatBaseErrorV1::StoryChanged { story_id });
        }
        let story_scalar_len = u32::try_from(story.text.chars().count()).map_err(|_| {
            EditorTextFormatBaseErrorV1::UnsupportedBase {
                story_id,
                reason: "Story scalar length exceeds u32".to_owned(),
            }
        })?;

        build_source_text_format_property_state_v1(
            story_id,
            source_revision_id,
            story_scalar_len,
            &self.source_typography_runs,
            property,
        )
        .map_err(|reason| EditorTextFormatBaseErrorV1::UnsupportedBase { story_id, reason })
    }

    pub fn current_text_format_overlay_v1(
        &self,
        story_id: StoryId,
    ) -> Result<TextFormatOverlayStateV1, EditorError> {
        let mut state = self
            .source_text_format_overlay_v1(story_id)
            .map_err(text_format_base_error_to_editor_v1)?;
        for operation in &self.undo {
            if text_format_operation_story_id_v1(operation) != Some(story_id) {
                continue;
            }
            state = if is_scoped_text_format_operation_v1(operation) {
                apply_text_format_history_operation_semantic_v1(&state, operation)?
            } else {
                apply_text_format_history_operation_v1(&state, operation)?
            };
        }
        Ok(state)
    }

    pub fn current_text_format_state_hash_v1(
        &self,
        story_id: StoryId,
    ) -> Result<String, EditorError> {
        let state = self.current_text_format_overlay_v1(story_id)?;
        state_hash_v1(&state).map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })
    }

    pub fn current_text_format_property_state_v1(
        &self,
        story_id: StoryId,
        property: FormatPropertyV1,
    ) -> Result<TextFormatPropertyStateV1, EditorError> {
        let state = self
            .source_text_format_property_state_v1(story_id, property)
            .map_err(text_format_base_error_to_editor_v1)?;
        fold_text_format_property_history_v1(state, &self.undo)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })
    }

    pub fn current_text_format_property_state_hash_v1(
        &self,
        story_id: StoryId,
        property: FormatPropertyV1,
    ) -> Result<String, EditorError> {
        let state = self.current_text_format_property_state_v1(story_id, property)?;
        text_format_property_state_hash_v1(&state)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })
    }

    pub fn current_text_format_property_segments_v1(
        &self,
        story_id: StoryId,
        property: FormatPropertyV1,
        start_scalar: u32,
        end_scalar: u32,
    ) -> Result<Vec<EffectivePropertySegmentV1>, EditorError> {
        let state = self.current_text_format_property_state_v1(story_id, property)?;
        effective_text_format_property_segments_v1(&state, start_scalar, end_scalar)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })
    }

    pub fn set_text_format_property_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        value: FormatValueV1,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let state = self.current_text_format_overlay_v1(story_id)?;
        let before_state_hash =
            state_hash_v1(&state).map_err(|error| EditorError::TextFormatStateInvalid {
                story_id,
                message: error.to_string(),
            })?;
        if before_state_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let receipt = overlay_set_text_format_property_v1(
            &state,
            start_scalar,
            end_scalar,
            property,
            value.clone(),
            expected_state_hash,
        )
        .map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
        if receipt.command.before_state_hash == receipt.command.after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }

        let operation = EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash: receipt.command.before_state_hash,
            after_state_hash: receipt.command.after_state_hash,
        };
        let replayed = apply_text_format_history_operation_v1(&state, &operation)?;
        if replayed != receipt.after_state {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "canonical format command did not reproduce its receipt state".to_owned(),
            });
        }

        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn clear_text_format_property_override_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let state = self.current_text_format_overlay_v1(story_id)?;
        let before_state_hash =
            state_hash_v1(&state).map_err(|error| EditorError::TextFormatStateInvalid {
                story_id,
                message: error.to_string(),
            })?;
        if before_state_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let receipt = overlay_clear_text_format_property_override_v1(
            &state,
            start_scalar,
            end_scalar,
            property,
            expected_state_hash,
        )
        .map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
        if receipt.command.before_state_hash == receipt.command.after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }

        let operation = EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash: receipt.command.before_state_hash,
            after_state_hash: receipt.command.after_state_hash,
        };
        let replayed = apply_text_format_history_operation_v1(&state, &operation)?;
        if replayed != receipt.after_state {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "canonical clear-format command did not reproduce its receipt state"
                    .to_owned(),
            });
        }

        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn set_text_format_property_scoped_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        value: FormatValueV1,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let state = self.current_text_format_property_state_v1(story_id, property)?;
        let before_state_hash = text_format_property_state_hash_v1(&state)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if before_state_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let after =
            set_text_format_property_state_v1(&state, start_scalar, end_scalar, value.clone())
                .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        let after_state_hash = text_format_property_state_hash_v1(&after)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if before_state_hash == after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }

        let operation = EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            after_state_hash,
        };
        let replayed = apply_text_format_property_operation_checked_v1(&state, &operation)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if replayed != after {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "canonical scoped format command did not reproduce its state".to_owned(),
            });
        }

        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn clear_text_format_property_override_scoped_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        property: FormatPropertyV1,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let state = self.current_text_format_property_state_v1(story_id, property)?;
        let before_state_hash = text_format_property_state_hash_v1(&state)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if before_state_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let after = clear_text_format_property_state_v1(&state, start_scalar, end_scalar)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        let after_state_hash = text_format_property_state_hash_v1(&after)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if before_state_hash == after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }

        let operation = EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            after_state_hash,
        };
        let replayed = apply_text_format_property_operation_checked_v1(&state, &operation)
            .map_err(|message| EditorError::TextFormatStateInvalid { story_id, message })?;
        if replayed != after {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "canonical scoped clear-format command did not reproduce its state"
                    .to_owned(),
            });
        }

        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    fn current_paragraph_alignment_overrides_v1(
        &self,
    ) -> Result<BTreeMap<ParagraphId, AuthoredParagraphAlignmentValueV1>, EditorError> {
        authored_paragraph_alignment_v1::paragraph_alignment_override_state_from_history_v1(
            &self.undo,
        )
        .map_err(paragraph_alignment_transition_error_to_editor_v1)
    }

    fn canonical_paragraph_alignment_target_ids_v1(
        &self,
        mut paragraph_ids: Vec<ParagraphId>,
    ) -> Result<Vec<ParagraphId>, EditorError> {
        self.validate_source_identity()?;
        if paragraph_ids.is_empty() {
            return Err(EditorError::ParagraphAlignmentTargetsEmpty);
        }
        paragraph_ids.sort_unstable();
        paragraph_ids.dedup();

        let available = self
            .imported_paragraphs_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
            .into_iter()
            .map(|paragraph| paragraph.paragraph_id)
            .collect::<BTreeSet<_>>();
        for paragraph_id in &paragraph_ids {
            if !available.contains(paragraph_id) {
                return Err(EditorError::ParagraphAlignmentParagraphUnavailable {
                    paragraph_id: *paragraph_id,
                });
            }
        }
        Ok(paragraph_ids)
    }

    pub fn authored_paragraph_alignment_override_v1(
        &self,
        paragraph_id: ParagraphId,
    ) -> Result<Option<AuthoredParagraphAlignmentValueV1>, EditorError> {
        Ok(self
            .current_paragraph_alignment_overrides_v1()?
            .get(&paragraph_id)
            .copied())
    }

    /// Resolves current effective paragraph alignment from one imported
    /// paragraph projection, one imported-base projection over that snapshot,
    /// and one authored-override history fold.
    pub fn effective_paragraph_alignments_v1(
        &self,
    ) -> Result<Vec<EffectiveParagraphAlignmentV1>, EditorError> {
        self.validate_source_identity()?;
        let paragraphs = self
            .imported_paragraphs_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?;
        let base_by_id = self
            .imported_paragraph_base_alignments_from_paragraphs_v1(&paragraphs)
            .into_iter()
            .map(|item| (item.paragraph_id, item.alignment))
            .collect::<BTreeMap<_, _>>();
        let overrides = self.current_paragraph_alignment_overrides_v1()?;

        Ok(paragraphs
            .into_iter()
            .map(|paragraph| {
                let paragraph_id = paragraph.paragraph_id;
                let imported_base = base_by_id.get(&paragraph_id).copied();
                let authored_override = overrides.get(&paragraph_id).copied();
                let (effective, authority) = if let Some(value) = authored_override {
                    (
                        Some(EffectiveParagraphAlignmentValueV1::from(value)),
                        Some(ParagraphAlignmentAuthorityV1::ChapteraOverride),
                    )
                } else if let Some(value) = imported_base {
                    (
                        Some(EffectiveParagraphAlignmentValueV1::from(value)),
                        Some(ParagraphAlignmentAuthorityV1::ImportedBase),
                    )
                } else {
                    (None, None)
                };

                EffectiveParagraphAlignmentV1 {
                    paragraph_id,
                    story_id: paragraph.story_id,
                    range: paragraph.range,
                    imported_base,
                    authored_override,
                    effective,
                    authority,
                }
            })
            .collect())
    }

    pub fn effective_paragraph_alignment_v1(
        &self,
        paragraph_id: ParagraphId,
    ) -> Result<EffectiveParagraphAlignmentV1, EditorError> {
        self.effective_paragraph_alignments_v1()?
            .into_iter()
            .find(|paragraph| paragraph.paragraph_id == paragraph_id)
            .ok_or(EditorError::ParagraphAlignmentParagraphUnavailable { paragraph_id })
    }

    pub fn set_paragraph_alignment_override_v1(
        &mut self,
        paragraph_ids: Vec<ParagraphId>,
        value: AuthoredParagraphAlignmentValueV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let paragraph_ids = self.canonical_paragraph_alignment_target_ids_v1(paragraph_ids)?;
        let overrides = self.current_paragraph_alignment_overrides_v1()?;
        let before = authored_paragraph_alignment_v1::paragraph_alignment_override_snapshots_v1(
            &overrides,
            &paragraph_ids,
        );
        let base_by_id = self
            .imported_paragraph_base_alignments_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
            .into_iter()
            .map(|item| (item.paragraph_id, item.alignment))
            .collect::<BTreeMap<_, _>>();
        let after = paragraph_ids
            .iter()
            .map(|paragraph_id| ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: *paragraph_id,
                value: authored_paragraph_alignment_v1::normalized_paragraph_alignment_override_v1(
                    base_by_id.get(paragraph_id).copied(),
                    value,
                ),
            })
            .collect::<Vec<_>>();
        if before == after {
            return Err(EditorError::ParagraphAlignmentNoChange);
        }

        let operation = EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids,
            value,
            before,
            after,
        };
        authored_paragraph_alignment_v1::validate_paragraph_alignment_operation_against_history_v1(
            &self.undo, &operation,
        )
        .map_err(paragraph_alignment_transition_error_to_editor_v1)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn clear_paragraph_alignment_override_v1(
        &mut self,
        paragraph_ids: Vec<ParagraphId>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let paragraph_ids = self.canonical_paragraph_alignment_target_ids_v1(paragraph_ids)?;
        let overrides = self.current_paragraph_alignment_overrides_v1()?;
        let before = authored_paragraph_alignment_v1::paragraph_alignment_override_snapshots_v1(
            &overrides,
            &paragraph_ids,
        );
        let after = paragraph_ids
            .iter()
            .map(|paragraph_id| ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: *paragraph_id,
                value: None,
            })
            .collect::<Vec<_>>();
        if before == after {
            return Err(EditorError::ParagraphAlignmentNoChange);
        }

        let operation = EditOperation::ClearParagraphAlignmentOverride {
            paragraph_ids,
            before,
            after,
        };
        authored_paragraph_alignment_v1::validate_paragraph_alignment_operation_against_history_v1(
            &self.undo, &operation,
        )
        .map_err(paragraph_alignment_transition_error_to_editor_v1)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    fn story_has_paragraph_alignment_history_v1(
        &self,
        story_id: StoryId,
    ) -> Result<bool, EditorError> {
        let has_paragraph_history = self.undo.iter().any(|operation| {
            matches!(
                operation,
                EditOperation::SetParagraphAlignmentOverride { .. }
                    | EditOperation::ClearParagraphAlignmentOverride { .. }
            )
        });
        if !has_paragraph_history {
            return Ok(false);
        }

        let paragraph_ids = self
            .imported_paragraphs_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
            .into_iter()
            .filter(|paragraph| paragraph.story_id == story_id)
            .map(|paragraph| paragraph.paragraph_id)
            .collect::<BTreeSet<_>>();
        if paragraph_ids.is_empty() {
            return Ok(false);
        }

        Ok(self.undo.iter().any(|operation| match operation {
            EditOperation::SetParagraphAlignmentOverride {
                paragraph_ids: targets,
                ..
            }
            | EditOperation::ClearParagraphAlignmentOverride {
                paragraph_ids: targets,
                ..
            } => targets
                .iter()
                .any(|paragraph_id| paragraph_ids.contains(paragraph_id)),
            _ => false,
        }))
    }

    pub fn prove_author_created_story_v1(
        &self,
        story_id: StoryId,
    ) -> Result<Option<ProvenAuthorCreatedStoryV1>, AuthorCreatedStoryProofError> {
        let claims = self
            .undo
            .iter()
            .filter_map(|operation| match operation {
                EditOperation::CreateTextBox {
                    node_id,
                    story_id: claimed_story_id,
                    page_id,
                    text_preset,
                    ..
                } if *claimed_story_id == story_id => Some((*node_id, *page_id, text_preset)),
                _ => None,
            })
            .collect::<Vec<_>>();

        if claims.is_empty() {
            return Ok(None);
        }
        if claims.len() != 1 {
            return Err(AuthorCreatedStoryProofError::unproven(
                "current history contains multiple applied CreateTextBox claims for one Story",
            ));
        }
        let (frame_id, page_id, text_preset) = claims[0];

        if !is_editor_created_uuid_v7_story_id(story_id)
            || !is_editor_created_uuid_v7_node_id(frame_id)
            || !validate_authoring_text_preset_v1(text_preset)
        {
            return Err(AuthorCreatedStoryProofError::unproven(
                "CreateTextBox identity or text preset no longer satisfies the canonical author-created contract",
            ));
        }

        let story = self.graph.stories.get(&story_id).ok_or_else(|| {
            AuthorCreatedStoryProofError::unproven(
                "applied CreateTextBox Story is absent from the current graph",
            )
        })?;
        if story.id != story_id
            || !story.source_refs.is_empty()
            || !story.paragraphs.is_empty()
            || !story.runs.is_empty()
            || !story.fields.is_empty()
            || !story.hyperlinks.is_empty()
        {
            return Err(AuthorCreatedStoryProofError::unproven(
                "current Story is not the bounded source-free Chaptera-created Story shape",
            ));
        }

        let owners = self
            .graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| {
                frame_from_payload(*node_id, &node.payload)
                    .filter(|frame| frame.story_id == story_id)
                    .map(|_| *node_id)
            })
            .collect::<Vec<_>>();
        if owners.as_slice() != [frame_id] {
            return self.prove_author_created_linked_story_v1(
                story_id,
                frame_id,
                page_id,
                text_preset,
            );
        }

        let node = self.graph.nodes.get(&frame_id).ok_or_else(|| {
            AuthorCreatedStoryProofError::unproven(
                "CreateTextBox TextFrame is absent from the current graph",
            )
        })?;
        let frame = frame_from_payload(frame_id, &node.payload).ok_or_else(|| {
            AuthorCreatedStoryProofError::unproven(
                "CreateTextBox node no longer carries one canonical StoryFrame",
            )
        })?;
        if node.kind != NodeKind::TextFrame
            || node.header.id != frame_id
            || node.header.parent_id != page_id.into_canonical()
            || node.header.transform != Affine2D::identity()
            || !node.header.source_refs.is_empty()
            || node.payload.contents_seq_num != 0
            || node.payload.table_story.is_some()
            || node.payload.table.is_some()
            || frame.story_id != story_id
            || frame.frame_id != frame_id
            || frame.ordinal != 0
            || frame.previous.is_some()
            || frame.next.is_some()
        {
            return Err(AuthorCreatedStoryProofError::unproven(
                "current TextFrame topology/provenance no longer matches the bounded CreateTextBox contract",
            ));
        }
        if node.header.bounds.width.get() <= 0
            || node.header.bounds.height.get() <= 0
            || node.header.bounds.right().is_none()
            || node.header.bounds.bottom().is_none()
        {
            return Err(AuthorCreatedStoryProofError::unproven(
                "current author-created TextFrame has invalid geometry",
            ));
        }

        let page = self.graph.pages.get(&page_id).ok_or_else(|| {
            AuthorCreatedStoryProofError::unproven(
                "CreateTextBox parent page is absent from the current graph",
            )
        })?;
        if page
            .children
            .iter()
            .filter(|child| **child == frame_id)
            .count()
            != 1
        {
            return Err(AuthorCreatedStoryProofError::unproven(
                "parent page must contain the author-created TextFrame exactly once",
            ));
        }

        Ok(Some(ProvenAuthorCreatedStoryV1 {
            story_id,
            frame_id,
            page_id,
            text_preset: text_preset.clone(),
        }))
    }

    pub fn authored_shapes(
        &self,
    ) -> impl ExactSizeIterator<Item = &AuthoredShapeRuntimeV1> + DoubleEndedIterator {
        self.authored_shapes.values()
    }

    pub fn authored_shape(&self, node_id: NodeId) -> Option<&AuthoredShapeRuntimeV1> {
        self.authored_shapes.get(&node_id)
    }

    pub fn authored_lines(
        &self,
    ) -> impl ExactSizeIterator<Item = &AuthoredLineRuntimeV1> + DoubleEndedIterator {
        self.authored_lines.values()
    }

    pub fn authored_line(&self, node_id: NodeId) -> Option<&AuthoredLineRuntimeV1> {
        self.authored_lines.get(&node_id)
    }

    pub fn authored_stack(&self, page_id: PageId) -> Option<AuthoredStackV1> {
        self.graph.pages.contains_key(&page_id).then(|| {
            self.authored_stacks
                .get(&page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(page_id))
        })
    }

    pub fn can_duplicate_authored_rectangle(
        &self,
        source_node_id: NodeId,
    ) -> Result<(), DuplicateAuthoredRectangleErrorV1> {
        self.validate_source_identity()
            .map_err(DuplicateAuthoredRectangleErrorV1::Commit)?;
        let source = self.authored_shapes.get(&source_node_id).ok_or(
            DuplicateAuthoredRectangleErrorV1::SourceUnsupported {
                node_id: source_node_id,
            },
        )?;
        if !self.graph.pages.contains_key(&source.page_id) {
            return Err(DuplicateAuthoredRectangleErrorV1::SourceUnsupported {
                node_id: source_node_id,
            });
        }
        validate_duplicate_authored_rectangle_source_v1(source)
    }

    pub fn duplicate_authored_rectangle(
        &mut self,
        source_node_id: NodeId,
        destination_node_id: NodeId,
        placement_policy: &str,
    ) -> Result<EditOperation, DuplicateAuthoredRectangleErrorV1> {
        self.can_duplicate_authored_rectangle(source_node_id)?;
        let source = self
            .authored_shapes
            .get(&source_node_id)
            .expect("Duplicate capability verified current authored source")
            .clone();
        let plan =
            plan_duplicate_authored_rectangle_v1(&source, destination_node_id, placement_policy)?;
        self.create_shape(
            plan.destination_node_id,
            plan.page_id,
            plan.bounds,
            plan.paint,
        )
        .map_err(DuplicateAuthoredRectangleErrorV1::Commit)
    }

    pub fn project(&self) -> EditorProject {
        self.try_project().expect(
            "EditorSession public mutation APIs preserve every operation-reachable runtime asset",
        )
    }

    pub fn try_project(&self) -> Result<EditorProject, EditorProjectError> {
        let table_grids = effective_table_grids_with_history(&self.graph, &self.undo);
        let carries_reorder = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::ReorderAuthoredStack { .. }));
        let carries_text_format = self
            .undo
            .iter()
            .any(|operation| text_format_operation_story_id_v1(operation).is_some());
        let carries_paragraph_alignment = self.undo.iter().any(|operation| {
            matches!(
                operation,
                EditOperation::SetParagraphAlignmentOverride { .. }
                    | EditOperation::ClearParagraphAlignmentOverride { .. }
            )
        });
        let carries_create_line = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::CreateLine { .. }));
        let carries_create_table = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::CreateTable { .. }));
        let carries_crop = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::SetImageCrop { .. }));
        let carries_table_track_extent = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::SetTableTrackExtent { .. }));
        let carries_table_rowcol = self
            .undo
            .iter()
            .any(|operation| table_rowcol_history_v1(operation).is_some());
        let carries_link = self
            .undo
            .iter()
            .any(|operation| matches!(operation, EditOperation::LinkTextFrameTail { .. }));
        if (carries_link
            || carries_table_rowcol
            || carries_table_track_extent
            || carries_crop
            || carries_reorder
            || carries_text_format
            || carries_paragraph_alignment
            || carries_create_line
            || carries_create_table)
            && self.project_identity.is_none()
        {
            return Err(EditorProjectError::MissingProjectIdentity);
        }
        let (schema_version, identity) = if let Some(identity) = &self.project_identity {
            (
                minimum_identity_project_schema_v1(&self.undo),
                Some(identity.clone()),
            )
        } else {
            let legacy_schema = if self
                .undo
                .iter()
                .any(|operation| table_rowcol_history_v1(operation).is_some())
            {
                EDITOR_PROJECT_VERSION_V0_21
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::SetTableTrackExtent { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_20
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::SetImageCrop { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_19
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::CreateTable { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_18
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::CreateLine { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_17
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::CreateShape { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_10
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::ResizeNodes { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_9
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::MoveNodes { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_8
            } else if self.undo.iter().any(|operation| {
                matches!(operation, EditOperation::BreakTextFrameForwardLink { .. })
            }) {
                EDITOR_PROJECT_VERSION_V0_7
            } else if !table_grids.is_empty() {
                EDITOR_PROJECT_VERSION_V0_6
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::ResizeNode { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_5
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::MoveNode { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_4
            } else if self
                .undo
                .iter()
                .any(|operation| matches!(operation, EditOperation::ReplaceImage { .. }))
            {
                EDITOR_PROJECT_VERSION_V0_3
            } else {
                EDITOR_PROJECT_VERSION_V0_2
            };
            (legacy_schema, None)
        };

        Ok(EditorProject {
            schema_version: schema_version.into(),
            source_hash: self.source_hash,
            identity,
            assets: self.project_asset_metadata()?,
            table_grids,
            operations: self.undo.clone(),
        })
    }

    pub fn fork_project_next_issue(&self) -> Result<EditorProject, EditorProjectForkError> {
        self.project().fork_next_issue()
    }

    pub fn persistence_requirements(&self) -> Vec<PersistenceRequirement> {
        self.project().persistence_requirements()
    }

    pub fn assess_mature_0x2c_pub_persistence(
        &self,
        writer: &WriterCapabilityManifest,
    ) -> Result<PersistenceCompatibilityAssessment, PersistenceCompatibilityError> {
        assess_mature_0x2c_pub_project_persistence(&self.project(), writer)
    }

    fn project_asset_metadata(&self) -> Result<Vec<EditorProjectAsset>, EditorProjectError> {
        required_editor_asset_refs_v1(&self.undo)
            .into_iter()
            .map(|sha256| {
                let asset = self
                    .replacement_assets
                    .get(&sha256)
                    .ok_or(EditorProjectError::RequiredAssetUnavailable { sha256 })?;
                Ok(editor_project_asset_metadata(asset))
            })
            .collect()
    }

    /// Replays a persisted editor project through the current capability-gated
    /// mutation APIs.
    ///
    /// Replay is transactional: this session is changed only if every
    /// operation is still accepted and regenerates the exact canonical
    /// operation recorded in the project.
    pub fn apply_project(&mut self, project: &EditorProject) -> Result<(), EditorProjectError> {
        self.apply_project_with_assets(project, &BTreeMap::new())
    }

    pub fn apply_project_with_assets(
        &mut self,
        project: &EditorProject,
        asset_bytes: &BTreeMap<Sha256Digest, Vec<u8>>,
    ) -> Result<(), EditorProjectError> {
        self.validate_source_identity()
            .map_err(EditorProjectError::Session)?;

        if project.schema_version != EDITOR_PROJECT_VERSION_V0_22 {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::LinkTextFrameTail { .. }))
            {
                return Err(
                    EditorProjectError::LegacyProjectCarriesLinkTextFrameOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_1
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_2
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_3
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_4
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_5
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_6
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_7
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            return Err(EditorProjectError::UnsupportedSchema {
                found: project.schema_version.clone(),
            });
        }
        if project.schema_version == EDITOR_PROJECT_VERSION_V0_1 && !project.assets.is_empty() {
            return Err(EditorProjectError::LegacyProjectCarriesAssets);
        }
        if project.schema_version == EDITOR_PROJECT_VERSION_V0_1
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_2
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::ReplaceImage { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesImageOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_4
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_5
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_6
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_7
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::MoveNode { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesGeometryOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_5
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_6
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_7
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::ResizeNode { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesResizeOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_6
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_7
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
            && !project.table_grids.is_empty()
        {
            return Err(EditorProjectError::LegacyProjectCarriesTableGrids);
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_7
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project.operations.iter().position(|operation| {
                matches!(operation, EditOperation::BreakTextFrameForwardLink { .. })
            }) {
                return Err(EditorProjectError::LegacyProjectCarriesBreakLinkOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_8
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::MoveNodes { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesMoveNodesOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_9
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::ResizeNodes { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesResizeNodesOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_10
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::CreateShape { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesCreateShapeOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::CreateLine { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesCreateLineOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::CreateTable { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesCreateTableOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::CreateTextBox { .. }))
            {
                return Err(
                    EditorProjectError::LegacyProjectCarriesCreateTextBoxOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::DeleteNode { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesDeleteNodeOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project.operations.iter().position(|operation| {
                matches!(operation, EditOperation::ReorderAuthoredStack { .. })
            }) {
                return Err(
                    EditorProjectError::LegacyProjectCarriesReorderAuthoredStackOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| text_format_operation_story_id_v1(operation).is_some())
            {
                return Err(EditorProjectError::LegacyProjectCarriesTextFormatOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(is_scoped_text_format_operation_v1)
            {
                return Err(
                    EditorProjectError::LegacyProjectCarriesScopedTextFormatOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project.operations.iter().position(|operation| {
                matches!(
                    operation,
                    EditOperation::SetParagraphAlignmentOverride { .. }
                        | EditOperation::ClearParagraphAlignmentOverride { .. }
                )
            }) {
                return Err(
                    EditorProjectError::LegacyProjectCarriesParagraphAlignmentOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| matches!(operation, EditOperation::SetImageCrop { .. }))
            {
                return Err(EditorProjectError::LegacyProjectCarriesImageCropOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project.operations.iter().position(|operation| {
                matches!(operation, EditOperation::SetTableTrackExtent { .. })
            }) {
                return Err(
                    EditorProjectError::LegacyProjectCarriesTableTrackExtentOperation { index },
                );
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
        {
            if let Some(index) = project
                .operations
                .iter()
                .position(|operation| table_rowcol_history_v1(operation).is_some())
            {
                return Err(EditorProjectError::LegacyProjectCarriesTableRowColOperation { index });
            }
        }
        if project.schema_version != EDITOR_PROJECT_VERSION_V0_11
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_12
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_13
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_14
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_15
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_16
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_17
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_18
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_19
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_20
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_21
            && project.schema_version != EDITOR_PROJECT_VERSION_V0_22
            && project.identity.is_some()
        {
            return Err(EditorProjectError::LegacyProjectCarriesIdentity);
        }
        if (project.schema_version == EDITOR_PROJECT_VERSION_V0_11
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_12
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_13
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_14
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_15
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_16
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_17
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_18
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_19
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_20
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_21
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_22)
            && project.identity.is_none()
        {
            return Err(EditorProjectError::MissingProjectIdentity);
        }
        if project.source_hash != self.source_hash {
            return Err(EditorProjectError::SourceHashMismatch {
                expected: self.source_hash,
                found: project.source_hash,
            });
        }
        if !self.undo.is_empty()
            || !self.redo.is_empty()
            || !self.replacement_assets.is_empty()
            || !self.image_replacements.is_empty()
            || !self.image_crop_overrides.is_empty()
            || !self.authored_shapes.is_empty()
            || !self.authored_lines.is_empty()
            || !self.authored_stacks.is_empty()
        {
            return Err(EditorProjectError::SessionNotEmpty);
        }

        if project.schema_version == EDITOR_PROJECT_VERSION_V0_11
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_12
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_13
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_14
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_15
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_16
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_17
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_18
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_19
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_20
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_21
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_22
        {
            let expected = required_editor_asset_refs_v1(&project.operations)
                .into_iter()
                .collect::<Vec<_>>();
            let found = project
                .assets
                .iter()
                .map(|asset| asset.sha256)
                .collect::<Vec<_>>();
            if found != expected {
                return Err(EditorProjectError::AssetReachabilityMismatch { expected, found });
            }
        }

        let mut candidate = self.clone();
        candidate.project_identity = project.identity.clone();
        for (index, metadata) in project.assets.iter().enumerate() {
            let bytes = asset_bytes
                .get(&metadata.sha256)
                .ok_or(EditorProjectError::MissingAssetBytes {
                    sha256: metadata.sha256,
                })?
                .clone();
            let found_len = u64::try_from(bytes.len()).map_err(|_| EditorProjectError::Asset {
                index,
                error: EditorAssetError::ByteLengthOverflow,
            })?;
            if found_len != metadata.byte_len {
                return Err(EditorProjectError::AssetLengthMismatch {
                    sha256: metadata.sha256,
                    expected: metadata.byte_len,
                    found: found_len,
                });
            }

            let asset = validated_editor_asset(metadata.mime.clone(), bytes)
                .map_err(|error| EditorProjectError::Asset { index, error })?;
            if asset.sha256 != metadata.sha256 {
                return Err(EditorProjectError::AssetHashMismatch {
                    expected: metadata.sha256,
                    found: asset.sha256,
                });
            }
            candidate.replacement_assets.insert(metadata.sha256, asset);
        }

        if canonical_editor_asset_metadata(&candidate.replacement_assets) != project.assets {
            return Err(EditorProjectError::AssetMetadataNonCanonical);
        }

        for (index, expected) in project.operations.iter().enumerate() {
            let actual = replay_canonical_operation(&mut candidate, expected, index)?;
            if &actual != expected {
                return Err(EditorProjectError::OperationMismatch { index });
            }
        }

        if project.schema_version == EDITOR_PROJECT_VERSION_V0_6
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_7
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_8
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_9
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_10
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_11
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_12
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_13
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_14
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_15
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_16
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_17
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_18
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_19
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_20
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_21
            || project.schema_version == EDITOR_PROJECT_VERSION_V0_22
        {
            let actual_grids =
                effective_table_grids_with_history(&candidate.graph, &candidate.undo);
            if actual_grids != project.table_grids {
                return Err(EditorProjectError::TableGridMismatch);
            }
        }

        *self = candidate;
        Ok(())
    }

    pub fn preview_editable_export(
        &self,
        target: EditorEditableTarget,
        source_label: impl Into<String>,
    ) -> Result<EditorEditableExportPreview, EditorExportError> {
        let (report, human_summary, plan) =
            self.build_editable_export_plan(target, source_label.into())?;

        if report.can_serialize {
            match target {
                EditorEditableTarget::Idml => {
                    let projection_plan =
                        idml_base_projection_plan(&plan, self.externally_projected_image_nodes());
                    let mut package = project_resolved_graph_to_idml(
                        &projection_plan,
                        &self.graph,
                        &IdmlWireProfile::legacy_dom_7(),
                        frame_from_payload,
                    )
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                    let placements = self.idml_image_placements()?;
                    add_embedded_images_to_idml(&plan, &mut package, &placements).map_err(
                        |error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        },
                    )?;
                    let typography = self.full_story_typography_v1();
                    add_full_story_typography_to_idml(&plan, &mut package, &typography).map_err(
                        |error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        },
                    )?;
                    let (paragraph_alignments, scoped_paragraph_alignments) = self
                        .effective_editable_paragraph_alignment_inputs_v1()
                        .map_err(|message| EditorExportError::Projection { target, message })?;
                    let scoped_placements = self
                        .idml_paragraph_scoped_alignment_placements_v1(&scoped_paragraph_alignments)
                        .map_err(|message| EditorExportError::Projection { target, message })?;
                    add_paragraph_scoped_alignment_to_idml(&plan, &mut package, &scoped_placements)
                        .map_err(|error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        })?;
                    add_full_story_paragraph_alignment_to_idml(
                        &plan,
                        &mut package,
                        &paragraph_alignments,
                    )
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                }
                EditorEditableTarget::Odg => {
                    let mut package =
                        project_resolved_graph_to_odg(&plan, &self.graph, frame_from_payload)
                            .map_err(|error| EditorExportError::Projection {
                                target,
                                message: error.to_string(),
                            })?;
                    let placements = self.odg_image_placements()?;
                    add_embedded_images_to_odg(&plan, &mut package, &placements).map_err(
                        |error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        },
                    )?;
                    let typography = self.full_story_typography_v1();
                    let typography_placements =
                        self.odg_full_story_typography_placements_v1(&typography);
                    add_full_story_typography_to_odg(&plan, &mut package, &typography_placements)
                        .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                    let (paragraph_alignments, scoped_paragraph_alignments) = self
                        .effective_editable_paragraph_alignment_inputs_v1()
                        .map_err(|message| EditorExportError::Projection { target, message })?;
                    let scoped_placements = self
                        .odg_paragraph_scoped_alignment_placements_v1(&scoped_paragraph_alignments);
                    add_paragraph_scoped_alignment_to_odg(&plan, &mut package, &scoped_placements)
                        .map_err(|error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        })?;
                    let paragraph_alignment_placements = self
                        .odg_full_story_paragraph_alignment_placements_v1(&paragraph_alignments);
                    add_full_story_paragraph_alignment_to_odg(
                        &plan,
                        &mut package,
                        &paragraph_alignment_placements,
                    )
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                }
            }
        }

        Ok(EditorEditableExportPreview {
            target,
            report,
            human_summary,
        })
    }

    pub fn export_editable(
        &self,
        target: EditorEditableTarget,
        source_label: impl Into<String>,
    ) -> Result<EditorEditableExport, EditorExportError> {
        let (report, human_summary, plan) =
            self.build_editable_export_plan(target, source_label.into())?;
        if !report.can_serialize {
            return Err(EditorExportError::Blocked {
                target,
                blockers: report.counts.blocking,
            });
        }

        let bytes = match target {
            EditorEditableTarget::Idml => {
                let projection_plan =
                    idml_base_projection_plan(&plan, self.externally_projected_image_nodes());
                let mut package = project_resolved_graph_to_idml(
                    &projection_plan,
                    &self.graph,
                    &IdmlWireProfile::legacy_dom_7(),
                    frame_from_payload,
                )
                .map_err(|error| EditorExportError::Projection {
                    target,
                    message: error.to_string(),
                })?;
                let placements = self.idml_image_placements()?;
                add_embedded_images_to_idml(&plan, &mut package, &placements).map_err(|error| {
                    EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    }
                })?;
                let typography = self.full_story_typography_v1();
                add_full_story_typography_to_idml(&plan, &mut package, &typography).map_err(
                    |error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    },
                )?;
                let (paragraph_alignments, scoped_paragraph_alignments) = self
                    .effective_editable_paragraph_alignment_inputs_v1()
                    .map_err(|message| EditorExportError::Projection { target, message })?;
                let scoped_placements = self
                    .idml_paragraph_scoped_alignment_placements_v1(&scoped_paragraph_alignments)
                    .map_err(|message| EditorExportError::Projection { target, message })?;
                add_paragraph_scoped_alignment_to_idml(&plan, &mut package, &scoped_placements)
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                add_full_story_paragraph_alignment_to_idml(
                    &plan,
                    &mut package,
                    &paragraph_alignments,
                )
                .map_err(|error| EditorExportError::Projection {
                    target,
                    message: error.to_string(),
                })?;
                write_idml_ucf(&package).map_err(|error| EditorExportError::Write {
                    target,
                    message: error.to_string(),
                })?
            }
            EditorEditableTarget::Odg => {
                let mut package =
                    project_resolved_graph_to_odg(&plan, &self.graph, frame_from_payload).map_err(
                        |error| EditorExportError::Projection {
                            target,
                            message: error.to_string(),
                        },
                    )?;
                let placements = self.odg_image_placements()?;
                add_embedded_images_to_odg(&plan, &mut package, &placements).map_err(|error| {
                    EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    }
                })?;
                let typography = self.full_story_typography_v1();
                let typography_placements =
                    self.odg_full_story_typography_placements_v1(&typography);
                add_full_story_typography_to_odg(&plan, &mut package, &typography_placements)
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                let (paragraph_alignments, scoped_paragraph_alignments) = self
                    .effective_editable_paragraph_alignment_inputs_v1()
                    .map_err(|message| EditorExportError::Projection { target, message })?;
                let scoped_placements =
                    self.odg_paragraph_scoped_alignment_placements_v1(&scoped_paragraph_alignments);
                add_paragraph_scoped_alignment_to_odg(&plan, &mut package, &scoped_placements)
                    .map_err(|error| EditorExportError::Projection {
                        target,
                        message: error.to_string(),
                    })?;
                let paragraph_alignment_placements =
                    self.odg_full_story_paragraph_alignment_placements_v1(&paragraph_alignments);
                add_full_story_paragraph_alignment_to_odg(
                    &plan,
                    &mut package,
                    &paragraph_alignment_placements,
                )
                .map_err(|error| EditorExportError::Projection {
                    target,
                    message: error.to_string(),
                })?;
                write_odg(&package).map_err(|error| EditorExportError::Write {
                    target,
                    message: error.to_string(),
                })?
            }
        };

        Ok(EditorEditableExport {
            target,
            report,
            human_summary,
            bytes,
        })
    }

    pub fn full_story_typography_v1(&self) -> Vec<FullStoryTypographyV1> {
        let table_story_ids = self
            .graph
            .nodes
            .values()
            .flat_map(|node| {
                [
                    node.payload
                        .table_story
                        .as_ref()
                        .and_then(|owner| owner.story_id),
                    node.payload.table.as_ref().and_then(|table| table.story_id),
                ]
            })
            .flatten()
            .collect::<BTreeSet<_>>();
        let ordinary_story_ids = self
            .graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| frame_from_payload(*node_id, &node.payload))
            .map(|frame| frame.story_id)
            .filter(|story_id| !table_story_ids.contains(story_id))
            .collect::<BTreeSet<_>>();

        let mut runs_by_story = BTreeMap::<StoryId, Vec<&PubTypographyRun>>::new();
        for run in &self.source_typography_runs {
            if ordinary_story_ids.contains(&run.story_id)
                && self.graph.stories.contains_key(&run.story_id)
            {
                runs_by_story.entry(run.story_id).or_default().push(run);
            }
        }

        let mut size_only_by_story = BTreeMap::<StoryId, Vec<&PubTypographySizeRun>>::new();
        for run in &self.source_typography_size_runs {
            if ordinary_story_ids.contains(&run.story_id)
                && self.graph.stories.contains_key(&run.story_id)
            {
                size_only_by_story
                    .entry(run.story_id)
                    .or_default()
                    .push(run);
            }
        }

        let mut result = Vec::new();
        for (story_id, mut runs) in runs_by_story {
            let Some(story) = self.graph.stories.get(&story_id) else {
                continue;
            };
            let current_story_state_id = story_state_id_v1(story_id, &story.text);
            if self.source_story_state_ids.get(&story_id) != Some(&current_story_state_id) {
                continue;
            }
            let Ok(story_scalar_len) = u32::try_from(story.text.chars().count()) else {
                continue;
            };
            let Ok(story_utf16_len) = u32::try_from(story.text.encode_utf16().count()) else {
                continue;
            };
            if story_scalar_len == 0 || story_utf16_len == 0 {
                continue;
            }

            runs.sort_by_key(|run| {
                (
                    run.story_scalar_start,
                    run.story_scalar_end,
                    run.story_utf16_start,
                    run.story_utf16_end,
                )
            });

            let mut scalar_cursor = 0_u32;
            let mut utf16_cursor = 0_u32;
            let mut font_family: Option<&str> = None;
            let mut font_size_emu: Option<u32> = None;
            let mut valid = true;

            for run in runs {
                if run.story_scalar_start != scalar_cursor
                    || run.story_utf16_start != utf16_cursor
                    || run.story_scalar_end <= run.story_scalar_start
                    || run.story_utf16_end <= run.story_utf16_start
                    || run.story_scalar_end > story_scalar_len
                    || run.story_utf16_end > story_utf16_len
                    || run.font_inherited
                    || run.size_inherited
                    || run.source_font_name.trim().is_empty()
                    || run.text_size_emu == 0
                {
                    valid = false;
                    break;
                }

                match font_family {
                    None => font_family = Some(run.source_font_name.as_str()),
                    Some(existing) if existing == run.source_font_name.as_str() => {}
                    Some(_) => {
                        valid = false;
                        break;
                    }
                }
                match font_size_emu {
                    None => font_size_emu = Some(run.text_size_emu),
                    Some(existing) if existing == run.text_size_emu => {}
                    Some(_) => {
                        valid = false;
                        break;
                    }
                }

                scalar_cursor = run.story_scalar_end;
                utf16_cursor = run.story_utf16_end;
            }

            if !valid || scalar_cursor != story_scalar_len || utf16_cursor != story_utf16_len {
                continue;
            }

            let Some(font_family) = font_family else {
                continue;
            };
            let Some(font_size_emu) = font_size_emu else {
                continue;
            };

            if size_only_by_story.get(&story_id).is_some_and(|runs| {
                runs.iter().any(|run| {
                    run.size_inherited
                        || run.text_size_emu == 0
                        || run.text_size_emu != font_size_emu
                        || run.story_scalar_end > story_scalar_len
                        || run.story_utf16_end > story_utf16_len
                })
            }) {
                continue;
            }

            result.push(FullStoryTypographyV1 {
                story_id,
                font_family: font_family.to_owned(),
                font_size_emu: LengthEmu::new(i64::from(font_size_emu)),
            });
        }

        result.sort_by_key(|item| item.story_id);
        result
    }

    fn effective_paragraph_scoped_alignment_v1(
        &self,
    ) -> Result<Vec<ParagraphScopedAlignmentV1>, String> {
        self.validate_source_identity()
            .map_err(|error| error.to_string())?;

        let overrides = self
            .current_paragraph_alignment_overrides_v1()
            .map_err(|error| error.to_string())?;
        let paragraphs = self
            .imported_paragraphs_v1()
            .map_err(|error| error.to_string())?;
        let mut scoped_story_ids = paragraphs
            .iter()
            .filter(|paragraph| overrides.contains_key(&paragraph.paragraph_id))
            .map(|paragraph| paragraph.story_id)
            .collect::<BTreeSet<_>>();

        let mut touched_paragraph_ids = BTreeSet::<ParagraphId>::new();
        for operation in &self.undo {
            match operation {
                EditOperation::SetParagraphAlignmentOverride { paragraph_ids, .. }
                | EditOperation::ClearParagraphAlignmentOverride { paragraph_ids, .. } => {
                    touched_paragraph_ids.extend(paragraph_ids.iter().copied());
                }
                _ => {}
            }
        }
        for paragraph in &paragraphs {
            if touched_paragraph_ids.contains(&paragraph.paragraph_id) {
                scoped_story_ids.insert(paragraph.story_id);
            }
        }

        if scoped_story_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for paragraph in paragraphs {
            if !scoped_story_ids.contains(&paragraph.story_id) {
                continue;
            }

            let effective = self
                .effective_paragraph_alignment_v1(paragraph.paragraph_id)
                .map_err(|error| error.to_string())?;
            let alignment =
                match authored_paragraph_alignment_v1::paragraph_scoped_alignment_value_v1(
                    effective.effective,
                ) {
                    Ok(alignment) => alignment,
                    Err(
                        authored_paragraph_alignment_v1::ParagraphScopedAlignmentProjectionErrorV1::InterWord,
                    ) => {
                        return Err(format!(
                            "Story {} Paragraph {} has effective InterWord alignment, which is outside the paragraph-scoped editable writer",
                            paragraph.story_id.as_canonical(),
                            paragraph.paragraph_id.as_canonical()
                        ));
                    }
                    Err(
                        authored_paragraph_alignment_v1::ParagraphScopedAlignmentProjectionErrorV1::Distribute,
                    ) => {
                        return Err(format!(
                            "Story {} Paragraph {} has effective Distribute alignment, which is outside the paragraph-scoped editable writer",
                            paragraph.story_id.as_canonical(),
                            paragraph.paragraph_id.as_canonical()
                        ));
                    }
                    Err(
                        authored_paragraph_alignment_v1::ParagraphScopedAlignmentProjectionErrorV1::Unknown,
                    ) => {
                        return Err(format!(
                            "Story {} Paragraph {} has unknown effective alignment, so scoped editable export cannot cover the complete canonical Story",
                            paragraph.story_id.as_canonical(),
                            paragraph.paragraph_id.as_canonical()
                        ));
                    }
                };

            result.push(ParagraphScopedAlignmentV1 {
                story_id: paragraph.story_id,
                paragraph_id: paragraph.paragraph_id,
                range: paragraph.range,
                alignment,
            });
        }

        result.sort_by_key(|item| {
            (
                item.story_id,
                item.range.start,
                item.range.end,
                item.paragraph_id,
            )
        });
        Ok(result)
    }

    fn idml_paragraph_scoped_alignment_placements_v1(
        &self,
        alignments: &[ParagraphScopedAlignmentV1],
    ) -> Result<Vec<IdmlParagraphScopedAlignmentPlacement>, String> {
        let mut by_story = BTreeMap::<StoryId, Vec<ParagraphScopedAlignmentV1>>::new();
        for item in alignments {
            by_story
                .entry(item.story_id)
                .or_default()
                .push(item.clone());
        }

        let mut result = Vec::with_capacity(by_story.len());
        for (story_id, mut paragraphs) in by_story {
            let story = self.graph.stories.get(&story_id).ok_or_else(|| {
                format!(
                    "scoped paragraph alignment Story {} is missing from current graph",
                    story_id.as_canonical()
                )
            })?;
            paragraphs.sort_by_key(|item| (item.range.start, item.range.end, item.paragraph_id));
            result.push(IdmlParagraphScopedAlignmentPlacement {
                story_id,
                story_text: story.text.clone(),
                paragraphs,
            });
        }
        Ok(result)
    }

    fn odg_paragraph_scoped_alignment_placements_v1(
        &self,
        alignments: &[ParagraphScopedAlignmentV1],
    ) -> Vec<OdgParagraphScopedAlignmentPlacement> {
        let scoped_story_ids = alignments
            .iter()
            .map(|item| item.story_id)
            .collect::<BTreeSet<_>>();
        let mut by_story = BTreeMap::<StoryId, Vec<ParagraphScopedAlignmentV1>>::new();
        for item in alignments {
            by_story
                .entry(item.story_id)
                .or_default()
                .push(item.clone());
        }

        let mut roots = BTreeMap::<StoryId, Vec<NodeId>>::new();
        for (node_id, node) in &self.graph.nodes {
            let Some(frame) = frame_from_payload(*node_id, &node.payload) else {
                continue;
            };
            if frame.previous.is_none() && scoped_story_ids.contains(&frame.story_id) {
                roots
                    .entry(frame.story_id)
                    .or_default()
                    .push(frame.frame_id);
            }
        }

        by_story
            .into_iter()
            .filter_map(|(story_id, mut paragraphs)| {
                let mut frame_ids = roots.get(&story_id)?.clone();
                frame_ids.sort_unstable();
                frame_ids.dedup();
                paragraphs
                    .sort_by_key(|item| (item.range.start, item.range.end, item.paragraph_id));
                (!frame_ids.is_empty()).then_some(OdgParagraphScopedAlignmentPlacement {
                    story_id,
                    paragraphs,
                    frame_ids,
                })
            })
            .collect()
    }

    fn effective_editable_paragraph_alignment_inputs_v1(
        &self,
    ) -> Result<
        (
            Vec<FullStoryParagraphAlignmentV1>,
            Vec<ParagraphScopedAlignmentV1>,
        ),
        String,
    > {
        let scoped = self.effective_paragraph_scoped_alignment_v1()?;
        let scoped_story_ids = scoped
            .iter()
            .map(|item| item.story_id)
            .collect::<BTreeSet<_>>();
        let full_story = self
            .effective_full_story_paragraph_alignment_v1()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|item| !scoped_story_ids.contains(&item.story_id))
            .collect();
        Ok((full_story, scoped))
    }

    pub fn effective_full_story_paragraph_alignment_v1(
        &self,
    ) -> Result<Vec<FullStoryParagraphAlignmentV1>, EditorError> {
        self.validate_source_identity()?;

        let table_story_ids = self
            .graph
            .nodes
            .values()
            .flat_map(|node| {
                [
                    node.payload
                        .table_story
                        .as_ref()
                        .and_then(|owner| owner.story_id),
                    node.payload.table.as_ref().and_then(|table| table.story_id),
                ]
            })
            .flatten()
            .collect::<BTreeSet<_>>();
        let ordinary_story_ids = self
            .graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| frame_from_payload(*node_id, &node.payload))
            .map(|frame| frame.story_id)
            .filter(|story_id| !table_story_ids.contains(story_id))
            .collect::<BTreeSet<_>>();

        let mut paragraphs_by_story = BTreeMap::<StoryId, Vec<ParagraphId>>::new();
        for paragraph in self
            .imported_paragraphs_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
        {
            if ordinary_story_ids.contains(&paragraph.story_id) {
                paragraphs_by_story
                    .entry(paragraph.story_id)
                    .or_default()
                    .push(paragraph.paragraph_id);
            }
        }

        let mut result = Vec::new();
        for (story_id, mut paragraph_ids) in paragraphs_by_story {
            paragraph_ids.sort_unstable();
            paragraph_ids.dedup();
            if paragraph_ids.is_empty() {
                continue;
            }

            let mut uniform: Option<ParagraphAlignmentV1> = None;
            let mut valid = true;
            for paragraph_id in paragraph_ids {
                let effective = self.effective_paragraph_alignment_v1(paragraph_id)?;
                let current = match effective.effective {
                    Some(EffectiveParagraphAlignmentValueV1::Center) => {
                        ParagraphAlignmentV1::Center
                    }
                    Some(EffectiveParagraphAlignmentValueV1::Right) => ParagraphAlignmentV1::Right,
                    _ => {
                        valid = false;
                        break;
                    }
                };

                match uniform {
                    None => uniform = Some(current),
                    Some(existing) if existing == current => {}
                    Some(_) => {
                        valid = false;
                        break;
                    }
                }
            }

            if valid {
                if let Some(alignment) = uniform {
                    result.push(FullStoryParagraphAlignmentV1 {
                        story_id,
                        alignment,
                    });
                }
            }
        }

        result.sort_by_key(|item| item.story_id);
        Ok(result)
    }

    fn odg_full_story_paragraph_alignment_placements_v1(
        &self,
        alignments: &[FullStoryParagraphAlignmentV1],
    ) -> Vec<OdgFullStoryParagraphAlignmentPlacement> {
        let eligible = alignments
            .iter()
            .map(|item| item.story_id)
            .collect::<BTreeSet<_>>();
        let mut roots = BTreeMap::<StoryId, Vec<NodeId>>::new();

        for (node_id, node) in &self.graph.nodes {
            let Some(frame) = frame_from_payload(*node_id, &node.payload) else {
                continue;
            };
            if frame.previous.is_none() && eligible.contains(&frame.story_id) {
                roots
                    .entry(frame.story_id)
                    .or_default()
                    .push(frame.frame_id);
            }
        }

        alignments
            .iter()
            .filter_map(|item| {
                let mut frame_ids = roots.get(&item.story_id)?.clone();
                frame_ids.sort_unstable();
                frame_ids.dedup();
                (!frame_ids.is_empty()).then(|| OdgFullStoryParagraphAlignmentPlacement {
                    alignment: item.clone(),
                    frame_ids,
                })
            })
            .collect()
    }

    fn odg_full_story_typography_placements_v1(
        &self,
        typography: &[FullStoryTypographyV1],
    ) -> Vec<OdgFullStoryTypographyPlacement> {
        let eligible = typography
            .iter()
            .map(|item| item.story_id)
            .collect::<BTreeSet<_>>();
        let mut roots = BTreeMap::<StoryId, Vec<NodeId>>::new();

        for (node_id, node) in &self.graph.nodes {
            let Some(frame) = frame_from_payload(*node_id, &node.payload) else {
                continue;
            };
            if frame.previous.is_none() && eligible.contains(&frame.story_id) {
                roots
                    .entry(frame.story_id)
                    .or_default()
                    .push(frame.frame_id);
            }
        }

        typography
            .iter()
            .filter_map(|item| {
                let mut frame_ids = roots.get(&item.story_id)?.clone();
                frame_ids.sort_unstable();
                frame_ids.dedup();
                (!frame_ids.is_empty()).then(|| OdgFullStoryTypographyPlacement {
                    typography: item.clone(),
                    frame_ids,
                })
            })
            .collect()
    }

    fn build_editable_export_plan(
        &self,
        target: EditorEditableTarget,
        source_label: String,
    ) -> Result<(ExportReport, String, ExportPlan), EditorExportError> {
        self.validate_source_identity()
            .map_err(EditorExportError::Session)?;
        let typography = self.full_story_typography_v1();
        let paragraph_alignments = self
            .effective_full_story_paragraph_alignment_v1()
            .map_err(EditorExportError::Session)?;
        let image_state = EditableExportImageState {
            replacements: &self.image_replacements,
            crop_overrides: &self.image_crop_overrides,
            source_nodes: &self.source_image_nodes,
        };
        let plan = editable_export_plan(
            target,
            &self.graph,
            image_state,
            EditableExportTypographyInputs {
                source_typography_runs: &self.source_typography_runs,
                source_typography_size_runs: &self.source_typography_size_runs,
                source_paragraph_alignments: &self.source_paragraph_alignments,
                full_story_typography: &typography,
                effective_full_story_paragraph_alignments: &paragraph_alignments,
            },
        )
        .map_err(|error| EditorExportError::Report(error.to_string()))?;
        let report = build_export_report(
            &plan,
            ExportReportSource {
                label: source_label,
                source_hash: Some(self.source_hash),
            },
        )
        .map_err(|error| EditorExportError::Report(error.to_string()))?;
        let human_summary = render_human_summary(&report);
        Ok((report, human_summary, plan))
    }

    fn idml_image_placements(&self) -> Result<Vec<IdmlEmbeddedImagePlacement>, EditorExportError> {
        let mut placements = self.idml_source_placements()?;
        placements.extend(self.idml_replacement_placements()?);
        placements.sort_by_key(|placement| placement.node_id);
        Ok(placements)
    }

    fn idml_source_placements(&self) -> Result<Vec<IdmlEmbeddedImagePlacement>, EditorExportError> {
        let target = EditorEditableTarget::Idml;
        let mut placements = Vec::with_capacity(self.source_image_nodes.len());

        for (node_id, resource_id) in &self.source_image_nodes {
            if self.image_replacements.contains_key(node_id) {
                continue;
            }
            let node =
                self.graph
                    .nodes
                    .get(node_id)
                    .ok_or_else(|| EditorExportError::Projection {
                        target,
                        message: format!(
                            "source image node {} is missing from the resolved graph",
                            node_id.as_canonical()
                        ),
                    })?;
            let asset = self.source_image_assets.get(resource_id).ok_or_else(|| {
                EditorExportError::Projection {
                    target,
                    message: format!(
                        "source image resource {} has no exact PNG/JPEG byte backing",
                        resource_id.as_canonical()
                    ),
                }
            })?;
            let (page_id, page) = self
                .graph
                .pages
                .iter()
                .find(|(page_id, _)| page_id.into_canonical() == node.header.parent_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "source image node {} is not directly authored on a page",
                        node_id.as_canonical()
                    ),
                })?;

            placements.push(IdmlEmbeddedImagePlacement {
                node_id: *node_id,
                page_id: *page_id,
                page_size: page.size,
                resource_id: *resource_id,
                frame_bounds: node.header.bounds,
                mime: asset.mime.clone(),
                bytes: asset.bytes.clone(),
            });
        }

        Ok(placements)
    }

    fn idml_replacement_placements(
        &self,
    ) -> Result<Vec<IdmlEmbeddedImagePlacement>, EditorExportError> {
        let target = EditorEditableTarget::Idml;
        let mut placements = Vec::with_capacity(self.image_replacements.len());

        for (node_id, asset_sha) in &self.image_replacements {
            let node =
                self.graph
                    .nodes
                    .get(node_id)
                    .ok_or_else(|| EditorExportError::Projection {
                        target,
                        message: format!(
                            "replacement image node {} is missing from the resolved graph",
                            node_id.as_canonical()
                        ),
                    })?;
            let asset = self.replacement_assets.get(asset_sha).ok_or_else(|| {
                EditorExportError::Projection {
                    target,
                    message: format!(
                        "replacement image asset {asset_sha} is missing from the editor session"
                    ),
                }
            })?;
            let (page_id, page) = self
                .graph
                .pages
                .iter()
                .find(|(page_id, _)| page_id.into_canonical() == node.header.parent_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "replacement image node {} is not directly authored on a page",
                        node_id.as_canonical()
                    ),
                })?;

            placements.push(IdmlEmbeddedImagePlacement {
                node_id: *node_id,
                page_id: *page_id,
                page_size: page.size,
                resource_id: replacement_asset_resource_id(*asset_sha),
                frame_bounds: node.header.bounds,
                mime: asset.mime.clone(),
                bytes: asset.bytes.clone(),
            });
        }

        Ok(placements)
    }

    fn odg_image_placements(&self) -> Result<Vec<OdgEmbeddedImagePlacement>, EditorExportError> {
        let mut placements = self.odg_source_placements()?;
        placements.extend(self.odg_replacement_placements()?);
        placements
            .sort_by_key(|placement| (placement.page_id, placement.z_index, placement.node_id));
        Ok(placements)
    }

    fn odg_source_placements(&self) -> Result<Vec<OdgEmbeddedImagePlacement>, EditorExportError> {
        let target = EditorEditableTarget::Odg;
        let mut placements = Vec::with_capacity(self.source_image_nodes.len());

        for (node_id, resource_id) in &self.source_image_nodes {
            if self.image_replacements.contains_key(node_id) {
                continue;
            }
            let node =
                self.graph
                    .nodes
                    .get(node_id)
                    .ok_or_else(|| EditorExportError::Projection {
                        target,
                        message: format!(
                            "source image node {} is missing from the resolved graph",
                            node_id.as_canonical()
                        ),
                    })?;
            let asset = self.source_image_assets.get(resource_id).ok_or_else(|| {
                EditorExportError::Projection {
                    target,
                    message: format!(
                        "source image resource {} has no exact PNG/JPEG byte backing",
                        resource_id.as_canonical()
                    ),
                }
            })?;
            let (page_id, page) = self
                .graph
                .pages
                .iter()
                .find(|(page_id, _)| page_id.into_canonical() == node.header.parent_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "source image node {} is not directly authored on a page",
                        node_id.as_canonical()
                    ),
                })?;

            let authored = self
                .graph
                .nodes
                .iter()
                .filter_map(|(candidate_id, candidate)| {
                    (candidate.header.parent_id == page_id.into_canonical())
                        .then_some(*candidate_id)
                })
                .collect::<Vec<_>>();
            let ordered = if page.children.len() == authored.len()
                && page
                    .children
                    .iter()
                    .zip(authored.iter())
                    .all(|(left, right)| left == right)
            {
                page.children.as_slice()
            } else {
                authored.as_slice()
            };
            let z_index = ordered
                .iter()
                .position(|candidate| candidate == node_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "source image node {} has no page-local object order",
                        node_id.as_canonical()
                    ),
                })?;

            placements.push(OdgEmbeddedImagePlacement {
                node_id: *node_id,
                page_id: *page_id,
                resource_id: *resource_id,
                frame_bounds: node.header.bounds,
                z_index,
                mime: asset.mime.clone(),
                bytes: asset.bytes.clone(),
            });
        }

        Ok(placements)
    }

    fn odg_replacement_placements(
        &self,
    ) -> Result<Vec<OdgEmbeddedImagePlacement>, EditorExportError> {
        let target = EditorEditableTarget::Odg;
        let mut placements = Vec::with_capacity(self.image_replacements.len());

        for (node_id, asset_sha) in &self.image_replacements {
            let node =
                self.graph
                    .nodes
                    .get(node_id)
                    .ok_or_else(|| EditorExportError::Projection {
                        target,
                        message: format!(
                            "replacement image node {} is missing from the resolved graph",
                            node_id.as_canonical()
                        ),
                    })?;
            let asset = self.replacement_assets.get(asset_sha).ok_or_else(|| {
                EditorExportError::Projection {
                    target,
                    message: format!(
                        "replacement image asset {asset_sha} is missing from the editor session"
                    ),
                }
            })?;
            let (page_id, page) = self
                .graph
                .pages
                .iter()
                .find(|(page_id, _)| page_id.into_canonical() == node.header.parent_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "replacement image node {} is not directly authored on a page",
                        node_id.as_canonical()
                    ),
                })?;

            let authored = self
                .graph
                .nodes
                .iter()
                .filter_map(|(candidate_id, candidate)| {
                    (candidate.header.parent_id == page_id.into_canonical())
                        .then_some(*candidate_id)
                })
                .collect::<Vec<_>>();
            let ordered = if page.children.len() == authored.len()
                && page
                    .children
                    .iter()
                    .zip(authored.iter())
                    .all(|(left, right)| left == right)
            {
                page.children.as_slice()
            } else {
                authored.as_slice()
            };
            let z_index = ordered
                .iter()
                .position(|candidate| candidate == node_id)
                .ok_or_else(|| EditorExportError::Projection {
                    target,
                    message: format!(
                        "replacement image node {} has no page-local object order",
                        node_id.as_canonical()
                    ),
                })?;

            placements.push(OdgEmbeddedImagePlacement {
                node_id: *node_id,
                page_id: *page_id,
                resource_id: replacement_asset_resource_id(*asset_sha),
                frame_bounds: node.header.bounds,
                z_index,
                mime: asset.mime.clone(),
                bytes: asset.bytes.clone(),
            });
        }

        Ok(placements)
    }

    pub fn create_text_box(
        &mut self,
        node_id: NodeId,
        story_id: StoryId,
        page_id: PageId,
        bounds: RectEmu,
        text_preset: AuthoringTextPresetV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        validate_create_text_box_candidate(
            &self.graph,
            node_id,
            story_id,
            page_id,
            bounds,
            &text_preset,
        )?;

        let operation = EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            text_preset,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    fn current_authored_stack_v1(&self, page_id: PageId) -> AuthoredStackV1 {
        self.authored_stacks
            .get(&page_id)
            .cloned()
            .unwrap_or_else(|| AuthoredStackV1::empty(page_id))
    }

    fn install_authored_stack_v1(&mut self, stack: AuthoredStackV1) {
        if stack.members.is_empty() {
            self.authored_stacks.remove(&stack.page_id);
        } else {
            self.authored_stacks.insert(stack.page_id, stack);
        }
    }

    pub fn create_shape(
        &mut self,
        node_id: NodeId,
        page_id: PageId,
        bounds: RectEmu,
        paint: AuthoredShapePaintV1,
    ) -> Result<EditOperation, EditorError> {
        let operation = EditOperation::CreateShape {
            node_id,
            page_id,
            parent_id: page_id,
            shape_kind: AuthoredShapeKindV1::Rectangle,
            bounds,
            transform: AuthoredShapeTransformV1::Identity,
            paint,
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        self.consume_canonical_create_shape(operation)
    }

    fn consume_canonical_create_shape(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let shape = authored_shape_from_operation(&operation)
            .expect("consume_canonical_create_shape receives CreateShape");
        self.validate_create_shape_candidate(&shape)?;

        let before_stack = self.current_authored_stack_v1(shape.page_id);
        let transition = plan_create_shape_append_v1(&before_stack, &shape).map_err(|_| {
            EditorError::StaleAuthoredStack {
                page_id: shape.page_id,
            }
        })?;
        let after_stack = apply_authored_stack_transition_forward_v1(&before_stack, &transition)
            .map_err(|_| EditorError::StaleAuthoredStack {
                page_id: shape.page_id,
            })?;

        self.authored_shapes.insert(shape.node_id, shape);
        self.install_authored_stack_v1(after_stack);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    fn validate_create_shape_candidate(
        &self,
        shape: &AuthoredShapeRuntimeV1,
    ) -> Result<(), EditorError> {
        if !self.graph.pages.contains_key(&shape.page_id) {
            return Err(EditorError::CreateShapePageMissing {
                page_id: shape.page_id,
            });
        }
        if self.graph.nodes.contains_key(&shape.node_id)
            || self.authored_shapes.contains_key(&shape.node_id)
            || self.authored_lines.contains_key(&shape.node_id)
        {
            return Err(EditorError::CreateShapeIdCollision {
                node_id: shape.node_id,
            });
        }
        match validate_authored_shape_runtime_v1(shape) {
            Ok(()) => Ok(()),
            Err(CreateShapeRuntimeValidationError::NodeIdNotUuidV7) => {
                Err(EditorError::CreateShapeInvalidNodeId {
                    node_id: shape.node_id,
                })
            }
            Err(CreateShapeRuntimeValidationError::InvalidBounds) => {
                Err(EditorError::CreateShapeInvalidBounds {
                    node_id: shape.node_id,
                })
            }
            Err(CreateShapeRuntimeValidationError::InvalidPaint) => {
                Err(EditorError::CreateShapeInvalidPaint {
                    node_id: shape.node_id,
                })
            }
            Err(
                CreateShapeRuntimeValidationError::NonAuthorCreatedProvenance
                | CreateShapeRuntimeValidationError::NonAuthorCreatedPaintProvenance,
            ) => Err(EditorError::CreateShapeInvalidProvenance {
                node_id: shape.node_id,
            }),
            Err(
                CreateShapeRuntimeValidationError::ParentPageMismatch
                | CreateShapeRuntimeValidationError::UnsupportedShapeKind
                | CreateShapeRuntimeValidationError::UnsupportedTransform,
            ) => Err(EditorError::CreateShapeMalformed {
                node_id: shape.node_id,
            }),
        }
    }

    pub fn create_table(
        &mut self,
        table: CreateTableRuntimeV1,
    ) -> Result<EditOperation, EditorError> {
        self.consume_canonical_create_table(EditOperation::CreateTable { table })
    }

    fn consume_canonical_create_table(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let EditOperation::CreateTable { table } = &operation else {
            unreachable!("consume_canonical_create_table receives CreateTable");
        };

        if self.authored_shapes.contains_key(&table.node_id)
            || self.authored_lines.contains_key(&table.node_id)
        {
            return Err(EditorError::TableEditUnsupported {
                node_id: table.node_id,
            });
        }
        build_create_table_plan_v1(table).map_err(|_| EditorError::TableEditUnsupported {
            node_id: table.node_id,
        })?;

        let before_stack = self.current_authored_stack_v1(table.page_id);
        let transition = plan_create_table_append_v1(&before_stack, table.node_id, table.page_id)
            .map_err(|_| EditorError::StaleAuthoredStack {
            page_id: table.page_id,
        })?;
        let after_stack = apply_authored_stack_transition_forward_v1(&before_stack, &transition)
            .map_err(|_| EditorError::StaleAuthoredStack {
                page_id: table.page_id,
            })?;

        let mut candidate_graph = self.graph.clone();
        apply_create_table_forward_v1(&mut candidate_graph, table).map_err(|_| {
            EditorError::TableEditUnsupported {
                node_id: table.node_id,
            }
        })?;

        self.graph = candidate_graph;
        self.install_authored_stack_v1(after_stack);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn create_line(
        &mut self,
        node_id: NodeId,
        page_id: PageId,
        geometry: LineGeometryV1,
        stroke: AuthoredSolidStrokeV1,
    ) -> Result<EditOperation, EditorError> {
        let operation = EditOperation::CreateLine {
            node_id,
            page_id,
            parent_id: page_id,
            geometry,
            stroke,
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        self.consume_canonical_create_line(operation)
    }

    fn consume_canonical_create_line(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let line = authored_line_from_operation(&operation)
            .expect("consume_canonical_create_line receives CreateLine");
        self.validate_create_line_candidate(&line)?;

        let before_stack = self.current_authored_stack_v1(line.page_id);
        let transition = plan_create_line_append_v1(&before_stack, &line).map_err(|_| {
            EditorError::StaleAuthoredStack {
                page_id: line.page_id,
            }
        })?;
        let after_stack = apply_authored_stack_transition_forward_v1(&before_stack, &transition)
            .map_err(|_| EditorError::StaleAuthoredStack {
                page_id: line.page_id,
            })?;

        self.authored_lines.insert(line.node_id, line);
        self.install_authored_stack_v1(after_stack);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    fn validate_create_line_candidate(
        &self,
        line: &AuthoredLineRuntimeV1,
    ) -> Result<(), EditorError> {
        if !self.graph.pages.contains_key(&line.page_id) {
            return Err(EditorError::CreateLinePageMissing {
                page_id: line.page_id,
            });
        }
        if self.graph.nodes.contains_key(&line.node_id)
            || self.authored_shapes.contains_key(&line.node_id)
            || self.authored_lines.contains_key(&line.node_id)
        {
            return Err(EditorError::CreateLineIdCollision {
                node_id: line.node_id,
            });
        }
        match validate_authored_line_runtime_v1(line) {
            Ok(()) => Ok(()),
            Err(CreateLineRuntimeValidationError::NodeIdNotUuidV7) => {
                Err(EditorError::CreateLineInvalidNodeId {
                    node_id: line.node_id,
                })
            }
            Err(
                CreateLineRuntimeValidationError::CoordinateOutOfRange
                | CreateLineRuntimeValidationError::DerivedBoundsOverflow,
            ) => Err(EditorError::CreateLineInvalidGeometry {
                node_id: line.node_id,
            }),
            Err(CreateLineRuntimeValidationError::InvalidStroke) => {
                Err(EditorError::CreateLineInvalidStroke {
                    node_id: line.node_id,
                })
            }
            Err(CreateLineRuntimeValidationError::NonAuthorCreatedProvenance) => {
                Err(EditorError::CreateLineInvalidProvenance {
                    node_id: line.node_id,
                })
            }
            Err(CreateLineRuntimeValidationError::ParentPageMismatch) => {
                Err(EditorError::CreateLineMalformed {
                    node_id: line.node_id,
                })
            }
        }
    }

    /// Bounded DeleteNode V1 capability: only a current author-created,
    /// direct page-owned ordinary Rectangle in the authored overlay is admitted.
    pub fn can_delete_node(&self, node_id: NodeId) -> Result<(), EditorError> {
        self.validate_source_identity()?;
        if self.graph.nodes.contains_key(&node_id) {
            return Err(EditorError::NodeDeleteUnsupported { node_id });
        }
        let shape = self
            .authored_shapes
            .get(&node_id)
            .ok_or(EditorError::NodeDeleteUnsupported { node_id })?;
        if !self.graph.pages.contains_key(&shape.page_id)
            || shape.parent_id != shape.page_id
            || shape.shape_kind != AuthoredShapeKindV1::Rectangle
            || shape.provenance != AuthoredEntityProvenanceV1::AuthorCreated
            || shape.paint.provenance != AuthoredEntityProvenanceV1::AuthorCreated
            || validate_authored_shape_runtime_v1(shape).is_err()
        {
            return Err(EditorError::NodeDeleteUnsupported { node_id });
        }
        Ok(())
    }

    pub fn delete_node(&mut self, node_id: NodeId) -> Result<EditOperation, EditorError> {
        self.can_delete_node(node_id)?;
        let before = self
            .authored_shapes
            .get(&node_id)
            .expect("DeleteNode capability verified authored shape")
            .clone();
        let operation = EditOperation::DeleteNode {
            node_id,
            page_id: before.page_id,
            before_state_id: authored_shape_state_id_v1(&before),
            before,
        };
        self.consume_canonical_delete_node(operation)
    }

    fn consume_canonical_delete_node(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let EditOperation::DeleteNode {
            node_id,
            page_id,
            before,
            before_state_id,
        } = &operation
        else {
            unreachable!("consume_canonical_delete_node receives DeleteNode")
        };

        if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id
        {
            return Err(EditorError::NodeDeletePageMismatch {
                node_id: *node_id,
                page_id: *page_id,
            });
        }
        if authored_shape_state_id_v1(before) != *before_state_id {
            return Err(EditorError::StaleNodeDelete { node_id: *node_id });
        }
        self.can_delete_node(*node_id)?;

        let before_stack = self.current_authored_stack_v1(*page_id);
        let transition = plan_delete_shape_remove_v1(&before_stack, before)
            .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
        let after_stack = apply_authored_stack_transition_forward_v1(&before_stack, &transition)
            .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;

        let mut candidate_shapes = self.authored_shapes.clone();
        apply_authored_shape_delete_forward(&mut candidate_shapes, &operation)?;

        self.authored_shapes = candidate_shapes;
        self.install_authored_stack_v1(after_stack);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn reorder_authored_stack(
        &mut self,
        page_id: PageId,
        node_id: NodeId,
        mode: AuthoredStackReorderModeV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::AuthoredStackReorderUnsupported { node_id });
        }
        let shape = self
            .authored_shapes
            .get(&node_id)
            .ok_or(EditorError::AuthoredStackReorderUnsupported { node_id })?;
        if shape.page_id != page_id
            || shape.parent_id != page_id
            || validate_authored_shape_runtime_v1(shape).is_err()
        {
            return Err(EditorError::AuthoredStackReorderUnsupported { node_id });
        }

        let before = self.current_authored_stack_v1(page_id);
        let transition = plan_reorder_authored_stack_v1(&before, node_id, mode).map_err(
            |error| match error {
                AuthoredStackReorderErrorV1::NoChange { .. } => {
                    EditorError::AuthoredStackReorderNoChange { node_id }
                }
                AuthoredStackReorderErrorV1::MissingMember { .. }
                | AuthoredStackReorderErrorV1::PageMismatch
                | AuthoredStackReorderErrorV1::InvalidStack => {
                    EditorError::AuthoredStackReorderUnsupported { node_id }
                }
                AuthoredStackReorderErrorV1::BeforeStateMismatch
                | AuthoredStackReorderErrorV1::AfterStateMismatch
                | AuthoredStackReorderErrorV1::TransitionMismatch => {
                    EditorError::StaleAuthoredStack { page_id }
                }
            },
        )?;
        self.consume_canonical_reorder_authored_stack(EditOperation::ReorderAuthoredStack {
            transition,
        })
    }

    fn consume_canonical_reorder_authored_stack(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let EditOperation::ReorderAuthoredStack { transition } = &operation else {
            unreachable!("consume_canonical_reorder_authored_stack receives ReorderAuthoredStack")
        };
        let shape = self.authored_shapes.get(&transition.node_id).ok_or(
            EditorError::AuthoredStackReorderUnsupported {
                node_id: transition.node_id,
            },
        )?;
        if shape.page_id != transition.page_id || shape.parent_id != transition.page_id {
            return Err(EditorError::AuthoredStackReorderUnsupported {
                node_id: transition.node_id,
            });
        }
        let current = self.current_authored_stack_v1(transition.page_id);
        let after =
            apply_authored_stack_reorder_forward_v1(&current, transition).map_err(|_| {
                EditorError::StaleAuthoredStack {
                    page_id: transition.page_id,
                }
            })?;
        self.install_authored_stack_v1(after);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn can_break_text_frame_forward_link(
        &self,
        upstream_frame_id: NodeId,
        downstream_frame_id: NodeId,
        new_story_id: StoryId,
    ) -> Result<(), EditorError> {
        self.validate_source_identity()?;

        if !is_editor_created_uuid_v7_story_id(new_story_id) {
            return Err(EditorError::NewStoryIdInvalid {
                story_id: new_story_id,
            });
        }
        if self.graph.stories.contains_key(&new_story_id) {
            return Err(EditorError::NewStoryIdConflict {
                story_id: new_story_id,
            });
        }

        let chain =
            explicit_story_chain_for_break(&self.graph, upstream_frame_id, downstream_frame_id)?;
        let source_story_id = chain
            .first()
            .expect("explicit chain must be non-empty")
            .story_id;
        if self.story_has_paragraph_alignment_history_v1(source_story_id)? {
            return Err(EditorError::ParagraphAlignmentLifecycleUnsupported {
                story_id: source_story_id,
            });
        }
        if source_story_id == new_story_id {
            return Err(EditorError::NewStoryIdConflict {
                story_id: new_story_id,
            });
        }
        Ok(())
    }

    pub fn break_text_frame_forward_link(
        &mut self,
        upstream_frame_id: NodeId,
        downstream_frame_id: NodeId,
        new_story_id: StoryId,
    ) -> Result<EditOperation, EditorError> {
        self.can_break_text_frame_forward_link(
            upstream_frame_id,
            downstream_frame_id,
            new_story_id,
        )?;

        let before_frames =
            explicit_story_chain_for_break(&self.graph, upstream_frame_id, downstream_frame_id)?;
        let story_id = before_frames
            .first()
            .expect("capability check verified non-empty chain")
            .story_id;
        let break_index = before_frames
            .iter()
            .position(|frame| {
                frame.frame_id == upstream_frame_id && frame.next == Some(downstream_frame_id)
            })
            .expect("capability check verified explicit break edge");

        let mut after_frames = before_frames.clone();
        after_frames[break_index].next = None;
        for (index, frame) in after_frames.iter_mut().enumerate().skip(break_index + 1) {
            frame.story_id = new_story_id;
            if index == break_index + 1 {
                frame.previous = None;
            }
        }

        let upstream_after = after_frames
            .iter()
            .filter(|frame| frame.story_id == story_id)
            .cloned()
            .collect::<Vec<_>>();
        let downstream_after = after_frames
            .iter()
            .filter(|frame| frame.story_id == new_story_id)
            .cloned()
            .collect::<Vec<_>>();
        if !validate_story_frames(&upstream_after).is_empty()
            || !validate_story_frames(&downstream_after).is_empty()
            || downstream_after.is_empty()
        {
            return Err(EditorError::BreakLinkUnsupported {
                upstream_frame_id,
                downstream_frame_id,
            });
        }

        let operation = EditOperation::BreakTextFrameForwardLink {
            story_id,
            upstream_frame_id,
            downstream_frame_id,
            new_story_id,
            before_frames,
            after_frames,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn undo(&mut self) -> Result<&EditOperation, EditorError> {
        let operation = self.undo.pop().ok_or(EditorError::NothingToUndo)?;
        let result = (|| {
            if authored_stack_operation_page_id_v1(&operation).is_some() {
                let before_stacks = derive_authored_stacks_from_operations_v1(&self.undo)?;
                let mut expected_after_stacks = before_stacks.clone();
                apply_authored_stack_history_forward_v1(&mut expected_after_stacks, &operation)?;
                if expected_after_stacks != self.authored_stacks {
                    return Err(EditorError::StaleAuthoredStack {
                        page_id: authored_stack_operation_page_id_v1(&operation)
                            .expect("lane operation has page"),
                    });
                }

                let mut candidate_shapes = self.authored_shapes.clone();
                let mut candidate_lines = self.authored_lines.clone();
                let mut candidate_graph = self.graph.clone();
                match &operation {
                    EditOperation::CreateShape { .. } => {
                        apply_authored_shape_inverse(&mut candidate_shapes, &operation)?;
                    }
                    EditOperation::CreateLine { .. } => {
                        apply_authored_line_inverse(&mut candidate_lines, &operation)?;
                    }
                    EditOperation::CreateTable { table } => {
                        apply_create_table_inverse_v1(&mut candidate_graph, table).map_err(
                            |_| EditorError::TableEditUnsupported {
                                node_id: table.node_id,
                            },
                        )?;
                    }
                    EditOperation::DeleteNode { .. } => {
                        apply_authored_shape_delete_inverse(&mut candidate_shapes, &operation)?;
                    }
                    EditOperation::ReorderAuthoredStack { .. } => {}
                    _ => unreachable!("authored-stack page helper only admits lane operations"),
                }
                self.authored_shapes = candidate_shapes;
                self.authored_lines = candidate_lines;
                self.graph = candidate_graph;
                self.authored_stacks = before_stacks;
            } else if let Some(history) = table_rowcol_history_v1(&operation) {
                if !table_rowcol_operation_matches_mutation_v1(&operation) {
                    return Err(EditorError::StaleTableRowCol {
                        node_id: history.table_id,
                    });
                }
                let current = table_structure_snapshot_from_graph_v1(
                    &self.graph,
                    history.table_id,
                    history.after.grid.clone(),
                    history.after.bounds,
                )
                .map_err(|_| EditorError::StaleTableRowCol {
                    node_id: history.table_id,
                })?;
                let before =
                    apply_table_rowcol_history_inverse_v1(&current, history).map_err(|_| {
                        EditorError::StaleTableRowCol {
                            node_id: history.table_id,
                        }
                    })?;
                let mut candidate_graph = self.graph.clone();
                apply_table_structure_snapshot_to_graph_v1(&mut candidate_graph, &before).map_err(
                    |_| EditorError::StaleTableRowCol {
                        node_id: history.table_id,
                    },
                )?;
                self.graph = candidate_graph;
            } else if let EditOperation::SetTableTrackExtent { history } = &operation {
                let grid = effective_table_grids_with_history(&self.graph, &self.undo)
                    .into_iter()
                    .find(|grid| grid.table_id == history.table_id)
                    .ok_or(EditorError::StaleTableTrackResize {
                        node_id: history.table_id,
                    })?;
                let bounds =
                    effective_table_bounds_with_history(&self.graph, &self.undo, history.table_id)
                        .ok_or(EditorError::StaleTableTrackResize {
                            node_id: history.table_id,
                        })?;
                apply_table_track_extent_history_forward_v1(&grid, bounds, history).map_err(
                    |_| EditorError::StaleTableTrackResize {
                        node_id: history.table_id,
                    },
                )?;
            } else if matches!(operation, EditOperation::SetImageCrop { .. }) {
                apply_crop_inverse(&self.graph, &mut self.image_crop_overrides, &operation)?;
            } else if matches!(operation, EditOperation::ReplaceImage { .. }) {
                apply_image_inverse(&mut self.image_replacements, &operation)?;
            } else if let Some(story_id) = text_format_operation_story_id_v1(&operation) {
                if is_scoped_text_format_operation_v1(&operation) {
                    let property = text_format_operation_property_v1(&operation)
                        .expect("scoped text-format operation carries property");
                    let before_state =
                        self.current_text_format_property_state_v1(story_id, property)?;
                    let _after_state =
                        apply_text_format_property_operation_checked_v1(&before_state, &operation)
                            .map_err(|message| EditorError::TextFormatStateInvalid {
                                story_id,
                                message,
                            })?;
                } else {
                    let before_state = self.current_text_format_overlay_v1(story_id)?;
                    let _after_state =
                        apply_text_format_history_operation_v1(&before_state, &operation)?;
                }
            } else if authored_paragraph_alignment_v1::paragraph_alignment_operation_snapshots_v1(
                &operation,
            )
            .is_some()
            {
                authored_paragraph_alignment_v1::validate_paragraph_alignment_operation_against_history_v1(
                    &self.undo,
                    &operation,
                )
                .map_err(paragraph_alignment_transition_error_to_editor_v1)?;
            } else {
                apply_inverse(&mut self.graph, &operation)?;
            }
            self.validate_source_identity()
        })();

        if let Err(error) = result {
            self.undo.push(operation);
            return Err(error);
        }
        self.redo.push(operation);
        Ok(self.redo.last().expect("just pushed undo operation"))
    }

    pub fn redo(&mut self) -> Result<&EditOperation, EditorError> {
        let operation = self.redo.pop().ok_or(EditorError::NothingToRedo)?;
        let result = (|| {
            if authored_stack_operation_page_id_v1(&operation).is_some() {
                let expected_before_stacks = derive_authored_stacks_from_operations_v1(&self.undo)?;
                if expected_before_stacks != self.authored_stacks {
                    return Err(EditorError::StaleAuthoredStack {
                        page_id: authored_stack_operation_page_id_v1(&operation)
                            .expect("lane operation has page"),
                    });
                }
                let mut after_stacks = expected_before_stacks.clone();
                apply_authored_stack_history_forward_v1(&mut after_stacks, &operation)?;

                let mut candidate_shapes = self.authored_shapes.clone();
                let mut candidate_lines = self.authored_lines.clone();
                let mut candidate_graph = self.graph.clone();
                match &operation {
                    EditOperation::CreateShape { .. } => {
                        let shape = authored_shape_from_operation(&operation)
                            .expect("CreateShape operation reconstructs authored shape");
                        self.validate_create_shape_candidate(&shape)?;
                        candidate_shapes.insert(shape.node_id, shape);
                    }
                    EditOperation::CreateLine { .. } => {
                        let line = authored_line_from_operation(&operation)
                            .expect("CreateLine operation reconstructs authored line");
                        self.validate_create_line_candidate(&line)?;
                        candidate_lines.insert(line.node_id, line);
                    }
                    EditOperation::CreateTable { table } => {
                        apply_create_table_forward_v1(&mut candidate_graph, table).map_err(
                            |_| EditorError::TableEditUnsupported {
                                node_id: table.node_id,
                            },
                        )?;
                    }
                    EditOperation::DeleteNode { .. } => {
                        apply_authored_shape_delete_forward(&mut candidate_shapes, &operation)?;
                    }
                    EditOperation::ReorderAuthoredStack { .. } => {}
                    _ => unreachable!("authored-stack page helper only admits lane operations"),
                }

                self.authored_shapes = candidate_shapes;
                self.authored_lines = candidate_lines;
                self.graph = candidate_graph;
                self.authored_stacks = after_stacks;
            } else if let Some(history) = table_rowcol_history_v1(&operation) {
                if !table_rowcol_operation_matches_mutation_v1(&operation) {
                    return Err(EditorError::StaleTableRowCol {
                        node_id: history.table_id,
                    });
                }
                let current = table_structure_snapshot_from_graph_v1(
                    &self.graph,
                    history.table_id,
                    history.before.grid.clone(),
                    history.before.bounds,
                )
                .map_err(|_| EditorError::StaleTableRowCol {
                    node_id: history.table_id,
                })?;
                let after =
                    apply_table_rowcol_history_forward_v1(&current, history).map_err(|_| {
                        EditorError::StaleTableRowCol {
                            node_id: history.table_id,
                        }
                    })?;
                let mut candidate_graph = self.graph.clone();
                apply_table_structure_snapshot_to_graph_v1(&mut candidate_graph, &after).map_err(
                    |_| EditorError::StaleTableRowCol {
                        node_id: history.table_id,
                    },
                )?;
                self.graph = candidate_graph;
            } else if let EditOperation::SetTableTrackExtent { history } = &operation {
                let grid = effective_table_grids_with_history(&self.graph, &self.undo)
                    .into_iter()
                    .find(|grid| grid.table_id == history.table_id)
                    .ok_or(EditorError::StaleTableTrackResize {
                        node_id: history.table_id,
                    })?;
                let bounds =
                    effective_table_bounds_with_history(&self.graph, &self.undo, history.table_id)
                        .ok_or(EditorError::StaleTableTrackResize {
                            node_id: history.table_id,
                        })?;
                apply_table_track_extent_history_forward_v1(&grid, bounds, history).map_err(
                    |_| EditorError::StaleTableTrackResize {
                        node_id: history.table_id,
                    },
                )?;
            } else if matches!(operation, EditOperation::SetImageCrop { .. }) {
                apply_crop_forward(&self.graph, &mut self.image_crop_overrides, &operation)?;
            } else if matches!(operation, EditOperation::ReplaceImage { .. }) {
                apply_image_forward(&mut self.image_replacements, &operation)?;
            } else if let Some(story_id) = text_format_operation_story_id_v1(&operation) {
                if is_scoped_text_format_operation_v1(&operation) {
                    let property = text_format_operation_property_v1(&operation)
                        .expect("scoped text-format operation carries property");
                    let before_state =
                        self.current_text_format_property_state_v1(story_id, property)?;
                    let _after_state =
                        apply_text_format_property_operation_checked_v1(&before_state, &operation)
                            .map_err(|message| EditorError::TextFormatStateInvalid {
                                story_id,
                                message,
                            })?;
                } else {
                    let before_state = self.current_text_format_overlay_v1(story_id)?;
                    let _after_state =
                        apply_text_format_history_operation_v1(&before_state, &operation)?;
                }
            } else if authored_paragraph_alignment_v1::paragraph_alignment_operation_snapshots_v1(
                &operation,
            )
            .is_some()
            {
                authored_paragraph_alignment_v1::validate_paragraph_alignment_operation_against_history_v1(
                    &self.undo,
                    &operation,
                )
                .map_err(paragraph_alignment_transition_error_to_editor_v1)?;
            } else {
                apply_forward(&mut self.graph, &operation)?;
            }
            self.validate_source_identity()
        })();

        if let Err(error) = result {
            self.redo.push(operation);
            return Err(error);
        }
        self.undo.push(operation);
        Ok(self.undo.last().expect("just pushed redo operation"))
    }

    fn validate_source_identity(&self) -> Result<(), EditorError> {
        if self.graph.source.source_hash != self.source_hash
            || self.graph.document.source_hash != self.source_hash
        {
            return Err(EditorError::SourceIdentityChanged);
        }
        Ok(())
    }
}

fn editor_project_asset_metadata(asset: &EditorReplacementAsset) -> EditorProjectAsset {
    EditorProjectAsset {
        sha256: asset.sha256,
        mime: asset.mime.clone(),
        byte_len: u64::try_from(asset.bytes.len())
            .expect("validated editor asset length must fit u64"),
    }
}

fn canonical_editor_asset_metadata(
    assets: &BTreeMap<Sha256Digest, EditorReplacementAsset>,
) -> Vec<EditorProjectAsset> {
    assets.values().map(editor_project_asset_metadata).collect()
}

pub fn editor_asset_file_name(
    sha256: Sha256Digest,
    mime: &str,
) -> Result<String, EditorAssetError> {
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        other => {
            return Err(EditorAssetError::UnsupportedMime {
                mime: other.to_owned(),
            });
        }
    };
    Ok(format!("asset-{sha256}.{extension}"))
}

fn validated_editor_asset(
    mime: String,
    bytes: Vec<u8>,
) -> Result<EditorReplacementAsset, EditorAssetError> {
    if bytes.is_empty() {
        return Err(EditorAssetError::EmptyBytes);
    }
    u64::try_from(bytes.len()).map_err(|_| EditorAssetError::ByteLengthOverflow)?;

    let signature_matches = match mime.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        other => {
            return Err(EditorAssetError::UnsupportedMime {
                mime: other.to_owned(),
            });
        }
    };
    if !signature_matches {
        return Err(EditorAssetError::SignatureMismatch { mime });
    }

    let digest = Sha256::digest(&bytes);
    let mut digest_bytes = [0_u8; 32];
    digest_bytes.copy_from_slice(&digest);
    Ok(EditorReplacementAsset {
        sha256: Sha256Digest::from_bytes(digest_bytes),
        mime,
        bytes,
    })
}

fn replay_canonical_operation(
    session: &mut EditorSession,
    expected: &EditOperation,
    index: usize,
) -> Result<EditOperation, EditorProjectError> {
    match expected {
        EditOperation::LinkTextFrameTail { transition } => session
            .link_text_frame_tail(transition.source_frame_id, transition.target_frame_id)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ReplaceStoryRange {
            story_id,
            start_scalar,
            end_scalar,
            expected_before,
            replacement_text,
            ..
        } => session
            .replace_story_range(
                *story_id,
                *start_scalar,
                *end_scalar,
                expected_before.clone(),
                replacement_text.clone(),
            )
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ReplaceStoryText {
            story_id, after, ..
        } => session
            .replace_story_text(*story_id, after.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::BreakTextFrameForwardLink {
            upstream_frame_id,
            downstream_frame_id,
            new_story_id,
            ..
        } => session
            .break_text_frame_forward_link(*upstream_frame_id, *downstream_frame_id, *new_story_id)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ReplaceTableCellText {
            node_id,
            story_id,
            cell_id,
            ..
        } => {
            // Materialize only a temporary expected-after graph to recover the
            // target cell's intended text. The real candidate session is then
            // mutated through the public capability-gated table API, and the
            // generated canonical operation must exactly match the persisted one.
            let mut expected_after_graph = session.graph.clone();
            apply_forward(&mut expected_after_graph, expected)
                .map_err(|error| EditorProjectError::Operation { index, error })?;

            let replacement = expected_after_graph
                .nodes
                .get(node_id)
                .and_then(|node| node.payload.table.as_ref())
                .filter(|table| table.story_id == Some(*story_id))
                .and_then(|table| {
                    let story = expected_after_graph.stories.get(story_id)?;
                    materialize_bounded_simple_table_cells(table, story)
                        .ok()?
                        .into_iter()
                        .find(|cell| cell.id == *cell_id)
                        .map(|cell| cell.text)
                })
                .ok_or(EditorProjectError::InvalidTableAfterState {
                    index,
                    node_id: *node_id,
                    cell_id: *cell_id,
                })?;

            session
                .replace_table_cell_text(*node_id, *cell_id, replacement)
                .map_err(|error| EditorProjectError::Operation { index, error })
        }
        EditOperation::ReplaceImage {
            node_id,
            after_asset,
            ..
        } => session
            .replace_image(*node_id, *after_asset)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::SetImageCrop {
            node_id,
            before,
            after,
        } => session
            .set_image_crop(*node_id, *before, *after)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::MoveNode { node_id, after, .. } => session
            .move_node_to(*node_id, after.x, after.y)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::MoveNodes { page_id, entries } => session
            .consume_canonical_move_nodes(*page_id, entries.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ResizeNode { node_id, after, .. } => session
            .resize_node_to(*node_id, *after)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ResizeNodes { page_id, entries } => session
            .consume_canonical_resize_nodes(*page_id, entries.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            text_preset,
        } => session
            .create_text_box(*node_id, *story_id, *page_id, *bounds, text_preset.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::CreateShape { .. } => session
            .consume_canonical_create_shape(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::CreateLine { .. } => session
            .consume_canonical_create_line(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::CreateTable { .. } => session
            .consume_canonical_create_table(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::SetTableTrackExtent { history } => session
            .set_table_track_extent_v1(history.table_id, history.target, history.after_extent)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::InsertTableRow { .. }
        | EditOperation::DeleteTableRow { .. }
        | EditOperation::InsertTableColumn { .. }
        | EditOperation::DeleteTableColumn { .. } => session
            .consume_canonical_table_rowcol_operation_v1(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::DeleteNode { .. } => session
            .consume_canonical_delete_node(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ReorderAuthoredStack { .. } => session
            .consume_canonical_reorder_authored_stack(expected.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            ..
        } => session
            .set_text_format_property_v1(
                *story_id,
                *start_scalar,
                *end_scalar,
                *property,
                value.clone(),
                before_state_hash,
            )
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            ..
        } => session
            .clear_text_format_property_override_v1(
                *story_id,
                *start_scalar,
                *end_scalar,
                *property,
                before_state_hash,
            )
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            ..
        } => session
            .set_text_format_property_scoped_v1(
                *story_id,
                *start_scalar,
                *end_scalar,
                *property,
                value.clone(),
                before_state_hash,
            )
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            ..
        } => session
            .clear_text_format_property_override_scoped_v1(
                *story_id,
                *start_scalar,
                *end_scalar,
                *property,
                before_state_hash,
            )
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids,
            value,
            ..
        } => session
            .set_paragraph_alignment_override_v1(paragraph_ids.clone(), *value)
            .map_err(|error| EditorProjectError::Operation { index, error }),
        EditOperation::ClearParagraphAlignmentOverride { paragraph_ids, .. } => session
            .clear_paragraph_alignment_override_v1(paragraph_ids.clone())
            .map_err(|error| EditorProjectError::Operation { index, error }),
    }
}

fn is_editor_created_uuid_v7_story_id(story_id: StoryId) -> bool {
    let bytes = story_id.as_canonical().as_bytes();
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

fn is_editor_created_uuid_v7_node_id(node_id: NodeId) -> bool {
    let bytes = node_id.as_canonical().as_bytes();
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

fn validate_authoring_text_preset_v1(preset: &AuthoringTextPresetV1) -> bool {
    !preset.resource_id.trim().is_empty()
        && preset.font_fingerprint_sha256.len() == 64
        && preset
            .font_fingerprint_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && preset.font_size_emu.get() > 0
        && preset.line_height_emu.get() > 0
}

fn validate_create_text_box_candidate(
    graph: &PubResolvedGraph,
    node_id: NodeId,
    story_id: StoryId,
    page_id: PageId,
    bounds: RectEmu,
    text_preset: &AuthoringTextPresetV1,
) -> Result<(), EditorError> {
    if !is_editor_created_uuid_v7_node_id(node_id) {
        return Err(EditorError::CreateTextBoxInvalidNodeId { node_id });
    }
    if !is_editor_created_uuid_v7_story_id(story_id) {
        return Err(EditorError::CreateTextBoxInvalidStoryId { story_id });
    }
    if !graph.pages.contains_key(&page_id) {
        return Err(EditorError::CreateTextBoxPageMissing { page_id });
    }
    if graph.nodes.contains_key(&node_id) {
        return Err(EditorError::CreateTextBoxNodeIdCollision { node_id });
    }
    if graph.stories.contains_key(&story_id) {
        return Err(EditorError::CreateTextBoxStoryIdCollision { story_id });
    }
    if bounds.width.get() <= 0
        || bounds.height.get() <= 0
        || bounds.right().is_none()
        || bounds.bottom().is_none()
    {
        return Err(EditorError::CreateTextBoxInvalidBounds { node_id });
    }
    if !validate_authoring_text_preset_v1(text_preset) {
        return Err(EditorError::CreateTextBoxInvalidTextPreset);
    }
    Ok(())
}

fn authored_text_box_node_v1(
    node_id: NodeId,
    story_id: StoryId,
    page_id: PageId,
    bounds: RectEmu,
) -> Node<PubResolvedNodePayload> {
    Node {
        kind: NodeKind::TextFrame,
        header: NodeHeader {
            id: node_id,
            parent_id: page_id.into_canonical(),
            bounds,
            transform: Affine2D::identity(),
            source_refs: Vec::new(),
            extensions: Vec::new(),
        },
        payload: PubResolvedNodePayload {
            // Source sequence identity does not exist for Chaptera-created nodes.
            // Zero is a bounded synthetic sentinel; source_refs remain empty.
            contents_seq_num: 0,
            officeart_shape_type: None,
            officeart_spid: None,
            image_slot: None,
            legacy_ole: None,
            explicit_image_crop: None,
            explicit_image_cardinal_rotation_degrees: None,
            explicit_paint: Default::default(),
            effective_paint: None,
            story_frame: Some(PubResolvedStoryFrame {
                story_id: Some(story_id),
                ordinal: 0,
                previous_frame: None,
                next_frame: None,
                vertical_alignment: None,
            }),
            text_frame_inset: None,
            table_story: None,
            table: None,
        },
    }
}

fn frame_snapshot(
    graph: &PubResolvedGraph,
    node_id: NodeId,
) -> Option<StoryFrame<StoryId, NodeId>> {
    let node = graph.nodes.get(&node_id)?;
    frame_from_payload(node_id, &node.payload)
}

fn explicit_story_chain_for_break(
    graph: &PubResolvedGraph,
    upstream_frame_id: NodeId,
    downstream_frame_id: NodeId,
) -> Result<Vec<StoryFrame<StoryId, NodeId>>, EditorError> {
    let upstream =
        frame_snapshot(graph, upstream_frame_id).ok_or(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        })?;
    let downstream =
        frame_snapshot(graph, downstream_frame_id).ok_or(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        })?;

    if upstream.story_id != downstream.story_id
        || upstream.next != Some(downstream_frame_id)
        || downstream.previous != Some(upstream_frame_id)
        || !graph.stories.contains_key(&upstream.story_id)
        || graph.nodes.values().any(|node| {
            node.payload
                .table_story
                .as_ref()
                .is_some_and(|owner| owner.story_id == Some(upstream.story_id))
                || node
                    .payload
                    .table
                    .as_ref()
                    .is_some_and(|table| table.story_id == Some(upstream.story_id))
        })
    {
        return Err(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        });
    }

    let frames = graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| {
            let frame = frame_from_payload(*node_id, &node.payload)?;
            (frame.story_id == upstream.story_id).then_some(frame)
        })
        .collect::<Vec<_>>();
    if frames.len() < 2 || !validate_story_frames(&frames).is_empty() {
        return Err(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        });
    }

    let heads = frames
        .iter()
        .filter(|frame| frame.previous.is_none())
        .map(|frame| frame.frame_id)
        .collect::<Vec<_>>();
    if heads.len() != 1 {
        return Err(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        });
    }

    let by_id = frames
        .iter()
        .map(|frame| (frame.frame_id, frame.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = Vec::with_capacity(frames.len());
    let mut seen = BTreeSet::new();
    let mut cursor = Some(heads[0]);

    while let Some(frame_id) = cursor {
        if !seen.insert(frame_id) {
            return Err(EditorError::BreakLinkUnsupported {
                upstream_frame_id,
                downstream_frame_id,
            });
        }
        let frame = by_id
            .get(&frame_id)
            .ok_or(EditorError::BreakLinkUnsupported {
                upstream_frame_id,
                downstream_frame_id,
            })?;
        ordered.push(frame.clone());
        cursor = frame.next;
    }

    if ordered.len() != frames.len()
        || !ordered.iter().any(|frame| {
            frame.frame_id == upstream_frame_id && frame.next == Some(downstream_frame_id)
        })
    {
        return Err(EditorError::BreakLinkUnsupported {
            upstream_frame_id,
            downstream_frame_id,
        });
    }

    Ok(ordered)
}

fn set_story_frame_snapshot(
    graph: &mut PubResolvedGraph,
    snapshot: &StoryFrame<StoryId, NodeId>,
) -> Result<(), EditorError> {
    let node = graph
        .nodes
        .get_mut(&snapshot.frame_id)
        .ok_or(EditorError::StaleFrameTopology {
            story_id: snapshot.story_id,
        })?;
    let frame = node
        .payload
        .story_frame
        .as_mut()
        .ok_or(EditorError::StaleFrameTopology {
            story_id: snapshot.story_id,
        })?;
    frame.story_id = Some(snapshot.story_id);
    frame.ordinal = snapshot.ordinal;
    frame.previous_frame = snapshot.previous;
    frame.next_frame = snapshot.next;
    Ok(())
}

fn frames_match_snapshots(
    graph: &PubResolvedGraph,
    snapshots: &[StoryFrame<StoryId, NodeId>],
) -> bool {
    snapshots
        .iter()
        .all(|expected| frame_snapshot(graph, expected.frame_id).as_ref() == Some(expected))
}

fn empty_editor_story(story_id: StoryId) -> Story {
    Story {
        id: story_id,
        text: String::new(),
        paragraphs: Vec::new(),
        runs: Vec::new(),
        fields: Vec::new(),
        hyperlinks: Vec::new(),
        source_refs: Vec::new(),
    }
}
fn effective_table_grids(graph: &PubResolvedGraph) -> Vec<EffectiveTableGridV1> {
    let mut grids = Vec::new();

    for (table_id, node) in &graph.nodes {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        let Some(simple) = table.simple_table.as_ref() else {
            continue;
        };

        let row_extent = table
            .layout_metrics
            .as_ref()
            .map(|metrics| metrics.row_pitch);
        let column_extent = table
            .layout_metrics
            .as_ref()
            .map(|metrics| metrics.cell_width);

        let rows = (0..simple.rows)
            .map(|index| EffectiveTableTrackV1 {
                id: TableRowId::from_canonical(
                    derive_source_canonical_id(SourceDerivedIdInput {
                        source_hash: &graph.source.source_hash,
                        adapter_id: "pub-rs",
                        source_object_key: &format!(
                            "table/{}/row/{index}",
                            table_id.as_canonical()
                        ),
                        semantic_role: "cdm.table_row",
                    })
                    .expect("fixed effective table row identity contract"),
                ),
                index,
                extent: row_extent,
            })
            .collect::<Vec<_>>();

        let columns = (0..simple.columns)
            .map(|index| EffectiveTableTrackV1 {
                id: TableColumnId::from_canonical(
                    derive_source_canonical_id(SourceDerivedIdInput {
                        source_hash: &graph.source.source_hash,
                        adapter_id: "pub-rs",
                        source_object_key: &format!(
                            "table/{}/column/{index}",
                            table_id.as_canonical()
                        ),
                        semantic_role: "cdm.table_column",
                    })
                    .expect("fixed effective table column identity contract"),
                ),
                index,
                extent: column_extent,
            })
            .collect::<Vec<_>>();

        let mut cells = simple
            .cells
            .iter()
            .map(|cell| {
                let source = table.cells.iter().find(|source| source.id == cell.id);
                EffectiveTableCellV1 {
                    id: cell.id,
                    row_id: rows[usize::try_from(cell.address.row).expect("row u32 fits usize")].id,
                    column_id: columns
                        [usize::try_from(cell.address.column).expect("column u32 fits usize")]
                    .id,
                    address: cell.address,
                    row_span: 1,
                    column_span: 1,
                    story_id: table.story_id,
                    utf16_start: table
                        .story_id
                        .and_then(|_| source.map(|source| source.utf16_start)),
                    utf16_end: table
                        .story_id
                        .and_then(|_| source.map(|source| source.utf16_end)),
                }
            })
            .collect::<Vec<_>>();
        cells.sort_by_key(|cell| (cell.address.row, cell.address.column, cell.id));

        let grid = EffectiveTableGridV1 {
            version: EFFECTIVE_TABLE_GRID_V1.into(),
            table_id: *table_id,
            rows,
            columns,
            cells,
        };
        grid.validate()
            .expect("grounded simple table must produce valid EffectiveTableGridV1");
        grids.push(grid);
    }

    grids.sort_by_key(|grid| grid.table_id);
    grids
}

fn table_rowcol_history_v1(operation: &EditOperation) -> Option<&TableRowColHistoryV1> {
    match operation {
        EditOperation::InsertTableRow { history }
        | EditOperation::DeleteTableRow { history }
        | EditOperation::InsertTableColumn { history }
        | EditOperation::DeleteTableColumn { history } => Some(history),
        _ => None,
    }
}

fn table_rowcol_operation_matches_mutation_v1(operation: &EditOperation) -> bool {
    matches!(
        operation,
        EditOperation::InsertTableRow {
            history: TableRowColHistoryV1 {
                mutation: TableRowColMutationV1::InsertRow { .. },
                ..
            },
        } | EditOperation::DeleteTableRow {
            history: TableRowColHistoryV1 {
                mutation: TableRowColMutationV1::DeleteRow { .. },
                ..
            },
        } | EditOperation::InsertTableColumn {
            history: TableRowColHistoryV1 {
                mutation: TableRowColMutationV1::InsertColumn { .. },
                ..
            },
        } | EditOperation::DeleteTableColumn {
            history: TableRowColHistoryV1 {
                mutation: TableRowColMutationV1::DeleteColumn { .. },
                ..
            },
        }
    )
}

fn effective_table_grids_with_history(
    graph: &PubResolvedGraph,
    operations: &[EditOperation],
) -> Vec<EffectiveTableGridV1> {
    let mut grids = effective_table_grids(graph);
    let mut bounds = graph
        .nodes
        .iter()
        .map(|(node_id, node)| (*node_id, node.header.bounds))
        .collect::<BTreeMap<_, _>>();

    let mut structural_anchors = BTreeMap::<NodeId, (usize, &TableRowColHistoryV1)>::new();
    for (index, operation) in operations.iter().enumerate() {
        if let Some(history) = table_rowcol_history_v1(operation) {
            structural_anchors.insert(history.table_id, (index, history));
        }
    }
    for (table_id, (_, history)) in &structural_anchors {
        if let Some(target) = grids.iter_mut().find(|grid| grid.table_id == *table_id) {
            *target = history.after.grid.clone();
        } else {
            grids.push(history.after.grid.clone());
        }
        bounds.insert(*table_id, history.after.bounds);
    }

    for (index, operation) in operations.iter().enumerate() {
        match operation {
            EditOperation::CreateTable { table } => {
                if structural_anchors
                    .get(&table.node_id)
                    .is_some_and(|(anchor, _)| index <= *anchor)
                {
                    continue;
                }
                let plan = build_create_table_plan_v1(table)
                    .expect("accepted CreateTable history must remain canonical");
                let current = grids
                    .iter()
                    .find(|grid| grid.table_id == table.node_id)
                    .cloned()
                    .expect("accepted CreateTable must materialize one effective table grid");
                let target = grids
                    .iter_mut()
                    .find(|grid| grid.table_id == table.node_id)
                    .expect("accepted CreateTable grid is present");

                *target = plan.grid;
                for cell in &mut target.cells {
                    let current_cell = current
                        .cells
                        .iter()
                        .find(|candidate| candidate.id == cell.id)
                        .expect("created table cell identity remains stable");
                    cell.utf16_start = current_cell.utf16_start;
                    cell.utf16_end = current_cell.utf16_end;
                }
                bounds.insert(table.node_id, table.bounds);
            }
            EditOperation::SetTableTrackExtent { history } => {
                if structural_anchors
                    .get(&history.table_id)
                    .is_some_and(|(anchor, _)| index <= *anchor)
                {
                    continue;
                }
                let target = grids
                    .iter_mut()
                    .find(|grid| grid.table_id == history.table_id)
                    .expect("accepted track-resize history must target one effective table grid");
                let before_bounds = *bounds
                    .get(&history.table_id)
                    .expect("accepted track-resize history must target one table bounds record");
                let (after_grid, after_bounds) =
                    apply_table_track_extent_history_forward_v1(target, before_bounds, history)
                        .expect("accepted track-resize history must remain canonical");
                *target = after_grid;
                bounds.insert(history.table_id, after_bounds);
            }
            _ => {}
        }
    }

    grids.sort_by_key(|grid| grid.table_id);
    grids
}

fn effective_table_bounds_with_history(
    graph: &PubResolvedGraph,
    operations: &[EditOperation],
    table_id: NodeId,
) -> Option<RectEmu> {
    let mut anchor = None;
    for (index, operation) in operations.iter().enumerate() {
        let Some(history) = table_rowcol_history_v1(operation) else {
            continue;
        };
        if history.table_id == table_id {
            anchor = Some((index, history.after.bounds));
        }
    }

    let (start_index, mut bounds) = match anchor {
        Some((index, bounds)) => (index + 1, bounds),
        None => (0, graph.nodes.get(&table_id)?.header.bounds),
    };
    for operation in operations.iter().skip(start_index) {
        let EditOperation::SetTableTrackExtent { history } = operation else {
            continue;
        };
        if history.table_id == table_id {
            if history.before_bounds != bounds {
                return None;
            }
            bounds = history.after_bounds;
        }
    }
    Some(bounds)
}

fn frame_from_payload(
    node_id: NodeId,
    payload: &PubResolvedNodePayload,
) -> Option<StoryFrame<StoryId, NodeId>> {
    let frame = payload.story_frame.as_ref()?;
    let story_id = frame.story_id?;

    Some(StoryFrame {
        story_id,
        frame_id: node_id,
        ordinal: frame.ordinal,
        previous: frame.previous_frame,
        next: frame.next_frame,
    })
}

struct EditableExportTypographyInputs<'a> {
    source_typography_runs: &'a [PubTypographyRun],
    source_typography_size_runs: &'a [PubTypographySizeRun],
    source_paragraph_alignments: &'a [PubParagraphAlignmentRun],
    full_story_typography: &'a [FullStoryTypographyV1],
    effective_full_story_paragraph_alignments: &'a [FullStoryParagraphAlignmentV1],
}

fn editable_export_plan(
    target: EditorEditableTarget,
    graph: &PubResolvedGraph,
    image_state: EditableExportImageState<'_>,
    typography: EditableExportTypographyInputs<'_>,
) -> Result<ExportPlan, ScopedCapabilityError> {
    let mut features = BTreeMap::new();
    features.insert("page.geometry".into(), CapabilityLevel::Preserved);
    features.insert("story.text".into(), CapabilityLevel::Preserved);
    features.insert("story.linked_frames".into(), CapabilityLevel::Preserved);
    if matches!(
        target,
        EditorEditableTarget::Idml | EditorEditableTarget::Odg
    ) {
        features.insert(IMAGE_BYTES_FEATURE.into(), CapabilityLevel::Preserved);
        features.insert(
            IMAGE_FRAME_GEOMETRY_FEATURE.into(),
            CapabilityLevel::Preserved,
        );
        features.insert(
            IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
            CapabilityLevel::Approximated,
        );
    }

    let manifest = TargetCapabilityManifest {
        target: TargetProfile {
            format: target.format().into(),
            adapter_version: target.adapter_version().into(),
            profile: "bounded-editable".into(),
            schema_fence: Some(target.schema_fence().into()),
        },
        features,
    };

    let mut requests = Vec::new();
    for page_id in &graph.document.pages {
        requests.push(SemanticFeatureRequest {
            feature: "page.geometry".into(),
            origin: Some(page_id.into_canonical()),
            property_path: Some("page.size".into()),
            require_preserved: true,
        });

        let page = graph
            .pages
            .get(page_id)
            .expect("resolved graph document page must be present");
        let authored = graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| {
                (node.header.parent_id == page_id.into_canonical()).then_some(*node_id)
            })
            .collect::<Vec<_>>();
        if page.children != authored {
            requests.push(SemanticFeatureRequest {
                feature: "page.object_order".into(),
                origin: Some(page_id.into_canonical()),
                property_path: Some("page.children".into()),
                require_preserved: false,
            });
        }
    }

    for story_id in graph.stories.keys() {
        requests.push(SemanticFeatureRequest {
            feature: "story.text".into(),
            origin: Some(story_id.into_canonical()),
            property_path: Some("story.text".into()),
            require_preserved: true,
        });
    }

    let font_family_stories = typography
        .source_typography_runs
        .iter()
        .filter_map(|run| {
            graph
                .stories
                .contains_key(&run.story_id)
                .then_some(run.story_id)
        })
        .collect::<BTreeSet<_>>();
    let font_size_stories = typography
        .source_typography_runs
        .iter()
        .map(|run| run.story_id)
        .chain(
            typography
                .source_typography_size_runs
                .iter()
                .map(|run| run.story_id),
        )
        .filter(|story_id| graph.stories.contains_key(story_id))
        .collect::<BTreeSet<_>>();
    let color_stories = typography
        .source_typography_runs
        .iter()
        .filter_map(|run| {
            (run.color_rgb.is_some() && graph.stories.contains_key(&run.story_id))
                .then_some(run.story_id)
        })
        .collect::<BTreeSet<_>>();
    let alignment_stories = typography
        .source_paragraph_alignments
        .iter()
        .filter_map(|run| {
            graph
                .stories
                .contains_key(&run.story_id)
                .then_some(run.story_id)
        })
        .chain(
            typography
                .effective_full_story_paragraph_alignments
                .iter()
                .map(|item| item.story_id),
        )
        .collect::<BTreeSet<_>>();

    for story_id in font_family_stories {
        requests.push(SemanticFeatureRequest {
            feature: STORY_FONT_FAMILY_FEATURE.into(),
            origin: Some(story_id.into_canonical()),
            property_path: Some("story.typography.font_family".into()),
            require_preserved: false,
        });
    }
    for story_id in font_size_stories {
        requests.push(SemanticFeatureRequest {
            feature: STORY_FONT_SIZE_FEATURE.into(),
            origin: Some(story_id.into_canonical()),
            property_path: Some("story.typography.font_size".into()),
            require_preserved: false,
        });
    }
    for story_id in color_stories {
        requests.push(SemanticFeatureRequest {
            feature: STORY_TEXT_COLOR_FEATURE.into(),
            origin: Some(story_id.into_canonical()),
            property_path: Some("story.typography.color".into()),
            require_preserved: false,
        });
    }
    for story_id in alignment_stories {
        requests.push(SemanticFeatureRequest {
            feature: STORY_PARAGRAPH_ALIGNMENT_FEATURE.into(),
            origin: Some(story_id.into_canonical()),
            property_path: Some("story.paragraph_alignment".into()),
            require_preserved: false,
        });
    }

    let page_ids = graph
        .document
        .pages
        .iter()
        .map(|page_id| page_id.into_canonical())
        .collect::<BTreeSet<_>>();
    let mut roots_per_story = BTreeMap::<StoryId, usize>::new();

    for (node_id, node) in &graph.nodes {
        if !page_ids.contains(&node.header.parent_id) {
            continue;
        }

        if let Some(frame) = frame_from_payload(*node_id, &node.payload) {
            if frame.previous.is_none() {
                *roots_per_story.entry(frame.story_id).or_default() += 1;
            }
            requests.push(SemanticFeatureRequest {
                feature: "story.linked_frames".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.story_frame".into()),
                require_preserved: true,
            });
        } else if let Some(asset_sha) = image_state.replacements.get(node_id) {
            let resource_id = replacement_asset_resource_id(*asset_sha);
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_BYTES_FEATURE.into(),
                origin: Some(resource_id.into_canonical()),
                property_path: Some("replacement_asset.bytes".into()),
                require_preserved: true,
            });
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_FRAME_GEOMETRY_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds".into()),
                require_preserved: true,
            });
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("image.content_transform".into()),
                require_preserved: image_state.crop_overrides.contains_key(node_id),
            });
        } else if let Some(resource_id) = image_state.source_nodes.get(node_id) {
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_BYTES_FEATURE.into(),
                origin: Some(resource_id.into_canonical()),
                property_path: Some("source_image.bytes".into()),
                require_preserved: true,
            });
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_FRAME_GEOMETRY_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds".into()),
                require_preserved: true,
            });
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("image.content_transform".into()),
                require_preserved: image_state.crop_overrides.contains_key(node_id),
            });
        } else if image_state.crop_overrides.contains_key(node_id) {
            requests.push(SemanticFeatureRequest {
                feature: IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("image.content_transform".into()),
                require_preserved: true,
            });
        } else {
            requests.push(SemanticFeatureRequest {
                feature: "node.unsupported".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node".into()),
                require_preserved: false,
            });
        }
    }

    for (story_id, roots) in roots_per_story {
        if roots > 1 {
            requests.push(SemanticFeatureRequest {
                feature: "story.shared_identity".into(),
                origin: Some(story_id.into_canonical()),
                property_path: Some("story.placements".into()),
                require_preserved: false,
            });
        }
    }

    let scoped = consumer_proven_typography_overrides_v1(target, typography.full_story_typography);
    plan_export_with_scoped_capabilities(&manifest, requests, scoped)
}

fn consumer_proven_typography_overrides_v1(
    target: EditorEditableTarget,
    typography: &[FullStoryTypographyV1],
) -> Vec<ScopedCapabilityOverride> {
    // Consumer-proven semantic class:
    // - #1079 proves real target-side edit -> save -> fresh reopen;
    // - #1091 proves the exact 1050 corpus contains 60 bounded Montserrat
    //   Stories across three source files and six independent size strata;
    // - #1097 proves all 60 wire carriers survive Scribus and LibreOffice
    //   save/reopen with exact family+size carrier histograms.
    //
    // Keep this predicate semantic: no source SHA, Story id, or tested-size
    // hardcode. Other families remain explicit Unsupported debt.
    let supports = |item: &FullStoryTypographyV1| match target {
        EditorEditableTarget::Idml | EditorEditableTarget::Odg => {
            item.font_family.trim() == "Montserrat"
        }
    };

    typography
        .iter()
        .filter(|item| supports(item))
        .flat_map(|item| {
            let origin = item.story_id.into_canonical();
            [
                ScopedCapabilityOverride {
                    origin,
                    feature: STORY_FONT_FAMILY_FEATURE.into(),
                    disposition: CapabilityLevel::Preserved,
                },
                ScopedCapabilityOverride {
                    origin,
                    feature: STORY_FONT_SIZE_FEATURE.into(),
                    disposition: CapabilityLevel::Preserved,
                },
            ]
        })
        .collect()
}

fn replacement_asset_resource_id(sha256: Sha256Digest) -> ResourceId {
    ResourceId::from_canonical(
        derive_source_canonical_id(SourceDerivedIdInput {
            source_hash: &sha256,
            adapter_id: "pub-editor",
            source_object_key: "replacement-image",
            semantic_role: "cdm.resource.image",
        })
        .expect("editor replacement asset identity contract uses fixed valid identifiers"),
    )
}

fn idml_base_projection_plan(
    plan: &ExportPlan,
    externally_projected_nodes: impl IntoIterator<Item = NodeId>,
) -> ExportPlan {
    let mut projection = plan.clone();
    for node_id in externally_projected_nodes {
        projection.losses.push(LossItem {
            feature: "node.external_image_projection".into(),
            origin: Some(node_id.into_canonical()),
            property_path: Some("node".into()),
            kind: LossKind::Unsupported,
            severity: LossSeverity::Semantic,
            target: projection.target.clone(),
            reversible: true,
            code: "export.internal.external_image_projection".into(),
        });
    }
    projection
}

fn apply_forward(
    graph: &mut PubResolvedGraph,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    match operation {
        EditOperation::LinkTextFrameTail { transition } => {
            link_text_frame_tail_v1::apply_transition(graph, transition, true)?;
        }
        EditOperation::ReplaceStoryRange {
            story_id,
            start_scalar,
            end_scalar,
            expected_before,
            replacement_text,
            before_story_state_id,
            after_story_state_id,
        } => {
            let story = graph
                .stories
                .get_mut(story_id)
                .ok_or(EditorError::MissingStory {
                    story_id: *story_id,
                })?;
            if story_state_id_v1(*story_id, &story.text) != *before_story_state_id {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            let after = replace_scalar_range_text(
                &story.text,
                *start_scalar,
                *end_scalar,
                expected_before,
                replacement_text,
            )
            .ok_or(EditorError::StaleOperation {
                story_id: *story_id,
            })?;
            if story_state_id_v1(*story_id, &after) != *after_story_state_id {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            story.text = after;
        }
        EditOperation::ReplaceStoryText {
            story_id,
            before,
            after,
        } => {
            let story = graph
                .stories
                .get_mut(story_id)
                .ok_or(EditorError::MissingStory {
                    story_id: *story_id,
                })?;
            if story.text != *before {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            story.text.clone_from(after);
        }
        EditOperation::BreakTextFrameForwardLink {
            story_id,
            new_story_id,
            before_frames,
            after_frames,
            ..
        } => {
            if !graph.stories.contains_key(story_id)
                || graph.stories.contains_key(new_story_id)
                || !frames_match_snapshots(graph, before_frames)
            {
                return Err(EditorError::StaleFrameTopology {
                    story_id: *story_id,
                });
            }

            let current_source_frames = graph
                .nodes
                .iter()
                .filter_map(|(node_id, node)| {
                    let frame = frame_from_payload(*node_id, &node.payload)?;
                    (frame.story_id == *story_id).then_some(frame.frame_id)
                })
                .collect::<BTreeSet<_>>();
            let expected_source_frames = before_frames
                .iter()
                .map(|frame| frame.frame_id)
                .collect::<BTreeSet<_>>();
            if current_source_frames != expected_source_frames {
                return Err(EditorError::StaleFrameTopology {
                    story_id: *story_id,
                });
            }

            graph
                .stories
                .insert(*new_story_id, empty_editor_story(*new_story_id));
            for frame in after_frames {
                set_story_frame_snapshot(graph, frame)?;
            }
        }
        EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            text_preset,
        } => {
            validate_create_text_box_candidate(
                graph,
                *node_id,
                *story_id,
                *page_id,
                *bounds,
                text_preset,
            )?;
            graph
                .pages
                .get_mut(page_id)
                .expect("CreateTextBox candidate validated page")
                .children
                .push(*node_id);
            graph
                .stories
                .insert(*story_id, empty_editor_story(*story_id));
            graph.nodes.insert(
                *node_id,
                authored_text_box_node_v1(*node_id, *story_id, *page_id, *bounds),
            );
        }
        EditOperation::ReplaceTableCellText {
            node_id,
            story_id,
            before_story,
            after_story,
            before_ranges,
            after_ranges,
            ..
        } => {
            apply_table_cell_state(
                graph,
                *node_id,
                *story_id,
                before_story,
                after_story,
                before_ranges,
                after_ranges,
            )?;
        }
        EditOperation::MoveNode {
            node_id,
            before,
            after,
        } => {
            let node = graph
                .nodes
                .get_mut(node_id)
                .ok_or(EditorError::NodeMoveUnsupported { node_id: *node_id })?;
            if node.header.bounds != *before {
                return Err(EditorError::StaleNodeMove { node_id: *node_id });
            }
            node.header.bounds = *after;
        }
        EditOperation::MoveNodes { page_id, entries } => {
            validate_move_nodes_transition(graph, *page_id, entries, true)?;
            for entry in entries {
                graph
                    .nodes
                    .get_mut(&entry.node_id)
                    .expect("validated MoveNodes node")
                    .header
                    .bounds = entry.after;
            }
        }
        EditOperation::ResizeNode {
            node_id,
            before,
            after,
        } => {
            let node = graph
                .nodes
                .get_mut(node_id)
                .ok_or(EditorError::NodeResizeUnsupported { node_id: *node_id })?;
            if node.header.bounds != *before {
                return Err(EditorError::StaleNodeResize { node_id: *node_id });
            }
            node.header.bounds = *after;
        }
        EditOperation::ResizeNodes { page_id, entries } => {
            validate_resize_nodes_transition(graph, *page_id, entries, true)?;
            for entry in entries {
                graph
                    .nodes
                    .get_mut(&entry.node_id)
                    .expect("validated ResizeNodes node")
                    .header
                    .bounds = entry.after;
            }
        }
        EditOperation::ReplaceImage { .. } => {
            unreachable!("image replacements are applied to editor overlay state")
        }
        EditOperation::SetImageCrop { .. } => {
            unreachable!("image crop is applied to editor overlay state")
        }
        EditOperation::CreateShape { .. } => {
            unreachable!("CreateShape is applied to the authored overlay state")
        }
        EditOperation::CreateLine { .. } => {
            unreachable!("CreateLine is applied to the authored overlay state")
        }
        EditOperation::CreateTable { .. } => {
            unreachable!("CreateTable is applied atomically with the authored-stack lane")
        }
        EditOperation::SetTableTrackExtent { .. } => {
            unreachable!("table track extents are derived from editor history")
        }
        EditOperation::InsertTableRow { .. }
        | EditOperation::DeleteTableRow { .. }
        | EditOperation::InsertTableColumn { .. }
        | EditOperation::DeleteTableColumn { .. } => {
            unreachable!(
                "table row/column lifecycle is applied through the structural snapshot bridge"
            )
        }
        EditOperation::DeleteNode { .. } => {
            unreachable!("DeleteNode is applied to the authored overlay state")
        }
        EditOperation::ReorderAuthoredStack { .. } => {
            unreachable!("ReorderAuthoredStack is applied to the authored lane overlay state")
        }
        EditOperation::SetTextFormatProperty { .. }
        | EditOperation::ClearTextFormatPropertyOverride { .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. } => {
            unreachable!("text-format operations are derived from editor history")
        }
        EditOperation::SetParagraphAlignmentOverride { .. }
        | EditOperation::ClearParagraphAlignmentOverride { .. } => {
            unreachable!("paragraph-alignment operations are derived from editor history")
        }
    }
    Ok(())
}

fn apply_inverse(
    graph: &mut PubResolvedGraph,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    match operation {
        EditOperation::LinkTextFrameTail { transition } => {
            link_text_frame_tail_v1::apply_transition(graph, transition, false)?;
        }
        EditOperation::ReplaceStoryRange {
            story_id,
            start_scalar,
            expected_before,
            replacement_text,
            before_story_state_id,
            after_story_state_id,
            ..
        } => {
            let story = graph
                .stories
                .get_mut(story_id)
                .ok_or(EditorError::MissingStory {
                    story_id: *story_id,
                })?;
            if story_state_id_v1(*story_id, &story.text) != *after_story_state_id {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            let replacement_end = start_scalar
                .checked_add(
                    u32::try_from(replacement_text.chars().count()).map_err(|_| {
                        EditorError::StaleOperation {
                            story_id: *story_id,
                        }
                    })?,
                )
                .ok_or(EditorError::StaleOperation {
                    story_id: *story_id,
                })?;
            let before = replace_scalar_range_text(
                &story.text,
                *start_scalar,
                replacement_end,
                replacement_text,
                expected_before,
            )
            .ok_or(EditorError::StaleOperation {
                story_id: *story_id,
            })?;
            if story_state_id_v1(*story_id, &before) != *before_story_state_id {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            story.text = before;
        }
        EditOperation::ReplaceStoryText {
            story_id,
            before,
            after,
        } => {
            let story = graph
                .stories
                .get_mut(story_id)
                .ok_or(EditorError::MissingStory {
                    story_id: *story_id,
                })?;
            if story.text != *after {
                return Err(EditorError::StaleOperation {
                    story_id: *story_id,
                });
            }
            story.text.clone_from(before);
        }
        EditOperation::BreakTextFrameForwardLink {
            story_id,
            new_story_id,
            before_frames,
            after_frames,
            ..
        } => {
            let expected_empty = empty_editor_story(*new_story_id);
            if !graph.stories.contains_key(story_id)
                || graph.stories.get(new_story_id) != Some(&expected_empty)
                || !frames_match_snapshots(graph, after_frames)
            {
                return Err(EditorError::StaleFrameTopology {
                    story_id: *story_id,
                });
            }

            let current_new_story_frames = graph
                .nodes
                .iter()
                .filter_map(|(node_id, node)| {
                    let frame = frame_from_payload(*node_id, &node.payload)?;
                    (frame.story_id == *new_story_id).then_some(frame.frame_id)
                })
                .collect::<BTreeSet<_>>();
            let expected_new_story_frames = after_frames
                .iter()
                .filter(|frame| frame.story_id == *new_story_id)
                .map(|frame| frame.frame_id)
                .collect::<BTreeSet<_>>();
            let current_source_story_frames = graph
                .nodes
                .iter()
                .filter_map(|(node_id, node)| {
                    let frame = frame_from_payload(*node_id, &node.payload)?;
                    (frame.story_id == *story_id).then_some(frame.frame_id)
                })
                .collect::<BTreeSet<_>>();
            let expected_source_story_frames = after_frames
                .iter()
                .filter(|frame| frame.story_id == *story_id)
                .map(|frame| frame.frame_id)
                .collect::<BTreeSet<_>>();
            if current_new_story_frames != expected_new_story_frames
                || current_source_story_frames != expected_source_story_frames
            {
                return Err(EditorError::StaleFrameTopology {
                    story_id: *story_id,
                });
            }

            graph.stories.remove(new_story_id);
            for frame in before_frames {
                set_story_frame_snapshot(graph, frame)?;
            }
        }
        EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            ..
        } => {
            let expected_story = empty_editor_story(*story_id);
            let expected_node = authored_text_box_node_v1(*node_id, *story_id, *page_id, *bounds);
            let page_has_exact_child = graph.pages.get(page_id).is_some_and(|page| {
                page.children.iter().filter(|id| **id == *node_id).count() == 1
            });
            if graph.stories.get(story_id) != Some(&expected_story)
                || graph.nodes.get(node_id) != Some(&expected_node)
                || !page_has_exact_child
            {
                return Err(EditorError::StaleCreateTextBox {
                    node_id: *node_id,
                    story_id: *story_id,
                });
            }
            graph.nodes.remove(node_id);
            graph.stories.remove(story_id);
            graph
                .pages
                .get_mut(page_id)
                .expect("CreateTextBox inverse validated page")
                .children
                .retain(|id| id != node_id);
        }
        EditOperation::ReplaceTableCellText {
            node_id,
            story_id,
            before_story,
            after_story,
            before_ranges,
            after_ranges,
            ..
        } => {
            apply_table_cell_state(
                graph,
                *node_id,
                *story_id,
                after_story,
                before_story,
                after_ranges,
                before_ranges,
            )?;
        }
        EditOperation::MoveNode {
            node_id,
            before,
            after,
        } => {
            let node = graph
                .nodes
                .get_mut(node_id)
                .ok_or(EditorError::NodeMoveUnsupported { node_id: *node_id })?;
            if node.header.bounds != *after {
                return Err(EditorError::StaleNodeMove { node_id: *node_id });
            }
            node.header.bounds = *before;
        }
        EditOperation::MoveNodes { page_id, entries } => {
            validate_move_nodes_transition(graph, *page_id, entries, false)?;
            for entry in entries {
                graph
                    .nodes
                    .get_mut(&entry.node_id)
                    .expect("validated MoveNodes node")
                    .header
                    .bounds = entry.before;
            }
        }
        EditOperation::ResizeNode {
            node_id,
            before,
            after,
        } => {
            let node = graph
                .nodes
                .get_mut(node_id)
                .ok_or(EditorError::NodeResizeUnsupported { node_id: *node_id })?;
            if node.header.bounds != *after {
                return Err(EditorError::StaleNodeResize { node_id: *node_id });
            }
            node.header.bounds = *before;
        }
        EditOperation::ResizeNodes { page_id, entries } => {
            validate_resize_nodes_transition(graph, *page_id, entries, false)?;
            for entry in entries {
                graph
                    .nodes
                    .get_mut(&entry.node_id)
                    .expect("validated ResizeNodes node")
                    .header
                    .bounds = entry.before;
            }
        }
        EditOperation::ReplaceImage { .. } => {
            unreachable!("image replacements are applied to editor overlay state")
        }
        EditOperation::SetImageCrop { .. } => {
            unreachable!("image crop is applied to editor overlay state")
        }
        EditOperation::CreateShape { .. } => {
            unreachable!("CreateShape is reverted in the authored overlay state")
        }
        EditOperation::CreateLine { .. } => {
            unreachable!("CreateLine is reverted in the authored overlay state")
        }
        EditOperation::CreateTable { .. } => {
            unreachable!("CreateTable is reverted atomically with the authored-stack lane")
        }
        EditOperation::SetTableTrackExtent { .. } => {
            unreachable!("table track extents are derived from editor history")
        }
        EditOperation::InsertTableRow { .. }
        | EditOperation::DeleteTableRow { .. }
        | EditOperation::InsertTableColumn { .. }
        | EditOperation::DeleteTableColumn { .. } => {
            unreachable!(
                "table row/column lifecycle is applied through the structural snapshot bridge"
            )
        }
        EditOperation::DeleteNode { .. } => {
            unreachable!("DeleteNode is reverted in the authored overlay state")
        }
        EditOperation::ReorderAuthoredStack { .. } => {
            unreachable!("ReorderAuthoredStack is reverted in the authored lane overlay state")
        }
        EditOperation::SetTextFormatProperty { .. }
        | EditOperation::ClearTextFormatPropertyOverride { .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. } => {
            unreachable!("text-format operations are derived from editor history")
        }
        EditOperation::SetParagraphAlignmentOverride { .. }
        | EditOperation::ClearParagraphAlignmentOverride { .. } => {
            unreachable!("paragraph-alignment operations are derived from editor history")
        }
    }
    Ok(())
}

fn authored_stack_operation_page_id_v1(operation: &EditOperation) -> Option<PageId> {
    match operation {
        EditOperation::CreateShape { page_id, .. }
        | EditOperation::CreateLine { page_id, .. }
        | EditOperation::DeleteNode { page_id, .. } => Some(*page_id),
        EditOperation::CreateTable { table } => Some(table.page_id),
        EditOperation::ReorderAuthoredStack { transition } => Some(transition.page_id),
        _ => None,
    }
}

fn install_authored_stack_in_map_v1(
    stacks: &mut BTreeMap<PageId, AuthoredStackV1>,
    stack: AuthoredStackV1,
) {
    if stack.members.is_empty() {
        stacks.remove(&stack.page_id);
    } else {
        stacks.insert(stack.page_id, stack);
    }
}

fn apply_authored_stack_history_forward_v1(
    stacks: &mut BTreeMap<PageId, AuthoredStackV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    match operation {
        EditOperation::CreateShape { page_id, .. } => {
            let shape = authored_shape_from_operation(operation)
                .expect("CreateShape reconstructs authored shape");
            let before = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_create_shape_append_v1(&before, &shape)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&before, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::CreateLine { page_id, .. } => {
            let line = authored_line_from_operation(operation)
                .expect("CreateLine reconstructs authored line");
            let before = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_create_line_append_v1(&before, &line)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&before, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::CreateTable { table } => {
            let before = stacks
                .get(&table.page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(table.page_id));
            let transition = plan_create_table_append_v1(&before, table.node_id, table.page_id)
                .map_err(|_| EditorError::StaleAuthoredStack {
                    page_id: table.page_id,
                })?;
            let after =
                apply_authored_stack_transition_forward_v1(&before, &transition).map_err(|_| {
                    EditorError::StaleAuthoredStack {
                        page_id: table.page_id,
                    }
                })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::DeleteNode {
            page_id, before, ..
        } => {
            let stack = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_delete_shape_remove_v1(&stack, before)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&stack, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::ReorderAuthoredStack { transition } => {
            let current = stacks
                .get(&transition.page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(transition.page_id));
            let after =
                apply_authored_stack_reorder_forward_v1(&current, transition).map_err(|_| {
                    EditorError::StaleAuthoredStack {
                        page_id: transition.page_id,
                    }
                })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        _ => {}
    }
    Ok(())
}

fn derive_authored_stacks_from_operations_v1(
    operations: &[EditOperation],
) -> Result<BTreeMap<PageId, AuthoredStackV1>, EditorError> {
    let mut stacks = BTreeMap::new();
    for operation in operations {
        apply_authored_stack_history_forward_v1(&mut stacks, operation)?;
    }
    Ok(stacks)
}

fn authored_shape_from_operation(operation: &EditOperation) -> Option<AuthoredShapeRuntimeV1> {
    match operation {
        EditOperation::CreateShape {
            node_id,
            page_id,
            parent_id,
            shape_kind,
            bounds,
            transform,
            paint,
            provenance,
        } => Some(AuthoredShapeRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            shape_kind: *shape_kind,
            bounds: *bounds,
            transform: *transform,
            paint: paint.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

fn authored_line_from_operation(operation: &EditOperation) -> Option<AuthoredLineRuntimeV1> {
    match operation {
        EditOperation::CreateLine {
            node_id,
            page_id,
            parent_id,
            geometry,
            stroke,
            provenance,
        } => Some(AuthoredLineRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            geometry: *geometry,
            stroke: stroke.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

fn apply_authored_line_inverse(
    authored_lines: &mut BTreeMap<NodeId, AuthoredLineRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let line = authored_line_from_operation(operation)
        .expect("CreateLine inverse receives CreateLine operation");
    if authored_lines.get(&line.node_id) != Some(&line) {
        return Err(EditorError::CreateLineIdCollision {
            node_id: line.node_id,
        });
    }
    authored_lines.remove(&line.node_id);
    Ok(())
}

fn apply_authored_shape_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let shape = authored_shape_from_operation(operation)
        .expect("CreateShape inverse receives CreateShape operation");
    if authored_shapes.get(&shape.node_id) != Some(&shape) {
        return Err(EditorError::CreateShapeIdCollision {
            node_id: shape.node_id,
        });
    }
    authored_shapes.remove(&shape.node_id);
    Ok(())
}

fn apply_authored_shape_delete_forward(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode forward receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.get(node_id) != Some(before)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.remove(node_id);
    Ok(())
}

fn apply_authored_shape_delete_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode inverse receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.contains_key(node_id)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.insert(*node_id, before.clone());
    Ok(())
}

fn snapshot_table_ranges(table: &pub_reader::PubTableSource) -> Vec<TableCellRangeSnapshot> {
    table
        .cells
        .iter()
        .map(|cell| TableCellRangeSnapshot {
            cell_id: cell.id,
            utf16_start: cell.utf16_start,
            utf16_end: cell.utf16_end,
        })
        .collect()
}

fn rebuild_simple_table_story(
    table: &pub_reader::PubTableSource,
    cells: &[pub_reader::PubMaterializedTableCell],
) -> Result<(String, Vec<TableCellRangeSnapshot>), ()> {
    let simple = table.simple_table.as_ref().ok_or(())?;
    let mut ordered = simple.cells.clone();
    ordered.sort_by_key(|cell| (cell.address.row, cell.address.column, cell.id));

    if table.text_id == AUTHORED_TABLE_SENTINEL_TEXT_ID_V1 && table.source_refs.is_empty() {
        let ordered_cells = ordered
            .iter()
            .map(|semantic| {
                let materialized = cells.iter().find(|cell| cell.id == semantic.id).ok_or(())?;
                Ok((semantic.id, materialized.text.clone()))
            })
            .collect::<Result<Vec<_>, ()>>()?;
        let (text, by_id) = rebuild_authored_table_story_v1(&ordered_cells).map_err(|_| ())?;
        let ranges = ordered
            .iter()
            .map(|semantic| {
                let (utf16_start, utf16_end) = by_id[&semantic.id];
                TableCellRangeSnapshot {
                    cell_id: semantic.id,
                    utf16_start,
                    utf16_end,
                }
            })
            .collect();
        return Ok((text, ranges));
    }

    let mut utf16 = Vec::<u16>::new();
    let mut ranges = Vec::with_capacity(ordered.len());

    for (index, semantic) in ordered.iter().enumerate() {
        let materialized = cells.iter().find(|cell| cell.id == semantic.id).ok_or(())?;
        let start = u32::try_from(utf16.len()).map_err(|_| ())?;
        if index > 0 {
            utf16.push(0x000D);
        }
        utf16.extend(materialized.text.encode_utf16());
        if index + 1 == ordered.len() {
            utf16.push(0x000D);
        }
        let end = u32::try_from(utf16.len()).map_err(|_| ())?;
        ranges.push(TableCellRangeSnapshot {
            cell_id: semantic.id,
            utf16_start: start,
            utf16_end: end,
        });
    }

    let text = String::from_utf16(&utf16).map_err(|_| ())?;
    Ok((text, ranges))
}

fn apply_table_cell_state(
    graph: &mut PubResolvedGraph,
    node_id: NodeId,
    story_id: StoryId,
    expected_story: &str,
    replacement_story: &str,
    expected_ranges: &[TableCellRangeSnapshot],
    replacement_ranges: &[TableCellRangeSnapshot],
) -> Result<(), EditorError> {
    let story = graph
        .stories
        .get_mut(&story_id)
        .ok_or(EditorError::MissingStory { story_id })?;
    if story.text != expected_story {
        return Err(EditorError::StaleOperation { story_id });
    }

    let node = graph
        .nodes
        .get_mut(&node_id)
        .ok_or(EditorError::TableEditUnsupported { node_id })?;
    let table = node
        .payload
        .table
        .as_mut()
        .ok_or(EditorError::TableEditUnsupported { node_id })?;
    if table.story_id != Some(story_id) {
        return Err(EditorError::StaleOperation { story_id });
    }

    if snapshot_table_ranges(table) != expected_ranges {
        return Err(EditorError::StaleOperation { story_id });
    }

    for range in replacement_ranges {
        let cell = table
            .cells
            .iter_mut()
            .find(|cell| cell.id == range.cell_id)
            .ok_or(EditorError::TableEditUnsupported { node_id })?;
        cell.utf16_start = range.utf16_start;
        cell.utf16_end = range.utf16_end;
    }
    story.text.clear();
    story.text.push_str(replacement_story);
    Ok(())
}

#[cfg(test)]
mod table_rowcol_metadata_tests {
    use super::*;

    #[test]
    fn source_typography_marks_table_story_ranges_as_unremapped() {
        let source_hash: Sha256Digest =
            "abababababababababababababababababababababababababababababababab"
                .parse()
                .expect("source hash");
        let story_id: StoryId =
            serde_json::from_str("\"55000000-0000-4000-8000-000000000001\"").expect("story id");
        let graph: PubResolvedGraph = pub_model::ResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: "rowcol-metadata-test".into(),
            source: pub_model::SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-rs/test".into(),
                source_hash,
            },
            document: pub_model::Document {
                id: serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                    .expect("document id"),
                format_origin: "pub".into(),
                source_hash,
                pages: Vec::new(),
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages: BTreeMap::new(),
            nodes: BTreeMap::new(),
            stories: BTreeMap::from([(
                story_id,
                Story {
                    id: story_id,
                    text: "x".into(),
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs: Vec::new(),
                },
            )]),
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        };
        let mut session = EditorSession::new(graph).expect("session");
        assert!(!session.table_story_has_unremapped_range_metadata_v1(story_id));

        session.source_typography_runs.push(PubTypographyRun {
            story_id,
            story_utf16_start: 0,
            story_utf16_end: 1,
            story_scalar_start: 0,
            story_scalar_end: 1,
            source_font_index: 0,
            source_font_name: "Arial".into(),
            text_size_emu: 152_400,
            font_inherited: false,
            size_inherited: false,
            color_rgb: Some([0, 0, 0]),
            color_scheme_slot: None,
            color_inherited: false,
            bold: None,
            italic: None,
        });

        assert!(session.table_story_has_unremapped_range_metadata_v1(story_id));
    }
}

#[cfg(test)]
mod asset_reachability_tests {
    use super::*;

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
