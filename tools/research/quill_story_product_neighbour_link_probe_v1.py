#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "corpus"))
from harvest_pub import common_crawl_fetch  # type: ignore

CC_MAIN_2012_INDEX = "https://index.commoncrawl.org/CC-MAIN-2012-index"
UA = "Chaptera-PUB-research/1.0 (+product-neighbour-link-probe)"
CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")

RECORDS = [
    {
        "url": "http://www.helenhudspith.com/resources/product/roy_johnstone/Lightspider.doc",
        "timestamp": "20120523172137",
        "digest": "33S6ARAPPUII6GF2Z5OT4UJMA632FYWD",
    },
    {
        "url": "http://www.helenhudspith.com/resources/product/roy_johnstone/Production%20Flowcharts%201.ppt",
        "timestamp": "20120523172252",
        "digest": "IVUKTMUHSEWIFS7YAUWNO63XZGIHIDE5",
    },
]


def request_json_lines(url: str, timeout: int = 20, attempts: int = 3) -> list[dict]:
    last_error: Exception | None = None
    for attempt in range(attempts):
        try:
            req = urllib.request.Request(
                url,
                headers={"User-Agent": UA, "Accept": "*/*"},
            )
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
        except Exception as exc:
            last_error = exc
            if attempt + 1 < attempts:
                import time
                time.sleep((1, 3, 7)[attempt])
    assert last_error is not None
    raise last_error


def ascii_strings(payload: bytes, min_len: int = 4) -> list[str]:
    pattern = rb"[\x20-\x7e]{%d,}" % min_len
    return [m.decode("ascii", errors="ignore") for m in re.findall(pattern, payload)]


def utf16le_ascii_strings(payload: bytes, min_len: int = 4) -> list[str]:
    pattern = rb"(?:[\x20-\x7e]\x00){%d,}" % min_len
    out = []
    for match in re.findall(pattern, payload):
        try:
            out.append(match.decode("utf-16le"))
        except UnicodeDecodeError:
            continue
    return out


def linkish(value: str) -> bool:
    folded = value.casefold()
    return (
        ".pub" in folded
        or "http://" in folded
        or "https://" in folded
        or "helenhudspith.com" in folded
    )


def normalize(value: str) -> str:
    return " ".join(value.replace("\x00", "").split())[:500]


def main() -> int:
    out = Path("out")
    out.mkdir(exist_ok=True)

    endpoint = CC_MAIN_2012_INDEX

    rows_out = []
    for record in RECORDS:
        split = urllib.parse.urlsplit(record["url"])
        canonical = split.netloc.removeprefix("www.") + split.path
        params = {
            "url": canonical,
            "output": "json",
            "filter": "status:200",
            "collapse": "digest",
        }
        query = endpoint + "?" + urllib.parse.urlencode(params)
        index_rows = request_json_lines(query)
        matches = [
            row
            for row in index_rows
            if str(row.get("timestamp", "")) == record["timestamp"]
            and str(row.get("digest", "")) == record["digest"]
        ]
        if len(matches) != 1:
            raise RuntimeError(
                f"expected one pinned row for {record['url']}, got {len(matches)}"
            )
        row = matches[0]
        seed = {
            "direct_url": str(row.get("url", record["url"])),
            "cc_warc_filename": str(row.get("filename", "")),
            "cc_warc_offset": str(row.get("offset", "")),
            "cc_warc_length": str(row.get("length", "")),
        }
        payload, _meta = common_crawl_fetch(
            seed,
            timeout=35,
            max_bytes=16 * 1024 * 1024,
            retries=1,
        )
        if not payload.startswith(CFB_MAGIC):
            raise RuntimeError(f"expected OLE/CFB payload for {record['url']}")

        candidates = set()
        for value in ascii_strings(payload):
            if linkish(value):
                candidates.add(normalize(value))
        for value in utf16le_ascii_strings(payload):
            if linkish(value):
                candidates.add(normalize(value))

        pub_candidates = sorted(
            value for value in candidates if ".pub" in value.casefold()
        )
        url_candidates = sorted(
            value
            for value in candidates
            if "http://" in value.casefold() or "https://" in value.casefold()
        )

        rows_out.append(
            {
                "source_url": record["url"],
                "crawl": "CC-MAIN-2012",
                "timestamp": record["timestamp"],
                "digest": record["digest"],
                "payload_byte_len": len(payload),
                "pub_linkish_string_count": len(pub_candidates),
                "pub_linkish_strings": pub_candidates,
                "url_linkish_string_count": len(url_candidates),
                "url_linkish_strings": url_candidates,
            }
        )

    report = {
        "schema": "chaptera.quill-story-product-neighbour-link-probe.v1",
        "record_count": len(rows_out),
        "records": rows_out,
        "evidence_boundary": (
            "exact two pinned CC-MAIN-2012 neighbouring Office resources only; "
            "payloads are fetched transiently and scanned only for ASCII/UTF-16LE strings "
            "containing .pub, http(s), or helenhudspith.com; ordinary document text and "
            "raw payload bytes are not retained"
        ),
    }
    (out / "quill-story-product-neighbour-link-probe.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
