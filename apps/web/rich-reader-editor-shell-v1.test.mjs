import test from "node:test";
import assert from "node:assert/strict";

import {
  isUndoShortcutEvent,
  historyShortcutCommand,
  nudgeShortcutDelta,
  PRODUCT_NUDGE_STEP_EMU,
  isUnchangedMoveGesture,
  RichReaderEditorShellV1,
  resolveRichEditorPointerTarget,
} from "./rich-reader-editor-shell-v1.mjs";

const PAGE = "20000000-0000-4000-8000-000000000001";
const A = "30000000-0000-4000-8000-000000000001";
const B = "30000000-0000-4000-8000-000000000002";

function interaction(nodes = [{
  node_id: A,
  page_id: PAGE,
  parent_node_id: null,
  kind: "shape",
  bounds: { x: 0, y: 0, width: 100, height: 100 },
  z_order: null,
  paint_order: null,
}]) {
  return {
    protocol_version: "chaptera.editor-interaction-scene.v1",
    document_id: "doc",
    source_hash: "a".repeat(64),
    revision_id: "sha256:" + "b".repeat(64),
    visual_protocol_version: "chaptera.reader-scene.v1",
    visual_scene_authority: "server_viewer_projection",
    stacking_fidelity: "unknown",
    pages: [{
      page_id: PAGE,
      order: 0,
      width_emu: 1000,
      height_emu: 1000,
    }],
    nodes,
  };
}

test("a projected visual on top never falls through to an editable origin", () => {
  const result = resolveRichEditorPointerTarget(
    interaction(),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    "sha256:" + "c".repeat(64),
  );
  assert.equal(result.kind, "read_only_visual");
});

test("blank visual area may hit one direct page-local interaction node", () => {
  assert.deepEqual(
    resolveRichEditorPointerTarget(
      interaction(),
      PAGE,
      { x_emu: 50, y_emu: 50 },
      null,
    ),
    { kind: "hit", node_id: A },
  );
});

test("overlapping direct nodes remain ambiguous without stacking authority", () => {
  const result = resolveRichEditorPointerTarget(
    interaction([
      {
        node_id: A,
        page_id: PAGE,
        parent_node_id: null,
        kind: "shape",
        bounds: { x: 0, y: 0, width: 100, height: 100 },
        z_order: null,
        paint_order: null,
      },
      {
        node_id: B,
        page_id: PAGE,
        parent_node_id: null,
        kind: "shape",
        bounds: { x: 0, y: 0, width: 100, height: 100 },
        z_order: null,
        paint_order: null,
      },
    ]),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    null,
  );
  assert.equal(result.kind, "ambiguous");
  assert.equal(result.reason, "stacking_not_authoritative");
});

test("visual target and geometry target must name the same canonical node", () => {
  const result = resolveRichEditorPointerTarget(
    interaction(),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    A,
  );
  assert.deepEqual(result, { kind: "hit", node_id: A });
});


test("Ctrl/Cmd+Z is reserved for authoritative Undo outside editable controls", () => {
  assert.equal(isUndoShortcutEvent({
    key: "z", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
    repeat: false, defaultPrevented: false, target: { tagName: "DIV" },
  }), true);
  assert.equal(isUndoShortcutEvent({
    key: "Z", ctrlKey: false, metaKey: true, altKey: false, shiftKey: false,
    repeat: false, defaultPrevented: false, target: { tagName: "DIV" },
  }), true);

  for (const event of [
    { key: "z", ctrlKey: true, shiftKey: true, target: { tagName: "DIV" } },
    { key: "z", ctrlKey: true, repeat: true, target: { tagName: "DIV" } },
    { key: "z", ctrlKey: true, target: { tagName: "INPUT" } },
    { key: "z", ctrlKey: true, target: { isContentEditable: true } },
    { key: "z", ctrlKey: false, metaKey: false, target: { tagName: "DIV" } },
  ]) {
    assert.equal(isUndoShortcutEvent({
      altKey: false,
      shiftKey: false,
      repeat: false,
      defaultPrevented: false,
      metaKey: false,
      ...event,
    }), false);
  }
});

