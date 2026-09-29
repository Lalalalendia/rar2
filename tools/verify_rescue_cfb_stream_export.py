#!/usr/bin/env python3
"""Independent verifier for chaptera.recovery-cfb-stream-export.v1."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

try:
    from .corpus.cfb_physical import CFB, sha256_bytes
except ImportError:
    from corpus.cfb_physical import CFB, sha256_bytes

SCHEMA = "chaptera.recovery-cfb-stream-export.v1"
RECEIPT_SCHEMA = "chaptera.recovery-cfb-stream-export-verification.v1"


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def verify_export(
    source_bytes: bytes,
    artifact_dir: Path,
    manifest: dict[str, Any],
) -> dict[str, Any]:
    _require(manifest.get("schema_version") == SCHEMA, "unexpected export schema")

    cfb = CFB(source_bytes)
    source = manifest.get("source")
    _require(isinstance(source, dict), "manifest source is missing")
    _require(source.get("sha256") == cfb.source_sha256, "source SHA mismatch")
    _require(source.get("byte_len") == cfb.source_byte_len, "source length mismatch")
    _require(
        manifest.get("identity") == "(source_sha256,directory_entry_sid)",
        "wrong stream identity law",
    )

    streams = manifest.get("streams")
    _require(isinstance(streams, list), "streams must be a list")

    expected_sids = {
        int(entry["i"])
        for entry in cfb.dirs
        if entry["type"] == 2 and entry["size"]
    }
    actual_sids = [int(item.get("sid", -1)) for item in streams if isinstance(item, dict)]
    _require(len(actual_sids) == len(set(actual_sids)), "duplicate SID in manifest")
    _require(set(actual_sids) == expected_sids, "manifest does not cover exact stream SID set")
    _require(manifest.get("artifact_count") == len(streams), "artifact_count mismatch")

    artifact_paths: list[str] = []
    verified: list[dict[str, Any]] = []

    for item in streams:
        _require(isinstance(item, dict), "stream entry must be an object")
        sid = int(item["sid"])
        entry = cfb.stream_entry_by_sid(sid)
        descriptor = cfb.stream_descriptor_by_sid(sid)

        _require(item.get("descriptive_name") == entry["name"], f"SID {sid}: name mismatch")
        _require(item.get("declared_size") == descriptor["declared_size"], f"SID {sid}: size mismatch")
        _require(item.get("storage") == descriptor["storage"], f"SID {sid}: storage mismatch")
        _require(item.get("chain") == descriptor["chain"], f"SID {sid}: chain mismatch")
        _require(
            item.get("physical_ranges") == descriptor["physical_ranges"],
            f"SID {sid}: physical ranges mismatch",
        )
        _require(
            item.get("payload_sha256") == descriptor["payload_sha256"],
            f"SID {sid}: payload SHA mismatch",
        )
        _require(
            item.get("payload_byte_len") == descriptor["payload_byte_len"],
            f"SID {sid}: payload length mismatch",
        )

        relative = item.get("artifact_path")
        _require(isinstance(relative, str) and relative, f"SID {sid}: artifact path missing")
        _require(relative.startswith(f"sid-{sid:06d}-"), f"SID {sid}: artifact path not SID-keyed")
        _require("/" not in relative and "\\" not in relative, f"SID {sid}: artifact path must be flat")
        artifact_paths.append(relative)

        artifact = (artifact_dir / relative).read_bytes()
        expected_payload = cfb.read_stream_by_sid(sid)
        _require(artifact == expected_payload, f"SID {sid}: artifact bytes differ from source stream")
        artifact_sha = sha256_bytes(artifact)
        _require(item.get("artifact_sha256") == artifact_sha, f"SID {sid}: artifact SHA mismatch")

        verified.append(
            {
                "sid": sid,
                "payload_sha256": descriptor["payload_sha256"],
                "payload_byte_len": descriptor["payload_byte_len"],
                "artifact_sha256": artifact_sha,
            }
        )

    _require(len(artifact_paths) == len(set(artifact_paths)), "artifact paths are not unique")

    return {
        "schema_version": RECEIPT_SCHEMA,
        "source_sha256": cfb.source_sha256,
        "source_byte_len": cfb.source_byte_len,
        "identity": "(source_sha256,directory_entry_sid)",
        "verified_stream_count": len(verified),
        "verified_streams": sorted(verified, key=lambda item: item["sid"]),
        "source_safe": True,
        "pass": True,
    }


def verify_files(
    source: Path,
    artifact_dir: Path,
    manifest_path: Path,
) -> dict[str, Any]:
    before = source.read_bytes()
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    receipt = verify_export(before, artifact_dir, manifest)
    after = source.read_bytes()
    _require(before == after, "source bytes changed during verification")
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("--artifact-dir", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()

    receipt = verify_files(args.source, args.artifact_dir, args.manifest)
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.receipt is not None:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
