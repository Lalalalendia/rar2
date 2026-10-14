# AGENTS.md — Chaptera / Rar execution contract

This file is mandatory operational guidance for every AI agent, automation, or human-assisted agent modifying `Lalalalendia/rar2`.

`Lalalalendia/rar2` is the single active repository/workspace for current Chaptera implementation. `HeisLuka/rar` is the immediate historical predecessor/upstream. Older repositories are provenance unless a task explicitly says otherwise.

## 0. Read before touching anything

Before creating a branch, changing code, pushing a commit, or opening a PR:

1. Read the owning task in Notion.
2. Confirm:
   - task identity;
   - Status;
   - Dispatch State;
   - Execution Owner;
   - Execution Workspace;
   - direct dependencies;
   - current Waiting Reason / Acceptance.
3. Search GitHub for an existing branch or PR for the exact task.
4. Inspect the current Actions load for the repo and for the candidate PR/head.
5. If the change affects a product/function boundary, check the relevant Chaptera function-chain page in Tela.
6. Work only when the task is actually admissible.

If the required control-plane source is unavailable, do not guess. Record the blocker instead of inventing authority.

## 1. One task = one current implementation owner

There must be exactly one authoritative current implementation line for a task.

- Do not open a second current PR for the same task while another current PR/branch exists.
- A stacked/donor branch is not a second authority.
- If a fresh-main replay is required:
  1. create the successor from current `main`;
  2. replay only task-owned changes;
  3. explicitly mark the predecessor superseded;
  4. close the predecessor unmerged as soon as the successor becomes authoritative.
- Do not leave superseded PRs open "for reference". Git history and Notion preserve provenance.
- Execution-only research PRs are closed unmerged immediately after their receipt is assimilated unless an explicit downstream dependency still needs the branch.

Never infer task ownership from an old PR title alone. Notion task state + current GitHub owner together define the live execution line.

## 2. Fresh-main convergence without replay churn

Current implementation must converge on current `main`, not on an accidental historical stack. But **main advancing is not, by itself, a reason to create a replacement PR or restart a valid acceptance cycle**.

Before merge or final acceptance:

- re-check the PR base and current `main`;
- ensure no stale prerequisite branch is still embedded;
- compare current-main drift with the task-owned changed-file set;
- preserve already-landed authority instead of overwriting it with an older donor copy.

Restack/replay a live implementation line only when at least one is true:

1. current-main drift intersects task-owned changed files;
2. current-main changed the acceptance/workflow semantics needed to validate the task, so the old receipt is materially stale;
3. GitHub reports a real merge conflict or an embedded stale prerequisite must be removed.

If drift is disjoint, record the zero-overlap audit, let the current exact-head CI finish, and merge the bounded delta onto then-current `main`. Do **not** create a v2/v3/fresh-main successor solely to chase unrelated CI, corpus, documentation, or research commits.

A green head proves that exact task delta under the acceptance semantics it actually ran. It remains usable across disjoint trunk drift; it does not remain sufficient if the relevant task files or acceptance law changed.

## 3. Push discipline — do not DDoS our own Actions

Every push to an open PR can fan out into many workflows. Treat pushes as expensive scheduling events.

Mandatory rules:

- Batch related edits locally/in the working branch before pushing.
- Do not push every formatting tweak, typo, comment, or speculative fix separately.
- Do not use no-op commits or meaningless file edits to retrigger CI.
- If a workflow/job failed transiently and the source is unchanged, rerun the failed job/run instead of pushing a new SHA.
- Before pushing another SHA to an open PR, inspect whether the previous head still has queued/in-progress workflows.
- If previous-head workflows are still active:
  - push only when the new head materially invalidates the old head **and** those PR workflows support latest-head cancellation; otherwise wait for drain/cancel.
- Never create rapid successive pushes merely to "see what CI says".

If a task requires several corrective edits, inspect failures first, make the bounded correction set, then push once.

### Local development loop before hosted feedback

