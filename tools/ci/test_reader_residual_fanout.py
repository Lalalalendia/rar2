#!/usr/bin/env python3
from pathlib import Path

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
}

def main() -> int:
    violations = []
    for raw, patterns in FORBIDDEN.items():
        text = Path(raw).read_text(encoding='utf-8')
        for pattern in patterns:
            needles = (f'- "{pattern}"', f"- '{pattern}'")
            if any(needle in text for needle in needles):
                violations.append(f'{raw}: residual duplicate PR trigger {pattern}')
    if violations:
        raise SystemExit('\n'.join(violations))
    print('reader residual fanout guard: ok')
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
