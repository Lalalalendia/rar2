#!/usr/bin/env python3
import argparse
import hashlib
import json
import re
from pathlib import Path

EXPECTED_PUB_SHA256 = "1e7f38b3ce1d0d956815992b15d361c405fc4bbdced5cdabb3c4581327cc183e"
EXPECTED_HTML_SHA256 = "6c2edf51c7a8d7ff76f1466d6bff69e37c7110ece27bcfac093cd0ce40aa7c5b"
TARGET_SHAPE_ID = 358
EXPECTED_FBID = "Basic...Wide Inline"

PRIMARY_FOPT = 0xF00B
TERTIARY_FOPT = 0xF122

PROP_LINE_COLOR = 0x01C0
PROP_LINE_BACK_COLOR = 0x01C2
PROP_LINE_WIDTH = 0x01CB
PROP_LINE_BOOLEANS = 0x01FF

EMU_PER_POINT = 12700


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def text_value(block, tag):
    m = re.search(
        rf"<b:{re.escape(tag)}(?:\s+[^>]*)?>(.*?)</b:{re.escape(tag)}>",
        block,
        re.I | re.S,
    )
    if not m:
        return None
    return re.sub(r"\s+", " ", m.group(1)).strip()


def bool_value(block, tag):
    value = text_value(block, tag)
    if value is None:
        return None
    return value.lower() in ("true", "1", "yes")


def find_target_tracking(html):
    candidates = re.findall(
        r"<b:OplOt\b[^>]*>(.*?)</b:OplOt>",
        html,
        re.I | re.S,
    )
    for block in candidates:
        if text_value(block, "OhTrack") == str(TARGET_SHAPE_ID):
            return block
    raise SystemExit(f"OplOt OhTrack={TARGET_SHAPE_ID} not found")


def find_target_vml(html):
    marker = f'oh="{TARGET_SHAPE_ID}"'
    pos = html.find(marker)
    if pos < 0:
        raise SystemExit(f"current OplPo oh={TARGET_SHAPE_ID} not found")
    start = html.rfind("<v:rect", 0, pos)
    end = html.find("</v:rect>", pos)
    if start < 0 or end < 0:
        raise SystemExit("target VML rect not found")
    block = html[start : end + len("</v:rect>")]
    opening_end = block.find(">")
    opening = block[: opening_end + 1]
    stroke_match = re.search(r"<v:stroke\b([^>]*)>", block, re.I | re.S)
    stroke_attrs = stroke_match.group(1) if stroke_match else ""
    return opening, stroke_attrs, block


def attr(blob, name):
    m = re.search(
        rf"\b{re.escape(name)}\s*=\s*(?:\"([^\"]*)\"|'([^']*)')",
        blob,
        re.I | re.S,
    )
    return (m.group(1) if m and m.group(1) is not None else m.group(2)) if m else None


def fopt_property(census, shape_id, rec_type, property_id):
    for shape in census.get("shapes", []):
        if shape.get("publisher_shape_id") != shape_id:
            continue
        rows = [
            p
            for p in shape.get("properties", [])
            if p.get("rec_type") == rec_type and p.get("property_id") == property_id
        ]
        if len(rows) != 1:
            raise SystemExit(
                f"expected one FOPT row shape={shape_id} rec={rec_type:#x} prop={property_id:#x}, got {len(rows)}"
            )
        return rows[0]
    raise SystemExit(f"shape {shape_id} not found in census")


def decode_line_booleans(op):
    # MS-ODRAW 2.3.8.38: low 16 bits are actual K..T bits and high
    # 16 bits are A..J "use" bits. Within each word six leading bits are
    # unused; the named ten bits occupy positions 9..0.
    actual = op & 0xFFFF
    use = (op >> 16) & 0xFFFF
    return {
        "op_hex": f"0x{op:08X}",
        "actual_word_hex": f"0x{actual:04X}",
        "use_word_hex": f"0x{use:04X}",
        "fUsefInsetPen": bool(use & (1 << 6)),
        "fUsefInsetPenOK": bool(use & (1 << 5)),
        "fInsetPen": bool(actual & (1 << 6)),
        "fInsetPenOK": bool(actual & (1 << 5)),
        "fUsefLineOpaqueBackColor": bool(use & (1 << 9)),
        "fLineOpaqueBackColor": bool(actual & (1 << 9)),
    }


