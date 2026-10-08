//! Bounded Microsoft Publisher 2002+ source adapter.
//!
//! This crate is the first format-aware layer above the raw CFB/Contents/Quill/
//! Escher readers. It projects only evidence-backed mature-0x2C semantics into
//! pub-model::SourceGraph and emits diagnostics instead of inventing missing
//! geometry, page roles, or relation state.

use anyhow::{Context, Result, anyhow, bail};

mod anchor_geometry;
mod asset_export;
mod assets;
mod borderart;
mod borderart_assets;
#[cfg(feature = "cmo-authority-bridge")]
mod cmo_bridge;
mod contents_access;
mod diagnostics;
mod direct_transform;
mod failure_envelope;
mod failure_intake;
mod family_classifier;
mod grouped_projection;
mod guide_bridge;
mod intake_protocol;
mod legacy22_graph;
mod legacy22_noquill_graph;
mod legacy22_page_role;
#[cfg(feature = "master-authority-bridge")]
mod master_bridge;
mod mature_wmf;
mod node_materialization;
mod ole_presentation;
mod page_projection;
mod paint_projection;
mod partial_root;
mod publication_document;
mod quill_admission;
mod resolve;
mod salvage;
mod salvage_authority;
mod source_graph_model;
mod source_identity;
mod source_paint_order;
mod story_frame_analysis;
mod story_frame_projection;
mod story_materialization;
mod story_provenance;
mod structural_base;
mod table_bridge;
mod typography_projection;
mod wmf;
mod wmf_preview;

