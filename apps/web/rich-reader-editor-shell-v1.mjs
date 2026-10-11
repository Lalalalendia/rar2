import { renderReaderScene } from "../cloud-reader/render-v1.mjs";
import {
  MoveGestureV1,
  hitTestSnapshot,
  reconcileMoveCommit,
} from "./interaction-v1.mjs";
import {
  assertRichInteractionIdentity,
  projectReaderSceneToEditorInteractionScene,
} from "./reader-scene-editor-interaction-v1.mjs";

const SVG_NS = "http://www.w3.org/2000/svg";

function clone(value) {
  return structuredClone(value);
}

function roundHalfAwayFromZero(value) {
  if (!Number.isFinite(value)) throw new TypeError("pointer coordinate must be finite");
  return Math.sign(value) * Math.floor(Math.abs(value) + 0.5);
}

function nodeById(interactionScene, nodeId) {
  return interactionScene.nodes.find((node) => node.node_id === nodeId) ?? null;
}

function editableShortcutTarget(target) {
  if (!target || typeof target !== "object") return false;
  const tag = String(target.tagName ?? "").toLowerCase();
  if (["input", "textarea", "select"].includes(tag)) return true;
  if (target.isContentEditable === true) return true;
  return !!target.closest?.("[contenteditable=true]");
}

export function isUndoShortcutEvent(event) {
  if (!event || typeof event !== "object") return false;
  if (event.defaultPrevented || event.repeat || event.altKey || event.shiftKey) return false;
  if (!(event.ctrlKey || event.metaKey)) return false;
  if (String(event.key ?? "").toLowerCase() !== "z") return false;
  return !editableShortcutTarget(event.target);
}

export function resolveRichEditorPointerTarget(
  interactionScene,
  pageId,
  point,
  visualNodeId = null,
) {
  const editableVisual = visualNodeId == null
    ? null
    : nodeById(interactionScene, visualNodeId);
  if (visualNodeId != null && !editableVisual) {
    return {
      kind: "read_only_visual",
      visual_node_id: visualNodeId,
    };
  }

  const hit = hitTestSnapshot(interactionScene, pageId, point);
  if (
    visualNodeId != null &&
    hit.kind === "hit" &&
    hit.node_id !== visualNodeId
  ) {
    return {
      kind: "ambiguous",
      node_ids: [hit.node_id, visualNodeId].sort(),
      reason: "visual_target_differs_from_geometry_hit",
    };
  }
  return hit;
}

export function richEditorPagePoint(pageSvg, clientX, clientY) {
  if (
    !pageSvg ||
    typeof pageSvg.createSVGPoint !== "function" ||
    typeof pageSvg.getScreenCTM !== "function"
  ) {
    throw new TypeError("rendered Reader page SVG is required");
  }
  const matrix = pageSvg.getScreenCTM();
  if (!matrix || typeof matrix.inverse !== "function") {
    throw new Error("rendered Reader page has no invertible screen transform");
  }
  const point = pageSvg.createSVGPoint();
  point.x = Number(clientX);
  point.y = Number(clientY);
  const local = point.matrixTransform(matrix.inverse());
  return {
    x_emu: roundHalfAwayFromZero(local.x),
    y_emu: roundHalfAwayFromZero(local.y),
  };
}

function svgNode(tag, attrs = {}) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) {
    node.setAttribute(key, String(value));
  }
  return node;
}

