#!/usr/bin/env python3
import argparse
import collections
import hashlib
import html
import json
import quopri
import re
import urllib.request
from pathlib import Path

UA = "chaptera-publisher-hidden-xml-harvester/1"

KNOWN_DESCRIPTOR_TO_WIRE = {
    0x00: 0x01,  # bounded: materialized boolean TRUE presence marker
    0x04: 0x04,
    0x11: 0x11,
    0x13: 0x13,
    0x18: 0x18,
}

PUB_BLOCK_RE = re.compile(
    r"<!--\s*\[if\s+pub\]\s*>(.*?)<!\s*\[endif\]\s*-->",
    re.IGNORECASE | re.DOTALL,
)
TOKEN_RE = re.compile(
    r"<\s*(/?)\s*b:([A-Za-z0-9_.-]+)([^>]*)>",
    re.IGNORECASE | re.DOTALL,
)
ATTR_RE = re.compile(
    r"([A-Za-z_][A-Za-z0-9_.:-]*)\s*=\s*(['\"])(.*?)\2",
    re.DOTALL,
)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def decode_transport(data: bytes) -> str:
    # Publisher-era HTML is frequently MIME quoted-printable, even when served with
    # misleading extensions/content-types. Decode once; plain HTML remains stable.
    raw = quopri.decodestring(data)
    for encoding in ("utf-8", "cp1252", "latin-1"):
        try:
            return raw.decode(encoding)
        except UnicodeDecodeError:
            pass
    return raw.decode("utf-8", "replace")


def normalize_block(block: str) -> str:
    block = html.unescape(block)
    block = re.sub(r"\s+", " ", block)
    block = re.sub(r"\s*=\s*", "=", block)
    return block.strip()


def parse_attrs(src: str):
    return {m.group(1): html.unescape(m.group(3)) for m in ATTR_RE.finditer(src)}


def parse_priv(raw: str):
    try:
        value = int(raw, 16)
    except Exception:
        return None
    opyid = value >> 8
    dtype = value & 0xFF
    wire_type = KNOWN_DESCRIPTOR_TO_WIRE.get(dtype)
    raw_tag = ((wire_type << 11) | opyid) if wire_type is not None else None
    return {
        "raw": raw.upper(),
        "value": value,
        "opyid": opyid,
        "descriptor_type": dtype,
        "wire_type": wire_type,
        "raw_tag": raw_tag,
        "raw_tag_hex": f"{raw_tag:04X}" if raw_tag is not None else None,
    }


def parse_block(block: str):
    observations = []
    stack = []
    matches = list(TOKEN_RE.finditer(block))
    for idx, m in enumerate(matches):
        closing = bool(m.group(1))
        name = m.group(2)
        attrs = parse_attrs(m.group(3))
        if closing:
            if stack:
                stack.pop()
            continue

        parent = stack[-1]["name"] if stack else None
        record = {
            "name": name,
            "parent": parent,
            "depth": len(stack),
            "type": attrs.get("type"),
            "oty": attrs.get("oty"),
            "oh": attrs.get("oh"),
            "priv": attrs.get("priv"),
        }
        if record["priv"]:
            record["priv_decoded"] = parse_priv(record["priv"])

        tail_start = m.end()
        tail_end = matches[idx + 1].start() if idx + 1 < len(matches) else len(block)
        tail = html.unescape(block[tail_start:tail_end])
        tail = re.sub(r"<[^>]+>", "", tail)
        tail = re.sub(r"\s+", " ", tail).strip()
        if tail:
            record["value"] = tail[:512]
        observations.append(record)

        # Push unless this is self-closing.
        if not m.group(3).rstrip().endswith("/"):
            stack.append({"name": name})

    return observations


def fetch(url: str):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        data = r.read()
        return {
            "url": url,
            "status": int(getattr(r, "status", 200)),
            "content_type": r.headers.get("Content-Type"),
            "size": len(data),
            "sha256": sha256_bytes(data),
            "data": data,
        }


