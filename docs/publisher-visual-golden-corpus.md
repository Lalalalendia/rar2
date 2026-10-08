# Publisher visual golden corpus

This registry tracks externally held Microsoft Publisher PDF visual-oracle bytes against immutable PUB source identities.

## Batch 01 / Tranche A

- IDs: 001-040
- Publisher runtime: 16.0 build 12527 (Publisher 2019)
- Exported PDFs: 23/40
- Publisher no-open: 17/40
- External PDF bundle SHA-256: `d7ee4d6a5976736fa4108f3fc9236d6e49038bed0f6189ea35c943fc1d39ab5e`
- PDF bytes are intentionally **not committed to git**. Git contains exact hashes, page/font/image census, source PUB hashes, and admission state.
- 024/025 have external dependency warnings and are not clean goldens.
- Successful exports remain pending font-warning classification because the exact warned IDs were not recorded in tranche A.

## Authority rule

Publisher PDF is visual authority only. It must not be used to infer hidden PUB semantics. Semantic claims require PUB/native evidence. Comparison tooling must materialize an external bundle and verify every PDF against the committed SHA before use.

A PDF page is also **not automatically the same semantic surface as a Reader logical page**. Reference pairs may represent a logical publication page, a facing/reader spread, an imposed production sheet, or an as-yet-unclassified output surface. The visual comparator therefore exposes `reference_surface_stage` with the bounded vocabulary `logical_page | viewport_spread | production_sheet | unknown`. Existing Batch 01 fingerprints do not contain independent stage authority, so missing values default to `unknown`; the comparator does not infer a stage from raster similarity, PDF page count or media size.

Stage promotion requires independent native/source/export provenance such as Publisher `Document.Pages.Count`, `ReaderSpread` membership/offsets, `Document.PrintStyle`, or an exact export-mode receipt. Until then, page-count agreement or disagreement is a visual diagnostic only and must not authorize Viewer PAGE membership.

Canonical tranche files:
- `tools/corpus/receipts/publisher-visual-golden-batch-01-tranche-a-2026-10-03.csv`
- `tools/corpus/receipts/publisher-visual-golden-batch-01-tranche-a-2026-10-03.json`

## Batch 01 / Tranche B

- IDs: 041-064
- Publisher runtime: 16.0 build 12527 (Publisher 2019)
- Exported PDFs: 20/24
- Publisher no-open: 4/24 — natural IDs 042, 052, 055 plus historical PAGEWIZ 056 SCHSPORT.PUB
- Historical 058 D7164.PUB successfully opens/exports on Publisher 2019
- External PDF bundle SHA-256: `4a6eb1e4a78724ba0d828b95291af892625a2d78740753113f79149ad35d2e54`
- First-page raster verification passed for all 20 PDFs; tranche includes Letter, A4, A3/custom sizes, multilingual text and image-rich layouts.
- Warning membership was not separately recorded in tranche B, so exported PDFs stay comparative-oracle candidates pending warning classification rather than clean-golden admission.

### Cumulative Batch 01 status through ID 064

- Processed: 64
- Exported Publisher PDFs: 43
- Publisher 2019 no-open: 21
- Natural corpus: 40 total / 37 exported
- Historical/container: 24 total / 6 exported


## Batch 01 / Tranche C

- IDs: 065-080
- Exported PDFs: 12/16
- Non-empty visual output: 10
- Empty exported visual output: 2 — IDs 072 and 076 each produce a one-page PDF with 0 text, 0 drawings and 0 images
- Publisher no-open: 4/16 — natural IDs 069 and 074; historical PAGEWIZ IDs 067 BIZSALE.PUB and 078 MEMBRDIR.PUB
- External PDF bundle SHA-256: `d82a183026b70948b0001a7f6edbdd2ba66c21279f4fdd90e62ca1ee50d1b38a`
- Tranche includes a 20-page document (068), a 27-page image-heavy document (075), large/custom page sizes, vector-only output, forms, labels and text-heavy layouts.
- First-page raster verification passed for all 12 PDFs; 072/076 are intentionally classified as degraded empty-output witnesses rather than clean goldens.

## Batch 01 complete status

