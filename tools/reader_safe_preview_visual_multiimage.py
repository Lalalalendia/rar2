#!/usr/bin/env python3
"""Publisher 64x64 visual comparison of real host-admitted Product PUB001/023.

The tested browser takes the actual authenticated Product Scene from the
ephemeral physical OIDC/HTTPS job. Never upload screenshots, private Scene
or image/PUB/PDF bytes. This metric is not pixel-exact Publisher parity.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid, sha256

REFERENCE = "chaptera.publisher-visual-fingerprint.v1"
BROWSER = "chaptera.product-multiimage-publisher-ab-browser.v1"
CASES = {
    "001": {
        "oracle_id": "batch01-001",
        "pub_sha256": "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade",
        "pdf_sha256": "7240b47b2acf4bcccc092e3822ac86aedc50f2d3084b374c7122e2ecc391c334",
        "grid_sha256": "a84dd78f6a5aa5a51d61fe32cd2123f78a0ffbd8748bd5c548c53cac2ea08aec",
        "images": 1,
    },
    "023": {
        "oracle_id": "batch01-023",
        "pub_sha256": "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8",
        "pdf_sha256": "7916fdf1321793f918f6f00f41ddc2b74be4453dd4e63a28ce6e805730986bf6",
        "grid_sha256": "41598501994a6e703ddd9ef3da0b74cdaa4d483a791911c38d7533b294be83c2",
        "images": 7,
    },
}


def check_receipt(browser: dict, case_id: str, head: str) -> tuple[dict, dict]:
    """Reject stale or forged physical proof before reading any raster bytes."""
    expected = CASES[case_id]
    if not isinstance(browser, dict) or browser.get("schema") != BROWSER:
        raise ValueError("not a Product multi-image Chromium receipt")
    if browser.get("fixture") != case_id or browser.get("source_sha256") != expected["pub_sha256"]:
        raise ValueError("Product source identity does not match Publisher oracle")
    if browser.get("repository_commit_sha") != head or not re.fullmatch(r"[0-9a-f]{40}", head):
        raise ValueError("Product exact checked-out repository head mismatch")
    if (browser.get("same_source_scene_head_and_browser") is not True
            or browser.get("image_only_ablation") is not True
            or browser.get("real_chromium") is not True
            or browser.get("external_url_request_count") != 0
            or browser.get("page_error_count") != 0
            or browser.get("host_resource_count") != expected["images"]):
        raise ValueError("Product real Chromium/host-only A/B was not proven")
    before, after = browser.get("before"), browser.get("after")
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise ValueError("missing before/after Chromium evidence")
    if (before.get("page_id_sha256") != after.get("page_id_sha256")
            or before.get("picture_frame_count") != after.get("picture_frame_count")
            or before.get("png_dimensions") != after.get("png_dimensions")
            or before.get("image_count") != 0
            or after.get("image_count", 0) < expected["images"]
            or after.get("unique_painted_resource_ids") != expected["images"]):
        raise ValueError("Product A/B page identity, geometry or image membership drift")
    for mode, state in (("before", before), ("after", after)):
        if (not isinstance(state.get("screenshot_sha256"), str)
                or not re.fullmatch(r"[0-9a-f]{64}", state["screenshot_sha256"])):
            raise ValueError("invalid private Chromium " + mode + " screenshot identity")
        dims = state["png_dimensions"]
        if (not isinstance(dims, dict) or set(dims) != {"width", "height"}
                or not all(type(v) is int and 0 < v <= 30000 for v in dims.values())):
            raise ValueError("invalid Product SVG screenshot geometry")
    if before["screenshot_sha256"] == after["screenshot_sha256"]:
        raise ValueError("host-owned preview has no observable raster effect")
    return before, after


def compare(case_id: str, fingerprint_path: Path, browser_dir: Path, output: Path, head: str) -> dict:
    if case_id not in CASES:
        raise ValueError("only pinned Publisher PUB001/023 is accepted")
    expected = CASES[case_id]
    reference = json.loads(fingerprint_path.read_text(encoding="utf-8"))
    if (reference.get("schema") != REFERENCE or reference.get("pair_count") != 55
            or reference.get("reference_page_count") != 162
            or reference.get("grid") != {
                "colorspace": "RGB", "height": 64,
                "reference_raster_dpi": 144, "resampling": "Pillow.BOX", "width": 64
            }):
        raise ValueError("Publisher 55/162 RGB/BOX reference corpus identity drift")
    pairs = [p for p in reference.get("pairs", []) if p.get("oracle_id") == expected["oracle_id"]]
    if len(pairs) != 1:
        raise ValueError("missing or duplicate Publisher pair")
    pair = pairs[0]
    if (pair.get("pub_sha256") != expected["pub_sha256"]
            or pair.get("pdf_sha256") != expected["pdf_sha256"]
            or pair.get("reference_pages") != 1 or len(pair.get("pages", [])) != 1
            or pair["pages"][0].get("grid_sha256") != expected["grid_sha256"]):
        raise ValueError("Publisher reference PUB/PDF/page identity drift")
    oracle = reference_grid(pair["pages"][0])
    browser = json.loads((browser_dir / "browser-receipt.json").read_text(encoding="utf-8"))
    before, after = check_receipt(browser, case_id, head)
    metrics = {}
    for mode, state in (("before", before), ("after", after)):
        screenshot = browser_dir / (mode + ".png")
        if sha256(screenshot) != state["screenshot_sha256"]:
            raise ValueError("Product raster SHA does not match exact browser capture")
        with screenshot.open("rb") as f:
            header = f.read(24)
        if (len(header) != 24 or header[:8] != bytes.fromhex("89504e470d0a1a0a")
                or int.from_bytes(header[16:20], "big") != state["png_dimensions"]["width"]
                or int.from_bytes(header[20:24], "big") != state["png_dimensions"]["height"]):
            raise ValueError("Product PNG geometry is not the verified capture")
        metrics[mode] = compare_grid(image_grid(screenshot), oracle)

    baseline = metrics["before"]["changed_cell_count"]
    product = metrics["after"]["changed_cell_count"]
    report = {
        "schema": "chaptera.product-multiimage-publisher-ab-comparison.v1",
        "fixture": case_id,
        "source_sha256": expected["pub_sha256"],
        "publisher_pdf_sha256": expected["pdf_sha256"],
        "publisher_reference_grid_sha256": expected["grid_sha256"],
        "publisher_reference_stage": pair.get("reference_surface_stage", "unknown"),
        "repository_head_sha": head,
        "host_owned_image_count": expected["images"],
        "same_source_scene_head_and_browser": True,
        "chromium_64x64_changed_without_images": baseline,
        "chromium_64x64_changed_with_images": product,
        "chromium_64x64_net_improvement": baseline - product,
        "visual_improvement_observed": product < baseline,
        "metrics_without_images": metrics["before"],
        "metrics_with_images": metrics["after"],
        "claims": {
            "measurement_only": True,
            "publisher_pixel_exact_parity": False,
            "original_pub_or_pdf_uploaded": False,
            "scene_images_fonts_and_screenshots_uploaded": False,
            "customer_production_host_verified": False,
            "publisher_reference_surface_semantic_authority": False,
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print("PRODUCT_MULTIIMAGE_PUBLISHER_SCORE " + json.dumps(report, sort_keys=True))
    return report


if __name__ == "__main__":
    if len(sys.argv) != 6:
        raise SystemExit("usage: reader_safe_preview_visual_multiimage.py ID REFERENCE BROWSER_DIR OUTPUT EXACT_HEAD")
    compare(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]), sys.argv[5])
