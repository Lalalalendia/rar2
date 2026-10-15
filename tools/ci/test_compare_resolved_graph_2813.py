"""Source-free adversarial checks for physical LINE 405 graph proof (#2813)."""
import copy
import unittest

from compare_resolved_graph_2813 import compare


def graph():
    return {
        "cdm_version": "0.1",
        "resolver_version": "1",
        "source": {"sha256": "pinned_public_source"},
        "document": {"pages": ["page"], "source_hash": "sha256:public"},
        "pages": {"page": {
            "id": "page", "size": {"width": 100, "height": 100},
            "children": ["old"],
        }},
        "nodes": {"old": {
            "kind": "shape", "header": {"id": "old", "parent_id": "page"},
            "payload": {"source_specific_property": "PRIVATE-DOCUMENT-TEXT"},
        }},
        "stories": {}, "paragraphs": {}, "text_runs": {},
        "resources": {}, "styles": {}, "extensions": {},
    }


def candidate():
    a = graph()
    a["nodes"]["new"] = {
        "kind": "shape",
        "header": {
            "id": "new", "parent_id": "page",
            "bounds": {
                "x": 1058750, "y": 705474, "width": 6729849, "height": 101600,
            },
            "source_refs": [{
                "path": "SpContainer/ClientAnchor/source-horizontal-line-stroke",
                "role": "projection", "authority": "authoritative",
                "confidence": "exact", "source_hash": "sha256:public",
                "carrier": "PRIVATE-CARRIER-DATA",
            }],
        },
        "payload": {
            "contents_seq_num": 405, "officeart_shape_type": 20,
            "effective_paint": {
                "line": {
                    "color_rgb": {"value": [255, 0, 255]},
                    "width_emu": {"value": 101600},
                    "visible": {"value": True},
                },
                "fill": {"solid": None, "visible": None, "color_rgb": None},
            },
            "arbitrary_text": "PRIVATE-SECRET",
        },
    }
    return a


class TestExactGraphDelta(unittest.TestCase):
    def test_one_source_backed_direct_page_line(self):
        proof, errors = compare(graph(), candidate())
        self.assertEqual(errors, [])
        self.assertEqual(proof["new_node_kind"], "shape")
        self.assertEqual(proof["new_page_children"], 0)
        self.assertEqual(proof["registries"]["nodes"]["added"], 1)
        self.assertTrue(proof["new_node_has_exact_stroke_source_authority"])
        self.assertTrue(proof["new_node_exact_source_line_405"])
        self.assertNotIn("PRIVATE", str(proof))

    def test_existing_node_mutation_rejected(self):
        new = candidate()
        new["nodes"]["old"]["payload"]["source_specific_property"] = "changed"
        self.assertIn("old_nodes_changed_or_removed", compare(graph(), new)[1])

    def test_story_mutation_rejected_without_exposing_text(self):
        new = candidate()
        new["stories"]["new_story"] = {"text": "SECRET"}
        report, errs = compare(graph(), new)
        self.assertIn("unexpected_registry_delta_stories", errs)
        self.assertNotIn("SECRET", str(report))

    def test_page_registry_mutation_rejected(self):
        new = candidate()
        new["pages"]["page"]["children"].append("new")
        self.assertIn("page_registry_changed", compare(graph(), new)[1])

    def test_unknown_new_kind_not_emitted(self):
        new = candidate()
        new["nodes"]["new"]["kind"] = "PRIVATE-SECRET-TYPE"
        report, errs = compare(graph(), new)
        self.assertIn("new_node_not_shape", errs)
        self.assertNotIn("PRIVATE-SECRET-TYPE", str(report))

    def test_missing_source_authority_rejected(self):
        new = candidate()
        new["nodes"]["new"]["header"]["source_refs"] = []
        self.assertIn("new_node_missing_exact_stroke_authority", compare(graph(), new)[1])

    def test_forged_unrelated_parent_rejected(self):
        new = candidate()
        new["nodes"]["new"]["header"]["parent_id"] = "unrelated"
        self.assertIn("new_node_missing_existing_page_parent", compare(graph(), new)[1])

    def test_wrong_width_and_hidden_fill_rejected(self):
        new = candidate()
        new["nodes"]["new"]["header"]["bounds"]["height"] = 0
        self.assertIn("new_node_not_exact_line_405_geometry_and_paint",
                      compare(graph(), new)[1])
        new = candidate()
        new["nodes"]["new"]["payload"]["effective_paint"]["fill"]["visible"] = True
        self.assertIn("new_node_not_exact_line_405_geometry_and_paint",
                      compare(graph(), new)[1])

    def test_source_sha_forgery_rejected(self):
        new = candidate()
        new["nodes"]["new"]["header"]["source_refs"][0]["source_hash"] = "attacker"
        self.assertIn("new_node_missing_exact_stroke_authority", compare(graph(), new)[1])


if __name__ == "__main__":
    unittest.main()
