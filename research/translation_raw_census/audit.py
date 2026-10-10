#!/usr/bin/env python3
"""Bounded, resumable per-work integrity census of public chapter HTML.

The tool emits ONLY metadata. The body text is never saved, logged, or uploaded.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
import time
import unicodedata
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable
from urllib.parse import urljoin, urlparse
from urllib.robotparser import RobotFileParser

import requests
from bs4 import BeautifulSoup

AGENT = "NovelRawIntegrityAudit/1.0 (metadata-only; GitHub Actions)"
BOOKS = {
    "fencing": ("narou", "n8001hc", 166),
    "return": ("narou", "n7481gn", 201),
    "red-reaper": ("narou", "n4734fn", 316),
    "witcher": ("royalroad", "65841", 260),
}
ROYAL_URL = "https://www.royalroad.com/fiction/65841/a-scholars-travels-with-a-witcher"
ALLOWED_HOSTS = {"ncode.syosetu.com", "www.royalroad.com"}
CHAPTER_SELECTORS = {
    "narou": ["div.js-novel-text.p-novel__text:not(.p-novel__text--preface):not(.p-novel__text--afterword)", "div.p-novel__text:not(.p-novel__text--preface):not(.p-novel__text--afterword)", "#novel_honbun", "#novel-honbun", "div.novel_view"],
    "royalroad": [".chapter-content", ".chapter-inner", "#chapter-container .chapter-content"],
}


@dataclass(frozen=True)
class Entry:
    number: int
    title: str
    url: str


def utcnow() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def canonical_text(node) -> str:
    cloned = BeautifulSoup(str(node), "html.parser")
    for tag in cloned.select("script, style, iframe, nav, button, noscript, .chapter-nav, .advertisement, .adsbox"):
        tag.decompose()
    text = cloned.get_text(" ", strip=True)
    return unicodedata.normalize("NFC", re.sub(r"\s+", " ", text).strip())


def extract_body(html: str, provider: str) -> str:
    soup = BeautifulSoup(html, "html.parser")
    for selector in CHAPTER_SELECTORS[provider]:
        nodes = soup.select(selector)
        if nodes:
            texts = [canonical_text(node) for node in nodes]
            # Avoid prefatory notes taking precedence over the actual body.
            text = max(texts, key=len, default="")
            if text:
                return text
    return ""


def narou_listing(html: str, code: str) -> list[Entry]:
    soup = BeautifulSoup(html, "html.parser")
    items: list[Entry] = []
    pattern = re.compile(rf"^/({re.escape(code)})/(\d+)/?$", re.I)
    seen: set[int] = set()
    for a in soup.find_all("a", href=True):
        u = urljoin(f"https://ncode.syosetu.com/{code}/", a["href"])
        parsed = urlparse(u)
        match = pattern.fullmatch(parsed.path)
        if parsed.hostname != "ncode.syosetu.com" or not match:
            continue
        number = int(match.group(2))
        if number not in seen:
            seen.add(number)
            items.append(Entry(number, a.get_text(" ", strip=True), f"https://ncode.syosetu.com/{code}/{number}/"))
    return items


def royal_listing(html: str, fiction_id: str) -> list[Entry]:
    soup = BeautifulSoup(html, "html.parser")
    pattern = re.compile(rf"^/fiction/{re.escape(fiction_id)}/[^/]+/chapter/(\d+)/[^/?#]+/?$", re.I)
    anchors = soup.select("table a[href]")
    if not anchors:
        anchors = soup.select("a[href]")
    seen: set[str] = set()
    out = []
    for a in anchors:
        absolute = urljoin(ROYAL_URL, a["href"])
        parsed = urlparse(absolute)
        if parsed.hostname != "www.royalroad.com" or not pattern.fullmatch(parsed.path):
            continue
        address = f"https://www.royalroad.com{parsed.path}"
        if address in seen:
            continue
        seen.add(address)
        out.append(Entry(len(out) + 1, a.get_text(" ", strip=True), address))
    return out


class Fetcher:
    def __init__(self, delay: float = 2.0, timeout: float = 25.0, session=None, sleeper: Callable = time.sleep):
        self.session = session or requests.Session()
        self.session.headers.update({"User-Agent": AGENT, "Accept-Language": "ja,en;q=0.8"})
        self.delay = max(1.0, delay)
        self.timeout = timeout
        self.sleeper = sleeper
        self.last_request = 0.0
        self.robots: dict[str, RobotFileParser] = {}

    def allowed(self, url: str) -> bool:
        parts = urlparse(url)
        if parts.scheme != "https" or parts.hostname not in ALLOWED_HOSTS:
            raise RuntimeError(f"host_not_allowed: {parts.hostname}")
        if parts.hostname not in self.robots:
            robots_url = f"https://{parts.hostname}/robots.txt"
            resp = self.session.get(robots_url, timeout=self.timeout, allow_redirects=False)
            parser = RobotFileParser()
            if resp.status_code == 404:
                parser.parse([])
            elif resp.status_code == 200:
                parser.parse(resp.text.splitlines())
            else:
                raise RuntimeError(f"robots_unavailable: {resp.status_code}")
            self.robots[parts.hostname] = parser
        return self.robots[parts.hostname].can_fetch(AGENT, url)

    def get(self, url: str) -> requests.Response:
        if not self.allowed(url):
            raise RuntimeError(f"robots_disallow: {url}")
        for attempt in range(3):
            since = time.monotonic() - self.last_request
            if self.last_request and since < self.delay:
                self.sleeper(self.delay - since)
            self.last_request = time.monotonic()
            r = self.session.get(url, timeout=self.timeout, allow_redirects=True)
            redirected = urlparse(r.url)
            if redirected.hostname not in ALLOWED_HOSTS or redirected.scheme != "https":
                raise RuntimeError(f"redirect_outside_allowlist: {redirected.hostname}")
            if r.status_code in (429, 500, 502, 503, 504) and attempt < 2:
                after = r.headers.get("Retry-After", "")
                self.sleeper(min(60.0, max(2 ** (attempt + 1), float(after) if after.isdecimal() else 0)))
                continue
            if r.status_code == 403:
                raise RuntimeError("access_blocked_http_403")
            if r.status_code == 429:
                raise RuntimeError("access_blocked_http_429")
            r.raise_for_status()
            return r
        raise RuntimeError("retry_exhausted")


def index_book(fetch: Fetcher, name: str) -> tuple[list[Entry], list[str]]:
    provider, code, expected = BOOKS[name]
    if provider == "royalroad":
        return royal_listing(fetch.get(ROYAL_URL).text, code), []
    all_items: dict[int, Entry] = {}
    errors = []
    # Narou episode tables are paged in 100-entry windows.
    for page in range(1, math.ceil(expected / 100) + 3):
        url = f"https://ncode.syosetu.com/{code}/" + (f"?p={page}" if page > 1 else "")
        try:
            got = narou_listing(fetch.get(url).text, code)
        except Exception as e:
            errors.append(f"index_page_{page}: {type(e).__name__}: {e}")
            break
        new = 0
        for item in got:
            if item.number not in all_items:
                all_items[item.number] = item
                new += 1
        if len(all_items) >= expected or new == 0:
            break
    return sorted(all_items.values(), key=lambda item: item.number), errors


def run_book(name: str, output: Path, fetch: Fetcher, limit: int = 0) -> dict:
    provider, code, expected = BOOKS[name]
    output.mkdir(parents=True, exist_ok=True)
    try:
        entries, index_errors = index_book(fetch, name)
    except Exception as e:
        entries, index_errors = [], [f"index_error: {type(e).__name__}: {e}"]
    index_numbers = [e.number for e in entries]
    missing_index = sorted(set(range(1, expected + 1)) - set(index_numbers)) if provider == "narou" else []
    results = []
    scans = entries[:limit] if limit > 0 else entries
    hard_block = None
    for pos, e in enumerate(scans, 1):
        result = {"ordinal": e.number, "title": e.title, "url": e.url, "status": "error", "chars": 0,
                  "sha256": None, "http_status": None, "error": None}
        try:
            response = fetch.get(e.url)
            result["http_status"] = response.status_code
            body = extract_body(response.text, provider)
            result["chars"] = len(body)
            if not body:
                result["error"] = "no_chapter_body_selector"
            elif len(body) < 120:
                result["error"] = "body_too_short_for_automatic_verification"
            else:
                result["status"] = "ok"
                result["sha256"] = hashlib.sha256(body.encode("utf-8")).hexdigest()
        except Exception as ex:
            result["error"] = f"{type(ex).__name__}: {ex}"[:500]
            if any(term in str(ex) for term in ("access_blocked_http_403", "access_blocked_http_429", "robots_disallow", "robots_unavailable")):
                hard_block = result["error"]
        results.append(result)
        if pos % 25 == 0:
            print(json.dumps({"book": name, "checked": pos, "indexed": len(entries), "ok": sum(r["status"] == "ok" for r in results)}), flush=True)
        if hard_block:
            break
    by_hash: dict[str, list[int]] = {}
    for r in results:
        if r["sha256"]:
            by_hash.setdefault(r["sha256"], []).append(r["ordinal"])
    duplicate_groups = [v for v in by_hash.values() if len(v) > 1]
    success = sum(r["status"] == "ok" for r in results)
    result = {
        "book": name, "provider": provider, "work_id": code,
        "checked_utc": utcnow(), "expected": expected, "indexed": len(entries),
        "scanned": len(results), "ok": success, "failed": len(results) - success,
        "unscanned": max(len(entries) - len(results), 0), "missing_index_numbers": missing_index,
        "index_errors": index_errors, "hard_block": hard_block,
        "duplicate_groups": duplicate_groups,
        "error_summary": dict(Counter(r["error"] for r in results if r["error"])),
        "all_verified": (
            len(entries) == expected and len(results) == expected and success == expected
            and not missing_index and not index_errors and not duplicate_groups
        ),
    }
    # No original prose or HTML is written, ever.
    (output / f"{name}.manifest.json").write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    (output / f"{name}.summary.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return result


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--book", choices=sorted(BOOKS), required=True)
    ap.add_argument("--output", type=Path, default=Path("raw-census-receipts"))
    ap.add_argument("--delay", type=float, default=2.0)
    ap.add_argument("--limit", type=int, default=0, help="diagnostic only; 0 = all")
    args = ap.parse_args(argv)
    result = run_book(args.book, args.output, Fetcher(delay=args.delay), args.limit)
    print(json.dumps(result, indent=2, ensure_ascii=False), flush=True)
    return 0 if result["all_verified"] else 2


if __name__ == "__main__":
    sys.exit(main())
