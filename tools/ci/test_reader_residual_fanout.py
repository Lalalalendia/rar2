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
    '.github/workflows/editor-project-fork.yml': (
        'vendor/producer-a/crates/pub-editor/**',
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/authoring-move-nodes-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/editor-resize-node.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/resize-nodes-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/editor-create-textbox-rust-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/editor-created-story-edit-rust-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/authoring-create-shape-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
    '.github/workflows/authoring-table-grid-v1.yml': (
        'vendor/producer-a/crates/pub-editor/src/lib.rs',
    ),
}

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

    reader_ci = Path('.github/workflows/reader-pr-ci.yml').read_text(encoding='utf-8')
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
