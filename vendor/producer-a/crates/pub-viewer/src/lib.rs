//! Read-only product boundary for PUB Viewer.
//!
//! This crate deliberately sits above the format-aware `pub-reader` adapter.
//! Its public document DTO is source-neutral: callers receive canonical model
//! identities, page sizes, semantic story text, stable viewer diagnostics, and
//! (for the visual path) an existing source-free `pub-layout` resolved scene.
//! CFB, Contents, Quill, Escher, byte offsets, and writer mutation state are not
//! part of the Viewer contract.

use anyhow::{Context, Result, anyhow};
#[cfg(feature = "cmo-slot-compose")]
use chaptera_layout_projection::{
    CarrierExtentV1, CmoStorySlotFlowInputV1, resolve_cmo_slot_flow_v1,
};
#[cfg(feature = "cmo-slot-compose")]
use chaptera_scene_instance::{SceneInstanceV1, SceneProjectionKindV1, cmo_story_slot_instance_v1};
use pub_layout::{
    BoundedAuthoringSlice, BoundedLayoutProjection, BoundedNodeGeometryInput, BoundedTableInput,
    BoundedTextFlowEnvironment, BoundedTextMetrics, BoundedUniformTableMetrics,
    ProjectedStoryFrame, ProjectionDiagnostic, ResolveDiagnostic, ResolvedPhysicalNode,
    project_bounded, resolve_bounded_geometry, resolve_bounded_text_flow,
    resolve_bounded_uniform_table_cells,
};
pub use pub_layout::{BoundedLayoutEnvironment, BoundedResolvedScene};
#[cfg(feature = "cmo-slot-compose")]
use pub_model::CanonicalId;
use pub_model::{
    Affine2D, AuthorityClass, LengthEmu, Node, NodeId, NodeKind, PageId, ReadConfidence, RectEmu,
    ResourceId, Sha256Digest, SourceDerivedIdInput, SourceRole, StoryFrame, StoryId,
    TableCellAddress, TableCellId, derive_source_canonical_id,
};
use pub_paint_bridge::{
    PubEffectiveFillSourceV1, PubEffectiveLineSourceV1, PubEffectivePaintAuthorityV1,
    PubEffectivePaintSourceSpanV1, PubEffectivePaintValueV1, PubEffectiveShapePaintSourceV1,
    PubExplicitFillSourceV1, PubExplicitLineSourceV1, PubExplicitShapePaintSourceV1,
    PubPaintSourceProvenanceV1, PubPaintSourceRoleV1, project_effective_source_paint_to_viewer_v1,
    project_explicit_source_paint_to_viewer_v1,
};
#[cfg(test)]
use pub_presentation_profile::STANDARD_PRINT_SERVICE_TAIL_PROFILE_ID_V1;
use pub_presentation_profile::{
    CARLTON_PRESENTATION_INPUT_SCHEMA_V1, CarltonPageEvidenceV1, CarltonPresentationProfileInputV1,
    STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1, StandardPrintServiceTailPageEvidenceV1,
    StandardPrintServiceTailProfileInputV1, carlton_admitted_carrier_page_seq_nums_v1,
    reference_fixture_profile_known_v1, select_carlton_customer_page_seq_nums_v1,
    select_reference_fixture_customer_page_seq_nums_v1,
    select_standard_print_service_tail_customer_page_seq_nums_v1,
};
#[cfg(test)]
use pub_reader::LegacyOleCachedPresentation;
#[cfg(feature = "cmo-slot-compose")]
use pub_reader::build_mature_0x2c_cmo_projection_bridge_v1;
pub use pub_reader::{
    CHAPTERA_EXACT_FILE_CONSENT_V1, CHAPTERA_INTAKE_RETENTION_POLICY_V1, FailureIntakeClass,
    FailureIntakeClassification, FailureIntakeConfidence, FailureIntakeReason,
    PubFamilyClassification, PubFamilyConfidence, PubFamilyProfile, PubFamilyReason,
    PubReaderRoute, READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1, READER_SALVAGE_PROBE_SCHEMA_V1,
    ReaderPartialSourceFact, ReaderPartialSourceGap, ReaderPartialSourceGraph,
    ReaderPartialSourceGraphError, ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility,
    ReaderSalvageProbe, ReaderSalvageStreamState, ReaderSalvageSubsystemProbe,
    ReaderSalvageTrigger, build_reader_partial_source_graph, classify_failure_candidate,
    classify_pub_family, exact_file_intake_eligible, probe_reader_salvage_candidate,
    probe_reader_salvage_candidate_with_trigger,
};
use pub_reader::{
    FailureCode, FailureEnvelope, FailureEnvelopeContext, FailureParserStage,
    FailureTelemetryChoice, LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1, LegacyOleCachedPresentationScan,
    LegacyOleCachedPresentationSelection, MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1,
    PubAssetExportDiagnostic, PubBridgeDiagnostic, PubEffectivePaintAuthority,
    PubExplicitImageCropSource, PubParagraphAlignment, PubResolveDiagnostic, PubResolvedGraph,
    PubResolvedGraphBuild, PubResolvedNodePayload, PubScriptFontEntryDisposition,
    PubSourceGraphBuild, PubSourcePagePaintOrderV1, PubTextFrameVerticalAlignment, WmfPreviewRgba,
    analyze_mature_0x2c_page_roles, build_failure_envelope, build_legacy_0x22_noquill_source_graph,
    build_legacy_0x22_quill_source_graph, build_mature_0x2c_asset_export_bundle_from_bytes,
    build_mature_0x2c_source_graph, build_mature_0x2c_wmf_preview_bundle_from_bytes,
    derive_pub_page_id, materialize_bounded_table_cells, rasterize_wmf_preview,
    read_legacy_0x22_image_wmfs, resolve_pub_source_graph, scan_legacy_ole_cached_presentations,
    select_unambiguous_legacy_ole_cached_presentation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

pub const VIEWER_DOCUMENT_SCHEMA_V0_1: &str = "0.1";
pub const VIEWER_GEOMETRY_SCHEMA_V0_1: &str = "0.1";

pub const VIEWER_FAILURE_REPORT_SCHEMA_V0_1: &str = "chaptera-viewer-failure-report/v0.1";
pub const VIEWER_FALLBACK_TEXT_METRICS_REVISION_V0_1: &str = "viewer-fallback-text-metrics-v0.1";
const VIEWER_FALLBACK_SCALAR_ADVANCE_EMU_V0_1: i64 = 57_150;
const VIEWER_FALLBACK_LINE_HEIGHT_EMU_V0_1: i64 = 142_875;
const MAX_LEGACY_OLE_PREVIEW_PNG_BYTES: usize = 8 * 1024 * 1024;
#[cfg(feature = "cmo-slot-compose")]
const CARLTON_MARCH_PRESENTATION_PROFILE_V1: &str = "carlton-school-jotter/march-2026/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerFailureDiagnosticReport {
    pub schema_version: String,
    pub envelope: FailureEnvelope,
}

/// Builds an inspectable, local-only diagnostic report for a failed Reader open.
///
/// The report deliberately reuses the allowlisted structural failure envelope:
/// it contains no filesystem path, filename, exact file hash, document text,
/// images, raw streams, or document bytes. This function performs no network
/// I/O and does not imply that the file is eligible for corpus submission.
pub fn build_local_failure_diagnostic_report(
    bytes: &[u8],
) -> Result<ViewerFailureDiagnosticReport> {
    let classification = classify_failure_candidate(bytes);
    let envelope = build_failure_envelope(
        FailureTelemetryChoice::MinimalStructural,
        &classification,
        FailureEnvelopeContext {
            parser_stage: FailureParserStage::PubReaderOpen,
            failure_code: FailureCode::OpenFailed,
            timeout: false,
            resource_limit: false,
            byte_len: bytes.len(),
            os_family: None,
            architecture: None,
            coarse_locale: None,
        },
    )
    .expect("minimal structural failure envelope must be enabled");

    Ok(ViewerFailureDiagnosticReport {
        schema_version: VIEWER_FAILURE_REPORT_SCHEMA_V0_1.to_owned(),
        envelope,
    })
}

/// Serializes the local diagnostic report as human-inspectable JSON.
///
/// The caller owns choosing a destination path and writing the returned bytes.
pub fn local_failure_diagnostic_json(bytes: &[u8]) -> Result<String> {
    let report = build_local_failure_diagnostic_report(bytes)?;
    serde_json::to_string_pretty(&report).context("serialize local Chaptera failure diagnostics")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerDocument {
    pub schema_version: String,
    pub source: ViewerSource,
    pub pages: Vec<ViewerPage>,
    pub stories: Vec<ViewerStory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ViewerDiagnostic>,
}

impl ViewerDocument {
    /// Product-facing visual fidelity status for a successfully opened document.
    ///
    /// `Unsupported` is reserved for the application open boundary: once a
    /// `ViewerDocument` exists, the document is either supported within the
    /// current scope or partial because one or more known fidelity warnings
    /// remain.
    pub fn fidelity_status(&self) -> ViewerFidelityStatus {
        if self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == ViewerDiagnosticSeverity::FidelityWarning)
        {
            ViewerFidelityStatus::Partial
        } else {
            ViewerFidelityStatus::Supported
        }
    }

    /// Exact search over recovered semantic story text.
    ///
    /// Results intentionally carry a stable StoryId but no page association:
    /// the current ViewerDocument contract does not expose a proven
    /// story/frame-to-page relation, and the Viewer must not invent one.
    pub fn search_text(&self, query: &str) -> Vec<ViewerTextMatch> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        for story in &self.stories {
            for (start, matched) in story.text.match_indices(query) {
                let end = start + matched.len();
                matches.push(ViewerTextMatch {
                    story_id: story.id,
                    start_byte: u64::try_from(start)
                        .expect("Viewer story byte offset must fit into u64"),
                    end_byte: u64::try_from(end)
                        .expect("Viewer story byte offset must fit into u64"),
                    text: matched.to_owned(),
                });
            }
        }
        matches
    }
}

