#!/usr/bin/env python3
"""Qualify public long-form Publisher fixtures for W2 without trusting filenames."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import urllib.error
import urllib.request
from typing import Any

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
DEFAULT_ITEMS = {
    "285": "November 2017 newsletter.pub",
    "288": "February 2018.pub",
    "299": "March 2018pub.pub",
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch_item(item_id: str, timeout: int) -> tuple[dict[str, Any], bytes]:
    url = f"https://www.klickitatcounty.gov/Archive/ViewFile/Item/{item_id}"
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Chaptera-W2-Fixture-Qualification/1.0",
            "Accept": "*/*",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            data = response.read()
            return (
                {
                    "requested_url": url,
                    "final_url": response.geturl(),
                    "http_status": response.status,
                    "content_type": response.headers.get("Content-Type", ""),
                    "content_disposition": response.headers.get("Content-Disposition", ""),
                    "byte_len": len(data),
                    "sha256": sha256_bytes(data),
                    "magic_hex": data[:8].hex(),
                    "is_cfb": data.startswith(CFB_MAGIC),
                    "fetch_error": None,
                },
                data,
            )
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        return (
            {
                "requested_url": url,
                "final_url": None,
                "http_status": None,
                "content_type": None,
                "content_disposition": None,
                "byte_len": None,
                "sha256": None,
                "magic_hex": None,
                "is_cfb": False,
                "fetch_error": f"{type(error).__name__}: {error}",
            },
            b"",
        )


def inspect_cfb(producer: pathlib.Path, fixture: pathlib.Path) -> dict[str, Any]:
    completed = subprocess.run(
        [str(producer), str(fixture)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        return {
            "chaptera_open_status": "failed",
            "chaptera_error": completed.stderr.decode("utf-8", errors="replace").strip(),
            "page_count": None,
            "scene_node_count": None,
        }
    try:
        value = json.loads(completed.stdout.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        return {
            "chaptera_open_status": "invalid_json",
            "chaptera_error": str(error),
            "page_count": None,
            "scene_node_count": None,
        }

    document = value.get("document") if isinstance(value, dict) else None
    pages = document.get("pages") if isinstance(document, dict) else None
    scene = value.get("scene") if isinstance(value, dict) else None
    nodes = scene.get("nodes") if isinstance(scene, dict) else None
    return {
        "chaptera_open_status": "opened",
        "chaptera_error": None,
        "page_count": len(pages) if isinstance(pages, list) else None,
        "scene_node_count": len(nodes) if isinstance(nodes, list) else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--producer", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--workdir", required=True, type=pathlib.Path)
    parser.add_argument("--timeout", type=int, default=45)
    parser.add_argument("item_ids", nargs="*", default=list(DEFAULT_ITEMS))
    args = parser.parse_args()

    args.workdir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, Any]] = []

    for item_id in args.item_ids:
        meta, data = fetch_item(item_id, args.timeout)
        row: dict[str, Any] = {
            "item_id": item_id,
            "archive_label": DEFAULT_ITEMS.get(item_id),
            **meta,
            "chaptera_open_status": "not_attempted",
            "chaptera_error": None,
            "page_count": None,
            "scene_node_count": None,
            "longform_pub_qualified": False,
        }
        if meta["is_cfb"]:
            fixture = args.workdir / f"klickitat-{item_id}.pub"
            fixture.write_bytes(data)
            row.update(inspect_cfb(args.producer, fixture))
            row["longform_pub_qualified"] = (
                row["chaptera_open_status"] == "opened"
                and isinstance(row["page_count"], int)
                and row["page_count"] >= 10
            )
        rows.append(row)

    result = {
        "schema": "chaptera.w2-longform-fixture-qualification.v1",
        "cfb_magic_required": CFB_MAGIC.hex(),
        "minimum_page_count": 10,
        "candidates": rows,
        "cfb_count": sum(bool(row["is_cfb"]) for row in rows),
        "qualified_count": sum(bool(row["longform_pub_qualified"]) for row in rows),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
