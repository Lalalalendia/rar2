#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

from operation_blast_radius_v1 import stream_inventory


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalize_publisher_pdf(data: bytes) -> bytes:
    """Remove known per-export identity/timestamp noise from Publisher PDFs."""
    replacements: list[tuple[bytes, bytes]] = [
        (
            rb"/CreationDate\s*\(D:[^)]*\)",
            b"/CreationDate(D:00000000000000Z)",
        ),
        (
            rb"/ModDate\s*\(D:[^)]*\)",
            b"/ModDate(D:00000000000000Z)",
        ),
        (
            rb"<xmp:CreateDate>.*?</xmp:CreateDate>",
            b"<xmp:CreateDate>0000-00-00T00:00:00Z</xmp:CreateDate>",
        ),
        (
            rb"<xmp:ModifyDate>.*?</xmp:ModifyDate>",
            b"<xmp:ModifyDate>0000-00-00T00:00:00Z</xmp:ModifyDate>",
        ),
        (
            rb"<xmp:MetadataDate>.*?</xmp:MetadataDate>",
            b"<xmp:MetadataDate>0000-00-00T00:00:00Z</xmp:MetadataDate>",
        ),
        (
            rb"<xmpMM:DocumentID>.*?</xmpMM:DocumentID>",
            b"<xmpMM:DocumentID>uuid:00000000-0000-0000-0000-000000000000</xmpMM:DocumentID>",
        ),
        (
            rb"<xmpMM:InstanceID>.*?</xmpMM:InstanceID>",
            b"<xmpMM:InstanceID>uuid:00000000-0000-0000-0000-000000000000</xmpMM:InstanceID>",
        ),
        (
            rb"/ID\s*\[\s*<[^>]+>\s*<[^>]+>\s*\]",
            b"/ID[<00000000000000000000000000000000><00000000000000000000000000000000>]",
        ),
    ]
    out = data
    for pattern, replacement in replacements:
        out = re.sub(pattern, replacement, out, flags=re.DOTALL)

    # Publisher's XMP may also carry bare uuid: values in attributes.
    out = re.sub(
        rb"uuid:[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}",
        b"uuid:00000000-0000-0000-0000-000000000000",
        out,
    )
    return out


def persistence_fingerprint(pub_data: bytes) -> dict[str, Any]:
    _, streams = stream_inventory(pub_data)
    logical_streams = sorted(
        (
            {
                "name": item["name"],
                "size": item["size"],
                "sha256": item["sha256"],
            }
            for item in streams.values()
        ),
        key=lambda item: (item["name"], item["size"], item["sha256"]),
    )
    return {
        "contract": "chaptera.pub-logical-stream-multiset.v1",
        "stream_count": len(logical_streams),
        "streams": logical_streams,
    }


def build_fingerprint(pub_path: Path, pdf_path: Path) -> dict[str, Any]:
    pub_data = pub_path.read_bytes()
    pdf_data = pdf_path.read_bytes()
    normalized_pdf = normalize_publisher_pdf(pdf_data)
    return {
        "schema": "chaptera.pub-operation-algebra-fingerprint.v1",
        "artifacts": {
            "pub_sha256": sha256(pub_data),
            "pub_size": len(pub_data),
            "pdf_sha256": sha256(pdf_data),
            "pdf_size": len(pdf_data),
            "normalized_pdf_sha256": sha256(normalized_pdf),
        },
        "persistence_fingerprint": persistence_fingerprint(pub_data),
        "render_fingerprint": {
            "contract": "chaptera.publisher-pdf-normalized.v1",
            "sha256": sha256(normalized_pdf),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pub", required=True, type=Path)
    parser.add_argument("--pdf", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    payload = build_fingerprint(args.pub, args.pdf)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "schema": payload["schema"],
                "pub_sha256": payload["artifacts"]["pub_sha256"],
                "normalized_pdf_sha256": payload["artifacts"]["normalized_pdf_sha256"],
                "stream_count": payload["persistence_fingerprint"]["stream_count"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
