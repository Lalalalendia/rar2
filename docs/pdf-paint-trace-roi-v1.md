# PDF-PAINT-TRACE-01 — bounded object-to-pixel candidate diagnostics

This is an **opt-in, local-only diagnostic**. The real Microsoft Publisher PDF is still the fidelity reference. Do not upload customer PUBs, PDFs, raster previews, source Story text, glyphs, fonts, full trace sidecars, or document-specific pixel masks to public GitHub Actions or Notion.

## Stage 1 — emitted PDF commands

`pub-pdf::render_bounded_pdf` returns a `PdfRenderOutput.paint_trace` **sidecar**, separate from `PdfRenderReport`. This separation preserves existing public `.loss.json` and golden byte hashes. For each paint atom actually emitted:

- `page_origin`, `node_origin`, `page_paint_index` are page-local, in back-to-front compositor order.
- `kind` = rectangle/image/text; optionally `resource_origin` (exact image ResourceId) or `text_run_ordinal`.
- `node_bounds_emu` is the projected node envelope, not exact ink/crop/clip pixels.
- `unwrapped_program_byte_range` addresses the precise operation in the PDF content **before** page-height transform. It does *not* refer to an absolute file offset or source-PUB byte range.
- `output_sheet_copies=9` marks nine-up repetition. The ROI join refuses these events until sheet transforms are modeled explicitly.

Unsupported/unemitted objects remain in existing `PdfNodeReport` diagnostics and **do not** create false paint events.

## Opt-in real-PUB CLI

For an authorized local source and licensed explicit fallback font, run the existing `pub-cli convert --to pdf --fallback-font ...` command with the environment variable `CHAPTERA_PDF_PAINT_TRACE=1`. It writes an additional local `<output.pdf>.paint-trace.json` alongside normal `<output.pdf>.loss.json` and `.loss.txt`. Default conversion writes **no** diagnostic sidecar.

The opt-in trace has `schema_version: chaptera.pdf-paint-trace.v1`, the exact output PDF SHA-256, `pages`, and `paint_trace`. It also includes the **primary source PUB SHA-256** and `source_nodes`, joined by the *same* canonical `NodeId` from the already loaded `resolved_graph`. No new PUB parsing is performed.

For each painted node, `source_nodes` reports only `SourceRef` values that satisfy **all** conditions: validate against the same primary source descriptor, `authority=authoritative`, `confidence=exact` and `role=semantic|projection`. It labels links `exact_source_byte_range` when the verified ref includes a byte offset/length, `exact_source_object_only` when only its carrier/object identity is available, `no_exact_authoritative_source_ref` if the canonical graph node is present without admitted refs, and `unmapped_projected_node` if it is not found in the canonical graph. No stage invents object links from nearby rectangles. A SourceRef is provenance, **not** proof of crop, font, transform or exact raster ink.

If a file is private, its geometry, carrier/keys/offsets and IDs are also private: do not collect it in public CI or telemetry. The CLI uses the existing `AdmittedSource::commit_derived_output` output boundary. Normal PDF bytes and standard loss report remain unchanged.

## Stage 1b — Viewer image placements versus fixed-PDF handoff

When source provenance is enabled, the private diagnostic now includes `image_inputs` for **each actually painted** image atom. Each entry joins by the *same* `NodeId + ResourceId` as the paint trace. It records source-exact status, MIME, Viewer source-window edges in Q16 (if explicitly present), Viewer recolor/rotation evidence and the corresponding placement flags **actually transferred** to `FixedPdfResources`.

The `transfer_status` is an explicit handoff result, not a visual correctness claim. Of special interest:

- `viewer_crop_not_forwarded`: Viewer recovered an explicit source crop, but the PDF adapter did not pass it on. The current bounded adapter can discard a standalone PNG/JPEG crop while still painting the image using its node rectangle. This is a **diagnosable fidelity loss**, not permission to label the output uncropped in Publisher.
- `viewer_recolor_not_forwarded` or `viewer_rotation_not_forwarded`: recovered source-image presentation was not forwarded.
- `placement_fields_forwarded`: tracked fields were retained. This **does not prove** image source crop/clip, scaling or pixel appearance matches Publisher.
- `no_explicit_viewer_placement`: no explicit placement was present in the Viewer projection; the original PUB may still have presentation semantics that the parser did not recover.
- Ambiguous or unmatched image use identities remain explicit failures of attribution; no nearest-object match.

The ROI join validates that every painted image has **exactly one** matching input entry, checks Viewer crop coordinates and evidence types, then attaches `image_placement_evidence` to changed-pixel **candidate** events. Older pre-extension trace sidecars remain usable and mark image evidence unavailable.

## Stage 2 — join with visual differences

Use the existing `tools/pdf_reference_diff_v1.py` comparator on **the same exact candidate PDF** and an external Publisher PDF to produce `chaptera.fidelity-reference-pdf.v1`. It yields page boxes, raster metadata and a compact significant-difference tile RLE mask.

Then run:

```sh
python tools/pdf_paint_trace_roi_join_v1.py \
  --trace private/output.pdf.paint-trace.json \
  --reference-diff private/reference-diff-receipt.json \
  --output private/paint-roi-candidates.json
```

The join requires matching candidate PDF SHA-256, page counts, boxes, unrotated/untranslated coordinates, MuPDF RGB + deterministic DPI, valid bounded RLE, contiguous per-page painter indices and one physical output copy. When `source_nodes` is supplied, it additionally validates primary PUB SHA-256 against every admitted SourceRef, exact authority, non-overflowing byte ranges and the painted NodeId set, and joins refs only by exact ID. Older trace receipts without `source_nodes` remain supported with `source_link_status=trace_not_available`. It refuses mismatches; it does **not** guess a geometric registration. It reports:

- significant-pixel count and changed tile count by page;
- unmatched changed tiles that no emitted node envelope overlaps;
- ambiguous tiles with multiple overlapping paint atoms;
- per-atom candidate tile counts, the last-painted candidate count and source identities.

These candidate tile counts **cannot be summed** into a pixel contribution score: under clipping, transparency, antialiasing and occlusion a geometric overlap is neither sufficient nor necessary proof of causation. To prove a particular SBDC discrepancy one must additionally compare controlled per-node isolation or a source-authoritative image/crop/text trace.

## Validation

`Free PUB PDF CLI V1` runs the source-neutral Rust tests for exact overlapping images, page-local ordinal ordering and byte-offset correspondence while retaining PDF/loss hashes. `FIDELITY-REFERENCE-PDF-01` runs `tools/test_pdf_paint_trace_roi_join_v1.py` against synthetic 100pt pages with deliberately overlapping objects, reverse source order, unrelated objects, stale PDF SHA, malformed RLE and rotated/cropped inputs.

Image placement handoff diagnostics do not include exact ink/crop/clip raster or a source extraction from Publisher. They only distinguish what Viewer reported from what the bounded PDF adapter forwarded.

Next integration work, **not yet implemented**: resolve inherited/derived or indirect shape mappings not represented by a direct `NodeId`, exact image crop/clip and glyph ink bounds, isolated per-object raster, and parity against Editor SVG of the same exact revision. Exact-authority direct `NodeId → SourceRef` is implemented when available, but many nodes will correctly remain unresolved: the trace is **not** blanket pixel-to-PUB attribution.