/// First real visual Viewer handoff.
///
/// `document` owns application order/text/diagnostics. `scene` is the
/// source-free physical geometry produced by the existing layout boundary. It
/// intentionally does not claim that text, images, fill/line, or effects have
/// been painted yet.
#[cfg(feature = "cmo-slot-compose")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerProjectedSceneInstanceV1 {
    /// Canonical visual identity/projection semantics from chaptera-scene-instance.
    pub scene_instance: SceneInstanceV1,
    /// Paint placement metadata only; not an identity authority.
    pub target_frame_node_id: NodeId,
    /// Story-global direct-paint cutoff from native Cmo slot-flow.
    /// Scalars at or after this authoritative first-nonfitting boundary remain
    /// canonical source text but are overset and must not paint in the target frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_frame_paint_scalar_end: Option<u32>,
    pub bounds: RectEmu,
    /// Source-backed inner text composition box. Outer projected geometry,
    /// paint, hit-test and slot identity remain on `bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_content_bounds: Option<RectEmu>,
    pub transform: Affine2D,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerGeometryDocument {
    pub schema_version: String,
    pub document: ViewerDocument,
    pub scene: BoundedResolvedScene,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paints: Vec<ViewerNodePaint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub story_frames: Vec<ViewerStoryFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_fragments: Vec<ViewerTextFragment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typography_runs: Vec<ViewerTypographyRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraph_alignments: Vec<ViewerParagraphAlignmentRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_color_runs: Vec<ViewerTextColorRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_font_maps: Vec<ViewerScriptFontMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<ViewerTable>,
    #[cfg(feature = "cmo-slot-compose")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projected_instances: Vec<ViewerProjectedSceneInstanceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ViewerEmbeddedImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerPagePaintOrderV1 {
    pub page_id: PageId,
    /// Canonical source-backed node identities in back-to-front paint order.
    pub node_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ViewerSourcePaintOrderApplicationStatsV1 {
    page_order_count: usize,
    known_node_count: usize,
}

/// Applies only the relative order already proven by source page-paint receipts.
///
/// pub-layout deliberately canonicalizes scene nodes by identity and does not
/// assign paint semantics to vector order. At the Viewer consumer boundary we
/// may restore the persisted OfficeArt order for the direct source-backed nodes
/// covered by ViewerPagePaintOrderV1. Nodes outside that bounded authority keep
/// their exact slots, so no relative ordering is invented for projected,
/// inherited, grouped, or otherwise unsupported classes.
fn apply_known_source_page_paint_orders_to_scene_nodes_v1(
    nodes: &mut [ResolvedPhysicalNode],
    source_orders: &[ViewerPagePaintOrderV1],
) -> ViewerSourcePaintOrderApplicationStatsV1 {
    let mut stats = ViewerSourcePaintOrderApplicationStatsV1::default();

    for order in source_orders {
        let mut rank = BTreeMap::<NodeId, usize>::new();
        let mut invalid_order = false;
        for (stack_rank, node_id) in order.node_ids.iter().copied().enumerate() {
            if rank.insert(node_id, stack_rank).is_some() {
                invalid_order = true;
                break;
            }
        }
        if invalid_order || rank.is_empty() {
            continue;
        }

        let expected_parent = order.page_id.into_canonical();
        let mut seen = BTreeSet::<NodeId>::new();
        let mut slots = Vec::new();
        let mut covered = Vec::new();
        for (slot, node) in nodes.iter().enumerate() {
            let Some(stack_rank) = rank.get(&node.origin).copied() else {
                continue;
            };
            if node.parent_origin != expected_parent || !seen.insert(node.origin) {
                invalid_order = true;
                break;
            }
            slots.push(slot);
            covered.push((stack_rank, node.clone()));
        }
        if invalid_order || covered.is_empty() {
            continue;
        }

        covered.sort_by_key(|(stack_rank, _)| *stack_rank);
        let covered_len = covered.len();
        for (slot, (_, node)) in slots.into_iter().zip(covered) {
            nodes[slot] = node;
        }

        stats.page_order_count += 1;
        stats.known_node_count += covered_len;
    }

    stats
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerOpenBundle {
    pub geometry: ViewerGeometryDocument,
    pub resolved_graph: PubResolvedGraph,
    /// Bounded source-backed page stacking authority. Missing pages remain
    /// explicitly unknown to consumers.
    pub source_page_paint_orders: Vec<ViewerPagePaintOrderV1>,
}

impl ViewerGeometryDocument {
    /// Reprojects Viewer Story text and bounded frame fragments from the current
    /// resolved graph without reparsing the immutable source PUB.
    ///
    /// This is the authoring-to-Viewer synchronization seam: EditorSession owns
    /// canonical mutations, while the Viewer continues to consume the same
    /// source-neutral pub-layout text-flow authority used during initial open.
    /// The update is transactional: on projection failure the existing Viewer
    /// text state is left untouched.
    pub fn refresh_text_projection_from_resolved(
        &mut self,
        graph: &PubResolvedGraph,
    ) -> Result<()> {
        if graph.source.source_hash != self.document.source.source_hash
            || graph.document.source_hash != self.document.source.source_hash
        {
            return Err(anyhow!(
                "Viewer text refresh rejected a resolved graph with different source identity"
            ));
        }

        let effective_page_ids = self
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        let authoring = bounded_authoring_slice_from_resolved_pages(graph, &effective_page_ids)?;
        let projection = project_bounded(authoring);
        let (text_fragments, text_flow_diagnostics) = resolve_viewer_text_fragments(&projection)?;
        let (tables, table_diagnostics) = viewer_tables_from_resolved(graph, &projection);

        let stories = graph
            .stories
            .values()
            .map(|story| ViewerStory {
                id: story.id,
                text: story.text.clone(),
            })
            .collect::<Vec<_>>();
        let story_frames = projection
            .story_frames
            .iter()
            .map(|frame| viewer_story_frame_from_projection(frame, graph))
            .collect::<Vec<_>>();

        let mut diagnostics = self.document.diagnostics.clone();
        diagnostics
            .retain(|diagnostic| !is_refreshable_text_flow_diagnostic(diagnostic.code.as_str()));
        diagnostics.extend(text_flow_diagnostics.iter().map(map_scene_diagnostic));
        diagnostics.extend(table_diagnostics);
        if !text_fragments.is_empty() {
            diagnostics.push(viewer_fallback_flow_metrics_diagnostic());
        }
        normalize_diagnostics(&mut diagnostics);

        self.document.stories = stories;
        self.story_frames = story_frames;
        self.text_fragments = text_fragments;
        self.tables = tables;
        self.document.diagnostics = diagnostics;
        Ok(())
    }
    /// Synchronizes the bounded author-created direct page-local TextFrame class
    /// into the current Viewer scene without rebuilding or deleting unrelated
    /// projected/inherited scene instances.
    ///
    /// The active IDs must come from a higher-layer durable creation proof
    /// (currently applied CreateTextBox operations). The previous IDs are
    /// transient product state used only to remove this same managed class on
    /// Undo/reopen; they are never treated as authoring truth.
    pub fn sync_editor_created_text_box_scene_nodes(
        &mut self,
        graph: &PubResolvedGraph,
        active_node_ids: &[NodeId],
        previously_synced_node_ids: &BTreeSet<NodeId>,
    ) -> Result<BTreeSet<NodeId>> {
        if graph.source.source_hash != self.document.source.source_hash
            || graph.document.source_hash != self.document.source.source_hash
        {
            return Err(anyhow!(
                "Viewer created-node scene sync rejected a resolved graph with different source identity"
            ));
        }

        let mut seen = BTreeSet::new();
        for node_id in active_node_ids {
            if !seen.insert(*node_id) {
                return Err(anyhow!(
                    "Viewer created-node scene sync received duplicate NodeId {}",
                    node_id.as_canonical()
                ));
            }
        }

        let surface_ids = self
            .scene
            .surfaces
            .iter()
            .map(|surface| surface.origin.into_canonical())
            .collect::<BTreeSet<_>>();

        let mut next_nodes = self
            .scene
            .nodes
            .iter()
            .filter(|node| !previously_synced_node_ids.contains(&node.origin))
            .cloned()
            .collect::<Vec<_>>();

        for node_id in active_node_ids {
            let node = graph.nodes.get(node_id).ok_or_else(|| {
                anyhow!(
                    "Viewer created-node scene sync missing current graph node {}",
                    node_id.as_canonical()
                )
            })?;
            if node.kind != NodeKind::TextFrame
                || !node.header.source_refs.is_empty()
                || node.header.transform != Affine2D::identity()
                || node.header.bounds.width.get() <= 0
                || node.header.bounds.height.get() <= 0
                || node.header.bounds.right().is_none()
                || node.header.bounds.bottom().is_none()
            {
                return Err(anyhow!(
                    "Viewer created-node scene sync rejected unsupported TextFrame {}",
                    node_id.as_canonical()
                ));
            }
            if !surface_ids.contains(&node.header.parent_id) {
                return Err(anyhow!(
                    "Viewer created-node scene sync rejected non-page parent for {}",
                    node_id.as_canonical()
                ));
            }
            let parent_page = graph
                .pages
                .values()
                .find(|page| page.id.as_canonical() == &node.header.parent_id)
                .ok_or_else(|| {
                    anyhow!(
                        "Viewer created-node scene sync could not resolve parent page for {}",
                        node_id.as_canonical()
                    )
                })?;
            if !parent_page.children.contains(node_id) {
                return Err(anyhow!(
                    "Viewer created-node scene sync rejected missing page-child membership for {}",
                    node_id.as_canonical()
                ));
            }
            let story_id = node
                .payload
                .story_frame
                .as_ref()
                .and_then(|frame| frame.story_id)
                .ok_or_else(|| {
                    anyhow!(
                        "Viewer created-node scene sync requires one StoryFrame owner for {}",
                        node_id.as_canonical()
                    )
                })?;
            if !graph.stories.contains_key(&story_id) {
                return Err(anyhow!(
                    "Viewer created-node scene sync missing Story {} for TextFrame {}",
                    story_id.as_canonical(),
                    node_id.as_canonical()
                ));
            }
            if next_nodes
                .iter()
                .any(|scene_node| scene_node.origin == *node_id)
            {
                return Err(anyhow!(
                    "Viewer created-node scene sync collided with an unrelated scene node {}",
                    node_id.as_canonical()
                ));
            }

            next_nodes.push(ResolvedPhysicalNode {
                origin: *node_id,
                parent_origin: node.header.parent_id,
                bounds: node.header.bounds,
                transform: node.header.transform.clone(),
            });
        }

        self.scene.nodes = next_nodes;
        Ok(seen)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTable {
    pub node_id: NodeId,
    pub story_id: StoryId,
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<ViewerTableCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub borders: Vec<ViewerTableBorderSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTableBorderSegment {
    pub x1_emu: i64,
    pub y1_emu: i64,
    pub x2_emu: i64,
    pub y2_emu: i64,
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTableCell {
    pub id: TableCellId,
    pub address: TableCellAddress,
    #[serde(
        default = "default_table_span",
        skip_serializing_if = "table_span_is_one"
    )]
    pub row_span: u32,
    #[serde(
        default = "default_table_span",
        skip_serializing_if = "table_span_is_one"
    )]
    pub column_span: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectEmu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_visible: Option<bool>,
}

fn viewer_table_border_segments(
    source: &pub_reader::PubTableSource,
    cells: &[ViewerTableCell],
) -> Vec<ViewerTableBorderSegment> {
    let boundary_x = |column: u32| -> Option<i64> {
        if column > source.columns || source.columns == 0 {
            return None;
        }
        if column == source.columns {
            let cell = cells
                .iter()
                .find(|cell| cell.address.row == 0 && cell.address.column + 1 == source.columns)?;
            let bounds = cell.bounds?;
            return bounds.x.get().checked_add(bounds.width.get());
        }
        cells
            .iter()
            .find(|cell| cell.address.row == 0 && cell.address.column == column)
            .and_then(|cell| cell.bounds)
            .map(|bounds| bounds.x.get())
    };
    let boundary_y = |row: u32| -> Option<i64> {
        if row > source.rows || source.rows == 0 {
            return None;
        }
        if row == source.rows {
            let cell = cells
                .iter()
                .find(|cell| cell.address.column == 0 && cell.address.row + 1 == source.rows)?;
            let bounds = cell.bounds?;
            return bounds.y.get().checked_add(bounds.height.get());
        }
        cells
            .iter()
            .find(|cell| cell.address.column == 0 && cell.address.row == row)
            .and_then(|cell| cell.bounds)
            .map(|bounds| bounds.y.get())
    };

    source
        .border_segments
        .iter()
        .filter_map(|segment| match segment.axis {
            pub_reader::PubTableBorderAxis::Horizontal => {
                let y = boundary_y(segment.row_start)?;
                let x1 = boundary_x(segment.column_start)?;
                let x2 = boundary_x(segment.column_end)?;
                (x1 < x2).then_some(ViewerTableBorderSegment {
                    x1_emu: x1,
                    y1_emu: y,
                    x2_emu: x2,
                    y2_emu: y,
                    rgb: segment.rgb,
                    width_emu: segment.width_emu,
                })
            }
            pub_reader::PubTableBorderAxis::Vertical => {
                let x = boundary_x(segment.column_start)?;
                let y1 = boundary_y(segment.row_start)?;
                let y2 = boundary_y(segment.row_end)?;
                (y1 < y2).then_some(ViewerTableBorderSegment {
                    x1_emu: x,
                    y1_emu: y1,
                    x2_emu: x,
                    y2_emu: y2,
                    rgb: segment.rgb,
                    width_emu: segment.width_emu,
                })
            }
        })
        .collect()
}

fn default_table_span() -> u32 {
    1
}

fn table_span_is_one(value: &u32) -> bool {
    *value == 1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerNodePaint {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_shape: Option<ViewerPresetShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_line: Option<ViewerSolidLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerPresetShape {
    RoundRect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerSolidLine {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

/// A semantic TABLE is not an ordinary Shape paint surface.
///
/// The generic OfficeArt owner fill is preserved upstream as source evidence,
/// but it is not authority for TABLE cell/background paint. Keep the line for
/// now; dedicated per-cell fill/border semantics remain owned by the TABLE
/// paint path.
fn fence_semantic_table_container_fill(
    is_semantic_table: bool,
    mut paint: ViewerNodePaint,
) -> ViewerNodePaint {
    if is_semantic_table {
        paint.solid_fill_rgb = None;
    }
    paint
}

fn bridge_effective_authority(
    authority: PubEffectivePaintAuthority,
) -> PubEffectivePaintAuthorityV1 {
    match authority {
        PubEffectivePaintAuthority::ShapeLocal => PubEffectivePaintAuthorityV1::ShapeLocal,
        PubEffectivePaintAuthority::DrawingGroupPrimary => {
            PubEffectivePaintAuthorityV1::DrawingGroupPrimary
        }
        PubEffectivePaintAuthority::DrawingGroupTertiary => {
            PubEffectivePaintAuthorityV1::DrawingGroupTertiary
        }
        PubEffectivePaintAuthority::NormativeDefault => {
            PubEffectivePaintAuthorityV1::NormativeDefault
        }
    }
}

fn bridge_effective_value<T: Clone>(
    value: &pub_reader::PubEffectivePaintValue<T>,
) -> PubEffectivePaintValueV1<T> {
    PubEffectivePaintValueV1 {
        value: value.value.clone(),
        authority: bridge_effective_authority(value.authority),
        source: value
            .source
            .as_ref()
            .map(|source| PubEffectivePaintSourceSpanV1 {
                stream: source.stream.0.clone(),
                offset: source.offset,
                len: source.len,
            }),
    }
}

fn viewer_preset_shape_from_canonical(
    node: &Node<PubResolvedNodePayload>,
) -> Option<ViewerPresetShape> {
    node.header
        .source_refs
        .iter()
        .any(|source_ref| {
            source_ref.path.as_deref() == Some("SpContainer/FSP/default-roundrect")
                && source_ref.authority == AuthorityClass::Authoritative
                && source_ref.confidence == Some(ReadConfidence::Exact)
                && matches!(source_ref.role, SourceRole::Projection)
        })
        .then_some(ViewerPresetShape::RoundRect)
}

fn viewer_node_paint_from_canonical_bridge(
    node: &Node<PubResolvedNodePayload>,
) -> Result<Option<ViewerNodePaint>> {
    if let Some(effective) = node.payload.effective_paint.as_ref() {
        let source = PubEffectiveShapePaintSourceV1 {
            fill: PubEffectiveFillSourceV1 {
                solid: effective.fill.solid.as_ref().map(bridge_effective_value),
                color_rgb: effective
                    .fill
                    .color_rgb
                    .as_ref()
                    .map(bridge_effective_value),
                visible: effective.fill.visible.as_ref().map(bridge_effective_value),
            },
            line: PubEffectiveLineSourceV1 {
                color_rgb: effective
                    .line
                    .color_rgb
                    .as_ref()
                    .map(bridge_effective_value),
                width_emu: effective
                    .line
                    .width_emu
                    .as_ref()
                    .map(bridge_effective_value),
                visible: effective.line.visible.as_ref().map(bridge_effective_value),
            },
        };
        return Ok(
            project_effective_source_paint_to_viewer_v1(&source).map(|paint| {
                fence_semantic_table_container_fill(
                    node.payload.table.is_some(),
                    ViewerNodePaint {
                        node_id: node.header.id,
                        preset_shape: viewer_preset_shape_from_canonical(node),
                        solid_fill_rgb: paint.solid_fill_rgb,
                        solid_line: paint.solid_line.map(|line| ViewerSolidLine {
                            rgb: line.rgb,
                            width_emu: line.width_emu,
                        }),
                    },
                )
            }),
        );
    }

    let source_ref = node.header.source_refs.iter().find(|source_ref| {
        source_ref.path.as_deref() == Some("SpContainer/FOPT")
            && source_ref.authority == AuthorityClass::Authoritative
            && source_ref.confidence == Some(ReadConfidence::Exact)
            && matches!(
                source_ref.role,
                SourceRole::Semantic | SourceRole::Projection
            )
    });
    let Some(source_ref) = source_ref else {
        return Ok(None);
    };

    let source = PubExplicitShapePaintSourceV1 {
        fill: PubExplicitFillSourceV1 {
            solid: node.payload.explicit_paint.fill.solid,
            color_rgb: node.payload.explicit_paint.fill.color_rgb,
            visible: node.payload.explicit_paint.fill.visible,
        },
        line: PubExplicitLineSourceV1 {
            color_rgb: node.payload.explicit_paint.line.color_rgb,
            width_emu: node.payload.explicit_paint.line.width_emu,
            visible: node.payload.explicit_paint.line.visible,
        },
    };
    let provenance = PubPaintSourceProvenanceV1 {
        format: source_ref.format.clone(),
        adapter_version: source_ref.adapter_version.clone(),
        source_hash_hex: source_ref.source_hash.to_string(),
        carrier: source_ref.carrier.clone(),
        object_key: source_ref.object_key.clone(),
        path: source_ref.path.clone(),
        role: match source_ref.role {
            SourceRole::Semantic => PubPaintSourceRoleV1::Semantic,
            SourceRole::Projection => PubPaintSourceRoleV1::Projection,
            _ => return Ok(None),
        },
    };

    let projected = project_explicit_source_paint_to_viewer_v1(&source, provenance)
        .map_err(|error| anyhow!("canonical paint bridge rejected Viewer node paint: {error:?}"))?;

    Ok(projected.map(|paint| {
        fence_semantic_table_container_fill(
            node.payload.table.is_some(),
            ViewerNodePaint {
                node_id: node.header.id,
                preset_shape: viewer_preset_shape_from_canonical(node),
                solid_fill_rgb: paint.solid_fill_rgb,
                solid_line: paint.solid_line.map(|line| ViewerSolidLine {
                    rgb: line.rgb,
                    width_emu: line.width_emu,
                }),
            },
        )
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerStoryFrame {
    pub story_id: StoryId,
    pub frame_id: NodeId,
    pub ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_content_bounds: Option<RectEmu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_alignment: Option<ViewerTextVerticalAlignment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerTextVerticalAlignment {
    Top,
    Center,
    Bottom,
}

fn uniform_text_content_bounds(bounds: RectEmu, inset_emu: u32) -> Option<RectEmu> {
    let inset = i64::from(inset_emu);
    let double = inset.checked_mul(2)?;
    let width = bounds.width.get().checked_sub(double)?;
    let height = bounds.height.get().checked_sub(double)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(RectEmu::new(
        LengthEmu::new(bounds.x.get().checked_add(inset)?),
        LengthEmu::new(bounds.y.get().checked_add(inset)?),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

fn viewer_story_frame_from_projection(
    frame: &ProjectedStoryFrame,
    graph: &PubResolvedGraph,
) -> ViewerStoryFrame {
    let node = graph.nodes.get(&frame.frame_origin);
    let text_content_bounds = node
        .and_then(|node| {
            node.payload
                .text_frame_inset
                .as_ref()
                .map(|inset| (node, inset))
        })
        .and_then(|(node, inset)| {
            uniform_text_content_bounds(node.header.bounds, inset.uniform_emu)
        });
    let vertical_alignment = node
        .and_then(|node| node.payload.story_frame.as_ref())
        .and_then(|frame| frame.vertical_alignment)
        .map(|alignment| match alignment {
            PubTextFrameVerticalAlignment::Top => ViewerTextVerticalAlignment::Top,
            PubTextFrameVerticalAlignment::Center => ViewerTextVerticalAlignment::Center,
            PubTextFrameVerticalAlignment::Bottom => ViewerTextVerticalAlignment::Bottom,
        });

    ViewerStoryFrame {
        story_id: frame.story_origin,
        frame_id: frame.frame_origin,
        ordinal: frame.ordinal,
        text_content_bounds,
        vertical_alignment,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTextFragment {
    pub story_id: StoryId,
    pub frame_id: NodeId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub line_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerScriptFontEntryDisposition {
    Resolved,
    UnresolvedSentinel,
    InvalidFontOrdinal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerScriptFontEntry {
    pub script_slot: u16,
    pub source_font_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_name: Option<String>,
    pub disposition: ViewerScriptFontEntryDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerScriptFontMap {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub entries: Vec<ViewerScriptFontEntry>,
    pub source_story_text_sha256: Sha256Digest,
}

impl ViewerScriptFontMap {
    pub fn applies_to_story_text(&self, text: &str) -> bool {
        self.source_story_text_sha256 == viewer_story_text_sha256(text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerParagraphAlignment {
    Center,
    Right,
    InterWord,
    Distribute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTextColorRun {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub color_index: u32,
    pub raw_reference: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rgb: Option<[u8; 3]>,
    pub source_story_text_sha256: Sha256Digest,
}

impl ViewerTextColorRun {
    pub fn applies_to_story_text(&self, text: &str) -> bool {
        self.source_story_text_sha256 == viewer_story_text_sha256(text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerParagraphAlignmentRun {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub alignment: ViewerParagraphAlignment,
    pub source_value: u16,
    pub source_story_text_sha256: Sha256Digest,
}

impl ViewerParagraphAlignmentRun {
    pub fn applies_to_story_text(&self, text: &str) -> bool {
        self.source_story_text_sha256 == viewer_story_text_sha256(text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTypographyRun {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub source_font_name: String,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
    pub source_story_text_sha256: Sha256Digest,
}

impl ViewerTypographyRun {
    pub fn applies_to_story_text(&self, text: &str) -> bool {
        self.source_story_text_sha256 == viewer_story_text_sha256(text)
    }
}

pub fn viewer_story_text_sha256(text: &str) -> Sha256Digest {
    let digest = Sha256::digest(text.as_bytes());
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Sha256Digest::from_bytes(bytes)
}

pub const VIEWER_IMAGE_SOURCE_Q16_ONE: i64 = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImageSourceWindowV1 {
    /// Normalized source-image viewport edges in signed Q16 units.
    ///
    /// Values may extend outside 0..1 for Publisher Fit/pan states. The
    /// backend clips the persisted source window against the real image
    /// domain instead of clamping the crop itself.
    pub left_q16: i64,
    pub top_q16: i64,
    pub right_q16: i64,
    pub bottom_q16: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImageRecolorV1 {
    pub target_rgb: [u8; 3],
    pub preserve_grays: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImagePlacementV1 {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_window: Option<ViewerImageSourceWindowV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recolor: Option<ViewerImageRecolorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerEmbeddedImage {
    pub resource_id: ResourceId,
    pub mime: String,
    pub node_ids: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<ViewerImagePlacementV1>,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerSource {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_version: Option<String>,
    pub source_hash: Sha256Digest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerPage {
    /// One-based document order, independent from source directory numbering.
    pub index: u32,
    pub id: PageId,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerStory {
    pub id: StoryId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTextMatch {
    pub story_id: StoryId,
    pub start_byte: u64,
    pub end_byte: u64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerFidelityStatus {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerDiagnosticSeverity {
    Info,
    FidelityWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerDiagnostic {
    /// Stable product-facing code. It must not encode source byte offsets or
    /// parser-private carrier names.
    pub code: String,
    pub severity: ViewerDiagnosticSeverity,
    pub message: String,
}

fn encode_wmf_preview_png(preview: &WmfPreviewRgba) -> Result<Vec<u8>> {
    let pixel_count = u64::from(preview.width)
        .checked_mul(u64::from(preview.height))
        .ok_or_else(|| anyhow!("WMF preview pixel count overflow"))?;
    let expected_len = usize::try_from(
        pixel_count
            .checked_mul(4)
            .ok_or_else(|| anyhow!("WMF preview RGBA byte count overflow"))?,
    )
    .map_err(|_| anyhow!("WMF preview RGBA byte count does not fit address space"))?;
    if preview.width == 0 || preview.height == 0 || preview.rgba.len() != expected_len {
        return Err(anyhow!("WMF preview RGBA buffer is inconsistent"));
    }

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, preview.width, preview.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .context("write WMF preview PNG header")?;
        writer
            .write_image_data(&preview.rgba)
            .context("write WMF preview PNG samples")?;
        writer.finish().context("finalize WMF preview PNG")?;
    }
    if encoded.len() > MAX_LEGACY_OLE_PREVIEW_PNG_BYTES {
        return Err(anyhow!("WMF preview PNG exceeds bounded size"));
    }
    Ok(encoded)
}

fn legacy_ole_preview_resource_id(
    source_hash: &Sha256Digest,
    storage_number: u16,
    wmf_bytes: &[u8],
) -> Result<ResourceId> {
    let digest = Sha256::digest(wmf_bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut wmf_sha256 = String::with_capacity(digest.len() * 2);
    for byte in digest {
        wmf_sha256.push(char::from(HEX[usize::from(byte >> 4)]));
        wmf_sha256.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let source_object_key = format!(
        "legacy-ole-preview/object-{storage_number}/wmf-sha256-{wmf_sha256}/{}",
        LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &source_object_key,
        semantic_role: "viewer.legacy-ole-preview-v1",
    })
    .map_err(|error| anyhow!("derive legacy OLE preview resource identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

fn viewer_legacy_ole_preview_image_from_scan(
    source_hash: &Sha256Digest,
    node_ids: &[NodeId],
    storage_number: u16,
    scan: &LegacyOleCachedPresentationScan,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Option<ViewerEmbeddedImage> {
    if node_ids.is_empty() {
        return None;
    }

    if !scan.diagnostics.is_empty() {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.legacy_ole.preview_sibling_rejected".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} persisted cached OLE presentation sibling(s) were rejected by bounded validation; source carrier details remain private.",
                scan.diagnostics.len()
            ),
        });
    }

    let presentation = match select_unambiguous_legacy_ole_cached_presentation(scan) {
        LegacyOleCachedPresentationSelection::None => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_ole.preview_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "No validated persisted cached OLE presentation is available for this inert object."
                        .to_owned(),
            });
            return None;
        }
        LegacyOleCachedPresentationSelection::Ambiguous { candidate_count } => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_ole.preview_ambiguous".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "{candidate_count} distinct validated cached OLE presentations are available; Viewer V1 does not invent a sibling-selection rule."
                ),
            });
            return None;
        }
        LegacyOleCachedPresentationSelection::Selected {
            presentation,
            equivalent_candidate_count,
        } => {
            if equivalent_candidate_count > 1 {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.legacy_ole.preview_equivalent_duplicates".to_owned(),
                    severity: ViewerDiagnosticSeverity::Info,
                    message: format!(
                        "{} additional cached OLE presentation sibling(s) are metadata-and-byte equivalent to the selected preview.",
                        equivalent_candidate_count - 1
                    ),
                });
            }
            presentation
        }
    };

    let preview = match rasterize_wmf_preview(
        &presentation.data,
        presentation.width,
        presentation.height,
    ) {
        Ok(preview) => preview,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_ole.preview_raster_unsupported".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "The selected cached OLE presentation is structurally valid but outside the bounded Viewer raster profile."
                        .to_owned(),
            });
            return None;
        }
    };
    let png = match encode_wmf_preview_png(&preview) {
        Ok(png) => png,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_ole.preview_encode_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "The bounded cached OLE preview could not be materialized as a Viewer image resource."
                        .to_owned(),
            });
            return None;
        }
    };
    let resource_id = match legacy_ole_preview_resource_id(
        source_hash,
        storage_number,
        &presentation.data,
    ) {
        Ok(resource_id) => resource_id,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_ole.preview_identity_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "The bounded cached OLE preview could not receive a deterministic Viewer resource identity."
                        .to_owned(),
            });
            return None;
        }
    };

    Some(ViewerEmbeddedImage {
        resource_id,
        mime: "image/png".to_owned(),
        node_ids: node_ids.to_vec(),
        placements: Vec::new(),
        bytes: png,
    })
}

fn legacy_image_preview_resource_id(
    source_hash: &Sha256Digest,
    image_object_id: u16,
    wmf_bytes: &[u8],
) -> Result<ResourceId> {
    let digest = Sha256::digest(wmf_bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut wmf_sha256 = String::with_capacity(digest.len() * 2);
    for byte in digest {
        wmf_sha256.push(char::from(HEX[usize::from(byte >> 4)]));
        wmf_sha256.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let source_object_key = format!(
        "legacy-image-preview/object-{image_object_id}/wmf-sha256-{wmf_sha256}/{}",
        LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &source_object_key,
        semantic_role: "viewer.legacy-image-preview-v1",
    })
    .map_err(|error| anyhow!("derive legacy IMAGE preview resource identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

fn legacy_image_raster_hints(width_emu: i64, height_emu: i64) -> Option<(u32, u32)> {
    let width = u64::try_from(width_emu).ok()?;
    let height = u64::try_from(height_emu).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let max_dimension = width.max(height);
    let limit = u64::from(u32::MAX);
    let divisor = if max_dimension <= limit {
        1
    } else {
        max_dimension.checked_add(limit - 1)?.checked_div(limit)?
    };
    let width = u32::try_from((width / divisor).max(1)).ok()?;
    let height = u32::try_from((height / divisor).max(1)).ok()?;
    Some((width, height))
}

fn viewer_legacy_image_preview_images(
    bytes: &[u8],
    source_hash: &Sha256Digest,
    graph: &PubResolvedGraph,
    scene: &BoundedResolvedScene,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Vec<ViewerEmbeddedImage> {
    let renderable_node_ids = scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut nodes = Vec::new();
    let mut object_ids = Vec::new();

    for node in graph.nodes.values() {
        if node.kind != NodeKind::ImageFrame || !renderable_node_ids.contains(&node.header.id) {
            continue;
        }
        let Ok(image_object_id) = u16::try_from(node.payload.contents_seq_num) else {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_image.identity_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "A grounded legacy IMAGE frame has an object identity outside the old-0x22 range."
                        .to_owned(),
            });
            continue;
        };
        object_ids.push(image_object_id);
        nodes.push((image_object_id, node));
    }

    if object_ids.is_empty() {
        return Vec::new();
    }
    object_ids.sort_unstable();
    object_ids.dedup();

    let wmfs = match read_legacy_0x22_image_wmfs(Cursor::new(bytes), &object_ids) {
        Ok(wmfs) => wmfs,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_image.native_wmf_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "Grounded legacy IMAGE frames could not be re-materialized from their direct bounded native WMF payloads."
                        .to_owned(),
            });
            return Vec::new();
        }
    };

    let mut images = Vec::new();
    for (image_object_id, node) in nodes {
        let Some(wmf) = wmfs.get(&image_object_id) else {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_image.native_wmf_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "A grounded legacy IMAGE frame has no re-materialized bounded native WMF payload."
                        .to_owned(),
            });
            continue;
        };
        let Some((width_hint, height_hint)) = legacy_image_raster_hints(
            node.header.bounds.width.get(),
            node.header.bounds.height.get(),
        ) else {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.legacy_image.bounds_unsupported".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "A grounded legacy IMAGE frame has bounds outside the bounded preview raster profile."
                        .to_owned(),
            });
            continue;
        };
        let preview = match rasterize_wmf_preview(wmf, width_hint, height_hint) {
            Ok(preview) => preview,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.legacy_image.preview_raster_unsupported".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A grounded legacy IMAGE has a structurally valid native WMF outside the bounded Viewer raster profile."
                            .to_owned(),
                });
                continue;
            }
        };
        let png = match encode_wmf_preview_png(&preview) {
            Ok(png) => png,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.legacy_image.preview_encode_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded legacy IMAGE preview could not be encoded as a Viewer image resource."
                            .to_owned(),
                });
                continue;
            }
        };
        let resource_id = match legacy_image_preview_resource_id(source_hash, image_object_id, wmf)
        {
            Ok(resource_id) => resource_id,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.legacy_image.preview_identity_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded legacy IMAGE preview could not receive a deterministic Viewer resource identity."
                            .to_owned(),
                });
                continue;
            }
        };
        images.push(ViewerEmbeddedImage {
            resource_id,
            mime: "image/png".to_owned(),
            node_ids: vec![node.header.id],
            placements: Vec::new(),
            bytes: png,
        });
    }

    images
}

fn mature_officeart_wmf_preview_resource_id(
    source_hash: &Sha256Digest,
    slot: u32,
    wmf_bytes: &[u8],
) -> Result<ResourceId> {
    let digest = Sha256::digest(wmf_bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut wmf_sha256 = String::with_capacity(digest.len() * 2);
    for byte in digest {
        wmf_sha256.push(char::from(HEX[usize::from(byte >> 4)]));
        wmf_sha256.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let source_object_key = format!(
        "mature-officeart-wmf-preview/slot-{slot}/wmf-sha256-{wmf_sha256}/{}/{}",
        MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1, LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1,
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &source_object_key,
        semantic_role: "viewer.mature-officeart-wmf-preview-v1",
    })
    .map_err(|error| anyhow!("derive mature OfficeArt WMF preview identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

#[cfg(test)]
mod mature_officeart_wmf_preview_identity_tests {
    use super::*;

    #[test]
    fn preview_resource_identity_is_source_slot_and_wmf_bound() {
        let source_hash = Sha256Digest::from_bytes([0x11; 32]);
        let first = mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-a")
            .expect("preview resource id");
        let repeated =
            mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-a")
                .expect("repeat preview resource id");
        let other_slot =
            mature_officeart_wmf_preview_resource_id(&source_hash, 4, b"normalized-wmf-a")
                .expect("other slot preview resource id");
        let other_wmf =
            mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-b")
                .expect("other WMF preview resource id");

        assert_eq!(first, repeated);
        assert_ne!(first, other_slot);
        assert_ne!(first, other_wmf);
    }
}

fn viewer_mature_officeart_wmf_preview_images(
    bytes: &[u8],
    source_hash: &Sha256Digest,
    source: &PubSourceGraphBuild,
    resolved: &PubResolvedGraph,
    scene: &BoundedResolvedScene,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Vec<ViewerEmbeddedImage> {
    let bundle = match build_mature_0x2c_wmf_preview_bundle_from_bytes(bytes, &source.graph) {
        Ok(bundle) => bundle,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.mature_officeart_wmf.preview_source_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "Source-backed mature OfficeArt WMF images could not be materialized through the bounded preview bridge."
                        .to_owned(),
            });
            return Vec::new();
        }
    };

    if bundle.rejected_source_count > 0 {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_wmf.preview_source_rejected".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} mature OfficeArt WMF source(s) remain outside the bounded preview profile.",
                bundle.rejected_source_count
            ),
        });
    }

    let renderable_node_ids = scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut images = Vec::new();

    for preview_source in bundle.sources {
        let mut node_ids = preview_source
            .uses
            .iter()
            .map(|usage| usage.node_id)
            .filter(|node_id| renderable_node_ids.contains(node_id))
            .collect::<Vec<_>>();
        node_ids.sort();
        node_ids.dedup();
        if node_ids.is_empty() {
            continue;
        }

        let preview = match rasterize_wmf_preview(
            &preview_source.wmf_bytes,
            preview_source.width_hint,
            preview_source.height_hint,
        ) {
            Ok(preview) => preview,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_raster_unsupported".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A structurally valid mature OfficeArt WMF is outside the existing bounded Viewer raster profile."
                            .to_owned(),
                });
                continue;
            }
        };
        let png = match encode_wmf_preview_png(&preview) {
            Ok(png) => png,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_encode_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded mature OfficeArt WMF preview could not be encoded as a Viewer image resource."
                            .to_owned(),
                });
                continue;
            }
        };
        let resource_id = match mature_officeart_wmf_preview_resource_id(
            source_hash,
            preview_source.slot,
            &preview_source.wmf_bytes,
        ) {
            Ok(resource_id) => resource_id,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_identity_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded mature OfficeArt WMF preview could not receive a deterministic Viewer resource identity."
                            .to_owned(),
                });
                continue;
            }
        };

        let mut placements = Vec::new();
        for node_id in &node_ids {
            let source_window = match resolved.nodes.get(node_id).map(|node| {
                viewer_image_source_window_v1(node.payload.explicit_image_crop.as_ref())
            }) {
                Some(Ok(source_window)) => source_window,
                Some(Err(reason)) => {
                    diagnostics.push(ViewerDiagnostic {
                        code: "viewer.image.crop_partial".to_owned(),
                        severity: ViewerDiagnosticSeverity::FidelityWarning,
                        message: format!(
                            "Image crop for node {} is present but cannot be projected exactly ({reason}); the bounded WMF preview remains available as the full-image fallback.",
                            node_id.as_canonical()
                        ),
                    });
                    None
                }
                None => None,
            };
            let recolor = source.graph.nodes.get(node_id).and_then(|node| {
                viewer_image_recolor_v1(node.payload.explicit_image_recolor.as_ref())
            });
            if source_window.is_some() || recolor.is_some() {
                placements.push(ViewerImagePlacementV1 {
                    node_id: *node_id,
                    source_window,
                    recolor,
                });
            }
        }

        images.push(ViewerEmbeddedImage {
            resource_id,
            mime: "image/png".to_owned(),
            node_ids,
            placements,
            bytes: png,
        });
    }

    if !images.is_empty() {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_wmf.preview_applied".to_owned(),
            severity: ViewerDiagnosticSeverity::Info,
            message: format!(
                "{} bounded source-backed mature OfficeArt WMF preview resource(s) admitted through the shared Viewer image path.",
                images.len()
            ),
        });
    }

    images
}

