#!/usr/bin/env python3
"""Static, offline, read-only visual review queue for verified migration receipts.

No external assets, scripts, remote calls, source-PUB reads or approval actions.
This module projects the existing canonical operator receipt without changing
its status, its loss policy, or its customer-delivery authority.
"""
from __future__ import annotations

from html import escape
import os
from pathlib import Path
import re
from urllib.parse import quote

SCHEMA = "chaptera.migration-operator-pdf-report.v1"
BOUNDARY = "operator_only_integrity_not_visual_fidelity_or_migration_certification"
PDF_NAME = re.compile(r"[A-Za-z0-9_-]{1,48}--[0-9a-f]{12}--[0-9a-f]{16}\.pdf\Z")
STATES = frozenset(("integrity_confirmed_review_required", "integrity_rejected", "exception"))
COUNT_KEYS = ("integrity_confirmed_review_required", "integrity_rejected", "exception", "not_yet_in_checkpoint")


class ReviewIndexRejected(ValueError):
    """Reject a forged or incomplete receipt instead of generating a false page."""


def _link(target: Path, output_root: Path, display: str) -> str:
    """Only existing regular local output files; never an absolute path in HTML."""
    if not target.is_file() or target.is_symlink():
        return '<span class="missing">Unavailable</span>'
    try:
        relative = os.path.relpath(target, start=output_root)
    except ValueError:  # Windows paths on separate drives.
        return '<span class="missing">Open from the PDF directory</span>'
    # Path is derived only from the canonical PDF filename, never a source path.
    href = quote(relative.replace("\\", "/"), safe="/.-_")
    return '<a href="' + escape(href, quote=True) + '" rel="noreferrer noopener">' + escape(display) + '</a>'


