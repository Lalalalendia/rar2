# PUB RE v0

`pub-re` is a standalone research tool outside the Chaptera product workspaces.

## v0

It compares two exact PUB/CFB inputs from a reproducible experiment manifest and emits a source-safe JSON receipt with whole-file hashes, CFB entry changes, changed stream hashes, and changed byte ranges without byte values or absolute input paths.

```json
{
  "schema": "chaptera.pub-re-experiment.v1",
  "experiment_id": "exact06-line-spacing-arm-a",
  "question": "which Publisher carrier changes?",
  "before": {"path": "baseline.pub", "expected_sha256": "<64 hex>"},
  "after": {"path": "mutated.pub", "expected_sha256": "<64 hex>"}
}
```

```bash
cargo run --manifest-path tools/pub-re/Cargo.toml -- analyze --manifest experiment.json --output receipt.json
```

Relative input paths resolve from the manifest directory. v0 deliberately does not infer Publisher semantics yet; it localizes stable byte/stream deltas for later COM/native-oracle joins.

Next layers: record/property joins, Publisher COM before/after/reopen receipts, visual first-divergence attribution, structural reduction, then bounded DynamoRIO/TTD only for unresolved carriers.


## v0.1 OfficeArt attribution

For an explicitly selected changed logical stream, the tool can join v0 byte ranges to canonical `pub-escher` `RawSpan` provenance:

```bash
cargo run --manifest-path tools/pub-re/Cargo.toml -- \
  attribute-officeart \
  --manifest experiment.json \
  --stream /Contents \
  --output officeart-receipt.json
```

The receipt preserves all true overlapping record/property candidates. It does not guess by nearest offset.

## v0.2 native Publisher transaction

`tools/pub-re/windows/Invoke-PubReNativeExperiment.ps1` adds the native semantic side of the loop on a dedicated Windows machine with Microsoft Publisher installed.

The first bounded operations are:

- `snapshot_only`: source-safe page/shape geometry inventory, no text and no save;
- `shape_rotation_delta`: select one exact `PageID + Shape.ID`, change only `Shape.Rotation`, save to a disposable output, close Publisher, reopen in a fresh process and snapshot the same exact selector when it remains stable.

Safety and provenance rules:

- the source SHA-256 is mandatory and verified before opening;
- the source is copied to a disposable run directory and never saved in place;
- Publisher must be idle before execution; the harness never kills an existing Publisher process;
- `Application.AutomationSecurity` must be successfully forced to disable automation content before opening the PUB;
- `Document.Close()` is parameterless, matching the verified Publisher 16 typelib;
- current Publisher SaveAs format is explicit numeric `1`;
- process exit is asserted with a bounded grace period (default 60 seconds);
- native receipts never contain source text, raw PUB bytes or local filesystem paths.

The optional `attribution.officeart_stream` field chains the native result directly into v0 CFB diff and v0.1 OfficeArt attribution.

### Dedicated runner

The `PUB RE native Publisher experiment` workflow requires a self-hosted runner labelled:

```
self-hosted, Windows, X64, chaptera-publisher-oracle
```

Configure `PUB_RE_MANIFEST_ROOT` on that runner for private/manual manifests. Workflow input remains only a simple manifest filename under that root; local source paths stay inside the private manifest and never enter GitHub inputs.

The owner-only control issue also supports repository-pinned public fixture aliases:

```text
/pub-re-native snapshot sample-newsletter
/pub-re-native rotate sample-newsletter 33554698 292 1
```

Aliases live in `tools/pub-re/native-fixtures.json`. For these modes the hosted gate passes only validated source-safe fields. The Windows job then reads the protected-main registry, downloads the exact HTTPS fixture into `RUNNER_TEMP`, verifies both pinned SHA-256 and byte length, generates an ephemeral native manifest, runs Publisher, and deletes the downloaded source with the normal `always()` cleanup. No runner-local manifest or persistent fixture is required.

The rotation command is deliberately bounded: `PageID` and `Shape.ID` must be positive signed-32-bit integers, and the delta must be nonzero with absolute value at most 45 degrees. The generated manifest uses `shape_rotation_delta`, so the normal save/reopen receipt and CFB differential join run automatically.

A trusted owner can cancel a queued or running PUB RE native workflow without touching the Windows machine:

```text
/pub-re-native cancel 37829026048
```

The cancel path runs on GitHub-hosted infrastructure with job-scoped `actions: write` permission. It accepts only a numeric run ID and verifies that the target is this repository's exact `.github/workflows/pub-re-native.yml` workflow before calling the GitHub Actions cancel endpoint. Cancel commands never schedule the Windows native job.

Normal native manifest/snapshot/rotation jobs have a five-minute wall-clock timeout; the bounded `research_0p75` mode keeps its existing 30-minute allowance. This watchdog is separate from the harness's 60-second Publisher process-exit grace because synchronous COM calls can block before an exit wait is reached.

The legacy owner command remains available for private runner-local manifests:

```text
/pub-re-native sample-newsletter-snapshot.json
```

The workflow uploads only JSON from the run's `evidence` directory and deletes the private copied/mutated PUB files in an `always()` cleanup step.
