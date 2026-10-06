#!/usr/bin/env python3
from pathlib import Path

PARENT = ".github/workflows/editor-build-once-pr.yml"
CONSUMERS = (
    ".github/workflows/editor-desktop-continuity-v2-windows.yml",
    ".github/workflows/editor-fixed-pdf-current-revision.yml",
    ".github/workflows/carlton-march-editor-sample-smoke.yml",
)

def require(text, markers, owner):
    missing = [marker for marker in markers if marker not in text]
    if missing:
        raise SystemExit(
            f"{owner}: missing Editor build-once marker(s): " + ", ".join(missing)
        )

def main():
    parent = Path(PARENT).read_text(encoding="utf-8")
    producer = Path(
        ".github/workflows/chaptera-editor-windows-binary.yml"
    ).read_text(encoding="utf-8")

    require(
        parent,
        (
            "editor-binary:",
            "uses: ./.github/workflows/chaptera-editor-windows-binary.yml",
            "artifact_name: chaptera-editor-windows-binary-${{ github.sha }}",
            "uses: ./.github/workflows/editor-desktop-continuity-v2-windows.yml",
            "uses: ./.github/workflows/editor-fixed-pdf-current-revision.yml",
            "uses: ./.github/workflows/carlton-march-editor-sample-smoke.yml",
            "editor_binary_artifact_name: chaptera-editor-windows-binary-${{ github.sha }}",
            "needs.classify.result != 'success'",
            'vendor/producer-a/crates/pub-editor/**',
            'apps/chaptera-desktop/src/acceptance.rs',
            'apps/chaptera-desktop/src/acceptance_cli.rs',
            'apps/chaptera-desktop/src/acceptance_v2.rs',
            'apps/chaptera-desktop/src/acceptance_v2_cli.rs',
            'packages/product/editor-desktop-continuity/v2/**',
            'tools/run_editor_desktop_continuity_v2.py',
            'tools/validate_editor_desktop_continuity_v2_receipt.py',
            'tools/verify_editable_export_geometry.py',
            'tools/run_editor_fixed_pdf_current_revision_v2.py',
            'tools/run_yab259_current_fixed_pdf_resource_request.py',
            'tools/yab259_current_fixed_pdf_resource_request.rs',
            'tools/run_yab259_fixed_pdf_packet_renderer.py',
            'tools/yab259_fixed_pdf_packet_renderer.rs',
            'tools/run_yab259_fixed_pdf_closure.py',
            'crates/chaptera-desktop-shaped-flow-runtime/src/bin/current_fixed_pdf_input.rs',
            'crates/chaptera-desktop-shaped-flow-runtime/src/lib.rs',
            'crates/chaptera-desktop-fallback-font-resource/**',
            'vendor/producer-a/crates/pub-odg/**',
            'vendor/producer-a/crates/pub-layout/src/shaped_flow.rs',
            'crates/chaptera-scene-instance/**',
        ),
        "parent",
    )

    require(
        producer,
        (
            'candidate_sha = "${{ github.sha }}"',
            'schema_version = "chaptera.editor-windows-binary.v1"',
            "cargo build -p chaptera-desktop --release --bin chaptera-editor",
            "validate_editor_binary_artifact.py",
            "retention-days: 1",
        ),
        "producer",
    )

    for raw in CONSUMERS:
        text = Path(raw).read_text(encoding="utf-8")
        require(
            text,
            (
                "workflow_call:",
                "workflow_dispatch:",
                "editor_binary_artifact_name:",
                "actions/download-artifact@v4",
                "validate_editor_binary_artifact.py",
                "if: inputs.editor_binary_artifact_name == ''",
            ),
            raw,
        )
        if "\n  pull_request:" in text:
            raise SystemExit(f"{raw}: direct PR admission must be owned by {PARENT}")

    fixed = Path(
        ".github/workflows/editor-fixed-pdf-current-revision.yml"
    ).read_text(encoding="utf-8")
    if "Build current fixed-PDF input" not in fixed:
        raise SystemExit("fixed-PDF consumer lost non-Editor helper build")

    trust = Path("tools/ci/check_chaptera_ci_trust.py").read_text(encoding="utf-8")
    if f'"{PARENT}": {{' not in trust:
        raise SystemExit("Carlton PR trust/admission ownership did not move to parent")
    if '".github/workflows/carlton-march-editor-sample-smoke.yml": {' in trust:
        raise SystemExit("Carlton child still owns direct PR trust/admission")

    print("Editor build-once admission wiring guard: ok")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
