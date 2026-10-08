#!/usr/bin/env python3
"""Emit a deterministic source-safe health receipt for the K2C planner.

The receipt uses synthetic registered-surface snapshots only. It proves the
planner's K0-K5/fail-closed contract without reading Notion bodies, GitHub
content, Tela state, PUB bytes, credentials, or customer data.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
from pathlib import Path
from typing import Any, Iterable

from k2c_reconcile import SurfaceState, plan_reconciliation


def base_state(**changes: Any) -> SurfaceState:
    value = SurfaceState(
        surface_id="reader/picture91-crop/v1",
        notion_page_id="3ea32a84beec8199bc98f9d34aa1c862",
        notion_edited_at="2026-09-29T10:00:00Z",
        github_cursor="issue#12@2026-09-29T10:00:00Z",
        semantic_fingerprint="semantic-v1",
        implementation_fingerprint="impl-v1",
        acceptance_fingerprint="accept-v1",
        owner_fingerprint="owner-v1",
        product_fingerprint="product-v1",
        classification="KNOWN + NOT CONSUMED",
        acceptance_closed=False,
        live_implementation_owner="rar2#12",
        code_anchor=(
            "vendor/producer-a/crates/pub-reader/src/resolve.rs"
            "::explicit_image_crop"
        ),
        code_anchor_exists=True,
        authority_link_ok=True,
        owner_check="unique",
        dependent_surface_ids=(),
    )
    return dataclasses.replace(value, **changes)


def _scenario(
    before: SurfaceState,
    current: SurfaceState,
    *,
    expected_class: str,
    expected_semantic_write: bool,
    expected_cursor_write: bool,
    expected_error_fragment: str | None = None,
) -> dict[str, Any]:
    plan = plan_reconciliation(before, current)
    if plan.change_class != expected_class:
        raise AssertionError(
            f"expected {expected_class}, got {plan.change_class}: {plan.errors}"
        )
    if plan.semantic_write != expected_semantic_write:
        raise AssertionError(
            f"{expected_class}: semantic_write={plan.semantic_write}, "
            f"expected {expected_semantic_write}"
        )
    if plan.cursor_write != expected_cursor_write:
        raise AssertionError(
            f"{expected_class}: cursor_write={plan.cursor_write}, "
            f"expected {expected_cursor_write}"
        )
    if expected_error_fragment is None:
        if plan.errors:
            raise AssertionError(f"{expected_class}: unexpected errors: {plan.errors}")
    elif not any(expected_error_fragment in item for item in plan.errors):
        raise AssertionError(
            f"{expected_class}: missing error fragment "
            f"{expected_error_fragment!r}: {plan.errors}"
        )
    return {
        "change_class": plan.change_class,
        "semantic_write": plan.semantic_write,
        "cursor_write": plan.cursor_write,
        "affected_surface_ids": list(plan.affected_surface_ids),
        "errors": list(plan.errors),
    }


def build_health_receipt(source_head: str) -> dict[str, Any]:
    k0_before = base_state()
    k0_current = base_state()

    k1_before = base_state()
    k1_current = base_state(
        github_cursor="issue#12@2026-09-29T10:01:00Z",
    )

    k2_before = base_state(
        code_anchor=(
            "vendor/producer-a/crates/pub-model/src/resolved_graph.rs"
            "::explicit_image_crop"
        )
    )
    k2_current = base_state(
        implementation_fingerprint="impl-v2",
        code_anchor=(
            "vendor/producer-a/crates/pub-reader/src/resolve.rs"
            "::explicit_image_crop"
        ),
    )

    legacy_owner_before = base_state(
        surface_id="reader/legacy22-noquill/v1",
        notion_page_id="3e932a84beec81ba91c1c54d1538b280",
        code_anchor=(
            "vendor/producer-a/crates/pub-reader/src/legacy22_noquill_graph.rs"
            "::LEGACY_TEXT_SHAPE_TYPE"
        ),
        live_implementation_owner="rar2#17",
        owner_fingerprint="owner-issue-17",
        acceptance_fingerprint="accept-pending",
        classification="KNOWN + NOT CONSUMED",
        acceptance_closed=False,
    )
    legacy_owner_closed = dataclasses.replace(
        legacy_owner_before,
        github_cursor="main@ec1a0dc9",
        live_implementation_owner=None,
        owner_fingerprint="owner-none-landed",
    )
    legacy_accepted = dataclasses.replace(
        legacy_owner_closed,
        github_cursor="main@97c2d610;run36565564426",
        acceptance_fingerprint="accept-run-36565564426-green",
        classification="KNOWN + CONSUMED",
        acceptance_closed=True,
    )

    k5_before = base_state()
    k5_current = base_state(
        semantic_fingerprint="semantic-v2",
        dependent_surface_ids=(
            "reader/closure-map/v1",
            "reader/table/v1",
        ),
    )

    invalid_authority = base_state(
        notion_page_id="",
        authority_link_ok=False,
    )
    invalid_owner = base_state(owner_check="ambiguous")
    invalid_anchor = base_state(code_anchor_exists=False)
    invalid_consumed = base_state(
        classification="KNOWN + CONSUMED",
        acceptance_closed=False,
        live_implementation_owner="rar2#12",
    )

    scenarios = {
        "k0_no_write": _scenario(
            k0_before,
            k0_current,
            expected_class="K0",
            expected_semantic_write=False,
            expected_cursor_write=False,
        ),
        "k1_cursor_only": _scenario(
            k1_before,
            k1_current,
            expected_class="K1",
            expected_semantic_write=False,
            expected_cursor_write=True,
        ),
        "k2_picture91_locator_repair": _scenario(
            k2_before,
            k2_current,
            expected_class="K2",
            expected_semantic_write=True,
            expected_cursor_write=True,
        ),
        "k4_legacy_owner_closure": _scenario(
            legacy_owner_before,
            legacy_owner_closed,
            expected_class="K4",
            expected_semantic_write=True,
            expected_cursor_write=True,
        ),
        "k3_legacy_acceptance_closure": _scenario(
            legacy_owner_closed,
            legacy_accepted,
            expected_class="K3",
            expected_semantic_write=True,
            expected_cursor_write=True,
        ),
        "k5_registered_fanout": _scenario(
            k5_before,
            k5_current,
            expected_class="K5",
            expected_semantic_write=True,
            expected_cursor_write=True,
        ),
        "invalid_missing_authority": _scenario(
            base_state(),
            invalid_authority,
            expected_class="INVALID",
            expected_semantic_write=False,
            expected_cursor_write=False,
            expected_error_fragment="authority",
        ),
        "invalid_owner_ambiguity": _scenario(
            base_state(),
            invalid_owner,
            expected_class="INVALID",
            expected_semantic_write=False,
            expected_cursor_write=False,
            expected_error_fragment="owner uniqueness",
        ),
        "invalid_missing_code_anchor": _scenario(
            base_state(),
            invalid_anchor,
            expected_class="INVALID",
            expected_semantic_write=False,
            expected_cursor_write=False,
            expected_error_fragment="code anchor",
        ),
        "invalid_consumed_without_acceptance": _scenario(
            base_state(),
            invalid_consumed,
            expected_class="INVALID",
            expected_semantic_write=False,
            expected_cursor_write=False,
            expected_error_fragment="closed acceptance",
        ),
    }

    # Explicit deterministic/idempotent check for the K0 receipt.
    first = plan_reconciliation(k0_before, k0_current).json_text()
    second = plan_reconciliation(k0_before, k0_current).json_text()
    if first != second:
        raise AssertionError("K0 reconciliation receipt is not byte-stable")

    return {
        "schema_version": "chaptera.k2c-health-receipt.v1",
        "planner_schema_version": "chaptera.k2c-reconcile-plan.v1",
        "source_head": source_head,
        "source_safe": True,
        "external_writes": False,
        "k0_byte_stable": True,
        "scenario_count": len(scenarios),
        "scenarios": scenarios,
    }


def write_receipt(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Emit source-safe K2C reconciliation health receipt."
    )
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    receipt = build_health_receipt(args.source_head)
    write_receipt(args.output, receipt)
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
