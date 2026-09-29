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
