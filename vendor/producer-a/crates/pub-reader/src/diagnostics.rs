//! Source-neutral diagnostics emitted by the mature Publisher bridge.

use super::*;

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
