#!/usr/bin/env python3
import dataclasses
import json
import unittest

from k2c_reconcile import SurfaceState, plan_reconciliation


def state(**changes):
    value = SurfaceState(
        surface_id="reader/picture91-crop/v1",
        notion_page_id="3ea32a84beec8199bc98f9d34aa1c862",
        notion_edited_at="2026-09-29T10:00:00Z",
        github_cursor="issue#12@2026-09-29T10:00:00Z",
        semantic_fingerprint="semantic-v1",
        implementation_fingerprint="impl-v1",
        acceptance_fingerprint="accept-v1",
        owner_fingerprint="owner-v1",
        product_fingerprint="product-v1",
        classification="KNOWN + NOT CONSUMED",
        acceptance_closed=False,
        live_implementation_owner="rar2#12",
        code_anchor="vendor/producer-a/crates/pub-reader/src/resolve.rs::explicit_image_crop",
        code_anchor_exists=True,
        authority_link_ok=True,
        owner_check="unique",
        dependent_surface_ids=(),
    )
    return dataclasses.replace(value, **changes)


class K2CReconcileTests(unittest.TestCase):
    def test_k0_is_byte_stable_no_write(self):
        before = state()
        first = plan_reconciliation(before, before)
        second = plan_reconciliation(before, before)
        self.assertEqual(first.change_class, "K0")
        self.assertFalse(first.semantic_write)
        self.assertFalse(first.cursor_write)
        self.assertEqual(first.affected_surface_ids, ())
        self.assertEqual(first.json_text(), second.json_text())

    def test_k1_cursor_only_churn_has_no_semantic_write(self):
        before = state()
        current = state(
            notion_edited_at="2026-09-29T10:01:00Z",
            github_cursor="issue#12@2026-09-29T10:01:00Z",
        )
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K1")
        self.assertFalse(plan.semantic_write)
        self.assertTrue(plan.cursor_write)
        self.assertEqual(plan.affected_surface_ids, ())

    def test_k2_picture91_stale_locator_repair_is_local(self):
        before = state(
            code_anchor="vendor/producer-a/crates/pub-model/src/resolved_graph.rs::explicit_image_crop"
        )
        current = state(
            implementation_fingerprint="impl-v2",
            code_anchor="vendor/producer-a/crates/pub-reader/src/resolve.rs::explicit_image_crop",
        )
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K2")
        self.assertTrue(plan.semantic_write)
        self.assertEqual(plan.affected_surface_ids, ("reader/picture91-crop/v1",))

    def test_k3_legacy_acceptance_closes_to_consumed(self):
        before = state(
            surface_id="reader/legacy22-noquill/v1",
            notion_page_id="3e932a84beec81ba91c1c54d1538b280",
            code_anchor="vendor/producer-a/crates/pub-reader/src/legacy22_noquill_graph.rs::LEGACY_TEXT_SHAPE_TYPE",
            live_implementation_owner="rar2#17",
            owner_fingerprint="owner-issue-17",
            acceptance_fingerprint="accept-pending",
            classification="KNOWN + NOT CONSUMED",
            acceptance_closed=False,
        )
        current = dataclasses.replace(
            before,
            notion_edited_at="2026-09-29T12:19:23Z",
            github_cursor="main@97c2d610;run36565564426",
            acceptance_fingerprint="accept-run-36565564426-green",
            classification="KNOWN + CONSUMED",
            acceptance_closed=True,
            live_implementation_owner=None,
            owner_fingerprint="owner-none-landed",
        )
        # Owner closure is a K4 signal and takes precedence over acceptance.
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K4")
        self.assertTrue(plan.semantic_write)
        self.assertEqual(plan.errors, ())

        # Once owner/supersession has already been reconciled, the acceptance
        # transition itself is K3 and may promote the derived classification.
        owner_reconciled = dataclasses.replace(
            before,
            live_implementation_owner=None,
            owner_fingerprint="owner-none-landed",
        )
        accepted = dataclasses.replace(
            owner_reconciled,
            github_cursor="main@97c2d610;run36565564426",
            acceptance_fingerprint="accept-run-36565564426-green",
            classification="KNOWN + CONSUMED",
            acceptance_closed=True,
        )
        acceptance_plan = plan_reconciliation(owner_reconciled, accepted)
        self.assertEqual(acceptance_plan.change_class, "K3")
        self.assertTrue(acceptance_plan.semantic_write)

    def test_k4_owner_supersession_is_local(self):
        before = state(
            live_implementation_owner="rar2#10",
            owner_fingerprint="issue#10",
        )
        current = state(
            github_cursor="issue#10+pr#15",
            live_implementation_owner="rar2#10/pr#15",
            owner_fingerprint="issue#10+paired-pr#15",
        )
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K4")
        self.assertEqual(plan.affected_surface_ids, ("reader/picture91-crop/v1",))

    def test_verification_pr_cursor_does_not_create_second_owner(self):
        before = state(
            surface_id="reader/legacy22-noquill/v1",
            live_implementation_owner=None,
            owner_fingerprint="owner-none-landed",
            classification="KNOWN + CONSUMED",
            acceptance_closed=True,
        )
        current = dataclasses.replace(
            before,
            github_cursor="main@ec1a0dc9+verification-pr#53",
        )
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K1")
        self.assertFalse(plan.semantic_write)

    def test_k5_semantic_delta_fans_out_only_registered_dependents(self):
        before = state()
        current = state(
            semantic_fingerprint="semantic-v2",
            dependent_surface_ids=(
                "reader/closure-map/v1",
                "reader/table/v1",
            ),
        )
        plan = plan_reconciliation(before, current)
        self.assertEqual(plan.change_class, "K5")
        self.assertEqual(
            plan.affected_surface_ids,
            (
                "reader/closure-map/v1",
                "reader/picture91-crop/v1",
                "reader/table/v1",
            ),
        )

    def test_missing_notion_authority_fails_closed(self):
        current = state(notion_page_id="", authority_link_ok=False)
        plan = plan_reconciliation(state(), current)
        self.assertEqual(plan.change_class, "INVALID")
        self.assertFalse(plan.cursor_write)
        self.assertTrue(any("authority" in item for item in plan.errors))

    def test_owner_ambiguity_fails_closed(self):
        current = state(owner_check="ambiguous")
        plan = plan_reconciliation(state(), current)
        self.assertEqual(plan.change_class, "INVALID")
        self.assertTrue(any("owner uniqueness" in item for item in plan.errors))

    def test_missing_code_anchor_fails_closed(self):
        current = state(code_anchor_exists=False)
        plan = plan_reconciliation(state(), current)
        self.assertEqual(plan.change_class, "INVALID")
        self.assertTrue(any("code anchor" in item for item in plan.errors))

    def test_consumed_requires_acceptance_and_no_live_owner(self):
        current = state(
            classification="KNOWN + CONSUMED",
            acceptance_closed=False,
            live_implementation_owner="rar2#12",
        )
        plan = plan_reconciliation(state(), current)
        self.assertEqual(plan.change_class, "INVALID")
        joined = "\n".join(plan.errors)
        self.assertIn("closed acceptance", joined)
        self.assertIn("live implementation owner", joined)

    def test_receipt_contains_no_unregistered_payload(self):
        before = state()
        current = state(github_cursor="issue#12@new")
        payload = json.loads(plan_reconciliation(before, current).json_text())
        self.assertEqual(
            set(payload),
            {
                "affected_surface_ids",
                "before_cursor",
                "change_class",
                "current_cursor",
                "cursor_write",
                "errors",
                "schema_version",
                "semantic_write",
                "surface_id",
            },
        )


if __name__ == "__main__":
    unittest.main()
