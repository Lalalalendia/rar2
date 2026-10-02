#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import io
import json
import pathlib
import time
import urllib.parse
import urllib.request
import zipfile
from collections import Counter

UA = "Chaptera-Migration-Corpus/2.0"
CORE_EXPECTED = 65
EXTENDED_EXPECTED = 86


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch_bytes(url: str, attempts: int = 3, timeout: int = 90) -> bytes:
    last = None
    for attempt in range(attempts):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=timeout) as response:
                return response.read()
        except Exception as exc:
            last = exc
            if attempt + 1 < attempts:
                time.sleep(2 ** attempt)
    raise RuntimeError(f"download failed after {attempts} attempts: {url}: {last}")


def load_json(path: pathlib.Path):
    return json.loads(path.read_text(encoding="utf-8"))


def build_entries(root: pathlib.Path, profile: str):
    reader = load_json(root / "tools/reader_corpus_manifest_v1.json")
    lalamu = load_json(
        root / "tools/corpus/receipts/lalamu-materialized-35-input-2026-09-29.json"
    )
    training = load_json(
        root
        / "tools/corpus/receipts/version-labelled-training-29-input-2026-09-25.json"
    )

    entries = []

    base = reader["upstream"]["base_url"].rstrip("/")
    for item in reader["fixtures"]:
        entries.append(
            {
                "sha256": item["sha256"],
                "byte_len": item["byte_len"],
                "name": item["name"],
                "stratum": "apache_poi",
                "family": item["family"],
                "semantic": bool(item["semantic"]),
                "kind": "direct",
                "url": base + "/" + urllib.parse.quote(item["name"]),
            }
        )

    for item in lalamu["files"]:
        if profile == "core65" and item["source_type"] != "github":
            continue
        entries.append(
            {
                "sha256": item["sha256"],
                "byte_len": item["size"],
                "name": item["source_filename"],
                "stratum": "lalamu_" + item["source_type"],
                "family": item["candidate_id"],
                "semantic": True,
                "kind": "direct",
                "url": item["source_url"],
            }
        )

    for package in training["packages"]:
        seen_in_package = set()
        for item in package["files"]:
            sha = item["sha256"]
            if sha in seen_in_package:
                continue
            seen_in_package.add(sha)
            entries.append(
                {
                    "sha256": sha,
                    "byte_len": item["size"],
                    "name": item["filename"],
                    "stratum": "microsoft_press_" + package["key"],
                    "family": package["key"],
                    "semantic": True,
                    "kind": "zip_member",
                    "url": package["source_download"],
                    "member": item["source_archive_path"],
                }
            )

    by_sha = {}
    for entry in entries:
        existing = by_sha.get(entry["sha256"])
        if existing is not None:
            raise RuntimeError(
                "cross-stratum duplicate in migration corpus: "
                f"{entry['sha256']} ({existing['stratum']} vs {entry['stratum']})"
            )
        by_sha[entry["sha256"]] = entry

    result = sorted(by_sha.values(), key=lambda row: (row["stratum"], row["sha256"]))
    expected = CORE_EXPECTED if profile == "core65" else EXTENDED_EXPECTED
    if len(result) != expected:
        counts = Counter(row["stratum"] for row in result)
        raise RuntimeError(
            f"{profile}: expected {expected} unique fixtures, got {len(result)}; "
            f"strata={dict(sorted(counts.items()))}"
        )
    return result


def materialize(args):
    root = args.repo_root.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    native = out / "native"
    native.mkdir(parents=True, exist_ok=True)

    entries = build_entries(root, args.profile)
    full_count = len(entries)
    selected = [
        row
        for index, row in enumerate(entries)
        if index % args.shard_count == args.shard_index
    ]

    zip_cache = {}
    materialized = []
    for entry in selected:
        if entry["kind"] == "direct":
            data = fetch_bytes(entry["url"])
        elif entry["kind"] == "zip_member":
            archive = zip_cache.get(entry["url"])
            if archive is None:
                archive = fetch_bytes(entry["url"], timeout=180)
                zip_cache[entry["url"]] = archive
            with zipfile.ZipFile(io.BytesIO(archive)) as zf:
                try:
                    data = zf.read(entry["member"])
                except KeyError as exc:
                    candidates = [name for name in zf.namelist() if name.endswith(entry["name"])]
                    raise RuntimeError(
                        f"archive member missing: {entry['member']}; filename matches={candidates}"
                    ) from exc
        else:
            raise RuntimeError(f"unsupported acquisition kind {entry['kind']}")

        actual_sha = sha256_bytes(data)
        if actual_sha != entry["sha256"]:
            raise RuntimeError(
                f"{entry['name']}: SHA-256 mismatch: {actual_sha} != {entry['sha256']}"
            )
        if len(data) != entry["byte_len"]:
            raise RuntimeError(
                f"{entry['name']}: byte length mismatch: {len(data)} != {entry['byte_len']}"
            )

        path = native / f"{actual_sha}.pub"
        path.write_bytes(data)
        row = dict(entry)
        row["path"] = str(path)
        materialized.append(row)

    manifest = {
        "schema": "chaptera.migration-corpus-materialization.v2",
        "profile": args.profile,
        "full_fixture_count": full_count,
        "shard_index": args.shard_index,
        "shard_count": args.shard_count,
        "selected_fixture_count": len(materialized),
        "stratum_counts": dict(
            sorted(Counter(row["stratum"] for row in materialized).items())
        ),
        "entries": materialized,
    }
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({k: manifest[k] for k in (
        "profile",
        "full_fixture_count",
        "shard_index",
        "shard_count",
        "selected_fixture_count",
        "stratum_counts",
    )}, indent=2, sort_keys=True))


