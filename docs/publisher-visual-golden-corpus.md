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
