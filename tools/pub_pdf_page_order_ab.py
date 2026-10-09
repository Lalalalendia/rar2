#!/usr/bin/env python3
"""One-variable PDF page-order screening against existing Publisher raster grids.

This experiment does not change, rewrite, or upload any PDF. It compares the
same current-main PDF pages in two orders: existing canonical order, and source-
Viewer document order established by a separate bounded source-only census.
All PUB/PDF/font bytes remain ephemeral on the trusted Actions runner.
"""
from __future__ import annotations

import argparse
from collections import Counter
from contextlib import ExitStack
import hashlib
import json
import math
from pathlib import Path
import re
import tempfile

import fitz
from PIL import Image

ORDER_SCHEMA = "chaptera.pub-pdf-page-order-corpus.v1"
REPORT_SCHEMA = "chaptera.pub-pdf-page-order-ab.v1"
SHA = re.compile(r"[0-9a-f]{64}\Z")
GRID = 64
DPI = 144
DELTA = 12
MAX_PIXELS = 60_000_000


def validate_order_census(receipt: dict) -> dict[str, dict]:
    if receipt.get("schema") != ORDER_SCHEMA or receipt.get("hosted_pair_count") != 79:
        raise ValueError("unsupported/incomplete page-order census authority")
    records = receipt.get("pairs")
    if not isinstance(records, list) or len(records) != 79:
        raise ValueError("wrong page-order registry cardinality")
    by_source: dict[str, dict] = {}
    for row in records:
        source = row.get("source_sha256")
        page_count = row.get("page_count")
        seq = row.get("canonical_pdf_order_as_viewer_ordinals")
        if not isinstance(source, str) or not SHA.fullmatch(source) or source in by_source:
            raise ValueError("invalid or duplicate source SHA")
        if not isinstance(page_count, int) or isinstance(page_count, bool) or page_count < 1 or page_count > 10_000:
            raise ValueError("invalid page count")
        if not isinstance(seq, list) or len(seq) != page_count or any(
            not isinstance(value, int) or isinstance(value, bool) for value in seq
        ) or sorted(seq) != list(range(1, page_count + 1)):
            raise ValueError("missing, duplicate or non-bijective source page mapping")
        changed_count = sum(index != index_expected for index_expected, index in enumerate(seq, start=1))
        if changed_count != row.get("changed_position_count") or (changed_count > 0) is not row.get("order_changed"):
            raise ValueError("inconsistent page-order change claim")
        if row.get("route") not in ("mature_0x2c", "legacy_0x22_low_text", "legacy_0x22_quill"):
            raise ValueError("unsupported source route")
        by_source[source] = row
    if len(by_source) != 79 or sum(r["order_changed"] for r in by_source.values()) != receipt.get("order_changed_pair_count"):
        raise ValueError("inconsistent page-order aggregate")
    return by_source


def viewer_order_indices(canonical_pdf_order_as_viewer_ordinals: list[int]) -> list[int]:
    """PDF page 1-based index at each 1-based Viewer document ordinal."""
    return [canonical_pdf_order_as_viewer_ordinals.index(ordinal) for ordinal in range(1, len(canonical_pdf_order_as_viewer_ordinals) + 1)]


def candidate_grid(page: fitz.Page) -> bytes:
    pix = page.get_pixmap(dpi=DPI, colorspace=fitz.csRGB, alpha=False)
    if pix.width <= 0 or pix.height <= 0 or pix.width * pix.height > MAX_PIXELS:
        raise ValueError("raster surface beyond configured budget")
    image = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    return image.resize((GRID, GRID), Image.Resampling.BOX).tobytes()


def score_grid(candidate: bytes, reference: bytes) -> dict:
    if len(candidate) != len(reference) or len(candidate) != GRID * GRID * 3:
        raise ValueError("64x64 RGB raster grid length mismatch")
    count = 0
    abs_sum = 0
    for index in range(0, len(candidate), 3):
        deltas = tuple(abs(candidate[index + offset] - reference[index + offset]) for offset in (0, 1, 2))
        abs_sum += sum(deltas)
        count += max(deltas) >= DELTA
    return {"changed_cell_count": count, "changed_cell_fraction": count / (GRID * GRID),
            "mean_abs_channel_delta": abs_sum / (GRID * GRID * 3)}


def reference_grid_checked(row: dict) -> bytes:
    """Reuse the canonical reference-grid verifier when called inside rar2."""
    from cloud_reader_visual_fingerprint_v1 import reference_grid
    return reference_grid(row)


