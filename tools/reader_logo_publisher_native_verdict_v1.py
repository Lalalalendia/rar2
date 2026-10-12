#!/usr/bin/env python3
"""Native-Publisher-first A/B for one exact public SampleNewsletter source.

LibreOffice is *not* the golden for this test. Requires the already existing
external ten-PUB/twenty-page Microsoft Publisher 2019 manual oracle ZIP, which
remains outside GitHub. Source PUB and PDF bytes are NEVER copied into report.
"""
from __future__ import annotations

import argparse
import json
import tempfile
from pathlib import Path

from cloud_reader_manual_oracle_bundle_v1 import (
    file_sha256, load_pairs, materialize_bundle,
)
from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters
from pdf_reference_diff_v1 import compare_rasters
from cloud_reader_reference_pdf_raster_v1 import render_png
from reader_logo_0x31_visual_ab_v1 import checked_capture, SOURCE_SHA256, DPI, PAGE_COUNT

SCHEMA = "chaptera.reader-logo-publisher-native-visual-ab.v1"
NATIVE_ORACLE_BUNDLE_SHA256 = "b4465c296282bcaba0d3b36e865fa302b99b0c6135bbaa9846f969e8aa8388ed"
BASE_COMMIT_SHA = "33ee33d239c50cd8488579c37dc81679f8c04afa"
PRODUCT_COMMIT_SHA = "2777a574e9eac1ccd1d24c3054f00f9dbe133f2a"


def require_exact_publisher_bundle(bundle: Path, actual_sha256: str) -> None:
    if not bundle.is_file() or bundle.suffix.lower() != ".zip":
        raise ValueError("native Publisher oracle must be the pinned external ZIP")
    if actual_sha256 != NATIVE_ORACLE_BUNDLE_SHA256:
        raise ValueError("native Publisher oracle ZIP SHA256 mismatch")


def select_newsletter(pairs: list[dict]) -> dict:
    if len(pairs) != 10 or sum(p["pdf_pages"] for p in pairs) != 20:
        raise ValueError("the Publisher manual oracle must contain exactly 10 pairs and 20 reference pages")
    rows = [p for p in pairs if p["basename"] == "SampleNewsletter"]
    if len(rows) != 1:
        raise ValueError("Publisher oracle lacks exactly one SampleNewsletter pair")
    reference = rows[0]
    if (
        reference["pub_sha256"] != SOURCE_SHA256
        or reference["pub_bytes"] != 291840
        or reference["pdf_pages"] != PAGE_COUNT
    ):
        raise ValueError("Publisher oracle PUB source/page identity mismatch")
    return reference


def disposition(before: int, after: int, direct_change: int) -> str:
    if direct_change == 0:
        return "no_visible_effect"
    if after < before:
        return "closer_to_native_publisher"
    if after > before:
        return "farther_from_native_publisher"
    return "equal_native_distance"


