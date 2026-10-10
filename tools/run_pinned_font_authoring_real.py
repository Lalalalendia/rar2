#!/usr/bin/env python3
"""End-to-end actual PUB -> trusted-scene -> selected Story -> Rust font Project.

This is a local producer integration witness, not the authenticated HTTP
font commit or authoritative reshaping/PDF consumer.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "services/editor-api"))

from adapt_viewer_scene_v1 import adapt_viewer_geometry
from live_font_delivery_v1 import bind_font_set_to_scene
from pinned_opentype_resource_v1 import ABEL_RESOURCE_ID, ABEL_SHA256, load_pinned_abel

PRODUCER = ROOT / "tools/native-pub-candidate-cli/Cargo.toml"
PROBE = ROOT / "target/local-font-ui"
SOURCE_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
RECEIPT = (
    ROOT / "packages/protocol/revision/v1/producer-receipts/sample-newsletter.real.json"
)


def worker(*args: str, ok: bool = True) -> dict | None:
    run = subprocess.run(
        ["cargo", "run", "--quiet", "--manifest-path", str(PRODUCER),
         "--bin", "pinned_font_authoring_v1", "--", *args],
        cwd=ROOT, capture_output=True, text=True, check=False, timeout=120,
    )
    if not ok:
        if run.returncode == 0:
            raise RuntimeError(f"Rust accepted rejected font request: {args}")
        return None
    if run.returncode:
        raise RuntimeError("Rust font worker failed: " + run.stderr[-3000:])
    return json.loads(run.stdout)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--viewer", required=True, type=pathlib.Path)
    args = parser.parse_args()
    source = args.fixture.resolve(strict=True)
    viewer_path = args.viewer.resolve(strict=True)
    source_bytes = source.read_bytes()
    assert len(source_bytes) == 291840
    assert hashlib.sha256(source_bytes).hexdigest() == SOURCE_SHA
    output = PROBE
    output.mkdir(parents=True, exist_ok=True)

    baseline = worker("init", str(source))
    assert baseline["source_hash"] == SOURCE_SHA
    assert baseline["identity"]["document_id"]
    project_path = output / "pinned-font-baseline.project.json"
    project_path.write_text(json.dumps(baseline, sort_keys=True) + "\n", encoding="utf-8")

    probe = worker("probe", str(source), str(project_path))
    assert probe["protocol_version"] == "chaptera.local-font-format-probe.v1"
    assert probe["source_hash"] == SOURCE_SHA
    assert probe["stories"], "actual Publisher Story format probe is empty"
    selected = probe["stories"][0]
    assert selected["story_scalar_len"] > 0

    real_receipt = json.loads(RECEIPT.read_text(encoding="utf-8"))
    viewer = json.loads(viewer_path.read_text(encoding="utf-8"))
    browser_scene = adapt_viewer_geometry(
        viewer, baseline["identity"]["document_id"], real_receipt["baseline"]["revision_id"]
    )
    scene = bind_font_set_to_scene(browser_scene, load_pinned_abel())
    assert scene["source_hash"] == SOURCE_SHA
    scene_path = output / "pinned-font-trusted-scene.json"
    scene_path.write_text(json.dumps(scene, sort_keys=True) + "\n", encoding="utf-8")

    context = scene["layout_environment"]
    command = {
        "protocol_version": "chaptera.local-pinned-font-intent.v1",
        "story_id": selected["story_id"],
        "start_scalar": 0, "end_scalar": 1,
        "expected_state_hash": selected["expected_state_hash"],
        "candidate": {
            "protocol_version": "chaptera.font-replacement-candidate.v1",
            "document_id": scene["document_id"],
            "expected_revision_id": scene["revision_id"],
            "scene_snapshot_id": scene["snapshot_id"],
            "layout_environment_id": context["environment_id"],
            "font_set_fingerprint": context["font_set_fingerprint"],
            "resource_id": ABEL_RESOURCE_ID,
            "font_fingerprint": "sha256:" + ABEL_SHA256,
            "content_hash": ABEL_SHA256,
            "face_index": 0,
            "authority": "candidate_only_server_validation_required",
        },
    }
    command_path = output / "pinned-font-command.json"
    command_path.write_text(json.dumps(command, sort_keys=True) + "\n", encoding="utf-8")
    result = worker("apply", str(source), str(project_path),
                    str(scene_path), str(command_path))
    assert result["protocol_version"] == "chaptera.local-pinned-font-apply.v1"
    assert result["source_hash"] == SOURCE_SHA
    assert result["story_id"] == selected["story_id"]
    assert result["canonical_operation"] == result["project"]["operations"][-1]
    assert result["canonical_operation"]["kind"] == "set_text_format_property"
    assert result["fixed_output_eligible"] is False
    assert result["layout_authority"] == "partial_not_reshaped"
    assert result["fresh_reopen_with_exact_bytes"] is True
    assert result["source_story_text_unchanged"] is True
    assert hashlib.sha256(source.read_bytes()).hexdigest() == SOURCE_SHA
    project_after = output / "pinned-font-edited.project.json"
    project_after.write_text(
        json.dumps(result["project"], ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    # Consume canonical Rust glyph coverage from the actually edited
    # Publisher Story. Only the replacement character has independently
    # admitted full physical bytes; every source-font scalar remains unknown.
    packet = worker(
        "glyph-spans", str(source), str(project_after), selected["story_id"],
    )
    assert packet["protocol_version"] == "chaptera.local-current-exact-glyph-spans.v1"
    assert packet["project_state_id"] == result["project_state_id"]
    assert packet["story_format_state_hash"] == result["format_state_hash"]
    assert packet["story_scalar_len"] == selected["story_scalar_len"]
    assert packet["admitted_scalar_count"] == 1
    assert packet["source_unresolved_scalar_count"] == selected["story_scalar_len"] - 1
    assert packet["shaped_glyph_count"] >= 1
    assert packet["all_scalars_shaped"] is False
    assert packet["authoritative_line_breaks"] is False
    assert packet["fixed_pdf_allowed"] is False
    exact = [span for span in packet["spans"] if span["kind"] == "admitted_exact"]
    unresolved = [span for span in packet["spans"] if span["kind"] == "source_unresolved"]
    assert exact and unresolved
    assert exact[0]["start_scalar"] == 0 and exact[0]["end_scalar"] == 1
    assert exact[0]["identity"]["content_hash"] == ABEL_SHA256
    assert all(glyph["cluster"] == 0 for glyph in exact[0]["shaped"]["glyphs"])
    assert all(span["source_font_binding_id"] for span in unresolved)
    (output / "pinned-font-physical-glyph-spans.json").write_text(
        json.dumps(packet, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    # Stale revision/snapshot, wrong physical identity and incorrect overlay
    # hash are each independently rejected by the canonical Rust worker.
    for kind in ("stale", "wrong_resource", "wrong_hash"):
        rejected = json.loads(json.dumps(command))
        if kind == "stale":
            rejected["candidate"]["scene_snapshot_id"] = "sha256:" + "0" * 64
        elif kind == "wrong_resource":
            rejected["candidate"]["resource_id"] = scene["document_id"]
        else:
            rejected["expected_state_hash"] = "sha256:" + "f" * 64
        denied_file = output / (f"deny-{kind}.json")
        denied_file.write_text(json.dumps(rejected) + "\n", encoding="utf-8")
        worker("apply", str(source), str(project_path), str(scene_path),
               str(denied_file), ok=False)
    receipt = {
        "receipt_kind": "chaptera.pinned-real-pub-font-rust-worker.v1",
        "real_publisher_source_sha256": SOURCE_SHA,
        "exact_full_abel_sha256": ABEL_SHA256,
        "story_id": selected["story_id"],
        "start_scalar": 0, "end_scalar": 1,
        "project_state_id": result["project_state_id"],
        "independent_reopen": True,
        "denials": ["stale", "wrong_resource", "wrong_hash"],
        "source_unchanged": True,
        "layout_authority": result["layout_authority"],
        "exact_physical_glyphs": packet["shaped_glyph_count"],
        "exact_physical_scalars": packet["admitted_scalar_count"],
        "source_unresolved_scalars": packet["source_unresolved_scalar_count"],
        "line_breaks_verified": False,
        "fixed_output_eligible": False,
    }
    (output / "pinned-font-apply-receipt.json").write_text(
        json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print("REAL_PUB_FONT_WORKER_OK " + json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
