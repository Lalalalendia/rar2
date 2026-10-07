//! Bounded Microsoft Publisher 2002+ source adapter.
//!
//! This crate is the first format-aware layer above the raw CFB/Contents/Quill/
//! Escher readers. It projects only evidence-backed mature-0x2C semantics into
//! pub-model::SourceGraph and emits diagnostics instead of inventing missing
//! geometry, page roles, or relation state.

use anyhow::{Context, Result, anyhow, bail};

mod asset_export;
mod assets;
mod borderart;
mod borderart_assets;
#[cfg(feature = "cmo-authority-bridge")]
mod cmo_bridge;
mod failure_envelope;
mod failure_intake;
mod family_classifier;
mod group_projection;
mod guide_bridge;
mod intake_protocol;
mod legacy22_graph;
mod legacy22_noquill_graph;
mod legacy22_page_role;
#[cfg(feature = "master-authority-bridge")]
mod master_bridge;
mod mature_wmf;
mod ole_presentation;
mod page_projection;
mod paint_projection;
mod partial_root;
mod resolve;
mod salvage;
mod salvage_authority;
mod source_paint_order;
mod story_frame_analysis;
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
#[cfg(test)]
use group_projection::project_rect_trunc;
use group_projection::{grouped_object_target_page_trace, project_grouped_object_shape};
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
use pub_model::{
    Affine2D, AuthorityClass, ByteRange, CanonicalId, Decimal, Document, DocumentId, LengthEmu,
    Node, NodeHeader, NodeId, NodeKind, Page, PageId, ReadConfidence, RectEmu, Sha256Digest,
    Size2D, SourceDerivedIdInput, SourceDescriptor, SourceGraph, SourceRef, SourceRole, Story,
    StoryId, derive_source_canonical_id,
};
use pub_quill::{
    QuillEffectiveBoolean, QuillGroundedStoryIdentity, QuillMcldReadError,
    QuillMcldVerticalAlignment, QuillParagraphAlignment, QuillParagraphFlowConstraint,
    QuillParagraphLineSpacing, QuillScriptFontEntryDisposition, QuillStoryReadError,
    QuillTypographyValueSource, bounded_mcld_text_frame_vertical_alignment,
    bounded_mcld_text_insets, parse_bounded_fdpp_exact_story_catalog, parse_bounded_mcld,
    parse_bounded_typography, parse_confirmed_story_catalog,
};
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
    probe_reader_salvage_candidate_with_trigger,
};
pub use salvage_authority::{
    ReaderEvidenceDisposition, ReaderSalvageAuthority, reader_evidence_disposition,
    typed_corruption_authority,
};
use serde::{Deserialize, Serialize};
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
#[cfg(test)]
use typography_projection::project_effective_boolean_v1;
pub use typography_projection::{
    PubParagraphAlignment, PubParagraphAlignmentRun, PubParagraphFlowConstraint,
    PubParagraphFlowRun, PubParagraphLineSpacing, PubParagraphLineSpacingRun, PubScriptFontEntry,
    PubScriptFontEntryDisposition, PubScriptFontMap, PubTypographyBooleanV1, PubTypographyRun,
    PubTypographySizeRun,
};
use typography_projection::{PubTypographyProjection, project_typography_catalog};
pub use wmf::{BoundedWmfMetafile, WmfMetafileInfo, bounded_wmf_metafile, validate_wmf_metafile};
pub use wmf_preview::{
    LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1, WmfPreviewRgba, rasterize_wmf_preview,
};

pub const PUB_ADAPTER_ID: &str = "pub-rs";
pub const PUB_FORMAT_PROFILE_ID: &str = "pub-mature-0x2c-v0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundedDirectImageTransform {
    Identity,
    Applied(Affine2D),
    Unsupported,
}

fn div_round_nearest_i128(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        -((-numerator + denominator / 2) / denominator)
    }
}

fn mul_affine_scaled(left: i128, right: i128) -> i128 {
    div_round_nearest_i128(left * right, AFFINE_DECIMAL_SCALE)
}

fn decimal_from_affine_scaled(value: i128) -> Decimal {
    let negative = value < 0;
    let absolute = value.abs();
    let integer = absolute / AFFINE_DECIMAL_SCALE;
    let fraction = absolute % AFFINE_DECIMAL_SCALE;
    let mut rendered = if fraction == 0 {
        integer.to_string()
    } else {
        let mut rendered = format!("{integer}.{fraction:012}");
        while rendered.ends_with('0') {
            rendered.pop();
        }
        rendered
    };
    if negative && absolute != 0 {
        rendered.insert(0, '-');
    }
    rendered
        .parse()
        .expect("internally generated affine decimal must be valid")
}

fn officeart_rotation_sin_cos_scaled(rotation_op: u32) -> (i128, i128) {
    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const HALF_TURN_UNITS: i64 = 180 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;

    let mut angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if angle > HALF_TURN_UNITS {
        angle -= FULL_TURN_UNITS;
    } else if angle < -HALF_TURN_UNITS {
        angle += FULL_TURN_UNITS;
    }

    if angle == 0 {
        return (0, AFFINE_DECIMAL_SCALE);
    }
    if angle == QUARTER_TURN_UNITS {
        return (AFFINE_DECIMAL_SCALE, 0);
    }
    if angle == -QUARTER_TURN_UNITS {
        return (-AFFINE_DECIMAL_SCALE, 0);
    }
    if angle == HALF_TURN_UNITS || angle == -HALF_TURN_UNITS {
        return (0, -AFFINE_DECIMAL_SCALE);
    }

    let mut cosine_sign = 1i128;
    if angle > QUARTER_TURN_UNITS {
        angle = HALF_TURN_UNITS - angle;
        cosine_sign = -1;
    } else if angle < -QUARTER_TURN_UNITS {
        angle = -HALF_TURN_UNITS - angle;
        cosine_sign = -1;
    }

    let radians =
        div_round_nearest_i128(i128::from(angle) * PI_SCALED, i128::from(HALF_TURN_UNITS));
    let radians_sq = mul_affine_scaled(radians, radians);

    let mut sine = radians;
    let mut sine_term = radians;
    for order in 1i128..=9 {
        sine_term = -div_round_nearest_i128(
            mul_affine_scaled(sine_term, radians_sq),
            (2 * order) * (2 * order + 1),
        );
        sine += sine_term;
    }

    let mut cosine = AFFINE_DECIMAL_SCALE;
    let mut cosine_term = AFFINE_DECIMAL_SCALE;
    for order in 1i128..=9 {
        cosine_term = -div_round_nearest_i128(
            mul_affine_scaled(cosine_term, radians_sq),
            (2 * order - 1) * (2 * order),
        );
        cosine += cosine_term;
    }

    (sine, cosine * cosine_sign)
}

fn affine_rotation_about_bounds(rotation_op: u32, bounds: RectEmu) -> Option<Affine2D> {
    let (sine, cosine) = officeart_rotation_sin_cos_scaled(rotation_op);
    let a = cosine;
    let b = sine;
    let c = -sine;
    let d = cosine;

    let center_x_twice = i128::from(bounds.x.get()) * 2 + i128::from(bounds.width.get());
    let center_y_twice = i128::from(bounds.y.get()) * 2 + i128::from(bounds.height.get());
    let denominator = 2 * AFFINE_DECIMAL_SCALE;

    let tx = div_round_nearest_i128(
        AFFINE_DECIMAL_SCALE * center_x_twice - a * center_x_twice - c * center_y_twice,
        denominator,
    );
    let ty = div_round_nearest_i128(
        AFFINE_DECIMAL_SCALE * center_y_twice - b * center_x_twice - d * center_y_twice,
        denominator,
    );

    Some(Affine2D {
        a: decimal_from_affine_scaled(a),
        b: decimal_from_affine_scaled(b),
        c: decimal_from_affine_scaled(c),
        d: decimal_from_affine_scaled(d),
        tx: LengthEmu::new(i64::try_from(tx).ok()?),
        ty: LengthEmu::new(i64::try_from(ty).ok()?),
    })
}

