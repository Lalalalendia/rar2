#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools" / "yab259_order_preserving_donor.py"
RUNNER = ROOT / "tools" / "run_yab259_order_preserving_fixed_pdf_packet_renderer.py"


class Yab259OrderPreservingDonorSourceTests(unittest.TestCase):
    def test_patch_is_exactly_bounded_to_page_and_node_input_order(self) -> None:
        text = HELPER.read_text(encoding="utf-8")
        self.assertIn('ORDERED_PDF_FILE = "crates/pub-pdf/src/lib.rs"', text)
        self.assertIn("surfaces.sort_by_key(|surface| surface.origin)", text)
        self.assertIn("nodes.sort_by_key(|node| node.origin)", text)
        self.assertIn("reports.sort_by_key(|node| node.origin)", text)
        self.assertIn("EXPECTED_ORDERED_REPAIR_FILES", text)

    def test_runner_reuses_existing_source_neutral_renderer_adapter(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("yab259_fixed_pdf_packet_renderer.rs", text)
        self.assertIn("prepare_order_preserving_pdf_donor", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)
        self.assertNotIn("convert_pdf", text)
        self.assertNotIn("build_pdf_artifact", text)


if __name__ == "__main__":
    unittest.main()
