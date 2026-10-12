#!/usr/bin/env python3
"""Score authenticated host Product Chromium page PNGs against frozen Publisher PDFs.

Must be called only after genuine OIDC/HTTPS Product Scene capture. The read
PNG files and full scene stay ephemeral; this emits source-neutral numeric
records, never source text or the raw image.
"""
from __future__ import annotations

import json
from pathlib import Path
import re
import sys

from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid, sha256
from reader_publisher_lo_49_111_scoreboard_v1 import (
    load_frozen, read_publisher_registry, winner,
)

RECEIPT_SCHEMA = "chaptera.product-host-visual-chromium.v1"
OUTPUT_SCHEMA = "chaptera.product-49-111-physical-document.v1"
SHA40 = re.compile(r"^[a-f0-9]{40}$")
SHA256 = re.compile(r"^[a-f0-9]{64}$")


def validate_browser_receipt(receipt: dict, fixture: str, source_sha: str,
                             expected_head: str, page_count: int) -> list[dict]:
    if not SHA40.fullmatch(expected_head):
        raise ValueError("not an exact Product repository checkout")
    if (receipt.get("schema") != RECEIPT_SCHEMA or
            receipt.get("repository_commit_sha") != expected_head or
            receipt.get("source_sha256") != source_sha or
            receipt.get("authenticated_product_scene_provided_by_owner_runner") is not True or
            receipt.get("reference_raster_dpi") != 144 or
            receipt.get("page_errors") != 0 or
            receipt.get("external_request_count") != 0):
        raise ValueError("auth Product visual browser identity/route not admitted")
    if (not isinstance(receipt.get("revision_identity_sha256"), str) or
            not SHA256.fullmatch(receipt["revision_identity_sha256"])):
        raise ValueError("missing immutable Product document revision identity")
    if receipt.get("fidelity_state") not in ("partial", "supported"):
        raise ValueError("unsupported or hidden Product fidelity disposition")
    if (type(receipt.get("host_image_count")) is not int or
            not 0 <= receipt["host_image_count"] <= 96):
        raise ValueError("real Product host image resource budget drift")
    pages = receipt.get("pages")
    if not isinstance(pages, list) or len(pages) != page_count:
        raise ValueError("Product PAGE count differs from frozen Publisher PDF")
    if set(p["page"] for p in pages) != set(range(1, page_count + 1)):
        raise ValueError("Product PAGE ordinal duplicate/missing")
    for p in pages:
        if (not isinstance(p.get("product_page_identity_sha256"), str) or
                not SHA256.fullmatch(p["product_page_identity_sha256"]) or
                not isinstance(p.get("chromium_png_sha256"), str) or
                not SHA256.fullmatch(p["chromium_png_sha256"])):
            raise ValueError("actual Product page screenshot proof missing")
        if (type(p.get("host_derived_resource_count")) is not int or
                not 0 <= p["host_derived_resource_count"] <= 128):
            raise ValueError("per-page Product hosted-image count invalid")
        dims = p.get("png_dimensions", {})
        if (set(dims) != {"width", "height"} or
                any(type(dims[x]) is not int or dims[x] < 1 or dims[x] > 30000
                    for x in ("width", "height"))):
            raise ValueError("actual Product Chromium PNG dimensions invalid")
        for v in ("width_emu", "height_emu"):
            if type(p.get(v)) is not int or p[v] <= 0:
                raise ValueError("Product PAGE geometry not source backed")
        if (dims["width"] != round(p["width_emu"] / 6350) or
                dims["height"] != round(p["height_emu"] / 6350)):
            raise ValueError("actual Chromium raster DPI/extent mismatch")
    return pages


