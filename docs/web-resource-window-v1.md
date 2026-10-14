# WEB-RESOURCE-SCHEDULER-01 — current-page/window demand binding

Task: https://app.notion.com/3e732a84beec815c815de6f02696d24a

## Scope implemented

`apps/web/web-resource-window-v1.mjs` derives the current visible blocking set and
bounded caller-selected near/speculative page demands. It composes the already
merged `WebResourceSchedulerV1` via `updateDemands()` and does not own fetch,
decode, ResourceId, HTTP/cache, original PUB bytes, Scene patches or revisions.

- Indexing a 500-page Scene does NOT create network/decode tasks for 500 pages.
- Resource demand is derived from page-scoped image node IDs, decorative border
  placements and resolved text font IDs (including font spans).
- A resource shared across pages produces one demand per page consumer. The
  existing scheduler deduplicates exact identities and promotes active fetches.
- Priorities are `VisibleBlocking`, explicit `VisibleUpgrade`, `NearWindow` and
  `Speculative`. The caller chooses the neighbor window; there is no baked-in
  `+/-1` radius.
- `WebResourceViewportObserverV1` chooses the page with greatest intersection,
  also includes all other visible pages, and sends fresh generations on scroll,
  visibility changes and scene refresh. It prefers browser `IntersectionObserver`
  so there are no per-scroll layout reads for all 500 page elements. A throttled
  `getBoundingClientRect` fallback supports environments without that API.
- `bindResourceWindowV1` wires an already rendered SVG page host and an existing
  resource scheduler. Its caller retains ownership of scheduler disposal.
- Missing or unverified `content_hash` is NOT fabricated and is NOT marked
  ready; `unsupported_visible_count` explicitly reports unschedulable assets.

### Example binding (after the SVG pages exist)

```js
import { bindResourceWindowV1 } from './web-resource-window-v1.mjs';
import { WebResourceSchedulerV1 } from './web-resource-scheduler-v1.mjs';
import { ResourceReadinessRuntimeV1 } from './resource-readiness-v1.mjs';

// All three IDs must come from the authoritative Scene, not a browser guess.
const readiness = new ResourceReadinessRuntimeV1({
  document_id: scene.document_id,
  revision_id: scene.revision_id,
  snapshot_id: scene.snapshot_id,
});
const scheduler = new WebResourceSchedulerV1({
  readiness,
  fetch_resource: authorizedFetchAdapter,
  decode_resource: resourceMaterializationAdapter,
  config: measuredProductBudgets,
});
const binding = bindResourceWindowV1({
  scene,
  scheduler,
  host: document.querySelector('#canvas'),
  scroll_container: document.querySelector('#canvas-wrap'),
  document_object: document,
  near_before: measuredBefore,
  near_after: measuredAfter,
  on_projection: projection => {
    // NOT a page-ready verdict while unsupported_visible_count > 0.
    reportWindowCounters(projection);
  },
});

// On view disposal / document or canonical Scene identity change:
binding.dispose();
scheduler.dispose();
```

The code above is an **integration contract**, not a claim that the current
product route already supplies the required adapters or has been wired.

## Exact remaining product gap (as of 2026-10-14)

The current `chaptera.reader-scene.v1` used by
`apps/web/product-editor-entry-v1.mjs` and
`apps/web/rich-reader-editor-shell-v1.mjs` transports image resources as
`inline_data_url`. `apps/web/reader-scene-editor-adapter-v1.mjs` currently maps
image `content_hash` to `null`. Fonts may carry `expected_sha256`. Therefore a
ResourceId/hash/derivative descriptor cannot be assumed for every image.

`apps/cloud-reader/render-v1.mjs` currently sets SVG image `href` directly and
installs all inline fonts during initial rendering. Simply invoking a resource
scheduler in the product entry point would NOT limit those independent native
browser fetch/decode/materialization paths. Do not wire a fake immediate
`decode_resource` just to make telemetry green: it would publish a dishonest
ready state and leave physical decoding unbounded.

Required before claiming **actual online editor integration**:

1. The source-neutral Reader Scene / approved delivery adapter must expose or
   provide per-image **verified** immutable `content_hash` and derivative
   identities, plus cost hints when known, without hashing the complete 500-page
   manifest up front or leaking raw PUB bytes.
2. Supply the existing authorized fetch/cache adapter and a real materialization
   adapter. Avoid double-decoding and ensure decode reservations correspond to
   physical work; use the already owned image-decode and HTTP-cache boundaries.
3. Make the Reader SVG presentation defer out-of-window materialization until
   readiness, and repaint only the affected pages. Preserve fidelity and pointer
   interactions; do not confuse visual placeholders with exact images.
4. Bind lifecycle to the current product route and test page 1 -> 80, rapid
   scroll/zoom, failed/offline/hidden state and 500-page documents in real
   Chromium/Firefox, with first useful page + memory receipts.

**No runtime UI wiring and no actual-PUB browser benchmark is claimed by this
slice.** The existing core scheduler and readiness authority remain unchanged.

## Test coverage

`node --test apps/web/resource-readiness-v1.test.mjs
apps/web/web-resource-scheduler-v1.test.mjs
apps/web/web-resource-window-v1.test.mjs`

Includes 500-page bounded selection, shared resources, priorities, page 1->80,
verified-hash fail-closed handling, font/border dependents, hidden-tab policies,
uncancellable prior work, observer scroll coalescing, IntersectionObserver
500-page fast path, and real scheduler + readiness composition.