# CI-LOCAL-DEV-TRANSPORT-01 — Chaptera Windows development feedback

Owning Notion task: https://app.notion.com/p/3f932a84beec81e5b4a5e3c363a70b1e

## Boundaries

The transport runs the existing `tools/dev_fast_loop.py`; it does not create AI tasks,
modify code, merge PRs, or substitute product acceptance. GitHub's protected
`main` is the sole automated source of code. A PR/issue command must never
execute on this worker. The existing `chaptera-publisher-oracle` runner,
its COM runtime and private files are **not** part of this deployment.

Create a **separate** least-privileged GitHub Actions Windows runner registered
only for this trusted repository with custom label `chaptera-dev-unprivileged`.
Use a separate non-administrator OS account, workspace, and runner installation.
Do not give it Publisher fixtures, native Oracle environment variables, Notion
tokens, deployments secrets, or credentials stored for other jobs. Run it only
on an isolated machine or properly isolated VM if this PC contains private data;
self-hosted Actions jobs run repository code with the runner user's OS rights.

The workflow is hourly and manual for trusted `main` only; every scheduled
run checks out the event's immutable `github.sha` without persisting the GitHub
token. No PR workflow executes on the PC. The source-safe receipt is uploaded as
a short-retention artifact; no stdout/stderr, private document, or source bundle
is uploaded by this adapter. The sanitized checkpoint is stored under `runner.tool_cache` to survive jobs
on a persistent runner. The directory must be owned by the unprivileged runner
account; do not share it with Publisher/native jobs. Full fast-loop logs remain
local and are not uploaded. They are only bounded to 64 KiB each, not automatically
purged by this adapter; configure host retention/cleanup separately.

## First run and limitations

1. Merge the reviewed transport PR and wait for its hosted contract tests.
2. Install the separate runner and Git/Rust/Python/Node as required by the
   pre-existing fast-loop. Never add the dev label to the Publisher oracle.
3. Run the workflow manually on `main`, inspect its job and artifact, then
   confirm recurring operation via its scheduled event.
4. Verify a real Windows run before marking the task DONE.
5. The first run checks only the latest commit (`bootstrap_last_commit_only`).
   Later runs use the last successful SHA while local state exists.
   Missing previous SHA or rewritten history blocks rather than silently
   claiming broader coverage.
6. This v1 emits reports only. It does not pull from developer worktrees, write
   commits to GitHub, publish PR comments, or transfer edited files. GitHub
   checkout handles source synchronization; existing agents own changes/PRs.
7. Developer feature work should still use a normal local checkout with
   `python tools/dev_fast_loop.py --mode feature --run` before a PR.