def evaluate(base_receipt: Path, candidate_receipt: Path, bundle: Path) -> dict:
    require_exact_publisher_bundle(bundle, file_sha256(bundle))
    b = checked_capture(base_receipt, BASE_COMMIT_SHA)
    a = checked_capture(candidate_receipt, PRODUCT_COMMIT_SHA)
    if b["page_ids"] != a["page_ids"]:
        raise ValueError("source customer PAGE identity or order drift")
    if a["fixture"]["nodes"] != b["fixture"]["nodes"] + 2:
        raise ValueError("the exact candidate must materialize two additional nodes")

    with tempfile.TemporaryDirectory(prefix="publisher-native-oracle-") as temp:
        root = materialize_bundle(bundle, Path(temp))
        reference = select_newsletter(load_pairs(root))
        pdf = Path(reference["pdf_path"])
        before = compare_cloud_rasters(base_receipt, "SampleNewsletter", pdf)
        after = compare_cloud_rasters(candidate_receipt, "SampleNewsletter", pdf)
        if before["comparison_state"] != "compared" or after["comparison_state"] != "compared":
            raise ValueError("native Publisher/Reader physical raster comparison not complete")
        if before["reference_sha256"] != after["reference_sha256"]:
            raise ValueError("native PDF changed between Reader comparisons")
        if before["reference_sha256"] != reference["pdf_sha256"]:
            raise ValueError("native reference source hash mismatch")
        pages = []
        for index in range(PAGE_COUNT):
            bp = before["pages"][index]
            ap = after["pages"][index]
            im_b = base_receipt.parent / b["fixture"]["screenshots"][index]["filename"]
            im_a = candidate_receipt.parent / a["fixture"]["screenshots"][index]["filename"]
            direct = compare_rasters(render_png(im_b), render_png(im_a))
            if not (
                bp["physical_page_size_matches_reference"]
                and ap["physical_page_size_matches_reference"]
                and bp["diff"]["raster_size_match"]
                and ap["diff"]["raster_size_match"]
                and direct["raster_size_match"]
            ):
                raise ValueError(f"native Publisher page {index + 1} extent differs or is unresolved")
            change = direct["significant_pixel_count"]
            if index < 3 and change:
                raise ValueError(f"non-target page {index + 1} regressed by {change} pixels")
            if index == 3 and not change:
                raise ValueError("PAGE381 candidate does not produce visible raster changes")
            pages.append({
                "customer_page": index + 1,
                "source_page_identity_sha256": b["page_ids"][index],
                "baseline_significant_fraction_vs_publisher": bp["diff"]["significant_fraction"],
                "candidate_significant_fraction_vs_publisher": ap["diff"]["significant_fraction"],
                "baseline_significant_pixels_vs_publisher": bp["diff"]["significant_pixel_count"],
                "candidate_significant_pixels_vs_publisher": ap["diff"]["significant_pixel_count"],
                "direct_changed_significant_pixels": change,
                "direct_changed_significant_fraction": direct["significant_fraction"],
            })

        p4 = pages[3]
        return {
            "schema": SCHEMA,
            "fixture": "SampleNewsletter",
            "source_pub_sha256": SOURCE_SHA256,
            "baseline_commit": BASE_COMMIT_SHA,
            "candidate_commit": PRODUCT_COMMIT_SHA,
            "browser_raster_dpi": DPI,
            "publisher_oracle": {
                "kind": "external_exact_manual_publisher2019_bundle",
                "bundle_sha256": NATIVE_ORACLE_BUNDLE_SHA256,
                "source_pair_oracle_id": reference["oracle_id"],
                "reference_pdf_sha256": reference["pdf_sha256"],
                "reference_page_count": reference["pdf_pages"],
                "publisher_exact_build_independently_verified": False,
                "reference_surface_stage_native_export_receipt": "not_separately_verified",
            },
            "pages": pages,
            "page4_disposition": disposition(
                p4["baseline_significant_pixels_vs_publisher"],
                p4["candidate_significant_pixels_vs_publisher"],
                p4["direct_changed_significant_pixels"],
            ),
            "claims": {
                "external_publisher_oracle_zip_sha_verified": True,
                "per_pair_pub_and_pdf_identity_verified": True,
                "all_four_customer_page_source_ids_unchanged": True,
                "libreoffice_used_as_reference": False,
                "publisher_pixels_authoritative_for_visual_comparison_only": True,
                "publisher_pixels_not_pub_semantic_authority": True,
                "raw_pub_or_pdf_bytes_emitted": False,
                "native_pixel_exact_parity_proven": False,
            },
        }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--publisher-bundle", type=Path, required=True)
    parser.add_argument("--baseline-receipt", type=Path, required=True)
    parser.add_argument("--candidate-receipt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = evaluate(args.baseline_receipt, args.candidate_receipt, args.publisher_bundle)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    p4 = result["pages"][3]
    print(
        "PUBLISHER_NATIVE_PAGE4 "
        f"before={p4['baseline_significant_fraction_vs_publisher']:.8f} "
        f"after={p4['candidate_significant_fraction_vs_publisher']:.8f} "
        f"state={result['page4_disposition']}"
    )


if __name__ == "__main__":
    main()