test("Ctrl/Cmd+Shift+Z and Ctrl+Y route Redo without stealing text or IME history", () => {
  const base = {
    ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
    repeat: false, defaultPrevented: false, isComposing: false,
    target: { tagName: "DIV" },
  };
  assert.equal(historyShortcutCommand({ ...base, key: "z" }), "undo");
  assert.equal(historyShortcutCommand({ ...base, key: "Z", shiftKey: true }), "redo");
  assert.equal(historyShortcutCommand({ ...base, key: "y" }), "redo");
  assert.equal(historyShortcutCommand({
    ...base, key: "Z", ctrlKey: false, metaKey: true, shiftKey: true,
  }), "redo");
  for (const e of [
    { key: "z", shiftKey: true, target: { tagName: "INPUT" } },
    { key: "y", target: { tagName: "TEXTAREA" } },
    { key: "y", target: { isContentEditable: true } },
    { key: "y", target: { closest: () => true } },
    { key: "y", altKey: true },
    { key: "y", isComposing: true },
    { key: "z", shiftKey: true, keyCode: 229 },
    { key: "y", repeat: true },
    { key: "z", shiftKey: true, defaultPrevented: true },
    { key: "y", shiftKey: true },
    { key: "y", ctrlKey: false },
  ]) {
    assert.equal(historyShortcutCommand({ ...base, ...e }), null);
  }
});

test("unresolved move save blocks both history commands before any HTTP request", async () => {
  let calls = 0;
  const fake = { addEventListener(){}, removeEventListener(){} };
  const service = {
    commit(){}, currentRichEditorState(){}, readerSceneForRevision(){},
    undo(){ calls++; }, redo(){ calls++; },
  };
  const receipts = [];
  const shell = new RichReaderEditorShellV1({
    host:fake,keyboardTarget:fake,service,onState:state=>receipts.push(state),
  });
  shell.readerScene = { source_hash:"a".repeat(64),revision_id:"sha256:"+"b".repeat(64) };
  shell.commitInFlight = true;
  await assert.rejects(()=>shell.undo(), /history unavailable until/);
  await assert.rejects(()=>shell.redo(), /history unavailable until/);
  assert.equal(calls,0);
  assert.deepEqual(receipts.map(state=>state.reason),[
    "history_blocked_during_save","history_blocked_during_save",
  ]);
  assert.ok(receipts.every(state=>state.commit_in_flight===true));
});

