// Product-only SafePreview image admission. This composes the existing resource
// scheduler, readiness and page-window policies; no PUB or Scene authority.
import { WebResourceWindowIndexV1, WebResourceViewportObserverV1 } from './web-resource-window-v1.mjs';
import { WebResourceSchedulerV1 } from './web-resource-scheduler-v1.mjs';
import { ResourceReadinessRuntimeV1, resourceKey } from './resource-readiness-v1.mjs';
import { EXACT_CREATE_IMAGE_BITMAP_OPTIONS } from './image-decode-v1.mjs';

export const PRODUCT_IMAGE_ADMISSION_SCHEMA = 'chaptera.product-safe-preview-image-admission.v1';
const EXACT_HASH = /^[0-9a-f]{64}$/;
const DERIVATIVE = /^safe-preview:[0-9a-f]{64}$/;
const IMAGE_DATA_URL = /^data:image\/(?:png|jpeg);base64,([A-Za-z0-9+/]+={0,2})$/;
const MIMES = new Set(['image/png','image/jpeg']);
const MAX_PREVIEW_PIXELS = 16_000_000; // Existing host SafePreview encoder ceiling.

// Conservative in-memory decode reservation for UNKNOWN page-derived costs.
// Values are product knobs, not a universal scheduler policy or prefetch law.
export const PRODUCT_SAFE_PREVIEW_BUDGET = Object.freeze({
  max_network_inflight: 2,
  max_decode_inflight: 1,
  max_network_bytes_inflight: 12 * 1024 * 1024,
  max_decode_bytes_reserved: 64 * 1024 * 1024,
  max_decoded_pixels_inflight: MAX_PREVIEW_PIXELS,
  unknown_decode_bytes_reservation: 64 * 1024 * 1024,
  unknown_pixels_reservation: MAX_PREVIEW_PIXELS,
});

function exactManifestImage(resource) {
  if (!resource || !MIMES.has(resource.mime) ||
      !EXACT_HASH.test(resource.encoded_artifact_sha256 ?? '') ||
      !DERIVATIVE.test(resource.derivative_id ?? '') ||
      resource.availability !== 'inline_data_url' ||
      !Number.isSafeInteger(resource.byte_len) || resource.byte_len <= 0 ||
      resource.byte_len > 5 * 1024 * 1024) return null;
  const href = resource.inline_data_url;
  if (typeof href !== 'string' || !IMAGE_DATA_URL.test(href) ||
      !href.startsWith('data:' + resource.mime + ';base64,')) return null;
  return Object.freeze({
    identity: Object.freeze({
      resource_id: resource.resource_id,
      content_hash: resource.encoded_artifact_sha256,
      derivative_id: resource.derivative_id,
    }),
    mime: resource.mime,
    byte_len: resource.byte_len,
    href,
  });
}

// Browser preflight verifies the exact host-derived encoded artifact. It does
// NOT call a new resource endpoint or reinterpret raw PUB image containers.
export async function verifySafePreviewInlineV1(meta, { subtle, atob_fn = globalThis.atob, signal } = {}) {
  if (!meta || typeof subtle?.digest !== 'function' || typeof atob_fn !== 'function') {
    throw new TypeError('WebCrypto and strict base64 decoder required');
  }
  if (signal?.aborted) throw new Error('resource_fetch_aborted');
  const match = IMAGE_DATA_URL.exec(meta.href);
  if (!match || !meta.href.startsWith('data:' + meta.mime + ';base64,')) {
    throw new Error('safe_preview_inline_url_invalid');
  }
  // Refuse unexpectedly large encoded input before allocation/decode.
  if (match[1].length > Math.ceil(meta.byte_len / 3) * 4 + 4) {
    throw new Error('safe_preview_inline_size_invalid');
  }
  const binary = atob_fn(match[1]);
  if (binary.length !== meta.byte_len) throw new Error('safe_preview_inline_size_mismatch');
  const bytes = Uint8Array.from(binary, char => char.charCodeAt(0));
  const digest = new Uint8Array(await subtle.digest('SHA-256', bytes));
  if (signal?.aborted) throw new Error('resource_fetch_aborted');
  const hash = Array.from(digest, byte => byte.toString(16).padStart(2, '0')).join('');
  if (hash !== meta.identity.content_hash) throw new Error('safe_preview_inline_sha256_mismatch');
  return Object.freeze({ bytes, mime: meta.mime });
}

