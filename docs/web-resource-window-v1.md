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

The source-neutral SafePreview host already freshly encodes approved image
derivatives and verifies `encoded_artifact_sha256` against its worker receipt.
The new Reader Scene delivery metadata now carries that **derivative artifact**
hash, its `safe-preview:<encode_spec_sha256>` identity and exact `byte_len`
*only if the host admits the inline preview within the Scene size budget*.
The Editor adapter maps that verified server receipt to `content_hash` without
confusing it with the original PUB resource hash. The page-window index also
understands the direct Reader Scene field. Legacy/unadmitted resources without
the receipt remain explicitly unschedulable and render with existing behavior.

The `chaptera.reader-scene.v1` still embeds the complete admitted image's
`inline_data_url` inside the Scene: no page-scoped HTTP demand/cache savings
are claimed by carrying a verified hash.

## Product SafePreview SVG admission (bounded integration slice)

The Product Editor route now opts into `materializeImageHref` in the **existing**
Reader SVG painter through `RichReaderEditorShellV1`. Its
`ProductSafePreviewImageAdmissionV1` captures only already host-admitted
SafePreview derivatives with exact encoded-artifact hash/recipe/byte length.
For those resources, the painter constructs a source-neutral pending outline
without assigning SVG `href` on off-window pages. Legacy/unreceipted image
paths retain the previous eager painter unchanged.

`WebResourceViewportObserverV1` supplies the current/visible page set to the
existing scheduler, whose `ResourceReadinessRuntimeV1` fences late completions.
The inline delivery adapter checks encoded length and SHA-256 on demand via
WebCrypto (never rehashing all 500 manifest items), then invokes the existing
browser `createImageBitmap` options under conservative single-decode working-set
reservations. Only **ready + still visible** images receive SVG `href`.
A rejected hash/oversized preflight does not publish a ready image. A true
scheduler *setup* failure alone falls back to the old painter; the normal
integrity-failure path does not silently bypass verification.

Important limits remain:

1. The entire approved inline data URL still arrives inside Reader Scene.
   This is **not** per-page network/CDN/CacheStorage delivery. Those functions
   remain owned by WEB-RESOURCE-DELIVERY-CACHE-01 and its upstream derivative
   producer; this code does not add a second HTTP authority.
2. `createImageBitmap` preflight is bounded, but SVG `href` can trigger an
   additional native browser decode. The exact SVG paint/GPU memory working set
   has **not** been measured or proven strictly limited. Fonts still load eagerly.
3. The initial Product policy uses no neighbor prefetch (`near_before = 0`,
   `near_after = 0`) until representative WEB-SCENE-SCALE evidence provides a
   justified radius. The scheduler remains configurable and source-neutral.
4. Browser/real-PUB acceptance needs page 1 -> 80, rapid-scroll, hidden-resume,
   shared/decorative image, error and memory receipts in Chromium and Firefox.
   Do not label initial paint complete before the SVG `load` event.
5. A complete 500-page SVG DOM is still constructed. Resource admission alone
   does not solve page/shard virtualization or the existing full-document font
   installation.

This slice is a concrete **product lifecycle/render hook**, not a claim that
network delivery, all font resources, or complete browser memory authority are
now solved. Canonical Scene, revision and PUB bytes are unchanged.

## Test coverage

`node --test apps/web/resource-readiness-v1.test.mjs
apps/web/web-resource-scheduler-v1.test.mjs
apps/web/web-resource-window-v1.test.mjs`

Includes 500-page bounded selection, shared resources, priorities, page 1->80,
verified-hash fail-closed handling, SafePreview host artifact identity, font/border dependents, hidden-tab policies,
uncancellable prior work, observer scroll coalescing, IntersectionObserver
500-page fast path, and real scheduler + readiness composition.