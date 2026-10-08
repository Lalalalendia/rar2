#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import urllib.parse
import urllib.request

UA = "Chaptera-PUB-VBA-Estate/1.0"


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(request, timeout=90) as response:
        return response.read()


def main() -> int:
    parser = argparse.ArgumentParser(description="Materialize exact reader PUB corpus from its pinned manifest")
    parser.add_argument("--manifest", type=pathlib.Path, default=pathlib.Path("tools/reader_corpus_manifest_v1.json"))
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    base = manifest["upstream"]["base_url"].rstrip("/")
    out = args.out.resolve()
    native = out / "native"
    native.mkdir(parents=True, exist_ok=True)

    rows = []
    for item in manifest["fixtures"]:
        url = base + "/" + urllib.parse.quote(item["name"])
        data = fetch(url)
        actual_sha = hashlib.sha256(data).hexdigest()
        if actual_sha != item["sha256"]:
            raise RuntimeError(f"{item['name']}: SHA-256 mismatch: {actual_sha} != {item['sha256']}")
        if len(data) != item["byte_len"]:
            raise RuntimeError(f"{item['name']}: byte length mismatch: {len(data)} != {item['byte_len']}")
        path = native / f"{actual_sha}.pub"
        path.write_bytes(data)
        rows.append({
            "sha256": actual_sha,
            "byte_len": len(data),
            "family": item["family"],
            "semantic": bool(item["semantic"]),
            "path": str(path),
        })

    receipt = {
        "schema": "chaptera.reader-corpus-materialization.v1",
        "source_manifest_schema": manifest["schema_version"],
        "upstream_repository": manifest["upstream"]["repository"],
        "upstream_commit": manifest["upstream"]["commit"],
        "fixture_count": len(rows),
        "entries": sorted(rows, key=lambda row: row["sha256"]),
    }
    (out / "materialization.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"fixture_count": len(rows), "upstream_commit": receipt["upstream_commit"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
