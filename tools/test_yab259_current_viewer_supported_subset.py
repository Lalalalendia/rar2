#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
MAPPER = ROOT / "tools" / "yab259_current_viewer_supported_subset_request.rs"
RUNNER = ROOT / "tools" / "run_yab259_current_viewer_supported_subset_pdf.py"


class CurrentViewerSupportedSubsetSourceTests(unittest.TestCase):
    def test_mapper_is_source_neutral_and_preserves_scene_vector_order(self) -> None:
        text = MAPPER.read_text(encoding="utf-8")
        self.assertIn("surfaces.push(ResolvedSurface", text)
        self.assertIn("nodes.push(ResolvedPhysicalNode", text)
        self.assertNotIn("sort_by_key", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)
        self.assertNotIn("open_mature", text)

    def test_projected_instances_keep_semantic_origin_but_get_unique_resolved_identity(self) -> None:
        text = MAPPER.read_text(encoding="utf-8")
        self.assertIn("projected_scene_instance: Option<CurrentProjectedSceneInstance>", text)
        self.assertIn("fn resolved_output_node_id_v1", text)
        self.assertIn("authoring_origin: node.node_id.into_canonical()", text)
        self.assertIn("resolved_node_origin: resolved_node_id", text)
        self.assertIn("node_id: resolved_node_id", text)
        self.assertNotIn(
            "current Viewer supported-subset mapping requires unique effective NodeIds",
            text,
        )

    def test_mapper_does_not_silently_claim_known_residual_classes(self) -> None:
        text = MAPPER.read_text(encoding="utf-8")
        self.assertIn("cropped_image_use_count", text)
        self.assertIn("table_node_count", text)
        self.assertIn("table_cell_paint_state_count", text)
        self.assertIn("table_nonempty_cell_text_count", text)
        self.assertIn("table_cell_story_range_count", text)
        self.assertIn("table_cell_typography_present_count", text)
        self.assertIn("table_cell_typography_complete_count", text)
        self.assertIn("table_cell_uniform_size_count", text)
        self.assertIn("table_cell_resolved_color_count", text)
        self.assertIn("table_cell_inset_bounds_valid_count", text)
        self.assertIn("complete_table_cell_typography_v1", text)
        self.assertIn("table_cell_inset_fits_bounds_v1", text)
        self.assertIn("table_uniform_cell_text_inset_count", text)
        self.assertIn("table_uniform_vertical_alignment_count", text)
        self.assertIn("mapped_table_text_cell_count", text)
        self.assertIn("mapped_table_text_run_count", text)
        self.assertIn("table_text_residual_cell_count", text)
        self.assertIn("append_single_line_table_text_runs_with_ascent_probe_v1", text)
        self.assertIn("compatible_natural_baseline_ascent_emu_v1", text)
        self.assertIn("single_line_natural_extent_emu", text)
        self.assertIn("uniform_cell_vertical_alignment: Option<CurrentTableVerticalAlignment>", text)
        self.assertIn("observe_table_text_authority_v1", text)
        self.assertIn("uniform_cell_text_inset_emu", text)
        self.assertIn("story_scalar_start", text)
        self.assertIn("story_scalar_end", text)
        self.assertIn("typography: Vec<CurrentTypographyRun>", text)
        self.assertIn("derived_table_fill_node_count", text)
        self.assertIn("derived_table_border_node_count", text)
        self.assertIn("derived_table_paint_node_count", text)
        self.assertIn("table_paint_incomplete_node_count", text)
        self.assertIn("table: Option<CurrentTable>", text)
        self.assertNotIn("table: Option<Value>", text)
        self.assertIn("fn append_table_paint_nodes_v1", text)
        self.assertIn("chaptera.current-viewer.table-paint.v1", text)
        self.assertIn("table_text", text)
        self.assertIn("backend_fallback_reason_counts", text)
        self.assertIn("shaped_span_line_count", text)
        self.assertIn("missing_shaping_line_count", text)
        self.assertIn("unresolved_text_color_line_count", text)
        self.assertIn("residual_node_signature_counts", text)
        self.assertIn("residual_projection_lane_counts", text)
        self.assertIn("text_resource_residual_signature_counts", text)
        self.assertIn("text_partial_residual_signature_counts", text)
        self.assertIn("mapped_resource_node_count", text)
        self.assertIn("text_resource_residual_node_count", text)
        self.assertIn("current Viewer residual partition does not cover every node exactly once", text)
        self.assertIn("current Viewer text-resource partition does not cover every text node exactly once", text)
        self.assertIn("shared_resolved:no_emittable_run", text)
        self.assertIn("text_resource_residual_cooccurrence_counts", text)
        self.assertIn("text_resource_residual_projection_lane_counts", text)
        self.assertIn("shared_resolved_line_outcome_counts", text)
        self.assertIn("backend_fallback:{reason}:{}", text)
        self.assertIn("uniform_size", text)
        self.assertIn("mixed_size", text)
        self.assertIn("size_profile_unknown", text)
        self.assertIn("unresolved_text_color:{}", text)
        self.assertIn("missing_rgb", text)
        self.assertIn("mixed_rgb", text)
        self.assertIn("coverage_gap", text)
        self.assertIn("invalid_range", text)
        self.assertIn("text_resource_residual_signature_lane_counts", text)
        self.assertIn("residual_signature_lane_counts", text)
        self.assertIn("shared_resolved_line_count", text)
        self.assertIn("current Viewer SharedResolved line outcomes do not partition every line exactly once", text)
        self.assertIn("derived TABLE paint census does not partition fill and border primitives", text)
        self.assertIn("derived TABLE paint node cardinality drift", text)

    def test_runner_defaults_to_exact_alpha_but_retains_explicit_reject_control(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("prepare_alpha_smask_pdf_donor", text)
        self.assertIn("prepare_order_preserving_pdf_donor", text)
        self.assertIn('choices=("smask", "reject")', text)
        self.assertIn('default="smask"', text)
        self.assertIn('args.image_alpha == "smask"', text)
        self.assertIn('"image_alpha_mode": args.image_alpha', text)
        self.assertIn("yab259_fixed_pdf_packet_renderer.rs", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)


# Current-main rerun marker after paragraph-spacing consumer landing.
if __name__ == "__main__":
    unittest.main()
