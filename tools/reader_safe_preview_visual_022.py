#!/usr/bin/env python3
"""Same-head physical Product Reader PNG/JPEG ablation against Publisher 022.

Only source-neutral coarse metrics are uploaded. The two real Chromium raster
files and image-bearing Reader Scene remain local to GitHub's ephemeral runner.
The Publisher reference fingerprint is pinned and never treated as semantic
page-role authority or exact pixel parity.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid, sha256

PINNED_CASES = {
    "001": ("0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", 1),
    "022": ("7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c", 1),
    "023": ("6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", 7),
}
REFERENCE_FINGERPRINT_SHA = "c522ebdc72d0b62ef68be3747494c2f5178ff69a70b83f14b3fd6a9bf8128404"


def compare(reference_path: Path, browser_dir: Path, output: Path, expected_head: str, case_id: str = "022") -> dict:
    if case_id not in PINNED_CASES:
        raise ValueError("unsupported pinned Product Publisher visual case")
    pinned_sha, expected_images = PINNED_CASES[case_id]
    if not re.fullmatch(r"[0-9a-f]{40}", expected_head):
        raise ValueError("expected checked-out repository head must be 40 lowercase hex chars")
    if sha256(reference_path) != REFERENCE_FINGERPRINT_SHA:
        raise ValueError("Publisher fingerprint identity changed")
    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    if reference.get("schema") != "chaptera.publisher-visual-fingerprint.v1":
        raise ValueError("invalid reference schema")
    if reference.get("pair_count") != 55 or reference.get("reference_page_count") != 162:
        raise ValueError("reference corpus cardinality changed")
    rows = [p for p in reference["pairs"] if p.get("oracle_id") == "batch01-" + case_id]
    if len(rows) != 1:
        raise ValueError("022 Publisher reference is not unique")
    pair = rows[0]
    if pair["pub_sha256"] != pinned_sha or pair["reference_pages"] != 1:
        raise ValueError("022 Publisher reference identity changed")
    oracle = reference_grid(pair["pages"][0])

    browser = json.loads((browser_dir / "browser-receipt.json").read_text(encoding="utf-8"))
    if browser.get("schema") != ("chaptera.product-safe-preview-022-browser.v1" if case_id == "022" else "chaptera.product-safe-preview-multiimage-browser.v1"):
        raise ValueError("this is not a real Product Chromium visual receipt")
    if browser.get("repository_commit_sha") != expected_head:
        raise ValueError("Product Chromium receipt does not match exact checked-out head")
    if browser.get("source_sha256") != pinned_sha or browser.get("fixture") != case_id:
        raise ValueError("Product browser source SHA differs from Publisher oracle")
    if not browser.get("same_scene_and_source") or not browser.get("image_sanitizer_ablation"):
        raise ValueError("Product browser before/after do not share a real scene")
    if browser.get("page_errors") or browser.get("external_url_requests"):
        raise ValueError("unsafe Chromium external fetch or paint failure")
    before, after = browser["before"], browser["after"]
    if before["page_id_sha256"] != after["page_id_sha256"]:
        raise ValueError("before/after Source PAGE identity changed")
    if before["picture_frame_count"] != after["picture_frame_count"]:
        raise ValueError("before/after picture frames changed")
    if before["image_count"] != 0 or after["image_count"] != expected_images:
        raise ValueError("host derivative was not independently visible in Chromium")

    measures: dict[str, dict] = {}
    for label, state in [("before", before), ("after", after)]:
        path = browser_dir / (label + ".png")
        if sha256(path) != state["screenshot_sha256"]:
            raise ValueError("Chromium raster bytes changed after screenshot")
        measures[label] = compare_grid(image_grid(path), oracle)

    changed_before = measures["before"]["changed_cell_count"]
    changed_after = measures["after"]["changed_cell_count"]
    report = {
        "schema": "chaptera.product-preview-publisher-022-ablation.v1" if case_id == "022" else "chaptera.product-preview-publisher-multiimage-ablation.v1",
        "fixture": case_id,
        "source_sha256": pinned_sha,
        **({"sha256_022": pinned_sha} if case_id == "022" else {}),
        "publisher_pdf_sha256": pair["pdf_sha256"],
        "publisher_reference_stage": pair.get("reference_surface_stage", "unknown"),
        "publisher_reference_grid_sha256": pair["pages"][0]["grid_sha256"],
        "repository_commit_sha": expected_head,
        "same_source_scene_head_and_browser": True,
        "real_browser": "chromium",
        "host_derived_image_count": after["image_count"],
        "missing_image_negative_control_count": before["image_count"],
        "page_identity_stable": True,
        "reference_grid_size": [64, 64],
        "changed_cell_before": changed_before,
        "changed_cell_after": changed_after,
        "changed_cell_improvement": changed_before - changed_after,
        "metrics_before": measures["before"],
        "metrics_after": measures["after"],
        "claims": {
            "measurement_only": True,
            "publisher_pixel_exact_parity": False,
            "source_pub_included": False,
            "image_containers_included": False,
            "screenshots_uploaded": False,
            "source_text_included": False,
            "production_deployment_verified": False,
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("PRODUCT_PUBLISHER_022_ABLATION " + json.dumps(report, sort_keys=True))
    if changed_after >= changed_before and case_id == "022":
        raise ValueError(
            f"safe preview did not improve 022 versus same-head missing-image control: "
            f"{changed_before} -> {changed_after}"
        )
    return report


if __name__ == "__main__":
    if len(sys.argv) not in (5, 6):
        raise SystemExit("usage: reader_safe_preview_visual_022.py REFERENCE BROWSER_DIR OUTPUT EXACT_HEAD [001|022|023]")
    compare(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4], sys.argv[5] if len(sys.argv) == 6 else "022")