test("lost MoveNode ACK keeps exact operation ID and locks all editing until source-exact server reconciliation", async () => {
  const SOURCE = "a".repeat(64);
  const BASE = "sha256:" + "b".repeat(64);
  const CHILD = "sha256:" + "c".repeat(64);
  let currentRevision = BASE;
  let recoveryMode = "network";
  let historyPosts = 0;
  const sent = [];
  const states = [];
  const host = { addEventListener(){}, removeEventListener(){} };
  const sceneFor = revisionId => ({
    protocol_version: "chaptera.reader-scene.v1",
    document_id: "doc",
    source_hash: SOURCE,
    revision_id: revisionId,
    pages: [{ page_id: PAGE, order: 0, width_emu: 1000, height_emu: 1000 }],
    nodes: [{ node_id: A, page_id: PAGE, kind: "shape",
      bounds: { x: 10, y: 10, width: 100, height: 100 },
      transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      paint: { preset_shape: null, fill_rgb: [20, 20, 20], line: null } }],
    stories: [], resources: [], fonts: [], diagnostics: [],
  });
  const service = {
    async commit(request) {
      sent.push(structuredClone(request));
      currentRevision = CHILD; // The authoritative server already committed.
      throw new Error("transport dropped after server commit");
    },
    async currentRichEditorState() {
      if (recoveryMode === "network") throw new Error("current document unreachable");
      const returnedRevision = recoveryMode === "stale" ? BASE : currentRevision;
      const scene = sceneFor(returnedRevision);
      if (recoveryMode === "wrong_source") scene.source_hash = "f".repeat(64);
      return {
        current_document: { protocol_version: "chaptera.current-document.v1",
          document_id: "doc", source_hash: SOURCE, revision_id: returnedRevision },
        reader_scene: scene,
      };
    },
    async readerSceneForRevision(revisionId) { return sceneFor(revisionId); },
    async undo() { historyPosts += 1; },
    async redo() { historyPosts += 1; },
  };
  const shell = new RichReaderEditorShellV1({
    host, keyboardTarget: host, service,
    operationIdFactory: () => "move-exact-operation-id",
    onState: state => states.push(state),
  });
  shell.readerScene = sceneFor(BASE);
  shell.interactionScene = interaction();
  shell._renderOverlay = () => {};
  shell.gesture = {
    update() {},
    commit(operationId) {
      return { protocol_version: "chaptera.commit-request.v1", document_id: "doc",
        source_hash: SOURCE, base_revision_id: BASE, client_operation_id: operationId,
        command: { kind: "move_node_to", node_id: A, x_emu: 50, y_emu: 50 } };
    },
  };
  await assert.rejects(() => shell._commitGesture({ x_emu: 50, y_emu: 50 }), /transport dropped/);
  assert.equal(shell.commitInFlight, false);
  assert.equal(shell.stateReceipt().commit_uncertain, true);
  assert.equal(shell.commitUncertain.clientOperationId, "move-exact-operation-id");
  assert.deepEqual(shell.pendingCommitRequest, sent[0]);
  assert.equal(states.at(-1).reason, "commit_ack_unknown");
  assert.equal(states.at(-1).commit_uncertain, true);
  await assert.rejects(() => shell.undo(), /history unavailable while commit outcome is uncertain/);
  await assert.rejects(() => shell.redo(), /history unavailable while commit outcome is uncertain/);
  await assert.rejects(() => shell._commitGesture({ x_emu: 60 }), /cannot commit another gesture/);
  assert.equal(historyPosts, 0);
  assert.equal(sent.length, 1);

  for (const mode of ["network", "stale", "wrong_source"]) {
    recoveryMode = mode;
    await assert.rejects(() => shell.reconcileUncertainCommit());
    assert.equal(shell.stateReceipt().commit_uncertain, true, "failed " + mode + " cannot unlock");
    assert.deepEqual(shell.pendingCommitRequest, sent[0], "operation ID must survive " + mode);
  }
  assert.equal(shell.readerScene.revision_id, BASE);
  // Only a source/doc/revision-exact authenticated current+Reader snapshot
  // may unlock. It does not prove that this particular lost ACK succeeded.
  recoveryMode = "confirmed";
  shell.loadScenes = async scene => {
    shell.readerScene = scene;
    shell.interactionScene = interaction();
  };
  const recovered = await shell.reconcileUncertainCommit();
  assert.equal(recovered.client_operation_id, "move-exact-operation-id");
  assert.equal(recovered.observed_revision_id, CHILD);
  assert.equal(recovered.original_commit_ack_proven, false);
  assert.equal(shell.stateReceipt().commit_uncertain, false);
  assert.equal(shell.readerScene.revision_id, CHILD);
  assert.equal(shell.pendingCommitRequest, null);
  assert.equal(states.at(-1).reason, "commit_authority_reconciled");
  assert.equal(historyPosts, 0);
});

