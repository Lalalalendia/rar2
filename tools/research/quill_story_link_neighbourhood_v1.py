#!/usr/bin/env python3
from __future__ import annotations

import html.parser
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "corpus"))
from harvest_pub import (  # type: ignore
    common_crawl_fetch,
    decode_common_crawl_arc_member,
    decode_common_crawl_warc_member,
)

COLLINFO = "https://index.commoncrawl.org/collinfo.json"
UA = "Chaptera-PUB-research/1.0 (+bounded-link-neighbourhood-probe)"

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

MAX_INDEX_ROWS_PER_CRAWL = 50
MAX_HTML_FETCHES_PER_TARGET = 80


class LinkParser(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.links: list[tuple[str, str]] = []
        self._href: str | None = None
        self._text: list[str] = []

    def handle_starttag(self, tag: str, attrs):
        if tag.casefold() != "a":
            return
        href = dict(attrs).get("href")
        if href:
            self._href = str(href)
            self._text = []

    def handle_data(self, data: str):
        if self._href is not None:
            self._text.append(data)

    def handle_endtag(self, tag: str):
        if tag.casefold() == "a" and self._href is not None:
            self.links.append((self._href, " ".join(self._text).strip()))
            self._href = None
            self._text = []


def request_json(url: str, timeout: int = 20):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return json.load(response)


def request_json_lines(url: str, timeout: int = 20) -> list[dict]:
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        raw = response.read().decode("utf-8", errors="replace")
    rows = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def collection_year(row: dict) -> int:
    label = str(row.get("id", ""))
    match = re.search(r"(19|20)\d{2}", label)
    return int(match.group(0)) if match else 9999


def parent_prefix_variants(url: str) -> list[str]:
    split = urllib.parse.urlsplit(url)
    host = split.netloc.removeprefix("www.")
    path = split.path.rsplit("/", 1)[0] + "/"
    return [
        urllib.parse.urlunsplit((scheme, candidate_host, path, "", ""))
        for scheme in ("http", "https")
        for candidate_host in (host, "www." + host)
    ]


def wanted_basename(url: str) -> str:
    return urllib.parse.unquote(urllib.parse.urlsplit(url).path.rsplit("/", 1)[-1]).casefold()


def pubish_href(url: str) -> bool:
    path = urllib.parse.unquote(urllib.parse.urlsplit(url).path).casefold()
    return path.endswith(".pub") or ".pub?" in path


def fallback_common_crawl_fetch(
    row: dict[str, str],
    timeout: int,
    max_bytes: int,
) -> tuple[bytes, dict, list[dict[str, str]]]:
    attempts: list[dict[str, str]] = []

    seed = {
        "direct_url": row["url"],
        "cc_warc_filename": row["filename"],
        "cc_warc_offset": row["offset"],
        "cc_warc_length": row["length"],
    }
    try:
        payload, meta = common_crawl_fetch(
            seed,
            timeout=timeout,
            max_bytes=max_bytes,
            retries=0,
        )
        attempts.append({"transport": "data.commoncrawl.org", "status": "success"})
        return payload, meta, attempts
    except Exception as exc:
        attempts.append(
            {
                "transport": "data.commoncrawl.org",
                "status": "error",
                "error_class": type(exc).__name__,
            }
        )

    filename = row["filename"].strip().lstrip("/")
    offset = int(row["offset"])
    length = int(row["length"])
    end = offset + length - 1
    legacy_arc = filename.casefold().endswith(".arc.gz")
    decoder = (
        decode_common_crawl_arc_member
        if legacy_arc
        else decode_common_crawl_warc_member
    )

    for transport, base in (
        ("ds5q9oxwqwsfj.cloudfront.net", "https://ds5q9oxwqwsfj.cloudfront.net/"),
    ):
        archive_url = base + filename
        try:
            req = urllib.request.Request(
                archive_url,
                headers={
                    "User-Agent": UA,
                    "Accept": "*/*",
                    "Accept-Encoding": "identity",
                    "Range": f"bytes={offset}-{end}",
                },
            )
            with urllib.request.urlopen(req, timeout=timeout) as response:
                status = getattr(response, "status", 0)
                if status != 206:
                    raise ValueError(f"range request returned HTTP {status}")
                compressed = response.read(length + 1)
            if len(compressed) != length:
                raise ValueError(
                    f"range length mismatch: expected {length}, got {len(compressed)}"
                )
            payload, meta = decoder(compressed, max_bytes)
            meta.update(
                {
                    "cc_fetch_source": "research_fallback_range",
                    "cc_archive_transport": transport,
                }
            )
            attempts.append({"transport": transport, "status": "success"})
            return payload, meta, attempts
        except Exception as exc:
            attempts.append(
                {
                    "transport": transport,
                    "status": "error",
                    "error_class": type(exc).__name__,
                }
            )

    raise RuntimeError(json.dumps(attempts, sort_keys=True))


def main() -> int:
    out = Path("out")
    out.mkdir(exist_ok=True)

    collections = request_json(COLLINFO, timeout=30)
    crawls = [
        row
        for row in collections
        if isinstance(row, dict)
        and row.get("cdx-api")
        and collection_year(row) <= 2017
    ]

    target_reports = []
    for target in TARGETS:
        prefixes = parent_prefix_variants(target["url"])
        wanted = wanted_basename(target["url"])
        index_rows = []
        errors = []
        seen_index = set()

        for crawl in crawls:
            endpoint = str(crawl["cdx-api"])
            for prefix in prefixes:
                params = {
                    "url": prefix,
                    "matchType": "prefix",
                    "output": "json",
                    "filter": "status:200",
                    "collapse": "digest",
                    "limit": str(MAX_INDEX_ROWS_PER_CRAWL),
                }
                query = endpoint + "?" + urllib.parse.urlencode(params)
                try:
                    rows = request_json_lines(query, timeout=15)
                except Exception as exc:
                    errors.append(type(exc).__name__)
                    continue
                for row in rows:
                    url = str(row.get("url", ""))
                    if pubish_href(url):
                        continue
                    key = (
                        str(row.get("digest", "")),
                        str(row.get("filename", "")),
                        str(row.get("offset", "")),
                    )
                    if key in seen_index:
                        continue
                    seen_index.add(key)
                    index_rows.append(
                        {
                            "crawl": str(crawl.get("id", "")),
                            "query_prefix": prefix,
                            "url": url,
                            "digest": str(row.get("digest", "")),
                            "filename": str(row.get("filename", "")),
                            "offset": str(row.get("offset", "")),
                            "length": str(row.get("length", "")),
                            "timestamp": str(row.get("timestamp", "")),
                        }
                    )

        # Prefer captures whose URL is closest to the target parent directory and
        # cap range fetches so this remains a bounded archaeology probe.
        index_rows.sort(key=lambda row: (row["timestamp"], row["url"]))
        if len(index_rows) > MAX_HTML_FETCHES_PER_TARGET:
            step = max(1, len(index_rows) // MAX_HTML_FETCHES_PER_TARGET)
            index_rows = index_rows[::step][:MAX_HTML_FETCHES_PER_TARGET]

        candidate_links = {}
        fetched_html = 0
        transport_attempts: list[dict[str, str]] = []
        transport_success_counts: dict[str, int] = {}
        for row in index_rows:
            try:
                payload, meta, row_attempts = fallback_common_crawl_fetch(
                    row,
                    timeout=12,
                    max_bytes=4 * 1024 * 1024,
                )
                transport_attempts.extend(row_attempts)
                successful = next(
                    (
                        attempt["transport"]
                        for attempt in reversed(row_attempts)
                        if attempt["status"] == "success"
                    ),
                    "unknown",
                )
                transport_success_counts[successful] = (
                    transport_success_counts.get(successful, 0) + 1
                )
            except Exception as exc:
                errors.append(type(exc).__name__)
                try:
                    decoded = json.loads(str(exc))
                    if isinstance(decoded, list):
                        transport_attempts.extend(
                            item for item in decoded if isinstance(item, dict)
                        )
                except json.JSONDecodeError:
                    pass
                continue

            content_type = str(meta.get("content_type", "")).casefold()
            if "html" not in content_type and b"<html" not in payload[:4096].lower():
                continue
            fetched_html += 1
            parser = LinkParser()
            try:
                parser.feed(payload.decode("utf-8", errors="replace"))
            except Exception:
                continue

            base = row["url"] or target["url"]
            for href, anchor in parser.links:
                absolute = urllib.parse.urljoin(base, href)
                if not pubish_href(absolute):
                    continue
                basename = urllib.parse.unquote(
                    urllib.parse.urlsplit(absolute).path.rsplit("/", 1)[-1]
                ).casefold()
                record = candidate_links.setdefault(
                    absolute,
                    {
                        "url": absolute,
                        "basename": basename,
                        "basename_matches_target": basename == wanted,
                        "seen_from_capture_count": 0,
                        "first_seen_timestamp": row["timestamp"],
                        "source_page_hosts": set(),
                        "anchor_samples": set(),
                    },
                )
                record["seen_from_capture_count"] += 1
                if row["timestamp"] and (
                    not record["first_seen_timestamp"]
                    or row["timestamp"] < record["first_seen_timestamp"]
                ):
                    record["first_seen_timestamp"] = row["timestamp"]
                host = urllib.parse.urlsplit(row["url"]).netloc
                if host:
                    record["source_page_hosts"].add(host)
                anchor_clean = " ".join(anchor.split())[:160]
                if anchor_clean:
                    record["anchor_samples"].add(anchor_clean)

        links = []
        for record in candidate_links.values():
            record["source_page_hosts"] = sorted(record["source_page_hosts"])
            record["anchor_samples"] = sorted(record["anchor_samples"])[:5]
            links.append(record)
        links.sort(
            key=lambda row: (
                not row["basename_matches_target"],
                row["basename"],
                row["url"],
            )
        )

        target_reports.append(
            {
                "source_sha256": target["source_sha256"],
                "parent_prefix_variants": prefixes,
                "collections_queried": len(crawls),
                "indexed_parent_html_candidates": len(index_rows),
                "indexed_parent_pages": [
                    {
                        "crawl": row["crawl"],
                        "timestamp": row["timestamp"],
                        "url": row["url"],
                        "digest": row["digest"],
                        "query_prefix": row["query_prefix"],
                    }
                    for row in index_rows
                ],
                "fetched_html_count": fetched_html,
                "transport_success_counts": transport_success_counts,
                "transport_attempt_summary": {
                    key: sum(
                        1
                        for attempt in transport_attempts
                        if attempt.get("transport") == key
                    )
                    for key in sorted(
                        {
                            attempt.get("transport", "unknown")
                            for attempt in transport_attempts
                        }
                    )
                },
                "transport_error_classes": sorted(
                    {
                        attempt.get("error_class", "")
                        for attempt in transport_attempts
                        if attempt.get("status") == "error"
                        and attempt.get("error_class")
                    }
                ),
                "pub_link_count": len(links),
                "exact_basename_link_count": sum(
                    row["basename_matches_target"] for row in links
                ),
                "error_classes": sorted(set(errors)),
                "pub_links": links,
            }
        )
        time.sleep(1)

    report = {
        "schema": "chaptera.quill-story-link-neighbourhood.v1",
        "target_count": len(TARGETS),
        "targets": target_reports,
        "evidence_boundary": (
            "exact two remaining target parent directories only, queried as explicit http/https and www/non-www prefix variants; Common Crawl historical "
            "HTML captures through 2017 are sampled with bounded fetch count; each record uses the canonical data.commoncrawl.org range path once, then the official Common Crawl CloudFront data endpoint once; receipt retains "
            "only public URLs, anchor samples, capture counts/timestamps and aggregate errors; "
            "no document text or PUB bytes are retained"
        ),
    }
    (out / "quill-story-link-neighbourhood.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