def decode_colorref(op):
    return {
        "op_hex": f"0x{op:08X}",
        "scheme_index_enabled": bool(op & 0x08000000),
        "scheme_index": (op & 0xFF) if (op & 0x08000000) else None,
        "rgb24": op & 0x00FFFFFF,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pub-receipt", required=True)
    ap.add_argument("--html", required=True)
    ap.add_argument("--census", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    pub_receipt = load(args.pub_receipt)
    if pub_receipt.get("source_sha256") != EXPECTED_PUB_SHA256:
        raise SystemExit("unexpected help.pub SHA")

    html_bytes = Path(args.html).read_bytes()
    html_sha = sha256_bytes(html_bytes)
    if html_sha != EXPECTED_HTML_SHA256:
        raise SystemExit(f"unexpected help.htm SHA: {html_sha}")
    html = html_bytes.decode("windows-1252", "replace")

    census = load(args.census)
    if census.get("source_sha256") != EXPECTED_PUB_SHA256:
        raise SystemExit("census/source SHA mismatch")

    tracking = find_target_tracking(html)
    lastfmt_match = re.search(
        r"<b:OplLastFmt\b[^>]*>(.*?)</b:OplLastFmt>",
        tracking,
        re.I | re.S,
    )
    if not lastfmt_match:
        raise SystemExit("target OplLastFmt not found")
    lastfmt = lastfmt_match.group(1)
    line_match = re.search(
        r"<b:Lt\b[^>]*type=\"OplOdpoLineStyle\"[^>]*>(.*?)</b:Lt>",
        lastfmt,
        re.I | re.S,
    )
    if not line_match:
        raise SystemExit("target LastFmt line style not found")
    line = line_match.group(1)

    fbid = text_value(lastfmt, "FBID")
    legacy = {
        "fbid": fbid,
        "line_color": int(text_value(line, "LineColor")),
        "line_back_color": int(text_value(line, "LineBackColor")),
        "line_width_emu": int(text_value(line, "LineWidth")),
        "f_inset_pen": bool_value(line, "FInsetPen"),
        "f_inset_pen_ok": bool_value(line, "FInsetPenOK"),
    }

    opening, stroke_attrs, vml_block = find_target_vml(html)
    color2 = attr(stroke_attrs, "color2")
    scheme_match = re.search(r"\[(\d+)\]", color2 or "")
    vml = {
        "rect_id": attr(opening, "id"),
        "strokecolor": attr(opening, "strokecolor"),
        "strokeweight": attr(opening, "strokeweight"),
        "insetpen": attr(opening, "insetpen"),
        "stroke_color2": color2,
        "stroke_color2_scheme_index": int(scheme_match.group(1)) if scheme_match else None,
        "contains_current_oplpo_358": f'oh="{TARGET_SHAPE_ID}"' in vml_block,
    }

    line_color = fopt_property(census, TARGET_SHAPE_ID, PRIMARY_FOPT, PROP_LINE_COLOR)
    line_back = fopt_property(census, TARGET_SHAPE_ID, PRIMARY_FOPT, PROP_LINE_BACK_COLOR)
    line_width = fopt_property(census, TARGET_SHAPE_ID, PRIMARY_FOPT, PROP_LINE_WIDTH)
    line_bool = fopt_property(census, TARGET_SHAPE_ID, TERTIARY_FOPT, PROP_LINE_BOOLEANS)

    color_decoded = decode_colorref(line_color["op"])
    back_decoded = decode_colorref(line_back["op"])
    bool_decoded = decode_line_booleans(line_bool["op"])

    officeart = {
        "line_color": {
            "property_id": PROP_LINE_COLOR,
            "op": line_color["op"],
            **color_decoded,
        },
        "line_back_color": {
            "property_id": PROP_LINE_BACK_COLOR,
            "op": line_back["op"],
            **back_decoded,
        },
        "line_width": {
            "property_id": PROP_LINE_WIDTH,
            "op": line_width["op"],
            "points": line_width["op"] / EMU_PER_POINT,
        },
        "line_booleans": {
            "property_id": PROP_LINE_BOOLEANS,
            **bool_decoded,
        },
    }

    target_shape = next(
        s for s in census["shapes"] if s.get("publisher_shape_id") == TARGET_SHAPE_ID
    )
    direct_pointer_flags = {
        "f_bid_property_count": sum(1 for p in target_shape["properties"] if p.get("f_bid")),
        "f_complex_property_count": sum(1 for p in target_shape["properties"] if p.get("f_complex")),
        "blip_id_property_count": sum(1 for p in target_shape["properties"] if p.get("op_is_blip_id")),
    }

    control_356 = next(
        s for s in census["shapes"] if s.get("publisher_shape_id") == 356
    )
    control_pointer_flags = {
        "f_bid_property_count": sum(1 for p in control_356["properties"] if p.get("f_bid")),
        "f_complex_property_count": sum(1 for p in control_356["properties"] if p.get("f_complex")),
        "blip_id_property_count": sum(1 for p in control_356["properties"] if p.get("op_is_blip_id")),
    }

    checks = {
        "fbid_exact": legacy["fbid"] == EXPECTED_FBID,
        "legacy_line_color_equals_fopt": legacy["line_color"] == line_color["op"],
        "legacy_line_back_color_equals_fopt": legacy["line_back_color"] == line_back["op"],
        "legacy_line_width_equals_fopt": legacy["line_width_emu"] == line_width["op"],
        "legacy_inset_pen_equals_fopt": legacy["f_inset_pen"] == bool_decoded["fInsetPen"],
        "legacy_inset_pen_ok_equals_fopt": legacy["f_inset_pen_ok"] == bool_decoded["fInsetPenOK"],
        "fopt_use_inset_pen": bool_decoded["fUsefInsetPen"],
        "fopt_use_inset_pen_ok": bool_decoded["fUsefInsetPenOK"],
        "vml_current_object_join": vml["contains_current_oplpo_358"],
        "vml_line_color_white": (
            (vml["strokecolor"] or "").lower() == "white"
            and color_decoded["rgb24"] == 0xFFFFFF
            and not color_decoded["scheme_index_enabled"]
        ),
        "vml_line_back_scheme_index": (
            vml["stroke_color2_scheme_index"] == back_decoded["scheme_index"] == 1
            and back_decoded["scheme_index_enabled"]
        ),
        "vml_line_width": (
            (vml["strokeweight"] or "").lower() == "6pt"
            and abs(officeart["line_width"]["points"] - 6.0) < 1e-9
        ),
        "vml_inset_pen": (
            (vml["insetpen"] or "").lower() in ("t", "true", "1")
            and bool_decoded["fInsetPen"]
            and bool_decoded["fInsetPenOK"]
        ),
        "target_has_no_direct_fopt_blip_or_complex_pointer": all(
            direct_pointer_flags[k] == 0 for k in direct_pointer_flags
        ),
        "same_file_control_proves_blip_detection": (
            control_pointer_flags["f_bid_property_count"] > 0
            and control_pointer_flags["f_complex_property_count"] > 0
            and control_pointer_flags["blip_id_property_count"] > 0
        ),
    }

    result = {
        "schema": "publisher11-borderart-projection-join.v1",
        "source": {
            "pub_sha256": EXPECTED_PUB_SHA256,
            "html_sha256": EXPECTED_HTML_SHA256,
        },
        "target": {
            "publisher_shape_id": TARGET_SHAPE_ID,
            "officeart_spid": target_shape.get("officeart_spid"),
            "fbid": EXPECTED_FBID,
        },
        "legacy_lastfmt": legacy,
        "officeart": officeart,
        "vml": vml,
        "direct_pointer_flags": direct_pointer_flags,
        "same_file_blip_control_shape_356": control_pointer_flags,
        "checks": checks,
        "verdict": "PASS" if all(checks.values()) else "FAIL",
        "closed": (
            "For exact Publisher 11 help.pub/help.htm Shape 358, the LastFmt line state "
            "adjacent to FBID has an exact active OfficeArt and VML projection: foreground "
            "line color, scheme-indexed background color, 6pt width, and inset-pen state."
        ),
        "not_closed": (
            "FBID itself is not a per-shape FOPT/BLIP pointer, and this receipt does not prove "
            "that the ordinary line projection alone is sufficient to reproduce the full "
            "Basic Wide Inline BorderArt appearance or that the same representation applies "
            "to every BorderArt profile/version."
        ),
    }

    Path(args.out).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({
        "schema": result["schema"],
        "verdict": result["verdict"],
        "target": result["target"],
        "legacy_lastfmt": result["legacy_lastfmt"],
        "officeart": result["officeart"],
        "vml": result["vml"],
        "direct_pointer_flags": result["direct_pointer_flags"],
        "same_file_blip_control_shape_356": result["same_file_blip_control_shape_356"],
        "checks": result["checks"],
    }, indent=2))
    if result["verdict"] != "PASS":
        raise SystemExit("Publisher 11 BorderArt projection join failed")


if __name__ == "__main__":
    main()
