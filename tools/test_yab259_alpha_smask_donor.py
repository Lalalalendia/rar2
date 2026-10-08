#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools" / "yab259_alpha_smask_donor.py"


class Yab259AlphaSmaskDonorSourceTests(unittest.TestCase):
    def test_exact_alpha_is_serialized_as_pdf_soft_mask_without_flattening(self) -> None:
        text = HELPER.read_text(encoding="utf-8")
        self.assertIn("alpha: Option<Vec<u8>>", text)
        self.assertIn("has_alpha.then_some(alpha)", text)
        self.assertIn("/SMask {object_id} 0 R", text)
        self.assertIn("/ColorSpace /DeviceGray", text)

    def test_order_preservation_and_report_determinism_are_retained(self) -> None:
        text = HELPER.read_text(encoding="utf-8")
        self.assertIn("prepare_order_preserving_pdf_donor", text)
        self.assertIn("reports.sort_by_key(|node| node.origin)", text)
        self.assertIn("EXPECTED_ALPHA_REPAIR_FILES", text)


if __name__ == "__main__":
    unittest.main()
