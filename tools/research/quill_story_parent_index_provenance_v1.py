#!/usr/bin/env python3
from __future__ import annotations

import concurrent.futures
import json
import re
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

COLLINFO = "https://index.commoncrawl.org/collinfo.json"
UA = "Chaptera-PUB-research/1.0 (+parent-index-provenance-replay)"

TARGETS = [
    {
        "source_sha256": "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
        "parent": "helenhudspith.com/resources/textiles/laura_dale/",
    },
    {
        "source_sha256": "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
        "parent": "helenhudspith.com/resources/product/roy_johnstone/",
    },
]

MAX_ROWS_PER_QUERY = 200


def request_json(url: str, timeout: int = 25):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return json.load(response)


def request_json_lines_with_retry(
    url: str,
    *,
    timeout: int = 12,
    attempts: int = 2,
) -> tuple[list[dict], list[str]]:
    errors: list[str] = []
    for attempt in range(attempts):
        try:
            req = urllib.request.Request(
                url,
                headers={"User-Agent": UA, "Accept": "*/*"},
            )
            with urllib.request.urlopen(req, timeout=timeout) as response:
                raw = response.read().decode("utf-8", errors="replace")
            rows: list[dict] = []
            for line in raw.splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    errors.append("JSONDecodeError")
                    continue
                if isinstance(row, dict):
                    rows.append(row)
            return rows, errors
        except urllib.error.HTTPError as exc:
            errors.append(f"HTTPError:{exc.code}")
            if exc.code not in (408, 425, 429, 500, 502, 503, 504):
                break
        except Exception as exc:
            errors.append(type(exc).__name__)

        if attempt + 1 < attempts:
            time.sleep(1)
    return [], errors


def collection_year(row: dict) -> int:
    label = str(row.get("id", ""))
    match = re.search(r"(19|20)\d{2}", label)
    return int(match.group(0)) if match else 9999


def is_pub(url: str) -> bool:
    path = urllib.parse.unquote(urllib.parse.urlsplit(url).path).casefold()
    return path.endswith(".pub") or ".pub?" in path


def likely_html(mime: str, url: str) -> bool:
    mime_fold = mime.casefold()
    if "html" in mime_fold:
        return True
    path = urllib.parse.unquote(urllib.parse.urlsplit(url).path).casefold()
    suffix = path.rsplit("/", 1)[-1]
    return (
        not suffix
        or suffix.endswith("/")
        or "." not in suffix
        or suffix.endswith((".html", ".htm", ".shtml", ".asp", ".aspx", ".php"))
    )


def query_variants(parent: str) -> list[str]:
    host, path = parent.split("/", 1)
    return [
        f"{host}/{path}",
        f"www.{host}/{path}",
    ]


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
        records: dict[tuple[str, str, str], dict] = {}
        query_receipts = []

        tasks = []
        for crawl in crawls:
            crawl_id = str(crawl.get("id", ""))
            endpoint = str(crawl["cdx-api"])
            for variant in query_variants(target["parent"]):
                params = {
                    "url": variant,
                    "matchType": "prefix",
                    "output": "json",
                    "filter": "status:200",
                    "collapse": "digest",
                    "limit": str(MAX_ROWS_PER_QUERY),
                }
                query = endpoint + "?" + urllib.parse.urlencode(params)
                tasks.append((crawl_id, variant, query))

        def run_query(task: tuple[str, str, str]) -> tuple[str, str, list[dict], list[str]]:
            crawl_id, variant, query = task
            rows, errors = request_json_lines_with_retry(query)
            return crawl_id, variant, rows, errors

        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            results = list(executor.map(run_query, tasks))

        for crawl_id, variant, rows_found, errors in results:
            query_receipts.append(
                {
                    "crawl": crawl_id,
                    "query_parent": variant,
                    "row_count": len(rows_found),
                    "errors": errors,
                }
            )
            for row in rows_found:
                url = str(row.get("url", ""))
                if not url or is_pub(url):
                    continue
                digest = str(row.get("digest", ""))
                timestamp = str(row.get("timestamp", ""))
                key = (crawl_id, timestamp, digest or url)
                records[key] = {
                    "crawl": crawl_id,
                    "timestamp": timestamp,
                    "url": url,
                    "status": str(row.get("status", "")),
                    "mime": str(row.get("mime", "")),
                    "digest": digest,
                    "length": str(row.get("length", "")),
                    "filename": str(row.get("filename", "")),
                    "offset": str(row.get("offset", "")),
                    "query_parent": variant,
                }

        rows = sorted(
            records.values(),
            key=lambda row: (row["timestamp"], row["url"], row["digest"]),
        )
        for row in rows:
            row["likely_html"] = likely_html(row["mime"], row["url"])

        target_reports.append(
            {
                "source_sha256": target["source_sha256"],
                "parent": target["parent"],
                "collections_queried": len(crawls),
                "query_count": len(query_receipts),
                "non_pub_record_count": len(rows),
                "likely_html_record_count": sum(row["likely_html"] for row in rows),
                "records": rows,
                "queries_with_rows": [
                    item for item in query_receipts if item["row_count"] > 0
                ],
                "query_error_classes": sorted(
                    {
                        error
                        for item in query_receipts
                        for error in item["errors"]
                    }
                ),
            }
        )

    report = {
        "schema": "chaptera.quill-story-parent-index-provenance.v1",
        "target_count": len(TARGETS),
        "targets": target_reports,
        "evidence_boundary": (
            "exact two historical parent directories only; Common Crawl index metadata "
            "through 2017 is queried with bounded retry/backoff and no WARC payload fetch; "
            "receipt retains only public index metadata and aggregate query diagnostics"
        ),
    }
    (out / "quill-story-parent-index-provenance.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
