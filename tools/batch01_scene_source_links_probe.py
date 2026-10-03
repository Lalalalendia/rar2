"""Exact-source node-role and image-target discriminator for GitHub issue #868."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

SOURCE_COMMIT = "3a1604911618a6ed4b19cf6cd48b850d3c6a066f"
ERROR_PREFIX = "CHAPTERA_SCENE_PROJECTION_ERROR "
GEOMETRY_PREFIX = "CHAPTERA_SCENE_GEOMETRY "
LINKS_PREFIX = "CHAPTERA_SCENE_SOURCE_LINKS "
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


RUST_SOURCE_LINKS = r"""
// Temporary measurement only. All retained values are fixed enums, flags or counts.
fn research_scene_identity<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

fn research_scene_kind<T: serde::Serialize>(value: &T) -> &'static str {
    match serde_json::to_value(value).ok().as_ref().and_then(serde_json::Value::as_str) {
        Some("shape") => "shape",
        Some("text_frame") => "text_frame",
        Some("image_frame") => "image_frame",
        Some("vector_path") => "vector_path",
        Some("group") => "group",
        Some("connector") => "connector",
        Some("table") => "table",
        Some("placed_artifact") => "placed_artifact",
        Some("unsupported") => "unsupported",
        _ => "unclassified",
    }
}

fn research_scene_extent<T: serde::Serialize>(bounds: &T) -> &'static str {
    let Ok(value) = serde_json::to_value(bounds) else { return "unclassified"; };
    let (Some(width), Some(height)) = (value.get("width").and_then(serde_json::Value::as_i64), value.get("height").and_then(serde_json::Value::as_i64)) else { return "unclassified"; };
    if width < 0 || height < 0 { "negative" }
    else if width == 0 && height == 0 { "both_zero" }
    else if width == 0 { "zero_width" }
    else if height == 0 { "zero_height" }
    else { "positive" }
}

fn research_scene_increment(histogram: &mut std::collections::BTreeMap<String, (serde_json::Value, u64)>, value: serde_json::Value) {
    let key = serde_json::to_string(&value).expect("finite diagnostic object serializes");
    histogram.entry(key).and_modify(|entry| entry.1 += 1).or_insert((value, 1));
}

