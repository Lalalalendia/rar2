"""Opt-in real OpenType delivery for task-local Chaptera Editor.

The server owns the exact bytes, delivery decision, and authoring policy.
Only a developer-enabled, pinned OFL witness is supported. A BrowserFontEnvironment
is descriptive and cannot authorize any edit or fixed-output operation.
"""
from __future__ import annotations

import copy
import hashlib
import json

from scene_v1 import finalize_snapshot, hash_id
from font_authoring_admission_v1 import issue_font_authoring_admission_v1
from pinned_opentype_resource_v1 import (
    ABEL_RESOURCE_ID, ABEL_SHA256, PinnedAbelResource,
)

FONT_ENVIRONMENT_PROTOCOL = "chaptera.font-environment.v1"
def current_fetch_handle(scene: dict) -> str:
    """Opaque, nonauthoritative handle scoped to exact current Scene."""
    payload = {
        "revision_id": scene["revision_id"],
        "scene_snapshot_id": scene["snapshot_id"],
        "resource_id": ABEL_RESOURCE_ID,
        "content_hash": ABEL_SHA256,
    }
    data = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return "abel_" + hashlib.sha256(data).hexdigest()[:40]


class LiveFontDeliveryDenied(ValueError):
    pass


def bind_font_set_to_scene(scene: dict, resource: PinnedAbelResource | None) -> dict:
    """Scope optional physical font-set to the actual Scene snapshot.

    The Viewer geometry remains unchanged and is NOT promoted to shaped text.
    Existing scenes are byte-for-byte unchanged without the opt-in resource.
    """
    if resource is None:
        return scene
    bound = copy.deepcopy(scene)
    layout = bound["layout_environment"]
    original_environment_id = layout["environment_id"]
    original_font_set = layout["font_set_fingerprint"]
    layout["font_set_fingerprint"] = hash_id({
        "protocol_version": "chaptera.explicit-test-font-set.v1",
        "source_font_set_fingerprint": original_font_set,
        "resources": [{
            "resource_id": ABEL_RESOURCE_ID,
            "content_hash": ABEL_SHA256,
            "font_fingerprint": "sha256:" + ABEL_SHA256,
            "face_index": 0,
        }],
    })
    layout["environment_id"] = hash_id({
        "protocol_version": "chaptera.explicit-test-layout-environment.v1",
        "source_layout_environment_id": original_environment_id,
        "font_set_fingerprint": layout["font_set_fingerprint"],
    })
    # Changing even authoring-only available resource scope invalidates the
    # previous Scene snapshot, so candidate admission is revision/snapshot-safe.
    return finalize_snapshot(bound)


def build_font_environment(scene: dict, resource: PinnedAbelResource | None) -> dict:
    layout = scene["layout_environment"]
    fonts = []
    if resource is not None:
        # This is an exact server-owned route, not a host family/path lookup.
        fonts.append({
            "resource_id": ABEL_RESOURCE_ID,
            "font_fingerprint": "sha256:" + ABEL_SHA256,
            "content_hash": ABEL_SHA256,
            "face_index": 0,
            "family": "Abel",
            "style": "Regular",
            "delivery": "deliver_exact",
            "reason_code": "delivery.allowed",
            "fetch_handle": current_fetch_handle(scene),
            "fallback": None,
        })
    return {
        "protocol_version": FONT_ENVIRONMENT_PROTOCOL,
        "document_id": scene["document_id"],
        "revision_id": scene["revision_id"],
        "scene_snapshot_id": scene["snapshot_id"],
        "layout_environment_id": layout["environment_id"],
        "font_set_fingerprint": layout["font_set_fingerprint"],
        "preview_authority": "server_frame_geometry_only",
        "fonts": fonts,
        "diagnostics": [],
    }


def issue_current_admission(
    *, scene: dict, tenant_id: str, resource: PinnedAbelResource | None,
) -> dict:
    environment = build_font_environment(scene, resource)
    trusted = ()
    if resource is not None:
        trusted = (resource.trusted(
            tenant_id=tenant_id,
            document_id=scene["document_id"],
            layout_environment_id=environment["layout_environment_id"],
            font_set_fingerprint=environment["font_set_fingerprint"],
        ),)
    return issue_font_authoring_admission_v1(
        tenant_id=tenant_id, scene=scene,
        font_environment=environment, trusted_resources=trusted,
    )


def read_current_exact_font(
    *, scene: dict, tenant_id: str, resource: PinnedAbelResource | None,
    resource_id: str, revision_id: str, snapshot_id: str,
    fetch_handle: str | None = None,
) -> bytes:
    if resource is None or resource_id != ABEL_RESOURCE_ID:
        raise LiveFontDeliveryDenied("font_resource_not_available")
    if revision_id != scene["revision_id"] or snapshot_id != scene["snapshot_id"]:
        raise LiveFontDeliveryDenied("font_delivery_stale_scene")
    if fetch_handle is not None and fetch_handle != current_fetch_handle(scene):
        raise LiveFontDeliveryDenied("font_delivery_stale_handle")
    # Require independent current-policy authoring admission even for binary
    # delivery: an old picker/handle cannot bypass its scope.
    admitted = issue_current_admission(
        scene=scene, tenant_id=tenant_id, resource=resource,
    )
    if not any(grant["resource_id"] == resource_id and
               grant["content_hash"] == ABEL_SHA256 and
               grant["face_index"] == 0 for grant in admitted["resources"]):
        raise LiveFontDeliveryDenied("font_delivery_not_admitted")
    if hashlib.sha256(resource.raw).hexdigest() != ABEL_SHA256:
        raise LiveFontDeliveryDenied("font_delivery_bytes_changed")
    return resource.raw
