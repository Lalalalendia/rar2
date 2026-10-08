#!/usr/bin/env python3
"""Guard PR restore-only Cloud cache and trusted-main cache seeding."""

from pathlib import Path

reader = Path(".github/workflows/reader-pr-ci.yml").read_text(encoding="utf-8")
cloud = Path(".github/workflows/cloud-reader-reference-pairs.yml").read_text(
    encoding="utf-8"
)

caller_start = "  cloud-reference:\n"
caller_end = "\n  virginia-page-role:\n"
assert reader.count(caller_start) == 1 and reader.count(caller_end) == 1
job = reader.split(caller_start, 1)[1].split(caller_end, 1)[0]
assert "    cache-mode: read\n" in job
assert "    cache-mode: write\n" not in job
assert "    uses: ./.github/workflows/cloud-reader-reference-pairs.yml\n" in job
assert "needs: [classify, tier-a]" in job

# The same workflow is a trusted main-push seeder after merge.
assert "  workflow_call:\n" in cloud
assert "  push:\n    branches: [main]\n" in cloud
assert '      - ".github/workflows/cloud-reader-reference-pairs.yml"\n' in cloud

key = "          key: ${{ steps.cloud-reference-cache-key.outputs.key }}\n"
path = "          path: target/cloud-reader-reference-rust\n"
pinned = "0057852bfaa89a56745cba8c7296529d2fc39830"
restore = "      - name: Restore Cloud reference Cargo build cache\n"
save = "      - name: Save trusted main Cloud reference Cargo cache\n"
assert cloud.count(restore) == 1 and cloud.count(save) == 1
restore_step = cloud.split(restore, 1)[1].split("\n      - name: ", 1)[0]
save_step = cloud.split(save, 1)[1].split("\n      - name: ", 1)[0]

# Never a combined restore+post-save action in PR; explicit read-only restore.
assert f"uses: actions/cache/restore@{pinned}" in restore_step
assert "uses: actions/cache@" not in restore_step
assert path in restore_step and key in restore_step
assert "id: cloud-reference-cargo-cache" in restore_step

# A trusted main push is the ONLY allowed writer, after production acceptance.
trusted_gate = (
    "if: ${{ github.event_name == 'push' "
    "&& github.ref == 'refs/heads/main' "
    "&& steps.cloud-reference-cargo-cache.outputs.cache-hit != 'true' }}"
)
assert trusted_gate in save_step
assert f"uses: actions/cache/save@{pinned}" in save_step
assert path in save_step and key in save_step
assert cloud.index("name: Compare Cloud Reader rasters") < cloud.index(save)
assert cloud.index("name: Build source-neutral pair summary") < cloud.index(save)

# Runtime mode is useful telemetry but not a reliable security dependency:
# a prior GitHub job reported an empty ACTIONS_CACHE_MODE despite a read cap.
assert "cloud_reference_cache_mode=" in cloud
assert "cloud_reference_cargo_cache_hit=" in cloud
assert "cloud-reader-reference-rust-v2-" in cloud
assert "name: Build canonical guest Scene producer" in cloud

print("Cloud reference PR restore-only/main-save contract: PASS")