test("lost Undo/Redo ACK preserves operation identity and blocks edits until canonical history recheck", async (t) => {
  const SOURCE = "a".repeat(64);
  const BASE = "sha256:" + "b".repeat(64);
  const CHILD = "sha256:" + "c".repeat(64);
  const sceneFor = revisionId => ({
    protocol_version: "chaptera.reader-scene.v1",
    document_id: "doc",
    source_hash: SOURCE,
    revision_id: revisionId,
    pages: [], nodes: [], stories: [], resources: [], fonts: [], diagnostics: [],
  });

  for (const kind of ["undo", "redo"]) {
    await t.test(kind, async () => {
      let recoveryMode = "network";
      let historyPosts = 0;
      let commitPosts = 0;
      const requests = [];
      const states = [];
      const host = { addEventListener() {}, removeEventListener() {} };
      const service = {
        async commit() { commitPosts += 1; },
        async undo(request) {
          historyPosts += 1;
          requests.push({ kind: "undo", ...structuredClone(request) });
          if (kind === "undo") throw new Error("transport dropped after durable undo");
          throw new Error("unexpected undo");
        },
        async redo(request) {
          historyPosts += 1;
          requests.push({ kind: "redo", ...structuredClone(request) });
          if (kind === "redo") throw new Error("transport dropped after durable redo");
          throw new Error("unexpected redo");
        },
        async currentRichEditorState() {
          if (recoveryMode === "network") throw new Error("server not reachable");
          const returnedRevision = recoveryMode === "stale" ? BASE : CHILD;
          const scene = sceneFor(returnedRevision);
          if (recoveryMode === "wrong_source") scene.source_hash = "f".repeat(64);
          if (recoveryMode === "wrong_document") scene.document_id = "other";
          if (recoveryMode === "different_scene_revision") scene.revision_id = BASE;
          return {
            current_document: {
              protocol_version: "chaptera.current-document.v1",
              document_id: "doc", source_hash: SOURCE, revision_id: returnedRevision,
            },
            reader_scene: scene,
          };
        },
        async readerSceneForRevision(revisionId) { return sceneFor(revisionId); },
      };
      const shell = new RichReaderEditorShellV1({
        host, keyboardTarget: host, service,
        historyOperationIdFactory: () => "history-exact-operation-id",
        onState: state => states.push(state),
      });
      shell.readerScene = sceneFor(BASE);
      shell.interactionScene = interaction();
      shell.loadScenes = async scene => {
        shell.readerScene = scene;
      };

      await assert.rejects(shell[kind](), /transport dropped after durable/);
      assert.equal(historyPosts, 1);
      assert.equal(shell.historyInFlight, false);
      assert.equal(shell.stateReceipt().history_uncertain, true);
      assert.equal(shell.historyUncertain.clientOperationId, "history-exact-operation-id");
      assert.deepEqual(shell.pendingHistoryRequest, { ...requests[0], documentId: "doc" });
      assert.equal(shell.pendingHistoryRequest.documentId, "doc");
      assert.equal(states.at(-1).reason, "history_ack_unknown");

      await assert.rejects(shell.undo(), /history unavailable while history outcome is uncertain/);
      await assert.rejects(shell.redo(), /history unavailable while history outcome is uncertain/);
      await assert.rejects(shell._commitGesture({ x_emu: 50, y_emu: 50 }), /cannot commit another gesture/);
      assert.equal(historyPosts, 1, "never send a duplicate history mutation");
      assert.equal(commitPosts, 0, "never send a MoveNode over unknown history");
      for (const mode of ["network", "stale", "wrong_source", "wrong_document", "different_scene_revision"]) {
        recoveryMode = mode;
        await assert.rejects(shell.reconcileUncertainHistory());
        assert.equal(shell.stateReceipt().history_uncertain, true, mode + " must stay locked");
        assert.deepEqual(shell.pendingHistoryRequest, { ...requests[0], documentId: "doc" });
      }
      assert.equal(shell.readerScene.revision_id, BASE);
      recoveryMode = "confirmed";
      const recovered = await shell.reconcileUncertainHistory();
      assert.equal(recovered.client_operation_id, "history-exact-operation-id");
      assert.equal(recovered.transition_kind, kind);
      assert.equal(recovered.observed_revision_id, CHILD);
      assert.equal(recovered.original_history_ack_proven, false);
      assert.equal(shell.stateReceipt().history_uncertain, false);
      assert.equal(shell.pendingHistoryRequest, null);
      assert.equal(shell.readerScene.revision_id, CHILD);
      assert.equal(states.at(-1).reason, "history_authority_reconciled");
      assert.equal(historyPosts, 1);
      assert.equal(commitPosts, 0);
    });
  }
});


test("canvas arrow nudge uses exact 1pt/10pt EMU and never steals forms or IME", () => {
  const event = {
    key: "ArrowRight", shiftKey: false, ctrlKey: false, metaKey: false,
    altKey: false, repeat: false, isComposing: false,
    defaultPrevented: false, target: { tagName: "DIV" },
  };
  assert.equal(PRODUCT_NUDGE_STEP_EMU, 12700);
  assert.deepEqual(nudgeShortcutDelta(event), { x_emu: 12700, y_emu: 0 });
  assert.deepEqual(nudgeShortcutDelta({ ...event, key: "ArrowLeft" }), { x_emu: -12700, y_emu: 0 });
  assert.deepEqual(nudgeShortcutDelta({ ...event, key: "ArrowUp" }), { x_emu: 0, y_emu: -12700 });
  assert.deepEqual(nudgeShortcutDelta({ ...event, key: "ArrowDown", shiftKey: true }), { x_emu: 0, y_emu: 127000 });
  for (const invalid of [
    { ctrlKey: true }, { metaKey: true }, { altKey: true }, { repeat: true },
    { isComposing: true }, { keyCode: 229 }, { defaultPrevented: true },
    { target: { tagName: "INPUT" } }, { target: { tagName: "TEXTAREA" } },
    { target: { tagName: "SELECT" } },
    { target: { tagName: "DIV", isContentEditable: true } },
    { target: { tagName: "SPAN", closest: sel => sel === "[contenteditable=true]" } },
    { key: "z" }, { key: "Delete" },
  ]) {
    assert.equal(nudgeShortcutDelta({ ...event, ...invalid }), null, JSON.stringify(invalid));
  }
});

