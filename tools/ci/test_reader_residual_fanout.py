#!/usr/bin/env python3
from pathlib import Path
import re

FORBIDDEN = {
    '.github/workflows/carlton-exact-pub-runtime-inputs.yml': (
        'vendor/producer-a/crates/pub-reader/**',
        'vendor/producer-a/crates/pub-viewer/**',
    ),
    '.github/workflows/web-scene-producer-a-ci-probe.yml': (
        'vendor/producer-a/crates/pub-reader/**',
        'vendor/producer-a/crates/pub-viewer/**',
    ),
    '.github/workflows/cdm-shape-paint-v1.yml': (
        'vendor/producer-a/crates/pub-reader/**',
        'vendor/producer-a/crates/pub-viewer/**',
    ),
    '.github/workflows/editor-desktop-shaped-flow-runtime-v1.yml': (
        'vendor/producer-a/crates/pub-reader/**',
        'vendor/producer-a/crates/pub-viewer/**',
    ),
    '.github/workflows/reader-cmo-scene-instance-compose-v1.yml': (
        'vendor/producer-a/crates/pub-viewer/**',
        'crates/chaptera-viewer-render-plan/**',
        'apps/chaptera-desktop/**',
    ),
    '.github/workflows/reader-page-role-observation.yml': (
        'vendor/producer-a/crates/pub-reader/**',
        'vendor/producer-a/crates/pub-viewer/**',
        'crates/pub-model/**',
        'crates/pub-presentation-profile/**',
    ),
    '.github/workflows/authoring-table-grid-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/tests/effective_table_grid_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/resize_node_v1.rs',
    ),
    '.github/workflows/authoring-move-nodes-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/src/writer_assessment.rs',
        'vendor/producer-a/crates/pub-editor/tests/move_nodes_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/resize_node_v1.rs',
    ),
    '.github/workflows/authoring-create-shape-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/src/writer_assessment.rs',
        'vendor/producer-a/crates/pub-editor/tests/create_shape_runtime_v1.rs',
    ),
    '.github/workflows/resize-nodes-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/src/writer_assessment.rs',
        'vendor/producer-a/crates/pub-editor/tests/resize_nodes_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/resize_node_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/move_nodes_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/effective_table_grid_v1.rs',
        'vendor/producer-a/crates/pub-editor/tests/break_text_frame_forward_link_v1.rs',
    ),
    '.github/workflows/editor-create-textbox-rust-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/src/writer_assessment.rs',
        'vendor/producer-a/crates/pub-editor/tests/create_text_box_v1.rs',
    ),
    '.github/workflows/editor-project-fork.yml': (
        'vendor/producer-a/Cargo.toml',
        'vendor/producer-a/crates/pub-editor/**',
    ),
    '.github/workflows/authoring-textframe-break-link-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/tests/break_text_frame_forward_link_v1.rs',
    ),
    '.github/workflows/editor-created-story-edit-rust-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/tests/create_text_box_v1.rs',
    ),
    '.github/workflows/editor-resize-node.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
        'vendor/producer-a/crates/pub-editor/src/writer_assessment.rs',
        'vendor/producer-a/crates/pub-editor/tests/resize_node_v1.rs',
    ),
    '.github/workflows/authoring-overset-state-v1.yml': (
        'vendor/producer-a/crates/pub-editor/**',
    ),
    '.github/workflows/reader1050-hosted-frontier.yml': (
        'vendor/producer-a/crates/pub-reader/src/lib.rs',
        'vendor/producer-a/crates/pub-viewer/src/lib.rs',
    ),
    '.github/workflows/editor-guide-projection-v1.yml': (
        'vendor/producer-a/crates/pub-reader/src/lib.rs',
    ),
    '.github/workflows/chaptera-win-support-matrix.yml': (
        'apps/chaptera-desktop/**',
    ),
}

NO_DIRECT_PR = (
    '.github/workflows/brochure-effective-paint-probe.yml',
)

DIRECT_PR = re.compile(r'^  pull_request:\s*$', re.MULTILINE)
REUSABLE_USE = re.compile(
    r'uses:\s+\./(\.github/workflows/[^\s]+\.(?:yml|yaml))'
)
DISPATCHER_PATH = '.github/workflows/overnight-hosted-pub-cycle.yml'


def main() -> int:
    violations = []

    for raw, patterns in FORBIDDEN.items():
        text = Path(raw).read_text(encoding='utf-8')
        for pattern in patterns:
            needles = (f'- "{pattern}"', f"- '{pattern}'")
            if any(needle in text for needle in needles):
                violations.append(f'{raw}: residual duplicate PR trigger {pattern}')

    for raw in NO_DIRECT_PR:
        text = Path(raw).read_text(encoding='utf-8')
        if DIRECT_PR.search(text):
            violations.append(
                f'{raw}: measurement workflow must run on main/manual, not direct PR'
            )

    reader_ci = Path('.github/workflows/reader-pr-ci.yml').read_text(encoding='utf-8')

    reader_smoke_section = reader_ci.split('\n  reader-windows-smoke:\n', 1)[1].split(
        '\n  reader-windows:\n', 1
    )[0]
    if "reader_windows_smoke == 'true'" not in reader_smoke_section:
        violations.append(
            'reader-pr-ci.yml: Reader Windows shared-core smoke lost classifier ownership'
        )
    if 'visual_oracle' in reader_smoke_section:
        violations.append(
            'reader-pr-ci.yml: shared-core smoke must stay parallel to visual oracle'
        )

    visual_section = reader_ci.split('\n  visual-oracle:\n', 1)[1].split(
        '\n  cloud-reference:\n', 1
    )[0]
    if 'run_windows_shared_core_smoke: false' not in visual_section:
        violations.append(
            'reader-pr-ci.yml: visual oracle must not serialize Reader shared-core smoke'
        )

    smoke_workflow = Path(
        '.github/workflows/chaptera-reader-windows-smoke.yml'
    ).read_text(encoding='utf-8')
    for invariant in (
        'CARGO_TARGET_DIR: target/reader-fidelity-win',
        'reader-fidelity-win-v1-',
        'mature_officeart_wmf_exact_product_tests',
        'Run bounded Reader product and real-PUB smoke',
    ):
        if invariant not in smoke_workflow:
            violations.append(
                'chaptera-reader-windows-smoke.yml: missing parallel evidence invariant '
                f'{invariant}'
            )

    reusable = sorted(set(REUSABLE_USE.findall(reader_ci)))
    for raw in reusable:
        text = Path(raw).read_text(encoding='utf-8')
        if DIRECT_PR.search(text):
            violations.append(
                f'{raw}: direct pull_request trigger duplicates Reader PR selective CI'
            )

    for path in sorted(Path('.github/workflows').glob('*.y*ml')):
        raw = path.as_posix()
        if raw == DISPATCHER_PATH:
            continue
        text = path.read_text(encoding='utf-8')
        if DISPATCHER_PATH in text:
            violations.append(
                f'{raw}: child workflow must not trigger from dispatcher-only path '
                f'{DISPATCHER_PATH}'
            )

    if violations:
        raise SystemExit('\n'.join(violations))

    print(
        'reader residual fanout guard: ok '
        f'({len(reusable)} centrally-routed reusable workflows checked)'
    )
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
