import test from "node:test";
import assert from "node:assert/strict";
import { bootTemporaryEditorPreview, temporaryCanvasStoryId } from "./temporary-editor-preview.mjs";

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
function readerScene(rev, x = 100, y = 200, widthEmu = 3000, heightEmu = 4000) {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: id, source_hash: source, revision_id: rev,
    pages: [{ page_id: pageId, order: 0, width_emu: widthEmu, height_emu: heightEmu }],
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
    "temporary-download-pdf", "temporary-download-pdf9up",
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
function server(classification = "supported", card = false) {
  const calls = [];
  let moved = null;
  const scene = (rev, x = 100, y = 200) => readerScene(
    rev, x, y, card ? 3_060_000 : 3000, card ? 1_980_000 : 4000,
  );
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
        scene: scene(beforeRev),
      });
      // Resume reads the same stored Guest session without another upload.
      if (url === "/v1/reader/guest-sessions/" + id + "/scene") return response({
        protocol_version: "chaptera.reader-guest-session.v1",
        session_id: id, classification, source_sha256: source,
        expires_at_ms: 4_600_000, scene: scene(beforeRev),
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
        scene: moved ? scene(afterRev, moved.x_emu, moved.y_emu) :
          scene(beforeRev),
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
    "temporary-story-text", "temporary-story-apply", "temporary-story-discard",
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
  const otherStoryId = "00000000-0000-7000-8000-000000000001";
  const otherNodeId = "30000000-0000-7000-8000-000000000003";
  const network = server(), renders = [];
  let savedText = null;
  const storyCurrent = (rev, cursor, text) => {
    const reply = current(rev, cursor, 100, 200);
    reply.authoring_graph.pages = {[pageId]: {size: {width: 3000, height: 4000}}};
    reply.authoring_graph.nodes[nodeId].header.parent_id = pageId;
    reply.authoring_graph.nodes[nodeId].payload = { story_frame: { story_id: storyId } };
    reply.authoring_graph.nodes[otherNodeId] = {
      header: {parent_id: pageId, bounds: {x: 800, y: 900, width: 300, height: 300}},
      payload: {story_frame: {story_id: otherStoryId}},
    };
    reply.authoring_graph.stories = {
      [storyId]: {text, runs: [], paragraphs: [], fields: [], hyperlinks: []},
      [otherStoryId]: {text: "Other\\r", runs: [], paragraphs: [], fields: [], hyperlinks: []},
    };
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
  assert.equal(dom.nodes["temporary-story-select"].disabled, false);
  assert.equal(dom.nodes["temporary-story-text"].disabled, false);
  assert.equal(dom.nodes["temporary-story-apply"].disabled, false);
  assert.equal(dom.nodes["temporary-story-select"].value, otherStoryId,
    "the dropdown initially shows the first eligible Story, not the clicked frame");
  const svg = dom.pointer(110, 210).target.closest("svg.page");
  svg.appendChild = () => {};
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(110, 210));
  assert.equal(dom.nodes["temporary-story-select"].value, storyId,
    "canvas selection must activate the Story owned by its canonical TextFrame");
  assert.equal(dom.nodes["temporary-story-text"].value, "Initial");
  assert.equal(seen.filter(call => /\\/(?:story-range|move-node)$/.test(call.url)).length, 0,
    "selection never commits or extends the TTL");
  const editor = dom.nodes["temporary-story-text"];
  const apply = dom.nodes["temporary-story-apply"];
  editor.value = "Edited";
  editor.handlers.input();
  const selector = dom.nodes["temporary-story-select"];
  selector.value = otherStoryId;
  selector.handlers.change();
  assert.equal(selector.value, storyId, "dirty Story must veto switching candidates");
  assert.equal(editor.value, "Edited", "dirty text must survive a blocked switch");
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(110, 210));
  assert.equal(seen.filter(call => call.url.endsWith("/move-node")).length, 0,
    "canvas gestures cannot discard a dirty Story");
  assert.equal(editor.value, "Edited");
  editor.handlers.compositionstart();
  assert.equal(apply.disabled, true, "IME composition must disable Story submission");
  await apply.handlers.click();
  assert.equal(seen.filter(call => call.url.endsWith("/story-range")).length, 0);
  editor.handlers.compositionend();
  assert.equal(apply.disabled, false, "Story submission resumes after composition");
  await apply.handlers.click();
  assert.equal(seen.filter(call => call.url.endsWith("/story-range")).length, 1);
  assert.equal(renders.at(-1).revision_id, afterRev);
  assert.equal(dom.nodes["temporary-story-text"].value, "Edited");
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
  assert.equal(dom.nodes["temporary-story-select"].disabled, false);
  assert.equal(dom.nodes["temporary-story-text"].disabled, false);
  assert.equal(dom.nodes["temporary-story-apply"].disabled, false);
  assert.match(dom.nodes["temporary-status"].textContent, /Текст подтверждён сервером/);
  editor.value = "Unsaved local change";
  editor.handlers.input();
  const editsBeforeDiscard = seen.filter(call => call.url.endsWith("/story-range")).length;
  assert.equal(dom.nodes["temporary-story-discard"].disabled, false);
  dom.nodes["temporary-story-discard"].handlers.click();
  assert.equal(editor.value, "Edited", "explicit discard restores authoritative Story");
  assert.equal(seen.filter(call => call.url.endsWith("/story-range")).length, editsBeforeDiscard,
    "discard is not a mutation");
  selector.value = otherStoryId;
  selector.handlers.change();
  assert.equal(selector.value, otherStoryId, "clean draft permits selecting a different Story");
  assert.equal(editor.value, "Other", "the new Story text is rendered from canonical state");
});

