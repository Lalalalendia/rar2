#!/usr/bin/env python3
import copy
import unittest

from validate_viewer_geometry_receipt import validate_schema


def minimal_receipt():
    return {
        "schema_version": "0.1",
        "document": {
            "schema_version": "0.1",
            "source": {
                "format": "publisher",
                "format_version": "0x2c",
                "source_hash": "a" * 64,
                "byte_len": 123,
            },
            "pages": [{
                "index": 1,
                "id": "10000000-0000-4000-8000-000000000001",
                "width_emu": 914400,
                "height_emu": 914400,
            }],
            "stories": [{
                "id": "20000000-0000-4000-8000-000000000001",
                "text": "public fixture text",
            }],
            "diagnostics": [],
        },
        "scene": {
            "environment": {
                "engine_revision": "viewer-geometry-v0.1",
                "font_set_fingerprint": "fonts:not-consumed:geometry-only",
                "resource_fingerprint": "resources:not-consumed:geometry-only",
            },
            "surfaces": [{
                "origin": "10000000-0000-4000-8000-000000000001",
                "size": {"width": 914400, "height": 914400},
                "bleed": None,
                "margins": None,
            }],
            "nodes": [{
                "origin": "30000000-0000-4000-8000-000000000001",
                "parent_origin": "10000000-0000-4000-8000-000000000001",
                "bounds": {"x": 0, "y": 0, "width": 127000, "height": 254000},
                "transform": {"a": "1", "b": "0", "c": "0", "d": "1", "tx": 0, "ty": 0},
            }],
            "origin_mapping": [{
                "authoring_origin": "30000000-0000-4000-8000-000000000001",
                "resolved_node_origin": "30000000-0000-4000-8000-000000000001",
            }],
            "diagnostics": [],
        },
        "paints": [],
        "story_frames": [],
        "images": [{
            "resource_id": "40000000-0000-4000-8000-000000000001",
            "mime": "image/png",
            "source_exact": True,
            "node_ids": ["30000000-0000-4000-8000-000000000001"],
        }],
    }


class ViewerGeometryReceiptSchemaTests(unittest.TestCase):
    def test_current_source_free_shape_is_admitted(self):
        validate_schema(minimal_receipt())

    def test_unknown_top_level_private_field_fails_closed(self):
        receipt = minimal_receipt()
        receipt["private_checkout_path"] = "synthetic-unreviewed-checkout"
        with self.assertRaises(AssertionError):
            validate_schema(receipt)

    def test_unknown_nested_source_field_fails_closed(self):
        receipt = minimal_receipt()
        receipt["document"]["source"]["local_path"] = "synthetic-unreviewed-fixture"
        with self.assertRaises(AssertionError):
            validate_schema(receipt)

    def test_source_exact_requires_a_boolean(self):
        receipt = minimal_receipt()
        receipt["images"][0]["source_exact"] = "yes"
        with self.assertRaises(AssertionError):
            validate_schema(receipt)

    def test_serialized_image_bytes_are_forbidden(self):
        receipt = minimal_receipt()
        receipt["images"][0]["bytes"] = "iVBORw0KGgo="
        with self.assertRaises(AssertionError):
            validate_schema(receipt)

    def test_exact_viewer_shape_and_font_index_provenance_is_bounded(self):
        # These are typed Viewer fields, not file paths or physical font grants.
        receipt = minimal_receipt()
        node_id = receipt["scene"]["nodes"][0]["origin"]
        story_id = receipt["document"]["stories"][0]["id"]
        receipt["paints"] = [{
            "node_id": node_id,
            "preset_shape": "ellipse",
            "solid_fill_rgb": [1, 2, 3],
        }]
        receipt["typography_runs"] = [{
            "story_id": story_id,
            "scalar_start": 0,
            "scalar_end": 1,
            "source_font_name": "Test Font",
            "source_font_index": 18,
            "text_size_emu": 228600,
            "font_inherited": False,
            "size_inherited": True,
            "source_story_text_sha256": "b" * 64,
            "color_inherited": False,
        }]
        validate_schema(receipt)
        for invalid in [-1, 4294967296, "18", None]:
            bad = copy.deepcopy(receipt)
            bad["typography_runs"][0]["source_font_index"] = invalid
            with self.subTest(invalid_font_index=invalid), self.assertRaises(AssertionError):
                validate_schema(bad)
        for invalid in ["triangle", "private/path", None, 17]:
            bad = copy.deepcopy(receipt)
            bad["paints"][0]["preset_shape"] = invalid
            with self.subTest(invalid_preset=invalid), self.assertRaises(AssertionError):
                validate_schema(bad)
        bad = copy.deepcopy(receipt)
        bad["typography_runs"][0]["source_font_path"] = "/tmp/source/private.otf"
        with self.assertRaises(AssertionError):
            validate_schema(bad)

    def test_unreviewed_fetch_handle_is_forbidden_in_v0_1_receipt(self):
        receipt = minimal_receipt()
        receipt["images"][0]["fetch_handle"] = "file:///private/cache/image.png"
        with self.assertRaises(AssertionError):
            validate_schema(receipt)


if __name__ == "__main__":
    unittest.main()
