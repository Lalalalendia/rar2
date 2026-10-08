#!/usr/bin/env python3
from __future__ import annotations

import hashlib
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
UPDATER = ROOT / "deploy/update-reader-font-config.py"
CONFIG = ROOT / "deploy/config/chaptera.reader.prod.example.toml"

EXPECTED = {
    "Carlito-Regular.ttf": "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595",
    "Caladea-Regular.ttf": "f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd",
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


for name, expected in EXPECTED.items():
    path = ROOT / "deploy/fonts" / name
    assert path.is_file(), path
    assert digest(path) == expected, (name, digest(path), expected)

source = CONFIG.read_text(encoding="utf-8")
assert source.count("# BEGIN CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1") == 1
assert source.count("# END CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1") == 1
assert 'source_family = "Calibri"' in source
assert 'source_family = "Cambria"' in source
assert EXPECTED["Carlito-Regular.ttf"] in source
assert EXPECTED["Caladea-Regular.ttf"] in source
assert source.count("layout_authoritative = false") == 2

with tempfile.TemporaryDirectory() as td:
    target = Path(td) / "chaptera.toml"
    target.write_text(source, encoding="utf-8")

    subprocess.run(["python3", str(UPDATER), str(target)], check=True)
    once = target.read_text(encoding="utf-8")
    subprocess.run(["python3", str(UPDATER), str(target)], check=True)
    twice = target.read_text(encoding="utf-8")
    assert once == twice
    assert twice.count('source_family = "Calibri"') == 1
    assert twice.count('source_family = "Cambria"') == 1
    assert twice.count("layout_authoritative = false") == 2

    begin = twice.index("# BEGIN CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1")
    end = twice.index("# END CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1")
    operator = (
        twice[:begin].rstrip()
        + "\n\n[[cloud_reader_guest.font_resources]]\n"
        + 'source_family = "Calibri"\n'
        + 'path = "/etc/chaptera/fonts/operator-calibri.ttf"\n'
        + 'expected_sha256 = "' + ("1" * 64) + '"\n'
        + "face_index = 0\n"
        + 'mime = "font/ttf"\n'
        + twice[end + len("# END CHAPTERA MANAGED OPEN FONT REPLACEMENTS V1"):]
    )
    target.write_text(operator, encoding="utf-8")
    subprocess.run(["python3", str(UPDATER), str(target)], check=True)
    overridden = target.read_text(encoding="utf-8")
    assert overridden.count('source_family = "Calibri"') == 1
    assert '/etc/chaptera/fonts/operator-calibri.ttf' in overridden
    assert '/opt/chaptera/current/fonts/Carlito-Regular.ttf' not in overridden
    assert overridden.count('source_family = "Cambria"') == 1
    assert '/opt/chaptera/current/fonts/Caladea-Regular.ttf' in overridden

print("cloud reader open-font packet contract: OK")
