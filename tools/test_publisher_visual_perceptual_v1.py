#!/usr/bin/env python3
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from publisher_visual_perceptual_v1 import compare_signature, perceptual_from_image


class PerceptualOracleTests(unittest.TestCase):
    def _image(self, root: Path, name: str, invert: bool = False) -> Path:
        path = root / name
        image = Image.new("RGB", (128, 96), "white" if not invert else "black")
        px = image.load()
        for y in range(20, 76):
            for x in range(24, 104):
                px[x, y] = (20, 80, 180) if not invert else (235, 175, 75)
        image.save(path)
        return path

    def test_identical_signature_is_zero_distance(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._image(Path(tmp), "a.png")
            sig = perceptual_from_image(path)
            diff = compare_signature(sig, sig)
            self.assertEqual(diff["score"], 0.0)

    def test_visible_change_increases_distance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a = perceptual_from_image(self._image(root, "a.png"))
            b = perceptual_from_image(self._image(root, "b.png", invert=True))
            diff = compare_signature(a, b)
            self.assertGreater(diff["score"], 0.1)

    def test_signature_is_source_free_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            sig = perceptual_from_image(self._image(Path(tmp), "a.png"))
            self.assertEqual(set(sig), {"dhash64", "ahash64", "inkhash256", "color_hist24", "edge_hist8"})
            self.assertEqual(len(sig["dhash64"]), 16)
            self.assertEqual(len(sig["ahash64"]), 16)
            self.assertEqual(len(sig["inkhash256"]), 64)
            self.assertEqual(len(sig["color_hist24"]), 24)
            self.assertEqual(len(sig["edge_hist8"]), 8)


if __name__ == "__main__":
    unittest.main()