def render_index(receipt: dict, pdf_dir: Path, report_dir: Path) -> str:
    """Render a deterministic snapshot, not an approval or live verification UI."""
    if not isinstance(receipt, dict) or receipt.get("schema") != SCHEMA or receipt.get("claim_boundary") != BOUNDARY:
        raise ReviewIndexRejected("unexpected_operator_authority")
    counts = receipt.get("counts")
    entries = receipt.get("entries")
    total = receipt.get("total_sources")
    checkpoint = receipt.get("checkpoint_rows")
    if (not isinstance(counts, dict) or not isinstance(entries, list)
            or type(total) is not int or not 1 <= total <= 2000
            or type(checkpoint) is not int or checkpoint != len(entries) or checkpoint > total):
        raise ReviewIndexRejected("invalid_review_denominator")
    if any(type(counts.get(key)) is not int or counts[key] < 0 for key in COUNT_KEYS):
        raise ReviewIndexRejected("invalid_review_counts")
    if sum(counts[key] for key in COUNT_KEYS) != total:
        raise ReviewIndexRejected("review_counts_do_not_cover_sources")
    if counts["not_yet_in_checkpoint"] != total - checkpoint:
        raise ReviewIndexRejected("review_pending_count_drift")
    digest = receipt.get("source_receipt_sha256")
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise ReviewIndexRejected("missing_source_manifest_fingerprint")

    rows: list[tuple[int, str]] = []
    state_counts = {name: 0 for name in STATES}
    seen: set[str] = set()
    for item in entries:
        if not isinstance(item, dict):
            raise ReviewIndexRejected("invalid_review_row")
        source, state, reason = (item.get("source_relative_path"), item.get("state"), item.get("reason"))
        if (not isinstance(source, str) or len(source) > 1024
                or not source.lower().endswith(".pub") or "\\" in source
                or source.startswith("/") or any(s in ("", ".", "..") for s in source.split("/"))
                or any(ord(c) < 32 for c in source)):
            raise ReviewIndexRejected("unsafe_review_source_label")
        if source.casefold() in seen:
            raise ReviewIndexRejected("duplicate_review_source")
        seen.add(source.casefold())
        if state not in STATES or not isinstance(reason, str) or not re.fullmatch(r"[a-z0-9_]{1,96}", reason):
            raise ReviewIndexRejected("unrecognized_review_state")
        state_counts[state] += 1
        name = item.get("output_pdf")
        # Only exact integrity-confirmed results get a link. A failed or
        # altered output must never be presented as something to deliver.
        if state == "integrity_confirmed_review_required":
            if not isinstance(name, str) or not PDF_NAME.fullmatch(name):
                raise ReviewIndexRejected("invalid_review_pdf_name")
            if not re.fullmatch(r"[0-9a-f]{64}", item.get("pdf_sha256", "")):
                raise ReviewIndexRejected("unbound_review_pdf_hash")
            pdf = _link(pdf_dir / name, report_dir, "Open PDF")
            loss = _link(pdf_dir / (name + ".loss.txt"), report_dir, "Loss report")
            action = pdf + ' <span class="separator">|</span> ' + loss
            priority = 1
            status_label = "Review PDF and losses"
        else:
            action = '<span class="missing">No verified PDF link</span>'
            priority = 0
            status_label = "Integrity rejected" if state == "integrity_rejected" else "Conversion exception"
        # All untrusted display data is escaped. No user name or report text is
        # evaluated as markup, URL, script or CSS.
        rows.append((priority,
            '<tr class="' + ("needs-review" if priority else "exception") + '">'
            '<td><bdi dir="auto">' + escape(source) + '</bdi></td>'
            '<td>' + escape(status_label) + '</td>'
            '<td><code>' + escape(reason) + '</code></td>'
            '<td>' + action + '</td></tr>'))
    if any(state_counts[name] != counts[name] for name in STATES):
        raise ReviewIndexRejected("review_row_count_drift")
    rows.sort(key=lambda row: row[0])  # Exceptions ahead of candidates for review.
    body = '\n'.join(html_row for _, html_row in rows)
    counts_markup = ''.join(
        '<div class="metric"><strong>' + str(counts[key]) + '</strong><span>' + label + '</span></div>'
        for key, label in (
            ("integrity_confirmed_review_required", "PDFs to review"),
            ("integrity_rejected", "Integrity rejected"),
            ("exception", "Conversion exceptions"),
            ("not_yet_in_checkpoint", "Not processed"),
        )
    )
    return ('''<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; object-src 'none'; form-action 'none'">
<title>Chaptera Migration - Local Review Index</title>
<style>
:root{font:15px/1.5 system-ui,-apple-system,Segoe UI,sans-serif;color:#172a38;background:#f4f7f9}
body{margin:0;padding:2rem 1rem}main{max-width:1200px;margin:auto}h1{font-size:1.65rem;margin:0 0 .5rem}
p{margin:.6rem 0 1rem}.muted,footer{color:#586b78}.notice{padding:1rem;background:#fff;border-left:4px solid #bd7929}
.metrics{display:grid;grid-template-columns:repeat(auto-fit,minmax(145px,1fr));gap:.8rem;margin:1.3rem 0}
.metric{background:#fff;padding:1rem;border:1px solid #d9e3e9;border-radius:8px}.metric strong{font-size:1.45rem;display:block}
.metric span{color:#526673}.table-wrap{overflow-x:auto;background:#fff;border:1px solid #d9e3e9;border-radius:8px}
table{border-collapse:collapse;width:100%;min-width:690px;text-align:left}th,td{padding:.75rem .85rem;border-bottom:1px solid #e1e9ec;vertical-align:top}
th{background:#ebf0f4}tr.exception{background:#fff8f3}tr.needs-review{background:#fff}code{overflow-wrap:anywhere}
a{color:#075b9c;text-decoration:underline}.missing{color:#715b59}.separator{color:#89939c}
footer{font-size:.82rem;margin-top:1.8rem;overflow-wrap:anywhere}bdi{overflow-wrap:anywhere}
</style></head><body><main>
<h1>Chaptera Migration - local PDF review</h1>
<p class="notice"><strong>Not approved for delivery.</strong> This is an offline snapshot of existing integrity evidence. Open each available PDF and inspect its loss report. Passing SHA checks does not establish Microsoft Publisher layout fidelity, PDF/A compliance or customer acceptance. No file is automatically approved.</p>
<div class="metrics">''' + counts_markup + '''</div>
<p class="muted">''' + str(total) + ''' original candidates; ''' + str(checkpoint) + ''' rows in this checkpoint. Unprocessed files are counted but cannot be named by this receipt.</p>
<div class="table-wrap"><table><thead><tr><th scope="col">Source PUB</th><th scope="col">Result</th><th scope="col">Evidence status</th><th scope="col">Local files</th></tr></thead><tbody>
''' + body + '''
</tbody></table></div>
<footer>Private local report. Do not upload this index: it contains customer file names. Receipt SHA-256: <code>''' + digest + '''</code>. Links point to already-produced local files only. This index does not update when files change; rerun the workflow before relying on it.</footer>
</main></body></html>
''')
