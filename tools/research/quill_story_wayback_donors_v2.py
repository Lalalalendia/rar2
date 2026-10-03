#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import time
import urllib.parse
import urllib.request
from pathlib import Path

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
UA = "Chaptera-PUB-research/1.0 (+exact-public-lineage-probe-v2)"

TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "url": "http://helenhudspith.com/resources/textiles/laura_dale/textSpec_word.pub",
        "known_timestamps": ["20090729050425"],
    },
    {
        "source_sha256": "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
        "url": "http://helenhudspith.com/resources/graphics/fatima/0408/(1)%20One%20Point%20Perspective.pub",
        "known_timestamps": [],
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "url": "http://helenhudspith.com/resources/product/roy_johnstone/Pod%20design%20ideas.pub",
        "known_timestamps": ["20170214165303"],
    },
]


def request_bytes(url: str, timeout: int = 45, attempts: int = 4) -> bytes:
    last = None
    for attempt in range(attempts):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read()
        except Exception as exc:
            last = exc
            if attempt + 1 < attempts:
                time.sleep(2 ** attempt)
    assert last is not None
    raise last


def cdx_query(url: str, *, prefix: bool = False) -> list[dict[str, str]]:
    params = {
        "url": url,
        "output": "json",
        "fl": "timestamp,original,statuscode,mimetype,digest,length",
        "filter": "statuscode:200",
        "collapse": "digest",
    }
    if prefix:
        params["matchType"] = "prefix"
    endpoint = "https://web.archive.org/cdx/search/cdx?" + urllib.parse.urlencode(params)
    raw = request_bytes(endpoint, timeout=30)
    parsed = json.loads(raw)
    if not parsed:
        return []
    header = parsed[0]
    out = []
    for row in parsed[1:]:
        if len(row) == len(header):
            out.append(dict(zip(header, row)))
    return out


def normalized_basename(url: str) -> str:
    return urllib.parse.unquote(urllib.parse.urlsplit(url).path.rsplit("/", 1)[-1]).lower()


def discover(target: dict[str, object]) -> tuple[list[dict[str, str]], list[str]]:
    url = str(target["url"])
    errors: list[str] = []
    rows: list[dict[str, str]] = []

    exact_variants = {
        url,
        url.replace("http://", "https://", 1),
        urllib.parse.unquote(url),
        urllib.parse.unquote(url).replace("http://", "https://", 1),
    }
    for candidate in sorted(exact_variants):
        try:
            rows.extend(cdx_query(candidate))
        except Exception as exc:
            errors.append(type(exc).__name__)

    if not rows:
        split = urllib.parse.urlsplit(url)
        directory = split.path.rsplit("/", 1)[0] + "/"
        for scheme in ("http", "https"):
            prefix = urllib.parse.urlunsplit((scheme, split.netloc, directory, "", ""))
            try:
                prefix_rows = cdx_query(prefix, prefix=True)
            except Exception as exc:
                errors.append(type(exc).__name__)
                continue
            wanted = normalized_basename(url)
            rows.extend(
                row for row in prefix_rows
                if normalized_basename(row.get("original", "")) == wanted
            )

    dedup: dict[tuple[str, str], dict[str, str]] = {}
    for row in rows:
        timestamp = row.get("timestamp", "")
        digest = row.get("digest", "")
        if timestamp:
            dedup[(timestamp, digest)] = row
    return sorted(dedup.values(), key=lambda row: row.get("timestamp", "")), errors


def replay_url(timestamp: str, original_url: str) -> str:
    quoted = urllib.parse.quote(urllib.parse.unquote(original_url), safe=":/%()?=&")
    return f"https://web.archive.org/web/{timestamp}id_/{quoted}"


def main() -> int:
    out_dir = Path("out")
    variants_dir = out_dir / "wayback-variants"
    variants_dir.mkdir(parents=True, exist_ok=True)

    report_targets = []
    for target in TARGETS:
        source_sha = str(target["source_sha256"])
        rows, errors = discover(target)
        candidates: dict[str, dict[str, str | None]] = {}

        for row in rows:
            timestamp = row.get("timestamp")
            if not timestamp:
                continue
            candidates[timestamp] = {
                "timestamp": timestamp,
                "archive_digest": row.get("digest"),
                "archive_length": row.get("length"),
                "mimetype": row.get("mimetype"),
                "discovery": "cdx",
            }

        for timestamp in target["known_timestamps"]:
            candidates.setdefault(
                timestamp,
                {
                    "timestamp": timestamp,
                    "archive_digest": None,
                    "archive_length": None,
                    "mimetype": None,
                    "discovery": "prior_exact_cdx_authority",
                },
            )

        captures = []
        seen_sha: set[str] = set()
        for timestamp, meta in sorted(candidates.items()):
            capture = dict(meta)
            try:
                payload = request_bytes(
                    replay_url(timestamp, str(target["url"])),
                    timeout=90,
                )
                if not payload.startswith(CFB_MAGIC):
                    capture["download_status"] = "not_cfb"
                    captures.append(capture)
                    continue
                actual = hashlib.sha256(payload).hexdigest()
                capture["download_status"] = "cfb"
                capture["sha256"] = actual
                capture["size_bytes"] = len(payload)
                capture["source_equal"] = actual == source_sha
                if actual not in seen_sha:
                    seen_sha.add(actual)
                    path = variants_dir / f"{source_sha}__{timestamp}__{actual}.pub"
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
                "cdx_status": "ok" if rows else ("error" if errors else "empty"),
                "cdx_error_classes": sorted(set(errors)),
                "cdx_capture_count": len(rows),
                "candidate_timestamp_count": len(candidates),
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
        "schema": "chaptera.quill-story-wayback-materialize.v2",
        "target_count": len(TARGETS),
        "targets": report_targets,
        "evidence_boundary": (
            "exact three unresolved helenhudspith.com source URLs only; Wayback discovery "
            "uses exact/prefix CDX plus two prior exact capture timestamps; downloaded bytes "
            "are temporary and only CFB SHA/size/timestamp metadata is retained in this receipt"
        ),
    }
    (out_dir / "quill-story-wayback-materialize.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
