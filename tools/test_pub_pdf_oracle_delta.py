#!/usr/bin/env python3
"""Synthetic source-safe positive and negative controls for the offline A/B join."""
from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from pub_pdf_oracle_delta import SCHEMA, compare


def fixture() -> dict:
    return {"schema": SCHEMA, "registered_pair_count": 2,
            "registered_publisher_page_count": 3, "hosted_expected_pair_count": 2,
            "private_expected_pair_count": 0, "compared_page_count": 3,
            "raster": {"renderer": "MuPDF", "dpi": 144, "grid": [64, 64], "resampling": "Pillow.BOX", "changed_cell_channel_delta": 12},
            "pairs": [
                {"fixture": "first", "pub_sha256": "a" * 64, "publisher_pdf_sha256": "c" * 64,
                 "oracle_id": "oracle-a", "reference_pages": 2, "reference_surface_stage": "unknown", "family": "batch01",
                 "status": "raster_compared_stage_unknown", "pages": [page(1, 256), page(2, 1024)],
                 "loss_summary": {"pdf_diagnostic_code_counts": {"pdf.image.placement_combination_unsupported": 1}}},
                {"fixture": "second", "pub_sha256": "b" * 64, "publisher_pdf_sha256": "d" * 64,
                 "oracle_id": "oracle-b", "reference_pages": 1, "reference_surface_stage": "unknown", "family": "batch01",
                 "status": "raster_compared_stage_unknown", "pages": [page(1, 0)]},
            ]}


def page(i: int, changed: int) -> dict:
    return {"page": i, "status": "compared", "changed_cell_count": changed,
            "changed_cell_fraction": changed / 4096, "mean_abs_channel_delta": 14.0,
            "max_channel_delta": 120, "reference_grid_sha256": ("e" if i == 1 else "f") * 64}


class DeltaContractTests(unittest.TestCase):
    def test_one_improvement_no_regression_and_pair_level_only(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"][0]["pages"][1] = page(2, 800)
        receipt = compare(before, after)
        self.assertEqual(receipt["verdict"], "nonregressing_screening_improvement")
        self.assertEqual((receipt["improved_pages"], receipt["regressed_pages"], receipt["unchanged_pages"]), (1, 0, 2))
        self.assertFalse(receipt["authority"]["all_reference_surfaces_known"])
        self.assertIn("image_placement_discriminator", receipt["biggest_improvements"][0]["candidate_lanes_not_proven_causes"])
        self.assertTrue(receipt["authority"]["diagnostic_codes_are_pair_level_not_page_causality"])

    def test_broad_change_regression_does_not_disappear_in_better_mean(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"][0]["pages"][0] = page(1, 0)
        after["pairs"][0]["pages"][1] = page(2, 1025)
        receipt = compare(before, after)
        self.assertEqual(receipt["verdict"], "regression_detected")
        self.assertEqual((receipt["improved_pages"], receipt["regressed_pages"]), (1, 1))
        self.assertLess(receipt["mean_changed_cell_fraction_after_matched"], receipt["mean_changed_cell_fraction_before_matched"])

    def test_coverage_loss_cannot_claim_zero_regression(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"][0]["pages"].pop()
        after["pairs"][0]["status"] = "partial_page_comparison"
        receipt = compare(before, after)
        self.assertEqual(receipt["verdict"], "inconclusive_coverage_drift")
        self.assertFalse(receipt["coverage_equal"])
        self.assertTrue(receipt["coverage_drift"])

    def test_changed_reference_identity_is_fatal(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"][0]["pages"][0]["reference_grid_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "reference page RGB fingerprint"):
            compare(before, after)
        after = copy.deepcopy(before)
        after["pairs"][0]["publisher_pdf_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "reference or surface identity"):
            compare(before, after)
        after = copy.deepcopy(before)
        after["raster"]["dpi"] = 300
        with self.assertRaisesRegex(ValueError, "raster protocol"):
            compare(before, after)

    def test_bad_counts_and_raw_diagnostics_fail_closed(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"][0]["pages"][0]["changed_cell_fraction"] = 0.1
        with self.assertRaisesRegex(ValueError, "count and fraction disagree"):
            compare(before, after)
        after = copy.deepcopy(before)
        after["pairs"][0]["loss_summary"]["pdf_diagnostic_code_counts"] = {"unsafe diagnostic code with spaces": 1}
        with self.assertRaisesRegex(ValueError, "unsafe diagnostic code"):
            compare(before, after)

    def test_reordered_pairs_are_rejoined_by_sha(self):
        before = fixture()
        after = copy.deepcopy(before)
        after["pairs"].reverse()
        self.assertEqual(compare(before, after)["verdict"], "no_measured_screening_change")


if __name__ == "__main__":
    unittest.main()
