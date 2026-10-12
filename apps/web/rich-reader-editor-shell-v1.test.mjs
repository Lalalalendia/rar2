import test from "node:test";
import assert from "node:assert/strict";

import {
  isUndoShortcutEvent,
  historyShortcutCommand,
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
