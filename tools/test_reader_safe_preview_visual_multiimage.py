#!/usr/bin/env python3
"""Source-safe Product 001/023 visual admission negative controls."""
from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from reader_safe_preview_visual_multiimage import CASES, BROWSER, check_receipt

HEAD = "a" * 40
HASH_A = "b" * 64
HASH_B = "c" * 64
PAGE = "d" * 64


def receipt(case_id: str) -> dict:
    p = {
        "page_id_sha256": PAGE,
        "picture_frame_count": 7,
        "png_dimensions": {"width": 500, "height": 600},
        "image_count": 0,
        "unique_painted_resource_ids": 0,
        "screenshot_sha256": HASH_A,
    }
    q = copy.deepcopy(p)
    q["image_count"] = CASES[case_id]["images"]
    q["unique_painted_resource_ids"] = CASES[case_id]["images"]
    q["screenshot_sha256"] = HASH_B
    return {
        "schema": BROWSER, "fixture": case_id,
        "source_sha256": CASES[case_id]["pub_sha256"],
        "repository_commit_sha": HEAD,
        "same_source_scene_head_and_browser": True,
        "image_only_ablation": True,
        "real_chromium": True,
        "external_url_request_count": 0,
        "page_error_count": 0,
        "host_resource_count": CASES[case_id]["images"],
        "before": p, "after": q,
    }


class ProductVisualAdmissionTests(unittest.TestCase):
    def test_positive_cases(self):
        for case_id in CASES:
            with self.subTest(case=case_id):
                before, after = check_receipt(receipt(case_id), case_id, HEAD)
                self.assertNotEqual(before["screenshot_sha256"], after["screenshot_sha256"])

    def test_fail_closed_mutations(self):
        for field, value in [
            ("source_sha256", "0" * 64),
            ("repository_commit_sha", "1" * 40),
            ("fixture", "022"),
            ("image_only_ablation", False),
            ("real_chromium", False),
            ("external_url_request_count", 1),
            ("page_error_count", 1),
            ("host_resource_count", 0),
        ]:
            with self.subTest(field=field):
                bad = receipt("023")
                bad[field] = value
                with self.assertRaises(ValueError):
                    check_receipt(bad, "023", HEAD)

    def test_fail_closed_raster_membership(self):
        for field, value in [
            ("page_id_sha256", "different"),
            ("picture_frame_count", 0),
            ("png_dimensions", {"width": 499, "height": 600}),
            ("image_count", 0),
            ("unique_painted_resource_ids", 6),
            ("screenshot_sha256", HASH_A),
        ]:
            with self.subTest(field=field):
                bad = receipt("023")
                bad["after"][field] = value
                with self.assertRaises(ValueError):
                    check_receipt(bad, "023", HEAD)

    def test_fail_closed_unbounded_geometry(self):
        for dim in [0, -1, 30001, 1.5, True]:
            with self.subTest(dim=dim):
                bad = receipt("001")
                bad["after"]["png_dimensions"]["height"] = dim
                bad["before"]["png_dimensions"]["height"] = dim
                with self.assertRaises(ValueError):
                    check_receipt(bad, "001", HEAD)


if __name__ == "__main__":
    unittest.main()
