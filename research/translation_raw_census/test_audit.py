"""Fast local contract tests: no real website or chapter prose is needed."""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import audit


class StaticResp:
    def __init__(self, text, status_code=200, url="https://ncode.syosetu.com/n7481gn/1/"):
        self.text = text
        self.status_code = status_code
        self.url = url
        self.headers = {}

    def raise_for_status(self):
        if self.status_code > 399:
            raise RuntimeError(f"http {self.status_code}")


class FakeFetcher:
    def __init__(self, data):
        self.data = data
        self.visited = []

    def get(self, url):
        self.visited.append(url)
        if url not in self.data:
            raise RuntimeError("unmocked " + url)
        return StaticResp(self.data[url], url=url)


def chapter_body(text="Real chapter body "):
    return f'<html><div class="js-novel-text p-novel__text p-novel__text--preface">' + ('Note ' * 150) + \
           f'</div><div class="js-novel-text p-novel__text"><p>{text * 20}</p></div>' + \
           '<div class="js-novel-text p-novel__text p-novel__text--afterword">' + ('After ' * 150) + '</div></html>'


class AuditTests(unittest.TestCase):
    def test_narou_body_ignores_long_preface_and_afterword(self):
        text = audit.extract_body(chapter_body(), "narou")
        self.assertIn("Real chapter body", text)
        self.assertNotIn("After", text)
        self.assertNotIn("Note", text)

    def test_royal_chapter_selector_not_catalog(self):
        text = audit.extract_body("<h1>Chapter 1</h1><div class='chapter-content'><p>" + "Story " * 80 + "</p></div>", "royalroad")
        self.assertGreater(len(text), 120)
        self.assertEqual(audit.extract_body("<h1>Chapter 1</h1><div>Next chapter</div>", "royalroad"), "")

    def test_narou_index_pagination_of_201(self):
        base = "https://ncode.syosetu.com/n7481gn/"
        def links(numbers):
            return "<html>" + "".join(f'<div class="p-eplist__sublist"><a href="/n7481gn/{j}/">Ch {j}</a></div>' for j in numbers) + "</html>"
        d = {base: links(range(1, 101)), base + "?p=2": links(range(101, 201)), base + "?p=3": links([201])}
        entries, errs = audit.index_book(FakeFetcher(d), "return")
        self.assertEqual(len(entries), 201)
        self.assertEqual(entries[0].number, 1)
        self.assertEqual(entries[-1].number, 201)
        self.assertEqual(errs, [])

    def test_royal_catalog_ordinals_and_dedupe(self):
        page = '<table><tbody>' + ''.join(f'<tr><td><a href="/fiction/65841/a-scholars-travels-with-a-witcher/chapter/{i}/chapter-{i}">Ch {i}</a></td></tr>' for i in [100, 200, 300]) + '</tbody></table>'
        page += '<a href="/fiction/65841/a-scholars-travels-with-a-witcher/chapter/100/chapter-100">Repeat</a>'
        items = audit.royal_listing(page, '65841')
        self.assertEqual([e.number for e in items], [1, 2, 3])
        self.assertEqual(len(items), 3)
        self.assertTrue(items[1].url.endswith('/200/chapter-200'))

    def test_missing_page_is_not_full_and_text_never_persisted(self):
        index = '<a href="/n7481gn/1/">First</a><a href="/n7481gn/2/">Second</a>'
        base = "https://ncode.syosetu.com/n7481gn/"
        source = FakeFetcher({base: index, base + "?p=2": "", base + "1/": chapter_body("unique body"), base + "2/": "<h1>Chapter heading only</h1>"})
        with tempfile.TemporaryDirectory() as dirname:
            summary = audit.run_book("return", Path(dirname), source)
            self.assertFalse(summary["all_verified"])
            self.assertEqual(summary["ok"], 1)
            self.assertEqual(summary["failed"], 1)
            self.assertEqual(summary["missing_index_numbers"][:3], [3, 4, 5])
            p = Path(dirname, "return.manifest.json")
            data = p.read_text(encoding="utf-8")
            self.assertNotIn("unique body", data)
            self.assertEqual(json.loads(data)[0]["status"], "ok")

    def test_identical_chapters_detected_as_duplicates(self):
        base = "https://ncode.syosetu.com/n7481gn/"
        source = FakeFetcher({base: '<a href="/n7481gn/1/">First</a><a href="/n7481gn/2/">Second</a>', base + "?p=2": "", base + "1/": chapter_body("duplicate "), base + "2/": chapter_body("duplicate ")})
        with tempfile.TemporaryDirectory() as d:
            summary = audit.run_book("return", Path(d), source)
            self.assertEqual(summary["duplicate_groups"], [[1, 2]])
            self.assertFalse(summary["all_verified"])


if __name__ == '__main__':
    unittest.main()
