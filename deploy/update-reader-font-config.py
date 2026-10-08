#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import os
import sys

BEGIN = "# BEGIN CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1"
END = "# END CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1"
RESOURCES = (
    ("Calibri", "/opt/chaptera/current/fonts/Carlito-Regular.ttf", "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595", "font/ttf"),
    ("Cambria", "/opt/chaptera/current/fonts/Caladea-Regular.ttf", "f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd", "font/ttf"),
)


def strip_managed_block(text: str) -> str:
    clean: list[str] = []
    in_managed = False
    for line in text.splitlines():
        if line == BEGIN:
            in_managed = True
            continue
        if line == END:
            in_managed = False
            continue
        if not in_managed:
            clean.append(line)
    if in_managed:
        raise ValueError("unterminated managed open-font block")
    return "\n".join(clean).rstrip() + "\n"


def configured_families(text: str) -> set[str]:
    configured: set[str] = set()
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("source_family") or "=" not in stripped:
            continue
        value = stripped.split("=", 1)[1].strip()
        if len(value) >= 2 and value[0] == value[-1] == '"':
            configured.add(value[1:-1].strip().casefold())
    return configured


def render_managed_block(existing: set[str]) -> str:
    blocks: list[str] = []
    for family, resource_path, digest, mime in RESOURCES:
        if family.casefold() in existing:
            continue
        blocks.append(
            "[[cloud_reader_guest.font_resources]]\n"
            f'source_family = "{family}"\n'
            f'path = "{resource_path}"\n'
            f'expected_sha256 = "{digest}"\n'
            "face_index = 0\n"
            f'mime = "{mime}"\n'
        )
    if not blocks:
        return ""
    return (
        "\n"
        + BEGIN
        + "\n"
        + "# Deterministic open replacements shipped in the immutable Reader release.\n"
        + "# Existing operator-owned entries for these families take precedence.\n"
        + "\n".join(blocks)
        + END
        + "\n"
    )


def update(path: Path) -> None:
    original = path.read_text(encoding="utf-8")
    clean = strip_managed_block(original)
    result = clean + render_managed_block(configured_families(clean))
    if result == original:
        return
    mode = path.stat().st_mode & 0o777
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(result, encoding="utf-8")
    os.chmod(tmp, mode)
    os.replace(tmp, path)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: update-reader-font-config.py <chaptera.toml>")
    update(Path(sys.argv[1]))


if __name__ == "__main__":
    main()
