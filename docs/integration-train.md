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
  --integration-ref <composed-integration-sha> \
  --receipt target/integration-train-42.json
```

Before planning, each candidate produces an exact-head feature-loop receipt:

```bash
python tools/dev_fast_loop.py \
  --base <exact-base-sha> \
  --head <candidate-sha> \
  --mode feature \
  --run \
  --receipt target/TASK-A-fast.json
```

The coordinator passes one proof per member:

```bash
python tools/integration_train.py \
  --base <exact-base-sha> \
  --candidate TASK-A=feature/task-a \
  --fast-proof TASK-A=target/TASK-A-fast.json \
  --candidate TASK-B=feature/task-b \
  --fast-proof TASK-B=target/TASK-B-fast.json \
  --receipt target/integration-train-42.json
```

The planner rejects a proof unless its schema/status/mode, exact base SHA, exact candidate head SHA and changed-path set all match the candidate.

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

## Heavy evidence amortization

The train does not create a second product-validation framework. It reuses the existing Reader PR impact classifier as the authority for expensive evidence families.

For each candidate the planner computes the heavy scopes it would independently request. The train receipt then records:

- the per-candidate heavy execution baseline;
- the union of heavy families for the composed train;
- the number of duplicate heavy executions avoided.

If two candidates both impact `visual_oracle`, the integration PR pays for that family once.

After composition, pass `--integration-ref <sha>` to make the receipt verify:

- the integration ref descends from the exact train base;
- its aggregate changed-path set equals the member union;
- every candidate-owned path has exactly the same content as that candidate head.

The actual product authority remains the existing selective/deep workflows triggered by the single integration PR SHA.

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
