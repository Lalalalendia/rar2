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
      "recovery_route": null,
      "recovery_class": null,
      "producer_receipt_sha256": null,
      "product_validation_sha256": null,
      "artifact_count": null,
      "fabricated_bytes": 0,
      "silent_drops": 0,
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


## Current natural Rescue seed

The committed source-free evidence file currently binds one real natural damaged witness:

- source SHA `6eb0a85afb42d329e2241ce060ec3b6957ecd429a5122cfc908b4621ab4884b8`;
- route/class `partially_recovered / partial_salvage`;
- producer receipt `1bbdaf7fd5fa47cbadc50b152d72d7227fca716468fc1de53bf003f02a5ceed8`;
- product validation `12d558a4fe1a699420e7b4a09e4616d0b045aaf36c95df575792593d1046bc02`;
- 12 verified media artifacts;
- fabricated bytes = 0;
- silent drops = 0;
- native validation = inconclusive.

This row may produce compact `rescue` when those exact source bytes are present in an input corpus. It does not change Reader admission.


## Additional exact natural Recovery witnesses

The committed Rescue evidence registry now also binds:

- Saudi `Publication1.pub` / `b18c713e…` — complete Contents + partial EscherDelay, missing Quill/Escher → research `salvage_only`.
- Thai `letter_011937.pub` / `74cfefff…` — partial Contents + partial Delay + physical OOB → research `salvage_only`.
- FAPinsideEdited / `dd44367a…` — complete Contents + preview, missing Quill/Escher, partial Delay → research `salvage_only`.
- Spring08Flyer / `a2d05984…` — complete Contents + preview, Quill unavailable → research `salvage_only`.
- THEAFlyer / `b4381b1e…` — complete Contents + preview, Quill unavailable → research `salvage_only`.
- `zbirka_ato.pub` / `2173c7dd…` — partial Delay but no verified useful payload in the tested prefix → `diagnostic_only`.

Research-level salvage rows retain `promotion_gap=research_bundle_not_product_validated`. They can establish broader Rescue capability for exact bytes but never change current Reader admission.
