# CI-LOCAL-DEV-HANDOFF-01 — local feature branch to GitHub

Owning task: https://app.notion.com/p/3f932a84beec81018b05ed87b37a4510

## Purpose and authority

This opt-in helper completes the **outbound** half of Chaptera's local development
loop. The incoming read-only trusted-main runner already landed in
[PR #2757](https://github.com/Lalalalendia/rar2/pull/2757).

**No local machine access is implied by code in GitHub.** A local development
agent/human must have an already-owned Implementation Task, a clean committed
feature branch and owner-approved GitHub authentication. This script neither
chooses tasks nor claims Notion authority, stages files, commits, uses Publisher,
merges PRs or deploys anything.

## Windows usage

Use a separate developer checkout and an ordinary low-privilege Windows account,
not the existing `chaptera-publisher-oracle` runner account.

1. `git clone https://github.com/Lalalalendia/rar2.git C:\chaptera-dev\rar2`
2. `cd C:\chaptera-dev\rar2` then `git fetch origin main`
3. Start a task-owned branch: `git switch -c localdev/ci-sample-feature-01 origin/main`
4. Have a coding agent make the task-specific edits. Review them, and explicitly
   commit the intended public source changes on this branch.
5. Preview without any network calls or push:

   `py -3 tools/ci/local_dev_handoff.py --task CI-SAMPLE-FEATURE-01 --receipt "$env:TEMP\chaptera-handoff.json"`

6. With a locally authenticated GitHub CLI (`gh auth login`, performed only
   by the owner), run the full handoff:

   `py -3 tools/ci/local_dev_handoff.py --task CI-SAMPLE-FEATURE-01 --receipt "$env:TEMP\chaptera-handoff.json" --publish --draft-pr`

Only the exact `localdev/<task-id-lowercase>` branch is eligible. The origin
must be exactly `Lalalalendia/rar2` over an accepted canonical HTTPS or SSH
URL. The helper fetches main, refuses intersecting upstream drift, verifies a
clean worktree, validates a small text-only owned diff under `apps/web/`,
`services/editor-api/`, or `vendor/producer-a/crates/pub-editor/`, and runs
the existing **feature** fast-loop. It then uses a non-force git push, and
optionally creates or reuses a draft PR with `gh`.

## Security and limitations

- No GitHub credential is passed through chat, command arguments, repo files or
  report. The owner authenticates `git` and `gh` on their PC.
- The helper checks obvious private-key/token markers and blocks binary or
  non-allowlisted paths. This is defense in depth, not a proof that source text
  has no secrets. Do not publish any private PUB files, fonts, licensed
  binaries, local fixtures or client documents.
- The machine-readable JSON is source-safe (task ID, branch, SHA, counts and
  status). Raw compiler logs and working files are never uploaded by the helper.
- `--publish` is explicit: preview mode has no network. No force-push, commit,
  branch creation, background daemon, or automatic merge.
- If branch push succeeds but draft PR creation fails, the report says
  `PUSHED_PR_BLOCKED`. Re-running is safe: the ordinary push is idempotent;
  authentication/PR creation can be retried without changing commits.
- This local handoff is *not* a protected release acceptance. The normal
  GitHub PR CI, owner/Notion review and merge gates remain authoritative.
- Other paths such as `.github/`, `deploy/`, binaries and broad shared
  infrastructure need normal reviewed PRs, not this automated helper.

## Validation

Offline tests use disposable Git repositories and a local bare remote,
including non-fast-forward rejection:

`python -m unittest discover -s tools/ci -p test_local_dev_handoff.py`

The existing hosted fast-loop self-test workflow runs them on PR changes.
A real Windows task branch/push/draft-PR is required before this task can be
declared fully operational.