test("canvas nudge owns keys only with a selected node and real canvas focus", async () => {
  const handlers = new Map();
  const host = {
    addEventListener() {}, removeEventListener() {},
    contains() { return false; },
  };
  const keyboardTarget = {
    addEventListener(name, fn) { handlers.set(name, fn); },
    removeEventListener(name) { handlers.delete(name); },
  };
  const service = {
    async commit() { throw Error("unexpected HTTP write"); },
    async undo() {}, async redo() {},
    async currentRichEditorState() {}, async readerSceneForRevision() {},
  };
  const shell = new RichReaderEditorShellV1({ host, keyboardTarget, service });
  shell.interactionScene = interaction();
  shell.readerScene = {
    protocol_version: "chaptera.reader-scene.v1", document_id: "doc",
    source_hash: "a".repeat(64), revision_id: "sha256:" + "b".repeat(64),
    pages: [], nodes: [],
  };
  shell.selectedNodeId = A;
  const deltas = [];
  shell.nudgeSelectedNode = async delta => { deltas.push(delta); };
  shell.bindKeyboardEvents();
  const keydown = handlers.get("keydown");
  const press = (target, rest = {}) => {
    let prevented = false;
    keydown({
      key: "ArrowRight", target, repeat: false, ctrlKey: false,
      metaKey: false, altKey: false, shiftKey: false, isComposing: false,
      defaultPrevented: false, preventDefault() { prevented = true; }, ...rest,
    });
    return prevented;
  };
  assert.equal(press(host), true);
  assert.deepEqual(deltas, [{ x_emu: 12700, y_emu: 0 }]);
  assert.equal(press({ tagName: "BUTTON" }), false);
  assert.equal(press({ tagName: "INPUT" }), false);
  assert.equal(press(host, { ctrlKey: true }), false);
  shell.selectedNodeId = null;
  assert.equal(press(host), false);
  shell.selectedNodeId = A;
  shell.commitUncertain = { originalRequest: true };
  assert.equal(press(host), true, "the lock must also stop browser-native key movement");
  assert.equal(deltas.length, 1, "cannot send nudge over an unknown durable save");
  shell.commitUncertain = null;
  shell.historyUncertain = { originalRequest: true };
  assert.equal(press(host), true);
  assert.equal(deltas.length, 1, "cannot send nudge over unknown Undo/Redo");
  shell.unbindKeyboardEvents();
  assert.equal(handlers.size, 0);
});

