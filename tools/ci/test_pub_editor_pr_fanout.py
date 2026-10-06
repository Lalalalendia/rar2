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


def classify_duplicate(paths, base=None, head=None):
    return mod.classify_duplicate_rectangle(
        paths,
        base_lib_source=base,
        head_lib_source=head,
    )


def classify_authored_stack(paths, base=None, head=None):
    return mod.classify_authored_stack_runtime(
        paths,
        base_lib_source=base,
        head_lib_source=head,
    )


def classify_authored_lifecycle(paths, base=None, head=None):
    return mod.classify_authored_stack_lifecycle(
        paths,
        base_lib_source=base,
        head_lib_source=head,
    )


def classify_fixed_output_images(paths, base=None, head=None):
    return mod.classify_fixed_output_image_resources(
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

    parent = Path(".github/workflows/editor-build-once-pr.yml").read_text(
        encoding="utf-8"
    )
    assert (
        ".github/workflows/editor-desktop-continuity-v2-windows.yml" in parent
    ), "Editor build-once parent must admit Continuity V2 workflow wiring changes"
    assert (
        'packages/product/editor-desktop-continuity/v2/**' in parent
    ), "Editor build-once parent lost Continuity V2 product admission ownership"


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

    parent = Path(".github/workflows/editor-build-once-pr.yml").read_text(
        encoding="utf-8"
    )
    assert (
        'vendor/producer-a/crates/pub-editor/**' in parent
    ), "Editor build-once parent lost fixed-PDF pub-editor admission ownership"
    assert (
        ".github/workflows/editor-fixed-pdf-current-revision.yml" in parent
    ), "Editor build-once parent must admit fixed-PDF workflow wiring changes"


def assert_duplicate_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/editor-duplicate-rectangle-v1.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor Duplicate Rectangle scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "if: ${{ github.event_name == 'pull_request' }}",
        "non_pr_event_fail_closed",
        "vendor/producer-a/crates/pub-editor/**",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.duplicate_rectangle == 'true'",
        "base_classifier_missing_duplicate_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "Duplicate selective consumer lost required base-authority/fail-closed wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/editor-duplicate-rectangle-v1.yml" in contract
    ), "cheap contract must run when Duplicate consumer wiring changes"


def assert_authored_stack_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/authoring-authored-stack-runtime-v1.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor AuthoredStack Runtime scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "if: ${{ github.event_name == 'pull_request' }}",
        "non_pr_event_fail_closed",
        "vendor/producer-a/crates/pub-editor/**",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.authored_stack_runtime == 'true'",
        "base_classifier_missing_authored_stack_runtime_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "AuthoredStack Runtime selective consumer lost required base-authority/fail-closed wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/authoring-authored-stack-runtime-v1.yml" in contract
    ), "cheap contract must run when AuthoredStack Runtime consumer wiring changes"


def assert_authored_lifecycle_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/authoring-authored-stack-lifecycle-v1.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor AuthoredStack Lifecycle scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "if: ${{ github.event_name == 'pull_request' }}",
        "non_pr_event_fail_closed",
        "vendor/producer-a/crates/pub-editor/**",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.authored_stack_lifecycle == 'true'",
        "base_classifier_missing_authored_stack_lifecycle_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "AuthoredStack Lifecycle selective consumer lost required wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/authoring-authored-stack-lifecycle-v1.yml" in contract
    ), "cheap contract must run when AuthoredStack Lifecycle wiring changes"


def assert_fixed_output_images_consumer_wiring() -> None:
    workflow_path = Path(".github/workflows/editor-fixed-output-current-image-resources.yml")
    workflow = workflow_path.read_text(encoding="utf-8")

    required = (
        "Classify pub-editor fixed-output image resources scope",
        "ref: ${{ github.event.pull_request.base.ref || github.sha }}",
        "python tools/ci/pub_editor_pr_fanout.py",
        "needs: classify",
        "needs.classify.result != 'success'",
        "needs.classify.outputs.fixed_output_image_resources == 'true'",
        "base_classifier_missing_fixed_output_image_resources_output",
    )
    missing = [marker for marker in required if marker not in workflow]
    assert not missing, (
        "fixed-output image resources selective consumer lost required wiring: "
        + ", ".join(missing)
    )

    contract = Path(
        ".github/workflows/pub-editor-selective-fanout-contract.yml"
    ).read_text(encoding="utf-8")
    assert (
        ".github/workflows/editor-fixed-output-current-image-resources.yml" in contract
    ), "cheap contract must run when fixed-output image resource wiring changes"