fn viewer_legacy_ole_cached_preview_images(
    bytes: &[u8],
    source_hash: &Sha256Digest,
    graph: &PubResolvedGraph,
    scene: &BoundedResolvedScene,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Vec<ViewerEmbeddedImage> {
    let renderable_node_ids = scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut uses_by_storage = BTreeMap::<u16, Vec<NodeId>>::new();

    for node in graph.nodes.values() {
        if node.kind != NodeKind::Unsupported || !renderable_node_ids.contains(&node.header.id) {
            continue;
        }
        let Some(legacy_ole) = node.payload.legacy_ole.as_ref() else {
            continue;
        };
        uses_by_storage
            .entry(legacy_ole.storage_number)
            .or_default()
            .push(node.header.id);
    }

    let mut images = Vec::<ViewerEmbeddedImage>::new();
    for (storage_number, mut node_ids) in uses_by_storage {
        node_ids.sort();
        node_ids.dedup();

        let scan = match scan_legacy_ole_cached_presentations(Cursor::new(bytes), storage_number) {
            Ok(scan) => scan,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.legacy_ole.preview_scan_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "Persisted cached OLE presentations could not be scanned within bounded Reader limits for this inert object."
                            .to_owned(),
                });
                continue;
            }
        };

        if let Some(image) = viewer_legacy_ole_preview_image_from_scan(
            source_hash,
            &node_ids,
            storage_number,
            &scan,
            diagnostics,
        ) {
            images.push(image);
        }
    }

    images
}

fn viewer_page_paint_order_from_source(
    source: &PubSourcePagePaintOrderV1,
) -> ViewerPagePaintOrderV1 {
    ViewerPagePaintOrderV1 {
        page_id: source.page_id,
        node_ids: source.node_ids.clone(),
    }
}

struct Mature0x2cPipeline {
    source_hash: Sha256Digest,
    source: PubSourceGraphBuild,
    resolved: PubResolvedGraphBuild,
    page_selection: ViewerPageSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewerPageSelection {
    page_ids: Vec<PageId>,
    disposition: ViewerPageSelectionDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewerPageSelectionDisposition {
    GenericNoLoss,
    FamilyProfileApplied {
        profile_id: String,
        raw_page_count: usize,
        customer_page_count: usize,
    },
    FamilyProfileUnavailable {
        reason: String,
    },
}

/// Opens one mature 0x2C Publisher file into the read-only Viewer manifest.
///
/// This function never writes to the supplied bytes and does not construct a
/// writer/mutation plan. Unsupported or ambiguous source observations that do
/// not make the bounded adapter fail are translated to stable Viewer
/// diagnostics instead of being silently discarded.
fn viewer_image_source_window_v1(
    crop: Option<&PubExplicitImageCropSource>,
) -> Result<Option<ViewerImageSourceWindowV1>, &'static str> {
    let Some(crop) = crop else {
        return Ok(None);
    };
    if crop.ambiguous {
        return Err("ambiguous_crop_properties");
    }
    if crop.top_raw.is_none()
        && crop.bottom_raw.is_none()
        && crop.left_raw.is_none()
        && crop.right_raw.is_none()
    {
        return Ok(None);
    }

    let signed_q16 = |raw: Option<u32>| -> i64 { raw.map_or(0, |value| i64::from(value as i32)) };
    let left_q16 = signed_q16(crop.left_raw);
    let top_q16 = signed_q16(crop.top_raw);
    let right_q16 = VIEWER_IMAGE_SOURCE_Q16_ONE - signed_q16(crop.right_raw);
    let bottom_q16 = VIEWER_IMAGE_SOURCE_Q16_ONE - signed_q16(crop.bottom_raw);

    if right_q16 <= left_q16 || bottom_q16 <= top_q16 {
        return Err("non_positive_source_window");
    }
    if right_q16 <= 0
        || bottom_q16 <= 0
        || left_q16 >= VIEWER_IMAGE_SOURCE_Q16_ONE
        || top_q16 >= VIEWER_IMAGE_SOURCE_Q16_ONE
    {
        return Err("source_window_outside_image");
    }

    Ok(Some(ViewerImageSourceWindowV1 {
        left_q16,
        top_q16,
        right_q16,
        bottom_q16,
    }))
}

fn viewer_image_recolor_v1(
    recolor: Option<&pub_reader::PubExplicitImageRecolorSource>,
) -> Option<ViewerImageRecolorV1> {
    recolor.map(|recolor| ViewerImageRecolorV1 {
        target_rgb: recolor.target_rgb,
        preserve_grays: recolor.preserve_grays,
    })
}

pub fn open_mature_0x2c(bytes: &[u8]) -> Result<ViewerDocument> {
    let pipeline = build_mature_0x2c_pipeline(bytes)?;
    viewer_document_from_pipeline(bytes.len(), &pipeline)
}

pub fn open_pub_geometry(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerGeometryDocument> {
    Ok(open_pub_bundle(bytes, environment)?.geometry)
}

/// Product-level Reader open outcome.
///
/// Normal parsing always runs first. Salvage is returned only when bounded
/// surviving evidence can be projected into the shared source-neutral partial
/// graph. The outcome carries no repair plan or reconstructed document state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerProductOpenOutcome {
    Normal(Box<ViewerGeometryDocument>),
    Salvage(ReaderPartialSourceGraph),
}

/// Opens a Publisher source normally first, then attempts the conservative
/// intake-only damaged-file salvage path.
///
/// Valid known-PUB files that fail normal parsing are not automatically called
/// damaged: those require a typed ProvenStructuralCorruption trigger from a
/// stronger discriminator. This keeps unsupported grammar research separate
/// from recovery.
pub fn open_pub_or_salvage(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerProductOpenOutcome> {
    open_pub_or_salvage_with_trigger(bytes, environment, ReaderSalvageTrigger::IntakeOnly)
}

/// Same product fallback seam with an explicit upstream corruption trigger.
///
/// Callers may use ProvenStructuralCorruption only after a bounded typed
/// discriminator has established corruption independently of the fact that
/// normal Reader open failed.
pub fn open_pub_or_salvage_with_trigger(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
    trigger: ReaderSalvageTrigger,
) -> Result<ViewerProductOpenOutcome> {
    match open_pub_geometry(bytes, environment) {
        Ok(document) => Ok(ViewerProductOpenOutcome::Normal(Box::new(document))),
        Err(normal_error) => {
            let probe = probe_reader_salvage_candidate_with_trigger(bytes, trigger);
            if probe.eligibility.is_eligible() && probe.has_surviving_evidence() {
                match build_reader_partial_source_graph(bytes, &probe) {
                    Ok(graph) => Ok(ViewerProductOpenOutcome::Salvage(graph)),
                    Err(_) => Err(normal_error),
                }
            } else {
                Err(normal_error)
            }
        }
    }
}

/// Opens one Publisher source through the same family router as the Viewer and
/// returns both the product geometry projection and the canonical resolved graph.
///
/// This is the in-process seam for the future isolated parser worker: callers
/// can serialize the source-neutral resolved graph without reparsing the source
/// bytes or duplicating Publisher-family dispatch. Embedded image byte transport
/// remains a separate bounded IPC concern.
pub fn open_pub_bundle(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerOpenBundle> {
    let classification = classify_pub_family(bytes);
    match classification.route {
        PubReaderRoute::Mature2c => open_mature_0x2c_bundle(bytes, environment),
        PubReaderRoute::Legacy22Quill => open_legacy_0x22_quill_bundle(bytes, environment),
        PubReaderRoute::Legacy22LowText => open_legacy_0x22_noquill_bundle(bytes, environment),
        PubReaderRoute::Unsupported => Err(anyhow!(
            "unsupported PUB family/profile: family={:?}, profile={}, route={}",
            classification.family,
            classification.profile.as_str(),
            classification.route.as_str()
        )),
    }
}

/// Opens the bounded Publisher2/95/97 old-0x22 no-Quill profile.
///
/// V1 uses the grounded low-family Contents text range and owner-boundary map,
/// authoritative DOCUMENT/PAGE lists, and admitted text-box geometry. It does
/// not invent Quill, Escher, codepage semantics, or unsupported legacy objects.
pub fn open_legacy_0x22_noquill_geometry(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerGeometryDocument> {
    Ok(open_legacy_0x22_noquill_bundle(bytes, environment)?.geometry)
}

fn open_legacy_0x22_noquill_bundle(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerOpenBundle> {
    let source_hash = sha256_digest(bytes)?;
    let source = build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash)
        .context("build legacy-0x22 no-Quill PUB source graph for Viewer")?;
    let resolved = resolve_pub_source_graph(&source.graph)
        .context("resolve legacy no-Quill PUB source graph for Viewer")?;
    let page_selection = ViewerPageSelection {
        page_ids: source.effective_pages.page_ids.clone(),
        disposition: ViewerPageSelectionDisposition::GenericNoLoss,
    };
    let mut document = viewer_document_from_graph(
        bytes.len(),
        source_hash,
        &source,
        &resolved,
        &page_selection,
    )?;
    let effective_page_ids = document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let authoring = bounded_legacy_noquill_authoring_slice_from_resolved_pages(
        &resolved.graph,
        &effective_page_ids,
    )?;
    let projection = project_bounded(authoring);

    document
        .diagnostics
        .extend(projection.diagnostics.iter().map(map_projection_diagnostic));

    let paints = resolved
        .graph
        .nodes
        .values()
        .map(viewer_node_paint_from_canonical_bridge)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let story_frames = projection
        .story_frames
        .iter()
        .map(|frame| viewer_story_frame_from_projection(frame, &resolved.graph))
        .collect::<Vec<_>>();

    let (text_fragments, text_flow_diagnostics) = resolve_viewer_text_fragments(&projection)?;
    document
        .diagnostics
        .extend(text_flow_diagnostics.iter().map(map_scene_diagnostic));
    if !text_fragments.is_empty() {
        document
            .diagnostics
            .push(viewer_fallback_flow_metrics_diagnostic());
    }

    let scene = resolve_bounded_geometry(&projection, environment).map_err(|blocked| {
        let codes = blocked
            .projection_errors
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow!("legacy no-Quill Viewer geometry resolution blocked by layout projection errors: {codes}")
    })?;
    document.diagnostics.extend(
        scene
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != "story_text_layout_not_implemented")
            .map(map_scene_diagnostic),
    );

    let preview_source_hash = document.source.source_hash;
    let mut images = viewer_legacy_ole_cached_preview_images(
        bytes,
        &preview_source_hash,
        &resolved.graph,
        &scene,
        &mut document.diagnostics,
    );
    images.extend(viewer_legacy_image_preview_images(
        bytes,
        &preview_source_hash,
        &resolved.graph,
        &scene,
        &mut document.diagnostics,
    ));

    if !scene.nodes.is_empty() {
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.visual.geometry_only".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: "Legacy no-Quill text, grounded positive-bounds native-WMF IMAGE frames, and text-box geometry are recovered where evidence-backed. Exact source typography, non-ASCII codepages, signed/reversed legacy image placement, image containers without native data, effects and unsupported legacy object kinds remain explicit fidelity gaps; cached OLE previews are shown only when one persisted presentation validates."
                .to_owned(),
        });
    }
    normalize_diagnostics(&mut document.diagnostics);

    let geometry = ViewerGeometryDocument {
        schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
        document,
        scene,
        paints,
        story_frames,
        text_fragments,
        typography_runs: Vec::new(),
        paragraph_alignments: Vec::new(),
        text_color_runs: Vec::new(),
        script_font_maps: Vec::new(),
        tables: Vec::new(),
        #[cfg(feature = "cmo-slot-compose")]
        projected_instances: Vec::new(),
        images,
    };
    Ok(ViewerOpenBundle {
        geometry,
        resolved_graph: resolved.graph,
        source_page_paint_orders: Vec::new(),
    })
}

/// Opens the bounded Publisher98/2000 old-0x22 + Quill profile.
///
/// V1 materializes the authoritative DOCUMENT/PAGE graph, grounded Quill
/// stories, and ordinary text-box geometry. Unsupported old-family object
/// markers stay explicit fidelity diagnostics instead of being guessed.
pub fn open_legacy_0x22_quill_geometry(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerGeometryDocument> {
    Ok(open_legacy_0x22_quill_bundle(bytes, environment)?.geometry)
}

fn open_legacy_0x22_quill_bundle(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerOpenBundle> {
    let source_hash = sha256_digest(bytes)?;
    let source = build_legacy_0x22_quill_source_graph(Cursor::new(bytes), source_hash)
        .context("build legacy-0x22+Quill PUB source graph for Viewer")?;
    let resolved = resolve_pub_source_graph(&source.graph)
        .context("resolve legacy PUB source graph for Viewer")?;
    let page_selection = ViewerPageSelection {
        page_ids: source.effective_pages.page_ids.clone(),
        disposition: ViewerPageSelectionDisposition::GenericNoLoss,
    };
    let mut document = viewer_document_from_graph(
        bytes.len(),
        source_hash,
        &source,
        &resolved,
        &page_selection,
    )?;
    let effective_page_ids = document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let authoring =
        bounded_authoring_slice_from_resolved_pages(&resolved.graph, &effective_page_ids)?;
    let projection = project_bounded(authoring);

    document
        .diagnostics
        .extend(projection.diagnostics.iter().map(map_projection_diagnostic));

    let paints = resolved
        .graph
        .nodes
        .values()
        .map(viewer_node_paint_from_canonical_bridge)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let story_frames = projection
        .story_frames
        .iter()
        .map(|frame| viewer_story_frame_from_projection(frame, &resolved.graph))
        .collect::<Vec<_>>();

    let (text_fragments, text_flow_diagnostics) = resolve_viewer_text_fragments(&projection)?;
    document
        .diagnostics
        .extend(text_flow_diagnostics.iter().map(map_scene_diagnostic));
    if !text_fragments.is_empty() {
        document
            .diagnostics
            .push(viewer_fallback_flow_metrics_diagnostic());
    }

    let scene = resolve_bounded_geometry(&projection, environment).map_err(|blocked| {
        let codes = blocked
            .projection_errors
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow!("legacy Viewer geometry resolution blocked by layout projection errors: {codes}")
    })?;
    document.diagnostics.extend(
        scene
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != "story_text_layout_not_implemented")
            .map(map_scene_diagnostic),
    );

    let preview_source_hash = document.source.source_hash;
    let images = viewer_legacy_ole_cached_preview_images(
        bytes,
        &preview_source_hash,
        &resolved.graph,
        &scene,
        &mut document.diagnostics,
    );

    if !scene.nodes.is_empty() {
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.visual.geometry_only".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: "Legacy object positions and sizes are resolved for the admitted old-0x22 text-box profile. Unsupported legacy object kinds, exact source typography, ordinary legacy image classes, effects, groups and version-sensitive transforms remain outside this bounded path; cached OLE previews are shown only when one persisted presentation validates."
                .to_owned(),
        });
    }
    normalize_diagnostics(&mut document.diagnostics);

    let geometry = ViewerGeometryDocument {
        schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
        document,
        scene,
        paints,
        story_frames,
        text_fragments,
        typography_runs: Vec::new(),
        paragraph_alignments: Vec::new(),
        text_color_runs: Vec::new(),
        script_font_maps: Vec::new(),
        tables: Vec::new(),
        #[cfg(feature = "cmo-slot-compose")]
        projected_instances: Vec::new(),
        images,
    };
    Ok(ViewerOpenBundle {
        geometry,
        resolved_graph: resolved.graph,
        source_page_paint_orders: Vec::new(),
    })
}