test("one arrow nudge creates one canonical MoveNode, preserving operation/scene authority", async () => {
  const SOURCE = "a".repeat(64);
  const BASE = "sha256:" + "b".repeat(64);
  const HEAD1 = "sha256:" + "c".repeat(64);
  const HEAD2 = "sha256:" + "d".repeat(64);
  const bounds = { x: 38000, y: 64000, width: 200000, height: 125000 };
  let revision = BASE;
  let canonicalBounds = { ...bounds };
  let operationId = 0;
  const sent = [];
  function readerScene() {
    return {
      protocol_version: "chaptera.reader-scene.v1", document_id: "doc",
      source_hash: SOURCE, revision_id: revision,
      pages: [{ page_id: PAGE, order: 0, width_emu: 800000, height_emu: 800000 }],
      nodes: [{ node_id: A, page_id: PAGE, parent_node_id: null,
        origin_node_id: null, kind: "shape", bounds: { ...canonicalBounds } }],
    };
  }
  const service = {
    async commit(req) {
      sent.push(structuredClone(req));
      assert.equal(req.base_revision_id, revision, "no stale or browser-synthetic base");
      const next = sent.length === 1 ? HEAD1 : HEAD2;
      canonicalBounds = { ...canonicalBounds, x: req.command.x_emu, y: req.command.y_emu };
      revision = next;
      return {
        protocol_version: "chaptera.commit-accepted.v1", document_id: "doc",
        source_hash: SOURCE, revision_id: next, base_revision_id: req.base_revision_id,
        client_operation_id: req.client_operation_id,
      };
    },
    async readerSceneForRevision(expected) {
      assert.equal(expected, revision);
      return readerScene();
    },
    async undo() {}, async redo() {},
    async currentRichEditorState() {},
  };
  const host = { addEventListener() {}, removeEventListener() {} };
  const shell = new RichReaderEditorShellV1({
    host, keyboardTarget: host, service,
    operationIdFactory: () => "nudge-" + (++operationId),
  });
  shell.readerScene = readerScene();
  shell.interactionScene = interaction([{ node_id: A, page_id: PAGE,
    parent_node_id: null, kind: "shape", bounds: { ...bounds },
    z_order: null, paint_order: null }]);
  shell.selectedNodeId = A;
  shell._renderOverlay = () => {};
  shell.loadScenes = async (scene, projected) => {
    shell.readerScene = structuredClone(scene);
    shell.interactionScene = structuredClone(projected);
    shell.gesture = null;
    shell.gesturePageId = null;
    shell.selectedNodeId = null;
  };
  const result1 = await shell.nudgeSelectedNode({ x_emu: 12700, y_emu: 0 });
  assert.equal(result1.result.revision_id, HEAD1);
  const result2 = await shell.nudgeSelectedNode({ x_emu: 0, y_emu: 127000 });
  assert.equal(result2.result.revision_id, HEAD2);
  assert.equal(shell.stateReceipt().commit_request_count, 2);
  assert.equal(shell.stateReceipt().selected_node_id, A);
  assert.equal(shell.stateReceipt().revision_id, HEAD2);
  assert.equal(shell.stateReceipt().commit_uncertain, false);
  assert.deepEqual(sent.map(x => x.command), [
    { kind: "move_node_to", node_id: A, x_emu: 50700, y_emu: 64000 },
    { kind: "move_node_to", node_id: A, x_emu: 50700, y_emu: 191000 },
  ]);
  assert.deepEqual(sent.map(x => x.client_operation_id), ["nudge-1", "nudge-2"]);
  assert.deepEqual(sent.map(x => x.base_revision_id), [BASE, HEAD1]);
  assert.ok(sent.every(x => x.source_hash === SOURCE && x.document_id === "doc"));
});

test("lost ACK while nudging fences all next mutations with the exact request", async () => {
  let posts = 0;
  const host = { addEventListener() {}, removeEventListener() {} };
  const service = {
    async commit() { ++posts; throw Error("ACK dropped after durable save"); },
    async undo() {}, async redo() {}, async currentRichEditorState() {},
    async readerSceneForRevision() {},
  };
  const shell = new RichReaderEditorShellV1({
    host, keyboardTarget: host, service,
    operationIdFactory: () => "nudge-stable-operation",
  });
  shell.readerScene = {
    protocol_version: "chaptera.reader-scene.v1", document_id: "doc",
    source_hash: "a".repeat(64), revision_id: "sha256:" + "b".repeat(64),
    pages: [], nodes: [],
  };
  shell.interactionScene = interaction();
  shell.selectedNodeId = A;
  shell._renderOverlay = () => {};
  await assert.rejects(shell.nudgeSelectedNode({ x_emu: 12700, y_emu: 0 }), /ACK dropped/);
  assert.equal(posts, 1);
  assert.equal(shell.stateReceipt().commit_uncertain, true);
  assert.equal(shell.commitUncertain.clientOperationId, "nudge-stable-operation");
  assert.deepEqual(shell.pendingCommitRequest, shell.commitUncertain.request);
  await assert.rejects(
    shell.nudgeSelectedNode({ x_emu: 12700, y_emu: 0 }),
    /nudge unavailable while an edit or history authority is pending/,
  );
  assert.equal(posts, 1);
  assert.equal(shell.commitUncertain.request.base_revision_id, "sha256:" + "b".repeat(64));
});


