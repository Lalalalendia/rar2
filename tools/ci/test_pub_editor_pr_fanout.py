#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path


MODULE = Path(__file__).with_name("pub_editor_pr_fanout.py")
spec = importlib.util.spec_from_file_location("pub_editor_pr_fanout", MODULE)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def classify(paths, base=None, head=None):
    return mod.classify_continuity_v2_windows(
        paths,
        base_lib_source=base,
        head_lib_source=head,
    )



def assert_continuity_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/editor-desktop-continuity-v2-windows.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor Continuity V2 scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.continuity_v2_windows == 'true'",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "Continuity V2 selective consumer lost required base-authority/fail-closed wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/editor-desktop-continuity-v2-windows.yml" in contract
    ), "cheap contract must run when the Continuity V2 consumer wiring changes"

def main() -> None:
    assert_continuity_consumer_wiring()

    base = """mod duplicate_authored_rectangle_v1;
mod imported_paragraphs_v1;

pub use duplicate_authored_rectangle_v1::{
    DuplicateAuthoredRectangleErrorV1, DuplicateAuthoredRectanglePlanV1,
};
pub use imported_paragraphs_v1::ImportedParagraphV1;

pub fn shared_core() {}
"""
    head_facade = """mod duplicate_authored_rectangle_v1;
mod imported_paragraph_alignment_v1;
mod imported_paragraphs_v1;

pub use duplicate_authored_rectangle_v1::{
    DuplicateAuthoredRectangleErrorV1, DuplicateAuthoredRectanglePlanV1,
};
pub use imported_paragraph_alignment_v1::{
    ImportedParagraphAlignmentValueV1, ImportedParagraphBaseAlignmentErrorV1,
    ImportedParagraphBaseAlignmentV1,
};
pub use imported_paragraphs_v1::ImportedParagraphV1;

pub fn shared_core() {}
"""
    assert mod.facade_change_is_safe(base, head_facade)

    run, reason = classify(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_CONTINUITY_V2_MODULES["imported_paragraph_alignment_v1"],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_continuity_pub_editor_slice"

    run, reason = classify(
        [mod.SAFE_CONTINUITY_V2_MODULES["duplicate_authored_rectangle_v1"]]
    )
    assert run is False and reason == "proven_non_continuity_pub_editor_slice"

    core_head = head_facade.replace(
        "pub fn shared_core() {}",
        "pub fn shared_core() { eprintln!(\"semantic change\"); }",
    )
    assert not mod.facade_change_is_safe(base, core_head)
    run, reason = classify([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    run, reason = classify(
        ["vendor/producer-a/crates/pub-editor/src/create_shape_runtime_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    for path in (
        "apps/chaptera-desktop/src/acceptance_v2.rs",
        "vendor/producer-a/crates/pub-odg/src/lib.rs",
        "crates/chaptera-scene-instance/src/lib.rs",
        ".github/workflows/editor-desktop-continuity-v2-windows.yml",
    ):
        run, reason = classify([path])
        assert run is True and reason == "direct_continuity_owner_changed", (path, reason)

    run, reason = classify(["README.md"])
    assert run is False and reason == "no_continuity_owner_changed"

    print("pub-editor PR fanout classifier self-test: ok")


if __name__ == "__main__":
    main()
