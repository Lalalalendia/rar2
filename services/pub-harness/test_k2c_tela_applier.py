#!/usr/bin/env python3
import unittest

from k2c_tela_applier import (
    ApplyError,
    ManifestError,
    PreconditionError,
    TelaLintOutcome,
    TelaPageSnapshot,
    TelaPatchManifest,
    TelaPatchOperation,
    TelaPatchOutcome,
    apply_manifest,
    validate_manifest,
)


class MockTela:
    def __init__(self):
        self.pages = {
            5049: TelaPageSnapshot(5049, 364, "2026-09-29 12:54:13"),
            5027: TelaPageSnapshot(5027, 364, "2026-09-29 12:27:19"),
        }
        self.calls = []
        self.applied = set()
        self.lint = TelaLintOutcome(5049, 0, 0)

    def get_page(self, page_id):
        self.calls.append(("get_page", page_id))
        return self.pages[page_id]

    def patch_page(self, *, page_id, target, operation, content, idempotency_key):
        self.calls.append(("patch_page", page_id, operation, idempotency_key))
        before = self.pages[page_id]
        replay = idempotency_key in self.applied
        if not replay:
            self.applied.add(idempotency_key)
            self.pages[page_id] = TelaPageSnapshot(
                page_id, before.space_id, before.updated_at + "+patched"
            )
        return TelaPatchOutcome(
            page_id=page_id,
            updated_at=self.pages[page_id].updated_at,
            idempotent_replay=replay,
        )

    def lint_page(self, page_id):
        self.calls.append(("lint_page", page_id))
        return TelaLintOutcome(
            page_id=page_id,
            errors=self.lint.errors,
            warnings=self.lint.warnings,
        )


def op(**changes):
    values = dict(
        page_id=5049,
        expected_updated_at="2026-09-29 12:54:13",
        target="Probe > Live acceptance target",
        operation="append",
        content="probe = once",
        idempotency_key="k2c-probe-v1",
        cursor_advance=False,
    )
    values.update(changes)
    return TelaPatchOperation(**values)


def manifest(*ops):
    return TelaPatchManifest(
        schema_version="chaptera.k2c-tela-patchset.v1",
        manifest_id="probe-manifest-v1",
        expected_space_id=364,
        operations=tuple(ops),
    )


class K2CTelaApplierTests(unittest.TestCase):
    def test_dry_run_preflights_without_write(self):
        t = MockTela()
        receipt = apply_manifest(
            manifest(op()), t, allowed_space_id=364, apply=False
        )
        self.assertTrue(receipt.completed)
        self.assertFalse(receipt.apply_requested)
        self.assertEqual([c[0] for c in t.calls], ["get_page"])
        self.assertEqual(receipt.operations[0].status, "planned")

    def test_apply_orders_preflight_patch_lint(self):
        t = MockTela()
        receipt = apply_manifest(
            manifest(op()), t, allowed_space_id=364, apply=True
        )
        self.assertTrue(receipt.completed)
        self.assertEqual(
            [c[0] for c in t.calls],
            ["get_page", "patch_page", "lint_page"],
        )
        self.assertEqual(receipt.operations[0].status, "applied")

    def test_all_pages_preflight_before_first_write(self):
        t = MockTela()
        m = manifest(
            op(),
            op(
                page_id=5027,
                expected_updated_at="2026-09-29 12:27:19",
                target="Purpose > Registry",
                content="cursor",
                idempotency_key="registry-cursor-v1",
                cursor_advance=True,
            ),
        )
        apply_manifest(m, t, allowed_space_id=364, apply=True)
        self.assertEqual(t.calls[0:2], [("get_page", 5027), ("get_page", 5049)])

    def test_wrong_space_fails_before_write(self):
        t = MockTela()
        t.pages[5049] = TelaPageSnapshot(5049, 999, "2026-09-29 12:54:13")
        with self.assertRaisesRegex(PreconditionError, "space"):
            apply_manifest(manifest(op()), t, allowed_space_id=364, apply=True)
        self.assertFalse(any(c[0] == "patch_page" for c in t.calls))

    def test_stale_cursor_fails_before_write(self):
        t = MockTela()
        t.pages[5049] = TelaPageSnapshot(5049, 364, "new")
        with self.assertRaisesRegex(PreconditionError, "stale"):
            apply_manifest(manifest(op()), t, allowed_space_id=364, apply=True)
        self.assertFalse(any(c[0] == "patch_page" for c in t.calls))

    def test_duplicate_idempotency_key_fails_closed(self):
        m = manifest(
            op(),
            op(
                page_id=5027,
                expected_updated_at="2026-09-29 12:27:19",
                target="Purpose > Registry",
            ),
        )
        with self.assertRaisesRegex(ManifestError, "duplicate idempotency_key"):
            validate_manifest(m, allowed_space_id=364)

    def test_cursor_advance_must_be_last(self):
        m = manifest(
            op(cursor_advance=True),
            op(
                page_id=5027,
                expected_updated_at="2026-09-29 12:27:19",
                target="Purpose > Registry",
                content="later",
                idempotency_key="later-v1",
            ),
        )
        with self.assertRaisesRegex(ManifestError, "must be last"):
            validate_manifest(m, allowed_space_id=364)

    def test_cursor_write_is_skipped_after_prior_lint_failure(self):
        t = MockTela()
        t.lint = TelaLintOutcome(5049, 1, 0)
        m = manifest(
            op(),
            op(
                page_id=5027,
                expected_updated_at="2026-09-29 12:27:19",
                target="Purpose > Registry",
                content="cursor",
                idempotency_key="registry-cursor-v1",
                cursor_advance=True,
            ),
        )
        with self.assertRaises(ApplyError):
            apply_manifest(m, t, allowed_space_id=364, apply=True)
        patched = [c[1] for c in t.calls if c[0] == "patch_page"]
        self.assertEqual(patched, [5049])

    def test_lint_warning_fails_closed(self):
        t = MockTela()
        t.lint = TelaLintOutcome(5049, 0, 1)
        with self.assertRaises(ApplyError):
            apply_manifest(manifest(op()), t, allowed_space_id=364, apply=True)

    def test_invalid_operation_fails_closed(self):
        with self.assertRaisesRegex(ManifestError, "unsupported operation"):
            validate_manifest(manifest(op(operation="update_page")), allowed_space_id=364)

    def test_completed_receipt_allows_idempotent_retry(self):
        t = MockTela()
        m = manifest(op())
        first = apply_manifest(m, t, allowed_space_id=364, apply=True)
        second = apply_manifest(
            m,
            t,
            allowed_space_id=364,
            apply=True,
            previous_receipt=first,
        )
        self.assertTrue(second.completed)
        self.assertTrue(second.idempotent_manifest_replay)
        self.assertEqual(second.operations[0].status, "already_applied")
        self.assertEqual(t.applied, {"k2c-probe-v1"})

    def test_receipt_is_source_safe(self):
        t = MockTela()
        receipt = apply_manifest(manifest(op()), t, allowed_space_id=364, apply=False)
        encoded = receipt.json_text()
        self.assertNotIn("probe = once", encoded)
        self.assertNotIn("Probe > Live acceptance target", encoded)
        self.assertIn("content_sha256", encoded)
        self.assertIn("target_sha256", encoded)


if __name__ == "__main__":
    unittest.main()
