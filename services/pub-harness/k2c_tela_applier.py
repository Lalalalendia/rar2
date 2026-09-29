"""Fail-closed core for applying validated K2C patches to derived Tela pages.

This module contains no network transport and no credentials. It models the
bounded v1 write contract used by K2C-TELA-MCP-APPLIER-01:

- existing pages only;
- section-scoped append/prepend/replace operations only;
- preflight every touched page before the first mutation;
- exact space + updated_at freshness checks;
- deterministic idempotency keys;
- lint after every write;
- registry/cursor advancement, when present, must be the final operation;
- source-safe receipts contain hashes/cursors, never markdown bodies.

The live MCP transport is intentionally a separate layer.
"""

from __future__ import annotations

import dataclasses
import hashlib
import json
from collections.abc import Mapping, Sequence
from typing import Any, Protocol

PATCHSET_SCHEMA = "chaptera.k2c-tela-patchset.v1"
RECEIPT_SCHEMA = "chaptera.k2c-tela-apply-receipt.v1"
ALLOWED_OPERATIONS = frozenset({"append", "prepend", "replace"})


class ManifestError(ValueError):
    """Patch manifest violates the bounded v1 contract."""


class PreconditionError(RuntimeError):
    """Live page state does not match the manifest's safe write preconditions."""


class ApplyError(RuntimeError):
    """A mutation or post-write lint check failed."""

    def __init__(self, message: str, receipt: "ApplyReceipt | None" = None) -> None:
        super().__init__(message)
        self.receipt = receipt


@dataclasses.dataclass(frozen=True)
class TelaPageSnapshot:
    page_id: int
    space_id: int
    updated_at: str


@dataclasses.dataclass(frozen=True)
class TelaPatchOutcome:
    page_id: int
    updated_at: str
    idempotent_replay: bool = False


@dataclasses.dataclass(frozen=True)
class TelaLintOutcome:
    page_id: int
    errors: int
    warnings: int


class TelaTransport(Protocol):
    def get_page(self, page_id: int) -> TelaPageSnapshot: ...

    def patch_page(
        self,
        *,
        page_id: int,
        target: str,
        operation: str,
        content: str,
        idempotency_key: str,
    ) -> TelaPatchOutcome: ...

    def lint_page(self, page_id: int) -> TelaLintOutcome: ...


@dataclasses.dataclass(frozen=True)
class TelaPatchOperation:
    page_id: int
    expected_updated_at: str
    target: str
    operation: str
    content: str
    idempotency_key: str
    cursor_advance: bool = False

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "TelaPatchOperation":
        allowed = {field.name for field in dataclasses.fields(cls)}
        unknown = sorted(set(value) - allowed)
        if unknown:
            raise ManifestError(
                "unknown patch operation field(s): " + ", ".join(unknown)
            )
        try:
            return cls(
                page_id=int(value["page_id"]),
                expected_updated_at=str(value["expected_updated_at"]),
                target=str(value["target"]),
                operation=str(value["operation"]),
                content=str(value["content"]),
                idempotency_key=str(value["idempotency_key"]),
                cursor_advance=bool(value.get("cursor_advance", False)),
            )
        except KeyError as exc:
            raise ManifestError(f"missing patch operation field: {exc.args[0]}") from exc

    def source_safe_descriptor(self) -> dict[str, Any]:
        return {
            "page_id": self.page_id,
            "target_sha256": _sha256_text(self.target),
            "operation": self.operation,
            "content_sha256": _sha256_text(self.content),
            "idempotency_key_sha256": _sha256_text(self.idempotency_key),
            "cursor_advance": self.cursor_advance,
        }