def compare_virtual_reorder(candidate_pdf: Path, pair: dict, permutation: list[int], *, reference_loader=reference_grid_checked) -> dict:
    if len(permutation) != pair.get("reference_pages"):
        return {"status": "page_count_or_order_mismatch", "before": [], "after": []}
    page_count = len(permutation)
    if sorted(permutation) != list(range(1, page_count + 1)):
        raise ValueError("non-bijective source page order")
    stage = pair.get("reference_surface_stage") or "unknown"
    if stage not in ("logical_page", "production_sheet", "viewport_spread", "unknown"):
        raise ValueError("invalid Publisher reference surface stage")
    if stage in ("production_sheet", "viewport_spread"):
        return {"status": "surface_mapping_required", "before": [], "after": []}
    references = pair.get("pages")
    if not isinstance(references, list) or len(references) != page_count:
        raise ValueError("incomplete reference pages")
    viewer_indices = viewer_order_indices(permutation)
    with fitz.open(candidate_pdf) as doc:
        if doc.page_count != page_count:
            return {"status": "page_count_or_order_mismatch", "before": [], "after": []}
        # Every candidate page is rasterized once; this experiment is not a
        # conversion, resampling, or output-resource modification A/B.
        grids = [candidate_grid(doc[i]) for i in range(page_count)]
        sizes = [(float(doc[i].rect.width), float(doc[i].rect.height)) for i in range(page_count)]
    before, after = [], []
    for position, ref in enumerate(references):
        ref_bytes = reference_loader(ref)
        reference_size = (float(ref["media_width_pt"]), float(ref["media_height_pt"]))
        for output, index in ((before, position), (after, viewer_indices[position])):
            width, height = sizes[index]
            extent_delta = (round(width - reference_size[0], 6), round(height - reference_size[1], 6))
            if any(abs(delta) > 0.05 for delta in extent_delta):
                output.append({"status": "media_extent_mismatch", "candidate_pdf_ordinal": index + 1})
            else:
                score = score_grid(grids[index], ref_bytes)
                output.append({"status": "compared", "candidate_pdf_ordinal": index + 1,
                               "changed_cell_count": score["changed_cell_count"],
                               "changed_cell_fraction": score["changed_cell_fraction"]})
    if any(p["status"] != "compared" for p in before + after):
        return {"status": "inconclusive_media_extent_drift", "before": before, "after": after}
    return {"status": "raster_screened_reference_stage_unknown" if stage == "unknown" else "raster_screened_logical_reference",
            "before": before, "after": after}


def summarize(rows: list[dict]) -> dict:
    improved, regressed, unchanged, changed_pages = [], [], 0, 0
    mismatched, ineligible = [], []
    seen_source = set()
    before_cells = after_cells = 0
    for row in rows:
        source = row["pub_sha256"]
        if source in seen_source:
            raise ValueError("duplicate source in A/B receipt")
        seen_source.add(source)
        status = row["status"]
        if not status.startswith("raster_screened_"):
            ineligible.append({"fixture": row["fixture"], "status": status})
            continue
        before, after = row["before"], row["after"]
        if len(before) != len(after) or not all(p["status"] == "compared" for p in before + after):
            mismatched.append({"fixture": row["fixture"], "status": "coverage_drift"})
            continue
        for ordinal, (b, a) in enumerate(zip(before, after), start=1):
            before_cells += b["changed_cell_count"]
            after_cells += a["changed_cell_count"]
            if b["candidate_pdf_ordinal"] != a["candidate_pdf_ordinal"]:
                changed_pages += 1
            change = b["changed_cell_count"] - a["changed_cell_count"]
            rec = {"fixture": row["fixture"], "page": ordinal,
                   "before_changed_cells": b["changed_cell_count"], "after_changed_cells": a["changed_cell_count"],
                   "change_cells": change, "previous_pdf_page": b["candidate_pdf_ordinal"],
                   "viewer_order_pdf_page": a["candidate_pdf_ordinal"]}
            if change > 0:
                improved.append(rec)
            elif change < 0:
                regressed.append(rec)
            else:
                unchanged += 1
    total = len(improved) + len(regressed) + unchanged
    improved.sort(key=lambda r: (-r["change_cells"], r["fixture"], r["page"]))
    regressed.sort(key=lambda r: (r["change_cells"], r["fixture"], r["page"]))
    if mismatched:
        verdict = "inconclusive_coverage_drift"
    elif regressed:
        verdict = "regressions_under_screening"
    elif improved:
        verdict = "nonregressing_screening_improvement"
    else:
        verdict = "no_screened_change"
    return {"schema": REPORT_SCHEMA, "verdict": verdict, "source_pairs_examined": len(rows),
            "pairs_screened": sum(row["status"].startswith("raster_screened_") for row in rows),
            "matched_pages": total, "viewer_order_changed_positions": changed_pages,
            "improved_pages": len(improved), "regressed_pages": len(regressed), "unchanged_pages": unchanged,
            "mean_changed_cell_fraction_before": before_cells / (4096 * total) if total else None,
            "mean_changed_cell_fraction_viewer_order": after_cells / (4096 * total) if total else None,
            "net_improved_cells": before_cells - after_cells,
            "worst_regressions": regressed[:20], "largest_improvements": improved[:20],
            "ineligible_pairs": ineligible[:40], "coverage_drift_pairs": mismatched[:40],
            "claims": {"intervention_is_page_reindex_only": True,
                       "same_candidate_pdf_bytes_for_both_arms": True,
                       "source_derived_permutation_not_raster_inferred": True,
                       "reference_stage_unknown_not_semantic_authority": True,
                       "no_pub_pdf_font_or_source_bytes_emitted": True,
                       "screening_is_not_product_acceptance": True}}


