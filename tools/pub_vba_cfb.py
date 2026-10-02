#!/usr/bin/env python3
from __future__ import annotations

import struct
from dataclasses import dataclass
from typing import Iterable

CFB_MAGIC = bytes.fromhex("D0CF11E0A1B11AE1")
FREESECT = 0xFFFFFFFF
ENDOFCHAIN = 0xFFFFFFFE
FATSECT = 0xFFFFFFFD
DIFSECT = 0xFFFFFFFC
NOSTREAM = 0xFFFFFFFF
MAX_CHAIN_STEPS = 1_000_000
MAX_VBA_STREAM_BYTES = 16 * 1024 * 1024
MAX_VBA_MODULES = 1024


class ScanError(Exception):
    pass


@dataclass(frozen=True)
class DirEntry:
    index: int
    name: str
    object_type: int
    left: int
    right: int
    child: int
    start_sector: int
    stream_size: int
    path: tuple[str, ...]


class CfbFile:
    def __init__(self, data: bytes):
        self.data = data
        if len(data) < 512 or data[:8] != CFB_MAGIC:
            raise ScanError("not_cfb")
        self.minor_version = _u16(data, 24)
        self.major_version = _u16(data, 26)
        if _u16(data, 28) != 0xFFFE:
            raise ScanError("unsupported_byte_order")
        sector_shift = _u16(data, 30)
        mini_sector_shift = _u16(data, 32)
        if self.major_version == 3 and sector_shift != 9:
            raise ScanError("invalid_v3_sector_shift")
        if self.major_version == 4 and sector_shift != 12:
            raise ScanError("invalid_v4_sector_shift")
        if self.major_version not in (3, 4):
            raise ScanError("unsupported_cfb_major_version")
        if mini_sector_shift != 6:
            raise ScanError("unsupported_mini_sector_shift")
        self.sector_size = 1 << sector_shift
        self.mini_sector_size = 1 << mini_sector_shift
        if len(data) < self.sector_size:
            raise ScanError("truncated_cfb_header_sector")
        self.num_fat_sectors = _u32(data, 44)
        self.first_dir_sector = _u32(data, 48)
        self.mini_stream_cutoff = _u32(data, 56)
        self.first_mini_fat_sector = _u32(data, 60)
        self.num_mini_fat_sectors = _u32(data, 64)
        self.first_difat_sector = _u32(data, 68)
        self.num_difat_sectors = _u32(data, 72)
        self.total_sectors = len(data) // self.sector_size - 1
        if self.total_sectors < 0:
            raise ScanError("truncated_cfb")
        self.difat = self._read_difat()
        self.fat = self._read_fat()
        self._dir_entries_raw = self._read_directory_entries()
        self.entries = self._build_paths()
        self._entry_by_path = {tuple(x.casefold() for x in e.path): e for e in self.entries}
        self.root = next((e for e in self.entries if e.index == 0), None)
        if self.root is None or self.root.object_type != 5:
            raise ScanError("missing_root_entry")
        self._mini_fat: list[int] | None = None
        self._root_mini_stream: bytes | None = None

    def _sector(self, sector_id: int) -> bytes:
        if sector_id >= self.total_sectors:
            raise ScanError("sector_out_of_bounds")
        start = (sector_id + 1) * self.sector_size
        end = start + self.sector_size
        if end > len(self.data):
            raise ScanError("truncated_sector")
        return self.data[start:end]

    def _read_difat(self) -> list[int]:
        difat = [x for x in struct.unpack_from("<109I", self.data, 76) if x != FREESECT]
        next_sector = self.first_difat_sector
        seen: set[int] = set()
        per_sector = self.sector_size // 4 - 1
        for _ in range(self.num_difat_sectors):
            if next_sector in (FREESECT, ENDOFCHAIN):
                raise ScanError("short_difat_chain")
            if next_sector in seen:
                raise ScanError("difat_cycle")
            seen.add(next_sector)
            block = self._sector(next_sector)
            values = struct.unpack_from(f"<{per_sector + 1}I", block, 0)
            difat.extend(x for x in values[:per_sector] if x != FREESECT)
            next_sector = values[-1]
        if len(difat) < self.num_fat_sectors:
            raise ScanError("fat_sector_list_short")
        return difat[: self.num_fat_sectors]

    def _read_fat(self) -> list[int]:
        values: list[int] = []
        for sector_id in self.difat:
            if sector_id >= self.total_sectors:
                raise ScanError("fat_sector_out_of_bounds")
            block = self._sector(sector_id)
            values.extend(struct.unpack_from(f"<{self.sector_size // 4}I", block, 0))
        return values

    def _chain(self, start_sector: int, fat: list[int], *, max_steps: int | None = None) -> list[int]:
        if start_sector in (FREESECT, ENDOFCHAIN):
            return []
        limit = max_steps or min(MAX_CHAIN_STEPS, len(fat) + 1)
        out: list[int] = []
        seen: set[int] = set()
        current = start_sector
        for _ in range(limit):
            if current in seen:
                raise ScanError("sector_chain_cycle")
            seen.add(current)
            if current >= len(fat):
                raise ScanError("sector_chain_out_of_fat")
            out.append(current)
            nxt = fat[current]
            if nxt == ENDOFCHAIN:
                return out
            if nxt in (FREESECT, FATSECT, DIFSECT):
                raise ScanError("invalid_sector_chain_marker")
            current = nxt
        raise ScanError("sector_chain_too_long")

    def _read_regular_stream(self, start_sector: int, size: int) -> bytes:
        if size == 0:
            return b""
        chain = self._chain(start_sector, self.fat)
        needed = (size + self.sector_size - 1) // self.sector_size
        if len(chain) < needed:
            raise ScanError("stream_chain_short")
        raw = b"".join(self._sector(x) for x in chain[:needed])
        return raw[:size]

    def _read_directory_entries(self) -> list[dict]:
        chain = self._chain(self.first_dir_sector, self.fat)
        if not chain:
            raise ScanError("missing_directory_stream")
        raw = b"".join(self._sector(x) for x in chain)
        rows: list[dict] = []
        for index, off in enumerate(range(0, len(raw) - 127, 128)):
            row = raw[off : off + 128]
            name_len = _u16(row, 64)
            if name_len == 0:
                name = ""
            else:
                if name_len < 2 or name_len > 64 * 2 or name_len % 2:
                    raise ScanError("invalid_directory_name_length")
                name = row[: name_len - 2].decode("utf-16le", errors="replace")
            object_type = row[66]
            stream_size = _u64(row, 120)
            if self.major_version == 3 and object_type != 5:
                stream_size &= 0xFFFFFFFF
            rows.append(
                {
                    "index": index,
                    "name": name,
                    "object_type": object_type,
                    "left": _u32(row, 68),
                    "right": _u32(row, 72),
                    "child": _u32(row, 76),
                    "start_sector": _u32(row, 116),
                    "stream_size": stream_size,
                }
            )
        return rows

    def _build_paths(self) -> list[DirEntry]:
        raw = self._dir_entries_raw
        if not raw:
            raise ScanError("empty_directory")
        out: list[DirEntry] = []
        visited: set[int] = set()

        def emit(index: int, parent: tuple[str, ...]) -> None:
            if index == NOSTREAM:
                return
            if index >= len(raw):
                raise ScanError("directory_index_out_of_bounds")
            if index in visited:
                raise ScanError("directory_tree_cycle")
            visited.add(index)
            row = raw[index]
            emit(row["left"], parent)
            path = parent if index == 0 else parent + (row["name"],)
            out.append(DirEntry(path=path, **row))
            if row["object_type"] in (1, 5):
                emit(row["child"], path)
            emit(row["right"], parent)

        emit(0, ())
        return out

    def _load_mini_fat(self) -> list[int]:
        if self._mini_fat is not None:
            return self._mini_fat
        if self.num_mini_fat_sectors == 0 or self.first_mini_fat_sector in (FREESECT, ENDOFCHAIN):
            self._mini_fat = []
            return self._mini_fat
        chain = self._chain(self.first_mini_fat_sector, self.fat, max_steps=self.num_mini_fat_sectors + 1)
        if len(chain) < self.num_mini_fat_sectors:
            raise ScanError("short_mini_fat_chain")
        raw = b"".join(self._sector(x) for x in chain[: self.num_mini_fat_sectors])
        self._mini_fat = list(struct.unpack_from(f"<{len(raw) // 4}I", raw, 0))
        return self._mini_fat

    def _load_root_mini_stream(self) -> bytes:
        if self._root_mini_stream is None:
            self._root_mini_stream = self._read_regular_stream(self.root.start_sector, self.root.stream_size)
        return self._root_mini_stream

    def read_stream(self, entry: DirEntry, *, max_bytes: int = MAX_VBA_STREAM_BYTES) -> bytes:
        if entry.object_type != 2:
            raise ScanError("not_a_stream")
        if entry.stream_size > max_bytes:
            raise ScanError("stream_too_large")
        if entry.stream_size == 0:
            return b""
        if entry.stream_size >= self.mini_stream_cutoff:
            return self._read_regular_stream(entry.start_sector, entry.stream_size)
        mini_fat = self._load_mini_fat()
        if not mini_fat:
            raise ScanError("mini_fat_missing")
        mini_stream = self._load_root_mini_stream()
        chain = self._chain(entry.start_sector, mini_fat)
        needed = (entry.stream_size + self.mini_sector_size - 1) // self.mini_sector_size
        if len(chain) < needed:
            raise ScanError("mini_stream_chain_short")
        chunks = []
        for mini_sector in chain[:needed]:
            start = mini_sector * self.mini_sector_size
            end = start + self.mini_sector_size
            if end > len(mini_stream):
                raise ScanError("mini_sector_out_of_bounds")
            chunks.append(mini_stream[start:end])
        return b"".join(chunks)[: entry.stream_size]

    def entry(self, path: Iterable[str]) -> DirEntry | None:
        return self._entry_by_path.get(tuple(x.casefold() for x in path))


