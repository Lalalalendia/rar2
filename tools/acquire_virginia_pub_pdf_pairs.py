#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import html
import json
import re
import sys
import time
from pathlib import Path
from urllib.parse import parse_qs, urljoin, urlparse

import requests

USER_AGENT = "Mozilla/5.0 (compatible; Chaptera-Paired-Oracle/1.0)"
MAX_PAGE_BYTES = 2_000_000

PAIRS = [
    {
        "pair_id": "virginia-devinettes-2021",
        "required": True,
        "source_page": "https://laclassedevirginia.blogspot.com/2021/08/devinettes-de-rentree.html",
        "pub": {
            "anchor": "Devinettes format modifiable Publisher",
            "artifact": "virginia-devinettes.pub",
            "size": 138_240,
            "sha256": "077612c7a228bd20bded939afde129cbdedae9b01b4f138f4619e332e5d7bd2e",
            "expected_drive_id": "13sib4Vd82kSeFV1YcfWNLkNxXGpN_6gk",
        },
        "pdf": {
            "anchor": "Devinettes à télécharger en PDF",
            "artifact": "virginia-devinettes-reference.pdf",
            "size": 88_364,
            "sha256": "5e7f716e1f3e828546a21a7c790b96663294a636cb5866ad6fc46c40db7d6bf6",
            "expected_drive_id": "15rqOJsTr9apuZHztNRGhaFBWTK4K_h8D",
        },
    },
    {
        "pair_id": "virginia-remplacante-zone-a-2015",
        "required": False,
        "source_page": "https://laclassedevirginia.blogspot.com/2015/08/cahier-de-la-maitresse-remplacante.html",
        "source_fallbacks": [
            "https://laclassedevirginia.blogspot.com/2015/08/",
            "https://laclassedevirginia.blogspot.com/2015/",
        ],
        "pub": {
            "anchor": "Version modifiable",
            "artifact": "virginia-remplacante-modifiable.pub",
            "size": 8_376_832,
            "sha256": "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506",
            "expected_drive_id": "0B6lkYRCQasWaVVhWVzVUUU5fZVE",
        },
        "pdf": {
            "anchor": "PDF Zone A",
            "artifact": "virginia-remplacante-zone-a-reference.pdf",
            "size": 6_086_212,
            "sha256": "df885f7288974bfaff9ec3c15416c0e98c5a4977c96618eba1572707937669f3",
            "expected_drive_id": "0B6lkYRCQasWaTGlRak1OOXl4ZGs",
        },
    },
]


def normalized_text(value: str) -> str:
    value = re.sub(r"<[^>]+>", " ", value)
    value = html.unescape(value)
    return " ".join(value.replace("\xa0", " ").split()).casefold()


def source_html(
    session: requests.Session, urls: list[str]
) -> tuple[str, str, str]:
    failures = []
    for url in urls:
        last_error: Exception | None = None
        last_status: int | None = None
        for attempt in range(3):
            try:
                response = session.get(
                    url,
                    params={"chaptera_pair_oracle": str(time.time_ns())},
                    timeout=30,
                )
                last_status = response.status_code
                if response.status_code == 429:
                    time.sleep(2 ** attempt)
                    continue
                response.raise_for_status()
                if len(response.content) > MAX_PAGE_BYTES:
                    raise RuntimeError("source page exceeded bounded HTML ceiling")
                return url, response.url, response.text
            except requests.RequestException as exc:
                last_error = exc
                time.sleep(2 ** attempt)
        failures.append(
            {
                "url": url,
                "status": last_status,
                "error": str(last_error) if last_error is not None else None,
            }
        )
    raise RuntimeError(
        "source pages unavailable: " + json.dumps(failures, sort_keys=True)
    )


def anchors(raw: str, base_url: str) -> list[tuple[str, str]]:
    found = []
    for match in re.finditer(
        r'<a\b[^>]*\bhref=(?:"([^"]+)"|\'([^\']+)\')[^>]*>(.*?)</a>',
        raw,
        flags=re.I | re.S,
    ):
        href = match.group(1) or match.group(2)
        label = normalized_text(match.group(3))
        if href and label:
            found.append((label, urljoin(base_url, html.unescape(href))))
    return found


def exact_anchor_link(entries: list[tuple[str, str]], label: str) -> str:
    needle = normalized_text(label)
    exact = [url for text, url in entries if text == needle]
    if len(exact) == 1:
        return exact[0]
    contains = [url for text, url in entries if needle in text]
    if len(contains) == 1:
        return contains[0]
    raise RuntimeError(
        f"anchor {label!r} is not unique: exact={len(exact)} contains={len(contains)}"
    )


def google_drive_id(url: str) -> str | None:
    parsed = urlparse(url)
    if parsed.hostname not in {"drive.google.com", "docs.google.com"}:
        return None
    match = re.search(r"/file/d/([A-Za-z0-9_-]+)", parsed.path)
    if match:
        return match.group(1)
    query_id = parse_qs(parsed.query).get("id", [])
    return query_id[0] if len(query_id) == 1 else None


def candidate_urls(link: str) -> list[str]:
    drive_id = google_drive_id(link)
    if not drive_id:
        return [link]
    return [
        f"https://drive.usercontent.google.com/download?id={drive_id}&export=download&confirm=t",
        f"https://drive.google.com/uc?export=download&id={drive_id}",
    ]


