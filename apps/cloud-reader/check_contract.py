#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parent
HTML = (ROOT / "index.html").read_text(encoding="utf-8")
RENDERER = (ROOT / "render-v1.mjs").read_text(encoding="utf-8")

required = [
    "Open a Publisher (.PUB) file online",
    "/v1/reader/guest-sessions",
    "/v1/reader/documents/",
    "x-chaptera-reader-session",
    "expected_byte_len: file.size",
    "research contribution are separate actions",
    "temporary service processing only",
    "read-only",
    'renderReaderScene',
    './render-v1.mjs',
]
for needle in required:
    if needle not in HTML:
        raise SystemExit(f"cloud-reader contract missing required marker: {needle!r}")

forbidden = [
    "/commit",
    "/v1/documents/",
    "/v1/projects/",
    "/v1/projects/from-upload",
    "/v1/uploads",
    "payload.geometry",
    "localStorage",
    "sessionStorage",
    "new FormData",
    "file.name",
]
for needle in forbidden:
    if needle in HTML:
        raise SystemExit(f"cloud-reader contract contains forbidden authority/retention marker: {needle!r}")

if 'body: file' not in HTML:
    raise SystemExit("guest upload must remain raw-body, not filename-bearing multipart")

print("cloud-reader read-only/ephemeral-consent contract: ok")


renderer_required = [
    "chaptera.reader-scene.v1",
    "inline_data_url",
    "imagePaintGeometry",
    "tableCellPaintGeometry",
    "resolvedTextLinePaintPlan",
    "server-shared-resolved",
    "data-table-cell-id",
    "assertReaderSceneSourceNeutral",
    'data-renderer',
]
for needle in renderer_required:
    if needle not in RENDERER:
        raise SystemExit(f"cloud-reader renderer missing required marker: {needle!r}")

for needle in forbidden:
    if needle in RENDERER:
        raise SystemExit(
            f"cloud-reader renderer contains forbidden authority/retention marker: {needle!r}"
        )

for needle in [
    "raw_pub_bytes",
    "source_path",
    "filesystem_path",
    "parser_record",
    "stream_path",
]:
    if needle not in RENDERER:
        raise SystemExit(
            f"cloud-reader renderer source-neutral guard missing forbidden key: {needle!r}"
        )