def main() -> None:
    assert_continuity_consumer_wiring()
    assert_textbox_consumer_wiring()
    assert_fixed_pdf_consumer_wiring()
    assert_duplicate_consumer_wiring()
    assert_authored_stack_consumer_wiring()
    assert_authored_lifecycle_consumer_wiring()
    assert_fixed_output_images_consumer_wiring()

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

    assert mod.facade_change_is_safe(
        base,
        head_facade,
        safe_modules=mod.SAFE_DUPLICATE_RECTANGLE_MODULES,
    )
    run, reason = classify_duplicate(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_DUPLICATE_RECTANGLE_MODULES[
                "imported_paragraph_alignment_v1"
            ],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_duplicate_pub_editor_slice"

    run, reason = classify_duplicate(
        [
            mod.SAFE_DUPLICATE_RECTANGLE_MODULES[
                "imported_paragraph_alignment_v1"
            ]
        ]
    )
    assert run is False and reason == "proven_non_duplicate_pub_editor_slice"

    run, reason = classify_duplicate([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    for path in (
        "vendor/producer-a/crates/pub-editor/src/duplicate_authored_rectangle_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/duplicate_authored_rectangle_v1.rs",
        "apps/chaptera-desktop/src/duplicate_rectangle.rs",
        "apps/chaptera-desktop/src/duplicate_rectangle_gui_tests.rs",
        ".github/workflows/editor-duplicate-rectangle-v1.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_duplicate([path])
        assert run is True and reason == "direct_duplicate_owner_changed", (
            path,
            reason,
        )

    run, reason = classify_duplicate(["README.md"])
    assert run is False and reason == "no_duplicate_owner_changed"

    assert mod.facade_change_is_safe(
        base,
        head_facade,
        safe_modules=mod.SAFE_AUTHORED_STACK_RUNTIME_MODULES,
    )
    run, reason = classify_authored_stack(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_AUTHORED_STACK_RUNTIME_MODULES[
                "imported_paragraph_alignment_v1"
            ],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_authored_stack_runtime_pub_editor_slice"

    run, reason = classify_authored_stack(
        [
            mod.SAFE_AUTHORED_STACK_RUNTIME_MODULES[
                "imported_paragraph_alignment_v1"
            ]
        ]
    )
    assert run is False and reason == "proven_non_authored_stack_runtime_pub_editor_slice"

    run, reason = classify_authored_stack([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    run, reason = classify_authored_stack(
        ["vendor/producer-a/crates/pub-editor/src/text_format_property_base_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    for path in (
        "vendor/producer-a/crates/pub-editor/src/authored_stack_lifecycle_v1.rs",
        "vendor/producer-a/crates/pub-editor/src/authored_stack_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/src/create_shape_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/authored_stack_lifecycle_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/authored_stack_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/create_shape_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/delete_node_runtime_v1.rs",
        "apps/chaptera-server/src/revision_materializer.rs",
        "apps/chaptera-desktop/src/agent.rs",
        ".github/workflows/authoring-authored-stack-runtime-v1.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_authored_stack([path])
        assert run is True and reason == "direct_authored_stack_runtime_owner_changed", (
            path,
            reason,
        )

    run, reason = classify_authored_stack(["README.md"])
    assert run is False and reason == "no_authored_stack_runtime_owner_changed"

    assert mod.facade_change_is_safe(
        base,
        head_facade,
        safe_modules=mod.SAFE_AUTHORED_STACK_LIFECYCLE_MODULES,
    )
    run, reason = classify_authored_lifecycle(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_AUTHORED_STACK_LIFECYCLE_MODULES[
                "imported_paragraph_alignment_v1"
            ],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_authored_stack_lifecycle_pub_editor_slice"

    run, reason = classify_authored_lifecycle([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    for path in (
        "vendor/producer-a/crates/pub-editor/src/authored_stack_lifecycle_v1.rs",
        "vendor/producer-a/crates/pub-editor/src/create_shape_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/authored_stack_lifecycle_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/create_shape_runtime_v1.rs",
        "vendor/producer-a/crates/pub-editor/tests/delete_node_runtime_v1.rs",
        ".github/workflows/authoring-authored-stack-lifecycle-v1.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_authored_lifecycle([path])
        assert run is True and reason == "direct_authored_stack_lifecycle_owner_changed", (
            path,
            reason,
        )

    run, reason = classify_authored_lifecycle(
        ["vendor/producer-a/crates/pub-editor/src/text_format_property_base_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    run, reason = classify_authored_lifecycle(["README.md"])
    assert run is False and reason == "no_authored_stack_lifecycle_owner_changed"

    assert mod.facade_change_is_safe(
        base,
        head_facade,
        safe_modules=mod.SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES,
    )
    run, reason = classify_fixed_output_images(
        [
            mod.PUB_EDITOR_LIB,
            mod.SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES[
                "imported_paragraph_alignment_v1"
            ],
        ],
        base,
        head_facade,
    )
    assert run is False and reason == "proven_non_fixed_output_image_resources_pub_editor_slice"

    run, reason = classify_fixed_output_images([mod.PUB_EDITOR_LIB], base, core_head)
    assert run is True and reason == "pub_editor_lib_core_change"

    run, reason = classify_fixed_output_images(
        ["vendor/producer-a/crates/pub-editor/src/text_format_property_base_v1.rs"]
    )
    assert run is True and reason == "unknown_or_core_pub_editor_path"

    for path in (
        ".github/workflows/editor-fixed-output-current-image-resources.yml",
        "tools/ci/pub_editor_pr_fanout.py",
        "tools/ci/test_pub_editor_pr_fanout.py",
    ):
        run, reason = classify_fixed_output_images([path])
        assert run is True and reason == "direct_fixed_output_image_resources_owner_changed", (
            path,
            reason,
        )

    run, reason = classify_fixed_output_images(["README.md"])
    assert run is False and reason == "no_fixed_output_image_resources_owner_changed"

    print("pub-editor PR fanout classifier self-test: ok")


if __name__ == "__main__":
    main()
