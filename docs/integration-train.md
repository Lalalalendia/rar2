# Chaptera bounded integration train

The integration train reduces hosted GitHub Actions fanout when several small, independent feature slices are already proven by the local development loop.

It is **optional** and must be explicitly assigned by the owning Notion control plane. The ordinary one-task/one-PR path remains the default.

## Candidate flow

Each feature agent:

1. starts from the exact train base;
2. implements only its owned semantic slice;
3. runs `python tools/dev_fast_loop.py --run` while iterating;
4. runs `python tools/dev_fast_loop.py --mode feature --run` before handoff;
5. pushes the exact task branch/head;
6. does **not** open a hosted PR when the task is explicitly enrolled in an active train.

The coordinator then validates all candidates without mutating the repository:

```bash
python tools/integration_train.py \
  --base <exact-base-sha> \
  --candidate TASK-A=feature/task-a \
  --candidate TASK-B=feature/task-b \
  --branch-name integration/train-42 \
  --receipt target/integration-train-42.json
```

The receipt contains exact candidate SHAs, changed paths, unique commits and shell-safe suggested cherry-pick commands. V0 never executes those commands itself.

## V0 admission

A train contains 2–8 candidates. Every candidate must:

- descend from the exact common train base;
- contain no merge commits;
- own unique commits not shared with another candidate;
- own changed paths that do not overlap any other candidate;
- change at most 25 files;
- stay inside bounded Rust feature/test surfaces.

The whole train is capped at 80 changed files.

V0 rejects control-plane and high-risk/shared surfaces including workflows, CI tooling, Cargo manifests/locks, installer/deploy/product packaging, updater crates, `AGENTS.md`, generic `src/lib.rs`, Desktop `main.rs`, `render_backend.rs`, and `source_font.rs`.

Those changes continue to use the normal task-specific PR path.

## Promotion

After a valid plan:

1. coordinator creates the named train branch from the exact base;
2. coordinator cherry-picks the receipt's exact commit order;
3. coordinator checks the resulting tree/diff;
4. run the fast feature loop on the composed tree;
5. open **one** integration PR to `main`;
6. normal selective/deep hosted evidence runs on that integration SHA;
7. each candidate task records the shared train PR plus its own exact candidate commit SHA.

The train PR is a promotion carrier. It does not become a second implementation owner for the tasks inside it.

If deep evidence fails, stop the train and bisect the bounded candidate set. Do not keep adding candidates to a red train.


## Heavy-evidence union

The train planner reuses the canonical Reader PR fanout classifier to compute the expensive hosted evidence families touched by each candidate and by the train as a whole.

The receipt now records:

```json
{
  "candidates": [
    {
      "label": "TASK-A",
      "heavy_families": ["visual_oracle", "typography_golden"]
    }
  ],
  "heavy_families": ["visual_oracle", "typography_golden"],
  "heavy_family_members": {
    "visual_oracle": ["TASK-A"],
    "typography_golden": ["TASK-A"]
  }
}
```

This is intentionally a **union**, not one heavy execution per candidate. The integration SHA is the authority that pays for each selected heavy family once.

The planner does not invent a second path-to-evidence map. It consumes the same `tools/ci/reader_pr_fanout.py` classification used by hosted Reader selective CI, so routing drift is visible in one authority.

V0 only records the required heavy-family closure. Hosted workflow composition that executes those families exactly once is a separate step and must preserve each family's existing evidence semantics.


## Promotion verification

After composing the train branch, verify that the exact integration head contains **only** the planned path union and still resolves to the same heavy-evidence family union:

```bash
python tools/integration_train.py \
  --base <exact-base-sha> \
  --candidate TASK-A=<exact-candidate-sha> \
  --candidate TASK-B=<exact-candidate-sha> \
  --verify-head <exact-integration-head-sha> \
  --receipt target/integration-train.json
```

A successful receipt includes `composed_verification` with the exact integration head SHA, changed paths and heavy families.

The verifier fails closed when:
- the composed head does not descend from the planned base;
- an expected candidate path is missing;
- an unplanned path appears;
- the aggregate heavy-family classification differs from the plan.

This is the promotion gate before trusting the train PR's single aggregate hosted acceptance.


## Required scopes vs executable heavy jobs

The Reader classifier describes semantic evidence requirements. The hosted DAG may compose several requirements into one reusable job.

The train receipt therefore keeps both layers:

- `heavy_families` — raw required heavy scopes from the canonical classifier;
- `effective_heavy_jobs` — top-level hosted jobs after current DAG composition.

Current composition law:
- when `visual_oracle` is required, its reusable workflow receives the typography-golden and Reader shared-core-smoke switches;
- standalone `typography_golden` and `reader_windows_smoke` are therefore not separate effective jobs;
- `android_core` is an independent selective job and must be present in both the required scope and effective-job model when the classifier selects it.

The receipt also records family/job → member attribution independently.

This split prevents two opposite errors:
1. under-counting a real hosted gate such as Android core;
2. over-counting embedded evidence as if smoke/typography allocated extra runners beside the visual oracle.

Promotion verification recomputes and compares both sets on the exact composed head.
