//! Public mature-PUB source-graph and bridge DTOs.
//!
//! This module owns source-neutral data declarations only. Mature stream parsing,
//! graph assembly, geometry projection, typography projection, recovery and
//! product routing remain in their existing owners.

use super::*;

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
    pub(super) fn map<U>(self, map: impl FnOnce(T) -> U) -> PubEffectivePaintValue<U> {
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
