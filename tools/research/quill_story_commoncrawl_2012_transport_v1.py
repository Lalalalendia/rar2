#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import sys
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "corpus"))
from harvest_pub import common_crawl_fetch  # type: ignore

COLLINFO = "https://index.commoncrawl.org/collinfo.json"
UA = "Chaptera-PUB-research/1.0 (+exact-2012-transport-tail-probe)"

TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "url": "http://helenhudspith.com/resources/textiles/laura_dale/textSpec_word.pub",
        "timestamp": "20120524040932",
        "digest": "3NGNAZMEG5ISDZ6YEM3TX74ELHSF3U7C",
        "capture_sha256": "56982d050e488df51fc32363ac0c907b611690e7b8815940d8fa9b849ad3e190",
        "capture_len": 43521,
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "url": "http://helenhudspith.com/resources/product/roy_johnstone/Pod%20design%20ideas.pub",
        "timestamp": "20120210122244",
        "digest": "GTPROUY2CX6JJQDHUD2JECUQI5VWZIBF",
        "capture_sha256": "61251db22838aff99a03fc1412455ed18229f55e5e1e2a5f15d262d9f3817a39",
        "capture_len": 44033,
    },
]


def request_json(url: str, timeout: int = 30):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return json.load(response)


def request_json_lines(url: str, timeout: int = 30) -> list[dict]:
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        raw = response.read().decode("utf-8", errors="replace")
    rows = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        row = json.loads(line)
        if isinstance(row, dict):
            rows.append(row)
    return rows


def canonical_index_url(url: str) -> str:
    split = urllib.parse.urlsplit(url)
    return split.netloc.removeprefix("www.") + split.path


def main() -> int:
    source_root = Path("_witness/lalamu/pub-corpus/corpus/native/unclassified")
    out = Path("out")
    out.mkdir(exist_ok=True)

    collections = request_json(COLLINFO)
    crawl = next(
        row for row in collections
        if isinstance(row, dict) and row.get("id") == "CC-MAIN-2012"
    )
    endpoint = str(crawl["cdx-api"])

    rows_out = []
    for target in TARGETS:
        source_path = source_root / f"{target['source_sha256']}.pub"
        source = source_path.read_bytes()
        source_sha = hashlib.sha256(source).hexdigest()
        if source_sha != target["source_sha256"]:
            raise RuntimeError(f"source SHA mismatch for {source_path}")

        params = {
            "url": canonical_index_url(target["url"]),
            "output": "json",
            "filter": "status:200",
            "collapse": "digest",
        }
        query = endpoint + "?" + urllib.parse.urlencode(params)
        index_rows = request_json_lines(query)
        matches = [
            row for row in index_rows
            if str(row.get("timestamp", "")) == target["timestamp"]
            and str(row.get("digest", "")) == target["digest"]
        ]
        if len(matches) != 1:
            raise RuntimeError(
                f"expected one pinned CC-MAIN-2012 row for {target['source_sha256']}, got {len(matches)}"
            )
        row = matches[0]
        seed = {
            "direct_url": str(row.get("url", target["url"])),
            "cc_warc_filename": str(row.get("filename", "")),
            "cc_warc_offset": str(row.get("offset", "")),
            "cc_warc_length": str(row.get("length", "")),
        }
        payload, _meta = common_crawl_fetch(
            seed,
            timeout=45,
            max_bytes=8 * 1024 * 1024,
            retries=2,
        )
        capture_sha = hashlib.sha256(payload).hexdigest()
        if capture_sha != target["capture_sha256"]:
            raise RuntimeError(
                f"capture SHA drift for {target['source_sha256']}: {capture_sha}"
            )
        if len(payload) != target["capture_len"]:
            raise RuntimeError(
                f"capture length drift for {target['source_sha256']}: {len(payload)}"
            )

        overlap = min(len(source), len(payload))
        overlap_diff_count = sum(
            left != right for left, right in zip(source[:overlap], payload[:overlap])
        )
        source_prefix_equal = len(payload) >= len(source) and payload[:len(source)] == source
        trailing = payload[len(source):] if source_prefix_equal else b""

        rows_out.append(
            {
                "source_sha256": target["source_sha256"],
                "capture_sha256": capture_sha,
                "crawl": "CC-MAIN-2012",
                "timestamp": target["timestamp"],
                "digest": target["digest"],
                "source_byte_len": len(source),
                "capture_byte_len": len(payload),
                "source_sector_aligned_512": len(source) % 512 == 0,
                "whole_file_overlap_diff_count": overlap_diff_count,
                "capture_is_source_plus_trailing_bytes": source_prefix_equal,
                "trailing_byte_count": len(trailing),
                "trailing_sha256": hashlib.sha256(trailing).hexdigest() if trailing else None,
                "trailing_all_zero": bool(trailing) and all(byte == 0 for byte in trailing),
                "trailing_all_ascii_whitespace": bool(trailing)
                and all(byte in (9, 10, 13, 32) for byte in trailing),
            }
        )

    report = {
        "schema": "chaptera.quill-story-commoncrawl-2012-transport.v1",
        "rows": rows_out,
        "pure_trailing_transport_count": sum(
            row["capture_is_source_plus_trailing_bytes"]
            and row["whole_file_overlap_diff_count"] == 0
            and row["trailing_byte_count"] > 0
            for row in rows_out
        ),
        "evidence_boundary": (
            "exact two pinned CC-MAIN-2012 captures only; each index row is pinned by "
            "timestamp+digest and each decoded payload by SHA-256+length; receipt retains "
            "only hashes, lengths and aggregate overlap/trailing classifications; no PUB "
            "bytes or document text retained"
        ),
    }
    (out / "quill-story-commoncrawl-2012-transport.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
