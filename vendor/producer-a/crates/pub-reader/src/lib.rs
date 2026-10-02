//! Bounded Microsoft Publisher 2002+ source adapter.
//!
//! This crate is the first format-aware layer above the raw CFB/Contents/Quill/
//! Escher readers. It projects only evidence-backed mature-0x2C semantics into
//! pub-model::SourceGraph and emits diagnostics instead of inventing missing
//! geometry, page roles, or relation state.

use anyhow::{Context, Result, anyhow, bail};

mod asset_export;
mod assets;
#[cfg(feature = "cmo-authority-bridge")]
mod cmo_bridge;
mod failure_envelope;
mod failure_intake;
mod family_classifier;
mod guide_bridge;
mod intake_protocol;
mod legacy22_graph;
mod legacy22_noquill_graph;
mod mature_wmf;
mod object_tracking_observer;
mod ole_presentation;
mod resolve;
mod salvage;
mod structural_base;
mod table_bridge;
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
pub use mature_wmf::{
    MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1, PubMatureOfficeArtWmfPreviewBundle,
    PubMatureOfficeArtWmfPreviewSource, build_mature_0x2c_wmf_preview_bundle_from_bytes,
};
pub use object_tracking_observer::{
    PUB_OBJECT_TRACKING_WRAP_OBSERVER_SCHEMA_V1, PubObjectTrackingWrapObserver, PubTrackingScalar,
    PubTrackingWrapObservation, observe_object_tracking_wrap_state,
};
pub use ole_presentation::{
    LegacyOleCachedPresentation, LegacyOleCachedPresentationDiagnostic,
    LegacyOleCachedPresentationScan, LegacyOleCachedPresentationSelection, OlePresentation,
    parse_cf_metafilepict_ole_presentation, read_legacy_ole_cached_presentations,
    scan_legacy_ole_cached_presentations, select_unambiguous_legacy_ole_cached_presentation,
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
    OFFICE_ART_PROPERTY_PIB, PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS,
    PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS, PublisherField, PublisherFieldRecord,
    SpContainerInventory, inspect_dgg_default_options, inspect_sp_containers,
};
use pub_model::{
    Affine2D, AuthorityClass, ByteRange, CanonicalId, Decimal, Document, DocumentId, LengthEmu,
    Node, NodeHeader, NodeId, NodeKind, Page, PageId, ReadConfidence, RectEmu, Sha256Digest,
    Size2D, SourceDerivedIdInput, SourceDescriptor, SourceGraph, SourceRef, SourceRole, Story,
    StoryId, derive_source_canonical_id,
};
use pub_quill::{
    QuillGroundedStoryIdentity, QuillMcldReadError, QuillMcldVerticalAlignment,
    QuillParagraphAlignment, QuillScriptFontEntryDisposition, QuillStoryReadError,
    QuillTypographyValueSource, bounded_mcld_text_frame_vertical_alignment,
    bounded_mcld_uniform_text_inset, parse_bounded_fdpp_exact_story_catalog, parse_bounded_mcld,
    parse_bounded_typography, parse_confirmed_story_catalog,
};
pub use resolve::{
    PUB_RESOLVER_VERSION_V1, PubResolveDiagnostic, PubResolvedGraph, PubResolvedGraphBuild,
    PubResolvedNodePayload, PubResolvedStoryFrame, resolve_pub_source_graph,
};
pub use salvage::{
    READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1, READER_SALVAGE_PROBE_SCHEMA_V1, ReaderPartialSourceFact,
    ReaderPartialSourceGap, ReaderPartialSourceGraph, ReaderPartialSourceGraphError,
    ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility, ReaderSalvageProbe,
    ReaderSalvageStreamState, ReaderSalvageSubsystemProbe, ReaderSalvageTrigger,
    build_reader_partial_source_graph, probe_reader_salvage_candidate,
    probe_reader_salvage_candidate_with_trigger,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom};
pub use structural_base::{
    PUB_STRUCTURAL_BASE_SCHEMA_V1, PubStructuralBaseCandidate, PubStructuralBaseManifest,
    PubStructuralBaseStreamDigest, build_mature_0x2c_structural_base_manifest,
    structural_base_manifest_json,
};
pub use table_bridge::{
    PubMaterializedTableCell, PubTableCellCoordinates, PubTableCellPaintSource, PubTableCellSource,
    PubTableLayoutMetricsSource, PubTableSource, PubTableStoryOwnershipSource, PubTableTextError,
    RAW_TYPE_TABLE, materialize_bounded_simple_table_cells, materialize_bounded_table_cells,
};
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
const FIELD_PREVIOUS_FRAME: u16 = 0x36;
const FIELD_NEXT_FRAME: u16 = 0x37;

const ROLE_DOCUMENT: &str = "cdm.document";
const ROLE_PAGE: &str = "cdm.page";
const ROLE_NODE: &str = "cdm.node";
const ROLE_STORY: &str = "cdm.story";

pub type PubSourceGraph = SourceGraph<PubNodePayload, (), (), (), String>;

