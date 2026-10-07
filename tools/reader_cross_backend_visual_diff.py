#!/usr/bin/env python3
"""Compare page-only local Reader and Cloud Reader PNGs at the same DPI."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageStat


def page_metrics(left_path: Path, right_path: Path) -> dict:
    left = Image.open(left_path).convert("RGBA")
    right = Image.open(right_path).convert("RGBA")
    if left.size != right.size:
        return {
            "size_match": False,
            "local_size": list(left.size),
            "cloud_size": list(right.size),
            "changed_pixel_count": None,
            "changed_fraction": None,
            "mean_abs_channel_delta": None,
            "bbox": None,
        }

    diff = ImageChops.difference(left, right)
    rgb = diff.convert("RGB")
    bbox = rgb.getbbox()
    width, height = left.size
    total = width * height

    changed = 0
    if bbox is not None:
        mask = rgb.convert("L").point(lambda value: 255 if value else 0)
        changed = int(ImageStat.Stat(mask).sum[0] / 255)

    stat = ImageStat.Stat(rgb)
    mean = sum(stat.mean) / len(stat.mean)
    return {
        "size_match": True,
        "local_size": list(left.size),
        "cloud_size": list(right.size),
        "changed_pixel_count": changed,
        "changed_fraction": changed / total if total else 0.0,
        "mean_abs_channel_delta": mean,
        "bbox": list(bbox) if bbox else None,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--local-dir", required=True)
    parser.add_argument("--cloud-dir", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--fixture", default="SampleNewsletter")
    parser.add_argument("--pages", type=int, default=4)
    parser.add_argument("--dpi", type=int, default=144)
    args = parser.parse_args()

    local_dir = Path(args.local_dir)
    cloud_dir = Path(args.cloud_dir)
    pages = []
    for page in range(1, args.pages + 1):
        name = f"{args.fixture}-page-{page}.png"
        local = local_dir / name
        cloud = cloud_dir / name
        if not local.is_file():
            raise SystemExit(f"missing local raster: {local}")
        if not cloud.is_file():
            raise SystemExit(f"missing cloud raster: {cloud}")
        pages.append({
            "page": page,
            "local_png": name,
            "cloud_png": name,
            "diff": page_metrics(local, cloud),
        })

    receipt = {
        "schema": "chaptera.reader-cross-backend-visual-diff.v1",
        "fixture": args.fixture,
        "dpi": args.dpi,
        "page_count": args.pages,
        "comparison": "local-egui-wgpu-vs-cloud-browser-svg",
        "claims": {
            "same_source_fixture": True,
            "same_requested_raster_dpi": True,
            "pixel_perfect_parity_claimed": False,
            "diagnostic_only": True,
        },
        "pages": pages,
        "summary": {
            "all_sizes_match": all(page["diff"]["size_match"] for page in pages),
            "changed_fractions": [page["diff"]["changed_fraction"] for page in pages],
            "mean_abs_channel_deltas": [
                page["diff"]["mean_abs_channel_delta"] for page in pages
            ],
        },
    }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt["summary"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
