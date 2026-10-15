import test from "node:test";
import assert from "node:assert/strict";
import {
  bootTemporaryEditorPreview, temporaryRectangleDragBounds,
} from "./temporary-editor-preview.mjs";

const id = "tempedit:" + "a".repeat(32);
const token = "b".repeat(64);
const source = "c".repeat(64);
const pageId = "20000000-0000-7000-8000-000000000002";
const nodeId = "019a2d80-1112-7001-8001-aabbccddee01";
const oldRev = "sha256:" + "d".repeat(64);
const newRev = "sha256:" + "e".repeat(64);
const oldCanon = "f".repeat(64);
const newCanon = "1".repeat(64);
const base = "/v1/editor/temporary-sessions/" + id;
const bounds = {x: 100, y: 200, width: 300, height: 300};
const operation = {
  kind: "create_shape", node_id: nodeId, page_id: pageId, parent_id: pageId,
  shape_kind: "rectangle", bounds,
  transform: {kind: "identity"},
  paint: {
    fill: {visible: true, color: {r: 18, g: 52, b: 86}},
    stroke: {visible: true, color: {r: 170, g: 187, b: 204}, width_emu: 12_700},
    provenance: {kind: "author_created"},
  },
  provenance: {kind: "author_created"},
};
const page = {page_id: pageId, order: 0, width_emu: 3_000, height_emu: 4_000};
const baselineScene = {
  protocol_version: "chaptera.reader-scene.v1",
  document_id: id, source_hash: source, revision_id: oldRev,
  pages: [page], nodes: [], stacking_fidelity: "unknown",
};
const newScene = {
  ...baselineScene, revision_id: newRev,
  fidelity: {state: "partial", reasons: ["authored_rectangle_mixed_order_unsupported"]},
  nodes: [{
    node_id: nodeId, page_id: pageId, kind: "shape", bounds,
    paint: {fill_rgb: [18, 52, 86], line: {rgb: [170, 187, 204], width_emu: 12_700}},
  }],
};
function current(edited = false) {
  return {
    protocol_version: "chaptera.temporary-editor-current.v1",
    document_id: id, source_hash: source,
    revision_id: edited ? newRev : oldRev,
    revision_cursor: edited ? 1 : 0,
    canonical_authoring_revision_id: edited ? newCanon : oldCanon,
    expires_at_ms: edited ? 8_200_000 : 4_600_000,
    project: {operations: edited ? [operation] : []},
    authoring_graph: {
      nodes: {}, pages: {[pageId]: {size: {width: 3_000, height: 4_000}}},
    },
  };
}
function reply(data, code = 200) {
  return new Response(JSON.stringify(data), {
    status: code, headers: {"content-type": "application/json"},
  });
}
function fakeDom() {
  const ids = [
    "temporary-preview-root", "temporary-upload-form", "temporary-file",
    "temporary-start", "temporary-resume", "temporary-forget",
    "temporary-status", "temporary-expiry", "temporary-pages",
    "temporary-export-actions", "temporary-draw-rectangle",
    "temporary-download-idml", "temporary-download-odg", "temporary-download-pdf",
  ];
  const nodes = Object.fromEntries(ids.map(id => [id, {
    handlers: {}, textContent: "", hidden: false, disabled: false, value: "",
    files: [], clientWidth: 700, attributes: {},
    addEventListener(kind, fn) {this.handlers[kind] = fn;},
    setAttribute(key, val) {this.attributes[key] = val;},
    getAttribute(key) {return this.attributes[key] ?? null;},
    replaceChildren() {this.clears = (this.clears ?? 0) + 1;},
    contains() {return true;},
    setPointerCapture() {},
  }]));
  const svg = {
    getAttribute(name) {return name === "data-page-id" ? pageId : null;},
    createSVGPoint() {
      return {x: 0, y: 0, matrixTransform() {return {x: this.x, y: this.y};}};
    },
    getScreenCTM() {return {inverse() {return {};}};},
    appendChild() {},
  };
  const target = {
    closest(selector) {
      return selector.startsWith("svg.") ? svg : null;
    },
  };
  return {
    nodes, documentObject: {
      getElementById: key => nodes[key] ?? null,
      createElementNS: () => ({setAttribute() {}, remove() {}}),
    },
    pointer(x,y) {return {
      button: 0, pointerId: 1, target, clientX: x, clientY: y,
      preventDefault() {},
    };},
  };
}
function server({badScene = false} = {}) {
  const calls = [];
  let applied = false;
  return {
    calls,
    async fetchImpl(url, options) {
      calls.push({url, options});
      if (url === "/v1/editor/temporary-sessions" && options.method === "POST") {
        return reply({
          protocol_version: "chaptera.reader-guest-session.v1",
          mode: "temporary_edit", session_id: id, access_token: token,
          expires_at_ms: 1_000_000, max_file_bytes: 1_000_000,
          upload_path: "/v1/reader/guest-sessions/" + id + "/content",
          open_path: "/v1/reader/guest-sessions/" + id + "/open",
          scene_path: "/v1/reader/guest-sessions/" + id + "/scene",
        });
      }
      if (url.endsWith("/content")) return reply({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, state: "stored", expires_at_ms: 4_600_000,
      });
      if (url.endsWith("/open")) return reply({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, classification: "supported",
        source_sha256: source, expires_at_ms: 4_600_000,
        scene: baselineScene,
      });
      if (url === base + "/current") return reply(current(applied));
      if (url === base + "/scene") return reply({
        protocol_version: "chaptera.temporary-editor-scene.v1",
        document_id: id, source_hash: source,
        revision_id: applied ? newRev : oldRev,
        expires_at_ms: applied ? 8_200_000 : 4_600_000,
        scene: applied ? (badScene ? {...newScene, nodes: []} : newScene) : baselineScene,
      });
      if (url === base + "/create-shape") {
        const body = JSON.parse(options.body);
        assert.equal(applied, false, "one canonical CreateShape per gesture");
        assert.equal(body.base_revision_id, oldRev);
        assert.equal(body.command.page_id, pageId);
        assert.equal(body.command.kind, "create_shape");
        assert.equal("node_id" in body.command, false);
        assert.deepEqual([
          body.command.x_emu, body.command.y_emu,
          body.command.width_emu, body.command.height_emu,
        ], [bounds.x, bounds.y, bounds.width, bounds.height]);
        applied = true;
        return reply({
          protocol_version: "chaptera.temporary-editor-create-shape.v1",
          document_id: id, source_hash: source,
          base_revision_id: oldRev, revision_id: newRev,
          canonical_authoring_revision_id: newCanon,
          client_operation_id: body.client_operation_id,
          expires_at_ms: 8_200_000,
          replayed: false, scene_refresh: "current",
          canonical_operation: operation,
        });
      }
      throw Error("unexpected HTTP: " + url);
    },
  };
}
function memoryStorage() {
  const data = new Map();
  return {
    getItem: key => data.get(key) ?? null,
    setItem: (key,val) => data.set(key,val),
    removeItem: key => data.delete(key),
  };
}
const secureRandom = { getRandomValues(bytes) {bytes.fill(7); return bytes;} };

