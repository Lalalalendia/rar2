#!/usr/bin/env python3
import argparse
import concurrent.futures
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from collections import Counter

CORPUS_COUNT = 1050
CORPUS_SHA_LIST_DIGEST = "21e9c00963410e1004d407e6476f2e909c6561a4374b4ddadad052499e448d43"
PROBE_SCHEMA = "chaptera.migration-editable-route-probe.v1"
SUMMARY_SCHEMA = "chaptera.migration-editable-route-corpus.v2"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_reader_acceptance(path: pathlib.Path):
    payload = json.loads(path.read_text(encoding="utf-8"))
    rows = payload.get("rows") or []
    by_sha = {}
    for row in rows:
        sha = str(row.get("source_sha256") or "").lower()
        if SHA_RE.fullmatch(sha):
            by_sha[sha] = row
    return payload, by_sha


def run_probe(probe_exe, source_path, timeout_seconds, materialize_dir=None):
    sha = source_path.stem.lower()
    argv = [
        str(probe_exe),
        str(source_path),
        "--label",
        f"{sha}.pub",
    ]
    if materialize_dir is not None:
        argv += ["--materialize-dir", str(materialize_dir)]
    try:
        completed = subprocess.run(
            argv,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "sha256": sha,
            "probe_error": "timeout",
            "returncode": None,
            "stderr": None,
            "route": None,
        }

    if completed.returncode != 0:
        return {
            "sha256": sha,
            "probe_error": "nonzero_exit",
            "returncode": completed.returncode,
            "stderr": completed.stderr[-4000:],
            "route": None,
        }

    try:
        route = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return {
            "sha256": sha,
            "probe_error": "invalid_json",
            "returncode": completed.returncode,
            "stderr": completed.stderr[-4000:],
            "route": None,
        }

    return {
        "sha256": sha,
        "probe_error": None,
        "returncode": completed.returncode,
        "stderr": None,
        "route": route,
    }


def validate_route_result(result, source_path, reader_row):
    hard = []
    sha = source_path.stem.lower()
    if result["probe_error"] is not None:
        hard.append(result["probe_error"])
        return None, hard

    route = result["route"]
    if route.get("schema") != PROBE_SCHEMA:
        hard.append("probe_schema_mismatch")
    if str(route.get("source_sha256") or "").lower() != sha:
        hard.append("probe_source_sha_mismatch")
    if route.get("source_bytes") != source_path.stat().st_size:
        hard.append("probe_source_size_mismatch")

    targets = route.get("targets") or {}
    for target in ("idml", "odg"):
        target_row = targets.get(target) or {}
        state = target_row.get("state")
        counts = target_row.get("counts") or {}
        if state == "available_with_declared_losses" and int(counts.get("blocking") or 0) != 0:
            hard.append(f"{target}_available_with_blockers")

    reader_outcome = reader_row.get("outcome") if reader_row else None
    if reader_outcome is None:
        hard.append("reader_acceptance_missing")
    elif reader_outcome != "normal_open":
        for target in ("idml", "odg"):
            if (targets.get(target) or {}).get("state") == "available_with_declared_losses":
                hard.append(f"{target}_available_for_non_normal_reader_source")

    row = {
        "source_sha256": sha,
        "source_bytes": route.get("source_bytes"),
        "reader_outcome": reader_outcome,
        "reader_salvage_eligibility": (
            reader_row.get("salvage_eligibility") if reader_row else None
        ),
        "open_state": route.get("open_state"),
        "reason_code": route.get("reason_code"),
        "idml_state": (targets.get("idml") or {}).get("state"),
        "idml_reason_code": (targets.get("idml") or {}).get("reason_code"),
        "idml_counts": (targets.get("idml") or {}).get("counts"),
        "odg_state": (targets.get("odg") or {}).get("state"),
        "odg_reason_code": (targets.get("odg") or {}).get("reason_code"),
        "odg_counts": (targets.get("odg") or {}).get("counts"),
    }
    return row, hard


