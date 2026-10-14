import { renderReaderScene } from "../cloud-reader/render-v1.mjs";
import {
  MoveGestureV1,
  assertSafeEmu,
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

export function historyShortcutCommand(event) {
  if (!event || typeof event !== "object") return null;
  if (event.defaultPrevented || event.repeat || event.altKey ||
      event.isComposing || event.keyCode === 229) return null;
  if (!(event.ctrlKey || event.metaKey) || editableShortcutTarget(event.target)) return null;
  const key = String(event.key ?? "").toLowerCase();
  if (key === "z") return event.shiftKey ? "redo" : "undo";
  if (key === "y" && !event.shiftKey) return "redo";
  return null;
}

export function isUndoShortcutEvent(event) {
  return historyShortcutCommand(event) === "undo";
}

// One physical point is exactly 12,700 English Metric Units; no pixel/zoom
// conversion, floating drift or duplicated authoring authority in the browser.
export const PRODUCT_NUDGE_STEP_EMU = 12_700;

export function nudgeShortcutDelta(event) {
  if (!event || typeof event !== "object" || event.defaultPrevented ||
      event.repeat || event.altKey || event.ctrlKey || event.metaKey ||
      event.isComposing || event.keyCode === 229 ||
      editableShortcutTarget(event.target)) return null;
  const step = PRODUCT_NUDGE_STEP_EMU * (event.shiftKey ? 10 : 1);
  switch (event.key) {
    case "ArrowLeft": return { x_emu: -step, y_emu: 0 };
    case "ArrowRight": return { x_emu: step, y_emu: 0 };
    case "ArrowUp": return { x_emu: 0, y_emu: -step };
    case "ArrowDown": return { x_emu: 0, y_emu: step };
    default: return null;
  }
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

export function isUnchangedMoveGesture(interactionScene, gesture) {
  if (!interactionScene || !gesture) return false;
  const base = nodeById(interactionScene, gesture.nodeId)?.bounds;
  const preview = gesture.previewBounds();
  return Boolean(base && preview &&
    base.x === preview.x && base.y === preview.y);
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
    this.lastHistoryKind = null;
    this.historyInFlight = false;
    // Lost Undo/Redo ACKs have the same durable-commit uncertainty as MoveNode.
    this.historyUncertain = null;
    this.historyRecoveryInFlight = false;
    this.pendingHistoryRequest = null;
    this.lastHistoryRecovery = null;
    this.commitInFlight = false;
    this.externalMutationLocked = false;
    // Never mistake a lost HTTP ACK for a rejected canonical commit.
    this.commitUncertain = null;
    this.commitRecoveryInFlight = false;
    this.pendingCommitRequest = null;
    this.lastCommitRecovery = null;
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
      if (event.button !== 0 || !this.interactionScene ||
          this.commitInFlight || this.historyInFlight ||
          this.commitUncertain || this.commitRecoveryInFlight ||
          this.historyUncertain || this.historyRecoveryInFlight ||
          this.externalMutationLocked) return;
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
      // Only explicit canvas focus owns arrows; toolbar, forms and text input
      // retain their browser-native keyboard behavior.
      this.host.focus?.({ preventScroll: true });
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
      if (!this.gesture || this.commitInFlight || this.historyInFlight ||
          this.commitUncertain || this.commitRecoveryInFlight ||
          this.historyUncertain || this.historyRecoveryInFlight ||
          this.externalMutationLocked) return;
      const pageSvg = this._pageSvg(this.gesturePageId);
      if (!pageSvg) {
        this.cancelGesture("gesture_page_missing");
        return;
      }
      const point = richEditorPagePoint(pageSvg, event.clientX, event.clientY);
      try {
        // Selection is ephemeral. A click (or a drag back to its origin) must
        // not create an empty durable revision or consume an Undo slot.
        this.gesture.update(point);
        if (isUnchangedMoveGesture(this.interactionScene, this.gesture)) {
          this.cancelGesture("gesture_selection_only");
          return;
        }
        await this._commitGesture(point);
      } catch (error) {
        this.gesture = null;
        this.gesturePageId = null;
        this._renderOverlay();
        // A DOM event listener must not leak a rejected promise after a
        // lost ACK. _commitGesture already emitted the exact uncertain/fail
        // status and preserved the mutation fence for explicit recovery.
        this._emitState(this.commitUncertain ? "commit_ack_unknown" : "commit_error");
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
      const command = historyShortcutCommand(event);
      if (command) {
        event.preventDefault?.();
        if (this.historyInFlight || this.commitUncertain || this.commitRecoveryInFlight ||
            this.historyUncertain || this.historyRecoveryInFlight ||
            this.externalMutationLocked) {
          this._emitState(this.commitUncertain || this.historyUncertain
            ? "history_blocked_unknown_save" : "history_blocked_during_save");
          return;
        }
        void this[command]().catch(() => {});
        return;
      }

      const delta = nudgeShortcutDelta(event);
      if (!delta || !this.selectedNodeId ||
          (event.target !== this.host && !this.host.contains?.(event.target))) return;
      event.preventDefault?.();
      if (this.gesture || this.historyInFlight || this.commitInFlight ||
          this.commitUncertain || this.commitRecoveryInFlight ||
          this.historyUncertain || this.historyRecoveryInFlight ||
          this.externalMutationLocked) {
        this._emitState(this.commitUncertain || this.historyUncertain
          ? "nudge_blocked_unknown_save" : "nudge_blocked_during_save");
        return;
      }
      void this.nudgeSelectedNode(delta).catch(() => {});
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

  async nudgeSelectedNode(delta) {
    if (!delta || !Number.isSafeInteger(delta.x_emu) ||
        !Number.isSafeInteger(delta.y_emu) ||
        (delta.x_emu === 0 && delta.y_emu === 0)) {
      throw new TypeError("nudge requires a nonzero exact EMU delta");
    }
    if (this.gesture || this.historyInFlight || this.commitInFlight ||
        this.commitUncertain || this.commitRecoveryInFlight ||
        this.historyUncertain || this.historyRecoveryInFlight ||
        this.externalMutationLocked) {
      throw new Error("nudge unavailable while an edit or history authority is pending");
    }
    if (!this.interactionScene || !this.readerScene || !this.selectedNodeId) {
      throw new Error("nudge requires a selected canonical node");
    }
    const node = nodeById(this.interactionScene, this.selectedNodeId);
    if (!node || node.parent_node_id != null || node.editable === false) {
      throw new Error("nudge requires an editable direct page-local node");
    }
    const x = assertSafeEmu(node.bounds.x + delta.x_emu, "nudge x");
    const y = assertSafeEmu(node.bounds.y + delta.y_emu, "nudge y");
    // The same transient MoveGesture and canonical MoveNode POST as real
    // pointer drag. One keypress must generate exactly one durable operation.
    this.gesture = new MoveGestureV1(this.interactionScene, node.node_id, {
      x_emu: node.bounds.x,
      y_emu: node.bounds.y,
    });
    this.gesturePageId = node.page_id;
    try {
      return await this._commitGesture({ x_emu: x, y_emu: y });
    } catch (error) {
      this.gesture = null;
      this.gesturePageId = null;
      this._renderOverlay();
      throw error;
    }
  }

  cancelGesture(reason = "gesture_cancelled") {
    if (!this.gesture || this.commitInFlight || this.commitUncertain ||
        this.commitRecoveryInFlight || this.historyInFlight ||
        this.historyUncertain || this.historyRecoveryInFlight) return false;
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
      history_uncertain: this.historyUncertain !== null,
      history_recovery_in_flight: this.historyRecoveryInFlight,
      commit_in_flight: this.commitInFlight,
      external_mutation_locked: this.externalMutationLocked,
      commit_uncertain: this.commitUncertain !== null,
      commit_recovery_in_flight: this.commitRecoveryInFlight,
      history_command: this.lastHistoryKind,
      last_history_protocol: this.lastHistoryResult?.protocol_version ?? null,
      visual_scene_is_edit_authority: false,
      browser_interaction_is_durable_authority: false,
    };
  }

  setExternalMutationLocked(locked) {
    if (typeof locked !== "boolean") throw new TypeError("external mutation lock is boolean");
    if (locked && (this.commitInFlight || this.historyInFlight ||
        this.commitUncertain || this.commitRecoveryInFlight ||
        this.historyUncertain || this.historyRecoveryInFlight)) {
      throw new Error("cannot start Story edit during another pending mutation");
    }
    if (locked && this.gesture) this.cancelGesture("gesture_cancelled_for_text_edit");
    this.externalMutationLocked = locked;
    this._emitState(locked ? "external_mutation_pending" : "external_mutation_reconciled");
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
    return this.#historyTransition("undo");
  }

  async redo() {
    return this.#historyTransition("redo");
  }

  async #historyTransition(kind) {
    if (this.externalMutationLocked) {
      this._emitState("history_blocked_during_save");
      throw new Error("history unavailable while Story mutation is pending");
    }
    if (this.commitUncertain || this.commitRecoveryInFlight) {
      this._emitState("history_blocked_unknown_save");
      throw new Error("history unavailable while commit outcome is uncertain");
    }
    if (this.commitInFlight) {
      this._emitState("history_blocked_during_save");
      throw new Error("history unavailable until the current commit is confirmed");
    }
    if (this.historyUncertain || this.historyRecoveryInFlight) {
      this._emitState("history_blocked_unknown_save");
      throw new Error("history unavailable while history outcome is uncertain");
    }
    if (this.historyInFlight) throw new Error("history transition already pending");
    if (typeof this.service[kind] !== "function") throw new TypeError("service cannot perform " + kind);
    if (!this.readerScene) throw new Error("editor scene is not loaded");
    if (this.gesture) this.cancelGesture("gesture_cancelled_for_" + kind);

    const request = {
      sourceHash: this.readerScene.source_hash,
      baseRevisionId: this.readerScene.revision_id,
      clientOperationId: this.historyOperationIdFactory(),
    };
    this.historyRequests += 1;
    this.lastHistoryKind = kind;
    this.pendingHistoryRequest = { kind, ...clone(request), documentId: this.readerScene.document_id };
    this.historyInFlight = true;
    this._emitState("history_sent");

    try {
      const result = await this.service[kind](request);
      if (result.transition_kind !== kind) throw new Error("history transition kind mismatch");
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
        this.pendingHistoryRequest = null;
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
      this.pendingHistoryRequest = null;
      this._emitState("history_rejected");
      return { request, result, reader_scene: clone(this.readerScene) };
    } catch (error) {
      // The authoritative history operation may have committed before the
      // HTTP ACK or post-commit Reader Scene disappeared. Do not unlock.
      if (this.pendingHistoryRequest) {
        this.historyUncertain = clone(this.pendingHistoryRequest);
      }
      this.historyInFlight = false;
      this._emitState(this.historyUncertain ? "history_ack_unknown" : "history_error");
      throw error;
    }
  }

  async reconcileUncertainHistory() {
    const pending = this.historyUncertain;
    if (!pending) throw new Error("no unknown history transition requires reconciliation");
    if (this.historyRecoveryInFlight || this.historyInFlight || this.commitInFlight ||
        this.commitUncertain || this.commitRecoveryInFlight) {
      throw new Error("history authority reconciliation already pending");
    }
    this.historyRecoveryInFlight = true;
    this._emitState("history_recovery_checking");
    let reason = "history_recovery_failed";
    try {
      // Recheck the authenticated canonical /current and exact Reader Scene.
      // Head advancement alone cannot attribute the change to this Undo/Redo.
      const state = await this.service.currentRichEditorState();
      const current = state?.current_document;
      const scene = state?.reader_scene;
      if (!current || !scene ||
          current.protocol_version !== "chaptera.current-document.v1" ||
          scene.protocol_version !== "chaptera.reader-scene.v1" ||
          current.document_id !== pending.documentId ||
          scene.document_id !== pending.documentId ||
          current.source_hash !== pending.sourceHash ||
          scene.source_hash !== pending.sourceHash ||
          current.revision_id !== scene.revision_id ||
          typeof current.revision_id !== "string" ||
          current.revision_id === pending.baseRevisionId) {
        throw new Error("server has not confirmed a distinct exact canonical history revision");
      }
      await this.loadScenes(
        scene,
        projectReaderSceneToEditorInteractionScene(scene),
        { preserveSelection: false },
      );
      this.lastHistoryRecovery = {
        transition_kind: pending.kind,
        client_operation_id: pending.clientOperationId,
        observed_revision_id: current.revision_id,
        original_history_ack_proven: false,
      };
      this.pendingHistoryRequest = null;
      this.historyUncertain = null;
      reason = "history_authority_reconciled";
      return clone(this.lastHistoryRecovery);
    } finally {
      this.historyRecoveryInFlight = false;
      this._emitState(reason);
    }
  }

  async _commitGesture(point) {
    if (this.externalMutationLocked || this.commitInFlight ||
        this.historyInFlight || this.commitUncertain ||
        this.commitRecoveryInFlight || this.historyUncertain ||
        this.historyRecoveryInFlight) {
      throw new Error("cannot commit another gesture while save authority is pending");
    }
    this.commitInFlight = true;
    let settledReason = "commit_error";
    try {
      const outcome = await this._commitGestureInternal(point);
      settledReason = outcome.reconciliation?.kind === "accepted_waiting_for_scene"
        ? "commit_reconciled" : "commit_rejected";
      return outcome;
    } catch (error) {
      // After the POST is dispatched, transport errors do not prove rejection.
      // Retain the exact operation/idempotency key; lock every mutation.
      if (this.pendingCommitRequest) {
        this.commitUncertain = {
          request: clone(this.pendingCommitRequest),
          baseRevisionId: this.pendingCommitRequest.base_revision_id,
          sourceHash: this.pendingCommitRequest.source_hash,
          documentId: this.pendingCommitRequest.document_id,
          clientOperationId: this.pendingCommitRequest.client_operation_id,
        };
      }
      throw error;
    } finally {
      this.commitInFlight = false;
      if (!this.commitUncertain) this.pendingCommitRequest = null;
      this._emitState(this.commitUncertain ? "commit_ack_unknown" : settledReason);
    }
  }

  async reconcileUncertainCommit() {
    const pending = this.commitUncertain;
    if (!pending) throw new Error("no unknown commit requires reconciliation");
    if (this.commitRecoveryInFlight || this.commitInFlight || this.historyInFlight ||
        this.historyUncertain || this.historyRecoveryInFlight) {
      throw new Error("commit authority reconciliation already pending");
    }
    this.commitRecoveryInFlight = true;
    this._emitState("commit_recovery_checking");
    let reason = "commit_recovery_failed";
    try {
      // A fresh authenticated Product /current plus exact Reader Scene.
      // This does NOT prove which writer made the revision advance.
      const state = await this.service.currentRichEditorState();
      const current = state?.current_document;
      const scene = state?.reader_scene;
      if (!current || !scene ||
          current.protocol_version !== "chaptera.current-document.v1" ||
          scene.protocol_version !== "chaptera.reader-scene.v1" ||
          current.document_id !== pending.documentId ||
          scene.document_id !== pending.documentId ||
          current.source_hash !== pending.sourceHash ||
          scene.source_hash !== pending.sourceHash ||
          current.revision_id !== scene.revision_id ||
          typeof current.revision_id !== "string" ||
          current.revision_id === pending.baseRevisionId) {
        throw new Error("server has not confirmed a distinct exact canonical revision");
      }
      await this.loadScenes(
        scene,
        projectReaderSceneToEditorInteractionScene(scene),
        { preserveSelection: false },
      );
      this.lastCommitRecovery = {
        client_operation_id: pending.clientOperationId,
        observed_revision_id: current.revision_id,
        original_commit_ack_proven: false,
      };
      this.pendingCommitRequest = null;
      this.commitUncertain = null;
      reason = "commit_authority_reconciled";
      return clone(this.lastCommitRecovery);
    } finally {
      this.commitRecoveryInFlight = false;
      // On failure, original request and fail-closed mutation fence remain.
      this._emitState(reason);
    }
  }

  async _commitGestureInternal(point) {
    this.gesture.update(point);
    const gesture = this.gesture;
    const selectedNodeId = this.selectedNodeId;
    const request = gesture.commit(this.operationIdFactory());
    this.pendingCommitRequest = clone(request);
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