fn research_scene_source_links(bundle: &pub_viewer::ViewerOpenBundle) {
    use std::collections::{BTreeMap, BTreeSet};
    let graph = &bundle.resolved_graph;
    let nodes = graph.nodes.values().map(|node| (research_scene_identity(&node.header.id), node)).collect::<BTreeMap<_, _>>();
    let pages = graph.pages.keys().map(research_scene_identity).collect::<BTreeSet<_>>();
    let selected = bundle.geometry.document.pages.iter().map(|page| research_scene_identity(&page.id)).collect::<BTreeSet<_>>();
    let scene_nodes = bundle.geometry.scene.nodes.iter().map(|node| research_scene_identity(&node.origin)).collect::<BTreeSet<_>>();
    let page_relation = |node_id: &str| {
        let Some(node) = nodes.get(node_id) else { return "missing_node"; };
        let mut current = research_scene_identity(&node.header.parent_id);
        let mut seen = BTreeSet::new();
        for _ in 0..=nodes.len() {
            if selected.contains(&current) { return "selected_page"; }
            if pages.contains(&current) { return "unselected_page"; }
            if !seen.insert(current.clone()) { return "cycle"; }
            let Some(parent) = nodes.get(&current) else { return "missing_parent"; };
            current = research_scene_identity(&parent.header.parent_id);
        }
        "cycle"
    };
    let describe = |node_id: &str| {
        let Some(node) = nodes.get(node_id) else {
            return serde_json::json!({"graph_present":false,"kind":"unclassified","source_extent":"unclassified","page_relation":"missing_node","parent_kind":"missing","direct_parent_selected":false,"source_ref_count":0,"projection_ref_count":0,"story_frame":false,"story_resolved":false,"table":false,"legacy_ole":false,"image_slot":false,"officeart_type":"none","viewer_fill":false,"viewer_line":false});
        };
        let parent = research_scene_identity(&node.header.parent_id);
        let parent_kind = if pages.contains(&parent) { "page" } else { nodes.get(&parent).map(|parent| research_scene_kind(&parent.kind)).unwrap_or("missing") };
        let paint = bundle.geometry.paints.iter().find(|paint| paint.node_id == node.header.id);
        let officeart_type = match node.payload.officeart_shape_type { None => "none", Some(20) => "msospt_20", Some(202) => "msospt_202", Some(_) => "other" };
        let projection_ref_count = node.header.source_refs.iter().filter(|reference| serde_json::to_value(&reference.role).ok().as_ref().and_then(serde_json::Value::as_str) == Some("projection")).count();
        serde_json::json!({
            "graph_present":true,"kind":research_scene_kind(&node.kind),"source_extent":research_scene_extent(&node.header.bounds),
            "page_relation":page_relation(node_id),"parent_kind":parent_kind,"direct_parent_selected":selected.contains(&parent),
            "source_ref_count":node.header.source_refs.len(),"projection_ref_count":projection_ref_count,
            "story_frame":node.payload.story_frame.is_some(),"story_resolved":node.payload.story_frame.as_ref().is_some_and(|frame| frame.story_id.is_some()),
            "table":node.payload.table.is_some(),"legacy_ole":node.payload.legacy_ole.is_some(),"image_slot":node.payload.image_slot.is_some(),
            "officeart_type":officeart_type,"viewer_fill":paint.is_some_and(|paint| paint.solid_fill_rgb.is_some()),"viewer_line":paint.is_some_and(|paint| paint.solid_line.is_some())
        })
    };
    let mut bad = BTreeMap::new();
    let mut first_bad = None;
    let mut bad_count = 0u64;
    for scene_node in &bundle.geometry.scene.nodes {
        let extent = research_scene_extent(&scene_node.bounds);
        if extent == "positive" { continue; }
        bad_count += 1;
        let id = research_scene_identity(&scene_node.origin);
        let mut value = describe(&id);
        value["scene_extent"] = serde_json::json!(extent);
        value["bounds_equal_source"] = serde_json::json!(nodes.get(&id).is_some_and(|node| serde_json::to_value(&node.header.bounds).ok() == serde_json::to_value(&scene_node.bounds).ok()));
        if first_bad.is_none() { first_bad = Some(value.clone()); }
        research_scene_increment(&mut bad, value);
    }
    let mut missing = BTreeMap::new();
    let mut placement_count = 0u64;
    let mut resource_count = 0u64;
    for image in &bundle.geometry.images {
        for node_id in &image.node_ids {
            let id = research_scene_identity(node_id);
            if scene_nodes.contains(&id) { continue; }
            resource_count += 1;
            let mut value = describe(&id);
            value["binding_role"] = serde_json::json!("resource_node");
            value["resource_member"] = serde_json::json!(true);
            value["crop"] = serde_json::json!(false);
            value["recolor"] = serde_json::json!(false);
            value["rotation"] = serde_json::json!(false);
            research_scene_increment(&mut missing, value);
        }
        for placement in &image.placements {
            let id = research_scene_identity(&placement.node_id);
            if scene_nodes.contains(&id) { continue; }
            placement_count += 1;
            let mut value = describe(&id);
            value["binding_role"] = serde_json::json!("placement");
            value["resource_member"] = serde_json::json!(image.node_ids.contains(&placement.node_id));
            value["crop"] = serde_json::json!(placement.source_window.is_some());
            value["recolor"] = serde_json::json!(placement.recolor.is_some());
            value["rotation"] = serde_json::json!(placement.content_rotation_degrees.is_some());
            research_scene_increment(&mut missing, value);
        }
    }
    let histogram_rows = |histogram: BTreeMap<String, (serde_json::Value, u64)>| histogram.into_values().map(|(mut value, count)| { value["count"] = serde_json::json!(count); value }).collect::<Vec<_>>();
    let family = match graph.source.format_version.as_deref() { Some("0x2c") => "0x2c", Some("0x22-quill") => "0x22-quill", Some("0x22-noquill") => "0x22-noquill", _ => "unclassified" };
    let result = serde_json::json!({
        "family":family,"source_document_pages":graph.document.pages.len(),"source_registry_pages":graph.pages.len(),"source_nodes":nodes.len(),"scene_nodes":scene_nodes.len(),
        "page_profile_applied":bundle.geometry.document.diagnostics.iter().any(|diagnostic| diagnostic.code == "viewer.page_projection.family_profile_applied"),
        "bad_scene_nodes":bad_count,"first_bad_scene_node":first_bad,"bad_node_histogram":histogram_rows(bad),
        "missing_image_placements":placement_count,"missing_image_resource_nodes":resource_count,"missing_image_histogram":histogram_rows(missing)
    });
    eprintln!("CHAPTERA_SCENE_SOURCE_LINKS {}", result);
}
"""


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def instrument(source: str) -> str:
    if source.count(ERROR_ARM) != 1:
        raise ValueError("projection_error_anchor_mismatch")
    marker = "            match from_viewer_geometry_with_fonts("
    if source.count(marker) != 1:
        raise ValueError("projection_call_anchor_mismatch")
    source = source.replace(ERROR_ARM, INSTRUMENTED_ARM)
    source = source.replace(marker, RUST_GEOMETRY + "\n            research_scene_source_links(&bundle);\n" + marker)
    return source + RUST_CLASSIFIER + RUST_SOURCE_LINKS


KINDS = {"shape", "text_frame", "image_frame", "vector_path", "group", "connector",
         "table", "placed_artifact", "unsupported", "unclassified"}
EXTENTS = {"positive", "both_zero", "zero_width", "zero_height", "negative", "unclassified"}
PAGE_RELATIONS = {"selected_page", "unselected_page", "missing_node", "missing_parent", "cycle"}
NODE_ENUMS = {"kind": KINDS, "source_extent": EXTENTS, "page_relation": PAGE_RELATIONS,
              "parent_kind": KINDS | {"page", "missing"},
              "officeart_type": {"none", "msospt_20", "msospt_202", "other"}}
NODE_BOOLS = {"graph_present", "direct_parent_selected", "story_frame", "story_resolved",
              "table", "legacy_ole", "image_slot", "viewer_fill", "viewer_line"}
NODE_COUNTS = {"source_ref_count", "projection_ref_count"}
LINK_COUNTS = {"source_document_pages", "source_registry_pages", "source_nodes", "scene_nodes",
               "bad_scene_nodes", "missing_image_placements", "missing_image_resource_nodes"}


def bounded_count(value) -> bool:
    return type(value) is int and 0 <= value <= 10_000_000


def valid_node_descriptor(value: dict, role: str) -> bool:
    enums, flags, counts = dict(NODE_ENUMS), set(NODE_BOOLS), set(NODE_COUNTS)
    if role == "bad":
        enums["scene_extent"] = EXTENTS
        flags.add("bounds_equal_source")
    elif role == "missing":
        enums["binding_role"] = {"resource_node", "placement"}
        flags |= {"resource_member", "crop", "recolor", "rotation"}
    if role != "first":
        counts.add("count")
    if role == "first":
        enums["scene_extent"] = EXTENTS
        flags.add("bounds_equal_source")
    return (isinstance(value, dict) and set(value) == set(enums) | flags | counts
            and all(type(value[k]) is str and value[k] in allowed for k, allowed in enums.items())
            and all(type(value[k]) is bool for k in flags)
            and all(bounded_count(value[k]) for k in counts))


def valid_source_links(value: dict) -> bool:
    keys = LINK_COUNTS | {"family", "page_profile_applied", "first_bad_scene_node",
                          "bad_node_histogram", "missing_image_histogram"}
    if not isinstance(value, dict) or set(value) != keys:
        return False
    if (type(value["family"]) is not str
            or value["family"] not in {"0x2c", "0x22-quill", "0x22-noquill", "unclassified"}
            or type(value["page_profile_applied"]) is not bool
            or not all(bounded_count(value[k]) for k in LINK_COUNTS)):
        return False
    for key, role in (("bad_node_histogram", "bad"), ("missing_image_histogram", "missing")):
        rows = value[key]
        if not isinstance(rows, list) or len(rows) > 128 or not all(valid_node_descriptor(row, role) for row in rows):
            return False
    first = value["first_bad_scene_node"]
    if value["bad_scene_nodes"]:
        if not valid_node_descriptor(first, "first"):
            return False
    elif first is not None:
        return False
    if sum(row["count"] for row in value["bad_node_histogram"]) != value["bad_scene_nodes"]:
        return False
    for role, key in (("placement", "missing_image_placements"), ("resource_node", "missing_image_resource_nodes")):
        if sum(row["count"] for row in value["missing_image_histogram"] if row["binding_role"] == role) != value[key]:
            return False
    return True


def safe_diagnostics(stderr: str) -> dict:
    reasons = []
    geometry = []
    links = []
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
        if line.startswith(LINKS_PREFIX):
            try:
                value = json.loads(line[len(LINKS_PREFIX):])
                if valid_source_links(value):
                    links.append(value)
            except (ValueError, TypeError):
                pass
    return {
        "projection_reason": reasons[0] if len(reasons) == 1 else None,
        "valid_reason_marker_count": len(reasons),
        "geometry_counts": geometry[0] if len(geometry) == 1 else None,
        "source_links": links[0] if len(links) == 1 else None,
        "valid_source_link_marker_count": len(links),
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
                    links = row["source_links"]
                    ok = (ok and links is not None and links["family"] != "unclassified"
                          and links["bad_scene_nodes"] == 0 and links["missing_image_placements"] == 0
                          and links["missing_image_resource_nodes"] == 0)
                    row["measurement_status"] = "control_passed" if ok else "control_failed"
                else:
                    ok = (receipt.get("classification") == "unsupported"
                          and receipt.get("terminal_code") == "reader_scene_projection_failed"
                          and row["projection_reason"] not in (None, "unclassified")
                          and row["geometry_counts"] is not None)
                    links = row["source_links"]
                    ok = (ok and links is not None and links["family"] != "unclassified"
                          and ((row["projection_reason"] == "node_bounds" and links["bad_scene_nodes"] > 0)
                               or (row["projection_reason"] == "image_placement_unknown_node"
                                   and links["missing_image_placements"] > 0)))
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
        "schema": "chaptera.batch01-scene-source-links-probe.v1",
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
