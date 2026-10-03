#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

MODULE_PATH = (
    Path(__file__).resolve().parents[1]
    / "reader1050_trophy_wayback_lineage.py"
)
SPEC = importlib.util.spec_from_file_location("trophy_lineage", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
lineage = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = lineage
SPEC.loader.exec_module(lineage)


def profile(
    directory: str,
    layout: str,
    *,
    major: int = 3,
    stream_count: int = 4,
    storage_count: int = 1,
) -> dict:
    return {
        "directory_shape_sha256": directory,
        "stream_layout_multiset_sha256": layout,
        "cfb_major": major,
        "sector_size": 512,
        "stream_count": stream_count,
        "storage_count": storage_count,
        "known_stream_name_counts": {"Contents": 1},
    }


class TrophyLineageTests(unittest.TestCase):
    def test_relation_prefers_exact_stream_layout(self) -> None:
        left = profile("dir-a", "layout-a")
        right = profile("dir-b", "layout-a")
        rank, name = lineage.relation("a" * 64, left, "b" * 64, right)
        self.assertEqual(rank, 4)
        self.assertEqual(name, "same_stream_layout")

    def test_byte_identical_is_not_a_distinct_control(self) -> None:
        result = lineage.decide(
            {"opened": False},
            [{
                "relation": "byte_identical",
                "relation_rank": 6,
                "reader": {"opened": True},
            }],
        )
        self.assertEqual(result["status"], "no_distinct_historical_variant")
        self.assertFalse(result["typed_corruption_authorized"])

    def test_close_normal_open_control_requires_followup_ab(self) -> None:
        result = lineage.decide(
            {"opened": False},
            [{
                "relation": "same_stream_layout",
                "relation_rank": 4,
                "reader": {"opened": True},
            }],
        )
        self.assertEqual(
            result["status"],
            "normal_open_close_historical_control_found",
        )
        self.assertFalse(result["typed_corruption_authorized"])
        self.assertIn("independent", result["next_operation"])

    def test_distant_normal_open_variant_does_not_authorize_corruption(self) -> None:
        result = lineage.decide(
            {"opened": False},
            [{
                "relation": "same_major_carrier_set",
                "relation_rank": 1,
                "reader": {"opened": True},
            }],
        )
        self.assertEqual(
            result["status"],
            "normal_open_historical_variant_not_close_enough",
        )
        self.assertFalse(result["typed_corruption_authorized"])

    def test_distinct_nonopening_variant_closes_lineage_route(self) -> None:
        result = lineage.decide(
            {"opened": False},
            [{
                "relation": "same_directory_shape",
                "relation_rank": 3,
                "reader": {"opened": False},
            }],
        )
        self.assertEqual(
            result["status"],
            "distinct_historical_variants_without_normal_open_control",
        )
        self.assertFalse(result["typed_corruption_authorized"])

    def test_obsolete_when_source_now_opens(self) -> None:
        result = lineage.decide({"opened": True}, [])
        self.assertEqual(result["status"], "obsolete_source_now_opens")
        self.assertFalse(result["typed_corruption_authorized"])


if __name__ == "__main__":
    unittest.main()