test("physical click selects direct object without a blank revision or Undo entry", async () => {
  const listeners = new Map();
  let commits = 0;
  let focuses = 0;
  let pointerReleases = 0;
  const page = {
    getAttribute(name) { return name === "data-page-id" ? PAGE : null; },
    getScreenCTM() { return { inverse() { return this; } }; },
    createSVGPoint() { return { x: 0, y: 0, matrixTransform() {
      return { x: this.x, y: this.y };
    } }; },
  };
  const visualNode = { getAttribute(name) {
    return name === "data-node-id" ? A : null;
  } };
  const host = {
    addEventListener(name, fn) { listeners.set(name, fn); },
    removeEventListener(name) { listeners.delete(name); },
    focus() { focuses += 1; },
    querySelectorAll() { return [page]; },
    setPointerCapture() {}, releasePointerCapture() { pointerReleases += 1; },
  };
  const service = {
    async commit() { commits += 1; throw Error("a click must not commit"); },
    async undo() {}, async redo() {},
    async currentRichEditorState() {}, async readerSceneForRevision() {},
  };
  const reasons = [];
  const shell = new RichReaderEditorShellV1({
    host, keyboardTarget: host, service, onState: receipt => reasons.push(receipt.reason),
  });
  shell.readerScene = {
    protocol_version: "chaptera.reader-scene.v1", document_id: "doc",
    source_hash: "a".repeat(64), revision_id: "sha256:" + "b".repeat(64),
    pages: [], nodes: [],
  };
  shell.interactionScene = interaction();
  shell._renderOverlay = () => {};
  shell.bindPointerEvents();
  const target = { closest(selector) {
    return selector === "svg.page[data-page-id]" ? page :
      selector === "[data-node-id]" ? visualNode : null;
  } };
  const pointer = (x, y) => ({
    button: 0, pointerId: 7, clientX: x, clientY: y, target,
  });
  listeners.get("pointerdown")(pointer(50, 50));
  assert.equal(isUnchangedMoveGesture(shell.interactionScene, shell.gesture), true);
  assert.equal(shell.selectedNodeId, A);
  await listeners.get("pointerup")(pointer(50, 50));
  assert.equal(shell.stateReceipt().commit_request_count, 0);
  assert.equal(commits, 0);
  assert.equal(shell.selectedNodeId, A);
  assert.equal(shell.gesture, null);
  assert.equal(reasons.at(-1), "gesture_selection_only");

  // A drag that returns to the original exact EMU coordinates is still selection.
  listeners.get("pointerdown")(pointer(50, 50));
  listeners.get("pointermove")(pointer(60, 60));
  assert.equal(isUnchangedMoveGesture(shell.interactionScene, shell.gesture), false);
  listeners.get("pointermove")(pointer(50, 50));
  assert.equal(isUnchangedMoveGesture(shell.interactionScene, shell.gesture), true);
  await listeners.get("pointerup")(pointer(50, 50));
  assert.equal(commits, 0);
  assert.equal(shell.selectedNodeId, A);
  assert.equal(focuses, 2);
  assert.equal(pointerReleases, 2);
  shell.unbindPointerEvents();
});

test("external Story mutation fence locks MoveNode/history until exact reconciliation", async () => {
  const host = {addEventListener() {}, removeEventListener() {}, replaceChildren() {}};
  const keyboard = {addEventListener() {}, removeEventListener() {}};
  const service = {
    commit() { throw new Error("must not POST MoveNode"); },
    undo() { throw new Error("must not POST history"); },
    currentRichEditorState() { throw new Error("not requested"); },
    readerSceneForRevision() { throw new Error("not requested"); },
  };
  const shell = new RichReaderEditorShellV1({host,service,keyboardTarget:keyboard});
  assert.equal(shell.stateReceipt().external_mutation_locked, false);
  shell.setExternalMutationLocked(true);
  assert.equal(shell.stateReceipt().external_mutation_locked, true);
  await assert.rejects(() => shell.undo(), /Story mutation is pending/);
  await assert.rejects(
    () => shell.nudgeSelectedNode({x_emu: 12700,y_emu: 0}),
    /nudge unavailable/
  );
  await assert.rejects(() => shell._commitGesture({x_emu: 0,y_emu: 0}), /save authority is pending/);
  shell.setExternalMutationLocked(false);
  assert.equal(shell.stateReceipt().external_mutation_locked, false);
  shell.commitInFlight = true;
  assert.throws(() => shell.setExternalMutationLocked(true), /another pending mutation/);
});
