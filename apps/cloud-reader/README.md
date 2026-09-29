# Chaptera Cloud Reader

Public, read-only web surface for opening Publisher (`.pub`) documents.

## Repository boundary

This app lives in the `rar2` monorepo so it can evolve atomically with the Viewer scene contract, but it is an independently deployable static web artifact. It is **not** embedded in the `chaptera-server` binary.

The first slice consumes:

```text
GET /v1/reader/documents/{document_id}/scene
```

and never calls the authoring commit API.

## Privacy boundary

Viewing a document and contributing it to Chaptera research are separate actions.

- A service upload may be processed temporarily to provide the requested viewing service.
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