/// Opens one mature 0x2C Publisher file through the real layout/scene boundary.
///
/// Current scope is deliberately geometry-only. The scene contains page
/// surfaces and physical node rectangles/transforms. Recovered text remains in
/// `ViewerDocument` for search/copy and the scene emits an explicit fidelity
/// warning that text layout is not yet painted.
pub fn open_mature_0x2c_geometry(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerGeometryDocument> {
    Ok(open_mature_0x2c_bundle(bytes, environment)?.geometry)
}

fn open_mature_0x2c_bundle(
    bytes: &[u8],
    environment: BoundedLayoutEnvironment,
) -> Result<ViewerOpenBundle> {
    let pipeline = build_mature_0x2c_pipeline(bytes)?;
    let mut document = viewer_document_from_pipeline(bytes.len(), &pipeline)?;
    let effective_page_ids = document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let authoring =
        bounded_authoring_slice_from_resolved_pages(&pipeline.resolved.graph, &effective_page_ids)?;
    let projection = project_bounded(authoring);

    document
        .diagnostics
        .extend(projection.diagnostics.iter().map(map_projection_diagnostic));

    let paints = pipeline
        .resolved
        .graph
        .nodes
        .values()
        .map(viewer_node_paint_from_canonical_bridge)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let story_frames = projection
        .story_frames
        .iter()
        .map(|frame| viewer_story_frame_from_projection(frame, &pipeline.resolved.graph))
        .collect::<Vec<_>>();

    let (text_fragments, text_flow_diagnostics) = resolve_viewer_text_fragments(&projection)?;
    document
        .diagnostics
        .extend(text_flow_diagnostics.iter().map(map_scene_diagnostic));
    if !text_fragments.is_empty() {
        document
            .diagnostics
            .push(viewer_fallback_flow_metrics_diagnostic());
    }

    let mut typography_runs = pipeline
        .source
        .typography_runs
        .iter()
        .filter_map(|run| {
            let story = pipeline.resolved.graph.stories.get(&run.story_id)?;
            Some(ViewerTypographyRun {
                story_id: run.story_id,
                scalar_start: run.story_scalar_start,
                scalar_end: run.story_scalar_end,
                source_font_name: run.source_font_name.clone(),
                text_size_emu: run.text_size_emu,
                font_inherited: run.font_inherited,
                size_inherited: run.size_inherited,
                source_story_text_sha256: viewer_story_text_sha256(&story.text),
            })
        })
        .collect::<Vec<_>>();
    typography_runs.extend(
        pipeline
            .source
            .typography_size_runs
            .iter()
            .filter_map(|run| {
                let story = pipeline.resolved.graph.stories.get(&run.story_id)?;
                Some(ViewerTypographyRun {
                    story_id: run.story_id,
                    scalar_start: run.story_scalar_start,
                    scalar_end: run.story_scalar_end,
                    source_font_name: String::new(),
                    text_size_emu: run.text_size_emu,
                    font_inherited: false,
                    size_inherited: run.size_inherited,
                    source_story_text_sha256: viewer_story_text_sha256(&story.text),
                })
            }),
    );
    typography_runs.sort_by_key(|run| (run.story_id, run.scalar_start, run.scalar_end));
    if !typography_runs.is_empty() {
        let inherited = typography_runs
            .iter()
            .filter(|run| run.font_inherited || run.size_inherited)
            .count();
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.text.source_typography_partial".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} source typography range(s) are available for preview sizing; {} use bounded inherited font/size authority. The renderer still uses the pinned fallback font face and does not claim Publisher-exact reflow.",
                typography_runs.len(),
                inherited
            ),
        });
    }

    let text_color_runs = pipeline
        .source
        .text_color_runs
        .iter()
        .filter_map(|run| {
            let story = pipeline.resolved.graph.stories.get(&run.story_id)?;
            Some(ViewerTextColorRun {
                story_id: run.story_id,
                scalar_start: run.story_scalar_start,
                scalar_end: run.story_scalar_end,
                color_index: run.color_index,
                raw_reference: run.raw_reference,
                rgb: run.direct_rgb,
                source_story_text_sha256: viewer_story_text_sha256(&story.text),
            })
        })
        .collect::<Vec<_>>();
    if !text_color_runs.is_empty() {
        let unresolved = text_color_runs
            .iter()
            .filter(|run| run.rgb.is_none())
            .count();
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.text.source_color_partial".to_owned(),
            severity: if unresolved == 0 {
                ViewerDiagnosticSeverity::Info
            } else {
                ViewerDiagnosticSeverity::FidelityWarning
            },
            message: format!(
                "{} source text-color range(s) are preserved; {} require palette/intensity resolution and remain Partial.",
                text_color_runs.len(),
                unresolved
            ),
        });
    }

    let paragraph_alignments = pipeline
        .source
        .paragraph_alignments
        .iter()
        .filter_map(|run| {
            let story = pipeline.resolved.graph.stories.get(&run.story_id)?;
            Some(ViewerParagraphAlignmentRun {
                story_id: run.story_id,
                scalar_start: run.story_scalar_start,
                scalar_end: run.story_scalar_end,
                alignment: match run.alignment {
                    PubParagraphAlignment::Center => ViewerParagraphAlignment::Center,
                    PubParagraphAlignment::Right => ViewerParagraphAlignment::Right,
                    PubParagraphAlignment::InterWord => ViewerParagraphAlignment::InterWord,
                    PubParagraphAlignment::Distribute => ViewerParagraphAlignment::Distribute,
                },
                source_value: run.source_value,
                source_story_text_sha256: viewer_story_text_sha256(&story.text),
            })
        })
        .collect::<Vec<_>>();
    if !paragraph_alignments.is_empty() {
        let unsupported = paragraph_alignments
            .iter()
            .filter(|run| {
                matches!(
                    run.alignment,
                    ViewerParagraphAlignment::InterWord | ViewerParagraphAlignment::Distribute
                )
            })
            .count();
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.text.paragraph_alignment_partial".to_owned(),
            severity: if unsupported == 0 {
                ViewerDiagnosticSeverity::Info
            } else {
                ViewerDiagnosticSeverity::FidelityWarning
            },
            message: format!(
                "{} explicit source paragraph-alignment range(s) are preserved; {} use InterWord/Distribute semantics that remain non-executable and stay Partial.",
                paragraph_alignments.len(),
                unsupported
            ),
        });
    }

    let script_font_maps = pipeline
        .source
        .script_font_maps
        .iter()
        .filter_map(|map| {
            let story = pipeline.resolved.graph.stories.get(&map.story_id)?;
            Some(ViewerScriptFontMap {
                story_id: map.story_id,
                scalar_start: map.story_scalar_start,
                scalar_end: map.story_scalar_end,
                entries: map
                    .entries
                    .iter()
                    .map(|entry| ViewerScriptFontEntry {
                        script_slot: entry.script_slot,
                        source_font_index: entry.source_font_index,
                        source_font_name: entry.source_font_name.clone(),
                        disposition: match entry.disposition {
                            PubScriptFontEntryDisposition::Resolved => {
                                ViewerScriptFontEntryDisposition::Resolved
                            }
                            PubScriptFontEntryDisposition::UnresolvedSentinel => {
                                ViewerScriptFontEntryDisposition::UnresolvedSentinel
                            }
                            PubScriptFontEntryDisposition::InvalidFontOrdinal => {
                                ViewerScriptFontEntryDisposition::InvalidFontOrdinal
                            }
                        },
                    })
                    .collect(),
                source_story_text_sha256: viewer_story_text_sha256(&story.text),
            })
        })
        .collect::<Vec<_>>();
    if !script_font_maps.is_empty() {
        let unresolved = script_font_maps
            .iter()
            .flat_map(|map| map.entries.iter())
            .filter(|entry| entry.disposition != ViewerScriptFontEntryDisposition::Resolved)
            .count();
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.text.script_font_map_preserved".to_owned(),
            severity: if unresolved == 0 {
                ViewerDiagnosticSeverity::Info
            } else {
                ViewerDiagnosticSeverity::FidelityWarning
            },
            message: format!(
                "{} source script-font map range(s) are preserved with exact source FONT ordinals; {} entrie(s) remain unresolved. The Reader does not yet use this map to change effective font selection.",
                script_font_maps.len(),
                unresolved
            ),
        });
    }

    let (tables, table_diagnostics) =
        viewer_tables_from_resolved(&pipeline.resolved.graph, &projection);
    document.diagnostics.extend(table_diagnostics);

    let mut images = match build_mature_0x2c_asset_export_bundle_from_bytes(
        bytes,
        &pipeline.source.graph,
    ) {
        Ok(bundle) => {
            for diagnostic in &bundle.manifest.diagnostics {
                match diagnostic {
                    PubAssetExportDiagnostic::AssetNotPromoted { .. } => {
                        document.diagnostics.push(ViewerDiagnostic {
                            code: "viewer.image.asset_not_exact".to_owned(),
                            severity: ViewerDiagnosticSeverity::FidelityWarning,
                            message: "An image placement exists, but exact embedded image bytes are not available for the Viewer overlay.".to_owned(),
                        });
                    }
                }
            }

            let mut images = Vec::new();
            for file in bundle.files {
                let Some(entry) = bundle
                    .manifest
                    .assets
                    .iter()
                    .find(|entry| entry.resource_id == file.resource_id)
                else {
                    continue;
                };

                let mut placements = Vec::with_capacity(entry.uses.len());
                for usage in &entry.uses {
                    let source_window = match pipeline.resolved.graph.nodes.get(&usage.node_id).map(
                        |node| {
                            viewer_image_source_window_v1(node.payload.explicit_image_crop.as_ref())
                        },
                    ) {
                        Some(Ok(source_window)) => source_window,
                        Some(Err(reason)) => {
                            document.diagnostics.push(ViewerDiagnostic {
                                code: "viewer.image.crop_partial".to_owned(),
                                severity: ViewerDiagnosticSeverity::FidelityWarning,
                                message: format!(
                                    "Image crop for node {} is present but cannot be projected exactly ({reason}); exact image bytes are preserved and the full image remains the fallback.",
                                    usage.node_id.as_canonical()
                                ),
                            });
                            None
                        }
                        None => None,
                    };
                    let recolor =
                        pipeline
                            .source
                            .graph
                            .nodes
                            .get(&usage.node_id)
                            .and_then(|node| {
                                viewer_image_recolor_v1(
                                    node.payload.explicit_image_recolor.as_ref(),
                                )
                            });
                    if source_window.is_some() || recolor.is_some() {
                        placements.push(ViewerImagePlacementV1 {
                            node_id: usage.node_id,
                            source_window,
                            recolor,
                        });
                    }
                }

                images.push(ViewerEmbeddedImage {
                    resource_id: file.resource_id,
                    mime: entry.mime.clone(),
                    node_ids: entry.uses.iter().map(|usage| usage.node_id).collect(),
                    placements,
                    bytes: file.bytes,
                });
            }
            images
        }
        Err(_) => {
            document.diagnostics.push(ViewerDiagnostic {
                code: "viewer.image.asset_pipeline_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: "The Viewer could not materialize the exact embedded-image resource bundle for this document.".to_owned(),
            });
            Vec::new()
        }
    };

    let mut scene = resolve_bounded_geometry(&projection, environment).map_err(|blocked| {
        let codes = blocked
            .projection_errors
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow!("Viewer geometry resolution blocked by layout projection errors: {codes}")
    })?;

    document.diagnostics.extend(
        scene
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != "story_text_layout_not_implemented")
            .map(map_scene_diagnostic),
    );

    images.extend(viewer_mature_officeart_wmf_preview_images(
        bytes,
        &pipeline.source_hash,
        &pipeline.source,
        &pipeline.resolved.graph,
        &scene,
        &mut document.diagnostics,
    ));

    #[cfg(feature = "cmo-slot-compose")]
    let projected_instances = match project_carlton_march_cmo_instances(bytes, &pipeline, &scene) {
        Ok(instances) => {
            if !instances.is_empty() {
                document.diagnostics.push(ViewerDiagnostic {
                    code: "viewer.cmo.slot_projection_applied".to_owned(),
                    severity: ViewerDiagnosticSeverity::Info,
                    message: format!(
                        "{} canonical read-only Cmo SceneInstanceV1 projection(s) admitted.",
                        instances.len()
                    ),
                });
            }
            instances
        }
        Err(error) => {
            document.diagnostics.push(ViewerDiagnostic {
                code: "viewer.cmo.slot_projection_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "Canonical Cmo SceneInstance projection rejected ({error}); source truth preserved."
                ),
            });
            Vec::new()
        }
    };

    let selected_pages = effective_page_ids.iter().copied().collect::<BTreeSet<_>>();
    let source_page_paint_orders = pipeline
        .source
        .source_page_paint_orders
        .iter()
        .filter(|order| selected_pages.contains(&order.page_id))
        .map(viewer_page_paint_order_from_source)
        .collect::<Vec<_>>();
    let source_order_stats = apply_known_source_page_paint_orders_to_scene_nodes_v1(
        &mut scene.nodes,
        &source_page_paint_orders,
    );
    if source_order_stats.known_node_count > 0 {
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.stacking.source_order_partial_applied".to_owned(),
            severity: ViewerDiagnosticSeverity::Info,
            message: format!(
                "Persisted source back-to-front order was applied to {} direct source-backed node(s) across {} page order receipt(s); nodes outside that bounded authority retained their existing slots.",
                source_order_stats.known_node_count,
                source_order_stats.page_order_count,
            ),
        });
    }

    if !scene.nodes.is_empty() {
        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.visual.geometry_only".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: "Object positions and sizes are resolved. The desktop Viewer may paint bounded semantic text, admitted source typography sizing, exact embedded PNG/JPEG bytes, bounded source-backed mature OfficeArt WMF previews, persisted bounded image source-window crop, and admitted solid fill/line state. Publisher-exact typography/reflow, unsupported or ambiguous image crop, gradients/patterns, effects, and broader transforms are not faithfully painted yet."
                .to_owned(),
        });
    }
    normalize_diagnostics(&mut document.diagnostics);

    let geometry = ViewerGeometryDocument {
        schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
        document,
        scene,
        paints,
        story_frames,
        text_fragments,
        typography_runs,
        paragraph_alignments,
        text_color_runs,
        script_font_maps,
        tables,
        #[cfg(feature = "cmo-slot-compose")]
        projected_instances,
        images,
    };
    Ok(ViewerOpenBundle {
        geometry,
        resolved_graph: pipeline.resolved.graph,
        source_page_paint_orders,
    })
}

fn viewer_fallback_text_flow_environment_v0_1() -> BoundedTextFlowEnvironment {
    BoundedTextFlowEnvironment {
        layout: BoundedLayoutEnvironment {
            engine_revision: VIEWER_FALLBACK_TEXT_METRICS_REVISION_V0_1.to_owned(),
            font_set_fingerprint: VIEWER_FALLBACK_TEXT_METRICS_REVISION_V0_1.to_owned(),
            resource_fingerprint: "resources:not-consumed:text-flow-v0.1".to_owned(),
        },
        text_metrics: Some(BoundedTextMetrics {
            font_fingerprint: VIEWER_FALLBACK_TEXT_METRICS_REVISION_V0_1.to_owned(),
            scalar_advance: LengthEmu::new(VIEWER_FALLBACK_SCALAR_ADVANCE_EMU_V0_1),
            line_height: LengthEmu::new(VIEWER_FALLBACK_LINE_HEIGHT_EMU_V0_1),
        }),
    }
}

fn viewer_fallback_flow_metrics_diagnostic() -> ViewerDiagnostic {
    ViewerDiagnostic {
        code: "viewer.text.fallback_flow_metrics".to_owned(),
        severity: ViewerDiagnosticSeverity::FidelityWarning,
        message: "Visible text fragments use Viewer fallback font metrics for bounded frame flow while admitted source font sizes may affect sizing. Their frame ownership is grounded, but line breaks and fragment boundaries are not claimed to match Publisher typography.".to_owned(),
    }
}

fn is_refreshable_text_flow_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "viewer.text.flow_not_explicit"
            | "viewer.text.flow_partial"
            | "viewer.text.fallback_overset"
            | "viewer.text.frame_capacity_partial"
            | "viewer.text.fallback_metrics_unavailable"
            | "viewer.text.fallback_flow_metrics"
    )
}

fn resolve_viewer_text_fragments(
    projection: &pub_layout::BoundedLayoutProjection,
) -> Result<(Vec<ViewerTextFragment>, Vec<ResolveDiagnostic>)> {
    let flow = resolve_bounded_text_flow(projection, viewer_fallback_text_flow_environment_v0_1())
        .map_err(|blocked| {
            let codes = blocked
                .projection_errors
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("Viewer text-flow resolution blocked by layout projection errors: {codes}")
        })?;

    let fragments = flow
        .text_fragments
        .into_iter()
        .map(|fragment| ViewerTextFragment {
            story_id: fragment.story_origin,
            frame_id: fragment.frame_origin,
            scalar_start: fragment.scalar_start,
            scalar_end: fragment.scalar_end,
            text: fragment.text,
            line_count: fragment.line_count,
        })
        .collect::<Vec<_>>();

    Ok((fragments, flow.diagnostics))
}

/// Explicit deterministic environment profile for the geometry-only Viewer
/// scene. Fonts and resources are fenced as not consumed by this resolver.
pub fn viewer_geometry_environment_v0_1() -> BoundedLayoutEnvironment {
    BoundedLayoutEnvironment {
        engine_revision: "viewer-geometry-v0.1".to_owned(),
        font_set_fingerprint: "fonts:not-consumed:geometry-only".to_owned(),
        resource_fingerprint: "resources:not-consumed:geometry-only".to_owned(),
    }
}

fn build_mature_0x2c_pipeline(bytes: &[u8]) -> Result<Mature0x2cPipeline> {
    let source_hash = sha256_digest(bytes)?;
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .context("build mature-0x2C PUB source graph for Viewer")?;
    let resolved =
        resolve_pub_source_graph(&source.graph).context("resolve PUB source graph for Viewer")?;
    let page_selection = select_viewer_pages(bytes, source_hash, &source, &resolved);

    Ok(Mature0x2cPipeline {
        source_hash,
        source,
        resolved,
        page_selection,
    })
}

fn select_viewer_pages(
    bytes: &[u8],
    source_hash: Sha256Digest,
    source: &PubSourceGraphBuild,
    resolved: &PubResolvedGraphBuild,
) -> ViewerPageSelection {
    let generic = || ViewerPageSelection {
        page_ids: source.effective_pages.page_ids.clone(),
        disposition: ViewerPageSelectionDisposition::GenericNoLoss,
    };

    let source_sha256 = source_hash.to_string();

    if reference_fixture_profile_known_v1(&source_sha256) {
        let page_roles = match analyze_mature_0x2c_page_roles(Cursor::new(bytes)) {
            Ok(receipt) => receipt,
            Err(error) => {
                return ViewerPageSelection {
                    page_ids: source.effective_pages.page_ids.clone(),
                    disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                        reason: format!("reference_page_role_evidence_unavailable:{error}"),
                    },
                };
            }
        };
        let observed_raw_page_seq_nums = page_roles
            .pages
            .iter()
            .map(|page| page.contents_seq_num)
            .collect::<Vec<_>>();
        let selection = match select_reference_fixture_customer_page_seq_nums_v1(
            &source_sha256,
            &observed_raw_page_seq_nums,
        ) {
            Ok(Some(selection)) => selection,
            Ok(None) => return generic(),
            Err(error) => {
                return ViewerPageSelection {
                    page_ids: source.effective_pages.page_ids.clone(),
                    disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                        reason: format!("reference_profile_rejected:{error}"),
                    },
                };
            }
        };

        let mut page_ids = Vec::with_capacity(selection.customer_page_seq_nums.len());
        for seq_num in &selection.customer_page_seq_nums {
            let page_id = match derive_pub_page_id(&source_hash, *seq_num) {
                Ok(page_id) => page_id,
                Err(error) => {
                    return ViewerPageSelection {
                        page_ids: source.effective_pages.page_ids.clone(),
                        disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                            reason: format!(
                                "reference_customer_page_identity_unavailable:{seq_num}:{error}"
                            ),
                        },
                    };
                }
            };
            if !resolved.graph.pages.contains_key(&page_id) {
                return ViewerPageSelection {
                    page_ids: source.effective_pages.page_ids.clone(),
                    disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                        reason: format!(
                            "reference_customer_page_missing_from_resolved_graph:{seq_num}"
                        ),
                    },
                };
            }
            page_ids.push(page_id);
        }

        return ViewerPageSelection {
            page_ids,
            disposition: ViewerPageSelectionDisposition::FamilyProfileApplied {
                profile_id: selection.profile_id,
                raw_page_count: selection.raw_page_count,
                customer_page_count: selection.customer_page_seq_nums.len(),
            },
        };
    }

    if carlton_admitted_carrier_page_seq_nums_v1(&source_sha256).is_none() {
        let page_roles = match analyze_mature_0x2c_page_roles(Cursor::new(bytes)) {
            Ok(receipt) => receipt,
            Err(_) => return generic(),
        };
        let input = StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: page_roles.document_page_list_entry_count,
            confirmed_page_count: page_roles.confirmed_page_count,
            special_entry_count: page_roles.special_entry_count,
            scenario_evidence_list_count: source.effective_pages.scenario_evidence_list_count,
            observed_scenario_page_count: source.effective_pages.observed_scenario_page_ids.len(),
            pages: page_roles
                .pages
                .into_iter()
                .map(|page| StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: page.document_ordinal,
                    contents_seq_num: page.contents_seq_num,
                    oid_dword0: page.oid_dword0,
                    oid_dword1: page.oid_dword1,
                    applied_master_seq_num: page.applied_master_seq_num,
                })
                .collect(),
        };
        if let Some(selection) = select_standard_print_service_tail_customer_page_seq_nums_v1(input)
        {
            let mut page_ids = Vec::with_capacity(selection.customer_page_seq_nums.len());
            for seq_num in &selection.customer_page_seq_nums {
                let Ok(page_id) = derive_pub_page_id(&source_hash, *seq_num) else {
                    return generic();
                };
                if !resolved.graph.pages.contains_key(&page_id) {
                    return generic();
                }
                page_ids.push(page_id);
            }
            return ViewerPageSelection {
                page_ids,
                disposition: ViewerPageSelectionDisposition::FamilyProfileApplied {
                    profile_id: selection.profile_id,
                    raw_page_count: selection.raw_page_count,
                    customer_page_count: selection.customer_page_seq_nums.len(),
                },
            };
        }
        return generic();
    }

    let Some(carrier_page_seq_nums) = carlton_admitted_carrier_page_seq_nums_v1(&source_sha256)
    else {
        return generic();
    };

    let page_roles = match analyze_mature_0x2c_page_roles(Cursor::new(bytes)) {
        Ok(receipt) => receipt,
        Err(error) => {
            return ViewerPageSelection {
                page_ids: source.effective_pages.page_ids.clone(),
                disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                    reason: format!("page_role_evidence_unavailable:{error}"),
                },
            };
        }
    };

    let input = CarltonPresentationProfileInputV1 {
        schema_version: CARLTON_PRESENTATION_INPUT_SCHEMA_V1.to_owned(),
        source_sha256,
        pages: page_roles
            .pages
            .into_iter()
            .map(|page| CarltonPageEvidenceV1 {
                document_ordinal: page.document_ordinal,
                contents_seq_num: page.contents_seq_num,
                oid_dword0: page.oid_dword0,
                oid_dword1: page.oid_dword1,
                applied_master_seq_num: page.applied_master_seq_num,
                shape_child_count: page.shape_child_count,
            })
            .collect(),
        // The exact SHA admission binds this to the PlcCmob carrier evidence
        // proven by CARLTON-PAGE-PROJECTION-01. Unknown hashes never reach here.
        carrier_page_seq_nums: carrier_page_seq_nums.to_vec(),
    };

    let selection = match select_carlton_customer_page_seq_nums_v1(input) {
        Ok(selection) => selection,
        Err(error) => {
            return ViewerPageSelection {
                page_ids: source.effective_pages.page_ids.clone(),
                disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                    reason: format!("family_profile_rejected:{error}"),
                },
            };
        }
    };

    let mut page_ids = Vec::with_capacity(selection.customer_page_seq_nums.len());
    for seq_num in &selection.customer_page_seq_nums {
        let page_id = match derive_pub_page_id(&source_hash, *seq_num) {
            Ok(page_id) => page_id,
            Err(error) => {
                return ViewerPageSelection {
                    page_ids: source.effective_pages.page_ids.clone(),
                    disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                        reason: format!("customer_page_identity_unavailable:{seq_num}:{error}"),
                    },
                };
            }
        };
        if !resolved.graph.pages.contains_key(&page_id) {
            return ViewerPageSelection {
                page_ids: source.effective_pages.page_ids.clone(),
                disposition: ViewerPageSelectionDisposition::FamilyProfileUnavailable {
                    reason: format!("customer_page_missing_from_resolved_graph:{seq_num}"),
                },
            };
        }
        page_ids.push(page_id);
    }

    ViewerPageSelection {
        page_ids,
        disposition: ViewerPageSelectionDisposition::FamilyProfileApplied {
            profile_id: selection.profile_id,
            raw_page_count: selection.raw_page_count,
            customer_page_count: selection.customer_page_seq_nums.len(),
        },
    }
}

fn viewer_document_from_pipeline(
    byte_len: usize,
    pipeline: &Mature0x2cPipeline,
) -> Result<ViewerDocument> {
    viewer_document_from_graph(
        byte_len,
        pipeline.source_hash,
        &pipeline.source,
        &pipeline.resolved,
        &pipeline.page_selection,
    )
}

fn viewer_document_from_graph(
    byte_len: usize,
    source_hash: Sha256Digest,
    source: &PubSourceGraphBuild,
    resolved: &PubResolvedGraphBuild,
    page_selection: &ViewerPageSelection,
) -> Result<ViewerDocument> {
    let graph = &resolved.graph;

    let effective_page_ids = &page_selection.page_ids;
    let mut pages = Vec::with_capacity(effective_page_ids.len());
    for (zero_based, page_id) in effective_page_ids.iter().enumerate() {
        let page = graph
            .pages
            .get(page_id)
            .with_context(|| format!("document references missing canonical page {page_id:?}"))?;
        let index = u32::try_from(zero_based + 1).context("Viewer page index exceeds u32")?;
        pages.push(ViewerPage {
            index,
            id: *page_id,
            width_emu: page.size.width.get(),
            height_emu: page.size.height.get(),
        });
    }

    let stories = graph
        .stories
        .values()
        .map(|story| ViewerStory {
            id: story.id,
            text: story.text.clone(),
        })
        .collect::<Vec<_>>();

    let mut diagnostics = source
        .diagnostics
        .iter()
        .map(map_bridge_diagnostic)
        .chain(resolved.diagnostics.iter().map(map_resolve_diagnostic))
        .collect::<Vec<_>>();

    match &page_selection.disposition {
        ViewerPageSelectionDisposition::GenericNoLoss => {}
        ViewerPageSelectionDisposition::FamilyProfileApplied {
            profile_id,
            raw_page_count,
            customer_page_count,
        } => {
            diagnostics
                .retain(|diagnostic| diagnostic.code != "viewer.page_projection.roles_unresolved");
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.page_projection.family_profile_applied".to_owned(),
                severity: ViewerDiagnosticSeverity::Info,
                message: format!(
                    "Admitted family presentation profile {profile_id} selects {customer_page_count} customer pages from {raw_page_count} preserved raw PAGE records."
                ),
            });
        }
        ViewerPageSelectionDisposition::FamilyProfileUnavailable { reason } => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.page_projection.family_profile_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "An exact family presentation profile was recognized but could not be applied safely ({reason}); the Viewer preserves every recovered raw PAGE."
                ),
            });
        }
    }
    normalize_diagnostics(&mut diagnostics);

    Ok(ViewerDocument {
        schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
        source: ViewerSource {
            format: graph.source.format.clone(),
            format_version: graph.source.format_version.clone(),
            source_hash,
            byte_len: u64::try_from(byte_len).context("PUB byte length exceeds u64")?,
        },
        pages,
        stories,
        diagnostics,
    })
}

/// Creates only the grounded semantic subset already accepted by pub-layout.
///
/// Viewer remains the semantic owner of this resolved-graph -> bounded-authoring
/// bridge. Desktop shaped-flow is the second concrete consumer, so the mapping
/// is public instead of being copied into another engine adapter.
pub fn bounded_authoring_slice_from_resolved(
    graph: &PubResolvedGraph,
) -> Result<BoundedAuthoringSlice> {
    bounded_authoring_slice_from_resolved_pages(graph, &graph.document.pages)
}

fn legacy_noquill_image_page(
    graph: &PubResolvedGraph,
    node: &Node<PubResolvedNodePayload>,
    selected_pages: &BTreeSet<PageId>,
) -> Option<PageId> {
    if node.kind != NodeKind::ImageFrame {
        return None;
    }

    let mut current = node.header.parent_id;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current) {
            return None;
        }

        let page_id = PageId::from_canonical(current);
        if graph.pages.contains_key(&page_id) {
            return selected_pages.contains(&page_id).then_some(page_id);
        }

        let parent = graph.nodes.get(&NodeId::from_canonical(current))?;
        if parent.kind != NodeKind::Group {
            return None;
        }
        current = parent.header.parent_id;
    }
}

