# M0: trusted read-only native Publisher research contract

Status: pre-execution safety contract. This file does not launch Publisher.

## Allowlisted experiment

- Packet: `tools/research-runner/experiments/quill-story-readonly-oracle-02.packet.json`
- Packet ID: `QUILL-STORY-READONLY-ORACLE-02`
- Operation: `tools/research-runner/operations/quill_story_readonly_oracle_02.ps1`
- Publisher profile: `publisher-2019`, bound to the executable SHA in the packet.
- The operation opens four exact SHA-pinned, public PUB witnesses read-only, checks that all inputs are byte-unchanged after opening, and produces `analysis/quill-story-readonly-oracle.json`, `logs/quill-story-readonly-oracle.txt`, and `environment.json`.
- Never publish raw PUB files, extracted document text, process logs with local paths, or arbitrary files.

## Admission and execution

1. Dispatch must originate from the trusted `main` commit, not from a PR checkout or untrusted comment.
2. The hosted gate runs the existing `validate_packet.py` with `publisher-2019`.
3. A Windows native executor must recheck the exact git SHA, packet SHA, operation path, and Publisher executable SHA before opening any document. A packet schema match alone is not sufficient to authorize a new operation.
4. Refuse to run when MSPUB is already active; serialize all use of the dedicated oracle host with the existing `pub-re-native-publisher-oracle` concurrency group.
5. Each native operation executes in a monitored child process with a deadline strictly shorter than the enclosing job; always preserve a source-safe stage receipt on failure. Terminate only proven experiment-owned processes.
6. Run existing `prepare_native_run.ps1` and `finalize_native_run.ps1`. Verify a receipt with four SHA-linked witnesses and `source_unchanged=true` for each. Distinguish a green job from a scientifically useful result.
7. Publish only allowlisted, sanitized evidence as a GitHub artifact and link it to the study in Notion.

## Negative controls

- Reject non-main ref and any packet or operation other than the one listed above.
- Reject wrong packet SHA, Publisher executable SHA, or source fixture SHA.
- Reject failure to spawn Publisher, child timeout, incomplete four-witness report, and any changed source witness.
- Ensure a failed or timed-out run yields an explicit invalid stage receipt without granting format authority.

## Scope and unresolved work

The legacy `pub-native-research.yml` only validates and prepares a manual handoff. The already deployed `pub-re-native.yml` provides a trusted self-hosted Windows execution lane and bounded watchdog for its explicitly approved commands.

This M0 contract is the next adapter to implement, not a claim that a new native workflow is running. Do not enable arbitrary packet dispatch on the existing Publisher runner.
