#!/usr/bin/env python3
"""Only synthetic pixels; never import a customer PUB/PDF into these tests."""
import unittest

from PIL import Image, ImageDraw

from sbdc_r1_raster_disagreement_census import census_r1_disagreement


def _scene(nodes):
    return {
        "pages": [{
            "page_id": "synthetic-page",
            "width_emu": 3_000,
            "height_emu": 3_000,
        }],
        "nodes": nodes,
    }


def _node(kind, bounds, *, text=None, layout=None, resource=None):
    return {
        "node_id": "synthetic-node",
        "page_id": "synthetic-page",
        "kind": kind,
        "bounds": bounds,
        "text": text,
        "text_layout": layout,
        "resource_id": resource,
        "transform": {"a": "1", "b": "0", "c": "0", "d": "1",
                      "tx": 0, "ty": 0},
    }


class RasterCensusTests(unittest.TestCase):
    def test_exclusive_regions_and_independent_pixel_total(self):
        bounds = {"x": 400, "y": 400, "width": 800, "height": 800}
        resolved = _node("text_frame", bounds, text="synthetic", layout={
            "disposition": "shared_resolved",
            "vertical_offset_emu": 0,
            "lines": [{
                "line_index": 0, "line_height_emu": 800,
                "measured_width_emu": 800, "x_offset_emu": 0,
                "text": "synthetic",
            }]
        })
        picture = _node(
            "picture_frame",
            {"x": 1100, "y": 1100, "width": 800, "height": 800},
            resource="synthetic-asset")
        white = Image.new("RGB", (30, 30), "white")
        painted = white.copy()
        ImageDraw.Draw(painted).point(
            [(5, 5), (18, 18), (10, 10), (27, 27)], fill="black")
        data = census_r1_disagreement(
            painted, white, _scene([resolved, picture]), 4,
            {"total": 1, "by_reason": {"base_font_unavailable": 1},
             "by_size_source": {"source_uniform_preview": 1}},
        )
        zones = data["zones_exclusive"]
        for name in ("text_only", "picture_only",
                     "text_picture_overlap", "outside_text_and_pictures"):
            self.assertEqual(zones[name]["significant_pixels"], 1, name)
        self.assertEqual(data["significant_pixels_total"], 4)
        self.assertEqual(sum(z["area_pixels"] for z in zones.values()), 900)
        self.assertEqual(data["text_source_node_counts"]["shared_resolved_plan_nodes"], 1)
        self.assertEqual(data["actual_chromium_text_preview"]["total"], 1)
        self.assertNotIn("synthetic", str(data))
        self.assertFalse(data["source_text_pixels_pdf_and_coordinates_exported"])

    def test_fallback_text_is_not_silently_classed_as_background(self):
        frame = _node("text_frame",
                      {"x": 200, "y": 200, "width": 500, "height": 500},
                      text="not exported", layout=None)
        reference = Image.new("RGB", (30, 30), "white")
        candidate = reference.copy()
        candidate.putpixel((4, 4), (0, 0, 0))
        data = census_r1_disagreement(candidate, reference, _scene([frame]), 1)
        self.assertEqual(data["zones_exclusive"]["text_only"]["significant_pixels"], 1)
        self.assertEqual(data["text_source_node_counts"]["fallback_frame_nodes"], 1)
        self.assertEqual(data["text_source_node_counts"]["shared_resolved_plan_nodes"], 0)

    def test_total_guard_does_not_accept_fabricated_improvement(self):
        reference = Image.new("RGB", (30, 30), "white")
        with self.assertRaisesRegex(AssertionError, "independent"):
            census_r1_disagreement(reference, reference, _scene([]), 123)


if __name__ == "__main__":
    unittest.main()
