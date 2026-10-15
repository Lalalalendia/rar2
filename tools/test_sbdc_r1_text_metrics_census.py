#!/usr/bin/env python3
"""Synthetic PDF and Scene checks; user source never copied to GitHub."""
import unittest

import fitz
from PIL import Image

from sbdc_r1_text_metrics_census import census_r1_text_metrics

EMU = 12700


def rectangle(x, y, width, height):
    return {
        "x": x * EMU, "y": y * EMU,
        "width": width * EMU, "height": height * EMU,
    }


def synthetic_inputs():
    doc = fitz.open()
    page = doc.new_page(width=100, height=100)
    page.insert_text((12, 24), "SYNTHETIC", fontsize=10)
    page.insert_text((12, 60), "PLACEHOLDER", fontsize=9)
    pdf = doc.tobytes()
    doc.close()
    source = {
        "pages": [{
            "page_id": "fake", "width_emu": 100 * EMU,
            "height_emu": 100 * EMU,
        }],
        "nodes": [{
            "page_id": "fake", "kind": "text_frame",
            "node_id": "not-to-be-exported",
            "text": "SYNTHETIC",
            "bounds": rectangle(10, 10, 80, 30),
            "text_layout": {
                "disposition": "shared_resolved",
                "font_size_emu": 18 * EMU,
                "line_height_emu": 18 * EMU,
                "lines": [{
                    "line_index": 0, "line_height_emu": 18 * EMU,
                    "x_offset_emu": 0, "measured_width_emu": 60 * EMU,
                    "text": "SYNTHETIC",
                }],
            },
            "transform": {
                "a": "1", "b": "0", "c": "0", "d": "1", "tx": 0, "ty": 0,
            },
        }, {
            "page_id": "fake", "kind": "text_frame",
            "node_id": "fallback-secret-node",
            "text": "PLACEHOLDER",
            "bounds": rectangle(10, 45, 80, 30),
            "text_layout": None,
            "transform": {
                "a": "1", "b": "0", "c": "0", "d": "1", "tx": 0, "ty": 0,
            },
        }],
    }
    browser = Image.new("RGB", (200, 200), "white")
    own_pdf = browser.copy()
    own_pdf.putpixel((30, 36), (0, 0, 0))
    own_pdf.putpixel((30, 110), (0, 0, 0))
    return source, pdf, browser, own_pdf


class TextMetricsTests(unittest.TestCase):
    def test_actual_synthetic_pdf_spans_and_font_size_mismatch(self):
        scene, pdf, browser, own_pdf = synthetic_inputs()
        receipt = census_r1_text_metrics(scene, pdf, browser, own_pdf, 2)
        self.assertEqual(receipt["source_text_frame_count"], 2)
        self.assertEqual(receipt["source_shared_resolved_frame_count"], 1)
        self.assertEqual(receipt["source_browser_fallback_frame_count"], 1)
        self.assertEqual(receipt["source_shared_layout_line_count"], 1)
        self.assertEqual(receipt["pdf_source_frames_with_assigned_spans"], 2)
        self.assertGreaterEqual(receipt["pdf_assigned_spans_to_source_frames"], 2)
        self.assertGreaterEqual(receipt["pdf_spans_with_nearest_source_size_gt_1_5pt"], 1)
        self.assertTrue(receipt["frames_anonymous_sorted_by_pixel_mismatch"])
        self.assertFalse(receipt["private_story_text_glyph_bytes_font_names_node_ids_positions_exported"])
        self.assertNotIn("SYNTHETIC", str(receipt))
        self.assertNotIn("PLACEHOLDER", str(receipt))
        self.assertNotIn("secret-node", str(receipt))

    def test_independent_raster_count_guard(self):
        scene, pdf, browser, own_pdf = synthetic_inputs()
        with self.assertRaisesRegex(AssertionError, "independent"):
            census_r1_text_metrics(scene, pdf, browser, own_pdf, 250)

    def test_missing_text_pdf_fails_instead_of_claiming_font_parity(self):
        scene, _, browser, own_pdf = synthetic_inputs()
        blank = fitz.open()
        blank.new_page(width=100, height=100)
        with self.assertRaisesRegex(AssertionError, "extractable"):
            census_r1_text_metrics(scene, blank.tobytes(), browser, own_pdf, 2)
        blank.close()


if __name__ == "__main__":
    unittest.main()
