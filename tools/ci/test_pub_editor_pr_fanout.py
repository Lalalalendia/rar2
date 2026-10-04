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


def classify_textbox(paths, base=None, head=None):
    return mod.classify_textbox_restore(
        paths,
        base_lib_source=base,
        head_lib_source=head,
    )


def classify_fixed_pdf(paths, base=None, head=None):
    return mod.classify_fixed_pdf_current_revision(
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


def assert_textbox_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/editor-desktop-textbox-restore-v1.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor TextBox Restore scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.textbox_restore == 'true'",
        "base_classifier_missing_textbox_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "TextBox Restore selective consumer lost required base-authority/fail-closed wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/editor-desktop-textbox-restore-v1.yml" in contract
    ), "cheap contract must run when TextBox Restore consumer wiring changes"


def assert_fixed_pdf_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/editor-fixed-pdf-current-revision.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor fixed-PDF current revision scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.fixed_pdf_current_revision == 'true'",
        "base_classifier_missing_fixed_pdf_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "fixed-PDF selective consumer lost required base-authority/fail-closed wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/editor-fixed-pdf-current-revision.yml" in contract
    ), "cheap contract must run when fixed-PDF consumer wiring changes"


def main() -> None:
    assert_continuity_consumer_wiring()
    assert_textbox_consumer_wiring()
    assert_fixed_pdf_consumer_wiring()

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
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify([path])
        assert run is True and reason == "direct_continuity_owner_changed", (path, reason)

    run, reason = classify(["README.md"])
    assert run is False and reason == "no_continuity_owner_changed"

    run, reason = classify_textbox(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_TEXTBOX_RESTORE_MODULES["imported_paragraph_alignment_v1"],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_textbox_pub_editor_slice"

    run, reason = classify_textbox(
        [mod.SAFE_TEXTBOX_RESTORE_MODULES["duplicate_authored_rectangle_v1"]]
    )
    assert run is False and reason == "proven_non_textbox_pub_editor_slice"

    run, reason = classify_textbox([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    run, reason = classify_textbox(
        ["vendor/producer-a/crates/pub-editor/src/create_shape_runtime_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    for path in (
        "apps/chaptera-desktop/src/text_box_creation.rs",
        "apps/chaptera-desktop/src/text_box_creation_shell.rs",
        "apps/chaptera-desktop/src/text_session.rs",
        "apps/chaptera-desktop/Cargo.toml",
        "crates/chaptera-canvas-creation-interaction/src/lib.rs",
        "crates/chaptera-desktop-fallback-font-resource/src/lib.rs",
        ".github/workflows/editor-desktop-textbox-restore-v1.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_textbox([path])
        assert run is True and reason == "direct_textbox_owner_changed", (path, reason)

    run, reason = classify_textbox(["README.md"])
    assert run is False and reason == "no_textbox_owner_changed"

    assert mod.facade_change_is_safe(
        base,
        head_facade,
        safe_modules=mod.SAFE_FIXED_PDF_CURRENT_REVISION_MODULES,
    )
    run, reason = classify_fixed_pdf(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_FIXED_PDF_CURRENT_REVISION_MODULES[
                "imported_paragraph_alignment_v1"
            ],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_fixed_pdf_pub_editor_slice"

    run, reason = classify_fixed_pdf(
        [
            mod.SAFE_FIXED_PDF_CURRENT_REVISION_MODULES[
                "imported_paragraph_alignment_v1"
            ]
        ]
    )
    assert run is False and reason == "proven_non_fixed_pdf_pub_editor_slice"

    run, reason = classify_fixed_pdf([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    run, reason = classify_fixed_pdf(
        ["vendor/producer-a/crates/pub-editor/src/create_shape_runtime_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    for path in (
        "tools/run_editor_fixed_pdf_current_revision_v2.py",
        "apps/chaptera-desktop/src/acceptance_v2.rs",
        "crates/chaptera-desktop-shaped-flow-runtime/src/lib.rs",
        "crates/chaptera-desktop-fallback-font-resource/src/lib.rs",
        "vendor/producer-a/crates/pub-layout/src/shaped_flow.rs",
        ".github/workflows/editor-fixed-pdf-current-revision.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_fixed_pdf([path])
        assert run is True and reason == "direct_fixed_pdf_owner_changed", (
            path,
            reason,
        )

    run, reason = classify_fixed_pdf(["README.md"])
    assert run is False and reason == "no_fixed_pdf_owner_changed"

    print("pub-editor PR fanout classifier self-test: ok")


if __name__ == "__main__":
    main()
