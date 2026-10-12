#!/usr/bin/env python3
"""Compare #2622 and a proposed PAGE-LOGO rectangle fix on exact customer p4.

Reference: immutable source-safe LO PNG from merged #2639 run 38222584563,
NOT a native Publisher PDF. Prove isolation of the change to customer page 4.
"""

import argparse
import hashlib
import json
from pathlib import Path

from cloud_reader_reference_pdf_raster_v1 import render_png
from pdf_reference_diff_v1 import compare_rasters
from reader_logo_0x31_visual_ab_v1 import checked_capture

BASE_COMMIT = "d6db6c917c11350b9a4a0f4536b503776ac4e8cb"
REFERENCE_PAGE4_SHA256 = "32d9e411d93b3f50b758e9dc2f5617be4f05a7041d99ad35784b98b8c30b1a52"
BASELINE_PAGE4_SHA256 = "f00814c15054fd26ef826741e291bb8b3ecd24ff3b8dfde3a5ed5e573665be72"
ORIGINAL_LO_REFERENCE_PDF_SHA256 = (
    "ad7fac30713ba47279ed305cae62ab551c4f71f6dbc495ca131ef187a3856812"
)


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(baseline: Path, candidate: Path, reference_page4: Path, commit: str) -> dict:
    if len(commit) != 40 or any(c not in "0123456789abcdef" for c in commit):
        raise ValueError("not an exact lowercase commit SHA")
    if sha256_file(reference_page4) != REFERENCE_PAGE4_SHA256:
        raise ValueError("LibreOffice reference raster identity mismatch")

    b = checked_capture(baseline, BASE_COMMIT)
    c = checked_capture(candidate, commit)
    if b["page_ids"] != c["page_ids"]:
        raise ValueError("source customer PAGE identity/order changed")
    old_pngs = [baseline.parent / x["filename"] for x in b["fixture"]["screenshots"]]
    new_pngs = [candidate.parent / x["filename"] for x in c["fixture"]["screenshots"]]
    if sha256_file(old_pngs[3]) != BASELINE_PAGE4_SHA256:
        raise ValueError("pinned previously measured pre-rectangle PNG drift")

    per_page = []
    for n in range(4):
        diff = compare_rasters(render_png(old_pngs[n]), render_png(new_pngs[n]))
        if not diff["raster_size_match"]:
            raise ValueError("changed pixel dimensions of exact captured page")
        per_page.append({
            "customer_page": n + 1,
            "direct_significant_pixel_count": diff["significant_pixel_count"],
            "direct_significant_fraction": diff["significant_fraction"],
            "direct_bbox": diff["significant_bbox"],
            "png_identical": sha256_file(old_pngs[n]) == sha256_file(new_pngs[n]),
        })
    lo = render_png(reference_page4)
    before = compare_rasters(render_png(old_pngs[3]), lo)
    after = compare_rasters(render_png(new_pngs[3]), lo)
    if not before["raster_size_match"] or not after["raster_size_match"]:
        raise ValueError("reference p4 is not physically pixel comparable")

    new_reference_difference = after["significant_pixel_count"] - before["significant_pixel_count"]
    return {
        "schema": "chaptera.reader-logo-effective-rect-visual-ab.v1",
        "source_pub_sha256": b["fixture"]["source_sha256"],
        "before_commit": BASE_COMMIT,
        "after_commit": commit,
        "captured_customer_page_ids_identical": True,
        "reference": {
            "kind": "libreoffice-import-export-proxy",
            "provenance_run_id": 38222584563,
            "reference_pdf_sha256": ORIGINAL_LO_REFERENCE_PDF_SHA256,
            "reference_p4_png_sha256": REFERENCE_PAGE4_SHA256,
            "original_lo_version": "LibreOffice 24.2.7.2",
            "native_publisher_parity_proven": False,
        },
        "pages": per_page,
        "page4_before_mismatch_pixels_vs_lo": before["significant_pixel_count"],
        "page4_after_mismatch_pixels_vs_lo": after["significant_pixel_count"],
        "page4_before_significant_fraction_vs_lo": before["significant_fraction"],
        "page4_after_significant_fraction_vs_lo": after["significant_fraction"],
        "page4_mismatch_pixel_delta": new_reference_difference,
        "state": (
            "improved_against_libreoffice_proxy" if new_reference_difference < 0
            else "unchanged_against_libreoffice_proxy" if new_reference_difference == 0
            else "regressed_against_libreoffice_proxy"
        ),
        "other_customer_pages_byte_identical": all(p["png_identical"] for p in per_page[:3]),
        "limitations": [
            "Independent LibreOffice conversion is not a native Publisher reference.",
            "This control uses one immutable previously measured PNG and checks exact SHA256.",
            "Pixel improvements do not independently establish Publisher editing or export fidelity.",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--reference-page4", type=Path, required=True)
    parser.add_argument("--candidate-commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = compare(
        args.baseline, args.candidate, args.reference_page4, args.candidate_commit
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(
        f"LOGO_RECT_P4: before_diff={report['page4_before_mismatch_pixels_vs_lo']} "
        f"after_diff={report['page4_after_mismatch_pixels_vs_lo']} "
        f"delta={report['page4_mismatch_pixel_delta']} status={report['state']}"
    )
    if not report["other_customer_pages_byte_identical"]:
        raise SystemExit("PAGE-LOGO visual scope regression: unrelated pages changed")
    if report["page4_mismatch_pixel_delta"] >= 0:
        raise SystemExit("PAGE-LOGO did not improve against pinned independent LO proxy")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
