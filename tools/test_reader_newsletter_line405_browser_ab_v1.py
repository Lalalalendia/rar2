#!/usr/bin/env python3
"""Source-free negative controls for LINE405 direct browser A/B."""
import unittest
from reader_newsletter_line405_browser_ab_v1 import (
    source_line_roi,
    expected_source_line_bbox,
    accept_page_delta,
    accept_source_line_footprint,
    verify_source_line_magenta,
)


class Line405BrowserABTests(unittest.TestCase):
    def test_source_envelope_clipped_to_exact_raster(self):
        roi = source_line_roi(
            {"width_emu": 7_560_000, "height_emu": 10_692_000},
            {"width": 1191, "height": 1684},
        )
        self.assertLess(roi["x"], 167)
        self.assertGreater(roi["y"] + roi["height"], 120)
        self.assertEqual(roi["x"] + roi["width"], 1191)

    def test_non_target_change_rejected(self):
        for page in range(3):
            accept_page_delta(page, 0, None, None)
            with self.assertRaisesRegex(ValueError, "non-target customer page"):
                accept_page_delta(page, 1, {"x": 0, "y": 0, "width": 1, "height": 1}, None)

    def test_page4_requires_visible_bounded_change(self):
        roi = {"x": 100, "y": 100, "width": 100, "height": 40}
        accept_page_delta(3, 6, {"x": 110, "y": 108, "width": 7, "height": 3}, roi)
        with self.assertRaisesRegex(ValueError, "measurable"):
            accept_page_delta(3, 0, None, roi)
        with self.assertRaisesRegex(ValueError, "escapes"):
            accept_page_delta(3, 7, {"x": 50, "y": 105, "width": 20, "height": 2}, roi)
        with self.assertRaisesRegex(ValueError, "malformed"):
            accept_page_delta(3, 1, {"x": "10", "y": 105, "width": 1, "height": 1}, roi)


    def test_exact_source_line_geometry_predicts_observed_real_144dpi_bbox(self):
        page = {"width_emu": 7_560_000, "height_emu": 10_692_000}
        raster = {"width": 1191, "height": 1684}
        expected = expected_source_line_bbox(page, raster)
        self.assertEqual(expected, {"x": 166, "y": 111, "width": 1025, "height": 16})
        accept_source_line_footprint(expected, 16_400, expected)

    def test_line_stroke_cannot_be_shifted_shortened_or_thinned(self):
        expected = {"x": 166, "y": 111, "width": 1025, "height": 16}
        cases = [
            {"x": 173, "y": 111, "width": 1018, "height": 16},
            {"x": 166, "y": 120, "width": 1025, "height": 16},
            {"x": 166, "y": 111, "width": 1000, "height": 16},
            {"x": 166, "y": 111, "width": 1025, "height": 11},
        ]
        for changed in cases:
            with self.subTest(changed=changed):
                with self.assertRaisesRegex(ValueError, "incorrect source-backed"):
                    accept_source_line_footprint(changed, 15_000, expected)
        with self.assertRaisesRegex(ValueError, "insufficient visible"):
            accept_source_line_footprint(expected, 100, expected)
        with self.assertRaisesRegex(ValueError, "no positive"):
            accept_source_line_footprint(expected, 0, expected)

    @staticmethod
    def synthetic_line_paint(colors):
        """One RGB row: white baseline and caller-controlled painted pixels."""
        width = len(colors)
        before = bytes([255, 255, 255] * width)
        after = bytes(component for rgb in colors for component in rgb)
        bbox = {"x": 0, "y": 0, "width": width, "height": 1}
        return (
            {"width": width, "height": 1, "stride": 3 * width, "samples": before},
            {"width": width, "height": 1, "stride": 3 * width, "samples": after},
            bbox,
        )

    def test_real_source_magenta_accepts_antialias_fringe(self):
        pixels = [(255, 0, 255)] * 99 + [(255, 229, 255)]
        before, after, bbox = self.synthetic_line_paint(pixels)
        evidence = verify_source_line_magenta(before, after, bbox, 100)
        self.assertEqual(evidence["source_rgb"], [255, 0, 255])
        self.assertEqual(evidence["strong_source_magenta_pixels"], 99)
        self.assertEqual(evidence["fraction_of_changed_significant_pixels"], 0.99)

    def test_wrong_source_paint_rgb_fails_even_inside_correct_stroke_bbox(self):
        pixels = [(255, 0, 255)] * 95 + [(0, 0, 0)] * 5
        before, after, bbox = self.synthetic_line_paint(pixels)
        with self.assertRaisesRegex(ValueError, "did not paint source-backed magenta"):
            verify_source_line_magenta(before, after, bbox, 100)
        before, after, bbox = self.synthetic_line_paint([(0, 0, 0)] * 100)
        with self.assertRaisesRegex(ValueError, "did not paint source-backed magenta"):
            verify_source_line_magenta(before, after, bbox, 100)

    def test_source_paint_tally_and_rgb_buffer_fail_closed(self):
        before, after, bbox = self.synthetic_line_paint([(255, 0, 255)] * 5)
        with self.assertRaisesRegex(ValueError, "tally differs"):
            verify_source_line_magenta(before, after, bbox, 4)
        broken = dict(after, samples=b"wrong")
        with self.assertRaisesRegex(ValueError, "malformed browser RGB"):
            verify_source_line_magenta(before, broken, bbox, 5)
        misplaced = dict(bbox, x=5)
        with self.assertRaisesRegex(ValueError, "bbox exceeds"):
            verify_source_line_magenta(before, after, misplaced, 5)


if __name__ == "__main__":
    unittest.main()
