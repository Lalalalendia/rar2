# PUB VBA Estate Scanner

`pub_vba_estate_scan.py` is a source-neutral, non-executing scanner for VBA projects stored inside CFB-based Microsoft Publisher files.

## Security boundary

The scanner treats VBA as inert evidence. It does not execute VBA, instantiate COM/OLE servers, refresh external links, invoke Microsoft Publisher, or emit recovered module source text. It parses CFB directory/storage structure, applies bounded MS-OVBA decompression, extracts only module metadata needed to locate compressed source, and counts a small allowlisted set of Publisher Object Model call families.

Per-file output is keyed by SHA-256 and byte length by default. Source paths are excluded unless `--include-paths` is explicitly requested.

## Usage

```bash
python tools/pub_vba_estate_scan.py scan \
  --input /path/to/pub-corpus \
  --recursive \
  --output receipt.json
```

For local diagnostics only:

```bash
python tools/pub_vba_estate_scan.py scan \
  --input file.pub \
  --include-paths
```

## States

- `absent` — parsed CFB, no `VBA` storage found.
- `non_project_vba_storage` — one or more storages literally named `VBA` exist, but none satisfy the full MS-OVBA project-root relation: the candidate storage parent must contain a `PROJECT` stream and the candidate `VBA` storage must contain `_VBA_PROJECT` + `dir`. This is explicitly **not** counted as macro-present.
- `structural_only` — structurally valid VBA storage exists but source metadata/source could not be safely extracted.
- `source_extracted` — at least one module source was safely decompressed and all discovered modules were extracted.
- `source_partial` — at least one module source was extracted and at least one module could not be extracted.
- `unknown` — the file could not be parsed/read well enough to classify VBA presence.

These are scanner evidence states, not claims that Publisher would execute a project successfully. A storage name alone is never treated as macro evidence; `vba_project_present_any` only counts structurally admitted MS-OVBA projects. Nested roots are supported, including Publisher-style topologies such as `VBA/PROJECT` plus `VBA/VBA/{_VBA_PROJECT,dir,Module...}`.

## Current call-family taxonomy

The receipt exposes `call_classifier_version`. Classifier `v2.2` keeps the pinned public tutorial calibration from PUB-T-900 and adds a second real-world calibration surface: the 2016 Stack Overflow Publisher bookmark/page-reference macro project. It separates document access from page access, avoids treating unrelated `.Rows` / `.Columns` members such as `LayoutGuides.Rows` as table evidence, and now records page identity, scratch-area, selection and richer hyperlink/text-frame usage that v2.1 under-counted.

Current families:

- `application_lifecycle`
- `documents`
- `pages`
- `page_lifecycle`
- `shapes`
- `text`
- `picture`
- `tables`
- `mail_merge`
- `layout`
- `output`
- `hyperlinks`
- `linked_text`
- `metadata_selectors`
- `ole_links`

Notable v2.2 corrections/additions:

- `ActiveDocument` / `ThisDocument` belong to `documents`, not `pages`;
- `Documents.Add` and `Documents.Open` are document-family evidence without also incrementing the generic `Documents` symbol;
- table `Rows` / `Columns` / `Cells` are counted only through a declared/assigned `Table` variable or an explicit `.Table.<member>` path, so `LayoutGuides.Rows` / `LayoutGuides.Columns` do not contaminate `tables`;
- both `CreateObject("Publisher.Application")` and `GetObject(, "Publisher.Application")` are recognized as application lifecycle calls;
- `LayoutGuides`, `RulerGuides`, `Align` and `Distribute` populate `layout`;
- `SaveAs`, `ExportEmailHTML` and `WebPagePreview` extend `output`;
- `UpdateOLEObjects` extends `ole_links`;\n- `MasterPages` extends `pages`;\n- `PageID` and `PageNumber` populate the new `page_identity` family;\n- `ScratchArea` and `Selection` receive dedicated families rather than disappearing from the histogram;\n- `HasTextFrame` extends `text`;\n- `TargetType` and `TextToDisplay` extend `hyperlinks`.

The scanner masks comments and ordinary string literals before classification. The deliberate exceptions are the Publisher application identity strings inside `CreateObject` and `GetObject`, because those API identities necessarily live in string literals.

## Bounds

- CFB chains are cycle/length checked.
- VBA streams are capped at 16 MiB each.
- VBA module count is capped at 1024.
- MS-OVBA compressed chunks are capped by the 4096-byte decompressed-chunk contract.
- project admission is fail-closed against the MS-OVBA root relation: parent `PROJECT` stream + child `VBA/_VBA_PROJECT` + child `VBA/dir`;
- `dir` module metadata is parsed fail-closed against the documented `PROJECTMODULES` / `MODULE` record sequence.

## Tests

```bash
python tools/test_pub_vba_estate_scan.py
python -m py_compile tools/pub_vba_cfb.py tools/pub_vba_estate_scan.py tools/test_pub_vba_estate_scan.py
```

The source-free synthetic fixtures cover nested Publisher-style macro-project detection, a missing-`PROJECT` negative, storage-name collisions, macro absence, malformed CFB, source extraction, comment/string masking, literal/raw MS-OVBA chunks, CopyToken decoding including the power-of-two `difference=16` boundary, and classifier-v2.2 regressions for document-vs-page separation, non-overlapping document collection calls, context-aware table members, late-bound Publisher acquisition, layout, output, OLE update calls, plus the real-world bookmark/reference families (`MasterPages`, `ScratchArea`, `Selection`, page identity, `HasTextFrame`, and hyperlink target/display members).

## Corpus integration

The first public corpus target is the SHA-pinned 22-file Apache POI Publisher stratum already described by `tools/reader_corpus_manifest_v1.json`. The scanner itself performs no network acquisition; an existing corpus materializer or a dedicated hosted job should provide exact bytes and then invoke this tool. Larger 65/86/1050 corpus runs should reuse their existing exact-source materialization rather than create a second acquisition authority.
