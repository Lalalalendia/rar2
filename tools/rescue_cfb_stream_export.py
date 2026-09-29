#!/usr/bin/env python3
"""Export physical CFB streams by (source_sha256, directory SID).

This is an internal/local Rescue bundle. It is intentionally separate from the
public chaptera.rescue-recovery-producer-receipt.v1 contract.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

try:
    from .corpus.cfb_physical import CFB, sha256_bytes
except ImportError:
    from corpus.cfb_physical import CFB, sha256_bytes

SCHEMA = "chaptera.recovery-cfb-stream-export.v1"


def _safe_name(value: str) -> str:
    value = value.strip().lower()
    value = re.sub(r"[^a-z0-9._-]+", "-", value)
    value = value.strip("-._")
    return value[:64] or "stream"


def artifact_name(sid: int, descriptive_name: str) -> str:
    # SID owns uniqueness; descriptive name is metadata only.
    return f"sid-{sid:06d}-{_safe_name(descriptive_name)}.bin"


def build_export_manifest(
    source_bytes: bytes,
    output_dir: Path,
) -> dict[str, Any]:
    cfb = CFB(source_bytes)
    entries: list[dict[str, Any]] = []

    output_dir.mkdir(parents=True, exist_ok=True)
    for entry in cfb.dirs:
        if entry["type"] != 2 or not entry["size"]:
            continue
        sid = int(entry["i"])
        descriptor = cfb.stream_descriptor_by_sid(sid)
        payload = cfb.read_stream_by_sid(sid)
        file_name = artifact_name(sid, entry["name"])
        artifact_path = output_dir / file_name
        artifact_path.write_bytes(payload)
        written = artifact_path.read_bytes()
        if written != payload:
            raise RuntimeError(f"artifact write mismatch for SID {sid}")

        entries.append(
            {
                "sid": sid,
                "descriptive_name": entry["name"],
                "declared_size": descriptor["declared_size"],
                "storage": descriptor["storage"],
                "chain": descriptor["chain"],
                "physical_ranges": descriptor["physical_ranges"],
                "payload_sha256": descriptor["payload_sha256"],
                "payload_byte_len": descriptor["payload_byte_len"],
                "artifact_path": file_name,
                "artifact_sha256": sha256_bytes(written),
            }
        )

    return {
        "schema_version": SCHEMA,
        "source": {
            "sha256": cfb.source_sha256,
            "byte_len": cfb.source_byte_len,
        },
        "identity": "(source_sha256,directory_entry_sid)",
        "artifact_count": len(entries),
        "streams": entries,
    }


def export_file(
    source: Path,
    output_dir: Path,
    manifest_path: Path,
) -> dict[str, Any]:
    before = source.read_bytes()
    before_sha = sha256_bytes(before)
    manifest = build_export_manifest(before, output_dir)
    after = source.read_bytes()
    if sha256_bytes(after) != before_sha or after != before:
        raise RuntimeError("source bytes changed during physical stream export")

    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    result = export_file(args.source, args.output_dir, args.manifest)
    print(
        json.dumps(
            {
                "schema_version": result["schema_version"],
                "source_sha256": result["source"]["sha256"],
                "source_byte_len": result["source"]["byte_len"],
                "artifact_count": result["artifact_count"],
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