class NotifyingReadinessV1 extends ResourceReadinessRuntimeV1 {
  constructor(identity, on_ready) {
    super(identity);
    this.onReady = on_ready;
  }
  completeReady(token, result) {
    const receipt = super.completeReady(token, result);
    if (receipt.accepted) this.onReady?.(token);
    return receipt;
  }
}

// Fail closed only for an EXACT host-admitted SafePreview derivative. Older
// inline images are left to the existing Reader renderer unchanged.
export class ProductSafePreviewImageAdmissionV1 {
  constructor({
    host, scroll_container, document_object = globalThis.document,
    crypto_object = globalThis.crypto,
    create_image_bitmap = globalThis.createImageBitmap?.bind(globalThis),
    atob_fn = globalThis.atob,
    scheduler_config = PRODUCT_SAFE_PREVIEW_BUDGET,
    make_scheduler = options => new WebResourceSchedulerV1(options),
    make_observer = options => new WebResourceViewportObserverV1(options),
    near_before = 0, near_after = 0,
  } = {}) {
    if (!host || !scroll_container) throw new TypeError('product canvas host and scroll container required');
    if (typeof make_scheduler !== 'function' || typeof make_observer !== 'function') {
      throw new TypeError('resource scheduler and observer factories required');
    }
    this.host = host;
    this.scrollContainer = scroll_container;
    this.documentObject = document_object;
    this.cryptoObject = crypto_object;
    this.atob = atob_fn;
    this.createImageBitmap = create_image_bitmap;
    this.schedulerConfig = scheduler_config;
    this.makeScheduler = make_scheduler;
    this.makeObserver = make_observer;
    this.nearBefore = near_before;
    this.nearAfter = near_after;
    this.available = typeof crypto_object?.subtle?.digest === 'function' &&
      typeof create_image_bitmap === 'function' && typeof atob_fn === 'function';
    this.epoch = 0;
    this.scene = null;
    this.index = null;
    this.catalog = new Map();
    this.bindings = new Map();
    this.readiness = null;
    this.scheduler = null;
    this.observer = null;
    this.visible = new Set();
    this.lastProjection = null;
    this.disposed = false;
    this.mounted = false;
    this.paintCount = 0;
    this.retirement = Promise.resolve();
    this.retiring = false;
    this.pendingSpec = null;
  }

