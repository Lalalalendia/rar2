# PDF-PUB-EXTRACT-01 — PDF paint probe (research only)

This is a **bounded diagnostics / extraction adapter** toward PDF → *real native Microsoft Publisher* `.pub` synthesis. It does **not** produce a PUB file, a Chaptera canonical document, or a native Publisher-authorized edit. Source authority: [Notion task](https://app.notion.com/p/3f732a84beec819da608cff1776610b4) and [GitHub #2647](https://github.com/Lalalalendia/rar2/issues/2647).

## Optional dependency and license gate

The prototype uses **PyMuPDF 1.26.7**, a third-party package licensed under AGPL-3.0 or a separate commercial agreement. Treat it as a **local, optional, non-product research tool**; do not link it into Chaptera's server or distribute a product package containing it without a deliberate licensing review. Select a license-compatible PDF parsing backend before production integration. No PyMuPDF package or PDF corpus bytes are added to the repository.

## Run locally

```
python -m pip install 'PyMuPDF==1.26.7'
python -m unittest discover -s tools -p test_pdf_pub_extract_v1.py -v
python tools/pdf_pub_extract_v1.py input.pdf --output private-candidates.json
```

- Requires a **new output path**. Output JSON is created privately, mode `0600` on Unix, without overwrite.
- Stdout contains counts and status only, not source text or local paths. JSON **contains source text** and must **not** be uploaded to public CI, Notion or GitHub for private/customer PDFs.
- Exit `0` = bounded candidate, `3` = explicit partial/unsupported, `2` = rejected or file I/O failure.
- **Do not process untrusted PDFs in a public-facing server with this CLI**. Isolated-worker/quotas/security engineering is a separate production requirement.

## Contract

`chaptera.pdf-pub-extract.v1` contains source SHA-256, visible page dimensions in 12,700 EMU/pt, source-ordered candidate `text_line` and opaque `filled_rectangle` paint elements, explicit page diagnostics, and `native_pub_written: false`. Text coordinates use PyMuPDF's top-left page space only for unrotated, uncropped, default-UserUnit pages. `editable: candidate_only` means **no native PUB write capability** has been proven.

This probe only admits opaque horizontal single-trace RGB text and plain filled RGB rectangles. **Nontrivial transforms, images, clipping, groups, links/annotations, path strokes, unsupported graphic paint, transparency, and overlapping candidates** are partial/diagnostic. Missing/ambiguous text mapping is partial. Font names are *hints*, not the physical font resource.

## Required downstream checks

1. Adapt typed candidates to the **existing** Chaptera source-neutral canonical model, without second document authority.
2. Extend the existing hash-pinned seeded PUB writer ([#1379](https://github.com/Lalalalendia/rar2/pull/1379)) using proven object/Story allocation and multi-stream consistency; no arbitrary PUB promise.
3. Validate actual new PUB through **Publisher Open → Save → Close → fresh Reopen**, object-editability/Story semantics and independent PDF visual comparison. Publisher acceptance is a distinct evidence gate.

All tests use synthetic generated PDFs; no Publisher binaries, real customer text or proprietary blank PUB bytes are committed.