- Selected PUB: **80**
- Publisher PDFs exported: **55**
- Non-empty visual outputs: **53**
- Empty exported outputs: **2**
- Publisher 2019 no-open: **25**
- Total reference PDF pages: **162**
- Natural corpus: **49/54 exported**, **5 no-open**
- Historical/container: **6/26 exported**, **20 no-open**
- External-dependency warnings explicitly recorded: **024, 025**
- No clean-golden admission is claimed yet because exact per-ID font/environment warning membership was not captured for the successful exports.
- Exact pair identity is durable in source-free CSV/JSON receipts; raw PDF bytes remain external oracle data and are validated by committed SHA-256 before comparison.

Canonical complete-batch receipt:
- `tools/corpus/receipts/publisher-visual-golden-batch-01-summary-2026-10-03.json`

## Normalized manual-oracle bundle

Batch 01 is normalized into the existing `tools/cloud_reader_manual_oracle_bundle_v1.py` layout:

- exact pair count: **55**
- bundle name: `publisher-visual-golden-batch-01-manual-oracle.zip`
- bundle SHA-256: `72949ba7cda9404f4d72cef5e18bc38d3f708fb14e9b310a6c4b912906bc2a76`
- bundle bytes: **91,039,995**
- committed executable pair registry: `tools/corpus/receipts/publisher-visual-golden-batch-01-pairs.csv`
- pair-registry SHA-256: `8f47ca1df8f8377193f07ab990fcdc5d10cc6410da3d56c0ddc1640a607985e5`
- committed original selection manifest: `tools/corpus/receipts/publisher-visual-golden-batch-01-selection.csv`

The binary bundle is intentionally not committed to normal git history. Consumers must verify bundle and pair SHA-256 before using it.


## Supplemental comparative corpus — 2026-10-06

A second manual-oracle bundle extends Batch 01 without changing its immutable receipts. It is intentionally separate because `tools/cloud_reader_manual_oracle_bundle_v1.py` admits at most 64 pairs per bundle.

- distinct PUB↔PDF pairs: **31**
- native Publisher PDF pages: **59**
- Publisher runtime: **16.0 build 12527 / Publisher 2019**
- cumulative corpus with Batch 01: **86 distinct pairs / 221 PDF pages**
- raw PUB/PDF bytes remain external; git retains exact pair identities and classification state

The 31 pairs comprise four provenance groups:

- **Batch 02 081–095:** 15 pairs / 24 pages.
- **Font-replacement set 096–100 + 102:** 6 distinct pairs / 8 pages.
- **User `01-main` corpus:** 3 pairs / 20 pages — Story stress, OfficeArt/WMF, and image-heavy A3.
- **Manual reduction family:** 7 pairs / 7 pages — `1` original → `2` aggregate residual → `2.1–2.5` isolated residuals. These seven pairs share one provenance lineage and must not be treated as seven independent semantic families.

Known font-substitution/degradation witnesses are:

- `batch02-086`
- `batch02-090`
- `batch02-091`
- `batch02-092`
- `batch02-093`
- `batch02-replacement-097`

They remain useful comparative/font-environment witnesses but are not clean visual goldens.

ID `101` is deliberately excluded from distinct-pair accounting because its source PUB SHA-256 is identical to ID `099`:
`00092479c94bf5ac1bb107173d60fb788b91e2c148453468456f98d0bb300597`.
The independent Publisher export is retained as repeatability evidence: its PDF bytes differ from 099, but a 144-dpi render comparison measured **2/2 unchanged pages and 0.0% changed pixels**.

Canonical source-free receipts:

- `tools/corpus/receipts/publisher-visual-golden-supplemental-2026-10-06-pairs.csv`
- `tools/corpus/receipts/publisher-visual-golden-supplemental-2026-10-06-summary.json`

### Supplemental normalized manual-oracle bundle

The externally held normalized bundle is directly compatible with `tools/cloud_reader_manual_oracle_bundle_v1.py`:

- bundle: `publisher-visual-golden-supplemental-2026-10-06-manual-oracle.zip`
- pairs: **31**
- reference pages: **59**
- bundle bytes: **23,407,040**
- bundle SHA-256: `1a4f1fb6682a7761fc74b93dfcdba9d1f622e871dcf292a9402c49bd5cc86da3`
- `PAIRS.csv` SHA-256: `6293b44b98d13c6216cb1a6bcedf5e2be10bdaa04c05b369ebabfdf91e6c560c`

The normalized bundle was locally validated against the same bounded rules enforced by the manual-oracle loader: 31/31 unique IDs and basenames, exact byte lengths and SHA-256 values, CFB magic for every PUB, exact PDF page counts, safe bundle paths, and all current pair/bundle size limits.
