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
        owner = nearest_owner(stack)
        explicit_priv = attrs.get("priv")
        if explicit_priv is None and owner is None:
            continue

        property_seq += 1
        value = clean_value(text[node["open_end"]:m.start()])
        if explicit_priv is not None:
            decoded = priv_decode(explicit_priv, value)
            priv_origin = "explicit"
        else:
            decoded = {
                "priv": None,
                "priv_u32": None,
                "opyid": None,
                "descriptor_type": None,
                "wire_type": None,
                "raw_tag": None,
            }
            priv_origin = "missing"

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
            "cb": attrs.get("cb"),
            "priv_origin": priv_origin,
            **decoded,
        })

    explicit_by_key = defaultdict(set)
    for prop in properties:
        if prop.get("priv") is None:
            continue
        owner_type = prop.get("owner_type")
        name = prop.get("name")
        if owner_type and name:
            explicit_by_key[(owner_type, name)].add(prop["priv"].upper())

    inferred = 0
    unresolved = 0
    for prop in properties:
        if prop.get("priv") is not None:
            continue
        key = (prop.get("owner_type"), prop.get("name"))
        candidates = explicit_by_key.get(key, set()) if key[0] and key[1] else set()
        is_self_container = bool(key[0] and key[1] and key[0].lower() == key[1].lower())
        if len(candidates) == 1 and not is_self_container:
            inferred_priv = next(iter(candidates))
            prop.update(priv_decode(inferred_priv, prop.get("value") or ""))
            prop["priv_origin"] = "inferred_same_source_owner_name"
            inferred += 1
        else:
            prop["priv_origin"] = "unresolved_missing"
            unresolved += 1

    ambiguous = [
        {
            "owner_type": owner_type,
            "property_name": name,
            "explicit_privs": sorted(values),
        }
        for (owner_type, name), values in sorted(explicit_by_key.items())
        if len(values) > 1
    ]
    inference = {
        "explicit": sum(1 for p in properties if p.get("priv_origin") == "explicit"),
        "inferred": inferred,
        "unresolved": unresolved,
        "ambiguous_keys": ambiguous,
    }
    return objects, properties, inference

def detect_generator(text):
    m = META_GENERATOR_RE.search(text) or META_GENERATOR_RE_REV.search(text)
    return clean_value(m.group(1)) if m else None

def publisher_major(generator):
    if not generator:
        return None
    m = re.search(r"\bMicrosoft\s+Publisher\s+(\d+)\b", generator, re.I)
    return int(m.group(1)) if m else None

