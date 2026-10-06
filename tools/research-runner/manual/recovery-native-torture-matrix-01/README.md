# RECOVERY-NATIVE-TORTURE-MATRIX-01

Read-only native Microsoft Publisher discriminator for the current exact-five natural Chaptera salvage witnesses.

## Run

From the existing Windows rar2 checkout:

```powershell
git fetch origin
git switch research/recovery-native-torture-matrix-01
git pull --ff-only

$InputRoot = "D:\Downloads\Downloads\pub-corpus"
powershell -ExecutionPolicy Bypass -File .\tools\research-runner\manual\recovery-native-torture-matrix-01\run_native.ps1 -InputRoot $InputRoot
```

The input root may contain other PUB files. The runner admits only files whose SHA-256 exactly matches the manifest.

## Safety invariants

- Publisher Open is read-only.
- AddToRecentFiles is disabled.
- No Save call is performed.
- Source SHA-256 is verified before and after each arm.
- Environment/pin/hash failures remain inconclusive.
- Returned JSON is source-free: no document text, image bytes, source paths, machine/user names or raw PUB bytes.

## Phase order

Run Phase 1A first:
- OPNHOUS `227961e2...`
- trophy_traditional `32b85747...`

Only after Phase 1A is operationally clean, use the same runner for Phase 1B:
- `c96df4b2...`
- `b29ed1e9...`
- OPCLS `6cb88d4e...`

## Output

`out/recovery-native-torture-summary.json` contains all admitted rows.

`out/phase1a-summary.json` contains only the initial two-witness discriminator.

A row becomes Class A only when Publisher refuses for a file/content reason while the canonical Chaptera result for the same exact SHA is already `salvage_open`.
