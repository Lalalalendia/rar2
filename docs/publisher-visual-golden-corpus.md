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

## Regular CI integration

The ordinary hosted visual suite now has a public-safe Batch 01 lane:

- workflow: `.github/workflows/publisher-visual-golden-batch01.yml`
- exact source PUB bytes come from the already-authoritative 1,050 materialization (with the same reconstruction fallback as the corpus baseline);
- the committed reference is **source-free**: perceptual hashes plus normalized color/edge histograms for 55 documents / 162 Publisher-PDF pages;
- raw Publisher PDF bytes and document text are not committed to git;
- the full private 55-pair PDF bundle remains the high-resolution diagnostic oracle when a perceptual regression needs localization;
- the workflow runs on visual-affecting `main` changes and from the overnight product slot;
- PRs retain the faster Carlton/Virginia exact-pixel gate instead of paying for 55-document replay on every edit.

Reference receipt:
`tools/corpus/receipts/publisher-visual-golden-batch-01-perceptual-v1.json`

Comparator:
`tools/publisher_visual_perceptual_v1.py`

The first current-main run is measurement/baseline establishment. Once its receipt is pinned, subsequent runs may fail only on bounded regressions relative to that baseline; PDF-derived state remains visual authority only and never PUB semantic authority.