def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def _u64(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def vba_decompress(container: bytes, *, max_output: int = MAX_VBA_STREAM_BYTES) -> bytes:
    if not container or container[0] != 0x01:
        raise ScanError("vba_compressed_container_signature")
    pos = 1
    output = bytearray()
    while pos < len(container):
        if pos + 2 > len(container):
            raise ScanError("vba_chunk_header_truncated")
        header = _u16(container, pos)
        signature = (header >> 12) & 0x7
        flag = (header >> 15) & 0x1
        if signature != 0x3:
            raise ScanError("vba_chunk_signature")
        chunk_size = (header & 0x0FFF) + 3
        chunk_end = pos + chunk_size
        if chunk_end > len(container):
            raise ScanError("vba_chunk_truncated")
        pos += 2
        chunk_start_output = len(output)
        if flag == 0:
            if chunk_size != 4098:
                raise ScanError("vba_raw_chunk_size")
            payload = container[pos:chunk_end]
            if len(payload) != 4096:
                raise ScanError("vba_raw_chunk_size")
            output.extend(payload)
            pos = chunk_end
        else:
            while pos < chunk_end:
                flags = container[pos]
                pos += 1
                for bit in range(8):
                    if pos >= chunk_end:
                        break
                    if flags & (1 << bit):
                        if pos + 2 > chunk_end:
                            raise ScanError("vba_copy_token_truncated")
                        token = _u16(container, pos)
                        pos += 2
                        decompressed_in_chunk = len(output) - chunk_start_output
                        if decompressed_in_chunk <= 0:
                            raise ScanError("vba_copy_token_before_literal")
                        offset_bits = max(4, (decompressed_in_chunk - 1).bit_length())
                        offset_bits = min(offset_bits, 12)
                        length_bits = 16 - offset_bits
                        length_mask = (1 << length_bits) - 1
                        length = (token & length_mask) + 3
                        offset = (token >> length_bits) + 1
                        if offset > decompressed_in_chunk:
                            raise ScanError("vba_copy_token_offset")
                        for _ in range(length):
                            output.append(output[-offset])
                            if len(output) > max_output:
                                raise ScanError("vba_decompressed_too_large")
                    else:
                        output.append(container[pos])
                        pos += 1
                        if len(output) > max_output:
                            raise ScanError("vba_decompressed_too_large")
        if len(output) - chunk_start_output > 4096:
            raise ScanError("vba_chunk_output_too_large")
    return bytes(output)


def _record_hits(data: bytes, record_id: int, *, min_size: int = 0, max_size: int = 1 << 20):
    needle = struct.pack("<H", record_id)
    start = 0
    while True:
        pos = data.find(needle, start)
        if pos < 0:
            return
        if pos + 6 <= len(data):
            size = _u32(data, pos + 2)
            end = pos + 6 + size
            if min_size <= size <= max_size and end <= len(data):
                yield pos, size, data[pos + 6 : end]
        start = pos + 1


def _sized_record(data: bytes, pos: int, expected_id: int, *, max_size: int) -> tuple[bytes, int]:
    if pos + 6 > len(data) or _u16(data, pos) != expected_id:
        raise ScanError(f"vba_dir_expected_record_{expected_id:04x}")
    size = _u32(data, pos + 2)
    if size > max_size or pos + 6 + size > len(data):
        raise ScanError(f"vba_dir_record_{expected_id:04x}_size")
    return data[pos + 6 : pos + 6 + size], pos + 6 + size


def _unicode_tail(data: bytes, pos: int, reserved_id: int, *, max_size: int) -> int:
    if pos + 6 > len(data) or _u16(data, pos) != reserved_id:
        raise ScanError(f"vba_dir_expected_reserved_{reserved_id:04x}")
    size = _u32(data, pos + 2)
    if size > max_size or size % 2 or pos + 6 + size > len(data):
        raise ScanError(f"vba_dir_unicode_{reserved_id:04x}_size")
    return pos + 6 + size


def _parse_module_record(data: bytes, pos: int) -> tuple[dict, int]:
    _, pos = _sized_record(data, pos, 0x0019, max_size=1024)
    if pos + 2 <= len(data) and _u16(data, pos) == 0x0047:
        _, pos = _sized_record(data, pos, 0x0047, max_size=2048)

    stream_name, pos = _sized_record(data, pos, 0x001A, max_size=1024)
    pos = _unicode_tail(data, pos, 0x0032, max_size=2048)

    _, pos = _sized_record(data, pos, 0x001C, max_size=4096)
    pos = _unicode_tail(data, pos, 0x0048, max_size=8192)

    offset_payload, pos = _sized_record(data, pos, 0x0031, max_size=4)
    if len(offset_payload) != 4:
        raise ScanError("vba_dir_module_offset_size")
    text_offset = _u32(offset_payload, 0)

    help_payload, pos = _sized_record(data, pos, 0x001E, max_size=4)
    if len(help_payload) != 4:
        raise ScanError("vba_dir_module_help_size")
    cookie_payload, pos = _sized_record(data, pos, 0x002C, max_size=2)
    if len(cookie_payload) != 2:
        raise ScanError("vba_dir_module_cookie_size")

    if pos + 6 > len(data) or _u16(data, pos) not in (0x0021, 0x0022):
        raise ScanError("vba_dir_module_type")
    if _u32(data, pos + 2) != 0:
        raise ScanError("vba_dir_module_type_reserved")
    pos += 6

    for optional_id in (0x0025, 0x0028):
        if pos + 2 <= len(data) and _u16(data, pos) == optional_id:
            if pos + 6 > len(data) or _u32(data, pos + 2) != 0:
                raise ScanError(f"vba_dir_module_optional_{optional_id:04x}")
            pos += 6

    if pos + 6 > len(data) or _u16(data, pos) != 0x002B or _u32(data, pos + 2) != 0:
        raise ScanError("vba_dir_module_terminator")
    pos += 6
    return {"stream_name_bytes": stream_name, "text_offset": text_offset}, pos


def parse_vba_dir_metadata(data: bytes) -> dict:
    codepage = None
    for _, _, payload in _record_hits(data, 0x0003, min_size=2, max_size=2):
        codepage = _u16(payload, 0)
        break

    needle = struct.pack("<H", 0x000F)
    search = 0
    last_error = None
    while True:
        pos = data.find(needle, search)
        if pos < 0:
            break
        search = pos + 1
        try:
            count_payload, cursor = _sized_record(data, pos, 0x000F, max_size=2)
            if len(count_payload) != 2:
                raise ScanError("vba_dir_module_count_size")
            module_count = _u16(count_payload, 0)
            if module_count > MAX_VBA_MODULES:
                raise ScanError("vba_dir_module_count_limit")
            cookie, cursor = _sized_record(data, cursor, 0x0013, max_size=2)
            if len(cookie) != 2:
                raise ScanError("vba_dir_project_cookie_size")
            modules = []
            for _ in range(module_count):
                module, cursor = _parse_module_record(data, cursor)
                modules.append(module)
            return {
                "parse_status": "parsed",
                "codepage": codepage,
                "module_count_declared": module_count,
                "modules": modules,
            }
        except ScanError as exc:
            last_error = str(exc)

    return {
        "parse_status": "not_parsed" if last_error is None else f"not_parsed:{last_error}",
        "codepage": codepage,
        "module_count_declared": None,
        "modules": [],
    }


def codec_for_codepage(codepage: int | None) -> str:
    if codepage is None:
        return "latin1"
    aliases = {65001: "utf-8", 1200: "utf-16le", 1201: "utf-16be"}
    return aliases.get(codepage, f"cp{codepage}")


def decode_codepage(data: bytes, codepage: int | None) -> str:
    codec = codec_for_codepage(codepage)
    try:
        return data.decode(codec, errors="replace")
    except LookupError:
        return data.decode("latin1", errors="replace")