  #clearScene() {
    this.observer?.dispose();
    this.observer = null;
    const retiringScheduler = this.scheduler;
    if (retiringScheduler) {
      retiringScheduler.dispose();
      // Abort is not physical completion. Across new Scene revisions, do NOT
      // overlap old uncooperative bitmap/fetch callbacks with a fresh budget.
      this.retiring = true;
      this.retirement = Promise.all([
        this.retirement, retiringScheduler.whenIdle(),
      ]).then(() => undefined);
    }
    this.scheduler = null;
    this.pendingSpec = null;
    for (const bindings of this.bindings.values()) {
      for (const binding of bindings) {
        binding.image.removeEventListener?.('load', binding.onLoad);
        binding.image.removeEventListener?.('error', binding.onError);
      }
    }
    this.bindings.clear();
    this.catalog.clear();
    this.readiness = null;
    this.index = null;
    this.scene = null;
    this.visible.clear();
    this.lastProjection = null;
    this.mounted = false;
  }

  prepare(scene) {
    if (this.disposed) throw new Error('image admission disposed');
    this.epoch++;
    this.#clearScene();
    // Scene revision and page identity remain exclusively server-owned.
    this.index = new WebResourceWindowIndexV1(scene);
    this.scene = scene;
    if (!this.available) return;
    for (const resource of scene.resources ?? []) {
      const admitted = exactManifestImage(resource);
      if (admitted) this.catalog.set(resource.resource_id, admitted);
    }
  }

  // Called synchronously while the renderer builds a specific SVG <image>.
  // Returning true is the ONLY path which suppresses the eager href assignment.
  captureImageHref({ image, resource, page_id, href } = {}) {
    if (!this.available || !this.scene || !this.index?.pageSet.has(page_id)) return false;
    const meta = this.catalog.get(resource?.resource_id);
    if (!meta || meta.href !== href || typeof image?.setAttribute !== 'function') return false;
    const key = resourceKey(meta.identity);
    if (!this.bindings.has(page_id)) this.bindings.set(page_id, []);
    const binding = {image, page_id, href, identity: meta.identity, key, epoch: this.epoch,
      onLoad: null, onError: null};
    binding.onLoad = () => {
      if (binding.epoch === this.epoch && image.getAttribute?.('href') === href) {
        const placeholder = image.previousElementSibling;
        if (placeholder?.getAttribute?.('data-resource-awaiting-image') === meta.identity.resource_id) {
          placeholder.setAttribute('visibility', 'hidden');
        }
        image.setAttribute('data-resource-activation', 'painted');
      }
    };
    binding.onError = () => {
      if (binding.epoch === this.epoch) image.setAttribute('data-resource-activation', 'failed-paint');
    };
    image.addEventListener?.('load', binding.onLoad);
    image.addEventListener?.('error', binding.onError);
    this.bindings.get(page_id).push(binding);
    return true;
  }

  mount() {
    if (this.disposed || !this.index || !this.scene) throw new Error('image admission not prepared');
    if (this.mounted) throw new Error('image admission already mounted');
    this.mounted = true;
    if (!this.available || this.catalog.size === 0 || this.bindings.size === 0) return;
    const epoch = this.epoch;
    this.readiness = new NotifyingReadinessV1({
      document_id: this.index.document_id,
      revision_id: this.index.revision_id,
      snapshot_id: this.index.snapshot_id || 'reader-snapshot:' + this.index.revision_id,
    }, identity => this.#onReady(identity, epoch));
    const catalogByKey = new Map(Array.from(this.catalog.values(), item => [resourceKey(item.identity), item]));
    // Dependency invalidations are runtime-only repaint hints; never Scene edits.
    const registered = new Set();
    for (const [pageId, entries] of this.bindings) {
      for (const {identity, key} of entries) {
        const cacheId = JSON.stringify([pageId,key]);
        if (registered.has(cacheId)) continue;
        registered.add(cacheId);
        this.readiness.registerCompiledDependency({
          cache_entry_id: cacheId, page_id: pageId,
          consumer_kind: 'image', ...identity,
        });
      }
    }
    this.scheduler = this.makeScheduler({
      readiness: this.readiness,
      config: this.schedulerConfig,
      fetch_resource: async ({identity, signal}) => {
        const meta = catalogByKey.get(resourceKey(identity));
        if (!meta) throw new Error('safe_preview_artifact_unknown');
        return verifySafePreviewInlineV1(meta, {
          subtle: this.cryptoObject.subtle, atob_fn: this.atob, signal,
        });
      },
      decode_resource: async ({identity, fetched, signal}) => {
        if (signal.aborted) throw new Error('decode_aborted');
        const blob = new Blob([fetched.bytes], {type: fetched.mime});
        const bitmap = await this.createImageBitmap(blob, EXACT_CREATE_IMAGE_BITMAP_OPTIONS);
        try {
          if (signal.aborted) throw new Error('decode_aborted');
          if (!Number.isSafeInteger(bitmap.width) || !Number.isSafeInteger(bitmap.height) ||
              bitmap.width <= 0 || bitmap.height <= 0 ||
              bitmap.width * bitmap.height > MAX_PREVIEW_PIXELS) {
            throw new Error('safe_preview_decode_pixel_budget_exceeded');
          }
          return Object.freeze({binding_identity:'safe-preview:' + resourceKey(identity)});
        } finally {
          bitmap.close?.();
        }
      },
    });
    this.observer = this.makeObserver({
      host: this.host, scroll_container: this.scrollContainer,
      document_object: this.documentObject,
      near_before: this.nearBefore, near_after: this.nearAfter,
      on_window: spec => this.updateWindow(spec),
    });
    this.observer.setIndex(this.index);
    if (this.retiring) {
      this.retirement.then(() => {
        if (this.disposed || epoch !== this.epoch) return;
        this.retiring = false;
        const spec = this.pendingSpec;
        this.pendingSpec = null;
        if (spec) this.updateWindow(spec);
      });
    }
  }

  updateWindow(spec) {
    if (!this.mounted || !this.scheduler || this.disposed) return null;
    const nextVisible = spec.hidden ? new Set() : new Set([spec.current_page_id, ...(spec.visible_page_ids ?? [])]);
    for (const pageId of this.visible) {
      if (nextVisible.has(pageId)) continue;
      for (const binding of this.bindings.get(pageId) ?? []) {
        binding.image.removeAttribute?.('href');
        binding.image.setAttribute('data-resource-activation', 'deferred');
        const placeholder = binding.image.previousElementSibling;
        if (placeholder?.getAttribute?.('data-resource-awaiting-image') === binding.identity.resource_id) {
          placeholder.removeAttribute?.('visibility');
        }
      }
    }
    this.visible = nextVisible;
    const projection = this.index.select(spec);
    this.lastProjection = projection;
    const requested = projection.demands.filter(d =>
      this.catalog.has(d.resource_id) && this.bindings.size &&
      this.catalog.get(d.resource_id).identity.content_hash === d.content_hash &&
      this.catalog.get(d.resource_id).identity.derivative_id === d.derivative_id
    );
    if (this.retiring) {
      // Latest observed current page wins when the previous Scene's physical
      // callbacks finally settle. Never queue stale page 1 prefetch in between.
      this.pendingSpec = {
        current_page_id: spec.current_page_id,
        visible_page_ids: [...(spec.visible_page_ids ?? [])],
        near_page_ids: [...(spec.near_page_ids ?? [])],
        hidden: spec.hidden ?? false,
        memory_pressure: spec.memory_pressure ?? false,
      };
      return projection;
    }
    this.scheduler.updateDemands({
      generation: this.index.generation + 1,
      demands: requested,
      hidden: spec.hidden ?? false,
      memory_pressure: spec.memory_pressure ?? false,
    });
    this.index.generation++;
    for (const pageId of this.visible) this.#paintIfReady(pageId);
    return projection;
  }

  #onReady(identity, epoch) {
    if (this.disposed || epoch !== this.epoch) return;
    // Readiness.completeReady already fenced request/backend generations.
    for (const pageId of this.visible) this.#paintIfReady(pageId, resourceKey(identity));
  }

  #paintIfReady(pageId, onlyKey = null) {
    for (const binding of this.bindings.get(pageId) ?? []) {
      if (onlyKey !== null && binding.key !== onlyKey) continue;
      if (this.readiness?.resourceState(binding.identity).state !== 'ready') continue;
      if (binding.image.getAttribute?.('href') === binding.href) continue;
      binding.image.setAttribute('data-resource-activation', 'awaiting-paint');
      binding.image.setAttribute('href', binding.href);
      this.paintCount++;
    }
  }

  fallbackToEager() {
    // Only used for observer/scheduler initialization failure, NEVER after an
    // integrity/decode rejection. Preserve the existing full-Scene view.
    for (const entries of this.bindings.values()) {
      for (const binding of entries) {
        const { image, href, identity } = binding;
        const placeholder = image.previousElementSibling;
        image.addEventListener?.('load', () => {
          if (image.getAttribute?.('href') === href &&
              placeholder?.getAttribute?.('data-resource-awaiting-image') === identity.resource_id) {
            placeholder.setAttribute('visibility', 'hidden');
          }
        }, { once: true });
        image.setAttribute('data-resource-activation', 'legacy-fallback');
        image.setAttribute('href', href);
      }
    }
    this.#clearScene();
    this.available = false;
  }

  snapshot() {
    return Object.freeze({
      schema: PRODUCT_IMAGE_ADMISSION_SCHEMA,
      enabled: this.available && this.catalog.size > 0,
      prepared_resources: this.catalog.size,
      deferred_image_instances: [...this.bindings.values()].reduce((n,v) => n + v.length,0),
      painted_requests: this.paintCount,
      retiring_prior_scene: this.retiring,
      current_visible_pages: this.visible.size,
      latest_projection: this.lastProjection && {
        selected_page_count: this.lastProjection.selected_page_count,
        unsupported_visible_count: this.lastProjection.unsupported_visible_count,
      },
      scheduler: this.scheduler?.snapshot() ?? null,
      readiness: this.readiness?.receipt() ?? null,
    });
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.#clearScene();
  }
}
