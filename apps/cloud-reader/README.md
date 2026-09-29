# Chaptera Cloud Reader

Public, read-only web surface for opening Publisher (`.pub`) documents.

## Repository boundary

This app lives in the `rar2` monorepo so it can evolve atomically with the Viewer scene contract, but it is an independently deployable static web artifact. It is **not** embedded in the `chaptera-server` binary.

The read-only product consumes two bounded surfaces:

```text
GET  /v1/reader/documents/{document_id}/scene

POST /v1/reader/guest-sessions
PUT  /v1/reader/guest-sessions/{session_id}/content
POST /v1/reader/guest-sessions/{session_id}/open
GET  /v1/reader/guest-sessions/{session_id}/scene
```

The guest capability token remains in browser memory, is sent only in the
`x-chaptera-reader-session` header and is never placed in the URL or web
storage. The app never calls an authoring commit/Project/Workspace endpoint.

## Privacy boundary

Viewing a document and contributing it to Chaptera research are separate actions.

- A service upload is processed in private quarantine only for the requested viewing service.
- Hostile-file scanning and Viewer scene production are server-side; Viewer open runs in the resource-limited isolation worker.
- Guest quarantine bytes and session metadata are deleted at the short service TTL.
- It must not become a durable corpus/research fixture by default.
- Exact-byte research retention requires a separate explicit contribution consent.
- Durable contribution intake belongs to the existing private quarantine / dedupe / safety / clustering pipeline.

## V0 evolution

1. authenticated durable-document read-only scene;
2. anonymous ephemeral service upload;
3. supported / partial / unsupported classification;
4. separate explicit contribution CTA for eligible high-value files;
5. private durable research promotion only after consent.

Cloud Editor mutation, collaboration, undo/redo and authoring export are intentionally out of scope.

## Reading controls

The interface consumes the same server-owned `chaptera.reader-scene.v1` DTO and
SVG renderer. Pages follow canonical order, with fit-width and percentage zoom,
page selection and PgUp/PgDn keyboard navigation in the focused page view.
Zoom changes only the SVG viewport dimensions.

Recovered stories provide literal, case-insensitive Unicode search and plain-text
selection/copy. Search selects an exact recovered-text range; the scene contract
does not currently identify its page location. Images can be saved only when the
service supplies an admitted inline PNG/JPEG/GIF resource, bounded to 4 MiB each
and 8 MiB in total. Display limitations remain visible alongside the document.

Opening can be cancelled or replaced by dropping another file. A superseded
network response or delayed font activation cannot publish an old document.
Guest paths, session IDs and response protocol versions must agree before the
client advances; capability tokens never go to another origin.

## UI validation

```sh
python3 apps/cloud-reader/check_contract.py
node --test apps/cloud-reader/render-v1.test.mjs apps/cloud-reader/reader-model.test.mjs
npm install --no-save playwright@1.55.0
npx playwright install --with-deps chromium
node apps/cloud-reader/reader-browser.test.mjs
```

The existing Cloud Reader workflow runs these checks and keeps desktop/mobile
screenshots and an exact-head receipt under `target/cloud-reader-ui/`. Inputs are
public-safe synthetic DTOs. This UI suite does not establish real-PUB visual
fidelity, live hostile-file scanning, deployed cleanup or contribution consent.
Those require their own release evidence.
