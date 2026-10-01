//! Bounded authoring session for Publisher migration workflows.
//!
//! This crate does not make a general "editable PUB" claim. Every edit
//! operation is capability-gated and preserves the immutable source identity.
//! The first operation is a bounded ordinary-story text replacement over the
//! resolved authoring graph. Native PUB materialization remains a separate
//! writer gate.

mod create_shape_runtime_v1;
mod native_pub;
mod writer_assessment;

pub use native_pub::{EditorNativePubCandidate, EditorNativePubMaterializationBlocked};
pub use create_shape_runtime_v1::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1,
    AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    CreateShapeRuntimeValidationError, Srgb8V1, validate_authored_shape_runtime_v1,
};
pub use writer_assessment::{
    EDITOR_PUB_WRITER_ASSESSMENT_SCHEMA_V0_1, EditorPubPersistenceAssessment,
    EditorPubWriterAssessment, EditorPubWriterAssessmentError, EditorStoryWriterProbeResult,
    EditorStoryWriterProbeState, EffectiveStoryTextMutation,
};

use pub_export::{
    CapabilityLevel, ExportPlan, ExportReport, ExportReportSource, FormatCompatibilityManifest,
    FormatRepresentability, LossItem, LossKind, LossSeverity, PersistenceCompatibilityAssessment,
    PersistenceCompatibilityError, PersistenceRequirement, PersistenceRequirements,
    PersistenceTargetProfile, SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile,
    WriterCapabilityManifest, assess_persistence_compatibility, build_export_report, plan_export,
    render_human_summary,
};
use pub_idml::{
    IDML_ADAPTER_VERSION_V0_1, IDML_SCHEMA_FENCE_LEGACY_DOM_7, IMAGE_BYTES_FEATURE,
    IMAGE_CONTENT_TRANSFORM_FEATURE, IMAGE_FRAME_GEOMETRY_FEATURE, IdmlEmbeddedImagePlacement,
    IdmlWireProfile, add_embedded_images_to_idml, project_resolved_graph_to_idml, write_idml_ucf,
};
use pub_model::{
    Affine2D, EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1,
    EffectiveTableTrackV1, Node, NodeHeader, NodeKind, ResourceId, SourceDerivedIdInput, Story,
    StoryFrame, TableColumnId, TableRowId, derive_source_canonical_id, validate_story_frames,
};
pub use pub_model::{LengthEmu, NodeId, PageId, RectEmu, Sha256Digest, StoryId, TableCellId};
use pub_odg::{
    ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgEmbeddedImagePlacement,
    add_embedded_images_to_odg, project_resolved_graph_to_odg, write_odg,
};
use pub_reader::{
    PubResolvedGraph, PubResolvedNodePayload, PubResolvedStoryFrame,
    build_mature_0x2c_source_graph, materialize_bounded_simple_table_cells,
    resolve_pub_source_graph,
};