def pinned_drive_link(spec: dict) -> str:
    drive_id = spec.get("expected_drive_id")
    if not drive_id:
        raise RuntimeError(f"{spec['artifact']}: no pinned Drive identity")
    return f"https://drive.google.com/file/d/{drive_id}/view"


def acquire_exact(
    session: requests.Session,
    link: str,
    spec: dict,
    output_dir: Path,
) -> dict:
    drive_id = google_drive_id(link)
    expected_drive_id = spec.get("expected_drive_id")
    if expected_drive_id is not None and drive_id != expected_drive_id:
        raise RuntimeError(
            f"{spec['artifact']}: Drive identity drift {drive_id!r} != {expected_drive_id!r}"
        )

    target = output_dir / spec["artifact"]
    last_status = None
    for candidate in candidate_urls(link):
        target.unlink(missing_ok=True)
        digest = hashlib.sha256()
        size = 0
        try:
            with session.get(
                candidate,
                timeout=(15, 120),
                stream=True,
                allow_redirects=True,
            ) as response:
                last_status = response.status_code
                if response.status_code != 200:
                    continue
                content_type = response.headers.get("content-type", "")
                with target.open("wb") as out:
                    for chunk in response.iter_content(1024 * 1024):
                        if not chunk:
                            continue
                        size += len(chunk)
                        if size > spec["size"]:
                            raise RuntimeError(
                                f"{spec['artifact']}: download exceeded exact expected size"
                            )
                        digest.update(chunk)
                        out.write(chunk)
        except requests.RequestException:
            target.unlink(missing_ok=True)
            continue

        if size != spec["size"]:
            target.unlink(missing_ok=True)
            continue
        actual_sha = digest.hexdigest()
        if actual_sha != spec["sha256"]:
            target.unlink(missing_ok=True)
            raise RuntimeError(
                f"{spec['artifact']}: SHA drift {actual_sha} != {spec['sha256']}"
            )
        return {
            "artifact": spec["artifact"],
            "size": size,
            "sha256": actual_sha,
            "source_link_host": urlparse(link).hostname,
            "download_host": urlparse(candidate).hostname,
            "drive_id": drive_id,
            "http_status": last_status,
            "content_type": content_type,
        }

    raise RuntimeError(
        f"{spec['artifact']}: all download candidates failed; last_status={last_status}"
    )


def main(argv: list[str]) -> None:
    if len(argv) != 2:
        raise SystemExit("usage: acquire_virginia_pub_pdf_pairs.py OUTPUT_DIR")
    output_dir = Path(argv[1])
    output_dir.mkdir(parents=True, exist_ok=True)

    session = requests.Session()
    session.headers.update(
        {
            "User-Agent": USER_AGENT,
            "Accept": "*/*",
            "Cache-Control": "no-cache",
            "Pragma": "no-cache",
        }
    )

    receipt_pairs = []
    for pair in PAIRS:
        source_page_revalidated = True
        source_page_error_class = None
        locator_basis = "live_first_party_anchor"
        try:
            try:
                selected_source, final_url, raw = source_html(
                    session,
                    [pair["source_page"], *pair.get("source_fallbacks", [])],
                )
                entries = anchors(raw, final_url)
                pub_link = exact_anchor_link(entries, pair["pub"]["anchor"])
                pdf_link = exact_anchor_link(entries, pair["pdf"]["anchor"])
            except RuntimeError as source_error:
                # The Drive identities below were pinned from an earlier successful
                # fetch of these exact first-party Blogspot anchors. A transient
                # source-page 429 must not force re-discovery or weaken byte identity.
                if not (
                    pair["pub"].get("expected_drive_id")
                    and pair["pdf"].get("expected_drive_id")
                    and str(source_error).startswith("source pages unavailable:")
                ):
                    raise
                selected_source = pair["source_page"]
                final_url = None
                pub_link = pinned_drive_link(pair["pub"])
                pdf_link = pinned_drive_link(pair["pdf"])
                source_page_revalidated = False
                source_page_error_class = type(source_error).__name__
                locator_basis = "pinned_first_party_anchor_drive_id"

            pub = acquire_exact(session, pub_link, pair["pub"], output_dir)
            pdf = acquire_exact(session, pdf_link, pair["pdf"], output_dir)
        except Exception as error:
            if pair.get("required", False):
                raise
            receipt_pairs.append(
                {
                    "pair_id": pair["pair_id"],
                    "source_page": pair["source_page"],
                    "pair_class": "exact_source_pair",
                    "status": "source_temporarily_unavailable",
                    "error_class": type(error).__name__,
                }
            )
            print(
                f"UNAVAILABLE {pair['pair_id']}: "
                f"{type(error).__name__}",
                file=sys.stderr,
            )
            continue

        receipt_pairs.append(
            {
                "pair_id": pair["pair_id"],
                "source_page": pair["source_page"],
                "selected_source_page": selected_source,
                "resolved_source_page": final_url,
                "source_page_revalidated": source_page_revalidated,
                "source_page_error_class": source_page_error_class,
                "locator_basis": locator_basis,
                "pair_class": "exact_source_pair",
                "status": "acquired",
                "pub": pub,
                "pdf": pdf,
            }
        )
        print(
            f"ACQUIRED {pair['pair_id']}: "
            f"PUB={pub['sha256']} PDF={pdf['sha256']}"
        )

    receipt = {
        "schema": "chaptera.public-pub-pdf-pairs.v1",
        "binary_payloads_committed": False,
        "pairs": receipt_pairs,
    }
    (output_dir / "acquisition.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main(sys.argv)