@dataclasses.dataclass(frozen=True)
class TelaPatchManifest:
    schema_version: str
    manifest_id: str
    expected_space_id: int
    operations: tuple[TelaPatchOperation, ...]

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "TelaPatchManifest":
        allowed = {
            "schema_version",
            "manifest_id",
            "expected_space_id",
            "operations",
        }
        unknown = sorted(set(value) - allowed)
        if unknown:
            raise ManifestError("unknown manifest field(s): " + ", ".join(unknown))
        try:
            raw_ops = value["operations"]
        except KeyError as exc:
            raise ManifestError("missing manifest field: operations") from exc
        if not isinstance(raw_ops, Sequence) or isinstance(raw_ops, (str, bytes)):
            raise ManifestError("operations must be a JSON array")
        try:
            return cls(
                schema_version=str(value["schema_version"]),
                manifest_id=str(value["manifest_id"]),
                expected_space_id=int(value["expected_space_id"]),
                operations=tuple(
                    TelaPatchOperation.from_dict(item)
                    for item in raw_ops
                    if isinstance(item, Mapping)
                ),
            )
        except KeyError as exc:
            raise ManifestError(f"missing manifest field: {exc.args[0]}") from exc

    def canonical_payload(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "manifest_id": self.manifest_id,
            "expected_space_id": self.expected_space_id,
            "operations": [
                {
                    "page_id": op.page_id,
                    "expected_updated_at": op.expected_updated_at,
                    "target": op.target,
                    "operation": op.operation,
                    "content": op.content,
                    "idempotency_key": op.idempotency_key,
                    "cursor_advance": op.cursor_advance,
                }
                for op in self.operations
            ],
        }

    def manifest_sha256(self) -> str:
        encoded = json.dumps(
            self.canonical_payload(),
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )
        return _sha256_text(encoded)


@dataclasses.dataclass(frozen=True)
class OperationReceipt:
    page_id: int
    target_sha256: str
    operation: str
    content_sha256: str
    idempotency_key_sha256: str
    cursor_advance: bool
    status: str
    before_updated_at: str
    after_updated_at: str | None
    lint_errors: int | None
    lint_warnings: int | None

    def as_dict(self) -> dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class ApplyReceipt:
    schema_version: str
    manifest_id: str
    manifest_sha256: str
    expected_space_id: int
    apply_requested: bool
    completed: bool
    idempotent_manifest_replay: bool
    operations: tuple[OperationReceipt, ...]
    final_page_cursors: tuple[tuple[int, str], ...]
    errors: tuple[str, ...] = ()

    def as_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "manifest_id": self.manifest_id,
            "manifest_sha256": self.manifest_sha256,
            "expected_space_id": self.expected_space_id,
            "apply_requested": self.apply_requested,
            "completed": self.completed,
            "idempotent_manifest_replay": self.idempotent_manifest_replay,
            "operations": [item.as_dict() for item in self.operations],
            "final_page_cursors": {
                str(page_id): updated_at
                for page_id, updated_at in self.final_page_cursors
            },
            "errors": list(self.errors),
        }

    def json_text(self) -> str:
        return json.dumps(self.as_dict(), indent=2, sort_keys=True) + "\n"

    def final_cursor_map(self) -> dict[int, str]:
        return dict(self.final_page_cursors)


def _sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def validate_manifest(
    manifest: TelaPatchManifest,
    *,
    allowed_space_id: int,
) -> None:
    if manifest.schema_version != PATCHSET_SCHEMA:
        raise ManifestError(
            f"unsupported schema_version: {manifest.schema_version!r}"
        )
    if not manifest.manifest_id.strip():
        raise ManifestError("manifest_id is required")
    if manifest.expected_space_id != allowed_space_id:
        raise ManifestError(
            "manifest expected_space_id is not the configured allowed space"
        )
    if not manifest.operations:
        raise ManifestError("at least one patch operation is required")

    seen_keys: set[str] = set()
    cursor_indexes: list[int] = []
    for index, op in enumerate(manifest.operations):
        if op.page_id <= 0:
            raise ManifestError(f"operation {index}: page_id must be positive")
        if not op.expected_updated_at.strip():
            raise ManifestError(
                f"operation {index}: expected_updated_at is required"
            )
        if not op.target.strip():
            raise ManifestError(f"operation {index}: target is required")
        if op.operation not in ALLOWED_OPERATIONS:
            raise ManifestError(
                f"operation {index}: unsupported operation {op.operation!r}"
            )
        if not op.content.strip():
            raise ManifestError(f"operation {index}: content is required")
        if not op.idempotency_key.strip():
            raise ManifestError(
                f"operation {index}: idempotency_key is required"
            )
        if op.idempotency_key in seen_keys:
            raise ManifestError(
                f"operation {index}: duplicate idempotency_key"
            )
        seen_keys.add(op.idempotency_key)
        if op.cursor_advance:
            cursor_indexes.append(index)

    if len(cursor_indexes) > 1:
        raise ManifestError("at most one cursor_advance operation is allowed")
    if cursor_indexes and cursor_indexes[0] != len(manifest.operations) - 1:
        raise ManifestError("cursor_advance operation must be last")


