#!/usr/bin/env python3
"""Credential-independent controls for the Chaptera Work/Actions executor.

This module never calls an inference API, claims a task, or merges a PR.
Snapshots must come from fresh authenticated Notion/GitHub reads. Its decision
is a control result, not a substitute for those authorities or an AI driver.
"""
from __future__ import annotations

import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile

SCHEMA = "chaptera.autonomous-control.v1"
REPOSITORY = "Lalalalendia/rar2"
PROOFS = (
    "inputs_available", "no_external_prerequisites", "hosted_implementation_possible",
    "automated_verification", "dependencies_closed", "exclusive_owner",
    "protected_merge_permitted", "definition_of_done_verified",
)
PHASES = ("claimed", "developing", "tested", "pr_open", "ci_wait", "merged", "notion_synced")
SHA = re.compile(r"^[0-9a-f]{40}$")


class ControlError(ValueError):
    pass


def timestamp(value: str) -> datetime:
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except (TypeError, ValueError, AttributeError) as error:
        raise ControlError("missing or invalid observation time") from error
    if result.tzinfo is None:
        raise ControlError("observation time must include timezone")
    return result.astimezone(timezone.utc)


def fresh(value: str, now: datetime) -> bool:
    age = (now - timestamp(value)).total_seconds()
    return 0 <= age <= 300


def admission_reasons(task: dict, snapshot: dict, now: datetime, *, resume: bool = False) -> list[str]:
    reasons = []
    expected = "IN PROGRESS" if resume else "TODO"
    status = task.get("Status") if task.get("kind") == "implementation" else task.get("Lifecycle")
    if status != (expected if task.get("kind") == "implementation" else ("Running" if resume else "Ready")):
        reasons.append("task status is not executable")
    for field, value in [("Autonomous Gate", "Admitted"), ("No-PC Readiness", "Executable now")]:
        if task.get(field) != value:
            reasons.append(f"{field} is not {value}")
    if not resume and task.get("Dispatch State") != "Runnable now":
        reasons.append("dispatch is not Runnable now")
    if task.get("Runtime Requirement") != ["GitHub-hosted"]:
        reasons.append("hosted-only runtime is unconfirmed")
    if not isinstance(task.get("Acceptance"), str) or not task["Acceptance"].strip():
        reasons.append("Definition of Done is missing")
    if not str(task.get("url", "")).startswith("https://app.notion.com/"):
        reasons.append("canonical Notion card is missing")
    proofs = task.get("proofs", {})
    for name in PROOFS:
        proof = proofs.get(name, {})
        try:
            observed_fresh = fresh(proof.get("observed_at"), now)
        except ControlError:
            observed_fresh = False
        if (proof.get("value") is not True or not proof.get("evidence")
                or proof.get("main_sha") != snapshot.get("main_sha")
                or not observed_fresh):
            reasons.append(f"unconfirmed proof: {name}")
    if task.get("kind") == "research":
        for field in ("Plan Epoch", "Read Claims", "Task Plan Revision Snapshots", "Evidence Reuse"):
            if not task.get(field):
                reasons.append(f"missing research planning field: {field}")
        if task.get("planning_verified") is not True:
            reasons.append("research protocol gate is unconfirmed")
    elif task.get("kind") != "implementation" or task.get("planning_verified") is not True:
        reasons.append("implementation DAG/planning gate is unconfirmed")
    return reasons


def select_task(snapshot: dict, checkpoint: dict, owner: str, now: datetime) -> dict:
    if (snapshot.get("repository") != REPOSITORY or not SHA.fullmatch(snapshot.get("main_sha", ""))
            or not fresh(snapshot.get("observed_at"), now)):
        raise ControlError("fresh canonical repository snapshot is required")
    policy = snapshot.get("merge_policy", {})
    if (policy.get("verified") is not True or policy.get("required_approvals") != 0
            or policy.get("bypass") is not False or not policy.get("required_contexts")):
        raise ControlError("autonomous protected merge is unconfirmed")
    if snapshot.get("queue_capacity_clear") is not True:
        raise ControlError("Actions capacity is not proven clear")
    tasks = snapshot.get("tasks", [])
    active = checkpoint.get("active")
    if active and active.get("phase") != "notion_synced":
        if active.get("owner") != owner:
            raise ControlError("current task belongs to another executor")
        task = next((row for row in tasks if row.get("url") == active.get("task_url")), None)
        if task is None:
            raise ControlError("active task must be fetched directly before recovery")
        if task.get("Execution Owner") != owner:
            raise ControlError("canonical task owner changed; do not steal the lease")
        if active.get("phase") == "merged":
            # Source proof is already in GitHub; only Notion synchronization remains.
            merge = snapshot.get("merges", {}).get(active["task_url"], {})
            if merge.get("merged") is not True or merge.get("sha") != active.get("merge_sha"):
                raise ControlError("actual merge must be reverified before closure recovery")
            return {"action": "sync_notion", "task_url": task["url"]}
        reasons = admission_reasons(task, snapshot, now, resume=True)
        if reasons:
            raise ControlError("active task admission changed: " + "; ".join(reasons))
        head = active.get("candidate_sha")
        if head and snapshot.get("branch_heads", {}).get(active.get("branch")) != head:
            raise ControlError("candidate moved; re-read authoritative branch/PR before resuming")
        return {"action": "resume", "task_url": task["url"], "phase": active["phase"]}
    rejected = {}
    admitted = []
    for task in tasks:
        reasons = admission_reasons(task, snapshot, now)
        if reasons:
            rejected[task.get("url", "unknown")] = reasons
        elif task.get("Priority") in ("P0", "P1", "P2", "P3"):
            admitted.append(task)
    if not admitted:
        return {"action": "blocked", "rejected": rejected}
    task = sorted(admitted, key=lambda row: (row["Priority"], row["url"]))[0]
    return {"action": "select", "task_url": task["url"], "rejected": rejected}


