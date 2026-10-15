#!/usr/bin/env python3
"""Physical no-login temporary EditorSession CURRENT + MoveNode: exact PUB, HTTPS, TTL."""
from __future__ import annotations

import argparse
import base64
from collections import Counter
from io import BytesIO
import hashlib
import json
import os
import pathlib
import re
from decimal import Decimal
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlencode

import run_cloud_reader_live_acceptance as live
import verify_editable_export_geometry as geometry


def checked(status: int, expected: int, action: str) -> None:
    if status != expected:
        raise AssertionError(f"{action} HTTP {status}, expected {expected}")


def verify_physical_edited_pdf(data: bytes, expected: geometry.RectEmu) -> dict:
    """Independent physical PDF cross-reference + painted MoveNode geometry consumer."""
    if not data.startswith(b"%PDF-") or not data.rstrip().endswith(b"%%EOF"):
        raise AssertionError("edited PDF has invalid header or trailer")
    marker = re.search(rb"startxref\s+(\d+)\s+%%EOF\s*$", data)
    if marker is None:
        raise AssertionError("edited PDF lacks a cross-reference pointer")
    xref = int(marker.group(1))
    if xref <= 0 or xref >= len(data) or not data[xref:].startswith(b"xref\n"):
        raise AssertionError("edited PDF cross-reference pointer does not reopen")
    page_count = data.count(b"/Type /Page /Parent")
    if page_count <= 0 or page_count > 1000:
        raise AssertionError("edited PDF has no admissible physical pages")
    rects = re.findall(
        rb"(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)\s+"
        rb"(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)\s+re\b",
        data,
    )
    # Convert back independently from PDF points. A moved shape must have
    # physical changed coordinates; a valid source-only PDF is insufficient.
    physical = tuple(Decimal(value) for value in (
        expected.x, expected.y, expected.width, expected.height
    ))
    for rect in rects:
        measured = tuple(Decimal(token.decode("ascii")) * 12700 for token in rect)
        if all(abs(a - b) < Decimal("0.01") for a, b in zip(measured, physical)):
            return {"geometry_form": "pdf_rectangle_stream", "pages": page_count}
    raise AssertionError("edited PDF stream does not paint the accepted MoveNode rectangle")


def sbdc_picture_scene_witness(scene: dict) -> dict:
    """Only the documented, exact SBDC034 4-use red CMYK source is admitted."""
    from PIL import Image
    nodes = scene.get("nodes")
    resources = scene.get("resources")
    if not isinstance(nodes, list) or not isinstance(resources, list):
        raise AssertionError("SBDC current Scene has no image graph")
    by_id = {item.get("resource_id"): item for item in resources
             if isinstance(item, dict) and isinstance(item.get("resource_id"), str)}
    image_uses = Counter(
        node.get("resource_id") for node in nodes
        if isinstance(node, dict) and node.get("kind") == "picture_frame"
        and isinstance(node.get("resource_id"), str)
    )
    safe_images = {}
    for rid, resource in by_id.items():
        if resource.get("mime") not in ("image/jpeg", "image/png"):
            continue
        mime = resource["mime"]
        prefix = f"data:{mime};base64,"
        url = resource.get("inline_data_url")
        if resource.get("availability") != "inline_data_url" or not isinstance(url, str):
            continue
        if not url.startswith(prefix):
            raise AssertionError("host image MIME and data URL mismatch")
        raw = base64.b64decode(url[len(prefix):], validate=True)
        if not 0 < len(raw) <= 5 * 1024 * 1024:
            raise AssertionError("SBDC image derivative exceeds safe limit")
        image = Image.open(BytesIO(raw)).convert("RGB")
        image.thumbnail((128, 128))
        pixels = list(image.getdata())
        red_pixels = sum(
            r > 180 and g < 75 and b < 155 and r - g > 140
            for r, g, b in pixels
        )
        safe_images[rid] = (image.width, image.height,
                            red_pixels / max(1, len(pixels)))
    stars = [
        rid for rid, (width, height, red) in safe_images.items()
        if image_uses[rid] == 4
        and 0.85 <= width / height <= 1.20 and red >= 0.05
    ]
    if len(stars) != 1:
        raise AssertionError("SBDC four red source picture frames not admitted")
    star = stars[0]
    positions = {
        node["node_id"]: node["bounds"]
        for node in nodes if node.get("resource_id") == star
    }
    if len(positions) != 4:
        raise AssertionError("SBDC star picture node identity was duplicated")
    logos = [
        rid for rid, (width, height, _) in safe_images.items()
        if rid != star and 1.95 <= width / height <= 2.65 and image_uses[rid] >= 1
    ]
    if not logos:
        raise AssertionError("SBDC wide CMYK logo is not admitted")
    return {"star_resource_id": star, "star_node_bounds": positions,
            "host_image_resources": len(safe_images),
            "logo_resource_id": logos[0], "logo_admitted": True}


def require_sbdc_fixed_pdf_image_uses(data: bytes, star_resource_id: str) -> int:
    """The fixed PDF paint program must invoke one star XObject four times."""
    from uuid import UUID
    canonical = UUID(star_resource_id).hex[:16]
    operator = ("/Im" + canonical + " Do").encode("ascii")
    uses = data.count(operator)
    if uses != 4 or data.count(b"/Subtype /Image") < 2:
        raise AssertionError(
            "SBDC current fixed PDF omitted star XObject uses or the logo"
        )
    return uses


