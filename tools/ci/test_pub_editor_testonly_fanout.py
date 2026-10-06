#!/usr/bin/env python3
from pathlib import Path

CLASSIFIER = "tools/ci/pub_editor_pr_fanout.py"
CLASSIFIER_TEST = "tools/ci/test_pub_editor_pr_fanout.py"
CHEAP_OWNER = ".github/workflows/pub-editor-selective-fanout-contract.yml"
HEAVY_WORKFLOWS = (
    ".github/workflows/editor-desktop-textbox-restore-v1.yml",
    ".github/workflows/editor-duplicate-rectangle-v1.yml",
    ".github/workflows/authoring-authored-stack-runtime-v1.yml",
    ".github/workflows/authoring-authored-stack-lifecycle-v1.yml",
    ".github/workflows/editor-fixed-output-current-image-resources.yml",
)


def pull_request_header(text: str) -> str:
    marker = "\n  workflow_dispatch:"
    if marker not in text:
        raise SystemExit("workflow is missing workflow_dispatch boundary")
    return text.split(marker, 1)[0]


def main() -> int:
    for raw in HEAVY_WORKFLOWS:
        text = Path(raw).read_text(encoding="utf-8")
        header = pull_request_header(text)
        if f'- "{CLASSIFIER}"' not in header:
            raise SystemExit(f"{raw}: production classifier trigger was removed")
        if f'- "{CLASSIFIER_TEST}"' in header:
            raise SystemExit(f"{raw}: classifier-test-only path still schedules heavy workflow")
        if f"run: python {CLASSIFIER_TEST}" not in text:
            raise SystemExit(f"{raw}: internal base-authority classifier self-test was removed")

    cheap = Path(CHEAP_OWNER).read_text(encoding="utf-8")
    cheap_header = pull_request_header(cheap)
    if f'- "{CLASSIFIER_TEST}"' not in cheap_header:
        raise SystemExit("cheap selective fanout contract lost classifier-test ownership")
    if "run: python tools/ci/test_pub_editor_testonly_fanout.py" not in cheap:
        raise SystemExit("cheap selective fanout contract does not execute the test-only fanout guard")

    print(
        "pub-editor classifier-test-only fanout guard: ok "
        f"({len(HEAVY_WORKFLOWS)} heavy workflows excluded)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
