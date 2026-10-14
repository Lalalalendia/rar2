#!/usr/bin/env python3
"""Join an opt-in PDF paint trace to the existing tiled visual-diff receipt.

This is an *envelope-candidate* diagnostic, not an object-to-pixel proof.
It consumes source-neutral JSON only; it does not parse PUB/PDF or emit text,
image pixels, font files, session identifiers, or private input paths.
"""
import argparse
import json
import re
from pathlib import Path

EMU_PER_POINT = 12_700
MAX_TILE_CELLS = 1_000_000
MAX_PAINT_EVENTS = 50_000
MAX_INTERSECTION_CHECKS = 20_000_000


class TraceJoinError(ValueError):
    pass


def significant_tiles(mask):
    if not mask or mask.get("encoding") != "row-major-tile-rle-v1":
        raise TraceJoinError("unsupported or missing significant tile mask")
    try:
        w, h, tile_px = (int(mask[k]) for k in ("tile_width", "tile_height", "tile_px"))
    except (KeyError, TypeError, ValueError) as error:
        raise TraceJoinError("invalid tile dimensions") from error
    if min(w, h, tile_px) <= 0 or w * h > MAX_TILE_CELLS:
        raise TraceJoinError("invalid or excessive tile dimensions")
    result, last = [], -1
    for pair in mask.get("runs", []):
        if not isinstance(pair, list) or len(pair) != 2:
            raise TraceJoinError("malformed tile RLE run")
        try:
            start, count = (int(k) for k in pair)
        except (TypeError, ValueError) as error:
            raise TraceJoinError("malformed tile RLE index") from error
        if count <= 0 or start <= last or start < 0 or start + count > w * h:
            raise TraceJoinError("overlapping or out-of-bounds tile RLE")
        for index in range(start, start + count):
            result.append((index % w, index // w))
        last = start + count - 1
    return result, tile_px


def _page_geometry(page, oracle, dpi):
    if not oracle.get("page_boxes_match", False):
        raise TraceJoinError("candidate/reference page boxes differ")
    if not oracle.get("diff", {}).get("raster_size_match", False):
        raise TraceJoinError("raster dimensions differ")
    boxes = oracle.get("candidate_boxes", {})
    rect = boxes.get("render_rect", {})
    if boxes.get("rotation") != 0:
        raise TraceJoinError("rotated PDF page needs a separate coordinate mapper")
    try:
        x0, y0 = float(rect["x0_pt"]), float(rect["y0_pt"])
        w, h = float(rect["width_pt"]), float(rect["height_pt"])
        mb = [float(n) for n in page["media_box_points"]]
    except (KeyError, TypeError, ValueError) as error:
        raise TraceJoinError("incomplete PDF page geometry") from error
    if len(mb) != 4 or not (10 <= dpi <= 1200):
        raise TraceJoinError("invalid page box or DPI")
    if abs(x0) > 0.01 or abs(y0) > 0.01 or abs(mb[0]) > 0.01 or abs(mb[1]) > 0.01:
        raise TraceJoinError("offset/cropped coordinate systems require explicit mapping")
    if abs(mb[2] - w) > 0.1 or abs(mb[3] - h) > 0.1:
        raise TraceJoinError("PDF trace dimensions disagree with candidate raster")
    if w <= 0 or h <= 0:
        raise TraceJoinError("nonpositive page geometry")


def _intersects(envelope, tx, ty, tile_px, dpi):
    if not isinstance(envelope, list) or len(envelope) != 4:
        raise TraceJoinError("malformed source node envelope")
    try:
        left, top, w, h = (int(v) for v in envelope)
    except (TypeError, ValueError) as error:
        raise TraceJoinError("invalid EMU envelope") from error
    if w <= 0 or h <= 0:
        raise TraceJoinError("nonpositive source node envelope")
    scale = dpi / (72.0 * EMU_PER_POINT)
    return (
        left * scale < (tx + 1) * tile_px
        and (left + w) * scale > tx * tile_px
        and top * scale < (ty + 1) * tile_px
        and (top + h) * scale > ty * tile_px
    )


def _verified_source_links(trace_receipt, painted_nodes):
    """Validate explicitly recorded source refs; never infer a PUB address."""
    entries = trace_receipt.get("source_nodes")
    if entries is None:
        return {}
    source_hash = trace_receipt.get("source_sha256")
    if not isinstance(source_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", source_hash):
        raise TraceJoinError("source-linked trace lacks valid primary PUB SHA-256")
    if not isinstance(entries, list) or len(entries) > MAX_PAINT_EVENTS:
        raise TraceJoinError("invalid source link count")
    statuses = {
        "unmapped_projected_node",
        "no_exact_authoritative_source_ref",
        "exact_source_object_only",
        "exact_source_byte_range",
    }
    linked = {}
    for item in entries:
        if not isinstance(item, dict):
            raise TraceJoinError("malformed source link")
        node = str(item.get("node_origin"))
        if node not in painted_nodes or node in linked:
            raise TraceJoinError("unknown or duplicate source-linked NodeId")
        status, refs = item.get("status"), item.get("source_refs")
        if status not in statuses or not isinstance(refs, list):
            raise TraceJoinError("invalid source link authority status")
        has_byte_range = False
        for ref in refs:
            if not isinstance(ref, dict) or ref.get("source_hash") != source_hash:
                raise TraceJoinError("source reference hash differs from primary PUB")
            if (ref.get("authority"), ref.get("confidence"), ref.get("role")) not in (
                ("authoritative", "exact", "semantic"),
                ("authoritative", "exact", "projection"),
            ):
                raise TraceJoinError("non-authoritative or inferred source reference")
            span = ref.get("byte_range")
            if span is not None:
                if not isinstance(span, dict):
                    raise TraceJoinError("malformed source byte range")
                offset, length = span.get("offset"), span.get("length")
                if not isinstance(offset, int) or not isinstance(length, int):
                    raise TraceJoinError("noninteger source byte range")
                if min(offset, length) < 0 or offset + length > (2**64 - 1):
                    raise TraceJoinError("overflowing source byte range")
                has_byte_range = True
        expected = (
            "exact_source_byte_range" if has_byte_range else "exact_source_object_only"
        ) if refs else status
        if refs and status != expected:
            raise TraceJoinError("claimed source range status disagrees with references")
        if not refs and status not in (
            "unmapped_projected_node", "no_exact_authoritative_source_ref"
        ):
            raise TraceJoinError("claimed exact source authority without references")
        linked[node] = {"status": status, "source_refs": refs}
    return linked



def _verified_image_inputs(trace_receipt, events):
    """One bounded Viewer↔fixed-PDF handoff per painted image operation."""
    supplied = trace_receipt.get("image_inputs")
    if supplied is None:
        return {}  # older v1 diagnostics legitimately have no image-input data
    if not isinstance(supplied, list):
        raise TraceJoinError("invalid image input trace")
    expected = {
        (str(event["node_origin"]), str(event["resource_origin"]))
        for event in events if event["kind"] == "image"
    }
    if len(supplied) != len(expected):
        raise TraceJoinError("image input count differs from emitted image uses")
    accepted_status = {
        "unmatched_or_ambiguous_image_identity",
        "ambiguous_placement_records",
        "pdf_has_unmatched_placement",
        "no_explicit_viewer_placement",
        "viewer_crop_not_forwarded",
        "pdf_crop_not_in_viewer",
        "viewer_recolor_not_forwarded",
        "pdf_recolor_not_in_viewer",
        "viewer_rotation_not_forwarded",
        "placement_fields_forwarded",
    }
    result = {}
    for item in supplied:
        if not isinstance(item, dict):
            raise TraceJoinError("malformed image input")
        key = (str(item.get("node_origin")), str(item.get("resource_origin")))
        if key not in expected or key in result:
            raise TraceJoinError("unknown or duplicate painted image input")
        status = item.get("transfer_status")
        if status not in accepted_status:
            raise TraceJoinError("invalid image input transfer status")
        if status not in (
            "unmatched_or_ambiguous_image_identity",
            "ambiguous_placement_records",
        ):
            for name in (
                "source_exact", "viewer_placement_present", "viewer_recolor_present",
                "pdf_placement_present", "pdf_source_window_present", "pdf_recolor_present",
            ):
                if type(item.get(name)) is not bool:
                    raise TraceJoinError("missing or nonboolean image input evidence")
            window = item.get("viewer_source_window_q16")
            if window is not None:
                if (not isinstance(window, list) or len(window) != 4
                    or any(type(v) is not int or not -(2**63) <= v < 2**63 for v in window)):
                    raise TraceJoinError("invalid viewer crop window coordinates")
            if window is None and status == "viewer_crop_not_forwarded":
                raise TraceJoinError("claimed viewer crop missing from evidence")
            if window is not None and not item["viewer_placement_present"]:
                raise TraceJoinError("crop cannot exist without Viewer placement")
            if type(item.get("mime")) is not str or not item["mime"].startswith("image/"):
                raise TraceJoinError("invalid image MIME evidence")
            for name in ("viewer_rotation_degrees", "pdf_rotation_degrees"):
                val = item.get(name)
                if val is not None and (type(val) is not int or not -360 <= val <= 360):
                    raise TraceJoinError("invalid image rotation evidence")
        result[key] = item
    return result


def join_paint_trace(trace_receipt, reference_receipt):
    """Return ordered candidate matches, not uniquely attributed pixel causes.

    trace_receipt: opt-in pub-cli .paint-trace.json from the exact PDF bytes
    reference_receipt: output of tools/pdf_reference_diff_v1.py compare_pdfs().
    """
    if trace_receipt.get("schema_version") != "chaptera.pdf-paint-trace.v1":
        raise TraceJoinError("unsupported paint trace version")
    candidate_hash = trace_receipt.get("candidate_pdf_sha256", "")
    if not isinstance(candidate_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", candidate_hash):
        raise TraceJoinError("missing or invalid PDF trace SHA-256")
    if candidate_hash != reference_receipt.get("candidate_sha256"):
        raise TraceJoinError("paint trace and raster diff refer to different PDF bytes")
    if reference_receipt.get("receipt_version") != "chaptera.fidelity-reference-pdf.v1":
        raise TraceJoinError("unsupported reference raster receipt")
    meta = reference_receipt.get("renderer", {})
    if meta.get("engine") != "MuPDF" or meta.get("colorspace") != "RGB" or meta.get("alpha") is not False:
        raise TraceJoinError("incompatible reference raster renderer")
    try:
        dpi = float(meta["raster_dpi"])
    except (KeyError, TypeError, ValueError) as error:
        raise TraceJoinError("reference raster DPI missing") from error
    if not reference_receipt.get("page_count", {}).get("match", False):
        raise TraceJoinError("candidate/reference page count differs")
    pages, oracle_pages = trace_receipt.get("pages", []), reference_receipt.get("pages", [])
    if not isinstance(pages, list) or len(pages) != len(oracle_pages):
        raise TraceJoinError("PDF trace and reference page counts differ")

    all_events = trace_receipt.get("paint_trace", [])
    if not isinstance(all_events, list) or len(all_events) > MAX_PAINT_EVENTS:
        raise TraceJoinError("invalid or excessive paint event count")
    events_by_page = {str(page["origin"]): [] for page in pages}
    if len(events_by_page) != len(pages):
        raise TraceJoinError("duplicate page origin in trace")
    for event in all_events:
        key = str(event.get("page_origin"))
        if key not in events_by_page:
            raise TraceJoinError("paint event references an unknown page")
        if event.get("output_sheet_copies") != 1:
            raise TraceJoinError("nine-up needs a physical-sheet transform trace")
        if event.get("kind") not in ("rectangle", "image", "text"):
            raise TraceJoinError("unknown paint event kind")
        indices = event.get("unwrapped_program_byte_range")
        if not isinstance(indices, list) or len(indices) != 2 or not (0 <= indices[0] < indices[1]):
            raise TraceJoinError("invalid PDF program byte range")
        events_by_page[key].append(event)
    painted_nodes = {str(event["node_origin"]) for event in all_events}
    source_links = _verified_source_links(trace_receipt, painted_nodes)
    image_inputs = _verified_image_inputs(trace_receipt, all_events)
    rows = []
    for page_index, (page, oracle) in enumerate(zip(pages, oracle_pages)):
        _page_geometry(page, oracle, dpi)
        tiles, tile_px = significant_tiles(oracle["diff"]["significant_mask"])
        events = events_by_page[str(page["origin"])]
        if len(events) * len(tiles) > MAX_INTERSECTION_CHECKS:
            raise TraceJoinError("unbounded candidate intersection work")
        for index, event in enumerate(events):
            if event.get("page_paint_index") != index:
                raise TraceJoinError("PDF paint indices must be contiguous and page-local")
        matches_by_tile = [[] for _ in tiles]
        for event_index, event in enumerate(events):
            for tile_index, (x, y) in enumerate(tiles):
                if _intersects(event.get("node_bounds_emu"), x, y, tile_px, dpi):
                    matches_by_tile[tile_index].append(event_index)
        candidates = []
        for event_index, event in enumerate(events):
            matching = [i for i, group in enumerate(matches_by_tile) if event_index in group]
            if not matching:
                continue
            candidates.append({
                "node_origin": event["node_origin"],
                "kind": event["kind"],
                "resource_origin": event.get("resource_origin"),
                "text_run_ordinal": event.get("text_run_ordinal"),
                "page_paint_index": event["page_paint_index"],
                "source_link_status": source_links.get(
                    str(event["node_origin"]), {}
                ).get("status", "trace_not_available"),
                "source_refs": source_links.get(
                    str(event["node_origin"]), {}
                ).get("source_refs", []),
                "image_placement_evidence": image_inputs.get((
                    str(event["node_origin"]), str(event.get("resource_origin"))
                )) if event["kind"] == "image" else None,
                "candidate_significant_tile_count": len(matching),
                "overlapped_candidate_tile_count": sum(
                    len(matches_by_tile[i]) > 1 for i in matching
                ),
                "last_painted_envelope_candidate_tiles": sum(
                    matches_by_tile[i][-1] == event_index for i in matching
                ),
            })
        rows.append({
            "page_index": page_index,
            "page_origin": page["origin"],
            "significant_pixel_count": oracle["diff"]["significant_pixel_count"],
            "significant_tile_count": len(tiles),
            "unmatched_significant_tile_count": sum(not group for group in matches_by_tile),
            "ambiguous_significant_tile_count": sum(len(group) > 1 for group in matches_by_tile),
            "candidate_paint_events": candidates,
        })
    return {
        "receipt_version": "chaptera.pdf-paint-trace-roi-join.v1",
        "pages": rows,
        "limitations": [
            "Node envelopes are not exact glyph ink, clipping or image crop bounds.",
            "A changed tile can intersect multiple objects; candidate tile counts are non-additive.",
            "Last painted envelope candidate is not proof of the visible or causal source.",
            "SourceRef links are admitted only when exact and authoritative for the primary PUB.",
            "Viewer image crop/rotation evidence describes adapter handoff, not exact PDF image ink.",
            "Publisher native PDF has no Chaptera NodeId; external source matching remains unproven.",
            "Only unrotated, uncropped one-copy pages are supported in this first join.",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trace", type=Path, required=True, help="controlled-access trace JSON")
    parser.add_argument("--reference-diff", type=Path, required=True, help="raster diff receipt JSON")
    parser.add_argument("--output", type=Path, required=True, help="result destination")
    args = parser.parse_args()
    output = join_paint_trace(
        json.loads(args.trace.read_text(encoding="utf-8")),
        json.loads(args.reference_diff.read_text(encoding="utf-8")),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, sort_keys=True, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
