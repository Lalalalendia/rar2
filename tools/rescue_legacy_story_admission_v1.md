# RESCUE-LEGACY-STORY-ADMISSION-01

Owner: GitHub issue #2861 and Notion task 3fc32a84-beec-8122-affe-dd723895d9ee.
This is a bounded Chaptera Rescue text extension, not a native PUB writer.

## Exact corpus diagnostic baseline

Pinned source artifact 11038217075, 1050 SHA-verified real PUB:
- 389 mature 0x2C parsed, 382 verified-positive / 5506 Story ranges
- 649 legacy 0x22 without Quill parsed, 626 positive
- 3 legacy 0x22 with Quill parsed, all 3 positive
- 9 unclassified, 30 classified with no proven Story
- 629 newly positive legacy PUB / 6158 provenance-backed Story ranges
- 79 legacy PUB with both verified and ambiguous Story candidates
Probe result: run 39076309385, artifact 12117480288.
The probe checks existing canonical SourceGraphs and exact Semantic
SourceRefs but does not independently reconstruct semantic Story text.

## Opt-in exporter

Command: pub extract-text SOURCE.pub --output PRIVATE_DIR --include-legacy
Without --include-legacy, the mature and eligible-damaged-source path
remains unchanged. With the flag, existing legacy 0x22 Quill and
no-Quill source graph builders are reused. Unadmitted Stories are
omitted and counted in the private manifest; no raw UTF-16 carving,
guessed codepages, fake page/frame ordering or restored native PUB.
Legacy no-Quill text remains bounded by the existing exact ASCII
profile. New extraction/evidence classes identify the legacy family.

Private stories.json/stories.txt are committed source-exact by SHA,
verified again by the independent byte-integrity validator and never
uploaded to CI logs/artifacts. This validator checks file bytes,
SHA, UTF-16 interval, source identity and claimed source carrier
bounds; it is not a second semantic parser.

## Version-bound batch replay

Command: rescue-engine run --output PRIVATE_QUEUE --pub-cli pub.exe --legacy-text-v1
The flag requires a pub CLI and opts into one re-probe of previously
final unavailable Story files last attempted under mature_v1. Private
text_profile=legacy_0x22_v1 prevents repeated retries at the same
capability version. Older verified evidence is re-validated and
retained. The source PUB, physical CFB bundles and previously
materialized image/stream galleries are never rewritten.

## Acceptance, limitations

Exact-head Rust/Python tests, Windows frozen runner (no installed
Python in child PATH) and real 1050-PUB SHA-pinned private export
roundtrip are required before promotion. Public receipts contain
only typed counts and SHA identity. Previous 382 mature positives
must remain unchanged, original-source identity and repeated runs
must pass, and tampered text fails closed. No independent Publisher
semantic/layout oracle, native repaired PUB or 1556/1556 claim.