def evenly_spaced(items, count):
    if count <= 0 or not items:
        return []
    if len(items) <= count:
        return list(items)
    if count == 1:
        return [items[len(items) // 2]]

    selected = []
    seen = set()
    last = len(items) - 1
    for index in range(count):
        pos = round(index * last / (count - 1))
        item = items[pos]
        key = item["source_sha256"]
        if key not in seen:
            selected.append(item)
            seen.add(key)

    if len(selected) < count:
        for item in items:
            key = item["source_sha256"]
            if key in seen:
                continue
            selected.append(item)
            seen.add(key)
            if len(selected) == count:
                break
    return selected


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-dir", required=True, type=pathlib.Path)
    parser.add_argument("--reader-acceptance", required=True, type=pathlib.Path)
    parser.add_argument("--probe-exe", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--per-file-dir", required=True, type=pathlib.Path)
    parser.add_argument("--materialize-dir", required=True, type=pathlib.Path)
    parser.add_argument("--materialize-count", type=int, default=128)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--timeout-seconds", type=int, default=45)
    args = parser.parse_args()

    sources = sorted(args.corpus_dir.glob("*.pub"), key=lambda path: path.stem.lower())
    if len(sources) != CORPUS_COUNT:
        raise SystemExit(f"expected {CORPUS_COUNT} PUB files, got {len(sources)}")

    shas = []
    for source in sources:
        stem = source.stem.lower()
        if not SHA_RE.fullmatch(stem):
            raise SystemExit(f"corpus member is not SHA-named: {source.name}")
        actual = sha256_bytes(source.read_bytes())
        if actual != stem:
            raise SystemExit(f"corpus member SHA mismatch: {stem} != {actual}")
        shas.append(stem)

    digest = hashlib.sha256(
        "".join(sha + "\n" for sha in shas).encode("ascii")
    ).hexdigest()
    if digest != CORPUS_SHA_LIST_DIGEST:
        raise SystemExit(
            f"corpus SHA-list digest mismatch: {digest} != {CORPUS_SHA_LIST_DIGEST}"
        )

    reader_payload, reader_by_sha = load_reader_acceptance(args.reader_acceptance)
    if len(reader_by_sha) != CORPUS_COUNT:
        raise SystemExit(
            f"reader acceptance must cover {CORPUS_COUNT} exact sources, got {len(reader_by_sha)}"
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.per_file_dir.mkdir(parents=True, exist_ok=True)
    args.materialize_dir.mkdir(parents=True, exist_ok=True)

    hard_violations = []
    rows = []

    def census_one(source):
        result = run_probe(
            args.probe_exe,
            source,
            args.timeout_seconds,
            materialize_dir=None,
        )
        row, hard = validate_route_result(
            result,
            source,
            reader_by_sha.get(source.stem.lower()),
        )
        return source, result, row, hard

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = [executor.submit(census_one, source) for source in sources]
        for future in concurrent.futures.as_completed(futures):
            source, result, row, hard = future.result()
            sha = source.stem.lower()
            if row is not None:
                rows.append(row)
                (args.per_file_dir / f"{sha}.json").write_text(
                    json.dumps(row, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
            if hard:
                hard_violations.append(
                    {
                        "stage": "census",
                        "source_sha256": sha,
                        "violations": sorted(set(hard)),
                    }
                )
                if result.get("stderr"):
                    (args.per_file_dir / f"{sha}.stderr.txt").write_text(
                        result["stderr"],
                        encoding="utf-8",
                    )

    rows.sort(key=lambda row: row["source_sha256"])

    candidates = []
    for row in rows:
        if row["reader_outcome"] != "normal_open":
            continue
        if row["open_state"] != "admitted":
            continue
        if not (
            row["idml_state"] == "available_with_declared_losses"
            or row["odg_state"] == "available_with_declared_losses"
        ):
            continue
        idml_unsupported = int((row.get("idml_counts") or {}).get("unsupported") or 0)
        odg_unsupported = int((row.get("odg_counts") or {}).get("unsupported") or 0)
        row["_selection_key"] = (
            int(row["source_bytes"] or 0),
            idml_unsupported + odg_unsupported,
            row["source_sha256"],
        )
        candidates.append(row)

    candidates.sort(key=lambda row: row["_selection_key"])
    selected = evenly_spaced(candidates, args.materialize_count)
    selected_shas = {row["source_sha256"] for row in selected}

    census_by_sha = {row["source_sha256"]: row for row in rows}

    def materialize_one(source):
        sha = source.stem.lower()
        out = args.materialize_dir / sha
        out.mkdir(parents=True, exist_ok=True)
        result = run_probe(
            args.probe_exe,
            source,
            args.timeout_seconds,
            materialize_dir=out,
        )
        if result.get("stderr"):
            (args.per_file_dir / f"{sha}.materialize.stderr.txt").write_text(
                result["stderr"],
                encoding="utf-8",
            )
        row, hard = validate_route_result(
            result,
            source,
            reader_by_sha.get(sha),
        )
        if row is None:
            return sha, hard or ["materialization_probe_failed"]

        census = census_by_sha[sha]
        for key in (
            "open_state",
            "idml_state",
            "idml_counts",
            "odg_state",
            "odg_counts",
        ):
            if row.get(key) != census.get(key):
                hard.append(f"materialization_{key}_drift")

        for target, extension in (("idml", "idml"), ("odg", "odg")):
            state = row[f"{target}_state"]
            artifact = out / f"output.{extension}"
            if state == "available_with_declared_losses":
                if not artifact.is_file() or artifact.stat().st_size == 0:
                    hard.append(f"{target}_artifact_missing")
            elif artifact.exists():
                hard.append(f"{target}_artifact_unexpected")

        return sha, hard

    selected_sources = [
        source for source in sources if source.stem.lower() in selected_shas
    ]
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(4, args.workers))) as executor:
        futures = [executor.submit(materialize_one, source) for source in selected_sources]
        for future in concurrent.futures.as_completed(futures):
            sha, hard = future.result()
            if hard:
                hard_violations.append(
                    {
                        "stage": "materialization",
                        "source_sha256": sha,
                        "violations": sorted(set(hard)),
                    }
                )

    def count(field, value):
        return sum(row.get(field) == value for row in rows)

    reader_counts = Counter(row["reader_outcome"] for row in rows)
    summary = {
        "schema": SUMMARY_SCHEMA,
        "corpus": {
            "source_count": len(rows),
            "sha_list_digest": "sha256:" + digest,
            "reader_acceptance_schema": reader_payload.get("schema"),
            "reader_outcomes": dict(sorted(reader_counts.items())),
        },
        "totals": {
            "editor_admitted": count("open_state", "admitted"),
            "editor_not_admitted": count("open_state", "not_admitted"),
            "idml_available_with_declared_losses": count(
                "idml_state", "available_with_declared_losses"
            ),
            "idml_unavailable": count("idml_state", "unavailable"),
            "idml_not_verified": count("idml_state", "not_verified"),
            "odg_available_with_declared_losses": count(
                "odg_state", "available_with_declared_losses"
            ),
            "odg_unavailable": count("odg_state", "unavailable"),
            "odg_not_verified": count("odg_state", "not_verified"),
        },
        "consumer_sample": {
            "requested_count": args.materialize_count,
            "selected_count": len(selected_shas),
            "selection": "evenly_spaced_over_source_size_and_declared_unsupported_count",
            "source_sha256": sorted(selected_shas),
            "consumer_validation_complete": False,
        },
        "hard_violations": sorted(
            hard_violations,
            key=lambda item: (
                item.get("stage", ""),
                item["source_sha256"],
                item["violations"],
            ),
        ),
        "claims": {
            "universal_pub_editable_export": False,
            "native_pub_save": False,
            "exact_source_admission_required": True,
            "non_normal_reader_sources_must_not_advertise_editable_routes": True,
            "consumer_validation_required_for_materialized_outputs": True,
            "corpus_bytes_uploaded_as_evidence": False,
        },
    }

    args.output.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(summary, indent=2, sort_keys=True))

    if hard_violations:
        print(
            json.dumps(
                {
                    "deferred_hard_violation_count": len(hard_violations),
                    "verdict": "deferred_to_consumer_finalize",
                },
                indent=2,
                sort_keys=True,
            ),
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
