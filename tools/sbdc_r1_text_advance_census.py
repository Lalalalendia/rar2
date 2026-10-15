"""Compare Chromium SVG text advance with Reader SharedResolved width.

The caller supplies only ephemeral numerical rows from the pinned same-R1
Chromium process. Individual widths stay in the isolated worker and only
cohort counts and aggregate statistics enter source-free CI receipts.
This is NOT an acceptance tolerance change or an instruction to rescale text.
"""
from __future__ import annotations

import math
from statistics import median

EMU_PER_CSS_PX = 9525
COHORTS = ("uniform_line", "mixed_span")
MAX_RECORDS = 4096
MAX_WIDTH_EMU = 100_000_000


def _summary(values: list[float]) -> dict:
    if not values:
        return {"min": None, "p10": None, "median": None, "p90": None,
                "p95": None, "max": None}
    ordered = sorted(values)
    n = len(ordered)
    percentile = lambda p: ordered[max(0, math.ceil(n * p) - 1)]
    return {
        "min": round(ordered[0], 4),
        "p10": round(percentile(0.10), 4),
        "median": round(median(ordered), 4),
        "p90": round(percentile(0.90), 4),
        "p95": round(percentile(0.95), 4),
        "max": round(ordered[-1], 4),
    }


def census_private_browser_reader_advances(rows: list[dict]) -> dict:
    """Aggregate absolute and signed differences without source-bearing rows."""
    if not isinstance(rows, list) or not (1 <= len(rows) <= MAX_RECORDS):
        raise AssertionError("private text-advance sample has invalid cardinality")
    by_cohort = {name: [] for name in COHORTS}
    for row in rows:
        if not isinstance(row, dict) or set(row) != {
            "cohort", "reader_width_emu", "chromium_width_emu"
        }:
            raise AssertionError("invalid private text-advance input schema")
        kind = row["cohort"]
        if kind not in by_cohort:
            raise AssertionError("unknown source text-advance cohort")
        expected = row["reader_width_emu"]
        actual = row["chromium_width_emu"]
        if (isinstance(expected, bool) or not isinstance(expected, int)
                or not 0 <= expected <= MAX_WIDTH_EMU
                or isinstance(actual, bool) or not isinstance(actual, (int, float))
                or not math.isfinite(actual) or not 0 <= actual <= MAX_WIDTH_EMU):
            raise AssertionError("invalid local CSS-pixel text width")
        by_cohort[kind].append((expected, float(actual)))

    def summarize(rows: list[tuple[int, float]]) -> dict:
        signed_px = [(browser - reader) / EMU_PER_CSS_PX
                     for reader, browser in rows]
        ratio_percent = [100.0 * (browser - reader) / reader
                         for reader, browser in rows if reader > 0]
        return {
            "sample_count": len(rows),
            "zero_reader_width_count": sum(reader == 0 for reader, _ in rows),
            "signed_chromium_minus_reader_css_px": _summary(signed_px),
            "absolute_chromium_minus_reader_css_px":
                _summary([abs(value) for value in signed_px]),
            "signed_chromium_minus_reader_percent": _summary(ratio_percent),
        }

    return {
        "schema": "chaptera.sbdc034-private-svg-computed-text-advance.v1",
        "source_scene_revision": "accepted_https_r1_only",
        "coordinate_unit": "svg_user_units_to_emu_via_9525",
        "cohorts": {name: summarize(by_cohort[name]) for name in COHORTS},
        "total_samples": len(rows),
        "individual_text_fonts_ids_widths_coordinates_exported": False,
        "interpretation": "advance_diagnostic_not_glyph_ink_or_pdf_parity_proof",
    }
