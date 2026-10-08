#!/usr/bin/env python3
"""Guard supplemental Cargo PR restore-only and trusted-main-only cache writes."""

from pathlib import Path

wf = Path(".github/workflows/publisher-visual-golden-supplemental.yml").read_text(encoding="utf-8")

assert "  pull_request:\n" in wf
assert "  push:\n    branches: [main]\n" in wf
assert wf.count('      - ".github/workflows/publisher-visual-golden-supplemental.yml"\n') >= 2

pinned = "0057852bfaa89a56745cba8c7296529d2fc39830"
path = "          path: target/publisher-visual-supplemental-rust\n"
key = "          key: " + "$" + "{{ steps.supplemental-cargo-cache-key.outputs.key }}\n"
restore = "      - name: Restore supplemental Cargo build cache\n"
save = "      - name: Save trusted main supplemental Cargo cache\n"
assert wf.count(restore) == 1 and wf.count(save) == 1
restore_step = wf.split(restore, 1)[1].split("\n      - ", 1)[0]
save_step = wf.split(save, 1)[1].split("\n      - ", 1)[0]
assert f"uses: actions/cache/restore@{pinned}" in restore_step
assert "uses: actions/cache@" not in restore_step
assert "id: supplemental-cargo-cache" in restore_step
assert path in restore_step and key in restore_step
assert 'echo "restore_prefix=publisher-visual-supplemental-rust-v1-${RUNNER_OS}-rust-1.94.1-" >> "$GITHUB_OUTPUT"' in wf
restore_prefix = "          restore-keys: |\n            " + "$" + "{{ steps.supplemental-cargo-cache-key.outputs.restore_prefix }}"
assert restore_prefix in restore_step

trusted = ("if: " + "$" + "{{ github.event_name == 'push' "
           "&& github.ref == 'refs/heads/main' "
           "&& steps.supplemental-cargo-cache.outputs.cache-hit != 'true' }}")
assert trusted in save_step
assert f"uses: actions/cache/save@{pinned}" in save_step
assert path in save_step and key.rstrip("\n") in save_step
assert wf.index("name: Render new hosted supplemental set through current Reader") < wf.index(save)
assert wf.index("name: Build current-Reader supplemental execution receipt") < wf.index(save)
assert wf.index("name: Enforce Reader execution acceptance and report surface-count differences") < wf.index(save)
assert wf.index(save) < wf.index("name: Upload source-free supplemental receipt")

# Browser caching intentionally stays on the existing independent policy.
chromium = wf.split("      - name: Restore pinned Playwright Chromium cache\n", 1)[1]
chromium = chromium.split("\n      - name: Install pinned browser acceptance runtime\n", 1)[0]
assert f"uses: actions/cache@{pinned}" in chromium

print("supplemental PR Cargo restore-only/main-save contract: PASS")
