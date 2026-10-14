#!/usr/bin/env python3
import json
from pathlib import Path
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "packages/protocol/editor-render-scene/v1/snapshot.schema.json"
FIXTURES = ROOT / "packages/protocol/editor-render-scene/v1/fixtures"
CANONICAL = ROOT / "packages/protocol/scene/v1/fixtures/simple-text.json"
FORBIDDEN = {
    "raw_pub_bytes", "raw_bytes", "source_path", "filesystem_path",
    "cfb_path", "stream_path", "stream_name", "parser_record",
    "carrier", "source_ref", "byte_range",
}

def walk(value, at="$"):
    if isinstance(value, list):
        for index, child in enumerate(value):
            walk(child, f"{at}[{index}]")
    elif isinstance(value, dict):
        for key, child in value.items():
            if key in FORBIDDEN:
                raise AssertionError(f"forbidden source carrier {key} at {at}")
            walk(child, f"{at}.{key}")

def main():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema)
    fixtures = sorted(FIXTURES.glob("*.json"))
    if not fixtures:
        raise AssertionError("editor render-scene fixtures missing")
    for path in fixtures:
        value = json.loads(path.read_text(encoding="utf-8"))
        errors = sorted(validator.iter_errors(value), key=lambda e: list(e.path))
        if errors:
            detail = "\n".join(f"{list(e.path)}: {e.message}" for e in errors)
            raise AssertionError(f"{path.name} schema errors\n{detail}")
        walk(value)
        if value["protocol_version"] != "chaptera.editor-render-scene.v1":
            raise AssertionError("wrong protocol version")
        node_ids = [node["node_id"] for node in value["nodes"]]
        if len(node_ids) != len(set(node_ids)):
            raise AssertionError("duplicate node ids")
        resources = [resource["resource_id"] for resource in value["resources"]]
        if len(resources) != len(set(resources)):
            raise AssertionError("duplicate resource ids")
        if not any(node.get("editable") is False and node["node_id"].startswith("scene-instance:") for node in value["nodes"]):
            raise AssertionError("fixture must prove non-editable projected instance")
        if not any(node.get("editable") is True for node in value["nodes"]):
            raise AssertionError("fixture must prove canonical editable node")

    canonical = json.loads(CANONICAL.read_text(encoding="utf-8"))
    if not list(validator.iter_errors(canonical)):
        raise AssertionError("canonical chaptera.scene.v1 must not masquerade as editor render scene")
    print(f"validated {len(fixtures)} editor render-scene fixture(s)")

if __name__ == "__main__":
    main()
