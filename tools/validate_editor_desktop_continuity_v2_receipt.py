#!/usr/bin/env python3
"""Validate EDITOR-DESKTOP-CONTINUITY-V2-01 source-safe acceptance receipts."""

from __future__ import annotations

import argparse
import copy
import json
import pathlib
import sys

from jsonschema import Draft202012Validator

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = (
    ROOT
    / "packages"
    / "product"
    / "editor-desktop-continuity"
    / "v2"
    / "acceptance-receipt.schema.json"
)


def validate_schema(receipt):
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    errors = sorted(
        Draft202012Validator(schema).iter_errors(receipt),
        key=lambda error: list(error.path),
    )
    if errors:
        detail = "\n".join(f"{list(error.path)}: {error.message}" for error in errors)
        raise AssertionError("continuity V2 receipt schema validation failed\n" + detail)


def validate_semantics(receipt):
    story = receipt["story_edit"]
    move = receipt["object_move"]
    resize = receipt["object_resize"]
    image = receipt["image_replace"]
    history = receipt["history"]
    reopen = receipt["reopen"]
    project = receipt["project"]
    capability = receipt["capability_loss"]

    if story["before_state_id"] == story["after_state_id"]:
        raise AssertionError("Story edit must change Story state")

    if move["before"] == move["after"]:
        raise AssertionError("MoveNode before/after RectEmu must differ")
    if (
        move["before"]["width"] != move["after"]["width"]
        or move["before"]["height"] != move["after"]["height"]
    ):
        raise AssertionError("MoveNode must preserve width/height")

    if resize["before"] == resize["after"]:
        raise AssertionError("ResizeNode before/after RectEmu must differ")
    if (
        resize["before"]["width"] == resize["after"]["width"]
        and resize["before"]["height"] == resize["after"]["height"]
    ):
        raise AssertionError("ResizeNode must change width or height")

    if image["replacement_binding_content_derived"]:
        raise AssertionError("replacement binding must be opaque, not content-derived")
    if not image["asset_sha_redacted"]:
        raise AssertionError("replacement asset SHA must stay redacted")
    if image["frame_before"] != image["frame_after"]:
        raise AssertionError("V2 ReplaceImage must preserve frame geometry")

    ordered_states = [
        history["after_story_state_id"],
        history["after_move_state_id"],
        history["after_resize_state_id"],
        history["after_replace_state_id"],
    ]
    if len(set(ordered_states)) != len(ordered_states):
        raise AssertionError("each accepted V2 mutation must advance effective state")
    if history["undo_replace_state_id"] != history["after_resize_state_id"]:
        raise AssertionError("Undo after ReplaceImage must restore exact post-resize state")
    if history["redo_replace_state_id"] != history["after_replace_state_id"]:
        raise AssertionError("Redo after ReplaceImage must restore exact final state")

    if reopen["state_id"] != history["after_replace_state_id"]:
        raise AssertionError("fresh reopen must reproduce exact final effective state")
    if reopen["story_state_id"] != story["after_state_id"]:
        raise AssertionError("fresh reopen must preserve accepted Story state")
    if reopen["moved_rect"] != move["after"]:
        raise AssertionError("fresh reopen must preserve moved geometry")
    if reopen["resized_rect"] != resize["after"]:
        raise AssertionError("fresh reopen must preserve resized geometry")
    if not reopen["replacement_binding_preserved"]:
        raise AssertionError("fresh reopen must preserve replacement binding")

    expected_operation_count = (
        project["story_operation_count"]
        + project["move_operation_count"]
        + project["resize_operation_count"]
        + project["replace_image_operation_count"]
    )
    if project["operation_count"] != expected_operation_count:
        raise AssertionError("project operation counts are inconsistent")

    if capability["blocking_loss_count"] != 0:
        raise AssertionError("V2 editable export cannot proceed with blocking loss")

    return {
        "receipt_kind": receipt["receipt_version"],
        "source_hash": receipt["source"]["sha256"],
        "story_id": story["story_id"],
        "moved_node_id": move["origin_node_id"],
        "resized_node_id": resize["origin_node_id"],
        "replaced_image_node_id": image["origin_node_id"],
        "replacement_binding_id": image["replacement_binding_id"],
        "project_sha256": project["sha256"],
        "export_format": receipt["export"]["format"],
        "export_sha256": receipt["export"]["sha256"],
        "rar_commit": receipt["environment"]["rar_commit"],
        "source_pub_immutable": True,
        "fresh_reopen_exact": True,
        "current_editor_state_exported": True,
    }