export class RichReaderEditorShellV1 {
  constructor({
    host,
    service,
    renderOptions = {},
    operationIdFactory = null,
    historyOperationIdFactory = null,
    keyboardTarget = globalThis.document ?? host,
    onState = null,
  }) {
    if (!host || typeof host.addEventListener !== "function") {
      throw new TypeError("host EventTarget is required");
    }
    if (
      !service ||
      typeof service.commit !== "function" ||
      typeof service.undo !== "function" ||
      typeof service.currentRichEditorState !== "function" ||
      typeof service.readerSceneForRevision !== "function"
    ) {
      throw new TypeError(
        "service must implement currentRichEditorState(), commit(), undo(), and readerSceneForRevision()",
      );
    }
    if (!keyboardTarget || typeof keyboardTarget.addEventListener !== "function") {
      throw new TypeError("keyboardTarget EventTarget is required");
    }
    this.host = host;
    this.keyboardTarget = keyboardTarget;
    this.service = service;
    this.renderOptions = { ...renderOptions };
    this.operationIdFactory =
      operationIdFactory ??
      (() => "rich-move-" + globalThis.crypto.randomUUID());
    this.historyOperationIdFactory =
      historyOperationIdFactory ??
      (() => "rich-undo-" + globalThis.crypto.randomUUID());
    this.onState = onState;
    this.readerScene = null;
    this.interactionScene = null;
    this.selectedNodeId = null;
    this.gesture = null;
    this.gesturePageId = null;
    this.pointerMoveCount = 0;
    this.commitRequests = 0;
    this.lastCommitResult = null;
    this.historyRequests = 0;
    this.lastHistoryResult = null;
    this.historyInFlight = false;
    this._handlers = null;
    this._bound = false;
    this._keyboardHandler = null;
    this._keyboardBound = false;
  }

  async start() {
    const state = await this.service.currentRichEditorState();
    await this.loadScenes(state.reader_scene, state.interaction_scene, {
      preserveSelection: false,
    });
    this.bindPointerEvents();
    this.bindKeyboardEvents();
    return this.stateReceipt();
  }

  async loadScenes(
    readerScene,
    interactionScene = projectReaderSceneToEditorInteractionScene(readerScene),
    { preserveSelection = true } = {},
  ) {
    assertRichInteractionIdentity(readerScene, interactionScene);
    const previousSelection = preserveSelection ? this.selectedNodeId : null;
    this.readerScene = clone(readerScene);
    this.interactionScene = clone(interactionScene);
    this.gesture = null;
    this.gesturePageId = null;

    await renderReaderScene(this.host, this.readerScene, this.renderOptions);

    if (
      previousSelection &&
      nodeById(this.interactionScene, previousSelection)
    ) {
      this.selectedNodeId = previousSelection;
    } else {
      this.selectedNodeId = null;
    }
    this._renderOverlay();
    this._emitState("scene_loaded");
    return this.stateReceipt();
  }

  bindPointerEvents() {
    if (this._bound) return;
    const down = (event) => {
      if (event.button !== 0 || !this.interactionScene) return;
      const pageSvg = this._eventPage(event);
      if (!pageSvg) {
        this._clearSelection("hit_test_none");
        return;
      }
      const pageId = pageSvg.getAttribute("data-page-id");
      const point = richEditorPagePoint(pageSvg, event.clientX, event.clientY);
      const visualNode = event.target?.closest?.("[data-node-id]") ?? null;
      const visualNodeId = visualNode?.getAttribute?.("data-node-id") ?? null;
      const hit = resolveRichEditorPointerTarget(
        this.interactionScene,
        pageId,
        point,
        visualNodeId,
      );
      if (hit.kind !== "hit") {
        this._clearSelection("hit_test_" + hit.kind);
        return;
      }

      this.selectedNodeId = hit.node_id;
      this.gesture = new MoveGestureV1(
        this.interactionScene,
        hit.node_id,
        point,
      );
      this.gesturePageId = pageId;
      try {
        this.host.setPointerCapture?.(event.pointerId);
      } catch {}
      this._renderOverlay();
      this._emitState("gesture_started");
    };

    const move = (event) => {
      if (!this.gesture) return;
      const pageSvg = this._pageSvg(this.gesturePageId);
      if (!pageSvg) {
        this.cancelGesture("gesture_page_missing");
        return;
      }
      const point = richEditorPagePoint(pageSvg, event.clientX, event.clientY);
      this.gesture.update(point);
      this.pointerMoveCount += 1;
      this._renderOverlay();
      this._emitState("gesture_preview");
    };

    const up = async (event) => {
      if (!this.gesture) return;
      const pageSvg = this._pageSvg(this.gesturePageId);
      if (!pageSvg) {
        this.cancelGesture("gesture_page_missing");
        return;
      }
      const point = richEditorPagePoint(pageSvg, event.clientX, event.clientY);
      try {
        await this._commitGesture(point);
      } catch (error) {
        this.gesture = null;
        this.gesturePageId = null;
        this._renderOverlay();
        this._emitState("commit_error");
        throw error;
      } finally {
        try {
          this.host.releasePointerCapture?.(event.pointerId);
        } catch {}
      }
    };

    const cancel = () => this.cancelGesture("gesture_cancelled");
    this.host.addEventListener("pointerdown", down);
    this.host.addEventListener("pointermove", move);
    this.host.addEventListener("pointerup", up);
    this.host.addEventListener("pointercancel", cancel);
    this._handlers = { down, move, up, cancel };
    this._bound = true;
  }

