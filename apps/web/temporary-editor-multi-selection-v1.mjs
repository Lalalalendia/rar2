// Browser-only transient selection for admitted, direct authored objects.
// An admitted candidate set is a UI filter, never server mutation authority.
// Rust pub-interaction::SelectionState defines primary/toggle semantics.

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const MAX_SELECTED = 1024;
const MAX_CANDIDATES = 16384;

function requireIdentity(value, label) {
  if (typeof value !== "string" || !value || value.length > 256 || /[\s\x00-\x1f\x7f]/.test(value)) {
    throw new TypeError(`${label} must be a nonempty bounded identifier`);
  }
  return value;
}

function requireCanonicalUuid(value, label, authored = false) {
  if (typeof value !== "string" || !(authored ? UUID_V7 : UUID).test(value)) {
    throw new TypeError(`${label} must be a canonical ${authored ? "UUIDv7" : "UUID"}`);
  }
  return value;
}

export class TemporaryEditorMultiSelectionV1 {
  #identity = null;
  #admitted = new Set();
  #selected = new Set();
  #primary = null;

  get pageId() { return this.#identity?.pageId ?? null; }
  get primary() { return this.#primary; }
  get size() { return this.#selected.size; }
  get selectedNodeIds() { return Object.freeze([...this.#selected].sort()); }

  // Replace the *view* candidate universe after authoritative Current/Scene.
  // A new document, source or page clears selection; same-page revisions prune.
  sync({ documentId, sourceHash, pageId, admittedNodeIds }) {
    requireIdentity(documentId, "documentId");
    if (typeof sourceHash !== "string" || !/^[0-9a-f]{64}$/.test(sourceHash)) {
      throw new TypeError("sourceHash must be lowercase SHA-256");
    }
    requireCanonicalUuid(pageId, "pageId");
    if (!Array.isArray(admittedNodeIds) || admittedNodeIds.length > MAX_CANDIDATES) {
      throw new RangeError("admittedNodeIds must be a bounded array");
    }
    const next = new Set();
    for (const nodeId of admittedNodeIds) {
      requireCanonicalUuid(nodeId, "admitted NodeId", true);
      if (next.has(nodeId)) throw new Error("duplicate admitted NodeId");
      next.add(nodeId);
    }
    const identityChanged = !this.#identity ||
      this.#identity.documentId !== documentId ||
      this.#identity.sourceHash !== sourceHash ||
      this.#identity.pageId !== pageId;
    this.#identity = { documentId, sourceHash, pageId };
    this.#admitted = next;
    if (identityChanged) {
      this.clear();
    } else {
      for (const nodeId of this.#selected) {
        if (!next.has(nodeId)) this.#selected.delete(nodeId);
      }
      if (this.#primary && !this.#selected.has(this.#primary)) {
        this.#primary = [...this.#selected].sort()[0] ?? null;
      }
    }
    return this.snapshot();
  }

  click(nodeId, { shiftKey = false } = {}) {
    if (!this.#identity) throw new Error("selection requires Current/Scene identity");
    if (typeof shiftKey !== "boolean") throw new TypeError("shiftKey must be boolean");
    if (nodeId === null) {
      this.clear();
      return this.snapshot();
    }
    // Unsupported visual objects may have another inspect path, never multi-select.
    if (!this.#admitted.has(nodeId)) {
      if (!shiftKey) this.clear();
      return this.snapshot();
    }
    if (!shiftKey) {
      this.#selected.clear();
      this.#selected.add(nodeId);
      this.#primary = nodeId;
    } else if (this.#selected.delete(nodeId)) {
      if (this.#primary === nodeId) this.#primary = [...this.#selected].sort()[0] ?? null;
    } else {
      if (this.#selected.size >= MAX_SELECTED) throw new RangeError("selection exceeds canonical batch limit");
      this.#selected.add(nodeId);
      this.#primary = nodeId;
    }
    return this.snapshot();
  }

  clear() {
    this.#selected.clear();
    this.#primary = null;
  }

  snapshot() {
    return Object.freeze({
      pageId: this.pageId,
      primary: this.#primary,
      selectedNodeIds: this.selectedNodeIds,
      count: this.#selected.size,
    });
  }

  toJSON() { throw new Error("transient selection is not durable authoring state"); }
}
