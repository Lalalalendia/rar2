#!/usr/bin/env python3
"""Run and independently verify EDITOR-DESKTOP-CONTINUITY-V2-01."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import os
import pathlib
import platform
import subprocess
import sys
import uuid
import zipfile
import xml.etree.ElementTree as ET
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from run_local_editor_desktop_vertical import (  # noqa: E402
    MAX_EXPORT_BYTES,
    MAX_PROJECT_BYTES,
    SAMPLE_SOURCE_BYTE_LEN,
    SAMPLE_SOURCE_HASH,
    DesktopVerticalError,
    bind_fixture,
    git_head,
    sha256_file,
)
from validate_editor_desktop_continuity_v2_receipt import (  # noqa: E402
    validate_schema,
    validate_semantics,
)
from verify_editable_export_geometry import (  # noqa: E402
    RectEmu,
    attr_by_local,
    canonical_node_hex,
    format_emu_points,
    local_name,
    verify_export as verify_editable_export_geometry,
)

OBSERVATION_VERSION = "chaptera.editor-desktop-continuity-observation.v2"
RECEIPT_VERSION = "chaptera.editor-desktop-continuity-acceptance.v2"
MAX_REPLACEMENT_BYTES = 64 * 1024 * 1024


class ContinuityV2Error(DesktopVerticalError):
    pass


def require_exact_keys(value: Any, expected: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ContinuityV2Error(f"{label} must be an object")
    actual = set(value)
    if actual != expected:
        raise ContinuityV2Error(
            f"{label} fields mismatch: missing={sorted(expected-actual)} "
            f"extra={sorted(actual-expected)}"
        )
    return value


def bind_replacement(path: pathlib.Path) -> tuple[pathlib.Path, bytes, str, str]:
    bound = path.expanduser().resolve(strict=True)
    if not bound.is_file():
        raise ContinuityV2Error("replacement path is not a regular file")
    raw = bound.read_bytes()
    if not raw or len(raw) > MAX_REPLACEMENT_BYTES:
        raise ContinuityV2Error("replacement byte length outside bounded range")
    if raw.startswith(b"\x89PNG\r\n\x1a\n"):
        mime = "image/png"
    elif raw.startswith(b"\xff\xd8\xff"):
        mime = "image/jpeg"
    else:
        raise ContinuityV2Error("replacement must be signature-valid PNG or JPEG")
    return bound, raw, mime, hashlib.sha256(raw).hexdigest()


def render_command(
    template: list[str],
    *,
    fixture: pathlib.Path,
    replacement: pathlib.Path,
    project_output: pathlib.Path,
    export_output: pathlib.Path,
) -> list[str]:
    if not template:
        raise ContinuityV2Error("desktop producer command is empty")
    replacements = {
        "{fixture}": str(fixture),
        "{replacement}": str(replacement),
        "{project}": str(project_output),
        "{export}": str(export_output),
    }
    for placeholder in replacements:
        count = sum(part.count(placeholder) for part in template)
        if count != 1:
            raise ContinuityV2Error(
                f"desktop producer command must contain {placeholder} exactly once"
            )
    rendered = list(template)
    for placeholder, replacement_value in replacements.items():
        rendered = [part.replace(placeholder, replacement_value) for part in rendered]
    return rendered


def validate_observation(
    value: Any,
    *,
    expected_source_hash: str,
    expected_commit: str,
    export_format: str,
    replacement_binding_id: str,
    replacement_mime: str,
    replacement_byte_len: int,
    expected_wrap_mutation_scope: str,
) -> dict[str, Any]:
    observation = require_exact_keys(
        value,
        {
            "protocol_version",
            "source_hash",
            "rar_commit",
            "story_edit",
            "object_move",
            "object_resize",
            "image_replace",
            "history",
            "reopen",
            "project",
            "capability_loss",
            "export",
            "invariants",
        },
        "desktop continuity V2 observation",
    )
    if observation["protocol_version"] != OBSERVATION_VERSION:
        raise ContinuityV2Error("desktop V2 observation protocol_version mismatch")
    if observation["source_hash"] != expected_source_hash:
        raise ContinuityV2Error("desktop V2 observation source_hash mismatch")
    if observation["rar_commit"] != expected_commit:
        raise ContinuityV2Error("desktop V2 producer ran a different Rar commit")

    story = require_exact_keys(
        observation["story_edit"],
        {
            "story_id",
            "operation_kind",
            "capability_admitted",
            "before_state_id",
            "after_state_id",
        },
        "story_edit",
    )
    move = require_exact_keys(
        observation["object_move"],
        {
            "instance_id",
            "projection_kind",
            "origin_node_id",
            "capability_admitted",
            "geometry_sync_policy",
            "before",
            "after",
            "durable_move_count",
            "transient_geometry_operation_count",
        },
        "object_move",
    )
    resize = require_exact_keys(
        observation["object_resize"],
        {
            "instance_id",
            "projection_kind",
            "origin_node_id",
            "capability_admitted",
            "geometry_sync_policy",
            "before",
            "after",
            "durable_resize_count",
            "transient_geometry_operation_count",
        },
        "object_resize",
    )
    image = require_exact_keys(
        observation["image_replace"],
        {
            "instance_id",
            "projection_kind",
            "origin_node_id",
            "capability_admitted",
            "replacement_binding_id",
            "replacement_binding_content_derived",
            "asset_sha_redacted",
            "after_asset_mime",
            "after_asset_byte_len",
            "frame_before",
            "frame_after",
            "explicit_crop_present",
            "durable_replace_count",
        },
        "image_replace",
    )
    history = require_exact_keys(
        observation["history"],
        {
            "after_story_state_id",
            "after_move_state_id",
            "after_resize_state_id",
            "after_replace_state_id",
            "undo_replace_state_id",
            "redo_replace_state_id",
        },
        "history",
    )
    reopen = require_exact_keys(
        observation["reopen"],
        {
            "fresh_session",
            "state_id",
            "story_state_id",
            "moved_rect",
            "resized_rect",
            "replacement_binding_preserved",
        },
        "reopen",
    )
    project = require_exact_keys(
        observation["project"],
        {
            "schema_version",
            "operation_count",
            "story_operation_count",
            "move_operation_count",
            "resize_operation_count",
            "replace_image_operation_count",
        },
        "project",
    )
    capability = require_exact_keys(
        observation["capability_loss"],
        {
            "observed_before_export",
            "blocking_loss_count",
            "approximations_explicit",
            "unsupported_partial_semantics_explicit",
        },
        "capability_loss",
    )
    export = require_exact_keys(
        observation["export"],
        {
            "format",
            "edited_story_present",
            "moved_geometry_present",
            "resized_geometry_present",
            "replacement_image_present",
        },
        "export",
    )
    invariants = require_exact_keys(
        observation["invariants"],
        {
            "native_pub_write_used",
            "no_hidden_network_upload",
            "direct_page_local_gate_used",
            "projected_object_mutation_fails_closed",
            "reopen_used_fresh_session",
            "export_from_current_editor_state",
            "replacement_asset_sha_emitted",
            "wrap_mutation_scope",
        },
        "invariants",
    )

    if export["format"] != export_format:
        raise ContinuityV2Error("desktop V2 observation export format mismatch")
    if image["replacement_binding_id"] != replacement_binding_id:
        raise ContinuityV2Error("desktop V2 replacement binding mismatch")
    if image["replacement_binding_content_derived"] is not False:
        raise ContinuityV2Error("replacement binding must not be content-derived")
    if image["asset_sha_redacted"] is not True:
        raise ContinuityV2Error("replacement asset SHA must remain redacted")
    if image["after_asset_mime"] != replacement_mime:
        raise ContinuityV2Error("replacement MIME differs from independently bound bytes")
    if image["after_asset_byte_len"] != replacement_byte_len:
        raise ContinuityV2Error("replacement byte length differs from independently bound bytes")
    if invariants["replacement_asset_sha_emitted"] is not False:
        raise ContinuityV2Error("producer claims replacement asset SHA emission")
    if invariants["wrap_mutation_scope"] != expected_wrap_mutation_scope:
        raise ContinuityV2Error("producer wrap mutation scope differs from requested acceptance mode")

    move_id = move["origin_node_id"]
    resize_id = resize["origin_node_id"]
    image_id = image["origin_node_id"]
    if expected_wrap_mutation_scope == "text_frame_non_intersecting":
        if move_id == resize_id:
            raise ContinuityV2Error(
                "newsletter V2 requires MoveNode on a distinct exact source image"
            )
        if image_id != resize_id:
            raise ContinuityV2Error(
                "newsletter V2 requires ReplaceImage on the resized photo frame"
            )
    elif len({move_id, resize_id, image_id}) != 3:
        raise ContinuityV2Error("V2 requires distinct move/resize/image targets")

    return {
        "story_edit": story,
        "object_move": move,
        "object_resize": resize,
        "image_replace": image,
        "history": history,
        "reopen": reopen,
        "project": project,
        "capability_loss": capability,
        "export": export,
        "invariants": invariants,
    }


def load_and_verify_project(
    path: pathlib.Path,
    *,
    source_hash: str,
    observation: dict[str, Any],
    replacement_sha256: str,
    replacement_mime: str,
    replacement_byte_len: int,
) -> tuple[bytes, dict[str, Any], str]:
    if not path.is_file():
        raise ContinuityV2Error("desktop V2 producer did not write EditorProject")
    raw = path.read_bytes()
    if not raw or len(raw) > MAX_PROJECT_BYTES:
        raise ContinuityV2Error("EditorProject byte length outside bounded range")
    try:
        project = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ContinuityV2Error("EditorProject is not valid JSON") from error
    if not isinstance(project, dict):
        raise ContinuityV2Error("EditorProject must be a JSON object")
    if project.get("source_hash") != source_hash:
        raise ContinuityV2Error("EditorProject source identity mismatch")

    schema_version = project.get("schema_version")
    supported = {f"pub-editor-v0.{version}" for version in range(3, 13)}
    if schema_version not in supported:
        raise ContinuityV2Error("V2 EditorProject schema_version cannot carry required state")
    if schema_version in {"pub-editor-v0.11", "pub-editor-v0.12"}:
        identity = project.get("identity")
        if not isinstance(identity, dict):
            raise ContinuityV2Error(f"{schema_version} project must carry durable identity")
        for field in ("project_id", "document_id", "history_id", "genesis_revision_id"):
            if not isinstance(identity.get(field), str) or not identity[field]:
                raise ContinuityV2Error(f"EditorProject identity missing {field}")

    operations = project.get("operations")
    if not isinstance(operations, list) or len(operations) != 4:
        raise ContinuityV2Error("V2 EditorProject must contain exactly four operations")
    by_kind: dict[str, list[dict[str, Any]]] = {}
    for operation in operations:
        if not isinstance(operation, dict) or not isinstance(operation.get("kind"), str):
            raise ContinuityV2Error("EditorProject operation is malformed")
        by_kind.setdefault(operation["kind"], []).append(operation)
    expected_kinds = {
        "replace_story_range",
        "move_node",
        "resize_node",
        "replace_image",
    }
    if set(by_kind) != expected_kinds or any(len(by_kind[kind]) != 1 for kind in expected_kinds):
        raise ContinuityV2Error("V2 EditorProject operation family set is not exact")

    story = by_kind["replace_story_range"][0]
    story_obs = observation["story_edit"]
    if story.get("story_id") != story_obs["story_id"]:
        raise ContinuityV2Error("EditorProject Story identity mismatch")
    if story.get("before_story_state_id") != story_obs["before_state_id"]:
        raise ContinuityV2Error("EditorProject Story before-state mismatch")
    if story.get("after_story_state_id") != story_obs["after_state_id"]:
        raise ContinuityV2Error("EditorProject Story after-state mismatch")
    replacement_text = story.get("replacement_text")
    if not isinstance(replacement_text, str) or len(replacement_text) < 8:
        raise ContinuityV2Error("V2 Story edit lacks a non-trivial export witness")

    for kind, obs_name in (("move_node", "object_move"), ("resize_node", "object_resize")):
        operation = by_kind[kind][0]
        observed = observation[obs_name]
        if operation.get("node_id") != observed["origin_node_id"]:
            raise ContinuityV2Error(f"EditorProject {kind} identity mismatch")
        if operation.get("before") != observed["before"] or operation.get("after") != observed["after"]:
            raise ContinuityV2Error(f"EditorProject {kind} RectEmu differs from observation")

    image = by_kind["replace_image"][0]
    image_obs = observation["image_replace"]
    if image.get("node_id") != image_obs["origin_node_id"]:
        raise ContinuityV2Error("EditorProject ReplaceImage identity mismatch")
    if image.get("after_asset") != replacement_sha256:
        raise ContinuityV2Error("EditorProject ReplaceImage asset differs from bound replacement bytes")

    assets = project.get("assets")
    if not isinstance(assets, list) or len(assets) != 1:
        raise ContinuityV2Error("V2 project must retain exactly one operation-reachable asset")
    asset = assets[0]
    if not isinstance(asset, dict):
        raise ContinuityV2Error("EditorProject asset metadata is malformed")
    expected_asset = {
        "sha256": replacement_sha256,
        "mime": replacement_mime,
        "byte_len": replacement_byte_len,
    }
    if asset != expected_asset:
        raise ContinuityV2Error("EditorProject asset metadata differs from bound replacement bytes")

    project_obs = observation["project"]
    counts = {
        "operation_count": 4,
        "story_operation_count": 1,
        "move_operation_count": 1,
        "resize_operation_count": 1,
        "replace_image_operation_count": 1,
    }
    for key, expected in counts.items():
        if project_obs[key] != expected:
            raise ContinuityV2Error(f"producer project count mismatch for {key}")
    if project_obs["schema_version"] != schema_version:
        raise ContinuityV2Error("producer project schema_version differs from written sidecar")

    return raw, project, replacement_text


def verify_story_in_export(
    archive: zipfile.ZipFile,
    names: set[str],
    export_format: str,
    replacement_text: str,
) -> None:
    if export_format == "idml":
        if "designmap.xml" not in names:
            raise ContinuityV2Error("IDML package is missing designmap.xml")
        story_parts = sorted(
            name for name in names if name.startswith("Stories/") and name.endswith(".xml")
        )
        if not story_parts:
            raise ContinuityV2Error("IDML package has no Story XML")
        try:
            present = any(
                replacement_text in "".join(ET.fromstring(archive.read(name)).itertext())
                for name in story_parts
            )
        except ET.ParseError as error:
            raise ContinuityV2Error("IDML Story XML is not parseable") from error
    elif export_format == "odg":
        if "mimetype" not in names or "content.xml" not in names:
            raise ContinuityV2Error("ODG package is missing mimetype/content.xml")
        if archive.read("mimetype") != b"application/vnd.oasis.opendocument.graphics":
            raise ContinuityV2Error("ODG mimetype mismatch")
        try:
            present = replacement_text in "".join(
                ET.fromstring(archive.read("content.xml")).itertext()
            )
        except ET.ParseError as error:
            raise ContinuityV2Error("ODG content.xml is not parseable") from error
    else:
        raise ContinuityV2Error("unsupported editable export format")
    if not present:
        raise ContinuityV2Error("edited export does not contain accepted Story witness")


def verify_replacement_bytes(
    archive: zipfile.ZipFile,
    names: set[str],
    export_format: str,
    replacement_bytes: bytes,
) -> None:
    if export_format == "odg":
        if any(
            name.startswith("Pictures/") and archive.read(name) == replacement_bytes
            for name in names
        ):
            return
        raise ContinuityV2Error("ODG does not contain exact replacement image bytes")

    for name in sorted(names):
        if not name.endswith(".xml"):
            continue
        try:
            root = ET.fromstring(archive.read(name))
        except ET.ParseError:
            continue
        for element in root.iter():
            if element.tag.rsplit("}", 1)[-1] != "Contents" or not element.text:
                continue
            candidate = "".join(element.text.split())
            try:
                decoded = base64.b64decode(candidate, validate=True)
            except (binascii.Error, ValueError):
                continue
            if decoded == replacement_bytes:
                return
    raise ContinuityV2Error("IDML does not contain exact base64 replacement image bytes")


def verify_geometry(
    path: pathlib.Path,
    export_format: str,
    node_id: str,
    rect: dict[str, Any],
    label: str,
) -> None:
    try:
        proof = verify_editable_export_geometry(
            path,
            export_format,
            node_id,
            RectEmu(
                x=rect["x"],
                y=rect["y"],
                width=rect["width"],
                height=rect["height"],
            ),
        )
    except (AssertionError, KeyError, TypeError, ValueError, zipfile.BadZipFile) as error:
        raise ContinuityV2Error(f"edited export does not reproduce {label} geometry") from error
    if proof.get("geometry_matches_edit") is not True:
        raise ContinuityV2Error(f"edited export {label} geometry proof is not affirmative")


def verify_odg_replacement_image_geometry(
    path: pathlib.Path,
    node_id: str,
    rect: dict[str, Any],
) -> None:
    """Verify the exact ODG image frame emitted by pub-odg.

    Generic editable-export geometry verification targets semantic TextFrame
    identities named Frame_<NodeId>. Replacement images are intentionally
    materialized by pub-odg as Image_<NodeId>, so the V2 acceptance boundary
    must verify that image-specific identity rather than looking up a text
    frame with the same source node id.
    """
    frame_name = "Image_" + canonical_node_hex(node_id)
    try:
        with zipfile.ZipFile(path) as archive:
            if "content.xml" not in archive.namelist():
                raise AssertionError("ODG package has no content.xml")
            root = ET.fromstring(archive.read("content.xml"))
    except (ET.ParseError, zipfile.BadZipFile) as error:
        raise ContinuityV2Error("edited export ODG content is not parseable") from error

    matches = [
        element
        for element in root.iter()
        if local_name(element.tag) == "frame"
        and attr_by_local(element, "name") == frame_name
    ]
    if len(matches) != 1:
        raise ContinuityV2Error(
            f"edited export expected exactly one replacement image frame, found {len(matches)}"
        )

    frame = matches[0]
    observed = {
        "x": attr_by_local(frame, "x"),
        "y": attr_by_local(frame, "y"),
        "width": attr_by_local(frame, "width"),
        "height": attr_by_local(frame, "height"),
    }
    expected = {
        "x": format_emu_points(rect["x"]) + "pt",
        "y": format_emu_points(rect["y"]) + "pt",
        "width": format_emu_points(rect["width"]) + "pt",
        "height": format_emu_points(rect["height"]) + "pt",
    }
    if observed != expected:
        raise ContinuityV2Error(
            f"edited export replacement image geometry mismatch: "
            f"observed={observed} expected={expected}"
        )


def verify_export_package(
    path: pathlib.Path,
    export_format: str,
    *,
    observation: dict[str, Any],
    replacement_text: str,
    replacement_bytes: bytes,
) -> bytes:
    if not path.is_file():
        raise ContinuityV2Error("desktop V2 producer did not write edited export")
    raw = path.read_bytes()
    if not raw or len(raw) > MAX_EXPORT_BYTES:
        raise ContinuityV2Error("export byte length outside bounded range")
    if not zipfile.is_zipfile(path):
        raise ContinuityV2Error(f"{export_format} output is not a ZIP package")

    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        verify_story_in_export(archive, names, export_format, replacement_text)
        verify_replacement_bytes(archive, names, export_format, replacement_bytes)

    verify_geometry(
        path,
        export_format,
        observation["object_move"]["origin_node_id"],
        observation["object_move"]["after"],
        "MoveNode",
    )
    verify_geometry(
        path,
        export_format,
        observation["object_resize"]["origin_node_id"],
        observation["object_resize"]["after"],
        "ResizeNode",
    )
    if export_format == "odg":
        verify_odg_replacement_image_geometry(
            path,
            observation["image_replace"]["origin_node_id"],
            observation["image_replace"]["frame_after"],
        )
    else:
        verify_geometry(
            path,
            export_format,
            observation["image_replace"]["origin_node_id"],
            observation["image_replace"]["frame_after"],
            "ReplaceImage frame",
        )
    return raw


def run_continuity_v2(
    *,
    fixture: pathlib.Path,
    replacement: pathlib.Path,
    project_output: pathlib.Path,
    export_output: pathlib.Path,
    receipt_output: pathlib.Path,
    command_template: list[str],
    expected_hash: str = SAMPLE_SOURCE_HASH,
    expected_len: int = SAMPLE_SOURCE_BYTE_LEN,
    rar_commit: str | None = None,
    require_explicit_crop: bool = False,
    require_wrap_irrelevant_mutations: bool = False,
) -> dict[str, Any]:
    fixture = bind_fixture(fixture, expected_hash=expected_hash, expected_len=expected_len)
    replacement, replacement_bytes, replacement_mime, replacement_sha256 = bind_replacement(
        replacement
    )
    rar_commit = rar_commit or git_head()
    replacement_binding_id = "continuity-v2-" + uuid.uuid4().hex

    export_output = export_output.expanduser().resolve()
    project_output = project_output.expanduser().resolve()
    receipt_output = receipt_output.expanduser().resolve()
    suffix = export_output.suffix.lower()
    if suffix != ".odg":
        raise ContinuityV2Error("Stage 0.1 continuity V2 closure is intentionally bounded to ODG")
    export_format = "odg"

    for output in (project_output, export_output, receipt_output):
        output.parent.mkdir(parents=True, exist_ok=True)
        if output.exists():
            output.unlink()

    command = render_command(
        command_template,
        fixture=fixture,
        replacement=replacement,
        project_output=project_output,
        export_output=export_output,
    )
    env = dict(os.environ)
    env["CHAPTERA_RAR_COMMIT"] = rar_commit
    env["CHAPTERA_SOURCE_HASH"] = expected_hash
    env["CHAPTERA_DESKTOP_CONTINUITY_V2"] = "1"
    env["CHAPTERA_REPLACEMENT_BINDING_ID"] = replacement_binding_id
    env["CHAPTERA_CONTINUITY_REQUIRE_EXPLICIT_CROP"] = (
        "1" if require_explicit_crop else "0"
    )
    env["CHAPTERA_CONTINUITY_REQUIRE_WRAP_IRRELEVANT"] = (
        "1" if require_wrap_irrelevant_mutations else "0"
    )

    completed = subprocess.run(
        command,
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", errors="replace").strip()
        raise ContinuityV2Error(
            "desktop V2 producer failed" + (f": {detail}" if detail else "")
        )

    try:
        decoded = completed.stdout.decode("utf-8")
        observation_raw = json.loads(decoded)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ContinuityV2Error(
            "desktop V2 producer stdout is not exactly one UTF-8 JSON value"
        ) from error

    if replacement_sha256 in decoded:
        raise ContinuityV2Error("desktop V2 producer leaked replacement asset SHA")

    observation = validate_observation(
        observation_raw,
        expected_source_hash=expected_hash,
        expected_commit=rar_commit,
        export_format=export_format,
        replacement_binding_id=replacement_binding_id,
        replacement_mime=replacement_mime,
        replacement_byte_len=len(replacement_bytes),
        expected_wrap_mutation_scope=(
            "text_frame_non_intersecting"
            if require_wrap_irrelevant_mutations
            else "not_asserted"
        ),
    )

    if fixture.stat().st_size != expected_len or sha256_file(fixture) != expected_hash:
        raise ContinuityV2Error("source PUB changed during desktop V2 acceptance")

    project_raw, project, replacement_text = load_and_verify_project(
        project_output,
        source_hash=expected_hash,
        observation=observation,
        replacement_sha256=replacement_sha256,
        replacement_mime=replacement_mime,
        replacement_byte_len=len(replacement_bytes),
    )
    export_raw = verify_export_package(
        export_output,
        export_format,
        observation=observation,
        replacement_text=replacement_text,
        replacement_bytes=replacement_bytes,
    )

    receipt = {
        "receipt_version": RECEIPT_VERSION,
        "receipt_kind": "real_local",
        "producer": {
            "implementation": "rar-desktop-continuity-v2-boundary",
            "commit_or_build": f"rar:{rar_commit}",
            "core_integration": True,
        },
        "source": {
            "sha256": expected_hash,
            "byte_len": expected_len,
            "immutable": True,
        },
        "story_edit": observation["story_edit"],
        "object_move": observation["object_move"],
        "object_resize": observation["object_resize"],
        "image_replace": observation["image_replace"],
        "history": observation["history"],
        "reopen": observation["reopen"],
        "project": {
            "schema_version": project["schema_version"],
            "sha256": hashlib.sha256(project_raw).hexdigest(),
            "byte_len": len(project_raw),
            "operation_count": 4,
            "story_operation_count": 1,
            "move_operation_count": 1,
            "resize_operation_count": 1,
            "replace_image_operation_count": 1,
        },
        "capability_loss": observation["capability_loss"],
        "export": {
            "format": export_format,
            "sha256": hashlib.sha256(export_raw).hexdigest(),
            "byte_len": len(export_raw),
            "package_valid": True,
            "edited_story_present": True,
            "moved_geometry_present": True,
            "resized_geometry_present": True,
            "replacement_image_present": True,
        },
        "environment": {
            "rar_commit": rar_commit,
            "os": platform.system() or "unknown",
            "arch": platform.machine() or "unknown",
        },
        "invariants": {
            "source_pub_immutable": True,
            "native_pub_write_used": observation["invariants"]["native_pub_write_used"],
            "no_hidden_network_upload": observation["invariants"]["no_hidden_network_upload"],
            "raw_document_text_emitted": False,
            "raw_source_bytes_emitted": False,
            "direct_page_local_gate_used": observation["invariants"][
                "direct_page_local_gate_used"
            ],
            "projected_object_mutation_fails_closed": observation["invariants"][
                "projected_object_mutation_fails_closed"
            ],
            "reopen_used_fresh_session": observation["invariants"]["reopen_used_fresh_session"],
            "export_from_current_editor_state": observation["invariants"][
                "export_from_current_editor_state"
            ],
            "replacement_asset_sha_emitted": False,
            "wrap_mutation_scope": observation["invariants"]["wrap_mutation_scope"],
        },
    }

    try:
        validate_schema(receipt)
        validate_semantics(receipt)
    except AssertionError as error:
        raise ContinuityV2Error(str(error)) from error

    receipt_output.write_text(
        json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run and independently verify imported-PUB continuity V2"
    )
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--replacement", required=True, type=pathlib.Path)
    parser.add_argument("--project-output", required=True, type=pathlib.Path)
    parser.add_argument("--export-output", required=True, type=pathlib.Path)
    parser.add_argument("--receipt-output", required=True, type=pathlib.Path)
    parser.add_argument("--expected-source-hash", default=SAMPLE_SOURCE_HASH)
    parser.add_argument("--expected-source-bytes", type=int, default=SAMPLE_SOURCE_BYTE_LEN)
    parser.add_argument("--require-explicit-crop", action="store_true")
    parser.add_argument("--require-wrap-irrelevant-mutations", action="store_true")
    parser.add_argument("desktop_command", nargs=argparse.REMAINDER)
    args = parser.parse_args()

    command = list(args.desktop_command)
    if command and command[0] == "--":
        command = command[1:]

    try:
        receipt = run_continuity_v2(
            fixture=args.fixture,
            replacement=args.replacement,
            project_output=args.project_output,
            export_output=args.export_output,
            receipt_output=args.receipt_output,
            command_template=command,
            expected_hash=args.expected_source_hash,
            expected_len=args.expected_source_bytes,
            require_explicit_crop=args.require_explicit_crop,
            require_wrap_irrelevant_mutations=args.require_wrap_irrelevant_mutations,
        )
    except (ContinuityV2Error, OSError) as error:
        print(str(error), file=sys.stderr)
        return 2

    print(
        json.dumps(
            {
                "status": "valid",
                "source_hash": receipt["source"]["sha256"],
                "replacement_binding_id": receipt["image_replace"]["replacement_binding_id"],
                "project_sha256": receipt["project"]["sha256"],
                "export_format": receipt["export"]["format"],
                "export_sha256": receipt["export"]["sha256"],
                "rar_commit": receipt["environment"]["rar_commit"],
                "receipt": str(args.receipt_output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