fn bounded_direct_image_cardinal_content_rotation_degrees(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
) -> Option<i16> {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0
        || rotation_properties.len() != 1
        || rotation_properties
            .iter()
            .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
    {
        return None;
    }

    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;
    let (rotation_op, _, _) = rotation_properties[0];
    let mut angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if angle < 0 {
        angle += FULL_TURN_UNITS;
    }

    match angle {
        QUARTER_TURN_UNITS => Some(90),
        angle if angle == 2 * QUARTER_TURN_UNITS => Some(180),
        angle if angle == 3 * QUARTER_TURN_UNITS => Some(270),
        _ => None,
    }
}

fn bounded_direct_image_transform(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
    bounds: RectEmu,
) -> BoundedDirectImageTransform {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0 {
        return BoundedDirectImageTransform::Unsupported;
    }
    if rotation_properties
        .iter()
        .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
        || rotation_properties.len() > 1
    {
        return BoundedDirectImageTransform::Unsupported;
    }

    let Some((rotation_op, _, _)) = rotation_properties.first().copied() else {
        return BoundedDirectImageTransform::Identity;
    };
    if rotation_op as i32 == 0 {
        return BoundedDirectImageTransform::Identity;
    }

    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const HALF_TURN_UNITS: i64 = 180 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;
    let mut signed_angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if signed_angle > HALF_TURN_UNITS {
        signed_angle -= FULL_TURN_UNITS;
    } else if signed_angle < -HALF_TURN_UNITS {
        signed_angle += FULL_TURN_UNITS;
    }
    if signed_angle == 0 {
        return BoundedDirectImageTransform::Identity;
    }
    if matches!(signed_angle.abs(), QUARTER_TURN_UNITS | HALF_TURN_UNITS) {
        return BoundedDirectImageTransform::Unsupported;
    }

    affine_rotation_about_bounds(rotation_op, bounds)
        .map(BoundedDirectImageTransform::Applied)
        .unwrap_or(BoundedDirectImageTransform::Unsupported)
}

fn bounded_direct_story_transform(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
    bounds: RectEmu,
) -> Option<Affine2D> {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0 {
        return None;
    }
    if rotation_properties
        .iter()
        .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
        || rotation_properties.len() > 1
    {
        return None;
    }

    let Some((rotation_op, _, _)) = rotation_properties.first().copied() else {
        return Some(Affine2D::identity());
    };
    if rotation_op as i32 == 0 {
        return Some(Affine2D::identity());
    }

    affine_rotation_about_bounds(rotation_op, bounds)
}

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

const ROLE_DOCUMENT: &str = "cdm.document";
const ROLE_PAGE: &str = "cdm.page";
const ROLE_NODE: &str = "cdm.node";
const ROLE_STORY: &str = "cdm.story";

