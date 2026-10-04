#!/usr/bin/env python3
from __future__ import annotations

import html.parser
import json
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

CDX = "https://web.archive.org/cdx/search/cdx"
UA = "Chaptera-PUB-research/1.0 (+wayback-parent-link-probe)"

TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "parent": "http://helenhudspith.com/resources/textiles/laura_dale/",
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "parent": "http://helenhudspith.com/resources/product/roy_johnstone/",
    },
]

MAX_CAPTURES_PER_TARGET = 60


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


def request_bytes(url: str, timeout: int = 25, attempts: int = 3) -> bytes:
    last_error: Exception | None = None
    for attempt in range(attempts):
        try:
            req = urllib.request.Request(
                url,
                headers={"User-Agent": UA, "Accept": "*/*"},
            )
            with urllib.request.urlopen(req, timeout=timeout) as response:
                return response.read()
        except Exception as exc:
            last_error = exc
            if attempt + 1 < attempts:
                time.sleep((1, 3, 7)[attempt])
    assert last_error is not None
    raise last_error


def cdx_rows(parent: str) -> tuple[list[dict[str, str]], list[str]]:
    split = urllib.parse.urlsplit(parent)
    host = split.netloc.removeprefix("www.")
    path = split.path
    variants = [
        urllib.parse.urlunsplit((scheme, candidate_host, path, "", ""))
        for scheme in ("http", "https")
        for candidate_host in (host, "www." + host)
    ]

    rows: list[dict[str, str]] = []
    errors: list[str] = []
    for variant in variants:
        params = [
            ("url", variant + "*"),
            ("matchType", "prefix"),
            ("output", "json"),
            ("fl", "timestamp,original,statuscode,mimetype,digest,length"),
            ("filter", "statuscode:200"),
            ("collapse", "digest"),
            ("limit", "1000"),
        ]
        endpoint = CDX + "?" + urllib.parse.urlencode(params)
        try:
            raw = request_bytes(endpoint, timeout=20, attempts=2)
            parsed = json.loads(raw)
        except Exception as exc:
            errors.append(type(exc).__name__)
            continue
        if not parsed:
            continue
        header = parsed[0]
        for item in parsed[1:]:
            if len(item) != len(header):
                continue
            row = dict(zip(header, item))
            mime = row.get("mimetype", "").casefold()
            original = row.get("original", "")
            if "html" not in mime:
                continue
            if urllib.parse.unquote(original).casefold().endswith(".pub"):
                continue
            rows.append(row)

    dedup = {}
    for row in rows:
        key = (
            row.get("timestamp", ""),
            row.get("original", ""),
            row.get("digest", ""),
        )
        dedup[key] = row
    return sorted(
        dedup.values(),
        key=lambda row: (row.get("timestamp", ""), row.get("original", "")),
    ), errors


def raw_snapshot_url(timestamp: str, original: str) -> str:
    quoted = urllib.parse.quote(urllib.parse.unquote(original), safe=":/%()?=&")
    return f"https://web.archive.org/web/{timestamp}id_/{quoted}"


def is_pub_url(url: str) -> bool:
    return urllib.parse.unquote(urllib.parse.urlsplit(url).path).casefold().endswith(".pub")


def main() -> int:
    out = Path("out")
    out.mkdir(exist_ok=True)

    target_reports = []
    for target in TARGETS:
        rows, errors = cdx_rows(target["parent"])
        if len(rows) > MAX_CAPTURES_PER_TARGET:
            step = max(1, len(rows) // MAX_CAPTURES_PER_TARGET)
            rows = rows[::step][:MAX_CAPTURES_PER_TARGET]

        captures = []
        links = {}
        for row in rows:
            timestamp = row.get("timestamp", "")
            original = row.get("original", "")
            capture = {
                "timestamp": timestamp,
                "url": original,
                "digest": row.get("digest", ""),
                "mimetype": row.get("mimetype", ""),
                "length": row.get("length", ""),
            }
            try:
                payload = request_bytes(
                    raw_snapshot_url(timestamp, original),
                    timeout=25,
                    attempts=2,
                )
                capture["fetch_status"] = "success"
            except Exception as exc:
                capture["fetch_status"] = "error"
                capture["error_class"] = type(exc).__name__
                captures.append(capture)
                continue

            parser = LinkParser()
            try:
                parser.feed(payload.decode("utf-8", errors="replace"))
            except Exception as exc:
                capture["parse_status"] = "error"
                capture["parse_error_class"] = type(exc).__name__
                captures.append(capture)
                continue

            capture["parse_status"] = "success"
            capture["anchor_count"] = len(parser.links)
            for href, anchor in parser.links:
                absolute = urllib.parse.urljoin(original, href)
                if not is_pub_url(absolute):
                    continue
                item = links.setdefault(
                    absolute,
                    {
                        "url": absolute,
                        "first_seen_timestamp": timestamp,
                        "seen_from_capture_count": 0,
                        "source_pages": set(),
                        "anchor_samples": set(),
                    },
                )
                item["seen_from_capture_count"] += 1
                if timestamp and timestamp < item["first_seen_timestamp"]:
                    item["first_seen_timestamp"] = timestamp
                item["source_pages"].add(original)
                cleaned = " ".join(anchor.split())[:160]
                if cleaned:
                    item["anchor_samples"].add(cleaned)
            captures.append(capture)

        links_out = []
        for item in links.values():
            item["source_pages"] = sorted(item["source_pages"])
            item["anchor_samples"] = sorted(item["anchor_samples"])[:5]
            links_out.append(item)
        links_out.sort(key=lambda row: row["url"])

        target_reports.append(
            {
                "source_sha256": target["source_sha256"],
                "parent": target["parent"],
                "html_capture_count": len(rows),
                "fetched_html_count": sum(
                    row.get("fetch_status") == "success" for row in captures
                ),
                "pub_link_count": len(links_out),
                "pub_links": links_out,
                "captures": captures,
                "cdx_error_classes": sorted(set(errors)),
            }
        )

    report = {
        "schema": "chaptera.quill-story-wayback-parent-links.v1",
        "target_count": len(TARGETS),
        "targets": target_reports,
        "evidence_boundary": (
            "exact two historical parent directories only; Wayback CDX is restricted to "
            "status-200 HTML captures and a bounded sample; raw HTML is transient; receipt "
            "retains only capture metadata and public .pub href/anchor evidence; no document "
            "text or PUB bytes retained"
        ),
    }
    (out / "quill-story-wayback-parent-links.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
