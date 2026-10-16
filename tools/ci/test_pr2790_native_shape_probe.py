"""Small negative controls for native per-object matching; not an app test."""
import unittest
import xml.etree.ElementTree as ET
from pr2790_native_shape_probe import (collect_scribus_shapes,
                                       collect_libreoffice_shapes,
                                       length_to_emu, uniquely_match)


EXPECTED = {
    'A': {'x':100000,'y':100000,'width':500000,'height':250000},
    'B': {'x':300000,'y':100000,'width':500000,'height':250000},
    'C': {'x':900000,'y':100000,'width':500000,'height':250000},
}

class NativeProbeTests(unittest.TestCase):
    def test_unit_conversion(self):
        self.assertEqual(length_to_emu('1pt'), 12700)
        self.assertEqual(length_to_emu('1in'), 914400)
        self.assertEqual(length_to_emu('2.54cm'), 914400)
        self.assertEqual(length_to_emu('25.4mm'), 914400)
        self.assertEqual(length_to_emu('72'), 914400)

    def test_injective_three_rectangles(self):
        observed = list(EXPECTED.values())
        result = uniquely_match(EXPECTED, observed)
        self.assertEqual(result['matched_shape_count'], 3)
        self.assertEqual(result['max_native_delta_emu'], 0)

    def test_fewer_native_shapes_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'native_individual_geometry_retention_failed'):
            uniquely_match(EXPECTED, list(EXPECTED.values())[:2])

    def test_duplicate_same_native_shape_not_double_counted(self):
        with self.assertRaisesRegex(AssertionError, 'native_individual_geometry_retention_failed'):
            uniquely_match({'A':EXPECTED['A'], 'B':EXPECTED['A'], 'C':EXPECTED['C']},
                           [EXPECTED['A'], EXPECTED['C']])

    def test_beyond_one_point_native_rounding_rejected(self):
        observed = [dict(v) for v in EXPECTED.values()]
        observed[1]['x'] += 12701
        with self.assertRaisesRegex(AssertionError, 'native_individual_geometry_retention_failed'):
            uniquely_match(EXPECTED, observed)

    def test_scribus_pageobjects_extraction(self):
        root = ET.fromstring('<SCRIBUSUTF8NEW><DOCUMENT><PAGEOBJECT XPOS="72" '
                             'YPOS="7.874015748031496" WIDTH="12" HEIGHT="19.685"/>'
                             '</DOCUMENT></SCRIBUSUTF8NEW>')
        shape = collect_scribus_shapes(root)[0]
        self.assertEqual(shape['x'], 914400)
        self.assertEqual(shape['width'], 152400)

    def test_libreoffice_fodg_rect_extraction(self):
        root = ET.fromstring('<document xmlns:draw="urn:draw" xmlns:svg="urn:svg">'
                             '<draw:page><draw:rect svg:x="1cm" svg:y="2cm" '
                             'svg:width="3cm" svg:height="4cm"/></draw:page></document>')
        shape = collect_libreoffice_shapes(root)[0]
        self.assertEqual(shape['x'], 360000)
        self.assertEqual(shape['height'], 1440000)

if __name__ == '__main__':
    unittest.main()