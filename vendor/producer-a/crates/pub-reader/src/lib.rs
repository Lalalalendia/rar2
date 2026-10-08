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
    project_rect_trunc, shape_has_fsp_flag, shape_has_nonzero_rotation,
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
use story_materialization::{decode_utf16le_strict, materialize_story_catalogs};
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
#[path = "root_tests.rs"]
mod tests;
