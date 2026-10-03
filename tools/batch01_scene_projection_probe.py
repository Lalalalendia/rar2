"""Exact-source, temporary instrumentation for GitHub issue #868."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

SOURCE_COMMIT = "52096e3a974f4afaa1569e49f578c2545dfcd3bb"
ERROR_PREFIX = "CHAPTERA_SCENE_PROJECTION_ERROR "
GEOMETRY_PREFIX = "CHAPTERA_SCENE_GEOMETRY "
REASONS = {
    "source_identity", "page_index", "page_dimensions", "duplicate_page",
    "duplicate_node", "node_bounds", "node_ancestry", "story_binding",
    "image_binding", "image_placement_unknown_node", "image_resource_unknown_node",
    "duplicate_image_resource", "multiple_image_resources", "duplicate_image_window",
    "duplicate_image_recolor", "table_binding", "paint_binding", "text_binding",
    "projected_instance", "scene_byte_cap", "fallback_font", "unclassified",
}
FIXTURES = [
    ("001", "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", 72704, "control", "warning_membership_unknown"),
    ("018", "48384326430f61ecd6b924d0010631eb72838b0ad09dc33e0e53f3998ff642fe", 343552, "target", "warning_membership_unknown"),
    ("023", "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", 4573696, "target", "warning_membership_unknown"),
    ("072", "7860acc670667c456fb29048a4cfa1840e4a7d5b57b7b2a2975c8d76929063dc", 314880, "target", "empty_visual_output"),
]
LIMITS = {"wall_seconds": 120, "cpu_seconds": 90, "address_space_mb": 768,
          "open_files": 64, "output_file_mb": 32}

# Classify only static error families; never print the source-derived error.
RUST_CLASSIFIER = """
fn research_scene_projection_reason(error: &str) -> &'static str {
    if error.starts_with("Viewer source hash ") { "source_identity" }
    else if error == "Viewer page index must be one-based" { "page_index" }
    else if error == "Viewer page dimensions must be positive" { "page_dimensions" }
    else if error.starts_with("duplicate Viewer page id ") { "duplicate_page" }
    else if error.starts_with("duplicate Viewer node id ") { "duplicate_node" }
    else if error.starts_with("Viewer node ") && error.ends_with(" has non-positive bounds") { "node_bounds" }
    else if error.starts_with("Viewer node parent cycle ") || error.starts_with("missing Viewer node ")
        || error.starts_with("Viewer node ") && error.ends_with(" resolves to neither page nor node") { "node_ancestry" }
    else if error.starts_with("story frame references unknown node ") { "story_binding" }
    else if error.starts_with("image placement references unknown node ") { "image_placement_unknown_node" }
    else if error.starts_with("image resource references unknown node ") { "image_resource_unknown_node" }
    else if error.starts_with("duplicate Viewer image resource ") { "duplicate_image_resource" }
    else if error.starts_with("node ") && error.ends_with(" has multiple image resources") { "multiple_image_resources" }
    else if error.starts_with("duplicate image source window ") { "duplicate_image_window" }
    else if error.starts_with("duplicate image recolor ") { "duplicate_image_recolor" }
    else if error.starts_with("image ") || error.starts_with("duplicate image ") { "image_binding" }
    else if error.starts_with("table ") || error.starts_with("duplicate table binding ") { "table_binding" }
    else if error.starts_with("duplicate paint binding ") { "paint_binding" }
    else if error.starts_with("text fragment ") || error.starts_with("direct render-plan ")
        || error.starts_with("duplicate direct render-plan text ") || error.starts_with("duplicate text ")
        || error.starts_with("semantic binding ") || error.starts_with("node ") && error.contains(" conflicting semantic kinds ") { "text_binding" }
    else if error.starts_with("projected Scene ") || error.starts_with("duplicate projected Scene ")
        || error.starts_with("duplicate Viewer projected ") || error.starts_with("render-plan projected ") { "projected_instance" }
    else if error.starts_with("Reader Scene exceeds serialized byte cap") { "scene_byte_cap" }
    else if error.starts_with("shared fallback font validation failed:") { "fallback_font" }
    else { "unclassified" }
}
"""
RUST_GEOMETRY = """
            // Counts only: no source text, node identifiers, or geometry coordinates.
            let mut counts = [0u64; 4];
            for node in &bundle.geometry.scene.nodes {
                if let Ok(value) = serde_json::to_value(&node.bounds) {
                    if let (Some(width), Some(height)) = (
                        value.get("width").and_then(serde_json::Value::as_i64),
                        value.get("height").and_then(serde_json::Value::as_i64),
                    ) {
                        counts[0] += u64::from(width == 0);
                        counts[1] += u64::from(height == 0);
                        counts[2] += u64::from(width < 0);
                        counts[3] += u64::from(height < 0);
                    }
                }
            }
            eprintln!("CHAPTERA_SCENE_GEOMETRY {} {} {} {} {} {}",
                bundle.geometry.document.pages.len(), bundle.geometry.scene.nodes.len(),
                counts[0], counts[1], counts[2], counts[3]);
