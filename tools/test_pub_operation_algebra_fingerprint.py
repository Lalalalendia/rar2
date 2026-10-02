import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import pub_operation_algebra_fingerprint as fp  # noqa: E402


class PublisherPdfNormalizationTests(unittest.TestCase):
    def test_known_export_noise_normalizes_to_same_bytes(self):
        left = br"""%PDF-1.7
1 0 obj
<< /CreationDate (D:20261002210000-04'00') /ModDate (D:20261002210100-04'00') >>
endobj
<xmp:CreateDate>2026-10-02T21:00:00-04:00</xmp:CreateDate>
<xmp:ModifyDate>2026-10-02T21:01:00-04:00</xmp:ModifyDate>
<xmp:MetadataDate>2026-10-02T21:01:01-04:00</xmp:MetadataDate>
<xmpMM:DocumentID>uuid:11111111-1111-1111-1111-111111111111</xmpMM:DocumentID>
<xmpMM:InstanceID>uuid:22222222-2222-2222-2222-222222222222</xmpMM:InstanceID>
trailer << /ID [<AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA><BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB>] >>
%%EOF
"""
        right = br"""%PDF-1.7
1 0 obj
<< /CreationDate (D:20261002220000-04'00') /ModDate (D:20261002220100-04'00') >>
endobj
<xmp:CreateDate>2026-10-02T22:00:00-04:00</xmp:CreateDate>
<xmp:ModifyDate>2026-10-02T22:01:00-04:00</xmp:ModifyDate>
<xmp:MetadataDate>2026-10-02T22:01:01-04:00</xmp:MetadataDate>
<xmpMM:DocumentID>uuid:AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA</xmpMM:DocumentID>
<xmpMM:InstanceID>uuid:BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB</xmpMM:InstanceID>
trailer << /ID [<CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC><DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD>] >>
%%EOF
"""
        self.assertEqual(
            fp.normalize_publisher_pdf(left),
            fp.normalize_publisher_pdf(right),
        )

    def test_visible_payload_difference_is_not_erased(self):
        left = b"%PDF-1.7\nBT (RED CELL) Tj ET\n%%EOF\n"
        right = b"%PDF-1.7\nBT (WHITE CELL) Tj ET\n%%EOF\n"
        self.assertNotEqual(
            fp.normalize_publisher_pdf(left),
            fp.normalize_publisher_pdf(right),
        )

    def test_trailer_id_without_other_noise_normalizes(self):
        left = b"trailer << /Size 3 /ID [<0123456789ABCDEF0123456789ABCDEF><11111111111111111111111111111111>] >>"
        right = b"trailer << /Size 3 /ID [<FEDCBA9876543210FEDCBA9876543210><22222222222222222222222222222222>] >>"
        self.assertEqual(
            fp.normalize_publisher_pdf(left),
            fp.normalize_publisher_pdf(right),
        )

    def test_semantic_fingerprint_ignores_json_key_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            left = root / "left.json"
            right = root / "right.json"
            left.write_text(
                json.dumps({"r2c2": {"fill": "red", "bold": True}, "rows": 4}),
                encoding="utf-8",
            )
            right.write_text(
                '{"rows":4,"r2c2":{"bold":true,"fill":"red"}}',
                encoding="utf-8",
            )
            self.assertEqual(
                fp.semantic_fingerprint(left),
                fp.semantic_fingerprint(right),
            )


if __name__ == "__main__":
    unittest.main()
