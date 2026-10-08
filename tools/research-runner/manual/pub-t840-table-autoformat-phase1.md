# PUB-T-840 TABLE AutoFormat Phase 1 native runbook

Research-only bootstrap for the native TABLE fidelity authority. It does not by itself decide durable-style vs one-shot materialization.

## Purpose

Create one plain 4x4 Publisher table, save/reopen it, then run two fresh-copy arms:

- A / no-op: SaveAs -> close -> fresh reopen.
- B / all-enabled: `ApplyAutoFormat(0, True, True, True, True)` where Publisher documents value `0` as Checkbook Register, then SaveAs -> close -> fresh reopen.

The operation records sanitized COM-effective cell fill, border, text and alignment signatures after fresh reopen. Generated PUB files and detailed private receipts remain under `private/`.

A valid Phase-1 result requires:
1. same generated baseline for both arms;
2. 4x4 table survives both fresh reopens;
3. treatment produces a durable effective formatting delta after fresh reopen;
4. no semantic claim is made yet about where that delta is persisted.

If Phase 1 is positive, inspect the private A/B PUB outputs with the established T594/T595 bounded TABLE/CELLS/MCLD/OfficeArt tools before deciding which Phase-2 category arms are needed.

## Direct execution

```powershell
$Packet = "tools/research-runner/experiments/pub-t840-table-autoformat-phase1.packet.json"
$Out = Join-Path $PWD "out\pub-research\pub-t840-table-autoformat-phase1"

python tools/research-runner/validate_packet.py --packet $Packet --expected-environment publisher-2019
pwsh -NoProfile -File tools/research-runner/prepare_native_run.ps1 -PacketPath $Packet -OutputRoot $Out
pwsh -NoProfile -File tools/research-runner/operations/publisher_table_autoformat_phase1.ps1 -PacketPath $Packet -OutputRoot $Out
pwsh -NoProfile -File tools/research-runner/finalize_native_run.ps1 -PacketPath $Packet -OutputRoot $Out
```

## Verdicts

- `autoformat-delta-persists`: all-enabled Checkbook Register formatting differs from the no-op arm after fresh reopen. Continue raw carrier matched-diff and only then category/scheme/growth arms.
- `autoformat-no-durable-delta`: both fresh-reopened effective snapshots are equal. Do not assume table-style authority; inspect local/private outputs.
- `inconclusive`: lifecycle/table identity/reopen guard failed.

Generated PUB bytes are private local research artifacts and must not be uploaded.
