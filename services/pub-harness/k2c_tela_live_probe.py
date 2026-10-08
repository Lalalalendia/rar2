#!/usr/bin/env python3
"""Bounded live acceptance for the Chaptera K2C Tela MCP applier."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from k2c_tela_applier import (
    PATCHSET_SCHEMA,
    TelaPatchManifest,
    TelaPatchOperation,
    apply_manifest,
)
from k2c_tela_mcp_transport import TelaMcpStdioTransport

PROBE_PAGE_ID = 5049
PROBE_SPACE_ID = 364
PROBE_TARGET = "Probe > Live acceptance target"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    marker = args.source_head[:12]
    idempotency_key = f"k2c-live-probe-{args.source_head}"

    with TelaMcpStdioTransport() as transport:
        before = transport.get_page(PROBE_PAGE_ID)
        if before.space_id != PROBE_SPACE_ID:
            raise RuntimeError("probe page moved outside guarded space")
        if PROBE_TARGET not in before.section_paths:
            raise RuntimeError("probe target missing from live heading map")

        manifest = TelaPatchManifest(
            schema_version=PATCHSET_SCHEMA,
            manifest_id=f"k2c-live-probe-{args.source_head}",
            expected_space_id=PROBE_SPACE_ID,
            operations=(
                TelaPatchOperation(
                    page_id=PROBE_PAGE_ID,
                    expected_updated_at=before.updated_at,
                    target=PROBE_TARGET,
                    operation="append",
                    content=f"\nacceptance_probe = {marker}\n",
                    idempotency_key=idempotency_key,
                ),
            ),
        )

        first = apply_manifest(
            manifest,
            transport,
            allowed_space_id=PROBE_SPACE_ID,
            apply=True,
        )
        after_first = transport.get_page(PROBE_PAGE_ID)

        retry = apply_manifest(
            manifest,
            transport,
            allowed_space_id=PROBE_SPACE_ID,
            apply=True,
            previous_receipt=first,
        )
        after_retry = transport.get_page(PROBE_PAGE_ID)

        if not first.completed or not retry.completed:
            raise RuntimeError("live probe did not complete")
        if not retry.idempotent_manifest_replay:
            raise RuntimeError("exact manifest retry was not recognized as idempotent")
        if after_retry.updated_at != after_first.updated_at:
            raise RuntimeError("exact manifest retry changed the page cursor")
        if first.operations[0].lint_errors or first.operations[0].lint_warnings:
            raise RuntimeError("first live write did not lint cleanly")

        receipt = {
            "schema_version": "chaptera.k2c-tela-live-probe-receipt.v1",
            "source_head": args.source_head,
            "source_safe": True,
            "page_id": PROBE_PAGE_ID,
            "space_id": PROBE_SPACE_ID,
            "before_updated_at": before.updated_at,
            "after_first_updated_at": after_first.updated_at,
            "after_retry_updated_at": after_retry.updated_at,
            "first_apply": first.as_dict(),
            "retry_apply": retry.as_dict(),
            "retry_changed_cursor": after_retry.updated_at != after_first.updated_at,
        }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
