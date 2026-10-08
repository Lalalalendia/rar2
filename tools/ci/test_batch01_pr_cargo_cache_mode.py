#!/usr/bin/env python3
"""Guard Batch01 Cargo PR restore-only and trusted-main-only cache writes."""

from pathlib import Path

reader = Path(".github/workflows/reader-pr-ci.yml").read_text(encoding="utf-8")
batch = Path(".github/workflows/publisher-visual-golden-batch01.yml").read_text(encoding="utf-8")

caller_start = "  publisher-visual-batch01:\n"
caller_end = "\n  typography-golden:\n"
assert reader.count(caller_start) == 1 and reader.count(caller_end) == 1
caller = reader.split(caller_start, 1)[1].split(caller_end, 1)[0]
assert "    uses: ./.github/workflows/publisher-visual-golden-batch01.yml\n" in caller

assert "  workflow_call:\n" in batch
assert "  push:\n    branches: [main]\n" in batch
assert '      - ".github/workflows/publisher-visual-golden-batch01.yml"\n' in batch

pinned = "0057852bfaa89a56745cba8c7296529d2fc39830"
path = "          path: target/publisher-visual-batch01-rust\n"
key = "          key: " + "$" + "{{ steps.batch01-cargo-cache-key.outputs.key }}\n"
restore = "      - name: Restore Batch01 Cargo build cache\n"
save = "      - name: Save trusted main Batch01 Cargo cache\n"
assert batch.count(restore) == 1 and batch.count(save) == 1
restore_step = batch.split(restore, 1)[1].split("\n      - ", 1)[0]
save_step = batch.split(save, 1)[1].split("\n      - ", 1)[0]
assert f"uses: actions/cache/restore@{pinned}" in restore_step
assert "uses: actions/cache@" not in restore_step
assert "id: batch01-cargo-cache" in restore_step
assert path in restore_step and key in restore_step

trusted = ("if: " + "$" + "{{ github.event_name == 'push' "
           "&& github.ref == 'refs/heads/main' "
           "&& steps.batch01-cargo-cache.outputs.cache-hit != 'true' }}")
assert trusted in save_step
assert f"uses: actions/cache/save@{pinned}" in save_step
assert path in save_step and key.rstrip("\n") in save_step
assert batch.index("name: Compare 55 pairs against source-free Publisher fingerprints") < batch.index(save)
assert batch.index("name: Enforce stage-aware exact PAGE membership authority") < batch.index(save)
assert batch.index("name: Print top residuals") < batch.index(save)
assert batch.index(save) < batch.index("name: Upload source-free Batch 01 visual receipt")

# Browser caching intentionally stays on its existing independent policy.
chromium = batch.split("      - name: Restore pinned Playwright Chromium cache\n", 1)[1]
chromium = chromium.split("\n      - name: Install pinned browser acceptance runtime\n", 1)[0]
assert f"uses: actions/cache@{pinned}" in chromium

print("Batch01 PR Cargo restore-only/main-save contract: PASS")
