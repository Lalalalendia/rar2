# PUB Harness v0

Local deterministic state layer for Codex/agent workflows.

## Boundary

This directory contains only public-safe code and schema.

Do **not** commit:
- Notion credentials or tokens;
- a live `pub-harness.db`;
- private/customer page bodies;
- private runtime artifacts.

The live SQLite database is local state. Notion remains the human-facing control plane and canonical source for task/knowledge records until a specific authority contract says otherwise.

## v0 architecture

```
Notion authority DBs
      |
      | bounded sync
      v
SQLite + WAL + FTS5
      |
      | typed query surface
      v
Codex / local agents
```

The agent is intentionally not given arbitrary SQL. It should use bounded operations such as:
- `task_get(id)`
- `task_next(lane, owner, limit)`
- `task_children(id)`
- `blockers_list(priority)`
- `search(query, entity_type)`

## Initial mirrored entity classes

1. Research Tasks
2. Implementation Tasks
3. Agent Runs
4. Canonical Claims
5. Knowledge Change Events
6. Change Proposals

v0 starts with task metadata + relations. Page body hydration is lazy and should happen only for pages the agent actually needs.

## Knowledge→code reconciliation planner

`k2c_reconcile.py` is the source-safe decision layer for the derived Chaptera
Notion + GitHub → Tela graph.

It does **not** call or write Notion, GitHub, or Tela. It accepts normalized
registered-surface snapshots from the existing adapters and emits one
deterministic reconciliation receipt:

- `K0` — no upstream delta, no write;
- `K1` — cursor/execution churn only, no semantic write;
- `K2` — implementation/code-anchor delta;
- `K3` — acceptance/evidence delta;
- `K4` — owner/supersession delta;
- `K5` — semantic/product-authority delta with bounded registered fan-out;
- `INVALID` — fail-closed authority/owner/code-anchor/classification state.

The planner consumes the result of existing owner-uniqueness checks rather than
reimplementing GitHub ownership law.

Example:

```bash
python k2c_reconcile.py \
  --before /path/to/before.json \
  --current /path/to/current.json \
  --receipt /tmp/k2c-reconcile.json
```

External writes are a separate adapter boundary. A Tela writer must use a
documented supported API and apply only a validated explicit patch set; this
planner must never scrape or invent a private write interface.

## Local smoke test

```bash
cd services/pub-harness
python -m unittest -v
```

No external Python packages are required for the store/query/planner core.
