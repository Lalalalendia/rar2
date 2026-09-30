#!/usr/bin/env python3
from __future__ import annotations

import json
import platform
import sys
from pathlib import Path

import fitz

from pdf_reference_diff_v1 import (
    RASTER_DPI,
    SIGNIFICANT_CHANNEL_DELTA,
    TILE,
    compare_rasters,
    file_sha256,
    page_boxes,
    render_page,
    sha256_bytes,
)

EMU_PER_POINT = 12_700.0
PHYSICAL_SIZE_TOLERANCE_PX = 0.1
PHYSICAL_SIZE_TOLERANCE_PT = 72.0 / RASTER_DPI * PHYSICAL_SIZE_TOLERANCE_PX


def render_png(path: Path) -> dict:
    pix = fitz.Pixmap(str(path))
    if pix.colorspace is None or pix.colorspace.n != 3:
        pix = fitz.Pixmap(fitz.csRGB, pix)
    if pix.alpha:
        pix = fitz.Pixmap(pix, 0)
    samples = bytes(pix.samples)
    return {
        "width": pix.width,
        "height": pix.height,
        "stride": pix.stride,
        "samples": samples,
        "raster_sha256": sha256_bytes(samples),
    }


def physical_size_comparison(page_geometry: dict, reference_box: dict) -> dict:
    expected_width = page_geometry["width_emu"] / EMU_PER_POINT
    expected_height = page_geometry["height_emu"] / EMU_PER_POINT
    actual = reference_box["render_rect"]
    width_delta_pt = expected_width - actual["width_pt"]
    height_delta_pt = expected_height - actual["height_pt"]
    return {
        "matches_within_tolerance": (
            abs(width_delta_pt) <= PHYSICAL_SIZE_TOLERANCE_PT
            and abs(height_delta_pt) <= PHYSICAL_SIZE_TOLERANCE_PT
        ),
        "width_delta_pt": round(width_delta_pt, 6),
        "height_delta_pt": round(height_delta_pt, 6),
        "width_delta_px": round(width_delta_pt * RASTER_DPI / 72.0, 6),
        "height_delta_px": round(height_delta_pt * RASTER_DPI / 72.0, 6),
    }


def find_fixture(receipt: dict, name: str) -> dict:
    matches = [entry for entry in receipt.get("results", []) if entry.get("fixture") == name]
    if len(matches) != 1:
        raise ValueError(f"fixture {name!r} is not unique in Cloud Reader receipt")
    return matches[0]