pub use asset_export::{
    PUB_ASSET_EXPORT_SCHEMA_V0_1, PUB_ASSET_MANIFEST_FILENAME, PubAssetExportBundle,
    PubAssetExportDiagnostic, PubAssetExportFile, PubAssetExportManifest,
    PubAssetExportManifestEntry, build_mature_0x2c_asset_export_bundle_from_bytes,
    build_pub_asset_export_bundle, pub_asset_manifest_json, write_pub_asset_export_bundle,
};
pub use assets::{
    PubAssetManifest, PubAssetManifestDiagnostic, PubAssetManifestEntry, PubAssetUse,
    PubImageAlpha, PubImageBlobRef, PubImageResource, PubImageResourceCatalog,
    PubImageResourceDiagnostic, build_pub_asset_manifest, build_pub_image_resource_catalog,
};
pub use borderart::{
    PubBorderArtCatalogDiagnosticV1, PubBorderArtCatalogEntryV1, PubBorderArtCatalogReadV1,
    PubBorderArtCatalogV1, PubBorderArtShapeUseV1, read_mature_0x2c_borderart_catalog_v1,
};
pub use borderart_assets::{
    PubBorderArtAssetEntryV1, PubBorderArtAssetReadV1, PubBorderArtSlotRefV1, PubBorderArtSlotV1,
    PubBorderArtWmfResourceV1, read_mature_0x2c_borderart_assets_from_pub_bytes_v1,
    read_mature_0x2c_borderart_assets_v1,
};
#[cfg(feature = "cmo-authority-bridge")]
pub use cmo_bridge::{PubCmoProjectionBridgeV1, build_mature_0x2c_cmo_projection_bridge_v1};
use contents_access::{
    build_reference_index, chunk_for_reference, seq_u32, single_parent_seq, single_raw_type,
    unique_block, unique_reference_by_raw_type, unique_u32_field,
};
pub use diagnostics::PubBridgeDiagnostic;
pub use failure_envelope::{
    CHAPTERA_FAILURE_ENVELOPE_SCHEMA_V1, CHAPTERA_READER_BUILD_ID, FailureArchitecture,
    FailureCoarseLocale, FailureCode, FailureContainerFamily, FailureEnvelope,
    FailureEnvelopeBuildError, FailureEnvelopeContext, FailureOsFamily, FailureParserStage,
    FailureSizeBucket, FailureTelemetryChoice, PUB_READER_ENGINE_BUILD_ID, build_failure_envelope,
};
pub use failure_intake::{
    FailureIntakeClass, FailureIntakeClassification, FailureIntakeConfidence, FailureIntakeReason,
    classify_failure_candidate,
};
pub use family_classifier::{
    PubFamilyClassification, PubFamilyConfidence, PubFamilyProfile, PubFamilyReason,
    PubReaderRoute, classify_pub_family,
};
use grouped_projection::{
    GroupedProjectionContext, coordinate_rect_i128, project_grouped_object_shape,
    project_rect_trunc,
};
pub use guide_bridge::{
    PubGroundedGuideBuild, PubGuideObservation, PubGuideProjectionDiagnostic,
    materialize_grounded_guides,
};
pub use intake_protocol::{
    CHAPTERA_EXACT_FILE_CONSENT_V1, CHAPTERA_INTAKE_PROTOCOL_SCHEMA_V1,
    CHAPTERA_INTAKE_RETENTION_POLICY_V1, IntakeCapabilityRequest, IntakeClusterDisposition,
    IntakeDedupeDisposition, IntakeProtocolError, IntakeReceipt, build_intake_capability_request,
    exact_file_intake_eligible, validate_intake_capability_request, validate_intake_receipt,
};
pub use legacy22_graph::{
    build_legacy_0x22_quill_from_streams, build_legacy_0x22_quill_source_graph, legacy22_object_key,
};
pub use legacy22_noquill_graph::{
    build_legacy_0x22_noquill_from_contents, build_legacy_0x22_noquill_source_graph,
    read_legacy_0x22_image_wmf, read_legacy_0x22_image_wmfs,
};
pub use legacy22_page_role::{
    LEGACY22_PAGE_ROLE_OBSERVATION_SCHEMA_V1, Legacy22PageListEntryObservationV1,
    Legacy22PageRoleObservationReceiptV1, analyze_legacy_0x22_page_roles,
};
#[cfg(feature = "master-authority-bridge")]
pub use master_bridge::{
    PubMasterProjectionBridgeV1, build_mature_0x2c_master_projection_bridge_v1,
};
pub use mature_wmf::{
    MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1, PubMatureOfficeArtWmfPreviewBundle,
    PubMatureOfficeArtWmfPreviewSource, build_mature_0x2c_wmf_preview_bundle_from_bytes,
};
use node_materialization::{MatureNodeMaterializationContext, materialize_mature_nodes};
pub use ole_presentation::{
    LegacyOleCachedPresentation, LegacyOleCachedPresentationDiagnostic,
    LegacyOleCachedPresentationScan, LegacyOleCachedPresentationSelection, OlePresentation,
    parse_cf_metafilepict_ole_presentation, read_legacy_ole_cached_presentations,
    scan_legacy_ole_cached_presentations, select_unambiguous_legacy_ole_cached_presentation,
};
use page_projection::derive_effective_page_projection;
pub use page_projection::{
    PUB_PAGE_ROLE_OBSERVATION_SCHEMA_V1, PubControllingFieldObservation, PubControllingObservation,
    PubDocumentPageListEntryObservation, PubEffectivePageProjection,
    PubEffectivePageProjectionAuthority, PubPageRoleObservation, PubPageRoleObservationReceipt,
    analyze_mature_0x2c_page_roles,
};
#[cfg(test)]
use page_projection::{build_effective_page_projection, resolve_scenario_page_ids_from_evidence};
pub use paint_projection::resolve_bounded_effective_officeart_paint;
use paint_projection::{
    FILL_FILLED_BIT, FILL_USE_FILLED_BIT, OFFICE_ART_FILL_BOOLEANS, OFFICE_ART_FILL_COLOR,
    OFFICE_ART_FILL_TYPE, OFFICE_ART_LINE_WIDTH, OFFICEART_SHAPE_TYPE_ELLIPSE,
    admits_normative_2d_paint_defaults, bounded_officeart_image_crop,
    bounded_officeart_image_recolor, bounded_officeart_rgb, direct_officeart_rgb,
    effective_paint_has_dgg_authority, explicit_officeart_paint, has_default_ellipse_geometry,
    has_default_line_geometry, has_default_roundrect_geometry,
    has_explicit_officeart_paint_observation, has_shape_local_dash_gel,
    paint_context_uses_officeart_scheme_color, unique_explicit_officeart_scalar,
};
#[cfg(test)]
use paint_projection::{
    LINE_LINE_BIT, LINE_USE_LINE_BIT, OFFICE_ART_LINE_BOOLEANS, OFFICE_ART_LINE_COLOR,
};
pub use partial_root::{
    READER_PARTIAL_CONTENTS_SEMANTIC_EVIDENCE_SCHEMA_V1,
    READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1, ReaderPartialContentsBoundary,
    ReaderPartialContentsChunkFact, ReaderPartialContentsClass,
    ReaderPartialContentsSemanticEvidence, ReaderPartialRootStreamEvidence,
    analyze_reader_partial_contents_prefix, build_reader_partial_root_stream_evidence,
};
use pub_contents::{
    BLOCK_TYPE_FIXED_8, BLOCK_TYPE_REFERENCE_U32, BLOCK_TYPE_U32, CONTENTS_RAW_TYPE_STORY_CATALOG,
    Contents0x2cChunk, Contents0x2cChunkReference, DOCUMENT_PAGE_LIST_ID, MatureColorScheme,
    RawContentsBlock, RawContentsBlockBody, StoryCatalogReadError, parse_0x2c_header,
    parse_bounded_empty_mature_story_catalog_variant, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_controlling_page_list, parse_confirmed_document_page_list,
    parse_confirmed_margins_page_extent, parse_confirmed_mature_color_scheme,
    parse_confirmed_mature_story_catalog, parse_confirmed_oid_identity_payload,
};
use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM, OFFICE_ART_PROPERTY_CROP_FROM_LEFT,
    OFFICE_ART_PROPERTY_CROP_FROM_RIGHT, OFFICE_ART_PROPERTY_CROP_FROM_TOP,
    OFFICE_ART_PROPERTY_PIB, OFFICE_ART_TERTIARY_FOPT, PUBLISHER_FIELD_SHAPE_ID,
    PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS, PublisherField,
    PublisherFieldRecord, SpContainerInventory, inspect_dgg_default_options, inspect_sp_containers,
};
#[cfg(test)]
use pub_model::CanonicalId;
use pub_model::{
    Affine2D, AuthorityClass, ByteRange, Decimal, Document, LengthEmu, Node, NodeHeader, NodeId,
    NodeKind, Page, PageId, ReadConfidence, RectEmu, Sha256Digest, Size2D, SourceDescriptor,
    SourceGraph, SourceRef, SourceRole, Story, StoryId,
};
use pub_quill::{
    QuillEffectiveBoolean, QuillMcldVerticalAlignment, QuillParagraphAlignment,
    QuillParagraphFlowConstraint, QuillParagraphLineSpacing, QuillScriptFontEntryDisposition,
    QuillTypographyValueSource, bounded_mcld_text_frame_vertical_alignment,
    bounded_mcld_text_insets,
};
use publication_document::{PublicationDocumentBootstrap, materialize_publication_document};
#[cfg(test)]
use publication_document::require_consensus_page_extent;
use quill_admission::{QuillAdmission, admit_quill_projection_inputs};
pub use resolve::{
    PUB_RESOLVER_VERSION_V1, PubResolveDiagnostic, PubResolvedGraph, PubResolvedGraphBuild,
    PubResolvedNodePayload, PubResolvedStoryFrame, resolve_pub_source_graph,
};
pub use salvage::{
    READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1, READER_SALVAGE_PROBE_SCHEMA_V1,
    ReaderPartialEscherDelayEvidence, ReaderPartialEscherDelayImageEvidence,
    ReaderPartialSourceFact, ReaderPartialSourceGap, ReaderPartialSourceGraph,
    ReaderPartialSourceGraphError, ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility,
    ReaderSalvageProbe, ReaderSalvageStreamState, ReaderSalvageSubsystemProbe,
    ReaderSalvageTrigger, build_reader_partial_escherdelay_evidence,
    build_reader_partial_source_graph, probe_reader_salvage_candidate,
    probe_reader_salvage_candidate_with_trigger, recovered_resource,
};
pub use salvage_authority::{
    ReaderEvidenceDisposition, ReaderSalvageAuthority, reader_evidence_disposition,
    typed_corruption_authority,
};
use serde::{Deserialize, Serialize};
pub use source_graph_model::{
    PubEffectiveFillSource, PubEffectiveLineSource, PubEffectivePaintAuthority,
    PubEffectivePaintValue, PubEffectiveShapePaintSource, PubExplicitFillSource,
    PubExplicitImageCropSource, PubExplicitImageRecolorSource, PubExplicitLineSource,
    PubExplicitShapePaintSource, PubLegacyOleSource, PubNodePayload, PubSourceGraph,
    PubSourceGraphBuild, PubStoryFrameSource, PubTextFrameInsetSource,
    PubTextFrameVerticalAlignment, PubTextFrameVerticalAlignmentSource,
};
use source_identity::{ROLE_DOCUMENT, ROLE_NODE, ROLE_PAGE, ROLE_STORY, derive_pub_id, source_ref};
pub use source_identity::{
    contents_object_key, derive_pub_document_id, derive_pub_node_id, derive_pub_page_id,
    derive_pub_story_id, quill_story_object_key,
};
pub use source_paint_order::{PUB_SOURCE_PAGE_PAINT_ORDER_SCHEMA_V1, PubSourcePagePaintOrderV1};
use source_paint_order::{index_escher_by_contents_seq, source_page_paint_orders_v1};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom};
pub use story_frame_analysis::{
    PubGroupedStoryGeometryCorrelation, PubStoryFrameCorrelation,
    analyze_mature_0x2c_grouped_story_geometry,
    analyze_mature_0x2c_grouped_story_geometry_from_streams,
    analyze_mature_0x2c_story_frame_candidates,
    analyze_mature_0x2c_story_frame_candidates_from_streams,
};
use story_frame_projection::{
    add_missing_link_target_diagnostics, build_story_frame, unique_story_id_scalar,
};
use story_materialization::materialize_story_catalogs;
pub use story_provenance::has_exact_mature_quill_story_identity_v1;
pub use structural_base::{
    PUB_STRUCTURAL_BASE_SCHEMA_V1, PubStructuralBaseCandidate, PubStructuralBaseManifest,
    PubStructuralBaseStreamDigest, build_mature_0x2c_structural_base_manifest,
    structural_base_manifest_json,
};
pub use table_bridge::{
    PubMaterializedTableCell, PubTableBorderAxis, PubTableBorderSegmentSource,
    PubTableCellCoordinates, PubTableCellPaintSource, PubTableCellSource,
    PubTableLayoutMetricsSource, PubTableSource, PubTableStoryOwnershipSource, PubTableTextError,
    PubTableUniformTextInsetSource, RAW_TYPE_TABLE, materialize_bounded_simple_table_cells,
    materialize_bounded_table_cells,
};
pub use typography_projection::{
    PubParagraphAlignment, PubParagraphAlignmentRun, PubParagraphFlowConstraint,
    PubParagraphFlowRun, PubParagraphLineSpacing, PubParagraphLineSpacingRun, PubScriptFontEntry,
    PubScriptFontEntryDisposition, PubScriptFontMap, PubTypographyBooleanV1, PubTypographyRun,
    PubTypographySizeRun,
};
use typography_projection::{PubTypographyProjection, project_typography_catalog};
#[cfg(test)]
use typography_projection::{
    bounded_quill_text_rgb, project_effective_boolean_v1, utf16_range_to_scalar_range,
};
pub use wmf::{BoundedWmfMetafile, WmfMetafileInfo, bounded_wmf_metafile, validate_wmf_metafile};
pub use wmf_preview::{
    LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1, WmfPreviewRgba, rasterize_wmf_preview,
};

pub const PUB_ADAPTER_ID: &str = "pub-rs";
pub const PUB_FORMAT_PROFILE_ID: &str = "pub-mature-0x2c-v0.1";

