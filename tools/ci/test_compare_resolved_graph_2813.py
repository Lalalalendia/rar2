"""Source-free adversarial checks for exact graph drift oracle (#2813)."""
import copy
import unittest

from compare_resolved_graph_2813 import compare, _subsequence


def graph():
    return {
        "cdm_version": "0.1",
        "resolver_version": "1",
        "source": {"sha256": "pinned_public_source"},
        "document": {"pages": ["page"]},
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
            "source_refs": [{"carrier": "PRIVATE-CARRIER-DATA"}],
        },
        "payload": {"arbitrary_text": "PRIVATE-SECRET"},
    }
    a["pages"]["page"]["children"].append("new")
    return a


class TestExactGraphDelta(unittest.TestCase):
    def test_one_additive_shape_with_physical_refs(self):
        proof, errors = compare(graph(), candidate())
        self.assertEqual(errors, [])
        self.assertEqual(proof["new_node_kind"], "shape")
        self.assertEqual(proof["new_page_children"], 1)
        self.assertEqual(proof["registries"]["nodes"]["added"], 1)
        self.assertNotIn("PRIVATE", str(proof))

    def test_existing_node_mutation_rejected(self):
        new = candidate()
        new["nodes"]["old"]["payload"]["source_specific_property"] = "changed"
        self.assertIn("old_nodes_changed_or_removed", compare(graph(), new)[1])

    def test_story_mutation_rejected(self):
        new = candidate()
        new["stories"]["new_story"] = {"text": "SECRET"}
        report, errs = compare(graph(), new)
        self.assertIn("unexpected_registry_delta_stories", errs)
        self.assertNotIn("SECRET", str(report))

    def test_old_page_reorder_rejected(self):
        old = graph()
        old["nodes"]["old2"] = copy.deepcopy(old["nodes"]["old"])
        old["pages"]["page"]["children"].append("old2")
        new = copy.deepcopy(old)
        new["nodes"]["new"] = candidate()["nodes"]["new"]
        new["pages"]["page"]["children"] = ["old2", "new", "old"]
        self.assertIn("existing_page_paint_order_changed", compare(old, new)[1])

    def test_unknown_source_fields_not_exposed(self):
        new = candidate()
        new["nodes"]["new"]["kind"] = "PRIVATE-SECRET-TYPE"
        report, errs = compare(graph(), new)
        self.assertIn("new_node_not_shape", errs)
        self.assertNotIn("PRIVATE-SECRET-TYPE", str(report))
        self.assertEqual(report["new_node_kind"], "unrecognized")

    def test_missing_source_authority_rejected(self):
        new = candidate()
        new["nodes"]["new"]["header"]["source_refs"] = []
        self.assertIn("new_node_missing_physical_source_refs", compare(graph(), new)[1])

    def test_subsequence_not_set_membership(self):
        self.assertTrue(_subsequence(["a", "b"], ["a", "new", "b"]))
        self.assertFalse(_subsequence(["a", "b"], ["b", "new", "a"]))


if __name__ == "__main__":
    unittest.main()
