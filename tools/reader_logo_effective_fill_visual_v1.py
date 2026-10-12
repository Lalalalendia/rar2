#!/usr/bin/env python3
"""Measure exactly the #2644 candidate LOGO fill against its exact base and LO.

Independent LibreOffice PDF is a visual proxy, NOT a native Publisher golden.
No PUB bytes or source Story text are present in the receipt.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import fitz

from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters, render_png
from pdf_reference_diff_v1 import compare_rasters, file_sha256
from reader_logo_0x31_visual_ab_v1 import (
    DPI,
    PAGE_COUNT,
    SOURCE_SHA256,
    checked_capture,
)

BASE_SHA = "33ee33d239c50cd8488579c37dc81679f8c04afa"
CANDIDATE_SHA = "2777a574e9eac1ccd1d24c3054f00f9dbe133f2a"


def evaluate(before: Path, after: Path, reference: Path, lo_version: str) -> dict:
    source = checked_capture(before, BASE_SHA)
    candidate = checked_capture(after, CANDIDATE_SHA)
    if source["page_ids"] != candidate["page_ids"]:
        raise ValueError("source PAGE identity/order drift")
    if candidate["fixture"]["nodes"] != source["fixture"]["nodes"] + 2:
        raise ValueError("new PAGE LOGO fill must add exactly two Scene nodes")

    with fitz.open(reference) as pdf:
        if pdf.page_count != PAGE_COUNT:
            raise ValueError(f"LibreOffice reference pages {pdf.page_count} != {PAGE_COUNT}")
    b = compare_cloud_rasters(before, "SampleNewsletter", reference)
    a = compare_cloud_rasters(after, "SampleNewsletter", reference)
    if b["comparison_state"] != "compared" or a["comparison_state"] != "compared":
        raise ValueError("Cloud Reader reference rendering incomplete")
    if b["reference_sha256"] != a["reference_sha256"]:
        raise ValueError("reference identity drift")
    pages = []
    for index in range(PAGE_COUNT):
        b_page, a_page = b["pages"][index], a["pages"][index]
        b_image = before.parent / source["fixture"]["screenshots"][index]["filename"]
        a_image = after.parent / candidate["fixture"]["screenshots"][index]["filename"]
        direct = compare_rasters(render_png(b_image), render_png(a_image))
        bounded = (
            b_page["physical_page_size_matches_reference"]
            and a_page["physical_page_size_matches_reference"]
            and b_page["diff"]["raster_size_match"]
            and a_page["diff"]["raster_size_match"]
            and direct["raster_size_match"]
        )
        if not bounded:
            raise ValueError(f"customer page {index + 1} cannot be compared at identical physical extent")
        before_fraction = b_page["diff"]["significant_fraction"]
        after_fraction = a_page["diff"]["significant_fraction"]
        changed = direct["significant_pixel_count"]
        if index < 3 and changed != 0:
            raise ValueError(f"non-target customer page {index + 1} changed {changed} pixels")
        if index == 3 and changed <= 0:
            raise ValueError("two admitted LOGO rectangles produced no visible pixel change")
        pages.append({
            "customer_page": index + 1,
            "source_page_identity_sha256": source["page_ids"][index],
            "significant_pixel_count_baseline_vs_lo": b_page["diff"]["significant_pixel_count"],
            "significant_pixel_count_candidate_vs_lo": a_page["diff"]["significant_pixel_count"],
            "significant_fraction_baseline_vs_lo": before_fraction,
            "significant_fraction_candidate_vs_lo": after_fraction,
            "delta_significant_fraction": round(after_fraction - before_fraction, 10),
            "direct_changed_significant_pixels": changed,
            "direct_changed_significant_fraction": direct["significant_fraction"],
            "direct_change_bbox": direct["significant_bbox"],
        })

    p4 = pages[-1]
    return {
        "schema": "chaptera.reader-logo-effective-fill-visual-ab.v1",
        "source_pub_sha256": SOURCE_SHA256,
        "baseline_commit_sha": BASE_SHA,
        "candidate_commit_sha": CANDIDATE_SHA,
        "reader_backend": "cloud-browser-svg",
        "dpi": DPI,
        "browser_versions": {
            "baseline": source["receipt"].get("browser"),
            "candidate": candidate["receipt"].get("browser"),
        },
        "lo_proxy": {
            "version": lo_version.strip(),
            "pdf_sha256": file_sha256(reference),
            "page_count": PAGE_COUNT,
        },
        "scenes": {
            "nodes_baseline": source["fixture"]["nodes"],
            "nodes_candidate": candidate["fixture"]["nodes"],
            "node_delta": 2,
        },
        "pages": pages,
        "page4_state_vs_lo": (
            "closer_to_libreoffice"
            if p4["delta_significant_fraction"] < 0
            else "farther_from_libreoffice"
            if p4["delta_significant_fraction"] > 0
            else "equal_reference_distance"
        ),
        "claims": {
            "same_exact_source_sha": True,
            "source_page_order_and_identity_match": True,
            "only_customer_page4_changed": True,
            "native_publisher_parity_proven": False,
            "libreoffice_is_only_a_proxy": True,
            "materialized_nodes_prove_visual_correctness": False,
            "no_raw_pub_or_story_text_emitted": True,
        },
    }


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--baseline", type=Path, required=True)
    p.add_argument("--candidate", type=Path, required=True)
    p.add_argument("--reference", type=Path, required=True)
    p.add_argument("--lo-version-file", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    result = evaluate(
        args.baseline, args.candidate, args.reference,
        args.lo_version_file.read_text(encoding="utf-8"),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    for item in result["pages"]:
        print(
            f"customer_p{item['customer_page']} "
            f"baseline_vs_LO={item['significant_fraction_baseline_vs_lo']:.8f} "
            f"candidate_vs_LO={item['significant_fraction_candidate_vs_lo']:.8f} "
            f"direct_changed_pixels={item['direct_changed_significant_pixels']}"
        )
    print("PAGE4_LO_PROXY_DISPOSITION=" + result["page4_state_vs_lo"])


if __name__ == "__main__":
    main()
