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
PINNED_CORPUS = [
    {
        "id": "springnewsletter2017",
        "label": "Springnewsletter2017.pub",
        "sha256": "0ec56aa214799a6d52cf164395dacf040c2da2130039b7a622a57a2fbf088c60",
        "size_bytes": 1107456,
    },
    {
        "id": "client-newsletter",
        "label": "Client newsletter.pub",
        "sha256": "1bcf3231ec598244dfad82e22824964c750d1ec25ed0a40b834b537ca1589930",
        "size_bytes": 229888,
    },
    {
        "id": "classroom-newsletter",
        "label": "lbennett.pub classroom newsletter",
        "sha256": "4ef8c8fb4402069386a5dc748c0c5a1f7e02d3012e49fe279bb00e2e049cec6e",
        "size_bytes": 164864,
    },
    {
        "id": "newsletter-template",
        "label": "newsletter-template.pub",
        "sha256": "98dce97613869c6919f273202b4fb2427ba62d9e9efce309eb8bec65a6499574",
        "size_bytes": 187392,
    },
    {
        "id": "may-2023-newsletter",
        "label": "may 2023 newsletter.pub",
        "sha256": "aeac4c03181582008c18655ad77b90a957b00eeddcc0b1c45f6d40ca88c765eb",
        "size_bytes": 4286464,
    },
    {
        "id": "january-2021-calendar",
        "label": "January 2021 Calendar.pub",
        "sha256": "c39a76257106881bd07fe94560d9b4c8c756de05746941816806c75aaf4e728b",
        "size_bytes": 846848,
    },
    {
        "id": "modern2c-officeart-wmf",
        "label": "05-modern2c-officeart-wmf",
        "sha256": "99ffa361aa8d0ba3a851ff86986bede358743945b6a8ea2e6bf19401bb00cd06",
        "size_bytes": 342016,
    },
]

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
                    "http_success": 200 <= response.status < 300,
                    "content_type": response.headers.get("Content-Type", ""),
                    "content_disposition": response.headers.get("Content-Disposition", ""),
                    "byte_len": len(data),
                    "sha256": sha256_bytes(data),
                    "magic_hex": data[:8].hex(),
                    "is_cfb": 200 <= response.status < 300 and data.startswith(CFB_MAGIC),
                    "fetch_error": None,
                },
                data,
            )
    except urllib.error.HTTPError as error:
        data = error.read()
        return (
            {
                "requested_url": url,
                "final_url": error.geturl(),
                "http_status": error.code,
                "http_success": False,
                "content_type": error.headers.get("Content-Type", ""),
                "content_disposition": error.headers.get("Content-Disposition", ""),
                "byte_len": len(data),
                "sha256": sha256_bytes(data),
                "magic_hex": data[:8].hex(),
                "is_cfb": False,
                "fetch_error": f"HTTPError: {error}",
            },
            data,
        )
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        return (
            {
                "requested_url": url,
                "final_url": None,
                "http_status": None,
                "http_success": False,
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


def qualify_pinned_corpus(
    producer: pathlib.Path,
    workdir: pathlib.Path,
    timeout: int,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    base = "https://raw.githubusercontent.com/Lalalalendia/lalamu/main/pub-corpus/corpus/native/unclassified"
    for candidate in PINNED_CORPUS:
        url = f"{base}/{candidate['sha256']}.pub"
        request = urllib.request.Request(
            url,
            headers={
                "User-Agent": "Chaptera-W2-Fixture-Qualification/1.0",
                "Accept": "application/octet-stream,*/*",
            },
        )
        row: dict[str, Any] = {
            "candidate_id": candidate["id"],
            "label": candidate["label"],
            "requested_url": url,
            "expected_sha256": candidate["sha256"],
            "expected_size_bytes": candidate["size_bytes"],
            "longform_pub_qualified": False,
        }
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                data = response.read()
                observed_sha = sha256_bytes(data)
                row.update(
                    {
                        "final_url": response.geturl(),
                        "http_status": response.status,
                        "content_type": response.headers.get("Content-Type", ""),
                        "byte_len": len(data),
                        "sha256": observed_sha,
                        "magic_hex": data[:8].hex(),
                        "is_cfb": data.startswith(CFB_MAGIC),
                        "identity_exact": (
                            observed_sha == candidate["sha256"]
                            and len(data) == candidate["size_bytes"]
                        ),
                    }
                )
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            row.update(
                {
                    "final_url": None,
                    "http_status": None,
                    "content_type": None,
                    "byte_len": None,
                    "sha256": None,
                    "magic_hex": None,
                    "is_cfb": False,
                    "identity_exact": False,
                    "fetch_error": f"{type(error).__name__}: {error}",
                }
            )
            rows.append(row)
            continue

        if row["identity_exact"] and row["is_cfb"]:
            fixture = workdir / f"{candidate['id']}.pub"
            fixture.write_bytes(data)
            row.update(inspect_cfb(producer, fixture))
            row["longform_pub_qualified"] = (
                row["chaptera_open_status"] == "opened"
                and isinstance(row["page_count"], int)
                and row["page_count"] >= 10
            )
        else:
            row.update(
                {
                    "chaptera_open_status": "not_attempted",
                    "chaptera_error": None,
                    "page_count": None,
                    "scene_node_count": None,
                }
            )
        rows.append(row)
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--producer", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--workdir", required=True, type=pathlib.Path)
    parser.add_argument("--timeout", type=int, default=45)
    parser.add_argument("--include-pinned-corpus", action="store_true")
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

    pinned_rows = (
        qualify_pinned_corpus(args.producer, args.workdir, args.timeout)
        if args.include_pinned_corpus
        else []
    )
    result = {
        "schema": "chaptera.w2-longform-fixture-qualification.v1",
        "cfb_magic_required": CFB_MAGIC.hex(),
        "minimum_page_count": 10,
        "candidates": rows,
        "pinned_corpus_candidates": pinned_rows,
        "cfb_count": sum(bool(row["is_cfb"]) for row in rows),
        "qualified_count": sum(bool(row["longform_pub_qualified"]) for row in rows),
        "pinned_corpus_cfb_count": sum(bool(row["is_cfb"]) for row in pinned_rows),
        "pinned_corpus_qualified_count": sum(
            bool(row["longform_pub_qualified"]) for row in pinned_rows
        ),
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