pub const PUB_PAGE_ROLE_OBSERVATION_SCHEMA_V1: &str = "chaptera.pub-page-role-observation.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubPageRoleObservationReceipt {
    pub schema: String,
    pub document_page_list_entry_count: usize,
    pub confirmed_page_count: usize,
    pub special_entry_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_entries: Vec<PubDocumentPageListEntryObservation>,
    pub pages: Vec<PubPageRoleObservation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controlling: Vec<PubControllingObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubDocumentPageListEntryObservation {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubPageRoleObservation {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid_dword0: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid_dword1: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_raw_type: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pgt_type: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_document_entry_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_document_entry_raw_type: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_document_entry_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_document_entry_raw_type: Option<u16>,
    pub child_raw_type_counts: BTreeMap<u16, usize>,
    pub shape_child_count: usize,
    pub group_child_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubControllingObservation {
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_seq_num: Option<u32>,
    pub fully_decoded: bool,
    pub fields: Vec<PubControllingFieldObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubControllingFieldObservation {
    pub id: u16,
    pub block_type: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_container_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pgids: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubEffectivePageProjectionAuthority {
    RawDocumentPageList,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubEffectivePageProjection {
    pub authority: PubEffectivePageProjectionAuthority,
    pub page_ids: Vec<PageId>,
    pub raw_page_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_scenario_page_ids: Vec<PageId>,
    pub scenario_evidence_list_count: usize,
}

pub const PUB_SOURCE_PAGE_PAINT_ORDER_SCHEMA_V1: &str = "chaptera.pub-source-page-paint-order.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubSourcePagePaintOrderV1 {
    pub schema_version: String,
    pub page_id: PageId,
    /// Canonical Node identities in persisted OfficeArt back-to-front order.
    pub node_ids: Vec<NodeId>,
}

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
    pub script_font_maps: Vec<PubScriptFontMap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubScriptFontEntryDisposition {
    Resolved,
    UnresolvedSentinel,
    InvalidFontOrdinal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScriptFontEntry {
    pub script_slot: u16,
    pub source_font_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_name: Option<String>,
    pub disposition: PubScriptFontEntryDisposition,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScriptFontMap {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub entries: Vec<PubScriptFontEntry>,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubParagraphAlignment {
    Center,
    Right,
    InterWord,
    Distribute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubParagraphAlignmentRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub alignment: PubParagraphAlignment,
    pub source_value: u16,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTypographyRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub source_font_index: u32,
    pub source_font_name: String,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTypographySizeRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub text_size_emu: u32,
    pub size_inherited: bool,
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
    pub uniform_emu: u32,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubStoryFrameCorrelation {
    pub quill_syid_count: usize,
    pub shape_chunk_count: usize,
    pub shape_chunks_with_opaque_tail: usize,
    pub decoded_field_27_syid_matches: usize,
    pub raw_scalar_syid_matches: usize,
    pub raw_field_27_syid_matches: usize,
    pub raw_field_27_syid_matches_in_opaque_tail: usize,
    pub shape_chunks_with_one_field_27_syid_match: usize,
    pub shape_chunks_with_multiple_field_27_syid_matches: usize,
    pub distinct_field_27_matched_syids: usize,
    pub shape_chunks_with_field_27_match_on_document_pages: usize,
    pub shape_chunks_with_field_27_match_outside_document_pages: usize,
    pub distinct_field_27_matched_syids_on_document_pages: usize,
    pub distinct_field_27_matched_syids_outside_document_pages: usize,
    pub table_chunk_count: usize,
    pub table_chunks_with_field_27_syid_match: usize,
    pub distinct_table_field_27_matched_syids: usize,
    pub table_chunks_with_field_27_match_on_document_pages: usize,
    pub table_chunks_with_field_27_match_outside_document_pages: usize,
    pub distinct_table_field_27_matched_syids_on_document_pages: usize,
    pub distinct_table_field_27_matched_syids_outside_document_pages: usize,
    pub distinct_story_syids_with_shape_or_table_identity: usize,
    pub distinct_story_syids_with_both_shape_and_table_identity: usize,
    pub distinct_story_syids_with_shape_only_identity: usize,
    pub distinct_story_syids_with_table_only_identity: usize,
    pub distinct_story_syids_without_shape_or_table_identity: usize,
    pub field_wire_match_counts: BTreeMap<String, usize>,
    pub field_27_parent_class_counts: BTreeMap<String, usize>,
    pub table_field_27_parent_class_counts: BTreeMap<String, usize>,
    pub multiframe_story_syid_count: usize,
    pub multiframe_shape_count: usize,
    pub multiframe_peer_seq_reference_counts: BTreeMap<String, usize>,
    pub multiframe_peer_seq_reference_story_counts: BTreeMap<String, usize>,
    pub multiframe_zero_based_ordinal_candidate_story_counts: BTreeMap<String, usize>,
    pub multiframe_one_based_ordinal_candidate_story_counts: BTreeMap<String, usize>,
    pub multiframe_unique_scalar_candidate_story_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubGroupedStoryGeometryCorrelation {
    pub grouped_story_shape_count: usize,
    pub grouped_story_distinct_syid_count: usize,
    pub unique_child_escher_shape_count: usize,
    pub child_anchor_count: usize,
    pub parent_group_shape_link_count: usize,
    pub parent_group_contents_identity_match_count: usize,
    pub parent_group_fspgr_count: usize,
    pub parent_group_complete_client_anchor_count: usize,
    pub one_level_page_group_count: usize,
    pub nested_group_count: usize,
    pub complete_one_level_geometry_count: usize,
    pub complete_recursive_geometry_count: usize,
    pub max_group_depth: usize,
    pub group_depth_counts: BTreeMap<String, usize>,
    pub grouped_story_child_rotation_count: usize,
    pub grouped_story_child_flip_h_count: usize,
    pub grouped_story_child_flip_v_count: usize,
    pub grouped_story_group_ancestor_rotation_count: usize,
    pub grouped_story_group_ancestor_flip_h_count: usize,
    pub grouped_story_group_ancestor_flip_v_count: usize,
    pub grouped_story_group_ancestor_nontrivial_transform_count: usize,
    pub direct_page_story_shape_count: usize,
    pub direct_page_story_incomplete_client_anchor_count: usize,
    pub direct_page_story_incomplete_client_anchor_with_child_anchor_count: usize,
    pub direct_page_story_incomplete_client_anchor_with_fspgr_count: usize,
    pub direct_page_story_incomplete_client_anchor_missing_field_counts: BTreeMap<String, usize>,
    pub top_group_incomplete_client_anchor_missing_field_counts: BTreeMap<String, usize>,
    pub grouped_table_story_count: usize,
    pub grouped_table_complete_recursive_geometry_count: usize,
    pub grouped_table_nontrivial_transform_count: usize,
    pub grouped_table_max_group_depth: usize,
    pub grouped_table_incomplete_reason_counts: BTreeMap<String, usize>,
    pub incomplete_reason_counts: BTreeMap<String, usize>,
    pub recursive_incomplete_reason_counts: BTreeMap<String, usize>,
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

/// Emits source-free PAGE-role evidence without classifying customer-visible pages.
///
/// Oid, applied-master state, PgtType and child inventory remain independent
/// axes here. No single axis is promoted to a universal visible-page rule.
pub fn analyze_mature_0x2c_page_roles<R: Read + Seek>(
    mut reader: R,
) -> Result<PubPageRoleObservationReceipt> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse mature-0x2C Contents header for PAGE-role observation")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature-0x2C Contents trailer for PAGE-role observation")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), &contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(&contents, page_list_block)
        .context("parse DOCUMENT PageList for PAGE-role observation")?;

    let document_entries = page_list
        .entries
        .iter()
        .enumerate()
        .map(
            |(document_ordinal, entry)| PubDocumentPageListEntryObservation {
                document_ordinal,
                contents_seq_num: entry.handle,
                raw_type: references.get(&entry.handle).and_then(single_raw_type),
            },
        )
        .collect::<Vec<_>>();

    let mut pages = Vec::new();
    let mut special_entry_count = 0_usize;

    for (document_ordinal, entry) in page_list.entries.iter().enumerate() {
        let Some(reference) = references.get(&entry.handle) else {
            continue;
        };
        match single_raw_type(reference) {
            Some(RAW_TYPE_PAGE_LIST_SPECIAL) => {
                special_entry_count += 1;
                continue;
            }
            Some(RAW_TYPE_PAGE) => {}
            _ => continue,
        }

        let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;
        let mut oid = None;
        let mut applied_master_seq_num = None;
        let mut pgt_type = None;

        for field in &chunk.fields {
            match (field.id, field.block_type) {
                (0x06, BLOCK_TYPE_FIXED_8) => {
                    if oid.is_some() {
                        bail!("PAGE {} repeats OplPd.Oid field0x06", entry.handle);
                    }
                    let parsed = parse_confirmed_oid_identity_payload(field.clone())
                        .context("parse exact OplPd.Oid field0x06")?;
                    oid = Some((parsed.dword0, parsed.dword1));
                }
                (0x0d, BLOCK_TYPE_REFERENCE_U32) => {
                    if applied_master_seq_num.is_some() {
                        bail!("PAGE {} repeats OplPd.OhpdMaster field0x0D", entry.handle);
                    }
                    let RawContentsBlockBody::U32 { value, .. } = &field.body else {
                        bail!("PAGE {} field0x0D has inconsistent body", entry.handle);
                    };
                    applied_master_seq_num = Some(*value);
                }
                (0x10, BLOCK_TYPE_U32) => {
                    if pgt_type.is_some() {
                        bail!("PAGE {} repeats OplPd.PgtType field0x10", entry.handle);
                    }
                    let RawContentsBlockBody::U32 { value, .. } = &field.body else {
                        bail!("PAGE {} field0x10 has inconsistent body", entry.handle);
                    };
                    pgt_type = Some(*value);
                }
                _ => {}
            }
        }

        let mut child_raw_type_counts = BTreeMap::<u16, usize>::new();
        for child in references.values() {
            if single_parent_seq(child) != Some(entry.handle) {
                continue;
            }
            if let Some(raw_type) = single_raw_type(child) {
                *child_raw_type_counts.entry(raw_type).or_insert(0) += 1;
            }
        }

        let applied_master_raw_type = applied_master_seq_num
            .and_then(|seq_num| references.get(&seq_num))
            .and_then(single_raw_type);
        let previous_document_entry = document_ordinal
            .checked_sub(1)
            .and_then(|ordinal| document_entries.get(ordinal));
        let next_document_entry = document_entries.get(document_ordinal + 1);

        pages.push(PubPageRoleObservation {
            document_ordinal,
            contents_seq_num: entry.handle,
            oid_dword0: oid.map(|value| value.0),
            oid_dword1: oid.map(|value| value.1),
            applied_master_seq_num,
            applied_master_raw_type,
            pgt_type,
            previous_document_entry_seq_num: previous_document_entry
                .map(|item| item.contents_seq_num),
            previous_document_entry_raw_type: previous_document_entry
                .and_then(|item| item.raw_type),
            next_document_entry_seq_num: next_document_entry.map(|item| item.contents_seq_num),
            next_document_entry_raw_type: next_document_entry.and_then(|item| item.raw_type),
            shape_child_count: child_raw_type_counts
                .get(&RAW_TYPE_SHAPE)
                .copied()
                .unwrap_or(0),
            group_child_count: child_raw_type_counts
                .get(&RAW_TYPE_GROUP)
                .copied()
                .unwrap_or(0),
            child_raw_type_counts,
        });
    }

    let mut controlling = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_CONTROLLING))
        .map(|reference| {
            let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;
            let fields = chunk
                .fields
                .iter()
                .map(|field| {
                    let (declared_length, observed_container_hex) = match &field.body {
                        RawContentsBlockBody::Container {
                            declared_length,
                            content_source,
                            ..
                        } => {
                            let observed = (field.id == 0x06)
                                .then(|| raw_span_hex(&contents, content_source))
                                .transpose()?;
                            (Some(*declared_length), observed)
                        }
                        _ => (None, None),
                    };
                    let pgids = if field.id == 0x06 {
                        parse_confirmed_controlling_page_list(&contents, field.clone())
                            .context("parse OplControlling PageList/Pgid observation")?
                            .entries
                            .into_iter()
                            .map(|entry| [entry.pgid.dword0, entry.pgid.dword1])
                            .collect()
                    } else {
                        Vec::new()
                    };
                    Ok(PubControllingFieldObservation {
                        id: field.id,
                        block_type: field.block_type,
                        declared_length,
                        observed_container_hex,
                        pgids,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(PubControllingObservation {
                contents_seq_num: seq_u32(reference.seq_num)?,
                parent_seq_num: single_parent_seq(reference),
                fully_decoded: chunk.is_fully_decoded(),
                fields,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    controlling.sort_by_key(|item| item.contents_seq_num);

    Ok(PubPageRoleObservationReceipt {
        schema: PUB_PAGE_ROLE_OBSERVATION_SCHEMA_V1.to_owned(),
        document_page_list_entry_count: page_list.entries.len(),
        confirmed_page_count: pages.len(),
        special_entry_count,
        document_entries,
        pages,
        controlling,
    })
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

/// Research-only correlation scan for the Story ↔ shape ownership bridge.
///
/// This function does not promote any raw candidate to semantics. It searches
/// mature-0x2C SHAPE chunks for known scalar wire forms whose u32 payload equals
/// a Quill SYID, separately records whether the currently expected field 0x27
/// occurs after the bounded chunk parser entered its opaque suffix, and
/// classifies matched SHAPE ownership by DOCUMENT PageList membership.
///
/// The result is suitable for corpus evidence gathering. Semantic readers must
/// continue to rely on explicitly decoded fields until a wire form is proven.
pub fn analyze_mature_0x2c_story_frame_candidates<R: Read + Seek>(
    mut reader: R,
) -> Result<PubStoryFrameCorrelation> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| format!("read {QUILL_STREAM_PATH}"))?;

    analyze_mature_0x2c_story_frame_candidates_from_streams(&contents, &quill)
}

pub fn analyze_mature_0x2c_story_frame_candidates_from_streams(
    contents: &[u8],
    quill: &[u8],
) -> Result<PubStoryFrameCorrelation> {
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), contents)
        .context("parse mature-0x2C Contents header for Story-frame correlation")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature-0x2C Contents trailer for Story-frame correlation")?;
    let references = build_reference_index(contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList for Story-frame correlation")?;
    let document_page_handles = page_list
        .entries
        .iter()
        .filter_map(|entry| {
            references
                .get(&entry.handle)
                .is_some_and(|reference| single_raw_type(reference) == Some(RAW_TYPE_PAGE))
                .then_some(entry.handle)
        })
        .collect::<BTreeSet<_>>();

    let quill_catalog = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), quill)
        .context("parse Quill story catalog for Story-frame correlation")?;
    let syids = quill_catalog
        .stories
        .iter()
        .map(|story| story.syid.0)
        .collect::<BTreeSet<_>>();

    let mut result = PubStoryFrameCorrelation {
        quill_syid_count: syids.len(),
        ..Default::default()
    };
    let mut field_27_matched_syids = BTreeSet::new();
    let mut field_27_matched_syids_on_document_pages = BTreeSet::new();
    let mut field_27_matched_syids_outside_document_pages = BTreeSet::new();
    let mut shape_scalars_by_story =
        BTreeMap::<u32, Vec<(u32, BTreeMap<(u16, u8), Vec<u32>>)>>::new();

    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }

        result.shape_chunk_count += 1;

        let parent_seq = single_parent_seq(reference);
        let parent_is_document_page =
            parent_seq.is_some_and(|seq_num| document_page_handles.contains(&seq_num));
        let parent_class = match parent_seq {
            Some(_) if parent_is_document_page => "document_page/0x43".to_owned(),
            Some(seq_num) => references
                .get(&seq_num)
                .and_then(single_raw_type)
                .map(|raw_type| format!("outside_document_pages/0x{raw_type:02x}"))
                .unwrap_or_else(|| "outside_document_pages/unknown".to_owned()),
            None => "missing_or_ambiguous_parent".to_owned(),
        };

        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        if chunk.unsupported_tail.is_some() {
            result.shape_chunks_with_opaque_tail += 1;
        }

        let decoded_story_id = chunk.fields.iter().find_map(|field| {
            (field.id == FIELD_STORY_ID)
                .then(|| match &field.body {
                    RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
                    RawContentsBlockBody::U32 { value, .. } => Some(*value),
                    _ => None,
                })
                .flatten()
        });
        if let Some(story_id) = decoded_story_id.filter(|value| syids.contains(value)) {
            let seq_num = seq_u32(reference.seq_num)?;
            let mut scalars = BTreeMap::<(u16, u8), Vec<u32>>::new();
            for field in &chunk.fields {
                let value = match &field.body {
                    RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
                    RawContentsBlockBody::U32 { value, .. } => Some(*value),
                    _ => None,
                };
                if let Some(value) = value {
                    scalars
                        .entry((field.id, field.block_type))
                        .or_default()
                        .push(value);
                }
            }
            shape_scalars_by_story
                .entry(story_id)
                .or_default()
                .push((seq_num, scalars));
        }

        for field in &chunk.fields {
            if field.id != FIELD_STORY_ID {
                continue;
            }
            if let RawContentsBlockBody::U32 { value, .. } = &field.body {
                if syids.contains(value) {
                    result.decoded_field_27_syid_matches += 1;
                }
            }
        }

        let start = usize::try_from(chunk.source.offset)
            .context("Story-frame correlation chunk offset does not fit usize")?;
        let len = usize::try_from(chunk.source.len)
            .context("Story-frame correlation chunk length does not fit usize")?;
        let end = start
            .checked_add(len)
            .filter(|end| *end <= contents.len())
            .context("Story-frame correlation chunk range is out of bounds")?;
        let raw = &contents[start..end];

        let tail_range = chunk.unsupported_tail.as_ref().and_then(|tail| {
            let tail_start = usize::try_from(tail.offset).ok()?;
            let tail_len = usize::try_from(tail.len).ok()?;
            let tail_end = tail_start.checked_add(tail_len)?;
            Some((tail_start, tail_end))
        });

        let mut field_27_matches_in_shape = 0_usize;
        if raw.len() >= 6 {
            for relative in 4..=raw.len() - 6 {
                let raw_tag = [raw[relative], raw[relative + 1]];
                let (id, wire) = pub_contents::decode_packed_field_tag(raw_tag);
                if !matches!(
                    wire,
                    pub_contents::BLOCK_TYPE_U32
                        | pub_contents::BLOCK_TYPE_REFERENCE_U32
                        | pub_contents::BLOCK_TYPE_HANDLE_U32
                ) {
                    continue;
                }

                let value = u32::from_le_bytes([
                    raw[relative + 2],
                    raw[relative + 3],
                    raw[relative + 4],
                    raw[relative + 5],
                ]);
                if !syids.contains(&value) {
                    continue;
                }

                result.raw_scalar_syid_matches += 1;
                *result
                    .field_wire_match_counts
                    .entry(format!("0x{id:02x}/0x{wire:02x}"))
                    .or_insert(0) += 1;

                if id != FIELD_STORY_ID {
                    continue;
                }

                result.raw_field_27_syid_matches += 1;
                field_27_matches_in_shape += 1;
                field_27_matched_syids.insert(value);
                if parent_is_document_page {
                    field_27_matched_syids_on_document_pages.insert(value);
                } else {
                    field_27_matched_syids_outside_document_pages.insert(value);
                }

                let absolute = start + relative;
                if tail_range.is_some_and(|(tail_start, tail_end)| {
                    absolute >= tail_start && absolute < tail_end
                }) {
                    result.raw_field_27_syid_matches_in_opaque_tail += 1;
                }
            }
        }

        match field_27_matches_in_shape {
            1 => result.shape_chunks_with_one_field_27_syid_match += 1,
            2.. => result.shape_chunks_with_multiple_field_27_syid_matches += 1,
            _ => {}
        }

        if field_27_matches_in_shape > 0 {
            if parent_is_document_page {
                result.shape_chunks_with_field_27_match_on_document_pages += 1;
            } else {
                result.shape_chunks_with_field_27_match_outside_document_pages += 1;
            }
            *result
                .field_27_parent_class_counts
                .entry(parent_class)
                .or_insert(0) += 1;
        }
    }

    result.distinct_field_27_matched_syids = field_27_matched_syids.len();
    result.distinct_field_27_matched_syids_on_document_pages =
        field_27_matched_syids_on_document_pages.len();
    result.distinct_field_27_matched_syids_outside_document_pages =
        field_27_matched_syids_outside_document_pages.len();

    for frames in shape_scalars_by_story
        .values()
        .filter(|frames| frames.len() > 1)
    {
        result.multiframe_story_syid_count += 1;
        result.multiframe_shape_count += frames.len();

        let frame_seq_nums = frames
            .iter()
            .map(|(seq_num, _)| *seq_num)
            .collect::<BTreeSet<_>>();
        let mut peer_fields_seen = BTreeSet::new();
        let mut all_keys = BTreeSet::new();
        for (_, scalars) in frames {
            all_keys.extend(scalars.keys().copied());
            for (&(id, wire), values) in scalars {
                if id == FIELD_STORY_ID {
                    continue;
                }
                for value in values {
                    if frame_seq_nums.contains(value) {
                        let key = format!("0x{id:02x}/0x{wire:02x}");
                        *result
                            .multiframe_peer_seq_reference_counts
                            .entry(key.clone())
                            .or_insert(0) += 1;
                        peer_fields_seen.insert(key);
                    }
                }
            }
        }
        for key in peer_fields_seen {
            *result
                .multiframe_peer_seq_reference_story_counts
                .entry(key)
                .or_insert(0) += 1;
        }

        let expected_zero =
            (0..u32::try_from(frames.len()).unwrap_or(u32::MAX)).collect::<BTreeSet<_>>();
        let expected_one =
            (1..=u32::try_from(frames.len()).unwrap_or(u32::MAX)).collect::<BTreeSet<_>>();

        for (id, wire) in all_keys {
            if id == FIELD_STORY_ID {
                continue;
            }
            let values = frames
                .iter()
                .filter_map(|(_, scalars)| {
                    let values = scalars.get(&(id, wire))?;
                    (values.len() == 1).then_some(values[0])
                })
                .collect::<Vec<_>>();
            if values.len() != frames.len() {
                continue;
            }

            let distinct = values.iter().copied().collect::<BTreeSet<_>>();
            if distinct.len() == frames.len() {
                let key = format!("0x{id:02x}/0x{wire:02x}");
                *result
                    .multiframe_unique_scalar_candidate_story_counts
                    .entry(key.clone())
                    .or_insert(0) += 1;
                if distinct == expected_zero {
                    *result
                        .multiframe_zero_based_ordinal_candidate_story_counts
                        .entry(key.clone())
                        .or_insert(0) += 1;
                }
                if distinct == expected_one {
                    *result
                        .multiframe_one_based_ordinal_candidate_story_counts
                        .entry(key)
                        .or_insert(0) += 1;
                }
            }
        }
    }

    let mut table_field_27_matched_syids = BTreeSet::new();
    let mut table_field_27_matched_syids_on_document_pages = BTreeSet::new();
    let mut table_field_27_matched_syids_outside_document_pages = BTreeSet::new();
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_TABLE) {
            continue;
        }

        result.table_chunk_count += 1;

        let parent_seq = single_parent_seq(reference);
        let parent_is_document_page =
            parent_seq.is_some_and(|seq_num| document_page_handles.contains(&seq_num));
        let parent_class = match parent_seq {
            Some(_) if parent_is_document_page => "document_page/0x43".to_owned(),
            Some(seq_num) => references
                .get(&seq_num)
                .and_then(single_raw_type)
                .map(|raw_type| format!("outside_document_pages/0x{raw_type:02x}"))
                .unwrap_or_else(|| "outside_document_pages/unknown".to_owned()),
            None => "missing_or_ambiguous_parent".to_owned(),
        };

        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let mut matched = false;
        for field in &chunk.fields {
            if field.id != FIELD_STORY_ID {
                continue;
            }
            let value = match &field.body {
                RawContentsBlockBody::U16 { value, .. } => u32::from(*value),
                RawContentsBlockBody::U32 { value, .. } => *value,
                _ => continue,
            };
            if syids.contains(&value) {
                matched = true;
                table_field_27_matched_syids.insert(value);
                if parent_is_document_page {
                    table_field_27_matched_syids_on_document_pages.insert(value);
                } else {
                    table_field_27_matched_syids_outside_document_pages.insert(value);
                }
            }
        }
        if matched {
            result.table_chunks_with_field_27_syid_match += 1;
            if parent_is_document_page {
                result.table_chunks_with_field_27_match_on_document_pages += 1;
            } else {
                result.table_chunks_with_field_27_match_outside_document_pages += 1;
            }
            *result
                .table_field_27_parent_class_counts
                .entry(parent_class)
                .or_insert(0) += 1;
        }
    }

    result.distinct_table_field_27_matched_syids = table_field_27_matched_syids.len();
    result.distinct_table_field_27_matched_syids_on_document_pages =
        table_field_27_matched_syids_on_document_pages.len();
    result.distinct_table_field_27_matched_syids_outside_document_pages =
        table_field_27_matched_syids_outside_document_pages.len();

    let shape_or_table = field_27_matched_syids
        .union(&table_field_27_matched_syids)
        .copied()
        .collect::<BTreeSet<_>>();
    result.distinct_story_syids_with_shape_or_table_identity = shape_or_table.len();
    result.distinct_story_syids_with_both_shape_and_table_identity = field_27_matched_syids
        .intersection(&table_field_27_matched_syids)
        .count();
    result.distinct_story_syids_with_shape_only_identity = field_27_matched_syids
        .difference(&table_field_27_matched_syids)
        .count();
    result.distinct_story_syids_with_table_only_identity = table_field_27_matched_syids
        .difference(&field_27_matched_syids)
        .count();
    result.distinct_story_syids_without_shape_or_table_identity =
        syids.difference(&shape_or_table).count();

    Ok(result)
}

/// Research-only census for grouped Story geometry prerequisites.
///
/// This does not materialize Group nodes or derive absolute child bounds. It
/// measures whether exact Contents Story ownership can be joined to the
/// OfficeArt group hierarchy and to the raw coordinate records required by
/// the proven FSPGR + ChildAnchor transform model.
pub fn analyze_mature_0x2c_grouped_story_geometry<R: Read + Seek>(
    mut reader: R,
) -> Result<PubGroupedStoryGeometryCorrelation> {
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

    analyze_mature_0x2c_grouped_story_geometry_from_streams(&contents, &quill, &escher)
}

pub fn analyze_mature_0x2c_grouped_story_geometry_from_streams(
    contents: &[u8],
    quill: &[u8],
    escher: &[u8],
) -> Result<PubGroupedStoryGeometryCorrelation> {
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), contents)
        .context("parse mature-0x2C Contents header for grouped Story census")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature-0x2C Contents trailer for grouped Story census")?;
    let references = build_reference_index(contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList for grouped Story census")?;
    let document_page_handles = page_list
        .entries
        .iter()
        .filter_map(|entry| {
            references
                .get(&entry.handle)
                .is_some_and(|reference| single_raw_type(reference) == Some(RAW_TYPE_PAGE))
                .then_some(entry.handle)
        })
        .collect::<BTreeSet<_>>();

    let quill_catalog = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), quill)
        .context("parse Quill story catalog for grouped Story census")?;
    let syids = quill_catalog
        .stories
        .iter()
        .map(|story| story.syid.0)
        .collect::<BTreeSet<_>>();

    let escher_inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), escher)
        .context("parse OfficeArt SpContainers for grouped Story census")?;
    let escher_by_contents_seq = index_escher_by_contents_seq(&escher_inventory);

    let mut result = PubGroupedStoryGeometryCorrelation::default();
    let mut grouped_syids = BTreeSet::new();

    // Measure the direct-page Story shapes that the runtime currently rejects
    // only because their Publisher ClientAnchor is incomplete. This is
    // evidence-only: alternate OfficeArt records are counted, not promoted.
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        if !document_page_handles.contains(&parent_seq) {
            continue;
        }

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.direct_page_story_shape_count += 1;
        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let shape = match matches {
            [index] => &escher_inventory.shapes[*index],
            _ => continue,
        };

        let complete = shape
            .client_anchor
            .as_ref()
            .is_some_and(anchor_has_unique_geometry_fields);
        if complete {
            continue;
        }

        result.direct_page_story_incomplete_client_anchor_count += 1;
        if shape.child_anchor.is_some() {
            result.direct_page_story_incomplete_client_anchor_with_child_anchor_count += 1;
        }
        if shape.fspgr.is_some() {
            result.direct_page_story_incomplete_client_anchor_with_fspgr_count += 1;
        }
        record_missing_anchor_fields(
            shape.client_anchor.as_ref(),
            &mut result.direct_page_story_incomplete_client_anchor_missing_field_counts,
        );
    }

    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }

        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        let Some(parent_reference) = references.get(&parent_seq) else {
            continue;
        };
        if single_raw_type(parent_reference) != Some(RAW_TYPE_GROUP) {
            continue;
        }

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.grouped_story_shape_count += 1;
        grouped_syids.insert(text_id);

        let mut complete = true;
        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let child = match matches {
            [index] => {
                result.unique_child_escher_shape_count += 1;
                &escher_inventory.shapes[*index]
            }
            [] => {
                increment_reason(&mut result, "child_escher_missing");
                continue;
            }
            _ => {
                increment_reason(&mut result, "child_escher_ambiguous");
                continue;
            }
        };

        if child.child_anchor.is_some() {
            result.child_anchor_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "child_anchor_missing");
        }

        let Some(parent_source) = child.parent_group_shape_source.as_ref() else {
            increment_reason(&mut result, "parent_group_shape_link_missing");
            continue;
        };
        result.parent_group_shape_link_count += 1;

        let mut parent_matches = escher_inventory
            .shapes
            .iter()
            .filter(|shape| &shape.source == parent_source);
        let Some(group_shape) = parent_matches.next() else {
            increment_reason(&mut result, "parent_group_shape_missing");
            continue;
        };
        if parent_matches.next().is_some() {
            increment_reason(&mut result, "parent_group_shape_ambiguous");
            continue;
        }

        let group_contents_id_matches = group_shape
            .client_data
            .as_ref()
            .and_then(|record| unique_escher_field(record, PUBLISHER_FIELD_SHAPE_ID))
            .is_some_and(|field| field.value == parent_seq);
        if group_contents_id_matches {
            result.parent_group_contents_identity_match_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_contents_identity_mismatch");
        }

        if group_shape.fspgr.is_some() {
            result.parent_group_fspgr_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_fspgr_missing");
        }

        let complete_group_anchor = group_shape
            .client_anchor
            .as_ref()
            .is_some_and(anchor_has_unique_geometry_fields);
        if complete_group_anchor {
            result.parent_group_complete_client_anchor_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_client_anchor_incomplete");
        }

        let group_parent_seq = single_parent_seq(parent_reference);
        let one_level = group_parent_seq
            .is_some_and(|group_parent| document_page_handles.contains(&group_parent));
        if one_level {
            result.one_level_page_group_count += 1;
        } else {
            complete = false;
            let nested = group_parent_seq
                .and_then(|group_parent| references.get(&group_parent))
                .and_then(single_raw_type)
                == Some(RAW_TYPE_GROUP);
            if nested {
                result.nested_group_count += 1;
                increment_reason(&mut result, "nested_group");
            } else {
                increment_reason(&mut result, "group_parent_not_document_page");
            }
        }

        if complete {
            result.complete_one_level_geometry_count += 1;
        }

        // A nested group does not have a page-level ClientAnchor. Its own
        // ChildAnchor positions it inside the next parent group's FSPGR
        // coordinate space. Walk that exact hierarchy until a DOCUMENT page
        // is reached, preserving the one-level counters above as a separate
        // measurement.
        let child_rotation = shape_has_nonzero_rotation(child);
        let child_flip_h = shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_H);
        let child_flip_v = shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_V);
        result.grouped_story_child_rotation_count += usize::from(child_rotation);
        result.grouped_story_child_flip_h_count += usize::from(child_flip_h);
        result.grouped_story_child_flip_v_count += usize::from(child_flip_v);

        let mut recursive_complete = child.child_anchor.is_some();
        if !recursive_complete {
            increment_recursive_reason(&mut result, "child_anchor_missing");
        }

        let mut has_group_rotation = false;
        let mut has_group_flip_h = false;
        let mut has_group_flip_v = false;
        let mut current_shape = child;
        let mut current_group_seq = parent_seq;
        let mut seen_group_seq = BTreeSet::new();
        let mut group_depth = 0_usize;

        loop {
            if !seen_group_seq.insert(current_group_seq) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_cycle");
                break;
            }
            group_depth += 1;

            let Some(group_reference) = references.get(&current_group_seq) else {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_contents_reference_missing");
                break;
            };
            if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_contents_wrong_raw_type");
                break;
            }

            let group_matches = escher_by_contents_seq
                .get(&current_group_seq)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let group_shape = match group_matches {
                [index] => &escher_inventory.shapes[*index],
                [] => {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "group_escher_missing");
                    break;
                }
                _ => {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "group_escher_ambiguous");
                    break;
                }
            };

            if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_link_mismatch");
            }
            if group_shape.fspgr.is_none() {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_fspgr_missing");
            }

            has_group_rotation |= shape_has_nonzero_rotation(group_shape);
            has_group_flip_h |= shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H);
            has_group_flip_v |= shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V);

            let Some(next_parent_seq) = single_parent_seq(group_reference) else {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_missing_or_ambiguous");
                break;
            };

            if document_page_handles.contains(&next_parent_seq) {
                if !group_shape
                    .client_anchor
                    .as_ref()
                    .is_some_and(anchor_has_unique_geometry_fields)
                {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "top_group_client_anchor_incomplete");
                    record_missing_anchor_fields(
                        group_shape.client_anchor.as_ref(),
                        &mut result.top_group_incomplete_client_anchor_missing_field_counts,
                    );
                }
                break;
            }

            let nested =
                references.get(&next_parent_seq).and_then(single_raw_type) == Some(RAW_TYPE_GROUP);
            if !nested {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_not_document_page_or_group");
                break;
            }

            if group_shape.child_anchor.is_none() {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "nested_group_child_anchor_missing");
            }

            current_shape = group_shape;
            current_group_seq = next_parent_seq;
        }

        result.max_group_depth = result.max_group_depth.max(group_depth);
        *result
            .group_depth_counts
            .entry(group_depth.to_string())
            .or_insert(0) += 1;
        result.grouped_story_group_ancestor_rotation_count += usize::from(has_group_rotation);
        result.grouped_story_group_ancestor_flip_h_count += usize::from(has_group_flip_h);
        result.grouped_story_group_ancestor_flip_v_count += usize::from(has_group_flip_v);
        result.grouped_story_group_ancestor_nontrivial_transform_count +=
            usize::from(has_group_rotation || has_group_flip_h || has_group_flip_v);
        if recursive_complete {
            result.complete_recursive_geometry_count += 1;
        }
    }

    result.grouped_story_distinct_syid_count = grouped_syids.len();

    // TABLE ownership is distinct from ordinary StoryFrame ownership, but a
    // grouped TABLE still needs the same OfficeArt ancestry to obtain runtime
    // geometry. Measure that fence independently without changing semantics.
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_TABLE) {
            continue;
        }
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            continue;
        }

        let table_seq = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.grouped_table_story_count += 1;
        let child_matches = escher_by_contents_seq
            .get(&table_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let child = match child_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry("child_escher_missing".into())
                    .or_insert(0) += 1;
                continue;
            }
            _ => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry("child_escher_ambiguous".into())
                    .or_insert(0) += 1;
                continue;
            }
        };

        match inspect_grouped_geometry_chain(
            child,
            parent_seq,
            &references,
            &document_page_handles,
            &escher_inventory,
            &escher_by_contents_seq,
        ) {
            Ok((depth, has_nontrivial_transform)) => {
                result.grouped_table_max_group_depth =
                    result.grouped_table_max_group_depth.max(depth);
                if has_nontrivial_transform {
                    result.grouped_table_nontrivial_transform_count += 1;
                } else {
                    result.grouped_table_complete_recursive_geometry_count += 1;
                }
            }
            Err(reason) => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry(reason)
                    .or_insert(0) += 1;
            }
        }
    }

    Ok(result)
}

