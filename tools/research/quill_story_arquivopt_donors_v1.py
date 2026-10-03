#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import urllib.parse
import urllib.request
from pathlib import Path

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
CDX = "https://arquivo.pt/wayback/cdx"
UA = "Chaptera-PUB-research/1.0 (+exact-arquivopt-lineage-probe)"

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


def request_bytes(url: str, timeout: int = 30, limit: int = 8 * 1024 * 1024) -> bytes:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": UA,
            "Accept": "*/*",
            "Accept-Encoding": "identity",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"response exceeds {limit} bytes")
    return data


def url_variants(url: str) -> list[str]:
    split = urllib.parse.urlsplit(url)
    hosts = [split.netloc]
    if not split.netloc.startswith("www."):
        hosts.append("www." + split.netloc)
    out = []
    for scheme in ("http", "https"):
        for host in hosts:
            out.append(urllib.parse.urlunsplit((scheme, host, split.path, "", "")))
    return out


def parse_cdx_payload(raw: bytes) -> list[dict]:
    text = raw.decode("utf-8", errors="replace").strip()
    if not text:
        return []

    # Arquivo.pt commonly returns NDJSON for CDX output=json, but tolerate
    # an array-shaped response as well.
    if text.startswith("["):
        parsed = json.loads(text)
        if isinstance(parsed, list) and parsed and isinstance(parsed[0], list):
            header = parsed[0]
            return [
                dict(zip(header, row))
                for row in parsed[1:]
                if isinstance(row, list) and len(row) == len(header)
            ]
        if isinstance(parsed, list):
            return [row for row in parsed if isinstance(row, dict)]

    out = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        row = json.loads(line)
        if isinstance(row, dict):
            out.append(row)
    return out


def cdx_query(url: str) -> list[dict]:
    params = [
        ("url", url),
        ("output", "json"),
        ("filter", "status:200"),
        ("collapse", "digest"),
    ]
    endpoint = CDX + "?" + urllib.parse.urlencode(params)
    return parse_cdx_payload(request_bytes(endpoint, timeout=25, limit=4 * 1024 * 1024))


def replay_url(timestamp: str, original_url: str) -> str:
    quoted = urllib.parse.quote(
        urllib.parse.unquote(original_url),
        safe=":/%()?=&,+;@",
    )
    return f"https://arquivo.pt/noFrame/replay/{timestamp}/{quoted}"


def main() -> int:
    out = Path("out")
    variants_dir = out / "arquivopt-variants"
    variants_dir.mkdir(parents=True, exist_ok=True)

    report_targets = []
    for target in TARGETS:
        source_sha = target["source_sha256"]
        rows = []
        errors = []
        for candidate_url in url_variants(target["url"]):
            try:
                rows.extend(cdx_query(candidate_url))
            except Exception as exc:
                errors.append(type(exc).__name__)

        dedup = {}
        for row in rows:
            timestamp = str(row.get("timestamp", ""))
            digest = str(row.get("digest", ""))
            original = str(row.get("url") or row.get("original") or target["url"])
            if timestamp:
                dedup[(timestamp, digest, original)] = row

        captures = []
        seen_sha = set()
        for (timestamp, digest, original), row in sorted(dedup.items()):
            capture = {
                "timestamp": timestamp,
                "archive_digest": digest,
                "archive_url": original,
                "archive_length": str(row.get("length", "")),
                "archive_mime": str(row.get("mime", row.get("mimetype", ""))),
            }
            try:
                payload = request_bytes(replay_url(timestamp, original), timeout=35)
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
                    path = variants_dir / f"{source_sha}__{timestamp}__{sha}.pub"
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
                "cdx_capture_count": len(dedup),
                "cdx_error_classes": sorted(set(errors)),
                "downloaded_cfb_count": sum(
                    row.get("download_status") == "cfb" for row in captures
                ),
                "distinct_downloaded_sha256_count": len(seen_sha),
                "distinct_non_source_variant_count": sum(
                    sha != source_sha for sha in seen_sha
                ),
                "captures": captures,
            }
        )

    report = {
        "schema": "chaptera.quill-story-arquivopt-materialize.v1",
        "target_count": len(TARGETS),
        "targets": report_targets,
        "evidence_boundary": (
            "exact two remaining helenhudspith source URLs only; Arquivo.pt CDX exact URL "
            "queries use http/https plus www/non-www variants and collapse by digest; binary "
            "replay uses noFrame; materialized PUB bytes are temporary; receipt retains only "
            "capture timestamp/digest/url/mime/size/hash/status metadata"
        ),
    }
    (out / "quill-story-arquivopt-materialize.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
