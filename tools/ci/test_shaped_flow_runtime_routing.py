#!/usr/bin/env python3
from pathlib import Path


GENERIC = Path(".github/workflows/editor-desktop-shaped-flow-runtime-v1.yml")
FIXED = Path(".github/workflows/editor-current-fixed-pdf-resource-input.yml")
NEW_MODULE = "crates/chaptera-desktop-shaped-flow-runtime/src/current_fixed_pdf.rs"
SHARED_LIB = "crates/chaptera-desktop-shaped-flow-runtime/src/lib.rs"
BROAD = "crates/chaptera-desktop-shaped-flow-runtime/**"


def quoted_path(text: str, path: str) -> bool:
    return f'- "{path}"' in text or f"- '{path}'" in text


def main() -> int:
    generic = GENERIC.read_text(encoding="utf-8")
    fixed = FIXED.read_text(encoding="utf-8")
    violations = []

    if quoted_path(generic, BROAD):
        violations.append("generic shaped-flow workflow must not regain crate-wide /** admission")
    if quoted_path(generic, NEW_MODULE):
        violations.append("fixed-PDF-only module must not admit generic shaped-flow workflow")
    if not quoted_path(generic, SHARED_LIB):
        violations.append("generic shaped-flow workflow lost shared lib.rs ownership")

    if quoted_path(fixed, BROAD):
        violations.append("fixed-PDF workflow must use explicit owned paths, not crate-wide /**")
    for required in (NEW_MODULE, SHARED_LIB):
        if not quoted_path(fixed, required):
            violations.append(f"fixed-PDF workflow missing owned/shared path {required}")

    if violations:
        raise SystemExit("\n".join(violations))

    print("shaped-flow routing split: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
