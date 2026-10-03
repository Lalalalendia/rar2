import importlib.util
import pathlib
import unittest

MODULE = pathlib.Path(__file__).with_name("publisher_html_harvest.py")
spec = importlib.util.spec_from_file_location("publisher_html_harvest", MODULE)
harvest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harvest)


class PublisherHtmlPrivInferenceTests(unittest.TestCase):
    def test_unique_same_owner_property_is_inferred(self):
        text = """
        <b:Root type="OplPo" oty="1" oh="1">
          <b:DxlMax priv="AA04">100</b:DxlMax>
          <b:DxlMax>200</b:DxlMax>
          <b:DxlMax>300</b:DxlMax>
        </b:Root>
        """
        _, props, meta = harvest.parse_publisher_xml(text, "s")
        rows = [p for p in props if p["name"] == "DxlMax"]
        self.assertEqual([p["priv"] for p in rows], ["AA04", "AA04", "AA04"])
        self.assertEqual(
            [p["priv_origin"] for p in rows],
            ["explicit", "inferred_same_source_owner_name", "inferred_same_source_owner_name"],
        )
        self.assertEqual(meta["inferred"], 2)
        self.assertEqual(meta["unresolved"], 0)

    def test_ambiguous_same_owner_property_stays_unresolved(self):
        text = """
        <b:Root type="OplEcp" oty="2" oh="1">
          <b:Data priv="10E">1</b:Data>
          <b:Data priv="20E">2</b:Data>
          <b:Data>3</b:Data>
        </b:Root>
        """
        _, props, meta = harvest.parse_publisher_xml(text, "s")
        rows = [p for p in props if p["name"] == "Data"]
        self.assertIsNone(rows[-1]["priv"])
        self.assertEqual(rows[-1]["priv_origin"], "unresolved_missing")
        self.assertEqual(meta["inferred"], 0)
        self.assertEqual(meta["unresolved"], 1)
        self.assertEqual(len(meta["ambiguous_keys"]), 1)

    def test_owner_class_is_part_of_inference_key(self):
        text = """
        <b:A type="OplA" oty="1" oh="1">
          <b:Color priv="104">10</b:Color>
          <b:Color>11</b:Color>
        </b:A>
        <b:B type="OplB" oty="2" oh="2">
          <b:Color priv="204">20</b:Color>
          <b:Color>21</b:Color>
        </b:B>
        """
        _, props, _ = harvest.parse_publisher_xml(text, "s")
        rows = [(p["owner_type"], p["value"], p["priv"]) for p in props if p["name"] == "Color"]
        self.assertEqual(rows, [
            ("OplA", "10", "104"),
            ("OplA", "11", "104"),
            ("OplB", "20", "204"),
            ("OplB", "21", "204"),
        ])

    def test_no_cross_source_state(self):
        one = '<b:R type="OplPo"><b:Qtf priv="3404">1</b:Qtf></b:R>'
        two = '<b:R type="OplPo"><b:Qtf>2</b:Qtf></b:R>'
        _, props1, _ = harvest.parse_publisher_xml(one, "one")
        _, props2, meta2 = harvest.parse_publisher_xml(two, "two")
        self.assertEqual(props1[0]["priv"], "3404")
        self.assertIsNone(props2[0]["priv"])
        self.assertEqual(meta2["inferred"], 0)
        self.assertEqual(meta2["unresolved"], 1)

    def test_self_container_never_infers_index(self):
        text = """
        <b:Root type="OplOt">
          <b:OplOt type="OplOt" priv="11"><b:OhTrack priv="10D">1</b:OhTrack></b:OplOt>
          <b:OplOt type="OplOt"><b:OhTrack>2</b:OhTrack></b:OplOt>
        </b:Root>
        """
        _, props, meta = harvest.parse_publisher_xml(text, "s")
        rows = [p for p in props if p["owner_type"] == "OplOt" and p["name"] == "OplOt"]
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0]["priv"], "11")
        self.assertIsNone(rows[1]["priv"])
        self.assertEqual(rows[1]["priv_origin"], "unresolved_missing")
        self.assertGreaterEqual(meta["unresolved"], 1)

    def test_publisher_major_parser(self):
        self.assertEqual(harvest.publisher_major("Microsoft Publisher 10"), 10)
        self.assertEqual(harvest.publisher_major("Microsoft Publisher 11"), 11)
        self.assertIsNone(harvest.publisher_major("Publisher.Document"))

    def test_version_diff_is_owner_aware_and_fail_closed(self):
        props = [
            {"source_publisher_major": 10, "owner_type": "OplA", "name": "Color", "priv": "104"},
            {"source_publisher_major": 11, "owner_type": "OplA", "name": "Color", "priv": "104"},
            {"source_publisher_major": 10, "owner_type": "OplB", "name": "Color", "priv": "204"},
            {"source_publisher_major": 11, "owner_type": "OplB", "name": "Color", "priv": "304"},
            {"source_publisher_major": 11, "owner_type": "OplC", "name": "Only11", "priv": "404"},
        ]
        diff = harvest.build_version_diff(props)
        self.assertEqual(diff["versions"], [10, 11])
        comp = diff["comparisons"][0]
        self.assertEqual(comp["exact_coordinate_intersection"], 1)
        self.assertEqual(comp["stable_singleton_keys"], 1)
        self.assertEqual(comp["differing_priv_set_keys"], 1)
        self.assertEqual(comp["a_only_coordinates"], 1)
        self.assertEqual(comp["b_only_coordinates"], 2)


if __name__ == "__main__":
    unittest.main()
