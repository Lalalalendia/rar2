"""Deliberately bounded actual-PUB font authoring bridge.

A canonical Rust FontResource edit can be saved before an authoritative
layout/PDF consumer exists, but all geometry must remain visibly Partial.
This is opt-in developer scope only. No browser font metrics or family
names become document geometry/output authority.
"""
from __future__ import annotations

import copy
import json
import pathlib
import re
import subprocess
import tempfile

from font_resource_intent_v1 import IDENTITY_FIELDS, OP_FIELDS
from scene_v1 import finalize_snapshot

FONT_PARTIAL_REASON = "font_resource_authoring_present_layout_not_reshaped"
PINNED_CLI_PROTOCOLS = {
    "font-initialize": "chaptera.pinned-font-project-init.v1",
    "font-capabilities": "chaptera.pinned-font-editor-capabilities.v1",
    "font-apply": "chaptera.pinned-font-operation-result.v1",
    "font-verify": "chaptera.pinned-font-project-verified.v1",
}
HEX64 = re.compile(r"[0-9a-f]{64}\Z")


def contains_font_history(project: dict) -> bool:
    operations = project.get("operations")
    if not isinstance(operations, list):
        raise ValueError("EditorProject requires canonical operation list")
    found = False
    for index, operation in enumerate(operations):
        if not isinstance(operation, dict):
            raise ValueError(f"EditorProject operation[{index}] is not an object")
        if (operation.get("kind") != "set_text_format_property"
                or operation.get("property") != "font_resource"):
            continue
        if (set(operation) != OP_FIELDS
                or not isinstance(operation.get("value"), dict)
                or set(operation["value"]) != IDENTITY_FIELDS):
            raise ValueError("recorded FontResource is not canonical")
        if (not all(isinstance(operation.get(key), str)
                    and operation[key].startswith("sha256:")
                    and HEX64.fullmatch(operation[key][7:])
                    for key in ("before_state_hash", "after_state_hash"))
                or operation["before_state_hash"] == operation["after_state_hash"]):
            raise ValueError("recorded FontResource state receipts are invalid")
        found = True
    return found


def unshaped_scene_project(project: dict) -> dict:
    """Project only already-proven Scene operations; keep format in Rust history."""
    copied = copy.deepcopy(project)
    contains_font_history(copied)
    copied["operations"] = [
        op for op in copied["operations"]
        if not (op.get("kind") == "set_text_format_property"
                and op.get("property") == "font_resource")
    ]
    return copied


def partial_font_scene(scene: dict) -> dict:
    copied = copy.deepcopy(scene)
    fidelity = copied["fidelity"]
    fidelity["state"] = "partial"
    reasons = fidelity.setdefault("reasons", [])
    if FONT_PARTIAL_REASON not in reasons:
        reasons.append(FONT_PARTIAL_REASON)
    return finalize_snapshot(copied)


def blocked_native_pub_preview(*, document_id: str, source_hash: str,
                               revision_id: str) -> dict:
    return {
        "protocol_version": "chaptera.native-pub-save-preview.v1",
        "document_id": document_id,
        "source_hash": source_hash,
        "revision_id": revision_id,
        "can_serialize": False,
        "can_download": False,
        "native_publisher_authorized": False,
        "download_blocker_code": "font_resource_native_pub_output_not_admitted",
        "blocker_code": "font_resource_native_pub_output_not_admitted",
        "output_hash": None,
        "byte_len": None,
        "chaptera_reopen_verified": False,
        "native_publisher_acceptance": "not_evaluated",
    }


def pinned_font_cli(
    *, mode: str, cli: pathlib.Path, source: pathlib.Path,
    work_dir: pathlib.Path, project: dict | None = None,
    command: dict | None = None, scope: dict | None = None,
) -> dict:
    expected = PINNED_CLI_PROTOCOLS.get(mode)
    if expected is None or not cli.is_file():
        raise ValueError("pinned font authoring CLI unavailable")
    with tempfile.TemporaryDirectory(prefix="chaptera-font-", dir=work_dir) as tmp:
        root = pathlib.Path(tmp)
        args = [str(cli), mode, str(source)]
        for name, value in (("project", project), ("command", command), ("scope", scope)):
            if value is not None:
                path = root / f"{name}.json"
                path.write_text(
                    json.dumps(value, ensure_ascii=False, sort_keys=True),
                    encoding="utf-8",
                )
                args.append(str(path))
        completed = subprocess.run(
            args, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, check=False,
        )
        if completed.returncode != 0:
            # Rust stderr can include private document text/paths. Do not
            # propagate it into HTTP responses, logs or the browser.
            raise ValueError("pinned font operation rejected by canonical Rust")
        try:
            receipt = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise ValueError("pinned font Rust receipt is invalid") from error
        if (not isinstance(receipt, dict)
                or receipt.get("protocol_version") != expected):
            raise ValueError("pinned font Rust receipt protocol mismatch")
        return receipt
