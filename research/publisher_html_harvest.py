#!/usr/bin/env python3
import argparse
import csv
import hashlib
import html
import json
import quopri
import re
import subprocess
import tempfile
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

USER_AGENT = "chaptera-publisher-html-harvester/1.0"
MAX_BYTES = 32 * 1024 * 1024
TAG_RE = re.compile(r"<\s*(/?)b:([A-Za-z_][\w.-]*)\b([^>]*)>", re.I | re.S)
ATTR_RE = re.compile(r"""([A-Za-z_:][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))""", re.S)
META_GENERATOR_RE = re.compile(r"""<meta\b[^>]*\bname\s*=\s*["']?generator["']?[^>]*\bcontent\s*=\s*["']([^"']+)["']""", re.I | re.S)
META_GENERATOR_RE_REV = re.compile(r"""<meta\b[^>]*\bcontent\s*=\s*["']([^"']+)["'][^>]*\bname\s*=\s*["']?generator["']?""", re.I | re.S)

def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()

def clean_value(value):
    value = re.sub(r"<[^>]+>", "", value)
    value = html.unescape(value).replace("\x00", "")
    return re.sub(r"\s+", " ", value).strip()

def parse_attrs(blob):
    attrs = {}
    for m in ATTR_RE.finditer(blob):
        value = m.group(2) if m.group(2) is not None else m.group(3)
        if value is None:
            value = m.group(4)
        attrs[m.group(1).lower()] = html.unescape(value or "")
    return attrs

def decode_candidates(raw, content_type):
    encodings = []
    if content_type:
        m = re.search(r"charset\s*=\s*([A-Za-z0-9._-]+)", content_type, re.I)
        if m:
            encodings.append(m.group(1))
    encodings.extend(["utf-8", "windows-1252", "latin-1"])
    text = None
    for encoding in dict.fromkeys(encodings):
        try:
            text = raw.decode(encoding)
            break
        except (LookupError, UnicodeDecodeError):
            pass
    if text is None:
        text = raw.decode("utf-8", "replace")
    candidates = [("plain", text)]
    if "&lt;" in text or "&#60;" in text:
        candidates.append(("html_unescape", html.unescape(text)))
    if "quoted-printable" in text.lower() or text.count("=3D") >= 3 or text.count("=\n") + text.count("=\r\n") >= 3:
        qp = quopri.decodestring(text.encode("latin-1", "replace")).decode("utf-8", "replace")
        candidates.append(("quoted_printable", qp))
        if "&lt;" in qp or "&#60;" in qp:
            candidates.append(("quoted_printable_html_unescape", html.unescape(qp)))
    return candidates

def pdf_text_candidates(raw):
    with tempfile.TemporaryDirectory() as td:
        src = Path(td) / "source.pdf"
        dst = Path(td) / "source.txt"
        src.write_bytes(raw)
        subprocess.run(
            ["pdftotext", "-layout", "-enc", "UTF-8", str(src), str(dst)],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=60,
        )
        text = dst.read_text(encoding="utf-8", errors="replace")
    candidates = [("pdf_text", text)]
    if "&lt;" in text or "&#60;" in text:
        candidates.append(("pdf_text_html_unescape", html.unescape(text)))
    if "quoted-printable" in text.lower() or text.count("=3D") >= 3:
        qp = quopri.decodestring(text.encode("latin-1", "replace")).decode("utf-8", "replace")
        candidates.append(("pdf_text_quoted_printable", qp))
        if "&lt;" in qp or "&#60;" in qp:
            candidates.append(("pdf_text_quoted_printable_html_unescape", html.unescape(qp)))
    return candidates

def score_candidate(text):
    return (
        25 * len(re.findall(r"\bpriv\s*=", text, re.I))
        + 10 * len(re.findall(r"\btype\s*=\s*[\"']?Opl", text, re.I))
        + 4 * text.lower().count("otyeschertext")
    )

def fetch_source(url):
    req = urllib.request.Request(url, headers={
        "User-Agent": USER_AGENT,
        "Accept": "text/html,text/plain,application/xhtml+xml,*/*;q=0.8",
    })
    with urllib.request.urlopen(req, timeout=45) as response:
        data = response.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise ValueError("source exceeds size limit")
        return data, {
            "status": int(getattr(response, "status", 200)),
            "final_url": response.geturl(),
            "content_type": response.headers.get("Content-Type"),
        }

