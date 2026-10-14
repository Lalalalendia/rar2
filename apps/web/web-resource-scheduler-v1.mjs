export const RESOURCE_SCHEDULER_SCHEMA = 'chaptera.web-resource-scheduler.v1';
export const RESOURCE_PRIORITY_CLASSES = Object.freeze([
  'VisibleBlocking', 'VisibleUpgrade', 'NearWindow', 'Speculative'
]);

const PRIORITY_RANK = Object.freeze({
  VisibleBlocking: 0,
  VisibleUpgrade: 1,
  NearWindow: 2,
  Speculative: 3,
});

const DEFAULT_CONFIG = Object.freeze({
  max_network_inflight: 3,
  max_decode_inflight: 2,
  max_network_bytes_inflight: 12_000_000,
  max_decode_bytes_reserved: 64_000_000,
  max_decoded_pixels_inflight: 24_000_000,
  unknown_network_bytes_reservation: 2_000_000,
  unknown_decode_bytes_reservation: 16_000_000,
  unknown_pixels_reservation: 4_000_000,
  max_queued_items: 4096,
  max_visible_wait_turns: 4,
});

function computeResourceKey(identity) {
  for (const key of ['resource_id', 'content_hash']) {
    if (typeof identity?.[key] !== 'string' || identity[key].length === 0) {
      throw new TypeError(`${key} must be a non-empty string`);
    }
  }
  const derivative = identity.derivative_id ?? 'original';
  if (typeof derivative !== 'string' || derivative.length === 0) {
    throw new TypeError('derivative_id must be nonempty');
  }
  return JSON.stringify([identity.resource_id, identity.content_hash, derivative]);
}

function validateReadiness(readiness) {
  if (!readiness || typeof readiness !== 'object') {
    throw new TypeError('readiness runtime must be an object');
  }
  const requiredMethods = [
    'beginRequest',
    'markDecoding',
    'completeReady',
    'completeTerminal',
    'resourceState',
    'receipt',
  ];
  for (const method of requiredMethods) {
    if (typeof readiness[method] !== 'function') {
      throw new TypeError(`readiness.${method} must be a function`);
    }
  }
}

function validateConfig(rawConfig) {
  if (rawConfig === null || typeof rawConfig !== 'object' || Array.isArray(rawConfig)) {
    throw new TypeError('config must be a plain object');
  }
  for (const key of Object.keys(rawConfig)) {
    if (!Object.prototype.hasOwnProperty.call(DEFAULT_CONFIG, key)) {
      throw new TypeError(`Unknown config property: ${key}`);
    }
  }
  const merged = { ...DEFAULT_CONFIG, ...rawConfig };
  for (const [key, value] of Object.entries(merged)) {
    if (key === 'max_queued_items') {
      if (!Number.isSafeInteger(value) || value < 0) {
        throw new RangeError(`${key} must be a non-negative safe integer`);
      }
    } else {
      if (!Number.isSafeInteger(value) || value <= 0) {
        throw new RangeError(`${key} must be a positive safe integer`);
      }
    }
  }
  return Object.freeze(merged);
}

