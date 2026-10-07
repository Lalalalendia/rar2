#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(".github/workflows")
RUNTIME = "crates/chaptera-desktop-shaped-flow-runtime"
CARGO = f"{RUNTIME}/Cargo.toml"
LIB = f"{RUNTIME}/src/lib.rs"
FONT = f"{RUNTIME}/src/font_resource.rs"
TYPO = f"{RUNTIME}/src/current_typography.rs"
PAGES = f"{RUNTIME}/src/fixed_pdf_pages.rs"
RESOURCES = f"{RUNTIME}/src/fixed_pdf_resources.rs"
FIXED_BIN = f"{RUNTIME}/src/bin/current_fixed_pdf_input.rs"
VIEWER_BIN = f"{RUNTIME}/src/bin/current_viewer_fixed_pdf_input.rs"
W2_BIN = f"{RUNTIME}/src/bin/w2_same_document_kernel.rs"
BROAD = f"{RUNTIME}/**"


def text(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")


def require_all(name: str, needles: tuple[str, ...]) -> None:
    body = text(name)
    missing = [needle for needle in needles if needle not in body]
    if missing:
        raise SystemExit(f"{name}: missing shaped-flow owner paths: {missing}")


def forbid(name: str, needle: str) -> None:
    if needle in text(name):
        raise SystemExit(f"{name}: stale broad shaped-flow admission remains: {needle}")


for name in (
    "current-viewer-fixed-pdf-input.yml",
    "carlton-current-viewer-supported-subset-pdf.yml",
    "carlton-alpha-smask-ab.yml",
):
    forbid(name, BROAD)
    require_all(name, (CARGO, VIEWER_BIN))

forbid("editor-current-fixed-pdf-resource-input.yml", BROAD)
require_all(
    "editor-current-fixed-pdf-resource-input.yml",
    (CARGO, LIB, FONT, TYPO, PAGES, RESOURCES, FIXED_BIN),
)

forbid("editor-newsletter-continuity-v1.yml", BROAD)
require_all(
    "editor-newsletter-continuity-v1.yml",
    (CARGO, LIB, FONT, TYPO, PAGES, RESOURCES, FIXED_BIN, W2_BIN),
)

require_all(
    "editor-desktop-text-session-restore-v1.yml",
    (LIB, TYPO),
)
require_all(
    "w2-longform-fixed-pdf-current-revision.yml",
    (LIB, FONT, TYPO, PAGES, RESOURCES, FIXED_BIN, W2_BIN),
)
require_all(
    "editor-build-once-pr.yml",
    (LIB, FONT, TYPO, PAGES, RESOURCES, FIXED_BIN),
)

forbid("editor-desktop-shaped-flow-runtime-v1.yml", BROAD)
require_all(
    "editor-desktop-shaped-flow-runtime-v1.yml",
    (CARGO, LIB, FONT, TYPO),
)
forbid("editor-desktop-shaped-flow-runtime-v1.yml", PAGES)
forbid("editor-desktop-shaped-flow-runtime-v1.yml", RESOURCES)

expected_library_sources = {
    f"{RUNTIME}/src/{path.name}"
    for path in Path(RUNTIME, "src").glob("*.rs")
}
owner_workflows = (
    "editor-desktop-shaped-flow-runtime-v1.yml",
    "editor-current-fixed-pdf-resource-input.yml",
    "editor-newsletter-continuity-v1.yml",
    "w2-longform-fixed-pdf-current-revision.yml",
    "editor-build-once-pr.yml",
)
owner_bodies = {name: text(name) for name in owner_workflows}
missing_owner = sorted(
    path
    for path in expected_library_sources
    if not any(path in body for body in owner_bodies.values())
)
if missing_owner:
    raise SystemExit(
        "top-level shaped-flow library source has no workflow owner: "
        + ", ".join(missing_owner)
    )

print("shaped-flow runtime routing contract: PASS")