def sbdc_r1_browser_own_pdf_raster(
    fixture: pathlib.Path,
    expected_hash: str,
    classification: str,
    session_id: str,
    baseline_revision_id: str,
    edited_revision_id: str,
    edited_scene: dict,
    pdf_bytes: bytes,
    star_bounds: dict,
    work: pathlib.Path,
) -> dict:
    """Real current HTTPS R1 Scene -> Chromium; same-R1 own PDF -> MuPDF.

    Private scene, screenshot, original media and PDF are confined to work
    and cleaned up by the caller. Only aggregate source-neutral metrics escape.
    """
    import fitz
    from PIL import Image
    from pdf_reference_diff_v1 import compare_rasters

    raster_dir = work / "sbdc-browser-vs-own-pdf"
    raster_dir.mkdir(mode=0o700)
    accepted_scene = raster_dir / "accepted-r1.json"
    manifest = raster_dir / "manifest.json"
    browser_dir = raster_dir / "browser"
    source_size = fixture.stat().st_size
    scene = {
        "protocol_version": "chaptera.live-accepted-temp-editor-scene.v1",
        "source_sha256": expected_hash,
        "source_byte_len": source_size,
        "classification": classification,
        "session_id": session_id,
        "baseline_revision_id": baseline_revision_id,
        "editor_revision_id": edited_revision_id,
        "revision_cursor": 1,
        "isolated_editor_scene_count": 1,
        "scene": edited_scene,
    }
    accepted_scene.write_text(json.dumps(scene, separators=(",", ":")), encoding="utf-8")
    manifest.write_text(json.dumps({
        "schema": "chaptera.cloud-reader-real-fixtures.v1",
        "fixtures": [{
            "name": "sbdc034-accepted-r1",
            "sha256": expected_hash,
            "bytes": source_size,
            "pages": 1,
            "require_render": True,
            "source_path": str(fixture),
        }],
    }), encoding="utf-8")
    env = os.environ.copy()
    env.pop("READER_ACCEPTED_SCENE_JSON", None)
    env.update({
        "READER_REAL_MANIFEST": str(manifest),
        "READER_ACCEPTED_EDITOR_SCENE_JSON": str(accepted_scene),
        "READER_REAL_OUTPUT": str(browser_dir),
        "READER_REFERENCE_RASTER_DPI": "144",
        "READER_REQUIRED_PAINTED_HOST_IMAGES": "4",
        "READER_EPHEMERAL_TEXT_GEOMETRY_JSON": str(raster_dir / "chromium-private-lines.json"),
    })
    subprocess.run(
        ["node", "apps/cloud-reader/real-browser.test.mjs"],
        env=env, cwd=live.ROOT, check=True, timeout=180,
    )
    browser_receipt = json.loads((browser_dir / "receipt.json").read_text())
    row, = browser_receipt["results"]
    if (
        row.get("scene_source") != "accepted_https_temp_editor_service"
        or row.get("source_sha256") != expected_hash
        or row.get("rendered") is not True
        or row.get("host_image_paint_proof", {}).get("admitted_image_resources") != 4
        or row.get("host_image_paint_proof", {}).get("painted_image_resources") != 4
        or row.get("host_image_paint_proof", {}).get("all_decoded_and_positive_geometry") is not True
        or len(row.get("screenshots", [])) != 1
        or browser_receipt.get("errors") or browser_receipt.get("foreign")
    ):
        raise AssertionError("R1 accepted real-editor Scene was not physically painted in Chromium")
    shot = row["screenshots"][0]
    png_path = browser_dir / shot["filename"]
    if live.sha256_file(png_path) != shot["sha256"]:
        raise AssertionError("R1 Chromium screenshot identity drift")
    browser = Image.open(png_path).convert("RGB")

    with fitz.open(stream=pdf_bytes, filetype="pdf") as physical_pdf:
        if physical_pdf.page_count != 1:
            raise AssertionError("R1 own PDF page count differs from current Scene")
        page = physical_pdf[0]
        if page.rotation != 0:
            raise AssertionError("R1 own PDF introduced unsupported page rotation")
        pix = page.get_pixmap(dpi=144, colorspace=fitz.csRGB, alpha=False)
        own_pdf = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
        from sbdc_r1_text_line_geometry import own_pdf_text_rects
        pdf_text_rects = own_pdf_text_rects(page, 144)
    width_diff = browser.width - own_pdf.width
    height_diff = browser.height - own_pdf.height
    if abs(width_diff) > 1 or abs(height_diff) > 1:
        raise AssertionError("R1 browser and own-PDF physical page geometry diverged")
    # Millimetre-to-point and SVG rounding can disagree by one edge pixel.
    width = min(browser.width, own_pdf.width)
    height = min(browser.height, own_pdf.height)
    browser = browser.crop((0, 0, width, height))
    own_pdf = own_pdf.crop((0, 0, width, height))
    page_info, = edited_scene["pages"]
    page_width = page_info["width_emu"]
    page_height = page_info["height_emu"]
    if abs(page_width / 914400 * 144 - width) > 1.5:
        raise AssertionError("R1 raster width does not match source page units")
    if abs(page_height / 914400 * 144 - height) > 1.5:
        raise AssertionError("R1 raster height does not match source page units")

    def red_ink(image: Image.Image, bounds: dict) -> dict:
        left = max(0, round(bounds["x"] * width / page_width))
        top = max(0, round(bounds["y"] * height / page_height))
        right = min(width, round((bounds["x"] + bounds["width"]) * width / page_width))
        bottom = min(height, round((bounds["y"] + bounds["height"]) * height / page_height))
        if right - left < 6 or bottom - top < 6:
            raise AssertionError("current star frame is too small for pixel comparison")
        crop = image.crop((left, top, right, bottom))
        pixels = crop.load()
        red = [
            (x + left, y + top) for y in range(crop.height)
            for x in range(crop.width)
            if (lambda c: c[0] > 180 and c[1] < 75 and c[2] < 155
                and c[0] - c[1] > 140)(pixels[x, y])
        ]
        if len(red) < 8:
            raise AssertionError("a source-backed star has no red pixels in the rendered page")
        return {
            "count": len(red),
            "center_x": sum(p[0] for p in red) / len(red),
            "center_y": sum(p[1] for p in red) / len(red),
        }

    star_metrics = []
    for bounds in star_bounds.values():
        a = red_ink(browser, bounds)
        b = red_ink(own_pdf, bounds)
        delta = ((a["center_x"] - b["center_x"]) ** 2
                 + (a["center_y"] - b["center_y"]) ** 2) ** 0.5
        # Geometry tolerance guards major shifts while allowing distinct
        # uncalibrated CMYK ICC conversion and raster antialiasing.
        tolerance = max(6.0, min(
            bounds["width"] * width / page_width,
            bounds["height"] * height / page_height,
        ) * 0.20)
        if delta > tolerance:
            raise AssertionError("R1 star centroid differs between actual browser and own PDF")
        star_metrics.append({
            "browser_red_pixels": a["count"],
            "pdf_red_pixels": b["count"],
            "center_offset_px": round(delta, 3),
            "center_tolerance_px": round(tolerance, 3),
        })
    diff = compare_rasters(
        {"width": width, "height": height, "samples": browser.tobytes()},
        {"width": width, "height": height, "samples": own_pdf.tobytes()},
    )
    if not diff["raster_size_match"]:
        raise AssertionError("R1 own PDF and browser raster consumer cannot be compared")
    from sbdc_r1_text_line_geometry import compare_actual_text_line_geometry
    private_browser_geometry = json.loads(
        (raster_dir / "chromium-private-lines.json").read_text(encoding="utf-8")
    )
    if (private_browser_geometry.get("schema")
            != "chaptera.private-r1-chromium-text-line-geometry.v1"
            or private_browser_geometry.get("dpi") != 144):
        raise AssertionError("browser text-line geometry is not from same 144dpi R1")
    text_line_geometry = compare_actual_text_line_geometry(
        private_browser_geometry["lines"], pdf_text_rects)
    from sbdc_r1_raster_disagreement_census import census_r1_disagreement
    from sbdc_r1_text_metrics_census import census_r1_text_metrics
    disagreement_census = census_r1_disagreement(
        browser, own_pdf, edited_scene, diff["significant_pixel_count"],
        row.get("browser_preview_census"),
    )
    text_metrics_census = census_r1_text_metrics(
        edited_scene, pdf_bytes, browser, own_pdf, diff["significant_pixel_count"],
    )
    return {
        "schema": "chaptera.sbdc034-r1-browser-ownpdf-raster.v1",
        "disagreement_census": disagreement_census,
        "text_line_geometry": text_line_geometry,
        "text_metrics_census": text_metrics_census,
        "source_r1_exact": True,
        "render_dpi": 144,
        "browser_source": "real_https_temporary_editor_scene",
        "pdf_source": "own_exact_r1_fixed_pdf",
        "page_pixels": [width, height],
        "page_rounding_edge_delta_px": [width_diff, height_diff],
        "browser_all_images_decoded": True,
        "four_star_red_pixels_in_both_rasters": True,
        "star_roi_metrics": star_metrics,
        "full_page_significant_fraction": round(diff["significant_fraction"], 6),
        "full_page_mean_abs_channel_delta": round(diff["mean_abs_channel_delta"], 4),
        "full_page_significant_pixel_count": diff["significant_pixel_count"],
        "fidelity_claim": "partial_not_publisher_or_pixel_exact",
        "private_scene_pdf_png_uploaded": False,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    for key in ("chaptera", "caddy", "fixture", "out"):
        ap.add_argument("--" + key, type=pathlib.Path, required=True)
    ap.add_argument("--fixture-sha256", required=True)
    ap.add_argument("--sbdc-cmyk-picture-proof", action="store_true")
    ap.add_argument("--sbdc-browser-pdf-visual", action="store_true")
    args = ap.parse_args()
    if args.sbdc_browser_pdf_visual and not args.sbdc_cmyk_picture_proof:
        raise AssertionError("browser/PDF raster mode requires exact SBDC source gate")

    fixture = args.fixture.resolve()
    expected_hash = live.sha256_file(fixture)
    if expected_hash != args.fixture_sha256 or not 0 < fixture.stat().st_size < live.FIXTURE_MAX_BYTES:
        raise AssertionError("pinned real PUB source identity/size mismatch")
    if args.sbdc_cmyk_picture_proof and (expected_hash != "0d2278960a0101ced0a1ba6d5922f5a26ea37b76b3ab00ef21e21873359d4a5a" or fixture.stat().st_size != 1019904):
        raise AssertionError("SBDC proof requires the exact existing native Batch01-034 source")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-temp-current-"))
    db, config, wrapper = work / "chaptera.sqlite", work / "config.toml", work / "wrapper.py"
    caddyfile, response = work / "Caddyfile", work / "response.json"
    server_log, caddy_log = out / "chaptera.log", out / "caddy.log"
    appport, httpport, httpsport = live.free_port(), live.free_port(), live.free_port()
    oidc = live.OidcServer(("127.0.0.1", 0))
    oidc_thread = threading.Thread(target=oidc.serve_forever, daemon=True)
    oidc_thread.start()

    live.write_isolation_wrapper(wrapper)
    live.write_config(
        config,
        sqlite_path=db,
        app_port=appport,
        oidc_port=int(oidc.server_address[1]),
        isolation_wrapper=wrapper,
        structural_worker=args.chaptera.resolve(),
        scan_root=work / "scan",
    )
    original_config = config.read_text(encoding="utf-8")
    if original_config.count('provider = "s3-compatible"') != 1:
        raise AssertionError("expected exactly one fixture provider declaration")
    test_config = original_config.replace(
        'provider = "s3-compatible"', 'provider = "filesystem"'
    )
    if args.sbdc_cmyk_picture_proof:
        # The same exact public Origin/Host authority must be used by curl
        # and the real Chromium UI. Keep the canonical unported behavior for
        # all pre-existing non-browser Guest acceptance controls.
        original_origin = f'public_origin = "https://{live.EDGE_HOST}"'
        if test_config.count(original_origin) != 1:
            raise AssertionError("pinned Caddy browser authority not configured")
        test_config = test_config.replace(
            original_origin,
            f'public_origin = "https://{live.EDGE_HOST}:{httpsport}"',
        )
    config.write_text(test_config, encoding="utf-8")
    live.write_caddy(caddyfile, app_port=appport, http_port=httpport, https_port=httpsport)
    site = caddyfile.read_text(encoding="utf-8")
    # Validate the production guest allowlist instead of patching the
    # deployed edge to pass the physical acceptance in CI.
    for path in (
        "/v1/editor/temporary-sessions",
        "/v1/editor/temporary-sessions/*",
        "/editor/temporary-preview",
        "/editor/temporary-edit-move-client-v1.mjs",
    ):
        if path not in site:
            raise AssertionError("Caddy anonymous editor route missing: " + path)
    if "@reader_surface path / /index.html" not in site:
        raise AssertionError("Caddy Reader surface unexpectedly changed")

    env = os.environ.copy()
    env.update(
        {
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS": str(
                live.ROOT / "tools" / "migration_pdf_worker_isolation.py"
            ),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE": str(work / "isolation.txt"),
        }
    )
    caddy_env = os.environ.copy()
    caddy_env.update(
        {"XDG_DATA_HOME": str(work / "caddy-data"), "XDG_CONFIG_HOME": str(work / "caddy-config")}
    )
    s_log = server_log.open("w", encoding="utf-8")
    c_log = caddy_log.open("w", encoding="utf-8")
    server: subprocess.Popen[str] | None = None
    proxy: subprocess.Popen[str] | None = None
    token: str | None = None

    rate_retries = 0

    def call(method: str, path: str, *, headers: list[str] | None = None, **other):
        nonlocal rate_retries
        for attempt in range(8):
            status, raw = live.curl_request(
                https_port=httpsport,
                method=method,
                path=path,
                output=response,
                headers=headers,
                include_origin_port=args.sbdc_cmyk_picture_proof,
                **other,
            )
            if status != 429 or attempt == 7:
                return status, raw
            # Keep adversarial cases inside the real 30/minute burst-six
            # ReaderSessionOpen admission, rather than weakening live policy.
            rate_retries += 1
            time.sleep(2.2)
        raise AssertionError("temporary private reader admission did not converge")

    try:
        live.run([str(args.chaptera.resolve()), "--config", str(config), "migrate", "up"], env=env)
        server = subprocess.Popen(
            [str(args.chaptera.resolve()), "--config", str(config), "serve"],
            cwd=live.ROOT, env=env, text=True, stdout=s_log, stderr=subprocess.STDOUT,
        )
        live.wait_http(appport, server, server_log)
        live.run([str(args.caddy.resolve()), "validate", "--config", str(caddyfile),
                  "--adapter", "caddyfile"], env=caddy_env, cwd=work)
        proxy = subprocess.Popen(
            [str(args.caddy.resolve()), "run", "--config", str(caddyfile),
             "--adapter", "caddyfile"], cwd=work, env=caddy_env,
            text=True, stdout=c_log, stderr=subprocess.STDOUT,
        )
        live.wait_tls(httpsport, proxy, caddy_log)

        status, raw = call(
            "POST", "/v1/editor/temporary-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "temporary capability issuance")
        issued = json.loads(raw)
        session_id, token = issued["session_id"], issued["access_token"]
        if not session_id.startswith("tempedit:") or len(token) != 64:
            raise AssertionError("temporary capability protocol mismatch")
        path = f"/v1/editor/temporary-sessions/{session_id}/current"
        scene_path = f"/v1/editor/temporary-sessions/{session_id}/scene"
        private = [f"x-chaptera-reader-session: {token}"]
        checked(call("GET", path)[0], 401, "no-capability current")
        checked(call("GET", path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign-capability current")
        checked(call("PUT", issued["upload_path"], upload=fixture, headers=private)[0],
                200, "exact temporary PUB upload")
        opened_status, opened_raw = call("POST", issued["open_path"], headers=private)
        checked(opened_status, 200, "real isolated Reader open")
        opened = json.loads(opened_raw)
        if opened.get("classification") not in {"supported", "partial"}:
            raise AssertionError("pinned Publisher source is not Reader-eligible")
        if opened.get("source_sha256") != expected_hash:
            raise AssertionError("Reader source hash differs from immutable upload")
        physical_source_files = [
            p for p in (work / "blob-store").rglob("*")
            if p.is_file() and p.stat().st_size == fixture.stat().st_size
            and live.sha256_file(p) == expected_hash
        ]
        if len(physical_source_files) != 1:
            raise AssertionError("the exact quarantined original PUB is not physically present")
        physical_source = physical_source_files[0]
        expected_expiry = int(opened["expires_at_ms"])
        if not 3_000_000 < expected_expiry - int(time.time() * 1000) <= 3_600_000:
            raise AssertionError("accepted upload did not start one-hour TTL")

        current_status, current_raw = call("GET", path, headers=private)
        checked(current_status, 200, "isolated canonical EditorSession current")
        current = json.loads(current_raw)
        if current.get("protocol_version") != "chaptera.temporary-editor-current.v1":
            raise AssertionError("canonical current protocol invalid")
        if current.get("document_id") != session_id or current.get("source_hash") != expected_hash:
            raise AssertionError("canonical current source/session binding mismatch")
        if current.get("revision_cursor") != 0:
            raise AssertionError("read-only current illegally created a revision")
        if not isinstance(current.get("project"), dict) or not isinstance(
            current.get("authoring_graph"), dict
        ):
            raise AssertionError("isolated EditorSession returned no project or geometry")
        revision = current.get("revision_id", "")
        authoring_revision = current.get("canonical_authoring_revision_id", "")
        if (
            not isinstance(revision, str)
            or not revision.startswith("sha256:")
            or len(revision) != 71
            or not isinstance(authoring_revision, str)
            or len(authoring_revision) != 64
        ):
            raise AssertionError("canonical source-derived revision identity invalid")
        if current.get("expires_at_ms") != expected_expiry:
            raise AssertionError("GET current illegally renewed temporary TTL")

        second_status, second_raw = call("GET", path, headers=private)
        checked(second_status, 200, "idempotent current")
        second = json.loads(second_raw)
        for field in ("source_hash", "project", "revision_id", "expires_at_ms"):
            if second.get(field) != current.get(field):
                raise AssertionError("second current read changed " + field)


        # Physical P0 mutation: use a bounded real-PUB direct geometric node.
        # Rejected unsupported nodes are skipped; the proof is positive only
        # if a canonical isolated MoveNode event is actually committed.
        # The guest editing surface must use the exact source-neutral isolated
        # Reader projection, never the original Guest upload scene.
        checked(call("GET", scene_path)[0], 401, "guest revised scene requires capability")
        checked(call("GET", scene_path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign capability cannot see revised scene")
        scene_status, scene_raw = call("GET", scene_path, headers=private)
        checked(scene_status, 200, "isolated guest exact baseline Reader Scene")
        scene_head = json.loads(scene_raw)
        if (
            scene_head.get("protocol_version") != "chaptera.temporary-editor-scene.v1"
            or scene_head.get("document_id") != session_id
            or scene_head.get("source_hash") != expected_hash
            or scene_head.get("revision_id") != revision
            or scene_head.get("expires_at_ms") != expected_expiry
        ):
            raise AssertionError("private Guest Reader Scene baseline identity/TTL mismatch")
        rich_scene = scene_head.get("scene")
        if (
            not isinstance(rich_scene, dict)
            or rich_scene.get("protocol_version") != "chaptera.reader-scene.v1"
            or rich_scene.get("document_id") != session_id
            or rich_scene.get("source_hash") != expected_hash
            or rich_scene.get("revision_id") != revision
            or not isinstance(rich_scene.get("nodes"), list)
        ):
            raise AssertionError("isolated baseline Reader Scene protocol or revision invalid")
        sbdc_r0 = sbdc_picture_scene_witness(rich_scene) if args.sbdc_cmyk_picture_proof else None
        # Physical P0 no-edit export: the canonical baseline has no event edge.
        # Exercise real Caddy HTTPS, the actual confined renderer, private
        # capabilities, exact identity and independent PDF xref/page reopening.
        original_pdf_query = urlencode({
            "revision_id": revision,
            "canonical_authoring_revision_id": authoring_revision,
        })
        baseline_pdf_path = (
            f"/v1/editor/temporary-sessions/{session_id}/exports/pdf?"
            + original_pdf_query
        )
        checked(call("GET", baseline_pdf_path)[0], 401,
                "original PDF download requires private bearer")
        checked(call("GET", baseline_pdf_path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32),
        ])[0], 404, "another bearer cannot export original PUB to PDF")
        for unchanged_target in ("idml", "odg"):
            checked(call(
                "GET",
                f"/v1/editor/temporary-sessions/{session_id}/exports/"
                + unchanged_target + "?" + original_pdf_query,
                headers=private,
            )[0], 422, "pristine editable exports still require an edit")
        report_status, report_body = call(
            "GET", baseline_pdf_path + "&report_only=true", headers=private,
        )
        checked(report_status, 200, "current baseline PDF LossReport")
        pristine_report = json.loads(report_body)
        if (
            pristine_report.get("protocol_version")
            != "chaptera.temporary-edited-export-report.v1"
            or pristine_report.get("revision_id") != revision
            or pristine_report.get("canonical_authoring_revision_id")
            != authoring_revision
            or pristine_report.get("source_hash") != expected_hash
            or pristine_report.get("target") != "pdf"
            or pristine_report.get("is_baseline") is not True
            or pristine_report.get("expires_at_ms") != expected_expiry
            or not isinstance(pristine_report.get("loss_report"), dict)
            or pristine_report["loss_report"].get("can_serialize") is not True
            or pristine_report["loss_report"].get("counts", {}).get("blocking") != 0
            or pristine_report["loss_report"].get("counts", {}).get("moved_nodes") != 0
        ):
            raise AssertionError("pristine report lacks exact source/zero-edit authority")
        baseline_pdf_headers = work / "baseline-pdf-headers.txt"
        pdf_status, pristine_pdf = call(
            "GET", baseline_pdf_path, headers=private,
            dump_headers=baseline_pdf_headers,
        )
        checked(pdf_status, 200, "physical immutable PUB to PDF download")
        pristine_header_values = live.parse_header_file(baseline_pdf_headers)
        pristine_sha = hashlib.sha256(pristine_pdf).hexdigest()
        if (
            not pristine_pdf.startswith(b"%PDF-")
            or not pristine_pdf.rstrip().endswith(b"%%EOF")
            or not 40 < len(pristine_pdf) <= 16 * 1024 * 1024
            or pristine_sha != pristine_report.get("artifact_sha256")
            or pristine_header_values.get("content-type") != "application/pdf"
            or pristine_header_values.get("x-chaptera-export-sha256") != pristine_sha
            or pristine_header_values.get("x-chaptera-loss-report-sha256")
            != pristine_report.get("loss_report_sha256")
            or pristine_header_values.get("x-chaptera-edit-revision") != revision
            or pristine_header_values.get("cache-control") != "no-store"
            or pristine_header_values.get("x-content-type-options") != "nosniff"
            or 'filename="chaptera-original.pdf"' not in
            pristine_header_values.get("content-disposition", "")
        ):
            raise AssertionError("physical no-edit PDF headers or bytes are not trustworthy")
        pristine_xref = re.search(rb"startxref\s+(\d+)\s+%%EOF\s*$", pristine_pdf)
        if pristine_xref is None:
            raise AssertionError("pristine physical PDF has no terminal xref")
        xref_offset = int(pristine_xref.group(1))
        pristine_pages = pristine_pdf.count(b"/Type /Page /Parent")
        if (
            not 0 < xref_offset < len(pristine_pdf)
            or not pristine_pdf[xref_offset:].startswith(b"xref\n")
            or not 0 < pristine_pages <= 1000
        ):
            raise AssertionError("pristine fixed-output PDF fails independent xref/page consumer")
        sbdc_pdf_r0_uses = (
            require_sbdc_fixed_pdf_image_uses(pristine_pdf, sbdc_r0["star_resource_id"])
            if sbdc_r0 else None
        )
        if live.sha256_file(physical_source) != expected_hash:
            raise AssertionError("original PDF conversion mutated the source PUB")
        with sqlite3.connect(db) as conn:
            pristine_edges = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if pristine_edges:
            raise AssertionError("pristine PDF export fabricated a revision")
        after_pristine_status, after_pristine_raw = call("GET", path, headers=private)
        checked(after_pristine_status, 200, "canonical current after pristine PDF export")
        after_pristine = json.loads(after_pristine_raw)
        if (
            after_pristine.get("revision_cursor") != 0
            or after_pristine.get("revision_id") != revision
            or after_pristine.get("project") != current["project"]
            or after_pristine.get("expires_at_ms") != expected_expiry
        ):
            raise AssertionError("baseline PDF changed canonical state or renewed TTL")

        visible_direct_ids = {
            node["node_id"]
            for node in rich_scene["nodes"]
            if isinstance(node, dict) and isinstance(node.get("node_id"), str)
            and node.get("parent_node_id") is None
            and node.get("origin_node_id") is None
        }
        nodes = current["authoring_graph"].get("nodes")
        if not isinstance(nodes, dict):
            raise AssertionError("canonical real PUB has no node map for editing")
        painted_direct_ids = {
            node["node_id"] for node in rich_scene["nodes"]
            if isinstance(node, dict) and isinstance(node.get("node_id"), str)
            and isinstance(node.get("paint"), dict)
            and (node["paint"].get("fill_rgb") is not None
                 or (isinstance(node["paint"].get("line"), dict)
                     and node["paint"]["line"].get("width_emu", 0) > 0))
        }
        candidates = []
        for node_id, node in sorted(nodes.items()):
            if not isinstance(node_id, str) or not isinstance(node, dict):
                continue
            if node_id not in visible_direct_ids:
                continue
            bounds = node.get("header", {}).get("bounds", {})
            if not isinstance(bounds, dict):
                continue
            x, y = bounds.get("x"), bounds.get("y")
            if type(x) is not int or type(y) is not int:
                continue
            candidates.append((node_id, x, y))
        if not candidates:
            raise AssertionError("pinned real PUB has no canonical geometric nodes")
        # Prefer a true PDF-rectangle-capable MoveNode for the physical PDF oracle.
        candidates.sort(key=lambda item: (item[0] not in painted_direct_ids, item[0]))

        move_path = f"/v1/editor/temporary-sessions/{session_id}/move-node"
        missing_request = {
            "protocol_version": "chaptera.commit-request.v1",
            "document_id": session_id,
            "source_hash": expected_hash,
            "base_revision_id": revision,
            "client_operation_id": "missing-capability",
            "command": {
                "kind": "move_node_to",
                "node_id": candidates[0][0],
                "x_emu": candidates[0][1] + 6350,
                "y_emu": candidates[0][2],
            },
        }
        checked(call("POST", move_path, body_json=missing_request)[0],
                401, "MoveNode requires private token")
        checked(call(
            "POST", move_path,
            headers=["x-chaptera-reader-session: " + secrets.token_hex(32)],
            body_json=missing_request,
        )[0], 404, "MoveNode rejects foreign private token")

        accepted = None
        chosen = None
        unsupported = 0
        for idx, (node_id, x, y) in enumerate(candidates[:24]):
            intent = {
                "protocol_version": "chaptera.commit-request.v1",
                "document_id": session_id,
                "source_hash": expected_hash,
                "base_revision_id": revision,
                "client_operation_id": f"physical-move-{idx}",
                "command": {
                    "kind": "move_node_to",
                    "node_id": node_id,
                    "x_emu": x + 6350,
                    "y_emu": y + 6350,
                },
            }
            status, raw = call("POST", move_path, headers=private, body_json=intent)
            if status == 422:
                unsupported += 1
                continue
            checked(status, 200, "real isolated canonical MoveNode")
            accepted = json.loads(raw)
            chosen = intent
            break
        if accepted is None:
            raise AssertionError(
                f"no supported visible MoveNode in first 24 direct scene nodes; rejected={unsupported}"
            )
        if (
            accepted.get("protocol_version") != "chaptera.temporary-editor-move-node.v1"
            or accepted.get("document_id") != session_id
            or accepted.get("source_hash") != expected_hash
            or accepted.get("base_revision_id") != revision
            or accepted.get("revision_id") == revision
            or accepted.get("replayed") is not False
            or accepted.get("scene_refresh") != "current"
            or int(accepted.get("expires_at_ms", 0)) <= expected_expiry
        ):
            raise AssertionError("accepted MoveNode does not prove a fresh source-bound revision")
        move_expiry = int(accepted["expires_at_ms"])
        if not 3_000_000 < move_expiry - int(time.time() * 1000) <= 3_600_000:
            raise AssertionError("accepted MoveNode did not atomically refresh one-hour TTL")

        reread_status, reread_raw = call("GET", path, headers=private)
        checked(reread_status, 200, "reopen edited canonical revision")
        reopened = json.loads(reread_raw)
        if (
            reopened.get("revision_id") != accepted["revision_id"]
            or reopened.get("revision_cursor") != 1
            or reopened.get("expires_at_ms") != move_expiry
            or reopened.get("source_hash") != expected_hash
            or reopened.get("project") == current["project"]
            or live.sha256_file(physical_source) != expected_hash
        ):
            raise AssertionError("edited canonical state was not reopened or original PUB mutated")

        chosen_id = chosen["command"]["node_id"]
        before_bounds = current["authoring_graph"]["nodes"][chosen_id]["header"]["bounds"]
        after_bounds = reopened["authoring_graph"]["nodes"][chosen_id]["header"]["bounds"]
        if (
            after_bounds.get("x") != chosen["command"]["x_emu"]
            or after_bounds.get("y") != chosen["command"]["y_emu"]
            or after_bounds == before_bounds
        ):
            raise AssertionError("canonical reopened Scene did not reflect the accepted MoveNode geometry")


        edited_scene_status, edited_scene_raw = call("GET", scene_path, headers=private)
        checked(edited_scene_status, 200, "private exact edited Reader Scene after MoveNode")
        edited_scene_reply = json.loads(edited_scene_raw)
        if (
            edited_scene_reply.get("protocol_version") != "chaptera.temporary-editor-scene.v1"
            or edited_scene_reply.get("document_id") != session_id
            or edited_scene_reply.get("source_hash") != expected_hash
            or edited_scene_reply.get("revision_id") != accepted["revision_id"]
            or edited_scene_reply.get("expires_at_ms") != move_expiry
        ):
            raise AssertionError("edited Guest Scene is not the exact accepted revision")
        edited_rich_scene = edited_scene_reply.get("scene")
        if not isinstance(edited_rich_scene, dict) or not isinstance(edited_rich_scene.get("nodes"), list):
            raise AssertionError("edited Guest Scene lost source-neutral page geometry")
        matches = [
            node for node in edited_rich_scene["nodes"]
            if isinstance(node, dict) and node.get("node_id") == chosen_id
        ]
        if len(matches) != 1 or (
            matches[0].get("bounds", {}).get("x") != after_bounds["x"]
            or matches[0].get("bounds", {}).get("y") != after_bounds["y"]
        ):
            raise AssertionError("real edited Reader Scene did not paint moved canonical node")
        sbdc_r1 = sbdc_picture_scene_witness(edited_rich_scene) if sbdc_r0 else None
        if sbdc_r0:
            if sbdc_r1["star_resource_id"] != sbdc_r0["star_resource_id"]:
                raise AssertionError("accepted edit changed the source star resource identity")
            for node_id, old_bounds in sbdc_r0["star_node_bounds"].items():
                if node_id != chosen_id and sbdc_r1["star_node_bounds"].get(node_id) != old_bounds:
                    raise AssertionError("unmodified star frame moved during canonical R1 replay")
        if live.sha256_file(physical_source) != expected_hash:
            raise AssertionError("revised Scene changed the immutable original PUB")

        # The very same pinned moved revision must produce an actual modified
        # editable package, not a converter re-reading immutable PUB bytes.
        expected_rect = geometry.RectEmu(
            x=after_bounds["x"], y=after_bounds["y"],
            width=after_bounds["width"], height=after_bounds["height"],
        )
        query = urlencode({
            "revision_id": accepted["revision_id"],
            "canonical_authoring_revision_id": accepted["canonical_authoring_revision_id"],
        })
        edited_formats = []
        unavailable_formats = []
        export_receipts = []
        sbdc_pdf_multiline_counts = None
        sbdc_visual_pair = None
        # The exact SBDC034 visual proof only owns the current fixed PDF.
        # Imported picture objects need not be editable ODG/IDML frames.
        # Keep the original all-format geometry gate for other real fixtures.
        for target in (("pdf",) if sbdc_r0 else ("odg", "idml", "pdf")):
            export_path = f"/v1/editor/temporary-sessions/{session_id}/exports/{target}?{query}"
            checked(call("GET", export_path)[0], 401, "private edited export capability")
            checked(call(
                "GET", export_path,
                headers=["x-chaptera-reader-session: " + secrets.token_hex(32)],
            )[0], 404, "foreign token cannot download edited output")
            pinned_stale = urlencode({
                "revision_id": revision,
                "canonical_authoring_revision_id": authoring_revision,
            })
            checked(call(
                "GET", export_path.split("?")[0] + "?" + pinned_stale,
                headers=private,
            )[0], 409, "original revision cannot be substituted for edited artifact")
            head_status, head_raw = call(
                "GET", export_path + "&report_only=true", headers=private,
            )
            if head_status == 422:
                if target == "pdf":
                    raise AssertionError("current edited PDF is blocked for the pinned real PUB")
                unavailable_formats.append(target)
                continue
            checked(head_status, 200, "private current edit LossReport")
            preflight = json.loads(head_raw)
            if (
                preflight.get("protocol_version") != "chaptera.temporary-edited-export-report.v1"
                or preflight.get("source_hash") != expected_hash
                or preflight.get("revision_id") != accepted["revision_id"]
                or preflight.get("canonical_authoring_revision_id") != accepted["canonical_authoring_revision_id"]
                or preflight.get("target") != target
                or preflight.get("expires_at_ms") != move_expiry
                or not isinstance(preflight.get("loss_report"), dict)
                or preflight["loss_report"].get("can_serialize") is not True
                or preflight["loss_report"].get("counts", {}).get("blocking") != 0
            ):
                raise AssertionError("preflight did not prove exact amended canonical output")
            headers_file = work / ("output-" + target + "-headers.txt")
            status, artifact = call(
                "GET", export_path, headers=private, dump_headers=headers_file,
            )
            checked(status, 200, "physical edited " + target + " download")
            headers_out = live.parse_header_file(headers_file)
            actual_sha = hashlib.sha256(artifact).hexdigest()
            if (
                not artifact.startswith(b"%PDF-" if target == "pdf" else b"PK")
                or actual_sha != preflight["artifact_sha256"]
                or headers_out.get("x-chaptera-export-sha256") != actual_sha
                or headers_out.get("x-chaptera-loss-report-sha256") != preflight["loss_report_sha256"]
                or headers_out.get("x-chaptera-edit-revision") != accepted["revision_id"]
                or headers_out.get("cache-control") != "no-store"
                or headers_out.get("x-content-type-options") != "nosniff"
                or not headers_out.get("content-disposition", "").startswith("attachment; filename=")
                or target not in headers_out.get("content-disposition", "")
            ):
                raise AssertionError("edited package response MIME/identity/private download mismatch")
            expected_mime = {
                "odg": "application/vnd.oasis.opendocument.graphics",
                "idml": "application/vnd.adobe.indesign-idml-package",
                "pdf": "application/pdf",
            }[target]
            if headers_out.get("content-type") != expected_mime:
                raise AssertionError("physical edited output MIME was substituted")
            local_artifact = work / ("edited-" + target + "." + target)
            local_artifact.write_bytes(artifact)
            try:
                observed = (
                    verify_physical_edited_pdf(artifact, expected_rect)
                    if target == "pdf" else
                    geometry.verify_export(local_artifact, target, chosen_id, expected_rect)
                )
            except (AssertionError, ValueError) as exc:
                raise AssertionError(
                    f"edited {target} did not preserve the accepted physical geometry"
                ) from exc
            if target == "pdf" and sbdc_r1:
                proof_counts = preflight["loss_report"]["counts"]
                sbdc_pdf_multiline_counts = {
                    "source_sized_multiline_stories": proof_counts.get("source_sized_multiline_stories", 0),
                    "source_sized_multiline_lines": proof_counts.get("source_sized_multiline_lines", 0),
                    "rejected_stories_by_reason": proof_counts.get("source_shared_rejections_by_reason", {}),
                    "rejected_reader_lines_by_reason": proof_counts.get("source_shared_rejected_lines_by_reason", {}),
                    "total_shaped_lines": proof_counts["shaped_lines"],
                }
                sbdc_pdf_r1_uses = require_sbdc_fixed_pdf_image_uses(
                    artifact, sbdc_r1["star_resource_id"]
                )
                if args.sbdc_browser_pdf_visual:
                    sbdc_visual_pair = sbdc_r1_browser_own_pdf_raster(
                        fixture, expected_hash, opened["classification"],
                        session_id, revision, accepted["revision_id"],
                        edited_rich_scene, artifact, sbdc_r1["star_node_bounds"], work,
                    )
            export_receipts.append({
                "target": target, "sha256": actual_sha,
                "artifact_length": len(artifact),
                "geometry_form": observed.get("geometry_form"),
            })
            edited_formats.append(target)
        if not edited_formats:
            raise AssertionError(
                "both editable formats blocked or failed the physical edited-geometry oracle: "
                + repr(unavailable_formats)
            )
        if list(work.rglob("edited-export.bin")):
            raise AssertionError("isolated worker retained private edited-export scratch bytes")
        if live.sha256_file(physical_source) != expected_hash:
            raise AssertionError("physical edit export modified immutable source PUB")
        if "pdf" not in edited_formats:
            raise AssertionError("temporary guest output never proved physical changed PDF")

        # The existing source-exact SBDC R1 flow has now proved real Rust
        # authoring/replay and FOUR physical star XObject paints in the own PDF.
        # Prove the very same accepted R1 in the actual Chromium Editor DOM
        # and page pixels, not a synthetic Reader scene or source-only preview.
        sbdc_browser_r1 = None
        if sbdc_r1:
            star_ids = set(sbdc_r1["star_node_bounds"])
            star_frames = [
                {"node_id": node["node_id"], "page_id": node["page_id"],
                 "bounds": node["bounds"]}
                for node in edited_rich_scene["nodes"]
                if node.get("node_id") in star_ids
            ]
            if len(star_frames) != 4:
                raise AssertionError("R1 browser witness cannot bind four source nodes")
            # The bearer is only written to a 0600 file inside this runner's
            # private temporary directory and is immediately unlinked again.
            # It is never passed on a command line or exported as a receipt.
            browser_private = work / "sbdc-r1-browser-private.json"
            browser_input = {
                "schema": "chaptera.sbdc034-private-r1-browser-input.v1",
                "source_sha256": expected_hash,
                "browser_record": {
                    "schema": "chaptera.temporary-edit.browser.v0",
                    "session_id": session_id,
                    "access_token": token,
                    "expires_at_ms": move_expiry,
                    "source_sha256": expected_hash,
                },
                "expected": {
                    "revision_id": accepted["revision_id"],
                    "star_resource_id": sbdc_r1["star_resource_id"],
                    "logo_resource_id": sbdc_r1["logo_resource_id"],
                    "star_frames": star_frames,
                },
            }
            fd = os.open(browser_private, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            try:
                with os.fdopen(fd, "w", encoding="utf-8") as handle:
                    json.dump(browser_input, handle, separators=(",", ":"))
                executed = subprocess.run(
                    [
                        "node", "tools/run_temporary_editor_sbdc034_r1_browser.mjs",
                        "--origin", f"https://{live.EDGE_HOST}:{httpsport}",
                        "--private-input", str(browser_private),
                    ],
                    cwd=live.ROOT, env=env, text=True, capture_output=True,
                    timeout=180, check=False,
                )
            finally:
                browser_private.unlink(missing_ok=True)
            if executed.returncode != 0:
                # One bounded source-safe failure code, never a raw Playwright
                # stack: an exception may otherwise reveal a private URL or
                # temporary bearer/session identity in an Actions log.
                lines = [
                    line.split(" ", 1)[1]
                    for line in executed.stderr.splitlines()
                    if line.startswith("SBDC034_R1_BROWSER_FAILURE ")
                ]
                evidence = json.loads(lines[-1]) if lines else {}
                stage = evidence.get("stage")
                code = evidence.get("code")
                allowed_stages = {
                    "browser_launch", "browser_load", "resume_ui",
                    "revision_readback", "render_dom", "r1_screenshot",
                    "r1_pixel_scan", "source_safe_receipt",
                }
                allowed_codes = {
                    "browser_origin_http", "missing_picture_dom", "image_transport",
                    "logo_dom_or_decode", "pixel_roi", "revision_identity",
                    "browser_timeout", "network_or_origin",
                    "assertion_unclassified", "browser_unclassified",
                }
                if stage not in allowed_stages:
                    stage = "unclassified_stage"
                if code not in allowed_codes:
                    code = "unclassified_code"
                numeric_status = evidence.get("http_status")
                status_note = (
                    f"/http{numeric_status}"
                    if code == "browser_origin_http"
                    and type(numeric_status) is int
                    and 100 <= numeric_status <= 599
                    else ""
                )
                raise AssertionError(
                    "SBDC R1 Chromium Editor four-ROI paint proof failed: "
                    + stage + "/" + code + status_note
                )
            physical_lines = [
                line.split(" ", 1)[1] for line in executed.stdout.splitlines()
                if line.startswith("SBDC034_EDITOR_R1_BROWSER_PROOF ")
            ]
            if len(physical_lines) != 1:
                raise AssertionError("SBDC R1 Chromium produced no bounded numeric proof")
            sbdc_browser_r1 = json.loads(physical_lines[0])
            counts = sbdc_browser_r1.get("r1_star_additional_red_pixels")
            if (sbdc_browser_r1.get("source_sha256") != expected_hash
                or sbdc_browser_r1.get("accepted_revision_matches_browser") is not True
                or sbdc_browser_r1.get("actual_editor_svg_star_nodes") != 4
                or sbdc_browser_r1.get("actual_editor_logo_images", 0) < 1
                or not isinstance(counts, list) or len(counts) != 4
                or any(not isinstance(x, int) or x < 8 for x in counts)
                or sbdc_browser_r1.get("browser_page_errors") != 0
                or sbdc_browser_r1.get("outbound_network_requests") != 0):
                raise AssertionError("SBDC R1 browser receipt did not prove all four ROIs")
            print("SBDC034_EDITOR_R1_BROWSER_PROOF " + json.dumps({
                "star_additional_red_pixel_counts": counts,
                "logo_browser_painted": True,
                "revision_bound": True,
                "browser_errors": 0,
                "private_payload_uploaded": False,
            }, sort_keys=True))

        replay_status, replay_raw = call(
            "POST", move_path, headers=private, body_json=chosen
        )
        checked(replay_status, 200, "exact-idempotency MoveNode retry")
        replay = json.loads(replay_raw)
        if (
            replay.get("replayed") is not True
            or replay.get("revision_id") != accepted["revision_id"]
            or replay.get("expires_at_ms") != move_expiry
        ):
            raise AssertionError("replayed operation illegally inserted a new revision or moved TTL")
        conflicting = json.loads(json.dumps(chosen))
        conflicting["command"]["x_emu"] += 1
        checked(call("POST", move_path, headers=private, body_json=conflicting)[0],
                409, "same operation ID with changed intent conflicts")
        stale = json.loads(json.dumps(chosen))
        stale["client_operation_id"] = "physical-stale-after-move"
        checked(call("POST", move_path, headers=private, body_json=stale)[0],
                409, "stale base cannot commit another MoveNode")
        final_status, final_raw = call("GET", path, headers=private)
        checked(final_status, 200, "current state after rejected replays")
        final = json.loads(final_raw)
        if final["revision_cursor"] != 1 or final["expires_at_ms"] != move_expiry:
            raise AssertionError("failed/replayed operations modified revision count or TTL")

        status, raw = call(
            "POST", "/v1/reader/guest-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "ordinary Guest Reader session issuance")
        guest = json.loads(raw)
        checked(call(
            "GET", f"/v1/editor/temporary-sessions/{guest['session_id']}/current",
            headers=[f"x-chaptera-reader-session: {guest['access_token']}"],
        )[0], 404, "ordinary guest forbidden from authoring current")

        with sqlite3.connect(db) as conn:
            conn.execute(
                "UPDATE reader_guest_sessions SET expires_at_ms=? WHERE session_id=?",
                (int(time.time() * 1000) - 1, session_id.encode()),
            )
        expired_status = call("GET", path, headers=private)[0]
        if expired_status not in {404, 410}:
            raise AssertionError("expired capability retained document access")
        checked(call("GET", scene_path, headers=private)[0], expired_status,
                "expired bearer may not read private edited Scene")
        checked(call(
            "GET", f"/v1/editor/temporary-sessions/{session_id}/exports/{edited_formats[0]}?{query}",
            headers=private,
        )[0], expired_status, "expired bearer may not download private edited artifact")
        with sqlite3.connect(db) as conn:
            remaining = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions WHERE session_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if remaining:
            raise AssertionError("expired temporary session kept its private metadata")
        if physical_source.exists():
            raise AssertionError("expired temporary session retained original PUB bytes")
        with sqlite3.connect(db) as conn:
            expired_revisions = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
            expired_bindings = conn.execute(
                "SELECT COUNT(*) FROM revision_identity_bindings WHERE document_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if expired_revisions or expired_bindings:
            raise AssertionError("TTL deletion retained private canonical revision bytes")

        # A separate real-PUB source proves user-triggered cleanup while the
        # session is still live; TTL expiration above remains independently tested.
        delete_issue_status, delete_raw = call(
            "POST", "/v1/editor/temporary-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(delete_issue_status, 200, "early-delete capability issuance")
        delete_issue = json.loads(delete_raw)
        delete_id = delete_issue["session_id"]
        delete_token = delete_issue["access_token"]
        if not delete_id.startswith("tempedit:") or len(delete_token) != 64:
            raise AssertionError("early-delete issued invalid guest identity")
        delete_path = f"/v1/editor/temporary-sessions/{delete_id}"
        delete_private = [f"x-chaptera-reader-session: {delete_token}"]
        checked(call("DELETE", delete_path)[0], 401, "delete requires own private capability")
        checked(call("DELETE", delete_path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign bearer cannot delete source")
        checked(call("PUT", delete_issue["upload_path"], upload=fixture,
                     headers=delete_private)[0], 200, "early-delete source upload")
        new_open_status, new_open_raw = call(
            "POST", delete_issue["open_path"], headers=delete_private,
        )
        checked(new_open_status, 200, "early-delete real PUB open")
        new_open = json.loads(new_open_raw)
        if new_open.get("classification") not in {"supported", "partial"}:
            raise AssertionError("early-delete source unexpectedly not Reader eligible")
        if new_open.get("source_sha256") != expected_hash:
            raise AssertionError("early-delete original source identity mismatch")
        new_source_files = [
            candidate for candidate in (work / "blob-store").rglob("*")
            if candidate.is_file() and candidate.stat().st_size == fixture.stat().st_size
            and live.sha256_file(candidate) == expected_hash
        ]
        if len(new_source_files) != 1:
            raise AssertionError("early-delete original source not physically present")
        new_physical_source = new_source_files[0]
        # Destroy an actual edited RevisionStream, not only a read-only upload.
        delete_current_status, delete_current_raw = call(
            "GET", delete_path + "/current", headers=delete_private,
        )
        checked(delete_current_status, 200, "early-delete canonical baseline")
        delete_current = json.loads(delete_current_raw)
        source_node = delete_current.get("authoring_graph", {}).get("nodes", {}).get(chosen_id)
        source_bounds = source_node.get("header", {}).get("bounds") if isinstance(source_node, dict) else None
        if not isinstance(source_bounds, dict):
            raise AssertionError("same real PUB lost deterministic moveable NodeId")
        delete_move = {
            "protocol_version": "chaptera.commit-request.v1",
            "document_id": delete_id,
            "source_hash": expected_hash,
            "base_revision_id": delete_current["revision_id"],
            "client_operation_id": "physical-delete-move",
            "command": {
                "kind": "move_node_to", "node_id": chosen_id,
                "x_emu": source_bounds["x"] + 6350,
                "y_emu": source_bounds["y"] + 6350,
            },
        }
        delete_move_status, delete_move_raw = call(
            "POST", delete_path + "/move-node",
            headers=delete_private, body_json=delete_move,
        )
        checked(delete_move_status, 200, "real canonical edit before explicit delete")
        delete_edit = json.loads(delete_move_raw)
        if delete_edit.get("revision_id") == delete_current["revision_id"]:
            raise AssertionError("delete test did not first create an edited revision")
        with sqlite3.connect(db) as conn:
            early_edges_before = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
        if early_edges_before != 1:
            raise AssertionError("delete test did not create one private revision edge")
        with sqlite3.connect(db) as conn:
            active_before_delete = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions "
                "WHERE session_id=? AND state='opened' AND expires_at_ms>?",
                (delete_id.encode(), int(time.time() * 1000)),
            ).fetchone()[0]
        if active_before_delete != 1:
            raise AssertionError("early-delete target was not a live opened session")
        delete_headers = work / "early-delete-response-headers.txt"
        delete_status, _ = call(
            "DELETE", delete_path, headers=delete_private,
            dump_headers=delete_headers,
        )
        checked(delete_status, 204, "explicit physical early-delete acknowledgement")
        if live.parse_header_file(delete_headers).get("cache-control") != "no-store":
            raise AssertionError("early-delete response was cacheable")
        for action in (
            f"/v1/editor/temporary-sessions/{delete_id}/current",
            f"/v1/editor/temporary-sessions/{delete_id}/scene",
            delete_issue["scene_path"],
        ):
            checked(call("GET", action, headers=delete_private)[0],
                    404, "deleted capability retained a private Reader endpoint")
        checked(call("DELETE", delete_path, headers=delete_private)[0],
                404, "deleted private capability resurrected on duplicate request")
        after_delete_query = urlencode({
            "revision_id": delete_edit["revision_id"],
            "canonical_authoring_revision_id": delete_edit["canonical_authoring_revision_id"],
        })
        checked(call(
            "GET", delete_path + "/exports/idml?" + after_delete_query,
            headers=delete_private,
        )[0], 404, "deleted bearer could download edited canonical bytes")
        if new_physical_source.exists():
            raise AssertionError("user deletion retained quarantined original PUB bytes")
        with sqlite3.connect(db) as conn:
            early_remaining = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions WHERE session_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
            early_edges = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
            early_bindings = conn.execute(
                "SELECT COUNT(*) FROM revision_identity_bindings WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
        if early_remaining or early_edges or early_bindings:
            raise AssertionError("user delete did not purge private session/revision rows")

        receipt = {
            "schema": "chaptera.tempedit-canonical-current-https.v1",
            "source_sha256": expected_hash, "source_byte_len": fixture.stat().st_size,
            "canonical_current_status": current_status, "revision_cursor": 0,
            "private_capability_required": True, "foreign_token_rejected": True,
            "ordinary_guest_rejected": True, "no_read_ttl_extension": True,
            "expired_access_status": expired_status, "expired_rows": remaining, "physical_original_deleted": True,
            "isolated_real_pub_read": True, "backend_filesystem": True,
            "raw_source_or_bearer_in_receipt": False,
            "mutation_proven": True, "changed_revision_reopened": True,
            "exact_replay_ttl_unchanged": True,
            "foreign_mutation_capability_rejected": True,
            "failed_and_stale_ops_rejected": True,
            "bounded_rate_retries": rate_retries,
            "private_revision_rows_after_ttl": expired_revisions,
            "private_canonical_bindings_after_ttl": expired_bindings,
            "pristine_pdf_without_edit_proven": True,
            "pristine_pdf_sha256": pristine_sha,
            "pristine_pdf_pages": pristine_pages,
            "pristine_pdf_revision_edges": pristine_edges,
            "edited_download_proven": True, "edited_fixed_pdf_proven": "pdf" in edited_formats,
            "edited_target_profiles": edited_formats,
            "revised_reader_scene_proven": True, "revised_node_id": chosen_id,
            "editable_unavailable_targets": unavailable_formats,
            "edited_artifact_receipts": export_receipts,
            "early_delete_proven": True, "early_delete_status": delete_status,
            "early_delete_rows_after_purge": early_remaining,
            "early_delete_edited_revisions_before": early_edges_before,
            "early_delete_physical_original_removed": True,
            "early_delete_foreign_capability_rejected": True,
            "persistent_export_artifacts": False, "v0_go": False,
            **({"sbdc_browser_own_pdf_r1": sbdc_visual_pair} if sbdc_visual_pair else {}),
            **({"sbdc_picture_proof": {
                "source_exact_batch01_034": True,
                "source_star_picture_nodes_r0": len(sbdc_r0["star_node_bounds"]),
                "source_star_picture_nodes_r1": len(sbdc_r1["star_node_bounds"]),
                "shared_star_resource_preserved": True,
                "host_image_resources_r0": sbdc_r0["host_image_resources"],
                "host_image_resources_r1": sbdc_r1["host_image_resources"],
                "cmyk_logo_admitted": sbdc_r1["logo_admitted"],
                "fixed_pdf_star_paint_uses_r0": sbdc_pdf_r0_uses,
                "fixed_pdf_star_paint_uses_r1": sbdc_pdf_r1_uses,
                "pdf_source_sized_multiline": sbdc_pdf_multiline_counts,
                "unmodified_star_bounds_preserved_after_move": True,
                "star_xobject_emitted_physically": True,
                "source_bytes_or_picture_payload_reported": False,
                "r1_editor_chromium_star_pixel_counts": sbdc_browser_r1["r1_star_additional_red_pixels"],
                "r1_editor_chromium_logo_painted": True,
                "r1_editor_chromium_revision_bound": True,
                "r1_editor_chromium_private_images_uploaded": False,
            }} if sbdc_r0 else {}),
        }
        (out / "receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps({"canonical_current_status": current_status, "mutation_proven": True,
                          "edited_download_proven": True, "targets": edited_formats, "v0_go": False}))
    finally:
        live.terminate(proxy)
        live.terminate(server)
        c_log.close()
        s_log.close()
        oidc.shutdown()
        oidc.server_close()
        oidc_thread.join(timeout=2)
        if token is not None:
            for log in (server_log, caddy_log):
                if token in log.read_text(encoding="utf-8", errors="replace"):
                    raise AssertionError("private capability appeared in diagnostic logs")
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