def summarize(args):
    root = args.input.resolve()
    route_paths = sorted(root.rglob("route.json"))
    rows = []
    violations = []
    seen = set()

    for route_path in route_paths:
        data = load_json(route_path)
        sha = data["source_sha256"]
        if sha in seen:
            raise RuntimeError(f"duplicate route receipt for {sha}")
        seen.add(sha)
        parent = route_path.parent
        idml = data["targets"]["idml"]
        odg = data["targets"]["odg"]
        row = {
            "source_sha256": sha,
            "source_label": data["source_label"],
            "source_bytes": data["source_bytes"],
            "open_state": data["open_state"],
            "idml_state": idml["state"],
            "idml_counts": idml.get("counts"),
            "idml_materialized": bool(idml.get("materialized")),
            "idml_consumer_opened": (parent / "idml.consumer.ok").is_file(),
            "odg_state": odg["state"],
            "odg_counts": odg.get("counts"),
            "odg_materialized": bool(odg.get("materialized")),
            "odg_consumer_opened": (parent / "odg.consumer.ok").is_file(),
        }
        for target in ("idml", "odg"):
            if row[f"{target}_materialized"] and not row[f"{target}_consumer_opened"]:
                violations.append(
                    {
                        "source_sha256": sha,
                        "target": target,
                        "code": "materialized_artifact_failed_consumer_validation",
                    }
                )
            if row[f"{target}_consumer_opened"] and not row[f"{target}_materialized"]:
                violations.append(
                    {
                        "source_sha256": sha,
                        "target": target,
                        "code": "consumer_marker_without_materialized_artifact",
                    }
                )
        rows.append(row)

    if len(rows) != args.expected_count:
        violations.append(
            {
                "code": "fixture_count_mismatch",
                "expected": args.expected_count,
                "actual": len(rows),
            }
        )

    def target_count(target, state):
        return sum(row[f"{target}_state"] == state for row in rows)

    summary = {
        "schema": "chaptera.migration-editable-route-corpus.v2",
        "fixture_count": len(rows),
        "rows": rows,
        "totals": {
            "editor_admitted": sum(row["open_state"] == "admitted" for row in rows),
            "editor_not_admitted": sum(row["open_state"] != "admitted" for row in rows),
            "idml_available_with_declared_losses": target_count(
                "idml", "available_with_declared_losses"
            ),
            "idml_unavailable": target_count("idml", "unavailable"),
            "idml_not_verified": target_count("idml", "not_verified"),
            "idml_consumer_opened": sum(row["idml_consumer_opened"] for row in rows),
            "odg_available_with_declared_losses": target_count(
                "odg", "available_with_declared_losses"
            ),
            "odg_unavailable": target_count("odg", "unavailable"),
            "odg_not_verified": target_count("odg", "not_verified"),
            "odg_consumer_opened": sum(row["odg_consumer_opened"] for row in rows),
            "hard_violation_count": len(violations),
        },
        "hard_violations": violations,
        "claims": {
            "universal_pub_editable_export": False,
            "native_pub_save": False,
            "exact_source_admission_required": True,
            "consumer_validation_required_for_materialized_outputs": True,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(summary["totals"], indent=2, sort_keys=True))
    if violations:
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("materialize")
    p.add_argument("--repo-root", type=pathlib.Path, default=pathlib.Path("."))
    p.add_argument("--profile", choices=("core65", "extended86"), default="core65")
    p.add_argument("--out", required=True, type=pathlib.Path)
    p.add_argument("--shard-index", type=int, default=0)
    p.add_argument("--shard-count", type=int, default=1)
    p.set_defaults(func=materialize)

    p = sub.add_parser("summarize")
    p.add_argument("--input", required=True, type=pathlib.Path)
    p.add_argument("--output", required=True, type=pathlib.Path)
    p.add_argument("--expected-count", required=True, type=int)
    p.set_defaults(func=summarize)

    args = parser.parse_args()
    if getattr(args, "shard_count", 1) <= 0:
        raise SystemExit("--shard-count must be positive")
    if getattr(args, "shard_index", 0) < 0 or getattr(args, "shard_index", 0) >= getattr(args, "shard_count", 1):
        raise SystemExit("--shard-index must be within shard count")
    args.func(args)


if __name__ == "__main__":
    main()