fn bounded_legacy_noquill_authoring_slice_from_resolved_pages(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice> {
    let mut authoring = bounded_authoring_slice_from_resolved_pages(graph, page_ids)?;
    let selected_pages = page_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut projected_ids = authoring
        .node_geometry
        .iter()
        .map(|node| node.node_id)
        .collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        if projected_ids.contains(&node.header.id) {
            continue;
        }
        let Some(page_id) = legacy_noquill_image_page(graph, node, &selected_pages) else {
            continue;
        };

        authoring.node_geometry.push(BoundedNodeGeometryInput {
            node_id: node.header.id,
            parent_origin: page_id.into_canonical(),
            bounds: node.header.bounds,
            transform: node.header.transform.clone(),
        });
        projected_ids.insert(node.header.id);
    }

    authoring.node_geometry.sort_by_key(|node| node.node_id);
    Ok(authoring)
}

fn bounded_authoring_slice_from_resolved_pages(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice> {
    let pages = page_ids
        .iter()
        .map(|page_id| {
            graph
                .pages
                .get(page_id)
                .cloned()
                .with_context(|| format!("layout projection missing document page {page_id:?}"))
        })
        .collect::<Result<Vec<_>>>()?;

    let page_origins = page_ids
        .iter()
        .map(|page_id| page_id.into_canonical())
        .collect::<BTreeSet<_>>();

    let node_geometry = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .map(|node| BoundedNodeGeometryInput {
            node_id: node.header.id,
            parent_origin: node.header.parent_id,
            bounds: node.header.bounds,
            transform: node.header.transform.clone(),
        })
        .collect();

    let stories = graph.stories.values().cloned().collect();

    let story_frames = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .filter_map(|node| {
            let frame = node.payload.story_frame.as_ref()?;
            let story_id = frame.story_id?;
            Some(StoryFrame {
                story_id,
                frame_id: node.header.id,
                ordinal: frame.ordinal,
                previous: frame.previous_frame,
                next: frame.next_frame,
            })
        })
        .collect();

    let tables = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .filter_map(|node| {
            let table = node.payload.table.as_ref()?.simple_table.as_ref()?.clone();
            Some(BoundedTableInput {
                node_id: node.header.id,
                table,
            })
        })
        .collect();

    Ok(BoundedAuthoringSlice {
        pages,
        node_geometry,
        stories,
        story_frames,
        tables,
        guides: Vec::new(),
        unknown_layout_state: Vec::new(),
    })
}

fn viewer_tables_from_resolved(
    graph: &PubResolvedGraph,
    projection: &BoundedLayoutProjection,
) -> (Vec<ViewerTable>, Vec<ViewerDiagnostic>) {
    let mut tables = Vec::new();
    let mut diagnostics = Vec::new();
    let visible_node_ids = projection
        .node_geometry
        .iter()
        .map(|geometry| geometry.origin)
        .collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        let node_id = node.header.id;
        if !visible_node_ids.contains(&node_id) {
            continue;
        }
        let Some(source) = node.payload.table.as_ref() else {
            continue;
        };
        let Some(story_id) = source.story_id else {
            continue;
        };
        let Some(story) = graph.stories.get(&story_id) else {
            continue;
        };

        let materialized = match materialize_bounded_table_cells(source, story) {
            Ok(cells) => cells,
            Err(error) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.table.cell_text_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message: format!(
                        "Bounded table cells could not be materialized safely ({error:?})."
                    ),
                });
                continue;
            }
        };

        let needs_fallback_geometry = materialized.iter().any(|cell| cell.bounds.is_none());
        let projected = projection
            .tables
            .iter()
            .find(|table| table.origin == node_id);
        let resolved_bounds = if needs_fallback_geometry {
            source
                .layout_metrics
                .as_ref()
                .zip(projected)
                .and_then(|(metrics, projected)| {
                    let mut table_projection = projection.clone();
                    table_projection
                        .tables
                        .retain(|table| table.origin == projected.origin);
                    table_projection
                        .node_geometry
                        .retain(|geometry| geometry.origin == projected.origin);
                    table_projection.diagnostics.clear();

                    resolve_bounded_uniform_table_cells(
                        &table_projection,
                        &[BoundedUniformTableMetrics {
                            table_origin: projected.origin,
                            cell_width: metrics.cell_width,
                            row_pitch: metrics.row_pitch,
                        }],
                    )
                    .ok()
                })
        } else {
            None
        };

        if needs_fallback_geometry && source.layout_metrics.is_some() && resolved_bounds.is_none() {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.table.cell_geometry_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: "Exact table track geometry was unavailable and the bounded fallback cell-geometry resolver rejected this table; semantic cells remain available.".to_owned(),
            });
        }

        let cells: Vec<ViewerTableCell> = materialized
            .into_iter()
            .map(|cell| ViewerTableCell {
                id: cell.id,
                address: cell.address,
                row_span: cell.row_span,
                column_span: cell.column_span,
                text: cell.text,
                bounds: cell.bounds.or_else(|| {
                    resolved_bounds.as_ref().and_then(|resolved| {
                        resolved
                            .cells
                            .iter()
                            .find(|candidate| candidate.origin == cell.id)
                            .map(|candidate| candidate.bounds)
                    })
                }),
                fill_rgb: cell.fill_rgb,
                fill_visible: cell.fill_visible,
            })
            .collect();

        let borders = viewer_table_border_segments(source, &cells);
        tables.push(ViewerTable {
            node_id,
            story_id,
            rows: source.rows,
            columns: source.columns,
            cells,
            borders,
        });
    }

    tables.sort_by_key(|table| table.node_id);
    (tables, diagnostics)
}

fn sha256_digest(bytes: &[u8]) -> Result<Sha256Digest> {
    let digest = Sha256::digest(bytes);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    hex.parse()
        .map_err(|error| anyhow!("invalid internally computed SHA-256: {error:?}"))
}

fn map_bridge_diagnostic(diagnostic: &PubBridgeDiagnostic) -> ViewerDiagnostic {
    use PubBridgeDiagnostic::*;

    let (code, severity, message) = match diagnostic {
        PageListSpecialEntry { .. } => (
            "viewer.page_list.special_entry",
            ViewerDiagnosticSeverity::Info,
            "The document page list contains a special non-page entry.",
        ),
        PageListUnknownEntry { .. } => (
            "viewer.page_list.unknown_entry",
            ViewerDiagnosticSeverity::FidelityWarning,
            "The document page list contains an entry the Viewer cannot classify.",
        ),
        OpaqueContentsTail { .. } => (
            "viewer.object.opaque_data",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Some object data is not understood by the current Viewer model.",
        ),
        MissingEscherGeometry { .. } => (
            "viewer.geometry.missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A placed object has no confirmed display geometry.",
        ),
        AmbiguousEscherGeometry { .. } => (
            "viewer.geometry.ambiguous",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A placed object has more than one possible display geometry.",
        ),
        AmbiguousImageSlot { .. } => (
            "viewer.image.identity_ambiguous",
            ViewerDiagnosticSeverity::FidelityWarning,
            "An image reference cannot be resolved to one confirmed embedded image.",
        ),
        IncompleteEscherAnchor { .. } => (
            "viewer.geometry.incomplete",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A placed object has incomplete position or size information.",
        ),
        InvalidEscherAnchor { .. } => (
            "viewer.geometry.invalid",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A placed object has invalid position or size information.",
        ),
        MissingQuillStory { .. } => (
            "viewer.text.story_missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A text frame refers to text that the Viewer could not recover.",
        ),
        LegacyObjectNotMaterialized { .. } => (
            "viewer.legacy.object_not_materialized",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A legacy Publisher page contains an object outside the currently admitted old-0x22 Reader profile.",
        ),
        LegacyTextEncodingUnresolved { .. } => (
            "viewer.text.legacy_encoding_unresolved",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Legacy text bytes are preserved, but their character encoding is not proven; page geometry remains available without guessing text.",
        ),
        McldRecordCountMismatch { .. } => (
            "viewer.table.mcld_layout_metrics_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A Quill layout-metrics table uses a structure outside the bounded MCLD profile; core document content remains available.",
        ),
        FdppExactStoryFallback { .. } => (
            "viewer.text.fdpp_exact_story_fallback",
            ViewerDiagnosticSeverity::Info,
            "Story text was recovered from exact persisted Contents identity, FDPP boundaries, and TEXT bytes because the ordinary Quill Story service plane is sentinel-filled.",
        ),
        EquivalentMarginsPageExtents { .. } => (
            "viewer.page_extent.equivalent_source_records",
            ViewerDiagnosticSeverity::Info,
            "Multiple source page-extent records agree exactly; the Viewer uses their shared page size.",
        ),
        ScenarioPageOrderObserved { .. } => (
            "viewer.page_projection.scenario_order_observed",
            ViewerDiagnosticSeverity::Info,
            "A persisted scenario/design page-identity order was recovered. It is retained as evidence only and is not used to suppress physical pages.",
        ),
        ScenarioPageOrderUnavailable { .. } => (
            "viewer.page_projection.scenario_order_unavailable",
            ViewerDiagnosticSeverity::Info,
            "Scenario/design page-order metadata could not be resolved safely; it is not used for physical page filtering.",
        ),
        PageRoleClassificationUnresolved { .. } => (
            "viewer.page_projection.roles_unresolved",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Generic customer/master/service page-role filtering is not proven for this file family, so the Viewer preserves all recovered physical PAGE records.",
        ),
        LinkedFrameNotMaterialized { .. } => (
            "viewer.text.link_target_missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A linked text frame refers to another frame that is not materialized.",
        ),
        GroupedStoryProjected { .. } => (
            "viewer.geometry.grouped_story_projected",
            ViewerDiagnosticSeverity::Info,
            "A grouped text shape was projected through its exact bounded group geometry chain.",
        ),
        GroupedStoryProjectionUnavailable { .. } => (
            "viewer.geometry.grouped_story_projection_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A grouped text shape falls outside the bounded group geometry profile.",
        ),
        GroupedImageProjected { .. } => (
            "viewer.geometry.grouped_image_projected",
            ViewerDiagnosticSeverity::Info,
            "A grouped image shape was projected through its exact bounded group geometry chain.",
        ),
        GroupedImageProjectionUnavailable { .. } => (
            "viewer.geometry.grouped_image_projection_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A grouped image shape falls outside the bounded group geometry profile.",
        ),
        GroupedTableProjected { .. } => (
            "viewer.geometry.grouped_table_projected",
            ViewerDiagnosticSeverity::Info,
            "A grouped table was projected through its exact bounded group geometry chain.",
        ),
        GroupedTableProjectionUnavailable { .. } => (
            "viewer.geometry.grouped_table_projection_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A grouped table falls outside the bounded group geometry profile.",
        ),
        TableMissingRequiredField { .. } => (
            "viewer.table.required_data_missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table is missing data required for the bounded table model.",
        ),
        TableMissingTcd { .. } => (
            "viewer.table.text_map_missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table's text mapping could not be recovered.",
        ),
        TableAmbiguousTcd { .. } => (
            "viewer.table.text_map_ambiguous",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table has more than one possible text mapping.",
        ),
        TableMissingCellsObject { .. } => (
            "viewer.table.cells_missing",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table refers to a cell collection that is not available.",
        ),
        TableCellsWrongRawType { .. } => (
            "viewer.table.cells_unexpected_kind",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table's cell collection has an unexpected object kind.",
        ),
        TableCellsWrongParent { .. } => (
            "viewer.table.cells_parent_mismatch",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table's cell collection has an unexpected ownership relation.",
        ),
        TableCellCountMismatch { .. } => (
            "viewer.table.cell_count_mismatch",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table's recovered cell counts disagree.",
        ),
        TableCellTextRangeInvalid { .. } => (
            "viewer.table.cell_text_range_invalid",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table cell points outside the recovered text range.",
        ),
        TableStoryLengthMismatch { .. } => (
            "viewer.table.story_length_mismatch",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table's cell text boundaries disagree with the recovered story length.",
        ),
        TableCellCoordinatesAmbiguous { .. } => (
            "viewer.table.cell_coordinates_ambiguous",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A table cell does not have one confirmed row and column position.",
        ),
        TableLayoutMetricsUnavailable { .. } => (
            "viewer.table.layout_metrics_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Exact table layout metrics are not available.",
        ),
        TypographyProjectionUnavailable { .. } => (
            "viewer.text.typography_projection_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Some source typography could not be projected safely; pinned fallback text rendering remains in use.",
        ),
        TypographyUnknownFixedBlockTypes { .. } => (
            "viewer.text.typography_unknown_block_type",
            ViewerDiagnosticSeverity::FidelityWarning,
            "The typography stream contains unproven fixed block widths; affected typography promotion fails closed.",
        ),
        ColorSchemeProjectionUnavailable { .. } => (
            "viewer.paint.color_scheme_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "The current publication color scheme could not be resolved safely; scheme-indexed shape colors remain unavailable.",
        ),
        AmbiguousOfficeArtDggDefaults { .. } => (
            "viewer.paint.dgg_defaults_ambiguous",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Document-wide OfficeArt drawing-group defaults are ambiguous; affected effective paint remains unresolved.",
        ),
    };

    ViewerDiagnostic {
        code: code.to_owned(),
        severity,
        message: message.to_owned(),
    }
}

fn map_resolve_diagnostic(diagnostic: &PubResolveDiagnostic) -> ViewerDiagnostic {
    match diagnostic {
        PubResolveDiagnostic::MissingStoryIdentity { .. } => ViewerDiagnostic {
            code: "viewer.text.story_identity_missing".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: "A text frame could not be joined to one recovered story.".to_owned(),
        },
    }
}

fn map_projection_diagnostic(diagnostic: &ProjectionDiagnostic) -> ViewerDiagnostic {
    let (code, message) = match diagnostic.code.as_str() {
        "invalid_page_semantics" => (
            "viewer.layout.invalid_page",
            "A page cannot be projected into the current visual layout model.",
        ),
        "missing_story_content" => (
            "viewer.layout.story_content_missing",
            "A text frame refers to story content absent from the visual projection.",
        ),
        "missing_frame_geometry" => (
            "viewer.layout.frame_geometry_missing",
            "A text frame has no confirmed geometry in the visual projection.",
        ),
        "unknown_layout_affecting_state" => (
            "viewer.layout.unknown_affecting_state",
            "Some layout-affecting state is not understood by the current Viewer.",
        ),
        _ => (
            "viewer.layout.projection_partial",
            "The current Viewer cannot fully project some authoring state.",
        ),
    };

    ViewerDiagnostic {
        code: code.to_owned(),
        severity: ViewerDiagnosticSeverity::FidelityWarning,
        message: message.to_owned(),
    }
}

#[cfg(feature = "cmo-slot-compose")]
fn parse_projected_node_id(value: &str, label: &str) -> Result<NodeId> {
    let canonical = value
        .parse::<CanonicalId>()
        .with_context(|| format!("{label} is not a canonical UUID: {value}"))?;
    Ok(NodeId::from_canonical(canonical))
}

#[cfg(feature = "cmo-slot-compose")]
fn parse_projected_story_id(value: &str, label: &str) -> Result<StoryId> {
    let canonical = value
        .parse::<CanonicalId>()
        .with_context(|| format!("{label} is not a canonical UUID: {value}"))?;
    Ok(StoryId::from_canonical(canonical))
}

#[cfg(feature = "cmo-slot-compose")]
fn page_for_resolved_node(graph: &PubResolvedGraph, node_id: NodeId) -> Result<PageId> {
    let mut current = graph
        .nodes
        .get(&node_id)
        .with_context(|| {
            format!(
                "projected target frame {} is absent",
                node_id.as_canonical()
            )
        })?
        .header
        .parent_id;
    let mut seen = BTreeSet::new();

    loop {
        if !seen.insert(current) {
            return Err(anyhow!(
                "projected target ancestry contains a cycle at {current}"
            ));
        }

        let page_id = PageId::from_canonical(current);
        if graph.pages.contains_key(&page_id) {
            return Ok(page_id);
        }

        let parent_node_id = NodeId::from_canonical(current);
        let parent = graph.nodes.get(&parent_node_id).with_context(|| {
            format!(
                "projected target ancestry {} is neither a page nor a resolved node",
                current
            )
        })?;
        current = parent.header.parent_id;
    }
}

