#!/usr/bin/env python3
"""Deterministic source-safe planner for Chaptera knowledge→code reconciliation.

This module does not call Notion, GitHub, or Tela. It consumes normalized,
public-safe snapshots produced by existing authority adapters and emits the
minimum derived-graph action required by the K0–K5 bridge contract.

Canonical authority remains upstream:
- Notion: semantic/task/productization authority;
- GitHub: live issue/PR/code authority;
- Tela: derived graph only.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
from pathlib import Path
from typing import Any, Iterable

VALID_CLASSIFICATIONS = {
    "KNOWN + CONSUMED",
    "KNOWN + NOT CONSUMED",
    "PARTLY KNOWN + CONSUMED",
    "OPEN FORMAT / DOMAIN",
    "DUPLICATE / STALE",
}

VALID_OWNER_CHECKS = {
    "not_applicable",
    "unique",
    "valid",
    "valid_issue_pr_pair",
}


@dataclasses.dataclass(frozen=True)
class SurfaceState:
    surface_id: str
    notion_page_id: str
    notion_edited_at: str
    github_cursor: str
    semantic_fingerprint: str
    implementation_fingerprint: str
    acceptance_fingerprint: str
    owner_fingerprint: str
    product_fingerprint: str
    classification: str
    acceptance_closed: bool
    live_implementation_owner: str | None
    code_anchor: str | None
    code_anchor_exists: bool = True
    authority_link_ok: bool = True
    owner_check: str = "not_applicable"
    dependent_surface_ids: tuple[str, ...] = ()

    @classmethod
    def from_dict(cls, value: dict[str, Any]) -> "SurfaceState":
        known = {field.name for field in dataclasses.fields(cls)}
        unknown = sorted(set(value) - known)
        if unknown:
            raise ValueError(f"unknown SurfaceState field(s): {', '.join(unknown)}")
        normalized = dict(value)
        normalized["dependent_surface_ids"] = tuple(
            str(item) for item in value.get("dependent_surface_ids", ())
        )
        return cls(**normalized)

    def cursor(self) -> dict[str, str]:
        return {
            "notion_edited_at": self.notion_edited_at,
            "github_cursor": self.github_cursor,
        }


@dataclasses.dataclass(frozen=True)
class ReconcilePlan:
    schema_version: str
    surface_id: str
    change_class: str
    semantic_write: bool
    cursor_write: bool
    affected_surface_ids: tuple[str, ...]
    errors: tuple[str, ...]
    before_cursor: dict[str, str]
    current_cursor: dict[str, str]

    def as_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "surface_id": self.surface_id,
            "change_class": self.change_class,
            "semantic_write": self.semantic_write,
            "cursor_write": self.cursor_write,
            "affected_surface_ids": list(self.affected_surface_ids),
            "errors": list(self.errors),
            "before_cursor": self.before_cursor,
            "current_cursor": self.current_cursor,
        }

    def json_text(self) -> str:
        return json.dumps(self.as_dict(), indent=2, sort_keys=True) + "\n"


def _present(value: str | None) -> bool:
    return bool(value and value.strip())


def validate_surface(state: SurfaceState) -> list[str]:
    errors: list[str] = []

    if not _present(state.surface_id):
        errors.append("surface_id is required")
    if not _present(state.notion_page_id):
        errors.append("canonical Notion authority is required")
    if not state.authority_link_ok:
        errors.append("canonical Notion authority link is missing or mismatched")
    if state.classification not in VALID_CLASSIFICATIONS:
        errors.append(f"unsupported classification: {state.classification}")
    if not _present(state.semantic_fingerprint):
        errors.append("semantic_fingerprint is required")
    if state.owner_check not in VALID_OWNER_CHECKS:
        errors.append(f"owner uniqueness check is not clean: {state.owner_check}")

    known_surface = state.classification in {
        "KNOWN + CONSUMED",
        "KNOWN + NOT CONSUMED",
        "PARTLY KNOWN + CONSUMED",
    }
    if known_surface and not _present(state.code_anchor):
        errors.append("known surface requires a registered code anchor")
    if _present(state.code_anchor) and not state.code_anchor_exists:
        errors.append(f"registered code anchor is missing: {state.code_anchor}")

    if state.classification == "KNOWN + CONSUMED":
        if not state.acceptance_closed:
            errors.append("KNOWN + CONSUMED requires closed acceptance")
        if _present(state.live_implementation_owner):
            errors.append(
                "KNOWN + CONSUMED must not retain a live implementation owner"
            )

    if (
        state.classification == "OPEN FORMAT / DOMAIN"
        and state.acceptance_closed
    ):
        errors.append("OPEN FORMAT / DOMAIN cannot have closed product acceptance")

    return errors


def classify_change(before: SurfaceState, current: SurfaceState) -> str:
    if before.surface_id != current.surface_id:
        raise ValueError(
            f"surface identity changed: {before.surface_id!r} -> {current.surface_id!r}"
        )

    if (
        before.semantic_fingerprint != current.semantic_fingerprint
        or before.product_fingerprint != current.product_fingerprint
    ):
        return "K5"

    if (
        before.owner_fingerprint != current.owner_fingerprint
        or before.live_implementation_owner != current.live_implementation_owner
    ):
        return "K4"

    if (
        before.acceptance_fingerprint != current.acceptance_fingerprint
        or before.acceptance_closed != current.acceptance_closed
        or before.classification != current.classification
    ):
        return "K3"

    if (
        before.implementation_fingerprint != current.implementation_fingerprint
        or before.code_anchor != current.code_anchor
        or before.code_anchor_exists != current.code_anchor_exists
    ):
        return "K2"

    if before.cursor() != current.cursor():
        return "K1"

    return "K0"


def _affected_surfaces(
    current: SurfaceState,
    change_class: str,
) -> tuple[str, ...]:
    if change_class in {"K0", "K1"}:
        return ()
    values: set[str] = {current.surface_id}
    if change_class == "K5":
        values.update(current.dependent_surface_ids)
    return tuple(sorted(values))


def plan_reconciliation(
    before: SurfaceState,
    current: SurfaceState,
) -> ReconcilePlan:
    if before.surface_id != current.surface_id:
        errors = (
            f"surface identity changed: {before.surface_id!r} -> {current.surface_id!r}",
        )
        return ReconcilePlan(
            schema_version="chaptera.k2c-reconcile-plan.v1",
            surface_id=current.surface_id,
            change_class="INVALID",
            semantic_write=False,
            cursor_write=False,
            affected_surface_ids=(),
            errors=errors,
            before_cursor=before.cursor(),
            current_cursor=current.cursor(),
        )

    errors = tuple(sorted(set(validate_surface(current))))
    if errors:
        return ReconcilePlan(
            schema_version="chaptera.k2c-reconcile-plan.v1",
            surface_id=current.surface_id,
            change_class="INVALID",
            semantic_write=False,
            cursor_write=False,
            affected_surface_ids=(),
            errors=errors,
            before_cursor=before.cursor(),
            current_cursor=current.cursor(),
        )

    change_class = classify_change(before, current)
    return ReconcilePlan(
        schema_version="chaptera.k2c-reconcile-plan.v1",
        surface_id=current.surface_id,
        change_class=change_class,
        semantic_write=change_class in {"K2", "K3", "K4", "K5"},
        cursor_write=change_class != "K0",
        affected_surface_ids=_affected_surfaces(current, change_class),
        errors=(),
        before_cursor=before.cursor(),
        current_cursor=current.cursor(),
    )


def write_receipt(path: Path, plan: ReconcilePlan) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(plan.json_text(), encoding="utf-8")


def load_state(path: Path) -> SurfaceState:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path}: expected a JSON object")
    return SurfaceState.from_dict(value)


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Plan one Chaptera knowledge→code reconciliation without external writes."
    )
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    before = load_state(args.before)
    current = load_state(args.current)
    plan = plan_reconciliation(before, current)
    write_receipt(args.receipt, plan)
    print(plan.json_text(), end="")
    return 2 if plan.errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
