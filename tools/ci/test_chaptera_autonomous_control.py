from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile
import unittest

from chaptera_autonomous_control import (
    ControlError, PROOFS, advance, begin_api_attempt, capability_preflight, load_checkpoint,
    reserve_cost, save_checkpoint, select_task,
)

NOW = datetime(2026, 10, 11, 21, 0, tzinfo=timezone.utc)
HEAD = "a" * 40
URL = "https://app.notion.com/p/synthetic-test-card"


def task():
    return {
        "url": URL, "kind": "implementation", "Status": "TODO", "Priority": "P1",
        "Dispatch State": "Runnable now", "Autonomous Gate": "Admitted",
        "No-PC Readiness": "Executable now", "Runtime Requirement": ["GitHub-hosted"],
        "Acceptance": "Synthetic test contract", "planning_verified": True,
        "proofs": {name: {"value": True, "evidence": "synthetic test evidence",
                            "main_sha": HEAD, "observed_at": NOW.isoformat()} for name in PROOFS},
    }


def snapshot(*tasks):
    return {"repository": "Lalalalendia/rar2", "main_sha": HEAD, "observed_at": NOW.isoformat(),
            "queue_capacity_clear": True, "tasks": list(tasks), "branch_heads": {"task-branch": HEAD},
            "merge_policy": {"verified": True, "required_approvals": 0, "bypass": False,
                             "required_contexts": ["required-ci", "frozen-repo-paths"]}}


