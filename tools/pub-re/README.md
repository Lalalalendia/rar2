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
