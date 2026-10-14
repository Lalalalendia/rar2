# Chaptera derived-file trust v1

Chaptera commits files derived from an admitted PUB through one shared boundary:
`AdmittedSource::commit_derived_output`. The boundary preserves exact output bytes,
prevents an output from aliasing its source, and applies the Windows host's native
Attachment Services policy while the admitted source identity is still held.

## Contract

1. The source must already be admitted by exact open-handle identity and SHA-256.
2. The destination is admitted by parent and target identity. Reparse targets and
   source aliases are rejected.
3. Bytes are committed by the existing sibling-candidate destination authority and
   verified by length, SHA-256, and final file identity.
4. If the source locator still names the admitted object, Windows Attachment Services
   receives that locator as the referrer and the exact output as the local path.
5. If the source locator was rebound, no origin is fabricated and the receipt says
   `no_origin_evidence`.
6. The source open handle is rehashed and the destination identity and main-stream
   bytes are checked again after host policy runs. Deletion, replacement, or mutation
   is a typed failure.

The result is a `DerivedFileTrustReceipt` with one of four dispositions:

- `applied`: the Windows host policy service ran successfully;
- `no_origin_evidence`: the admitted source path no longer proves origin;
- `filesystem_or_policy_not_supported`: exact output was committed, but the host has
  no supported policy service;
- `blocked_or_modified`: reserved for policy/output interference and represented by
  the typed `derived_file_blocked_or_modified` error when exactness cannot be proved.

Chaptera does not parse, copy, or synthesize a private alternate-data-stream policy.
Windows remains the policy authority. Hosted Windows acceptance proves both directions:
an Internet-zone source propagates ZoneId 3 to derived files, while a local source does
not acquire a fabricated Internet zone. The public fixture and receipts contain no
private source bytes or source paths.

## Current consumers

- exact asset extraction, including `manifest.json`;
- IDML and ODG output plus loss sidecars;
- bounded PDF output plus loss sidecars.

Reader asset browsing remains a separate downstream product task. This boundary only
establishes durable derived-file provenance and exact-byte safety.