test("A4 nine-card control is available only for the exact edited 85x55 mm page", async () => {
  for (const card of [true, false]) {
    const dom = fakeDom(), network = server("supported", card);
    const controller = await bootTemporaryEditorPreview({
      documentObject: dom.documentObject,
      cryptoObject: { getRandomValues(bytes) { bytes.fill(7); return bytes; } },
      storage: storage(), fetchImpl: network.fetchImpl,
      renderer: async () => {},
    });
    await controller.start({ name: "card.pub", size: 1729 });
    const button = dom.nodes["temporary-download-pdf9up"];
    assert.equal(button.hidden, !card, "invalid card geometry must not offer 9-up");
    assert.equal(button.disabled, true, "unmodified source cannot be exported as edited");
    dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
    dom.nodes["temporary-pages"].handlers.pointermove(dom.pointer(150, 260));
    await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(180, 270));
    assert.equal(network.calls.filter(c => c.url.endsWith("/move-node")).length, 1);
    assert.equal(button.hidden, !card);
    assert.equal(button.disabled, !card, "only the admitted edited card can be printed 9-up");
  }
});

test("A4 sheet preview displays only report-bound PDF and revokes it on forget; a forged report is refused", async () => {
  for (const matching of [true, false]) {
    const dom = fakeDom(), network = server("supported", true);
    for (const key of ["temporary-preview-pdf9up", "temporary-sheet-preview", "temporary-sheet-pdf-frame"]) {
      dom.nodes[key] = {
        hidden: false, disabled: false, src: "", textContent: "", handlers: {},
        addEventListener(type, callback) { this.handlers[type] = callback; },
      };
    }
    const pdf = new TextEncoder().encode("%PDF-1.7\\n1 0 obj\\n<<>>\\nendobj\\n%%EOF\\n");
    const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", pdf));
    const hash = [...digest].map(x => x.toString(16).padStart(2, "0")).join("");
    const calls = [];
    const fetchImpl = async (url, options) => {
      if (!url.startsWith(base + "/exports/pdf9up?")) return network.fetchImpl(url, options);
      calls.push({url, options});
      const query = new URL(url, "https://reader.example").searchParams;
      assert.equal(query.get("revision_id"), afterRev);
      assert.equal(query.get("canonical_authoring_revision_id"), afterCanon);
      assert.equal(options.headers["x-chaptera-reader-session"], token);
      assert.ok(!url.includes(token), "bearer must not be exposed in export URL");
      if (query.get("report_only") === "true") {
        assert.equal(options.headers.accept, "application/json");
        return response({
          protocol_version: "chaptera.temporary-edited-export-report.v1",
          source_hash: source, revision_id: afterRev,
          canonical_authoring_revision_id: afterCanon, target: "pdf9up",
          artifact_sha256: matching ? hash : "0".repeat(64),
          loss_report: {counts: {blocking: 0}},
        });
      }
      assert.equal(options.headers.accept, "application/pdf");
      return new Response(pdf, {headers: {
        "content-type": "application/pdf",
        "x-chaptera-edit-revision": afterRev,
        "x-chaptera-export-sha256": hash,
      }});
    };
    const controller = await bootTemporaryEditorPreview({
      documentObject: dom.documentObject,
      cryptoObject: {getRandomValues(bytes) {bytes.fill(7);return bytes;}},
      storage: storage(), fetchImpl, renderer: async () => {},
    });
    await controller.start({name: "card.pub", size: 1729});
    const preview = dom.nodes["temporary-preview-pdf9up"];
    assert.equal(preview.hidden, false);
    assert.equal(preview.disabled, true, "unmodified card cannot be previewed as edited");
    dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
    dom.nodes["temporary-pages"].handlers.pointermove(dom.pointer(150, 260));
    await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(180, 270));
    assert.equal(preview.disabled, false);
    await preview.handlers.click();
    assert.equal(calls.length, 2, "exactly one report and one PDF request");
    if (matching) {
      assert.equal(dom.nodes["temporary-sheet-preview"].hidden, false);
      assert.match(dom.nodes["temporary-sheet-pdf-frame"].src, /^blob:/);
      assert.match(dom.nodes["temporary-status"].textContent, /контрольной сумме/);
    } else {
      assert.equal(dom.nodes["temporary-sheet-preview"].hidden, true);
      assert.equal(dom.nodes["temporary-sheet-pdf-frame"].src, "");
      assert.equal(preview.disabled, true, "bad digest must fence further exports");
      assert.match(dom.nodes["temporary-status"].textContent, /не подтверждён/);
    }
    controller.forget();
    assert.equal(dom.nodes["temporary-sheet-preview"].hidden, true);
    assert.equal(dom.nodes["temporary-sheet-pdf-frame"].src, "");
  }
});


