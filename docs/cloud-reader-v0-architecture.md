# Cloud Reader V0 architecture

Status: accepted baseline, 2026-09-29.

## Product boundary

Cloud Reader is a separate read-only product surface in the `rar2` monorepo.

- `apps/cloud-reader` owns the independently deployable browser artifact.
- `apps/chaptera-server` owns Reader HTTP/API authorities.
- canonical PUB parse, semantic projection, Viewer and layout remain shared engine authority.
- Cloud Reader must never expose semantic commit/mutation capability.

## Two lifecycles

### Saved cloud document

```text
authenticated principal
  -> CAP_VIEW
  -> durable document/source/revision authority
  -> server Viewer projection
  -> versioned Reader Scene
  -> browser render
```

### Anonymous service upload

```text
anonymous browser
  -> trusted-edge public abuse control
  -> bounded upload admission
  -> opaque ephemeral Reader session
  -> private temporary quarantine
  -> scanner / isolated PUB open
  -> Viewer projection + classification
  -> versioned Reader Scene
  -> short TTL deletion
```

An anonymous Reader session is not a Workspace project, durable DocumentId, or revision stream.

## Research retention is separate

Uploading a file for viewing authorizes only service processing needed to provide the Reader.

Durable research retention requires a second explicit action:

```text
eligible unsupported/damaged classification
  -> explicit contribution consent
  -> short-lived one-shot intake capability
  -> private immutable research quarantine
  -> server SHA-256 / scanner / dedupe
  -> clustering / bounded research queue
```

Viewing must never imply corpus contribution.

## Scene authority

The browser consumes `chaptera.reader-scene.v1`, a versioned source-neutral DTO produced by the server from canonical Viewer/layout output.

The public Reader protocol must not expose `ViewerGeometryDocument` or parser/layout implementation DTOs directly. Canonical IDs, integer EMU geometry and exact transforms are preserved while internal Rust type evolution remains private.

Stacking is explicitly marked unknown until an authoritative stacking-order source exists.

## Security invariants

- raw PUB is hostile input;
- authoritative parse/open remains server-side and isolated;
- browser never supplies authoritative tenant/document/hash identity;
- no active content execution;
- no raw-byte/request-body logging;
- no public object URL for service or research bytes;
- request abuse control, upload admission and expensive-job quota are separate layers;
- corpus identity uses server-computed exact SHA-256;
- Cloud Reader actions cannot create semantic revisions.

## Current execution owners

- #223: independent Cloud Reader app + saved-document CAP_VIEW scene foundation.
- #226: anonymous ephemeral Reader-session ingress.
- #227: explicit consented corpus promotion.

Notion authority: `DECISION — Cloud Reader V0 architecture — authority, lifecycle, privacy — 2026-09-29`.
