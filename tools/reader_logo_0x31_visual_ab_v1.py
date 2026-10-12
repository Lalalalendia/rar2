#!/usr/bin/env python3
"""Source-safe, exact-commit SampleNewsletter LOGO 0x31 visual A/B vs LibreOffice.

The independent reference is a LibreOffice import/export PDF, NOT a native
Microsoft Publisher golden. A lower delta is LO-proxy improvement only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

SOURCE_SHA256 = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
BASE_SHA = "586441154179e11f88f39654f7847a89823fa606"  # merged #2619, pre-fix
FIX_SHA = "d6db6c917c11350b9a4a0f4536b503776ac4e8cb"  # merged #2622
PAGE_COUNT = 4
DPI = 144


def checked_capture(path: Path, expected_commit: str) -> dict:
    receipt = json.loads(path.read_text(encoding="utf-8"))
    if receipt.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError("unexpected Cloud Reader browser receipt protocol")
    if receipt.get("repository_commit_sha") != expected_commit:
        raise ValueError("Reader binary / source commit identity mismatch")
    rows = [row for row in receipt.get("results", []) if row.get("fixture") == "SampleNewsletter"]
    if len(rows) != 1:
        raise ValueError("expected exactly one SampleNewsletter capture")
    fixture = rows[0]
    if fixture.get("source_sha256") != SOURCE_SHA256 or fixture.get("source_byte_len") != 291840:
        raise ValueError("exact source PUB SHA-256 / byte length mismatch")
    if fixture.get("rendered") is not True or fixture.get("pages") != PAGE_COUNT:
        raise ValueError("Reader did not render all four customer pages")
    if fixture.get("reference_raster_dpi") != DPI:
        raise ValueError("Reader capture is not 144 DPI")
    if fixture.get("browser_preserved_scene_node_order") is not True:
        raise ValueError("Reader browser did not preserve exact Scene node order")
    geometry = fixture.get("page_geometry")
    screenshots = fixture.get("screenshots")
    if not isinstance(geometry, list) or len(geometry) != PAGE_COUNT:
        raise ValueError("missing customer PAGE identity / geometry")
    if not isinstance(screenshots, list) or len(screenshots) != PAGE_COUNT:
        raise ValueError("incomplete Page screenshots")
    page_ids = []
    for index, (page, shot) in enumerate(zip(geometry, screenshots), 1):
        if page.get("order") != index - 1:
            raise ValueError("customer PAGE order is not zero-based and contiguous")
        identity = page.get("page_identity_sha256")
        if not isinstance(identity, str) or len(identity) != 64:
            raise ValueError("missing canonical source PAGE identity")
        if not all(ch in "0123456789abcdef" for ch in identity):
            raise ValueError("noncanonical source PAGE identity digest")
        if shot.get("page") != index or shot.get("filename") != f"SampleNewsletter-page-{index}.png":
            raise ValueError("unexpected PAGE capture order or filename")
        png = path.parent / shot["filename"]
        if not png.is_file() or hashlib.sha256(png.read_bytes()).hexdigest() != shot.get("sha256"):
            raise ValueError("missing or mutated exact PAGE PNG")
        page_ids.append(identity)
    if len(set(page_ids)) != PAGE_COUNT:
        raise ValueError("duplicate customer PAGE identity")
    return {"fixture": fixture, "page_ids": page_ids, "receipt": receipt}


def classify_visual_delta(before: float | None, after: float | None, changed: float | None) -> str:
    if before is None or after is None or changed is None:
        return "not_comparable"
    if changed == 0:
        return "no_visible_pixel_change"
    if after < before:
        return "closer_to_libreoffice"
    if after > before:
        return "farther_from_libreoffice"
    return "visible_change_equal_reference_distance"


def measure(before_path: Path, after_path: Path, lo_pdf: Path, lo_version: str) -> dict:
    import fitz
    from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters, render_png
    from pdf_reference_diff_v1 import compare_rasters, file_sha256

    before_capture = checked_capture(before_path, BASE_SHA)
    after_capture = checked_capture(after_path, FIX_SHA)
    if before_capture["page_ids"] != after_capture["page_ids"]:
        raise ValueError("customer PAGE source identity/order drift across exact commits")

    with fitz.open(lo_pdf) as pdf:
        if pdf.page_count != PAGE_COUNT:
            raise ValueError(f"LibreOffice PDF exported {pdf.page_count} pages, expected four")

    before = compare_cloud_rasters(before_path, "SampleNewsletter", lo_pdf)
    after = compare_cloud_rasters(after_path, "SampleNewsletter", lo_pdf)
    if before["comparison_state"] != "compared" or after["comparison_state"] != "compared":
        raise ValueError("one of the exact Readers cannot be compared with the LO reference")
    if before["reference_sha256"] != after["reference_sha256"]:
        raise ValueError("LO PDF identity changed between comparisons")

    pages = []
    for index in range(PAGE_COUNT):
        base_row, fixed_row = before["pages"][index], after["pages"][index]
        base_png = before_path.parent / before_capture["fixture"]["screenshots"][index]["filename"]
        fixed_png = after_path.parent / after_capture["fixture"]["screenshots"][index]["filename"]
        direct = compare_rasters(render_png(base_png), render_png(fixed_png))
        bdiff, fdiff = base_row["diff"], fixed_row["diff"]
        comparable = bool(
            base_row["physical_page_size_matches_reference"]
            and fixed_row["physical_page_size_matches_reference"]
            and bdiff["raster_size_match"]
            and fdiff["raster_size_match"]
            and direct["raster_size_match"]
        )
        bvalue = bdiff["significant_fraction"] if comparable else None
        fvalue = fdiff["significant_fraction"] if comparable else None
        direct_value = direct["significant_fraction"] if comparable else None
        pages.append(
            {
                "customer_page": index + 1,
                "source_page_identity_sha256": before_capture["page_ids"][index],
                "before_significant_fraction_vs_lo": bvalue,
                "after_significant_fraction_vs_lo": fvalue,
                "delta_significant_fraction": round(fvalue - bvalue, 10)
                if comparable else None,
                "before_significant_pixels_vs_lo": bdiff["significant_pixel_count"]
                if comparable else None,
                "after_significant_pixels_vs_lo": fdiff["significant_pixel_count"]
                if comparable else None,
                "direct_before_after_significant_fraction": direct_value,
                "direct_before_after_significant_pixels": direct["significant_pixel_count"]
                if comparable else None,
                "direct_change_bbox": direct["significant_bbox"] if comparable else None,
                "page_extent_comparable": comparable,
                "interpretation": classify_visual_delta(bvalue, fvalue, direct_value),
            }
        )

    return {
        "schema": "chaptera.reader-logo-0x31-lo-visual-ab.v1",
        "fixture": "SampleNewsletter",
        "source_pub_sha256": SOURCE_SHA256,
        "before_commit": BASE_SHA,
        "after_commit": FIX_SHA,
        "reference": {
            "kind": "libreoffice-import-export-proxy",
            "version": lo_version.strip(),
            "pdf_sha256": file_sha256(lo_pdf),
            "page_count": PAGE_COUNT,
        },
        "capture": {
            "backend": "cloud-browser-svg",
            "dpi": DPI,
            "source_page_id_order_equal": True,
            "customer_page_count": PAGE_COUNT,
            "physical_page_seq381_to_customer_page4_join_independently_proven": False,
        },
        "pages": pages,
        "page4_result": pages[3]["interpretation"],
        "status": "compared" if all(p["page_extent_comparable"] for p in pages) else "not_comparable_page_extent",
        "claims": {
            "native_publisher_parity_proven": False,
            "libreoffice_import_used_as_independent_proxy": True,
            "materialized_nodes_prove_visible_correctness": False,
            "no_other_commits_between_before_and_after": True,
        },
        "limitations": [
            "LibreOffice is not Microsoft Publisher. Similarity to this PDF is a proxy, not the native truth.",
            "The exact pinned before/after commits isolate the #2622 change but font availability and runtime raster differences still affect pixels.",
            "SourceGraph seq381 was tested separately; this report does not independently prove its customer-page mapping.",
            "No original PUB bytes or source Story text are copied into this JSON.",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--lo-version-file", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = measure(
        args.before, args.after, args.reference,
        args.lo_version_file.read_text(encoding="utf-8"),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for page in report["pages"]:
        print(
            f"customer_p{page['customer_page']} "
            f"before={page['before_significant_fraction_vs_lo']} "
            f"after={page['after_significant_fraction_vs_lo']} "
            f"direct={page['direct_before_after_significant_fraction']} "
            f"state={page['interpretation']}"
        )
    print("PAGE_4_LOGO_VISUAL_DISPOSITION=" + report["page4_result"])
    return 0 if report["status"] == "compared" else 2


if __name__ == "__main__":
    raise SystemExit(main())
