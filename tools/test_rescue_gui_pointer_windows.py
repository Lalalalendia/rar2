"""Source-free cross-platform checks for real Windows pointer probe geometry."""
import unittest

from tools.probe_rescue_gui_pointer_windows import (
    Input, KeyInput, MouseInput, OsInput, SCHEMA, slot_coordinates,
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
        self.clipboard_opens = 0

    def GetForegroundWindow(self):
        return self.foreground

    def SetForegroundWindow(self, hwnd):
        self.foreground = hwnd
        return 1

    def SendInput(self, *_args):
        self.injections += 1
        return 1

    def OpenClipboard(self, _hwnd):
        self.clipboard_opens += 1
        return 1


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

    def test_clipboard_does_not_open_without_target_foreground(self):
        driver = self.native(12)
        with self.assertRaisesRegex(RuntimeError, "not foreground"):
            driver.clipboard("SYNTHETIC")
        self.assertEqual(driver.user.clipboard_opens, 0)

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


if __name__ == "__main__":
    unittest.main()
