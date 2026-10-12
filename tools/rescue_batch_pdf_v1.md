# RESCUE-BATCH-PDF-01 — resumable batch PUB → PDF (bounded V1)

**Authority:** [Chaptera task](https://app.notion.com/p/3e132a84beec81e7a5ecc87f1b8a2353), [rar2 #640](https://github.com/Lalalalendia/rar2/issues/640). This is a thin, offline **orchestration** layer over the repository's **existing** compiled `pub convert --to pdf` command. It does not parse PUB, render PDF, change font choices, or repair source documents.

## Run

First build/install the existing `pub` executable with the current Chaptera producer-a toolchain. Then:

```sh
python3 tools/rescue_batch_pdf_v1.py /private/pubs \
  --output /private/exported-pdfs \
  --fallback-font /private/licensed-fallback.ttf \
  --pub-cli /path/to/compiled/pub
```

The first argument may instead be a **ZIP** archive containing `.pub` files. Directory input is recursive. Input paths are never overwritten; nested input/output directory trees are rejected. Output is always private-local and must **not** be uploaded to CI/artifact storage if it contains user documents.

The deterministic output name contains a sanitized source basename, a SHA-256 prefix of the relative source path, and a SHA-256 prefix of exact source bytes. ZIP members are parsed without unsafely extracting arbitrary archive paths: each selected `.pub` member is briefly materialized inside a private temporary directory for the existing `pub` converter, then removed.

Each success produces:
- `name--pathhash--sourcehash.pdf`;
- `name--pathhash--sourcehash.pdf.loss.json`;
- `name--pathhash--sourcehash.pdf.loss.txt`.

The output directory also contains **`manifest.json`** (`chaptera.rescue-batch-pdf.v1`): each discovered source, exact source/fallback-font hash, stable output name, output+two-loss-sidecar hashes, and one of `converted`, `reused`, or `failed`. Converter stdout/stderr are deliberately suppressed to prevent accidental source-text leakage into shared logs. The terminal summary prints **counts only**. `--json` explicitly prints source-relative paths and hashes; treat this JSON as private customer metadata.

## Resume and failures

The manifest is replaced atomically **after every completed source**, including failures. A rerun reuses an output only if the previous receipt says successful, its source bytes and fallback font identity match, and **all three** output/sidecar files still match their committed SHA-256 digests. Otherwise only that source is reconverted. Corrupt or incompatible prior manifests fail closed rather than pretending conversion completed.

An unsuccessful PUB does **not** abort later documents. Exit codes: `0` all files converted/reused; `1` one or more per-file failures (recorded in manifest); `2` invalid/insecure input, missing converter or output-lock problem. Failures do not expand the supported font/format semantics of `pub convert --to pdf`. An existing PDF/sidecar may remain after a failed refresh, but its status is `failed`, it is **not reusable**, and the manifest is the authority—not directory contents alone.

Only one worker may mutate the output tree: `.rescue-batch.lock` is exclusively created. A stale lock after an abnormal process termination requires verifying no batch process remains and **manually** removing that lock before resuming. All successful output names and manifests remain stable across process restarts; per-run statuses correctly change from `converted` to `reused`.

## Safety/limits

- Input: ordinary directory or ZIP only; ZIP path traversal, duplicate names (case-folded), symlinks/special entries, directory symlinks and nested source/output trees are rejected.
- Maximum 2,000 PUBs, 128 MiB per PUB, 1 GiB aggregate ZIP-expanded entries; no arbitrary ZIP file extraction.
- Per-file converter timeout 300 seconds, configurable via `--timeout 1..3600`.
- Uses `subprocess` argument vector without a shell; invokes only the existing compiled `pub` converter and does not attempt recovery or Publisher COM execution.
- No untrusted PUB parsing occurs in this script, but the **existing CLI** itself must be isolated and security-reviewed before exposing this entrypoint as a network-facing service.
- Reuse attests **byte identity and converter output existence**; it does not prove Publisher PDF visual parity, correct fallback font licensing, or correctness of the underlying renderer.

## Offline synthetic regression

```sh
python3 -m compileall -q tools/rescue_batch_pdf_v1.py tools/test_rescue_batch_pdf_v1.py
python3 -m unittest discover -s tools -p test_rescue_batch_pdf_v1.py -v
```

The 16 synthetic test scenarios use a self-contained fake `pub convert` executable and source-free fixtures. They prove routing, ZIP safety, resume decisions, hashing, manifests, and failure isolation. **They are not a real-PUB fixed-output end-to-end test.** A separate exact-public-fixture acceptance against the actual compiled `pub` and its native fallback-font restrictions is required before declaring the entire Rescue feature done.
