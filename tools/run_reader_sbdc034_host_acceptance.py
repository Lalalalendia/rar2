#!/usr/bin/env python3
"""Private-in-runner, source-neutral SBDC 034 real-Guest four-star visual proof.

Only reuses the pre-existing SHA-pinned 1050 corpus artifact and Publisher
fingerprint. No source, scene, images or screenshots enter public artifacts.
"""
import argparse
import base64
from collections import Counter
from io import BytesIO
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from PIL import Image
from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid

SOURCE_SHA = "0d2278960a0101ced0a1ba6d5922f5a26ea37b76b3ab00ef21e21873359d4a5a"
SOURCE_BYTES = 1019904
ORACLE_ID = "batch01-034"
PAGE_COUNT = 1


def red_fraction(image):
    sample = image.convert("RGB").resize((128, 128))
    red = sum(
        r > 180 and g < 75 and b < 155 and r - g > 140
        for r, g, b in sample.getdata()
    )
    return red / (128 * 128)


def image_from_resource(resource):
    mime = resource.get("mime")
    url = resource.get("inline_data_url")
    prefix = f"data:{mime};base64,"
    if resource.get("availability") != "inline_data_url" or not isinstance(url, str):
        return None
    if mime not in ("image/jpeg", "image/png") or not url.startswith(prefix):
        raise AssertionError("bad host-owned image transport")
    encoded = base64.b64decode(url[len(prefix):], validate=True)
    assert 0 < len(encoded) <= 5 * 1024 * 1024, "host image byte cap"
    image = Image.open(BytesIO(encoded))
    image.load()
    assert image.width > 0 and image.height > 0
    return image


def run_browser(scene_path, manifest, destination, head, required=None):
    env = dict(os.environ)
    env.update({
        "READER_REAL_MANIFEST": str(manifest),
        "READER_ACCEPTED_SCENE_JSON": str(scene_path),
        "READER_REAL_OUTPUT": str(destination),
        "READER_REFERENCE_RASTER_DPI": "144",
        "REPOSITORY_COMMIT_SHA": head,
    })
    if required is not None:
        env["READER_REQUIRED_PAINTED_HOST_IMAGES"] = str(required)
    else:
        env.pop("READER_REQUIRED_PAINTED_HOST_IMAGES", None)
    subprocess.run(
        ["node", "apps/cloud-reader/real-browser.test.mjs"],
        env=env, check=True, timeout=150,
    )
    receipt = json.loads((destination / "receipt.json").read_text())
    assert receipt["protocol"] == "chaptera.cloud-reader-real-scene-browser.v1"
    assert receipt["repository_commit_sha"] == head
    assert len(receipt["results"]) == 1
    row = receipt["results"][0]
    assert row["scene_source"] == "accepted_https_guest_service"
    assert row["source_sha256"] == SOURCE_SHA
    assert row["rendered"] is True and row["pages"] == PAGE_COUNT
    shot, = row["screenshots"]
    png = destination / shot["filename"]
    return row, png


