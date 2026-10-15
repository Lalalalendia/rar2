#!/usr/bin/env python3
"""Only synthetic data; no Publisher file or source text is included."""
import unittest

from sbdc_r1_text_advance_outlier_census import (
    FLAGS, classify_private_advance_outliers
)


def record(cohort, expected, actual, positives=()):
    return {
        "cohort": cohort,
        "reader_width_emu": expected,
        "chromium_width_emu": float(actual),
        "flags": {name: name in positives for name in FLAGS},
    }


class OutlierClassificationTests(unittest.TestCase):
    def test_large_absolute_outlier_and_whitespace_feature(self):
        sample = [
            record("uniform_line", 4_762_500, 952_500, (
                "has_any_whitespace", "has_trailing_whitespace",
                "svg_css_preserves_whitespace", "chromium_font_set_check_true"
            )),
            record("uniform_line", 952_500, 947_738, (
                "svg_css_preserves_whitespace", "chromium_font_set_check_true"
            )),
            record("mixed_span", 952_500, 761_900, ("has_nbsp",))
        ]
        result = classify_private_advance_outliers(sample)
        u = result["cohorts"]["uniform_line"]
        m = result["cohorts"]["mixed_span"]
        self.assertEqual(u["sample_count"], 2)
        self.assertEqual(u["over_25px_count"], 1)
        self.assertEqual(u["over_10_percent_count"], 1)
        self.assertEqual(u["feature_positive_over_25px"]["has_trailing_whitespace"], 1)
        self.assertEqual(u["feature_positive_other"]["has_trailing_whitespace"], 0)
        self.assertEqual(m["over_10_percent_count"], 1)
        self.assertEqual(m["over_25px_count"], 0)
        self.assertFalse(result["source_text_ids_font_names_hashes_widths_coordinates_exported"])
        self.assertNotIn("SECRET", str(result))

    def test_exact_boundary_is_classified(self):
        sample = [record("mixed_span", 952_500, 714_375)]
        result = classify_private_advance_outliers(sample)
        self.assertEqual(result["cohorts"]["mixed_span"]["over_25px_count"], 1)
        self.assertEqual(result["cohorts"]["mixed_span"]["over_10_percent_count"], 1)

    def test_rejects_private_text_and_unknown_flags(self):
        good = record("uniform_line", 952_500, 940_000)
        for bad in [
            {**good, "private_text": "SECRET"},
            {**good, "flags": {**good["flags"], "font_family": "SECRET"}},
            {**good, "flags": {**good["flags"], "has_tab": "SECRET"}},
            {**good, "chromium_width_emu": float("nan")},
            {**good, "reader_width_emu": 0},
            {**good, "cohort": "SECRET"},
        ]:
            with self.subTest(bad=bad), self.assertRaises(AssertionError):
                classify_private_advance_outliers([bad])
        with self.assertRaises(AssertionError):
            classify_private_advance_outliers([])


if __name__ == "__main__":
    unittest.main()
