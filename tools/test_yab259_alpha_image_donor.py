#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools" / "yab259_alpha_image_donor.py"
RUNNER = ROOT / "tools" / "run_yab259_current_viewer_alpha_pdf.py"


class Yab259AlphaImageDonorSourceTests(unittest.TestCase):
    def test_alpha_patch_uses_exact_pdf_soft_mask_without_flattening(self) -> None:
        text = HELPER.read_text(encoding="utf-8")
        self.assertIn("PreparedImage::Rgba", text)
        self.assertIn("/SMask {soft_mask_id} 0 R", text)
        self.assertIn("/DeviceGray", text)
        self.assertIn("alpha_bytes", text)
        self.assertNotIn("flatten", text.lower())
        self.assertIn('"pdf.image.alpha_unsupported" in text', text)

    def test_alpha_donor_preserves_product_order_and_report_determinism(self) -> None:
        text = HELPER.read_text(encoding="utf-8")
        self.assertIn("prepare_order_preserving_pdf_donor", text)
        self.assertIn("surfaces.sort_by_key(|surface| surface.origin)", text)
        self.assertIn("nodes.sort_by_key(|node| node.origin)", text)
        self.assertIn("reports.sort_by_key(|node| node.origin)", text)

    def test_runner_reuses_supported_subset_mapper_and_existing_renderer(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("yab259_current_viewer_supported_subset_request.rs", text)
        self.assertIn("yab259_fixed_pdf_packet_renderer.rs", text)
        self.assertIn("prepare_alpha_image_pdf_donor", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)


if __name__ == "__main__":
    unittest.main()