def _replay_pages(
    manifest: TelaPatchManifest,
    snapshots: Mapping[int, TelaPageSnapshot],
    previous_receipt: ApplyReceipt | None,
) -> set[int]:
    if previous_receipt is None:
        return set()
    if not previous_receipt.completed:
        return set()
    if previous_receipt.manifest_sha256 != manifest.manifest_sha256():
        return set()

    previous_cursors = previous_receipt.final_cursor_map()
    replay_pages: set[int] = set()
    for page_id, snapshot in snapshots.items():
        previous = previous_cursors.get(page_id)
        if previous is not None and previous == snapshot.updated_at:
            replay_pages.add(page_id)
    return replay_pages


def _preflight(
    manifest: TelaPatchManifest,
    transport: TelaTransport,
    *,
    allowed_space_id: int,
    previous_receipt: ApplyReceipt | None,
) -> tuple[dict[int, TelaPageSnapshot], set[int]]:
    validate_manifest(manifest, allowed_space_id=allowed_space_id)

    snapshots: dict[int, TelaPageSnapshot] = {}
    for page_id in sorted({op.page_id for op in manifest.operations}):
        snapshot = transport.get_page(page_id)
        if snapshot.page_id != page_id:
            raise PreconditionError(
                f"page identity mismatch: requested {page_id}, got {snapshot.page_id}"
            )
        if snapshot.space_id != allowed_space_id:
            raise PreconditionError(
                f"page {page_id} is in space {snapshot.space_id}, "
                f"expected {allowed_space_id}"
            )
        snapshots[page_id] = snapshot

    replay_pages = _replay_pages(
        manifest,
        snapshots,
        previous_receipt,
    )

    for index, op in enumerate(manifest.operations):
        snapshot = snapshots[op.page_id]
        if op.page_id in replay_pages:
            continue
        if snapshot.updated_at != op.expected_updated_at:
            raise PreconditionError(
                f"operation {index}: stale page {op.page_id}: "
                f"expected {op.expected_updated_at!r}, "
                f"got {snapshot.updated_at!r}"
            )

    return snapshots, replay_pages


