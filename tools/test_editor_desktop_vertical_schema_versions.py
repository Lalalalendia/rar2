#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import pathlib
import re
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

RUNNER = TOOLS / "run_local_editor_desktop_vertical.py"
PUB_EDITOR = ROOT / "vendor" / "producer-a" / "crates" / "pub-editor" / "src" / "lib.rs"
RECEIPT_SCHEMA = (
    ROOT
    / "packages"
    / "product"
    / "editor-desktop-vertical"
    / "v1"
    / "acceptance-receipt.schema.json"
)


def current_pub_editor_schema() -> str:
    source = PUB_EDITOR.read_text(encoding="utf-8")
    current = re.search(
        r'pub const EDITOR_PROJECT_VERSION_CURRENT: &str = (EDITOR_PROJECT_VERSION_V0_\d+);',
        source,
    )
    if current is None:
        raise AssertionError("cannot resolve EDITOR_PROJECT_VERSION_CURRENT")
    symbol = current.group(1)
    value = re.search(
        rf'pub const {re.escape(symbol)}: &str = "([^"]+)";',
        source,
    )
    if value is None:
        raise AssertionError(f"cannot resolve {symbol}")
    return value.group(1)


def load_runner():
    spec = importlib.util.spec_from_file_location("desktop_vertical_runner", RUNNER)
    if spec is None or spec.loader is None:
        raise AssertionError("cannot load Desktop V0 runner")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DesktopVerticalSchemaSyncTests(unittest.TestCase):
    def test_current_pub_editor_schema_is_admitted_everywhere(self):
        runner = load_runner()
        current = current_pub_editor_schema()
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        receipt_versions = set(
            schema["properties"]["project"]["properties"]["schema_version"]["enum"]
        )

        self.assertIn(current, runner.SUPPORTED_EDITOR_PROJECT_SCHEMA_VERSIONS)
        self.assertIn(current, receipt_versions)

    def test_identity_requirement_tracks_current_identity_era(self):
        runner = load_runner()
        current = current_pub_editor_schema()
        version = int(current.rsplit(".", 1)[1])
        self.assertGreaterEqual(version, 11)
        self.assertIn(current, runner.IDENTITY_REQUIRED_EDITOR_PROJECT_SCHEMA_VERSIONS)

    def test_v013_is_explicitly_supported(self):
        runner = load_runner()
        self.assertIn("pub-editor-v0.12", runner.SUPPORTED_EDITOR_PROJECT_SCHEMA_VERSIONS)
        self.assertIn("pub-editor-v0.13", runner.SUPPORTED_EDITOR_PROJECT_SCHEMA_VERSIONS)
        self.assertIn(
            "pub-editor-v0.13",
            runner.IDENTITY_REQUIRED_EDITOR_PROJECT_SCHEMA_VERSIONS,
        )


if __name__ == "__main__":
    unittest.main()
