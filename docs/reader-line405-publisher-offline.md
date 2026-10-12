# LINE405: offline Microsoft Publisher 2019 visual verdict

This procedure completes the visual-reference measurement after the real
`SampleNewsletter.pub` Reader/Chromium A/B. **Microsoft Publisher is the
visual authority. LibreOffice is not used.** This is a local-only comparison:
no S3, running Publisher, self-hosted Windows runner, or Publisher PDF upload
to GitHub is required.

## Immutable inputs

1. From [Actions run 38260303184](https://github.com/Lalalalendia/rar2/actions/runs/38260303184),
   download the original **ZIP artifact** `reader-line405-browser-before`
   (artifact ID `11752465627`, SHA-256
   `fd21f8041019ea8c5d52b7ba86c9c4b3d3b7e48fbfc28e28d297150fe2226943`).
   This is exact Reader commit
   `47eb72faac780692335eef7f00ac0dbd9daaf206`.
2. From that same run, download the original **ZIP artifact**
   `reader-line405-browser-after` (artifact ID `11751779333`, SHA-256
   `91fe08f33e22a776422c4c04f7f6e5580e9b6743f280ec044eede2317f5e4d31`).
   This is exact Reader commit
   `846c735f2f7eb2ea92451236bdb146cb0eb481d8`.
3. Supply the **external local** Microsoft Publisher 2019 oracle ZIP
   (SHA-256 `b4465c296282bcaba0d3b36e865fa302b99b0c6135bbaa9846f969e8aa8388ed`).
   It contains exactly ten Publisher-exported **PDFs / twenty pages**, with
   four pages in `SampleNewsletter.pdf`. It **does not** contain any original
   PUB, PAIRS.csv, or cryptographic PUB-to-PDF pairing manifest.

The browser artifacts include four SHA-checked 144-DPI customer-page PNGs
and one receipt each. Their exact ZIP digests are required; do not unpack
and repack them. GitHub Actions artifacts have finite retention (the capture
workflow requests seven days).

## Run without sending private PDF bytes anywhere

Python 3.12+ with `PyMuPDF==1.26.7` is required, alongside this repository's
`tools` directory. Use private paths for the three downloaded ZIP inputs.

```bash
python -m pip install "PyMuPDF==1.26.7"
python tools/reader_line405_publisher_offline_v1.py \
  --baseline-browser-zip /private/reader-line405-browser-before.zip \
  --candidate-browser-zip /private/reader-line405-browser-after.zip \
  --publisher-pdf-zip /private/publisher2019-10pdf-oracle.zip \
  --output /private/line405-native-numeric-verdict.json
```

The runner rejects wrong input ZIP digests, missing or malicious artifact
members, changed PNG/receipt identities, wrong PUB source hash, incorrect
PAGE381-to-customer-page-four join, altered page counts/extents, changes on
customer pages 1–3, and a candidate with anything other than **one** added
source LINE405 node. It leaves an existing output intact on failure.

A successful numerical JSON reports physical Publisher residuals for all
four pages and classifies the *change* on page four as closer, farther,
or equal to Publisher. It does **not** assert exact Publisher pixel parity.
Only the explicitly SHA-checked Publisher-exported pixels are used as
visual reference. The PDF archive's source-PUB correspondence is a prior
local 2026-10-01 attestation, not a self-contained cryptographic proof.
The exact Publisher build/export stage is not independently established.

The tool never uploads files and never copies the PDF, PNG, or PUB into
the output JSON. **Keep all three ZIPs outside the repository.**

## Verified prerequisite

The [physical Chromium A/B run](https://github.com/Lalalalendia/rar2/actions/runs/38260303184)
passed on the original SHA-pinned `SampleNewsletter.pub`: source PAGE381
identity matched customer page 4; pages 1–3 had **0** significant changes;
page 4 had **16,400** significant changed pixels within the bounded source
line envelope. That is a Reader source-visibility fact, **not a native
Publisher residual**.

The hosted contract CI for this local runner uses only synthetic ZIPs and
negative controls. It does not have access to the external Publisher PDF
archive and cannot claim a real native verdict.
