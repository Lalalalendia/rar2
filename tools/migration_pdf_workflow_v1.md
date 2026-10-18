# MIGRATION-PDF-WORKFLOW-01 — one local folder/ZIP to PDF + exception report

**Product owner:** Chaptera Migration/Archive. **Implementation:** a one-command local adapter, NOT a second converter, Editor, Rescue parser, migration assurance engine, delivery bundle, or GUI.

This workflow composes two existing merged authorities:

- `tools/rescue_batch_pdf_v1.py` — bounded input discovery, existing `pub convert --to pdf`, durable per-file checkpoint, source/file SHA, PDF and loss sidecars;
- `tools/migration_operator_pdf_report_v1.py` — read-only SHA verification of all three existing outputs and private operator JSON/CSV exception reports.

The new entrypoint **only coordinates the two**, and does not claim PDF/A, PDF/X, Publisher visual fidelity, editable structure or any form of customer certification.

## Usage

Optional offline count-only discovery (does not decode or certify a document):

```sh
python3 tools/migration_pdf_workflow_v1.py inventory /private/source-pubs
```

The input may be either a folder (recursive `.pub` lookup) or a ZIP archive. Run both existing tools with a single local command:

```sh
python3 tools/migration_pdf_workflow_v1.py run /private/source-pubs \
  --pdf-dir /private/migration-pdfs \
  --report-dir /private/migration-reports \
  --fallback-font /private/fonts/licensed-fallback.ttf \
  --pub-cli /path/to/current/chaptera/pub
```

Two **different** output directories must be outside the source tree. A symlink at any selected root is rejected. The reporter does not read or copy original PUB files. All source-relative paths, hashes and output artifacts are private customer data and must never be uploaded to a public GitHub Actions artifact.

Successful PDF conversion produces `<name>--<pathhash>--<sourcehash>.pdf` and its `.loss.json`/`.loss.txt` siblings via the **unchanged** batch converter. The report directory contains:

- `operator-report.json`: original exact manifest hash, total source count, checkpointed rows and typed outcomes;
- `operator-report.csv`: per-file integrity/review status;
- `exceptions.csv`: only per-file failures and integrity rejects. These are local operator outputs, not a certified delivery bundle.

`integrity_confirmed_review_required` means the PDF and both loss sidecars have the recorded SHA-256 bytes and basic structure, **not** that layout, colours, fonts, images, semantics or external PDF consumers have been validated. The operator must independently review the visual and loss report before delivery. Unsupported, unreadable, malformed and failed files must be triaged separately through the owning Migration and Rescue paths.

## Return codes and restart

- **0** — all discovered documents have intact PDF/loss outputs. Visual/loss review remains required;
- **1** — one or more per-file conversion failures, integrity rejects or incomplete batch checkpoint. Failed files do not hide the completed ones;
- **2** — unsafe/inconsistent intake, unreadable archive, missing compiler-provided `pub` binary or font, collision/lock, invalid output tree, unsupported report manifest or another preflight/read failure. Never claim acceptance after these errors.

Restart the same `run` command to reuse verified, unchanged PDF/sidecar output and reconvert only invalid/tampered outputs under the existing durable batch receipt. The canonical batch manifest is the authority for restart, not the presence of files in the output folder. Results are private local data. A process crash before report generation can leave older report files: re-run the route to refresh and confirm new manifest SHA before relying on them.

## Safety and limitations

- Existing batch limits remain: at most 2,000 PUB, at most 128 MiB per source, at most 1 GiB ZIP-expanded input, 1–3,600-second per-file timeout, collision-safe output filenames, source immutability/re-hash, one output writer lock, fail-closed ZIP paths and symlinks.
- This is local/offline operator tooling, not a public upload or multi-tenant service. The existing `pub` converter subprocess is **not** claimed to have the separate per-file resource isolation gate defined by `MIGRATION-PDF-SEC-01` (currently parked). Process unknown/untrusted PUB on an isolated, authorized workstation rather than exposing this to a network endpoint.
- This command does not overwrite existing source PUBs, automatically approve known loss, route mixed batches into IDML/ODG/DOCX, reconstruct native `.pub`, or fulfill the parked `MIGRATION-UX-01` desktop dashboard or upstream-blocked MigrationDeliveryBundleV1.
- Exact reproducibility and PDF visual fidelity have separate owners. No new source parsing, image extraction or PDF renderer is introduced.

## Acceptance

```sh
python3 -m compileall -q tools/migration_pdf_workflow_v1.py tools/test_migration_pdf_workflow_v1.py
python3 -m unittest discover -s tools -p 'test_migration_pdf_workflow_v1.py' -v
```

The new tests include source-free cases for inventory (folder, ZIP, malicious member and symlink), output-tree separation, timeout, owner composition and fail-closed accounting, plus real Python subprocess end-to-end against the existing **unmodified** batch module and operator report, using a fake `pub` executable. No private customer PUB or real rendered PDF is used in these tests. The previous real Rust `pub` converter acceptance under `RESCUE-BATCH-PDF-01` remains the materialized PDF-authority proof, not this fixture.

## Following routes (not part of this change)

1. Managed 10–25 file pilot: per-file compatibility, scope/quote, certified output and operator/customer acceptance (owner `PUB-MIGRATION-PILOT-01`).
2. Mixed-format batch target planning: current `MIGRATION-BATCH-ASSURANCE-01`, waiting on its dependencies; do not auto-route without certified target and loss consent.
3. Full migration dashboard: existing parked `MIGRATION-UX-01`; readmission requires lifting its current focus lock.
4. Damaged/incomplete files: separate Rescue worker and `Partial / Needs review`, not a green migration certificate.
