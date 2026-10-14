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
    "temporary-download-pdf",
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
      // A reload resumes the same unedited Guest Reader file, not a new upload.
      if (url === "/v1/reader/guest-sessions/" + id + "/scene") return response({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, classification, source_sha256: source,
        expires_at_ms: 4_600_000, scene: readerScene(beforeRev),
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
  assert.equal(dom.nodes["temporary-download-pdf"].disabled, false,
    "baseline PDF can be downloaded without a MoveNode");
  assert.equal(dom.nodes["temporary-download-pdf"].textContent, "Скачать PDF");
  assert.equal(dom.nodes["temporary-download-idml"].disabled, true);
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
  assert.equal(dom.nodes["temporary-download-pdf"].disabled, false);
  assert.equal(dom.nodes["temporary-download-pdf"].textContent, "Скачать изменённый PDF");
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
  assert.equal(dom.nodes["temporary-download-pdf"].disabled, false,
    "click-only selection must not create an edit to unlock PDF");
});

for (const classification of ["supported", "partial"]) {
  test("guest " + classification + " reopens baseline PDF after page reload without new upload or revision", async () => {
    const sharedStorage = storage(), network = server(classification);
    const first = fakeDom(), second = fakeDom();
    const settings = {
      cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
      storage: sharedStorage, fetchImpl: network.fetchImpl,
      renderer: async () => {},
    };
    const opened = await bootTemporaryEditorPreview({
      ...settings, documentObject: first.documentObject,
    });
    await opened.start({ name: "saved.pub", size: 1729 });
    const initialExpiry = first.nodes["temporary-expiry"].textContent;
    assert.equal(first.nodes["temporary-download-pdf"].disabled, false);
    assert.equal(first.nodes["temporary-download-idml"].disabled, true);
    const resumed = await bootTemporaryEditorPreview({
      ...settings, documentObject: second.documentObject,
    });
    await resumed.resume();
    assert.equal(second.nodes["temporary-download-pdf"].disabled, false);
    assert.equal(second.nodes["temporary-download-pdf"].textContent, "Скачать PDF");
    assert.equal(second.nodes["temporary-download-idml"].disabled, true);
    assert.equal(second.nodes["temporary-download-odg"].disabled, true);
    assert.equal(second.nodes["temporary-expiry"].textContent, initialExpiry,
      "read-only Resume must not extend the server's one-hour TTL");
    assert.equal(network.calls.filter(x => x.url ===
      "/v1/editor/temporary-sessions" && x.options.method === "POST").length, 1,
      "refresh/resume must never mint an extra temporary session");
    assert.equal(network.calls.filter(x => x.url.endsWith("/content")).length, 1);
    assert.equal(network.calls.filter(x => x.url.endsWith("/move-node")).length, 0);
    assert.equal(network.calls.filter(x => x.url ===
      "/v1/reader/guest-sessions/" + id + "/scene").length, 1);
    if (classification === "partial") {
      assert.match(second.nodes["temporary-status"].textContent, /Needs review \/ Partial/);
    }
    assert.ok(network.calls.every(x => !x.url.includes(token)),
      "the private capability must not leak into request URLs");
  });
}

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
  assert.equal(dom.nodes["temporary-download-pdf"].disabled, true);
});


test("temporary Guest plain Story field posts exact private range and repaints accepted Scene", async () => {
  const dom = fakeDom();
  for (const key of [
    "temporary-story-panel", "temporary-story-select",
    "temporary-story-text", "temporary-story-apply",
  ]) {
    dom.nodes[key] = {
      hidden: false, disabled: false, value: "", textContent: "",
      handlers: {}, options: [],
      addEventListener(type, callback) { this.handlers[type] = callback; },
      replaceChildren() { this.options = []; },
      appendChild(item) { this.options.push(item); },
    };
  }
  dom.documentObject.createElement = tag => {
    assert.equal(tag, "option");
    return {value: "", textContent: ""};
  };
  const storyId = "01234567-89ab-4def-8abc-123456789abc";
  const network = server(), renders = [];
  let savedText = null;
  const storyCurrent = (rev, cursor, text) => {
    const reply = current(rev, cursor, 100, 200);
    reply.authoring_graph.nodes[nodeId].payload = { story_frame: { story_id: storyId } };
    reply.authoring_graph.stories = { [storyId]: {text, runs: [], paragraphs: [], fields: [], hyperlinks: []} };
    return reply;
  };
  const seen = [];
  const fetchImpl = async (url, options) => {
    seen.push({url, options});
    if (url === base + "/current") {
      return response(savedText === null ?
        storyCurrent(beforeRev, 0, "Initial\r") :
        storyCurrent(afterRev, 1, savedText));
    }
    if (url === base + "/scene") {
      return response({
        protocol_version: "chaptera.temporary-editor-scene.v1",
        document_id: id, source_hash: source,
        revision_id: savedText === null ? beforeRev : afterRev,
        expires_at_ms: savedText === null ? 4_600_000 : 8_200_000,
        scene: readerScene(savedText === null ? beforeRev : afterRev),
      });
    }
    if (url === base + "/story-range") {
      const body = JSON.parse(options.body);
      assert.equal(body.command.story_id, storyId);
      assert.equal(body.command.kind, "replace_story_range");
      assert.equal(body.command.start_scalar, 0);
      assert.equal(body.command.end_scalar, 7);
      assert.equal(body.command.replacement_text, "Edited");
      assert.equal(body.base_revision_id, beforeRev);
      assert.equal(options.headers["x-chaptera-reader-session"], token);
      assert.equal(savedText, null);
      // An original terminal Publisher CR must survive the browser edit.
      savedText = body.command.replacement_text + "\r";
      return response({
        protocol_version: "chaptera.temporary-editor-story-range.v1",
        document_id: id, source_hash: source, base_revision_id: beforeRev,
        revision_id: afterRev, canonical_authoring_revision_id: afterCanon,
        client_operation_id: body.client_operation_id, expires_at_ms: 8_200_000,
        replayed: false, scene_refresh: "current",
        canonical_operation: {
          kind: "replace_story_range", story_id: storyId,
          start_scalar: 0, end_scalar: 7, replacement_text: "Edited",
        },
      });
    }
    return network.fetchImpl(url, options);
  };
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: {getRandomValues(bytes) {bytes.fill(7);return bytes;}},
    storage: storage(), fetchImpl,
    renderer: async (_, scene) => {renders.push(scene);},
  });
  await controller.start({name: "example.pub", size: 1729});
  assert.equal(dom.nodes["temporary-story-panel"].hidden, false);
  assert.equal(dom.nodes["temporary-story-select"].value, storyId);
  assert.equal(dom.nodes["temporary-story-text"].value, "Initial");
  const editor = dom.nodes["temporary-story-text"];
  const apply = dom.nodes["temporary-story-apply"];
  editor.value = "Edited";
  editor.handlers.compositionstart();
  await apply.handlers.click();
  assert.equal(seen.filter(call => call.url.endsWith("/story-range")).length, 0);
  editor.handlers.compositionend();
  await apply.handlers.click();
  assert.equal(seen.filter(call => call.url.endsWith("/story-range")).length, 1);
  assert.equal(renders.at(-1).revision_id, afterRev);
  assert.equal(dom.nodes["temporary-story-text"].value, "Edited");
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
  assert.match(dom.nodes["temporary-status"].textContent, /Текст подтверждён сервером/);
});
