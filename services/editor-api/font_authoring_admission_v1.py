"""Server-owned font authoring admission for an exact Browser Font Environment.

This is *not* a font resolver, font parser, licence analyzer, or edit command.
A trusted caller supplies explicit complete font bytes plus independently
verified parser and authoring-policy decisions. Browser-sent descriptors, family
names, and fetch handles can never create an authoring grant by themselves.

No file/network/host-font I/O occurs here. On deployments without an approved
resource registry the only valid admission catalog contains zero resources.
"""
from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Iterable, Mapping, Any


PROTOCOL = "chaptera.font-authoring-admission.v1"
ENV_PROTOCOL = "chaptera.font-environment.v1"
_UUID = re.compile(r"[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}\Z")
_SHA = re.compile(r"[0-9a-f]{64}\Z")
MAX_FULL_FONT_BYTES = 32 * 1024 * 1024


class FontAdmissionDenied(ValueError):
    """Publicly reportable *code*, never a path, font bytes, or private policy."""


@dataclass(frozen=True)
class TrustedFontAuthoringResourceV1:
    """Internal registry record, never populated from a client JSON body.

    The parser_verified and authoring_admitted booleans are separate decisions
    made by the trusted provider, not inferred here from the font name, OS/2
    flags, presence on the host, or browser delivery permission.
    """

    tenant_id: str
    document_id: str
    layout_environment_id: str
    font_set_fingerprint: str
    resource_id: str
    font_fingerprint: str
    content_hash: str
    face_index: int
    face_count: int
    full_font_bytes: bytes
    parser_verified: bool
    is_full_resource: bool
    authoring_admitted: bool


def _denied(code: str) -> None:
    raise FontAdmissionDenied(code)


def _identity(value: Mapping[str, Any]) -> tuple[str, str, str, int]:
    resource_id = value.get("resource_id")
    fingerprint = value.get("font_fingerprint")
    content_hash = value.get("content_hash")
    face = value.get("face_index")
    if not isinstance(resource_id, str) or _UUID.fullmatch(resource_id) is None:
        _denied("invalid_font_resource_id")
    if (
        not isinstance(fingerprint, str)
        or not fingerprint.startswith("sha256:")
        or _SHA.fullmatch(fingerprint[7:]) is None
        or not isinstance(content_hash, str)
        or _SHA.fullmatch(content_hash) is None
    ):
        _denied("invalid_font_resource_digest")
    if type(face) is not int or not 0 <= face <= 65535:
        _denied("invalid_font_face")
    return resource_id, fingerprint, content_hash, face


def _scope(scene: Mapping[str, Any], environment: Mapping[str, Any]) -> dict:
    if environment.get("protocol_version") != ENV_PROTOCOL:
        _denied("font_environment_protocol_mismatch")
    layout = scene.get("layout_environment")
    if not isinstance(layout, dict):
        _denied("scene_layout_environment_missing")
    pairs = (
        ("document_id", scene.get("document_id")),
        ("revision_id", scene.get("revision_id")),
        ("scene_snapshot_id", scene.get("snapshot_id")),
        ("layout_environment_id", layout.get("environment_id")),
        ("font_set_fingerprint", layout.get("font_set_fingerprint")),
    )
    scope = {}
    for key, expected in pairs:
        if not isinstance(expected, str) or not expected or environment.get(key) != expected:
            _denied("font_environment_" + key + "_mismatch")
        scope[key] = expected
    if not isinstance(environment.get("fonts"), list):
        _denied("font_environment_resources_missing")
    return scope


def issue_font_authoring_admission_v1(
    *,
    tenant_id: str,
    scene: Mapping[str, Any],
    font_environment: Mapping[str, Any],
    trusted_resources: Iterable[TrustedFontAuthoringResourceV1],
) -> dict:
    """Intersect exact server-delivered resources with independent full-byte grants.

    Every call is bound to the current Scene/revision; no result is cached
    across policy changes. The HTTP caller must separately authorize edit_text
    and obtain the Scene, FontEnvironment and registry from trusted storage.
    Result is an informational picker *catalog*, never a signed edit grant:
    Rust independently checks bytes/policy again on every command/reopen.
    """
    if not isinstance(tenant_id, str) or not tenant_id:
        _denied("tenant_scope_missing")
    scope = _scope(scene, font_environment)
    descriptors = {}
    for item in font_environment["fonts"]:
        if not isinstance(item, dict):
            _denied("invalid_font_environment_descriptor")
        rid = item.get("resource_id")
        if rid is None:
            continue
        key = _identity(item)
        if rid in descriptors:
            _denied("duplicate_delivered_font_resource")
        descriptors[rid] = (key, item)

    registry = {}
    for trusted in trusted_resources:
        if not isinstance(trusted, TrustedFontAuthoringResourceV1):
            _denied("untrusted_font_registry_entry")
        if trusted.tenant_id != tenant_id:
            continue
        if trusted.document_id != scope["document_id"]:
            continue
        if (
            trusted.layout_environment_id != scope["layout_environment_id"]
            or trusted.font_set_fingerprint != scope["font_set_fingerprint"]
        ):
            _denied("trusted_font_registry_environment_mismatch")
        if trusted.resource_id in registry:
            _denied("duplicate_trusted_font_resource")
        registry[trusted.resource_id] = trusted

    admitted = []
    for rid, (identity, descriptor) in sorted(descriptors.items()):
        if descriptor.get("delivery") != "deliver_exact":
            continue
        if not isinstance(descriptor.get("fetch_handle"), str) or not descriptor["fetch_handle"]:
            _denied("exact_font_delivery_handle_missing")
        trusted = registry.get(rid)
        if trusted is None or not trusted.authoring_admitted:
            continue
        trusted_identity = _identity({
            "resource_id": trusted.resource_id,
            "font_fingerprint": trusted.font_fingerprint,
            "content_hash": trusted.content_hash,
            "face_index": trusted.face_index,
        })
        if identity != trusted_identity:
            _denied("trusted_font_resource_identity_mismatch")
        if not trusted.parser_verified or not trusted.is_full_resource:
            continue
        if (
            type(trusted.face_count) is not int
            or trusted.face_count <= 0
            or trusted.face_index >= trusted.face_count
        ):
            _denied("trusted_font_face_not_admitted")
        data = trusted.full_font_bytes
        if not isinstance(data, bytes) or not 0 < len(data) <= MAX_FULL_FONT_BYTES:
            _denied("trusted_full_font_bytes_missing")
        if hashlib.sha256(data).hexdigest() != trusted.content_hash:
            _denied("trusted_font_bytes_hash_mismatch")
        admitted.append({
            "resource_id": rid,
            "font_fingerprint": trusted.font_fingerprint,
            "content_hash": trusted.content_hash,
            "face_index": trusted.face_index,
        })

    return {"protocol_version": PROTOCOL, **scope, "resources": admitted}
