# MIGRATION-OPERATOR-PDF-REPORT-01 — private batch triage, no migration certificate

**Scope:** A read-only, local operator layer for an *already completed or checkpointed* `chaptera.rescue-batch-pdf.v1` folder/ZIP conversion. It does not open, mutate, parse or copy original PUB files; it does not convert to a new target, approve losses or produce a customer-delivery bundle. The separate Migration product remains the owner of future archive workflow, per-file route authorization and final delivery.

## User route: private folder/ZIP → PDF batch → exceptions

Run the existing admitted batch converter (requires compiled Chaptera `pub` and a licensed fallback font):

```sh
python3 tools/rescue_batch_pdf_v1.py /private/source-pubs \
  --output /private/pdf-batch \
  --fallback-font /private/fonts/explicit-fallback.ttf \
  --pub-cli /path/to/pub
```

Use a ZIP in place of `/private/source-pubs` if desired. Then produce the **local operator report** in a separate directory:

```sh
python3 tools/migration_operator_pdf_report_v1.py /private/pdf-batch \
  --report-dir /private/migration-review
```

Outputs are `operator-report.json`, `operator-report.csv` and `exceptions.csv`. Keep them private: relative filenames and original hashes may be customer-sensitive. They must not be uploaded to public GitHub artifacts or inserted into public documentation.

## Report contract

- `integrity_confirmed_review_required` means the existing PDF + both canonical loss sidecars have the exact SHA-256 values from the current batch receipt, the PDF starts with `%PDF-`, and the loss JSON parses as an object. **It does not prove visual fidelity, PDF/A, output page geometry, consumer acceptance or editability.** An operator must still inspect the loss and PDF before customer delivery.
- `integrity_rejected` means a recorded PDF or loss sidecar is missing, non-regular, symlinked, altered or unreadable. It is an **exception**, even if some old PDF remains on disk.
- `exception` preserves an existing failed-file reason from the converter; one failed PUB does not change a different file's state.
- `not_yet_in_checkpoint` counts declared original sources absent from a partial crash/restart manifest. They are **never** treated as converted or accepted.
- `exceptions.csv` contains only rejected and failed *known rows*; the pending aggregate count is kept in JSON. CSV escapes formula-leading cells.

This is a **source-free integrity projection of a producer receipt**, not a new independent PDF renderer, a target-selection rule or a certifying MigrationDeliveryBundleV1. The output rows are reproducible and sorted by source-relative identity. There is no automatic DOCX/IDML/ODG execution; those routes retain their own admission, consent and consumer validations.

## Security and lifecycle

- Bounded receipt (16 MiB) and up to 2,000 PUB identities; output hashing streams rather than loading PDFs into RAM; loss sidecars bounded at 16 MiB.
- Rejects symlink/non-regular inputs and outputs, suspicious manifest relative paths, duplicate case-folded source identities, unsafe target names, unknown statuses and a batch whose `.rescue-batch.lock` is still present.
- Reports must be outside the producer batch tree; source PDFs, fonts and PUBs are never copied into reports.
- Each report file is atomically replaced; repeated runs on immutable receipts produce byte-identical JSON/CSV.
- CLI exit: `0` integrity checks complete (visual review still required); `1` files failed/rejected or checkpoint incomplete; `2` unsafe or malformed input/report destination.

## Bounded verification

```sh
python3 -m compileall -q tools/migration_operator_pdf_report_v1.py tools/test_migration_operator_pdf_report_v1.py
python3 -m unittest discover -s tools -p 'test_migration_operator_pdf_report_v1.py' -v
```

These tests use synthetic PDF and loss sidecars and cannot prove Publisher-fidelity, real PDF/A conformance or customer acceptance. The existing separate real-Rust-PUB-CLI PDF batch acceptance remains authoritative for actual conversion.

**Next, separately owned:** complete the Migration pilot's representative-file customer acceptance; route normal files to existing PDF/IDML/ODG only after certified gates; present source-safe exceptions and controlled Editor/Rescue handoffs via their owning tasks. Full GUI wizard is parked under `MIGRATION-UX-01` and must not be silently reactivated here.