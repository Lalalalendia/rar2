# RESCUE-TEXT-EVIDENCE-01 — source-backed text evidence (bounded producer)

This is a distinct, opt-in technical stage on `feat/RESCUE-TEXT-EVIDENCE-01`. It is **not** yet wired into automatic batch promotion. The shipping AutoRestore stage in `main` still supports verified images, physical CFB carriers and unsupported only.

## Local CLI contract

```sh
pub extract-text /path/to/source.pub --output /separate/private/story-evidence
python -m tools.verify_rescue_text_evidence_v1 /path/to/source.pub --output /separate/private/story-evidence
```

When the canonical parser admits text, the CLI creates:
- `stories.json`: full private UTF-8 Story text and each `story_key`, UTF-16 interval, content SHA-256 and canonical source provenance.
- `stories.txt`: a deterministic, human-readable private Story listing.
- The producer emits only a numeric count on stdout. Do **not** upload private outputs to public CI logs, artifacts, telemetry or source-free `summary.json`.

### Source authority

The command reuses `AdmittedSource::open` for bounded immutable local PUB snapshots.

1. For supported mature-0x2C, `pub_reader::build_mature_0x2c_source_graph` and its `graph.stories` must provide a **semantic, authoritative and exact** text `SourceRef`, with valid SHA-bound carrier range.
2. If mature opening fails, `pub_reader::probe_reader_salvage_candidate` and `build_reader_partial_source_graph` must independently admit the damaged source and produce typed `TextRange` facts. No raw strings carving, guessed UTF-16, or fabricated document pages.
3. All ranges check exact UTF-16 units; maximum 16,384 Story records, 16 MiB of promoted text, 24 MiB of serialized private JSON.
4. Output uses source-bound derived-file trust and a staging directory; rerunning an existing output is byte-exact or rejected without overwrite. Unknown or unproven sources do not produce a success artifact.

`tools.verify_rescue_text_evidence_v1` is an independently implemented byte-integrity and schema verifier: it re-hashes the original source, emitted Story text, UTF-16 units, manifest and `stories.txt`, checks exclusive expected output entries and symlink fences. It **does not independently parse Quill** and must never be reported as a second semantic oracle.

### Scope gates

- This stage produces source-proven text *ranges*, not the original document's visual page order, all linked frame positions, tables, images, font layout or a repaired `.pub`.
- The new CLI is **not** automatically invoked by Rescue batch; integration requires existing Windows Job / process, file/time and security constraints and full CI.
- No changes to existing `pub extract-assets`, parser, Reader/Editor, Windows workflows, or the user-paused #2852 pointer task.
- Verify real fixture support and behavior on both mature and damaged Publisher sources. A synthetic manifest proves byte-integrity rules, **not** semantic recovery.
- The 1556 private source corpus is unavailable and **must not** be called tested.

## Next gate

Add a frozen `pub.exe extract-text` Windows acceptance with independently source-backed fixture, then wire optional private text bundle into batch and AutoRestore after complete format/security review. Maintain the distinction between `source_exact_story_text`, `needs_review`, and `unsupported`.
