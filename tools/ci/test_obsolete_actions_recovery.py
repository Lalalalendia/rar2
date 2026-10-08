#!/usr/bin/env python3
from __future__ import annotations

import unittest
from datetime import datetime, timedelta, timezone

from obsolete_actions_recovery import (
    NOT_QUEUED_MESSAGE,
    ForceCancelContext,
    ProviderOrphanContext,
    force_cancel_denials,
    force_cancel_eligible,
    provider_orphan_denials,
    provider_orphan_eligible,
)


class ForceCancelDecisionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.now = datetime(2026, 10, 1, tzinfo=timezone.utc)
        self.base = dict(
            normal_cancel_status=409,
            normal_cancel_message=NOT_QUEUED_MESSAGE,
            event="pull_request",
            status="queued",
            head_sha="obsolete-head",
            created_at=self.now - timedelta(minutes=16),
            total_jobs=0,
            open_heads=frozenset({"current-head"}),
            now=self.now,
        )

    def context(self, **changes: object) -> ForceCancelContext:
        values = dict(self.base)
        values.update(changes)
        return ForceCancelContext(**values)

    def test_exact_stale_zero_job_prequeue_conflict_is_eligible(self) -> None:
        self.assertTrue(force_cancel_eligible(self.context()))
        self.assertEqual(force_cancel_denials(self.context()), ())

    def test_normal_cancel_must_be_exact_http409(self) -> None:
        context = self.context(normal_cancel_status=500)
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("normal_cancel_not_http409", force_cancel_denials(context))

    def test_normal_cancel_message_must_match(self) -> None:
        context = self.context(normal_cancel_message="Conflict")
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("normal_cancel_message_mismatch", force_cancel_denials(context))

    def test_current_open_pr_head_is_never_force_cancelled(self) -> None:
        context = self.context(open_heads=frozenset({"obsolete-head"}))
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("current_open_pr_head", force_cancel_denials(context))

    def test_in_progress_run_is_not_prequeue_recovery(self) -> None:
        context = self.context(status="in_progress")
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("not_queued", force_cancel_denials(context))

    def test_any_job_history_denies_force_cancel(self) -> None:
        context = self.context(total_jobs=1)
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("jobs_present", force_cancel_denials(context))

    def test_recent_run_is_not_old_enough(self) -> None:
        context = self.context(created_at=self.now - timedelta(minutes=14))
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("younger_than_15_minutes", force_cancel_denials(context))

    def test_non_pr_run_is_never_force_cancelled(self) -> None:
        context = self.context(event="workflow_dispatch")
        self.assertFalse(force_cancel_eligible(context))
        self.assertIn("not_pull_request", force_cancel_denials(context))

class ProviderOrphanDecisionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.now = datetime(2026, 10, 1, tzinfo=timezone.utc)
        created = self.now - timedelta(hours=1)
        self.base = dict(
            force_cancel_status=409,
            force_cancel_message=NOT_QUEUED_MESSAGE,
            event="pull_request",
            status="queued",
            head_sha="obsolete-head",
            created_at=created,
            updated_at=created,
            total_jobs=0,
            pending_deployments=0,
            open_heads=frozenset({"current-head"}),
            now=self.now,
        )

    def context(self, **changes: object) -> ProviderOrphanContext:
        values = dict(self.base)
        values.update(changes)
        return ProviderOrphanContext(**values)

    def test_exact_provider_orphan_does_not_block_runner_capacity(self) -> None:
        self.assertTrue(provider_orphan_eligible(self.context()))
        self.assertEqual(provider_orphan_denials(self.context()), ())

    def test_jobs_keep_run_capacity_blocking(self) -> None:
        context = self.context(total_jobs=1)
        self.assertFalse(provider_orphan_eligible(context))
        self.assertIn("jobs_present", provider_orphan_denials(context))

    def test_pending_deployment_keeps_run_capacity_blocking(self) -> None:
        context = self.context(pending_deployments=1)
        self.assertFalse(provider_orphan_eligible(context))
        self.assertIn(
            "pending_deployments_present",
            provider_orphan_denials(context),
        )

    def test_state_change_keeps_run_capacity_blocking(self) -> None:
        context = self.context(updated_at=self.now - timedelta(minutes=30))
        self.assertFalse(provider_orphan_eligible(context))
        self.assertIn(
            "run_state_changed_since_creation",
            provider_orphan_denials(context),
        )

    def test_force_cancel_must_fail_with_exact_provider_conflict(self) -> None:
        context = self.context(force_cancel_status=500)
        self.assertFalse(provider_orphan_eligible(context))
        self.assertIn(
            "force_cancel_not_http409",
            provider_orphan_denials(context),
        )


if __name__ == "__main__":
    unittest.main()