fn inspect_grouped_geometry_chain(
    child: &pub_escher::SpContainerObservation,
    first_group_seq: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    document_page_handles: &BTreeSet<u32>,
    escher_inventory: &SpContainerInventory,
    escher_by_contents_seq: &BTreeMap<u32, Vec<usize>>,
) -> std::result::Result<(usize, bool), String> {
    if child.child_anchor.is_none() {
        return Err("child_anchor_missing".into());
    }

    let mut current_shape = child;
    let mut current_group_seq = first_group_seq;
    let mut seen = BTreeSet::new();
    let mut depth = 0_usize;
    let mut nontrivial_transform = shape_has_nonzero_rotation(child)
        || shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_H)
        || shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_V);

    loop {
        if !seen.insert(current_group_seq) {
            return Err("group_cycle".into());
        }
        depth += 1;

        let group_reference = references
            .get(&current_group_seq)
            .ok_or_else(|| "group_contents_reference_missing".to_owned())?;
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            return Err("group_contents_wrong_raw_type".into());
        }

        let group_matches = escher_by_contents_seq
            .get(&current_group_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_shape = match group_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => return Err("group_escher_missing".into()),
            _ => return Err("group_escher_ambiguous".into()),
        };

        if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            return Err("group_parent_link_mismatch".into());
        }
        if group_shape.fspgr.is_none() {
            return Err("group_fspgr_missing".into());
        }
        nontrivial_transform |= shape_has_nonzero_rotation(group_shape)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V);

        let parent_seq = single_parent_seq(group_reference)
            .ok_or_else(|| "group_parent_missing_or_ambiguous".to_owned())?;
        if document_page_handles.contains(&parent_seq) {
            if !group_shape
                .client_anchor
                .as_ref()
                .is_some_and(anchor_has_unique_geometry_fields)
            {
                return Err("top_group_client_anchor_incomplete".into());
            }
            return Ok((depth, nontrivial_transform));
        }

        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            return Err("group_parent_not_document_page_or_group".into());
        }
        if group_shape.child_anchor.is_none() {
            return Err("nested_group_child_anchor_missing".into());
        }

        current_shape = group_shape;
        current_group_seq = parent_seq;
    }
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

