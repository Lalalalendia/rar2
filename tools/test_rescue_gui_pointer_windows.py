"""Source-free cross-platform checks for real Windows pointer probe geometry."""
import unittest

from tools.probe_rescue_gui_pointer_windows import (
    Input, KeyInput, MouseInput, OsInput, SCHEMA, slot_coordinates,
    validate_scanned_source, validate_completed_source, BATCH_REPORT_SCHEMA,
)


class GeometryTests(unittest.TestCase):
    def test_normal_window_has_distinct_field_rows(self):
        slots = slot_coordinates(980, 760, 96)
        self.assertLess(slots["source"][1], slots["output"][1])
        self.assertLess(slots["output"][1], slots["scan"][1])
        self.assertEqual(slots["scan"][1], slots["run"][1])
        self.assertNotEqual(slots["source"], slots["output"])

    def test_hidpi_coordinates_remain_inside_viewport(self):
        slots = slot_coordinates(1920, 1400, 144)
        self.assertTrue(all(0 < x < 1920 and 0 < y < 1400 for x, y in slots.values()))

    def test_too_small_window_rejected(self):
        with self.assertRaisesRegex(ValueError, "outside GUI probe envelope"):
            slot_coordinates(640, 220, 96)

    def test_unexpected_dpi_rejected(self):
        with self.assertRaisesRegex(ValueError, "outside GUI probe envelope"):
            slot_coordinates(800, 700, 500)

    def test_win32_input_structure_builds_without_platform_calls(self):
        self.assertEqual(Input(type=1, ki=KeyInput(0, 65, 4, 0, 0)).type, 1)
        self.assertEqual(Input(type=0, mi=MouseInput(0, 0, 0, 2, 0, 0)).type, 0)

    def test_receipt_contract_cannot_be_mistaken_for_full_gui_acceptance(self):
        self.assertEqual(SCHEMA, "chaptera.rescue-gui-pointer-scan.v1")



class FakeUser:
    def __init__(self, foreground):
        self.foreground = foreground
        self.injections = 0
        self.batches = []

    def GetForegroundWindow(self):
        return self.foreground

    def SetForegroundWindow(self, hwnd):
        self.foreground = hwnd
        return 1

    def SendInput(self, count, buffer, _size):
        self.injections += count
        self.batches.append([
            (buffer[i].type, buffer[i].ki.wVk, buffer[i].ki.dwFlags)
            for i in range(count)
        ])
        return count


class FocusGuardTests(unittest.TestCase):
    def native(self, active):
        driver = OsInput.__new__(OsInput)
        driver.hwnd = 13
        driver.user = FakeUser(active)
        return driver

    def test_unrelated_foreground_blocks_global_sendinput(self):
        driver = self.native(12)
        with self.assertRaisesRegex(RuntimeError, "not foreground"):
            driver._send(Input(type=1, ki=KeyInput(65, 0, 0, 0, 0)))
        self.assertEqual(driver.user.injections, 0)

    def test_unrelated_foreground_blocks_entire_key_combo(self):
        driver = self.native(12)
        with self.assertRaisesRegex(RuntimeError, "not foreground"):
            driver.combo(65)
        self.assertEqual(driver.user.injections, 0)
        self.assertEqual(driver.user.batches, [])

    def test_exact_target_foreground_accepts_input(self):
        driver = self.native(13)
        driver._send(Input(type=1, ki=KeyInput(65, 0, 0, 0, 0)))
        self.assertEqual(driver.user.injections, 1)

    def test_focus_is_confirmed_before_input(self):
        driver = self.native(12)
        driver._focus_target()
        self.assertEqual(driver.user.foreground, 13)
        driver._send(Input(type=1, ki=KeyInput(65, 0, 0, 0, 0)))
        self.assertEqual(driver.user.injections, 1)


    def test_ctrl_a_sent_as_one_atomic_four_event_batch(self):
        driver = self.native(13)
        driver.combo(65)
        self.assertEqual(len(driver.user.batches), 1)
        self.assertEqual(driver.user.batches[0], [
            (1, 0x11, 0), (1, 0x41, 0),
            (1, 0x41, 2), (1, 0x11, 2),
        ])
        self.assertEqual(driver.user.injections, 4)

    def test_unicode_press_release_in_one_batch(self):
        driver = self.native(13)
        driver.unicode("A")
        self.assertEqual(len(driver.user.batches), 1)
        self.assertEqual(driver.user.batches[0], [(1, 0, 4), (1, 0, 6)])

    def test_global_clipboard_is_not_a_test_dependency(self):
        self.assertFalse(hasattr(OsInput, "clipboard"))
        self.assertFalse(hasattr(OsInput, "field_equals"))


class CanonicalReportTests(unittest.TestCase):
    digest = "a" * 64

    def state(self, status="pending", **row_changes):
        row = {
            "sha256": self.digest, "status": status,
            "attempts": 0 if status == "pending" else 1,
            "outcome": None if status == "pending" else "partial",
            "reason": None, "stream_count": 0 if status == "pending" else 2,
        }
        row.update(row_changes)
        return {
            "schema_version": BATCH_REPORT_SCHEMA,
            "source_free": True,
            "unique_sources": 1, "duplicate_paths": 0,
            "rejected_paths": {}, "counts": {status: 1},
            "entries": [row],
        }

    def test_correct_scan_is_admitted(self):
        validate_scanned_source(self.state(), self.digest)

    def test_correct_completed_two_sid_bundle_is_admitted(self):
        validate_completed_source(self.state(status="done_partial"), self.digest)

    def test_wrong_pub_sha_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "SHA"):
            validate_scanned_source(self.state(sha256="b" * 64), self.digest)

    def test_missing_source_free_is_rejected(self):
        state = self.state()
        state["source_free"] = False
        with self.assertRaises(AssertionError):
            validate_scanned_source(state, self.digest)

    def test_forged_counts_are_rejected(self):
        state = self.state()
        state["counts"] = {"pending": 0, "done_partial": 1}
        with self.assertRaises(AssertionError):
            validate_scanned_source(state, self.digest)

    def test_unverified_sid_count_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "SID streams"):
            validate_completed_source(self.state("done_partial", stream_count=1), self.digest)

    def test_unverified_outcome_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "SID streams"):
            validate_completed_source(self.state("done_partial", outcome="unsupported"), self.digest)

    def test_zero_attempts_cannot_claim_completed_run(self):
        with self.assertRaisesRegex(AssertionError, "SID streams"):
            validate_completed_source(self.state("done_partial", attempts=0), self.digest)


if __name__ == "__main__":
    unittest.main()
