#!/usr/bin/env python3
"""Synthetic-only tests: never place a real Publisher fixture in Git."""
import unittest

from PIL import Image, ImageDraw, ImageChops

from sbdc_r1_text_ink_shift_census import probe_shared_text_composite_shift

EMU = 1000


def scene():
    return {
        "pages": [{"page_id": "synthetic", "width_emu": 100 * EMU,
                   "height_emu": 100 * EMU}],
        "nodes": [{
            "node_id": "fake-private-node",
            "page_id": "synthetic",
            "kind": "text_frame",
            "text": "PRIVATE-SYNTHETIC",
            "bounds": {"x": 6 * EMU, "y": 10 * EMU,
                       "width": 80 * EMU, "height": 60 * EMU},
            "text_layout": {
                "disposition": "shared_resolved",
                "vertical_offset_emu": 0,
                "lines": [{
                    "line_index": 0, "text": "PRIVATE-SYNTHETIC",
                    "line_height_emu": 40 * EMU,
                    "measured_width_emu": 55 * EMU,
                    "x_offset_emu": 0,
                }],
            },
        }],
    }


def sample(offset=4):
    browser = Image.new("RGB", (100, 100), "white")
    pdf = browser.copy()
    ImageDraw.Draw(browser).rectangle((16, 34, 24, 37), fill="black")
    ImageDraw.Draw(pdf).rectangle((16, 34 + offset, 24, 37 + offset), fill="black")
    channels = ImageChops.difference(browser, pdf).split()
    peak = ImageChops.lighter(
        channels[0], ImageChops.lighter(channels[1], channels[2])
    )
    changed = peak.point(lambda x: 255 if x >= 24 else 0).histogram()[255]
    return browser, pdf, changed


class ShiftProbeTest(unittest.TestCase):
    def test_detects_virtual_pdf_shift_without_claiming_baseline(self):
        browser, pdf, changed = sample(4)
        result = probe_shared_text_composite_shift(browser, pdf, scene(), changed)
        self.assertEqual(result["best_virtual_pdf_dy_px"], -4)
        self.assertEqual(result["best_virtual_different_pixels"], 0)
        self.assertGreater(result["baseline_source_mask_different_pixels"], 0)
        self.assertFalse(result["source_text_ids_coordinates_fonts_pixels_exported"])
        self.assertNotIn("PRIVATE-SYNTHETIC", str(result))
        self.assertNotIn("fake-private-node", str(result))
        self.assertEqual(len(result["shift_scores"]), 13)
        for direction in (
            "pixel_neighborhood_browser_to_pdf",
            "pixel_neighborhood_pdf_to_browser",
        ):
            stats = result[direction]
            self.assertLessEqual(
                stats["radius2_unmatched_pixels"], stats["radius1_unmatched_pixels"]
            )
            self.assertLessEqual(
                stats["radius1_unmatched_pixels"],
                result["baseline_source_mask_different_pixels"],
            )

    def test_one_pixel_edge_displacement_recovers_without_moving_pdf(self):
        browser, pdf, changed = sample(1)
        result = probe_shared_text_composite_shift(browser, pdf, scene(), changed)
        self.assertGreater(result["baseline_source_mask_different_pixels"], 0)
        self.assertEqual(result["best_virtual_pdf_dy_px"], 0)
        for direction in (
            "pixel_neighborhood_browser_to_pdf",
            "pixel_neighborhood_pdf_to_browser",
        ):
            self.assertEqual(result[direction]["radius1_unmatched_pixels"], 0)
            self.assertEqual(result[direction]["radius2_unmatched_pixels"], 0)
        self.assertNotIn("PRIVATE-SYNTHETIC", str(result))
        self.assertNotIn("fake-private-node", str(result))

    def test_baseline_is_best_on_identical_inputs(self):
        browser, _, _ = sample(4)
        result = probe_shared_text_composite_shift(browser, browser.copy(), scene(), 0)
        self.assertEqual(result["best_virtual_pdf_dy_px"], 0)
        self.assertEqual(result["potential_reduction_pixels"], 0)

    def test_requires_physical_baseline_agreement(self):
        browser, pdf, changed = sample(4)
        with self.assertRaisesRegex(AssertionError, "independent R1"):
            probe_shared_text_composite_shift(browser, pdf, scene(), changed + 1)

    def test_rejects_empty_text_mask(self):
        browser, pdf, changed = sample(4)
        without_text = scene()
        without_text["nodes"] = []
        with self.assertRaisesRegex(AssertionError, "no source-resolved"):
            probe_shared_text_composite_shift(browser, pdf, without_text, changed)


if __name__ == "__main__":
    unittest.main()
