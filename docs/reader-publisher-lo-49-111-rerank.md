# Publisher / Reader / LibreOffice — frozen 49/111 ordinal visual rerank

## Authority and purpose

This is a **measurement-only** job: Publisher 2019 PDFs are the visual oracle;
LibreOffice 24.2.7.2 is only a frozen competing renderer and debugging reference.
The job reuses the existing `publisher-visual-golden-batch01.yml` real Chromium
Reader pipeline, **not** a second parser or renderer. No original PUB, PDF,
ODG, PNG or Story text is published by the scoreboard. It does **not** use S3.

The original science run [SK1 PUB-T-999](https://github.com/calex794/sk1/actions/runs/38145210671)
measured 49 SHA-verified public PUBs and fixed 39 documents / 111 comparable
ordinal pages; Reader 17 / LO 85 / near 9 and document scores Reader 8 /
LO 26 / near 5. This baseline predates SafePreview `#2629`.

`tools/corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json`
contains *only source-free benchmark counters and SHA identities*, derived
from artifact ID `11704247616` (ZIP sha256 `228d9833e65d491a865754a132e358b317dda64c2432c21ac1d7cca2a4fc261c`,
`report.json` sha256 `2f3f1bcddaa6aa094b46893825593d9aef4789b6960620dfdaf8d01a6af5b5fd`).
The comparator checks its own fixed SHA; 49 source PUB SHA-256 and 49 Publisher
PDF SHA-256 identities are independently bound to the exact 55-pair reference
registry and signed fingerprint Git blobs. The manifest is readable, source-free JSON,
not original media or a physical font.

## Gates and denominators

- Preserve the same **49 PUB**, **39 eligible documents**, **111 eligible pages**.
  Ten historically ineligible documents are listed, never reselected after
  seeing visual results; six missing public source donors are excluded upstream.
- Preserve source and PDF SHA, original Publisher fingerprint Git blob
  `1e87b52d268f87dcd4896f3d1af351a0b8cb9d96`, pair registry blob
  `2c2b03652e79390befe55f2889dbc248eb2abe01`, and reader 55/162 coverage.
- Require exact checkout SHA from actual Reader receipt, not the PR title,
  and the 64x64 RGB Pillow.BOX / 144 DPI / channel-delta 12 algorithm.
- Require the same 39-page-count admission and extent tolerance ±0.05pt;
  any materially drifted formerly eligible page **fails** instead of
  disappearing from the denominator.
- Recompute old 85/17/9 and 26/8/5 from the science counters as a self-check;
  new winner threshold remains ±0.015 changed-cell fraction.
- Show both page and document winners, mean changed-cell residuals, 10 largest
  current LO advantages, 10 largest Reader improvements, and 10 fixed exclusions.

## Exact SHA and workflow semantics

The PR job executes under `github.sha`, which is GitHub's tested **PR merge
snapshot**, not necessarily `head.sha`. A successful PR receipt must **not** be
called a current-main score. After merge, the `push: main` trigger reruns the
same Reader 55/162 pipeline at the actual new main SHA. The source-neutral
summary carries and checks that exact commit SHA. Main advancement after that
run makes the report historical again until the next current-main rerun.

This is a coarse ordinal PAGE comparison only; all Publisher reference surface
stages remain unknown. It is **not 144-DPI pixel-exact parity** and the same
font environment is not independently established. Updating a win count
never authorizes PUB Writer/Save or production readiness.
