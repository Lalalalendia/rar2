# VIEWER-PUBLISHER-DGG-TEXTBOX-APPLICABILITY-01 native runbook

This experiment is research-only. It does not authorize a Reader semantic change by itself.

## Authority

The exact source must be the Virginia Remplaçante PUB with SHA-256:

`88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506`

The operation accepts either:

- `PUB_RESEARCH_FIXTURE` pointing directly to that PUB; or
- `PUB_RESEARCH_FIXTURE_ROOT` pointing to the local research corpus root.

When only the pinned archive is present, the operation accepts exactly:

- `french_teacher_pub_pdf_corpus_2026-09-20.zip`
- SHA-256 `306fa66cf3238ceab0d91e7de79bb95d138331bdcbcf6c22aaa1b9b5510976f1`

It extracts the archive only below the private experiment output and rechecks the PUB SHA-256.

## Causal law

The two arms are identical except for one structurally located scalar:

- OfficeArt DGG-primary `fillColor` / property `0x0181`;
- only its 4-byte `op` is changed;
- the containing `/Escher/EscherStm` is replaced through the preservation-first bounded CFB stream primitive.

Both arms then:

1. select the same source-profiled sparse TextBox on Publisher page 24 or 25;
2. tag it;
3. delete surrounding shapes on that page;
4. SaveAs;
5. close Publisher;
6. fresh reopen;
7. record Publisher COM effective Fill state;
8. re-profile the saved PUB and require the sparse local state plus the DGG delta to survive.

A semantic verdict is emitted only if all causal guardrails survive.

## Direct local execution

From a trusted `main` checkout after this tooling lands:

```powershell
$Packet = "tools/research-runner/experiments/viewer-publisher-dgg-textbox-applicability-01.packet.json"
$Out = Join-Path $PWD "out\pub-research\dgg-textbox-applicability-01"

# Use one of these:
$env:PUB_RESEARCH_FIXTURE = "<exact-local-path>\virginia-remplacante-modifiable.pub"
# or:
$env:PUB_RESEARCH_FIXTURE_ROOT = "<local-research-corpus-root>"

python tools/research-runner/validate_packet.py --packet $Packet --expected-environment publisher-2019
pwsh -NoProfile -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $Out
pwsh -NoProfile -File tools/research-runner/operations/publisher_dgg_textbox_applicability_01.ps1 -PacketPath $Packet -OutputRoot $Out
pwsh -NoProfile -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $Out
```

## Verdicts

- `sparse-textbox-fill-follows-dgg`: the Publisher effective fill color changes with the isolated DGG-only mutation. The hosted #704 semantic treatment is rejected.
- `publisher-specific-dgg-fillcolor-nonapplicability`: the DGG mutation survives Save/reopen, the same sparse TextBox state survives, Fill visibility/type stay controlled, but Publisher effective fill color does not follow DGG. This is the required native authority for the bounded treatment class.
- `inconclusive`: any target identity, sparse-state, DGG-persistence, or control guard fails. Do not merge product semantics.

PUB/PDF source bytes, shape IDs, raw DGG scalar values, and local filesystem paths remain under the experiment's `private/` directory and are excluded from the upload-safe evidence bundle.
