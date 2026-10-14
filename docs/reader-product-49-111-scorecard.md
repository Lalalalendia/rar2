# READER-PRODUCT-49-111-SCORE-01: authenticated visual score contract

**Status: source-free validation foundation only. No Product corpus runner is implemented by this change.**

## Why this is a separate score

The existing `reader-publisher-lo-49-111-rerank.yml` runs `guest-reader-scene`
and its direct isolated worker path omits the host-admitted image derivatives.
That reference score (LibreOffice 85 / Reader 17 / near 9) cannot describe
the real OIDC-authenticated Product. The fixed frozen Publisher PDFs remain
the visual oracle; LibreOffice 24.2.7.2 remains a competitor, not an oracle.

## Authoritative inputs

- `tools/corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json`: 49 public
  source SHA pairs, 39 comparable documents, 111 ordinal pages and ten
  historical predeclared exclusions.
- `tools/corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json`
  and `publisher-visual-golden-batch-01-pairs.csv`: SHA-pinned Publisher PDF
  identities, grid hashes and media extents.
- A **new, separately executed physical source-neutral Product receipt**
  (`chaptera.product-49-111-physical-pages.v1`). It must be produced by
  real OIDC/HTTPS SourceIngress -> durable document revision -> isolated
  Product worker -> independent host-admission -> actual Chromium render.
  No DIRECT/Guest fallback. Every page must carry a captured PNG SHA,
  Publisher reference grid SHA, source/revision bound identity and
  BOX64/RGB12 changed-cell count; original pixels stay private.

## Fail-closed scoring

`python tools/reader_product_49_111_scoreboard_v1.py --physical-product
source-neutral-physical.json --frozen-lo
tools/corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json
--publisher-fingerprint
tools/corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json
--publisher-pairs tools/corpus/receipts/publisher-visual-golden-batch-01-pairs.csv
--expected-head <full-40-digit-product-checkout-sha> --out ranking.json`

Only a complete 49 document / 39 eligible / 111 page physical receipt
with all required Product auth/host/privacy checks produces any winner
statistics. Missing or rejected Product pages are failures, not exclusions
or ties. Predeclared ten-ineligible rows must be explicit, untouched and
source-safe. Matching the source SHA alone does not establish native
Publisher PAGE membership or full 144-DPI pixel parity: the historical
Publisher reference surface stage is still unknown.

## Test & next owner boundary

`python tools/test_reader_product_49_111_scoreboard_v1.py` runs 19
synthetic correctness/negative checks. Their PASS does **not** count
as physical Product visual evidence. The next slice is a bounded Product
OIDC corpus producer that can report every fixed fixture, never silently
remove a rejection, and aggregate image-on Chromium captures in isolated
CI with the existing security leak scanner unchanged.

Neither this source-free scorer nor an isolated 001/022/023 page win is
release authority, physical S3/IAM proof, or a global Product-vs-LibreOffice
parity claim. See Notion `READER-PRODUCT-49-111-SCORE-01` and open P0 #2627.
