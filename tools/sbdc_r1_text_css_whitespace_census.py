"""R1 source-private SVG whitespace preservation counterfactual.

The browser measures real SVG advances twice: unmodified and with temporary
CSS white-space: pre. The original element style is restored and the original
length is verified before physical acceptance continues. Public evidence
contains only grouped statistics; never source text, individual widths, hashes,
font families, coordinates, or screenshots.
"""
from __future__ import annotations

import math
from statistics import median

EMU_PER_CSS_PX = 9525
MAX_WIDTH_EMU = 100_000_000
MAX_SAMPLES = 4096
COHORTS = ("uniform_line", "mixed_span")


def _stats(values: list[float]) -> dict:
    if not values:
        return {"min": None, "median": None, "p90": None, "max": None}
    ordered = sorted(values)
    return {
        "min": round(ordered[0], 4),
        "median": round(median(ordered), 4),
        "p90": round(ordered[max(0, math.ceil(len(ordered) * 0.9) - 1)], 4),
        "max": round(ordered[-1], 4),
    }


def census_private_css_whitespace_probe(records: list[dict]) -> dict:
    if not isinstance(records, list) or not 1 <= len(records) <= MAX_SAMPLES:
        raise AssertionError("invalid R1 whitespace probe cardinality")
    buckets = {kind: [] for kind in COHORTS}
    for record in records:
        if not isinstance(record, dict) or set(record) != {
            "cohort", "reader_width_emu", "chromium_normal_width_emu",
            "chromium_pre_width_emu", "has_repeated_whitespace",
            "has_trailing_whitespace", "computed_normal_preserves_whitespace",
        }:
            raise AssertionError("whitespace sample schema must be source-private")
        expected = record["reader_width_emu"]
        normal = record["chromium_normal_width_emu"]
        pre = record["chromium_pre_width_emu"]
        kind = record["cohort"]
        if kind not in buckets:
            raise AssertionError("unknown whitespace sample cohort")
        if (type(expected) is not int or not 0 < expected <= MAX_WIDTH_EMU
            or type(normal) not in (int, float)
            or type(pre) not in (int, float)
            or not math.isfinite(normal) or not math.isfinite(pre)
            or not 0 <= normal <= MAX_WIDTH_EMU
            or not 0 <= pre <= MAX_WIDTH_EMU
            or any(type(record[name]) is not bool for name in (
                "has_repeated_whitespace", "has_trailing_whitespace",
                "computed_normal_preserves_whitespace",
            ))
        ):
            raise AssertionError("invalid private widths or whitespace flags")
        buckets[kind].append(record)

    def summarize(rows: list[dict]) -> dict:
        baseline = [
            (x["chromium_normal_width_emu"] - x["reader_width_emu"])
            / EMU_PER_CSS_PX for x in rows
        ]
        proposed = [
            (x["chromium_pre_width_emu"] - x["reader_width_emu"])
            / EMU_PER_CSS_PX for x in rows
        ]
        outlier = [i for i, a in enumerate(baseline) if abs(a) >= 25]
        return {
            "sample_count": len(rows),
            "computed_normal_preserves_whitespace_count": sum(
                x["computed_normal_preserves_whitespace"] for x in rows
            ),
            "has_repeated_and_trailing_whitespace_count": sum(
                x["has_repeated_whitespace"] and x["has_trailing_whitespace"]
                for x in rows
            ),
            "normal_signed_error_css_px": _stats(baseline),
            "virtual_pre_signed_error_css_px": _stats(proposed),
            "normal_absolute_error_css_px": _stats([abs(v) for v in baseline]),
            "virtual_pre_absolute_error_css_px": _stats([abs(v) for v in proposed]),
            "baseline_over_25px_count": len(outlier),
            "virtual_pre_over_25px_count": sum(abs(v) >= 25 for v in proposed),
            "better_count": sum(abs(b) + 0.0001 < abs(a)
                                for a, b in zip(baseline, proposed)),
            "worse_count": sum(abs(a) + 0.0001 < abs(b)
                               for a, b in zip(baseline, proposed)),
            "unchanged_count": sum(abs(abs(a) - abs(b)) <= 0.0001
                                   for a, b in zip(baseline, proposed)),
            "baseline_outliers_improved_count": sum(
                abs(proposed[i]) + 0.0001 < abs(baseline[i])
                for i in outlier
            ),
            "baseline_outlier_normal_absolute_error_css_px":
                _stats([abs(baseline[i]) for i in outlier]),
            "baseline_outlier_virtual_pre_absolute_error_css_px":
                _stats([abs(proposed[i]) for i in outlier]),
        }

    return {
        "schema": "chaptera.sbdc034-r1-private-svg-css-whitespace-counterfactual.v1",
        "source_r1_exact": True,
        "applied_css": "temporary_white_space_pre_restored_before_receipt",
        "cohorts": {kind: summarize(buckets[kind]) for kind in COHORTS},
        "sample_count": len(records),
        "per_line_text_ids_font_names_widths_coordinates_exported": False,
        "interpretation": "css_whitespace_diagnostic_not_pdf_or_browser_paint_proof",
    }
