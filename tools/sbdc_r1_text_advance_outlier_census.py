"""Source-private classification of Chromium vs Reader width outliers.

Individual source runs and booleans remain in an ephemeral CI-only file. The
receipt contains only fixed-category counts, never text, node IDs, font family,
individual widths, absolute coordinates, hashes, or images. No paint edits.
"""
from __future__ import annotations

import math

MAX_RECORDS = 4096
EMU_PER_CSS_PX = 9525
COHORTS = ("uniform_line", "mixed_span")
FLAGS = (
    "has_any_whitespace",
    "has_leading_whitespace",
    "has_trailing_whitespace",
    "has_repeated_whitespace",
    "has_tab",
    "has_nbsp",
    "has_zero_width_or_format",
    "has_non_ascii",
    "svg_css_preserves_whitespace",
    "chromium_font_set_check_true",
)


def classify_private_advance_outliers(records: list[dict]) -> dict:
    if not isinstance(records, list) or not 1 <= len(records) <= MAX_RECORDS:
        raise AssertionError("invalid ephemeral sample count")
    cohorts = {}
    for cohort in COHORTS:
        cohorts[cohort] = {
            "sample_count": 0,
            "over_25px_count": 0,
            "over_10_percent_count": 0,
            "over_either_count": 0,
            "feature_positive_all": {flag: 0 for flag in FLAGS},
            "feature_positive_over_25px": {flag: 0 for flag in FLAGS},
            "feature_positive_other": {flag: 0 for flag in FLAGS},
        }
    for row in records:
        if not isinstance(row, dict) or set(row) != {
            "cohort", "reader_width_emu", "chromium_width_emu", "flags"
        }:
            raise AssertionError("private outlier record has unexpected fields")
        kind = row["cohort"]
        if kind not in cohorts:
            raise AssertionError("invalid text-advance cohort")
        expected = row["reader_width_emu"]
        actual = row["chromium_width_emu"]
        flags = row["flags"]
        if (
            isinstance(expected, bool) or not isinstance(expected, int)
            or not 0 < expected <= 100_000_000
            or isinstance(actual, bool) or not isinstance(actual, (float, int))
            or not math.isfinite(actual) or not 0 <= actual <= 100_000_000
            or not isinstance(flags, dict) or set(flags) != set(FLAGS)
            or any(type(flags[flag]) is not bool for flag in FLAGS)
        ):
            raise AssertionError("invalid width or forbidden private feature data")
        difference = abs(float(actual) - expected)
        major_absolute = difference >= 25 * EMU_PER_CSS_PX
        major_relative = difference / expected >= 0.10
        output = cohorts[kind]
        output["sample_count"] += 1
        output["over_25px_count"] += int(major_absolute)
        output["over_10_percent_count"] += int(major_relative)
        output["over_either_count"] += int(major_absolute or major_relative)
        group = (
            "feature_positive_over_25px" if major_absolute
            else "feature_positive_other"
        )
        for flag in FLAGS:
            if flags[flag]:
                output["feature_positive_all"][flag] += 1
                output[group][flag] += 1

    return {
        "schema": "chaptera.sbdc034-private-advance-outlier-classes.v1",
        "thresholds": {"absolute_css_px": 25, "relative_percent": 10},
        "cohorts": cohorts,
        "sample_count": len(records),
        "source_text_ids_font_names_hashes_widths_coordinates_exported": False,
        "interpretation": "outlier_classification_only_no_font_or_paint_fix",
    }
