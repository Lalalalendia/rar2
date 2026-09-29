#!/usr/bin/env python3
"""Reusable physical CFB parser for recovery/corpus tooling.

Identity law:
    physical stream identity = (source_sha256, directory_entry_sid)

Logical path/name is descriptive metadata only. This module deliberately does
not use olefile and does not reconstruct a logical storage tree.
"""

from __future__ import annotations

import dataclasses
import hashlib
import struct
from typing import Any

SIG = bytes.fromhex("d0cf11e0a1b11ae1")
FREE = 0xFFFFFFFF
END = 0xFFFFFFFE
FAT = 0xFFFFFFFD
DIF = 0xFFFFFFFC
NO = 0xFFFFFFFF
SPECIAL = {FREE, END, FAT, DIF}


def u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def u64(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# Compatibility alias used by cfb_physical_diff.py.
h = sha256_bytes


def decode_directory_name(raw: bytes, byte_len: int) -> str:
    if 2 <= byte_len <= 64 and byte_len % 2 == 0:
        return raw[: byte_len - 2].decode("utf-16le", errors="replace")
    return ""


# Compatibility alias used by older corpus code.
name = decode_directory_name


@dataclasses.dataclass(frozen=True)
class PhysicalRange:
    offset: int
    length: int

    def as_dict(self, source: bytes | None = None) -> dict[str, Any]:
        value: dict[str, Any] = {
            "offset": self.offset,
            "length": self.length,
        }
        if source is not None:
            value["sha256"] = sha256_bytes(
                source[self.offset : self.offset + self.length]
            )
        return value


class CFB:
    """Bounded physical Compound File Binary parser.

    The public dictionary shape intentionally preserves the older
    cfb_physical_diff.CFB surface while adding SID-keyed recovery APIs.
    """

    def __init__(self, data: bytes):
        self.b = data
        if len(data) < 512 or data[:8] != SIG:
            raise ValueError("not CFB")

        self.major = u16(data, 26)
        self.ss = 1 << u16(data, 30)
        self.ms = 1 << u16(data, 32)
        if self.ss not in (512, 4096) or self.ms != 64 or len(data) % self.ss:
            raise ValueError("unsupported/alignment")

        self.nsec = len(data) // self.ss - 1
        self.nfat = u32(data, 44)
        self.dir0 = u32(data, 48)
        self.cut = u32(data, 56)
        self.mini0 = u32(data, 60)
        self.nmini = u32(data, 64)
        self.dif0 = u32(data, 68)
        self.ndif = u32(data, 72)

        self.difsecs: list[int] = []
        self.fatsecs = self._difat()
        self.fat = self._table(self.fatsecs)
        self.dirsecs = self.chain(self.dir0, self.fat, "dir")
        self.minisecs = (
            self.chain(self.mini0, self.fat, "minifat") if self.nmini else []
        )
        if len(self.minisecs) != self.nmini:
            raise ValueError("MiniFAT count")

        self.minifat = self._table(self.minisecs)
        self.dirs = self._dirs()
        roots = [entry for entry in self.dirs if entry["type"] == 5]
        if not roots:
            raise ValueError("no root")
        self.root = roots[0]
        self.rootchain = (
            self.chain(self.root["start"], self.fat, "root")
            if self.root["size"]
            else []
        )

        # Kept for cfb_physical_diff byte-category compatibility.
        self.lab = ["unknown"] * len(data)
        self._labels()

    @property
    def source_sha256(self) -> str:
        return sha256_bytes(self.b)

    @property
    def source_byte_len(self) -> int:
        return len(self.b)

    def off(self, sector: int) -> int:
        if sector < 0 or sector >= self.nsec:
            raise ValueError(f"sector range {sector}")
        return (sector + 1) * self.ss

    def sec(self, sector: int) -> bytes:
        offset = self.off(sector)
        return self.b[offset : offset + self.ss]

    def _difat(self) -> list[int]:
        ids = [u32(self.b, 76 + 4 * i) for i in range(109)]
        ids = [value for value in ids if value != FREE]

        sector = self.dif0
        seen: set[int] = set()
        per_sector = self.ss // 4 - 1
        for _ in range(self.ndif):
            if sector in SPECIAL or sector in seen:
                raise ValueError("DIFAT chain")
            seen.add(sector)
            self.difsecs.append(sector)
            data = self.sec(sector)
            ids.extend(
                u32(data, 4 * i)
                for i in range(per_sector)
                if u32(data, 4 * i) != FREE
            )
            sector = u32(data, self.ss - 4)

        if len(ids) < self.nfat:
            raise ValueError("FAT ids")
        return ids[: self.nfat]

    def _table(self, sectors: list[int]) -> list[int]:
        values: list[int] = []
        for sector in sectors:
            values.extend(
                struct.unpack("<" + "I" * (self.ss // 4), self.sec(sector))
            )
        return values

    def chain(self, start: int, table: list[int], label: str) -> list[int]:
        if start in (FREE, END):
            return []

        out: list[int] = []
        seen: set[int] = set()
        bound = self.nsec if table is self.fat else len(table)
        sector = start
        while sector != END:
            if (
                sector in SPECIAL
                or sector in seen
                or sector >= bound
                or sector >= len(table)
            ):
                raise ValueError(label + " chain")
            seen.add(sector)
            out.append(sector)
            sector = table[sector]
            if len(out) > bound + 1:
                raise ValueError(label + " long")
        return out

    def _dirs(self) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        index = 0
        for sector in self.dirsecs:
            data = self.sec(sector)
            base = self.off(sector)
            for position in range(0, self.ss, 128):
                raw = data[position : position + 128]
                entry_type = raw[66]
                if entry_type not in (0, 1, 2, 5):
                    raise ValueError("dir type")
                low = u32(raw, 120)
                high = u32(raw, 124)
                effective = low if self.ss == 512 else low + (high << 32)
                out.append(
                    {
                        "i": index,
                        "name": decode_directory_name(raw[:64], u16(raw, 64)),
                        "type": entry_type,
                        "color": raw[67],
                        "left": u32(raw, 68),
                        "right": u32(raw, 72),
                        "child": u32(raw, 76),
                        "clsid": raw[80:96].hex(),
                        "state": u32(raw, 96),
                        "ctime": u64(raw, 100),
                        "mtime": u64(raw, 108),
                        "start": u32(raw, 116),
                        "size": effective,
                        "size_low": low,
                        "size_high": high,
                        "raw": base + position,
                    }
                )
                index += 1
        return out

    def directory_entry_by_sid(self, sid: int) -> dict[str, Any]:
        if sid < 0 or sid >= len(self.dirs):
            raise ValueError(f"directory SID range {sid}")
        entry = self.dirs[sid]
        if entry["i"] != sid:
            raise ValueError(f"directory SID mismatch {sid}")
        return entry

    def stream_entry_by_sid(self, sid: int) -> dict[str, Any]:
        entry = self.directory_entry_by_sid(sid)
        if entry["type"] != 2:
            raise ValueError(f"directory SID {sid} is not a stream")
        return entry

    def schain(self, entry: dict[str, Any]) -> list[int]:
        if not entry["size"]:
            return []
        table = (
            self.minifat
            if entry["type"] == 2 and entry["size"] < self.cut
            else self.fat
        )
        return self.chain(entry["start"], table, "stream " + entry["name"])

    def mark(self, start: int, end: int, label: str) -> None:
        for index in range(max(0, start), min(len(self.lab), end)):
            self.lab[index] = label

    def msec(self, sector: int, label: str) -> None:
        offset = self.off(sector)
        self.mark(offset, offset + self.ss, label)

    def rawmini(self, offset: int) -> int:
        sector_index, remainder = divmod(offset, self.ss)
        if sector_index >= len(self.rootchain):
            raise ValueError("mini root range")
        return self.off(self.rootchain[sector_index]) + remainder

    def stream_ranges_by_sid(self, sid: int) -> list[PhysicalRange]:
        entry = self.stream_entry_by_sid(sid)
        remaining = int(entry["size"])
        if remaining == 0:
            return []

        out: list[PhysicalRange] = []
        if entry["size"] >= self.cut:
            for sector in self.schain(entry):
                length = min(remaining, self.ss)
                out.append(PhysicalRange(self.off(sector), length))
                remaining -= length
        else:
            for mini_sector in self.schain(entry):
                length = min(remaining, self.ms)
                out.append(
                    PhysicalRange(self.rawmini(mini_sector * self.ms), length)
                )
                remaining -= length

        if remaining:
            raise ValueError(f"stream SID {sid} chain too short")
        return out

    def read_stream_by_sid(self, sid: int) -> bytes:
        entry = self.stream_entry_by_sid(sid)
        chunks = [
            self.b[item.offset : item.offset + item.length]
            for item in self.stream_ranges_by_sid(sid)
        ]
        payload = b"".join(chunks)
        if len(payload) != entry["size"]:
            raise ValueError(f"stream SID {sid} byte length mismatch")
        return payload

    def stream_descriptor_by_sid(self, sid: int) -> dict[str, Any]:
        entry = self.stream_entry_by_sid(sid)
        payload = self.read_stream_by_sid(sid)
        return {
            "source_sha256": self.source_sha256,
            "source_byte_len": self.source_byte_len,
            "sid": sid,
            "name": entry["name"],
            "declared_size": entry["size"],
            "storage": "minifat" if entry["size"] < self.cut else "fat",
            "chain": self.schain(entry),
            "physical_ranges": [
                item.as_dict(self.b) for item in self.stream_ranges_by_sid(sid)
            ],
            "payload_sha256": sha256_bytes(payload),
            "payload_byte_len": len(payload),
        }

    def _labels(self) -> None:
        self.mark(0, self.ss, "header_area")
        for start, end, label in [
            (0, 8, "signature"),
            (8, 24, "clsid"),
            (24, 26, "minor"),
            (26, 28, "major"),
            (28, 30, "byte_order"),
            (30, 32, "sector_shift"),
            (32, 34, "mini_shift"),
            (34, 40, "reserved"),
            (40, 44, "num_dir"),
            (44, 48, "num_fat"),
            (48, 52, "first_dir"),
            (52, 56, "transaction"),
            (56, 60, "mini_cutoff"),
            (60, 64, "first_minifat"),
            (64, 68, "num_minifat"),
            (68, 72, "first_difat"),
            (72, 76, "num_difat"),
            (76, 512, "difat_slots"),
        ]:
            self.mark(start, end, "header." + label)

        for sector in range(self.nsec):
            label = (
                "unallocated_sector"
                if sector >= len(self.fat) or self.fat[sector] == FREE
                else "allocated_other_sector"
            )
            self.msec(sector, label)

        for sector in self.difsecs:
            self.msec(sector, "difat_sector")
        for sector in self.fatsecs:
            self.msec(sector, "fat_sector")
        for sector in self.minisecs:
            self.msec(sector, "minifat_sector")
        for sector in self.dirsecs:
            self.msec(sector, "directory_sector")

        fields = [
            (0, 64, "name"),
            (64, 66, "name_length"),
            (66, 67, "type"),
            (67, 68, "color"),
            (68, 72, "left"),
            (72, 76, "right"),
            (76, 80, "child"),
            (80, 96, "clsid"),
            (96, 100, "state"),
            (100, 108, "ctime"),
            (108, 116, "mtime"),
            (116, 120, "start"),
            (120, 124, "size_low"),
            (124, 128, "size_high"),
        ]
        for entry in self.dirs:
            for start, end, label in fields:
                self.mark(
                    entry["raw"] + start,
                    entry["raw"] + end,
                    f"directory[{entry['i']}].{label}",
                )

        for sector in self.rootchain:
            self.msec(sector, "root_ministream_container")

        for entry in self.dirs:
            if entry["type"] != 2 or not entry["size"]:
                continue
            remaining = entry["size"]
            if entry["size"] >= self.cut:
                for sector in self.schain(entry):
                    offset = self.off(sector)
                    length = min(remaining, self.ss)
                    self.mark(
                        offset,
                        offset + length,
                        "stream_payload:" + entry["name"],
                    )
                    self.mark(
                        offset + length,
                        offset + self.ss,
                        "stream_slack:" + entry["name"],
                    )
                    remaining -= length
            else:
                for mini_sector in self.schain(entry):
                    offset = self.rawmini(mini_sector * self.ms)
                    length = min(remaining, self.ms)
                    self.mark(
                        offset,
                        offset + length,
                        "mini_stream_payload:" + entry["name"],
                    )
                    self.mark(
                        offset + length,
                        offset + self.ms,
                        "mini_stream_slack:" + entry["name"],
                    )
                    remaining -= length
            if remaining:
                raise ValueError("stream chain too short")

    def summary(self) -> dict[str, Any]:
        streams = []
        for entry in self.dirs:
            if entry["type"] == 2 and entry["size"]:
                streams.append(
                    {
                        "i": entry["i"],
                        "name": entry["name"],
                        "storage": (
                            "minifat" if entry["size"] < self.cut else "fat"
                        ),
                        "chain": self.schain(entry),
                        "size": entry["size"],
                    }
                )
        return {
            "sha256": self.source_sha256,
            "byte_len": self.source_byte_len,
            "major": self.major,
            "sector_size": self.ss,
            "mini_sector_size": self.ms,
            "fat_sector_ids": self.fatsecs,
            "difat_sector_ids": self.difsecs,
            "directory_sector_ids": self.dirsecs,
            "minifat_sector_ids": self.minisecs,
            "root_chain": self.rootchain,
            "streams": streams,
            "directory_entries": [
                {key: value for key, value in entry.items() if key != "raw"}
                for entry in self.dirs
                if entry["type"]
            ],
        }
