"""Authenticated font-resource intent and canonical Rust-operation guard.

This module is *not* a font resolver or a layout engine. A browser
candidate is never independent authoring authority. Only an executor with
a current server-owned resource grant may produce the canonical operation.
"""
from __future__ import annotations

import re

from revision_store import RevisionKernel

PROTOCOL = "chaptera.font-resource-intent.v1"
CANDIDATE_PROTOCOL = "chaptera.font-replacement-candidate.v1"
AUTHORITY = "candidate_only_server_validation_required"
UUID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")
SHA_ID = re.compile(r"sha256:[0-9a-f]{64}\Z")
CANDIDATE_FIELDS = {
    "protocol_version", "document_id", "expected_revision_id",
    "scene_snapshot_id", "layout_environment_id", "font_set_fingerprint",
    "resource_id", "font_fingerprint", "content_hash", "face_index",
    "authority",
}
COMMAND_FIELDS = {"kind", "story_id", "start_scalar", "end_scalar", "candidate"}
REQUEST_FIELDS = {
    "protocol_version", "document_id", "source_hash", "base_revision_id",
    "client_operation_id", "command",
}
OP_FIELDS = {
    "kind", "story_id", "start_scalar", "end_scalar", "property",
    "value", "before_state_hash", "after_state_hash",
}
IDENTITY_FIELDS = {"resource_id", "font_fingerprint", "content_hash", "face_index"}


def _require_exact_keys(value: object, keys: set[str], label: str) -> dict:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{label} has noncanonical or authority-bearing fields")
    return value


def _require_hex(value: object, pattern: re.Pattern, label: str) -> None:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        raise ValueError(f"{label} has invalid canonical identity")


def validate_font_intent_v1(request: dict) -> None:
    request = _require_exact_keys(request, REQUEST_FIELDS, "font request")
    if request["protocol_version"] != PROTOCOL:
        raise ValueError("unsupported font intent protocol")
    _require_hex(request["document_id"], UUID, "document_id")
    _require_hex(request["source_hash"], SHA, "source_hash")
    _require_hex(request["base_revision_id"], SHA_ID, "base_revision_id")
    if (not isinstance(request["client_operation_id"], str)
            or not 8 <= len(request["client_operation_id"]) <= 192):
        raise ValueError("font client_operation_id is invalid")
    command = _require_exact_keys(request["command"], COMMAND_FIELDS, "font command")
    if command["kind"] != "set_admitted_font_resource":
        raise ValueError("font intent requires set_admitted_font_resource")
    _require_hex(command["story_id"], UUID, "story_id")
    start, end = command["start_scalar"], command["end_scalar"]
    if (type(start) is not int or type(end) is not int
            or start < 0 or end <= start or end > 0xFFFFFFFF):
        raise ValueError("font command requires a nonempty Unicode scalar range")
    candidate = _require_exact_keys(command["candidate"], CANDIDATE_FIELDS, "font candidate")
    if (candidate["protocol_version"] != CANDIDATE_PROTOCOL
            or candidate["authority"] != AUTHORITY):
        raise ValueError("font candidate must be explicitly untrusted")
    if (candidate["document_id"] != request["document_id"]
            or candidate["expected_revision_id"] != request["base_revision_id"]):
        raise ValueError("font candidate is not bound to exact request document/revision")
    _require_hex(candidate["scene_snapshot_id"], SHA_ID, "scene_snapshot_id")
    _require_hex(candidate["layout_environment_id"], SHA_ID, "layout_environment_id")
    _require_hex(candidate["font_set_fingerprint"], SHA_ID, "font_set_fingerprint")
    _require_hex(candidate["resource_id"], UUID, "resource_id")
    _require_hex(candidate["font_fingerprint"], SHA_ID, "font_fingerprint")
    _require_hex(candidate["content_hash"], SHA, "content_hash")
    face = candidate["face_index"]
    if type(face) is not int or not 0 <= face <= 65535:
        raise ValueError("font face index is invalid")


def validate_font_operation_v1(command: dict, operation: dict) -> None:
    op = _require_exact_keys(operation, OP_FIELDS, "Rust font operation")
    if op["kind"] != "set_text_format_property" or op["property"] != "font_resource":
        raise ValueError("only canonical Rust FontResource property can be committed")
    for field in ("story_id", "start_scalar", "end_scalar"):
        if op[field] != command[field]:
            raise ValueError(f"Rust font operation target mismatch: {field}")
    identity = _require_exact_keys(op["value"], IDENTITY_FIELDS, "font resource identity")
    for field in IDENTITY_FIELDS:
        if identity[field] != command["candidate"][field]:
            raise ValueError(f"Rust font identity does not match intent: {field}")
    for field in ("before_state_hash", "after_state_hash"):
        _require_hex(op[field], SHA_ID, field)
    if op["before_state_hash"] == op["after_state_hash"]:
        raise ValueError("font no-op is not an accepted edit")


def validate_server_font_scope_v1(command: dict, scene: dict, admission: dict) -> None:
    """Check independent current Scene and server-issued grant; no browser trust."""
    candidate = command["candidate"]
    expected = {
        "document_id": scene["document_id"],
        "expected_revision_id": scene["revision_id"],
        "scene_snapshot_id": scene["snapshot_id"],
        "layout_environment_id": scene["layout_environment"]["environment_id"],
        "font_set_fingerprint": scene["layout_environment"]["font_set_fingerprint"],
    }
    for key, current in expected.items():
        if candidate[key] != current:
            raise ValueError(f"stale or forged font candidate {key}")
    if admission.get("protocol_version") != "chaptera.font-authoring-admission.v1":
        raise ValueError("independent current font authoring admission is required")
    for field, key in (
        ("document_id", "document_id"), ("revision_id", "expected_revision_id"),
        ("scene_snapshot_id", "scene_snapshot_id"),
        ("layout_environment_id", "layout_environment_id"),
        ("font_set_fingerprint", "font_set_fingerprint"),
    ):
        if admission.get(field) != candidate[key]:
            raise ValueError(f"server font authoring scope mismatch: {field}")
    grants = admission.get("resources")
    if not isinstance(grants, list) or len(grants) != 1:
        raise ValueError("exactly one independently admitted font is required")
    grant = _require_exact_keys(grants[0], IDENTITY_FIELDS, "trusted font grant")
    for key in IDENTITY_FIELDS:
        if grant[key] != candidate[key]:
            raise ValueError(f"trusted font grant mismatch: {key}")


class FontRevisionKernel(RevisionKernel):
    """Use the existing immutable/idempotent transaction machinery unchanged."""

    def commit_admitted_font_resource(self, request: dict, executor) -> dict:
        return self._commit_command(
            request,
            executor,
            request_validator=validate_font_intent_v1,
            canonical_validator=validate_font_operation_v1,
        )
