# AUTH-WRAP #1188: guarded current-main execution binding

Owner: [rar2 issue #1188](https://github.com/Lalalalendia/rar2/issues/1188).
Scientific owner: [PUB-T-150 / AUTH-WRAP-01](https://app.notion.com/p/3df32a84beec81a9841fd13ef469cde9).
This replaces no Product/Editor code or format law.

## Bounded scope

- Reuses the previous `auth_wrap_oracle_tool` Rust inspector/one-reference patcher
  from the 2026-10-04 donor branch. The donor is historical; this source tree is
  based on the then-current `rar2/main` and is the single PR owner.
- Experiment packet accepts only public Apache POI SampleNewsletter PUB, SHA-256
  `6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf`.
- Publisher executable must be 16.0.12527.* with SHA-256
  `e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b`.
- Family A: replace precisely one reference inside a copied Contents 0x47
  member, verify original geometry and OfficeArt FOPT are untouched, open in
  Publisher, save a **new** PUB, close and fresh-reopen.
- Family B: leave 0x47 untouched in a new source copy and change only
  `Shape.TextWrap.DistanceLeft` when the existing shape already admits manual
  wrap distances. No `DistanceAuto` setter or speculative enum values.
- A fresh, independently verified real reset/restore provider receipt is
  **mandatory before each** family; a pair must pass the existing cold-restore
  verifier. Two guardrails: zero pre-existing MSPUB processes and an unchanged
  original fixture digest. Failure to isolate the target stops without a law.
- Private `.pub` files and raw inspection details stay within
  private folders of the local OutputRoot. Public receipts contain bounded
  booleans, enums, and source-free SHA only.

## Execution contract

The provider/authorized host must already have a real snapshot with the
experiment packet's `reset.baseline_id` and `reset.snapshot_id`.
Those IDs are declarative labels, **not** evidence the backend exists.
The authorized lab configures `PUB_RESEARCH_RESET_PROVIDER` (backend adapter)
plus `PUB_RESEARCH_FIXTURE_ROOT` (containing exact `SampleNewsletter.pub`).
No automatic downloads and no runner registration are performed.

Run *from the fresh and trusted repository checkout* using its independently
pinned full commit SHA (not an untrusted branch name):

```powershell
pwsh -NoProfile -File tools/research-runner/auth_wrap_direct_1188.ps1 `
  -TrustedCommitSha <verified40hexsha> `
  -OutputRoot C:\Temp\auth-wrap-1188-exact `
  -AcknowledgeIsolatedLab
```

Use `-CheckOnly` for source/packet preflight without native execution.
That mode cannot claim Publisher semantic success.
A fresh OutputRoot must not exist yet; all experimental documents remain
inside it. Exact Git blobs of packet, operation, comparator, launcher,
and oracle must match the trusted checkout.

## Evidence and stop rules

- The Python comparator rejects cross-document/shape identity drift, pre-mutation
  FOPT/geometry differences, duplicated 0x47 targets, incorrect counts, and
  changes to non-target frames.
- Any host, COM, fixture, Writer, reset or ambiguity error is a STOP; do not
  fill receipt booleans with synthetic 'PASS'.
- A structurally valid result is **partial native observation** only: no
  approved PDF line-layout comparison is included, so `layout_outcome =
  not_observed`, `authority_class = inconclusive` and `closure_candidate = false`.
- PUB-T-150 remains responsible for full glyph/band layout, conflict
  classification, new law admission and downstream EDITOR-WRAP-PRESERVE.

## Tests and current limitations

Offline tests (one line):
`python -m unittest discover -s tools/research-runner/tests -p 'test_auth_wrap_*_1188.py'`.
Main packet validator:
`python tools/research-runner/validate_packet.py --packet tools/research-runner/experiments/auth-wrap-01.packet.json --expected-environment publisher-2019`.
Existing native receipt validator is reused unchanged.

This implementation has not been compiled with Rust or executed against live
Publisher in the current Linux authoring environment. The branch and PR remain
DRAFT until an independently verified Windows native run is recorded. Geometry
joining and manual wrap-distance applicability are intentionally strict; on an
unsupported source they fail closed rather than inventing a match.

## 2026-10-15 bounded native child and advisory lease

Each AUTH-WRAP A/B arm now runs in one independent PowerShell child with a
600-second limit. Preparation, COM operation and finalization preserve the
same process-local fixture and Publisher SHA bindings. Private logs stay
under OutputRoot/private. The watchdog terminates only its own child process
handle on timeout, marks local quarantine, and stops rather than killing a
Publisher process by creation time. Restore a dedicated lab snapshot after
any child timeout or orphaned MSPUB process.

The named global mutex only coordinates Chaptera launchers. It does NOT
prove that the out-of-process Publisher COM server belongs to the test, or
exclude concurrent human Publisher use. The approved isolated VM, owned-PID
review, and independently verified reset-provider negative tests remain
required. Hosted -CheckOnly is not a native semantic or safety PASS.


## 2026-10-15 native-mode interlock and reset-time collision checks

Native execution now requires an explicit `-AcknowledgeIsolatedLab` flag;
omitting it fails before native host access or output creation. This flag is a
human acknowledgement, **not** proof that the Publisher instance is owned.
The child rejects MSPUB processes observed after reset-provider preparation,
and the operation rejects ambient MSPUB processes immediately before each
of the two COM activations (Open and fresh reopen). These checks narrow the
race window but cannot replace exclusive VM access and independently
validated ownership of the out-of-process COM server PID.

The oracle binary path is resolved using Cargo metadata, honoring an
explicit `CARGO_TARGET_DIR` rather than assuming a workspace-local target.
The Windows research packet CI exercises the no-acknowledgement negative
path without starting Publisher. Keep the PR Draft and the native gate blocked
until physical lab owner review and reset-provider negative controls.
