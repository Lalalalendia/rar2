#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGET = ROOT / "tools" / "current_viewer_fixed_pdf_residual_census.py"

spec = importlib.util.spec_from_file_location("residual_census", TARGET)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)


def line(start: int, end: int, text: str, *, spans=False, shaping=True):
    return {
        "scalar_start": start,
        "scalar_end": end,
        "text": text,
        "measured_width_emu": 100,
        "line_height_emu": 100,
        "x_offset_emu": 0,
        "spans": ([{"secret": "must-not-leak"}] if spans else []),
        "shaping": ({"opaque": True} if shaping else None),
    }


class ResidualCensusTests(unittest.TestCase):
    def test_source_safe_overlap_census(self) -> None:
        packet = {
            "protocol_version": module.PACKET_VERSION,
            "binding": {"source_hash": "sha256:test"},
            "pages": [
                {
                    "page_id": "p1",
                    "nodes": [
                        {
                            "node_id": "n-backend",
                            "bounds": {},
                            "transform": {},
                            "projected_scene_instance": {"secret_instance": "must-not-leak"},
                            "text": {
                                "typography": [],
                                "layout": {
                                    "disposition": {
                                        "kind": "backend_fallback",
                                        "reason": "story_extent_mismatch",
                                    },
                                    "lines": [],
                                },
                            },
                        },
                        {
                            "node_id": "n-incomplete-uniform",
                            "bounds": {},
                            "transform": {},
                            "text": {
                                "typography": [
                                    {"scalar_start": 0, "scalar_end": 4, "text_size_emu": 100}
                                ],
                                "layout": {
                                    "disposition": {
                                        "kind": "backend_fallback",
                                        "reason": "shared_layout_incomplete",
                                    },
                                    "lines": [],
                                },
                            },
                        },
                        {
                            "node_id": "n-incomplete-mixed",
                            "bounds": {},
                            "transform": {},
                            "text": {
                                "typography": [
                                    {"scalar_start": 0, "scalar_end": 2, "text_size_emu": 100},
                                    {"scalar_start": 2, "scalar_end": 4, "text_size_emu": 200},
                                ],
                                "layout": {
                                    "disposition": {
                                        "kind": "backend_fallback",
                                        "reason": "shared_layout_incomplete",
                                    },
                                    "lines": [],
                                },
                            },
                        },
                        {
                            "node_id": "n-crop-table",
                            "bounds": {},
                            "transform": {},
                            "image": {
                                "resource_id": "SECRET-RESOURCE-ID",
                                "mime": "image/png",
                                "source_window": {"secret": "crop"},
                            },
                            "table": {"secret": "table"},
                        },
                        {
                            "node_id": "n-color",
                            "bounds": {},
                            "transform": {},
                            "text": {
                                "typography": [
                                    {
                                        "scalar_start": 0,
                                        "scalar_end": 5,
                                        "color_rgb": None,
                                    }
                                ],
                                "layout": {
                                    "disposition": {"kind": "shared_resolved"},
                                    "lines": [line(0, 5, "SECRET-TEXT")],
                                },
                            },
                        },
                        {
                            "node_id": "n-span",
                            "bounds": {},
                            "transform": {},
                            "text": {
                                "typography": [],
                                "layout": {
                                    "disposition": {"kind": "shared_resolved"},
                                    "lines": [line(0, 5, "OTHER-SECRET", spans=True)],
                                },
                            },
                        },
                    ],
                }
            ],
        }
        renderer = {
            "renderer_result": {
                "summary": {"node_unsupported": 6},
                "node_reports": [
                    {
                        "origin_node_id": node_id,
                        "code": "pdf.node.resource_missing",
                        "disposition": "unsupported",
                    }
                    for node_id in (
                        "n-backend",
                        "n-incomplete-uniform",
                        "n-incomplete-mixed",
                        "n-crop-table",
                        "n-color",
                        "n-span",
                    )
                ],
            }
        }

        census = module.build_census(packet, renderer)
        self.assertEqual(census["resource_missing_node_count"], 6)
        self.assertEqual(
            census["reason_node_counts"]["text_backend_fallback:story_extent_mismatch"],
            1,
        )
        self.assertEqual(census["reason_node_counts"]["scene_projection:projected"], 1)
        self.assertEqual(census["reason_node_counts"]["scene_projection:base"], 5)
        self.assertEqual(
            census["reason_node_counts"]["text_backend_fallback:shared_layout_incomplete"],
            2,
        )
        self.assertEqual(
            census["reason_node_counts"]["shared_layout_incomplete:uniform_size"],
            1,
        )
        self.assertEqual(
            census["reason_node_counts"]["shared_layout_incomplete:mixed_size"],
            1,
        )
        self.assertEqual(census["reason_node_counts"]["cropped_image"], 1)
        self.assertEqual(census["reason_node_counts"]["table_present"], 1)
        self.assertEqual(census["reason_node_counts"]["shared_resolved_unresolved_color"], 1)
        self.assertEqual(
            census["reason_node_counts"]["shared_resolved_unresolved_color:missing_rgb"],
            1,
        )
        self.assertEqual(
            census["line_reason_counts"]["unresolved_text_color:missing_rgb"],
            1,
        )
        self.assertEqual(
            census["reason_node_counts"]["shared_resolved_shaped_spans_only_or_partial"],
            1,
        )
        serialized = json.dumps(census, sort_keys=True)
        for forbidden in (
            "SECRET-TEXT",
            "OTHER-SECRET",
            "SECRET-RESOURCE-ID",
            "must-not-leak",
            "secret_instance",
            '"color_rgb"',
            '"bounds"',
            '"source_window"',
        ):
            self.assertNotIn(forbidden, serialized)

    def test_uniform_color_requires_complete_contiguous_uniform_coverage(self) -> None:
        self.assertTrue(
            module.uniform_color_for_range(
                [{"scalar_start": 0, "scalar_end": 5, "color_rgb": [1, 2, 3]}],
                0,
                5,
            )
        )
        self.assertFalse(
            module.uniform_color_for_range(
                [{"scalar_start": 0, "scalar_end": 4, "color_rgb": [1, 2, 3]}],
                0,
                5,
            )
        )
        self.assertFalse(
            module.uniform_color_for_range(
                [
                    {"scalar_start": 0, "scalar_end": 2, "color_rgb": [1, 2, 3]},
                    {"scalar_start": 2, "scalar_end": 5, "color_rgb": [9, 9, 9]},
                ],
                0,
                5,
            )
        )
        self.assertEqual(
            module.color_resolution_for_range(
                [{"scalar_start": 0, "scalar_end": 5, "color_rgb": None}],
                0,
                5,
            ),
            "missing_rgb",
        )
        self.assertEqual(
            module.color_resolution_for_range(
                [
                    {"scalar_start": 0, "scalar_end": 2, "color_rgb": [1, 2, 3]},
                    {"scalar_start": 2, "scalar_end": 5, "color_rgb": [9, 9, 9]},
                ],
                0,
                5,
            ),
            "mixed_rgb",
        )
        self.assertEqual(
            module.color_resolution_for_range(
                [{"scalar_start": 0, "scalar_end": 4, "color_rgb": [1, 2, 3]}],
                0,
                5,
            ),
            "coverage_gap",
        )


if __name__ == "__main__":
    unittest.main()
