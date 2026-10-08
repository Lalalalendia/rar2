#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
from pathlib import Path
import subprocess
from typing import Iterable


PUB_EDITOR_PREFIX = "vendor/producer-a/crates/pub-editor/"
PUB_EDITOR_AUTHORING_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-authoring-core/"
PUB_EDITOR_TABLE_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-table-core/"
PUB_EDITOR_GEOMETRY_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-geometry-core/"
PUB_EDITOR_LIB = PUB_EDITOR_PREFIX + "src/lib.rs"


def is_pub_editor_domain_path(path: str) -> bool:
    return (
        path.startswith(PUB_EDITOR_PREFIX)
        or path.startswith(PUB_EDITOR_AUTHORING_CORE_PREFIX)
        or path.startswith(PUB_EDITOR_TABLE_CORE_PREFIX)
        or path.startswith(PUB_EDITOR_GEOMETRY_CORE_PREFIX)
    )

# Deliberately tiny first allowlist. These modules are feature-owned and are
# not consumed by the Desktop Continuity V2 acceptance transaction.
SAFE_CONTINUITY_V2_MODULES = {
    "duplicate_authored_rectangle_v1": PUB_EDITOR_PREFIX + "src/duplicate_authored_rectangle_v1.rs",
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}
# Stage 3 intentionally starts with the same tiny proven-safe set. Grow this
# only with consumer-specific negative controls.
SAFE_TEXTBOX_RESTORE_MODULES = SAFE_CONTINUITY_V2_MODULES
# Stage 4 starts narrower: only the real paragraph-base slice has measured
# fixed-PDF false-positive evidence.
SAFE_FIXED_PDF_CURRENT_REVISION_MODULES = {
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}
# Stage 5 starts from the same measured safe paragraph slice.
SAFE_DUPLICATE_RECTANGLE_MODULES = {
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}
# Stage 6 remains deliberately narrow: only the measured imported-paragraph
# facade slice is allowed to skip AuthoredStack Runtime.
SAFE_AUTHORED_STACK_RUNTIME_MODULES = {
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}
SAFE_AUTHORED_STACK_LIFECYCLE_MODULES = {
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}
SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES = {
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}

DIRECT_CONTINUITY_V2_OWNERS = (
    ".github/workflows/editor-desktop-continuity-v2-windows.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "apps/chaptera-desktop/src/acceptance.rs",
    "apps/chaptera-desktop/src/acceptance_v2.rs",
    "apps/chaptera-desktop/src/acceptance_v2_cli.rs",
    "packages/product/editor-desktop-continuity/v2/**",
    "tools/run_editor_desktop_continuity_v2.py",
    "tools/validate_editor_desktop_continuity_v2_receipt.py",
    "tools/verify_editable_export_geometry.py",
    "vendor/producer-a/crates/pub-odg/**",
    "crates/chaptera-scene-instance/**",
)

DIRECT_TEXTBOX_RESTORE_OWNERS = (
    ".github/workflows/editor-desktop-textbox-restore-v1.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "apps/chaptera-desktop/src/text_box_creation.rs",
    "apps/chaptera-desktop/src/text_box_creation_shell.rs",
    "apps/chaptera-desktop/src/text_box_creation_gui_tests.rs",
    "apps/chaptera-desktop/src/text_session.rs",
    "apps/chaptera-desktop/src/text_session_shell.rs",
    "apps/chaptera-desktop/src/selection_keyboard_shell.rs",
    "apps/chaptera-desktop/Cargo.toml",
    "crates/chaptera-canvas-creation-interaction/**",
    "crates/chaptera-desktop-fallback-font-resource/**",
)

DIRECT_FIXED_PDF_CURRENT_REVISION_OWNERS = (
    ".github/workflows/editor-fixed-pdf-current-revision.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "tools/run_editor_fixed_pdf_current_revision_v2.py",
    "tools/run_editor_desktop_continuity_v2.py",
    "tools/validate_editor_desktop_continuity_v2_receipt.py",
    "tools/run_yab259_current_fixed_pdf_resource_request.py",
    "tools/yab259_current_fixed_pdf_resource_request.rs",
    "tools/run_yab259_fixed_pdf_packet_renderer.py",
    "tools/yab259_fixed_pdf_packet_renderer.rs",
    "tools/run_yab259_fixed_pdf_closure.py",
    "apps/chaptera-desktop/src/acceptance_v2.rs",
    "crates/chaptera-desktop-shaped-flow-runtime/src/bin/current_fixed_pdf_input.rs",
    "crates/chaptera-desktop-shaped-flow-runtime/src/lib.rs",
    "crates/chaptera-desktop-fallback-font-resource/**",
    "vendor/producer-a/crates/pub-layout/src/shaped_flow.rs",
)

