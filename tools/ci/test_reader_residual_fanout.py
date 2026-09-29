#!/usr/bin/env python3
from pathlib import Path

BROAD_READER_TRIGGERS = (
    'vendor/producer-a/crates/pub-reader/**',
    'vendor/producer-a/crates/pub-viewer/**',
)

TARGETS = (
    '.github/workflows/carlton-exact-pub-runtime-inputs.yml',
    '.github/workflows/web-scene-producer-a-ci-probe.yml',
    '.github/workflows/cdm-shape-paint-v1.yml',
    '.github/workflows/editor-desktop-shaped-flow-runtime-v1.yml',
)

def main() -> int:
    violations = []
    for raw in TARGETS:
        path = Path(raw)
        text = path.read_text(encoding='utf-8')
        for pattern in BROAD_READER_TRIGGERS:
            needle = f'- "{pattern}"'
            needle_single = f"- '{pattern}'"
            if needle in text or needle_single in text:
                violations.append(f'{raw}: broad Reader PR trigger {pattern}')
    if violations:
        raise SystemExit('\n'.join(violations))
    print('reader residual fanout guard: ok')
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
