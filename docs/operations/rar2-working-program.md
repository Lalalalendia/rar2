# Rar2 working program

Canonical Notion program: https://app.notion.com/p/3ea32a84beec8177b01fef8e89b019cf

## Objective

Ship Chaptera Reader Public V0 while building reusable PUB engine, corpus, differential, security, and release evidence infrastructure.

## Authority model

- Notion: task semantics, priority, dependencies, acceptance, blockers.
- Lalalalendia/rar2: current branches, PRs, Actions, receipts, release artifacts.
- HeisLuka/rar and older repositories: historical provenance only.

Do not bulk-migrate historical work. Recreate only current, admitted, dependency-ready work.

## Workstreams

1. WS0 Repository control plane
2. WS1 Reader Public V0
3. WS2 Visual fidelity convergence
4. WS3 Windows security + lifecycle
5. WS4 Corpus intelligence
6. WS5 Differential + visual evidence farm
7. WS6 Release engineering
8. WS7 Post-V0 product expansion

## W0: executable now

Start with three clean P0 heads that have no current rar2 implementation line:

- CHAPTERA-WIN-SUPPORT-MATRIX-01
- CHAPTERA-WIN-DLL-SEARCH-HARDEN-01
- CHAPTERA-WIN-PROCESS-LAUNCH-HARDEN-01

Historical GitHub issues are provenance only:
- HeisLuka/rar#1152
- HeisLuka/rar#1139
- HeisLuka/rar#1144

## W0-R: reconcile before replay

Do not duplicate historical in-flight work until its delta is checked against rar2/main:

- CHAPTERA-CI-TRUST-01 — historical rar#1281
- DESKTOP-PUB-SANDBOX-WIN-01 — historical rar#1260
- READER-REFERENCE-PAGE-PROJECTION-01 — historical rar#1170
- CHAPTERA-WIN-PATH-IDENTITY-01 — historical rar#1285
- READER-WIN-SHELL-ACTIVATION-01 — historical rar#1279
- CHAPTERA-WIN-SIGN-PREFLIGHT-01 — producer evidence in Lalalalendia/lalamu

## Concurrency

Maximum three independent implementation heads plus one control-plane/reconciliation head.

One Notion task maps to at most one current rar2 implementation branch/PR line.

## Closure

Green CI is not enough. Closure records:
- exact PR
- exact SHA
- workflow/run IDs
- artifacts/receipts
- observable acceptance result
- bounded remaining limitations
