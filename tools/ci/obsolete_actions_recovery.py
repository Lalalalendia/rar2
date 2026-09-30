#!/usr/bin/env python3
"""Drain obsolete GitHub Actions PR runs with a narrowly fenced force-cancel fallback."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

API_VERSION = "2022-11-28"
NOT_QUEUED_MESSAGE = "Cannot cancel a workflow run that has not been queued yet."
MIN_STALE_AGE_SECONDS = 15 * 60


def parse_github_time(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


@dataclass(frozen=True)
class ForceCancelContext:
    normal_cancel_status: int
    normal_cancel_message: str
    event: str
    status: str
    head_sha: str
    created_at: datetime
    total_jobs: int
    open_heads: frozenset[str]
    now: datetime


def force_cancel_denials(context: ForceCancelContext) -> tuple[str, ...]:
    denials: list[str] = []
    if context.normal_cancel_status != 409:
        denials.append("normal_cancel_not_http409")
    if context.normal_cancel_message != NOT_QUEUED_MESSAGE:
        denials.append("normal_cancel_message_mismatch")
    if context.event != "pull_request":
        denials.append("not_pull_request")
    if context.status != "queued":
        denials.append("not_queued")
    if not context.head_sha:
        denials.append("missing_head_sha")
    elif context.head_sha in context.open_heads:
        denials.append("current_open_pr_head")
    if context.total_jobs != 0:
        denials.append("jobs_present")
    age_seconds = (context.now - context.created_at).total_seconds()
    if age_seconds < MIN_STALE_AGE_SECONDS:
        denials.append("younger_than_15_minutes")
    return tuple(denials)


def force_cancel_eligible(context: ForceCancelContext) -> bool:
    return not force_cancel_denials(context)


class GitHubApi:
    def __init__(self, repo: str, token: str) -> None:
        self.repo = repo
        self.token = token
        self.root = f"https://api.github.com/repos/{repo}"

    def request(
        self,
        path: str,
        *,
        method: str = "GET",
        query: dict[str, str | int] | None = None,
    ) -> tuple[int, Any]:
        url = f"{self.root}/{path.lstrip('/')}"
        if query:
            url = f"{url}?{urlencode(query)}"
        data = b"" if method != "GET" else None
        request = Request(
            url,
            data=data,
            method=method,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {self.token}",
                "X-GitHub-Api-Version": API_VERSION,
                "User-Agent": "chaptera-actions-recovery",
            },
        )
        try:
            with urlopen(request, timeout=30) as response:
                body = response.read()
                return response.status, json.loads(body) if body else None
        except HTTPError as error:
            body = error.read()
            try:
                payload: Any = json.loads(body) if body else None
            except json.JSONDecodeError:
                payload = {"message": body.decode("utf-8", errors="replace")}
            return error.code, payload

    @staticmethod
    def require(status: int, expected: int, payload: Any, operation: str) -> Any:
        if status != expected:
            raise RuntimeError(
                f"{operation} failed: HTTP {status}: {json.dumps(payload, sort_keys=True)}"
            )
        return payload

    def open_heads(self) -> set[str]:
        heads: set[str] = set()
        page = 1
        while True:
            status, payload = self.request(
                "pulls",
                query={"state": "open", "per_page": 100, "page": page},
            )
            rows = self.require(status, 200, payload, "list open pulls")
            if not isinstance(rows, list):
                raise RuntimeError("list open pulls returned a non-list payload")
            heads.update(
                row.get("head", {}).get("sha", "")
                for row in rows
                if row.get("head", {}).get("sha")
            )
            if len(rows) < 100:
                return heads
            page += 1

    def pr_runs(self, status_name: str) -> list[dict[str, Any]]:
        runs: list[dict[str, Any]] = []
        page = 1
        while True:
            status, payload = self.request(
                "actions/runs",
                query={
                    "status": status_name,
                    "event": "pull_request",
                    "per_page": 100,
                    "page": page,
                },
            )
            data = self.require(status, 200, payload, f"list {status_name} runs")
            rows = data.get("workflow_runs", [])
            if not isinstance(rows, list):
                raise RuntimeError("workflow_runs is not a list")
            runs.extend(rows)
            if len(rows) < 100:
                return runs
            page += 1

    def run(self, run_id: int) -> dict[str, Any]:
        status, payload = self.request(f"actions/runs/{run_id}")
        return self.require(status, 200, payload, f"read run {run_id}")

    def job_count_all_attempts(self, run_id: int) -> int:
        status, payload = self.request(
            f"actions/runs/{run_id}/jobs",
            query={"filter": "all", "per_page": 100},
        )
        data = self.require(status, 200, payload, f"read jobs for run {run_id}")
        return int(data.get("total_count", 0))

    def cancel(self, run_id: int) -> tuple[int, Any]:
        return self.request(f"actions/runs/{run_id}/cancel", method="POST")

    def force_cancel(self, run_id: int) -> tuple[int, Any]:
        return self.request(f"actions/runs/{run_id}/force-cancel", method="POST")


def error_message(payload: Any) -> str:
    if isinstance(payload, dict):
        value = payload.get("message")
        if isinstance(value, str):
            return value
    return ""


def remaining_obsolete_runs(client: GitHubApi) -> list[dict[str, Any]]:
    open_heads = client.open_heads()
    remaining: list[dict[str, Any]] = []
    for status_name in ("queued", "in_progress"):
        for run in client.pr_runs(status_name):
            if run.get("head_sha") not in open_heads:
                remaining.append(
                    {
                        "id": run.get("id"),
                        "name": run.get("name"),
                        "status": run.get("status"),
                        "head_branch": run.get("head_branch"),
                        "head_sha": run.get("head_sha"),
                    }
                )
    return remaining


def drain(client: GitHubApi, *, now: datetime) -> dict[str, Any]:
    initial_open_heads = client.open_heads()
    candidates: list[dict[str, Any]] = []
    for status_name in ("queued", "in_progress"):
        candidates.extend(client.pr_runs(status_name))

    decisions: list[dict[str, Any]] = []
    for run in candidates:
        run_id = int(run["id"])
        head_sha = str(run.get("head_sha") or "")
        record: dict[str, Any] = {
            "id": run_id,
            "name": run.get("name"),
            "initial_status": run.get("status"),
            "head_branch": run.get("head_branch"),
            "head_sha": head_sha,
        }
        if head_sha in initial_open_heads:
            record["action"] = "keep_current_open_head"
            decisions.append(record)
            continue

        cancel_status, cancel_payload = client.cancel(run_id)
        cancel_message = error_message(cancel_payload)
        record["normal_cancel_http_status"] = cancel_status
        record["normal_cancel_message"] = cancel_message

        if cancel_status in (202, 204):
            record["action"] = "normal_cancel_accepted"
            decisions.append(record)
            continue

        if cancel_status != 409 or cancel_message != NOT_QUEUED_MESSAGE:
            record["action"] = "normal_cancel_failed_no_force"
            decisions.append(record)
            continue

        # The normal endpoint hit the exact provider pre-queue conflict. Re-read every
        # safety predicate immediately before considering the stronger endpoint.
        live_open_heads = client.open_heads()
        live_run = client.run(run_id)
        total_jobs = client.job_count_all_attempts(run_id)
        context = ForceCancelContext(
            normal_cancel_status=cancel_status,
            normal_cancel_message=cancel_message,
            event=str(live_run.get("event") or ""),
            status=str(live_run.get("status") or ""),
            head_sha=str(live_run.get("head_sha") or ""),
            created_at=parse_github_time(str(live_run["created_at"])),
            total_jobs=total_jobs,
            open_heads=frozenset(live_open_heads),
            now=now,
        )
        denials = force_cancel_denials(context)
        record["live_recheck"] = {
            "event": context.event,
            "status": context.status,
            "head_sha": context.head_sha,
            "total_jobs_all_attempts": context.total_jobs,
            "age_seconds": int((context.now - context.created_at).total_seconds()),
            "head_is_current_open_pr": context.head_sha in context.open_heads,
            "denials": list(denials),
        }
        if denials:
            record["action"] = "force_cancel_denied_by_guard"
            decisions.append(record)
            continue

        force_status, force_payload = client.force_cancel(run_id)
        record["force_cancel_http_status"] = force_status
        record["force_cancel_message"] = error_message(force_payload)
        record["action"] = (
            "force_cancel_accepted"
            if force_status in (202, 204)
            else "force_cancel_failed"
        )
        decisions.append(record)

    remaining: list[dict[str, Any]] = []
    for attempt in range(7):
        remaining = remaining_obsolete_runs(client)
        if not remaining:
            break
        if attempt < 6:
            time.sleep(2)

    return {
        "schema": "chaptera.ci-actions-prequeue-recovery.v1",
        "repository": client.repo,
        "minimum_force_cancel_age_seconds": MIN_STALE_AGE_SECONDS,
        "decisions": decisions,
        "remaining_obsolete_pull_request_runs": remaining,
        "queue_drained": not remaining,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()

    token = os.environ.get("GH_TOKEN", "")
    if not token:
        print("GH_TOKEN is required", file=sys.stderr)
        return 2

    client = GitHubApi(args.repo, token)
    receipt = drain(client, now=datetime.now(timezone.utc))
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, indent=2, sort_keys=True))
    if not receipt["queue_drained"]:
        print(
            "obsolete pull-request run records remain after bounded recovery",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