test("selected page-owned shape exposes eight handles and one canonical pointer-up ResizeNode", async () => {
  const dom = fakeDom();
  const svg = dom.pointer(101,201).target.closest("svg.page");
  const drawn = [];
  svg.appendChild = element => drawn.push(element);
  dom.documentObject.createElementNS = () => {
    const attrs = {};
    return {
      attrs,
      setAttribute(key,value) {attrs[key]=value;},
      getAttribute(key) {return attrs[key] ?? null;},
      remove() {},
    };
  };
  const original = current(beforeRev,0,100,200);
  const resized = current(afterRev,1,100,200);
  resized.authoring_graph.nodes[nodeId].header.bounds = {
    x:100,y:200,width:400,height:400,
  };
  const scenes = [];
  const baseNetwork=server();
  let saved = false;
  const posts=[];
  const fetchImpl=async (url, options) => {
    if(url===base+"/current") return response(saved?resized:original);
    if(url===base+"/scene") {
      const scene=readerScene(saved?afterRev:beforeRev);
      if(saved) scene.nodes[0].bounds={x:100,y:200,width:400,height:400};
      return response({
        protocol_version:"chaptera.temporary-editor-scene.v1",
        document_id:id,source_hash:source,revision_id:saved?afterRev:beforeRev,
        expires_at_ms:saved?8_200_000:4_600_000,scene,
      });
    }
    if(url===base+"/resize-node") {
      const body=JSON.parse(options.body);
      posts.push({body,options});
      assert.deepEqual(body.command,{
        kind:"resize_node",node_id:nodeId,x_emu:100,y_emu:200,
        width_emu:400,height_emu:400,
      });
      assert.equal(options.headers["x-chaptera-reader-session"],token);
      saved=true;
      return response({
        protocol_version:"chaptera.temporary-editor-resize-node.v1",
        document_id:id,source_hash:source,base_revision_id:beforeRev,
        revision_id:afterRev,canonical_authoring_revision_id:afterCanon,
        client_operation_id:body.client_operation_id,expires_at_ms:8_200_000,
        replayed:false,scene_refresh:"current",
        canonical_operation:{
          kind:"resize_node",node_id:nodeId,
          before:{x:100,y:200,width:300,height:300},
          after:{x:100,y:200,width:400,height:400},
        },
      });
    }
    return baseNetwork.fetchImpl(url, options);
  };
  const controller=await bootTemporaryEditorPreview({
    documentObject:dom.documentObject,
    cryptoObject:{getRandomValues(bytes){bytes.fill(4);return bytes;}},
    storage:storage(),fetchImpl,renderer:async(_,scene)=>scenes.push(scene),
  });
  await controller.start({name:"example.pub",size:1729});
  const pages=dom.nodes["temporary-pages"];
  const selected=dom.pointer(120,230);
  pages.handlers.pointerdown(selected);
  await pages.handlers.pointerup(selected);
  const handles=drawn.filter(el=>el.attrs?.["data-temporary-resize-handle"]);
  assert.equal(handles.length,8);
  const corner=handles.find(el=>el.getAttribute("data-temporary-resize-handle")==="bottom-right");
  assert.ok(corner);
  const target={
    closest(selector) {
      if(selector.startsWith("svg.")) return svg;
      if(selector==="[data-temporary-resize-handle]"||selector==="[data-node-id]")return corner;
      return null;
    },
  };
  const resizeDown={...dom.pointer(400,500),target};
  const resizeUp={...dom.pointer(500,600),target};
  pages.handlers.pointerdown(resizeDown);
  pages.handlers.pointermove(resizeUp);
  await pages.handlers.pointerup(resizeUp);
  assert.equal(posts.length,1);
  assert.equal(scenes.at(-1).nodes[0].bounds.width,400);
  assert.equal(dom.nodes["temporary-download-idml"].disabled,false);
  assert.match(dom.nodes["temporary-status"].textContent,/Правка сохранена/);
});

