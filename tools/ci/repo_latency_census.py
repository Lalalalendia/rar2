#!/usr/bin/env python3
"""Reproducible GitHub Actions latency census across all rar2 components.

This reports wall-clock feedback/queue latency, not GitHub billing or CPU time.
Exact-head workflow run creation is a proxy for dispatch, not source edit time.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import statistics
import sys
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


SCHEMA = "rar2.repo-ci-latency.v1"
ADMIN_WORKFLOWS = {
    "Cancel obsolete PR head runs",
    "PR merge authority",
    "Rar public boundary guard",
    "PR workflow concurrency guard",
    "Workflow YAML syntax guard",
    "CHAPTERA-CI-TRUST-01 release workflow trust guard",
}


def timestamp(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return result if result.tzinfo is not None else None
    except ValueError:
        return None


def seconds(start: str | None, end: str | None) -> int | None:
    left, right = timestamp(start), timestamp(end)
    if left is None or right is None or right < left:
        return None
    return int((right - left).total_seconds())


def distribution(values: list[int | None]) -> dict:
    valid = sorted(value for value in values if value is not None)
    if not valid:
        return {"n": 0, "p50_s": None, "p95_s": None}
    return {
        "n": len(valid),
        "p50_s": statistics.median(valid),
        "p95_s": valid[math.ceil(0.95 * len(valid)) - 1],
    }


def domains(paths: list[str]) -> list[str]:
    result: set[str] = set()
    for path in paths:
        low = path.lower()
        if "android" in low or "/jni/" in low:
            result.add("android")
        elif low.startswith(("apps/cloud-reader/", "tools/cloud_reader_")):
            result.add("cloud-reader")
        elif low.startswith(("apps/web/", "packages/web/", "packages/protocol/editor-render-scene/", "tools/run_web_")) or "web-" in low:
            result.add("web")
        elif low.startswith(("apps/chaptera-server/", "crates/chaptera-cloud-")):
            result.add("cloud-server")
        elif low.startswith(("apps/chaptera-desktop/", "crates/chaptera-desktop-")):
            result.add("desktop")
        elif (
            "/pub-reader" in low or "/pub-viewer" in low
            or low.startswith(("apps/chaptera-reader/", "crates/chaptera-reader-"))
        ):
            result.add("reader")
        elif (
            "/pub-editor" in low or "/pub-quill" in low
            or low.startswith("crates/chaptera-editor-")
        ):
            result.add("editor")
        elif low.startswith("tools/perf/"):
            result.add("performance-infra")
        elif low.startswith(("tools/research-", "fixtures/", "research/", "tools/windows/run_paragraph_", ".github/workflows/pub-re-", ".github/workflows/publisher-direct-research-")):
            result.add("research")
        elif low.startswith((".github/", "tools/ci/", "tools/dev_fast_loop")):
            result.add("ci-infra")
        else:
            result.add("shared-or-other")
    return sorted(result) or ["unknown"]


def primary_class(groups: list[str]) -> str:
    substantive = [name for name in groups if name not in ("ci-infra", "shared-or-other")]
    if len(substantive) > 1:
        return "mixed"
    if len(substantive) == 1:
        return substantive[0]
    return "ci-infra" if "ci-infra" in groups else groups[0]


def analyze_pr(pr: dict, runs: list[dict], jobs_by_run: dict, paths: list[str]) -> dict:
    head = pr["head"]["sha"]
    merged = pr.get("merged_at")
    applicable = []
    for run in runs:
        if run.get("head_sha") != head or run.get("event") != "pull_request":
            continue
        if merged and timestamp(run.get("created_at")) and timestamp(merged):
            if timestamp(run["created_at"]) > timestamp(merged):
                continue
        applicable.append(run)

    start = min(
        (r["created_at"] for r in applicable if timestamp(r.get("created_at"))),
        default=None,
    )
    jobs = []
    for run in applicable:
        for job in jobs_by_run.get(str(run["id"]), []):
            jobs.append({
                "workflow": run.get("name", ""),
                "run_id": run["id"],
                "job_id": job["id"],
                "job": job.get("name", ""),
                "conclusion": job.get("conclusion"),
                "status": job.get("status"),
                "created_at": job.get("created_at"),
                "started_at": job.get("started_at"),
                "completed_at": job.get("completed_at"),
                "runner_labels": job.get("labels", []),
                "queue_s": seconds(job.get("created_at"), job.get("started_at")),
                "execution_s": seconds(job.get("started_at"), job.get("completed_at")),
            })

    gates = [
        j for j in jobs
        if j["workflow"] == "PR merge authority"
        and j["job"] == "required-ci"
        and j["conclusion"] == "success"
        and timestamp(j["completed_at"])
    ]
    gate = max(gates, key=lambda j: j["completed_at"], default=None)
    relevant = [
        j for j in jobs
        if j["workflow"] not in ADMIN_WORKFLOWS
        and not j["job"].lower().startswith("classify ")
        and j["conclusion"] in ("success", "failure")
        and timestamp(j["completed_at"])
    ]
    first = min(relevant, key=lambda j: j["completed_at"], default=None)
    failures = [j for j in jobs if j["conclusion"] == "failure" and timestamp(j["completed_at"])]
    first_failure = min(failures, key=lambda j: j["completed_at"], default=None)
    # GitHub can stamp a skipped/never-started job with equal created/started.
    # Those are not observed runner dispatches and must not dilute queue p50/p95.
    queued = [
        j for j in jobs
        if j["status"] == "completed"
        and j["conclusion"] not in (None, "skipped")
        and j["queue_s"] is not None
    ]
    longest = max(queued, key=lambda j: j["queue_s"], default=None)
    eligible_tail = [
        j for j in relevant
        if gate and j["completed_at"] <= gate["completed_at"]
    ]
    tail = max(eligible_tail, key=lambda j: j["completed_at"], default=None)
    groups = domains(paths)
    return {
        "pr_number": pr["number"],
        "title": pr.get("title"),
        "head_sha": head,
        "domains": groups,
        "class": primary_class(groups),
        "changed_file_count": len(paths),
        "workflow_count": len(applicable),
        "job_count": len(jobs),
        "first_dispatch_at": start,
        "first_relevant_feedback_s": seconds(start, first["completed_at"]) if first else None,
        "first_failure_s": seconds(start, first_failure["completed_at"]) if first_failure else None,
        "merge_ready_s": seconds(start, gate["completed_at"]) if gate else None,
        "pr_created_to_merged_s": seconds(pr.get("created_at"), merged),
        "post_gate_to_merge_s": seconds(gate["completed_at"], merged) if gate else None,
        "runner_active_s": sum(j["execution_s"] or 0 for j in jobs if j["conclusion"] != "skipped"),
        "queue_wait": distribution([j["queue_s"] for j in queued]),
        "longest_queue": (
            {"workflow": longest["workflow"], "job": longest["job"],
             "job_id": longest["job_id"], "queue_s": longest["queue_s"]}
            if longest else None
        ),
        "critical_tail": (
            {"workflow": tail["workflow"], "job": tail["job"],
             "job_id": tail["job_id"], "completed_at": tail["completed_at"],
             "gap_to_gate_s": seconds(tail["completed_at"], gate["completed_at"])}
            if tail and gate else None
        ),
        "gate": (
            {"job_id": gate["job_id"], "completed_at": gate["completed_at"]}
            if gate else None
        ),
        "queue_outliers": [
            {"workflow": j["workflow"], "job": j["job"], "job_id": j["job_id"],
             "created_at": j["created_at"], "started_at": j["started_at"],
             "queue_s": j["queue_s"], "execution_s": j["execution_s"]}
            for j in sorted(queued, key=lambda item: item["queue_s"], reverse=True)[:5]
        ],
    }


def summarize(rows: list[dict]) -> dict:
    result = {}
    for domain in sorted({r["class"] for r in rows}):
        selected = [r for r in rows if r["class"] == domain]
        result[domain] = {
            "pr_count": len(selected),
            "first_feedback": distribution([r["first_relevant_feedback_s"] for r in selected]),
            "merge_ready": distribution([r["merge_ready_s"] for r in selected]),
            "lifetime": distribution([r["pr_created_to_merged_s"] for r in selected]),
        }
    return result


class GitHub:
    def __init__(self, repository: str, token: str):
        self.repository = repository
        self.token = token

    def get(self, path: str) -> dict | list:
        url = "https://api.github.com/repos/" + self.repository + "/" + path
        headers = {
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "rar2-repo-latency-census",
        }
        if self.token:
            headers["Authorization"] = "Bearer " + self.token
        req = Request(url, headers=headers)
        with urlopen(req, timeout=35) as response:
            return json.load(response)

    def paged(self, path: str, key: str | None = None, max_pages: int = 8) -> list:
        collected = []
        for page in range(1, max_pages + 1):
            connector = "&" if "?" in path else "?"
            payload = self.get(path + connector + f"per_page=100&page={page}")
            values = payload[key] if key else payload
            collected.extend(values)
            if len(values) < 100:
                return collected
        raise RuntimeError(f"pagination exceeded {max_pages} pages: {path}")


def census(client: GitHub, limit: int) -> dict:
    merged = []
    for page in range(1, 9):
        items = client.get(
            f"pulls?state=closed&sort=updated&direction=desc&per_page=100&page={page}"
        )
        merged.extend(pr for pr in items if pr.get("merged_at"))
        if len(merged) >= limit:
            break
    if len(merged) < limit:
        raise RuntimeError(f"only found {len(merged)} merged PRs; requested {limit}")

    rows = []
    for pr in merged[:limit]:
        number = pr["number"]
        paths = [
            f["filename"]
            for f in client.paged(f"pulls/{number}/files", max_pages=8)
        ]
        sha = quote(pr["head"]["sha"], safe="")
        runs = client.paged(
            f"actions/runs?head_sha={sha}&event=pull_request",
            key="workflow_runs",
            max_pages=8,
        )
        applicable = [
            run for run in runs
            if run.get("head_sha") == pr["head"]["sha"]
            and run.get("event") == "pull_request"
            and (
                not pr.get("merged_at")
                or not timestamp(run.get("created_at"))
                or timestamp(run["created_at"]) <= timestamp(pr["merged_at"])
            )
        ]
        jobs = {
            str(run["id"]): client.paged(
                f"actions/runs/{run['id']}/jobs", key="jobs", max_pages=8
            )
            for run in applicable
        }
        rows.append(analyze_pr(pr, applicable, jobs, paths))
    return {
        "schema": SCHEMA,
        "repository": client.repository,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "sample_count": len(rows),
        "by_class": summarize(rows),
        "prs": rows,
        "caveats": [
            "Runs are selected by exact PR head SHA and pull_request event before merge.",
            "First run creation is a dispatch proxy, not the actual edit/push timestamp.",
            "First relevant feedback excludes administrative workflows and classify jobs.",
            "Job created_to_started is scheduler/runner waiting after needs become satisfied.",
            "Merge-ready is required-ci SUCCESS; it can include polling, not CPU time.",
            "PR lifetime includes development, approval, restacks and human waiting.",
            "P95 uses nearest rank; small groups are descriptive only.",
            "Summed runner_active_s is occupancy, not billed GitHub Actions minutes.",
            "Latest job attempt metadata may omit earlier rerun attempts.",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="Lalalalendia/rar2")
    parser.add_argument("--limit", type=int, default=12, help="recent merged PRs (1..50)")
    parser.add_argument("--out", type=Path, help="write JSON receipt; default stdout")
    args = parser.parse_args()
    if not 1 <= args.limit <= 50:
        parser.error("--limit must be 1..50")
    if "/" not in args.repo or args.repo.count("/") != 1:
        parser.error("--repo must be owner/name")
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or ""
    try:
        receipt = census(GitHub(args.repo, token), args.limit)
    except (HTTPError, RuntimeError) as exc:
        print(f"ci latency census failed: {exc}", file=sys.stderr)
        return 1
    data = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(data, encoding="utf-8")
    else:
        print(data, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
