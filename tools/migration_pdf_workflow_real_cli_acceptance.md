# MIGRATION-PDF-WORKFLOW-REAL-CLI-01 — physical Rust/PUB acceptance

**Product:** Chaptera Migration local offline archive PDF operator route.
**Owners reused:** existing \`vendor/producer-a\` Rust \`pub\` PDF converter, \`rescue_batch_pdf_v1.py\` batch, and \`migration_operator_pdf_report_v1.py\` reporter. This change is **an acceptance gate**, not a new converter, editor, GUI, rescue algorithm or delivery certificate.

## Why it exists

The merged [MIGRATION-PDF-WORKFLOW-01](https://github.com/Lalalalendia/rar2/pull/2874) verified 16/16 tests against actual Python modules but with a **fake \`pub\` binary**. That proved wrapper integration, not actual conversion of any real PUB source to a real PDF.

This dedicated gate compiles the actual existing Rust \`pub\` on the runner and uses the known public Apache POI \`SampleNewsletter.pub\` pinned to Git SHA \`942d95d85b15d0dfdb3bc9ba1b4f273f277757c8\`, source SHA-256 \`6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf\`, size **291,840 bytes**. The fallback font comes from the existing Chaptera fallback-font resource binary; public CI does not embed/upload any font bytes.

## What is physically proven

Two real one-command user cases, using \`tools/migration_pdf_workflow_v1.py\`:

1. **Folder:** one SHA-exact valid PUB + one intentionally malformed .pub. Inventory reports two candidates without claiming support; actual Rust \`pub convert --to pdf\` produces a PDF and two loss sidecars for the good file. The bad file is a typed \`converter_failed\` exception, not silent success. The operator JSON/CSV refer to the exact batch manifest digest, and \`exceptions.csv\` contains only the failed source.
2. **Repeat:** rerun the same folder. The valid PDF is marked \`reused\` with unchanged exact output hashes and mtime, while the invalid file still fails. Corrupt its loss-text sidecar; a third run must reconvert and restore the original source-bound hash set.
3. **ZIP:** the exact same genuine source inside a safe ZIP. Inventory reports one candidate; actual conversion generates a real PDF and independent operator report, with zero exceptions; a second run reuses the same byte-exact outputs.

Every output is checked using the already-admitted \`successful_pdf\` verifier: canonical PDF header and report schema, original source and explicit fallback-font hash bindings, allowed PDF target and actual PDF/sidecar SHA. The report is checked for the exact batch manifest hash, full per-file denominator, non-silent pending/exception states, CSV completeness and \`integrity_confirmed_review_required\`.

**This test does not measure Microsoft Publisher-equivalent layout, image placement, text accuracy, PDF/A conformance, accessibility or customer approval.** It is a physical execution and integrity acceptance for a single well-defined public sample, not a new corpus-wide oracle.

## Source and artifact boundary

- The real PUB, generated licensed font, actual binary, PDF, sidecars and private operator CSV/JSON exist only in ephemeral runner directories.
- CI uploads **only** \`migration-pdf-real-cli-proof.json\`: public fixture SHA/size, compiled executable/font fingerprints, success/failure labels, output SHA digests and explicit false claims for visual equivalence and raw artifact upload. No user/customer bytes or local path labels are uploaded.
- Every exact source check fails closed. The acceptance script emits stable error codes, not private contents or file paths, on failure.
- This is a local operator proof, **not** authorization to expose unknown PUB files to an internet-facing converter. Separate \`MIGRATION-PDF-SEC-01\` worker isolation remains parked.

## Re-run and commands

Host-side checks do not need actual Publisher or proprietary customer input:

\`\`\`sh
python3 -m compileall -q tools/migration_pdf_workflow_real_cli_acceptance.py tools/test_migration_pdf_workflow_real_cli_acceptance.py
python3 -m unittest discover -s tools -p 'test_migration_pdf_workflow_real_cli_acceptance.py' -v
\`\`\`

Hosted exact Rust binary proof is the task-owned workflow \`.github/workflows/migration-pdf-workflow-real-cli.yml\`. It compiles \`pub-cli\` and materializes the font, fetches the exact public source and runs:

\`\`\`sh
python3 tools/migration_pdf_workflow_real_cli_acceptance.py \
  --source "$RUNNER_TEMP/SampleNewsletter.pub" \
  --source-sha256 6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf \
  --font "$RUNNER_TEMP/chaptera-fallback.ttf" \
  --pub-cli vendor/producer-a/target/debug/pub \
  --workflow tools/migration_pdf_workflow_v1.py \
  --receipt "$RUNNER_TEMP/migration-pdf-real-cli-proof.json"
\`\`\`

The acceptance run must complete with zero return code and a source-free artifact before this implementation is promoted. It does **not** change the existing \`PUB-MIGRATION-PILOT-01\` status: a real customer set, reviewer acceptance, separately certified editable route and full delivery remain pending. \`MIGRATION-UX-01\`, \`MIGRATION-PDF-SEC-01\`, \`MIGRATION-ACTUAL-OUTPUT-VALIDATE-01\` keep their existing parked/upstream statuses.

Do not restart the remaining legacy PUB corpus hunt under this task.
