#!/usr/bin/env python3
"""Shared source-controlled knowledge bridge for Reader-1050 automation."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
REGISTRY_PATH = (
    REPO_ROOT
    / "vendor"
    / "producer-a"
    / "crates"
    / "pub-reader"
    / "data"
    / "reader_known_evidence_registry.v1.json"
)
LEDGER_PATH = REPO_ROOT / "tools" / "research-runner" / "reader1050_discriminator_ledger.v1.json"

REGISTRY_SCHEMA = "chaptera.reader-known-evidence-registry.v1"
LEDGER_SCHEMA = "chaptera.reader1050-discriminator-ledger.v1"
PROPOSED_LEDGER_ENTRY_SCHEMA = "chaptera.reader1050-discriminator-ledger-entry.v1"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
ALLOWED_KINDS = {"format_owner", "typed_corruption_evidence"}
ALLOWED_ROUTES = {"existing_format_owner", "existing_typed_corruption_evidence"}


def _read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _validate_sha(value: Any, field: str) -> str:
    normalized = str(value or "").lower()
    if not SHA256_RE.fullmatch(normalized):
        raise ValueError(f"{field} must be a lowercase SHA-256: {value!r}")
    return normalized


def load_evidence_registry(path: Path = REGISTRY_PATH) -> dict[str, dict[str, Any]]:
    payload = _read_json(path)
    if payload.get("schema") != REGISTRY_SCHEMA:
        raise ValueError(f"unexpected Reader evidence registry schema: {payload.get('schema')!r}")

    entries: dict[str, dict[str, Any]] = {}
    for raw in payload.get("entries") or []:
        if not isinstance(raw, dict):
            raise ValueError("Reader evidence registry entries must be objects")
        row = dict(raw)
        sha = _validate_sha(row.get("source_sha256"), "source_sha256")
        if sha in entries:
            raise ValueError(f"duplicate Reader evidence registry SHA: {sha}")

        kind = row.get("kind")
        route = row.get("route")
        if kind not in ALLOWED_KINDS:
            raise ValueError(f"unsupported Reader evidence kind for {sha}: {kind!r}")
        if route not in ALLOWED_ROUTES:
            raise ValueError(f"unsupported Reader evidence route for {sha}: {route!r}")
        if not row.get("owner"):
            raise ValueError(f"Reader evidence entry {sha} must declare an owner")

        if kind == "typed_corruption_evidence":
            if route != "existing_typed_corruption_evidence":
                raise ValueError(f"typed corruption entry {sha} has wrong route: {route!r}")
            if not row.get("corruption_evidence"):
                raise ValueError(f"typed corruption entry {sha} lacks corruption_evidence")
            authority = row.get("authority")
            if not isinstance(authority, dict):
                raise ValueError(f"typed corruption entry {sha} lacks authority object")
            _validate_sha(authority.get("control_sha256"), "authority.control_sha256")
            task_ids = authority.get("task_ids") or []
            if not task_ids:
                raise ValueError(f"typed corruption entry {sha} lacks authority task_ids")
            if authority.get("evidence_boundary") != "source-free":
                raise ValueError(f"typed corruption entry {sha} must remain source-free")
        elif row.get("corruption_evidence") is not None:
            raise ValueError(f"format owner entry {sha} cannot declare corruption_evidence")

        row["source_sha256"] = sha
        entries[sha] = row

    return entries


def load_discriminator_ledger(
    path: Path = LEDGER_PATH,
    *,
    registry: dict[str, dict[str, Any]] | None = None,
) -> list[dict[str, Any]]:
    payload = _read_json(path)
    if payload.get("schema") != LEDGER_SCHEMA:
        raise ValueError(f"unexpected Reader discriminator ledger schema: {payload.get('schema')!r}")

    rows: list[dict[str, Any]] = []
    seen: set[tuple[str, str, str]] = set()
    registry = registry if registry is not None else load_evidence_registry()

    for raw in payload.get("entries") or []:
        if not isinstance(raw, dict):
            raise ValueError("Reader discriminator ledger entries must be objects")
        row = dict(raw)
        sha = _validate_sha(row.get("source_sha256"), "ledger.source_sha256")
        discriminator_kind = str(row.get("discriminator_kind") or "")
        run_id = str(row.get("discriminator_run_id") or "")
        if not discriminator_kind or not run_id:
            raise ValueError(f"ledger entry {sha} must declare discriminator_kind and run id")
        key = (sha, discriminator_kind, run_id)
        if key in seen:
            raise ValueError(f"duplicate discriminator ledger entry: {key}")
        seen.add(key)

        status = row.get("status")
        if status not in {"executed", "closed", "handoff", "superseded"}:
            raise ValueError(f"unsupported discriminator ledger status for {sha}: {status!r}")

        superseded_by = row.get("superseded_by")
        if status == "superseded":
            if not isinstance(superseded_by, dict):
                raise ValueError(f"superseded ledger entry {sha} lacks superseded_by")
            registry_sha = _validate_sha(
                superseded_by.get("registry_source_sha256"),
                "superseded_by.registry_source_sha256",
            )
            if registry_sha != sha or registry_sha not in registry:
                raise ValueError(
                    f"superseded ledger entry {sha} does not resolve to current evidence registry"
                )

        row["source_sha256"] = sha
        rows.append(row)

    return rows


def load_runtime_discriminator_cursor(root: Path | None) -> list[dict[str, Any]]:
    if root is None or not root.exists():
        return []

    rows: list[dict[str, Any]] = []
    seen: set[tuple[str, str, str]] = set()
    for path in sorted(root.rglob("ledger-entry.proposed.json")):
        raw = _read_json(path)
        if raw.get("schema") != PROPOSED_LEDGER_ENTRY_SCHEMA:
            raise ValueError(
                f"unexpected proposed discriminator ledger schema at {path}: "
                f"{raw.get('schema')!r}"
            )
        sha = _validate_sha(raw.get("source_sha256"), "runtime.source_sha256")
        discriminator_kind = str(raw.get("discriminator_kind") or "")
        run_id = str(raw.get("discriminator_run_id") or "")
        status = raw.get("status")
        if not discriminator_kind or not run_id:
            raise ValueError(
                f"runtime discriminator entry {path} lacks discriminator kind or run id"
            )
        if status not in {"executed", "closed", "handoff"}:
            raise ValueError(
                f"runtime discriminator entry {path} has unsupported status {status!r}"
            )
        key = (sha, discriminator_kind, run_id)
        if key in seen:
            continue
        seen.add(key)
        row = dict(raw)
        row.pop("schema", None)
        row["source_sha256"] = sha
        row["runtime_cursor_source"] = str(path.relative_to(root))
        rows.append(row)

    return rows


def merge_discriminator_history(
    reviewed: list[dict[str, Any]],
    runtime: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    merged: list[dict[str, Any]] = []
    seen: set[tuple[str, str, str]] = set()
    for row in [*reviewed, *runtime]:
        key = (
            str(row.get("source_sha256") or ""),
            str(row.get("discriminator_kind") or ""),
            str(row.get("discriminator_run_id") or ""),
        )
        if key in seen:
            continue
        seen.add(key)
        merged.append(row)
    return merged


def evidence_for_sha(
    source_sha256: str,
    registry: dict[str, dict[str, Any]] | None = None,
) -> dict[str, Any] | None:
    registry = registry if registry is not None else load_evidence_registry()
    return registry.get(str(source_sha256).lower())


def ledger_for_sha(
    source_sha256: str,
    ledger: list[dict[str, Any]] | None = None,
) -> list[dict[str, Any]]:
    ledger = ledger if ledger is not None else load_discriminator_ledger()
    sha = str(source_sha256).lower()
    return [row for row in ledger if row.get("source_sha256") == sha]


def validate_knowledge_bridge() -> dict[str, int]:
    registry = load_evidence_registry()
    ledger = load_discriminator_ledger(registry=registry)
    return {
        "registry_entries": len(registry),
        "typed_corruption_entries": sum(
            1 for row in registry.values() if row.get("kind") == "typed_corruption_evidence"
        ),
        "format_owner_entries": sum(
            1 for row in registry.values() if row.get("kind") == "format_owner"
        ),
        "ledger_entries": len(ledger),
    }


if __name__ == "__main__":
    print(json.dumps(validate_knowledge_bridge(), indent=2, sort_keys=True))