fn increment_reason(result: &mut PubGroupedStoryGeometryCorrelation, reason: &str) {
    *result
        .incomplete_reason_counts
        .entry(reason.to_owned())
        .or_insert(0) += 1;
}

fn increment_recursive_reason(result: &mut PubGroupedStoryGeometryCorrelation, reason: &str) {
    *result
        .recursive_incomplete_reason_counts
        .entry(reason.to_owned())
        .or_insert(0) += 1;
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

    let mut typography_runs = Vec::new();
    let mut typography_size_runs = Vec::new();
    let mut paragraph_alignments = Vec::new();
    let mut script_font_maps = Vec::new();
    if let Some(catalog) = typography_catalog {
        for map in &catalog.script_font_maps {
            let syid = map.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("script-font map references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("script-font map Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                map.story_start_utf16,
                map.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "script-font map range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        map.story_start_utf16, map.story_end_utf16
                    ),
                });
                continue;
            };

            let object_key = quill_story_object_key(syid);
            let entries = map
                .entries
                .iter()
                .map(|entry| PubScriptFontEntry {
                    script_slot: entry.script_slot,
                    source_font_index: entry.font_index,
                    source_font_name: entry.font_name.clone(),
                    disposition: match entry.disposition {
                        QuillScriptFontEntryDisposition::Resolved => {
                            PubScriptFontEntryDisposition::Resolved
                        }
                        QuillScriptFontEntryDisposition::UnresolvedSentinel => {
                            PubScriptFontEntryDisposition::UnresolvedSentinel
                        }
                        QuillScriptFontEntryDisposition::InvalidFontOrdinal => {
                            PubScriptFontEntryDisposition::InvalidFontOrdinal
                        }
                    },
                    source_ref: source_ref(
                        &graph.source,
                        &entry.source,
                        Some(object_key.clone()),
                        Some(format!(
                            "FDPC/ScriptFonts/script-slot/{}",
                            entry.script_slot
                        )),
                        SourceRole::Semantic,
                        AuthorityClass::Authoritative,
                        ReadConfidence::Exact,
                    ),
                })
                .collect::<Vec<_>>();

            script_font_maps.push(PubScriptFontMap {
                story_id,
                story_utf16_start: map.story_start_utf16,
                story_utf16_end: map.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                entries,
                source_ref: source_ref(
                    &graph.source,
                    &map.fdpc_style_source,
                    Some(object_key),
                    Some("FDPC/ScriptFonts".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.paragraph_alignments {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph alignment references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph alignment Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "paragraph alignment range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            paragraph_alignments.push(PubParagraphAlignmentRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                alignment: match run.alignment {
                    QuillParagraphAlignment::Center => PubParagraphAlignment::Center,
                    QuillParagraphAlignment::Right => PubParagraphAlignment::Right,
                    QuillParagraphAlignment::InterWord => PubParagraphAlignment::InterWord,
                    QuillParagraphAlignment::Distribute => PubParagraphAlignment::Distribute,
                },
                source_value: run.source_value,
                source_ref: source_ref(
                    &graph.source,
                    &run.fdpp_style_source,
                    Some(quill_story_object_key(syid)),
                    Some("FDPP/ParagraphAlignment".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.size_only_runs {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("size-only typography references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("size-only typography Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "size-only typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            typography_size_runs.push(PubTypographySizeRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                text_size_emu: run.text_size_emu,
                size_inherited: run.text_size_source == QuillTypographyValueSource::InheritedStsh1,
            });
        }
        if !catalog.effective_runs.is_empty() {
            for run in catalog.effective_runs {
                let syid = run.story_syid.0;
                let Some(story_id) = story_by_syid.get(&syid).copied() else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "effective typography references missing Story SYID {syid}"
                        ),
                    });
                    continue;
                };
                let Some(story) = graph.stories.get(&story_id) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("effective typography Story {story_id:?} is absent"),
                    });
                    continue;
                };
                let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                    &story.text,
                    run.story_start_utf16,
                    run.story_end_utf16,
                ) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "effective typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                            run.story_start_utf16, run.story_end_utf16
                        ),
                    });
                    continue;
                };
                typography_runs.push(PubTypographyRun {
                    story_id,
                    story_utf16_start: run.story_start_utf16,
                    story_utf16_end: run.story_end_utf16,
                    story_scalar_start,
                    story_scalar_end,
                    source_font_index: run.font_index,
                    source_font_name: run.font_name,
                    text_size_emu: run.text_size_emu,
                    font_inherited: run.font_source == QuillTypographyValueSource::InheritedStsh1,
                    size_inherited: run.text_size_source
                        == QuillTypographyValueSource::InheritedStsh1,
                });
            }
        } else {
            for run in catalog.explicit_runs {
                let syid = run.story_syid.0;
                let Some(story_id) = story_by_syid.get(&syid).copied() else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("explicit typography references missing Story SYID {syid}"),
                    });
                    continue;
                };
                let Some(story) = graph.stories.get(&story_id) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("explicit typography Story {story_id:?} is absent"),
                    });
                    continue;
                };
                let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                    &story.text,
                    run.story_start_utf16,
                    run.story_end_utf16,
                ) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "explicit typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                            run.story_start_utf16, run.story_end_utf16
                        ),
                    });
                    continue;
                };
                typography_runs.push(PubTypographyRun {
                    story_id,
                    story_utf16_start: run.story_start_utf16,
                    story_utf16_end: run.story_end_utf16,
                    story_scalar_start,
                    story_scalar_end,
                    source_font_index: run.font_index,
                    source_font_name: run.font_name,
                    text_size_emu: run.text_size_emu,
                    font_inherited: false,
                    size_inherited: false,
                });
            }
        }
    }

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
        let grouped_projection = if direct_page.is_none()
            && references.get(&parent_seq).and_then(single_raw_type) == Some(RAW_TYPE_GROUP)
            && (exact_story_identity.is_some() || exact_grouped_image_identity)
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
                    } else {
                        PubBridgeDiagnostic::GroupedImageProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    });
                    None
                }
            }
        } else {
            None
        };

        let (page_id, bounds, grouped_sources) = if let Some(page_id) = direct_page {
            let Some(anchor) = shape.client_anchor.as_ref() else {
                diagnostics.push(PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num });
                continue;
            };
            let Some(bounds) = page_relative_bounds(
                graph
                    .pages
                    .get(&page_id)
                    .expect("page id came from graph registry"),
                anchor,
            ) else {
                let complete = anchor_has_unique_geometry_fields(anchor);
                diagnostics.push(if complete {
                    PubBridgeDiagnostic::InvalidEscherAnchor { seq_num }
                } else {
                    PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num }
                });
                continue;
            };
            (page_id, bounds, Vec::new())
        } else if let Some(projection) = grouped_projection {
            (
                projection.page_id,
                projection.bounds,
                projection.group_sources,
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
            let inset = bounded_mcld_uniform_text_inset(mcld, *layout_record_id).ok()?;
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
                uniform_emu: inset.inset_emu,
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

        let direct_image_transform = if raw_type == Some(RAW_TYPE_SHAPE)
            && exact_story_identity.is_none()
            && image_slot.is_some()
            && grouped_sources.is_empty()
        {
            let rotation_properties = shape
                .fopts
                .iter()
                .flat_map(|record| record.properties.iter())
                .filter(|property| property.property_id() == OFFICE_ART_PROPERTY_ROTATION)
                .map(|property| (property.op, property.f_bid(), property.f_complex()))
                .collect::<Vec<_>>();
            let fsp_flags = shape.fsp.as_ref().map(|fsp| fsp.flags).unwrap_or(0);
            bounded_direct_image_transform(&rotation_properties, fsp_flags, bounds)
        } else {
            BoundedDirectImageTransform::Identity
        };
        let (node_transform, direct_image_rotation_applied) = match direct_image_transform {
            BoundedDirectImageTransform::Identity | BoundedDirectImageTransform::Unsupported => {
                (Affine2D::identity(), false)
            }
            BoundedDirectImageTransform::Applied(transform) => (transform, true),
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
        if direct_image_rotation_applied {
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
        script_font_maps,
    })
}

fn derive_effective_page_projection(
    contents_stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    raw_page_ids: &[PageId],
) -> (PubEffectivePageProjection, Vec<PubBridgeDiagnostic>) {
    let mut diagnostics = vec![PubBridgeDiagnostic::PageRoleClassificationUnresolved {
        raw_page_count: raw_page_ids.len(),
    }];

    let mut pgid_lists = Vec::<Vec<(u32, u32)>>::new();
    let mut observed_page_list = false;
    for reference in references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_CONTROLLING))
    {
        let chunk = match chunk_for_reference(contents_stream.clone(), contents, reference) {
            Ok(chunk) => chunk,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason: format!("controlling_chunk_unavailable:{error}"),
                    raw_page_count: raw_page_ids.len(),
                });
                return (
                    build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                    diagnostics,
                );
            }
        };
        let page_list_fields = chunk
            .fields
            .iter()
            .filter(|field| field.id == pub_contents::CONTROLLING_PAGE_LIST_ID)
            .cloned()
            .collect::<Vec<_>>();
        if page_list_fields.len() > 1 {
            diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                reason: format!("duplicate_controlling_page_list:seq={}", reference.seq_num),
                raw_page_count: raw_page_ids.len(),
            });
            return (
                build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                diagnostics,
            );
        }
        let Some(field) = page_list_fields.into_iter().next() else {
            continue;
        };
        observed_page_list = true;
        let parsed = match parse_confirmed_controlling_page_list(contents, field) {
            Ok(parsed) => parsed,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason: format!(
                        "controlling_page_list_invalid:seq={}:{}",
                        reference.seq_num, error
                    ),
                    raw_page_count: raw_page_ids.len(),
                });
                return (
                    build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                    diagnostics,
                );
            }
        };
        let pgids = parsed
            .entries
            .into_iter()
            .map(|entry| (entry.pgid.dword0, entry.pgid.dword1))
            .collect::<Vec<_>>();
        if pgids.is_empty() {
            diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                reason: format!("controlling_page_list_empty:seq={}", reference.seq_num),
                raw_page_count: raw_page_ids.len(),
            });
            return (
                build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                diagnostics,
            );
        }
        pgid_lists.push(pgids);
    }

    let mut pages_by_oid = BTreeMap::<(u32, u32), Vec<PageId>>::new();
    for (seq_num, page_id) in page_seq_to_id {
        let Some(reference) = references.get(seq_num) else {
            continue;
        };
        let chunk = match chunk_for_reference(contents_stream.clone(), contents, reference) {
            Ok(chunk) => chunk,
            Err(_) => continue,
        };
        let oid_fields = chunk
            .fields
            .iter()
            .filter(|field| field.id == 0x06 && field.block_type == BLOCK_TYPE_FIXED_8)
            .collect::<Vec<_>>();
        if oid_fields.len() != 1 {
            continue;
        }
        let Ok(oid) = parse_confirmed_oid_identity_payload(oid_fields[0].clone()) else {
            continue;
        };
        pages_by_oid
            .entry((oid.dword0, oid.dword1))
            .or_default()
            .push(*page_id);
    }

    let observed_scenario_page_ids =
        match resolve_scenario_page_ids_from_evidence(&pgid_lists, &pages_by_oid) {
            Ok(projected) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderObserved {
                    raw_page_count: raw_page_ids.len(),
                    scenario_page_count: projected.len(),
                    evidence_list_count: pgid_lists.len(),
                });
                projected
            }
            Err(reason) if observed_page_list => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason,
                    raw_page_count: raw_page_ids.len(),
                });
                Vec::new()
            }
            Err(_) => Vec::new(),
        };

    (
        build_effective_page_projection(raw_page_ids, observed_scenario_page_ids, pgid_lists.len()),
        diagnostics,
    )
}

