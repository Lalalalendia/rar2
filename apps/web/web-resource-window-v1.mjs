// Derived current-page/window resource demand. No document authority, fetching,
// decoding, resource identity invention, or implicit neighbor radius lives here.
export const WEB_RESOURCE_WINDOW_SCHEMA = 'chaptera.web-resource-window.v1';

const SHA256 = /^[a-f0-9]{64}$/;
const READER_SCENE = 'chaptera.reader-scene.v1';
const EDITOR_SCENE = 'chaptera.editor-render-scene.v1';

function ident(value, name) {
  if (typeof value !== 'string' || !value) throw new TypeError(name + ' must be non-empty');
  return value;
}
function safeCount(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) throw new RangeError(name + ' must be a non-negative safe integer');
  return value;
}
function optionalCost(value) {
  if (value === undefined || value === null) return undefined;
  return Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}
function exactHash(desc) {
  const hash = desc?.content_hash ?? desc?.expected_sha256;
  return typeof hash === 'string' && SHA256.test(hash) ? hash : null;
}
function resourceKey(d) {
  return JSON.stringify([d.resource_id, d.content_hash, d.derivative_id]);
}
function refsForNode(node) {
  const ids = new Set();
  if (typeof node?.resource_id === 'string' && node.resource_id) ids.add(node.resource_id);
  for (const placement of node?.decorative_border?.placements ?? []) {
    if (typeof placement?.resource_id === 'string' && placement.resource_id) ids.add(placement.resource_id);
  }
  const layout = node?.text_layout;
  if (typeof layout?.font_resource_id === 'string' && layout.font_resource_id) {
    ids.add(layout.font_resource_id);
  }
  for (const line of layout?.lines ?? []) {
    for (const span of line?.spans ?? []) {
      if (typeof span?.font_resource_id === 'string' && span.font_resource_id) {
        ids.add(span.font_resource_id);
      }
    }
  }
  return ids;
}
function list(value, name) {
  if (!Array.isArray(value)) throw new TypeError(name + ' must be an array');
  const ids = new Set();
  for (const id of value) ids.add(ident(id, name + ' page id'));
  return ids;
}

// An index is scoped to ONE canonical Scene. Replace both the readiness runtime
// and scheduler when document/revision/snapshot identity changes.
export class WebResourceWindowIndexV1 {
  constructor(scene) {
    if (!scene || ![READER_SCENE, EDITOR_SCENE].includes(scene.protocol_version)) {
      throw new TypeError('supported Reader/Editor Scene protocol required');
    }
    this.document_id = ident(scene.document_id, 'scene.document_id');
    this.revision_id = ident(scene.revision_id, 'scene.revision_id');
    this.snapshot_id = scene.snapshot_id ?? null;
    const pages = [...(scene.pages ?? [])].sort((a, b) => a.order - b.order);
    this.pages = Object.freeze(pages.map(page => ident(page.page_id, 'page_id')));
    if (new Set(this.pages).size !== this.pages.length) throw new Error('duplicate page id');
    this.pageSet = new Set(this.pages);
    this.pagePosition = new Map(this.pages.map((id, i) => [id, i]));

    this.descriptors = new Map();
    for (const descriptor of [...(scene.resources ?? []), ...(scene.fonts ?? [])]) {
      const id = ident(descriptor?.resource_id, 'resource_id');
      if (this.descriptors.has(id)) throw new Error('duplicate resource descriptor id');
      // Capture only immutable identity/admission hints, not inline_data_url,
      // scene object references, source bytes, delivery handles or credentials.
      this.descriptors.set(id, Object.freeze({
        resource_id: id,
        content_hash: descriptor.content_hash,
        expected_sha256: descriptor.expected_sha256,
        derivative_id: descriptor.derivative_id,
        byte_len: descriptor.byte_len,
        estimated_decode_bytes: descriptor.estimated_decode_bytes,
        estimated_pixels: descriptor.estimated_pixels,
      }));
    }
    this.pageResources = new Map(this.pages.map(id => [id, new Set()]));
    for (const node of scene.nodes ?? []) {
      const pageResources = this.pageResources.get(node?.page_id);
      if (!pageResources) throw new Error('node refers to unknown page');
      for (const id of refsForNode(node)) pageResources.add(id);
    }
    this.lastResult = null;
    this.generation = 0;
  }

