import test from "node:test";
import assert from "node:assert/strict";
import {
  bootTemporaryEditorPreview,
  describeTemporaryPreview,
  temporaryExpiryText,
} from "./temporary-editor-preview.mjs";

const id = "tempedit:" + "a".repeat(32);
const bearer = "b".repeat(64);
const sha = "c".repeat(64);
const path = "/v1/reader/guest-sessions/" + id + "/";
const scene = { protocol_version: "chaptera.reader-scene.v1", pages: [] };

function response(value, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
}
function fakeDom() {
  const nodes = {};
  for (const id of [
    "temporary-preview-root", "temporary-upload-form", "temporary-file",
    "temporary-start", "temporary-resume", "temporary-forget",
    "temporary-status", "temporary-expiry", "temporary-pages",
  ]) {
    nodes[id] = {
      textContent: "", hidden: false, disabled: false, clientWidth: 700,
      value: "", files: [], handlers: {},
      addEventListener(type, handler) { this.handlers[type] = handler; },
      replaceChildren() { this.childrenCleared = true; },
    };
  }
  return { nodes, documentObject: { getElementById: (id) => nodes[id] ?? null } };
}
function memoryStorage() {
  const m = new Map();
  return {
    getItem: (key) => m.get(key) ?? null,
    setItem: (key, val) => m.set(key, val),
    removeItem: (key) => m.delete(key),
  };
}
const cryptoObject = {
  getRandomValues(bytes) { bytes.fill(0xaa); return bytes; },
};
const file = { name: "example.PUB", size: 1729 };
const issue = {
  protocol_version: "chaptera.reader-guest-session.v1", mode: "temporary_edit",
  session_id: id, access_token: bearer, expires_at_ms: 1_000_000,
  max_file_bytes: 8192, upload_path: path + "content",
  open_path: path + "open", scene_path: path + "scene",
};
const stored = {
  protocol_version: issue.protocol_version, session_id: id,
  expires_at_ms: 4_600_000, state: "stored",
};
const opened = {
  protocol_version: issue.protocol_version, session_id: id,
  expires_at_ms: 4_600_000, classification: "supported",
  source_sha256: sha, scene,
  can_edit: false, can_download_changed_file: false,
};

test("temporary preview explicitly forbids edit and changed-file downloads", () => {
  const described = describeTemporaryPreview({ kind: "temporary_preview", ...opened });
  assert.equal(described.renderable, true);
  assert.match(described.message, /Редактирование пока недоступно/);
  assert.equal(describeTemporaryPreview({
    kind: "temporary_preview", ...opened, classification: "partial",
  }).renderable, true);
  assert.equal(describeTemporaryPreview({
    kind: "temporary_preview", ...opened, classification: "salvage",
  }).renderable, false);
  assert.throws(() => describeTemporaryPreview({
    kind: "temporary_preview", ...opened, can_edit: true,
  }), /must not claim edit/);
  assert.throws(() => describeTemporaryPreview({
    kind: "temporary_preview", ...opened, can_download_changed_file: true,
  }), /must not claim edit/);
});

test("only valid server timestamp is displayed; no client-side TTL renewal", () => {
  assert.equal(temporaryExpiryText(NaN), "");
  assert.equal(temporaryExpiryText(-1), "");
  assert.match(temporaryExpiryText(4_600_000), /Обновление страницы не продлевает/);
});

test("real temporary browser client drives same-origin upload and uses existing reader renderer", async () => {
  const { nodes, documentObject } = fakeDom();
  const requests = [], renders = [], storage = memoryStorage();
  const replies = [issue, stored, opened];
  const fetchImpl = async (url, options) => {
    requests.push({ url, options });
    return response(replies[requests.length - 1]);
  };
  const controller = await bootTemporaryEditorPreview({
    documentObject, cryptoObject, storage, fetchImpl,
    renderer: async (host, payload) => { renders.push({ host, payload }); },
  });
  await controller.start(file);
  assert.deepEqual(requests.map(({ url, options }) => [url, options.method]), [
    ["/v1/editor/temporary-sessions", "POST"],
    [path + "content", "PUT"],
    [path + "open", "POST"],
  ]);
  for (const { url, options } of requests) {
    assert.equal(options.credentials, "omit");
    assert.equal(options.redirect, "error");
    assert.equal(options.cache, "no-store");
    assert.ok(!url.includes(bearer));
    assert.match(options.headers["x-csrf-token"], /^[0-9a-f]{32}$/);
  }
  assert.equal(renders.length, 1);
  assert.equal(renders[0].host, nodes["temporary-pages"]);
  assert.equal(renders[0].payload, scene);
  assert.match(nodes["temporary-status"].textContent, /Редактирование пока недоступно/);
  assert.equal(nodes["temporary-resume"].hidden, false);
  assert.equal(nodes["temporary-forget"].hidden, false);
  controller.forget();
  assert.equal(nodes["temporary-resume"].hidden, true);
  assert.equal(nodes["temporary-forget"].hidden, true);
  assert.deepEqual(requests.map(({url}) => url), [
    "/v1/editor/temporary-sessions", path + "content", path + "open",
  ], "forget must never attempt a pretend server deletion");
});

test("invalid extension and untrusted server errors do not reflect file contents", async () => {
  const { nodes, documentObject } = fakeDom();
  let requests = 0;
  const controller = await bootTemporaryEditorPreview({
    documentObject, cryptoObject,
    fetchImpl: async () => {
      requests += 1;
      return response({ error: "sensitive-private-file-text-would-leak" }, 500);
    },
  });
  await controller.start({ name: "test.txt", size: 6 });
  assert.equal(requests, 0);
  assert.match(nodes["temporary-status"].textContent, /расширением .pub/);
  await controller.start(file);
  assert.equal(requests, 1);
  assert.doesNotMatch(nodes["temporary-status"].textContent, /sensitive-private/);
  assert.match(nodes["temporary-status"].textContent, /temporary_request_failed/);
});

test("resume reads a stored Scene without any session creation or edit operation", async () => {
  const { documentObject, nodes } = fakeDom();
  const storage = memoryStorage();
  const calls = [], replies = [issue, stored, opened];
  let fetchImpl = async (url, options) => {
    calls.push([url, options.method]);
    return response(replies[calls.length - 1]);
  };
  const controller = await bootTemporaryEditorPreview({
    documentObject, cryptoObject, storage, fetchImpl,
    renderer: async () => {},
  });
  await controller.start(file);
  assert.equal(calls.length, 3);
  const secondDom = fakeDom();
  const resumedCalls = [];
  fetchImpl = async (url, options) => {
    resumedCalls.push([url, options.method]);
    return response(opened);
  };
  const secondController = await bootTemporaryEditorPreview({
    documentObject: secondDom.documentObject, cryptoObject, storage, fetchImpl,
    renderer: async () => {},
  });
  await secondController.resume();
  assert.deepEqual(resumedCalls, [[path + "scene", "GET"]]);
  assert.match(secondDom.nodes["temporary-status"].textContent, /PUB открыт/);
});
