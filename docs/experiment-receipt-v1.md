# Chaptera source-free experiment receipt v1

`ExperimentReceiptV1` is a provenance envelope for one experiment run. It binds
the task, run, producer, system, build, environment, source/output identities,
operation, semantic observation phases, lifecycle result and authority ceiling.
It does not replace or reinterpret specialized evidence such as
`StateTransitionReceiptV1`.

The JSON Schema is
`packages/protocol/experiment-receipt/v1/receipt.schema.json`. Checked-in
fixtures cover a complete envelope, an explicit partial/not-comparable
envelope, and negative digest, version, environment and privacy cases.

## Deterministic identity

Canonical JSON is UTF-8 JSON with keys sorted recursively, no insignificant
whitespace, Unicode preserved, and non-finite numbers rejected. The receipt
hash and each referenced evidence hash are SHA-256 of those canonical bytes.
This makes object insertion order irrelevant while keeping arrays ordered.

Each evidence reference names a registered specialized receipt type and its
exact version. Validation loads the referenced JSON, runs that specialized
schema, checks its declared version, and then checks its canonical digest. An
envelope cannot upgrade, weaken or silently reinterpret the referenced schema.

## Explicit incomplete evidence

Before, after and reopen observations use one of `known`, `unknown`, or
`unavailable`. Unknown and unavailable values require a reason. A completed
run must have all three phases known; any other combination validates only as
`not_comparable` and uses a partial, failed or aborted lifecycle disposition.

`authority_ceiling` is an upper bound, not a verdict. `non_claims` is mandatory.
Synthetic fixtures use `synthetic_contract` and cannot authorize a Publisher,
fidelity or product decision.

## Public privacy boundary

The validator rejects absolute Windows, UNC, POSIX and home-relative paths;
user/host/home identifiers; secret/token fields; embedded source/binary payload
fields; and base64 data URIs. Public envelopes contain identities, normalized
observations and bounded summaries, never PUB bytes or customer content.

## Validate

```bash
python tools/validate_experiment_receipt.py \
  packages/protocol/experiment-receipt/v1/fixtures/complete.synthetic.json \
  --evidence state-transition.synthetic.json=packages/protocol/experiment-receipt/v1/fixtures/state-transition.synthetic.json
```

Run the positive, negative and regression suite with:

```bash
python -m unittest tools/test_experiment_receipt.py -v
```
