#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import platform
import sys
from pathlib import Path

import fitz

from pdf_reference_diff_v1 import compare_rasters, file_sha256

SCHEMA = "chaptera.reader-cross-backend-visual-parity.v1"
DESKTOP_SCHEMA = "chaptera.reader-golden-carlton-march.v1"
CLOUD_PROTOCOL = "chaptera.cloud-reader-real-scene-browser.v1"


def png_raster(path: Path) -> dict:
    pix = fitz.Pixmap(str(path))
    width = pix.width
    height = pix.height
    samples = bytes(pix.samples)

    if pix.n == 3:
        rgb = samples
    elif pix.n == 4:
        rgb = bytearray(width * height * 3)
        for pixel in range(width * height):
            src = pixel * 4
            dst = pixel * 3
            rgb[dst : dst + 3] = samples[src : src + 3]
        rgb = bytes(rgb)
    else:
        converted = fitz.Pixmap(fitz.csRGB, pix)
        converted_samples = bytes(converted.samples)
        if converted.n == 3:
            rgb = converted_samples
        elif converted.n == 4:
            rgb = bytearray(width * height * 3)
            for pixel in range(width * height):
                src = pixel * 4
                dst = pixel * 3
                rgb[dst : dst + 3] = converted_samples[src : src + 3]
            rgb = bytes(rgb)
        else:
            raise ValueError(f"unsupported PNG channel count for {path}: {converted.n}")

    expected = width * height * 3
    if len(rgb) != expected:
        raise ValueError(f"RGB byte length mismatch for {path}: {len(rgb)} != {expected}")
    return {"width": width, "height": height, "samples": rgb}


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def find_cloud_fixture(receipt: dict, fixture: str) -> dict:
    rows = [row for row in receipt.get("results", []) if row.get("fixture") == fixture]
    if len(rows) != 1:
        raise ValueError(f"cloud fixture {fixture!r} is not unique")
    row = rows[0]
    if row.get("rendered") is not True:
        raise ValueError(
            f"cloud fixture {fixture!r} did not render: "
            f"classification={row.get('classification')!r} terminal={row.get('terminal_code')!r}"
        )
    return row


