#!/usr/bin/env python3
import argparse
import concurrent.futures
import hashlib
import json
import pathlib
import subprocess


SCHEMA = "chaptera.migration-editable-route-corpus.v2"


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def select_evenly(paths, count):
    if count <= 0 or count >= len(paths):
        return list(paths)
    if count == 1:
        return [paths[0]]
    picked = []
    used = set()
    for i in range(count):
        idx = round(i * (len(paths) - 1) / (count - 1))
        if idx not in used:
            picked.append(paths[idx])
            used.add(idx)
    if len(picked) < count:
        for idx, path in enumerate(paths):
            if idx not in used:
                picked.append(path)
                used.add(idx)
                if len(picked) == count:
                    break
    return picked


def run_one(probe, path, output_root, materialize, timeout_seconds):
    before = sha256_file(path)
    item_dir = output_root / before
    item_dir.mkdir(parents=True, exist_ok=True)
    cmd = [str(probe), str(path), "--label", path.name]
    if materialize:
        cmd += ["--materialize-dir", str(item_dir)]
    try:
        completed = subprocess.run(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "source_path": path.name,
            "source_sha256": before,
            "hard_violation": True,
            "failure": "probe_timeout",
            "requested_materialization": materialize,
        }

    after = sha256_file(path)
    if before != after:
        return {
            "source_path": path.name,
            "source_sha256": before,
            "hard_violation": True,
            "failure": "source_mutated",
            "requested_materialization": materialize,
        }
    if completed.returncode != 0:
        return {
            "source_path": path.name,
            "source_sha256": before,
            "hard_violation": True,
            "failure": "probe_failed",
            "returncode": completed.returncode,
            "stderr_tail": completed.stderr[-2000:],
            "requested_materialization": materialize,
        }
    try:
        receipt = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return {
            "source_path": path.name,
            "source_sha256": before,
            "hard_violation": True,
            "failure": "probe_receipt_invalid",
            "detail": str(error),
            "stdout_tail": completed.stdout[-2000:],
            "requested_materialization": materialize,
        }

    if receipt.get("source_sha256") != before:
        return {
            "source_path": path.name,
            "source_sha256": before,
            "hard_violation": True,
            "failure": "probe_source_identity_mismatch",
            "receipt_source_sha256": receipt.get("source_sha256"),
            "requested_materialization": materialize,
        }

    receipt["source_path"] = path.name
    receipt["hard_violation"] = False
    receipt["requested_materialization"] = materialize

    if materialize and receipt.get("open_state") == "admitted":
        failures = []
        for target in ("idml", "odg"):
            target_row = receipt["targets"][target]
            if target_row.get("state") != "available_with_declared_losses":
                continue
            artifact = item_dir / f"output.{target}"
            materialization_state = target_row.get("materialization_state")
            if materialization_state != "succeeded":
                failures.append({
                    "target": target,
                    "failure": "preview_materialization_disagreement",
                    "materialization_state": materialization_state,
                    "error_class": target_row.get("materialization_error_class"),
                })
                continue
            if not artifact.is_file() or artifact.stat().st_size == 0:
                failures.append({
                    "target": target,
                    "failure": "materialization_artifact_missing",
                    "materialization_state": materialization_state,
                    "error_class": None,
                })
        if failures:
            receipt["hard_violation"] = True
            receipt["failure"] = "preview_materialization_disagreement"
            receipt["materialization_failures"] = failures

    (item_dir / "route.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return receipt


def run_many(probe, paths, output_root, materialize, workers, timeout_seconds):
    rows = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        futures = {
            pool.submit(
                run_one,
                probe,
                path,
                output_root,
                materialize,
                timeout_seconds,
            ): path
            for path in paths
        }
        for future in concurrent.futures.as_completed(futures):
            row = future.result()
            rows.append(row)
            print(
                json.dumps(
                    {
                        "phase": "materialize" if materialize else "admission",
                        "source": row.get("source_path"),
                        "sha256": row.get("source_sha256"),
                        "open_state": row.get("open_state"),
                        "idml": (row.get("targets") or {}).get("idml", {}).get("state"),
                        "odg": (row.get("targets") or {}).get("odg", {}).get("state"),
                        "hard_violation": row.get("hard_violation"),
                        "failure": row.get("failure"),
                        "materialization_failures": row.get("materialization_failures"),
                    },
                    sort_keys=True,
                ),
                flush=True,
            )
    return rows


def state_counts(rows, target):
    states = {
        "available_with_declared_losses": 0,
        "unavailable": 0,
        "not_verified": 0,
    }
    for row in rows:
        state = (row.get("targets") or {}).get(target, {}).get("state")
        if state in states:
            states[state] += 1
    return states


def materializable(row):
    if row.get("hard_violation") or row.get("open_state") != "admitted":
        return False
    targets = row.get("targets") or {}
    return any(
        (targets.get(target) or {}).get("state") == "available_with_declared_losses"
        for target in ("idml", "odg")
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-dir", required=True, type=pathlib.Path)
    parser.add_argument("--probe", required=True, type=pathlib.Path)
    parser.add_argument("--out", required=True, type=pathlib.Path)
    parser.add_argument("--sample-count", type=int, default=0)
    parser.add_argument("--materialize-count", type=int, default=0)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--timeout-seconds", type=int, default=30)
    args = parser.parse_args()

    paths = sorted(args.corpus_dir.glob("*.pub"), key=lambda p: p.name.casefold())
    if not paths:
        raise SystemExit("no .pub files found")

    selected = select_evenly(paths, args.sample_count)
    args.out.mkdir(parents=True, exist_ok=True)
    per_file = args.out / "per-file"

    # Phase 1 is admission-only for every selected exact source.
    rows = run_many(
        args.probe,
        selected,
        per_file,
        False,
        args.workers,
        args.timeout_seconds,
    )
    rows_by_sha = {row.get("source_sha256"): row for row in rows}

    # Phase 2 chooses from files already proven materializable, so a request
    # for N consumer artifacts is not wasted on unsupported source profiles.
    eligible_rows = sorted(
        [row for row in rows if materializable(row)],
        key=lambda row: row.get("source_sha256", ""),
    )
    chosen_rows = select_evenly(
        eligible_rows,
        min(args.materialize_count, len(eligible_rows)),
    )
    chosen_paths = [
        args.corpus_dir / row["source_path"]
        for row in chosen_rows
    ]
    if chosen_paths:
        materialized_rows = run_many(
            args.probe,
            chosen_paths,
            per_file,
            True,
            args.workers,
            args.timeout_seconds,
        )
        for row in materialized_rows:
            rows_by_sha[row.get("source_sha256")] = row

    rows = sorted(rows_by_sha.values(), key=lambda row: row.get("source_sha256", ""))
    hard = [row for row in rows if row.get("hard_violation")]
    admitted = [row for row in rows if row.get("open_state") == "admitted"]
    not_admitted = [row for row in rows if row.get("open_state") == "not_admitted"]

    source_digest = hashlib.sha256(
        "".join(row.get("source_sha256", "") + "\n" for row in rows).encode("ascii")
    ).hexdigest()

    summary = {
        "schema": SCHEMA,
        "corpus_file_count": len(paths),
        "selected_file_count": len(rows),
        "selection": {
            "algorithm": "evenly_spaced_over_casefolded_filename_order_v1",
            "requested_sample_count": args.sample_count,
            "source_sha_list_digest": "sha256:" + source_digest,
        },
        "materialization": {
            "requested_count": args.materialize_count,
            "eligible_count": len(eligible_rows),
            "selected_for_materialization_count": len(chosen_rows),
            "idml_materialized_count": sum(
                1
                for row in rows
                if (row.get("targets") or {}).get("idml", {}).get("materialized") is True
            ),
            "odg_materialized_count": sum(
                1
                for row in rows
                if (row.get("targets") or {}).get("odg", {}).get("materialized") is True
            ),
        },
        "open_state_counts": {
            "admitted": len(admitted),
            "not_admitted": len(not_admitted),
            "other": len(rows) - len(admitted) - len(not_admitted) - len(hard),
        },
        "idml_state_counts": state_counts(rows, "idml"),
        "odg_state_counts": state_counts(rows, "odg"),
        "hard_violation_count": len(hard),
        "hard_violations": [
            {
                "source_sha256": row.get("source_sha256"),
                "source_path": row.get("source_path"),
                "failure": row.get("failure"),
                "stderr_tail": row.get("stderr_tail"),
                "materialization_failures": row.get("materialization_failures"),
            }
            for row in hard
        ],
        "claims": {
            "universal_pub_editable_export": False,
            "native_pub_save": False,
            "exact_source_admission_required": True,
            "availability_is_not_consumer_validation": True,
        },
        "rows": rows,
    }
    (args.out / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({k: v for k, v in summary.items() if k != "rows"}, indent=2, sort_keys=True))

    if hard:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
