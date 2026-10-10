#!/usr/bin/env python3
"""Export a small private/local pack from an already verified public Narou census.

Run locally; never upload copyrighted prose to a public repository or CI artifact.
The input metadata-only manifests come from RAW-CENSUS-20261010-01.
All chapter bodies must still match the pinned SHA256/length from that census.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tempfile
import time
import unicodedata
import zipfile
from pathlib import Path
from urllib.parse import urlparse
from urllib.robotparser import RobotFileParser

import requests
from bs4 import BeautifulSoup, NavigableString

BOOKS = {"fencing": ("n8001hc", 166), "return": ("n7481gn", 201)}
AGENT = "NovelPersonalReader/1.0 (metadata-verified; noncommercial personal reading)"
HOST = "ncode.syosetu.com"
SELECTORS = [
    "div.js-novel-text.p-novel__text:not(.p-novel__text--preface):not(.p-novel__text--afterword)",
    "div.p-novel__text:not(.p-novel__text--preface):not(.p-novel__text--afterword)",
    "#novel_honbun", "#novel-honbun", "div.novel_view",
]
REMOVABLE = "script, style, iframe, nav, button, noscript, .chapter-nav, .advertisement, .adsbox"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
REPO_ROOT = Path(__file__).resolve().parents[2]


def digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def canon(node) -> str:
    clone = BeautifulSoup(str(node), "html.parser")
    for tag in clone.select(REMOVABLE):
        tag.decompose()
    return unicodedata.normalize("NFC", re.sub(r"\s+", " ", clone.get_text(" ", strip=True)).strip())


def chapter_node(html: str):
    soup = BeautifulSoup(html, "html.parser")
    for selector in SELECTORS:
        nodes = soup.select(selector)
        if nodes:
            node = max(nodes, key=lambda x: len(canon(x)))
            if canon(node):
                return node
    return None


def flatten(node) -> str:
    if isinstance(node, NavigableString):
        return str(node)
    if getattr(node, "name", None) in {"rt", "rp", "script", "style", "iframe", "nav", "button", "noscript"}:
        return ""
    if getattr(node, "name", None) == "br":
        return "\n"
    return "".join(flatten(child) for child in node.children)


def render_paragraphs(node) -> str:
    clone = BeautifulSoup(str(node), "html.parser")
    for tag in clone.select(REMOVABLE):
        tag.decompose()
    ps = clone.select("p")
    chunks = [flatten(p) for p in ps] if ps else [flatten(clone)]
    paragraphs = []
    for chunk in chunks:
        for line in chunk.split("\n"):
            line = unicodedata.normalize("NFC", re.sub(r"[\t\u00a0 ]+", " ", line).strip())
            if line:
                paragraphs.append(line)
    return "\n\n".join(paragraphs)


def evidence(summary_path: Path, manifest_path: Path, book: str) -> dict[int, dict]:
    code, expected = BOOKS[book]
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    required = ("expected", "indexed", "scanned", "ok")
    if (summary.get("book"), summary.get("work_id"), summary.get("provider")) != (book, code, "narou"):
        raise ValueError("census identity mismatch")
    if any(summary.get(k) != expected for k in required) or summary.get("all_verified") is not True:
        raise ValueError("census is incomplete or not verified")
    if summary.get("failed") != 0 or summary.get("unscanned") != 0 or summary.get("duplicate_groups") or summary.get("missing_index_numbers"):
        raise ValueError("census contains failures or duplicate/missing records")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(data, list) or len(data) != expected:
        raise ValueError("manifest length mismatch")
    seen: dict[int, dict] = {}
    hashes: set[str] = set()
    for row in data:
        if not isinstance(row, dict) or type(row.get("ordinal")) is not int:
            raise ValueError("invalid manifest row")
        n = row["ordinal"]
        url = f"https://{HOST}/{code}/{n}/"
        h = row.get("sha256")
        if (n < 1 or n > expected or n in seen or row.get("url") != url
                or row.get("status") != "ok" or type(row.get("chars")) is not int
                or row["chars"] < 120 or not isinstance(h, str) or not SHA_RE.fullmatch(h)
                or h in hashes):
            raise ValueError(f"invalid/duplicate census evidence at ordinal {n}")
        seen[n] = row
        hashes.add(h)
    if set(seen) != set(range(1, expected + 1)):
        raise ValueError("manifest is not contiguous 1..N")
    return seen


class Reader:
    """Bounded, public, unauthenticated HTTPS reader with a robots gate."""

    def __init__(self, delay: float = 2.5):
        self.session = requests.Session()
        self.session.headers.update({"User-Agent": AGENT, "Accept-Language": "ja,en;q=0.7"})
        self.delay = max(2.5, delay)
        self.last = 0.0
        self.robots = None

    def get(self, url: str) -> str:
        parts = urlparse(url)
        if parts.scheme != "https" or parts.hostname != HOST:
            raise RuntimeError("URL rejected outside allowed public source")
        if self.robots is None:
            rr = self.session.get(f"https://{HOST}/robots.txt", timeout=25, allow_redirects=False)
            if rr.status_code not in (200, 404):
                raise RuntimeError(f"robots gate unavailable ({rr.status_code})")
            parser = RobotFileParser()
            parser.parse(rr.text.splitlines() if rr.status_code == 200 else [])
            self.robots = parser
        if not self.robots.can_fetch(AGENT, url):
            raise RuntimeError("robots policy denies this URL")
        elapsed = time.monotonic() - self.last
        if self.last and elapsed < self.delay:
            time.sleep(self.delay - elapsed)
        self.last = time.monotonic()
        resp = self.session.get(url, timeout=25, allow_redirects=False)
        if resp.status_code != 200 or resp.url != url:
            raise RuntimeError(f"HTTP page not admitted ({resp.status_code})")
        return resp.text


def validated_chapter(fetcher, row: dict) -> tuple[str, str]:
    body = chapter_node(fetcher.get(row["url"]))
    if body is None:
        raise ValueError(f"No readable chapter body at #{row['ordinal']}")
    canonical = canon(body)
    if len(canonical) != row["chars"] or digest(canonical) != row["sha256"]:
        raise ValueError(f"Stale or mismatched census at #{row['ordinal']}; renew census before export")
    readable = render_paragraphs(body)
    if not readable or len(readable) < len(canonical) * 0.55:
        raise ValueError(f"Incomplete readable text at #{row['ordinal']}")
    return canonical, readable


def safe_output(location: Path) -> Path:
    path = location.expanduser().resolve()
    if path == REPO_ROOT or REPO_ROOT in path.parents:
        raise ValueError("Refusing to write source prose inside public Git repository")
    return path


def export_pack(book: str, summary: Path, manifest: Path, start: int, count: int, out: Path, fetcher=None) -> Path:
    if book not in BOOKS or count < 1 or count > 10 or start < 1 or start + count - 1 > BOOKS[book][1]:
        raise ValueError("Invalid book or bounded pack range (1..10 chapters)")
    output = safe_output(out)
    all_rows = evidence(summary, manifest, book)
    reader = fetcher if fetcher is not None else Reader()
    chapters: list[tuple[int, str, dict]] = []
    # Verify EVERY requested chapter before creating ANY artifact.
    for n in range(start, start + count):
        row = all_rows[n]
        _, text = validated_chapter(reader, row)
        title = re.sub(r"[\x00-\x1f\x7f]", " ", str(row.get("title") or f"Chapter {n}"))[:180].strip()
        source = row["url"]
        content = f"{n:03d} — {title}\nИсточник: {source}\n\n{text}\n"
        chapters.append((n, content, {
            "number": n, "title": title, "url": source, "canonical_sha256": row["sha256"],
            "text_sha256": digest(content), "characters": len(text),
            "paragraphs": len(text.split("\n\n")),
        }))
    name = f"{book}-{start:03d}-{start + count - 1:03d}"
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"{name}.zip"
    if destination.exists():
        raise FileExistsError(f"Will not overwrite existing pack: {destination}")
    # Files are assembled in a temporary location outside the repo, and made visible atomically.
    with tempfile.TemporaryDirectory(prefix="pilot-", dir=output) as tmp:
        staged = Path(tmp) / destination.name
        with zipfile.ZipFile(staged, "w", zipfile.ZIP_DEFLATED) as archive:
            for n, data, _ in chapters:
                archive.writestr(f"{name}/{n:03d}.txt", data)
            index = {"book": book, "work_id": BOOKS[book][0], "chapter_start": start,
                     "chapter_end": start + count - 1, "chapters": [rec for _, _, rec in chapters],
                     "source": "RAW-CENSUS-20261010-01; source SHA256 compared for each chapter",
                     "intended_use": "Local private reading/translation; not publication"}
            archive.writestr(f"{name}/index.json", json.dumps(index, indent=2, ensure_ascii=False) + "\n")
        os.replace(staged, destination)
    return destination


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--book", choices=sorted(BOOKS), required=True)
    parser.add_argument("--summary", required=True, type=Path, help="downloaded metadata-only *.summary.json")
    parser.add_argument("--manifest", required=True, type=Path, help="downloaded metadata-only *.manifest.json")
    parser.add_argument("--start", type=int, default=1)
    parser.add_argument("--count", type=int, default=5)
    parser.add_argument("--output", required=True, type=Path, help="private local directory OUTSIDE this git repo")
    args = parser.parse_args(argv)
    path = export_pack(args.book, args.summary, args.manifest, args.start, args.count, args.output)
    print(json.dumps({"book": args.book, "start": args.start, "end": args.start + args.count - 1,
                      "local_zip": str(path), "zip_sha256": hashlib.sha256(path.read_bytes()).hexdigest()}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
