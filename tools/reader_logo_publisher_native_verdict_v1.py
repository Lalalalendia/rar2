#!/usr/bin/env python3
"""Compare exact Reader browser A/B to the external PDF-only Publisher 2019 oracle.

The 2026-10-01 archived SHA-pinned ZIP contains ten Publisher-exported PDFs and
NO PUB files, PAIRS.csv, or built-in PUB-to-PDF cryptographic linkage. PUB source
identity is verified independently from both Reader capture receipts. Treat
Microsoft Publisher PDF pixels as VISUAL authority, never hidden PUB semantics.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

import fitz

from cloud_reader_manual_oracle_bundle_v1 import file_sha256
from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters, render_png
from pdf_reference_diff_v1 import compare_rasters
from reader_logo_0x31_visual_ab_v1 import checked_capture, SOURCE_SHA256, DPI, PAGE_COUNT

SCHEMA = "chaptera.reader-logo-publisher-native-visual-ab.v2"
NATIVE_ORACLE_BUNDLE_SHA256 = "b4465c296282bcaba0d3b36e865fa302b99b0c6135bbaa9846f969e8aa8388ed"
BASE_COMMIT_SHA = "33ee33d239c50cd8488579c37dc81679f8c04afa"
PRODUCT_COMMIT_SHA = "2777a574e9eac1ccd1d24c3054f00f9dbe133f2a"
SHA40 = re.compile(r"^[0-9a-f]{40}$")
# Exact census and named page counts from the already executed Publisher 2019
# manual oracle receipt (Notion CLOUD-READER-MANUAL-ORACLE-BUNDLE-01).
REFERENCE_PAGES = {
    "SampleNewsletter": 4,
    "SampleBrochure": 2,
    "Sample98": 2,
    "Sample2000": 2,
    "Sample_2010": 2,
    "Sample2_2010": 2,
    "Sample3_2010": 2,
    "Sample4_2010": 2,
    "51318": 1,
    "60685": 1,
}
MAX_PDF_BYTES = 64 * 1024 * 1024
MAX_TOTAL_BYTES = 256 * 1024 * 1024


def require_exact_publisher_bundle(bundle: Path) -> None:
    if not bundle.is_file() or bundle.suffix.lower() != ".zip" or not zipfile.is_zipfile(bundle):
        raise ValueError("native Publisher oracle must be an existing ZIP")
    if file_sha256(bundle) != NATIVE_ORACLE_BUNDLE_SHA256:
        raise ValueError("native Publisher PDF-only ZIP SHA256 mismatch")


def inspect_pdf_only_bundle(bundle: Path) -> dict:
    """Validate safe PDF-only census; return only the Newsletter PDF bytes in memory.

    Call require_exact_publisher_bundle() FIRST for the actual external oracle.
    Synthetic archives may call this helper in negative/contract unit tests.
    """
    with zipfile.ZipFile(bundle) as archive:
        members = [member for member in archive.infolist() if not member.is_dir()]
        if len(members) != len(REFERENCE_PAGES):
            raise ValueError("Publisher oracle must contain exactly ten PDFs, no PUB or PAIRS.csv")
        total = 0
        found = {}
        newsletter_bytes = None
        for member in members:
            name = PurePosixPath(member.filename)
            mode = (member.external_attr >> 16) & 0xFFFF
            if (
                name.is_absolute()
                or ".." in name.parts
                or "\\" in member.filename
                or stat.S_ISLNK(mode)
                or member.flag_bits & 0x1
                or member.file_size <= 0
                or member.file_size > MAX_PDF_BYTES
            ):
                raise ValueError("unsafe, encrypted, empty or oversized PDF member")
            total += member.file_size
            if total > MAX_TOTAL_BYTES:
                raise ValueError("Publisher oracle exceeds PDF byte budget")
            if name.suffix.lower() != ".pdf" or name.stem not in REFERENCE_PAGES:
                raise ValueError(f"unexpected non-PDF or reference basename: {name.name}")
            if name.stem in found:
                raise ValueError(f"duplicate Publisher PDF basename: {name.stem}")
            # The archive-wide SHA is the pinned PDF authority. Verify each
            # document is structurally readable and has exactly the source
            # receipt's declared page count.
            content = archive.read(member)
            if len(content) != member.file_size:
                raise ValueError(f"ZIP member length mismatch: {name.name}")
            with fitz.open(stream=content, filetype="pdf") as document:
                pages = document.page_count
            if pages != REFERENCE_PAGES[name.stem]:
                raise ValueError(f"Publisher PDF page count drift: {name.name}")
            found[name.stem] = {
                "pdf_sha256": hashlib.sha256(content).hexdigest(),
                "pdf_pages": pages,
                "pdf_bytes": len(content),
            }
            if name.stem == "SampleNewsletter":
                newsletter_bytes = content

    if set(found) != set(REFERENCE_PAGES) or newsletter_bytes is None:
        raise ValueError("Publisher PDF-only ten-document census mismatch")
    if sum(item["pdf_pages"] for item in found.values()) != 20:
        raise ValueError("Publisher oracle does not contain exactly 20 reference pages")
    return {
        "files": found,
        "pdf_count": len(found),
        "reference_pages": 20,
        "newsletter_pdf_bytes": newsletter_bytes,
    }


def disposition(before: int, after: int, direct_change: int) -> str:
    if direct_change == 0:
        return "no_visible_effect"
    if after < before:
        return "closer_to_native_publisher"
    if after > before:
        return "farther_from_native_publisher"
    return "equal_native_distance"


def evaluate(
    base_receipt: Path,
    candidate_receipt: Path,
    bundle: Path,
    *,
    baseline_commit: str = BASE_COMMIT_SHA,
    candidate_commit: str = PRODUCT_COMMIT_SHA,
    expected_added_nodes: int = 2,
) -> dict:
    if not SHA40.fullmatch(baseline_commit) or not SHA40.fullmatch(candidate_commit):
        raise ValueError("both Reader commits must be exact 40-hex SHA pins")
    if baseline_commit == candidate_commit or not 1 <= expected_added_nodes <= 10:
        raise ValueError("A/B needs distinct commits and bounded positive node delta")
    require_exact_publisher_bundle(bundle)
    b = checked_capture(base_receipt, baseline_commit)
    a = checked_capture(candidate_receipt, candidate_commit)
    if b["page_ids"] != a["page_ids"]:
        raise ValueError("source customer PAGE identity or order drift")
    if a["fixture"]["nodes"] != b["fixture"]["nodes"] + expected_added_nodes:
        raise ValueError("expected exact node-count change not observed")

    with tempfile.TemporaryDirectory(prefix="publisher-native-oracle-") as temporary:
        scanned = inspect_pdf_only_bundle(bundle)
        reference = scanned["files"]["SampleNewsletter"]
        pdf = Path(temporary) / "SampleNewsletter.pdf"
        pdf.write_bytes(scanned["newsletter_pdf_bytes"])
        if file_sha256(pdf) != reference["pdf_sha256"]:
            raise ValueError("native PDF materialization SHA mismatch")
        before = compare_cloud_rasters(base_receipt, "SampleNewsletter", pdf)
        after = compare_cloud_rasters(candidate_receipt, "SampleNewsletter", pdf)
        if before["comparison_state"] != "compared" or after["comparison_state"] != "compared":
            raise ValueError("native Publisher/Reader physical raster comparison not complete")
        if before["reference_sha256"] != after["reference_sha256"]:
            raise ValueError("native PDF changed between Reader comparisons")
        if before["reference_sha256"] != reference["pdf_sha256"]:
            raise ValueError("native PDF identity mismatch")
        if before["page_count"]["match"] is not True or after["page_count"]["match"] is not True:
            raise ValueError("Publisher PDF vs Reader page-count mismatch")
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
                raise ValueError(f"native Publisher page {index + 1} extent differs or unresolved")
            change = direct["significant_pixel_count"]
            if index < 3 and change:
                raise ValueError(f"non-target page {index + 1} changed by {change} pixels")
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
            "baseline_commit": baseline_commit,
            "candidate_commit": candidate_commit,
            "browser_raster_dpi": DPI,
            "publisher_oracle": {
                "kind": "external_pdf_only_publisher2019_10pdf_20page_archive",
                "bundle_sha256": NATIVE_ORACLE_BUNDLE_SHA256,
                "reference_pdf_sha256": reference["pdf_sha256"],
                "reference_page_count": reference["pdf_pages"],
                "archive_pdf_count": scanned["pdf_count"],
                "archive_reference_page_count": scanned["reference_pages"],
                "pub_bytes_in_archive": False,
                "source_to_pdf_pair_identity": "external_2026-10-01_manual_oracle_attestation",
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
                "external_publisher_pdf_only_zip_sha_verified": True,
                "per_pdf_sha_derived_from_pinned_zip": True,
                "pub_source_identity_checked_from_both_reader_receipts": True,
                "per_pair_pub_and_pdf_identity_inside_zip_verified": False,
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
    parser.add_argument("--baseline-commit", default=BASE_COMMIT_SHA)
    parser.add_argument("--candidate-commit", default=PRODUCT_COMMIT_SHA)
    parser.add_argument("--expected-added-nodes", type=int, default=2)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = evaluate(
        args.baseline_receipt, args.candidate_receipt, args.publisher_bundle,
        baseline_commit=args.baseline_commit,
        candidate_commit=args.candidate_commit,
        expected_added_nodes=args.expected_added_nodes,
    )
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