def reserve_cost(checkpoint: dict, run_id: str, attempt_id: str, maximum_microusd: int, limits: dict, now: datetime) -> dict:
    """Reserve the whole upper bound; a crash never refunds unobserved spending."""
    if any(not isinstance(value, str) or not value.strip() for value in (run_id, attempt_id)):
        raise ControlError("execution-run and unique inference-attempt identities are required")
    if type(maximum_microusd) is not int or maximum_microusd <= 0:
        raise ControlError("a positive, verified API charge upper bound is required")
    for key in ("run", "day", "month"):
        if type(limits.get(key)) is not int or limits[key] <= 0:
            raise ControlError("API budgets are disabled or unconfirmed")
    updated = copy.deepcopy(checkpoint)
    reservations = updated.setdefault("reservations", {})
    if attempt_id in reservations:
        if (reservations[attempt_id]["maximum_microusd"] != maximum_microusd
                or reservations[attempt_id]["run_id"] != run_id):
            raise ControlError("a reservation cannot change after a retry")
        return updated
    day, month = now.strftime("%Y-%m-%d"), now.strftime("%Y-%m")
    run_total = sum(row["maximum_microusd"] for row in reservations.values() if row["run_id"] == run_id)
    day_total = sum(row["maximum_microusd"] for row in reservations.values() if row["day"] == day)
    month_total = sum(row["maximum_microusd"] for row in reservations.values() if row["month"] == month)
    if run_total + maximum_microusd > limits["run"] or day_total + maximum_microusd > limits["day"] or month_total + maximum_microusd > limits["month"]:
        raise ControlError("API reservation exceeds the run/day/month budget")
    reservations[attempt_id] = {"run_id": run_id, "maximum_microusd": maximum_microusd, "day": day, "month": month, "dispatch_started": False}
    return updated


def begin_api_attempt(checkpoint: dict, attempt_id: str) -> dict:
    """Publish this state durably before dispatch; never repeat an unknown charge."""
    updated = copy.deepcopy(checkpoint)
    reservation = updated.get("reservations", {}).get(attempt_id)
    if reservation is None or reservation.get("dispatch_started") is not False:
        raise ControlError("attempt is unreserved or already dispatched; retry needs a new reservation")
    reservation["dispatch_started"] = True
    return updated


def advance(checkpoint: dict, phase: str, *, proof: dict | None = None) -> dict:
    updated = copy.deepcopy(checkpoint)
    active = updated["active"]
    current = active["phase"]
    if current not in PHASES or phase not in PHASES or PHASES.index(phase) != PHASES.index(current) + 1:
        raise ControlError("checkpoint transition must advance exactly one proven phase")
    if phase == "merged":
        proof = proof or {}
        if (proof.get("github_merged") is not True or not SHA.fullmatch(proof.get("merge_sha", ""))
                or not SHA.fullmatch(active.get("candidate_sha", ""))
                or proof.get("checked_sha") != active.get("candidate_sha")
                or proof.get("required_ci_success") is not True):
            raise ControlError("actual merge and exact-head required CI proof are required")
        checks = proof.get("checks", {})
        contexts = active.get("required_contexts")
        if not isinstance(contexts, list) or not contexts or any(not isinstance(name, str) or not name for name in contexts):
            raise ControlError("the observed required contexts must be recorded explicitly")
        for context in contexts:
            check = checks.get(context, {})
            if check.get("sha") != active["candidate_sha"] or check.get("conclusion") != "success":
                raise ControlError(f"required check {context} lacks exact-head success")
        active["merge_sha"] = proof["merge_sha"]
    if phase == "notion_synced" and (proof or {}).get("notion_verified") is not True:
        raise ControlError("canonical card update must be re-read before closure")
    active["phase"] = phase
    updated["generation"] = updated.get("generation", 0) + 1
    return updated


def digest(payload: dict) -> str:
    return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def save_checkpoint(path: Path, payload: dict) -> None:
    if path.is_symlink():
        raise ControlError("checkpoint symlink is forbidden")
    record = {"schema": SCHEMA, "payload": payload, "sha256": digest(payload)}
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(record, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def load_checkpoint(path: Path) -> dict:
    try:
        record = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise ControlError("checkpoint is unavailable/corrupt; never reset the cost ledger silently") from error
    if record.get("schema") != SCHEMA or digest(record.get("payload")) != record.get("sha256"):
        raise ControlError("checkpoint integrity failed")
    return record["payload"]


def capability_preflight(environment: dict) -> dict:
    required = ("GH_TOKEN", "CHAPTERA_NOTION_TOKEN", "CHAPTERA_AGENT_API_KEY", "CHAPTERA_AGENT_VERIFIED_BRIDGE")
    missing = [key for key in required if not environment.get(key)]
    # Presence is not verification of model pricing, scopes, or a usable bridge.
    return {"schema": SCHEMA, "status": "blocked", "ai_executed": False,
            "missing_capabilities": missing,
            "reason": "live inference/Notion bridge, persistent reservation publication and protected task-cycle proof remain required"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    save_checkpoint(args.output, capability_preflight(dict(os.environ)))
    print("Chaptera Actions AI execution is blocked; control checkpoint written, no API call made.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