use direct_transform::bounded_node_transform_projection;
#[cfg(test)]
use direct_transform::{
    BoundedDirectImageTransform, bounded_direct_image_cardinal_content_rotation_degrees,
    bounded_direct_image_transform, bounded_direct_story_transform,
};

pub fn format_profile()
-> Result<pub_format_registry::FormatProfileEntry, pub_format_registry::RegistryError> {
    pub_format_registry::resolve(PUB_FORMAT_PROFILE_ID)
}
pub const CONTENTS_STREAM_PATH: &str = "/Contents";
pub const QUILL_STREAM_PATH: &str = "/Quill/QuillSub/CONTENTS";
pub const ESCHER_STREAM_PATH: &str = "/Escher/EscherStm";
pub const ESCHER_DELAY_STREAM_PATH: &str = "/Escher/EscherDelayStm";

const RAW_TYPE_SHAPE: u16 = 0x01;
const OFFICE_ART_PROPERTY_ROTATION: u16 = 0x0004;
const FSP_FLIP_H: u32 = 1 << 6;
const FSP_FLIP_V: u32 = 1 << 7;
const AFFINE_DECIMAL_SCALE: i128 = 1_000_000_000_000;
const PI_SCALED: i128 = 3_141_592_653_590;
const RAW_TYPE_GROUP: u16 = 0x30;
const RAW_TYPE_PAGE: u16 = 0x43;
const RAW_TYPE_DOCUMENT: u16 = 0x44;
const RAW_TYPE_MARGINS: u16 = 0x4C;
const RAW_TYPE_CONTROLLING: u16 = 0x4D;
const RAW_TYPE_PAGE_LIST_SPECIAL: u16 = 0x59;
const RAW_TYPE_COLOR_SCHEME: u16 = 0x5C;

const OFFICEART_PROPERTY_ROTATION: u16 = 0x0004;
const OFFICEART_FSP_FLIP_H: u32 = 1 << 6;
const OFFICEART_FSP_FLIP_V: u32 = 1 << 7;

const FIELD_STORY_ID: u16 = 0x27;
const FIELD_FRAME_ORDINAL: u16 = 0x28;
const FIELD_SHAPE_WIDTH: u16 = 0xAA;
const FIELD_SHAPE_HEIGHT: u16 = 0xAB;
const FIELD_PREVIOUS_FRAME: u16 = 0x36;
const FIELD_NEXT_FRAME: u16 = 0x37;

fn raw_span_hex(bytes: &[u8], span: &pub_core::RawSpan) -> Result<String> {
    let start = usize::try_from(span.offset).context("raw span offset does not fit usize")?;
    let len = usize::try_from(span.len).context("raw span length does not fit usize")?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .context("raw span is outside Contents")?;
    Ok(bytes[start..end]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

/// Builds a bounded mature-0x2C SourceGraph from one complete CFB file.
///
/// The caller supplies a verified SHA-256 digest. This function deliberately
/// does not pretend that a representation type is a hashing implementation.
pub fn build_mature_0x2c_source_graph<R: Read + Seek>(
    mut reader: R,
    source_hash: Sha256Digest,
) -> Result<PubSourceGraphBuild> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| format!("read {QUILL_STREAM_PATH}"))?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), ESCHER_STREAM_PATH)
        .with_context(|| format!("read {ESCHER_STREAM_PATH}"))?;

    build_mature_0x2c_from_streams(source_hash, &contents, &quill, &escher)
}

pub fn build_mature_0x2c_from_streams(
    source_hash: Sha256Digest,
    contents: &[u8],
    quill: &[u8],
    escher: &[u8],
) -> Result<PubSourceGraphBuild> {
    let adapter_version = format!("pub-rs/{}", env!("CARGO_PKG_VERSION"));
    let source = SourceDescriptor {
        format: "pub".into(),
        format_version: Some("0x2c".into()),
        adapter_version,
        source_hash,
    };

    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), contents)
        .context("parse mature-0x2C Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature-0x2C Contents trailer")?;
    let references = build_reference_index(contents, &trailer.directory)?;

    let story_catalog_reference = unique_reference_by_raw_type(
        &references,
        CONTENTS_RAW_TYPE_STORY_CATALOG,
        "Story catalog 0x65",
    )?;
    let story_catalog_chunk =
        chunk_for_reference(contents_stream.clone(), contents, story_catalog_reference)?;
    let (story_layout_keys, grounded_story_catalog, physical_empty_story_catalog) =
        match parse_confirmed_mature_story_catalog(contents, &story_catalog_chunk) {
            Ok(story_catalog) => {
                let story_layout_keys = story_catalog
                    .entries
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            entry.text_id,
                            (entry.layout_key?, entry.layout_key_source.as_ref()?.clone()),
                        ))
                    })
                    .collect::<BTreeMap<_, _>>();
                (story_layout_keys, Some(story_catalog), false)
            }
            Err(StoryCatalogReadError::MissingDeclaredCount) => {
                let _physical_empty = parse_bounded_empty_mature_story_catalog_variant(
                    contents,
                    &story_catalog_chunk,
                )
                .context("parse bounded physical-empty Story catalog 0x65 variant")?;

                let mut referenced_story_ids = BTreeSet::new();
                for reference in references.values() {
                    if !matches!(
                        single_raw_type(reference),
                        Some(RAW_TYPE_SHAPE) | Some(RAW_TYPE_TABLE)
                    ) {
                        continue;
                    }
                    let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
                    if let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? {
                        referenced_story_ids.insert(text_id);
                    }
                }
                if !referenced_story_ids.is_empty() {
                    bail!(
                        "physical-empty Story catalog 0x65 conflicts with live Story references: {:?}",
                        referenced_story_ids
                    );
                }
                (BTreeMap::new(), None, true)
            }
            Err(error) => return Err(error).context("parse mature Story catalog 0x65"),
        };

    let PublicationDocumentBootstrap {
        mut graph,
        effective_pages,
        page_seq_to_id,
        color_scheme,
        mut diagnostics,
    } = materialize_publication_document(
        source_hash,
        source,
        contents_stream.clone(),
        contents,
        &references,
    )?;

    let QuillAdmission {
        quill_catalog,
        fdpp_story_catalog,
        typography_catalog,
        mcld,
    } = admit_quill_projection_inputs(
        quill,
        grounded_story_catalog.as_ref(),
        physical_empty_story_catalog,
        &mut diagnostics,
    )?;

    let story_by_syid = materialize_story_catalogs(
        &source_hash,
        quill_catalog.as_ref(),
        fdpp_story_catalog.as_ref(),
        &mut graph,
    )?;

    let PubTypographyProjection {
        typography_runs,
        typography_size_runs,
        paragraph_alignments,
        paragraph_line_spacings,
        paragraph_flow_runs,
        script_font_maps,
    } = project_typography_catalog(
        typography_catalog,
        &story_by_syid,
        &graph,
        color_scheme.as_ref().map(|value| &value.scheme),
        &mut diagnostics,
    );

    let source_page_paint_orders = materialize_mature_nodes(
        MatureNodeMaterializationContext {
            source_hash: &source_hash,
            contents_stream: &contents_stream,
            contents,
            escher,
            references: &references,
            page_seq_to_id: &page_seq_to_id,
            story_by_syid: &story_by_syid,
            story_layout_keys: &story_layout_keys,
            quill_catalog: quill_catalog.as_ref(),
            mcld: mcld.as_ref(),
            color_scheme: color_scheme.as_ref(),
        },
        &mut graph,
        &mut diagnostics,
    )?;

    Ok(PubSourceGraphBuild {
        graph,
        effective_pages,
        source_page_paint_orders,
        diagnostics,
        typography_runs,
        typography_size_runs,
        paragraph_alignments,
        paragraph_line_spacings,
        paragraph_flow_runs,
        script_font_maps,
    })
}

