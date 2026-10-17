"""Source-free cross-platform checks for real Windows pointer probe geometry."""
import unittest

from tools.probe_rescue_gui_pointer_windows import (
    Input, KeyInput, MouseInput, SCHEMA, slot_coordinates,
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


if __name__ == "__main__":
    unittest.main()
