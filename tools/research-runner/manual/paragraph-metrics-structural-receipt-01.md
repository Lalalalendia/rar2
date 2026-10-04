# PUB-T-823 structured FDPP/TEXT post-run analysis

This is the structural handoff for `PARAGRAPH-METRICS-AUTH-01`, after the
common-seed native matrix from #1156. It consumes the retained private outputs;
it does not run Publisher or change Reader layout.

The probe reuses `pub-cfb`, confirmed `pub-quill` Story/descriptor parsing and
the existing FDPP/style grammar. Its optional `research-inspection` feature
rejects unknown framing instead of assuming an empty block. Snapshots retain
hashes, range coordinates, property tags and the raw `0x34`/`0x234` candidate.
They omit recovered text, opaque payloads, filenames and local paths.

The independent analyzer requires all 11 named arms, exact seed/control/arm
file identities, available and identical pre-mutation COM/frame snapshots,
confirmed Story partition identity and exact Quill TEXT hash/length equality.
Equal-length text substitutions fail. It reports FDPP/STSH descriptor deltas,
raw candidate bits and after-mutation/fresh-reopen metrics without assigning
units or granting an omitted/default line-spacing law.

## Offline Publisher-host command

Use a locally built or already staged `paragraph-metrics-probe.exe`. Build
dependencies must already be present on the host; do not acquire them there.

```powershell
Set-Location C:\Users\User\rar2
cargo build --offline --locked --release --manifest-path tools\research-runner\paragraph-metrics-probe\Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "Offline probe build failed; pre-stage the exact build/dependencies." }

$Out = Join-Path $PWD "out\pub-research\paragraph-metrics-auth-01-direct"
$Probe = Join-Path $PWD "tools\research-runner\paragraph-metrics-probe\target\release\paragraph-metrics-probe.exe"
# If CARGO_TARGET_DIR is explicitly set, resolve the probe under that exact directory instead.
python tools\research-runner\analysis\paragraph_metrics_auth_01_structural.py --output-root $Out --snapshot-tool $Probe
if ($LASTEXITCODE -ne 0) { throw "Structural matrix rejected; do not promote paragraph metrics." }
Get-Content -LiteralPath (Join-Path $Out "analysis\paragraph-metrics-auth-01-structural.json") -Raw
```

Missing private outputs, an old independently seeded native receipt, absent
COM values, unsupported framing, changed TEXT/Story identity or identity
mismatch fail closed. An earlier output receipt is removed before a fresh CLI
analysis so that a failed rerun cannot leave a stale success behind.

Hosted tests use generated synthetic CFB/Quill inputs and adversarial receipt
fixtures. They prove the tooling; they do not prove Publisher's carrier law.
Native `Single / 1.5 / Exactly 18pt / Exactly 24pt` persistence, effective line
origins/heights and the source-neutral #1139 handoff remain parent-authority
work. Do not promote fallback-font metrics or the historical libmspub decoding
from this receipt.
