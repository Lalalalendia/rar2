#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parent
HTML = (ROOT / "index.html").read_text(encoding="utf-8")

required = [
    "Open a Publisher (.PUB) file online",
    "/v1/reader/documents/",
    "research contribution are separate actions",
    "read-only",
]
for needle in required:
    if needle not in HTML:
        raise SystemExit(f"cloud-reader contract missing required marker: {needle!r}")

forbidden = [
    "/commit",
    "/v1/documents/",
    "method: \"POST\"",
    "method:'POST'",
    'method:"POST"',
    "payload.geometry",
]
for needle in forbidden:
    if needle in HTML:
        raise SystemExit(f"cloud-reader contract contains forbidden mutation marker: {needle!r}")

print("cloud-reader read-only/consent contract: ok")
