# Cloud Editor observability V1

This slice implements the public, vendor-neutral part of CLOUD-OBSERVABILITY-01 in `HeisLuka/miy`.

## Contract

Observability follows semantic identities for correlation but never becomes semantic authority.

- `client_operation_id` remains the idempotency/semantic operation identity.
- `trace_id` identifies one execution attempt and may change on retry.
- browser and service spans share the same trace context when one HTTP request crosses the boundary.
- metrics use bounded labels only.
- high-cardinality document/user/revision/operation identifiers are excluded from metric labels.
- document text, assets, raw PUB bytes, filenames, source hashes and signed resource URLs are not observability payload.

## Trace context

`chaptera.trace-context.v1` carries:

- trace_id
- interaction_id
- session_incarnation
- operation_class
- browser_family
- optional client_operation_id

The current public HTTP harness propagates these fields in bounded `x-chaptera-*` headers.

## Current public span coverage

Browser:

- `browser.scene_current`
- `browser.commit_http`
- `browser.scene_revision`

Synthetic service harness:

- `gateway.scene_current`
- `gateway.commit`
- `gateway.scene_revision`

This is deliberately not a production SLO claim. The current seam proves context propagation, privacy/cardinality guards and end-to-end correlation across an actual browser → HTTP process boundary.

## Metrics

The current in-memory public harness emits `cloud_editor_stage_latency_ms` with only bounded labels:

- stage
- operation_class
- outcome
- region
- protocol_major
- browser_family

DocumentId, RevisionId, StoryId, client_operation_id, interaction_id, trace_id, hashes, filenames and principal/user IDs are explicitly rejected as metric labels.

## Acceptance boundary

A green public run proves:

1. browser creates a bounded trace context;
2. commit HTTP request propagates it across CORS;
3. server records a `gateway.commit` span under the same trace;
4. the semantic `client_operation_id` is available for trace correlation but not metric labels;
5. browser and server trace summaries are machine-readable;
6. metrics contain no prohibited high-cardinality labels;
7. document payload is not recorded.

The run remains synthetic protocol plumbing: it does not close WEB-ACCEPTANCE-01 or establish numeric production SLOs. A later real-PUB acceptance run should reuse this trace contract rather than create a second telemetry model.

## 2026-10-15 — anonymous Temporary Guest Editor trace extension

The existing `chaptera.trace-context.v1` contract is reused for browser Guest upload/open and for the authenticated-by-private-capability, no-login temporary edit/Scene/export path. The browser keeps one opaque, tab-local `interaction_id` for this editing journey. Upload/create/open share one trace ID; a canonical commit, exact operation-ID retry, verified Current and Scene share another; the export report and matching binary export reuse one export trace ID. The semantic `client_operation_id`, revision and bearer identities remain separate from tracing.

`apps/web/temporary-editor-preview.mjs` loads the pre-existing, public Reader allowlisted `/observability-v1.mjs`, not a new telemetry provider. Missing/invalid telemetry is non-authoritative: all existing guest auth, TTL, write replay, loss report, Scene identity and artifact checks remain unchanged. In-memory browser spans only include bounded trace metadata, outcome and duration; the bearer, PUB/Story payload, filename, source hash and exact revision are never recorded in span labels.

The already-existing server `ReaderObservabilityV1` records sanitized HTTP request outcomes/latencies for `temporary.session_create`, `temporary.current`, `temporary.commit`, `temporary.scene`, `temporary.export` and `temporary.delete`. The middleware consults the static Axum `MatchedPath` route template, never real URL/query/source paths. The existing Reader create/upload/open/scanner/cleanup stages are retained without changes. Bounded operation class, outcome, region, protocol and browser labels are unchanged; new event stages are explicitly enumerated.

**Evidence boundary:** this code supplies request/route correlation, not isolated-worker per-substep spans, physical production tracing proof or new numeric SLOs. CI must verify real PUB browser/HTTPS edit/export and privacy fences, then a separate source-safe live receipt can join browser and server interaction IDs at an exact deployed SHA. A successful GitHub PR alone is not physical deploy.
