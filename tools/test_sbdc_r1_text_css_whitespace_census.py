#!/usr/bin/env python3
"""Synthetic-only tests; no original PUB text or bitmap fixtures."""
import unittest

from sbdc_r1_text_css_whitespace_census import census_private_css_whitespace_probe


def row(cohort, reader, normal, pre, *, repeated=False, trailing=False,
        preserves=False):
    return {
        "cohort": cohort, "reader_width_emu": reader,
        "chromium_normal_width_emu": float(normal),
        "chromium_pre_width_emu": float(pre),
        "has_repeated_whitespace": repeated,
        "has_trailing_whitespace": trailing,
        "computed_normal_preserves_whitespace": preserves,
    }


class CSSWhitespaceCensusTests(unittest.TestCase):
    def test_pre_reduces_big_outlier_but_worsens_other(self):
        r = census_private_css_whitespace_probe([
            row("uniform_line", 3_810_000, 952_500, 3_810_000,
                repeated=True, trailing=True),
            row("uniform_line", 952_500, 943_000, 960_000),
            row("mixed_span", 952_500, 810_000, 952_500, trailing=True),
        ])
        uniform = r["cohorts"]["uniform_line"]
        self.assertEqual(uniform["sample_count"], 2)
        self.assertEqual(uniform["baseline_over_25px_count"], 1)
        self.assertEqual(uniform["virtual_pre_over_25px_count"], 0)
        self.assertEqual(uniform["better_count"], 2)
        self.assertEqual(uniform["baseline_outliers_improved_count"], 1)
        self.assertEqual(r["cohorts"]["mixed_span"]["sample_count"], 1)
        self.assertEqual(r["sample_count"], 3)
        self.assertFalse(r["per_line_text_ids_font_names_widths_coordinates_exported"])
        self.assertNotIn("secret", str(r))

    def test_worse_pre_and_unchanged_are_separate(self):
        r = census_private_css_whitespace_probe([
            row("uniform_line", 952_500, 952_500, 857_250),
            row("uniform_line", 952_500, 952_500, 952_500),
        ])
        self.assertEqual(r["cohorts"]["uniform_line"]["worse_count"], 1)
        self.assertEqual(r["cohorts"]["uniform_line"]["unchanged_count"], 1)

    def test_rejects_private_fields_and_bad_values(self):
        valid = row("uniform_line", 952_500, 943_000, 950_000)
        for invalid in [
            {**valid, "private_text": "secret"},
            {**valid, "font_name": "secret"},
            {**valid, "reader_width_emu": 0},
            {**valid, "chromium_pre_width_emu": float("nan")},
            {**valid, "has_trailing_whitespace": "secret"},
            {**valid, "cohort": "wrong"},
        ]:
            with self.subTest(invalid=invalid), self.assertRaises(AssertionError):
                census_private_css_whitespace_probe([invalid])
        with self.assertRaises(AssertionError):
            census_private_css_whitespace_probe([])


if __name__ == "__main__":
    unittest.main()
