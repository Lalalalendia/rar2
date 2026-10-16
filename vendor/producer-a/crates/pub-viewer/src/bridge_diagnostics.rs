//! Product vocabulary for the existing source-adapter diagnostic contract.

use super::{PubBridgeDiagnostic, ViewerDiagnostic, ViewerDiagnosticSeverity};

// Reusable text-flow diagnostics belong to the diagnostics module, not the root.

pub(super) fn map_bridge_diagnostic(diagnostic: &PubBridgeDiagnostic) -> ViewerDiagnostic {
    use PubBridgeDiagnostic::*;

    let (code, severity, message) = match diagnostic {
        MissingRawType { .. } => (
            "viewer.source.missing_raw_type",
            ViewerDiagnosticSeverity::Info,
            "Some source metadata needed to identify an object is absent; this alone does not establish file damage.",
        ),
        MissingChunkOffset { .. } => (
            "viewer.source.missing_chunk_offset",
            ViewerDiagnosticSeverity::Info,
            "Some source metadata needed to locate an object is absent; this alone does not establish file damage.",
        ),
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
        LegacyTextFormattingUnresolved { .. } => (
            "viewer.text.legacy_formatting_unresolved",
            ViewerDiagnosticSeverity::FidelityWarning,
            "Legacy text remains available but exact old-generation formatting could not be projected safely.",
        ),
        McldRecordCountMismatch { .. } | McldOuterBoundViolation { .. } => (
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
        GroupedPrimitiveProjected { .. } => (
            "viewer.geometry.grouped_primitive_projected",
            ViewerDiagnosticSeverity::Info,
            "A grouped primitive shape was projected through its exact bounded group geometry chain.",
        ),
        GroupedPrimitiveProjectionUnavailable { .. } => (
            "viewer.geometry.grouped_primitive_projection_unavailable",
            ViewerDiagnosticSeverity::FidelityWarning,
            "A grouped primitive shape falls outside the bounded group geometry profile.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use pub_reader::PubBridgeDiagnostic;

    #[test]
    fn missing_directory_evidence_keeps_distinct_informational_product_codes() {
        // Provenance stays in the Reader contract; the source-neutral Viewer
        // receives a bounded explanation, without a health classification.
        let source = pub_reader::PubBridgeDiagnostic::MissingRawType {
            seq_num: 1,
            source: serde_json::from_value(
                serde_json::json!({"stream":"/Synthetic","offset":2,"len":18}),
            )
            .unwrap(),
            parent_seq_num: Some(23),
        };
        let offset = PubBridgeDiagnostic::MissingChunkOffset {
            seq_num: 1,
            source: serde_json::from_value(
                serde_json::json!({"stream":"/Synthetic","offset":2,"len":18}),
            )
            .unwrap(),
            parent_seq_num: Some(23),
        };
        for (diagnostic, code) in [
            (source, "viewer.source.missing_raw_type"),
            (offset, "viewer.source.missing_chunk_offset"),
        ] {
            let mapped = map_bridge_diagnostic(&diagnostic);
            assert_eq!(mapped.code, code);
            assert_eq!(mapped.severity, ViewerDiagnosticSeverity::Info);
            let value = serde_json::to_value(mapped).unwrap();
            assert_eq!(value["code"], code);
            assert!(value.get("health").is_none());
            assert!(value.get("chunk_offset").is_none());
        }
        let existing =
            map_bridge_diagnostic(&PubBridgeDiagnostic::MissingEscherGeometry { seq_num: 2 });
        assert_eq!(existing.code, "viewer.geometry.missing");
        assert_eq!(existing.severity, ViewerDiagnosticSeverity::FidelityWarning);
    }
}

pub(super) fn viewer_fallback_flow_metrics_diagnostic() -> ViewerDiagnostic {
    ViewerDiagnostic {
        code: "viewer.text.fallback_flow_metrics".to_owned(),
        severity: ViewerDiagnosticSeverity::FidelityWarning,
        message: "Visible text fragments use Viewer fallback font metrics for bounded frame flow while admitted source font sizes may affect sizing. Their frame ownership is grounded, but line breaks and fragment boundaries are not claimed to match Publisher typography.".to_owned(),
    }
}

pub(super) fn viewer_paragraph_flow_policy_applied_diagnostic(count: usize) -> ViewerDiagnostic {
    ViewerDiagnostic {
        code: "viewer.text.paragraph_flow_policy_applied".to_owned(),
        severity: ViewerDiagnosticSeverity::Info,
        message: format!(
            "{count} explicit source paragraph-flow constraint(s) participate in bounded fallback linked-frame allocation. Fallback font metrics remain non-authoritative for Publisher-exact line breaks."
        ),
    }
}

pub(super) fn is_refreshable_text_flow_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "viewer.text.flow_not_explicit"
            | "viewer.text.flow_partial"
            | "viewer.text.fallback_overset"
            | "viewer.text.frame_capacity_partial"
            | "viewer.text.fallback_metrics_unavailable"
            | "viewer.text.fallback_flow_metrics"
            | "viewer.text.paragraph_flow_policy_applied"
    )
}