pub type PubSourceGraph = SourceGraph<PubNodePayload, (), (), (), String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubSourceGraphBuild {
    pub graph: PubSourceGraph,
    pub effective_pages: PubEffectivePageProjection,
    /// Page-local back-to-front order for bounded source-backed paint
    /// participants whose persisted OfficeArt SpContainer position can be
    /// joined unambiguously to canonical Nodes. This includes direct page
    /// objects and exact depth-1 grouped descendants at their proven top-level
    /// GROUP carrier rank. Pages/classes with incomplete or ambiguous coverage
    /// remain partial rather than receiving an invented order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_page_paint_orders: Vec<PubSourcePagePaintOrderV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PubBridgeDiagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typography_runs: Vec<PubTypographyRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typography_size_runs: Vec<PubTypographySizeRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraph_alignments: Vec<PubParagraphAlignmentRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraph_line_spacings: Vec<PubParagraphLineSpacingRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraph_flow_runs: Vec<PubParagraphFlowRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_font_maps: Vec<PubScriptFontMap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubLegacyOleSource {
    /// Exact persisted CFB storage number joined to `/Objects/Object N`.
    pub storage_number: u16,
    /// Persisted legacy Publisher-side flag. Its semantics remain intentionally opaque.
    pub raw_flag: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTextFrameInsetSource {
    pub layout_record_id: u32,
    pub top_emu: u32,
    pub left_emu: u32,
    pub bottom_emu: u32,
    pub right_emu: u32,
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubTextFrameVerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTextFrameVerticalAlignmentSource {
    pub layout_record_id: u32,
    pub alignment: PubTextFrameVerticalAlignment,
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubNodePayload {
    pub contents_seq_num: u32,
    pub officeart_shape_type: Option<u16>,
    pub officeart_spid: Option<u32>,
    /// Exact one-based OfficeArt BStore identity from non-complex fBid pib.
    pub image_slot: Option<u32>,
    /// Bounded legacy OLE identity. Presence never authorizes OLE/COM activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_ole: Option<PubLegacyOleSource>,
    /// Bounded observation of explicit OfficeArt picture-crop properties on
    /// this image-bound shape. Raw scalar values are intentionally not
    /// reinterpreted as Publisher points or normalized crop geometry here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_image_crop: Option<PubExplicitImageCropSource>,
    /// Exact cardinal OfficeArt picture-content rotation admitted separately
    /// from the outer NodeHeader transform. This keeps already-resolved frame
    /// geometry fixed while preserving bounded image-fill orientation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_image_cardinal_rotation_degrees: Option<i16>,
    /// Bounded source-backed picture recolor state. This is placement/node state,
    /// not image-resource state: the same embedded image can be reused with
    /// different recolor targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_image_recolor: Option<PubExplicitImageRecolorSource>,
    /// Explicit shape-local OfficeArt paint state only.
    pub explicit_paint: PubExplicitShapePaintSource,
    /// Bounded effective solid-paint resolution for admitted 2-D shapes.
    /// Each field retains whether it came from shape-local properties, DGG
    /// defaults, or the normative MS-ODRAW default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_paint: Option<PubEffectiveShapePaintSource>,
    pub story_frame: Option<PubStoryFrameSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_frame_inset: Option<PubTextFrameInsetSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_story: Option<PubTableStoryOwnershipSource>,
    pub table: Option<PubTableSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubExplicitImageCropSource {
    pub top_raw: Option<u32>,
    pub bottom_raw: Option<u32>,
    pub left_raw: Option<u32>,
    pub right_raw: Option<u32>,
    /// True when at least one crop property cannot be represented as one
    /// unambiguous non-complex scalar value. Presence remains explicit so
    /// callers must fail closed rather than misclassify the image as crop-free.
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubExplicitImageRecolorSource {
    pub target_rgb: [u8; 3],
    pub preserve_grays: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubExplicitShapePaintSource {
    pub fill: PubExplicitFillSource,
    pub line: PubExplicitLineSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubExplicitFillSource {
    /// Explicit `fillType` is solid (MS-ODRAW 0x0180 == 0).
    pub solid: bool,
    /// Direct RGB only; scheme/system/palette COLORREF forms are not promoted.
    pub color_rgb: Option<[u8; 3]>,
    /// Set only when fUsefFilled is present in explicit 0x01BF.
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubExplicitLineSource {
    /// Direct RGB only; scheme/system/palette COLORREF forms are not promoted.
    pub color_rgb: Option<[u8; 3]>,
    /// Explicit shape-local line width in EMU (MS-ODRAW 0x01CB).
    pub width_emu: Option<i64>,
    /// Set only when fUsefLine is present in explicit 0x01FF.
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubEffectivePaintAuthority {
    ShapeLocal,
    DrawingGroupPrimary,
    DrawingGroupTertiary,
    NormativeDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubEffectivePaintValue<T> {
    pub value: T,
    pub authority: PubEffectivePaintAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RawSpan>,
}

impl<T> PubEffectivePaintValue<T> {
    fn map<U>(self, map: impl FnOnce(T) -> U) -> PubEffectivePaintValue<U> {
        PubEffectivePaintValue {
            value: map(self.value),
            authority: self.authority,
            source: self.source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubEffectiveShapePaintSource {
    pub fill: PubEffectiveFillSource,
    pub line: PubEffectiveLineSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubEffectiveFillSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<PubEffectivePaintValue<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_rgb: Option<PubEffectivePaintValue<[u8; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<PubEffectivePaintValue<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubEffectiveLineSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_rgb: Option<PubEffectivePaintValue<[u8; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_emu: Option<PubEffectivePaintValue<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<PubEffectivePaintValue<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubStoryFrameSource {
    pub text_id: u32,
    pub story_id: Option<StoryId>,
    /// Exact source state. Omission is not rewritten to effective ordinal zero
    /// until the resolver layer.
    pub explicit_ordinal: Option<u32>,
    pub previous_seq_num: Option<u32>,
    pub previous_frame: Option<NodeId>,
    pub next_seq_num: Option<u32>,
    pub next_frame: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_alignment: Option<PubTextFrameVerticalAlignmentSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PubBridgeDiagnostic {
    PageListSpecialEntry {
        handle: u32,
        raw_type: u16,
    },
    PageListUnknownEntry {
        handle: u32,
        raw_type: Option<u16>,
    },
    OpaqueContentsTail {
        seq_num: u32,
        byte_range: ByteRange,
    },
    MissingEscherGeometry {
        seq_num: u32,
    },
    AmbiguousEscherGeometry {
        seq_num: u32,
        matches: usize,
    },
    AmbiguousImageSlot {
        seq_num: u32,
        slots: Vec<u32>,
    },
    IncompleteEscherAnchor {
        seq_num: u32,
    },
    InvalidEscherAnchor {
        seq_num: u32,
    },
    MissingQuillStory {
        seq_num: u32,
        text_id: u32,
    },
    LegacyObjectNotMaterialized {
        object_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw_type: Option<u16>,
        reason: String,
    },
    LegacyTextEncodingUnresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_id: Option<u32>,
        source: RawSpan,
        high_byte_count: usize,
    },
    McldRecordCountMismatch {
        record_count: u32,
        record_id_count: u32,
    },
    McldOuterBoundViolation {
        outer_value: u32,
        max_live_record_id: u32,
    },
    FdppExactStoryFallback {
        story_count: usize,
    },
    EquivalentMarginsPageExtents {
        count: usize,
        width_emu: u32,
        height_emu: u32,
    },
    ScenarioPageOrderObserved {
        raw_page_count: usize,
        scenario_page_count: usize,
        evidence_list_count: usize,
    },
    ScenarioPageOrderUnavailable {
        reason: String,
        raw_page_count: usize,
    },
    PageRoleClassificationUnresolved {
        raw_page_count: usize,
    },
    LinkedFrameNotMaterialized {
        seq_num: u32,
        target_seq_num: u32,
    },
    GroupedStoryProjected {
        seq_num: u32,
        depth: usize,
    },
    GroupedStoryProjectionUnavailable {
        seq_num: u32,
        reason: String,
    },
    GroupedImageProjected {
        seq_num: u32,
        depth: usize,
    },
    GroupedImageProjectionUnavailable {
        seq_num: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_page_id: Option<PageId>,
        image_slot: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        group_ancestry: Vec<u32>,
        reason: String,
    },
    GroupedPrimitiveProjected {
        seq_num: u32,
        shape_type: u16,
        depth: usize,
    },
    GroupedPrimitiveProjectionUnavailable {
        seq_num: u32,
        shape_type: u16,
        reason: String,
    },
    GroupedTableProjected {
        seq_num: u32,
        depth: usize,
    },
    GroupedTableProjectionUnavailable {
        seq_num: u32,
        reason: String,
    },
    TableMissingRequiredField {
        seq_num: u32,
        field_id: u16,
    },
    TableMissingTcd {
        seq_num: u32,
        text_id: u32,
    },
    TableAmbiguousTcd {
        seq_num: u32,
        text_id: u32,
    },
    TableMissingCellsObject {
        seq_num: u32,
        cells_seq_num: u32,
    },
    TableCellsWrongRawType {
        seq_num: u32,
        cells_seq_num: u32,
        raw_type: Option<u16>,
    },
    TableCellsWrongParent {
        seq_num: u32,
        cells_seq_num: u32,
        parent_seq_num: Option<u32>,
    },
    TableCellCountMismatch {
        seq_num: u32,
        cells_records: usize,
        tcd_boundaries: usize,
    },
    TableCellTextRangeInvalid {
        seq_num: u32,
        stored_record_index: u32,
        previous_end: u32,
        end: u32,
        story_len: u32,
    },
    TableStoryLengthMismatch {
        seq_num: u32,
        tcd_last_end: u32,
        story_len: u32,
    },
    TableCellCoordinatesAmbiguous {
        seq_num: u32,
        stored_record_index: u32,
    },
    TableLayoutMetricsUnavailable {
        seq_num: u32,
        text_id: u32,
        layout_key: Option<u32>,
        reason: String,
    },
    TypographyProjectionUnavailable {
        reason: String,
    },
    TypographyUnknownFixedBlockTypes {
        block_types: Vec<u8>,
    },
    ColorSchemeProjectionUnavailable {
        reason: String,
    },
    AmbiguousOfficeArtDggDefaults {
        count: usize,
    },
}

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

/// Canonical source key for a physical mature-0x2C Contents directory slot.
pub fn contents_object_key(seq_num: u32) -> String {
    format!("contents/0x2c/seq/{seq_num}")
}

/// Canonical source key for a persistent Quill SYID story identity.
pub fn quill_story_object_key(syid: u32) -> String {
    format!("quill/syid/{syid}")
}

pub fn derive_pub_document_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<DocumentId> {
    Ok(DocumentId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_DOCUMENT,
    )?))
}

pub fn derive_pub_page_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<PageId> {
    Ok(PageId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_PAGE,
    )?))
}

pub fn derive_pub_node_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<NodeId> {
    Ok(NodeId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_NODE,
    )?))
}

pub fn derive_pub_story_id(source_hash: &Sha256Digest, syid: u32) -> Result<StoryId> {
    Ok(StoryId::from_canonical(derive_pub_id(
        source_hash,
        &quill_story_object_key(syid),
        ROLE_STORY,
    )?))
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

fn shape_has_nonzero_rotation(shape: &pub_escher::SpContainerObservation) -> bool {
    shape.fopts.iter().any(|record| {
        record.properties.iter().any(|property| {
            property.property_id() == OFFICEART_PROPERTY_ROTATION && property.op != 0
        })
    })
}

fn shape_has_fsp_flag(shape: &pub_escher::SpContainerObservation, flag: u32) -> bool {
    shape.fsp.as_ref().is_some_and(|fsp| fsp.flags & flag != 0)
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

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_seq = seq_u32(document_reference.seq_num)?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList")?;

    let mut diagnostics = Vec::new();
    let color_scheme =
        match current_publication_color_scheme(contents_stream.clone(), contents, &references) {
            Ok(scheme) => scheme,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ColorSchemeProjectionUnavailable {
                    reason: error.to_string(),
                });
                None
            }
        };
    let (page_width_emu, page_height_emu, margins_count) =
        consensus_publication_page_extent(contents_stream.clone(), contents, &references)?;
    if margins_count > 1 {
        diagnostics.push(PubBridgeDiagnostic::EquivalentMarginsPageExtents {
            count: margins_count,
            width_emu: page_width_emu,
            height_emu: page_height_emu,
        });
    }

    let document_id = derive_pub_document_id(&source_hash, document_seq)?;
    let mut document_pages = Vec::new();
    let mut pages = BTreeMap::new();
    let mut page_seq_to_id = BTreeMap::new();
    let mut seen_page_handles = BTreeSet::new();

    for entry in &page_list.entries {
        let Some(reference) = references.get(&entry.handle) else {
            diagnostics.push(PubBridgeDiagnostic::PageListUnknownEntry {
                handle: entry.handle,
                raw_type: None,
            });
            continue;
        };

        match single_raw_type(reference) {
            Some(RAW_TYPE_PAGE) => {
                if !seen_page_handles.insert(entry.handle) {
                    bail!("DOCUMENT PageList repeats PAGE handle {}", entry.handle);
                }
                let page_id = derive_pub_page_id(&source_hash, entry.handle)?;
                document_pages.push(page_id);
                page_seq_to_id.insert(entry.handle, page_id);
                pages.insert(
                    page_id,
                    Page {
                        id: page_id,
                        size: Size2D::new(
                            LengthEmu::new(i64::from(page_width_emu)),
                            LengthEmu::new(i64::from(page_height_emu)),
                        ),
                        bleed: None,
                        margins: None,
                        // Membership is preserved by NodeHeader.parent_id. We do
                        // not guess a z-order from directory seqNum.
                        children: Vec::new(),
                        extensions: Vec::new(),
                    },
                );
            }
            Some(RAW_TYPE_PAGE_LIST_SPECIAL) => {
                diagnostics.push(PubBridgeDiagnostic::PageListSpecialEntry {
                    handle: entry.handle,
                    raw_type: RAW_TYPE_PAGE_LIST_SPECIAL,
                });
            }
            other => {
                diagnostics.push(PubBridgeDiagnostic::PageListUnknownEntry {
                    handle: entry.handle,
                    raw_type: other,
                });
            }
        }
    }

    if pages.is_empty() {
        bail!("DOCUMENT PageList exposes no confirmed PAGE 0x43 entries");
    }

    let (effective_pages, page_projection_diagnostics) = derive_effective_page_projection(
        contents_stream.clone(),
        contents,
        &references,
        &page_seq_to_id,
        &document_pages,
    );
    diagnostics.extend(page_projection_diagnostics);

    let document = Document {
        id: document_id,
        format_origin: "pub".into(),
        source_hash,
        pages: document_pages,
        resources: Vec::new(),
        styles: Vec::new(),
    };
    let mut graph = PubSourceGraph::empty(source, document);
    graph.pages = pages;

    let quill_stream = StreamPath(QUILL_STREAM_PATH.into());
    let mut fdpp_story_catalog = None;
    let quill_catalog = match parse_confirmed_story_catalog(quill_stream.clone(), quill) {
        Ok(catalog) => Some(catalog),
        Err(QuillStoryReadError::MissingRequiredChunk { name })
            if physical_empty_story_catalog && name == *b"STRS" =>
        {
            diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                reason: "physical-empty Story catalog has no live Story references and Quill omits STRS; admitting geometry-only publication without fabricating Story text".to_owned(),
            });
            None
        }
        Err(ordinary_error) => {
            let Some(story_catalog) = grounded_story_catalog.as_ref() else {
                return Err(ordinary_error).context("parse grounded Quill story catalog");
            };
            let identities = story_catalog
                .entries
                .iter()
                .map(|entry| QuillGroundedStoryIdentity {
                    syid: pub_core::QuillSyid(entry.text_id),
                    source: entry.text_id_source.clone(),
                })
                .collect::<Vec<_>>();
            match parse_bounded_fdpp_exact_story_catalog(quill_stream.clone(), quill, &identities)
                .context("parse bounded exact-FDPP Story fallback")?
            {
                Some(catalog) => {
                    diagnostics.push(PubBridgeDiagnostic::FdppExactStoryFallback {
                        story_count: catalog.stories.len(),
                    });
                    fdpp_story_catalog = Some(catalog);
                    None
                }
                None => {
                    return Err(ordinary_error).context("parse grounded Quill story catalog");
                }
            }
        }
    };
    let typography_catalog = if let Some(quill_catalog) = quill_catalog.as_ref() {
        match parse_bounded_typography(quill, quill_catalog) {
            Ok(catalog) => {
                let mut unknown = catalog.unknown_block_types_assumed_zero_length.clone();
                unknown.extend(
                    catalog
                        .inheritance_unknown_block_types_assumed_zero_length
                        .iter()
                        .copied(),
                );
                unknown.sort_unstable();
                unknown.dedup();
                if !unknown.is_empty() {
                    diagnostics.push(PubBridgeDiagnostic::TypographyUnknownFixedBlockTypes {
                        block_types: unknown,
                    });
                }
                Some(catalog)
            }
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: error.to_string(),
                });
                None
            }
        }
    } else {
        None
    };
    let mcld = if let Some(quill_catalog) = quill_catalog.as_ref() {
        match parse_bounded_mcld(quill_stream.clone(), quill, &quill_catalog.descriptor_nodes) {
            Ok(mcld) => Some(mcld),
            Err(QuillMcldReadError::MissingMcldDescriptor) => None,
            Err(QuillMcldReadError::RecordCountMismatch {
                record_count,
                record_id_count,
            }) => {
                diagnostics.push(PubBridgeDiagnostic::McldRecordCountMismatch {
                    record_count,
                    record_id_count,
                });
                None
            }
            Err(QuillMcldReadError::RecordIdOutsideOuterBound {
                outer_value,
                max_live_record_id,
            }) => {
                diagnostics.push(PubBridgeDiagnostic::McldOuterBoundViolation {
                    outer_value,
                    max_live_record_id,
                });
                None
            }
            Err(error) => return Err(error).context("parse bounded Quill MCLD"),
        }
    } else {
        None
    };
    let mut story_by_syid = BTreeMap::new();

    if let Some(quill_catalog) = quill_catalog.as_ref() {
        for story_slice in &quill_catalog.stories {
            let syid = story_slice.syid.0;
            let story_id = derive_pub_story_id(&source_hash, syid)?;
            let object_key = quill_story_object_key(syid);
            let text = decode_utf16le_strict(&story_slice.utf16le)
                .with_context(|| format!("decode Quill story SYID {syid} as strict UTF-16LE"))?;

            let source_refs = vec![
                source_ref(
                    &graph.source,
                    &story_slice.syid_source,
                    Some(object_key.clone()),
                    Some("SYID".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.text_source,
                    Some(object_key),
                    Some("TEXT".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ];

            graph.stories.insert(
                story_id,
                Story {
                    id: story_id,
                    text,
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs,
                },
            );
            story_by_syid.insert(syid, story_id);
        }
    }

    if let Some(fdpp_catalog) = fdpp_story_catalog.as_ref() {
        for story_slice in &fdpp_catalog.stories {
            let syid = story_slice.syid.0;
            let story_id = derive_pub_story_id(&source_hash, syid)?;
            let object_key = quill_story_object_key(syid);
            let text = decode_utf16le_strict(&story_slice.utf16le)
                .with_context(|| format!("decode FDPP-bounded Story {syid} as strict UTF-16LE"))?;

            let source_refs = vec![
                source_ref(
                    &graph.source,
                    &story_slice.identity_source,
                    Some(object_key.clone()),
                    Some("Contents/0x65/textId".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.boundary_source,
                    Some(object_key.clone()),
                    Some("FDPP/storyEnd".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.text_source,
                    Some(object_key),
                    Some("TEXT".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ];

            graph.stories.insert(
                story_id,
                Story {
                    id: story_id,
                    text,
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs,
                },
            );
            story_by_syid.insert(syid, story_id);
        }
    }

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

    let escher_inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), escher)
        .context("parse OfficeArt SpContainers")?;
    let escher_by_contents_seq = index_escher_by_contents_seq(&escher_inventory);
    let dgg_default_inventory =
        inspect_dgg_default_options(StreamPath(ESCHER_STREAM_PATH.into()), escher)
            .context("parse OfficeArt DGG default options")?;
    let dgg_defaults_unambiguous = dgg_default_inventory.drawing_groups.len() <= 1;
    if !dgg_defaults_unambiguous {
        diagnostics.push(PubBridgeDiagnostic::AmbiguousOfficeArtDggDefaults {
            count: dgg_default_inventory.drawing_groups.len(),
        });
    }
    let dgg_defaults = dgg_default_inventory.drawing_groups.first();

    for reference in references.values() {
        let raw_type = single_raw_type(reference);
        if raw_type != Some(RAW_TYPE_SHAPE) && raw_type != Some(RAW_TYPE_TABLE) {
            continue;
        }

        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        if let Some(tail) = &chunk.unsupported_tail {
            diagnostics.push(PubBridgeDiagnostic::OpaqueContentsTail {
                seq_num,
                byte_range: ByteRange::new(tail.offset, tail.len),
            });
        }

        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let shape = match matches {
            [] => {
                diagnostics.push(PubBridgeDiagnostic::MissingEscherGeometry { seq_num });
                continue;
            }
            [index] => &escher_inventory.shapes[*index],
            many => {
                diagnostics.push(PubBridgeDiagnostic::AmbiguousEscherGeometry {
                    seq_num,
                    matches: many.len(),
                });
                continue;
            }
        };

        let direct_page = page_seq_to_id.get(&parent_seq).copied();
        let exact_story_identity = match raw_type {
            Some(RAW_TYPE_SHAPE) => unique_u32_field(&chunk, FIELD_STORY_ID)?
                .map(|(value, _)| value)
                .filter(|value| story_by_syid.contains_key(value)),
            Some(RAW_TYPE_TABLE) => {
                unique_story_id_scalar(&chunk)?.filter(|value| story_by_syid.contains_key(value))
            }
            _ => None,
        };
        let image_slot = exact_image_slot(shape, seq_num, &mut diagnostics);
        let exact_grouped_image_identity = raw_type == Some(RAW_TYPE_SHAPE) && image_slot.is_some();
        let exact_grouped_primitive_shape_type = if raw_type == Some(RAW_TYPE_SHAPE)
            && exact_story_identity.is_none()
            && image_slot.is_none()
            && has_default_ellipse_geometry(shape)
        {
            Some(OFFICEART_SHAPE_TYPE_ELLIPSE)
        } else {
            None
        };
        let grouped_projection = if direct_page.is_none()
            && references.get(&parent_seq).and_then(single_raw_type) == Some(RAW_TYPE_GROUP)
            && (exact_story_identity.is_some()
                || exact_grouped_image_identity
                || exact_grouped_primitive_shape_type.is_some())
        {
            match project_grouped_object_shape(
                parent_seq,
                shape,
                &references,
                &page_seq_to_id,
                &graph.pages,
                &escher_inventory,
                &escher_by_contents_seq,
            ) {
                Ok(Some(projection)) => {
                    diagnostics.push(if raw_type == Some(RAW_TYPE_TABLE) {
                        PubBridgeDiagnostic::GroupedTableProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    } else if exact_story_identity.is_some() {
                        PubBridgeDiagnostic::GroupedStoryProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    } else if let Some(shape_type) = exact_grouped_primitive_shape_type {
                        PubBridgeDiagnostic::GroupedPrimitiveProjected {
                            seq_num,
                            shape_type,
                            depth: projection.depth,
                        }
                    } else {
                        PubBridgeDiagnostic::GroupedImageProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    });
                    Some(projection)
                }
                Ok(None) => None,
                Err(error) => {
                    diagnostics.push(if raw_type == Some(RAW_TYPE_TABLE) {
                        PubBridgeDiagnostic::GroupedTableProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    } else if exact_story_identity.is_some() {
                        PubBridgeDiagnostic::GroupedStoryProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    } else if let Some(shape_type) = exact_grouped_primitive_shape_type {
                        PubBridgeDiagnostic::GroupedPrimitiveProjectionUnavailable {
                            seq_num,
                            shape_type,
                            reason: error.to_string(),
                        }
                    } else {
                        let (target_page_id, group_ancestry) = grouped_object_target_page_trace(
                            parent_seq,
                            &references,
                            &page_seq_to_id,
                        );
                        PubBridgeDiagnostic::GroupedImageProjectionUnavailable {
                            seq_num,
                            target_page_id,
                            image_slot: image_slot
                                .expect("grouped image identity requires exact image slot"),
                            group_ancestry,
                            reason: error.to_string(),
                        }
                    });
                    None
                }
            }
        } else {
            None
        };

        let (page_id, bounds, grouped_sources, direct_image_anchor_recovered_from_contents_extent) =
            if let Some(page_id) = direct_page {
                let Some(anchor) = shape.client_anchor.as_ref() else {
                    diagnostics.push(PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num });
                    continue;
                };
                let page = graph
                    .pages
                    .get(&page_id)
                    .expect("page id came from graph registry");
                let (bounds, recovered_from_contents_extent) =
                    if let Some(bounds) = page_relative_bounds(page, anchor) {
                        (bounds, false)
                    } else {
                        let recovered = if raw_type == Some(RAW_TYPE_SHAPE)
                            && exact_story_identity.is_none()
                            && image_slot.is_some()
                        {
                            match (
                                unique_u32_field(&chunk, FIELD_SHAPE_WIDTH)?,
                                unique_u32_field(&chunk, FIELD_SHAPE_HEIGHT)?,
                            ) {
                                (Some((width, _)), Some((height, _))) => {
                                    page_relative_bounds_from_contents_missing_xe(
                                        page, anchor, width, height,
                                    )
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let Some(bounds) = recovered else {
                            let complete = anchor_has_unique_geometry_fields(anchor);
                            diagnostics.push(if complete {
                                PubBridgeDiagnostic::InvalidEscherAnchor { seq_num }
                            } else {
                                PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num }
                            });
                            continue;
                        };
                        (bounds, true)
                    };
                (page_id, bounds, Vec::new(), recovered_from_contents_extent)
            } else if let Some(projection) = grouped_projection {
                (
                    projection.page_id,
                    projection.bounds,
                    projection.group_sources,
                    false,
                )
            } else {
                continue;
            };

        let node_id = derive_pub_node_id(&source_hash, seq_num)?;
        let explicit_paint =
            explicit_officeart_paint(shape, color_scheme.as_ref().map(|scheme| &scheme.scheme));
        let effective_paint = dgg_defaults_unambiguous
            .then(|| {
                resolve_bounded_effective_officeart_paint(
                    shape,
                    dgg_defaults,
                    color_scheme.as_ref().map(|scheme| &scheme.scheme),
                    admits_normative_2d_paint_defaults(shape),
                )
            })
            .flatten();
        let explicit_image_crop = image_slot
            .is_some()
            .then(|| bounded_officeart_image_crop(shape))
            .flatten();
        let explicit_image_recolor = image_slot
            .is_some()
            .then(|| {
                bounded_officeart_image_recolor(
                    shape,
                    color_scheme.as_ref().map(|scheme| &scheme.scheme),
                )
            })
            .flatten();
        let mut story_frame = if raw_type == Some(RAW_TYPE_SHAPE) {
            build_story_frame(
                source_hash,
                seq_num,
                &chunk,
                &story_by_syid,
                &mut diagnostics,
            )?
        } else {
            None
        };
        let text_frame_inset = story_frame.as_ref().and_then(|frame| {
            let (layout_record_id, layout_key_source) = story_layout_keys.get(&frame.text_id)?;
            let mcld = mcld.as_ref()?;
            let inset = bounded_mcld_text_insets(mcld, *layout_record_id).ok()?;
            let object_key = quill_story_object_key(frame.text_id);
            let mut source_refs = vec![source_ref(
                &graph.source,
                layout_key_source,
                Some(object_key.clone()),
                Some("Contents/0x65/layoutKey".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            )];
            source_refs.extend(inset.sources.iter().map(|source| {
                source_ref(
                    &graph.source,
                    source,
                    Some(object_key.clone()),
                    Some("MCLD/06..09/text-inset".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                )
            }));
            Some(PubTextFrameInsetSource {
                layout_record_id: *layout_record_id,
                top_emu: inset.top_emu,
                left_emu: inset.left_emu,
                bottom_emu: inset.bottom_emu,
                right_emu: inset.right_emu,
                source_refs,
            })
        });
        let text_frame_vertical_alignment = story_frame.as_ref().and_then(|frame| {
            let (layout_record_id, layout_key_source) = story_layout_keys.get(&frame.text_id)?;
            let mcld = mcld.as_ref()?;
            let vertical =
                bounded_mcld_text_frame_vertical_alignment(mcld, *layout_record_id).ok()?;
            let object_key = quill_story_object_key(frame.text_id);
            Some(PubTextFrameVerticalAlignmentSource {
                layout_record_id: *layout_record_id,
                alignment: match vertical.alignment {
                    QuillMcldVerticalAlignment::Top => PubTextFrameVerticalAlignment::Top,
                    QuillMcldVerticalAlignment::Center => PubTextFrameVerticalAlignment::Center,
                    QuillMcldVerticalAlignment::Bottom => PubTextFrameVerticalAlignment::Bottom,
                },
                source_refs: vec![
                    source_ref(
                        &graph.source,
                        layout_key_source,
                        Some(object_key.clone()),
                        Some("Contents/0x65/layoutKey".into()),
                        SourceRole::Relation,
                        AuthorityClass::Authoritative,
                        ReadConfidence::Exact,
                    ),
                    source_ref(
                        &graph.source,
                        &vertical.source,
                        Some(object_key),
                        Some("MCLD/18/text-vertical-align".into()),
                        SourceRole::Semantic,
                        AuthorityClass::Authoritative,
                        ReadConfidence::Exact,
                    ),
                ],
            })
        });
        if let (Some(frame), Some(vertical_alignment)) =
            (story_frame.as_mut(), text_frame_vertical_alignment)
        {
            frame.vertical_alignment = Some(vertical_alignment);
        }
        let (table_story, table) = if raw_type == Some(RAW_TYPE_TABLE) {
            if let Some(quill_catalog) = quill_catalog.as_ref() {
                let context = table_bridge::TableBridgeContext {
                    source: &graph.source,
                    contents_stream: &contents_stream,
                    contents,
                    references: &references,
                    quill_catalog,
                    story_by_syid: &story_by_syid,
                    story_layout_keys: &story_layout_keys,
                    mcld: mcld.as_ref(),
                    table_bounds: &bounds,
                    officeart_owner_shape: shape,
                    officeart_inventory: &escher_inventory,
                    color_scheme: color_scheme.as_ref().map(|value| &value.scheme),
                    dgg_defaults: if dgg_defaults_unambiguous {
                        dgg_defaults
                    } else {
                        None
                    },
                };
                (
                    table_bridge::build_table_story_ownership_source(&context, seq_num, &chunk)?,
                    table_bridge::build_table_source(&context, seq_num, &chunk, &mut diagnostics)?,
                )
            } else {
                // The only no-catalog admission is the already-fenced physical-empty
                // Story65 variant with no live SHAPE/TABLE Story identity. Do not
                // fabricate Quill/TCD-backed table semantics in that geometry-only path.
                (None, None)
            }
        } else {
            (None, None)
        };

        let direct_image_candidate = raw_type == Some(RAW_TYPE_SHAPE)
            && exact_story_identity.is_none()
            && image_slot.is_some()
            && grouped_sources.is_empty();
        let direct_story_candidate =
            raw_type == Some(RAW_TYPE_SHAPE) && story_frame.is_some() && grouped_sources.is_empty();
        let direct_rotation_properties = shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == OFFICE_ART_PROPERTY_ROTATION)
            .map(|property| (property.op, property.f_bid(), property.f_complex()))
            .collect::<Vec<_>>();
        let direct_image_rotation_properties = if direct_image_candidate {
            direct_rotation_properties.clone()
        } else {
            Vec::new()
        };
        let direct_fsp_flags = shape.fsp.as_ref().map(|fsp| fsp.flags).unwrap_or(0);
        let direct_image_transform = if direct_image_candidate {
            bounded_direct_image_transform(
                &direct_image_rotation_properties,
                direct_fsp_flags,
                bounds,
            )
        } else {
            BoundedDirectImageTransform::Identity
        };
        let direct_story_transform = if direct_story_candidate {
            bounded_direct_story_transform(&direct_rotation_properties, direct_fsp_flags, bounds)
        } else {
            None
        };
        let direct_image_cardinal_rotation_degrees =
            if direct_image_candidate && explicit_image_crop.is_none() {
                bounded_direct_image_cardinal_content_rotation_degrees(
                    &direct_image_rotation_properties,
                    direct_fsp_flags,
                )
            } else {
                None
            };
        let (node_transform, direct_image_rotation_applied) =
            if let Some(transform) = direct_story_transform {
                (transform, false)
            } else {
                match direct_image_transform {
                    BoundedDirectImageTransform::Identity
                    | BoundedDirectImageTransform::Unsupported => (Affine2D::identity(), false),
                    BoundedDirectImageTransform::Applied(transform) => (transform, true),
                }
            };

        let object_key = contents_object_key(seq_num);
        let mut source_refs = vec![source_ref(
            &graph.source,
            &chunk.source,
            Some(object_key.clone()),
            Some("chunk".into()),
            SourceRole::Semantic,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )];
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some(if grouped_sources.is_empty() {
                "SpContainer/ClientAnchor".into()
            } else {
                "SpContainer/ChildAnchor".into()
            }),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
        if direct_image_anchor_recovered_from_contents_extent {
            for (field_id, path) in [
                (FIELD_SHAPE_WIDTH, "Contents/0x01/shape-width"),
                (FIELD_SHAPE_HEIGHT, "Contents/0x01/shape-height"),
            ] {
                if let Some((_, value_source)) = unique_u32_field(&chunk, field_id)? {
                    source_refs.push(source_ref(
                        &graph.source,
                        &value_source,
                        Some(object_key.clone()),
                        Some(path.into()),
                        SourceRole::Projection,
                        AuthorityClass::Authoritative,
                        ReadConfidence::Exact,
                    ));
                }
            }
        }
        if has_default_roundrect_geometry(shape) {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FSP/default-roundrect".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
        if has_default_ellipse_geometry(shape) {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FSP/default-ellipse".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
        if has_default_line_geometry(shape) {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FSP/default-line".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
            if has_shape_local_dash_gel(shape) {
                source_refs.push(source_ref(
                    &graph.source,
                    &shape.source,
                    Some(format!("escher/client-data-shape-id/{seq_num}")),
                    Some("SpContainer/FOPT/line-dashing-dash-gel".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }
        if direct_image_rotation_applied || direct_image_cardinal_rotation_degrees.is_some() {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FOPT/rotation".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
        if has_explicit_officeart_paint_observation(shape) {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FOPT".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
        if paint_context_uses_officeart_scheme_color(shape, dgg_defaults) {
            if let Some(color_scheme) = &color_scheme {
                source_refs.push(source_ref(
                    &graph.source,
                    &color_scheme.scheme.source,
                    Some(contents_object_key(color_scheme.seq_num)),
                    Some("OplSccm/current-color-scheme".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }
        if effective_paint
            .as_ref()
            .is_some_and(effective_paint_has_dgg_authority)
        {
            if let Some(dgg_defaults) = dgg_defaults {
                source_refs.push(source_ref(
                    &graph.source,
                    &dgg_defaults.source,
                    Some("escher/dgg/default-options".into()),
                    Some("DggContainer/FOPT-defaults".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }
        for (depth, span) in grouped_sources.iter().enumerate() {
            source_refs.push(source_ref(
                &graph.source,
                span,
                Some(format!("escher/group-ancestor/{seq_num}/{depth}")),
                Some("SpgrContainer/SpContainer".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
        if let Some(text_frame_inset) = &text_frame_inset {
            source_refs.extend(text_frame_inset.source_refs.clone());
        }
        if let Some(vertical_alignment) = story_frame
            .as_ref()
            .and_then(|frame| frame.vertical_alignment.as_ref())
        {
            source_refs.extend(vertical_alignment.source_refs.clone());
        }
        if let Some(table) = &table {
            source_refs.extend(table.source_refs.clone());
        } else if let Some(table_story) = &table_story {
            source_refs.extend(table_story.source_refs.clone());
        }

        graph.nodes.insert(
            node_id,
            Node {
                // Grouped Story/image shapes and TABLEs are projected to page-relative
                // geometry while exact group ancestry remains in provenance.
                // The current resolver does not yet compose Group transforms.
                kind: if raw_type == Some(RAW_TYPE_TABLE) {
                    NodeKind::Table
                } else {
                    NodeKind::Shape
                },
                header: NodeHeader {
                    id: node_id,
                    parent_id: page_id.into_canonical(),
                    bounds,
                    transform: node_transform,
                    source_refs,
                    extensions: Vec::new(),
                },
                payload: PubNodePayload {
                    contents_seq_num: seq_num,
                    officeart_shape_type: shape.fsp.as_ref().map(|fsp| fsp.shape_type),
                    officeart_spid: shape.fsp.as_ref().map(|fsp| fsp.spid),
                    image_slot,
                    legacy_ole: None,
                    explicit_image_crop,
                    explicit_image_cardinal_rotation_degrees:
                        direct_image_cardinal_rotation_degrees,
                    explicit_image_recolor,
                    explicit_paint,
                    effective_paint,
                    story_frame,
                    text_frame_inset,
                    table_story,
                    table,
                },
            },
        );
    }

    add_missing_link_target_diagnostics(&graph, &mut diagnostics);

    let source_page_paint_orders = source_page_paint_orders_v1(
        source_hash,
        &graph,
        &references,
        &page_seq_to_id,
        &escher_inventory,
    );

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

fn utf16_range_to_scalar_range(text: &str, start_utf16: u32, end_utf16: u32) -> Option<(u32, u32)> {
    if start_utf16 > end_utf16 {
        return None;
    }

    fn boundary(text: &str, target_utf16: u32) -> Option<u32> {
        if target_utf16 == 0 {
            return Some(0);
        }

        let mut utf16_cursor = 0_u32;
        let mut scalar_cursor = 0_u32;
        for scalar in text.chars() {
            utf16_cursor = utf16_cursor.checked_add(scalar.len_utf16() as u32)?;
            scalar_cursor = scalar_cursor.checked_add(1)?;
            if utf16_cursor == target_utf16 {
                return Some(scalar_cursor);
            }
            if utf16_cursor > target_utf16 {
                return None;
            }
        }
        (utf16_cursor == target_utf16).then_some(scalar_cursor)
    }

    Some((boundary(text, start_utf16)?, boundary(text, end_utf16)?))
}

fn build_reference_index(
    contents: &[u8],
    directory: &pub_contents::Contents0x2cDirectory,
) -> Result<BTreeMap<u32, Contents0x2cChunkReference>> {
    let mut references = BTreeMap::new();

    for seq_num in 0..directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(contents, directory, seq_num)
            .with_context(|| format!("parse Contents directory reference seq {seq_num}"))?
        else {
            continue;
        };
        let key = seq_u32(reference.seq_num)?;
        if references.insert(key, reference).is_some() {
            bail!("duplicate Contents directory seq {key}");
        }
    }

    Ok(references)
}

#[derive(Debug, Clone)]
struct PubPublicationColorScheme {
    seq_num: u32,
    scheme: MatureColorScheme,
}

fn current_publication_color_scheme(
    stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Result<Option<PubPublicationColorScheme>> {
    let matches = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_COLOR_SCHEME))
        .collect::<Vec<_>>();

    let reference = match matches.as_slice() {
        [] => return Ok(None),
        [reference] => *reference,
        many => bail!(
            "multiple OplSccm/current ColorScheme raw type 0x{RAW_TYPE_COLOR_SCHEME:02X} objects: {}",
            many.len()
        ),
    };

    let seq_num = seq_u32(reference.seq_num)?;
    let chunk = chunk_for_reference(stream, contents, reference)?;
    let scheme = parse_confirmed_mature_color_scheme(contents, &chunk)
        .with_context(|| format!("parse OplSccm current ColorScheme seq {seq_num}"))?;
    Ok(Some(PubPublicationColorScheme { seq_num, scheme }))
}

fn consensus_publication_page_extent(
    stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Result<(u32, u32, usize)> {
    let margins = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_MARGINS))
        .collect::<Vec<_>>();

    if margins.is_empty() {
        bail!("missing Margins/OplMg raw type 0x{RAW_TYPE_MARGINS:02X}");
    }

    let mut dimensions = Vec::with_capacity(margins.len());
    for reference in margins {
        let chunk = chunk_for_reference(stream.clone(), contents, reference)?;
        let extent = parse_confirmed_margins_page_extent(contents, &chunk).with_context(|| {
            format!("parse Margins/OplMg page extent seq {}", reference.seq_num)
        })?;
        dimensions.push((extent.width_emu, extent.height_emu));
    }

    let (width_emu, height_emu) = require_consensus_page_extent(&dimensions)?;
    Ok((width_emu, height_emu, dimensions.len()))
}

fn require_consensus_page_extent(extents: &[(u32, u32)]) -> Result<(u32, u32)> {
    let first = extents
        .first()
        .copied()
        .context("publication has no confirmed Margins/OplMg page extent")?;
    if first.0 == 0 || first.1 == 0 {
        bail!("publication page extent must be positive");
    }

    for &(width_emu, height_emu) in &extents[1..] {
        if width_emu == 0 || height_emu == 0 {
            bail!("publication page extent must be positive");
        }
        if (width_emu, height_emu) != first {
            bail!(
                "conflicting Margins/OplMg page extents: expected {}x{} EMU, found {}x{} EMU",
                first.0,
                first.1,
                width_emu,
                height_emu
            );
        }
    }

    Ok(first)
}

fn unique_reference_by_raw_type<'a>(
    references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
    raw_type: u16,
    label: &str,
) -> Result<&'a Contents0x2cChunkReference> {
    let mut matches = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(raw_type));
    let first = matches
        .next()
        .with_context(|| format!("missing {label} raw type 0x{raw_type:02X}"))?;
    if matches.next().is_some() {
        bail!("multiple {label} raw type 0x{raw_type:02X} objects");
    }
    Ok(first)
}

fn chunk_for_reference(
    stream: StreamPath,
    contents: &[u8],
    reference: &Contents0x2cChunkReference,
) -> Result<Contents0x2cChunk> {
    if reference.chunk_offsets.len() != 1 {
        bail!(
            "Contents seq {} has {} chunk offsets, expected exactly one",
            reference.seq_num,
            reference.chunk_offsets.len()
        );
    }

    parse_confirmed_0x2c_chunk(stream, contents, reference.chunk_offsets[0].value)
        .with_context(|| format!("parse Contents chunk seq {}", reference.seq_num))
}

fn unique_block(chunk: &Contents0x2cChunk, id: u16) -> Result<&RawContentsBlock> {
    let mut matches = chunk.fields.iter().filter(|field| field.id == id);
    let first = matches
        .next()
        .with_context(|| format!("missing Contents field 0x{id:02X}"))?;
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{id:02X}");
    }
    Ok(first)
}

fn single_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn single_parent_seq(reference: &Contents0x2cChunkReference) -> Option<u32> {
    match reference.parent_seq_nums.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn seq_u32(seq_num: usize) -> Result<u32> {
    u32::try_from(seq_num).map_err(|_| anyhow!("Contents seqNum does not fit u32: {seq_num}"))
}

fn derive_pub_id(
    source_hash: &Sha256Digest,
    object_key: &str,
    semantic_role: &str,
) -> Result<CanonicalId> {
    derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: PUB_ADAPTER_ID,
        source_object_key: object_key,
        semantic_role,
    })
    .map_err(|error| anyhow!("source-derived identity error: {error:?}"))
}

fn decode_utf16le_strict(bytes: &[u8]) -> Result<String> {
    if bytes.len() % 2 != 0 {
        bail!("UTF-16LE byte length is odd: {}", bytes.len());
    }
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|error| anyhow!("invalid UTF-16LE: {error}"))
}

fn source_ref(
    source: &SourceDescriptor,
    span: &RawSpan,
    object_key: Option<String>,
    path: Option<String>,
    role: SourceRole,
    authority: AuthorityClass,
    confidence: ReadConfidence,
) -> SourceRef {
    SourceRef {
        format: source.format.clone(),
        adapter_version: source.adapter_version.clone(),
        source_hash: source.source_hash,
        carrier: span.stream.0.clone(),
        object_key,
        path,
        byte_range: Some(ByteRange::new(span.offset, span.len)),
        role,
        authority,
        confidence: Some(confidence),
    }
}

fn bounded_quill_text_rgb(
    direct_rgb: Option<[u8; 3]>,
    scheme_slot: Option<u8>,
    color_scheme: Option<&MatureColorScheme>,
) -> Option<[u8; 3]> {
    match (direct_rgb, scheme_slot) {
        (Some(rgb), None) => Some(rgb),
        (None, Some(slot)) => color_scheme?.slots.get(usize::from(slot))?.rgb,
        // Both carriers at once are not a grounded Quill state; neither is
        // absence of both. Keep those cases fail-closed.
        _ => None,
    }
}

fn exact_image_slot(
    shape: &pub_escher::SpContainerObservation,
    seq_num: u32,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Option<u32> {
    let mut slots = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            property.property_id() == OFFICE_ART_PROPERTY_PIB && property.op_is_blip_id()
        })
        .map(|property| property.op)
        .collect::<BTreeSet<_>>();

    match slots.len() {
        0 => None,
        1 => slots.pop_first(),
        _ => {
            diagnostics.push(PubBridgeDiagnostic::AmbiguousImageSlot {
                seq_num,
                slots: slots.into_iter().collect(),
            });
            None
        }
    }
}

fn page_relative_bounds_from_contents_missing_xe(
    page: &Page,
    anchor: &PublisherFieldRecord,
    contents_width: u32,
    contents_height: u32,
) -> Option<RectEmu> {
    if anchor
        .fields
        .iter()
        .filter(|field| field.id == PUBLISHER_FIELD_XE)
        .count()
        != 0
    {
        return None;
    }

    let xs = signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE)?;
    page_relative_bounds_from_contents_missing_xe_values(
        page,
        xs,
        ys,
        ye,
        contents_width,
        contents_height,
    )
}

fn page_relative_bounds_from_contents_missing_xe_values(
    page: &Page,
    xs: i64,
    ys: i64,
    ye: i64,
    contents_width: u32,
    contents_height: u32,
) -> Option<RectEmu> {
    let width = i64::from(contents_width);
    let height = i64::from(contents_height);
    if width <= 0 || height <= 0 || ye.checked_sub(ys)? != height {
        return None;
    }

    xs.checked_add(width)?;
    let x = page.size.width.get().checked_div(2)?.checked_add(xs)?;
    let y = page.size.height.get().checked_div(2)?.checked_add(ys)?;

    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

fn page_relative_bounds(page: &Page, anchor: &PublisherFieldRecord) -> Option<RectEmu> {
    let xs = signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let xe = signed_field(anchor, PUBLISHER_FIELD_XE)?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE)?;

    let width = xe.checked_sub(xs)?;
    let height = ye.checked_sub(ys)?;
    if width <= 0 || height <= 0 {
        return None;
    }

    let x = page.size.width.get().checked_div(2)?.checked_add(xs)?;
    let y = page.size.height.get().checked_div(2)?.checked_add(ys)?;

    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

fn signed_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let field = unique_escher_field(record, id)?;
    Some(i64::from(i32::from_le_bytes(field.value.to_le_bytes())))
}

fn unique_escher_field(record: &PublisherFieldRecord, id: u16) -> Option<&PublisherField> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn anchor_has_unique_geometry_fields(anchor: &PublisherFieldRecord) -> bool {
    [
        PUBLISHER_FIELD_XS,
        PUBLISHER_FIELD_YS,
        PUBLISHER_FIELD_XE,
        PUBLISHER_FIELD_YE,
    ]
    .into_iter()
    .all(|id| unique_escher_field(anchor, id).is_some())
}

fn record_missing_anchor_fields(
    anchor: Option<&PublisherFieldRecord>,
    counts: &mut BTreeMap<String, usize>,
) {
    for (id, label) in [
        (PUBLISHER_FIELD_XS, "xs"),
        (PUBLISHER_FIELD_YS, "ys"),
        (PUBLISHER_FIELD_XE, "xe"),
        (PUBLISHER_FIELD_YE, "ye"),
    ] {
        if anchor
            .and_then(|record| unique_escher_field(record, id))
            .is_none()
        {
            *counts.entry(label.to_owned()).or_insert(0) += 1;
        }
    }
}

fn build_story_frame(
    source_hash: Sha256Digest,
    seq_num: u32,
    chunk: &Contents0x2cChunk,
    story_by_syid: &BTreeMap<u32, StoryId>,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<Option<PubStoryFrameSource>> {
    let Some((text_id, _)) = unique_u32_field(chunk, FIELD_STORY_ID)? else {
        return Ok(None);
    };

    let story_id = story_by_syid.get(&text_id).copied();
    if story_id.is_none() {
        diagnostics.push(PubBridgeDiagnostic::MissingQuillStory { seq_num, text_id });
    }

    let explicit_ordinal = unique_u32_field(chunk, FIELD_FRAME_ORDINAL)?.map(|(value, _)| value);
    let previous_seq = unique_u32_field(chunk, FIELD_PREVIOUS_FRAME)?.map(|(value, _)| value);
    let next_seq = unique_u32_field(chunk, FIELD_NEXT_FRAME)?.map(|(value, _)| value);

    let previous_frame = previous_seq
        .map(|target| derive_pub_node_id(&source_hash, target))
        .transpose()?;
    let next_frame = next_seq
        .map(|target| derive_pub_node_id(&source_hash, target))
        .transpose()?;

    Ok(Some(PubStoryFrameSource {
        text_id,
        story_id,
        explicit_ordinal,
        previous_seq_num: previous_seq,
        previous_frame,
        next_seq_num: next_seq,
        next_frame,
        vertical_alignment: None,
    }))
}

fn unique_story_id_scalar(chunk: &Contents0x2cChunk) -> Result<Option<u32>> {
    let mut matches = chunk
        .fields
        .iter()
        .filter(|field| field.id == FIELD_STORY_ID);
    let Some(field) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{FIELD_STORY_ID:02X} in one chunk");
    }

    match &field.body {
        RawContentsBlockBody::U16 { value, .. } => Ok(Some(u32::from(*value))),
        RawContentsBlockBody::U32 { value, .. } => Ok(Some(*value)),
        _ => bail!(
            "Contents Story field 0x{FIELD_STORY_ID:02X} at {} is not a confirmed u16/u32 scalar body",
            field.source.offset
        ),
    }
}

fn unique_u32_field(chunk: &Contents0x2cChunk, id: u16) -> Result<Option<(u32, RawSpan)>> {
    let mut matches = chunk.fields.iter().filter(|field| field.id == id);
    let Some(field) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{id:02X} in one chunk");
    }

    match &field.body {
        RawContentsBlockBody::U32 {
            value,
            value_source,
        } => Ok(Some((*value, value_source.clone()))),
        _ => bail!(
            "Contents field 0x{id:02X} at {} is not a confirmed u32/reference body",
            field.source.offset
        ),
    }
}

fn add_missing_link_target_diagnostics(
    graph: &PubSourceGraph,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) {
    let node_ids = graph.nodes.keys().copied().collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        let Some(frame) = node.payload.story_frame.as_ref() else {
            continue;
        };
        for (target_seq_num, target) in [
            (frame.previous_seq_num, frame.previous_frame),
            (frame.next_seq_num, frame.next_frame),
        ] {
            let (Some(target_seq_num), Some(target)) = (target_seq_num, target) else {
                continue;
            };
            if node_ids.contains(&target) {
                continue;
            }

            diagnostics.push(PubBridgeDiagnostic::LinkedFrameNotMaterialized {
                seq_num: node.payload.contents_seq_num,
                target_seq_num,
            });
        }
    }
}

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
        let bounds = page_relative_bounds_from_contents_missing_xe_values(
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
            page_relative_bounds_from_contents_missing_xe_values(
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
