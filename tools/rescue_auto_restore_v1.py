#!/usr/bin/env python3
"""Offline, evidence-gated per-PUB auto-presentation from existing Rescue bundles.

This module is NOT a PUB repair writer. It never parses CFB, invents pages/text,
or mutates the original/immutable salvage bundle. The existing independently
verified salvage report is the only admission authority.
"""
from __future__ import annotations

import hashlib
import html
import json
from pathlib import Path
import tempfile
from urllib.parse import quote

from tools import rescue_salvage_bundle_v1 as salvage

SCHEMA = "chaptera.rescue-auto-presentation.v1"
TIERS = frozenset({"verified_images", "verified_streams", "verified_text", "unsupported"})
MAX_HTML_BYTES = 8 * 1024 * 1024
MAX_PRESENTATION_BYTES = 16 * 1024
IMAGE_EXTENSIONS = frozenset({".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"})


class AutoRestoreError(ValueError):
    """An existing result is unsafe or no supported presentation can be trusted."""


def _safe_names(manifest: dict, key: str) -> list[str]:
    records = manifest.get(key)
    if not isinstance(records, list) or len(records) > 100_000:
        raise AutoRestoreError("invalid_artifact_manifest")
    names = []
    seen = set()
    for item in records:
        name = item.get("filename" if key == "assets" else "artifact_path") if isinstance(item, dict) else None
        if (not isinstance(name, str) or name in (".", "..")
                or not salvage.SAFE_NAME.fullmatch(name) or name in seen):
            raise AutoRestoreError("unsafe_artifact_name")
        seen.add(name)
        names.append(name)
    return names


def _manifest(bundle: Path, part: str, key: str, expected_count: int) -> list[str]:
    # read_bundle_report already verified exact files, payload hashes and manifests.
    folder = bundle / part
    if folder.is_symlink():
        raise AutoRestoreError("symlink_in_verified_bundle")
    data = salvage.load_json(folder / "manifest.json")
    names = _safe_names(data, key)
    if type(expected_count) is not int or expected_count < 0 or len(names) != expected_count:
        raise AutoRestoreError("verified_count_mismatch")
    if part == "images":
        root = folder
    else:
        root = folder / "streams"
    for name in names:
        file = root / name
        if file.is_symlink() or not file.is_file():
            raise AutoRestoreError("artifact_disappeared")
    return names


def _html_record(digest: str, tier: str, images: list[str], streams: list[str]) -> bytes:
    base = "../../bundles/" + digest + "/"
    result = [
        "<!doctype html>",
        '<html lang="en"><head><meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width, initial-scale=1">',
        '<meta http-equiv="Content-Security-Policy" '
        'content="default-src &#39;none&#39;; img-src &#39;self&#39;; '
        'style-src &#39;unsafe-inline&#39;">',
        "<title>Chaptera Rescue - verified local evidence</title>",
        "<style>body{font:16px system-ui,sans-serif;max-width:1000px;margin:3rem auto;"
        "padding:0 1rem;background:#e9ebf0;color:#222536}"
        "main{padding:1.5rem;border-radius:14px;background:#fff}"
        "a{color:#5540c3;overflow-wrap:anywhere}"
        "img{display:block;max-width:240px;max-height:180px;object-fit:contain;"
        "background:#f5f5f8;margin:.5rem 0}"
        "li{margin:.75rem 0}small{color:#555d70}</style></head><body><main>",
        "<h1>Chaptera Rescue</h1>",
        "<p><strong>Locally verified evidence, not a repaired PUB.</strong> "
        "Page geometry and semantic text have not been reconstructed.</p>",
        "<p>Source SHA-256: <code>" + digest + "</code></p>",
        "<p>Automatic result: <strong>" + html.escape(tier) + "</strong></p>",
    ]
    if images:
        result.extend(["<h2>Verified extracted image assets</h2>", "<ol>"])
        for name in images:
            path = base + "images/" + quote(name, safe="")
            visible = html.escape(name, quote=True)
            result.append('<li><a href="' + path + '">' + visible + "</a>")
            if Path(name).suffix.lower() in IMAGE_EXTENSIONS:
                # The link remains useful if a browser restricts local previews.
                result.append('<img loading="lazy" alt="Extracted image" src="' + path + '">')
            result.append("</li>")
        result.append("</ol>")
    if streams:
        result.extend([
            "<h2>Verified physical CFB streams</h2>",
            "<p><small>These are raw SID carriers, not readable pages or edited PUB files."
            "</small></p><ol>",
        ])
        for name in streams:
            path = base + "physical/streams/" + quote(name, safe="")
            result.append('<li><a href="' + path + '">' +
                          html.escape(name, quote=True) + "</a></li>")
        result.append("</ol>")
    if not images and not streams:
        result.append("<p>No independently verified recoverable payload was found.</p>")
    result.extend([
        "<p><small>Files are referenced from the immutable local Rescue bundle. "
        "Keep both folders together. No network access is required.</small></p>",
        "</main></body></html>",
    ])
    encoded = ("\n".join(result) + "\n").encode("utf-8")
    if len(encoded) > MAX_HTML_BYTES:
        raise AutoRestoreError("presentation_size_limit")
    return encoded