DIRECT_DUPLICATE_RECTANGLE_OWNERS = (
    ".github/workflows/editor-duplicate-rectangle-v1.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "vendor/producer-a/crates/pub-editor/src/duplicate_authored_rectangle_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/duplicate_authored_rectangle_v1.rs",
    "apps/chaptera-desktop/src/duplicate_rectangle.rs",
    "apps/chaptera-desktop/src/duplicate_rectangle_gui_tests.rs",
)

DIRECT_AUTHORED_STACK_RUNTIME_OWNERS = (
    ".github/workflows/authoring-authored-stack-runtime-v1.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "vendor/producer-a/crates/pub-editor-authoring-core/src/authored_stack_lifecycle_v1.rs",
    "vendor/producer-a/crates/pub-editor-authoring-core/src/authored_stack_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor-authoring-core/src/create_shape_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor-authoring-core/tests/authored_stack_lifecycle_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/authored_stack_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/create_shape_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/delete_node_runtime_v1.rs",
    "apps/chaptera-server/src/revision_materializer.rs",
    "apps/chaptera-desktop/src/agent.rs",
)

DIRECT_AUTHORED_STACK_LIFECYCLE_OWNERS = (
    ".github/workflows/authoring-authored-stack-lifecycle-v1.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
    "vendor/producer-a/crates/pub-editor-authoring-core/src/authored_stack_lifecycle_v1.rs",
    "vendor/producer-a/crates/pub-editor-authoring-core/src/create_shape_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor-authoring-core/tests/authored_stack_lifecycle_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/create_shape_runtime_v1.rs",
    "vendor/producer-a/crates/pub-editor/tests/delete_node_runtime_v1.rs",
)

DIRECT_FIXED_OUTPUT_IMAGE_RESOURCES_OWNERS = (
    ".github/workflows/editor-fixed-output-current-image-resources.yml",
    "tools/ci/pub_editor_pr_fanout.py",
    "tools/ci/test_pub_editor_pr_fanout.py",
)


def changed_paths(base: str, head: str) -> list[str]:
    return sorted(
        dict.fromkeys(
            path
            for path in subprocess.check_output(
                ["git", "diff", "--name-only", f"{base}...{head}"],
                text=True,
            ).splitlines()
            if path
        )
    )


def matches(path: str, patterns: Iterable[str]) -> bool:
    for pattern in patterns:
        if pattern.endswith("/**") and path.startswith(pattern[:-3]):
            return True
        if fnmatch.fnmatchcase(path, pattern):
            return True
    return False


def strip_safe_facade_blocks(
    source: str,
    safe_modules: dict[str, str] | None = None,
) -> str:
    """Remove only declarations/re-exports for explicitly safe modules."""
    if safe_modules is None:
        safe_modules = SAFE_CONTINUITY_V2_MODULES
    lines = source.splitlines(keepends=True)
    output: list[str] = []
    index = 0

    while index < len(lines):
        stripped = lines[index].strip()
        module = next(
            (
                name
                for name in safe_modules
                if stripped == f"mod {name};"
            ),
            None,
        )
        if module is not None:
            index += 1
            continue

        module = next(
            (
                name
                for name in safe_modules
                if stripped.startswith(f"pub use {name}::")
            ),
            None,
        )
        if module is not None:
            if stripped.endswith(";"):
                index += 1
                continue
            index += 1
            while index < len(lines):
                if lines[index].strip() == "};":
                    index += 1
                    break
                index += 1
            continue

        output.append(lines[index])
        index += 1

    return "".join(output)


def facade_change_is_safe(
    base_source: str,
    head_source: str,
    *,
    safe_modules: dict[str, str] | None = None,
) -> bool:
    return strip_safe_facade_blocks(
        base_source, safe_modules
    ) == strip_safe_facade_blocks(head_source, safe_modules)