def apply_manifest(
    manifest: TelaPatchManifest,
    transport: TelaTransport,
    *,
    allowed_space_id: int,
    apply: bool,
    previous_receipt: ApplyReceipt | None = None,
) -> ApplyReceipt:
    """Preflight and optionally apply one bounded patch manifest.

    All touched pages are fetched and freshness-checked before the first write.
    When a completed previous receipt for the exact same manifest is supplied,
    pages whose current cursor equals the prior final cursor are treated as
    already-applied and skipped. Any other drift fails closed.
    """

    snapshots, replay_pages = _preflight(
        manifest,
        transport,
        allowed_space_id=allowed_space_id,
        previous_receipt=previous_receipt,
    )
    manifest_hash = manifest.manifest_sha256()

    if not apply:
        planned = tuple(
            OperationReceipt(
                **op.source_safe_descriptor(),
                status=(
                    "already_applied"
                    if op.page_id in replay_pages
                    else "planned"
                ),
                before_updated_at=snapshots[op.page_id].updated_at,
                after_updated_at=(
                    snapshots[op.page_id].updated_at
                    if op.page_id in replay_pages
                    else None
                ),
                lint_errors=None,
                lint_warnings=None,
            )
            for op in manifest.operations
        )
        return ApplyReceipt(
            schema_version=RECEIPT_SCHEMA,
            manifest_id=manifest.manifest_id,
            manifest_sha256=manifest_hash,
            expected_space_id=manifest.expected_space_id,
            apply_requested=False,
            completed=True,
            idempotent_manifest_replay=bool(replay_pages),
            operations=planned,
            final_page_cursors=tuple(
                sorted(
                    (page_id, snapshot.updated_at)
                    for page_id, snapshot in snapshots.items()
                )
            ),
        )

    operation_receipts: list[OperationReceipt] = []
    final_cursors = {
        page_id: snapshot.updated_at
        for page_id, snapshot in snapshots.items()
    }

    for op in manifest.operations:
        descriptor = op.source_safe_descriptor()
        before_cursor = final_cursors[op.page_id]

        if op.page_id in replay_pages:
            operation_receipts.append(
                OperationReceipt(
                    **descriptor,
                    status="already_applied",
                    before_updated_at=before_cursor,
                    after_updated_at=before_cursor,
                    lint_errors=0,
                    lint_warnings=0,
                )
            )
            continue

        try:
            outcome = transport.patch_page(
                page_id=op.page_id,
                target=op.target,
                operation=op.operation,
                content=op.content,
                idempotency_key=op.idempotency_key,
            )
        except Exception as exc:  # transport boundary: convert to bounded receipt
            receipt = ApplyReceipt(
                schema_version=RECEIPT_SCHEMA,
                manifest_id=manifest.manifest_id,
                manifest_sha256=manifest_hash,
                expected_space_id=manifest.expected_space_id,
                apply_requested=True,
                completed=False,
                idempotent_manifest_replay=False,
                operations=tuple(operation_receipts),
                final_page_cursors=tuple(sorted(final_cursors.items())),
                errors=(f"patch failed for page {op.page_id}: {type(exc).__name__}",),
            )
            raise ApplyError(
                f"patch failed for page {op.page_id}",
                receipt,
            ) from exc

        if outcome.page_id != op.page_id:
            raise ApplyError(
                f"patch returned wrong page id: {outcome.page_id} != {op.page_id}"
            )

        final_cursors[op.page_id] = outcome.updated_at

        try:
            lint = transport.lint_page(op.page_id)
        except Exception as exc:
            receipt = ApplyReceipt(
                schema_version=RECEIPT_SCHEMA,
                manifest_id=manifest.manifest_id,
                manifest_sha256=manifest_hash,
                expected_space_id=manifest.expected_space_id,
                apply_requested=True,
                completed=False,
                idempotent_manifest_replay=False,
                operations=tuple(operation_receipts),
                final_page_cursors=tuple(sorted(final_cursors.items())),
                errors=(f"lint failed for page {op.page_id}: {type(exc).__name__}",),
            )
            raise ApplyError(
                f"lint failed for page {op.page_id}",
                receipt,
            ) from exc

        op_receipt = OperationReceipt(
            **descriptor,
            status=(
                "idempotent_replay"
                if outcome.idempotent_replay
                else "applied"
            ),
            before_updated_at=before_cursor,
            after_updated_at=outcome.updated_at,
            lint_errors=lint.errors,
            lint_warnings=lint.warnings,
        )
        operation_receipts.append(op_receipt)

        if lint.errors or lint.warnings:
            receipt = ApplyReceipt(
                schema_version=RECEIPT_SCHEMA,
                manifest_id=manifest.manifest_id,
                manifest_sha256=manifest_hash,
                expected_space_id=manifest.expected_space_id,
                apply_requested=True,
                completed=False,
                idempotent_manifest_replay=False,
                operations=tuple(operation_receipts),
                final_page_cursors=tuple(sorted(final_cursors.items())),
                errors=(
                    f"lint rejected page {op.page_id}: "
                    f"errors={lint.errors}, warnings={lint.warnings}",
                ),
            )
            raise ApplyError(
                f"lint rejected page {op.page_id}",
                receipt,
            )

    return ApplyReceipt(
        schema_version=RECEIPT_SCHEMA,
        manifest_id=manifest.manifest_id,
        manifest_sha256=manifest_hash,
        expected_space_id=manifest.expected_space_id,
        apply_requested=True,
        completed=True,
        idempotent_manifest_replay=bool(replay_pages),
        operations=tuple(operation_receipts),
        final_page_cursors=tuple(sorted(final_cursors.items())),
    )