def compare_backends(
    desktop_receipt_path: Path,
    cloud_receipt_path: Path,
    fixture: str,
    output_path: Path,
) -> dict:
    desktop = load_json(desktop_receipt_path)
    cloud = load_json(cloud_receipt_path)

    if desktop.get("schema") != DESKTOP_SCHEMA:
        raise ValueError(f"unsupported desktop receipt schema: {desktop.get('schema')!r}")
    if cloud.get("protocol") != CLOUD_PROTOCOL:
        raise ValueError(f"unsupported cloud receipt protocol: {cloud.get('protocol')!r}")

    cloud_fixture = find_cloud_fixture(cloud, fixture)
    source_sha = desktop.get("source_sha256")
    if not isinstance(source_sha, str) or len(source_sha) != 64:
        raise ValueError("desktop receipt has no canonical source SHA-256")
    if cloud_fixture.get("source_sha256") != source_sha:
        raise ValueError("desktop/cloud source identity mismatch")

    desktop_pages = sorted(desktop.get("pages", []), key=lambda row: int(row["page_number"]))
    cloud_shots = sorted(cloud_fixture.get("screenshots", []), key=lambda row: int(row["page"]))
    cloud_geometry = cloud_fixture.get("page_geometry", [])
    if len(desktop_pages) != len(cloud_shots):
        raise ValueError(
            f"desktop/cloud page-count mismatch: {len(desktop_pages)} != {len(cloud_shots)}"
        )
    if len(cloud_geometry) != len(cloud_shots):
        raise ValueError("cloud page geometry is incomplete")

    desktop_dpi = desktop.get("raster_dpi")
    cloud_dpi = cloud_fixture.get("reference_raster_dpi")
    if desktop_dpi != cloud_dpi:
        raise ValueError(f"desktop/cloud raster DPI mismatch: {desktop_dpi!r} != {cloud_dpi!r}")

    page_rows = []
    exact_pages = 0
    size_mismatch_pages = 0
    significant_fractions = []

    for index, (desktop_page, cloud_shot, cloud_page) in enumerate(
        zip(desktop_pages, cloud_shots, cloud_geometry),
        start=1,
    ):
        if int(desktop_page["page_number"]) != index or int(cloud_shot["page"]) != index:
            raise ValueError(f"page ordering drift at page {index}")

        desktop_png = desktop_receipt_path.parent / desktop_page["png"]
        cloud_png = cloud_receipt_path.parent / cloud_shot["filename"]
        if not desktop_png.is_file():
            raise ValueError(f"desktop PNG is missing: {desktop_png}")
        if not cloud_png.is_file():
            raise ValueError(f"cloud PNG is missing: {cloud_png}")

        desktop_raster = png_raster(desktop_png)
        cloud_raster = png_raster(cloud_png)
        diff = compare_rasters(desktop_raster, cloud_raster)
        if diff["raster_size_match"]:
            significant_fractions.append(diff["significant_fraction"])
            if diff["significant_pixel_count"] == 0:
                exact_pages += 1
        else:
            size_mismatch_pages += 1

        page_rows.append(
            {
                "page": index,
                "desktop_page_id": desktop_page.get("page_id"),
                "cloud_page_identity_sha256": cloud_page.get("page_identity_sha256"),
                "page_extent_emu": {
                    "desktop": [
                        desktop_page.get("width_emu"),
                        desktop_page.get("height_emu"),
                    ],
                    "cloud": [
                        cloud_page.get("width_emu"),
                        cloud_page.get("height_emu"),
                    ],
                    "match": (
                        desktop_page.get("width_emu") == cloud_page.get("width_emu")
                        and desktop_page.get("height_emu") == cloud_page.get("height_emu")
                    ),
                },
                "desktop_png_sha256": file_sha256(desktop_png),
                "cloud_png_sha256": file_sha256(cloud_png),
                "desktop_raster_px": [desktop_raster["width"], desktop_raster["height"]],
                "cloud_raster_px": [cloud_raster["width"], cloud_raster["height"]],
                "diff": diff,
            }
        )

    if size_mismatch_pages:
        state = "raster_size_mismatch"
    elif exact_pages == len(page_rows):
        state = "pixel_exact"
    else:
        state = "backend_divergence"

    payload = {
        "schema": SCHEMA,
        "fixture": fixture,
        "source_pub_sha256": source_sha,
        "repository_commit_sha": cloud.get("repository_commit_sha"),
        "desktop_backend": desktop.get("render_backend"),
        "cloud_backend": "browser-svg-dom",
        "raster_dpi": desktop_dpi,
        "page_count": len(page_rows),
        "state": state,
        "pixel_exact_page_count": exact_pages,
        "raster_size_mismatch_page_count": size_mismatch_pages,
        "mean_significant_fraction": (
            sum(significant_fractions) / len(significant_fractions)
            if significant_fractions
            else None
        ),
        "max_significant_fraction": (
            max(significant_fractions) if significant_fractions else None
        ),
        "pages": page_rows,
        "comparator": {
            "significant_channel_delta": 24,
            "region_tile_px": 16,
            "desktop_png_loader": "PyMuPDF",
            "cloud_png_loader": "PyMuPDF",
            "python": sys.version.split()[0],
            "platform": platform.platform(),
        },
        "claims": {
            "same_source_identity_required": True,
            "same_raster_dpi_required": True,
            "measurement_only": True,
            "hard_visual_threshold_applied": False,
            "shared_render_plan_equivalence_proven": False,
            "pixel_exact_visual_parity_claimed": state == "pixel_exact",
            "backend_divergence_localization_only": True,
            "publisher_reference_used": False,
            "raw_pub_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("desktop_receipt", type=Path)
    parser.add_argument("cloud_receipt", type=Path)
    parser.add_argument("fixture")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    receipt = compare_backends(
        args.desktop_receipt,
        args.cloud_receipt,
        args.fixture,
        args.output,
    )
    print(
        json.dumps(
            {
                "fixture": receipt["fixture"],
                "state": receipt["state"],
                "page_count": receipt["page_count"],
                "pixel_exact_pages": receipt["pixel_exact_page_count"],
                "raster_size_mismatch_pages": receipt["raster_size_mismatch_page_count"],
                "mean_significant_fraction": receipt["mean_significant_fraction"],
                "max_significant_fraction": receipt["max_significant_fraction"],
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