  #assertPage(pageId) {
    if (!this.pageSet.has(pageId)) throw new Error('unknown page id');
  }

  // This method creates demands ONLY for selected pages. It does not fetch,
  // hash inline bytes, parse PUB, or mark unsupported assets ready.
  select({
    current_page_id,
    visible_page_ids = [],
    near_page_ids = [],
    speculative_page_ids = [],
    upgrade_resource_ids = [],
  } = {}) {
    this.#assertPage(current_page_id);
    const visible = list(visible_page_ids, 'visible_page_ids');
    const near = list(near_page_ids, 'near_page_ids');
    const speculative = list(speculative_page_ids, 'speculative_page_ids');
    const upgrades = list(upgrade_resource_ids, 'upgrade_resource_ids');
    for (const id of [...visible, ...near, ...speculative]) this.#assertPage(id);
    visible.add(current_page_id);

    const priorities = new Map();
    for (const pageId of speculative) priorities.set(pageId, 'Speculative');
    for (const pageId of near) priorities.set(pageId, 'NearWindow');
    for (const pageId of visible) priorities.set(pageId, 'VisibleBlocking');

    const demands = [];
    const unsupported = new Set();
    let unsupportedVisible = 0;
    // Visit only demanded pages, not every page of a large publication.
    for (const pageId of [...priorities.keys()].sort((a,b) => this.pagePosition.get(a) - this.pagePosition.get(b))) {
      const priority = priorities.get(pageId);
      for (const resourceId of this.pageResources.get(pageId)) {
        const desc = this.descriptors.get(resourceId);
        const contentHash = exactHash(desc);
        if (!contentHash) {
          unsupported.add(resourceId);
          if (priority === 'VisibleBlocking') unsupportedVisible++;
          continue;
        }
        const isUpgrade = priority === 'VisibleBlocking' && upgrades.has(resourceId);
        const derivative = desc.derivative_id ?? 'original';
        ident(derivative, 'derivative_id');
        const demand = {
          page_id: pageId,
          resource_id: resourceId,
          content_hash: contentHash,
          derivative_id: derivative,
          priority: isUpgrade ? 'VisibleUpgrade' : priority,
        };
        const networkBytes = optionalCost(desc.byte_len);
        const decodeBytes = optionalCost(desc.estimated_decode_bytes);
        const pixels = optionalCost(desc.estimated_pixels);
        if (networkBytes !== undefined) demand.estimated_network_bytes = networkBytes;
        if (decodeBytes !== undefined) demand.estimated_decode_bytes = decodeBytes;
        if (pixels !== undefined) demand.estimated_pixels = pixels;
        demands.push(demand);
      }
    }
    return Object.freeze({
      schema: WEB_RESOURCE_WINDOW_SCHEMA,
      current_page_id,
      selected_page_count: priorities.size,
      demands,
      unsupported_resource_count: unsupported.size,
      unsupported_visible_count: unsupportedVisible,
      // Unsupported visible resources MUST NOT be treated as 'page ready'.
      all_visible_resources_schedulable: unsupportedVisible === 0,
    });
  }

  update(scheduler, spec) {
    if (!scheduler || typeof scheduler.updateDemands !== 'function') {
      throw new TypeError('resource scheduler with updateDemands required');
    }
    const generation = spec?.generation ?? this.generation + 1;
    safeCount(generation, 'generation');
    if (generation <= this.generation) throw new RangeError('generation must increase');
    const result = this.select(spec);
    scheduler.updateDemands({
      generation,
      demands: result.demands,
      hidden: spec?.hidden ?? false,
      memory_pressure: spec?.memory_pressure ?? false,
    });
    this.generation = generation;
    this.lastResult = result;
    return result;
  }
}

// Browser viewport observation is derived state. The callback owns admission;
// this adapter never writes Scene, revision, cache, or resource readiness.
export class WebResourceViewportObserverV1 {
  constructor({
    host,
    scroll_container,
    document_object,
    on_window,
    near_before = 0,
    near_after = 0,
    request_frame = callback => requestAnimationFrame(callback),
    cancel_frame = id => cancelAnimationFrame(id),
    intersection_observer_factory = globalThis.IntersectionObserver
      ? callback => new globalThis.IntersectionObserver(callback, { root: scroll_container })
      : null,
  }) {
    if (typeof host?.querySelectorAll !== 'function') throw new TypeError('host querySelectorAll required');
    if (typeof scroll_container?.getBoundingClientRect !== 'function' ||
        typeof scroll_container?.addEventListener !== 'function') throw new TypeError('scroll container required');
    if (typeof on_window !== 'function') throw new TypeError('on_window callback required');
    safeCount(near_before, 'near_before');
    safeCount(near_after, 'near_after');
    this.host = host;
    this.scrollContainer = scroll_container;
    this.document = document_object;
    this.onWindow = on_window;
    this.nearBefore = near_before;
    this.nearAfter = near_after;
    this.requestFrame = request_frame;
    this.cancelFrame = cancel_frame;
    this.frame = null;
    this.disposed = false;
    this.currentPageId = null;
    this.index = null;
    this.visibleEntries = new Map();
    this.intersectionObserver = null;
    if (intersection_observer_factory != null) {
      if (typeof intersection_observer_factory !== 'function') throw new TypeError('intersection_observer_factory must be a function');
      this.intersectionObserver = intersection_observer_factory(entries => {
        if (this.disposed) return;
        for (const entry of entries) {
          const pageId = entry.target?.getAttribute?.('data-page-id');
          if (!this.index?.pageSet.has(pageId)) continue;
          if (entry.isIntersecting && entry.intersectionRect?.width > 0 && entry.intersectionRect?.height > 0) {
            this.visibleEntries.set(pageId, entry.intersectionRect.width * entry.intersectionRect.height);
          } else {
            this.visibleEntries.delete(pageId);
          }
        }
        this.schedule();
      });
      if (typeof this.intersectionObserver?.observe !== 'function' ||
          typeof this.intersectionObserver?.disconnect !== 'function') {
        throw new TypeError('IntersectionObserver-compatible object required');
      }
    }
    this.onScroll = () => this.schedule();
    this.scrollContainer.addEventListener('scroll', this.onScroll, { passive: true });
    this.document?.addEventListener?.('visibilitychange', this.onScroll);
  }

