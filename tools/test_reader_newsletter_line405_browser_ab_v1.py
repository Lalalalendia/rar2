#!/usr/bin/env python3
"""Source-free negative controls for LINE405 direct browser A/B."""
import unittest
from reader_newsletter_line405_browser_ab_v1 import source_line_roi, accept_page_delta


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


if __name__ == "__main__":
    unittest.main()
