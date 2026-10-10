"""Offline synthetic tests: no copyrighted text or live website requests."""
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import export_verified as ep


class FakeReader:
    def __init__(self, bodies):
        self.bodies = bodies
        self.urls = []

    def get(self, url):
        self.urls.append(url)
        return self.bodies[url]


class PilotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.book = "fencing"
        self.code = "n8001hc"
        self.manifest = self.root / "fencing.manifest.json"
        self.summary = self.root / "fencing.summary.json"
        self.rows = []
        self.pages = {}
        for n in range(1, 167):
            url = f"https://ncode.syosetu.com/n8001hc/{n}/"
            html = f"<div class='js-novel-text p-novel__text p-novel__text--preface'><p>{'preface ' * 70}</p></div>" + \
                   f"<div class='js-novel-text p-novel__text'><p>第{n}話: {'漢字仮名カタカナ' * 25}</p><p>試験本文第{n}節に<ruby>漢<rt>かん</rt></ruby>字があります。</p></div>" + \
                   f"<div class='js-novel-text p-novel__text p-novel__text--afterword'><p>{'afterword ' * 70}</p></div>"
            self.pages[url] = html
            body = ep.chapter_node(html)
            canonical = ep.canon(body)
            self.rows.append({"ordinal": n, "title": f"Episode {n}", "url": url, "status": "ok", "chars": len(canonical), "sha256": ep.digest(canonical), "error": None})
        self.meta = {"book": "fencing", "provider": "narou", "work_id": "n8001hc", "expected": 166, "indexed": 166, "scanned": 166, "ok": 166, "failed": 0, "unscanned": 0, "missing_index_numbers": [], "duplicate_groups": [], "all_verified": True}
        self.write()

    def write(self):
        self.manifest.write_text(json.dumps(self.rows, ensure_ascii=False), encoding="utf-8")
        self.summary.write_text(json.dumps(self.meta), encoding="utf-8")

    def run_pack(self, reader=None):
        return ep.export_pack("fencing", self.summary, self.manifest, 1, 5, self.root / "private", reader or FakeReader(self.pages))

    def test_happy_path_private_zip_has_all_five_chapters(self):
        reader = FakeReader(self.pages)
        archive = self.run_pack(reader)
        self.assertEqual(len(reader.urls), 5)
        with zipfile.ZipFile(archive) as z:
            self.assertEqual(len(z.namelist()), 6)
            info = json.loads(z.read("fencing-001-005/index.json"))
            self.assertEqual([x["number"] for x in info["chapters"]], [1, 2, 3, 4, 5])
            text = z.read("fencing-001-005/001.txt").decode("utf-8")
            self.assertIn("試験本文", text)
            self.assertIn("漢字", text)
            self.assertNotIn("かん", text)
            self.assertNotIn("preface", text)
            self.assertNotIn("afterword", text)
            self.assertIn("\n\n", text)

    def test_stale_body_does_not_write_partial_zip(self):
        broken = dict(self.pages)
        broken["https://ncode.syosetu.com/n8001hc/3/"] = "<div class='p-novel__text'>edited now and not verified</div>"
        with self.assertRaisesRegex(ValueError, "Stale or mismatched"):
            self.run_pack(FakeReader(broken))
        self.assertFalse((self.root / "private").exists())

    def test_wrong_identity_rejected(self):
        self.meta["work_id"] = "n7481gn"
        self.write()
        with self.assertRaisesRegex(ValueError, "identity"):
            self.run_pack()

    def test_catalog_gap_and_hash_repeat_rejected(self):
        self.rows.pop(5)
        self.write()
        with self.assertRaisesRegex(ValueError, "length mismatch"):
            self.run_pack()
        self.rows.insert(5, dict(self.rows[4]))
        self.write()
        with self.assertRaisesRegex(ValueError, "invalid/duplicate"):
            self.run_pack()

    def test_off_host_manifest_url_rejected(self):
        self.rows[0]["url"] = "https://example.com/evil"
        self.write()
        with self.assertRaisesRegex(ValueError, "invalid/duplicate"):
            self.run_pack()

    def test_incomplete_census_rejected(self):
        self.meta["all_verified"] = False
        self.write()
        with self.assertRaisesRegex(ValueError, "not verified"):
            self.run_pack()

    def test_limit_and_overwrite(self):
        with self.assertRaisesRegex(ValueError, "bounded pack"):
            ep.export_pack("fencing", self.summary, self.manifest, 1, 11, self.root)
        self.run_pack()
        with self.assertRaises(FileExistsError):
            self.run_pack()

    def test_repo_output_rejected(self):
        with self.assertRaisesRegex(ValueError, "inside public Git repository"):
            ep.safe_output(ep.REPO_ROOT / "raw-packs")

    def test_no_story_is_not_success(self):
        broken = dict(self.pages)
        broken["https://ncode.syosetu.com/n8001hc/2/"] = "<body><h1>Chapter heading only</h1></body>"
        with self.assertRaisesRegex(ValueError, "No readable chapter body"):
            self.run_pack(FakeReader(broken))
        self.assertFalse((self.root / "private").exists())


if __name__ == "__main__":
    unittest.main()
