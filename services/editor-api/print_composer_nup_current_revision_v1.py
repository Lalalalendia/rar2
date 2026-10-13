#!/usr/bin/env python3
"""Bind nine-up output to an already accepted Chaptera current Editor PDF receipt.

This is a source-neutral consumer. A receipt from an untrusted party is not
cryptographic producer authentication; verify it inside the trusted pipeline.
"""
from __future__ import annotations
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import stat
from print_composer_nup_v1 import PrintComposerNupError, plan_editor_card_a4_9up_v1
from print_composer_nup_pdf_v1 import (
    ComposedNupPdfV1, MAX_FIXED_PDF_BYTES, compose_editor_card_a4_9up_pdf_v1,
)

RECEIPT_V2 = "chaptera.editor-fixed-pdf-current-revision-receipt.v2"
MAX_PROJECT = 32 * 1024 * 1024
MAX_SOURCE = 256 * 1024 * 1024
MAX_RECEIPT = 256 * 1024
TRUE_LAWS = ("current_editor_project_authoritative", "authoritative_rust_project_replay",
             "source_pub_immutable", "replacement_exact_bytes_reach_renderer_request",
             "move_target_painted", "resize_target_painted", "replacement_target_painted",
             "edited_story_materialized")
FALSE_LAWS = ("raw_document_text_emitted", "raw_source_bytes_emitted",
              "replacement_asset_sha_emitted", "native_pub_write_used",
              "pdf_renderer_reimplemented_in_rar")

def sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()

def obj(value, code):
    if not isinstance(value, dict):
        raise PrintComposerNupError("current_receipt_" + code + "_missing")
    return value

def eq(value, expected, code):
    if type(value) is not type(expected) or value != expected:
        raise PrintComposerNupError("current_receipt_" + code + "_mismatch")

def ident(value, code):
    if not isinstance(value, str) or not (0 < len(value) <= 256) or any(ord(c)<32 for c in value):
        raise PrintComposerNupError("current_receipt_" + code + "_invalid")
    return value

def validate_current_fixed_pdf_receipt_v2(*, receipt, source_bytes, project_bytes, fixed_pdf_bytes):
    """Return accepted current Editor state identity after exact hash/receipt checks."""
    if not isinstance(source_bytes, bytes) or not (0 < len(source_bytes) <= MAX_SOURCE):
        raise PrintComposerNupError("source_size_or_type_invalid")
    if not isinstance(project_bytes, bytes) or not (0 < len(project_bytes) <= MAX_PROJECT):
        raise PrintComposerNupError("project_size_or_type_invalid")
    if not isinstance(fixed_pdf_bytes, bytes) or not (40 <= len(fixed_pdf_bytes) <= MAX_FIXED_PDF_BYTES):
        raise PrintComposerNupError("fixed_pdf_size_or_type_invalid")
    receipt=obj(receipt, "root")
    eq(receipt.get("receipt_version"), RECEIPT_V2, "version")
    eq(receipt.get("receipt_kind"), "real_hosted", "kind")
    producer=obj(receipt.get("producer"), "producer")
    eq(producer.get("implementation"), "rar-editor-current-revision-fixed-pdf-v2", "producer")
    eq(producer.get("source_neutral_renderer"), True, "source_neutral")
    source=obj(receipt.get("source"), "source")
    eq(source.get("sha256"), sha(source_bytes), "source_sha")
    eq(source.get("byte_len"), len(source_bytes), "source_length")
    eq(source.get("immutable"), True, "source_immutable")
    stage=obj(receipt.get("stage_0_1"), "stage")
    eq(stage.get("project_sha256"), sha(project_bytes), "project_sha")
    artifact=obj(receipt.get("artifact"), "artifact")
    eq(artifact.get("format"), "pdf", "artifact_format")
    eq(artifact.get("sha256"), sha(fixed_pdf_bytes), "artifact_sha")
    eq(artifact.get("byte_len"), len(fixed_pdf_bytes), "artifact_len")
    current=obj(receipt.get("current_revision"), "current_revision")
    state=ident(current.get("project_state_id"), "state_id")
    for key in ("story_target_state_current", "move_geometry_current", "resize_geometry_current",
                "replacement_image_current"):
        eq(current.get(key),True,key)
    eq(current.get("mutation_target_count"),4,"mutation_count")

    renderer = obj(receipt.get("renderer"), "renderer")
    summary = obj(renderer.get("summary"), "summary")
    eq(summary.get("page_count"), 1, "page_count")
    for key in ("node_painted", "node_partial", "node_unsupported"):
        value = summary.get(key)
        if type(value) is not int or value < 0 or value > 100_000:
            raise PrintComposerNupError("current_receipt_" + key + "_invalid")
    if summary["node_painted"] < 3:
        raise PrintComposerNupError("current_fixed_pdf_missing_painted_targets")
    codes = summary.get("diagnostic_codes")
    if (
        not isinstance(codes, list)
        or len(codes) > 100
        or any(not isinstance(code, str) or not (0 < len(code) <= 96) for code in codes)
    ):
        raise PrintComposerNupError("current_fixed_pdf_diagnostics_invalid")
    # V1 does not silently print repeated partial/unsupported source content.
    # Require loss-free upstream rendering; customer-facing override is
    # deliberately out of scope.
    if summary["node_partial"] or summary["node_unsupported"] or codes:
        raise PrintComposerNupError("current_fixed_pdf_visual_loss_unresolved")
    reports=obj(renderer.get("target_reports"),"reports")
    for name in ("move_node", "resize_node", "replace_image"):
        eq(obj(reports.get(name),name).get("disposition"),"painted",name)
    chain=obj(receipt.get("source_neutral_chain"),"chain")
    for key in ("resource_input_sha256","render_request_sha256","scene_snapshot_id",
                "resources_id","renderer_repair_sha256"):
        ident(chain.get(key),key)
    laws=obj(receipt.get("invariants"),"laws")
    for name in TRUE_LAWS: eq(laws.get(name), True,name)
    for name in FALSE_LAWS: eq(laws.get(name),False,name)
    eq(laws.get("source_reparse_after_edit_count"),0,"source_reparse")
    return state

