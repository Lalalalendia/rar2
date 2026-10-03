#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

SCHEMA = "chaptera.mature-029-special59-census.v1"
RECEIPT_SCHEMA = "chaptera.mature-029-special59-signature.v1"

EXPECTED = {
    "273d283c93fafb0f14b772ba8475a6b48db0a4f9c1a0789605ce26ea811996d3",
    "5eb5055bc75918ca8dbc7093fa267a3365f25129fd2c3dc88c7be3540f44e4cd",
    "7c630704ce369f775fe7a24ec180f5c7997eb779f28bd57ab8c7dda66297a17c",
    "9915c5612d7d8ce49fc733bbbe748c80325fb0094af0ba0d42a2b15fc648e5bd",
    "a8c8a3c36a925fdc9de7c4f5e43e014a606751b2d09635cbc1b04f50ccb7fcea",
    "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e",
}


def canonical_signature(row: dict) -> dict:
    chunk = row["special_chunk"]
    return {
        "document_page_list_entry_count": row["document_page_list_entry_count"],
        "confirmed_page_count": row["confirmed_page_count"],
        "special_document_ordinal": row["special_document_ordinal"],
        "declared_length": chunk["declared_length"],
        "fully_decoded": chunk["fully_decoded"],
        "unsupported_tail_present": chunk["unsupported_tail_present"],
        "unsupported_tail_length": chunk["unsupported_tail_length"],
        "fields": chunk["fields"],
    }


def digest(value: dict) -> str:
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(payload).hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("receipt_dir", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    rows = []
    observed = set()
    signatures = Counter()
    signature_examples = {}
    target_classes = Counter()
    field_shapes = Counter()

    for path in sorted(args.receipt_dir.glob("*.json")):
        row = json.loads(path.read_text(encoding="utf-8"))
        assert row["schema"] == RECEIPT_SCHEMA
        sha = row["source_sha256"]
        assert sha in EXPECTED, sha
        assert sha not in observed, sha
        observed.add(sha)
        claims = row["claims"]
        assert claims["measurement_only"] is True
        assert claims["raw_u16_values_retained"] is False
        assert claims["raw_u32_values_retained"] is False
        assert claims["raw_fixed_bytes_retained"] is False
        assert claims["raw_seq_nums_retained"] is False
        assert claims["story_text_retained"] is False
        assert claims["source_graph_mutated"] is False
        assert claims["viewer_semantics_changed"] is False

        sig = canonical_signature(row)
        sig_digest = digest(sig)
        signatures[sig_digest] += 1
        signature_examples.setdefault(sig_digest, sig)

        for field in row["special_chunk"]["fields"]:
            body = field["body"]
            field_shapes[(str(field["field_id"]), field["block_type"], body["kind"])] += 1
            if body["kind"] == "u32_relation":
                target_classes[body["target_class"]] += 1

        rows.append({
            "source_sha256": sha,
            "source_bytes": row["source_bytes"],
            "signature_sha256": sig_digest,
            "special_document_ordinal": row["special_document_ordinal"],
            "field_count": len(row["special_chunk"]["fields"]),
            "fully_decoded": row["special_chunk"]["fully_decoded"],
        })

    assert observed == EXPECTED, sorted(EXPECTED - observed)
    assert len(rows) == 6

    signature_rows = [
        {
            "signature_sha256": key,
            "fixture_count": count,
            "signature": signature_examples[key],
        }
        for key, count in sorted(signatures.items(), key=lambda kv: (-kv[1], kv[0]))
    ]

    out = {
        "schema": SCHEMA,
        "fixture_count": len(rows),
        "signature_count": len(signatures),
        "all_six_identical_signature": len(signatures) == 1,
        "fully_decoded_count": sum(int(row["fully_decoded"]) for row in rows),
        "target_class_histogram": dict(sorted(target_classes.items())),
        "field_shape_histogram": {
            "|".join(key): value for key, value in sorted(field_shapes.items())
        },
        "signatures": signature_rows,
        "fixtures": sorted(rows, key=lambda row: row["source_sha256"]),
        "claims": {
            "measurement_only": True,
            "publisher_or_pdf_reference_used": False,
            "raster_reference_used": False,
            "raw_values_retained": False,
            "raw_seq_nums_retained": False,
            "source_graph_mutated": False,
            "viewer_semantics_changed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "fixture_count": out["fixture_count"],
        "signature_count": out["signature_count"],
        "all_six_identical_signature": out["all_six_identical_signature"],
        "fully_decoded_count": out["fully_decoded_count"],
        "target_class_histogram": out["target_class_histogram"],
        "field_shape_histogram": out["field_shape_histogram"],
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