fn build_effective_page_projection(
    raw_page_ids: &[PageId],
    observed_scenario_page_ids: Vec<PageId>,
    scenario_evidence_list_count: usize,
) -> PubEffectivePageProjection {
    // Pgid is scenario/design identity evidence, not generic physical visible-page authority.
    // Native Page.Duplicate/Pages.Add can create persisted visible pages without OplControlling/Pgid,
    // so generic Reader must not suppress any raw PAGE from this observation alone.
    // observed_scenario_page_ids is evidence only; any product-specific filtering needs a separate,
    // explicitly authorized profile law and must not mutate this generic page_ids projection.
    PubEffectivePageProjection {
        authority: PubEffectivePageProjectionAuthority::RawDocumentPageList,
        page_ids: raw_page_ids.to_vec(),
        raw_page_count: raw_page_ids.len(),
        observed_scenario_page_ids,
        scenario_evidence_list_count,
    }
}

fn resolve_scenario_page_ids_from_evidence(
    pgid_lists: &[Vec<(u32, u32)>],
    pages_by_oid: &BTreeMap<(u32, u32), Vec<PageId>>,
) -> std::result::Result<Vec<PageId>, String> {
    let Some(consensus) = pgid_lists.first() else {
        return Err("no_controlling_page_list_observation".to_owned());
    };
    if consensus.is_empty() {
        return Err("controlling_scenario_page_projection_empty".to_owned());
    }
    if pgid_lists
        .iter()
        .skip(1)
        .any(|candidate| candidate != consensus)
    {
        return Err("controlling_page_lists_disagree".to_owned());
    }

    let mut projected = Vec::with_capacity(consensus.len());
    let mut seen = BTreeSet::new();
    for pgid in consensus {
        let Some(matches) = pages_by_oid.get(pgid) else {
            return Err(format!("pgid_has_no_page:{:08x}:{:08x}", pgid.0, pgid.1));
        };
        if matches.len() != 1 {
            return Err(format!(
                "pgid_is_ambiguous:{:08x}:{:08x}:matches={}",
                pgid.0,
                pgid.1,
                matches.len()
            ));
        }
        let page_id = matches[0];
        if !seen.insert(page_id) {
            return Err("controlling_page_list_repeats_page".to_owned());
        }
        projected.push(page_id);
    }

    Ok(projected)
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

fn index_escher_by_contents_seq(inventory: &SpContainerInventory) -> BTreeMap<u32, Vec<usize>> {
    let mut index = BTreeMap::<u32, Vec<usize>>::new();

    for (shape_index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        for field in client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
        {
            let entry = index.entry(field.value).or_default();
            if entry.last().copied() != Some(shape_index) {
                entry.push(shape_index);
            }
        }
    }

    index
}

type GroupedCarrierParticipant = (usize, NodeId);
type GroupedCarrierMap = BTreeMap<u32, (PageId, Vec<GroupedCarrierParticipant>)>;

fn append_grouped_carrier_participants(
    seq_num: u32,
    grouped_by_carrier: &GroupedCarrierMap,
    seen_seq: &mut BTreeSet<u32>,
    rejected: &mut BTreeSet<PageId>,
    ordered: &mut BTreeMap<PageId, Vec<NodeId>>,
) -> bool {
    let Some((page_id, grouped_nodes)) = grouped_by_carrier.get(&seq_num) else {
        return false;
    };
    if !seen_seq.insert(seq_num) {
        rejected.insert(*page_id);
        return true;
    }
    ordered
        .entry(*page_id)
        .or_default()
        .extend(grouped_nodes.iter().map(|(_, node_id)| *node_id));
    true
}

fn source_page_paint_orders_v1(
    source_hash: Sha256Digest,
    graph: &PubSourceGraph,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    inventory: &SpContainerInventory,
) -> Vec<PubSourcePagePaintOrderV1> {
    let escher_by_contents_seq = index_escher_by_contents_seq(inventory);
    let mut expected = BTreeMap::<PageId, BTreeSet<NodeId>>::new();

    // Depth-1 grouped descendants are projected as page-owned canonical Nodes,
    // but their persisted Contents parent remains the GROUP carrier. Admit that
    // class into page paint order only when every participating descendant on
    // the page has one exact child Escher shape, one exact top-level GROUP
    // Escher carrier, and the OfficeArt parent-group source link agrees.
    let mut grouped_pending = BTreeMap::<PageId, Vec<(u32, usize, NodeId)>>::new();
    let mut grouped_invalid_pages = BTreeSet::<PageId>::new();

    for node in graph.nodes.values() {
        let seq_num = node.payload.contents_seq_num;
        let Some(reference) = references.get(&seq_num) else {
            continue;
        };
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };

        if let Some(page_id) = page_seq_to_id.get(&parent_seq).copied() {
            if node.header.parent_id == page_id.into_canonical() {
                expected.entry(page_id).or_default().insert(node.header.id);
            }
            continue;
        }

        let Some(group_reference) = references.get(&parent_seq) else {
            continue;
        };
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            continue;
        }
        let Some(group_parent_seq) = single_parent_seq(group_reference) else {
            continue;
        };
        let Some(page_id) = page_seq_to_id.get(&group_parent_seq).copied() else {
            // Nested groups remain outside this first bounded stack-order slice.
            continue;
        };
        if node.header.parent_id != page_id.into_canonical() {
            continue;
        }
        // #632 proves the first carrier-rank class only for visible grouped
        // Story and image descendants. Grouped TABLE/other classes remain
        // outside this slice even when they happen to have exact ancestry.
        if node.payload.story_frame.is_none() && node.payload.image_slot.is_none() {
            continue;
        }

        let child_matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_matches = escher_by_contents_seq
            .get(&parent_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let ([child_index], [group_index]) = (child_matches, group_matches) else {
            grouped_invalid_pages.insert(page_id);
            continue;
        };
        let child_shape = &inventory.shapes[*child_index];
        let group_shape = &inventory.shapes[*group_index];
        if child_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            grouped_invalid_pages.insert(page_id);
            continue;
        }

        grouped_pending.entry(page_id).or_default().push((
            parent_seq,
            *child_index,
            node.header.id,
        ));
    }

    let mut grouped_by_carrier = GroupedCarrierMap::new();
    for (page_id, entries) in grouped_pending {
        if grouped_invalid_pages.contains(&page_id) {
            continue;
        }
        for (carrier_seq, child_index, node_id) in entries {
            expected.entry(page_id).or_default().insert(node_id);
            let entry = grouped_by_carrier
                .entry(carrier_seq)
                .or_insert_with(|| (page_id, Vec::new()));
            if entry.0 != page_id {
                grouped_invalid_pages.insert(page_id);
                continue;
            }
            entry.1.push((child_index, node_id));
        }
    }
    for (_, grouped) in grouped_by_carrier.values_mut() {
        grouped.sort_by_key(|(child_index, _)| *child_index);
    }

    let mut ordered = BTreeMap::<PageId, Vec<NodeId>>::new();
    let mut rejected = BTreeSet::<PageId>::new();
    let mut seen_seq = BTreeSet::<u32>::new();

    // inspect_sp_containers preserves serialized traversal order. Do not sort
    // SPIDs here: serialized page SpContainer order is the bounded authority.
    // Exact depth-1 grouped descendants occupy the serialized position of their
    // top-level GROUP carrier; their internal order remains the already-proven
    // child SpContainer traversal order.
    for shape in &inventory.shapes {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let Some(shape_id) = unique_escher_field(client_data, PUBLISHER_FIELD_SHAPE_ID) else {
            continue;
        };
        let seq_num = shape_id.value;

        if append_grouped_carrier_participants(
            seq_num,
            &grouped_by_carrier,
            &mut seen_seq,
            &mut rejected,
            &mut ordered,
        ) {
            continue;
        }

        let Some(reference) = references.get(&seq_num) else {
            continue;
        };
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        let Some(page_id) = page_seq_to_id.get(&parent_seq).copied() else {
            continue;
        };
        let Ok(node_id) = derive_pub_node_id(&source_hash, seq_num) else {
            rejected.insert(page_id);
            continue;
        };
        if !graph.nodes.contains_key(&node_id) {
            continue;
        }
        if !seen_seq.insert(seq_num) {
            rejected.insert(page_id);
            continue;
        }
        ordered.entry(page_id).or_default().push(node_id);
    }

    expected
        .into_iter()
        .filter_map(|(page_id, expected_nodes)| {
            if rejected.contains(&page_id) || expected_nodes.is_empty() {
                return None;
            }
            let node_ids = ordered.remove(&page_id)?;
            let actual_nodes = node_ids.iter().copied().collect::<BTreeSet<_>>();
            (node_ids.len() == actual_nodes.len() && actual_nodes == expected_nodes).then(|| {
                PubSourcePagePaintOrderV1 {
                    schema_version: PUB_SOURCE_PAGE_PAINT_ORDER_SCHEMA_V1.to_owned(),
                    page_id,
                    node_ids,
                }
            })
        })
        .collect()
}

