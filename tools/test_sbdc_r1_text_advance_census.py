#!/usr/bin/env python3
"""Source-free synthetic assertions for Chromium vs Reader text advances."""
import unittest

from sbdc_r1_text_advance_census import census_private_browser_reader_advances


def row(kind, reader, chromium):
    return {"cohort": kind, "reader_width_emu": reader,
            "chromium_width_emu": chromium}


class AdvanceCensusTests(unittest.TestCase):
    def test_sign_direction_and_group_separation(self):
        result = census_private_browser_reader_advances([
            row("uniform_line", 95250, 85725.0),
            row("uniform_line", 95250, 114300.0),
            row("mixed_span", 95250, 104775.0),
        ])
        u = result["cohorts"]["uniform_line"]
        m = result["cohorts"]["mixed_span"]
        self.assertEqual(u["sample_count"], 2)
        self.assertEqual(m["sample_count"], 1)
        self.assertEqual(u["signed_chromium_minus_reader_css_px"]["min"], -1)
        self.assertEqual(u["signed_chromium_minus_reader_css_px"]["max"], 2)
        self.assertEqual(u["absolute_chromium_minus_reader_css_px"]["median"], 1.5)
        self.assertEqual(m["signed_chromium_minus_reader_percent"]["median"], 10.0)
        self.assertFalse(result["individual_text_fonts_ids_widths_coordinates_exported"])

    def test_empty_cohort_and_zero_denominator(self):
        result = census_private_browser_reader_advances([
            row("uniform_line", 0, 9525.0)
        ])
        self.assertEqual(result["cohorts"]["mixed_span"]["sample_count"], 0)
        self.assertIsNone(
            result["cohorts"]["uniform_line"]
            ["signed_chromium_minus_reader_percent"]["median"]
        )
        self.assertEqual(
            result["cohorts"]["uniform_line"]
            ["signed_chromium_minus_reader_css_px"]["median"], 1
        )

    def test_fail_closed_on_private_strings_and_invalid_sizes(self):
        for bad in [
            {"cohort": "uniform_line", "reader_width_emu": 100,
             "chromium_width_emu": 100.0, "secret_text": "private"},
            row("unknown", 9525, 9525.0),
            row("mixed_span", -1, 9525.0),
            row("uniform_line", 9525, float("nan")),
            row("uniform_line", True, 42.0),
        ]:
            with self.subTest(bad=bad), self.assertRaises(AssertionError):
                census_private_browser_reader_advances([bad])
        with self.assertRaises(AssertionError):
            census_private_browser_reader_advances([])


if __name__ == "__main__":
    unittest.main()
