#!/usr/bin/env python3
import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools.corpus.cfb_physical import CFB, END, FAT, FREE, NO, SIG, sha256_bytes
from tools.rescue_cfb_stream_export import export_file
from tools.verify_rescue_cfb_stream_export import verify_files


def collision_fixture() -> bytes:
    sector_size = 512
    fat_sector = 17
    sector_count = 18
    data = bytearray(sector_size * (sector_count + 1))
    data[:8] = SIG

    for offset, value, fmt in [
        (24, 0x003E, "H"),
        (26, 3, "H"),
        (28, 0xFFFE, "H"),
        (30, 9, "H"),
        (32, 6, "H"),
        (40, 0, "I"),
        (44, 1, "I"),
        (48, 0, "I"),
        (52, 0, "I"),
        (56, 4096, "I"),
        (60, END, "I"),
        (64, 0, "I"),
        (68, END, "I"),
        (72, 0, "I"),
    ]:
        struct.pack_into("<" + fmt, data, offset, value)

    for index in range(109):
        struct.pack_into("<I", data, 76 + 4 * index, FREE)
    struct.pack_into("<I", data, 76, fat_sector)

    def directory_entry(
        sid: int,
        name: str,
        entry_type: int,
        *,
        child: int = NO,
        left: int = NO,
        right: int = NO,
        start: int = END,
        size: int = 0,
    ) -> None:
        offset = sector_size + sid * 128
        encoded = (name + "\0").encode("utf-16le")
        data[offset : offset + len(encoded)] = encoded
        struct.pack_into("<H", data, offset + 64, len(encoded))
        data[offset + 66] = entry_type
        data[offset + 67] = 1
        struct.pack_into("<I", data, offset + 68, left)
        struct.pack_into("<I", data, offset + 72, right)
        struct.pack_into("<I", data, offset + 76, child)
        struct.pack_into("<I", data, offset + 116, start)
        struct.pack_into("<Q", data, offset + 120, size)

    directory_entry(0, "Root Entry", 5, child=1)
    directory_entry(1, "Contents", 2, right=2, start=1, size=4096)
    directory_entry(2, "CONTENTS", 2, start=9, size=4096)

    payload_a = bytes((index * 3 + 7) % 251 for index in range(4096))
    payload_b = bytes((index * 5 + 11) % 251 for index in range(4096))
    for chunk_index, sector in enumerate(range(1, 9)):
        start = (sector + 1) * sector_size
        data[start : start + sector_size] = payload_a[
            chunk_index * sector_size : (chunk_index + 1) * sector_size
        ]
    for chunk_index, sector in enumerate(range(9, 17)):
        start = (sector + 1) * sector_size
        data[start : start + sector_size] = payload_b[
            chunk_index * sector_size : (chunk_index + 1) * sector_size
        ]

    fat = [FREE] * 128
    fat[0] = END
    for sector in range(1, 8):
        fat[sector] = sector + 1
    fat[8] = END
    for sector in range(9, 16):
        fat[sector] = sector + 1
    fat[16] = END
    fat[fat_sector] = FAT
    struct.pack_into(
        "<" + "I" * 128,
        data,
        (fat_sector + 1) * sector_size,
        *fat,
    )
    return bytes(data)


class RescueCfbStreamIdentityTests(unittest.TestCase):
    def test_case_colliding_names_remain_distinct_by_sid(self):
        source = collision_fixture()
        cfb = CFB(source)

        one = cfb.stream_descriptor_by_sid(1)
        two = cfb.stream_descriptor_by_sid(2)

        self.assertEqual(one["name"], "Contents")
        self.assertEqual(two["name"], "CONTENTS")
        self.assertEqual(one["sid"], 1)
        self.assertEqual(two["sid"], 2)
        self.assertEqual(one["payload_byte_len"], 4096)
        self.assertEqual(two["payload_byte_len"], 4096)
        self.assertNotEqual(one["payload_sha256"], two["payload_sha256"])
        self.assertEqual(len(one["physical_ranges"]), 8)
        self.assertEqual(len(two["physical_ranges"]), 8)
        self.assertNotEqual(
            cfb.read_stream_by_sid(1),
            cfb.read_stream_by_sid(2),
        )

    def test_export_and_independent_verify_preserve_both_streams(self):
        source_bytes = collision_fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "ContentsCaseCollision.pub"
            artifacts = root / "artifacts"
            manifest_path = root / "manifest.json"
            source.write_bytes(source_bytes)

            manifest = export_file(source, artifacts, manifest_path)
            receipt = verify_files(source, artifacts, manifest_path)

            self.assertEqual(manifest["source"]["sha256"], sha256_bytes(source_bytes))
            self.assertEqual(manifest["artifact_count"], 2)
            self.assertEqual(receipt["verified_stream_count"], 2)
            self.assertTrue(receipt["pass"])

            streams = {item["sid"]: item for item in manifest["streams"]}
            self.assertEqual(set(streams), {1, 2})
            self.assertTrue(streams[1]["artifact_path"].startswith("sid-000001-"))
            self.assertTrue(streams[2]["artifact_path"].startswith("sid-000002-"))
            # Both descriptive names sanitize to the same suffix. SID is what
            # keeps the artifact identity collision-safe.
            self.assertNotEqual(
                streams[1]["artifact_path"],
                streams[2]["artifact_path"],
            )
            self.assertNotEqual(
                streams[1]["payload_sha256"],
                streams[2]["payload_sha256"],
            )
            self.assertEqual(source.read_bytes(), source_bytes)

    def test_verifier_fails_closed_on_artifact_tamper(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "ContentsCaseCollision.pub"
            artifacts = root / "artifacts"
            manifest_path = root / "manifest.json"
            source.write_bytes(collision_fixture())

            manifest = export_file(source, artifacts, manifest_path)
            first = artifacts / manifest["streams"][0]["artifact_path"]
            payload = bytearray(first.read_bytes())
            payload[0] ^= 0xFF
            first.write_bytes(payload)

            with self.assertRaisesRegex(AssertionError, "artifact bytes differ"):
                verify_files(source, artifacts, manifest_path)

    def test_invalid_sid_fails_typed_before_path_lookup(self):
        cfb = CFB(collision_fixture())
        with self.assertRaisesRegex(ValueError, "directory SID range"):
            cfb.read_stream_by_sid(999)


if __name__ == "__main__":
    unittest.main()