For ordinary repository code/workflow changes, GitHub Actions is not the development REPL.

- After a bounded local edit batch, run `python tools/dev_fast_loop.py --run` and fix task-owned failures locally before publishing another head.
- Before publishing a coherent semantic slice, run `python tools/dev_fast_loop.py --mode feature --run`.
- If the local environment genuinely cannot execute a selected check, record the exact missing runtime/tool in the task/PR instead of silently skipping it or pushing speculative fixes.
- Do not push a head that is already known to fail the local fast loop solely to obtain hosted diagnostics.
- A green fast loop is development evidence only. It does not replace the task's declared product, visual, native, integration, security, packaging, or release acceptance.
- When `sccache` is installed, the fast loop automatically reuses the shared compiler-object cache across sibling git worktrees; do not replace this with a shared `CARGO_TARGET_DIR`.
- Do not delete `.chaptera-local/dev-fast-loop/history.jsonl` merely to make a task look faster. It is ignored local telemetry for finding the actual p50/p95 development bottleneck.

The purpose is to permit many local implementation iterations per hosted head while preserving the existing acceptance law.

### Optional bounded integration-train mode

The ordinary task-specific PR remains the default. A task may stay branch-only for promotion through a shared integration train **only** when its owning Notion task explicitly names the active train/coordinator.

In that mode:

- the task branch remains the sole implementation owner;
- the agent must complete both local fast loops before handing off its exact head SHA;
- do not open a separate task PR merely to obtain hosted feedback;
- the coordinator must validate all candidates with `python tools/integration_train.py` against one exact common base;
- V0 trains are limited to the tool's bounded non-overlapping feature-code surface; any rejected/control/shared/release-sensitive change returns to the ordinary per-task PR path;
- the train PR is a promotion carrier, not a second implementation owner;
- closure for each task records both its exact candidate commit SHA and the shared train PR/accepted integration SHA.

Never improvise a train outside an explicit Notion assignment, and never add candidates to a train after its hosted acceptance has gone red.

## 4. Mandatory latest-head concurrency for PR validation

A PR-validation workflow whose older head loses evidentiary value when a newer commit arrives must cancel obsolete same-PR attempts.

New or materially edited PR-validation workflows must normally include:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

This rule is specifically for replaceable PR validation.

Repository safety net: `.github/workflows/pr-obsolete-head-canceller.yml` may cancel older PR-head runs when a new head arrives, but it is a backstop, not a substitute for workflow-level `concurrency`. A saturated runner pool can delay the backstop; workflow-level cancellation is the primary mechanism.

Do **not** use cancellation when completion of every historical attempt is itself required evidence. Such exceptions must be explicit in the workflow/task and must not be invented casually.

For mixed triggers such as `pull_request` + `push` + `workflow_dispatch`, PR attempts should supersede older PR attempts, while independent push/manual evidence may remain independent.

## 5. CI routing: preserve evidence, remove redundant scheduling

Do not make CI cheaper by silently dropping required coverage.

Instead:

- Shared kernel/shared-surface changes should have one umbrella regression owner.
- Feature workflows should trigger on task-owned modules/tests/protocol files, not broad shared roots when an umbrella owner exists.
- Prefer package/task-scoped `cargo fmt`, clippy and tests over unrelated workspace-wide checks inside feature workflows.
- A feature workflow may consume shared regression tests, but shared-file edits should not schedule every feature workflow independently.
- Do not add duplicate workflows that prove the same law on the same inputs.
- Do not add a second semantic runtime/provider/authority merely because CI composition is inconvenient.

A routing-only CI repair must not change product semantics.

## 6. Queue health stop conditions

Before opening another hosted PR or pushing a high-fan-out shared-surface change, inspect queue pressure.

Stop and repair/drain instead of adding more work when any of these is true:

- the same PR has queued/running workflows for more than one obsolete head generation;
- a superseded PR/branch is still consuming significant runner capacity;
- a single PR is responsible for a large fraction of the live queue;
- queue growth is caused by repeated pushes rather than independent tasks;
- the proposed change touches a known high-fan-out shared surface while an overlapping head is already active.

