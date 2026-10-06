#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
from typing import Any


def walk(value: Any, path: tuple[str, ...] = ()):
    if isinstance(value, dict):
        yield path, value
        for key, child in value.items():
            yield from walk(child, path + (str(key),))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk(child, path + (str(index),))


def numeric_nonzero(value: Any) -> bool:
    if isinstance(value, bool):
        return False
    if isinstance(value, int):
        return value != 0
    if isinstance(value, dict):
        return any(numeric_nonzero(child) for child in value.values())
    if isinstance(value, list):
        return any(numeric_nonzero(child) for child in value)
    return False


def inspect(producer: pathlib.Path, pub_path: pathlib.Path) -> dict[str, Any]:
    completed = subprocess.run(
        [str(producer), str(pub_path)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        return {
            "file": pub_path.name,
            "status": "open_unsupported",
            "stderr": completed.stderr.strip()[-1000:],
        }

    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return {
            "file": pub_path.name,
            "status": "invalid_viewer_json",
            "error": str(error),
        }

    crops = []
    for path, obj in walk(value):
        crop = obj.get("explicit_image_crop")
        if crop is None:
            continue
        crops.append(
            {
                "json_path": "/".join(path),
                "nonzero": numeric_nonzero(crop),
                "ambiguous": bool(crop.get("ambiguous")) if isinstance(crop, dict) else None,
                "crop": crop,
            }
        )

    return {
        "file": pub_path.name,
        "status": "opened",
        "explicit_crop_count": len(crops),
        "nonzero_crop_count": sum(1 for item in crops if item["nonzero"]),
        "unambiguous_nonzero_crop_count": sum(
            1 for item in crops if item["nonzero"] and item["ambiguous"] is False
        ),
        "crops": crops,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--producer", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("pubs", nargs="+", type=pathlib.Path)
    args = parser.parse_args()

    rows = [inspect(args.producer, path) for path in args.pubs]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    hits = [
        row["file"]
        for row in rows
        if row.get("unambiguous_nonzero_crop_count", 0) > 0
    ]
    print(json.dumps({"status": "valid", "unambiguous_nonzero_crop_files": hits}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
