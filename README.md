# rar2 — current public execution workspace

> **Agents and automation:** read [`AGENTS.md`](AGENTS.md) before creating branches, pushing commits, opening PRs, or changing workflows. It is the mandatory execution/CI contract for this repository.

`Lalalalendia/rar2` is the **single active repository/workspace** for current PUB / Chaptera work from 2026-09-29 onward.

## Repository authority

- **Current source/workspace:** `Lalalalendia/rar2`.
- **Immediate historical predecessor/upstream:** `HeisLuka/rar`.
- **Older provenance:** `HeisLuka/miy`, `HeisLuka/yab`, `HeisLuka/pub-rs`.
- New branches, PRs, issues, GitHub Actions, corpus work, experiments, packaging and current receipts belong in `rar2`.
- There is no routine promotion-back-to-`HeisLuka/rar` step.
- Historical repository links, issue/PR numbers, commits, workflow runs and artifacts remain valid provenance and must not be mechanically renamed as if they existed in `rar2`.

## Boundary

- Public-safe workflows, bounded experiments, CI validation, schemas, sanitized receipts and corpus acquisition run here.
- Private Chaptera source, customer documents, licensed/proprietary binaries, secrets and private runtime state must not be copied here.
- Local/private/native producers may emit only bounded source-free artifacts/receipts allowed by the owning task.
- When historical code is needed, port only the minimum task-required slice and preserve its provenance.

## Migration rule

For an active task that still references `HeisLuka/rar` or an older repository:

1. preserve old repo links/hashes/PRs/issues/runs as provenance;
2. route the **current execution workspace** to `Lalalalendia/rar2`;
3. if a historical branch/PR contained useful unmerged work, replay only the task-owned delta from current `rar2/main`;
4. do not create fake `rar2` issue/PR identities by renaming old `rar#...` references;
5. record the new `rar2` branch/PR/run/receipt back in Notion.

## Default rule

**All live work goes through `rar2`; `HeisLuka/rar` is historical provenance.**