"""
ERROR_ARM = """                Err(_) => (
                    "unsupported".to_owned(),
                    Some("reader_scene_projection_failed".to_owned()),
                    None,
                    None,
                    structural_scan_duration_us,
                    Some(duration_us(scene_started.elapsed())),
                ),"""
INSTRUMENTED_ARM = """                Err(error) => {
                    eprintln!("CHAPTERA_SCENE_PROJECTION_ERROR {}",
                        research_scene_projection_reason(&error));
                    (
                        "unsupported".to_owned(),
                        Some("reader_scene_projection_failed".to_owned()),
                        None,
                        None,
                        structural_scan_duration_us,
                        Some(duration_us(scene_started.elapsed())),
                    )
                },"""


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def instrument(source: str) -> str:
    if source.count(ERROR_ARM) != 1:
        raise ValueError("projection_error_anchor_mismatch")
    marker = "            match from_viewer_geometry_with_fonts("
    if source.count(marker) != 1:
        raise ValueError("projection_call_anchor_mismatch")
    source = source.replace(ERROR_ARM, INSTRUMENTED_ARM)
    source = source.replace(marker, RUST_GEOMETRY + marker)
    return source + RUST_CLASSIFIER


def safe_diagnostics(stderr: str) -> dict:
    reasons = []
    geometry = []
    for line in stderr.splitlines():
        if line.startswith(ERROR_PREFIX):
            token = line[len(ERROR_PREFIX):]
            if token in REASONS:
                reasons.append(token)
        if line.startswith(GEOMETRY_PREFIX):
            fields = line[len(GEOMETRY_PREFIX):].split(" ")
            if len(fields) == 6 and all(re.fullmatch(r"[0-9]{1,10}", v) for v in fields):
                geometry.append(dict(zip(
                    ("pages", "nodes", "zero_width_nodes", "zero_height_nodes",
                     "negative_width_nodes", "negative_height_nodes"), map(int, fields))))
    return {
        "projection_reason": reasons[0] if len(reasons) == 1 else None,
        "valid_reason_marker_count": len(reasons),
        "geometry_counts": geometry[0] if len(geometry) == 1 else None,
    }


def observe(source_root: Path, corpus: Path, output: Path, probe_commit: str) -> dict:
    # subprocess.cwd differs from this caller; all data paths cross that boundary absolutely.
    source_root, corpus, output = source_root.resolve(), corpus.resolve(), output.resolve()
    worker = source_root / "target/scene-projection-probe/debug/chaptera"
    harness = source_root / "tools/migration_pdf_worker_isolation.py"
    rows = []
    output.parent.mkdir(parents=True, exist_ok=True)
    work_root = output.parent / "private-worker-results"
    for index, (fid, sha, size, role, ref_state) in enumerate(FIXTURES, 1):
        row = {"fixture_id": fid, "source_sha256": sha, "source_byte_len": size,
               "role": role, "publisher_reference_state": ref_state}
        source = corpus / (sha + ".pub")
        worker_output = work_root / fid
        stage = "fixture_admission"
        try:
            data = source.read_bytes()
            if len(data) != size or digest(data) != sha:
                raise ValueError("fixture_identity_mismatch")
            command = [
                "python3", str(harness), "run", "--output-dir", str(worker_output),
                "--input", str(source), "--timeout", "120", "--cpu-seconds", "90",
                "--address-space-mb", "768", "--open-files", "64", "--output-file-mb", "32",
                "--clear-environment", "--", str(worker), "guest-reader-scene",
                "--session-id", "guest:" + str(index).zfill(32),
                "--expected-sha256", sha, "--expected-byte-len", str(size),
            ]
            stage = "worker_execution"
            completed = subprocess.run(command, cwd=source_root, capture_output=True,
                                       text=True, timeout=135, check=False)
            isolation = json.loads(completed.stdout)
            row.update({k: isolation.get(k) for k in ("status", "exit_code", "timed_out", "network_policy")})
            row.update(safe_diagnostics(str(isolation.get("stderr_tail", ""))))
            if isolation.get("status") != "success":
                row["measurement_status"] = "isolation_failed"
            else:
                stage = "worker_receipt_read"
                receipt_bytes = (worker_output / "result.json").read_bytes()
                receipt = json.loads(receipt_bytes)
                stage = "worker_receipt_admission"
                if receipt.get("source_sha256") != sha or receipt.get("source_byte_len") != size:
                    raise ValueError("worker_receipt_identity_mismatch")
                if receipt.get("filesystem_confinement") is not True:
                    raise ValueError("worker_receipt_confinement_missing")
                if isolation.get("network_policy") != "seccomp_default_deny":
                    raise ValueError("worker_network_policy_mismatch")
                row.update({
                    "worker_receipt_sha256": digest(receipt_bytes),
                    "reader_classification": receipt.get("classification"),
                    "reader_terminal_code": receipt.get("terminal_code"),
                    "structural_scan_duration_us": receipt.get("structural_scan_duration_us"),
                    "scene_duration_us": receipt.get("scene_duration_us"),
                })
                if role == "control":
                    ok = (receipt.get("classification") in ("partial", "supported")
                          and receipt.get("terminal_code") is None
                          and row["valid_reason_marker_count"] == 0
                          and row["geometry_counts"] is not None)
                    row["measurement_status"] = "control_passed" if ok else "control_failed"
                else:
                    ok = (receipt.get("classification") == "unsupported"
                          and receipt.get("terminal_code") == "reader_scene_projection_failed"
                          and row["projection_reason"] not in (None, "unclassified")
                          and row["geometry_counts"] is not None)
                    row["measurement_status"] = "classified" if ok else "inconclusive"
        except (ValueError, OSError, subprocess.TimeoutExpired):
            # Exceptions may contain source paths/raw subprocess output; retain a fixed code only.
            row["measurement_status"] = "probe_input_or_harness_failed"
            row["probe_failure_stage"] = stage
        finally:
            shutil.rmtree(worker_output, ignore_errors=True)
        rows.append(row)
        print(json.dumps(row, sort_keys=True))
    complete = (rows[0]["measurement_status"] == "control_passed"
                and all(row["measurement_status"] == "classified" for row in rows[1:]))
    result = {
        "schema": "chaptera.batch01-scene-projection-probe.v1",
        "issue": 868, "source_commit_sha": SOURCE_COMMIT, "probe_commit_sha": probe_commit,
        "worker_binary_sha256": digest(worker.read_bytes()),
        "instrumented_worker_source_sha256": digest(
            (source_root / "apps/chaptera-server/src/guest_reader_worker.rs").read_bytes()),
        "worker_limits": LIMITS, "complete": complete, "fixtures": rows,
        "claims": {"raw_source_bytes_emitted": False, "raw_source_text_emitted": False,
                   "raw_stderr_emitted": False, "parser_or_scene_semantics_changed": False,
                   "publisher_format_law_claimed": False},
    }
    output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("instrument", "observe"))
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--probe-commit", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.probe_commit):
        parser.error("probe commit must be a full lowercase SHA")
    if args.action == "instrument":
        path = args.source_root / "apps/chaptera-server/src/guest_reader_worker.rs"
        original = path.read_bytes()
        patched = instrument(original.decode("utf-8"))
        path.write_text(patched)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({
            "schema": "chaptera.scene-projection-instrumentation.v1",
            "source_commit_sha": SOURCE_COMMIT, "probe_commit_sha": args.probe_commit,
            "original_source_sha256": digest(original), "patched_source_sha256": digest(path.read_bytes()),
            "replacement_count": 1, "changed_product_terminal_codes": False,
        }, indent=2, sort_keys=True) + "\n")
        return 0
    if args.corpus is None:
        parser.error("--corpus is required for observe")
    result = observe(args.source_root, args.corpus, args.output, args.probe_commit)
    return 0 if result["complete"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
