# RESCUE-AUTORESTORE-01 — automatic evidence-based recovery output

The **existing Rescue batch** calls `rescue_salvage_bundle_v1.read_bundle_report` before promoting any result. The new auto-presentation stage composes that **same** independent verification authority and does not parse CFB, copy or modify original PUBs, or invent a native repair writer.

## On-disk layout

```text
<dedicated output>/
  queue-private.json             LOCAL ONLY — source paths and original filenames
  summary.json                   source-free (SHA, status, counts, auto_tier, auto_index)
  entries.csv                    source-free machine-sortable queue report
  bundles/<source_sha256>/        existing immutable physical/assets salvage
    bundle.json
    physical/manifest.json       if verified streams exist
    physical/streams/sid-*.bin
    images/manifest.json         if extracted images passed validation
    images/<verified filename>
  recovered/<source_sha256>/      new atomic, immutable preview
    recovery.json                exact SHA, selected tier, manifest binding
    index.html                   offline links into the verified local bundle
```

Open `recovered/<full_sha256>/index.html` locally. The summary/CSV `auto_index` path points there and is generated **only from a validated SHA**; no source file name or private path enters public reports. The page has strict HTML escaping, no JavaScript, and a restrictive CSP. Keep `bundles/` and `recovered/` together: HTML links reference verified files rather than duplicating large payloads. No server, network access, cloud upload, PDF generation, document text claims, or per-page reconstruction is involved.

## Automatic precedence

| `auto_tier` | Evidence required | User-visible result |
| --- | --- | --- |
| `verified_images` | `images.status=verified` with at least one independently verified asset | Offline asset list/gallery (not reconstructed pages) plus any independently verified CFB stream links |
| `verified_streams` | At least one independently verified physical CFB SID stream and no verified images | Offline raw carrier index with exact SID links |
| `unsupported` | Validated bundle but no promoted image/SID artifacts | Explicit unsupported local record, not a successful reconstruction |

**NOT DELIVERED:** an allegedly repaired `.pub`, recovered semantic story text, original page layout, or a guaranteed healthy original. Native PUB repair requires an independent `RepairPlan` / writer gate and explicit user approval. A retained `original.pub` is still an unchanged original, **not** a repaired file.

## Durability and boundary

- Each unique source SHA owns at most one `recovered/<SHA>` directory, committed by staging + rename. Re-running verifies byte-for-byte output; it does **not** overwrite tampered existing content.
- An existing successfully verified salvage bundle is rechecked before a missing legacy auto-presentation can be backfilled.
- If the auto-presentation fails or is tampered, the batch reports `failed/auto_restore_integrity`. The underlying immutable salvage bundle remains intact. The user must investigate unexpected contents; retries cannot silently destroy evidence.
- Private path mappings remain only in `queue-private.json`; public reports are source-free. The preview itself is a **local private** output and is not an approved public CI artifact.
- Existing Windows Job Object, worker cancellation tokens, source immutability checks, file and time limits, and exact source SHA gates are retained.
- Tests use only synthetic contents and independently verified CFB fixtures. The full private registered 1,556 PUB corpus has not been physically replayed.

## Acceptance

Unit tests: unsupported, verified-image-only selector, true 2-SID synthetic CFB, source unchanged, repeat idempotency, legacy auto backfill, tamper, symlink and unsafe asset name rejection, private-string public-report guard. Portable Windows smoke: frozen `rescue-engine.exe` with Python removed from PATH must create the exact SHA presentation and reject tampered stream bundles. Visual browser layout on a real Windows host and full real-corpus replay are separate acceptance gates.
