import test from "node:test";
import assert from "node:assert/strict";
import { bootTemporaryEditorPreview } from "./temporary-editor-preview.mjs";

const id = "tempedit:" + "a".repeat(32);
const token = "b".repeat(64);
const source = "c".repeat(64);
const beforeRev = "sha256:" + "d".repeat(64);
const afterRev = "sha256:" + "e".repeat(64);
const beforeCanon = "f".repeat(64);
const afterCanon = "1".repeat(64);
const nodeId = "10000000-0000-7000-8000-000000000001";
const pageId = "20000000-0000-7000-8000-000000000002";
const base = "/v1/editor/temporary-sessions/" + id;
const initialNode = { node_id: nodeId, page_id: pageId, kind: "shape",
  bounds: { x: 100, y: 200, width: 300, height: 300 } };
function readerScene(rev, x = 100, y = 200) {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: id, source_hash: source, revision_id: rev,
    pages: [{ page_id: pageId, order: 0, width_emu: 3000, height_emu: 4000 }],
    nodes: [{ ...initialNode, bounds: { ...initialNode.bounds, x, y } }],
  };
}
function current(rev, cursor, x, y) {
  return {
    protocol_version: "chaptera.temporary-editor-current.v1",
    document_id: id, source_hash: source, revision_id: rev,
    revision_cursor: cursor,
    canonical_authoring_revision_id: cursor ? afterCanon : beforeCanon,
    expires_at_ms: cursor ? 8_200_000 : 4_600_000,
    project: { schema_version: "editor" },
    authoring_graph: { nodes: { [nodeId]: { header: { bounds: {
      x, y, width: 300, height: 300,
    } } } } },
  };
}
function response(body, status = 200) {
  return new Response(JSON.stringify(body), { status,
    headers: { "content-type": "application/json" } });
}
function storage() {
  const values = new Map();
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: key => values.delete(key),
  };
}
function fakeDom() {
  const ids = [
    "temporary-preview-root", "temporary-upload-form", "temporary-file",
    "temporary-start", "temporary-resume", "temporary-forget",
    "temporary-status", "temporary-expiry", "temporary-pages",
    "temporary-export-actions", "temporary-download-idml", "temporary-download-odg",
  ];
  const nodes = {};
  for (const id of ids) {
    nodes[id] = {
      textContent: "", hidden: false, disabled: false, value: "",
      clientWidth: 700, files: [], handlers: {},
      addEventListener(name, handler) { this.handlers[name] = handler; },
      replaceChildren() { this.clears = (this.clears ?? 0) + 1; },
      contains() { return true; },
      setPointerCapture() {},
    };
  }
  const svg = {
    getAttribute(key) { return key === "data-page-id" ? pageId : null; },
    createSVGPoint() {
      return {
        x: 0, y: 0,
        matrixTransform() { return { x: this.x, y: this.y }; },
      };
    },
    getScreenCTM() { return { inverse() { return {}; } }; },
    appendChild() {},
  };
  const visualNode = { getAttribute: () => nodeId };
  const target = {
    closest(selector) {
      return selector.startsWith("svg.") ? svg :
        selector === "[data-node-id]" ? visualNode : null;
    },
  };
  const documentObject = {
    getElementById: name => nodes[name] ?? null,
    createElementNS: () => ({
      setAttribute() {}, remove() {},
    }),
  };
  const pointer = (clientX, clientY) => ({
    button: 0, pointerId: 1, target, clientX, clientY,
    preventDefault() {},
  });
  return { nodes, documentObject, pointer };
}
function server(classification = "supported") {
  const calls = [];
  let moved = null;
  return {
    calls,
    async fetchImpl(url, options) {
      calls.push({ url, options });
      if (url === "/v1/editor/temporary-sessions" && options.method === "POST") {
        return response({
          protocol_version: "chaptera.reader-guest-session.v1",
          mode: "temporary_edit", session_id: id, access_token: token,
          expires_at_ms: 1_000_000, max_file_bytes: 1000000,
          upload_path: "/v1/reader/guest-sessions/" + id + "/content",
          open_path: "/v1/reader/guest-sessions/" + id + "/open",
          scene_path: "/v1/reader/guest-sessions/" + id + "/scene",
        });
      }
      if (url.endsWith("/content")) return response({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, state: "stored", expires_at_ms: 4_600_000,
      });
      if (url.endsWith("/open")) return response({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, classification,
        source_sha256: source, expires_at_ms: 4_600_000,
        scene: readerScene(beforeRev),
      });
      if (url === base + "/current") return response(
        moved ? current(afterRev, 1, moved.x_emu, moved.y_emu) :
          current(beforeRev, 0, 100, 200),
      );
      if (url === base + "/scene") return response({
        protocol_version: "chaptera.temporary-editor-scene.v1",
        document_id: id, source_hash: source,
        revision_id: moved ? afterRev : beforeRev,
        expires_at_ms: moved ? 8_200_000 : 4_600_000,
        scene: moved ? readerScene(afterRev, moved.x_emu, moved.y_emu) :
          readerScene(beforeRev),
      });
      if (url === base + "/move-node") {
        assert.equal(options.method, "POST");
        assert.ok(!moved, "exactly one canonical commit");
        const body = JSON.parse(options.body);
        assert.equal(body.base_revision_id, beforeRev);
        moved = body.command;
        return response({
          protocol_version: "chaptera.temporary-editor-move-node.v1",
          document_id: id, source_hash: source,
          base_revision_id: beforeRev, revision_id: afterRev,
          canonical_authoring_revision_id: afterCanon,
          client_operation_id: body.client_operation_id,
          expires_at_ms: 8_200_000,
          replayed: false, scene_refresh: "current", canonical_operation: {},
        });
      }
      throw new Error("unexpected URL " + url);
    },
  };
}

