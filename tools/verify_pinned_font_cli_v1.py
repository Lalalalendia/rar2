#!/usr/bin/env python3
"""Actual licensed-font CLI boundary on a pinned real PUB, with negative controls."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import subprocess
import tempfile

SOURCE_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
FONT_SHA = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4"
FONT_ID = "f27a8036-8492-480f-8fa6-d2e775cc9f12"


def call(cli: pathlib.Path, *args: str, success: bool = True) -> dict | None:
    result = subprocess.run(
        [str(cli), *args], text=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, check=False,
    )
    if not success:
        if result.returncode == 0:
            raise AssertionError("unauthorized pinned-font CLI request unexpectedly succeeded")
        return None
    if result.returncode != 0:
        # Do not expose private source paths or source text in product logs.
        raise AssertionError("pinned-font CLI failed without a source-safe receipt")
    return json.loads(result.stdout)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", required=True, type=pathlib.Path)
    parser.add_argument("--source", required=True, type=pathlib.Path)
    args = parser.parse_args()
    assert args.cli.is_file() and args.source.is_file()
    assert args.source.stat().st_size == 291_840
    original = args.source.read_bytes()
    assert hashlib.sha256(original).hexdigest() == SOURCE_SHA

    with tempfile.TemporaryDirectory(prefix="chaptera-font-bridge-") as folder:
        root = pathlib.Path(folder)
        initial = call(args.cli, "font-initialize", str(args.source))
        assert initial["source_hash"] == SOURCE_SHA
        project = initial["project"]
        assert project["identity"]["document_id"]
        assert project["operations"] == []
        project_path = root / "project.json"
        project_path.write_text(json.dumps(project), encoding="utf-8")

        capabilities = call(args.cli, "font-capabilities", str(args.source), str(project_path))
        assert capabilities["source_hash"] == SOURCE_SHA
        assert capabilities["font_editable_story_ids"]
        story_id = capabilities["font_editable_story_ids"][0]
        doc_id = project["identity"]["document_id"]
        scope = {
            "document_id": doc_id,
            "revision_id": "sha256:" + "1" * 64,
            "scene_snapshot_id": "sha256:" + "2" * 64,
            "layout_environment_id": "sha256:" + "3" * 64,
            "font_set_fingerprint": "sha256:" + "4" * 64,
        }
        scope_path = root / "server-scope.json"
        scope_path.write_text(json.dumps(scope), encoding="utf-8")
        candidate = {
            "protocol_version": "chaptera.font-replacement-candidate.v1",
            "document_id": doc_id,
            "expected_revision_id": scope["revision_id"],
            "scene_snapshot_id": scope["scene_snapshot_id"],
            "layout_environment_id": scope["layout_environment_id"],
            "font_set_fingerprint": scope["font_set_fingerprint"],
            "resource_id": FONT_ID,
            "font_fingerprint": "sha256:" + FONT_SHA,
            "content_hash": FONT_SHA,
            "face_index": 0,
            "authority": "candidate_only_server_validation_required",
        }
        command = {
            "kind": "set_admitted_font_resource",
            "story_id": story_id, "start_scalar": 0, "end_scalar": 1,
            "candidate": candidate,
        }
        command_path = root / "intent.json"
        command_path.write_text(json.dumps(command), encoding="utf-8")

        altered = copy.deepcopy(command)
        altered["candidate"]["content_hash"] = "0" * 64
        tampered = root / "tampered-command.json"
        tampered.write_text(json.dumps(altered), encoding="utf-8")
        call(args.cli, "font-apply", str(args.source), str(project_path),
             str(tampered), str(scope_path), success=False)
        altered = copy.deepcopy(command)
        altered["candidate"]["scene_snapshot_id"] = "sha256:" + "9" * 64
        tampered.write_text(json.dumps(altered), encoding="utf-8")
        call(args.cli, "font-apply", str(args.source), str(project_path),
             str(tampered), str(scope_path), success=False)

        result = call(args.cli, "font-apply", str(args.source), str(project_path),
                      str(command_path), str(scope_path))
        assert result["protocol_version"] == "chaptera.pinned-font-operation-result.v1"
        assert result["source_hash"] == SOURCE_SHA and result["source_text_unchanged"]
        assert result["authoritative_relayout"] is False
        assert result["fixed_pdf_allowed"] is False
        operation = result["operation"]
        assert operation["kind"] == "set_text_format_property"
        assert operation["property"] == "font_resource"
        assert operation["value"]["resource_id"] == FONT_ID
        assert result["project"]["operations"] == [operation]
        assert result["project"]["identity"] == project["identity"]
        edited = root / "edited.json"
        edited.write_text(json.dumps(result["project"]), encoding="utf-8")
        reopened = call(args.cli, "font-verify", str(args.source), str(edited))
        assert reopened["full_font_re_admitted"]
        assert reopened["operation_count"] == 1
        assert reopened["layout_reshaped"] is False
        assert reopened["fixed_pdf_allowed"] is False

        # Do not mistake project JSON for independent physical-font authoring rights.
        forged = copy.deepcopy(result["project"])
        forged["identity"]["document_id"] = "00000000-0000-4000-8000-000000000000"
        bad_project = root / "wrong-project.json"
        bad_project.write_text(json.dumps(forged), encoding="utf-8")
        # A changed document identity cannot be used to reuse the original scope.
        call(args.cli, "font-apply", str(args.source), str(bad_project),
             str(command_path), str(scope_path), success=False)

        assert args.source.read_bytes() == original
        print(json.dumps({
            "receipt_kind": "chaptera.actual-pub-font-cli-bridge.v1",
            "source_sha256": SOURCE_SHA, "font_sha256": FONT_SHA,
            "real_pub": True, "rust_font_operation": True,
            "project_fresh_reopen": True, "source_bytes_unchanged": True,
            "authoritative_relayout": False, "fixed_pdf_allowed": False,
        }, sort_keys=True))


if __name__ == "__main__":
    main()
