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
        self.assertIn("backend_fallback_reason_counts", text)
        self.assertIn("shaped_span_line_count", text)
        self.assertIn("missing_shaping_line_count", text)
        self.assertIn("unresolved_text_color_line_count", text)

    def test_runner_reuses_order_preserving_donor_and_existing_renderer(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("prepare_order_preserving_pdf_donor", text)
        self.assertIn("yab259_fixed_pdf_packet_renderer.rs", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)


if __name__ == "__main__":
    unittest.main()