test("real guest UI pointer drag posts once and renders exact revised Scene", async () => {
  const dom = fakeDom(), network = server(), renders = [];
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async (_, scene) => { renders.push(scene); },
  });
  await controller.start({ name: "example.pub", size: 1729 });
  assert.equal(renders.length, 1);
  assert.equal(renders[0].revision_id, beforeRev);
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  dom.nodes["temporary-pages"].handlers.pointermove(dom.pointer(150, 260));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(180, 270));
  assert.equal(network.calls.filter(x => x.url.endsWith("/move-node")).length, 1);
  assert.equal(renders.length, 2);
  assert.equal(renders[1].revision_id, afterRev);
  assert.equal(renders[1].nodes[0].bounds.x, 170);
  assert.equal(renders[1].nodes[0].bounds.y, 260);
  assert.match(dom.nodes["temporary-status"].textContent, /Правка сохранена/);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
  assert.ok(network.calls.every(x => !x.url.includes(token)));
});

test("click-only selection never writes a revision", async () => {
  const dom = fakeDom(), network = server();
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async () => {},
  });
  await controller.start({ name: "example.pub", size: 1729 });
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(110, 210));
  assert.equal(network.calls.filter(x => x.url.endsWith("/move-node")).length, 0);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, true);
});

test("partial real source retains a visible Needs review fence during edit", async () => {
  const dom = fakeDom(), network = server("partial");
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async () => {},
  });
  await controller.start({ name: "partial.pub", size: 1729 });
  assert.match(dom.nodes["temporary-status"].textContent, /Needs review \/ Partial/);
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  dom.nodes["temporary-pages"].handlers.pointermove(dom.pointer(150, 260));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(180, 270));
  assert.equal(network.calls.filter(c => c.url.endsWith("/move-node")).length, 1);
  assert.match(dom.nodes["temporary-status"].textContent, /Needs review \/ Partial/);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
});

test("unsupported upload never requests authoring state or permits a move", async () => {
  const dom = fakeDom(), network = server("unsupported");
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async () => { throw Error("unsupported rendered"); },
  });
  await controller.start({ name: "unsupported.pub", size: 1729 });
  assert.equal(network.calls.filter(c => c.url.endsWith("/current")).length, 0);
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(180, 270));
  assert.equal(network.calls.filter(c => c.url.endsWith("/move-node")).length, 0);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, true);
});