class ControlTests(unittest.TestCase):
    def test_selects_highest_admitted_priority_and_skips_blocked_task(self):
        lower, blocked = task(), task()
        lower["Priority"] = "P2"
        blocked["Priority"] = "P0"
        blocked["proofs"]["dependencies_closed"]["value"] = False
        result = select_task(snapshot(lower, blocked), {}, "executor", NOW)
        self.assertEqual(result["action"], "select")
        self.assertTrue(result["rejected"])

    def test_each_missing_proof_is_fail_closed(self):
        for name in PROOFS:
            with self.subTest(name=name):
                row = task()
                del row["proofs"][name]
                self.assertEqual(select_task(snapshot(row), {}, "executor", NOW)["action"], "blocked")

    def test_stale_or_future_snapshots_review_requirements_and_capacity_reject(self):
        for value in ["2026-10-11T20:54:00Z", "2026-10-11T21:00:01Z"]:
            data = snapshot(task())
            data["observed_at"] = value
            with self.assertRaises(ControlError):
                select_task(data, {}, "executor", NOW)
        for key, value in [("required_approvals", 1), ("bypass", True)]:
            data = snapshot(task())
            data["merge_policy"][key] = value
            with self.assertRaises(ControlError):
                select_task(data, {}, "executor", NOW)
        data = snapshot(task())
        data["queue_capacity_clear"] = False
        with self.assertRaises(ControlError):
            select_task(data, {}, "executor", NOW)

    def test_research_never_backfills_missing_planning_fields(self):
        row = task()
        row.update(kind="research", Lifecycle="Ready")
        self.assertEqual(select_task(snapshot(row), {}, "executor", NOW)["action"], "blocked")
        for name in ["Plan Epoch", "Read Claims", "Task Plan Revision Snapshots", "Evidence Reuse"]:
            row[name] = "synthetic protocol field"
        self.assertEqual(select_task(snapshot(row), {}, "executor", NOW)["action"], "select")

    def test_resume_requires_same_owner_and_exact_branch_head(self):
        row = task()
        row["Status"] = "IN PROGRESS"
        row["Execution Owner"] = "executor"
        state = {"active": {"owner": "executor", "task_url": URL, "phase": "ci_wait",
                            "branch": "task-branch", "candidate_sha": HEAD}}
        self.assertEqual(select_task(snapshot(row), state, "executor", NOW)["action"], "resume")
        with self.assertRaises(ControlError):
            select_task(snapshot(row), state, "second-executor", NOW)
        data = snapshot(row)
        data["branch_heads"]["task-branch"] = "b" * 40
        with self.assertRaises(ControlError):
            select_task(data, state, "executor", NOW)

    def test_merge_and_notion_closure_need_actual_proof(self):
        state = {"active": {"phase": "ci_wait", "candidate_sha": HEAD,
                            "required_contexts": ["required-ci", "frozen-repo-paths"]}}
        with self.assertRaises(ControlError):
            advance(state, "merged")
        merged = advance(state, "merged", proof={"github_merged": True, "merge_sha": "b" * 40,
                                                   "checked_sha": HEAD, "required_ci_success": True,
                                                   "checks": {name: {"sha": HEAD, "conclusion": "success"}
                                                              for name in ["required-ci", "frozen-repo-paths"]}})
        with self.assertRaises(ControlError):
            advance(merged, "notion_synced")
        self.assertEqual(advance(merged, "notion_synced", proof={"notion_verified": True})["generation"], 2)

    def test_crash_reservation_is_idempotent_and_never_refunded(self):
        limits = {"run": 100, "day": 150, "month": 200}
        state = reserve_cost({}, "run-1", "attempt-1", 100, limits, NOW)
        self.assertEqual(reserve_cost(state, "run-1", "attempt-1", 100, limits, NOW), state)
        with self.assertRaises(ControlError):
            reserve_cost(state, "run-2", "attempt-2", 51, limits, NOW)
        with self.assertRaises(ControlError):
            reserve_cost(state, "run-1", "attempt-1", 90, limits, NOW)
        with self.assertRaises(ControlError):
            reserve_cost({}, "run-1", "attempt-1", 100, {"run": 0, "day": 150, "month": 200}, NOW)

    def test_api_retry_cannot_reuse_an_attempt_with_unknown_charge(self):
        limits = {"run": 100, "day": 150, "month": 200}
        state = reserve_cost({}, "run-1", "attempt-1", 100, limits, NOW)
        dispatched = begin_api_attempt(state, "attempt-1")
        recovered = reserve_cost(dispatched, "run-1", "attempt-1", 100, limits, NOW)
        with self.assertRaises(ControlError):
            begin_api_attempt(recovered, "attempt-1")
        with self.assertRaises(ControlError):
            begin_api_attempt({}, "unreserved")

    def test_run_budget_covers_all_attempts_and_reservations_cannot_move_runs(self):
        limits = {"run": 100, "day": 500, "month": 500}
        state = reserve_cost({}, "run-1", "attempt-1", 60, limits, NOW)
        with self.assertRaises(ControlError):
            reserve_cost(state, "run-1", "attempt-2", 41, limits, NOW)
        with self.assertRaises(ControlError):
            reserve_cost(state, "run-2", "attempt-1", 60, limits, NOW)
        self.assertIn("attempt-2", reserve_cost(state, "run-2", "attempt-2", 41, limits, NOW)["reservations"])

    def test_merge_cannot_invent_missing_required_contexts(self):
        proof = {"github_merged": True, "merge_sha": "b" * 40,
                 "checked_sha": HEAD, "required_ci_success": True}
        for contexts in (None, [], [""]):
            with self.subTest(contexts=contexts), self.assertRaises(ControlError):
                advance({"active": {"phase": "ci_wait", "candidate_sha": HEAD,
                                    "required_contexts": contexts}}, "merged", proof=proof)

    def test_one_task_with_missing_observation_does_not_hide_another_admitted_task(self):
        malformed, eligible = task(), task()
        malformed["url"] += "-unconfirmed"
        del malformed["proofs"]["inputs_available"]["observed_at"]
        self.assertEqual(select_task(snapshot(malformed, eligible), {}, "executor", NOW)["task_url"], URL)

    def test_checkpoint_roundtrip_and_corruption_cannot_reset_cost_state(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            state = {"generation": 3, "reservations": {"prior-run": {"maximum_microusd": 100}}}
            save_checkpoint(path, state)
            self.assertEqual(load_checkpoint(path), state)
            record = json.loads(path.read_text())
            record["payload"]["reservations"] = {}
            path.write_text(json.dumps(record))
            with self.assertRaises(ControlError):
                load_checkpoint(path)
            with self.assertRaises(ControlError):
                load_checkpoint(path.with_name("missing.json"))

    def test_missing_credentials_never_executes_ai_even_if_control_contract_is_green(self):
        result = capability_preflight({})
        self.assertEqual(result["status"], "blocked")
        self.assertFalse(result["ai_executed"])
        self.assertIn("CHAPTERA_AGENT_API_KEY", result["missing_capabilities"])
        configured = capability_preflight({name: "synthetic-present" for name in result["missing_capabilities"]})
        self.assertEqual(configured["status"], "blocked")
        self.assertFalse(configured["ai_executed"])


if __name__ == "__main__":
    unittest.main()