const OFFICE_ART_ADJUST_VALUE: u16 = 0x0147;
const OFFICE_ART_FILL_TYPE: u16 = 0x0180;
const OFFICE_ART_FILL_COLOR: u16 = 0x0181;
const OFFICE_ART_FILL_BOOLEANS: u16 = 0x01BF;
const OFFICE_ART_LINE_COLOR: u16 = 0x01C0;
const OFFICE_ART_LINE_WIDTH: u16 = 0x01CB;
const OFFICE_ART_LINE_BOOLEANS: u16 = 0x01FF;

// OfficeArt boolean property sets persist each use/value pair in mirrored
// high-word/low-word bit positions. Publisher corpus controls preserve the
// exact negative/positive pairs 0x00100000/0x00100010 for fill and
// 0x00080000/0x00080008 for line.
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;
const LINE_USE_LINE_BIT: u32 = 1 << 19;
const LINE_LINE_BIT: u32 = 1 << 3;
const OFFICEART_FSP_CONNECTOR_BIT: u32 = 1 << 8;
const OFFICEART_SHAPE_TYPE_NOT_PRIMITIVE: u16 = 0x0000;
const OFFICEART_SHAPE_TYPE_LINE: u16 = 0x0014;

// MS-ODRAW normative property defaults for the bounded solid 2-D paint surface.
const NORMATIVE_FILL_TYPE: u32 = 0;
const NORMATIVE_FILL_COLOR: u32 = 0x00FF_FFFF;
const NORMATIVE_LINE_COLOR: u32 = 0x0000_0000;
const NORMATIVE_LINE_WIDTH_EMU: u32 = 0x0000_2535;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaintScalarLayer {
    Absent,
    Value(PubEffectivePaintValue<u32>),
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaintLineVisibilityLayer {
    Absent,
    Value(PubEffectivePaintValue<bool>),
    Unresolved,
}

fn has_default_roundrect_geometry(shape: &pub_escher::SpContainerObservation) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(0x0002) {
        return false;
    }
    !shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .any(|property| property.property_id() == OFFICE_ART_ADJUST_VALUE)
}