#[cfg(feature = "cmo-slot-compose")]
fn project_carlton_march_cmo_instances(
    bytes: &[u8],
    pipeline: &Mature0x2cPipeline,
    scene: &BoundedResolvedScene,
) -> Result<Vec<ViewerProjectedSceneInstanceV1>> {
    let ViewerPageSelectionDisposition::FamilyProfileApplied { profile_id, .. } =
        &pipeline.page_selection.disposition
    else {
        return Ok(Vec::new());
    };
    if profile_id != CARLTON_MARCH_PRESENTATION_PROFILE_V1 {
        return Ok(Vec::new());
    }

    let bridge = build_mature_0x2c_cmo_projection_bridge_v1(
        bytes,
        pipeline.source_hash,
        &pipeline.source.graph,
        &pipeline.resolved.graph,
    )
    .context("build active Reader Cmo authority bridge for Carlton March")?;
    if !bridge.active_graph_identity_parity {
        return Err(anyhow!(
            "active Reader Cmo bridge did not prove identity parity"
        ));
    }

    let graph = &pipeline.resolved.graph;
    let context = &bridge.output.context;
    let target_qsids = context
        .cmo_relations
        .iter()
        .map(|relation| relation.target_qsid)
        .collect::<BTreeSet<_>>();
    let expected_target_qsids = BTreeSet::from([49_u32, 120, 216, 218]);
    if target_qsids != expected_target_qsids {
        return Err(anyhow!(
            "exact Carlton March Cmo target set changed: expected {:?}, got {:?}",
            expected_target_qsids,
            target_qsids
        ));
    }
    let mut projected = Vec::new();

    for target_qsid in target_qsids {
        let relations = context
            .cmo_relations
            .iter()
            .filter(|relation| relation.target_qsid == target_qsid)
            .collect::<Vec<_>>();
        let first = relations
            .first()
            .copied()
            .with_context(|| format!("Cmo target Qsid {target_qsid} has no relations"))?;

        if relations.iter().any(|relation| {
            relation.target_story_id != first.target_story_id
                || relation.target_frame_node_id != first.target_frame_node_id
        }) {
            return Err(anyhow!(
                "Cmo target Qsid {target_qsid} has inconsistent Story/frame authority"
            ));
        }

        let target_story_id =
            parse_projected_story_id(&first.target_story_id, "Cmo target_story_id")?;
        let target_frame_text = first
            .target_frame_node_id
            .as_deref()
            .with_context(|| format!("Cmo target Qsid {target_qsid} has no unique target frame"))?;
        let target_frame_node_id =
            parse_projected_node_id(target_frame_text, "Cmo target_frame_node_id")?;
        let target_frame = graph.nodes.get(&target_frame_node_id).with_context(|| {
            format!("Cmo target Qsid {target_qsid} frame is absent from resolved graph")
        })?;
        let target_frame_story = target_frame
            .payload
            .story_frame
            .as_ref()
            .and_then(|frame| frame.story_id)
            .with_context(|| {
                format!("Cmo target Qsid {target_qsid} frame has no Story identity")
            })?;
        if target_frame_story != target_story_id {
            return Err(anyhow!(
                "Cmo target Qsid {target_qsid} frame/Story identity mismatch"
            ));
        }

        let target_page_id = page_for_resolved_node(graph, target_frame_node_id)?;
        if !pipeline.page_selection.page_ids.contains(&target_page_id) {
            return Err(anyhow!(
                "Cmo target Qsid {target_qsid} resolves outside the admitted customer-page set"
            ));
        }

        let target_scene_node = scene
            .nodes
            .iter()
            .find(|node| node.origin == target_frame_node_id)
            .with_context(|| {
                format!("Cmo target Qsid {target_qsid} frame is absent from resolved Viewer scene")
            })?;
        let target_story = graph.stories.get(&target_story_id).with_context(|| {
            format!("Cmo target Qsid {target_qsid} Story is absent from resolved graph")
        })?;
        let object_marker_scalars = target_story
            .text
            .chars()
            .enumerate()
            .filter_map(|(index, ch)| {
                (ch == '\u{FFFC}')
                    .then(|| u32::try_from(index).context("Cmo marker scalar exceeds u32"))
            })
            .collect::<Result<Vec<_>>>()?;

        let expected_markers: &[u32] = match target_qsid {
            120 | 216 | 218 => &[0],
            49 => &[0, 3, 5, 7, 9, 11],
            _ => unreachable!("exact target set checked above"),
        };
        if object_marker_scalars != expected_markers {
            return Err(anyhow!(
                "exact Carlton March Cmo markers changed for Qsid {target_qsid}: expected {expected_markers:?}, got {object_marker_scalars:?}"
            ));
        }

        let frame_count = graph
            .nodes
            .values()
            .filter(|node| {
                node.payload
                    .story_frame
                    .as_ref()
                    .and_then(|frame| frame.story_id)
                    == Some(target_story_id)
            })
            .count();
        let frame_count =
            u32::try_from(frame_count).context("Cmo target frame count exceeds u32")?;

        let carrier_extents = relations
            .iter()
            .map(|relation| {
                let carrier_node_id =
                    parse_projected_node_id(&relation.carrier_node_id, "Cmo carrier_node_id")?;
                let carrier = graph.nodes.get(&carrier_node_id).with_context(|| {
                    format!(
                        "Cmo carrier Ohpo {} is absent from resolved graph",
                        relation.carrier_ohpo
                    )
                })?;
                let nested_cmo = relation.carrier_story_id.as_ref().is_some_and(|story_id| {
                    context
                        .cmo_relations
                        .iter()
                        .any(|candidate| candidate.target_story_id == *story_id)
                });
                Ok(CarrierExtentV1 {
                    carrier_node_id: relation.carrier_node_id.clone(),
                    width_emu: carrier.header.bounds.width.get(),
                    height_emu: carrier.header.bounds.height.get(),
                    nested_cmo,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Exact March target Stories start with the semantic object marker. For
        // the three one-slot targets there is no preceding visible text. For
        // Qsid49 the already-proven second carrier fails both width and height
        // even under the zero-inter-slot-text lower bound, so an empty line set
        // cannot expose a later slot incorrectly. This is deliberately scoped
        // to the exact admitted March profile, not a generic Cmo text-flow law.
        let output = resolve_cmo_slot_flow_v1(
            context,
            &CmoStorySlotFlowInputV1 {
                target_qsid,
                target_page_id: target_page_id.as_canonical().to_string(),
                target_story_id: target_story_id.as_canonical().to_string(),
                target_frame_node_id: target_frame_node_id.as_canonical().to_string(),
                frame_count,
                host_width_emu: target_scene_node.bounds.width.get(),
                host_height_emu: target_scene_node.bounds.height.get(),
                object_marker_scalars,
                text_lines: Vec::new(),
                carrier_extents,
            },
        )
        .with_context(|| format!("resolve Carlton March Cmo target Qsid {target_qsid}"))?;

        let expected_visible_cmo_ids: &[u32] = match target_qsid {
            218 => &[1],
            120 => &[6],
            216 => &[5],
            49 => &[7],
            _ => unreachable!("exact target set checked above"),
        };
        let visible_cmo_ids = output
            .visible_slots
            .iter()
            .map(|slot| slot.cmo_id)
            .collect::<Vec<_>>();
        if visible_cmo_ids != expected_visible_cmo_ids {
            return Err(anyhow!(
                "exact Carlton March visible Cmo prefix changed for Qsid {target_qsid}: expected {expected_visible_cmo_ids:?}, got {visible_cmo_ids:?}"
            ));
        }
        if output.scaling_applied || output.skip_to_fit || output.carrier_reparent_count != 0 {
            return Err(anyhow!(
                "exact Carlton March Cmo slot-flow violated no-scale/no-skip/no-reparent law for Qsid {target_qsid}"
            ));
        }
        if target_qsid == 49
            && (!output.overset.story_overset
                || output.overset.first_nonfitting_slot_index != Some(1)
                || output.overset.first_nonfitting_scalar_index != Some(3))
        {
            return Err(anyhow!(
                "exact Carlton March q49 first-nonfit discriminator changed: {:?}",
                output.overset
            ));
        }
        if target_qsid != 49 && output.overset.story_overset {
            return Err(anyhow!(
                "exact Carlton March single-slot target Qsid {target_qsid} unexpectedly overset"
            ));
        }

        let target_story_scalar_count = u32::try_from(target_story.text.chars().count())
            .context("Cmo target Story scalar count exceeds u32")?;
        let target_frame_paint_scalar_end = output.overset.first_nonfitting_scalar_index;
        if target_frame_paint_scalar_end.is_some_and(|value| value > target_story_scalar_count) {
            return Err(anyhow!(
                "exact Carlton March target Qsid {target_qsid} overset scalar exceeds Story length"
            ));
        }

        for slot in output.visible_slots {
            let origin_node_id =
                parse_projected_node_id(&slot.carrier_node_id, "visible Cmo carrier_node_id")?;
            let carrier = graph.nodes.get(&origin_node_id).with_context(|| {
                format!("visible Cmo carrier {} is absent", slot.carrier_node_id)
            })?;
            let carrier_story_id = slot
                .carrier_story_id
                .as_deref()
                .map(|value| parse_projected_story_id(value, "visible Cmo carrier_story_id"))
                .transpose()?;
            if let Some(story_id) = carrier_story_id
                && !graph.stories.contains_key(&story_id)
            {
                return Err(anyhow!(
                    "visible Cmo carrier Story {} is absent",
                    story_id.as_canonical()
                ));
            }

            let x = target_scene_node
                .bounds
                .x
                .get()
                .checked_add(slot.resolved_x_emu)
                .context("Cmo projected x overflow")?;
            let y = target_scene_node
                .bounds
                .y
                .get()
                .checked_add(slot.resolved_y_emu)
                .context("Cmo projected y overflow")?;
            let bounds = RectEmu::new(
                LengthEmu::new(x),
                LengthEmu::new(y),
                LengthEmu::new(slot.resolved_width_emu),
                LengthEmu::new(slot.resolved_height_emu),
            );
            let text_content_bounds =
                carrier
                    .payload
                    .text_frame_inset
                    .as_ref()
                    .and_then(|source| {
                        let inset = i64::from(source.uniform_emu);
                        let double = inset.checked_mul(2)?;
                        let content_x = bounds.x.get().checked_add(inset)?;
                        let content_y = bounds.y.get().checked_add(inset)?;
                        let content_width = bounds.width.get().checked_sub(double)?;
                        let content_height = bounds.height.get().checked_sub(double)?;
                        (content_width > 0 && content_height > 0).then(|| {
                            RectEmu::new(
                                LengthEmu::new(content_x),
                                LengthEmu::new(content_y),
                                LengthEmu::new(content_width),
                                LengthEmu::new(content_height),
                            )
                        })
                    });

            let relation = relations.get(slot.slot_index).copied().with_context(|| {
                format!(
                    "visible Cmo slot {} has no canonical relation",
                    slot.slot_index
                )
            })?;
            let scene_instance = cmo_story_slot_instance_v1(
                relation,
                &target_page_id.as_canonical().to_string(),
                slot.slot_index,
                slot.scalar_index,
            )
            .context("derive canonical Cmo SceneInstanceV1")?;
            if scene_instance.instance_id != slot.instance_id
                || scene_instance.projection_kind != SceneProjectionKindV1::CmoStorySlot
                || scene_instance.origin_node_id != slot.carrier_node_id
                || scene_instance.story_authority_id != slot.carrier_story_id
            {
                return Err(anyhow!(
                    "slot-flow / SceneInstance authority mismatch for Qsid {target_qsid} slot {}",
                    slot.slot_index
                ));
            }

            projected.push(ViewerProjectedSceneInstanceV1 {
                scene_instance,
                target_frame_node_id,
                target_frame_paint_scalar_end,
                bounds,
                text_content_bounds,
                transform: carrier.header.transform.clone(),
            });
        }
    }

    projected.sort_by(|left, right| {
        (
            left.scene_instance.target_page_id.as_str(),
            left.scene_instance.cmo_scalar_index,
            left.scene_instance.instance_id.as_str(),
        )
            .cmp(&(
                right.scene_instance.target_page_id.as_str(),
                right.scene_instance.cmo_scalar_index,
                right.scene_instance.instance_id.as_str(),
            ))
    });

    if projected.len() != 4 {
        return Err(anyhow!(
            "exact Carlton March Cmo projection expected 4 visible slot instances, got {}",
            projected.len()
        ));
    }

    Ok(projected)
}

fn map_scene_diagnostic(diagnostic: &ResolveDiagnostic) -> ViewerDiagnostic {
    let (code, message) = match diagnostic.code.as_str() {
        "story_text_layout_not_implemented" => (
            "viewer.layout.text_not_rendered",
            "Text is recovered for search and copy, but is not yet visually laid out in this Viewer slice.",
        ),
        "shared_story_without_explicit_flow" => (
            "viewer.text.flow_not_explicit",
            "Several frames share one recovered Story, but no explicit reciprocal flow chain is proven, so the Viewer does not invent one.",
        ),
        "ambiguous_story_flow"
        | "cyclic_story_flow"
        | "broken_story_flow"
        | "non_reciprocal_story_flow"
        | "disconnected_story_flow" => (
            "viewer.text.flow_partial",
            "A recovered multi-frame Story does not form one bounded explicit flow chain, so its preview text placement remains partial.",
        ),
        "story_overset" => (
            "viewer.text.fallback_overset",
            "The explicit frame chain cannot place all Story text under the Viewer fallback metrics. This is not Publisher-native overset evidence.",
        ),
        "text_frame_geometry_missing" | "text_frame_has_no_capacity" => (
            "viewer.text.frame_capacity_partial",
            "A recovered text frame cannot accept bounded fallback text placement with the current resolved geometry.",
        ),
        "text_metrics_missing" | "text_metrics_font_mismatch" | "invalid_text_metrics" => (
            "viewer.text.fallback_metrics_unavailable",
            "The Viewer fallback text environment is unavailable or inconsistent, so text flow is not materialized.",
        ),
        _ => (
            "viewer.layout.scene_partial",
            "Some visual content is only partially resolved by the current Viewer.",
        ),
    };

    ViewerDiagnostic {
        code: code.to_owned(),
        severity: ViewerDiagnosticSeverity::FidelityWarning,
        message: message.to_owned(),
    }
}

fn normalize_diagnostics(diagnostics: &mut Vec<ViewerDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        (&left.code, &left.message, severity_order(left.severity)).cmp(&(
            &right.code,
            &right.message,
            severity_order(right.severity),
        ))
    });
    diagnostics.dedup();
}

const fn severity_order(severity: ViewerDiagnosticSeverity) -> u8 {
    match severity {
        ViewerDiagnosticSeverity::Info => 0,
        ViewerDiagnosticSeverity::FidelityWarning => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{
        Affine2D, CanonicalId, Document, DocumentId, LengthEmu, Node, NodeHeader, NodeId, NodeKind,
        Page, RectEmu, Size2D, SourceDescriptor, Story,
    };
    use pub_reader::{PubResolvedNodePayload, PubResolvedStoryFrame};
    use std::collections::BTreeMap;

    #[test]
    fn local_failure_report_exposes_structural_class_without_source_content() {
        let bytes = b"<!DOCTYPE html><html><body>private-customer-text-8271</body></html>";
        let json = local_failure_diagnostic_json(bytes).expect("local failure report");

        assert!(json.contains(VIEWER_FAILURE_REPORT_SCHEMA_V0_1));
        assert!(json.contains("\"intake_class\": \"not_pub\""));
        assert!(json.contains("\"container_family\": \"foreign\""));
        assert!(!json.contains("private-customer-text-8271"));
        assert!(!json.contains("<!DOCTYPE"));
        assert!(!json.contains("\"path\""));
        assert!(!json.contains("\"filename\""));
        assert!(!json.contains("\"source_hash\""));
        assert!(!json.contains("\"sha256\""));
    }

    #[test]
    fn salvage_product_outcome_carries_partial_source_graph_not_probe() {
        let graph = ReaderPartialSourceGraph {
            schema_version: READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1.to_owned(),
            source_sha256: "a".repeat(64),
            contents_family: None,
            subsystems: ReaderSalvageSubsystemProbe {
                contents: ReaderSalvageStreamState::Readable,
                quill: ReaderSalvageStreamState::Absent,
                escher: ReaderSalvageStreamState::Absent,
                escher_delay: ReaderSalvageStreamState::Absent,
            },
            facts: Vec::new(),
            gaps: vec![
                ReaderPartialSourceGap::TextUnavailable,
                ReaderPartialSourceGap::ImageFactsUnavailable,
                ReaderPartialSourceGap::GeometryFactsUnavailable,
            ],
        };
        let outcome = ViewerProductOpenOutcome::Salvage(graph.clone());
        assert_eq!(outcome, ViewerProductOpenOutcome::Salvage(graph));
    }

    #[test]
    fn foreign_input_never_enters_salvage_fallback() {
        let outcome = open_pub_or_salvage(
            b"<!DOCTYPE html><html>not a Publisher file</html>",
            viewer_geometry_environment_v0_1(),
        );
        assert!(outcome.is_err());
    }

    #[test]
    fn local_failure_report_uses_size_bucket_instead_of_exact_length() {
        let bytes = vec![0_u8; 5_123];
        let report = build_local_failure_diagnostic_report(&bytes).expect("local failure report");

        assert_eq!(
            report.envelope.size_bucket,
            pub_reader::FailureSizeBucket::Under64KiB
        );

        let json = serde_json::to_string(&report).expect("serialize report");
        assert!(!json.contains("\"byte_len\""));
        assert!(!json.contains("5123"));
    }

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    #[test]
    fn semantic_table_fences_generic_owner_fill_but_preserves_line() {
        let line = ViewerSolidLine {
            rgb: [1, 2, 3],
            width_emu: 42,
        };
        let table_paint = fence_semantic_table_container_fill(
            true,
            ViewerNodePaint {
                node_id: NodeId::from_canonical(id(90)),
                preset_shape: None,
                solid_fill_rgb: Some([91, 155, 213]),
                solid_line: Some(line.clone()),
            },
        );
        assert_eq!(table_paint.solid_fill_rgb, None);
        assert_eq!(table_paint.solid_line, Some(line.clone()));

        let shape_paint = fence_semantic_table_container_fill(
            false,
            ViewerNodePaint {
                node_id: NodeId::from_canonical(id(91)),
                preset_shape: None,
                solid_fill_rgb: Some([91, 155, 213]),
                solid_line: Some(line.clone()),
            },
        );
        assert_eq!(shape_paint.solid_fill_rgb, Some([91, 155, 213]));
        assert_eq!(shape_paint.solid_line, Some(line));
    }

    fn resolved_graph_fixture() -> PubResolvedGraph {
        let source_hash = Sha256Digest::from_bytes([0xAB; 32]);
        let page_id = PageId::from_canonical(id(2));
        let node_id = NodeId::from_canonical(id(3));
        let story_id = StoryId::from_canonical(id(4));

        PubResolvedGraph {
            cdm_version: "0.1".to_owned(),
            resolver_version: "test".to_owned(),
            source: SourceDescriptor {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                adapter_version: "test".to_owned(),
                source_hash,
            },
            document: Document {
                id: DocumentId::from_canonical(id(1)),
                format_origin: "pub".to_owned(),
                source_hash,
                pages: vec![page_id],
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages: BTreeMap::from([(
                page_id,
                Page {
                    id: page_id,
                    size: Size2D::new(LengthEmu::new(1_000), LengthEmu::new(2_000)),
                    bleed: None,
                    margins: None,
                    children: Vec::new(),
                    extensions: Vec::new(),
                },
            )]),
            nodes: BTreeMap::from([(
                node_id,
                Node {
                    kind: NodeKind::Shape,
                    header: NodeHeader {
                        id: node_id,
                        parent_id: page_id.into_canonical(),
                        bounds: RectEmu::new(
                            LengthEmu::new(10),
                            LengthEmu::new(20),
                            LengthEmu::new(300),
                            LengthEmu::new(400),
                        ),
                        transform: Affine2D::identity(),
                        source_refs: Vec::new(),
                        extensions: Vec::new(),
                    },
                    payload: PubResolvedNodePayload {
                        contents_seq_num: 7,
                        officeart_shape_type: None,
                        officeart_spid: None,
                        image_slot: None,
                        legacy_ole: None,
                        explicit_image_crop: None,
                        explicit_paint: pub_reader::PubExplicitShapePaintSource::default(),
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
                },
            )]),
            stories: BTreeMap::from([(
                story_id,
                Story {
                    id: story_id,
                    text: "Hello Viewer".to_owned(),
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
        }
    }

    #[test]
    fn grouped_legacy_image_projects_to_page_surface_without_mutating_graph_parent() {
        let mut graph = resolved_graph_fixture();
        let page_id = graph.document.pages[0];
        let group_id = *graph.nodes.keys().next().expect("fixture node");
        let image_id = NodeId::from_canonical(id(5));
        let image_bounds = RectEmu::new(
            LengthEmu::new(120),
            LengthEmu::new(240),
            LengthEmu::new(360),
            LengthEmu::new(480),
        );

        {
            let group = graph.nodes.get_mut(&group_id).expect("group node");
            group.kind = NodeKind::Group;
            group.payload.story_frame = None;
        }

        let mut image = graph.nodes[&group_id].clone();
        image.kind = NodeKind::ImageFrame;
        image.header.id = image_id;
        image.header.parent_id = group_id.into_canonical();
        image.header.bounds = image_bounds;
        image.payload.contents_seq_num = 99;
        image.payload.story_frame = None;
        graph.nodes.insert(image_id, image);

        let authoring =
            bounded_legacy_noquill_authoring_slice_from_resolved_pages(&graph, &[page_id])
                .expect("legacy authoring slice");
        let projected = authoring
            .node_geometry
            .iter()
            .find(|node| node.node_id == image_id)
            .expect("grouped IMAGE projected");

        assert_eq!(projected.parent_origin, page_id.into_canonical());
        assert_eq!(projected.bounds, image_bounds);
        assert_eq!(
            graph.nodes[&image_id].header.parent_id,
            group_id.into_canonical(),
            "Viewer projection must not rewrite canonical GROUP ownership"
        );
    }

    #[test]
    fn legacy_image_projection_rejects_non_group_ancestry() {
        let mut graph = resolved_graph_fixture();
        let page_id = graph.document.pages[0];
        let parent_id = *graph.nodes.keys().next().expect("fixture node");
        let image_id = NodeId::from_canonical(id(6));

        {
            let parent = graph.nodes.get_mut(&parent_id).expect("parent node");
            parent.kind = NodeKind::Shape;
            parent.payload.story_frame = None;
        }

        let mut image = graph.nodes[&parent_id].clone();
        image.kind = NodeKind::ImageFrame;
        image.header.id = image_id;
        image.header.parent_id = parent_id.into_canonical();
        image.payload.contents_seq_num = 100;
        graph.nodes.insert(image_id, image);

        let authoring =
            bounded_legacy_noquill_authoring_slice_from_resolved_pages(&graph, &[page_id])
                .expect("legacy authoring slice");
        assert!(
            authoring
                .node_geometry
                .iter()
                .all(|node| node.node_id != image_id),
            "only a pure GROUP ancestry may flatten a legacy IMAGE onto a page"
        );
    }

    #[test]
    fn inert_legacy_ole_preserves_geometry_without_shape_paint() {
        let mut graph = resolved_graph_fixture();
        let node_id = *graph.nodes.keys().next().expect("fixture node");
        let expected_bounds = {
            let node = graph.nodes.get_mut(&node_id).expect("fixture node");
            node.kind = NodeKind::Unsupported;
            node.payload.story_frame = None;
            node.payload.legacy_ole = Some(pub_reader::PubLegacyOleSource {
                storage_number: 73,
                raw_flag: 0x8000,
            });
            node.header.bounds
        };

        let authoring = bounded_authoring_slice_from_resolved(&graph).expect("authoring slice");
        assert_eq!(authoring.node_geometry.len(), 1);
        assert_eq!(authoring.node_geometry[0].node_id, node_id);
        assert_eq!(authoring.node_geometry[0].bounds, expected_bounds);

        let node = graph.nodes.get(&node_id).expect("fixture node");
        assert!(
            viewer_node_paint_from_canonical_bridge(node)
                .expect("paint projection")
                .is_none()
        );
    }

    fn minimal_legacy_ole_cached_presentation(stream_ordinal: u16) -> LegacyOleCachedPresentation {
        let mut data = Vec::new();
        data.extend_from_slice(&1_u16.to_le_bytes());
        data.extend_from_slice(&9_u16.to_le_bytes());
        data.extend_from_slice(&0x0300_u16.to_le_bytes());
        data.extend_from_slice(&12_u32.to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());

        let wmf = pub_reader::validate_wmf_metafile(&data).expect("minimal WMF");
        LegacyOleCachedPresentation {
            stream_path: format!("/Objects/Object 73/\u{2}OlePres{stream_ordinal:03}"),
            stream_name: format!("\u{2}OlePres{stream_ordinal:03}"),
            stream_ordinal,
            clipboard_format: 3,
            aspect: 1,
            lindex: u32::MAX,
            advf: 2,
            width: 8,
            height: 8,
            wmf,
            data,
        }
    }

    #[test]
    fn unique_valid_legacy_ole_preview_becomes_deterministic_png_overlay() {
        let source_hash = Sha256Digest::from_bytes([0xAB; 32]);
        let node_id = NodeId::from_canonical(id(42));
        let scan = LegacyOleCachedPresentationScan {
            presentations: vec![minimal_legacy_ole_cached_presentation(1)],
            diagnostics: Vec::new(),
        };
        let mut diagnostics = Vec::new();

        let image = viewer_legacy_ole_preview_image_from_scan(
            &source_hash,
            &[node_id],
            73,
            &scan,
            &mut diagnostics,
        )
        .expect("unique preview");

        assert!(diagnostics.is_empty());
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.node_ids, vec![node_id]);
        assert!(image.placements.is_empty());
        assert!(image.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

        let repeated = viewer_legacy_ole_preview_image_from_scan(
            &source_hash,
            &[node_id],
            73,
            &scan,
            &mut Vec::new(),
        )
        .expect("same unique preview");
        assert_eq!(repeated.resource_id, image.resource_id);
        assert_eq!(repeated.bytes, image.bytes);

        let same_wmf_other_ordinal = minimal_legacy_ole_cached_presentation(2);
        let equivalent_id =
            legacy_ole_preview_resource_id(&source_hash, 73, &same_wmf_other_ordinal.data)
                .expect("equivalent presentation identity");
        assert_eq!(equivalent_id, image.resource_id);

        let other_payload_id = legacy_ole_preview_resource_id(&source_hash, 73, b"different-wmf")
            .expect("different WMF identity");
        assert_ne!(other_payload_id, image.resource_id);
    }

    #[test]
    fn equivalent_legacy_ole_siblings_render_but_distinct_siblings_fail_closed() {
        let source_hash = Sha256Digest::from_bytes([0xCD; 32]);
        let node_id = NodeId::from_canonical(id(43));

        let equivalent = LegacyOleCachedPresentationScan {
            presentations: vec![
                minimal_legacy_ole_cached_presentation(1),
                minimal_legacy_ole_cached_presentation(2),
            ],
            diagnostics: Vec::new(),
        };
        let mut equivalent_diagnostics = Vec::new();
        assert!(
            viewer_legacy_ole_preview_image_from_scan(
                &source_hash,
                &[node_id],
                73,
                &equivalent,
                &mut equivalent_diagnostics,
            )
            .is_some()
        );
        assert!(equivalent_diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "viewer.legacy_ole.preview_equivalent_duplicates"
        }));

        let mut distinct = minimal_legacy_ole_cached_presentation(2);
        distinct.width += 1;
        let ambiguous = LegacyOleCachedPresentationScan {
            presentations: vec![minimal_legacy_ole_cached_presentation(1), distinct],
            diagnostics: Vec::new(),
        };
        let mut ambiguous_diagnostics = Vec::new();
        assert!(
            viewer_legacy_ole_preview_image_from_scan(
                &source_hash,
                &[node_id],
                73,
                &ambiguous,
                &mut ambiguous_diagnostics,
            )
            .is_none()
        );
        assert!(
            ambiguous_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "viewer.legacy_ole.preview_ambiguous")
        );

        let missing = LegacyOleCachedPresentationScan::default();
        let mut missing_diagnostics = Vec::new();
        assert!(
            viewer_legacy_ole_preview_image_from_scan(
                &source_hash,
                &[node_id],
                73,
                &missing,
                &mut missing_diagnostics,
            )
            .is_none()
        );
        assert!(
            missing_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "viewer.legacy_ole.preview_unavailable")
        );
    }

    #[test]
    fn malformed_legacy_ole_sibling_does_not_hide_one_valid_preview_or_leak_path() {
        let source_hash = Sha256Digest::from_bytes([0xEF; 32]);
        let node_id = NodeId::from_canonical(id(44));
        let scan = LegacyOleCachedPresentationScan {
            presentations: vec![minimal_legacy_ole_cached_presentation(1)],
            diagnostics: vec![pub_reader::LegacyOleCachedPresentationDiagnostic {
                stream_path: "/Objects/Object 73/private-carrier".to_owned(),
                stream_name: "private-carrier".to_owned(),
                reason: "private parser detail".to_owned(),
            }],
        };
        let mut diagnostics = Vec::new();

        assert!(
            viewer_legacy_ole_preview_image_from_scan(
                &source_hash,
                &[node_id],
                73,
                &scan,
                &mut diagnostics,
            )
            .is_some()
        );
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "viewer.legacy_ole.preview_sibling_rejected")
            .expect("source-neutral sibling diagnostic");
        assert!(!diagnostic.message.contains("Objects/Object"));
        assert!(!diagnostic.message.contains("private-carrier"));
        assert!(!diagnostic.message.contains("private parser detail"));
    }

    fn linked_resolved_graph_fixture(text: &str) -> PubResolvedGraph {
        let mut graph = resolved_graph_fixture();
        let page_id = graph.document.pages[0];
        let first_frame = *graph.nodes.keys().next().expect("fixture frame");
        let second_frame = NodeId::from_canonical(id(5));
        let story_id = *graph.stories.keys().next().expect("fixture story");
        let scalar_advance = VIEWER_FALLBACK_SCALAR_ADVANCE_EMU_V0_1;
        let line_height = VIEWER_FALLBACK_LINE_HEIGHT_EMU_V0_1;

        {
            let page = graph.pages.get_mut(&page_id).expect("fixture page");
            page.size = Size2D::new(
                LengthEmu::new(scalar_advance * 12),
                LengthEmu::new(line_height * 4),
            );
            page.children = vec![first_frame, second_frame];
        }

        let mut second_node = graph.nodes.get(&first_frame).expect("first frame").clone();
        second_node.header.id = second_frame;
        second_node.header.bounds = RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::new(line_height * 2),
            LengthEmu::new(scalar_advance * 4),
            LengthEmu::new(line_height),
        );
        second_node.payload.story_frame = Some(PubResolvedStoryFrame {
            story_id: Some(story_id),
            ordinal: 1,
            previous_frame: Some(first_frame),
            next_frame: None,
            vertical_alignment: None,
        });

        {
            let first = graph.nodes.get_mut(&first_frame).expect("first frame");
            first.header.bounds = RectEmu::new(
                LengthEmu::ZERO,
                LengthEmu::ZERO,
                LengthEmu::new(scalar_advance * 4),
                LengthEmu::new(line_height),
            );
            first.payload.story_frame = Some(PubResolvedStoryFrame {
                story_id: Some(story_id),
                ordinal: 0,
                previous_frame: None,
                next_frame: Some(second_frame),
                vertical_alignment: None,
            });
        }
        graph.nodes.insert(second_frame, second_node);
        graph
            .stories
            .get_mut(&story_id)
            .expect("fixture story")
            .text = text.to_owned();
        graph
    }

    #[test]
    fn image_source_window_projects_signed_q16_crop_without_intrinsic_size_guessing() {
        let fill = PubExplicitImageCropSource {
            top_raw: Some(0x0000_5988),
            bottom_raw: Some(0x0000_5988),
            left_raw: Some(0),
            right_raw: Some(0),
            ambiguous: false,
        };
        assert_eq!(
            viewer_image_source_window_v1(Some(&fill)).expect("fill crop"),
            Some(ViewerImageSourceWindowV1 {
                left_q16: 0,
                top_q16: 0x5988,
                right_q16: VIEWER_IMAGE_SOURCE_Q16_ONE,
                bottom_q16: VIEWER_IMAGE_SOURCE_Q16_ONE - 0x5988,
            })
        );

        let fit = PubExplicitImageCropSource {
            top_raw: Some(0),
            bottom_raw: Some(0),
            left_raw: Some(0xFFFE_D618),
            right_raw: Some(0xFFFE_D618),
            ambiguous: false,
        };
        let fit_window = viewer_image_source_window_v1(Some(&fit))
            .expect("fit crop")
            .expect("fit source window");
        assert!(fit_window.left_q16 < 0);
        assert!(fit_window.right_q16 > VIEWER_IMAGE_SOURCE_Q16_ONE);
        assert_eq!(
            fit_window.right_q16 - VIEWER_IMAGE_SOURCE_Q16_ONE,
            -fit_window.left_q16
        );
    }

    #[test]
    fn image_source_window_fails_closed_on_ambiguous_or_empty_windows() {
        let ambiguous = PubExplicitImageCropSource {
            top_raw: Some(0),
            bottom_raw: None,
            left_raw: None,
            right_raw: None,
            ambiguous: true,
        };
        assert!(viewer_image_source_window_v1(Some(&ambiguous)).is_err());

        let collapsed = PubExplicitImageCropSource {
            top_raw: None,
            bottom_raw: None,
            left_raw: Some(40_000),
            right_raw: Some(40_000),
            ambiguous: false,
        };
        assert!(viewer_image_source_window_v1(Some(&collapsed)).is_err());
    }

    #[test]
    fn normative_effective_paint_bridge_precedes_explicit_fopt_projection() {
        let mut graph = resolved_graph_fixture();
        let node_id = *graph.nodes.keys().next().expect("fixture node");
        let node = graph.nodes.get_mut(&node_id).expect("fixture node");
        node.payload.explicit_paint = pub_reader::PubExplicitShapePaintSource {
            fill: pub_reader::PubExplicitFillSource {
                solid: true,
                color_rgb: Some([1, 2, 3]),
                visible: Some(true),
            },
            line: pub_reader::PubExplicitLineSource::default(),
        };
        node.payload.effective_paint = Some(pub_reader::PubEffectiveShapePaintSource {
            fill: pub_reader::PubEffectiveFillSource {
                solid: Some(pub_reader::PubEffectivePaintValue {
                    value: true,
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
                color_rgb: Some(pub_reader::PubEffectivePaintValue {
                    value: [0xFF, 0xFF, 0xFF],
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
                visible: Some(pub_reader::PubEffectivePaintValue {
                    value: true,
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
            },
            line: pub_reader::PubEffectiveLineSource {
                color_rgb: Some(pub_reader::PubEffectivePaintValue {
                    value: [0, 0, 0],
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
                width_emu: Some(pub_reader::PubEffectivePaintValue {
                    value: 9_525,
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
                visible: Some(pub_reader::PubEffectivePaintValue {
                    value: true,
                    authority: PubEffectivePaintAuthority::NormativeDefault,
                    source: None,
                }),
            },
        });

        let paint = viewer_node_paint_from_canonical_bridge(node)
            .expect("bridge projection")
            .expect("effective paint");
        assert_eq!(paint.solid_fill_rgb, Some([0xFF, 0xFF, 0xFF]));
        assert_eq!(
            paint.solid_line,
            Some(ViewerSolidLine {
                rgb: [0, 0, 0],
                width_emu: 9_525,
            })
        );
    }

    #[test]
    fn created_text_box_scene_sync_tracks_create_undo_redo_without_touching_baseline_nodes() {
        let mut graph = resolved_graph_fixture();
        let projection =
            project_bounded(bounded_authoring_slice_from_resolved(&graph).expect("projection"));
        let baseline_scene =
            resolve_bounded_geometry(&projection, viewer_geometry_environment_v0_1())
                .expect("baseline scene");
        let baseline_nodes = baseline_scene.nodes.clone();
        let source_hash = graph.source.source_hash;
        let mut visual = ViewerGeometryDocument {
            schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
            document: ViewerDocument {
                schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
                source: ViewerSource {
                    format: "pub".to_owned(),
                    format_version: Some("0x2c".to_owned()),
                    source_hash,
                    byte_len: 1,
                },
                pages: Vec::new(),
                stories: Vec::new(),
                diagnostics: Vec::new(),
            },
            scene: baseline_scene,
            paints: Vec::new(),
            story_frames: Vec::new(),
            text_fragments: Vec::new(),
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            text_color_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            #[cfg(feature = "cmo-slot-compose")]
            projected_instances: Vec::new(),
            images: Vec::new(),
        };

        let page_id = graph.document.pages[0];
        let node_id = NodeId::from_canonical(id(9));
        let story_id = StoryId::from_canonical(id(10));
        let bounds = RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(300),
            LengthEmu::new(400),
        );
        let story = Story {
            id: story_id,
            text: String::new(),
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs: Vec::new(),
        };
        let node = Node {
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
                contents_seq_num: 0,
                officeart_shape_type: None,
                officeart_spid: None,
                image_slot: None,
                legacy_ole: None,
                explicit_image_crop: None,
                explicit_paint: pub_reader::PubExplicitShapePaintSource::default(),
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
        };

        graph.stories.insert(story_id, story.clone());
        graph.nodes.insert(node_id, node.clone());
        graph
            .pages
            .get_mut(&page_id)
            .expect("page")
            .children
            .push(node_id);

        let synced = visual
            .sync_editor_created_text_box_scene_nodes(&graph, &[node_id], &BTreeSet::new())
            .expect("create sync");
        let created = visual
            .scene
            .nodes
            .iter()
            .find(|scene_node| scene_node.origin == node_id)
            .expect("created TextBox scene node");
        assert_eq!(created.parent_origin, page_id.into_canonical());
        assert_eq!(created.bounds, bounds);
        assert_eq!(
            visual
                .scene
                .nodes
                .iter()
                .filter(|scene_node| !synced.contains(&scene_node.origin))
                .cloned()
                .collect::<Vec<_>>(),
            baseline_nodes
        );

        graph.nodes.remove(&node_id);
        graph.stories.remove(&story_id);
        graph
            .pages
            .get_mut(&page_id)
            .expect("page")
            .children
            .retain(|child| *child != node_id);
        let after_undo = visual
            .sync_editor_created_text_box_scene_nodes(&graph, &[], &synced)
            .expect("undo sync");
        assert!(after_undo.is_empty());
        assert_eq!(visual.scene.nodes, baseline_nodes);

        graph.stories.insert(story_id, story);
        graph.nodes.insert(node_id, node);
        graph
            .pages
            .get_mut(&page_id)
            .expect("page")
            .children
            .push(node_id);
        let after_redo = visual
            .sync_editor_created_text_box_scene_nodes(&graph, &[node_id], &after_undo)
            .expect("redo sync");
        assert_eq!(after_redo, BTreeSet::from([node_id]));
        assert_eq!(
            visual
                .scene
                .nodes
                .iter()
                .find(|scene_node| scene_node.origin == node_id)
                .expect("redo node")
                .bounds,
            bounds
        );
        assert_eq!(visual.document.source.source_hash, source_hash);
    }

    #[test]
    fn created_text_box_scene_sync_fails_closed_on_unproven_source_backed_node() {
        let mut graph = resolved_graph_fixture();
        let projection =
            project_bounded(bounded_authoring_slice_from_resolved(&graph).expect("projection"));
        let scene = resolve_bounded_geometry(&projection, viewer_geometry_environment_v0_1())
            .expect("scene");
        let source_hash = graph.source.source_hash;
        let existing = *graph.nodes.keys().next().expect("fixture node");
        let mut visual = ViewerGeometryDocument {
            schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
            document: ViewerDocument {
                schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
                source: ViewerSource {
                    format: "pub".to_owned(),
                    format_version: Some("0x2c".to_owned()),
                    source_hash,
                    byte_len: 1,
                },
                pages: Vec::new(),
                stories: Vec::new(),
                diagnostics: Vec::new(),
            },
            scene,
            paints: Vec::new(),
            story_frames: Vec::new(),
            text_fragments: Vec::new(),
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            text_color_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            #[cfg(feature = "cmo-slot-compose")]
            projected_instances: Vec::new(),
            images: Vec::new(),
        };
        let before = visual.scene.nodes.clone();

        graph.nodes.get_mut(&existing).expect("fixture node").kind = NodeKind::TextFrame;
        assert!(
            visual
                .sync_editor_created_text_box_scene_nodes(&graph, &[existing], &BTreeSet::new(),)
                .is_err()
        );
        assert_eq!(visual.scene.nodes, before);
    }

    #[test]
    fn source_hash_matches_sha256_known_answer() {
        let expected: Sha256Digest =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                .parse()
                .expect("known SHA-256 should parse");

        assert_eq!(sha256_digest(b"abc").unwrap(), expected);
    }

    #[test]
    fn bridge_diagnostic_is_mapped_to_stable_product_vocabulary() {
        let mapped =
            map_bridge_diagnostic(&PubBridgeDiagnostic::MissingEscherGeometry { seq_num: 42 });

        assert_eq!(mapped.code, "viewer.geometry.missing");
        assert_eq!(mapped.severity, ViewerDiagnosticSeverity::FidelityWarning);
        assert!(!mapped.message.contains("Escher"));
        assert!(!mapped.message.contains("seq_num"));
    }

    #[test]
    fn grouped_image_projection_uses_source_neutral_product_vocabulary() {
        let mapped = map_bridge_diagnostic(&PubBridgeDiagnostic::GroupedImageProjected {
            seq_num: 42,
            depth: 1,
        });

        assert_eq!(mapped.code, "viewer.geometry.grouped_image_projected");
        assert_eq!(mapped.severity, ViewerDiagnosticSeverity::Info);
        assert!(mapped.message.contains("grouped image shape"));
        assert!(!mapped.message.contains("seq_num"));
        assert!(!mapped.message.contains("42"));
    }

    #[test]
    fn equivalent_margins_are_reported_without_fidelity_loss() {
        let mapped = map_bridge_diagnostic(&PubBridgeDiagnostic::EquivalentMarginsPageExtents {
            count: 8,
            width_emu: 7_772_400,
            height_emu: 10_058_400,
        });

        assert_eq!(mapped.code, "viewer.page_extent.equivalent_source_records");
        assert_eq!(mapped.severity, ViewerDiagnosticSeverity::Info);
        assert!(mapped.message.contains("agree exactly"));
    }

    #[test]
    fn mismatched_mcld_is_reported_as_bounded_fidelity_loss() {
        let mapped = map_bridge_diagnostic(&PubBridgeDiagnostic::McldRecordCountMismatch {
            record_count: 44,
            record_id_count: 4,
        });

        assert_eq!(mapped.code, "viewer.table.mcld_layout_metrics_unavailable");
        assert_eq!(mapped.severity, ViewerDiagnosticSeverity::FidelityWarning);
        assert!(
            mapped
                .message
                .contains("core document content remains available")
        );
    }

    #[test]
    fn unavailable_color_scheme_is_reported_as_bounded_fidelity_loss() {
        let mapped =
            map_bridge_diagnostic(&PubBridgeDiagnostic::ColorSchemeProjectionUnavailable {
                reason: "synthetic control".into(),
            });

        assert_eq!(mapped.code, "viewer.paint.color_scheme_unavailable");
        assert_eq!(mapped.severity, ViewerDiagnosticSeverity::FidelityWarning);
        assert!(mapped.message.contains("scheme-indexed shape colors"));
        assert!(!mapped.message.contains("synthetic control"));
    }

    #[test]
    fn viewer_diagnostic_json_does_not_expose_source_carrier_fields() {
        let mapped = map_resolve_diagnostic(&PubResolveDiagnostic::MissingStoryIdentity {
            node_id: pub_model::NodeId::from_canonical(pub_model::CanonicalId::from_bytes(
                [0x11; 16],
            )),
            text_id: 9,
        });
        let json = serde_json::to_string(&mapped).unwrap();

        assert!(json.contains("viewer.text.story_identity_missing"));
        assert!(!json.contains("text_id"));
        assert!(!json.contains("node_id"));
    }

    #[test]
    fn resolved_graph_bridges_to_existing_layout_scene() {
        let graph = resolved_graph_fixture();
        let slice = bounded_authoring_slice_from_resolved(&graph).unwrap();
        let projection = project_bounded(slice);
        let scene =
            resolve_bounded_geometry(&projection, viewer_geometry_environment_v0_1()).unwrap();

        assert_eq!(scene.surfaces.len(), 1);
        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.surfaces[0].origin, graph.document.pages[0]);
        assert_eq!(
            scene.nodes[0].parent_origin,
            graph.document.pages[0].into_canonical()
        );
        assert_eq!(scene.diagnostics.len(), 1);
        assert_eq!(
            scene.diagnostics[0].code,
            "story_text_layout_not_implemented"
        );
    }

    #[test]
    fn fidelity_status_is_supported_without_fidelity_warnings() {
        let mut document = ViewerDocument {
            schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
            source: ViewerSource {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                source_hash: Sha256Digest::from_bytes([0x22; 32]),
                byte_len: 123,
            },
            pages: Vec::new(),
            stories: Vec::new(),
            diagnostics: vec![ViewerDiagnostic {
                code: "viewer.page_list.special_entry".to_owned(),
                severity: ViewerDiagnosticSeverity::Info,
                message: "Informational diagnostic.".to_owned(),
            }],
        };

        assert_eq!(document.fidelity_status(), ViewerFidelityStatus::Supported);

        document.diagnostics.push(ViewerDiagnostic {
            code: "viewer.geometry.missing".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: "Known visual limitation.".to_owned(),
        });

        assert_eq!(document.fidelity_status(), ViewerFidelityStatus::Partial);
    }

    #[test]
    fn unsupported_status_is_not_derived_from_an_open_document() {
        let document = ViewerDocument {
            schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
            source: ViewerSource {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                source_hash: Sha256Digest::from_bytes([0x33; 32]),
                byte_len: 0,
            },
            pages: Vec::new(),
            stories: Vec::new(),
            diagnostics: Vec::new(),
        };

        assert_ne!(
            document.fidelity_status(),
            ViewerFidelityStatus::Unsupported
        );
    }

    #[test]
    fn semantic_search_returns_stable_story_ranges_in_document_order() {
        let source_hash = Sha256Digest::from_bytes([0x44; 32]);
        let first_story = StoryId::from_canonical(id(4));
        let second_story = StoryId::from_canonical(id(5));
        let document = ViewerDocument {
            schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
            source: ViewerSource {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                source_hash,
                byte_len: 10,
            },
            pages: Vec::new(),
            stories: vec![
                ViewerStory {
                    id: first_story,
                    text: "alpha beta alpha".to_owned(),
                },
                ViewerStory {
                    id: second_story,
                    text: "alpha".to_owned(),
                },
            ],
            diagnostics: Vec::new(),
        };

        let matches = document.search_text("alpha");

        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].story_id, first_story);
        assert_eq!((matches[0].start_byte, matches[0].end_byte), (0, 5));
        assert_eq!(matches[0].text, "alpha");
        assert_eq!((matches[1].start_byte, matches[1].end_byte), (11, 16));
        assert_eq!(matches[2].story_id, second_story);
        assert_eq!((matches[2].start_byte, matches[2].end_byte), (0, 5));
    }

    #[test]
    fn semantic_search_empty_query_returns_no_matches() {
        let document = ViewerDocument {
            schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
            source: ViewerSource {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                source_hash: Sha256Digest::from_bytes([0x55; 32]),
                byte_len: 5,
            },
            pages: Vec::new(),
            stories: vec![ViewerStory {
                id: StoryId::from_canonical(id(6)),
                text: "hello".to_owned(),
            }],
            diagnostics: Vec::new(),
        };

        assert!(document.search_text("").is_empty());
    }

    #[test]
    fn search_match_contract_does_not_claim_page_ownership() {
        let json = serde_json::to_value(ViewerTextMatch {
            story_id: StoryId::from_canonical(id(7)),
            start_byte: 1,
            end_byte: 3,
            text: "bc".to_owned(),
        })
        .unwrap();

        let object = json
            .as_object()
            .expect("ViewerTextMatch must serialize as object");
        assert!(object.contains_key("story_id"));
        assert!(!object.contains_key("page_id"));
        assert!(!object.contains_key("page"));
    }

    fn linked_text_projection(explicit_links: bool) -> pub_layout::BoundedLayoutProjection {
        let page_id = PageId::from_canonical(id(40));
        let first_frame = NodeId::from_canonical(id(41));
        let second_frame = NodeId::from_canonical(id(42));
        let story_id = StoryId::from_canonical(id(43));
        let scalar_advance = VIEWER_FALLBACK_SCALAR_ADVANCE_EMU_V0_1;
        let line_height = VIEWER_FALLBACK_LINE_HEIGHT_EMU_V0_1;

        project_bounded(BoundedAuthoringSlice {
            pages: vec![Page {
                id: page_id,
                size: Size2D::new(
                    LengthEmu::new(scalar_advance * 12),
                    LengthEmu::new(line_height * 4),
                ),
                bleed: None,
                margins: None,
                children: vec![first_frame, second_frame],
                extensions: Vec::new(),
            }],
            node_geometry: vec![
                BoundedNodeGeometryInput {
                    node_id: first_frame,
                    parent_origin: page_id.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::ZERO,
                        LengthEmu::new(scalar_advance * 4),
                        LengthEmu::new(line_height),
                    ),
                    transform: Affine2D::identity(),
                },
                BoundedNodeGeometryInput {
                    node_id: second_frame,
                    parent_origin: page_id.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::new(line_height * 2),
                        LengthEmu::new(scalar_advance * 4),
                        LengthEmu::new(line_height),
                    ),
                    transform: Affine2D::identity(),
                },
            ],
            stories: vec![Story {
                id: story_id,
                text: "ABCDEFG".to_owned(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            }],
            story_frames: vec![
                StoryFrame {
                    story_id,
                    frame_id: first_frame,
                    ordinal: 0,
                    previous: None,
                    next: explicit_links.then_some(second_frame),
                },
                StoryFrame {
                    story_id,
                    frame_id: second_frame,
                    ordinal: 1,
                    previous: explicit_links.then_some(first_frame),
                    next: None,
                },
            ],
            tables: Vec::new(),
            guides: Vec::new(),
            unknown_layout_state: Vec::new(),
        })
    }

    #[test]
    fn viewer_fallback_flow_materializes_explicit_linked_story_frames() {
        let projection = linked_text_projection(true);
        let (fragments, diagnostics) =
            resolve_viewer_text_fragments(&projection).expect("fallback flow should resolve");

        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[0].text, "ABCD");
        assert_eq!(fragments[0].scalar_start, 0);
        assert_eq!(fragments[0].scalar_end, 4);
        assert_eq!(fragments[1].text, "EFG");
        assert_eq!(fragments[1].scalar_start, 4);
        assert_eq!(fragments[1].scalar_end, 7);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "story_overset")
        );
    }

    #[test]
    fn viewer_fallback_flow_does_not_invent_ordinal_only_chain() {
        let projection = linked_text_projection(false);
        let (fragments, diagnostics) =
            resolve_viewer_text_fragments(&projection).expect("fallback flow should resolve");

        assert!(fragments.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "shared_story_without_explicit_flow" })
        );
    }

    #[test]
    fn viewer_text_fragment_contract_is_source_neutral() {
        let fragment = ViewerTextFragment {
            story_id: StoryId::from_canonical(id(50)),
            frame_id: NodeId::from_canonical(id(51)),
            scalar_start: 2,
            scalar_end: 5,
            text: "abc".to_owned(),
            line_count: 1,
        };
        let json = serde_json::to_string(&fragment).expect("serialize Viewer text fragment");

        assert!(json.contains("story_id"));
        assert!(json.contains("frame_id"));
        assert!(json.contains("scalar_start"));
        for forbidden in ["Quill", "FDPC", "BTEC", "Contents", "Escher", "offset"] {
            assert!(
                !json.contains(forbidden),
                "Viewer text fragment must not expose parser-private {forbidden}"
            );
        }
    }

    #[test]
    fn viewer_text_projection_refresh_uses_current_resolved_story_state() {
        let mut graph = resolved_graph_fixture();
        let page_id = graph.document.pages[0];
        let node_id = *graph.nodes.keys().next().expect("fixture node");
        let story_id = *graph.stories.keys().next().expect("fixture story");

        graph.pages.get_mut(&page_id).expect("fixture page").size =
            Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(2_000_000));
        graph
            .pages
            .get_mut(&page_id)
            .expect("fixture page")
            .children = vec![node_id];
        graph
            .nodes
            .get_mut(&node_id)
            .expect("fixture node")
            .header
            .bounds = RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::ZERO,
            LengthEmu::new(1_000_000),
            LengthEmu::new(1_000_000),
        );

        let projection =
            project_bounded(bounded_authoring_slice_from_resolved(&graph).expect("projection"));
        let scene = resolve_bounded_geometry(&projection, viewer_geometry_environment_v0_1())
            .expect("scene");
        let (initial_fragments, _) =
            resolve_viewer_text_fragments(&projection).expect("initial text flow");
        assert!(!initial_fragments.is_empty());

        let source_hash = graph.source.source_hash;
        let mut visual = ViewerGeometryDocument {
            schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
            document: ViewerDocument {
                schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
                source: ViewerSource {
                    format: "pub".to_owned(),
                    format_version: Some("0x2c".to_owned()),
                    source_hash,
                    byte_len: 1,
                },
                pages: vec![ViewerPage {
                    index: 1,
                    id: page_id,
                    width_emu: graph.pages[&page_id].size.width.get(),
                    height_emu: graph.pages[&page_id].size.height.get(),
                }],
                stories: vec![ViewerStory {
                    id: story_id,
                    text: graph.stories[&story_id].text.clone(),
                }],
                diagnostics: vec![viewer_fallback_flow_metrics_diagnostic()],
            },
            scene,
            paints: Vec::new(),
            story_frames: Vec::new(),
            text_fragments: initial_fragments,
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            text_color_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            #[cfg(feature = "cmo-slot-compose")]
            projected_instances: Vec::new(),
            images: Vec::new(),
        };

        graph
            .stories
            .get_mut(&story_id)
            .expect("fixture story")
            .text = "Changed after edit".to_owned();

        visual
            .refresh_text_projection_from_resolved(&graph)
            .expect("refresh current editor graph");

        assert_eq!(visual.document.stories[0].text, "Changed after edit");
        assert_eq!(
            visual
                .text_fragments
                .iter()
                .map(|fragment| fragment.text.as_str())
                .collect::<String>(),
            "Changed after edit"
        );
        assert_eq!(visual.story_frames.len(), 1);
        assert_eq!(
            visual
                .document
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "viewer.text.fallback_flow_metrics")
                .count(),
            1
        );
    }

    #[test]
    fn viewer_text_projection_refresh_reflows_same_explicit_linked_chain() {
        let mut graph = linked_resolved_graph_fixture("ABCDEFGHI");
        let page_id = graph.document.pages[0];
        let story_id = *graph.stories.keys().next().expect("fixture story");
        let projection =
            project_bounded(bounded_authoring_slice_from_resolved(&graph).expect("projection"));
        let scene = resolve_bounded_geometry(&projection, viewer_geometry_environment_v0_1())
            .expect("scene");
        let (initial_fragments, initial_flow_diagnostics) =
            resolve_viewer_text_fragments(&projection).expect("initial linked flow");
        assert_eq!(
            initial_fragments
                .iter()
                .map(|fragment| fragment.text.as_str())
                .collect::<String>(),
            "ABCDEFGH"
        );
        assert!(
            initial_flow_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "story_overset")
        );

        let initial_frames = projection
            .story_frames
            .iter()
            .map(|frame| ViewerStoryFrame {
                story_id: frame.story_origin,
                frame_id: frame.frame_origin,
                ordinal: frame.ordinal,
                text_content_bounds: None,
                vertical_alignment: None,
            })
            .collect::<Vec<_>>();
        let mut diagnostics = initial_flow_diagnostics
            .iter()
            .map(map_scene_diagnostic)
            .collect::<Vec<_>>();
        diagnostics.push(viewer_fallback_flow_metrics_diagnostic());
        normalize_diagnostics(&mut diagnostics);

        let source_hash = graph.source.source_hash;
        let mut visual = ViewerGeometryDocument {
            schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
            document: ViewerDocument {
                schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
                source: ViewerSource {
                    format: "pub".to_owned(),
                    format_version: Some("0x2c".to_owned()),
                    source_hash,
                    byte_len: 1,
                },
                pages: vec![ViewerPage {
                    index: 1,
                    id: page_id,
                    width_emu: graph.pages[&page_id].size.width.get(),
                    height_emu: graph.pages[&page_id].size.height.get(),
                }],
                stories: vec![ViewerStory {
                    id: story_id,
                    text: "ABCDEFGHI".to_owned(),
                }],
                diagnostics,
            },
            scene,
            paints: Vec::new(),
            story_frames: initial_frames.clone(),
            text_fragments: initial_fragments,
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            text_color_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            #[cfg(feature = "cmo-slot-compose")]
            projected_instances: Vec::new(),
            images: Vec::new(),
        };

        graph
            .stories
            .get_mut(&story_id)
            .expect("fixture story")
            .text = "XYZ1234".to_owned();

        visual
            .refresh_text_projection_from_resolved(&graph)
            .expect("refresh linked Story");

        assert_eq!(
            visual
                .text_fragments
                .iter()
                .map(|fragment| fragment.text.as_str())
                .collect::<String>(),
            "XYZ1234"
        );
        assert_eq!(visual.story_frames, initial_frames);
        assert!(
            !visual
                .document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "viewer.text.fallback_overset")
        );
    }

    #[test]
    fn viewer_text_projection_refresh_rejects_source_identity_change_transactionally() {
        let graph = resolved_graph_fixture();
        let source_hash = graph.source.source_hash;
        let mut visual = ViewerGeometryDocument {
            schema_version: VIEWER_GEOMETRY_SCHEMA_V0_1.to_owned(),
            document: ViewerDocument {
                schema_version: VIEWER_DOCUMENT_SCHEMA_V0_1.to_owned(),
                source: ViewerSource {
                    format: "pub".to_owned(),
                    format_version: Some("0x2c".to_owned()),
                    source_hash,
                    byte_len: 1,
                },
                pages: Vec::new(),
                stories: vec![ViewerStory {
                    id: StoryId::from_canonical(id(99)),
                    text: "keep me".to_owned(),
                }],
                diagnostics: Vec::new(),
            },
            scene: resolve_bounded_geometry(
                &project_bounded(
                    bounded_authoring_slice_from_resolved(&graph).expect("projection"),
                ),
                viewer_geometry_environment_v0_1(),
            )
            .expect("scene"),
            paints: Vec::new(),
            story_frames: Vec::new(),
            text_fragments: Vec::new(),
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            text_color_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            #[cfg(feature = "cmo-slot-compose")]
            projected_instances: Vec::new(),
            images: Vec::new(),
        };
        let before = visual.clone();
        visual.document.source.source_hash = Sha256Digest::from_bytes([0xCD; 32]);
        let mismatched_before = visual.clone();

        assert!(
            visual
                .refresh_text_projection_from_resolved(&graph)
                .is_err()
        );
        assert_eq!(visual, mismatched_before);
        assert_ne!(visual, before);
    }

    #[test]
    fn source_page_paint_order_reorders_only_known_direct_node_slots() {
        let page_id = PageId::from_canonical(id(40));
        let other_page_id = PageId::from_canonical(id(41));
        let node = |byte: u8, parent: PageId| ResolvedPhysicalNode {
            origin: NodeId::from_canonical(id(byte)),
            parent_origin: parent.into_canonical(),
            bounds: RectEmu::new(
                LengthEmu::ZERO,
                LengthEmu::ZERO,
                LengthEmu::new(10),
                LengthEmu::new(10),
            ),
            transform: Affine2D::identity(),
        };

        let mut nodes = vec![
            node(1, page_id),
            node(90, other_page_id),
            node(2, page_id),
            node(91, page_id),
            node(3, page_id),
        ];
        let order = ViewerPagePaintOrderV1 {
            page_id,
            node_ids: vec![
                NodeId::from_canonical(id(3)),
                NodeId::from_canonical(id(1)),
                NodeId::from_canonical(id(2)),
            ],
        };

        let stats = apply_known_source_page_paint_orders_to_scene_nodes_v1(&mut nodes, &[order]);

        assert_eq!(stats.page_order_count, 1);
        assert_eq!(stats.known_node_count, 3);
        assert_eq!(
            nodes.iter().map(|node| node.origin).collect::<Vec<_>>(),
            vec![
                NodeId::from_canonical(id(3)),
                NodeId::from_canonical(id(90)),
                NodeId::from_canonical(id(1)),
                NodeId::from_canonical(id(91)),
                NodeId::from_canonical(id(2)),
            ],
            "only covered direct nodes may move; unknown/other-page nodes keep their slots"
        );
    }

    #[test]
    fn invalid_or_wrong_page_source_order_stays_fail_closed() {
        let page_id = PageId::from_canonical(id(50));
        let other_page_id = PageId::from_canonical(id(51));
        let first = NodeId::from_canonical(id(4));
        let second = NodeId::from_canonical(id(5));
        let original = vec![
            ResolvedPhysicalNode {
                origin: first,
                parent_origin: other_page_id.into_canonical(),
                bounds: RectEmu::new(
                    LengthEmu::ZERO,
                    LengthEmu::ZERO,
                    LengthEmu::new(10),
                    LengthEmu::new(10),
                ),
                transform: Affine2D::identity(),
            },
            ResolvedPhysicalNode {
                origin: second,
                parent_origin: page_id.into_canonical(),
                bounds: RectEmu::new(
                    LengthEmu::ZERO,
                    LengthEmu::ZERO,
                    LengthEmu::new(10),
                    LengthEmu::new(10),
                ),
                transform: Affine2D::identity(),
            },
        ];

        let mut wrong_page_nodes = original.clone();
        let wrong_page = ViewerPagePaintOrderV1 {
            page_id,
            node_ids: vec![second, first],
        };
        assert_eq!(
            apply_known_source_page_paint_orders_to_scene_nodes_v1(
                &mut wrong_page_nodes,
                &[wrong_page],
            ),
            ViewerSourcePaintOrderApplicationStatsV1::default()
        );
        assert_eq!(wrong_page_nodes, original);

        let mut duplicate_order_nodes = original.clone();
        let duplicate = ViewerPagePaintOrderV1 {
            page_id: other_page_id,
            node_ids: vec![first, first],
        };
        assert_eq!(
            apply_known_source_page_paint_orders_to_scene_nodes_v1(
                &mut duplicate_order_nodes,
                &[duplicate],
            ),
            ViewerSourcePaintOrderApplicationStatsV1::default()
        );
        assert_eq!(duplicate_order_nodes, original);
    }

    #[test]
    fn viewer_geometry_environment_is_explicit_and_stable() {
        assert_eq!(
            viewer_geometry_environment_v0_1(),
            viewer_geometry_environment_v0_1()
        );
    }
}