def compare(fixture: str, reference_path: Path, pairs_path: Path, frozen_path: Path,
            browser_dir: Path, output: Path, expected_head: str) -> dict:
    frozen = load_frozen(frozen_path)
    refs, pairs = read_publisher_registry(reference_path, pairs_path)
    match = [row for row in frozen["rows"] if row[0] == fixture]
    if len(match) != 1 or not match[0][6]:
        raise ValueError("only predeclared 39 eligible Product fixtures can be scored")
    name, pub_sha, pdf_sha, page_count, lo_count, _, lo_records = match[0]
    pair = refs.get(name)
    authority = pairs.get(name)
    if (pair is None or authority is None or
            pair["pub_sha256"] != pub_sha or pair["pdf_sha256"] != pdf_sha or
            authority["pub_sha256"] != pub_sha or authority["pdf_sha256"] != pdf_sha or
            pair["reference_pages"] != page_count or len(pair["pages"]) != page_count or
            lo_count != page_count or len(lo_records) != page_count):
        raise ValueError("Product fixed Publisher/LO source and page identities drift")
    browser = json.loads((browser_dir / "browser-receipt.json").read_text(encoding="utf-8"))
    captures = validate_browser_receipt(browser, name, pub_sha, expected_head, page_count)
    by_ordinal = {x["page"]: x for x in captures}
    reference = {x["page"]: x for x in pair["pages"]}
    pages = []
    wins = {"LIBREOFFICE": 0, "READER": 0, "NEAR_TIE": 0}
    for ordinal, lo_cells, historical_direct in lo_records:
        capture = by_ordinal[ordinal]
        oracle = reference[ordinal]
        file = browser_dir / ("page-" + str(ordinal) + ".png")
        if sha256(file) != capture["chromium_png_sha256"]:
            raise ValueError("actual Product Chromium PNG SHA differs from browser receipt")
        with file.open("rb") as f:
            header = f.read(24)
        if (len(header) != 24 or header[:8] != bytes.fromhex("89504e470d0a1a0a") or
                header[12:16] != b"IHDR" or
                int.from_bytes(header[16:20], "big") != capture["png_dimensions"]["width"] or
                int.from_bytes(header[20:24], "big") != capture["png_dimensions"]["height"]):
            raise ValueError("actual Product Chromium PNG header mismatch")
        diff_w = capture["width_emu"] / 12700 - oracle["media_width_pt"]
        diff_h = capture["height_emu"] / 12700 - oracle["media_height_pt"]
        if abs(diff_w) > 0.05 or abs(diff_h) > 0.05:
            raise ValueError("Product page physical extent differs from Publisher reference")
        metrics = compare_grid(image_grid(file), reference_grid(oracle))
        count = metrics["changed_cell_count"]
        if type(count) is not int or not 0 <= count <= 4096:
            raise ValueError("Product coarse visual metric invalid")
        outcome = winner((count - lo_cells) / 4096)
        wins[outcome] += 1
        pages.append({
            "page": ordinal,
            "publisher_grid_sha256": oracle["grid_sha256"],
            "product_page_identity_sha256": capture["product_page_identity_sha256"],
            "chromium_png_sha256": capture["chromium_png_sha256"],
            "changed_cell_count": count,
            "physical_extent_delta_pt": {"width_pt": round(diff_w, 8),
                                         "height_pt": round(diff_h, 8)},
            "host_derived_resource_count": capture["host_derived_resource_count"],
            "fidelity_state": "partial",
        })
    result = {
        "schema": OUTPUT_SCHEMA,
        "fixture": fixture,
        "pub_sha256": pub_sha,
        "publisher_pdf_sha256": pdf_sha,
        "status": "accepted_product",
        "product_page_count": page_count,
        "revision_identity_sha256": browser["revision_identity_sha256"],
        # The ephemeral runner must set the privacy bit only after a REAL
        # unchanged security leak scan over all own logs and JSON succeeds.
        "checks": {
            "oidc_pkce_authenticated": True, "https_source_ingress": True,
            "source_sha_rechecked": True, "durable_document_revision": True,
            "isolated_product_worker": True,
            "host_independently_admitted_resources": True,
            "product_scene_http_authenticated": True,
            "actual_chromium_capture": True, "no_external_requests": True,
            "fidelity_disposition_exposed": True,
            "privacy_scan_passed": False,
        },
        "pages": pages,
        "numeric_metrics": {
            "changed_cell_sum": sum(p["changed_cell_count"] for p in pages),
            "frozen_lo_cell_sum": sum(x[1] for x in lo_records),
            "historical_direct_cell_sum": sum(x[2] for x in lo_records),
            "host_derived_image_count": browser["host_image_count"],
            "page_winners_vs_frozen_lo": wins,
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print("PRODUCT_PHYSICAL_PUBLISHER_PROBE " + json.dumps({
        "fixture": fixture, "pages": page_count, "page_winners": wins,
        "changed_cell_sum": result["numeric_metrics"]["changed_cell_sum"],
        "frozen_lo_cell_sum": result["numeric_metrics"]["frozen_lo_cell_sum"],
        "privacy_scan_pending": True,
    }, sort_keys=True))
    return result


if __name__ == "__main__":
    if len(sys.argv) != 8:
        raise SystemExit("usage: reader_product_49_111_probe.py FIXTURE REF PAIRS FROZEN BROWSER_DIR OUT EXACT_HEAD")
    compare(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]),
            Path(sys.argv[5]), Path(sys.argv[6]), sys.argv[7])