def classify_continuity_v2_windows(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_CONTINUITY_V2_OWNERS) for path in paths):
        return True, "direct_continuity_owner_changed"

    pub_editor_paths = [
        path for path in paths if is_pub_editor_domain_path(path)
    ]
    if not pub_editor_paths:
        return False, "no_continuity_owner_changed"

    allowed_paths = set(SAFE_CONTINUITY_V2_MODULES.values()) | {PUB_EDITOR_LIB}
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(base_lib_source, head_lib_source):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_continuity_pub_editor_slice"


def classify_textbox_restore(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_TEXTBOX_RESTORE_OWNERS) for path in paths):
        return True, "direct_textbox_owner_changed"

    pub_editor_paths = [
        path for path in paths if is_pub_editor_domain_path(path)
    ]
    if not pub_editor_paths:
        return False, "no_textbox_owner_changed"

    allowed_paths = set(SAFE_TEXTBOX_RESTORE_MODULES.values()) | {PUB_EDITOR_LIB}
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(base_lib_source, head_lib_source):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_textbox_pub_editor_slice"


def classify_fixed_pdf_current_revision(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_FIXED_PDF_CURRENT_REVISION_OWNERS) for path in paths):
        return True, "direct_fixed_pdf_owner_changed"

    pub_editor_paths = [
        path for path in paths if is_pub_editor_domain_path(path)
    ]
    if not pub_editor_paths:
        return False, "no_fixed_pdf_owner_changed"

    allowed_paths = set(SAFE_FIXED_PDF_CURRENT_REVISION_MODULES.values()) | {
        PUB_EDITOR_LIB
    }
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(
            base_lib_source,
            head_lib_source,
            safe_modules=SAFE_FIXED_PDF_CURRENT_REVISION_MODULES,
        ):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_fixed_pdf_pub_editor_slice"


def classify_duplicate_rectangle(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_DUPLICATE_RECTANGLE_OWNERS) for path in paths):
        return True, "direct_duplicate_owner_changed"

    pub_editor_paths = [
        path for path in paths if is_pub_editor_domain_path(path)
    ]
    if not pub_editor_paths:
        return False, "no_duplicate_owner_changed"

    allowed_paths = set(SAFE_DUPLICATE_RECTANGLE_MODULES.values()) | {
        PUB_EDITOR_LIB
    }
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(
            base_lib_source,
            head_lib_source,
            safe_modules=SAFE_DUPLICATE_RECTANGLE_MODULES,
        ):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_duplicate_pub_editor_slice"


def classify_authored_stack_runtime(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_AUTHORED_STACK_RUNTIME_OWNERS) for path in paths):
        return True, "direct_authored_stack_runtime_owner_changed"

    pub_editor_paths = [
        path for path in paths if is_pub_editor_domain_path(path)
    ]
    if not pub_editor_paths:
        return False, "no_authored_stack_runtime_owner_changed"

    allowed_paths = set(SAFE_AUTHORED_STACK_RUNTIME_MODULES.values()) | {
        PUB_EDITOR_LIB
    }
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(
            base_lib_source,
            head_lib_source,
            safe_modules=SAFE_AUTHORED_STACK_RUNTIME_MODULES,
        ):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_authored_stack_runtime_pub_editor_slice"


def classify_authored_stack_lifecycle(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_AUTHORED_STACK_LIFECYCLE_OWNERS) for path in paths):
        return True, "direct_authored_stack_lifecycle_owner_changed"

    pub_editor_paths = [path for path in paths if is_pub_editor_domain_path(path)]
    if not pub_editor_paths:
        return False, "no_authored_stack_lifecycle_owner_changed"

    allowed_paths = set(SAFE_AUTHORED_STACK_LIFECYCLE_MODULES.values()) | {
        PUB_EDITOR_LIB
    }
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(
            base_lib_source,
            head_lib_source,
            safe_modules=SAFE_AUTHORED_STACK_LIFECYCLE_MODULES,
        ):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_authored_stack_lifecycle_pub_editor_slice"


def classify_fixed_output_image_resources(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_FIXED_OUTPUT_IMAGE_RESOURCES_OWNERS) for path in paths):
        return True, "direct_fixed_output_image_resources_owner_changed"

    pub_editor_paths = [path for path in paths if is_pub_editor_domain_path(path)]
    if not pub_editor_paths:
        return False, "no_fixed_output_image_resources_owner_changed"

    allowed_paths = set(SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES.values()) | {
        PUB_EDITOR_LIB
    }
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(
            base_lib_source,
            head_lib_source,
            safe_modules=SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES,
        ):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_fixed_output_image_resources_pub_editor_slice"