function validateOptionalCost(value, fieldName) {
  if (value === undefined || value === null) {
    return null;
  }
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${fieldName} must be a non-negative safe integer or null/undefined`);
  }
  return value;
}

function validateAndNormalizeDemand(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new TypeError('Each demand must be an object');
  }
  const {
    page_id,
    resource_id,
    content_hash,
    derivative_id = 'original',
    priority,
    estimated_network_bytes,
    estimated_decode_bytes,
    estimated_pixels,
    retry = false,
  } = raw;

  if (typeof page_id !== 'string' || page_id.length === 0) {
    throw new TypeError('demand.page_id must be a non-empty string');
  }
  if (typeof resource_id !== 'string' || resource_id.length === 0) {
    throw new TypeError('demand.resource_id must be a non-empty string');
  }
  if (typeof content_hash !== 'string' || content_hash.length === 0) {
    throw new TypeError('demand.content_hash must be a non-empty string');
  }
  if (typeof derivative_id !== 'string' || derivative_id.length === 0) {
    throw new TypeError('demand.derivative_id must be a non-empty string');
  }
  if (!Object.prototype.hasOwnProperty.call(PRIORITY_RANK, priority)) {
    throw new TypeError(`Invalid demand.priority: ${String(priority)}`);
  }
  if (typeof retry !== 'boolean') {
    throw new TypeError('demand.retry must be a boolean when provided');
  }

  const netBytes = validateOptionalCost(estimated_network_bytes, 'estimated_network_bytes');
  const decodeBytes = validateOptionalCost(estimated_decode_bytes, 'estimated_decode_bytes');
  const pixels = validateOptionalCost(estimated_pixels, 'estimated_pixels');

  const identity = Object.freeze({
    resource_id,
    content_hash,
    derivative_id,
  });
  const key = computeResourceKey(identity);

  return {
    key,
    identity,
    page_id,
    priority,
    priorityRank: PRIORITY_RANK[priority],
    estimated_network_bytes: netBytes,
    estimated_decode_bytes: decodeBytes,
    estimated_pixels: pixels,
    retry,
  };
}

export class WebResourceSchedulerV1 {
  #readiness;
  #fetchResource;
  #decodeResource;
  #config;
  #now;

  #generation = 0;
  #hidden = false;
  #memoryPressure = false;
  #disposed = false;

  #nextSeq = 1;
  #nextJobId = 1;

  // Map<key, Task> for currently demanded active/queued/paused resources
  #tasks = new Map();
  // Map<key, number> tracking latest jobId assigned to each resource key
  #latestJobIdByKey = new Map();
  // Cancelled callbacks that have not actually settled still consume stage capacity.
  // Keep them here so a fresh demand cannot start duplicate work for the same key.
  #retiringTasksByKey = new Map();

  // Physical resource reservations (including cancelled, unsettled callbacks)
  #inflightFetchCount = 0;
  #inflightDecodeCount = 0;
  #reservedNetworkBytes = 0;
  #reservedDecodeBytes = 0;
  #reservedPixels = 0;

  // Idle waiters: Array<{ resolve: Function }>
  #idleWaiters = [];

  // Monotone telemetry counters
  #metrics = {
    fetch_started: 0,
    decode_started: 0,
    deduped_demands: 0,
    promotions: 0,
    cancelled_obsolete: 0,
    stale_completions: 0,
    failures: 0,
    unknown_cost_admissions: 0,
    canonical_revisions_emitted: 0,
    scene_patches_emitted: 0,
    authoring_operations_emitted: 0,
  };

  constructor({
    readiness,
    fetch_resource,
    decode_resource,
    config = {},
    now = () => performance.now(),
  } = {}) {
    validateReadiness(readiness);
    if (typeof fetch_resource !== 'function') {
      throw new TypeError('fetch_resource must be a function');
    }
    if (typeof decode_resource !== 'function') {
      throw new TypeError('decode_resource must be a function');
    }
    if (typeof now !== 'function') {
      throw new TypeError('now must be a function');
    }

    this.#readiness = readiness;
    this.#fetchResource = fetch_resource;
    this.#decodeResource = decode_resource;
    this.#config = validateConfig(config);
    this.#now = now;
  }

  updateDemands({
    generation,
    demands,
    hidden = false,
    memory_pressure = false,
  } = {}) {
    if (this.#disposed) {
      throw new Error('Cannot updateDemands on a disposed scheduler');
    }
    if (!Number.isSafeInteger(generation) || generation <= 0 || generation <= this.#generation) {
      throw new RangeError(
        `generation must be a strictly increasing positive safe integer (current: ${this.#generation}, received: ${String(generation)})`
      );
    }
    if (typeof hidden !== 'boolean') {
      throw new TypeError('hidden must be a boolean');
    }
    if (typeof memory_pressure !== 'boolean') {
      throw new TypeError('memory_pressure must be a boolean');
    }
    if (!Array.isArray(demands)) {
      throw new TypeError('demands must be an array');
    }

    // Validate all demands atomically before mutating any state.
    const normalizedList = demands.map(validateAndNormalizeDemand);

    // Group demands by canonical resource key.
    const aggregated = new Map();
    let batchDeduped = 0;

    for (const item of normalizedList) {
      const existing = aggregated.get(item.key);
      if (!existing) {
        aggregated.set(item.key, {
          key: item.key,
          identity: item.identity,
          consumers: new Set([item.page_id]),
          priority: item.priority,
          priorityRank: item.priorityRank,
          estimated_network_bytes: item.estimated_network_bytes,
          estimated_decode_bytes: item.estimated_decode_bytes,
          estimated_pixels: item.estimated_pixels,
          retry: item.retry,
          demandOrder: this.#nextSeq++,
        });
      } else {
        batchDeduped++;
        existing.consumers.add(item.page_id);
        if (item.priorityRank < existing.priorityRank) {
          existing.priority = item.priority;
          existing.priorityRank = item.priorityRank;
        }
        if (item.estimated_network_bytes !== null) {
          existing.estimated_network_bytes =
            existing.estimated_network_bytes === null
              ? item.estimated_network_bytes
              : Math.max(existing.estimated_network_bytes, item.estimated_network_bytes);
        }
        if (item.estimated_decode_bytes !== null) {
          existing.estimated_decode_bytes =
            existing.estimated_decode_bytes === null
              ? item.estimated_decode_bytes
              : Math.max(existing.estimated_decode_bytes, item.estimated_decode_bytes);
        }
        if (item.estimated_pixels !== null) {
          existing.estimated_pixels =
            existing.estimated_pixels === null
              ? item.estimated_pixels
              : Math.max(existing.estimated_pixels, item.estimated_pixels);
        }
        if (item.retry) {
          existing.retry = true;
        }
      }
    }

    // Commit validated generation and environmental state.
    this.#generation = generation;
    this.#hidden = hidden;
    this.#memoryPressure = memory_pressure;
    this.#metrics.deduped_demands += batchDeduped;

    // 1. Cancel tasks whose resource keys are no longer demanded by any page.
    for (const [key, task] of this.#tasks.entries()) {
      if (!aggregated.has(key)) {
        this.#cancelTask(task);
      }
    }

    // 2. Under hidden or memory_pressure, cancel or demote in-flight speculative/near-window tasks
    // if they are no longer eligible to run (shed speculative work first).
    if (this.#hidden || this.#memoryPressure) {
      for (const [key, task] of this.#tasks.entries()) {
        const nextSpec = aggregated.get(key);
        if (!nextSpec) continue;
        if (!this.#isPriorityRunnable(nextSpec.priorityRank)) {
          if (task.stage === 'inflight_fetch' || task.stage === 'inflight_decode') {
            this.#cancelTask(task);
          }
        }
      }
    }

    // 3. Update existing live tasks (promotions, priority updates, cost updates) and create new tasks.
    for (const [key, spec] of aggregated.entries()) {
      const existingTask = this.#tasks.get(key);
      if (existingTask) {
        // Check if new page consumers joined an already active task (beyond batch dedup).
        for (const pageId of spec.consumers) {
          if (!existingTask.consumers.has(pageId) && spec.consumers.size === 1) {
            this.#metrics.deduped_demands++;
          }
        }
        existingTask.consumers = spec.consumers;
        existingTask.lastSeenGeneration = generation;

        if (spec.priorityRank < existingTask.priorityRank) {
          existingTask.priority = spec.priority;
          existingTask.priorityRank = spec.priorityRank;
          existingTask.classOrder = this.#nextSeq++;
          this.#metrics.promotions++;
        } else if (spec.priorityRank > existingTask.priorityRank) {
          existingTask.priority = spec.priority;
          existingTask.priorityRank = spec.priorityRank;
          existingTask.classOrder = this.#nextSeq++;
        }

        // Update unreserved future stage costs if better estimates were provided.
        if (existingTask.stage === 'queued_fetch') {
          existingTask.estimated_network_bytes = spec.estimated_network_bytes;
        }
        if (existingTask.stage === 'queued_fetch' || existingTask.stage === 'inflight_fetch' || existingTask.stage === 'queued_decode') {
          existingTask.estimated_decode_bytes = spec.estimated_decode_bytes;
          existingTask.estimated_pixels = spec.estimated_pixels;
        }
        continue;
      }

      // Inspect readiness state before creating a new task.
      const readinessState = this.#readiness.resourceState(spec.identity)?.state ?? 'unrequested';
      if (readinessState === 'ready') {
        // Already fetched and materialized; reuse existing ready material.
        continue;
      }
      if ((readinessState === 'failed' || readinessState === 'blocked') && !spec.retry) {
        // Do not enter an automatic retry loop on mere updateDemands.
        continue;
      }

      const jobId = this.#nextJobId++;
      this.#latestJobIdByKey.set(key, jobId);

      const task = {
        jobId,
        key,
        identity: spec.identity,
        consumers: spec.consumers,
        priority: spec.priority,
        priorityRank: spec.priorityRank,
        estimated_network_bytes: spec.estimated_network_bytes,
        estimated_decode_bytes: spec.estimated_decode_bytes,
        estimated_pixels: spec.estimated_pixels,
        createdOrder: spec.demandOrder,
        classOrder: spec.demandOrder,
        lastSeenGeneration: generation,
        waitTurns: 0,
        stage: 'queued_fetch', // 'queued_fetch' | 'inflight_fetch' | 'queued_decode' | 'inflight_decode'
        token: null,
        abortController: null,
        fetchedPayload: null,
        reservedNetBytes: 0,
        reservedDecodeBytes: 0,
        reservedPixels: 0,
        netReservationReleased: false,
        decodeReservationReleased: false,
        cancelled: false,
        createdAt: this.#now(),
      };

      this.#tasks.set(key, task);
    }

    this.#enforceQueueCapacity();
    this.#schedule();
    this.#checkIdle();
  }

  #isPriorityRunnable(priorityRank) {
    if (this.#hidden || this.#memoryPressure) {
      // Only VisibleBlocking and VisibleUpgrade may run while hidden or under memory pressure;
      // NearWindow (2) and Speculative (3) are paused.
      return priorityRank <= PRIORITY_RANK.VisibleUpgrade;
    }
    return true;
  }

  #compareTasks(a, b) {
    if (a.priorityRank !== b.priorityRank) {
      return a.priorityRank - b.priorityRank;
    }
    // Bounded aging / FIFO fairness within the same priority class:
    // tasks that have waited more turns or entered the priority class earlier go first.
    if (a.waitTurns !== b.waitTurns && (a.waitTurns >= this.#config.max_visible_wait_turns || b.waitTurns >= this.#config.max_visible_wait_turns)) {
      return b.waitTurns - a.waitTurns;
    }
    if (a.classOrder !== b.classOrder) {
      return a.classOrder - b.classOrder;
    }
    return a.createdOrder - b.createdOrder;
  }

  #getNetworkReservation(task) {
    const unknown = task.estimated_network_bytes === null;
    const bytes = unknown
      ? this.#config.unknown_network_bytes_reservation
      : task.estimated_network_bytes;
    return { bytes, unknown };
  }

  #getDecodeReservation(task) {
    const unknownBytes = task.estimated_decode_bytes === null;
    const unknownPixels = task.estimated_pixels === null;
    const bytes = unknownBytes
      ? this.#config.unknown_decode_bytes_reservation
      : task.estimated_decode_bytes;
    const pixels = unknownPixels
      ? this.#config.unknown_pixels_reservation
      : task.estimated_pixels;
    return {
      bytes,
      pixels,
      unknown: unknownBytes || unknownPixels,
    };
  }

  #enforceQueueCapacity() {
    const queuedFetchTasks = [];
    let runnableQueuedCount = 0;
    for (const task of this.#tasks.values()) {
      if (task.stage === 'queued_fetch' && this.#isPriorityRunnable(task.priorityRank)) {
        queuedFetchTasks.push(task);
        runnableQueuedCount++;
      } else if (task.stage === 'queued_decode' && this.#isPriorityRunnable(task.priorityRank)) {
        runnableQueuedCount++;
      }
    }

    // How many queued_fetch tasks can immediately start right now?
    // Only enforce max_queued_items on excess items beyond available immediate fetch slots + max_queued_items.
    const availableImmediateFetchSlots = Math.max(
      0,
      this.#config.max_network_inflight - this.#inflightFetchCount
    );
    const maxAllowedRunnableQueued = availableImmediateFetchSlots + this.#config.max_queued_items;
    if (runnableQueuedCount <= maxAllowedRunnableQueued) {
      return;
    }

    // Sort runnable queued_fetch tasks from best to worst, then block excess from the tail.
    queuedFetchTasks.sort((a, b) => this.#compareTasks(a, b));
    let excess = runnableQueuedCount - maxAllowedRunnableQueued;
    for (let i = queuedFetchTasks.length - 1; i >= 0 && excess > 0; i--) {
      const victim = queuedFetchTasks[i];
      this.#blockTask(victim, 'queue_capacity_exceeded');
      excess--;
    }
  }

  #blockTask(task, reasonCode) {
    task.cancelled = true;
    this.#tasks.delete(task.key);
    this.#forgetJob(task);
    if (!task.token) {
      task.token = this.#readiness.beginRequest(task.identity);
    }
    this.#readiness.completeTerminal(task.token, {
      state: 'blocked',
      reason_code: reasonCode,
    });
  }

  #cancelTask(task) {
    if (task.cancelled) return;
    task.cancelled = true;
    this.#metrics.cancelled_obsolete++;
    // Abort is a request, not evidence that the underlying callback has stopped.
    // Retain stage reservations until the corresponding promise settles.
    if ((task.stage === 'inflight_fetch' && !task.netReservationReleased) ||
        (task.stage === 'inflight_decode' && !task.decodeReservationReleased)) {
      let retiring = this.#retiringTasksByKey.get(task.key);
      if (!retiring) {
        retiring = new Set();
        this.#retiringTasksByKey.set(task.key, retiring);
      }
      retiring.add(task);
    }
    if (task.abortController) {
      try {
        task.abortController.abort();
      } catch {
        // Ignore abort listener errors.
      }
    }
    this.#tasks.delete(task.key);
    this.#forgetJob(task);
  }

  #forgetJob(task) {
    if (!this.#tasks.has(task.key) && this.#latestJobIdByKey.get(task.key) === task.jobId) {
      this.#latestJobIdByKey.delete(task.key);
    }
  }

  #finishRetirement(task) {
    const retiring = this.#retiringTasksByKey.get(task.key);
    if (retiring) {
      retiring.delete(task);
      if (retiring.size === 0) this.#retiringTasksByKey.delete(task.key);
    }
  }

  #releaseNetworkReservation(task) {
    if (task.stage === 'inflight_fetch' && !task.netReservationReleased) {
      task.netReservationReleased = true;
      this.#inflightFetchCount = Math.max(0, this.#inflightFetchCount - 1);
      this.#reservedNetworkBytes = Math.max(0, this.#reservedNetworkBytes - task.reservedNetBytes);
      task.reservedNetBytes = 0;
    }
  }

  #releaseDecodeReservation(task) {
    if (task.stage === 'inflight_decode' && !task.decodeReservationReleased) {
      task.decodeReservationReleased = true;
      this.#inflightDecodeCount = Math.max(0, this.#inflightDecodeCount - 1);
      this.#reservedDecodeBytes = Math.max(0, this.#reservedDecodeBytes - task.reservedDecodeBytes);
      this.#reservedPixels = Math.max(0, this.#reservedPixels - task.reservedPixels);
      task.reservedDecodeBytes = 0;
      task.reservedPixels = 0;
    }
  }

  #isTaskFenced(task) {
    return (
      this.#disposed ||
      task.cancelled ||
      this.#tasks.get(task.key) !== task ||
      this.#latestJobIdByKey.get(task.key) !== task.jobId
    );
  }

  #schedule() {
    if (this.#disposed) return;

    this.#scheduleDecodes();
    this.#scheduleFetches();
  }

  #scheduleDecodes() {
    const candidates = [];
    for (const task of this.#tasks.values()) {
      if (task.stage === 'queued_decode' && this.#isPriorityRunnable(task.priorityRank)) {
        candidates.push(task);
      }
    }
    if (candidates.length === 0) return;

    candidates.sort((a, b) => this.#compareTasks(a, b));

    const skippedForBudget = [];

    for (const task of candidates) {
      if (this.#disposed || task.cancelled) continue;

      const { bytes, pixels, unknown } = this.#getDecodeReservation(task);

      // Single-task oversized check: fail closed immediately so it never blocks other tasks.
      if (
        !Number.isSafeInteger(bytes) ||
        !Number.isSafeInteger(pixels) ||
        bytes > this.#config.max_decode_bytes_reserved ||
        pixels > this.#config.max_decoded_pixels_inflight
      ) {
        this.#blockTask(task, 'decode_budget_exceeded');
        continue;
      }

      if (this.#inflightDecodeCount >= this.#config.max_decode_inflight) {
        break;
      }

      // If a higher-priority/older task has already waited max_visible_wait_turns for budget,
      // hold the runway so it cannot starve behind smaller tasks.
      if (
        skippedForBudget.some(
          s =>
            s.priorityRank === PRIORITY_RANK.VisibleBlocking &&
            s.waitTurns >= this.#config.max_visible_wait_turns
        )
      ) {
        break;
      }

      const nextReservedBytes = this.#reservedDecodeBytes + bytes;
      const nextReservedPixels = this.#reservedPixels + pixels;

      if (
        !Number.isSafeInteger(nextReservedBytes) ||
        !Number.isSafeInteger(nextReservedPixels) ||
        nextReservedBytes > this.#config.max_decode_bytes_reserved ||
        nextReservedPixels > this.#config.max_decoded_pixels_inflight
      ) {
        skippedForBudget.push(task);
        continue;
      }

      // If we admitted a task behind tasks that were skipped for budget, age the skipped tasks.
      for (const skipped of skippedForBudget) {
        skipped.waitTurns++;
      }

      this.#startDecode(task, { bytes, pixels, unknown });
    }
  }

  #scheduleFetches() {
    const candidates = [];
    for (const task of this.#tasks.values()) {
      if (task.stage === 'queued_fetch' && this.#isPriorityRunnable(task.priorityRank)) {
        candidates.push(task);
      }
    }
    if (candidates.length === 0) return;

    candidates.sort((a, b) => this.#compareTasks(a, b));

    const skippedForBudget = [];

    for (const task of candidates) {
      if (this.#disposed || task.cancelled) continue;
      // A cancelled same-key callback may ignore AbortSignal. Never overlap it
      // with equivalent work, even if new demand has arrived meanwhile.
      if (this.#retiringTasksByKey.has(task.key)) continue;

      const { bytes, unknown } = this.#getNetworkReservation(task);

      // Single-task oversized network check: fail closed immediately without blocking others.
      if (
        !Number.isSafeInteger(bytes) ||
        bytes > this.#config.max_network_bytes_inflight
      ) {
        this.#blockTask(task, 'network_budget_exceeded');
        continue;
      }

      if (this.#inflightFetchCount >= this.#config.max_network_inflight) {
        break;
      }

      if (
        skippedForBudget.some(
          s =>
            s.priorityRank === PRIORITY_RANK.VisibleBlocking &&
            s.waitTurns >= this.#config.max_visible_wait_turns
        )
      ) {
        break;
      }

      const nextReservedBytes = this.#reservedNetworkBytes + bytes;
      if (
        !Number.isSafeInteger(nextReservedBytes) ||
        nextReservedBytes > this.#config.max_network_bytes_inflight
      ) {
        skippedForBudget.push(task);
        continue;
      }

      for (const skipped of skippedForBudget) {
        skipped.waitTurns++;
      }

      this.#startFetch(task, { bytes, unknown });
    }
  }

  #startFetch(task, { bytes, unknown }) {
    task.token = this.#readiness.beginRequest(task.identity);
    task.stage = 'inflight_fetch';
    task.waitTurns = 0;
    task.reservedNetBytes = bytes;
    task.netReservationReleased = false;
    task.abortController = new AbortController();

    this.#inflightFetchCount++;
    this.#reservedNetworkBytes += bytes;
    this.#metrics.fetch_started++;
    if (unknown) {
      this.#metrics.unknown_cost_admissions++;
    }

    const signal = task.abortController.signal;
    Promise.resolve()
      .then(() => {
        if (this.#isTaskFenced(task)) throw new Error('fetch_cancelled_before_start');
        return this.#fetchResource({ identity: task.identity, signal });
      })
      .then(
        fetched => {
          this.#onFetchSuccess(task, fetched);
        },
        _err => {
          this.#onFetchFailure(task);
        }
      );
  }

  #onFetchSuccess(task, fetched) {
    this.#releaseNetworkReservation(task);
    this.#finishRetirement(task);

    if (this.#isTaskFenced(task)) {
      this.#metrics.stale_completions++;
      this.#schedule();
      this.#checkIdle();
      return;
    }

    task.fetchedPayload = fetched;
    task.stage = 'queued_decode';
    task.waitTurns = 0;
    task.abortController = null;

    this.#schedule();
    this.#checkIdle();
  }

  #onFetchFailure(task) {
    this.#releaseNetworkReservation(task);
    this.#finishRetirement(task);

    if (this.#isTaskFenced(task)) {
      this.#metrics.stale_completions++;
      this.#schedule();
      this.#checkIdle();
      return;
    }

    task.cancelled = true;
    this.#tasks.delete(task.key);
    this.#forgetJob(task);
    this.#metrics.failures++;
    this.#readiness.completeTerminal(task.token, {
      state: 'failed',
      reason_code: 'fetch_failed',
    });

    this.#schedule();
    this.#checkIdle();
  }

  #startDecode(task, { bytes, pixels, unknown }) {
    const markRes = this.#readiness.markDecoding(task.token);
    if (!markRes?.accepted) {
      task.cancelled = true;
      this.#tasks.delete(task.key);
      this.#metrics.stale_completions++;
      return;
    }

    task.stage = 'inflight_decode';
    task.waitTurns = 0;
    task.reservedDecodeBytes = bytes;
    task.reservedPixels = pixels;
    task.decodeReservationReleased = false;
    task.abortController = new AbortController();

    this.#inflightDecodeCount++;
    this.#reservedDecodeBytes += bytes;
    this.#reservedPixels += pixels;
    this.#metrics.decode_started++;
    if (unknown) {
      this.#metrics.unknown_cost_admissions++;
    }

    const signal = task.abortController.signal;
    const fetched = task.fetchedPayload;
    Promise.resolve()
      .then(() => {
        if (this.#isTaskFenced(task)) throw new Error('decode_cancelled_before_start');
        return this.#decodeResource({
          identity: task.identity,
          fetched,
          signal,
        });
      })
      .then(
        decoded => {
          this.#onDecodeSuccess(task, decoded);
        },
        _err => {
          this.#onDecodeFailure(task);
        }
      );
  }

  #onDecodeSuccess(task, decoded) {
    this.#releaseDecodeReservation(task);
    this.#finishRetirement(task);

    if (this.#isTaskFenced(task)) {
      this.#metrics.stale_completions++;
      this.#schedule();
      this.#checkIdle();
      return;
    }

    task.cancelled = true;
    this.#tasks.delete(task.key);
    this.#forgetJob(task);

    const bindingIdentity = decoded?.binding_identity;
    if (typeof bindingIdentity !== 'string' || bindingIdentity.length === 0) {
      this.#metrics.failures++;
      this.#readiness.completeTerminal(task.token, {
        state: 'failed',
        reason_code: 'invalid_binding_identity',
      });
    } else {
      const readyRes = this.#readiness.completeReady(task.token, {
        binding_identity: bindingIdentity,
      });
      if (!readyRes?.accepted) {
        this.#metrics.stale_completions++;
      }
    }

    this.#schedule();
    this.#checkIdle();
  }

  #onDecodeFailure(task) {
    this.#releaseDecodeReservation(task);
    this.#finishRetirement(task);

    if (this.#isTaskFenced(task)) {
      this.#metrics.stale_completions++;
      this.#schedule();
      this.#checkIdle();
      return;
    }

    task.cancelled = true;
    this.#tasks.delete(task.key);
    this.#forgetJob(task);
    this.#metrics.failures++;
    this.#readiness.completeTerminal(task.token, {
      state: 'failed',
      reason_code: 'decode_failed',
    });

    this.#schedule();
    this.#checkIdle();
  }

  #getQueueCounts() {
    let queuedFetch = 0;
    let queuedDecode = 0;
    let visibleBlockingPending = 0;

    for (const task of this.#tasks.values()) {
      if (task.cancelled) continue;
      if (task.priorityRank === PRIORITY_RANK.VisibleBlocking) {
        visibleBlockingPending++;
      }
      if (!this.#isPriorityRunnable(task.priorityRank)) {
        continue;
      }
      if (task.stage === 'queued_fetch') {
        queuedFetch++;
      } else if (task.stage === 'queued_decode') {
        queuedDecode++;
      }
    }

    return { queuedFetch, queuedDecode, visibleBlockingPending };
  }

  #isQuiescent() {
    if (this.#inflightFetchCount > 0 || this.#inflightDecodeCount > 0) {
      return false;
    }
    const { queuedFetch, queuedDecode } = this.#getQueueCounts();
    return queuedFetch === 0 && queuedDecode === 0;
  }

  #checkIdle() {
    if (!this.#isQuiescent()) return;
    if (this.#idleWaiters.length === 0) return;
    const waiters = this.#idleWaiters.splice(0, this.#idleWaiters.length);
    for (const waiter of waiters) {
      waiter.resolve();
    }
  }

  snapshot() {
    const { queuedFetch, queuedDecode, visibleBlockingPending } = this.#getQueueCounts();
    return {
      schema: RESOURCE_SCHEDULER_SCHEMA,
      generation: this.#generation,
      hidden: this.#hidden,
      memory_pressure: this.#memoryPressure,
      queued_fetch: queuedFetch,
      inflight_fetch: this.#inflightFetchCount,
      queued_decode: queuedDecode,
      inflight_decode: this.#inflightDecodeCount,
      retiring_callbacks: [...this.#retiringTasksByKey.values()].reduce((n, tasks) => n + tasks.size, 0),
      reserved_network_bytes: this.#reservedNetworkBytes,
      reserved_decode_bytes: this.#reservedDecodeBytes,
      reserved_pixels: this.#reservedPixels,
      visible_blocking_pending: visibleBlockingPending,
      metrics: { ...this.#metrics },
    };
  }

  async whenIdle() {
    this.#schedule();
    if (this.#isQuiescent()) {
      return;
    }
    return new Promise(resolve => {
      this.#idleWaiters.push({ resolve });
    });
  }

  dispose() {
    if (this.#disposed) return;
    this.#disposed = true;
    for (const task of this.#tasks.values()) {
      this.#cancelTask(task);
    }
    this.#tasks.clear();
    this.#latestJobIdByKey.clear();
    // On dispose the physical callbacks can outlive the view. They still own
    // their reservations and drain them via #onFetch*/#onDecode* callbacks.
    this.#checkIdle();
  }
}
