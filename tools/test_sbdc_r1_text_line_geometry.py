#!/usr/bin/env python3
"""Synthetic-only tests for actual R1 browser/PDF line geometry receipts."""
import unittest

from sbdc_r1_text_line_geometry import (
    compare_actual_text_line_geometry, normalized_text_hash,
)


def line(text, x=10.0, y=20.0, width=40.0, height=15.0):
    return {
        "text_sha256": normalized_text_hash(text),
        "x": x, "y": y, "width": width, "height": height,
    }


class TextGeometryTest(unittest.TestCase):
    def test_unicode_normalization_and_offset_aggregates(self):
        browser = [line("Héllo", 10, 20, 30, 14),
                   line("  same   words", 15, 40, 35, 14)]
        pdf = [line("He\u0301llo", 12, 24, 32, 14),
               line("same words", 15, 39, 35, 14)]
        result = compare_actual_text_line_geometry(browser, pdf)
        self.assertEqual(result["exact_unicode_line_matches"], 2)
        self.assertEqual(result["browser_lines_without_exact_pdf_text"], 0)
        self.assertEqual(result["pdf_lines_without_exact_browser_text"], 0)
        self.assertEqual(result["matched_abs_top_offset_px"]["median"], 2.5)
        self.assertEqual(result["matched_abs_left_offset_px"]["max"], 2.0)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_top_px"]["median"], 1.5)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_top_px"]["min"], -1)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_top_px"]["max"], 4)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_left_px"]["median"], 1.0)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_width_px"]["median"], 1.0)
        self.assertFalse(result["source_text_hashes_and_coordinates_exported"])
        self.assertNotIn(normalized_text_hash("Héllo"), str(result))
        self.assertNotIn("Héllo", str(result))

    def test_duplicate_text_uses_nearest_unused_physical_match(self):
        browser = [line("Title", 10, 100), line("Title", 10, 600)]
        pdf = [line("Title", 11, 601), line("Title", 9, 100)]
        result = compare_actual_text_line_geometry(browser, pdf)
        self.assertEqual(result["exact_unicode_line_matches"], 2)
        self.assertEqual(result["matched_abs_top_offset_px"]["max"], 1.0)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_top_px"]["median"], 0.5)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_left_px"]["median"], 0)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_left_px"]["min"], -1)
        self.assertEqual(result["matched_signed_pdf_minus_chromium_left_px"]["max"], 1)

    def test_reflow_or_unicode_mapping_mismatch_is_explicit(self):
        result = compare_actual_text_line_geometry(
            [line("hello world"), line("next line")],
            [line("hello"), line("world next")],
        )
        self.assertEqual(result["exact_unicode_line_matches"], 0)
        self.assertEqual(result["browser_lines_without_exact_pdf_text"], 2)
        self.assertEqual(result["pdf_lines_without_exact_browser_text"], 2)
        self.assertIsNone(result["matched_abs_left_offset_px"]["median"])
        self.assertIsNone(result["matched_signed_pdf_minus_chromium_top_px"]["median"])
        self.assertIsNone(result["matched_signed_pdf_minus_chromium_top_px"]["p10"])

    def test_reject_bad_bounds_and_missing_hash(self):
        with self.assertRaises(AssertionError):
            compare_actual_text_line_geometry([{
                "text_sha256": "not-a-digest",
                "x": 0, "y": 0, "width": 1, "height": 1,
            }], [])
        with self.assertRaises(AssertionError):
            compare_actual_text_line_geometry(
                [line("glyph", width=float("nan"))], [],
            )


if __name__ == "__main__":
    unittest.main()
