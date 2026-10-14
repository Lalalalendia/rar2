#!/usr/bin/env python3
"""Read-only, source-safe A/B triage of existing PUB -> PDF Publisher receipts.

Never reads PUB, PDF or font bytes. A 64x64 raster screening improvement is not
proof of Publisher visual parity, page surface authority, or a causal diagnosis.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import re

SCHEMA = "chaptera.pub-pdf-cli-publisher-fingerprint.v1"
OUT_SCHEMA = "chaptera.pub-pdf-oracle-delta.v1"
COMPARED = {"raster_compared", "raster_compared_stage_unknown"}
IDENTITY = ("oracle_id", "pub_sha256", "publisher_pdf_sha256", "reference_pages", "reference_surface_stage", "family")
CODE = re.compile(r"^[A-Za-z0-9_.-]{1,100}$")
SHA = re.compile(r"^[a-f0-9]{64}$")


def number(value: object, name: str, lower: float, upper: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"invalid numeric {name}")
    value = float(value)
    if not math.isfinite(value) or not lower <= value <= upper:
        raise ValueError(f"out-of-range {name}")
    return value


def page_map(pair: dict) -> dict[int, dict]:
    result = {}
    for page in pair.get("pages", []):
        index = page.get("page")
        if not isinstance(index, int) or isinstance(index, bool) or index <= 0 or index in result:
            raise ValueError("missing, invalid or duplicate page index")
        if page.get("status") == "compared":
            count = page.get("changed_cell_count")
            if not isinstance(count, int) or isinstance(count, bool) or not 0 <= count <= 4096:
                raise ValueError("invalid raster changed-cell count")
            fraction = number(page.get("changed_cell_fraction"), "changed-cell fraction", 0, 1)
            if abs(fraction - count / 4096) > 1e-9:
                raise ValueError("changed-cell count and fraction disagree")
            number(page.get("mean_abs_channel_delta"), "mean channel delta", 0, 255)
            number(page.get("max_channel_delta"), "max channel delta", 0, 255)
            if not SHA.fullmatch(str(page.get("reference_grid_sha256", ""))):
                raise ValueError("missing or invalid Publisher reference grid identity")
        result[index] = page
    return result


def safe_codes(pair: dict) -> list[str]:
    loss = pair.get("loss_summary") or {}
    groups = ("pdf_diagnostic_code_counts", "node_code_counts", "shaped_flow_diagnostic_code_counts", "skipped_text_code_counts")
    codes = set()
    for group in groups:
        counts = loss.get(group, {})
        if not isinstance(counts, dict):
            raise ValueError("invalid diagnostic count group")
        for code, count in counts.items():
            if not isinstance(code, str) or not CODE.fullmatch(code):
                raise ValueError("unsafe diagnostic code")
            if not isinstance(count, int) or isinstance(count, bool) or count < 0:
                raise ValueError("invalid diagnostic count")
            if count:
                codes.add(code)
    return sorted(codes)


def lanes(codes: list[str]) -> list[str]:
    """Pair-level associations only, NEVER automated attribution to a page."""
    result = set()
    for code in codes:
        if "image" in code and ("placement" in code or "crop" in code or "rotation" in code):
            result.add("image_placement_discriminator")
        elif "image" in code or "mime" in code:
            result.add("image_admission_discriminator")
        if "table" in code or "mcld" in code:
            result.add("table_metrics_discriminator")
        if "font" in code or "text" in code or "typograph" in code or "shaped" in code:
            result.add("text_font_layout_discriminator")
    return sorted(result) if result else ["unattributed_requires_object_scene_census"]


def load(path: Path) -> dict:
    report = json.loads(path.read_text(encoding="utf-8"))
    if report.get("schema") != SCHEMA or not isinstance(report.get("pairs"), list):
        raise ValueError("unexpected Publisher oracle receipt schema")
    if report.get("registered_pair_count") != len(report["pairs"]):
        raise ValueError("registered source count drift")
    if not isinstance(report.get("raster"), dict):
        raise ValueError("missing raster protocol")
    return report


def compare(baseline: dict, candidate: dict, *, top: int = 12) -> dict:
    if not isinstance(top, int) or not 1 <= top <= 100:
        raise ValueError("invalid top bound")
    if baseline.get("schema") != SCHEMA or candidate.get("schema") != SCHEMA:
        raise ValueError("cannot compare different oracle receipt schemas")
    for key in ("raster", "registered_pair_count", "registered_publisher_page_count", "hosted_expected_pair_count", "private_expected_pair_count"):
        if baseline.get(key) != candidate.get(key):
            raise ValueError(f"different comparison cohort or raster protocol: {key}")
    if not isinstance(baseline.get("pairs"), list) or not isinstance(candidate.get("pairs"), list):
        raise ValueError("missing pair records")
    if len(baseline["pairs"]) != baseline.get("registered_pair_count") or len(candidate["pairs"]) != candidate.get("registered_pair_count"):
        raise ValueError("incomplete registered-pair identities")

    def indexed(report: dict) -> dict[str, dict]:
        result = {}
        for pair in report["pairs"]:
            digest = pair.get("pub_sha256")
            if not isinstance(digest, str) or not SHA.fullmatch(digest) or digest in result:
                raise ValueError("invalid or duplicate source identity")
            if not SHA.fullmatch(str(pair.get("publisher_pdf_sha256", ""))):
                raise ValueError("invalid reference PDF identity")
            result[digest] = pair
        return result

    b_pairs = indexed(baseline)
    c_pairs = indexed(candidate)
    if b_pairs.keys() != c_pairs.keys():
        raise ValueError("source cohort identity changed")
    improved, regressed, unchanged, drift, unresolved, current = [], [], 0, [], [], []
    for source in sorted(b_pairs):
        b, c = b_pairs[source], c_pairs[source]
        if any(b.get(field) != c.get(field) for field in IDENTITY) or b.get("fixture") != c.get("fixture"):
            raise ValueError("Publisher pair reference or surface identity drift")
        b_pages, c_pages = page_map(b), page_map(c)
        for pair, pages in ((b, b_pages), (c, c_pages)):
            if pair.get("status") in COMPARED and (
                len(pages) != pair.get("reference_pages")
                or any(p.get("status") != "compared" for p in pages.values())
            ):
                raise ValueError("fully compared pair is missing a comparable page")
        if b.get("status") != c.get("status") or b_pages.keys() != c_pages.keys():
            drift.append({"fixture": b["fixture"], "baseline_status": b.get("status"), "candidate_status": c.get("status")})
        codes = safe_codes(c)
        old_codes = safe_codes(b)
        if b.get("status") not in COMPARED or c.get("status") not in COMPARED:
            unresolved.append({"fixture": b["fixture"], "baseline_status": b.get("status"), "candidate_status": c.get("status")})
        for index in sorted(set(b_pages) | set(c_pages)):
            before, after = b_pages.get(index), c_pages.get(index)
            if before is None or after is None or before.get("status") != after.get("status"):
                drift.append({"fixture": b["fixture"], "page": index, "reason": "page_coverage_changed"})
                continue
            if before.get("status") != "compared":
                continue
            if before["reference_grid_sha256"] != after["reference_grid_sha256"]:
                raise ValueError("reference page RGB fingerprint changed")
            delta_cells = before["changed_cell_count"] - after["changed_cell_count"]
            row = {"fixture": b["fixture"], "pub_sha256": source, "page": index,
                   "baseline_changed_cell_fraction": before["changed_cell_fraction"],
                   "candidate_changed_cell_fraction": after["changed_cell_fraction"],
                   "improvement_cells": delta_cells,
                   "mean_channel_delta_after": after["mean_abs_channel_delta"],
                   "diagnostic_codes_pair_level": codes,
                   "pair_codes_added": sorted(set(codes) - set(old_codes)),
                   "pair_codes_removed": sorted(set(old_codes) - set(codes)),
                   "candidate_lanes_not_proven_causes": lanes(codes)}
            current.append(row)
            if delta_cells > 0:
                improved.append(row)
            elif delta_cells < 0:
                regressed.append(row)
            else:
                unchanged += 1
    coverage_ok = not drift
    if not coverage_ok:
        verdict = "inconclusive_coverage_drift"
    elif regressed:
        verdict = "regression_detected"
    elif improved:
        verdict = "nonregressing_screening_improvement"
    else:
        verdict = "no_measured_screening_change"
    improved.sort(key=lambda row: (-row["improvement_cells"], row["fixture"], row["page"]))
    regressed.sort(key=lambda row: (row["improvement_cells"], row["fixture"], row["page"]))
    current.sort(key=lambda row: (-row["candidate_changed_cell_fraction"], row["fixture"], row["page"]))
    matched = len(current)
    return {"schema": OUT_SCHEMA,
            "verdict": verdict,
            "coverage_equal": coverage_ok,
            "registered_pair_count": len(b_pairs),
            "matched_raster_pages": matched,
            "baseline_compared_page_count": baseline.get("compared_page_count"),
            "candidate_compared_page_count": candidate.get("compared_page_count"),
            "improved_pages": len(improved), "regressed_pages": len(regressed), "unchanged_pages": unchanged,
            "mean_changed_cell_fraction_before_matched": (sum(row["baseline_changed_cell_fraction"] for row in current) / matched) if matched else None,
            "mean_changed_cell_fraction_after_matched": (sum(row["candidate_changed_cell_fraction"] for row in current) / matched) if matched else None,
            "coverage_drift": drift[:top], "nonraster_pairs": unresolved[:top],
            "biggest_improvements": improved[:top], "biggest_regressions": regressed[:top],
            "worst_remaining_pages": current[:top],
            "authority": {"source_pdf_and_font_bytes_present": False,
                          "raster_is_screening_not_parity": True,
                          "all_reference_surfaces_known": all(p.get("reference_surface_stage") == "logical_page" for p in b_pairs.values() if p.get("status") in COMPARED),
                          "diagnostic_codes_are_pair_level_not_page_causality": True},
            "next_step": "Join worst/regressed page with existing source-safe object/Scene/paint census; do not infer cause from pair-level diagnostics."}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--top", type=int, default=12)
    parser.add_argument("--expect", choices=("inconclusive_coverage_drift", "regression_detected", "nonregressing_screening_improvement", "no_measured_screening_change"))
    args = parser.parse_args()
    result = compare(load(args.baseline), load(args.candidate), top=args.top)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({k: result[k] for k in ("verdict", "coverage_equal", "matched_raster_pages", "improved_pages", "regressed_pages", "unchanged_pages")}, sort_keys=True))
    if args.expect and result["verdict"] != args.expect:
        parser.error(f"expected {args.expect}, observed {result['verdict']}")


if __name__ == "__main__":
    main()