Do not confuse "many queued workflow runs" with "many independent tasks".

A proven GitHub provider-orphan record is **not** live queue pressure by itself. It may be excluded from runner-capacity stop decisions only when the bounded recovery path proves all of the following on a fresh re-read: obsolete pull-request head, public status queued, age at least 15 minutes, zero jobs across all attempts, zero pending deployments, updated_at equals created_at, and both normal cancel and force-cancel return the exact provider HTTP 409 saying the workflow run has not been queued yet. Keep these records visible in receipts and continue to report queue_drained=false; only queue_capacity_clear=true permits new hosted work. Any state change, job, deployment, current head, non-409 failure, or different provider response remains capacity-blocking until classified.

## 7. Failure classification before editing code

Classify a red run before making changes:

1. **Task-owned semantic/code defect** — fix the implementation.
2. **Receipt/artifact/output-path defect** — fix evidence production, not unrelated semantics.
3. **CI-routing/format scope defect** — fix the workflow boundary.
4. **Transient runner/provider failure** — rerun; do not mutate source.
5. **Stale/superseded head** — do not repair; replay/close according to current authority.
6. **Intersecting upstream-main drift/conflict** — reconcile when task-owned files or acceptance semantics overlap; disjoint trunk drift does not force a replay.

Do not turn every red check into a product-code change.

## 8. Branch and PR naming/lifecycle

- Branch and PR names must carry the task identity.
- PR body must state the bounded owned slice and explicitly name what it does **not** re-own.
- Draft is allowed while composition is incomplete.
- Once superseded, say so in title/body and close promptly.
- Do not keep multiple "v1/v2/v3" PRs open after authority has moved to the latest one.
- Do not merge execution-only/provenance PRs unless the task explicitly requires repository changes on main.

## 9. Evidence and closure

A task is not DONE because "CI looked green".

Closure evidence must identify the authoritative current head and relevant receipts:

- PR number;
- exact commit SHA;
- workflow/run IDs;
- artifacts/receipts where applicable;
- real acceptance result;
- known bounded limitations.

Use the latest authoritative head. Old-head receipts remain provenance, not current closure.

After material progress or closure:

- update the owning Notion task;
- record merge/close status and exact evidence;
- update affected control-plane/audit pages when routing or scheduler behavior changed;
- when product/function meaning changed, update or verify the corresponding Tela function-chain page.

Do not let GitHub, Notion, and Tela disagree silently.

## 10. Public/private boundary

Never put private Chaptera source, customer files, credentials, proprietary/licensed binaries, or private runtime state into this repository.

Local/private/native producers may contribute only bounded source-free artifacts/receipts allowed by the task.

Do not weaken this boundary to make hosted CI convenient.

## 11. No architecture invention during implementation

When an owner/law/provider already exists:

- compose it;
- adapt to it;
- prove it.

Do not introduce:
- a second document authority;
- a second revision law;
- duplicate SourceIngress;
- duplicate BlobStore semantics;
- duplicate AuthN/AuthZ authority;
- a new renderer/runtime layer solely to bypass an integration problem.

If the current architecture is actually insufficient, stop and raise the gap in the owning control plane before coding a competing authority.

## 12. Handoff rule

When leaving work unfinished, the handoff must say:

- exact task;
- authoritative branch/PR/head SHA;
- what is already proven;
- exact remaining failure/blocker;
- which files/surfaces are currently owned;
- whether another agent may safely take over;
- current Actions state if CI is still active.

Never hand off with only "continue from here".

## 13. Default priority

When several actions are possible, prefer in this order:

1. stop queue/control-plane corruption;
2. finish/reconcile the current authoritative head;
3. close superseded execution;
4. unblock the launch critical path;
5. only then open new parallel implementation.

The goal is not maximum agent activity. The goal is maximum trustworthy progress per current authoritative head.
