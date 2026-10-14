#!/usr/bin/env python3
"""Prove the accepted Stage 0.1 Editor revision reaches the real fixed-PDF backend."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[1]
RESOURCE_INPUT_VERSION = "chaptera.current-fixed-pdf-resource-input.v1"
REQUEST_VERSION = "chaptera.fixed-pdf-packet-render-request.v1"
RESULT_VERSION = "chaptera.fixed-pdf-packet-render-result.v1"
RECEIPT_VERSION = "chaptera.editor-fixed-pdf-current-revision-receipt.v2"
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


def run_checked(command: list[str], label: str) -> None:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise CurrentRevisionPdfError(label + (f": {detail}" if detail else ""))


def exact_four_operations(project: dict[str, Any]) -> dict[str, dict[str, Any]]:
    operations = project.get("operations")
    if not isinstance(operations, list) or len(operations) != 4:
        raise CurrentRevisionPdfError("EditorProject must contain exactly four Stage 0.1 operations")
    by_kind: dict[str, list[dict[str, Any]]] = {}
    for operation in operations:
        if not isinstance(operation, dict) or not isinstance(operation.get("kind"), str):
            raise CurrentRevisionPdfError("EditorProject operation is malformed")
        by_kind.setdefault(operation["kind"], []).append(operation)
    expected = {"replace_story_range", "move_node", "resize_node", "replace_image"}
    if set(by_kind) != expected or any(len(by_kind[kind]) != 1 for kind in expected):
        raise CurrentRevisionPdfError("EditorProject operation family differs from Stage 0.1 V2")
    return {kind: by_kind[kind][0] for kind in expected}


def find_scene_node(scene: dict[str, Any], node_id: str, label: str) -> dict[str, Any]:
    nodes = scene.get("nodes")
    if not isinstance(nodes, list):
        raise CurrentRevisionPdfError(f"{label} scene nodes are missing")
    matches = [
        node
        for node in nodes
        if isinstance(node, dict) and node.get("origin") == node_id
    ]
    if len(matches) != 1:
        raise CurrentRevisionPdfError(f"{label} target must occur exactly once in current scene")
    return matches[0]


def find_image_resource(resources: list[Any], node_id: str, label: str) -> dict[str, Any]:
    matches = [
        resource
        for resource in resources
        if isinstance(resource, dict)
        and isinstance(resource.get("node_ids"), list)
        and node_id in resource["node_ids"]
    ]
    if len(matches) != 1:
        raise CurrentRevisionPdfError(f"{label} target must bind exactly one image resource")
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

    for kind, receipt_key in (("move_node", "object_move"), ("resize_node", "object_resize")):
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

    mime = "image/png" if replacement_raw.startswith(b"\x89PNG\r\n\x1a\n") else "image/jpeg"
    if project.get("assets") != [
        {"sha256": replacement_sha, "mime": mime, "byte_len": len(replacement_raw)}
    ]:
        raise CurrentRevisionPdfError("EditorProject replacement asset metadata is not exact")
    return operations


def validate_resource_input(
    *,
    resource_input: dict[str, Any],
    project: dict[str, Any],
    operations: dict[str, dict[str, Any]],
    source_sha: str,
    replacement_raw: bytes,
    stage01: dict[str, Any],
) -> set[str]:
    if resource_input.get("protocol_version") != RESOURCE_INPUT_VERSION:
        raise CurrentRevisionPdfError("current fixed-PDF resource input protocol mismatch")
    binding = resource_input.get("binding")
    if not isinstance(binding, dict):
        raise CurrentRevisionPdfError("current fixed-PDF binding missing")
    expected_binding = {
        "protocol_version": "chaptera.editor-fixed-pdf-binding.v1",
        "source_hash": source_sha,
        "project_schema_version": project.get("schema_version"),
        "story_id": operations["replace_story_range"]["story_id"],
        "move_node_id": operations["move_node"]["node_id"],
        "resize_node_id": operations["resize_node"]["node_id"],
        "replacement_node_id": operations["replace_image"]["node_id"],
    }
    for key, expected in expected_binding.items():
        if binding.get(key) != expected:
            raise CurrentRevisionPdfError(f"current fixed-PDF binding {key} mismatch")
    project_state_id = binding.get("project_state_id")
    if not isinstance(project_state_id, str) or not project_state_id.startswith("sha256:"):
        raise CurrentRevisionPdfError("current fixed-PDF binding project_state_id missing")

    flow = resource_input.get("shaped_flow")
    if not isinstance(flow, dict):
        raise CurrentRevisionPdfError("current fixed-PDF shaped_flow missing")
    for kind in ("move_node", "resize_node"):
        operation = operations[kind]
        node = find_scene_node(flow, operation["node_id"], kind)
        if node.get("bounds") != operation.get("after"):
            raise CurrentRevisionPdfError(f"current fixed-PDF {kind} geometry is stale")

    image_id = operations["replace_image"]["node_id"]
    image_node = find_scene_node(flow, image_id, "replace_image")
    image_receipt = stage01.get("image_replace")
    if not isinstance(image_receipt, dict) or image_node.get("bounds") != image_receipt.get("frame_after"):
        raise CurrentRevisionPdfError("current fixed-PDF ReplaceImage frame geometry is stale")

    image_resources = resource_input.get("image_resources")
    if not isinstance(image_resources, list):
        raise CurrentRevisionPdfError("current fixed-PDF image resources missing")
    image = find_image_resource(image_resources, image_id, "ReplaceImage")
    raw = image.get("bytes")
    if not isinstance(raw, list) or bytes(raw) != replacement_raw:
        raise CurrentRevisionPdfError("current fixed-PDF image resource is not exact replacement bytes")

    story_id = operations["replace_story_range"]["story_id"]
    lines = flow.get("lines")
    if not isinstance(lines, list):
        raise CurrentRevisionPdfError("current fixed-PDF shaped lines missing")
    story_frames = {
        line.get("frame_origin")
        for line in lines
        if isinstance(line, dict)
        and line.get("story_origin") == story_id
        and isinstance(line.get("text"), str)
        and line.get("text")
        and isinstance(line.get("frame_origin"), str)
    }
    if not story_frames:
        raise CurrentRevisionPdfError("accepted edited Story has no current shaped-flow line")
    return story_frames


def validate_request(
    *,
    request: dict[str, Any],
    resource_input: dict[str, Any],
    operations: dict[str, dict[str, Any]],
    replacement_raw: bytes,
    story_frames: set[str],
) -> None:
    if request.get("protocol_version") != REQUEST_VERSION:
        raise CurrentRevisionPdfError("fixed-PDF render request protocol mismatch")
    if request.get("binding") != resource_input.get("binding"):
        raise CurrentRevisionPdfError("fixed-PDF render request binding mismatch")
    scene = request.get("scene")
    if not isinstance(scene, dict):
        raise CurrentRevisionPdfError("fixed-PDF request scene missing")
    for kind in ("move_node", "resize_node"):
        operation = operations[kind]
        node = find_scene_node(scene, operation["node_id"], kind)
        if node.get("bounds") != operation.get("after"):
            raise CurrentRevisionPdfError(f"fixed-PDF request {kind} geometry is stale")

    resources = request.get("resources")
    if not isinstance(resources, dict):
        raise CurrentRevisionPdfError("FixedPdfResources missing")
    images = resources.get("images")
    if not isinstance(images, list):
        raise CurrentRevisionPdfError("fixed-PDF request images missing")
    image = find_image_resource(images, operations["replace_image"]["node_id"], "ReplaceImage")
    raw = image.get("bytes")
    if not isinstance(raw, list) or bytes(raw) != replacement_raw:
        raise CurrentRevisionPdfError("replacement bytes changed before renderer request")

    text_runs = resources.get("text_runs")
    if not isinstance(text_runs, list) or not any(
        isinstance(run, dict)
        and run.get("node_id") in story_frames
        and isinstance(run.get("logical_text"), str)
        and run.get("logical_text")
        for run in text_runs
    ):
        raise CurrentRevisionPdfError("accepted edited Story did not materialize into FixedTextRuns")


def validate_renderer_result(
    *,
    wrapper: dict[str, Any],
    binding: dict[str, Any],
    operations: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    repair_sha = wrapper.get("repair_sha256")
    if not isinstance(repair_sha, str) or not SHA_RE.fullmatch(repair_sha):
        raise CurrentRevisionPdfError("renderer repair identity missing")
    result = wrapper.get("renderer_result")
    if not isinstance(result, dict) or result.get("protocol_version") != RESULT_VERSION:
        raise CurrentRevisionPdfError("fixed-PDF renderer result protocol mismatch")
    if result.get("binding") != binding:
        raise CurrentRevisionPdfError("fixed-PDF renderer binding echo mismatch")
    reports = result.get("node_reports")
    if not isinstance(reports, list):
        raise CurrentRevisionPdfError("fixed-PDF renderer node reports missing")
    by_origin = {
        report.get("origin_node_id"): report
        for report in reports
        if isinstance(report, dict) and isinstance(report.get("origin_node_id"), str)
    }
    for kind, label in (
        ("move_node", "MoveNode"),
        ("resize_node", "ResizeNode"),
        ("replace_image", "ReplaceImage"),
    ):
        node_id = operations[kind]["node_id"]
        report = by_origin.get(node_id)
        if not isinstance(report, dict):
            raise CurrentRevisionPdfError(f"{label} target absent from renderer report")
        if report.get("disposition") != "painted":
            raise CurrentRevisionPdfError(
                f"{label} target not painted: code={report.get('code')}"
            )
    return result


def build_receipt(
    *,
    source_path: pathlib.Path,
    replacement_path: pathlib.Path,
    source_raw: bytes,
    project_raw: bytes,
    project: dict[str, Any],
    replacement_raw: bytes,
    stage01_raw: bytes,
    resource_input_raw: bytes,
    resource_input: dict[str, Any],
    request_raw: bytes,
    request: dict[str, Any],
    renderer_wrapper: dict[str, Any],
    renderer_result: dict[str, Any],
    operations: dict[str, dict[str, Any]],
    pdf_raw: bytes,
) -> dict[str, Any]:
    binding = resource_input["binding"]
    target_reports = {}
    reports = renderer_result["node_reports"]
    by_origin = {report["origin_node_id"]: report for report in reports}
    for kind in ("move_node", "resize_node", "replace_image"):
        node_id = operations[kind]["node_id"]
        report = by_origin[node_id]
        target_reports[kind] = {
            "node_id": node_id,
            "disposition": report["disposition"],
            "code": report["code"],
        }

    receipt = {
        "receipt_version": RECEIPT_VERSION,
        "receipt_kind": "real_hosted",
        "producer": {
            "implementation": "rar-editor-current-revision-fixed-pdf-v2",
            "source_neutral_renderer": True,
        },
        "source": {
            "sha256": sha256_bytes(source_raw),
            "byte_len": len(source_raw),
            "immutable": True,
        },
        "stage_0_1": {
            "receipt_sha256": sha256_bytes(stage01_raw),
            "project_sha256": sha256_bytes(project_raw),
            "project_schema_version": project.get("schema_version"),
        },
        "current_revision": {
            "project_state_id": binding["project_state_id"],
            "accepted_story_state_id": operations["replace_story_range"]["after_story_state_id"],
            "story_target_state_current": True,
            "move_geometry_current": True,
            "resize_geometry_current": True,
            "replacement_image_current": True,
            "mutation_target_count": 4,
        },
        "source_neutral_chain": {
            "resource_input_sha256": sha256_bytes(resource_input_raw),
            "render_request_sha256": sha256_bytes(request_raw),
            "scene_snapshot_id": hash_id(request["scene"]),
            "resources_id": hash_id(request["resources"]),
            "renderer_repair_sha256": renderer_wrapper["repair_sha256"],
        },
        "renderer": {
            "renderer_revision": renderer_result.get("renderer_revision"),
            "target_profile": renderer_result.get("target_profile"),
            "summary": renderer_result.get("summary"),
            "target_reports": target_reports,
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
            "replacement_exact_bytes_reach_renderer_request": True,
            "move_target_painted": True,
            "resize_target_painted": True,
            "replacement_target_painted": True,
            "edited_story_materialized": True,
            "raw_document_text_emitted": False,
            "raw_source_bytes_emitted": False,
            "replacement_asset_sha_emitted": False,
            "native_pub_write_used": False,
            "pdf_renderer_reimplemented_in_rar": False,
        },
    }

    encoded = json.dumps(receipt, ensure_ascii=False, sort_keys=True)
    replacement_sha = sha256_bytes(replacement_raw)
    if replacement_sha in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked replacement asset SHA")
    if str(source_path.resolve()) in encoded or str(replacement_path.resolve()) in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked local input path")
    replacement_text = operations["replace_story_range"].get("replacement_text")
    if isinstance(replacement_text, str) and replacement_text and replacement_text in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked raw replacement Story text")
    if "replacement_text" in encoded:
        raise CurrentRevisionPdfError("retained receipt leaked replacement_text field")
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=pathlib.Path)
    parser.add_argument("--project", required=True, type=pathlib.Path)
    parser.add_argument("--replacement", required=True, type=pathlib.Path)
    parser.add_argument("--stage01-receipt", required=True, type=pathlib.Path)
    parser.add_argument("--resource-input", required=True, type=pathlib.Path)
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--request-output", required=True, type=pathlib.Path)
    parser.add_argument("--renderer-result-output", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--receipt-output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    try:
        source_raw = args.source.read_bytes()
        project_raw, project = read_json(args.project, "EditorProject")
        replacement_raw = args.replacement.read_bytes()
        stage01_raw, stage01 = read_json(args.stage01_receipt, "Stage 0.1 receipt")
        resource_input_raw, resource_input = read_json(args.resource_input, "current resource input")

        operations = validate_stage01_binding(
            source_raw=source_raw,
            project_raw=project_raw,
            project=project,
            replacement_raw=replacement_raw,
            stage01=stage01,
        )
        story_frames = validate_resource_input(
            resource_input=resource_input,
            project=project,
            operations=operations,
            source_sha=sha256_bytes(source_raw),
            replacement_raw=replacement_raw,
            stage01=stage01,
        )

        run_checked(
            [
                sys.executable,
                str(ROOT / "tools" / "run_yab259_current_fixed_pdf_resource_request.py"),
                "--yab-checkout",
                str(args.yab_checkout),
                "--input",
                str(args.resource_input),
                "--output",
                str(args.request_output),
            ],
            "current fixed-PDF resource materialization failed",
        )
        request_raw, request = read_json(args.request_output, "fixed-PDF render request")
        validate_request(
            request=request,
            resource_input=resource_input,
            operations=operations,
            replacement_raw=replacement_raw,
            story_frames=story_frames,
        )

        run_checked(
            [
                sys.executable,
                str(ROOT / "tools" / "run_yab259_fixed_pdf_packet_renderer.py"),
                "--yab-checkout",
                str(args.yab_checkout),
                "--request",
                str(args.request_output),
                "--pdf-output",
                str(args.pdf_output),
                "--result-output",
                str(args.renderer_result_output),
            ],
            "source-neutral fixed-PDF renderer failed",
        )
        _, renderer_wrapper = read_json(args.renderer_result_output, "fixed-PDF renderer result")
        renderer_result = validate_renderer_result(
            wrapper=renderer_wrapper,
            binding=resource_input["binding"],
            operations=operations,
        )

        pdf_raw = args.pdf_output.read_bytes()
        if len(pdf_raw) < 8 or not pdf_raw.startswith(b"%PDF-"):
            raise CurrentRevisionPdfError("renderer artifact is not a PDF")
        if args.source.read_bytes() != source_raw:
            raise CurrentRevisionPdfError("source PUB changed during current-revision PDF closure")

        receipt = build_receipt(
            source_path=args.source,
            replacement_path=args.replacement,
            source_raw=source_raw,
            project_raw=project_raw,
            project=project,
            replacement_raw=replacement_raw,
            stage01_raw=stage01_raw,
            resource_input_raw=resource_input_raw,
            resource_input=resource_input,
            request_raw=request_raw,
            request=request,
            renderer_wrapper=renderer_wrapper,
            renderer_result=renderer_result,
            operations=operations,
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