use anchor_geometry::{
    anchor_has_unique_geometry_fields, page_relative_bounds,
    page_relative_bounds_from_contents_missing_xe, record_missing_anchor_fields, signed_field,
    unique_escher_field,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bounds() -> RectEmu {
        RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(300),
            LengthEmu::new(500),
        )
    }

    #[test]
    fn direct_image_missing_xe_can_recover_from_exact_contents_extent() {
        let page = Page {
            id: test_page_id(99),
            size: Size2D::new(LengthEmu::new(7_772_400), LengthEmu::new(10_058_400)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };
        let bounds = anchor_geometry::page_relative_bounds_from_contents_missing_xe_values(
            &page, -3_429_000, 259_080, 2_899_410, 3_429_000, 2_640_330,
        )
        .expect("Contents extent should recover the measured missing-XE image anchor");
        assert_eq!(bounds.x.get(), 457_200);
        assert_eq!(bounds.width.get(), 3_429_000);
        assert_eq!(bounds.height.get(), 2_640_330);
    }

    #[test]
    fn direct_image_missing_xe_recovery_rejects_cross_stream_height_mismatch() {
        let page = Page {
            id: test_page_id(100),
            size: Size2D::new(LengthEmu::new(7_772_400), LengthEmu::new(10_058_400)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };
        assert_eq!(
            anchor_geometry::page_relative_bounds_from_contents_missing_xe_values(
                &page, -3_429_000, 259_080, 2_899_410, 3_429_000, 2_640_329,
            ),
            None
        );
    }

    #[test]
    fn direct_story_rotation_preserves_exact_cardinal_affine_transform() {
        for rotation_op in [90u32 << 16, ((-90i32) << 16) as u32, 180u32 << 16] {
            let transform =
                bounded_direct_story_transform(&[(rotation_op, false, false)], 0, test_bounds())
                    .expect("bounded direct Story rotation should be admitted");
            assert_ne!(transform, Affine2D::identity());
        }
    }

    #[test]
    fn direct_story_rotation_identity_and_unsupported_states_fail_closed() {
        assert_eq!(
            bounded_direct_story_transform(&[], 0, test_bounds()),
            Some(Affine2D::identity())
        );
        assert_eq!(
            bounded_direct_story_transform(&[(0, false, false)], 0, test_bounds()),
            Some(Affine2D::identity())
        );
        assert_eq!(
            bounded_direct_story_transform(
                &[(90u32 << 16, false, false)],
                FSP_FLIP_H,
                test_bounds(),
            ),
            None
        );
        assert_eq!(
            bounded_direct_story_transform(
                &[(90u32 << 16, false, false), (180u32 << 16, false, false)],
                0,
                test_bounds(),
            ),
            None
        );
        assert_eq!(
            bounded_direct_story_transform(&[(90u32 << 16, false, true)], 0, test_bounds(),),
            None
        );
    }

    #[test]
    fn direct_image_rotation_absent_or_zero_stays_identity() {
        assert_eq!(
            bounded_direct_image_transform(&[], 0, test_bounds()),
            BoundedDirectImageTransform::Identity
        );
        assert_eq!(
            bounded_direct_image_transform(&[(0, false, false)], 0, test_bounds()),
            BoundedDirectImageTransform::Identity
        );
        assert_eq!(
            bounded_direct_image_transform(&[((360u32) << 16, false, false)], 0, test_bounds()),
            BoundedDirectImageTransform::Identity
        );
    }

    #[test]
    fn direct_image_cardinal_rotation_is_preserved_for_picture_content_only() {
        assert_eq!(
            bounded_direct_image_cardinal_content_rotation_degrees(
                &[((90u32) << 16, false, false)],
                0,
            ),
            Some(90)
        );
        assert_eq!(
            bounded_direct_image_cardinal_content_rotation_degrees(
                &[((180u32) << 16, false, false)],
                0,
            ),
            Some(180)
        );
        assert_eq!(
            bounded_direct_image_cardinal_content_rotation_degrees(
                &[((270u32) << 16, false, false)],
                0,
            ),
            Some(270)
        );
        assert_eq!(
            bounded_direct_image_cardinal_content_rotation_degrees(
                &[((12u32) << 16, false, false)],
                0,
            ),
            None
        );
        assert_eq!(
            bounded_direct_image_cardinal_content_rotation_degrees(
                &[((90u32) << 16, false, false)],
                FSP_FLIP_H,
            ),
            None
        );
    }

    #[test]
    fn direct_image_rotation_keeps_exact_cardinal_angles_fail_closed() {
        for rotation_op in [
            90u32 << 16,
            ((-90i32) << 16) as u32,
            180u32 << 16,
            ((-180i32) << 16) as u32,
        ] {
            assert_eq!(
                bounded_direct_image_transform(&[(rotation_op, false, false)], 0, test_bounds()),
                BoundedDirectImageTransform::Unsupported
            );
        }
    }

    #[test]
    fn direct_image_rotation_keeps_fractional_16_16_angle_nonidentity() {
        let half_degree = 32_768u32;
        let transform =
            bounded_direct_image_transform(&[(half_degree, false, false)], 0, test_bounds());
        let BoundedDirectImageTransform::Applied(transform) = transform else {
            panic!("fractional scalar rotation must be admitted");
        };
        assert_ne!(transform, Affine2D::identity());
        assert_ne!(transform.b.as_str(), "0");
        assert_ne!(transform.c.as_str(), "0");
    }

    #[test]
    fn direct_image_rotation_rejects_flip_duplicate_and_complex_states() {
        assert_eq!(
            bounded_direct_image_transform(&[(1, false, false)], FSP_FLIP_H, test_bounds()),
            BoundedDirectImageTransform::Unsupported
        );
        assert_eq!(
            bounded_direct_image_transform(
                &[(1, false, false), (2, false, false)],
                0,
                test_bounds(),
            ),
            BoundedDirectImageTransform::Unsupported
        );
        assert_eq!(
            bounded_direct_image_transform(&[(1, false, true)], 0, test_bounds()),
            BoundedDirectImageTransform::Unsupported
        );
    }

    #[test]
    #[ignore = "requires CHAPTERA_SOURCE_STACK_FIXTURE and CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS"]
    fn exact_public_source_stack_order_covers_materialized_grouped_nodes() {
        let fixture = std::env::var_os("CHAPTERA_SOURCE_STACK_FIXTURE")
            .map(std::path::PathBuf::from)
            .expect("CHAPTERA_SOURCE_STACK_FIXTURE");
        let expected = std::env::var("CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS")
            .expect("CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS")
            .split(',')
            .map(|value| value.parse::<usize>().expect("page count"))
            .collect::<Vec<_>>();
        let bytes = std::fs::read(fixture).expect("read exact public PUB");
        let source_hash: Sha256Digest = std::env::var("CHAPTERA_SOURCE_STACK_SHA256")
            .expect("CHAPTERA_SOURCE_STACK_SHA256")
            .parse()
            .expect("valid source SHA-256");
        let build = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
            .expect("build mature source graph");

        let page_ordinals = build
            .graph
            .document
            .pages
            .iter()
            .enumerate()
            .map(|(ordinal, page_id)| (*page_id, ordinal))
            .collect::<BTreeMap<_, _>>();
        let mut actual = build
            .source_page_paint_orders
            .iter()
            .filter_map(|order| {
                page_ordinals
                    .get(&order.page_id)
                    .copied()
                    .map(|ordinal| (ordinal, order.node_ids.len()))
            })
            .collect::<Vec<_>>();
        actual.sort_unstable();
        let actual_counts = actual.iter().map(|(_, count)| *count).collect::<Vec<_>>();

        assert_eq!(
            actual_counts, expected,
            "source stack order must cover all already-materialized page visuals in exact serialized OfficeArt order"
        );
        let unique = build
            .source_page_paint_orders
            .iter()
            .flat_map(|order| order.node_ids.iter().copied())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            expected.iter().sum::<usize>(),
            "one materialized node may occupy exactly one source stack slot"
        );
    }

    fn source_hash() -> Sha256Digest {
        "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
            .parse()
            .expect("known SampleNewsletter SHA-256")
    }

    #[test]
    #[ignore = "requires CHAPTERA_SCRIPT_FONT_MAP_FIXTURE and CHAPTERA_SCRIPT_FONT_MAP_OUT"]
    fn exact_fonts_pub_preserves_script_font_map_without_scalar_promotion() {
        let fixture = std::env::var_os("CHAPTERA_SCRIPT_FONT_MAP_FIXTURE")
            .map(std::path::PathBuf::from)
            .expect("CHAPTERA_SCRIPT_FONT_MAP_FIXTURE");
        let output_dir = std::env::var_os("CHAPTERA_SCRIPT_FONT_MAP_OUT")
            .map(std::path::PathBuf::from)
            .expect("CHAPTERA_SCRIPT_FONT_MAP_OUT");
        std::fs::create_dir_all(&output_dir).expect("create script-font-map output");

        let bytes = std::fs::read(&fixture).expect("read exact fonts.pub");
        let exact_source_hash: Sha256Digest =
            "8d50872a7d8ee6130b889efbe99275ee333747bc7777f3c5256a05f2c6d32048"
                .parse()
                .expect("known fonts.pub SHA-256");
        let build =
            build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), exact_source_hash)
                .expect("build exact fonts.pub source graph");

        assert!(
            build.typography_runs.is_empty(),
            "this preservation slice must not silently promote ScriptFonts into the legacy scalar typography path"
        );
        assert!(
            !build.script_font_maps.is_empty(),
            "exact fonts.pub must preserve at least one source script-font map"
        );

        let mut resolved_entries = 0_usize;
        let mut unresolved_entries = 0_usize;
        let mut invalid_entries = 0_usize;
        let mut times_new_roman_slots = Vec::new();
        let mut map_receipts = Vec::new();

        for map in &build.script_font_maps {
            let mut seen_slots = BTreeSet::new();
            let entries = map
                .entries
                .iter()
                .map(|entry| {
                    assert!(
                        seen_slots.insert(entry.script_slot),
                        "one source ScriptFonts map must not repeat a raw script slot"
                    );
                    match entry.disposition {
                        PubScriptFontEntryDisposition::Resolved => resolved_entries += 1,
                        PubScriptFontEntryDisposition::UnresolvedSentinel => {
                            unresolved_entries += 1
                        }
                        PubScriptFontEntryDisposition::InvalidFontOrdinal => invalid_entries += 1,
                    }
                    if entry.source_font_index == 0
                        && entry.source_font_name.as_deref() == Some("Times New Roman")
                    {
                        times_new_roman_slots.push(entry.script_slot);
                    }
                    serde_json::json!({
                        "script_slot": entry.script_slot,
                        "source_font_index": entry.source_font_index,
                        "source_font_name": entry.source_font_name,
                        "disposition": entry.disposition,
                    })
                })
                .collect::<Vec<_>>();
            map_receipts.push(serde_json::json!({
                "story_id": map.story_id,
                "story_utf16_range": [map.story_utf16_start, map.story_utf16_end],
                "story_scalar_range": [map.story_scalar_start, map.story_scalar_end],
                "entries": entries,
            }));
        }

        times_new_roman_slots.sort_unstable();
        times_new_roman_slots.dedup();
        assert!(
            !times_new_roman_slots.is_empty(),
            "exact fonts.pub must preserve at least one ScriptFonts slot resolving to FONT[0] Times New Roman"
        );
        assert_eq!(
            invalid_entries, 0,
            "exact positive must not invent out-of-range FONT ordinals"
        );

        let receipt = serde_json::json!({
            "schema": "chaptera.viewer-script-font-map-exact-fixture.v1",
            "source_pub_sha256": exact_source_hash,
            "legacy_scalar_typography_run_count": build.typography_runs.len(),
            "script_font_map_count": build.script_font_maps.len(),
            "resolved_entry_count": resolved_entries,
            "unresolved_entry_count": unresolved_entries,
            "invalid_entry_count": invalid_entries,
            "times_new_roman_font_ordinal": 0,
            "times_new_roman_script_slots": times_new_roman_slots,
            "maps": map_receipts,
        });
        std::fs::write(
            output_dir.join("viewer-script-font-map-fonts-pub.json"),
            serde_json::to_vec_pretty(&receipt).expect("serialize script-font-map receipt"),
        )
        .expect("write script-font-map receipt");

        println!(
            "script-font exact fixture: maps={} resolved={} unresolved={} invalid={} times_new_roman_slots={:?}",
            build.script_font_maps.len(),
            resolved_entries,
            unresolved_entries,
            invalid_entries,
            receipt["times_new_roman_script_slots"],
        );
    }

    #[test]
    fn typography_boolean_projection_preserves_xor_operands_and_effective_value() {
        let source = QuillEffectiveBoolean {
            local_toggle: true,
            local_toggle_source: Some(RawSpan {
                stream: StreamPath("/Quill/QuillSub/CONTENTS".into()),
                offset: 100,
                len: 2,
            }),
            inherited_value: true,
            inherited_style_source: RawSpan {
                stream: StreamPath("/Quill/QuillSub/CONTENTS".into()),
                offset: 200,
                len: 12,
            },
            effective_value: false,
        };

        assert_eq!(
            project_effective_boolean_v1(&source),
            PubTypographyBooleanV1 {
                local_toggle: true,
                inherited_value: true,
                effective_value: false,
            }
        );
    }

    #[test]
    fn typography_utf16_to_scalar_range_is_surrogate_safe() {
        let text = "A😀B";
        assert_eq!(utf16_range_to_scalar_range(text, 1, 3), Some((1, 2)));
        assert_eq!(utf16_range_to_scalar_range(text, 0, 4), Some((0, 3)));
        assert_eq!(utf16_range_to_scalar_range(text, 1, 2), None);
        assert_eq!(utf16_range_to_scalar_range(text, 2, 3), None);
        assert_eq!(utf16_range_to_scalar_range(text, 3, 1), None);
    }

    fn test_page_id(seed: u8) -> PageId {
        PageId::from_canonical(CanonicalId::from_bytes([seed; 16]))
    }

    fn test_node_id(seed: u8) -> NodeId {
        NodeId::from_canonical(CanonicalId::from_bytes([seed; 16]))
    }

    #[test]
    fn grouped_carrier_participants_expand_at_one_source_order_slot_and_duplicate_fails_closed() {
        let page_id = test_page_id(10);
        let direct_before = test_node_id(11);
        let grouped_first = test_node_id(12);
        let grouped_second = test_node_id(13);
        let carrier_seq = 300_u32;

        let grouped_by_carrier = BTreeMap::from([(
            carrier_seq,
            (
                page_id,
                vec![(4_usize, grouped_first), (5_usize, grouped_second)],
            ),
        )]);
        let mut seen_seq = BTreeSet::from([299_u32]);
        let mut rejected = BTreeSet::new();
        let mut ordered = BTreeMap::from([(page_id, vec![direct_before])]);

        assert!(source_paint_order::append_grouped_carrier_participants(
            carrier_seq,
            &grouped_by_carrier,
            &mut seen_seq,
            &mut rejected,
            &mut ordered,
        ));
        assert_eq!(
            ordered.get(&page_id),
            Some(&vec![direct_before, grouped_first, grouped_second])
        );
        assert!(rejected.is_empty());

        assert!(source_paint_order::append_grouped_carrier_participants(
            carrier_seq,
            &grouped_by_carrier,
            &mut seen_seq,
            &mut rejected,
            &mut ordered,
        ));
        assert!(rejected.contains(&page_id));
        assert_eq!(
            ordered.get(&page_id),
            Some(&vec![direct_before, grouped_first, grouped_second]),
            "duplicate carrier must not duplicate descendants"
        );
    }

    #[test]
    fn scenario_page_observation_uses_unanimous_pgid_order() {
        let p0 = test_page_id(1);
        let p1 = test_page_id(2);
        let p2 = test_page_id(3);
        let pgids = vec![vec![(1, 0), (1, 1), (1, 2)], vec![(1, 0), (1, 1), (1, 2)]];
        let pages_by_oid = BTreeMap::from([
            ((1, 0), vec![p0]),
            ((1, 1), vec![p1]),
            ((1, 2), vec![p2]),
            ((2, 0), vec![test_page_id(9)]),
        ]);

        assert_eq!(
            resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid).unwrap(),
            vec![p0, p1, p2]
        );
    }

    #[test]
    fn effective_page_projection_never_drops_raw_page_missing_from_scenario_order() {
        let p0 = test_page_id(1);
        let p1 = test_page_id(2);
        let newly_created_visible_page = test_page_id(3);
        let effective =
            build_effective_page_projection(&[p0, p1, newly_created_visible_page], vec![p0, p1], 2);

        assert_eq!(
            effective.authority,
            PubEffectivePageProjectionAuthority::RawDocumentPageList
        );
        assert_eq!(
            effective.page_ids,
            vec![p0, p1, newly_created_visible_page],
            "scenario Pgid evidence must never suppress a raw page; native Publisher can create visible pages without OplControlling/Pgid"
        );
        assert_eq!(effective.observed_scenario_page_ids, vec![p0, p1]);
    }

    #[test]
    fn scenario_page_observation_rejects_disagreeing_lists() {
        let pgids = vec![vec![(1, 0), (1, 1)], vec![(1, 1), (1, 0)]];
        let pages_by_oid = BTreeMap::from([
            ((1, 0), vec![test_page_id(1)]),
            ((1, 1), vec![test_page_id(2)]),
        ]);

        assert_eq!(
            resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid).unwrap_err(),
            "controlling_page_lists_disagree"
        );
    }

    #[test]
    fn scenario_page_observation_rejects_ambiguous_page_oid() {
        let pgids = vec![vec![(1, 0)]];
        let pages_by_oid = BTreeMap::from([((1, 0), vec![test_page_id(1), test_page_id(2)])]);

        assert!(
            resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid)
                .unwrap_err()
                .starts_with("pgid_is_ambiguous:")
        );
    }

    fn crop_test_span(offset: u64, len: u64) -> RawSpan {
        RawSpan {
            stream: StreamPath("Escher/EscherStm".to_owned()),
            offset,
            len,
        }
    }

    fn crop_test_property(property_id: u16, op: u32) -> pub_escher::Fopte {
        pub_escher::Fopte {
            opid: property_id,
            op,
            source: crop_test_span(0, 6),
            complex_source: None,
            complex_data: None,
        }
    }

    fn crop_test_shape(properties: Vec<pub_escher::Fopte>) -> pub_escher::SpContainerObservation {
        pub_escher::SpContainerObservation {
            source: crop_test_span(0, 0),
            parent_group_shape_source: None,
            fspgr: None,
            fsp: None,
            fopts: vec![pub_escher::FoptObservation {
                rec_type: 0xF00B,
                source: crop_test_span(0, 0),
                properties,
            }],
            client_anchor: None,
            client_data: None,
            client_textbox: None,
            child_anchor: None,
            unknown_children: Vec::new(),
        }
    }

    #[test]
    fn bounded_image_crop_preserves_unique_raw_scalars_and_marks_ambiguity() {
        let shape = crop_test_shape(vec![
            crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_TOP, 28_954),
            crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM, 21_446),
        ]);
        let crop = bounded_officeart_image_crop(&shape).expect("explicit crop");
        assert_eq!(crop.top_raw, Some(28_954));
        assert_eq!(crop.bottom_raw, Some(21_446));
        assert_eq!(crop.left_raw, None);
        assert_eq!(crop.right_raw, None);
        assert!(!crop.ambiguous);

        let conflicting = crop_test_shape(vec![
            crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_LEFT, 1),
            crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_LEFT, 2),
        ]);
        let crop = bounded_officeart_image_crop(&conflicting).expect("conflicting crop");
        assert_eq!(crop.left_raw, None);
        assert!(crop.ambiguous);

        let mut bid = crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_RIGHT, 7);
        bid.opid |= 0x4000;
        let crop = bounded_officeart_image_crop(&crop_test_shape(vec![bid]))
            .expect("fBid crop property remains visible but ambiguous");
        assert_eq!(crop.right_raw, None);
        assert!(crop.ambiguous);
    }

    #[test]
    fn grouped_projection_maps_full_fspgr_extent_exactly() {
        let source = [109_743_916, 106_908_792, 113_353_061, 109_780_257];
        let target = [-837_598, -3_276_408, 3_167_861, -89_633];

        assert_eq!(project_rect_trunc(source, source, target).unwrap(), target);
    }

    #[test]
    fn grouped_projection_matches_obs_017_child_298_with_truncation_toward_zero() {
        let group_coords = [109_743_916, 106_908_792, 113_353_061, 109_780_257];
        let group_absolute = [-837_598, -3_276_408, 3_167_861, -89_633];
        let child_anchor = [111_348_265, 107_124_104, 112_981_404, 108_383_122];

        assert_eq!(
            project_rect_trunc(child_anchor, group_coords, group_absolute).unwrap(),
            [942_921, -3_037_454, 2_755_392, -1_640_185]
        );
    }

    #[test]
    fn page_extent_consensus_accepts_one_extent() {
        assert_eq!(
            require_consensus_page_extent(&[(7_560_000, 10_692_000)]).unwrap(),
            (7_560_000, 10_692_000)
        );
    }

    #[test]
    fn page_extent_consensus_accepts_equivalent_duplicates() {
        assert_eq!(
            require_consensus_page_extent(&[
                (7_772_400, 10_058_400),
                (7_772_400, 10_058_400),
                (7_772_400, 10_058_400),
            ])
            .unwrap(),
            (7_772_400, 10_058_400)
        );
    }

    #[test]
    fn page_extent_consensus_rejects_conflicts() {
        let error =
            require_consensus_page_extent(&[(7_772_400, 10_058_400), (7_560_000, 10_692_000)])
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("conflicting Margins/OplMg page extents")
        );
    }

    #[test]
    fn page_extent_consensus_rejects_zero_dimension() {
        let error = require_consensus_page_extent(&[(7_772_400, 0)]).unwrap_err();
        assert!(error.to_string().contains("must be positive"));
    }

    #[test]
    fn direct_officeart_rgb_accepts_only_unflagged_colorref() {
        assert_eq!(direct_officeart_rgb(0x0000_00FF), Some([0xFF, 0x00, 0x00]));
        assert_eq!(direct_officeart_rgb(0x0000_FF00), Some([0x00, 0xFF, 0x00]));
        assert_eq!(direct_officeart_rgb(0x00FF_0000), Some([0x00, 0x00, 0xFF]));
        assert_eq!(direct_officeart_rgb(0x0800_0007), None);
    }

    #[test]
    fn officeart_scheme_index_resolves_only_in_range_non_dummy_slots() {
        let scheme = MatureColorScheme {
            source: crop_test_span(100, 40),
            declared_count: 3,
            declared_count_source: crop_test_span(106, 4),
            slots: vec![
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 0,
                    rgb: Some([0x10, 0x20, 0x30]),
                    source: crop_test_span(110, 12),
                    rgb_source: Some(crop_test_span(118, 4)),
                },
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 1,
                    rgb: Some([0, 0, 0]),
                    source: crop_test_span(122, 2),
                    rgb_source: None,
                },
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 2,
                    rgb: Some([0xAA, 0xBB, 0xCC]),
                    source: crop_test_span(124, 12),
                    rgb_source: Some(crop_test_span(132, 4)),
                },
            ],
            name: Some("fixture".into()),
            name_source: Some(crop_test_span(136, 14)),
        };

        assert_eq!(
            bounded_officeart_rgb(0x0800_0000, Some(&scheme)),
            Some([0x10, 0x20, 0x30])
        );
        assert_eq!(
            bounded_officeart_rgb(0x0800_0001, Some(&scheme)),
            Some([0, 0, 0])
        );
        assert_eq!(
            bounded_officeart_rgb(0x0800_0002, Some(&scheme)),
            Some([0xAA, 0xBB, 0xCC])
        );
        assert_eq!(bounded_officeart_rgb(0x0800_0003, Some(&scheme)), None);
        assert_eq!(bounded_officeart_rgb(0x0800_0000, None), None);
        assert_eq!(
            bounded_officeart_rgb(0x0000_00FF, Some(&scheme)),
            Some([0xFF, 0x00, 0x00])
        );
        assert_eq!(bounded_officeart_rgb(0x1000_0000, Some(&scheme)), None);
    }

    #[test]
    fn quill_scheme_text_color_resolves_only_through_publication_scheme() {
        let scheme = MatureColorScheme {
            source: crop_test_span(200, 28),
            declared_count: 2,
            declared_count_source: crop_test_span(206, 4),
            slots: vec![
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 0,
                    rgb: Some([0, 0, 0]),
                    source: crop_test_span(210, 2),
                    rgb_source: None,
                },
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 1,
                    rgb: Some([0x11, 0x22, 0x33]),
                    source: crop_test_span(212, 12),
                    rgb_source: Some(crop_test_span(220, 4)),
                },
            ],
            name: Some("fixture".into()),
            name_source: Some(crop_test_span(224, 14)),
        };

        assert_eq!(
            bounded_quill_text_rgb(Some([0xAA, 0xBB, 0xCC]), None, None),
            Some([0xAA, 0xBB, 0xCC])
        );
        assert_eq!(
            bounded_quill_text_rgb(None, Some(0), Some(&scheme)),
            Some([0, 0, 0])
        );
        assert_eq!(
            bounded_quill_text_rgb(None, Some(1), Some(&scheme)),
            Some([0x11, 0x22, 0x33])
        );
        assert_eq!(bounded_quill_text_rgb(None, Some(2), Some(&scheme)), None);
        assert_eq!(bounded_quill_text_rgb(None, Some(0), None), None);
        assert_eq!(
            bounded_quill_text_rgb(Some([1, 2, 3]), Some(0), Some(&scheme)),
            None
        );
    }

    fn dgg_test_defaults(
        primary: Vec<pub_escher::Fopte>,
        tertiary: Vec<pub_escher::Fopte>,
    ) -> pub_escher::DggDefaultOptionsObservation {
        pub_escher::DggDefaultOptionsObservation {
            source: crop_test_span(500, 40),
            primary_options: (!primary.is_empty())
                .then(|| pub_escher::FoptObservation {
                    rec_type: pub_escher::OFFICE_ART_FOPT,
                    source: crop_test_span(504, 16),
                    properties: primary,
                })
                .into_iter()
                .collect(),
            tertiary_options: (!tertiary.is_empty())
                .then(|| pub_escher::FoptObservation {
                    rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                    source: crop_test_span(520, 16),
                    properties: tertiary,
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn effective_officeart_paint_uses_normative_solid_2d_defaults() {
        let shape = crop_test_shape(Vec::new());
        let paint = resolve_bounded_effective_officeart_paint(&shape, None, None, true)
            .expect("bounded 2-D defaults");

        assert_eq!(paint.fill.solid.as_ref().map(|v| v.value), Some(true));
        assert_eq!(
            paint.fill.color_rgb.as_ref().map(|v| v.value),
            Some([0xFF, 0xFF, 0xFF])
        );
        assert_eq!(paint.fill.visible.as_ref().map(|v| v.value), Some(true));
        assert_eq!(
            paint.line.color_rgb.as_ref().map(|v| v.value),
            Some([0, 0, 0])
        );
        assert_eq!(paint.line.width_emu.as_ref().map(|v| v.value), Some(0x2535));
        assert_eq!(paint.line.visible.as_ref().map(|v| v.value), Some(true));
        assert_eq!(
            paint.fill.color_rgb.as_ref().map(|v| v.authority),
            Some(PubEffectivePaintAuthority::NormativeDefault)
        );
        assert!(paint.line.width_emu.as_ref().unwrap().source.is_none());
    }

    #[test]
    fn effective_officeart_paint_prefers_shape_then_dgg_and_honors_use_bits() {
        let shape = crop_test_shape(vec![
            crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_FF00),
            // Value bit without fUse does not participate; DGG visibility wins.
            crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_FILLED_BIT),
            crop_test_property(OFFICE_ART_LINE_WIDTH, 30_000),
        ]);
        let dgg = dgg_test_defaults(
            vec![
                crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF),
                crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_USE_FILLED_BIT),
                crop_test_property(OFFICE_ART_LINE_WIDTH, 20_000),
                crop_test_property(OFFICE_ART_LINE_BOOLEANS, LINE_USE_LINE_BIT | LINE_LINE_BIT),
            ],
            vec![crop_test_property(OFFICE_ART_LINE_COLOR, 0x00FF_0000)],
        );

        let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
            .expect("effective paint");

        let fill_color = paint.fill.color_rgb.unwrap();
        assert_eq!(fill_color.value, [0, 0xFF, 0]);
        assert_eq!(fill_color.authority, PubEffectivePaintAuthority::ShapeLocal);

        let fill_visible = paint.fill.visible.unwrap();
        assert!(!fill_visible.value);
        assert_eq!(
            fill_visible.authority,
            PubEffectivePaintAuthority::DrawingGroupPrimary
        );

        let line_color = paint.line.color_rgb.unwrap();
        assert_eq!(line_color.value, [0, 0, 0xFF]);
        assert_eq!(
            line_color.authority,
            PubEffectivePaintAuthority::DrawingGroupTertiary
        );

        let line_width = paint.line.width_emu.unwrap();
        assert_eq!(line_width.value, 30_000);
        assert_eq!(line_width.authority, PubEffectivePaintAuthority::ShapeLocal);
        assert!(paint.line.visible.unwrap().value);
    }

    #[test]
    fn split_shape_fill_boolean_records_resolve_only_the_ffilled_subfield() {
        let mut visible = crop_test_shape(Vec::new());
        visible.fopts = vec![
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(10, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_FILL_BOOLEANS,
                    FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
                )],
            },
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(20, 8),
                properties: vec![crop_test_property(OFFICE_ART_FILL_BOOLEANS, 0x0060_0020)],
            },
        ];

        assert_eq!(explicit_officeart_paint(&visible, None).fill.visible, None);
        let effective = resolve_bounded_effective_officeart_paint(&visible, None, None, true)
            .expect("bounded 2-D paint");
        let fill = effective.fill.visible.expect("shape-local fill visibility");
        assert!(fill.value);
        assert_eq!(fill.authority, PubEffectivePaintAuthority::ShapeLocal);

        let mut hidden = visible.clone();
        hidden.fopts[0].properties[0] =
            crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_USE_FILLED_BIT);
        assert!(
            !resolve_bounded_effective_officeart_paint(&hidden, None, None, true)
                .expect("bounded 2-D paint")
                .fill
                .visible
                .expect("shape-local hidden fill")
                .value
        );
    }

    #[test]
    fn non_participating_fill_boolean_records_fall_through_to_dgg_or_normative_default() {
        let mut shape = crop_test_shape(Vec::new());
        shape.fopts = vec![
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(10, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_FILL_BOOLEANS,
                    FILL_FILLED_BIT,
                )],
            },
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(20, 8),
                properties: vec![crop_test_property(OFFICE_ART_FILL_BOOLEANS, 0x0060_0020)],
            },
        ];

        let normative = resolve_bounded_effective_officeart_paint(&shape, None, None, true)
            .expect("bounded 2-D paint")
            .fill
            .visible
            .expect("normative fill visibility");
        assert!(normative.value);
        assert_eq!(
            normative.authority,
            PubEffectivePaintAuthority::NormativeDefault
        );

        let dgg = dgg_test_defaults(
            vec![crop_test_property(
                OFFICE_ART_FILL_BOOLEANS,
                FILL_USE_FILLED_BIT,
            )],
            Vec::new(),
        );
        let inherited = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
            .expect("bounded 2-D paint")
            .fill
            .visible
            .expect("DGG fill visibility");
        assert!(!inherited.value);
        assert_eq!(
            inherited.authority,
            PubEffectivePaintAuthority::DrawingGroupPrimary
        );
    }

    #[test]
    fn conflicting_ffilled_use_records_remain_fail_closed() {
        let mut shape = crop_test_shape(Vec::new());
        shape.fopts = vec![
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(10, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_FILL_BOOLEANS,
                    FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
                )],
            },
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(20, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_FILL_BOOLEANS,
                    FILL_USE_FILLED_BIT,
                )],
            },
        ];

        assert_eq!(
            resolve_bounded_effective_officeart_paint(&shape, None, None, true)
                .expect("bounded 2-D paint")
                .fill
                .visible,
            None
        );
    }

    #[test]
    fn split_shape_line_boolean_records_resolve_only_the_fline_subfield() {
        let mut visible = crop_test_shape(Vec::new());
        visible.fopts = vec![
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(10, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_LINE_BOOLEANS,
                    LINE_USE_LINE_BIT | LINE_LINE_BIT,
                )],
            },
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(20, 8),
                properties: vec![crop_test_property(OFFICE_ART_LINE_BOOLEANS, 0x0060_0020)],
            },
        ];

        let explicit = explicit_officeart_paint(&visible, None);
        assert_eq!(explicit.line.visible, Some(true));
        let effective = resolve_bounded_effective_officeart_paint(&visible, None, None, true)
            .expect("bounded 2-D paint");
        let line = effective.line.visible.expect("shape-local line visibility");
        assert!(line.value);
        assert_eq!(line.authority, PubEffectivePaintAuthority::ShapeLocal);

        let mut hidden = visible.clone();
        hidden.fopts[0].properties[0] =
            crop_test_property(OFFICE_ART_LINE_BOOLEANS, LINE_USE_LINE_BIT);
        hidden.fopts[1].properties[0] = crop_test_property(OFFICE_ART_LINE_BOOLEANS, 0x0040_0000);
        assert_eq!(
            explicit_officeart_paint(&hidden, None).line.visible,
            Some(false)
        );
        assert!(
            !resolve_bounded_effective_officeart_paint(&hidden, None, None, true)
                .expect("bounded 2-D paint")
                .line
                .visible
                .expect("shape-local hidden line")
                .value
        );
    }

    #[test]
    fn conflicting_fline_use_records_remain_fail_closed() {
        let mut shape = crop_test_shape(Vec::new());
        shape.fopts = vec![
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(10, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_LINE_BOOLEANS,
                    LINE_USE_LINE_BIT | LINE_LINE_BIT,
                )],
            },
            pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(20, 8),
                properties: vec![crop_test_property(
                    OFFICE_ART_LINE_BOOLEANS,
                    LINE_USE_LINE_BIT,
                )],
            },
        ];

        assert_eq!(explicit_officeart_paint(&shape, None).line.visible, None);
        assert_eq!(
            resolve_bounded_effective_officeart_paint(&shape, None, None, true)
                .expect("bounded 2-D paint")
                .line
                .visible,
            None
        );
    }

    #[test]
    fn effective_officeart_paint_keeps_sparse_explicit_fill_on_normative_color() {
        let mut shape = crop_test_shape(vec![crop_test_property(
            OFFICE_ART_FILL_BOOLEANS,
            FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
        )]);
        shape.fsp = Some(pub_escher::FspRecord {
            spid: 1,
            flags: 0,
            shape_type: 0x0002,
            source: crop_test_span(0, 8),
            trailing_source: None,
        });
        let dgg = dgg_test_defaults(
            vec![crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF)],
            Vec::new(),
        );

        let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
            .expect("sparse RoundRectangle fill remains bounded");

        let fill_color = paint.fill.color_rgb.expect("normative fill color");
        assert_eq!(fill_color.value, [0xFF, 0xFF, 0xFF]);
        assert_eq!(
            fill_color.authority,
            PubEffectivePaintAuthority::NormativeDefault
        );
        assert!(paint.fill.visible.expect("explicit visibility").value);

        shape.fsp.as_mut().expect("fsp").shape_type = 0x00CA;
        let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
            .expect("native-proven sparse TextBox fill remains bounded");
        let fill_color = paint.fill.color_rgb.expect("normative TextBox fill color");
        assert_eq!(fill_color.value, [0xFF, 0xFF, 0xFF]);
        assert_eq!(
            fill_color.authority,
            PubEffectivePaintAuthority::NormativeDefault
        );
        assert!(
            paint
                .fill
                .visible
                .expect("explicit TextBox visibility")
                .value
        );

        shape.fsp.as_mut().expect("fsp").shape_type = 0x0001;
        let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
            .expect("other shape keeps existing DGG fallback");
        assert_eq!(
            paint.fill.color_rgb.expect("DGG fill color").authority,
            PubEffectivePaintAuthority::DrawingGroupPrimary
        );
    }

    #[test]
    fn effective_officeart_paint_fails_closed_on_ambiguous_or_unsupported_override() {
        let ambiguous = crop_test_shape(vec![
            crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF),
            crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_FF00),
        ]);
        let paint = resolve_bounded_effective_officeart_paint(&ambiguous, None, None, true)
            .expect("other normative fields remain available");
        assert_eq!(paint.fill.color_rgb, None);

        let unsupported =
            crop_test_shape(vec![crop_test_property(OFFICE_ART_FILL_COLOR, 0x1000_0000)]);
        let paint = resolve_bounded_effective_officeart_paint(&unsupported, None, None, true)
            .expect("unsupported color stays partial");
        assert_eq!(paint.fill.color_rgb, None);
    }

    #[test]
    fn officeart_visibility_masks_match_publisher_activation_pairs() {
        let fill_disabled = 0x0010_0000;
        let fill_enabled = 0x0010_0010;
        let fill_value_without_use = 0x0000_0010;
        let line_disabled = 0x0008_0000;
        let line_enabled = 0x0008_0008;
        let line_value_without_use = 0x0000_0008;

        assert_eq!(FILL_USE_FILLED_BIT, 0x0010_0000);
        assert_eq!(FILL_FILLED_BIT, 0x0000_0010);
        assert_eq!(LINE_USE_LINE_BIT, 0x0008_0000);
        assert_eq!(LINE_LINE_BIT, 0x0000_0008);

        assert_eq!(
            (fill_disabled & FILL_USE_FILLED_BIT != 0)
                .then_some(fill_disabled & FILL_FILLED_BIT != 0),
            Some(false)
        );
        assert_eq!(
            (fill_enabled & FILL_USE_FILLED_BIT != 0)
                .then_some(fill_enabled & FILL_FILLED_BIT != 0),
            Some(true)
        );
        assert_eq!(
            (fill_value_without_use & FILL_USE_FILLED_BIT != 0)
                .then_some(fill_value_without_use & FILL_FILLED_BIT != 0),
            None
        );
        assert_eq!(
            (line_disabled & LINE_USE_LINE_BIT != 0).then_some(line_disabled & LINE_LINE_BIT != 0),
            Some(false)
        );
        assert_eq!(
            (line_enabled & LINE_USE_LINE_BIT != 0).then_some(line_enabled & LINE_LINE_BIT != 0),
            Some(true)
        );
        assert_eq!(
            (line_value_without_use & LINE_USE_LINE_BIT != 0)
                .then_some(line_value_without_use & LINE_LINE_BIT != 0),
            None
        );
    }

    #[test]
    fn source_object_key_vocabularies_are_explicit_and_disjoint() {
        assert_eq!(contents_object_key(330), "contents/0x2c/seq/330");
        assert_eq!(quill_story_object_key(22), "quill/syid/22");
    }

    #[test]
    fn node_and_story_identity_do_not_collapse_numeric_namespaces() {
        let hash = source_hash();
        let node = derive_pub_node_id(&hash, 22).unwrap();
        let story = derive_pub_story_id(&hash, 22).unwrap();

        assert_ne!(node.into_canonical(), story.into_canonical());
    }
}