def harvest_source(source):
    url = source["url"]
    meta = {"url": url, "label": source.get("label")}
    try:
        fetched = fetch(url)
        meta.update({
            "http_status": fetched["status"],
            "content_type": fetched["content_type"],
            "source_size": fetched["size"],
            "source_sha256": fetched["sha256"],
        })
        text = decode_transport(fetched["data"])
    except Exception as exc:
        meta.update({
            "error": f"{type(exc).__name__}: {exc}",
            "block_count": 0,
            "observation_count": 0,
        })
        return meta, [], []

    blocks = []
    observations = []
    for block in PUB_BLOCK_RE.findall(text):
        norm = normalize_block(block)
        if "b:" not in norm:
            continue
        block_hash = sha256_bytes(norm.encode("utf-8"))
        parsed = parse_block(block)
        blocks.append({
            "block_sha256": block_hash,
            "normalized_size": len(norm.encode("utf-8")),
            "observation_count": len(parsed),
        })
        for rec in parsed:
            rec = dict(rec)
            rec["source_url"] = url
            rec["block_sha256"] = block_hash
            observations.append(rec)

    meta["block_count"] = len(blocks)
    meta["observation_count"] = len(observations)
    return meta, blocks, observations


def build_result(manifest):
    source_rows = []
    all_blocks = []
    observations = []
    seen_blocks = set()

    for src in manifest["sources"]:
        meta, blocks, obs = harvest_source(src)
        source_rows.append(meta)
        for block in blocks:
            if block["block_sha256"] not in seen_blocks:
                seen_blocks.add(block["block_sha256"])
                all_blocks.append(block)
        observations.extend(obs)

    class_counter = collections.Counter()
    property_counter = collections.Counter()
    priv_counter = collections.Counter()
    type_counter = collections.Counter()

    for r in observations:
        if r.get("type"):
            class_counter[r["type"]] += 1
        property_counter[r["name"]] += 1
        if r.get("priv"):
            priv_counter[(r.get("parent") or "", r["name"], r["priv"].upper())] += 1
        if r.get("priv_decoded"):
            type_counter[r["priv_decoded"]["descriptor_type"]] += 1

    unique_priv = [
        {
            "parent": parent or None,
            "name": name,
            "priv": priv,
            "count": count,
            "decoded": parse_priv(priv),
        }
        for (parent, name, priv), count in sorted(priv_counter.items())
    ]

    return {
        "schema": "publisher-hidden-xml-harvest.v1",
        "source_count": len(source_rows),
        "sources_ok": sum(1 for s in source_rows if not s.get("error")),
        "sources_with_pub_blocks": sum(1 for s in source_rows if s.get("block_count", 0) > 0),
        "unique_block_count": len(all_blocks),
        "observation_count": len(observations),
        "distinct_type_names": len(class_counter),
        "distinct_property_names": len(property_counter),
        "distinct_priv_coordinates": len(unique_priv),
        "sources": source_rows,
        "blocks": all_blocks,
        "type_names": [
            {"name": k, "count": v} for k, v in class_counter.most_common()
        ],
        "property_names": [
            {"name": k, "count": v} for k, v in property_counter.most_common()
        ],
        "descriptor_types": [
            {"descriptor_type": k, "count": v}
            for k, v in sorted(type_counter.items())
        ],
        "priv_coordinates": unique_priv,
        "observations": observations,
    }


def self_test():
    sample = b'''MIME-Version: 1.0\n<!--[if pub]><b:otyEscherText type=3D"OplPo" oty=3D"1" oh=3D"12"><b:FUserChangedFmt priv=3D"200">True</b:FUserChangedFmt><b:OplLastFmt type=3D"OplLastFmt"><b:PoFormatting type=3D"OplOdpo"><b:GroupShape type=3D"OplOdpoGroupShape" priv=3D"E13"></b:GroupShape></b:PoFormatting></b:OplLastFmt></b:otyEscherText><![endif]-->'''
    text = decode_transport(sample)
    blocks = PUB_BLOCK_RE.findall(text)
    assert len(blocks) == 1
    obs = parse_block(blocks[0])
    by_name = {r["name"]: r for r in obs}
    assert by_name["otyEscherText"]["type"] == "OplPo"
    assert by_name["FUserChangedFmt"]["priv_decoded"]["opyid"] == 2
    assert by_name["FUserChangedFmt"]["priv_decoded"]["descriptor_type"] == 0
    assert by_name["GroupShape"]["priv_decoded"]["raw_tag_hex"] == "980E"
    print(json.dumps({"self_test": "PASS", "observation_count": len(obs)}))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("manifest", nargs="?")
    ap.add_argument("output", nargs="?")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.manifest or not args.output:
        ap.error("manifest and output are required unless --self-test is used")

    manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
    result = build_result(manifest)
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    Path(args.output).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({
        k: result[k]
        for k in (
            "schema", "source_count", "sources_ok", "sources_with_pub_blocks",
            "unique_block_count", "observation_count", "distinct_type_names",
            "distinct_property_names", "distinct_priv_coordinates"
        )
    }, indent=2))


if __name__ == "__main__":
    main()
