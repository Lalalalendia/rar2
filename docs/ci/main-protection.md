# Main branch protection

Authority: GitHub issue #138 (REPO-MAIN-PROTECTION-01).

The CI merge authority workflow does not itself configure branch protection. The connected GitHub App cannot write repository administration settings; the branch-protection API returned HTTP 403.

## Why one required summary check

Most owner checks are path-filtered. GitHub leaves required checks from skipped workflows pending. The unconditional PR merge authority job (check name: required-ci) instead inspects expected path-selected workflows and waits for their exact-head results. Failed, missing, cancelled and timed-out workflows fail closed. This costs one additional polling runner during PR validation.

## Admin activation order

1. First land the CI-only merge-gate PR through manual exact-head verification. Verify the required-ci check on an unmerged control PR, including a deliberately missing/failed owner check; it must never green prematurely.
2. Open https://github.com/Lalalalendia/rar2/settings/branches and add a protection rule for main.
3. Require pull requests before merging. Do not require an impossible second-person approval in a solo-owner repository.
4. Require status checks: required-ci and frozen-repo-paths, both from GitHub Actions. Use the exact check names observed on live PRs.
5. Require branches to be up to date; enforce for administrators; block force pushes and deletion; minimize bypasses; require conversation resolution if practical.
6. Verify GET /repos/Lalalalendia/rar2/branches/main reports protected=true; an admin GET protection must show strict=true and both required checks.
7. Confirm a bad PR cannot merge and a direct push to main is rejected. Only then enable auto-merge selectively for reviewed product or structural owner PRs. Never enable auto-merge on sacrificial, MEASURE, leaf-control or research PRs.

## Caveats

- Never add path filters to the required-ci workflow.
- The initial gate covers pull_request-triggered workflows. push, workflow_dispatch and pull_request_target-only acceptance must be evaluated separately before making them merge-critical.
- Disabled workflows or unsupported path syntax can leave the gate pending. Resolve the actual mismatch instead of bypassing it.
- Require deliberate review of changes to the gate or workflow definitions: a PR that rewrites its own safety check may weaken the meaning of that check.
- Strict current-main checks can generate restacks; prefer serialized landing rather than relaxing safety without a replacement.

Status is not DONE until admin branch protection and negative controls are verified.

## Verification record — 2026-10-08

- Repository ruleset `Protect main` is active for exact ref `refs/heads/main`.
- Required GitHub Actions checks: `required-ci` and `frozen-repo-paths`, with strict up-to-date enforcement.
- Sacrificial PR #2090 intentionally failed `required-ci`; `frozen-repo-paths` stayed green and GitHub reported `mergeable_state=blocked`. It was closed unmerged.
- This runbook change is the positive control intended to merge through the protected path and verify normal auto-merge behavior.