def priv_decode(priv, value):
    raw = priv.strip()
    if raw.lower().startswith("0x"):
        raw = raw[2:]
    if not re.fullmatch(r"[0-9A-Fa-f]+", raw):
        return {"priv": priv, "priv_u32": None, "opyid": None, "descriptor_type": None, "wire_type": None, "raw_tag": None}
    packed = int(raw, 16)
    opyid = packed >> 8
    dtype = packed & 0xFF
    lower = value.strip().lower()
    if dtype == 0 and lower in ("true", "1", "yes"):
        wire_type = 1
    elif dtype == 0:
        wire_type = None
    else:
        wire_type = dtype
    raw_tag = ((wire_type << 11) | opyid) if wire_type is not None else None
    return {
        "priv": priv,
        "priv_u32": packed,
        "opyid": opyid,
        "descriptor_type": dtype,
        "wire_type": wire_type,
        "raw_tag": raw_tag,
    }

def nearest_owner(stack):
    for node in reversed(stack):
        attrs = node["attrs"]
        if "type" in attrs or "oty" in attrs or "oh" in attrs:
            return node
    return None

def parse_publisher_xml(text, source_id):
    stack = []
    objects = []
    properties = []
    object_seq = 0
    property_seq = 0
    for m in TAG_RE.finditer(text):
        closing = bool(m.group(1))
        tag = m.group(2)
        attrs = parse_attrs(m.group(3))
        if not closing:
            node = {"tag": tag, "attrs": attrs, "open_end": m.end(), "source_id": source_id}
            owner_before = nearest_owner(stack)
            if "type" in attrs or "oty" in attrs or "oh" in attrs:
                object_seq += 1
                node["object_index"] = object_seq
                objects.append({
                    "source_id": source_id,
                    "object_index": object_seq,
                    "tag": tag,
                    "type": attrs.get("type"),
                    "oty": attrs.get("oty"),
                    "oh": attrs.get("oh"),
                    "priv": attrs.get("priv"),
                    "parent_object_index": owner_before.get("object_index") if owner_before else None,
                    "parent_type": owner_before["attrs"].get("type") if owner_before else None,
                })
            stack.append(node)
            continue

        match_index = None
        for i in range(len(stack) - 1, -1, -1):
            if stack[i]["tag"].lower() == tag.lower():
                match_index = i
                break
        if match_index is None:
            continue
        node = stack[match_index]
        del stack[match_index:]
        attrs = node["attrs"]
        if "priv" not in attrs:
            continue
        property_seq += 1
        value = clean_value(text[node["open_end"]:m.start()])
        owner = nearest_owner(stack)
        decoded = priv_decode(attrs["priv"], value)
        properties.append({
            "source_id": source_id,
            "property_index": property_seq,
            "owner_object_index": owner.get("object_index") if owner else None,
            "owner_tag": owner["tag"] if owner else None,
            "owner_type": owner["attrs"].get("type") if owner else None,
            "owner_oty": owner["attrs"].get("oty") if owner else None,
            "owner_oh": owner["attrs"].get("oh") if owner else None,
            "name": tag,
            "value": value,
            **decoded,
        })
    return objects, properties

def detect_generator(text):
    m = META_GENERATOR_RE.search(text) or META_GENERATOR_RE_REV.search(text)
    return clean_value(m.group(1)) if m else None

def payload_hash(objects, properties):
    canonical = {
        "objects": [{k: o.get(k) for k in ("tag","type","oty","oh","priv","parent_object_index","parent_type")} for o in objects],
        "properties": [{k: p.get(k) for k in ("owner_type","owner_oty","owner_oh","name","priv","value","opyid","descriptor_type","wire_type","raw_tag")} for p in properties],
    }
    return sha256_bytes(json.dumps(canonical, ensure_ascii=False, sort_keys=True).encode("utf-8"))