  unbindPointerEvents() {
    if (!this._bound) return;
    const { down, move, up, cancel } = this._handlers;
    this.host.removeEventListener("pointerdown", down);
    this.host.removeEventListener("pointermove", move);
    this.host.removeEventListener("pointerup", up);
    this.host.removeEventListener("pointercancel", cancel);
    this._handlers = null;
    this._bound = false;
  }

  bindKeyboardEvents() {
    if (this._keyboardBound) return;
    const keydown = (event) => {
      if (!isUndoShortcutEvent(event)) return;
      event.preventDefault?.();
      if (this.historyInFlight) return;
      void this.undo().catch(() => {});
    };
    this.keyboardTarget.addEventListener("keydown", keydown);
    this._keyboardHandler = keydown;
    this._keyboardBound = true;
  }

  unbindKeyboardEvents() {
    if (!this._keyboardBound) return;
    this.keyboardTarget.removeEventListener("keydown", this._keyboardHandler);
    this._keyboardHandler = null;
    this._keyboardBound = false;
  }

  cancelGesture(reason = "gesture_cancelled") {
    if (!this.gesture) return false;
    this.gesture.cancel();
    this.gesture = null;
    this.gesturePageId = null;
    this._renderOverlay();
    this._emitState(reason);
    return true;
  }

  stateReceipt() {
    return {
      protocol_version: "chaptera.rich-reader-editor-shell-state.v1",
      product_acceptance: false,
      visual_protocol_version: this.readerScene?.protocol_version ?? null,
      interaction_protocol_version:
        this.interactionScene?.protocol_version ?? null,
      document_id: this.readerScene?.document_id ?? null,
      source_hash: this.readerScene?.source_hash ?? null,
      revision_id: this.readerScene?.revision_id ?? null,
      selected_node_id: this.selectedNodeId,
      pointer_move_count: this.pointerMoveCount,
      commit_request_count: this.commitRequests,
      last_commit_protocol: this.lastCommitResult?.protocol_version ?? null,
      history_request_count: this.historyRequests,
      history_in_flight: this.historyInFlight,
      last_history_protocol: this.lastHistoryResult?.protocol_version ?? null,
      visual_scene_is_edit_authority: false,
      browser_interaction_is_durable_authority: false,
    };
  }

  destroy() {
    this.unbindPointerEvents();
    this.unbindKeyboardEvents();
    this.gesture = null;
    this.gesturePageId = null;
    this.readerScene = null;
    this.interactionScene = null;
    this.selectedNodeId = null;
    this.host.replaceChildren();
  }

  async undo() {
    if (!this.readerScene) throw new Error("editor scene is not loaded");
    if (this.gesture) this.cancelGesture("gesture_cancelled_for_undo");

    const request = {
      sourceHash: this.readerScene.source_hash,
      baseRevisionId: this.readerScene.revision_id,
      clientOperationId: this.historyOperationIdFactory(),
    };
    this.historyRequests += 1;
    this.historyInFlight = true;
    this._emitState("history_sent");

    try {
      const result = await this.service.undo(request);
      this.lastHistoryResult = clone(result);

      if (result.protocol_version === "chaptera.history-transition-accepted.v1") {
        const nextReaderScene =
          await this.service.readerSceneForRevision(result.revision_id);
        await this.loadScenes(
          nextReaderScene,
          projectReaderSceneToEditorInteractionScene(nextReaderScene),
          { preserveSelection: false },
        );
        this.historyInFlight = false;
        this._emitState("history_reconciled");
        return { request, result, reader_scene: clone(nextReaderScene) };
      }

      if (result.protocol_version !== "chaptera.history-transition-rejected.v1") {
        throw new Error("unknown history transition result protocol");
      }
      if (result.current_revision_id) {
        const currentReaderScene =
          await this.service.readerSceneForRevision(result.current_revision_id);
        await this.loadScenes(
          currentReaderScene,
          projectReaderSceneToEditorInteractionScene(currentReaderScene),
          { preserveSelection: false },
        );
      }
      this.historyInFlight = false;
      this._emitState("history_rejected");
      return { request, result, reader_scene: clone(this.readerScene) };
    } catch (error) {
      this.historyInFlight = false;
      this._emitState("history_error");
      throw error;
    }
  }