fn has_explicit_officeart_paint_observation(shape: &pub_escher::SpContainerObservation) -> bool {
    shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .any(|property| {
            matches!(
                property.property_id(),
                OFFICE_ART_FILL_TYPE
                    | OFFICE_ART_FILL_COLOR
                    | OFFICE_ART_FILL_BOOLEANS
                    | OFFICE_ART_LINE_COLOR
                    | OFFICE_ART_LINE_WIDTH
                    | OFFICE_ART_LINE_BOOLEANS
            )
        })
}

fn explicit_officeart_paint(
    shape: &pub_escher::SpContainerObservation,
    color_scheme: Option<&MatureColorScheme>,
) -> PubExplicitShapePaintSource {
    let fill_type = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_TYPE);
    let fill_color = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_COLOR)
        .and_then(|value| bounded_officeart_rgb(value, color_scheme));
    let fill_visible =
        unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_BOOLEANS).and_then(|value| {
            (value & FILL_USE_FILLED_BIT != 0).then_some(value & FILL_FILLED_BIT != 0)
        });

    let line_color = unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_COLOR)
        .and_then(|value| bounded_officeart_rgb(value, color_scheme));
    let line_width = unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_WIDTH)
        .and_then(|value| (value <= 0x0132_F540).then_some(i64::from(value)));
    let line_visible =
        match line_visibility_from_records(&shape.fopts, PubEffectivePaintAuthority::ShapeLocal) {
            PaintLineVisibilityLayer::Value(value) => Some(value.value),
            PaintLineVisibilityLayer::Absent | PaintLineVisibilityLayer::Unresolved => None,
        };

    PubExplicitShapePaintSource {
        fill: PubExplicitFillSource {
            solid: fill_type == Some(0),
            color_rgb: fill_color,
            visible: fill_visible,
        },
        line: PubExplicitLineSource {
            color_rgb: line_color,
            width_emu: line_width,
            visible: line_visible,
        },
    }
}

/// Resolves only the bounded solid-paint subset of the MS-ODRAW effective
/// property hierarchy. The caller must admit the shape as a 2-D shape before
/// enabling normative 2-D visibility defaults.
pub fn resolve_bounded_effective_officeart_paint(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
    color_scheme: Option<&MatureColorScheme>,
    admit_normative_2d_defaults: bool,
) -> Option<PubEffectiveShapePaintSource> {
    if !admit_normative_2d_defaults {
        return None;
    }

    let fill_solid = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_FILL_TYPE,
        NORMATIVE_FILL_TYPE,
    )
    .and_then(|value| (value.value == 0).then(|| value.map(|_| true)));

    let fill_color = if shape_has_explicit_filled_without_fill_color(shape) {
        bounded_officeart_rgb(NORMATIVE_FILL_COLOR, color_scheme).map(|rgb| {
            PubEffectivePaintValue {
                value: rgb,
                authority: PubEffectivePaintAuthority::NormativeDefault,
                source: None,
            }
        })
    } else {
        resolve_effective_officeart_scalar(
            shape,
            dgg_defaults,
            OFFICE_ART_FILL_COLOR,
            NORMATIVE_FILL_COLOR,
        )
        .and_then(|value| {
            bounded_officeart_rgb(value.value, color_scheme).map(|rgb| value.map(|_| rgb))
        })
    };

    let fill_visible = resolve_effective_officeart_fill_visibility(shape, dgg_defaults);

    let line_color = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_LINE_COLOR,
        NORMATIVE_LINE_COLOR,
    )
    .and_then(|value| {
        bounded_officeart_rgb(value.value, color_scheme).map(|rgb| value.map(|_| rgb))
    });

    let line_width = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_LINE_WIDTH,
        NORMATIVE_LINE_WIDTH_EMU,
    )
    .and_then(|value| (value.value <= 0x0132_F540).then(|| value.map(i64::from)));

    let line_visible = resolve_effective_officeart_line_visibility(shape, dgg_defaults);

    Some(PubEffectiveShapePaintSource {
        fill: PubEffectiveFillSource {
            solid: fill_solid,
            color_rgb: fill_color,
            visible: fill_visible,
        },
        line: PubEffectiveLineSource {
            color_rgb: line_color,
            width_emu: line_width,
            visible: line_visible,
        },
    })
}

fn shape_has_explicit_filled_without_fill_color(
    shape: &pub_escher::SpContainerObservation,
) -> bool {
    let shape_type = shape.fsp.as_ref().map(|fsp| fsp.shape_type);
    if !matches!(shape_type, Some(0x0002 | 0x00CA)) {
        return false;
    }

    let fill_color = paint_scalar_from_records(
        &shape.fopts,
        OFFICE_ART_FILL_COLOR,
        PubEffectivePaintAuthority::ShapeLocal,
    );
    if !matches!(fill_color, PaintScalarLayer::Absent) {
        return false;
    }

    if shape_type == Some(0x00CA) {
        return matches!(
            fill_visibility_from_records(
                &shape.fopts,
                PubEffectivePaintAuthority::ShapeLocal,
            ),
            PaintLineVisibilityLayer::Value(value) if value.value
        );
    }

    matches!(
        paint_scalar_from_records(
            &shape.fopts,
            OFFICE_ART_FILL_BOOLEANS,
            PubEffectivePaintAuthority::ShapeLocal,
        ),
        PaintScalarLayer::Value(value)
            if value.value & FILL_USE_FILLED_BIT != 0
                && value.value & FILL_FILLED_BIT != 0
    )
}

fn resolve_effective_officeart_scalar(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
    property_id: u16,
    normative_default: u32,
) -> Option<PubEffectivePaintValue<u32>> {
    match paint_scalar_from_records(
        &shape.fopts,
        property_id,
        PubEffectivePaintAuthority::ShapeLocal,
    ) {
        PaintScalarLayer::Value(value) => return Some(value),
        PaintScalarLayer::Unresolved => return None,
        PaintScalarLayer::Absent => {}
    }

    if let Some(dgg) = dgg_defaults {
        match paint_scalar_from_records(
            &dgg.primary_options,
            property_id,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ) {
            PaintScalarLayer::Value(value) => return Some(value),
            PaintScalarLayer::Unresolved => return None,
            PaintScalarLayer::Absent => {}
        }
        match paint_scalar_from_records(
            &dgg.tertiary_options,
            property_id,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ) {
            PaintScalarLayer::Value(value) => return Some(value),
            PaintScalarLayer::Unresolved => return None,
            PaintScalarLayer::Absent => {}
        }
    }

    Some(PubEffectivePaintValue {
        value: normative_default,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

fn resolve_effective_officeart_fill_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> Option<PubEffectivePaintValue<bool>> {
    let mut layers = vec![fill_visibility_from_records(
        &shape.fopts,
        PubEffectivePaintAuthority::ShapeLocal,
    )];
    if let Some(dgg) = dgg_defaults {
        layers.push(fill_visibility_from_records(
            &dgg.primary_options,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ));
        layers.push(fill_visibility_from_records(
            &dgg.tertiary_options,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ));
    }

    for layer in layers {
        match layer {
            PaintLineVisibilityLayer::Absent => {}
            PaintLineVisibilityLayer::Unresolved => return None,
            PaintLineVisibilityLayer::Value(value) => return Some(value),
        }
    }

    Some(PubEffectivePaintValue {
        value: true,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

fn fill_visibility_from_records(
    records: &[pub_escher::FoptObservation],
    authority: PubEffectivePaintAuthority,
) -> PaintLineVisibilityLayer {
    let mut candidates = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == OFFICE_ART_FILL_BOOLEANS);

    let mut resolved = None;
    for property in candidates.by_ref() {
        if property.f_bid() || property.f_complex() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        if property.op & FILL_USE_FILLED_BIT == 0 {
            continue;
        }
        if resolved.is_some() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        resolved = Some(PubEffectivePaintValue {
            value: property.op & FILL_FILLED_BIT != 0,
            authority,
            source: Some(property.source.clone()),
        });
    }

    resolved
        .map(PaintLineVisibilityLayer::Value)
        .unwrap_or(PaintLineVisibilityLayer::Absent)
}

fn resolve_effective_officeart_line_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> Option<PubEffectivePaintValue<bool>> {
    let mut layers = vec![line_visibility_from_records(
        &shape.fopts,
        PubEffectivePaintAuthority::ShapeLocal,
    )];
    if let Some(dgg) = dgg_defaults {
        layers.push(line_visibility_from_records(
            &dgg.primary_options,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ));
        layers.push(line_visibility_from_records(
            &dgg.tertiary_options,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ));
    }

    for layer in layers {
        match layer {
            PaintLineVisibilityLayer::Absent => {}
            PaintLineVisibilityLayer::Unresolved => return None,
            PaintLineVisibilityLayer::Value(value) => return Some(value),
        }
    }

    Some(PubEffectivePaintValue {
        value: true,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

fn line_visibility_from_records(
    records: &[pub_escher::FoptObservation],
    authority: PubEffectivePaintAuthority,
) -> PaintLineVisibilityLayer {
    let mut candidates = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == OFFICE_ART_LINE_BOOLEANS);

    let mut resolved = None;
    for property in candidates.by_ref() {
        if property.f_bid() || property.f_complex() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        if property.op & LINE_USE_LINE_BIT == 0 {
            continue;
        }
        if resolved.is_some() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        resolved = Some(PubEffectivePaintValue {
            value: property.op & LINE_LINE_BIT != 0,
            authority,
            source: Some(property.source.clone()),
        });
    }

    resolved
        .map(PaintLineVisibilityLayer::Value)
        .unwrap_or(PaintLineVisibilityLayer::Absent)
}

fn paint_scalar_from_records(
    records: &[pub_escher::FoptObservation],
    property_id: u16,
    authority: PubEffectivePaintAuthority,
) -> PaintScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => PaintScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => {
            PaintScalarLayer::Value(PubEffectivePaintValue {
                value: property.op,
                authority,
                source: Some(property.source.clone()),
            })
        }
        _ => PaintScalarLayer::Unresolved,
    }
}

fn admits_normative_2d_paint_defaults(shape: &pub_escher::SpContainerObservation) -> bool {
    let Some(fsp) = shape.fsp.as_ref() else {
        return false;
    };
    fsp.shape_type != OFFICEART_SHAPE_TYPE_NOT_PRIMITIVE
        && fsp.shape_type != OFFICEART_SHAPE_TYPE_LINE
        && fsp.flags & OFFICEART_FSP_CONNECTOR_BIT == 0
}

fn effective_paint_has_dgg_authority(paint: &PubEffectiveShapePaintSource) -> bool {
    let authorities = [
        paint.fill.solid.as_ref().map(|value| value.authority),
        paint.fill.color_rgb.as_ref().map(|value| value.authority),
        paint.fill.visible.as_ref().map(|value| value.authority),
        paint.line.color_rgb.as_ref().map(|value| value.authority),
        paint.line.width_emu.as_ref().map(|value| value.authority),
        paint.line.visible.as_ref().map(|value| value.authority),
    ];
    authorities.into_iter().flatten().any(|authority| {
        matches!(
            authority,
            PubEffectivePaintAuthority::DrawingGroupPrimary
                | PubEffectivePaintAuthority::DrawingGroupTertiary
        )
    })
}

fn fopt_records_use_officeart_scheme_color(records: &[pub_escher::FoptObservation]) -> bool {
    records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            matches!(
                property.property_id(),
                OFFICE_ART_FILL_COLOR | OFFICE_ART_LINE_COLOR
            ) && !property.f_bid()
                && !property.f_complex()
        })
        .any(|property| (property.op >> 24) as u8 == 0x08)
}

fn paint_context_uses_officeart_scheme_color(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> bool {
    fopt_records_use_officeart_scheme_color(&shape.fopts)
        || dgg_defaults.is_some_and(|dgg| {
            fopt_records_use_officeart_scheme_color(&dgg.primary_options)
                || fopt_records_use_officeart_scheme_color(&dgg.tertiary_options)
        })
}

fn unique_explicit_officeart_scalar(
    shape: &pub_escher::SpContainerObservation,
    property_id: u16,
) -> Option<u32> {
    let values = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            property.property_id() == property_id && !property.f_bid() && !property.f_complex()
        })
        .map(|property| property.op)
        .collect::<BTreeSet<_>>();

    if values.len() == 1 {
        values.iter().next().copied()
    } else {
        None
    }
}

fn bounded_officeart_image_crop(
    shape: &pub_escher::SpContainerObservation,
) -> Option<PubExplicitImageCropSource> {
    fn arm(
        shape: &pub_escher::SpContainerObservation,
        property_id: u16,
    ) -> (bool, Option<u32>, bool) {
        let properties = shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == property_id)
            .collect::<Vec<_>>();

        if properties.is_empty() {
            return (false, None, false);
        }

        if properties
            .iter()
            .any(|property| property.f_bid() || property.f_complex())
        {
            return (true, None, true);
        }

        let values = properties
            .iter()
            .map(|property| property.op)
            .collect::<BTreeSet<_>>();
        match values.len() {
            1 => (true, values.iter().next().copied(), false),
            _ => (true, None, true),
        }
    }

    let (top_present, top_raw, top_ambiguous) = arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_TOP);
    let (bottom_present, bottom_raw, bottom_ambiguous) =
        arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM);
    let (left_present, left_raw, left_ambiguous) = arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_LEFT);
    let (right_present, right_raw, right_ambiguous) =
        arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_RIGHT);

    if !(top_present || bottom_present || left_present || right_present) {
        return None;
    }

    Some(PubExplicitImageCropSource {
        top_raw,
        bottom_raw,
        left_raw,
        right_raw,
        ambiguous: top_ambiguous || bottom_ambiguous || left_ambiguous || right_ambiguous,
    })
}