def build_version_diff(properties):
    version_sets = defaultdict(set)
    owner_name_sets = defaultdict(lambda: defaultdict(set))
    for prop in properties:
        major = prop.get("source_publisher_major")
        priv = prop.get("priv")
        owner_type = prop.get("owner_type")
        name = prop.get("name")
        if major is None or not priv or not owner_type or not name:
            continue
        version_sets[major].add((owner_type, name, priv.upper()))
        owner_name_sets[major][(owner_type, name)].add(priv.upper())

    versions = sorted(version_sets)
    per_version = {}
    for major in versions:
        coords = sorted(version_sets[major])
        per_version[str(major)] = {
            "coordinate_count": len(coords),
            "coordinates": [
                {"owner_type": owner, "property_name": name, "priv": priv}
                for owner, name, priv in coords
            ],
        }

    comparisons = []
    for i, a in enumerate(versions):
        for b in versions[i + 1:]:
            set_a = version_sets[a]
            set_b = version_sets[b]
            common_owner_names = sorted(set(owner_name_sets[a]) & set(owner_name_sets[b]))
            stable_singletons = []
            differing_sets = []
            for key in common_owner_names:
                a_privs = owner_name_sets[a][key]
                b_privs = owner_name_sets[b][key]
                if len(a_privs) == 1 and a_privs == b_privs:
                    stable_singletons.append({
                        "owner_type": key[0],
                        "property_name": key[1],
                        "priv": next(iter(a_privs)),
                    })
                elif a_privs != b_privs:
                    differing_sets.append({
                        "owner_type": key[0],
                        "property_name": key[1],
                        "a_privs": sorted(a_privs),
                        "b_privs": sorted(b_privs),
                    })
            comparisons.append({
                "a": a,
                "b": b,
                "a_coordinate_count": len(set_a),
                "b_coordinate_count": len(set_b),
                "exact_coordinate_intersection": len(set_a & set_b),
                "a_only_coordinates": len(set_a - set_b),
                "b_only_coordinates": len(set_b - set_a),
                "common_owner_property_keys": len(common_owner_names),
                "stable_singleton_keys": len(stable_singletons),
                "differing_priv_set_keys": len(differing_sets),
                "stable_singletons": stable_singletons,
                "differing_priv_sets": differing_sets,
            })

    return {
        "schema": "publisher-html-version-diff.v1",
        "versions": versions,
        "per_version": per_version,
        "comparisons": comparisons,
    }

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
            objects, properties, inference = parse_publisher_xml(text, source_id)
            generator = detect_generator(text)
            major = publisher_major(generator)
            for obj in objects:
                obj["source_generator"] = generator
                obj["source_publisher_major"] = major
            for prop in properties:
                prop["source_generator"] = generator
                prop["source_publisher_major"] = major
            resolved_properties = [p for p in properties if p.get("priv") is not None]
            phash = payload_hash(objects, resolved_properties)
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
                "generator": generator,
                "publisher_major": major,
                "publisher_marker_score": score_candidate(text),
                "object_count": len(objects),
                "property_count": len(resolved_properties),
                "property_candidate_count": len(properties),
                "property_explicit_count": inference["explicit"],
                "property_inferred_count": inference["inferred"],
                "property_unresolved_count": inference["unresolved"],
                "ambiguous_inference_keys": inference["ambiguous_keys"],
                "payload_sha256": phash,
                "duplicate_of": duplicate_of,
            })
            all_objects.extend(objects)
            all_properties.extend(properties)
        except Exception as exc:
            doc["error"] = "%s: %s" % (type(exc).__name__, exc)
        documents.append(doc)

    aggregate = defaultdict(lambda: {"source_ids": set(), "observations": 0, "explicit_observations": 0, "inferred_observations": 0, "values": Counter(), "oty_values": Counter()})
    for prop in all_properties:
        if prop.get("priv") is None:
            continue
        key = (prop.get("owner_type") or "", prop.get("name") or "", prop.get("priv") or "", prop.get("opyid"), prop.get("descriptor_type"), prop.get("wire_type"), prop.get("raw_tag"))
        rec = aggregate[key]
        rec["source_ids"].add(prop["source_id"])
        rec["observations"] += 1
        if prop.get("priv_origin") == "explicit":
            rec["explicit_observations"] += 1
        elif prop.get("priv_origin") == "inferred_same_source_owner_name":
            rec["inferred_observations"] += 1
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
            "explicit_observations": rec["explicit_observations"],
            "inferred_observations": rec["inferred_observations"],
            "source_ids": sorted(rec["source_ids"]),
            "common_values": [{"value": v, "count": c} for v, c in rec["values"].most_common(5)],
            "owner_oty_values": [{"oty": v, "count": c} for v, c in rec["oty_values"].most_common(5)],
        })
    registry.sort(key=lambda r: (r["owner_type"] or "", r["opyid"] if r["opyid"] is not None else 1 << 30, r["property_name"]))

    resolved_properties = [p for p in all_properties if p.get("priv") is not None]
    explicit_properties = [p for p in all_properties if p.get("priv_origin") == "explicit"]
    inferred_properties = [p for p in all_properties if p.get("priv_origin") == "inferred_same_source_owner_name"]
    unresolved_properties = [p for p in all_properties if p.get("priv_origin") == "unresolved_missing"]
    ambiguous_keys = [
        {
            "source_id": d["source_id"],
            **item,
        }
        for d in documents
        for item in d.get("ambiguous_inference_keys", [])
    ]

    version_diff = build_version_diff(resolved_properties)
    unique_classes = sorted({o["type"] for o in all_objects if o.get("type") and o.get("type").lower().startswith("opl")})
    unique_properties = sorted({p["name"] for p in resolved_properties})
    summary = {
        "schema": "publisher-html-harvest.v1",
        "seed_count": len(seeds),
        "sources_ok": sum(d["status"] == "ok" for d in documents),
        "sources_failed": sum(d["status"] != "ok" for d in documents),
        "sources_with_properties": sum(d.get("property_count", 0) > 0 for d in documents),
        "deduplicated_payloads": len(first_payload_source),
        "object_observations": len(all_objects),
        "property_candidates_total": len(all_properties),
        "property_observations": len(resolved_properties),
        "property_observations_explicit": len(explicit_properties),
        "property_observations_inferred": len(inferred_properties),
        "property_observations_unresolved": len(unresolved_properties),
        "ambiguous_inference_key_count": len(ambiguous_keys),
        "ambiguous_inference_keys": ambiguous_keys,
        "registry_entries": len(registry),
        "unique_opl_classes": len(unique_classes),
        "unique_property_names": len(unique_properties),
        "publisher_versions": version_diff["versions"],
        "version_comparisons": [
            {
                k: v
                for k, v in comp.items()
                if k not in ("stable_singletons", "differing_priv_sets")
            }
            for comp in version_diff["comparisons"]
        ],
        "classes": unique_classes,
        "documents": documents,
        "registry": registry,
    }

    (out_dir / "harvest.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "objects.json").write_text(json.dumps(all_objects, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "properties.json").write_text(json.dumps(all_properties, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "version-diff.json").write_text(json.dumps(version_diff, ensure_ascii=False, indent=2), encoding="utf-8")
    write_tsv(out_dir / "properties.tsv", all_properties, ["source_id","source_generator","source_publisher_major","property_index","owner_object_index","owner_tag","owner_type","owner_oty","owner_oh","name","priv","priv_origin","priv_u32","opyid","descriptor_type","wire_type","raw_tag","value"])
    write_tsv(out_dir / "registry.tsv", registry, ["owner_type","property_name","priv","opyid","descriptor_type","wire_type","raw_tag","source_count","observations","explicit_observations","inferred_observations"])

    print(json.dumps({k:v for k,v in summary.items() if k not in ("documents","registry")}, ensure_ascii=False, indent=2))
    if not all_properties:
        raise SystemExit("no Publisher hidden-XML properties recovered")

if __name__ == "__main__":
    main()
