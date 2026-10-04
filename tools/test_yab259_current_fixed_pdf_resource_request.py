#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "tools" / "yab259_current_fixed_pdf_resource_request.rs"


class CurrentFixedPdfResourceRequestTests(unittest.TestCase):
    def test_adapter_materializes_without_reshaping_or_pub_reparse(self) -> None:
        text = ADAPTER.read_text(encoding="utf-8")
        self.assertIn("scalar_base: line.scalar_start", text)
        self.assertIn("units_per_em: line.units_per_em", text)
        self.assertIn("input.shaped_flow.geometry_scene()", text)
        self.assertIn("plan_output_fonts", text)
        self.assertNotIn("shape_bounded_ltr", text)
        self.assertNotIn("resolve_bounded_shaped_flow", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)
        self.assertNotIn("render_bounded_pdf", text)

    def test_input_is_strict_and_output_matches_low_level_renderer(self) -> None:
        text = ADAPTER.read_text(encoding="utf-8")
        self.assertIn("#[serde(deny_unknown_fields)]", text)
        self.assertIn(
            '"chaptera.current-fixed-pdf-resource-input.v1"',
            text,
        )
        self.assertIn(
            '"chaptera.fixed-pdf-packet-render-request.v1"',
            text,
        )


if __name__ == "__main__":
    unittest.main()