def compose_verified_current_editor_card_nup_v1(
    *, upstream_receipt, original_pub_bytes, editor_project_bytes,
    current_fixed_pdf_bytes, logical_page_id, gap_mm=5
) -> ComposedNupPdfV1:
    revision=validate_current_fixed_pdf_receipt_v2(
        receipt=upstream_receipt,source_bytes=original_pub_bytes,
        project_bytes=editor_project_bytes,fixed_pdf_bytes=current_fixed_pdf_bytes)
    plan=plan_editor_card_a4_9up_v1(
        source_pub_sha256=sha(original_pub_bytes),
        editor_revision_id=revision,
        current_fixed_pdf_sha256=sha(current_fixed_pdf_bytes),
        logical_page_id=logical_page_id,gap_mm=gap_mm)
    result=compose_editor_card_a4_9up_pdf_v1(
        plan=plan,current_fixed_pdf_bytes=current_fixed_pdf_bytes,
        source_pub_bytes=original_pub_bytes)
    provenance={
        "protocol_version":"chaptera.editor-nine-up-current-revision.v1",
        "project_sha256":sha(editor_project_bytes),
        "upstream_receipt_sha256":sha(json.dumps(upstream_receipt,sort_keys=True,
            separators=(",",":"),ensure_ascii=False).encode("utf-8")),
        "project_state_id":revision,
        "source_immutable":True,
        "source_reparse_after_edit_count":0,
        "proof_class":"trusted_pipeline_structural_binding",
    }
    return ComposedNupPdfV1(result.pdf_bytes,result.preview_png_bytes,
                            {**result.receipt,"upstream":provenance})

def read_regular_bounded(path: Path, limit: int) -> bytes:
    meta=path.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_size <= 0 or meta.st_size > limit:
        raise PrintComposerNupError("input_invalid_file_class_or_size")
    with path.open("rb") as stream:
        raw=stream.read(limit+1)
    if not (0 < len(raw) <= limit):
        raise PrintComposerNupError("input_size_changed")
    return raw

def main() -> int:
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("source-pub","editor-project","current-fixed-pdf","current-fixed-receipt",
                 "logical-page-id","output-pdf","output-preview","output-receipt"):
        parser.add_argument("--"+name,required=True)
    parser.add_argument("--gap-mm",type=int,default=5)
    args=parser.parse_args()
    ins=[Path(getattr(args,name)) for name in
         ("source_pub","editor_project","current_fixed_pdf","current_fixed_receipt")]
    outs=[Path(getattr(args,name)) for name in
          ("output_pdf","output_preview","output_receipt")]
    try:
        if len({p.resolve() for p in ins+outs}) != len(ins)+len(outs):
            raise PrintComposerNupError("input_output_path_collision")
        if any(p.exists() or p.is_symlink() or not p.parent.is_dir() for p in outs):
            raise PrintComposerNupError("output_must_be_new_file")
        original=read_regular_bounded(ins[0],MAX_SOURCE)
        project=read_regular_bounded(ins[1],MAX_PROJECT)
        fixed=read_regular_bounded(ins[2],MAX_FIXED_PDF_BYTES)
        try: receipt=json.loads(read_regular_bounded(ins[3],MAX_RECEIPT))
        except (UnicodeError,ValueError) as exc:
            raise PrintComposerNupError("upstream_receipt_invalid_json") from exc
        product=compose_verified_current_editor_card_nup_v1(
            upstream_receipt=receipt, original_pub_bytes=original,
            editor_project_bytes=project,current_fixed_pdf_bytes=fixed,
            logical_page_id=args.logical_page_id,gap_mm=args.gap_mm)
        payloads=[product.pdf_bytes,product.preview_png_bytes,
                  (json.dumps(product.receipt,sort_keys=True,indent=2)+"\n").encode()]
        created=[]
        try:
            for path,payload in zip(outs,payloads):
                with path.open("xb") as output:
                    created.append(path)
                    output.write(payload)
        except OSError:
            for path in created:path.unlink(missing_ok=True)
            raise
        print(json.dumps({"ok":True,"pdf_sha256":sha(product.pdf_bytes),
                          "preview_sha256":sha(product.preview_png_bytes)},sort_keys=True))
        return 0
    except (PrintComposerNupError,OSError) as exc:
        print(json.dumps({"ok":False,"error":str(exc) if isinstance(exc,PrintComposerNupError)
                         else "file_io_error"},sort_keys=True))
        return 2

if __name__ == "__main__":
    raise SystemExit(main())
