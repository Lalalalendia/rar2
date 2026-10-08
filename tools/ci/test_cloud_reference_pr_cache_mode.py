#!/usr/bin/env python3
"""Guard PR read-only Cloud reference cache; keep trusted main warm-up unchanged."""

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

# The standalone workflow still runs on trusted main pushes, including changes
# to itself. The PR caller's cache limit must not weaken main cache seeding.
assert "  workflow_call:\n" in cloud
assert "  push:\n    branches: [main]\n" in cloud
assert '      - ".github/workflows/cloud-reader-reference-pairs.yml"\n' in cloud

# Same cache key/path/action and real build; only access mode/telemetry changed.
assert cloud.count("name: Restore/save Cloud reference Cargo build cache") == 1
assert cloud.count("id: cloud-reference-cargo-cache") == 1
assert (
    "uses: actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830"
    in cloud
)
assert "          path: target/cloud-reader-reference-rust\n" in cloud
assert "          key: ${{ steps.cloud-reference-cache-key.outputs.key }}\n" in cloud
assert "cloud-reader-reference-rust-v2-" in cloud
assert "name: Build canonical guest Scene producer" in cloud

# Explicit runtime measurement. PR workflows must fail closed if GitHub does
# not apply read-only scoped cache tokens, before cache restore or build.
probe = "      - name: Verify effective Cloud reference PR cache access\n"
restore = "      - name: Restore/save Cloud reference Cargo build cache\n"
assert cloud.count(probe) == 1
assert cloud.index(probe) < cloud.index(restore)
assert '"$GITHUB_EVENT_NAME" == "pull_request"' in cloud
assert '"${ACTIONS_CACHE_MODE:-}" != "read"' in cloud
assert "cloud_reference_cache_mode=" in cloud
assert "cloud_reference_cargo_cache_hit=" in cloud

print("Cloud reference PR main-cache restore-only contract: PASS")
