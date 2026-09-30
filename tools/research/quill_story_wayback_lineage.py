#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "url": "http://helenhudspith.com/resources/textiles/laura_dale/textSpec_word.pub",
    },
    {
        "source_sha256": "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
        "url": "http://helenhudspith.com/resources/graphics/laura_dale/packag_advDis.pub",
    },
    {
        "source_sha256": "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
        "url": "http://helenhudspith.com/resources/graphics/fatima/0408/(1)%20One%20Point%20Perspective.pub",
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "url": "http://helenhudspith.com/resources/product/roy_johnstone/Pod%20design%20ideas.pub",
    },
]

UA = "Chaptera-PUB-research/1.0 (+exact-public-lineage-probe)"


def get_bytes(url: str, timeout: int = 45) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read()


def cdx_rows(url: str) -> list[dict[str, str]]:
    params = urllib.parse.urlencode(
        {
            "url": url,
            "output": "json",
            "fl": "timestamp,original,statuscode,mimetype,digest,length",
            "filter": "statuscode:200",
            "collapse": "digest",
        }
    )
    raw = get_bytes("https://web.archive.org/cdx/search/cdx?" + params, timeout=20)
    parsed = json.loads(raw)
    if not parsed:
        return []
    header = parsed[0]
    out = []
    for row in parsed[1:]:
        if len(row) != len(header):
            continue
        out.append(dict(zip(header, row)))
    return out


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: quill_story_wayback_lineage.py OUTPUT_DIR")
    out_dir = Path(sys.argv[1])
    variants_dir = out_dir / "wayback-variants"
    variants_dir.mkdir(parents=True, exist_ok=True)

    report_rows = []
    saved_sha = set()

    for target in TARGETS:
        item = {
            "source_sha256": target["source_sha256"],
            "cdx_status": "unattempted",
            "cdx_capture_count": 0,
            "downloaded_cfb_count": 0,
            "distinct_downloaded_sha256_count": 0,
            "distinct_non_source_variant_count": 0,
            "captures": [],
        }
        try:
            rows = cdx_rows(target["url"])
            item["cdx_status"] = "ok"
            item["cdx_capture_count"] = len(rows)
        except Exception as exc:
            item["cdx_status"] = "error"
            item["cdx_error_class"] = type(exc).__name__
            report_rows.append(item)
            continue

        archive_digests = set()
        for row in rows:
            digest = row.get("digest")
            if digest:
                archive_digests.add(digest)
            item["captures"].append(
                {
                    "timestamp": row.get("timestamp"),
                    "archive_digest": digest,
                    "archive_length": row.get("length"),
                    "mimetype": row.get("mimetype"),
                }
            )
        item["distinct_archive_digest_count"] = len(archive_digests)
        item["has_multiple_archive_digests"] = len(archive_digests) > 1
        report_rows.append(item)

    report = {
        "schema": "chaptera.quill-story-wayback-lineage.v1",
        "target_count": len(TARGETS),
        "targets": report_rows,
        "targets_with_multiple_archive_digests": sum(
            bool(row.get("has_multiple_archive_digests")) for row in report_rows
        ),
        "evidence_boundary": (
            "exact four public helenhudspith.com source URLs only; Wayback CDX is queried "
            "with digest collapse; this discovery pass records only archive metadata and "
            "does not materialize archived document bytes; report contains timestamps, digests, "
            "reported sizes and status only, never document text"
        ),
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "quill-story-wayback-lineage.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