test("Rectangle drag computes bounded positive page-local geometry for either direction", () => {
  const a = {x_emu: 500, y_emu: 800}, b = {x_emu: 100, y_emu: 200};
  assert.deepEqual(temporaryRectangleDragBounds(a,b,page), {
    x: 100, y: 200, width: 400, height: 600,
  });
  assert.equal(temporaryRectangleDragBounds(a,a,page), null);
  assert.equal(temporaryRectangleDragBounds({x_emu: -1,y_emu: 10},b,page), null);
  assert.equal(temporaryRectangleDragBounds(a,{x_emu:4000,y_emu:900},page), null);
  assert.equal(temporaryRectangleDragBounds(a,{x_emu:NaN,y_emu:0},page), null);
});

test("Rectangle mode drags one new shape, reopens canonical Scene and unlocks verified export", async () => {
  const dom = fakeDom(), network = server(), scenes = [];
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject, cryptoObject: secureRandom,
    storage: memoryStorage(), fetchImpl: network.fetchImpl,
    renderer: async (_,scene) => {scenes.push(scene);},
  });
  await controller.start({name:"editor.pub",size:1729});
  assert.equal(dom.nodes["temporary-draw-rectangle"].disabled, false);
  dom.nodes["temporary-draw-rectangle"].handlers.click();
  assert.equal(dom.nodes["temporary-draw-rectangle"].attributes["aria-pressed"], "true");
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(100,200));
  dom.nodes["temporary-pages"].handlers.pointermove(dom.pointer(350,450));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(400,500));
  assert.equal(network.calls.filter(c => c.url.endsWith("/create-shape")).length, 1);
  assert.equal(scenes.length, 2);
  assert.deepEqual(scenes[1], newScene);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
  assert.match(dom.nodes["temporary-status"].textContent, /Прямоугольник сохранён сервером/);
  assert.match(dom.nodes["temporary-status"].textContent, /Needs review/);
  assert.ok(network.calls.every(call => !call.url.includes(token)));
});

test("Rectangle click without drag creates no revision and keeps IDML unavailable", async () => {
  const dom = fakeDom(), network = server();
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject, cryptoObject: secureRandom,
    storage: memoryStorage(), fetchImpl: network.fetchImpl,
    renderer: async () => {},
  });
  await controller.start({name:"editor.pub",size:1729});
  dom.nodes["temporary-draw-rectangle"].handlers.click();
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(100,200));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(100,200));
  assert.equal(network.calls.filter(c => c.url.endsWith("/create-shape")).length, 0);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, true);
});

test("Misprojected authored shape never appears as accepted in the browser", async () => {
  const dom = fakeDom(), network = server({badScene:true});
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject, cryptoObject: secureRandom,
    storage: memoryStorage(), fetchImpl: network.fetchImpl,
    renderer: async () => {},
  });
  await controller.start({name:"editor.pub",size:1729});
  dom.nodes["temporary-draw-rectangle"].handlers.click();
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(100,200));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(400,500));
  assert.match(dom.nodes["temporary-status"].textContent, /не подтверждено/);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, true);
  assert.ok(dom.nodes["temporary-pages"].clears > 0);
});