  setIndex(index) {
    if (this.disposed) throw new Error('observer disposed');
    if (!(index instanceof WebResourceWindowIndexV1)) throw new TypeError('WebResourceWindowIndexV1 required');
    this.index = index;
    this.currentPageId = null;
    this.visibleEntries.clear();
    if (this.intersectionObserver) {
      this.intersectionObserver.disconnect();
      for (const page of this.host.querySelectorAll('svg.page[data-page-id]')) {
        if (this.index.pageSet.has(page.getAttribute('data-page-id'))) this.intersectionObserver.observe(page);
      }
    } else {
      this.schedule();
    }
  }

  schedule() {
    if (this.disposed || !this.index || this.frame !== null) return;
    this.frame = this.requestFrame(() => {
      this.frame = null;
      if (!this.disposed) this.flush();
    });
  }

  flush() {
    if (this.disposed || !this.index) return;
    let areas;
    if (this.intersectionObserver) {
      // Browser computes intersections; no O(500) layout reads on every scroll.
      if (this.visibleEntries.size === 0 && this.currentPageId === null) return;
      areas = this.visibleEntries;
    } else {
      const viewport = this.scrollContainer.getBoundingClientRect();
      areas = new Map();
      for (const page of this.host.querySelectorAll('svg.page[data-page-id]')) {
        const id = page.getAttribute('data-page-id');
        if (!this.index.pageSet.has(id)) continue;
        const rect = page.getBoundingClientRect();
        const width = Math.max(0, Math.min(rect.right, viewport.right) - Math.max(rect.left, viewport.left));
        const height = Math.max(0, Math.min(rect.bottom, viewport.bottom) - Math.max(rect.top, viewport.top));
        if (width * height > 0) areas.set(id, width * height);
      }
    }
    const visible = [...areas.keys()].sort((a,b) => this.index.pagePosition.get(a) - this.index.pagePosition.get(b));
    let largestArea = -1;
    let best = null;
    for (const [id, area] of areas) {
      if (area > largestArea) { largestArea = area; best = id; }
    }
    const current = best ?? this.currentPageId ?? this.index.pages[0];
    if (!current) return;
    this.currentPageId = current;
    const offset = this.index.pagePosition.get(current);
    const near = [];
    for (let i = Math.max(0, offset - this.nearBefore); i <= Math.min(this.index.pages.length - 1, offset + this.nearAfter); i++) {
      if (this.index.pages[i] !== current && !visible.includes(this.index.pages[i])) near.push(this.index.pages[i]);
    }
    this.onWindow({
      current_page_id: current,
      visible_page_ids: visible,
      near_page_ids: near,
      hidden: this.document?.visibilityState === 'hidden',
    });
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    if (this.frame !== null) this.cancelFrame(this.frame);
    this.frame = null;
    this.intersectionObserver?.disconnect();
    this.visibleEntries.clear();
    this.scrollContainer.removeEventListener('scroll', this.onScroll);
    this.document?.removeEventListener?.('visibilitychange', this.onScroll);
    this.index = null;
  }
}

// Connect a completed host Scene (after SVG pages exist) to the existing
// scheduler without reimplementing its queue, readiness generations or cache.
// Caller owns the scheduler/readiness instance and MUST replace it when the
// canonical Scene identity changes, and dispose it when the view unmounts.
export function bindResourceWindowV1({
  scene, scheduler, host, scroll_container, document_object,
  near_before = 0, near_after = 0,
  request_frame, cancel_frame, intersection_observer_factory,
  on_projection = null,
}) {
  const index = new WebResourceWindowIndexV1(scene);
  if (on_projection !== null && typeof on_projection !== 'function') throw new TypeError('on_projection must be a function');
  const options = {
    host, scroll_container, document_object, near_before, near_after,
    on_window: spec => {
      const projection = index.update(scheduler, spec);
      on_projection?.(projection);
    },
  };
  if (request_frame !== undefined) options.request_frame = request_frame;
  if (cancel_frame !== undefined) options.cancel_frame = cancel_frame;
  if (intersection_observer_factory !== undefined) options.intersection_observer_factory = intersection_observer_factory;
  const observer = new WebResourceViewportObserverV1(options);
  observer.setIndex(index);
  return Object.freeze({ index, observer, dispose: () => observer.dispose() });
}