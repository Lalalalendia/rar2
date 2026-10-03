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

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
COLLINFO = "https://index.commoncrawl.org/collinfo.json"
UA = "Chaptera-PUB-research/1.0 (+exact-commoncrawl-lineage-probe)"

TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "url": "http://helenhudspith.com/resources/textiles/laura_dale/textSpec_word.pub",
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "url": "http://helenhudspith.com/resources/product/roy_johnstone/Pod%20design%20ideas.pub",
    },
]


def request_json(url: str, timeout: int = 25):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return json.load(response)


def request_json_lines(url: str, timeout: int = 25) -> list[dict]:
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        raw = response.read().decode("utf-8", errors="replace")
    out = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            out.append(row)
    return out


def crawl_year(crawl_id: str) -> int:
    try:
        return int(crawl_id.split("-")[2])
    except (IndexError, ValueError):
        return 9999


def url_variants(url: str) -> list[str]:
    split = urllib.parse.urlsplit(url)
    hosts = [split.netloc]
    if not split.netloc.startswith("www."):
        hosts.append("www." + split.netloc)
    variants = []
    for scheme in ("http", "https"):
        for host in hosts:
            variants.append(
                urllib.parse.urlunsplit((scheme, host, split.path, "", ""))
            )
    return variants


def main() -> int:
    out = Path("out")
    variants = out / "commoncrawl-variants"
    variants.mkdir(parents=True, exist_ok=True)

    collections = request_json(COLLINFO, timeout=30)
    crawls = [
        row
        for row in collections
        if isinstance(row, dict)
        and str(row.get("id", "")).startswith("CC-MAIN-")
        and crawl_year(str(row.get("id", ""))) <= 2017
        and row.get("cdx-api")
    ]

    report_targets = []
    for target in TARGETS:
        source_sha = target["source_sha256"]
        discovered = []
        errors = []
        seen_capture = set()

        query_specs = []
        for crawl in crawls:
            crawl_id = str(crawl["id"])
            endpoint = str(crawl["cdx-api"])
            for candidate_url in url_variants(target["url"]):
                params = {
                    "url": candidate_url,
                    "output": "json",
                    "filter": "status:200",
                    "collapse": "digest",
                }
                query_specs.append(
                    (crawl_id, candidate_url, endpoint + "?" + urllib.parse.urlencode(params))
                )

        def run_query(spec):
            crawl_id, candidate_url, query = spec
            try:
                return crawl_id, candidate_url, request_json_lines(query, timeout=14), None
            except Exception as exc:
                return crawl_id, candidate_url, [], type(exc).__name__

        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            for crawl_id, candidate_url, rows, error_class in pool.map(
                run_query, query_specs
            ):
                if error_class:
                    errors.append({"crawl": crawl_id, "error_class": error_class})
                    continue
                for row in rows:
                    key = (
                        str(row.get("digest", "")),
                        str(row.get("filename", "")),
                        str(row.get("offset", "")),
                    )
                    if key in seen_capture:
                        continue
                    seen_capture.add(key)
                    discovered.append(
                        {
                            "crawl": crawl_id,
                            "url": str(row.get("url", candidate_url)),
                            "timestamp": str(row.get("timestamp", "")),
                            "digest": str(row.get("digest", "")),
                            "filename": str(row.get("filename", "")),
                            "offset": str(row.get("offset", "")),
                            "length": str(row.get("length", "")),
                        }
                    )

        captures = []
        seen_sha = set()
        for row in discovered:
            seed = {
                "direct_url": row["url"],
                "cc_warc_filename": row["filename"],
                "cc_warc_offset": row["offset"],
                "cc_warc_length": row["length"],
            }
            capture = {
                "crawl": row["crawl"],
                "timestamp": row["timestamp"],
                "digest": row["digest"],
            }
            try:
                payload, _meta = common_crawl_fetch(
                    seed,
                    timeout=35,
                    max_bytes=8 * 1024 * 1024,
                    retries=1,
                )
                if not payload.startswith(CFB_MAGIC):
                    capture["download_status"] = "not_cfb"
                    captures.append(capture)
                    continue
                sha = hashlib.sha256(payload).hexdigest()
                capture["download_status"] = "cfb"
                capture["sha256"] = sha
                capture["size_bytes"] = len(payload)
                capture["source_equal"] = sha == source_sha
                if sha not in seen_sha:
                    seen_sha.add(sha)
                    path = variants / f"{source_sha}__{row['crawl']}__{row['timestamp']}__{sha}.pub"
                    path.write_bytes(payload)
                    capture["materialized"] = True
                else:
                    capture["materialized"] = False
            except Exception as exc:
                capture["download_status"] = "error"
                capture["error_class"] = type(exc).__name__
            captures.append(capture)

        report_targets.append(
            {
                "source_sha256": source_sha,
                "collections_queried": len(crawls),\n                "index_query_count": len(query_specs),
                "index_capture_count": len(discovered),
                "downloaded_cfb_count": sum(
                    row.get("download_status") == "cfb" for row in captures
                ),
                "distinct_downloaded_sha256_count": len(seen_sha),
                "distinct_non_source_variant_count": sum(
                    sha != source_sha for sha in seen_sha
                ),
                "error_classes": sorted({row["error_class"] for row in errors}),
                "captures": captures,
            }
        )

    report = {
        "schema": "chaptera.quill-story-commoncrawl-materialize.v1",
        "target_count": len(TARGETS),
        "targets": report_targets,
        "evidence_boundary": (
            "exact two remaining helenhudspith source URLs only; queries all published "
            "Common Crawl collections through 2017 across http/https and www/non-www exact "
            "URL variants; only bounded WARC/ARC range payloads are materialized temporarily; "
            "receipt retains only crawl/timestamp/digest/hash/size/status metadata"
        ),
    }
    (out / "quill-story-commoncrawl-materialize.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