  async _commitGesture(point) {
    this.gesture.update(point);
    const gesture = this.gesture;
    const selectedNodeId = this.selectedNodeId;
    const request = gesture.commit(this.operationIdFactory());
    this.commitRequests += 1;
    this._emitState("commit_sent");

    let result;
    try {
      result = await this.service.commit(clone(request));
    } catch (error) {
      this.gesture = null;
      this.gesturePageId = null;
      this._renderOverlay();
      throw error;
    }

    this.lastCommitResult = clone(result);
    const reconciliation = reconcileMoveCommit(
      this.interactionScene,
      gesture,
      result,
    );
    this.gesture = null;
    this.gesturePageId = null;

    if (reconciliation.kind === "accepted_waiting_for_scene") {
      const nextReaderScene =
        await this.service.readerSceneForRevision(result.revision_id);
      const nextInteraction =
        projectReaderSceneToEditorInteractionScene(nextReaderScene);
      await this.loadScenes(nextReaderScene, nextInteraction, {
        preserveSelection: false,
      });
      if (nodeById(this.interactionScene, selectedNodeId)) {
        this.selectedNodeId = selectedNodeId;
      }
      this._renderOverlay();
      this._emitState("commit_reconciled");
      return {
        request,
        result,
        reconciliation,
        reader_scene: clone(nextReaderScene),
      };
    }

    if (result.current_revision_id) {
      const currentReaderScene =
        await this.service.readerSceneForRevision(result.current_revision_id);
      await this.loadScenes(
        currentReaderScene,
        projectReaderSceneToEditorInteractionScene(currentReaderScene),
        { preserveSelection: false },
      );
    } else {
      this._renderOverlay();
    }
    this._emitState("commit_rejected");
    return {
      request,
      result,
      reconciliation,
      reader_scene: clone(this.readerScene),
    };
  }

  _clearSelection(reason) {
    this.selectedNodeId = null;
    this.gesture = null;
    this.gesturePageId = null;
    this._renderOverlay();
    this._emitState(reason);
  }

  _eventPage(event) {
    return event.target?.closest?.("svg.page[data-page-id]") ?? null;
  }

  _pageSvg(pageId) {
    for (const page of this.host.querySelectorAll("svg.page[data-page-id]")) {
      if (page.getAttribute("data-page-id") === pageId) return page;
    }
    return null;
  }

  _renderOverlay() {
    for (const page of this.host.querySelectorAll("svg.page[data-page-id]")) {
      for (const old of page.querySelectorAll(
        ':scope > [data-layer="editor-transient-overlay"]',
      )) {
        old.remove();
      }
    }
    if (!this.interactionScene || !this.selectedNodeId) return;
    const selected = nodeById(this.interactionScene, this.selectedNodeId);
    if (!selected) return;
    const pageSvg = this._pageSvg(selected.page_id);
    if (!pageSvg) return;
    const preview = this.gesture?.previewBounds() ?? selected.bounds;
    const group = svgNode("g", {
      "data-layer": "editor-transient-overlay",
      "pointer-events": "none",
    });
    group.appendChild(svgNode("rect", {
      x: preview.x,
      y: preview.y,
      width: preview.width,
      height: preview.height,
      fill: "none",
      stroke: "rgb(0 80 220)",
      "stroke-width": 19050,
      "vector-effect": "non-scaling-stroke",
      "data-selection-node-id": selected.node_id,
    }));
    pageSvg.appendChild(group);
  }

  _emitState(reason) {
    this.onState?.({ reason, ...this.stateReceipt() });
  }
}