def git_show(revision: str, path: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "show", f"{revision}:{path}"],
            text=True,
        )
    except subprocess.CalledProcessError:
        return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    base_lib = git_show(args.base, PUB_EDITOR_LIB) if PUB_EDITOR_LIB in paths else None
    head_lib = git_show(args.head, PUB_EDITOR_LIB) if PUB_EDITOR_LIB in paths else None
    run_windows, reason = classify_continuity_v2_windows(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_textbox, textbox_reason = classify_textbox_restore(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_fixed_pdf, fixed_pdf_reason = classify_fixed_pdf_current_revision(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_duplicate, duplicate_reason = classify_duplicate_rectangle(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_authored_stack, authored_stack_reason = classify_authored_stack_runtime(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_authored_lifecycle, authored_lifecycle_reason = classify_authored_stack_lifecycle(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )
    run_fixed_output_images, fixed_output_images_reason = classify_fixed_output_image_resources(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )

    receipt = {
        "schema": "chaptera.pub-editor-pr-fanout.v1",
        "base": args.base,
        "head": args.head,
        "changed_paths": paths,
        "continuity_v2_windows": run_windows,
        "reason": reason,
        "safe_continuity_v2_modules": sorted(SAFE_CONTINUITY_V2_MODULES.values()),
        "textbox_restore": run_textbox,
        "textbox_reason": textbox_reason,
        "safe_textbox_restore_modules": sorted(SAFE_TEXTBOX_RESTORE_MODULES.values()),
        "fixed_pdf_current_revision": run_fixed_pdf,
        "fixed_pdf_reason": fixed_pdf_reason,
        "safe_fixed_pdf_current_revision_modules": sorted(
            SAFE_FIXED_PDF_CURRENT_REVISION_MODULES.values()
        ),
        "duplicate_rectangle": run_duplicate,
        "duplicate_reason": duplicate_reason,
        "safe_duplicate_rectangle_modules": sorted(
            SAFE_DUPLICATE_RECTANGLE_MODULES.values()
        ),
        "authored_stack_runtime": run_authored_stack,
        "authored_stack_runtime_reason": authored_stack_reason,
        "safe_authored_stack_runtime_modules": sorted(
            SAFE_AUTHORED_STACK_RUNTIME_MODULES.values()
        ),
        "authored_stack_lifecycle": run_authored_lifecycle,
        "authored_stack_lifecycle_reason": authored_lifecycle_reason,
        "safe_authored_stack_lifecycle_modules": sorted(
            SAFE_AUTHORED_STACK_LIFECYCLE_MODULES.values()
        ),
        "fixed_output_image_resources": run_fixed_output_images,
        "fixed_output_image_resources_reason": fixed_output_images_reason,
        "safe_fixed_output_image_resources_modules": sorted(
            SAFE_FIXED_OUTPUT_IMAGE_RESOURCES_MODULES.values()
        ),
    }

    payload = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.receipt:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(payload, encoding="utf-8")
    else:
        print(payload, end="")

    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as handle:
            handle.write(f"continuity_v2_windows={'true' if run_windows else 'false'}\n")
            handle.write(f"reason={reason}\n")
            handle.write(f"textbox_restore={'true' if run_textbox else 'false'}\n")
            handle.write(f"textbox_reason={textbox_reason}\n")
            handle.write(
                f"fixed_pdf_current_revision={'true' if run_fixed_pdf else 'false'}\n"
            )
            handle.write(f"fixed_pdf_reason={fixed_pdf_reason}\n")
            handle.write(
                f"duplicate_rectangle={'true' if run_duplicate else 'false'}\n"
            )
            handle.write(f"duplicate_reason={duplicate_reason}\n")
            handle.write(
                f"authored_stack_runtime={'true' if run_authored_stack else 'false'}\n"
            )
            handle.write(f"authored_stack_runtime_reason={authored_stack_reason}\n")
            handle.write(
                f"authored_stack_lifecycle={'true' if run_authored_lifecycle else 'false'}\n"
            )
            handle.write(f"authored_stack_lifecycle_reason={authored_lifecycle_reason}\n")
            handle.write(
                f"fixed_output_image_resources={'true' if run_fixed_output_images else 'false'}\n"
            )
            handle.write(
                f"fixed_output_image_resources_reason={fixed_output_images_reason}\n"
            )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
