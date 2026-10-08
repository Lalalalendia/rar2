#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checked_sha(value: str, label: str) -> str:
    normalized = value.strip().lower()
    if not SHA256_RE.fullmatch(normalized):
        raise SystemExit(f"{label} must be exactly 64 lowercase hexadecimal characters")
    return normalized


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a pub-re experiment manifest from a pinned reader-corpus materialization receipt."
    )
    parser.add_argument("--materialization", type=Path, required=True)
    parser.add_argument("--before-sha256", required=True)
    parser.add_argument("--after-sha256", required=True)
    parser.add_argument("--question", required=True)
    parser.add_argument("--experiment-id", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    before_sha = checked_sha(args.before_sha256, "before_sha256")
    after_sha = checked_sha(args.after_sha256, "after_sha256")
    if not args.question.strip():
        raise SystemExit("question must not be empty")
    if not args.experiment_id.strip():
        raise SystemExit("experiment_id must not be empty")

    receipt = json.loads(args.materialization.read_text(encoding="utf-8"))
    if receipt.get("schema") != "chaptera.reader-corpus-materialization.v1":
        raise SystemExit("unsupported materialization receipt schema")

    entries = {entry["sha256"]: entry for entry in receipt.get("entries", [])}
    missing = [value for value in (before_sha, after_sha) if value not in entries]
    if missing:
        raise SystemExit(
            "requested SHA-256 is not present in the pinned reader corpus: " + ", ".join(missing)
        )

    selected = []
    for label, expected in (("before", before_sha), ("after", after_sha)):
        path = Path(entries[expected]["path"])
        if not path.is_file():
            raise SystemExit(f"{label} materialized fixture is missing")
        actual = sha256(path)
        if actual != expected:
            raise SystemExit(f"{label} materialized SHA-256 mismatch: {actual} != {expected}")
        selected.append((label, path, expected))

    manifest = {
        "schema": "chaptera.pub-re-experiment.v1",
        "experiment_id": args.experiment_id.strip(),
        "question": args.question.strip(),
        "before": {
            "path": str(selected[0][1]),
            "expected_sha256": selected[0][2],
        },
        "after": {
            "path": str(selected[1][1]),
            "expected_sha256": selected[1][2],
        },
        "policy": {
            "max_stream_bytes": 67108864,
            "max_changed_ranges_per_stream": 128,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "schema": manifest["schema"],
                "experiment_id": manifest["experiment_id"],
                "before_sha256": before_sha,
                "after_sha256": after_sha,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
