#!/usr/bin/env python3
"""Independently prove Stage 0.1 current Editor state reaches a real fixed PDF."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
from typing import Any

PACKET_VERSION = "chaptera.desktop-fixed-output-packet.v1"
RENDER_REQUEST_VERSION = "chaptera.editor-fixed-pdf-render-request.v1"
RENDER_RESULT_VERSION = "chaptera.editor-fixed-pdf-render-result.v1"
RECEIPT_VERSION = "chaptera.editor-fixed-pdf-current-revision-receipt.v1"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")


class CurrentRevisionPdfError(RuntimeError):
    pass


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def hash_id(value: Any) -> str:
    return "sha256:" + sha256_bytes(canonical_json(value))


def read_json(path: pathlib.Path, label: str) -> tuple[bytes, dict[str, Any]]:
    raw = path.read_bytes()
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise CurrentRevisionPdfError(f"{label} is not valid JSON") from error
    if not isinstance(value, dict):
        raise CurrentRevisionPdfError(f"{label} must be a JSON object")
    return raw, value


def exact_four_operations(project: dict[str, Any]) -> dict[str, dict[str, Any]]:
    operations = project.get("operations")
    if not isinstance(operations, list) or len(operations) != 4:
        raise CurrentRevisionPdfError("EditorProject must contain exactly four Stage 0.1 operations")
    by_kind: dict[str, list[dict[str, Any]]] = {}
    for operation in operations:
        if not isinstance(operation, dict) or not isinstance(operation.get("kind"), str):
            raise CurrentRevisionPdfError("EditorProject operation is malformed")
        by_kind.setdefault(operation["kind"], []).append(operation)
    expected = {
        "replace_story_range",
        "move_node",
        "resize_node",
        "replace_image",
    }
    if set(by_kind) != expected or any(len(by_kind[kind]) != 1 for kind in expected):
        raise CurrentRevisionPdfError("EditorProject operation family differs from Stage 0.1 V2")
    return {kind: by_kind[kind][0] for kind in expected}


def find_scene_node(packet: dict[str, Any], node_id: str, label: str) -> dict[str, Any]:
    shaped_flow = packet.get("shaped_flow")
    if not isinstance(shaped_flow, dict):
        raise CurrentRevisionPdfError("packet shaped_flow is missing")
    nodes = shaped_flow.get("nodes")
    if not isinstance(nodes, list):
        raise CurrentRevisionPdfError("packet shaped_flow.nodes must be an array")
    matches = [
        node
        for node in nodes
        if isinstance(node, dict) and node.get("origin") == node_id
    ]
    if len(matches) != 1:
        raise CurrentRevisionPdfError(
            f"{label} target must occur exactly once in current shaped-flow scene"
        )
    return matches[0]


def validate_stage01_binding(
    *,
    source_raw: bytes,
    project_raw: bytes,
    project: dict[str, Any],
    replacement_raw: bytes,
    stage01: dict[str, Any],
) -> dict[str, dict[str, Any]]:
    source_sha = sha256_bytes(source_raw)
    replacement_sha = sha256_bytes(replacement_raw)

    source = stage01.get("source")
    if not isinstance(source, dict) or source.get("sha256") != source_sha:
        raise CurrentRevisionPdfError("Stage 0.1 receipt source identity mismatch")
    if source.get("immutable") is not True:
        raise CurrentRevisionPdfError("Stage 0.1 receipt does not preserve immutable source")
    project_receipt = stage01.get("project")
    if not isinstance(project_receipt, dict):
        raise CurrentRevisionPdfError("Stage 0.1 project receipt is missing")
    if project_receipt.get("sha256") != sha256_bytes(project_raw):
        raise CurrentRevisionPdfError("Stage 0.1 receipt project SHA differs from project bytes")

    if project.get("source_hash") != source_sha:
        raise CurrentRevisionPdfError("EditorProject source identity mismatch")
    operations = exact_four_operations(project)

    story = operations["replace_story_range"]
    story_receipt = stage01.get("story_edit")
    if (
        not isinstance(story_receipt, dict)
        or story_receipt.get("story_id") != story.get("story_id")
        or story_receipt.get("after_state_id") != story.get("after_story_state_id")
    ):
        raise CurrentRevisionPdfError("Stage 0.1 Story witness differs from EditorProject")

    for kind, receipt_key in (
        ("move_node", "object_move"),
        ("resize_node", "object_resize"),
    ):
        operation = operations[kind]
        observed = stage01.get(receipt_key)
        if (
            not isinstance(observed, dict)
            or observed.get("origin_node_id") != operation.get("node_id")
            or observed.get("after") != operation.get("after")
        ):
            raise CurrentRevisionPdfError(f"Stage 0.1 {kind} witness differs from EditorProject")

    image = operations["replace_image"]
    image_receipt = stage01.get("image_replace")
    if (
        not isinstance(image_receipt, dict)
        or image_receipt.get("origin_node_id") != image.get("node_id")
    ):
        raise CurrentRevisionPdfError("Stage 0.1 ReplaceImage witness differs from EditorProject")
    if image.get("after_asset") != replacement_sha:
        raise CurrentRevisionPdfError("EditorProject ReplaceImage is not bound to replacement bytes")

    assets = project.get("assets")
    expected_asset = {
        "sha256": replacement_sha,
        "mime": "image/png" if replacement_raw.startswith(b"\x89PNG\r\n\x1a\n") else "image/jpeg",
        "byte_len": len(replacement_raw),
    }
    if assets != [expected_asset]:
        raise CurrentRevisionPdfError("EditorProject replacement asset metadata is not exact")

    return operations


def validate_packet(
    *,
    packet: dict[str, Any],
    source_sha: str,
    project: dict[str, Any],
    operations: dict[str, dict[str, Any]],
    replacement_raw: bytes,
    stage01: dict[str, Any],
) -> None:
    if packet.get("protocol_version") != PACKET_VERSION:
        raise CurrentRevisionPdfError("current fixed-output packet protocol mismatch")
    if packet.get("source_hash") != source_sha:
        raise CurrentRevisionPdfError("current fixed-output packet source identity mismatch")
    if not isinstance(packet.get("project_state_id"), str) or not packet["project_state_id"].startswith(
        "sha256:"
    ):
        raise CurrentRevisionPdfError("current fixed-output packet project_state_id missing")

    invariants = packet.get("invariants")
    if not isinstance(invariants, dict) or invariants != {
        "authoritative_rust_project_replay": True,
        "source_reparse_after_project_apply_count": 0,
        "source_refs_in_renderer_packet": False,
        "output_adapter_reshaping_calls": 0,
    }:
        raise CurrentRevisionPdfError("current fixed-output packet invariants are not exact")

    encoded = canonical_json(packet)
    if b"source_refs" in encoded:
        raise CurrentRevisionPdfError("renderer packet leaked source provenance")

    story = operations["replace_story_range"]
    story_id = story["story_id"]
    story_states = packet.get("story_states")
    if not isinstance(story_states, list):
        raise CurrentRevisionPdfError("packet story_states missing")
    story_state = next(
        (
            state
            for state in story_states
            if isinstance(state, dict) and state.get("story_id") == story_id
        ),
        None,
    )
    if (
        story_state is None
        or story_state.get("story_state_id") != story.get("after_story_state_id")
        or story_state.get("story_state_id") != stage01["story_edit"]["after_state_id"]
    ):
        raise CurrentRevisionPdfError("packet Story state is not the accepted current Story")

    expected_targets = {
        "story_mutation_ids": [story_id],
        "move_node_ids": [operations["move_node"]["node_id"]],
        "resize_node_ids": [operations["resize_node"]["node_id"]],
        "replacement_node_ids": [operations["replace_image"]["node_id"]],
    }
    for key, expected in expected_targets.items():
        if packet.get(key) != expected:
            raise CurrentRevisionPdfError(f"packet {key} differs from exact V2 mutation target")

    for kind in ("move_node", "resize_node"):
        operation = operations[kind]
        node = find_scene_node(packet, operation["node_id"], kind)
        if node.get("bounds") != operation.get("after"):
            raise CurrentRevisionPdfError(f"packet {kind} geometry is stale")

    image_node_id = operations["replace_image"]["node_id"]
    image_node = find_scene_node(packet, image_node_id, "replace_image")
    if image_node.get("bounds") != stage01["image_replace"]["frame_after"]:
        raise CurrentRevisionPdfError("packet ReplaceImage frame geometry is stale")

    image_resources = packet.get("image_resources")
    if not isinstance(image_resources, list):
        raise CurrentRevisionPdfError("packet image_resources missing")
    image_matches = [
        resource
        for resource in image_resources
        if isinstance(resource, dict)
        and isinstance(resource.get("node_ids"), list)
        and image_node_id in resource["node_ids"]
    ]
    if len(image_matches) != 1:
        raise CurrentRevisionPdfError(
            "ReplaceImage target must bind exactly one current image resource"
        )
    raw = image_matches[0].get("bytes")
    if not isinstance(raw, list) or bytes(raw) != replacement_raw:
        raise CurrentRevisionPdfError(
            "ReplaceImage current image resource is not exact replacement bytes"
        )

    shaped_flow = packet.get("shaped_flow")
    lines = shaped_flow.get("lines") if isinstance(shaped_flow, dict) else None
    if not isinstance(lines, list) or not any(
        isinstance(line, dict)
        and line.get("story_origin") == story_id
        and isinstance(line.get("text"), str)
        and line["text"]
        for line in lines
    ):
        raise CurrentRevisionPdfError("accepted edited Story has no current shaped-flow line")


def scene_view(packet: dict[str, Any]) -> dict[str, Any]:
    flow = packet["shaped_flow"]
    return {
        "environment": flow["environment"]["shaping"]["layout"],
        "surfaces": flow["surfaces"],
        "nodes": flow["nodes"],
        "origin_mapping": flow["origin_mapping"],
        "diagnostics": flow.get("diagnostics", []),
    }


def invoke_renderer(
    renderer_command: list[str],
    *,
    packet: dict[str, Any],
    project_raw: bytes,
    pdf_output: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, str]]:
    if not renderer_command:
        raise CurrentRevisionPdfError("renderer command is required")

    identities = {
        "source_hash": packet["source_hash"],
        "project_hash": "sha256:" + sha256_bytes(project_raw),
        "packet_id": hash_id(packet),
        "scene_snapshot_id": hash_id(scene_view(packet)),
        "flow_id": hash_id(packet["shaped_flow"]),
    }
    request = {
        "protocol_version": RENDER_REQUEST_VERSION,
        **identities,
        "packet": packet,
    }

    pdf_output.parent.mkdir(parents=True, exist_ok=True)
    if pdf_output.exists():
        pdf_output.unlink()
    env = dict(os.environ)
    env["CHAPTERA_PDF_OUTPUT"] = str(pdf_output.resolve())
    completed = subprocess.run(
        renderer_command,
        input=json.dumps(request, ensure_ascii=False, separators=(",", ":")),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip()
        raise CurrentRevisionPdfError(
            "current-revision PDF renderer failed" + (f": {detail}" if detail else "")
        )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise CurrentRevisionPdfError("current-revision renderer returned invalid JSON") from error

    if not isinstance(result, dict) or result.get("protocol_version") != RENDER_RESULT_VERSION:
        raise CurrentRevisionPdfError("current-revision renderer result protocol mismatch")
    for key, expected in identities.items():
        if result.get(key) != expected:
            raise CurrentRevisionPdfError(f"renderer result {key} differs from request")
    summary = result.get("summary")
    if not isinstance(summary, dict):
        raise CurrentRevisionPdfError("renderer summary missing")

    if not pdf_output.is_file():
        raise CurrentRevisionPdfError("renderer succeeded without writing PDF")
    artifact = pdf_output.read_bytes()
    if len(artifact) < 8 or not artifact.startswith(b"%PDF-"):
        raise CurrentRevisionPdfError("renderer artifact is not a PDF")

    return result, identities


def build_receipt(
    *,
    source_path: pathlib.Path,
    source_raw: bytes,
    project_raw: bytes,
    project: dict[str, Any],
    replacement_raw: bytes,
    stage01_raw: bytes,
    stage01: dict[str, Any],
    packet_raw: bytes,
    packet: dict[str, Any],
    renderer_result: dict[str, Any],
    identities: dict[str, str],
    pdf_raw: bytes,
) -> dict[str, Any]:
    source_sha = sha256_bytes(source_raw)
    replacement_sha = sha256_bytes(replacement_raw)
    operations = exact_four_operations(project)
    shaped_flow = packet["shaped_flow"]
    lines = shaped_flow["lines"]

    receipt = {
        "receipt_version": RECEIPT_VERSION,
        "receipt_kind": "real_local",
        "producer": {
            "implementation": "rar-editor-current-revision-fixed-pdf-v1",
            "core_integration": True,
        },
        "source": {
            "sha256": source_sha,
            "byte_len": len(source_raw),
            "immutable": True,
        },
        "stage_0_1": {
            "receipt_sha256": sha256_bytes(stage01_raw),
            "project_sha256": sha256_bytes(project_raw),
            "project_schema_version": project.get("schema_version"),
            "replacement_binding_id": stage01["image_replace"]["replacement_binding_id"],
        },
        "current_revision": {
            "project_state_id": packet["project_state_id"],
            "accepted_story_state_id": operations["replace_story_range"]["after_story_state_id"],
            "story_target_state_current": True,
            "move_geometry_current": True,
            "resize_geometry_current": True,
            "replacement_image_current": True,
            "mutation_target_count": 4,
        },
        "packet": {
            "packet_sha256": sha256_bytes(packet_raw),
            "packet_id": identities["packet_id"],
            "scene_snapshot_id": identities["scene_snapshot_id"],
            "scene_geometry_hash": hash_id(
                {
                    "surfaces": shaped_flow["surfaces"],
                    "nodes": shaped_flow["nodes"],
                }
            ),
            "flow_id": identities["flow_id"],
            "visible_line_count": sum(
                1 for line in lines if isinstance(line, dict) and line.get("text")
            ),
            "image_resource_count": len(packet.get("image_resources", [])),
            "node_paint_count": len(packet.get("node_paints", [])),
        },
        "renderer": {
            "renderer_revision": renderer_result.get("renderer_revision"),
            "target_profile": renderer_result.get("target_profile"),
            "summary": renderer_result.get("summary"),
        },
        "artifact": {
            "format": "pdf",
            "sha256": sha256_bytes(pdf_raw),
            "byte_len": len(pdf_raw),
            "header": pdf_raw[:8].decode("ascii", errors="replace"),
        },
        "invariants": {
            "current_editor_project_authoritative": True,
            "authoritative_rust_project_replay": True,
            "source_reparse_after_edit_count": 0,
            "source_pub_immutable": True,
            "replacement_exact_bytes_reach_renderer_packet": True,
            "replacement_target_painted_or_renderer_failed": True,
            "move_target_painted_or_renderer_failed": True,
            "resize_target_painted_or_renderer_failed": True,
            "edited_story_materialized_or_renderer_failed": True,
            "raw_document_text_emitted": False,
            "raw_source_bytes_emitted": False,
            "replacement_asset_sha_emitted": False,
            "native_pub_write_used": False,
            "pdf_renderer_reimplemented_in_rar": False,
        },
    }

    encoded = json.dumps(receipt, ensure_ascii=False, sort_keys=True)
    if replacement_sha in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked replacement asset SHA")
    if str(source_path.resolve()) in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked source local path")
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=pathlib.Path)
    parser.add_argument("--project", required=True, type=pathlib.Path)
    parser.add_argument("--replacement", required=True, type=pathlib.Path)
    parser.add_argument("--stage01-receipt", required=True, type=pathlib.Path)
    parser.add_argument("--packet", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--receipt-output", required=True, type=pathlib.Path)
    parser.add_argument("renderer_command", nargs=argparse.REMAINDER)
    args = parser.parse_args()

    command = list(args.renderer_command)
    if command and command[0] == "--":
        command = command[1:]

    try:
        source_raw = args.source.read_bytes()
        project_raw, project = read_json(args.project, "EditorProject")
        replacement_raw = args.replacement.read_bytes()
        stage01_raw, stage01 = read_json(args.stage01_receipt, "Stage 0.1 receipt")
        packet_raw, packet = read_json(args.packet, "current fixed-output packet")

        operations = validate_stage01_binding(
            source_raw=source_raw,
            project_raw=project_raw,
            project=project,
            replacement_raw=replacement_raw,
            stage01=stage01,
        )
        source_sha = sha256_bytes(source_raw)
        validate_packet(
            packet=packet,
            source_sha=source_sha,
            project=project,
            operations=operations,
            replacement_raw=replacement_raw,
            stage01=stage01,
        )

        renderer_result, identities = invoke_renderer(
            command,
            packet=packet,
            project_raw=project_raw,
            pdf_output=args.pdf_output,
        )
        pdf_raw = args.pdf_output.read_bytes()

        if args.source.read_bytes() != source_raw:
            raise CurrentRevisionPdfError("source PUB changed during current-revision PDF closure")

        receipt = build_receipt(
            source_path=args.source,
            source_raw=source_raw,
            project_raw=project_raw,
            project=project,
            replacement_raw=replacement_raw,
            stage01_raw=stage01_raw,
            stage01=stage01,
            packet_raw=packet_raw,
            packet=packet,
            renderer_result=renderer_result,
            identities=identities,
            pdf_raw=pdf_raw,
        )
        args.receipt_output.parent.mkdir(parents=True, exist_ok=True)
        args.receipt_output.write_text(
            json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        print(
            json.dumps(
                {
                    "status": "valid",
                    "source_sha256": receipt["source"]["sha256"],
                    "project_sha256": receipt["stage_0_1"]["project_sha256"],
                    "packet_id": receipt["packet"]["packet_id"],
                    "pdf_sha256": receipt["artifact"]["sha256"],
                    "pdf_byte_len": receipt["artifact"]["byte_len"],
                    "receipt": str(args.receipt_output),
                },
                sort_keys=True,
            )
        )
        return 0
    except (OSError, CurrentRevisionPdfError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