#[cfg(test)]
mod legacy22_noquill_exact_product_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires CHAPTERA_OPNHOUS_PUB exact public Publisher97 fixture"]
    fn exact_opnhous_publisher97_noquill_product_open() {
        let path = std::env::var_os("CHAPTERA_OPNHOUS_PUB")
            .map(PathBuf::from)
            .expect("CHAPTERA_OPNHOUS_PUB is required");
        let before = fs::read(&path).expect("read exact OPNHOUS fixture");
        assert_eq!(before.len(), 11_264);
        assert_eq!(
            sha256_digest(&before).unwrap().to_string(),
            "0c74bed1b862f4603a77567f817ad22bf1f7c42eb5afbee0c907732953534b5c"
        );

        let bundle = open_pub_bundle(&before, viewer_geometry_environment_v0_1())
            .expect("Publisher97 no-Quill fixture must open through product bundle boundary");
        assert_eq!(
            bundle.resolved_graph.source.source_hash.to_string(),
            sha256_digest(&before).unwrap().to_string(),
            "bundle resolved graph must preserve exact source identity"
        );
        let visual = &bundle.geometry;

        assert_eq!(
            visual.document.source.format_version.as_deref(),
            Some("0x22-noquill")
        );
        assert_eq!(
            visual.document.pages.len(),
            4,
            "authoritative Publisher97 page list"
        );
        assert_eq!(
            visual.scene.surfaces.len(),
            4,
            "one surface per admitted page"
        );
        assert!(!visual.scene.nodes.is_empty(), "grounded legacy geometry");
        assert!(
            !visual.document.search_text("OPEN HOUSE").is_empty(),
            "no-Quill text must be searchable"
        );
        assert!(
            !visual.document.search_text("date of event").is_empty(),
            "second grounded literal must be searchable"
        );
        assert!(
            !visual.document.search_text("street address").is_empty(),
            "third grounded literal must be searchable"
        );
        assert_eq!(
            visual.document.fidelity_status(),
            ViewerFidelityStatus::Partial
        );

        let after = fs::read(&path).expect("re-read exact OPNHOUS fixture");
        assert_eq!(after, before, "Reader path mutated source PUB");
    }
}