test("focused canvas ArrowRight makes exactly one canonical 1pt MoveNode and repaints the committed Scene", async () => {
  const dom = fakeDom(), network = server(), renders = [];
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: {getRandomValues(bytes) {bytes.fill(7);return bytes;}},
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async (_, scene) => {renders.push(scene);},
  });
  await controller.start({name: "keyboard.pub", size: 1729});
  const pages = dom.nodes["temporary-pages"];
  dom.nodes["temporary-pages"].handlers.pointerdown(dom.pointer(110, 210));
  await dom.nodes["temporary-pages"].handlers.pointerup(dom.pointer(110, 210));
  let prevented = false;
  await pages.handlers.keydown({
    key: "ArrowRight", target: pages,
    preventDefault() {prevented = true;},
  });
  const moves = network.calls.filter(call => call.url === base + "/move-node");
  assert.equal(moves.length, 1);
  assert.equal(prevented, true);
  const intent = JSON.parse(moves[0].options.body);
  assert.equal(intent.command.kind, "move_node_to");
  assert.equal(intent.command.node_id, nodeId);
  assert.equal(intent.command.x_emu, 100 + 12_700);
  assert.equal(intent.command.y_emu, 200);
  assert.equal(intent.base_revision_id, beforeRev);
  assert.equal(moves[0].options.headers["x-chaptera-reader-session"], token);
  assert.equal(renders.at(-1).revision_id, afterRev);
  assert.equal(renders.at(-1).nodes[0].bounds.x, 12_800);
  assert.equal(dom.nodes["temporary-download-idml"].disabled, false);
  assert.match(dom.nodes["temporary-status"].textContent, /Объект перемещён/);
});

test("nudge cannot escape canvas focus, modifiers, composition, missing selection or Escape", async () => {
  const dom = fakeDom(), network = server();
  const controller = await bootTemporaryEditorPreview({
    documentObject: dom.documentObject,
    cryptoObject: {getRandomValues(bytes) {bytes.fill(7);return bytes;}},
    storage: storage(), fetchImpl: network.fetchImpl,
    renderer: async () => {},
  });
  await controller.start({name: "keyboard.pub", size: 1729});
  const pages = dom.nodes["temporary-pages"];
  let prevented = 0;
  const key = changes => pages.handlers.keydown({
    key: "ArrowLeft", target: pages,
    preventDefault() {prevented++;}, ...changes,
  });
  await key({});
  pages.handlers.pointerdown(dom.pointer(110, 210));
  await pages.handlers.pointerup(dom.pointer(110, 210));
  await key({ctrlKey: true});
  await key({altKey: true});
  await key({metaKey: true});
  await key({shiftKey: true});
  await key({isComposing: true});
  await key({target: {tagName: "TEXTAREA"}});
  await pages.handlers.keydown({key: "Escape", target: pages});
  await key({});
  assert.equal(network.calls.filter(call => call.url.endsWith("/move-node")).length, 0);
  assert.equal(prevented, 0, "unsupported keyboard routes must not consume Arrow");
});

test("canvas Story join refuses wrong-page, foreign, missing and unadmitted owners", () => {
  const storyId = "story-a";
  const graph = {
    pages: {[pageId]: {}, ["40000000-0000-7000-8000-000000000004"]: {}},
    nodes: {[nodeId]: {header: {parent_id: pageId},
      payload: {story_frame: {story_id: storyId}}}},
  };
  const candidates = [{story_id: storyId, text: "Hello"}];
  assert.equal(temporaryCanvasStoryId(graph, candidates, nodeId, pageId), storyId);
  assert.equal(temporaryCanvasStoryId(graph, candidates, nodeId,
    "40000000-0000-7000-8000-000000000004"), null);
  assert.equal(temporaryCanvasStoryId(graph, candidates, "foreign-node", pageId), null);
  assert.equal(temporaryCanvasStoryId(graph, [], nodeId, pageId), null,
    "unsupported linked/table/rich Stories are never client-admitted");
  assert.equal(temporaryCanvasStoryId({pages: {}, nodes: graph.nodes},
    candidates, nodeId, pageId), null);
  assert.equal(temporaryCanvasStoryId({pages: graph.pages, nodes: {
    [nodeId]: {header: {parent_id: pageId}, payload: {table: {story_id: storyId}}},
  }}, candidates, nodeId, pageId), null);
});