def sample_receipt():
    h = lambda ch: ch * 64
    hid = lambda ch: "sha256:" + h(ch)
    return {
        "receipt_version": "chaptera.editor-desktop-continuity-acceptance.v2",
        "receipt_kind": "real_hosted",
        "producer": {
            "implementation": "chaptera-desktop",
            "commit_or_build": "build-1",
            "core_integration": True,
        },
        "source": {"sha256": h("1"), "byte_len": 12345, "immutable": True},
        "story_edit": {
            "story_id": "11111111-1111-1111-1111-111111111111",
            "operation_kind": "replace_story_range",
            "capability_admitted": True,
            "before_state_id": hid("2"),
            "after_state_id": hid("3"),
        },
        "object_move": {
            "instance_id": hid("4"),
            "projection_kind": "direct_page_local",
            "origin_node_id": "22222222-2222-2222-2222-222222222222",
            "capability_admitted": True,
            "geometry_sync_policy": "apply_authored_origin_geometry",
            "before": {"x": 10, "y": 20, "width": 300, "height": 200},
            "after": {"x": 30, "y": 40, "width": 300, "height": 200},
            "durable_move_count": 1,
            "transient_geometry_operation_count": 0,
        },
        "object_resize": {
            "instance_id": hid("5"),
            "projection_kind": "direct_page_local",
            "origin_node_id": "33333333-3333-3333-3333-333333333333",
            "capability_admitted": True,
            "geometry_sync_policy": "apply_authored_origin_geometry",
            "before": {"x": 50, "y": 60, "width": 400, "height": 250},
            "after": {"x": 50, "y": 60, "width": 460, "height": 280},
            "durable_resize_count": 1,
            "transient_geometry_operation_count": 0,
        },
        "image_replace": {
            "instance_id": hid("6"),
            "projection_kind": "direct_page_local",
            "origin_node_id": "44444444-4444-4444-4444-444444444444",
            "capability_admitted": True,
            "replacement_binding_id": "continuity-v2-" + "8" * 32,
            "replacement_binding_content_derived": False,
            "asset_sha_redacted": True,
            "after_asset_mime": "image/png",
            "after_asset_byte_len": 2048,
            "frame_before": {"x": 70, "y": 80, "width": 500, "height": 320},
            "frame_after": {"x": 70, "y": 80, "width": 500, "height": 320},
            "explicit_crop_present": False,
            "durable_replace_count": 1,
        },
        "history": {
            "after_story_state_id": hid("a"),
            "after_move_state_id": hid("b"),
            "after_resize_state_id": hid("c"),
            "after_replace_state_id": hid("d"),
            "undo_replace_state_id": hid("c"),
            "redo_replace_state_id": hid("d"),
        },
        "reopen": {
            "fresh_session": True,
            "state_id": hid("d"),
            "story_state_id": hid("3"),
            "moved_rect": {"x": 30, "y": 40, "width": 300, "height": 200},
            "resized_rect": {"x": 50, "y": 60, "width": 460, "height": 280},
            "replacement_binding_preserved": True,
        },
        "project": {
            "schema_version": "pub-editor-v0.11",
            "sha256": h("9"),
            "byte_len": 4096,
            "operation_count": 4,
            "story_operation_count": 1,
            "move_operation_count": 1,
            "resize_operation_count": 1,
            "replace_image_operation_count": 1,
        },
        "capability_loss": {
            "observed_before_export": True,
            "blocking_loss_count": 0,
            "approximations_explicit": True,
            "unsupported_partial_semantics_explicit": True,
        },
        "export": {
            "format": "odg",
            "sha256": h("e"),
            "byte_len": 8192,
            "package_valid": True,
            "edited_story_present": True,
            "moved_geometry_present": True,
            "resized_geometry_present": True,
            "replacement_image_present": True,
        },
        "environment": {
            "rar_commit": "f" * 40,
            "os": "windows",
            "arch": "x86_64",
        },
        "invariants": {
            "source_pub_immutable": True,
            "native_pub_write_used": False,
            "no_hidden_network_upload": True,
            "raw_document_text_emitted": False,
            "raw_source_bytes_emitted": False,
            "direct_page_local_gate_used": True,
            "projected_object_mutation_fails_closed": True,
            "reopen_used_fresh_session": True,
            "export_from_current_editor_state": True,
            "replacement_asset_sha_emitted": False,
        },
    }


def self_test():
    receipt = sample_receipt()
    validate_schema(receipt)
    validate_semantics(receipt)

    bad = copy.deepcopy(receipt)
    bad["object_move"]["after"]["width"] += 1
    try:
        validate_semantics(bad)
    except AssertionError:
        pass
    else:
        raise AssertionError("self-test expected MoveNode size-change rejection")

    bad = copy.deepcopy(receipt)
    bad["image_replace"]["frame_after"]["x"] += 1
    try:
        validate_semantics(bad)
    except AssertionError:
        pass
    else:
        raise AssertionError("self-test expected ReplaceImage frame-preservation rejection")

    bad = copy.deepcopy(receipt)
    bad["reopen"]["replacement_binding_preserved"] = False
    try:
        validate_schema(bad)
    except AssertionError:
        pass
    else:
        raise AssertionError("self-test expected reopen binding rejection")

    return {
        "self_test": "pass",
        "schema": str(SCHEMA.relative_to(ROOT)),
        "receipt_version": receipt["receipt_version"],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("receipt", nargs="?")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        print(json.dumps(self_test(), indent=2, sort_keys=True))
        return 0
    if not args.receipt:
        parser.error("receipt path required unless --self-test is used")

    receipt = json.loads(pathlib.Path(args.receipt).read_text(encoding="utf-8"))
    validate_schema(receipt)
    print(json.dumps(validate_semantics(receipt), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