def write_tsv(path, rows, fields):
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields, delimiter="\t", extrasaction="ignore")
        writer.writeheader()
        writer.writerows(rows)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    seeds = json.loads(Path(args.seeds).read_text(encoding="utf-8"))

    documents = []
    all_objects = []
    all_properties = []
    first_payload_source = {}

    for index, seed in enumerate(seeds, 1):
        source_id = seed.get("id") or "source-%03d" % index
        doc = {"source_id": source_id, "url": seed["url"], "note": seed.get("note"), "status": "error"}
        try:
            raw, receipt = fetch_source(seed["url"])
            candidates = decode_candidates(raw, receipt.get("content_type"))
            if raw.startswith(b"%PDF") or "application/pdf" in (receipt.get("content_type") or "").lower():
                candidates.extend(pdf_text_candidates(raw))
            mode, text = max(candidates, key=lambda item: score_candidate(item[1]))
            objects, properties = parse_publisher_xml(text, source_id)
            phash = payload_hash(objects, properties)
            duplicate_of = first_payload_source.get(phash)
            if duplicate_of is None:
                first_payload_source[phash] = source_id
            doc.update({
                "status": "ok",
                "http_status": receipt.get("status"),
                "final_url": receipt.get("final_url"),
                "content_type": receipt.get("content_type"),
                "raw_size": len(raw),
                "raw_sha256": sha256_bytes(raw),
                "decode_mode": mode,
                "generator": detect_generator(text),
                "publisher_marker_score": score_candidate(text),
                "object_count": len(objects),
                "property_count": len(properties),
                "payload_sha256": phash,
                "duplicate_of": duplicate_of,
            })
            all_objects.extend(objects)
            all_properties.extend(properties)
        except Exception as exc:
            doc["error"] = "%s: %s" % (type(exc).__name__, exc)
        documents.append(doc)

    aggregate = defaultdict(lambda: {"source_ids": set(), "observations": 0, "values": Counter(), "oty_values": Counter()})
    for prop in all_properties:
        key = (prop.get("owner_type") or "", prop.get("name") or "", prop.get("priv") or "", prop.get("opyid"), prop.get("descriptor_type"), prop.get("wire_type"), prop.get("raw_tag"))
        rec = aggregate[key]
        rec["source_ids"].add(prop["source_id"])
        rec["observations"] += 1
        if prop.get("value"):
            rec["values"][prop["value"]] += 1
        if prop.get("owner_oty"):
            rec["oty_values"][prop["owner_oty"]] += 1

    registry = []
    for key, rec in aggregate.items():
        owner_type, name, priv, opyid, dtype, wire_type, raw_tag = key
        registry.append({
            "owner_type": owner_type or None,
            "property_name": name,
            "priv": priv,
            "opyid": opyid,
            "descriptor_type": dtype,
            "wire_type": wire_type,
            "raw_tag": raw_tag,
            "source_count": len(rec["source_ids"]),
            "observations": rec["observations"],
            "source_ids": sorted(rec["source_ids"]),
            "common_values": [{"value": v, "count": c} for v, c in rec["values"].most_common(5)],
            "owner_oty_values": [{"oty": v, "count": c} for v, c in rec["oty_values"].most_common(5)],
        })
    registry.sort(key=lambda r: (r["owner_type"] or "", r["opyid"] if r["opyid"] is not None else 1 << 30, r["property_name"]))

    unique_classes = sorted({o["type"] for o in all_objects if o.get("type") and o.get("type").lower().startswith("opl")})
    unique_properties = sorted({p["name"] for p in all_properties})
    summary = {
        "schema": "publisher-html-harvest.v1",
        "seed_count": len(seeds),
        "sources_ok": sum(d["status"] == "ok" for d in documents),
        "sources_failed": sum(d["status"] != "ok" for d in documents),
        "sources_with_properties": sum(d.get("property_count", 0) > 0 for d in documents),
        "deduplicated_payloads": len(first_payload_source),
        "object_observations": len(all_objects),
        "property_observations": len(all_properties),
        "registry_entries": len(registry),
        "unique_opl_classes": len(unique_classes),
        "unique_property_names": len(unique_properties),
        "classes": unique_classes,
        "documents": documents,
        "registry": registry,
    }

    (out_dir / "harvest.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "objects.json").write_text(json.dumps(all_objects, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "properties.json").write_text(json.dumps(all_properties, ensure_ascii=False, indent=2), encoding="utf-8")
    write_tsv(out_dir / "properties.tsv", all_properties, ["source_id","property_index","owner_object_index","owner_tag","owner_type","owner_oty","owner_oh","name","priv","priv_u32","opyid","descriptor_type","wire_type","raw_tag","value"])
    write_tsv(out_dir / "registry.tsv", registry, ["owner_type","property_name","priv","opyid","descriptor_type","wire_type","raw_tag","source_count","observations"])

    print(json.dumps({k:v for k,v in summary.items() if k not in ("documents","registry")}, ensure_ascii=False, indent=2))
    if not all_properties:
        raise SystemExit("no Publisher hidden-XML properties recovered")

if __name__ == "__main__":
    main()
