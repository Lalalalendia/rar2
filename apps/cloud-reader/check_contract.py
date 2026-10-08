#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parent
HTML = (ROOT / "index.html").read_text(encoding="utf-8")
APP = (ROOT / "reader-app.mjs").read_text(encoding="utf-8")
MODEL = (ROOT / "reader-model.mjs").read_text(encoding="utf-8")
CSS = (ROOT / "reader.css").read_text(encoding="utf-8")
SURFACE = HTML + APP + MODEL
RENDERER = (ROOT / "render-v1.mjs").read_text(encoding="utf-8")

required = [
    "Open a PUB file",
    "Choose or drop a .PUB file",
    "Read-only · your original PUB is not modified.",
    'id="details-dialog"',
    'id="reader-warning"',
    "/v1/reader/guest-sessions",
    "/v1/reader/documents/",
    "x-chaptera-reader-session",
    "expected_byte_len: file.size",
    "Contribute this exact file?",
    "the exact file will be retained separately",
    "chaptera-intake-consent-v1",
    "chaptera-intake-retention-v1",
    "x-chaptera-reader-contribution",
    "contributionEligible",
    'renderReaderScene',
    './render-v1.mjs',
]
for needle in required:
    if needle not in SURFACE:
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
    if needle in SURFACE:
        raise SystemExit(f"cloud-reader contract contains forbidden authority/retention marker: {needle!r}")

if 'body: file' not in APP:
    raise SystemExit("guest upload must remain raw-body, not filename-bearing multipart")

for needle in [
    "height:100dvh",
    "html,body{height:100%;overflow:hidden",
    ".viewer{",
    "overflow:auto;",
    "overscroll-behavior:contain",
]:
    if needle not in CSS:
        raise SystemExit(f"cloud-reader fixed-shell contract missing: {needle!r}")

if 'if (file) openFile(file);' not in APP:
    raise SystemExit("cloud-reader file selection must auto-open the selected PUB")

if "const AUTO_MAX_PAGE_WIDTH = 960;" not in APP:
    raise SystemExit("cloud-reader automatic fit must keep the desktop page comfortably bounded")

head = HTML.split("</head>", 1)[0]
if "\\n" in head:
    raise SystemExit("cloud-reader HTML head must not contain a literal backslash-n sequence")

for needle in [
    'property="og:url" content="https://reader.chaptera.online/"',
    'property="og:image" content="https://chaptera.online/social-preview.png"',
    'property="og:image:width" content="1200"',
    'property="og:image:height" content="630"',
    'name="twitter:card" content="summary_large_image"',
]:
    if needle not in HTML:
        raise SystemExit(f"cloud-reader social preview metadata missing: {needle!r}")

print("cloud-reader read-only/ephemeral-consent/simple-viewer contract: ok")

for needle in ["#page-select", "#zoom-select", "#search-query", "#story-text", "#assets", "#limitations"]:
    if needle not in APP:
        raise SystemExit(f"cloud-reader reading control missing: {needle!r}")
for needle in ["AbortController", "isCurrent(operation)", "guestRequestPath", "credentials: \"omit\""]:
    if needle not in APP:
        raise SystemExit(f"cloud-reader open lifecycle guard missing: {needle!r}")
if "innerHTML" in SURFACE:
    raise SystemExit("Reader recovered content must use textContent, not HTML injection")


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
