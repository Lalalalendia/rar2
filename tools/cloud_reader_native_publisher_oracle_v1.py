#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
from pathlib import Path

from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters
from pdf_reference_diff_v1 import file_sha256

EXPECTED = {
    "SampleNewsletter": {
        "source_sha256": "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
        "pages": 4,
        "pdf": "SampleNewsletter.publisher.pdf",
    },
    "SampleBrochure": {
        "source_sha256": "ffed034ac87e679f0bd08ff9cf74ad11c0e510a42b1bc1a7502415f6c29c87",
        "pages": 2,
        "pdf": "SampleBrochure.publisher.pdf",
    },
}
EXPECTED_PUBLISHER_VERSION = "16.0"
EXPECTED_PUBLISHER_BUILD = "12527"
EXPECTED_PUBLISHER_EXE_SHA256 = (
    "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
)


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def fixture_by_name(receipt: dict, name: str) -> dict:
    matches = [item for item in receipt.get("fixtures", []) if item.get("fixture") == name]
    if len(matches) != 1:
        raise ValueError(f"native receipt fixture {name!r} is missing or duplicated")
    return matches[0]


def cloud_result_by_name(receipt: dict, name: str) -> dict:
    matches = [item for item in receipt.get("results", []) if item.get("fixture") == name]
    if len(matches) != 1:
        raise ValueError(f"Cloud Reader receipt fixture {name!r} is missing or duplicated")
    return matches[0]


def validate_native_receipt(path: Path, receipt: dict) -> dict[str, Path]:
    if receipt.get("schema") != "chaptera.cloud-reader-native-publisher-reference.v1":
        raise ValueError("unsupported native Publisher receipt")
    if receipt.get("status") != "success":
        raise ValueError("native Publisher capture did not complete successfully")

    publisher = receipt.get("publisher", {})
    if publisher.get("version") != EXPECTED_PUBLISHER_VERSION:
        raise ValueError("Publisher version drift")
    if publisher.get("build") != EXPECTED_PUBLISHER_BUILD:
        raise ValueError("Publisher build drift")
    if publisher.get("executable_sha256") != EXPECTED_PUBLISHER_EXE_SHA256:
        raise ValueError("Publisher executable identity drift")
    if publisher.get("clean_process_exit") is not True:
        raise ValueError("Publisher process did not exit cleanly")

    capture = receipt.get("capture", {})
    if capture.get("source_open_mode") != "read_only":
        raise ValueError("native capture was not read-only")
    if capture.get("source_save_or_save_as_called") is not False:
        raise ValueError("native capture claims a source save")
    if capture.get("raw_pub_bytes_emitted") is not False:
        raise ValueError("native capture receipt crossed the raw PUB boundary")

    pdfs: dict[str, Path] = {}
    for name, expected in EXPECTED.items():
        item = fixture_by_name(receipt, name)
        if item.get("source_pub_sha256") != expected["source_sha256"]:
            raise ValueError(f"{name} source identity drift")
        if item.get("source_modified") is not False:
            raise ValueError(f"{name} source was modified")
        if item.get("opened_read_only") is not True:
            raise ValueError(f"{name} did not open read-only")
        if item.get("page_count") != expected["pages"]:
            raise ValueError(f"{name} native page-count drift")

        pdf_name = item.get("reference_pdf")
        if pdf_name != expected["pdf"]:
            raise ValueError(f"{name} reference PDF filename drift")
        pdf_path = path.parent / pdf_name
        if not pdf_path.is_file():
            raise ValueError(f"{name} reference PDF is missing")
        if file_sha256(pdf_path) != item.get("reference_pdf_sha256"):
            raise ValueError(f"{name} reference PDF SHA-256 drift")
        pdfs[name] = pdf_path
    return pdfs


def residual_regions(page: dict, limit: int = 5) -> list[dict]:
    diff = page.get("diff", {})
    return [
        {
            "pixel_bbox": region.get("pixel_bbox"),
            "tile_count": region.get("tile_count"),
        }
        for region in diff.get("regions", [])[:limit]
    ]


def main(argv: list[str]) -> None:
    if len(argv) != 4:
        raise SystemExit(
            "usage: cloud_reader_native_publisher_oracle_v1.py "
            "CLOUD_BROWSER_RECEIPT.json PUBLISHER_NATIVE_RECEIPT.json OUTPUT_DIR"
        )

    cloud_receipt_path = Path(argv[1]).resolve()
    native_receipt_path = Path(argv[2]).resolve()
    output_dir = Path(argv[3]).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    cloud_receipt = load_json(cloud_receipt_path)
    if cloud_receipt.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError("unsupported Cloud Reader browser receipt")

    native_receipt = load_json(native_receipt_path)
    pdfs = validate_native_receipt(native_receipt_path, native_receipt)

    comparisons = {}
    summary_rows = []
    for name, expected in EXPECTED.items():
        cloud_fixture = cloud_result_by_name(cloud_receipt, name)
        if cloud_fixture.get("source_sha256") != expected["source_sha256"]:
            raise ValueError(f"{name} Cloud Reader source identity drift")
        if cloud_fixture.get("pages") != expected["pages"]:
            raise ValueError(f"{name} Cloud Reader page-count drift")
        if cloud_fixture.get("rendered") is not True:
            raise ValueError(f"{name} Cloud Reader did not render")

        comparison = compare_cloud_rasters(
            cloud_receipt_path,
            name,
            pdfs[name],
        )
        if comparison.get("comparison_state") != "compared":
            raise ValueError(f"{name} native comparison was unavailable")
        if comparison.get("page_count", {}).get("match") is not True:
            raise ValueError(f"{name} candidate/reference page-count mismatch")

        output_path = output_dir / f"{name}.native-publisher-diff.json"
        output_path.write_text(
            json.dumps(comparison, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        comparisons[name] = comparison

        for page in comparison["pages"]:
            diff = page["diff"]
            summary_rows.append(
                {
                    "fixture": name,
                    "page": page["page_index"] + 1,
                    "significant_fraction": diff["significant_fraction"],
                    "noise_only_fraction": diff["noise_only_fraction"],
                    "mean_abs_channel_delta": diff["mean_abs_channel_delta"],
                    "max_channel_delta": diff["max_channel_delta"],
                    "physical_page_size_matches_reference": page[
                        "physical_page_size_matches_reference"
                    ],
                    "largest_residual_regions": residual_regions(page),
                    "stacking_fidelity": comparison.get("stacking_fidelity"),
                    "fidelity_reasons": comparison.get("fidelity_reasons", []),
                    "diagnostic_codes": comparison.get("diagnostic_codes", []),
                }
            )

    summary = {
        "schema": "chaptera.cloud-reader-native-publisher-oracle-summary.v1",
        "cloud_repository_commit_sha": cloud_receipt.get("repository_commit_sha"),
        "publisher": native_receipt["publisher"],
        "source_identity_checked": True,
        "native_publisher_reference": True,
        "publisher_visual_parity_claim": False,
        "raw_pub_bytes_emitted": False,
        "raw_story_text_emitted": False,
        "pages": summary_rows,
        "next_step": (
            "Rank pages by significant_fraction, inspect the largest residual regions, "
            "then choose the smallest source-backed display fix that explains the dominant residual."
        ),
    }
    summary_path = output_dir / "summary.json"
    summary_path.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    metrics = " ".join(
        f"{row['fixture']}:p{row['page']}={row['significant_fraction']:.6f}"
        for row in summary_rows
    )
    print(f"CLOUD READER NATIVE PUBLISHER ORACLE COMPLETE {metrics}")
    print(summary_path)


if __name__ == "__main__":
    main(sys.argv)
