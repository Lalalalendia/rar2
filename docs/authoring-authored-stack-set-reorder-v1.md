# AuthoredStack selected-set reorder V1 — bounded pure-core law

Owned task: `AUTHORING-AUTHORED-STACK-SET-REORDER-01`; GitHub issue #2669.

## Source of truth and boundary

`AuthoredStackV1.members` is the canonical back-to-front Chaptera **authored-only** lane. The *selected set* is transient UI state; it does not have authority to change the ordering of individual members. The planner normalizes selected `NodeId` values to their **before-lane order**. A requested set must have at least two distinct IDs already in the same lane; otherwise the entire operation fails. No safe-subset promotion occurs.

The pure algorithm is at `vendor/producer-a/crates/pub-editor-authoring-core/src/authored_stack_set_reorder_v1.rs`, and uses the already-shipped `AuthoredStackReorderModeV1` enum (`StepForward`, `StepBackward`, `ToFront`, `ToBack`) to avoid a second command vocabulary.

- `ToFront`: `[unselected in before order][selected in before order]`.
- `ToBack`: `[selected][unselected]` in before order.
- `StepForward`: each maximal contiguous selected block simultaneously exchanges places with **one immediately-front unselected member**. A frontmost block cannot move.
- `StepBackward`: the symmetric exchange with the immediately-back unselected member. A backmost block cannot move.

`NoChange` is a rejection: callers must not create any EditorProject operation, revision, or Undo record for it.

## Transition and replay

`AuthoredStackSetReorderTransitionV1` stores an exact protocol version, page ID, normalized selected set, mode, entire before/after stack and both canonical state SHA-256 hashes. `apply_*_forward_v1` and `apply_*_inverse_v1` validate protocol, hashes, recomputed canonical output, normalized membership and current state before returning a new stack. No in-place mutation and no UI lane arithmetic are admitted. Caller enumeration must not change the generated transition.

## Explicit product limitation / incomplete acceptance

This is **only** the source-neutral, compile-isolated planner law. The current `pub-editor::EditOperation`, durable schema version, `EditorSession`, project replay and Undo/Redo are **not** modified by this slice. Therefore this PR alone is not task completion and does not make a multi-selection UI available.

The future `EditorSession` consumer must revalidate each selected NodeId's `AuthorCreated` provenance, page-parent identity and authoritative stack membership, reject source-backed/unknown objects **as a set**, commit exactly one persisted operation and maintain one Undo/Redo unit. The pending Editor v0.32 schema work in PR #2572 must be reconciled before choosing a new on-disk operation version. Imported Publisher z-order and binary PUB serialization remain separate.

## Validation

Targeted hosted CI uses only self-contained NodeId fixtures and the existing Rust authoring-core crate. It checks exhaustive selected subsets of a five-member lane, both relative-order invariants, all four modes, edge no-ops, tampering, stale replay, JSON round trips, malformed lane IDs and existing single-node lifecycle regressions. No private PUB/font, Publisher runtime, data exports or source document bytes are needed.