def page_crop(page_image, bounds, page_width, page_height):
    w, h = page_image.size
    left = max(0, int(bounds["x"] / page_width * w))
    top = max(0, int(bounds["y"] / page_height * h))
    right = min(w, int((bounds["x"] + bounds["width"]) / page_width * w + 1))
    bottom = min(h, int((bounds["y"] + bounds["height"]) / page_height * h + 1))
    assert right > left + 5 and bottom > top + 5, "star ROI outside rendered page"
    return page_image.crop((left, top, right, bottom))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--fingerprint", required=True, type=Path)
    parser.add_argument("--chaptera", required=True, type=Path)
    parser.add_argument("--caddy", required=True, type=Path)
    parser.add_argument("--repository-commit", required=True)
    parser.add_argument("--receipt-out", required=True, type=Path)
    args = parser.parse_args()
    head = args.repository_commit
    assert len(head) == 40 and all(c in "0123456789abcdef" for c in head)
    source = args.corpus / (SOURCE_SHA + ".pub")
    assert source.is_file() and source.stat().st_size == SOURCE_BYTES
    assert hashlib.sha256(source.read_bytes()).hexdigest() == SOURCE_SHA

    gold = json.loads(args.fingerprint.read_text())
    pair, = (p for p in gold["pairs"] if p["oracle_id"] == ORACLE_ID)
    assert pair["pub_sha256"] == SOURCE_SHA and len(pair["pages"]) == PAGE_COUNT
    assert pair["pub_bytes"] == SOURCE_BYTES

    with tempfile.TemporaryDirectory(prefix="sbdc034-") as temp:
        root = Path(temp)
        source_copy = root / "034.pub"
        shutil.copyfile(source, source_copy)
        accepted_path = root / "accepted.json"
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps({
            "schema": "chaptera.cloud-reader-real-fixtures.v1",
            "fixtures": [{
                "name": "034_0d227896",
                "sha256": SOURCE_SHA,
                "bytes": SOURCE_BYTES,
                "pages": PAGE_COUNT,
                "require_render": True,
                "source_path": str(source_copy),
            }],
        }))
        subprocess.run([
            sys.executable, "tools/run_cloud_reader_live_acceptance.py",
            "--chaptera", str(args.chaptera),
            "--structural-worker", str(args.chaptera),
            "--caddy", str(args.caddy),
            "--fixture", str(source_copy),
            "--fixture-sha256", SOURCE_SHA,
            "--repository-commit", head,
            "--out", str(root / "guest"),
            "--accepted-scene-out", str(accepted_path),
        ], check=True, timeout=240)
        wrapped = json.loads(accepted_path.read_text())
        assert wrapped["protocol_version"] == "chaptera.live-accepted-guest-scene.v1"
        assert wrapped["source_sha256"] == SOURCE_SHA
        assert wrapped["source_byte_len"] == SOURCE_BYTES
        assert wrapped["isolated_guest_scene_count"] == 1
        scene = wrapped["scene"]
        assert scene["source_hash"] == SOURCE_SHA
        assert len(scene["pages"]) == 1
        page_model = scene["pages"][0]
        page_width, page_height = page_model["width_emu"], page_model["height_emu"]
        assert page_width > 0 and page_height > 0
        nodes = [
            node for node in scene["nodes"]
            if node.get("kind") == "picture_frame" and node.get("resource_id")
        ]
        uses = Counter(node["resource_id"] for node in nodes)
        resources = {
            resource["resource_id"]: resource
            for resource in scene["resources"]
            if resource.get("mime") in ("image/jpeg", "image/png")
        }
        assert len(resources) >= 2, "SBDC picture resource admission lost"
        eligible = {}
        for rid, resource in resources.items():
            image = image_from_resource(resource)
            if image is not None:
                eligible[rid] = (image, red_fraction(image))

        # A single source 507x482 red JPEG must be reused at four placements.
        stars = [
            rid for rid, (image, redness) in eligible.items()
            if uses[rid] == 4 and 0.85 <= image.width / image.height <= 1.20
            and redness >= 0.05
        ]
        assert len(stars) == 1, (
            "source-backed red star JPEG not admitted at exactly four placements: "
            f"resource_count={len(resources)},admitted={len(eligible)},"
            f"picture_uses={sorted(uses.values())}"
        )
        star_id = stars[0]
        star_frames = [node for node in nodes if node["resource_id"] == star_id]
        frame_bounds = [node["bounds"] for node in star_frames]
        assert len({node["node_id"] for node in star_frames}) == 4
        assert len({tuple(bounds[k] for k in ("x", "y", "width", "height"))
                    for bounds in frame_bounds}) == 4
        for bounds in frame_bounds:
            assert bounds["width"] > 0 and bounds["height"] > 0
            assert 0 <= bounds["x"] < page_width
            assert 0 <= bounds["y"] < page_height
        # Footer logo is a distinct wide Adobe-CMYK source; its host-owned
        # derivative must be available independently of the four star uses.
        logos = [
            rid for rid, (image, _) in eligible.items()
            if rid != star_id and 1.95 <= image.width / image.height <= 2.65
            and uses[rid] >= 1
        ]
        assert logos, "wide source-backed SBDC logo preview missing"

        admitted_count = len(eligible)
        row, positive_png = run_browser(
            accepted_path, manifest, root / "positive", head, admitted_count
        )
        paint = row["host_image_paint_proof"]
        assert paint["admitted_image_resources"] == admitted_count
        assert paint["painted_image_resources"] == admitted_count
        assert paint["painted_svg_elements"] >= sum(
            uses[rid] for rid in eligible
        ), "not all source-backed picture placements reached actual Chromium"
        assert paint["all_decoded_and_positive_geometry"] is True

        # Negative control removes ONLY the star derivative. All other
        # images, fonts, page geometry, revision and source stay identical.
        control = json.loads(accepted_path.read_text())
        for res in control["scene"]["resources"]:
            if res["resource_id"] == star_id:
                res.pop("inline_data_url", None)
                res["availability"] = "descriptor_only"
        control_path = root / "no-stars.json"
        control_path.write_text(json.dumps(control, separators=(",", ":")))
        before, negative_png = run_browser(
            control_path, manifest, root / "negative", head
        )
        assert before["page_geometry"] == row["page_geometry"]
        assert before["scene_source"] == row["scene_source"]
        assert before["descriptor_only_resource_count"] == (
            row["descriptor_only_resource_count"] + 1
        )

        visible = Image.open(positive_png).convert("RGB")
        hidden = Image.open(negative_png).convert("RGB")
        assert visible.size == hidden.size
        red_counts = []
        for bounds in frame_bounds:
            area_after = page_crop(visible, bounds, page_width, page_height)
            area_before = page_crop(hidden, bounds, page_width, page_height)
            assert area_after.size == area_before.size
            red_after = sum(
                r > 180 and g < 75 and b < 155 and r - g > 140
                for r, g, b in area_after.getdata()
            )
            red_before = sum(
                r > 180 and g < 75 and b < 155 and r - g > 140
                for r, g, b in area_before.getdata()
            )
            assert red_after >= red_before + 8, (
                "fourth-frame pixel-proof failed: actual red mark is not visible"
            )
            red_counts.append(red_after)
        reference = reference_grid(pair["pages"][0])
        after_diff = compare_grid(image_grid(positive_png), reference)
        before_diff = compare_grid(image_grid(negative_png), reference)
        receipt = {
            "schema": "chaptera.sbdc034-real-guest-four-stars.v1",
            "repository_commit_sha": head,
            "source_sha256": SOURCE_SHA,
            "source_byte_length": SOURCE_BYTES,
            "source_from_existing_pinned_corpus": True,
            "page_count": PAGE_COUNT,
            "host_admitted_image_resources": admitted_count,
            "star_picture_placements": len(star_frames),
            "star_shared_host_resource_count": 1,
            "star_distinct_node_ids": 4,
            "star_distinct_bounds": 4,
            "star_red_pixel_counts": red_counts,
            "logo_wide_resource_admitted": bool(logos),
            "browser_painted_svg_images": paint["painted_svg_elements"],
            "browser_all_images_decoded": True,
            "publisher_grid_changed_cells_with_stars": after_diff["changed_cell_count"],
            "publisher_grid_changed_cells_without_stars": before_diff["changed_cell_count"],
            "user_source_or_scene_bytes_uploaded": False,
            "private_screenshots_uploaded": False,
        }
        # Do not require a global Publisher improvement: unrelated typography
        # and anti-aliasing can dominate a 64x64 coarse grid. Individual star
        # ROIs above provide the strict visible-pixel proof.
        args.receipt_out.parent.mkdir(parents=True, exist_ok=True)
        args.receipt_out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print("SBDC034_REAL_GUEST_STAR_PROOF " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
