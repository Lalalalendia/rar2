# Chaptera development fast loop

\`tools/dev_fast_loop.py\` is the local edit/feature feedback loop for agents and humans working in \`Lalalalendia/rar2\`.

It deliberately does **not** replace Reader/Editor/product/deep acceptance. Its job is to answer the development question first: *does this small change compile and does its nearest micro-regression still pass?*

## Default edit loop

Preview the checks selected from \`main...HEAD\` plus staged, unstaged, and untracked files:

\`\`\`bash
python tools/dev_fast_loop.py --plan
\`\`\`

Execute them fail-fast:

\`\`\`bash
python tools/dev_fast_loop.py --run
\`\`\`

The edit loop routes only local checks that can be inferred safely from the changed paths:

- changed Python → \`py_compile\` + existing \`test_<name>.py\` companion;
- changed Node \`.js/.mjs/.cjs\` → \`node --check\` + existing \`<name>.test.*\` companion;
- changed GitHub workflow YAML → repository YAML syntax guard;
- changed Rust source → nearest package \`cargo fmt --check\` + \`cargo check\`;
- changed Rust integration test → its exact \`cargo test --test <target>\`;
- important mismatched component/test names → source-free commands from \`tools/dev_fast_loop_components.json\`;
- changed Cargo workspace manifest/lockfile → affected workspace fmt/check.

The command never schedules visual goldens, full corpora, native Publisher, installer/update, package/release or unrelated product acceptance.

### Component micro-test registry

\`tools/dev_fast_loop_components.json\` is the small data-driven exception table for components whose production module and fast regression test do not share a filename. Registry commands must stay source-free, deterministic, and suitable for the local edit loop. Network acquisition, ignored real-PUB acceptance, native Publisher, visual or packaging commands do not belong there.

The initial rules cover pub-editor paragraph alignment, table row/column history, and scoped text-format property history using bounded \`--lib\` tests already owned by their existing workflows. The registry is schema-validated and malformed entries fail closed.

## Feature loop

Before publishing a coherent feature slice, add package unit tests:

\`\`\`bash
python tools/dev_fast_loop.py --mode feature --run
\`\`\`

This is still not release acceptance. It is the second, slightly wider local loop.

Agents should keep iterating locally until the edit loop is green. Before publishing a coherent semantic slice as a new hosted head, run the feature loop once. Do not push a known local fast-loop failure merely to ask GitHub Actions for the same answer.

## Explicit paths

Agents can ask what a proposed change would cost without creating a git commit:

\`\`\`bash
python tools/dev_fast_loop.py \
  --paths vendor/producer-a/crates/pub-editor/src/imported_paragraph_alignment_v1.rs \
  --plan
\`\`\`

JSON is available for agent orchestration:

\`\`\`bash
python tools/dev_fast_loop.py --json --plan
\`\`\`

## Timing target

The warm edit-loop target is **10–60 seconds**. A first cold Rust compile can exceed that target; the script reports total time and warns when the configured budget is exceeded rather than silently widening coverage.

If one fast check remains slow after the compiler cache is warm, optimize or split that discriminator. Do not add broad acceptance to the edit loop merely because it already exists in CI.

## Development / acceptance split

\`\`\`text
edit repeatedly
  -> tools/dev_fast_loop.py --run
  -> coherent semantic slice
  -> tools/dev_fast_loop.py --mode feature --run
  -> one hosted PR head
  -> selective PR admission
  -> deep/integration/release evidence separately
\`\`\`

A green fast loop is **development evidence only**. Existing owning product gates remain authoritative for merge/release claims.
