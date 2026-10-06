# RECOVERY-DAMAGE-CLASSIFIER-01

Source-safe classifier for Chaptera's own damaged-PUB capability.

It deliberately separates:

- current Reader outcome;
- useful salvage facts/gaps;
- broader Rescue evidence;
- missing promotion authority.

It does **not** invoke Microsoft Publisher.

## Build

```bash
cargo build --manifest-path vendor/producer-a/Cargo.toml -p pub-viewer --release --bin recovery-damage-classifier
```

## Run

```bash
recovery-damage-classifier CORPUS_DIR OUTPUT.json [RESCUE_EVIDENCE.json]
```

The output contains no filenames, source paths, recovered text or image bytes. Exact SHA-256 is the source identity.

## Compact verdicts

- `open` — ordinary Reader succeeds.
- `open_partial` — production Salvage opens the source.
- `rescue` — current Reader cannot safely display the file, but source-safe recovery primitives or an exact Rescue receipt prove useful surviving content / a bounded repair candidate.
- `no_safe_recovery` — no current safe useful-content route is proven.

## Promotion-gap rule

For current `awaiting_typed_corruption_evidence` files, the classifier may run the already-existing forced structural-corruption probe **only as a diagnostic**.

It returns `rescue` only when the forced partial graph contains at least one source-safe useful fact:

- semantic text range;
- verified image;
- grounded geometry.

A graph containing only gaps is not Rescue.

Such a row keeps:

```json
{
  "reader_outcome": "cannot_safely_display",
  "compact_verdict": "rescue",
  "evidence_level": "inferred",
  "promotion_gap": "missing_production_typed_corruption_authority"
}
```

This does not relax Reader admission.

## Optional exact Rescue evidence

The optional third argument binds retained recovery/repair evidence by exact source SHA without hardcoding research-only repair law into `pub-viewer`.

Schema:

```json
{
  "schema": "chaptera.recovery-rescue-evidence.v1",
  "rows": [
    {
      "source_sha256": "<64 hex>",
      "rescue_outcome": "bounded_repair_candidate",
      "repair_gate": "quill_donor",
      "repair_native_status": "pass",
      "evidence_level": "controlled_native",
      "confidence": "high",
      "promotion_gap": null
    }
  ]
}
```

Allowed `rescue_outcome`:
- `none_needed`
- `bounded_repair_candidate`
- `salvage_only`
- `diagnostic_only`
- `unsupported`

Allowed `evidence_level`:
- `production`
- `exact_sha_authority`
- `controlled_native`
- `recovery_research`
- `inferred`

This is the intended bridge for retained CR03/CR04 and future exact Rescue receipts. Rescue evidence may upgrade the **Rescue** classification; it never changes an ordinary Reader outcome.

## Exact-1050 acceptance

The existing `Reader 1050 corpus baseline` workflow builds and runs the classifier.

Hard gate:
- 1041 `normal_open`;
- 5 `salvage_open`;
- 4 `cannot_safely_display`.

For the four current Reader rejects, the workflow reports how many become theoretical `rescue` because current recovery primitives already expose useful facts but typed production admission is missing.

That count is diagnostic, not automatically a new product claim.