def run(*, orders: dict[str, dict], pairs: list[dict], source_root: Path, cli: Path,
        font: Path, timeout: int) -> dict:
    from pub_pdf_cli_publisher_oracle_v1 import build_source_index, convert, sha256
    if not cli.is_file() or not font.is_file():
        raise ValueError("CLI or fallback font missing")
    hosted = [pair for pair in pairs if pair.get("pub_sha256") in orders]
    if len(hosted) != 79 or {p["pub_sha256"] for p in hosted} != set(orders):
        raise ValueError("source registry and page-order corpus do not match")
    sources = build_source_index(source_root)
    rows = []
    with tempfile.TemporaryDirectory(prefix="pub-pdf-page-order-ab-") as tmp:
        root = Path(tmp)
        for index, pair in enumerate(sorted(hosted, key=lambda p: p["pub_sha256"])):
            sha = pair["pub_sha256"]
            source_paths = sources.get(sha, [])
            if len(source_paths) != 1 or sha256(source_paths[0]) != sha:
                raise ValueError("hosted pinned source missing or source SHA mismatch")
            order = orders[sha]
            if order["page_count"] != pair["reference_pages"]:
                # Known 029/033/083/086/095 surface and membership mismatches
                # must not be interpreted as reorderable logical pages.
                rows.append({"fixture": pair["basename"], "pub_sha256": sha,
                             "route": order["route"], "reference_surface_stage": pair.get("reference_surface_stage", "unknown"),
                             "source_order_changed": order["order_changed"],
                             "changed_position_count": order["changed_position_count"],
                             "status": "source_reference_cardinality_mismatch", "before": [], "after": []})
                continue
            output = root / f"pdf-{index:02d}.pdf"
            status, _ = convert(cli, source_paths[0], font, output, timeout)
            result = {"fixture": pair["basename"], "pub_sha256": sha,
                      "route": order["route"], "reference_surface_stage": pair.get("reference_surface_stage", "unknown"),
                      "source_order_changed": order["order_changed"],
                      "changed_position_count": order["changed_position_count"],
                      "status": status, "before": [], "after": []}
            if status == "ok":
                measured = compare_virtual_reorder(output, pair, order["canonical_pdf_order_as_viewer_ordinals"])
                result.update(measured)
            rows.append(result)
            for suffix in ("", ".loss.json", ".loss.txt"):
                (root / (output.name + suffix)).unlink(missing_ok=True)
    summary = summarize(rows)
    summary["source_registry_count"] = len(orders)
    summary["pair_status_counts"] = dict(sorted(Counter(r["status"] for r in rows).items()))
    summary["highlight_pairs"] = [
        {"fixture": row["fixture"], "status": row["status"],
         "source_order_changed": row["source_order_changed"],
         "changed_position_count": row["changed_position_count"],
         "improved_pages": sum(b.get("changed_cell_count", 0) > a.get("changed_cell_count", 0) for b, a in zip(row["before"], row["after"])),
         "regressed_pages": sum(b.get("changed_cell_count", 0) < a.get("changed_cell_count", 0) for b, a in zip(row["before"], row["after"])),
         "net_improved_cells": sum(b.get("changed_cell_count", 0) - a.get("changed_cell_count", 0) for b, a in zip(row["before"], row["after"]))}
        for row in rows if row["fixture"].startswith(("075_", "027_", "094_", "029_", "041_"))]
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--order-receipt", required=True, type=Path)
    parser.add_argument("--batch01", required=True, type=Path)
    parser.add_argument("--supplemental", required=True, type=Path)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--fallback-font", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args()
    from pub_pdf_cli_publisher_oracle_v1 import load_references
    orders = validate_order_census(json.loads(args.order_receipt.read_text(encoding="utf-8")))
    pairs = load_references(args.batch01, args.supplemental)
    result = run(orders=orders, pairs=pairs, source_root=args.source_root,
                 cli=args.cli, font=args.fallback_font, timeout=args.timeout)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: result[key] for key in (
        "verdict", "source_registry_count", "pairs_screened", "matched_pages", "improved_pages",
        "regressed_pages", "mean_changed_cell_fraction_before", "mean_changed_cell_fraction_viewer_order")}, sort_keys=True))


if __name__ == "__main__":
    main()