def _render(source: Path, bundle: Path, receipt: dict) -> tuple[dict, bytes]:
    digest = salvage.sha_file(source)
    if (receipt.get("source", {}).get("sha256") != digest
            or receipt.get("source_immutable") is not True
            or receipt.get("native_pub_delivery_allowed") is not False
            or receipt.get("text") != {"status": "not_implemented"}
            or receipt.get("outcome") not in ("partial", "unsupported")
            or bundle.name != digest):
        raise AutoRestoreError("unverified_source_or_native_repair_claim")

    physical = receipt["physical"]
    images = receipt["images"]
    streams = _manifest(bundle, "physical", "streams", physical["stream_count"]) \
        if physical["status"] == "verified" else []
    assets = _manifest(bundle, "images", "assets", images["asset_count"]) \
        if images["status"] == "verified" else []
    if assets:
        tier = "verified_images"
    elif streams:
        tier = "verified_streams"
    else:
        tier = "unsupported"

    page = _html_record(digest, tier, assets, streams)
    data = {
        "schema_version": SCHEMA,
        "source_sha256": digest,
        "tier": tier,
        "image_asset_count": len(assets),
        "physical_stream_count": len(streams),
        "bundle_receipt_sha256": salvage.sha_file(bundle / "bundle.json"),
        "html_sha256": hashlib.sha256(page).hexdigest(),
        "native_pub_repair_attempted": False,
        "native_pub_delivery_allowed": False,
        "semantic_text_reconstructed": False,
        "page_geometry_reconstructed": False,
        "source_immutable": True,
    }
    return data, page


def _json_bytes(data: dict) -> bytes:
    encoded = (json.dumps(data, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("utf-8")
    if len(encoded) > MAX_PRESENTATION_BYTES:
        raise AutoRestoreError("presentation_manifest_size_limit")
    return encoded


def _verify_saved(target: Path, expected_json: bytes, expected_html: bytes) -> None:
    try:
        salvage.check_no_symlinks(target)
        if {entry.name for entry in target.iterdir()} != {"recovery.json", "index.html"}:
            raise AutoRestoreError("unexpected_presentation_artifact")
        report_path, html_path = target / "recovery.json", target / "index.html"
        if (report_path.stat().st_size > MAX_PRESENTATION_BYTES
                or html_path.stat().st_size > MAX_HTML_BYTES):
            raise AutoRestoreError("presentation_read_limit")
        if (report_path.read_bytes() != expected_json
                or html_path.read_bytes() != expected_html):
            raise AutoRestoreError("presentation_integrity_mismatch")
    except OSError as exc:
        raise AutoRestoreError("presentation_unreadable") from exc


def materialize(source: Path, bundle: Path, output_root: Path,
                verified_receipt: dict) -> dict:
    """Create or verify one immutable SHA-owned human-readable local presentation.

    Caller MUST have passed the receipt obtained from salvage.read_bundle_report
    for this source and bundle. This gate is deliberately *not* a second parser.
    """
    salvage.no_symlink_components(source)
    salvage.no_symlink_components(bundle)
    salvage.no_symlink_components(output_root)
    data, page = _render(source, bundle, verified_receipt)
    manifest = _json_bytes(data)
    # Both source and bundle are already verified by the caller; prevent drift
    # even if they change while the HTML is constructed.
    if salvage.sha_file(source) != data["source_sha256"]:
        raise AutoRestoreError("source_changed_during_presentation")
    if salvage.sha_file(bundle / "bundle.json") != data["bundle_receipt_sha256"]:
        raise AutoRestoreError("bundle_changed_during_presentation")

    if output_root.exists() and not output_root.is_dir():
        raise AutoRestoreError("unsafe_presentation_root")
    output_root.mkdir(parents=True, exist_ok=True)
    target = output_root / data["source_sha256"]
    if target.is_symlink():
        raise AutoRestoreError("symlink_presentation")
    if target.exists():
        _verify_saved(target, manifest, page)
        return data
    with tempfile.TemporaryDirectory(prefix=".stage-rescue-presentation-", dir=output_root) as temp:
        stage = Path(temp) / "presentation"
        stage.mkdir()
        (stage / "index.html").write_bytes(page)
        (stage / "recovery.json").write_bytes(manifest)
        _verify_saved(stage, manifest, page)
        if target.exists() or target.is_symlink():
            raise AutoRestoreError("presentation_collision")
        stage.rename(target)
    return data