def compare_cloud_rasters(
    cloud_receipt_path: Path,
    fixture_name: str,
    reference_pdf_path: Path,
) -> dict:
    cloud = json.loads(cloud_receipt_path.read_text(encoding="utf-8"))
    if cloud.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError(f"unsupported Cloud Reader receipt: {cloud.get('protocol')!r}")

    fixture = find_fixture(cloud, fixture_name)
    reference = fitz.open(reference_pdf_path)
    base = {
        "receipt_version": "chaptera.cloud-reader-reference-pdf-raster.v1",
        "fixture": fixture_name,
        "source_pub_sha256": fixture["source_sha256"],
        "reference_sha256": file_sha256(reference_pdf_path),
        "classification": fixture["classification"],
        "terminal_code": fixture.get("terminal_code"),
        "browser": cloud.get("browser"),
        "repository_commit_sha": cloud.get("repository_commit_sha"),
        "page_count": {
            "candidate": fixture.get("pages") if fixture.get("rendered") else None,
            "reference": reference.page_count,
            "match": (
                fixture.get("pages") == reference.page_count
                if fixture.get("rendered")
                else None
            ),
        },
        "comparator": {
            "engine": "MuPDF",
            "binding": "PyMuPDF",
            "pymupdf_version": fitz.VersionBind,
            "mupdf_version": fitz.VersionFitz,
            "python": sys.version.split()[0],
            "platform": platform.platform(),
            "raster_dpi": RASTER_DPI,
            "colorspace": "RGB",
            "alpha": False,
            "significant_channel_delta": SIGNIFICANT_CHANNEL_DELTA,
            "region_tile_px": TILE,
            "physical_size_tolerance_pt": PHYSICAL_SIZE_TOLERANCE_PT,
            "physical_size_tolerance_px": PHYSICAL_SIZE_TOLERANCE_PX,
        },
        "fidelity": fixture.get("fidelity"),
        "stacking_fidelity": fixture.get("stacking_fidelity"),
        "fidelity_reasons": fixture.get("fidelity_reasons", []),
        "diagnostic_codes": fixture.get("diagnostic_codes", []),
        "browser_preserved_scene_node_order": fixture.get(
            "browser_preserved_scene_node_order"
        ),
        "pages": [],
        "claims": {
            "author_supplied_reference_pdf": True,
            "publisher_visual_parity": False,
            "raw_pub_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }

    if not fixture.get("rendered"):
        base["comparison_state"] = "unavailable_reader_unsupported"
        base["limitations"] = [
            "The exact public PUB/PDF pair was reacquired and identity-checked, but the current Reader did not produce a renderable Scene.",
            "No visual parity claim is made from an unsupported Reader result.",
        ]
        return base

    if fixture.get("reference_raster_dpi") != RASTER_DPI:
        raise ValueError(
            f"Cloud Reader candidate was not captured at {RASTER_DPI} DPI: "
            f"{fixture.get('reference_raster_dpi')!r}"
        )

    screenshots = fixture.get("screenshots", [])
    geometry = fixture.get("page_geometry", [])
    if len(screenshots) != fixture["pages"] or len(geometry) != fixture["pages"]:
        raise ValueError("Cloud Reader page receipt is incomplete")

    compared = []
    for index in range(min(fixture["pages"], reference.page_count)):
        shot = screenshots[index]
        page_geometry = geometry[index]
        candidate_path = cloud_receipt_path.parent / shot["filename"]
        if file_sha256(candidate_path) != shot["sha256"]:
            raise ValueError(f"candidate PNG identity drift: {shot['filename']}")

        candidate = render_png(candidate_path)
        reference_page = reference.load_page(index)
        reference_box = page_boxes(reference_page)
        reference_raster = render_page(reference_page)
        diff = compare_rasters(candidate, reference_raster)
        physical = physical_size_comparison(page_geometry, reference_box)
        compared.append(
            {
                "page_index": index,
                "candidate_png": shot["filename"],
                "candidate_png_sha256": shot["sha256"],
                "candidate_raster_sha256": candidate["raster_sha256"],
                "reference_raster_sha256": reference_raster["raster_sha256"],
                "reference_boxes": reference_box,
                "candidate_physical_size": {
                    "width_emu": page_geometry["width_emu"],
                    "height_emu": page_geometry["height_emu"],
                    "width_pt": round(page_geometry["width_emu"] / EMU_PER_POINT, 6),
                    "height_pt": round(page_geometry["height_emu"] / EMU_PER_POINT, 6),
                },
                "physical_page_size_matches_reference": physical[
                    "matches_within_tolerance"
                ],
                "physical_page_size_delta": {
                    "width_pt": physical["width_delta_pt"],
                    "height_pt": physical["height_delta_pt"],
                    "width_px": physical["width_delta_px"],
                    "height_px": physical["height_delta_px"],
                },
                "diff": diff,
            }
        )

    base["comparison_state"] = "compared"
    base["pages"] = compared
    base["limitations"] = [
        "Cloud Reader SVG page rasters are compared with the author-supplied PDF at the same nominal DPI; this localizes visible disagreement but does not prove authoring-semantic equivalence.",
        "Current Reader fallback-font execution is measured as rendered; Publisher-exact font/reflow behavior is not assumed.",
        "Below-threshold raster noise is reported separately from significant differences.",
        "PDF object semantics and PDF/X conformance are outside this receipt.",
    ]
    return base


def main(argv: list[str]) -> None:
    if len(argv) != 5:
        raise SystemExit(
            "usage: cloud_reader_reference_pdf_raster_v1.py "
            "CLOUD_RECEIPT.json FIXTURE_NAME REFERENCE.pdf OUTPUT.json"
        )
    receipt = compare_cloud_rasters(
        Path(argv[1]),
        argv[2],
        Path(argv[3]),
    )
    output = Path(argv[4])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    if receipt["comparison_state"] == "compared":
        summary = " ".join(
            f"p{page['page_index'] + 1}="
            + (
                f"{page['diff']['significant_fraction']:.6f}"
                if page["diff"]["significant_fraction"] is not None
                else "size-mismatch"
            )
            for page in receipt["pages"]
        )
    else:
        summary = receipt["comparison_state"]
    print(f"CLOUD READER REFERENCE PDF COMPLETE {receipt['fixture']} {summary}")


if __name__ == "__main__":
    main(sys.argv)
