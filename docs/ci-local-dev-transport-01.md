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

The Windows transport workflow is hourly and manual for trusted `main` only.
PR changes to the transport are tested by the existing hosted
`dev-fast-loop-selftest.yml` workflow. No PR workflow executes on the PC. Each production run checks out the immutable `github.sha`
with full Git history for the last-success SHA comparison, and without persisting
the GitHub token. The source-safe receipt is uploaded as
a short-retention artifact; no stdout/stderr, private document, or source bundle
is uploaded by this adapter. The sanitized checkpoint is stored under `runner.tool_cache` to survive jobs
on a persistent runner. The directory must be owned by the unprivileged runner
account; do not share it with Publisher/native jobs. Full fast-loop logs are written to `runner.temp` rather than persistent cache;
they are not uploaded. Only the hashed commit identities and last-success marker
remain in persistent runner state. The runner administrator should periodically
clean the temp area if that runner does not clear it automatically.

## First run and limitations

1. Merge the reviewed transport PR and wait for its hosted contract tests.
2. Install the separate runner and Git/Rust/Python/Node as required by the
   pre-existing fast-loop. Never add the dev label to the Publisher oracle.
   Keep the repository variable `CHAPTERA_LOCAL_DEV_RUNNER_ENABLED` absent or
   `false` until the distinct runner is actually online. Then set it to `true`
   in Settings > Secrets and variables > Actions > Variables. With the variable
   unset, scheduled local jobs are skipped: source code may merge safely before
   registration without accumulating self-hosted queue backlog.
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