fn direct_officeart_rgb(value: u32) -> Option<[u8; 3]> {
    // OfficeArtCOLORREF uses upper-byte flags for non-direct color forms.
    // This bounded path accepts only the unflagged direct RGB form.
    if value & 0xFF00_0000 != 0 {
        return None;
    }
    let bytes = value.to_le_bytes();
    Some([bytes[0], bytes[1], bytes[2]])
}

fn bounded_officeart_rgb(value: u32, color_scheme: Option<&MatureColorScheme>) -> Option<[u8; 3]> {
    match (value >> 24) as u8 {
        0x00 => direct_officeart_rgb(value),
        0x08 => {
            let ordinal = usize::try_from(value & 0x00FF_FFFF).ok()?;
            color_scheme?.slots.get(ordinal)?.rgb
        }
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

#[derive(Debug)]
struct GroupedObjectProjection {
    page_id: PageId,
    bounds: RectEmu,
    depth: usize,
    group_sources: Vec<RawSpan>,
}

fn project_grouped_object_shape(
    first_group_seq: u32,
    child_shape: &pub_escher::SpContainerObservation,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    pages: &BTreeMap<PageId, Page>,
    escher_inventory: &SpContainerInventory,
    escher_by_contents_seq: &BTreeMap<u32, Vec<usize>>,
) -> Result<Option<GroupedObjectProjection>> {
    if shape_has_nonzero_rotation(child_shape)
        || shape_has_fsp_flag(child_shape, OFFICEART_FSP_FLIP_H)
        || shape_has_fsp_flag(child_shape, OFFICEART_FSP_FLIP_V)
    {
        bail!("grouped child has rotation or flip");
    }

    let child_anchor = child_shape
        .child_anchor
        .as_ref()
        .context("grouped child is missing ChildAnchor")?;
    let mut rect = coordinate_rect_i128(child_anchor)?;
    let mut current_shape = child_shape;
    let mut current_group_seq = first_group_seq;
    let mut seen = BTreeSet::new();
    let mut group_sources = Vec::new();

    for depth in 1..=2 {
        if !seen.insert(current_group_seq) {
            bail!("group ancestry cycle");
        }
        let group_reference = references
            .get(&current_group_seq)
            .context("group Contents reference missing")?;
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            bail!("group ancestry raw type is not 0x30");
        }

        let group_matches = escher_by_contents_seq
            .get(&current_group_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_shape = match group_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => bail!("group Escher shape missing"),
            _ => bail!("group Escher shape ambiguous"),
        };
        if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            bail!("OfficeArt parent-group link does not match Contents ancestry");
        }
        if shape_has_nonzero_rotation(group_shape)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V)
        {
            bail!("group ancestor has rotation or flip");
        }

        let fspgr = group_shape
            .fspgr
            .as_ref()
            .context("group is missing FSPGR")?;
        let group_coords = coordinate_rect_i128(fspgr)?;
        group_sources.push(group_shape.source.clone());

        let parent_seq =
            single_parent_seq(group_reference).context("group parent is missing or ambiguous")?;
        if let Some(&page_id) = page_seq_to_id.get(&parent_seq) {
            let anchor = group_shape
                .client_anchor
                .as_ref()
                .context("top group is missing ClientAnchor")?;
            let absolute = publisher_anchor_rect_i128(anchor)?;
            rect = project_rect_trunc(rect, group_coords, absolute)?;
            let page = pages
                .get(&page_id)
                .context("group page id is missing from graph")?;
            let bounds = center_origin_rect_to_page_bounds(page, rect)?;
            return Ok(Some(GroupedObjectProjection {
                page_id,
                bounds,
                depth,
                group_sources,
            }));
        }

        if depth == 2 {
            bail!("group ancestry exceeds bounded depth 2");
        }
        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            bail!("group parent is neither DOCUMENT page nor group");
        }

        let placement = group_shape
            .child_anchor
            .as_ref()
            .context("nested group is missing ChildAnchor")?;
        rect = project_rect_trunc(rect, group_coords, coordinate_rect_i128(placement)?)?;
        current_shape = group_shape;
        current_group_seq = parent_seq;
    }

    Ok(None)
}

fn coordinate_rect_i128(rect: &pub_escher::OfficeArtCoordinateRect) -> Result<[i128; 4]> {
    let out = [
        i128::from(rect.x_left),
        i128::from(rect.y_top),
        i128::from(rect.x_right),
        i128::from(rect.y_bottom),
    ];
    if out[2] <= out[0] || out[3] <= out[1] {
        bail!("coordinate rectangle is non-positive");
    }
    Ok(out)
}

fn publisher_anchor_rect_i128(anchor: &PublisherFieldRecord) -> Result<[i128; 4]> {
    let xs = signed_field(anchor, PUBLISHER_FIELD_XS).context("group ClientAnchor missing XS")?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS).context("group ClientAnchor missing YS")?;
    let xe = signed_field(anchor, PUBLISHER_FIELD_XE).context("group ClientAnchor missing XE")?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE).context("group ClientAnchor missing YE")?;
    let out = [
        i128::from(xs),
        i128::from(ys),
        i128::from(xe),
        i128::from(ye),
    ];
    if out[2] <= out[0] || out[3] <= out[1] {
        bail!("group ClientAnchor rectangle is non-positive");
    }
    Ok(out)
}

/// Deterministic grouped-geometry projection.
///
/// Integer division in Rust truncates toward zero. Raw FSPGR/ChildAnchor
/// records remain authoritative provenance; these rounded EMU coordinates are
/// a derived runtime projection only.
fn project_rect_trunc(
    rect: [i128; 4],
    source_space: [i128; 4],
    target_space: [i128; 4],
) -> Result<[i128; 4]> {
    let x0 = project_axis_trunc(
        rect[0],
        source_space[0],
        source_space[2],
        target_space[0],
        target_space[2],
    )?;
    let y0 = project_axis_trunc(
        rect[1],
        source_space[1],
        source_space[3],
        target_space[1],
        target_space[3],
    )?;
    let x1 = project_axis_trunc(
        rect[2],
        source_space[0],
        source_space[2],
        target_space[0],
        target_space[2],
    )?;
    let y1 = project_axis_trunc(
        rect[3],
        source_space[1],
        source_space[3],
        target_space[1],
        target_space[3],
    )?;
    if x1 <= x0 || y1 <= y0 {
        bail!("projected grouped rectangle is non-positive");
    }
    Ok([x0, y0, x1, y1])
}

fn project_axis_trunc(
    value: i128,
    source_start: i128,
    source_end: i128,
    target_start: i128,
    target_end: i128,
) -> Result<i128> {
    let source_len = source_end - source_start;
    let target_len = target_end - target_start;
    if source_len <= 0 || target_len <= 0 {
        bail!("group projection has non-positive coordinate extent");
    }
    Ok(target_start + (value - source_start) * target_len / source_len)
}

fn center_origin_rect_to_page_bounds(page: &Page, rect: [i128; 4]) -> Result<RectEmu> {
    let half_width = i128::from(page.size.width.get()) / 2;
    let half_height = i128::from(page.size.height.get()) / 2;
    let x = half_width + rect[0];
    let y = half_height + rect[1];
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];

    let to_i64 = |value: i128, label: &str| {
        i64::try_from(value).with_context(|| format!("{label} does not fit i64"))
    };
    Ok(RectEmu::new(
        LengthEmu::new(to_i64(x, "grouped x")?),
        LengthEmu::new(to_i64(y, "grouped y")?),
        LengthEmu::new(to_i64(width, "grouped width")?),
        LengthEmu::new(to_i64(height, "grouped height")?),
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

        assert!(append_grouped_carrier_participants(
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

        assert!(append_grouped_carrier_participants(
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