#[cfg(test)]
mod legacy22_exact_product_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn exact_fixture(env_name: &str, expected_sha256: &str) {
        let path = std::env::var_os(env_name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("{env_name} is required"));
        let before = fs::read(&path).expect("read exact legacy fixture");
        assert_eq!(sha256_digest(&before).unwrap().to_string(), expected_sha256);

        let visual = open_pub_geometry(&before, viewer_geometry_environment_v0_1())
            .expect("legacy fixture must open through family-dispatched Viewer product boundary");

        assert_eq!(
            visual.document.source.format_version.as_deref(),
            Some("0x22-quill")
        );
        assert!(!visual.document.pages.is_empty(), "legacy Viewer pages");
        assert!(!visual.document.stories.is_empty(), "legacy Quill stories");
        assert!(
            !visual.scene.nodes.is_empty(),
            "legacy grounded scene nodes"
        );
        assert_eq!(
            visual.document.fidelity_status(),
            ViewerFidelityStatus::Partial,
            "bounded legacy V1 must stay explicit about omitted object classes"
        );

        let after = fs::read(&path).expect("re-read exact legacy fixture");
        assert_eq!(after, before, "legacy Reader path mutated source PUB");
    }

    #[test]
    #[ignore = "requires CHAPTERA_SAMPLE98_PUB exact public fixture"]
    fn exact_sample98_legacy_quill_product_open() {
        exact_fixture(
            "CHAPTERA_SAMPLE98_PUB",
            "8912f295ff0cd1f55b873b3b98a182218f33c420877e025d2f0fd795f12dd0c0",
        );
    }

    #[test]
    #[ignore = "requires CHAPTERA_SAMPLE2000_PUB exact public fixture"]
    fn exact_sample2000_legacy_quill_product_open() {
        exact_fixture(
            "CHAPTERA_SAMPLE2000_PUB",
            "40701ca47b26d04771cdd58467764e9ab39d3da69b59fbc529afd16d263d2f86",
        );
    }
}

#[cfg(test)]
mod standard_print_service_tail_exact_product_tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn exact_standard_print_fixture(env_name: &str, expected_sha256: &str, expected_pages: usize) {
        let path = std::env::var_os(env_name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("{env_name} is required"));
        let before = fs::read(&path).expect("read exact standard-print fixture");
        assert_eq!(sha256_digest(&before).unwrap().to_string(), expected_sha256);

        let document = open_mature_0x2c(&before).expect("exact standard-print fixture must open");
        assert_eq!(document.pages.len(), expected_pages);
        assert!(
            document.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "viewer.page_projection.family_profile_applied"
                    && diagnostic
                        .message
                        .contains(STANDARD_PRINT_SERVICE_TAIL_PROFILE_ID_V1)
            }),
            "bounded standard-print family profile must be explicit"
        );
        assert!(
            !document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "viewer.page_projection.roles_unresolved"),
            "admitted profile must replace generic roles_unresolved"
        );

        let after = fs::read(&path).expect("re-read exact standard-print fixture");
        assert_eq!(after, before, "Viewer page projection mutated source PUB");
    }

    #[test]
    #[ignore = "requires exact Virginia Devinettes public fixture"]
    fn exact_virginia_devinettes_standard_print_page_projection() {
        exact_standard_print_fixture(
            "CHAPTERA_VIRGINIA_DEVINETTES_PUB",
            "077612c7a228bd20bded939afde129cbdedae9b01b4f138f4619e332e5d7bd2e",
            6,
        );
    }

    #[test]
    #[ignore = "requires exact Virginia Remplacante public fixture"]
    fn exact_virginia_remplacante_standard_print_page_projection() {
        exact_standard_print_fixture(
            "CHAPTERA_VIRGINIA_REMPLACANTE_PUB",
            "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506",
            25,
        );
    }
}

#[cfg(test)]
mod mature_officeart_wmf_exact_product_tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn exact_wmf_fixture(env_name: &str, expected_sha256: &str, expected_counts: [usize; 8]) {
        let [
            expected_physical_wmf_records,
            expected_live_bstore_wmf_slots,
            expected_source_wmf_resources,
            expected_source_wmf_uses,
            expected_image_resources,
            expected_image_uses,
            expected_wmf_preview_resources,
            expected_wmf_preview_uses,
        ] = expected_counts;
        let path = std::env::var_os(env_name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("{env_name} is required"));
        let before = fs::read(&path).expect("read exact mature OfficeArt WMF fixture");
        assert_eq!(sha256_digest(&before).unwrap().to_string(), expected_sha256);

        let source_hash = sha256_digest(&before).expect("hash exact WMF fixture");
        let source = build_mature_0x2c_source_graph(Cursor::new(before.as_slice()), source_hash)
            .expect("build exact mature OfficeArt source graph");
        let source_wmf = build_mature_0x2c_wmf_preview_bundle_from_bytes(&before, &source.graph)
            .expect("materialize exact mature OfficeArt WMF source bundle");
        let graph_image_slot_nodes = source
            .graph
            .nodes
            .values()
            .filter(|node| node.payload.image_slot.is_some())
            .count();
        let graph_image_slots = source
            .graph
            .nodes
            .values()
            .filter_map(|node| node.payload.image_slot)
            .collect::<BTreeSet<_>>();
        let graph_page_ids = source
            .graph
            .pages
            .keys()
            .map(|page_id| page_id.into_canonical())
            .collect::<BTreeSet<_>>();
        let graph_page_bound_image_slot_nodes = source
            .graph
            .nodes
            .values()
            .filter(|node| {
                node.payload.image_slot.is_some() && graph_page_ids.contains(&node.header.parent_id)
            })
            .count();
        let raw_assets = build_mature_0x2c_asset_export_bundle_from_bytes(&before, &source.graph)
            .expect("build exact mature image export bundle");
        let raw_asset_mime_counts = raw_assets.manifest.assets.iter().fold(
            BTreeMap::<&str, usize>::new(),
            |mut counts, asset| {
                *counts.entry(asset.mime.as_str()).or_default() += 1;
                counts
            },
        );
        let graph_image_slot_bindings = source
            .graph
            .nodes
            .values()
            .filter_map(|node| {
                node.payload
                    .image_slot
                    .map(|slot| (node.payload.contents_seq_num, slot))
            })
            .collect::<Vec<_>>();
        eprintln!(
            "EXACT_MATURE_WMF_SOURCE_BINDINGS bindings={:?} bridge_diagnostics={:?}",
            graph_image_slot_bindings, source.diagnostics,
        );

        let geometry = open_mature_0x2c_geometry(&before, viewer_geometry_environment_v0_1())
            .expect("exact mature OfficeArt WMF fixture must open through Viewer product boundary");
        let scene_node_ids = geometry
            .scene
            .nodes
            .iter()
            .map(|node| node.origin)
            .collect::<BTreeSet<_>>();
        let scene_bound_wmf_resources = source_wmf
            .sources
            .iter()
            .filter(|source| {
                source
                    .uses
                    .iter()
                    .any(|usage| scene_node_ids.contains(&usage.node_id))
            })
            .count();
        let scene_bound_wmf_uses = source_wmf
            .sources
            .iter()
            .flat_map(|source| &source.uses)
            .filter(|usage| scene_node_ids.contains(&usage.node_id))
            .count();

        let wmf_previews = geometry
            .images
            .iter()
            .filter(|image| image.mime == "image/png")
            .collect::<Vec<_>>();
        let wmf_preview_uses = wmf_previews
            .iter()
            .map(|image| image.node_ids.len())
            .sum::<usize>();
        let diagnostic_counts = geometry.document.diagnostics.iter().fold(
            BTreeMap::<&str, usize>::new(),
            |mut counts, diagnostic| {
                *counts.entry(diagnostic.code.as_str()).or_default() += 1;
                counts
            },
        );
        eprintln!(
            "EXACT_MATURE_WMF_ACCEPTANCE graph_image_slot_nodes={} graph_image_slots={} graph_page_bound_image_slot_nodes={} raw_asset_mime_counts={:?} raw_asset_diagnostics={} physical_wmf_records={} live_bstore_wmf_slots={} source_resources={} source_uses={} source_rejected={} scene_bound_resources={} scene_bound_uses={} viewer_wmf_resources={} viewer_wmf_uses={} viewer_images={} viewer_image_uses={} diagnostics={:?}",
            graph_image_slot_nodes,
            graph_image_slots.len(),
            graph_page_bound_image_slot_nodes,
            raw_asset_mime_counts,
            raw_assets.manifest.diagnostics.len(),
            source_wmf.physical_wmf_record_count,
            source_wmf.live_bstore_wmf_slot_count,
            source_wmf.sources.len(),
            source_wmf
                .sources
                .iter()
                .map(|source| source.uses.len())
                .sum::<usize>(),
            source_wmf.rejected_source_count,
            scene_bound_wmf_resources,
            scene_bound_wmf_uses,
            wmf_previews.len(),
            wmf_preview_uses,
            geometry.images.len(),
            geometry
                .images
                .iter()
                .map(|image| image.node_ids.len())
                .sum::<usize>(),
            diagnostic_counts,
        );
        assert_eq!(
            source_wmf.physical_wmf_record_count, expected_physical_wmf_records,
            "exact fixture physical WMF record count drift"
        );
        assert_eq!(
            source_wmf.live_bstore_wmf_slot_count, expected_live_bstore_wmf_slots,
            "exact fixture live BStore WMF slot count drift"
        );
        assert_eq!(
            source_wmf.sources.len(),
            expected_source_wmf_resources,
            "exact fixture grounded source WMF resource count drift"
        );
        assert_eq!(
            source_wmf
                .sources
                .iter()
                .map(|source| source.uses.len())
                .sum::<usize>(),
            expected_source_wmf_uses,
            "exact fixture source WMF grounded-use count drift"
        );
        assert_eq!(
            source_wmf.rejected_source_count, 0,
            "exact fixture contains a WMF source outside the bounded decode profile"
        );
        assert_eq!(
            wmf_previews.len(),
            expected_wmf_preview_resources,
            "bounded mature OfficeArt WMF preview-resource count drift"
        );
        assert_eq!(
            wmf_preview_uses, expected_wmf_preview_uses,
            "bounded mature OfficeArt WMF preview-use count drift"
        );
        assert_eq!(
            geometry.images.len(),
            expected_image_resources,
            "exact fixture image-resource count drift"
        );
        assert_eq!(
            geometry
                .images
                .iter()
                .map(|image| image.node_ids.len())
                .sum::<usize>(),
            expected_image_uses,
            "exact fixture grounded image-use count drift"
        );
        assert!(
            wmf_previews.iter().all(|image| !image.bytes.is_empty()),
            "bounded WMF previews must carry deterministic PNG bytes"
        );
        assert!(
            geometry.document.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "viewer.mature_officeart_wmf.preview_applied"
            }),
            "exact fixture must record bounded mature OfficeArt WMF preview admission"
        );

        let after = fs::read(&path).expect("re-read exact mature OfficeArt WMF fixture");
        assert_eq!(after, before, "Viewer WMF preview path mutated source PUB");
    }

    #[test]
    #[ignore = "requires CHAPTERA_SAMPLE_NEWSLETTER exact Apache POI fixture"]
    fn exact_sample_newsletter_mature_officeart_wmf_previews_reach_viewer() {
        exact_wmf_fixture(
            "CHAPTERA_SAMPLE_NEWSLETTER",
            "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
            [8, 8, 7, 8, 8, 9, 7, 8],
        );
    }

    #[test]
    #[ignore = "requires CHAPTERA_SAMPLE_BROCHURE exact Apache POI fixture"]
    fn exact_sample_brochure_mature_officeart_wmf_previews_reach_viewer() {
        exact_wmf_fixture(
            "CHAPTERA_SAMPLE_BROCHURE",
            "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87",
            [5, 5, 5, 5, 6, 6, 5, 5],
        );
    }
}
