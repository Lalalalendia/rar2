import test from "node:test";
import assert from "node:assert/strict";
import {
  TemporaryEditBrowserSessionV0,
  TEMPORARY_EDIT_STORAGE_KEY,
} from "./temporary-edit-browser-session-v0.mjs";

const id = "tempedit:" + "a".repeat(32);
const token = "b".repeat(64);
const sha = "c".repeat(64);
const csrfToken = "d".repeat(32);
const path = "/v1/reader/guest-sessions/" + id + "/";
const file = { size: 1729, type: "application/octet-stream" };
const scene = { protocol_version: "chaptera.reader-scene.v1", pages: [] };
const issue = {
  protocol_version: "chaptera.reader-guest-session.v1",
  mode: "temporary_edit",
  session_id: id,
  access_token: token,
  expires_at_ms: 1_000_000,
  max_file_bytes: 8192,
  upload_path: path + "content",
  open_path: path + "open",
  scene_path: path + "scene",
};
const stored = {
  protocol_version: issue.protocol_version, session_id: id,
  expires_at_ms: 4_600_000, state: "stored",
};
const opened = {
  protocol_version: issue.protocol_version, session_id: id,
  expires_at_ms: 4_600_000, classification: "supported",
  source_sha256: sha, scene,
};
const response = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status, headers: { "content-type": "application/json" },
  });

function storage() {
  const map = new Map();
  return {
    getItem: (name) => map.get(name) ?? null,
    setItem: (name, value) => map.set(name, value),
    removeItem: (name) => map.delete(name),
  };
}

function flow({ issued = issue, uploaded = stored, open = opened } = {}) {
  const calls = [];
  const fetchImpl = async (url, opts) => {
    calls.push({ url, opts });
    return [response(issued), response(uploaded), response(open)][calls.length - 1];
  };
  return { calls, fetchImpl };
}

test("temporary upload uses only same-origin paths, private header and no credentials", async () => {
  const store = storage();
  const { fetchImpl, calls } = flow();
  const client = new TemporaryEditBrowserSessionV0({ fetchImpl, csrfToken, storage: store });
  const result = await client.start(file);
  assert.equal(result.kind, "temporary_preview");
  assert.equal(result.can_edit, false);
  assert.equal(result.can_download_changed_file, false);
  assert.equal(result.expires_at_ms, 4_600_000);
  assert.equal(result.source_sha256, sha);
  assert.equal(result.restorable, true);
  assert.deepEqual(calls.map((x) => [x.url, x.opts.method]), [
    ["/v1/editor/temporary-sessions", "POST"],
    [path + "content", "PUT"],
    [path + "open", "POST"],
  ]);
  assert.deepEqual(calls[0].opts.body, JSON.stringify({ expected_byte_len: file.size }));
  assert.equal(calls[1].opts.body, file);
  for (const { url, opts } of calls) {
    assert.ok(!url.includes(token));
    assert.equal(opts.credentials, "omit");
    assert.equal(opts.redirect, "error");
    assert.equal(opts.cache, "no-store");
    assert.equal(opts.referrerPolicy, "no-referrer");
    assert.equal(opts.headers["x-csrf-token"], csrfToken);
  }
  assert.equal(calls[0].opts.headers["x-chaptera-reader-session"], undefined);
  assert.equal(calls[1].opts.headers["x-chaptera-reader-session"], token);
  assert.equal(calls[2].opts.headers["x-chaptera-reader-session"], token);
  const record = JSON.parse(store.getItem(TEMPORARY_EDIT_STORAGE_KEY));
  assert.equal(record.access_token, token);
  assert.equal(record.source_sha256, sha);
  assert.ok(!store.getItem(TEMPORARY_EDIT_STORAGE_KEY).includes(file.type));
});

test("reload resumes same temporary Scene without session creation or TTL renewal", async () => {
  const store = storage();
  const initial = flow();
  await new TemporaryEditBrowserSessionV0({
    fetchImpl: initial.fetchImpl, storage: store, csrfToken,
  }).start(file);
  const calls = [];
  const client = new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken,
    fetchImpl: async (url, opts) => {
      calls.push({ url, opts });
      return response(opened);
    },
  });
  assert.equal(client.hasSavedSession(), true);
  const result = await client.resume();
  assert.equal(result.kind, "temporary_preview");
  assert.equal(result.expires_at_ms, stored.expires_at_ms);
  assert.deepEqual(calls.map((c) => [c.url, c.opts.method]), [[path + "scene", "GET"]]);
  assert.equal(calls[0].opts.headers["x-chaptera-reader-session"], token);
  assert.equal(calls[0].opts.body, undefined);
});

test("no storage permits current-tab view but reports resume unavailable", async () => {
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, fetchImpl: flow().fetchImpl,
  });
  const result = await client.start(file);
  assert.equal(result.restorable, false);
});

test("interrupted open after accepted upload resumes only that server handshake", async () => {
  const store = storage();
  await assert.rejects(() => new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken,
    fetchImpl: flow({ open: { error: "guest_scene_worker_failed" } }).fetchImpl,
  }).start(file), /temporary_response_identity_mismatch/);
  const calls = [];
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store,
    fetchImpl: async (url, opts) => {
      calls.push({ url, opts });
      if (calls.length === 1) {
        return response({ error: "guest_scene_not_ready" }, 409);
      }
      return response(opened);
    },
  });
  const result = await client.resume();
  assert.equal(result.kind, "temporary_preview");
  assert.equal(result.expires_at_ms, stored.expires_at_ms);
  assert.deepEqual(calls.map(({url, opts}) => [url, opts.method]), [
    [path + "scene", "GET"],
    [path + "open", "POST"],
  ]);
  assert.equal(calls[1].opts.body, "{}");
  assert.equal(calls[1].opts.headers["x-chaptera-reader-session"], token);
});

test("unrelated 409 never authorizes a second open", async () => {
  const store = storage();
  await new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store, fetchImpl: flow().fetchImpl,
  }).start(file);
  const calls = [];
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store,
    fetchImpl: async (url) => {
      calls.push(url);
      return response({ error: "guest_upload_state_conflict" }, 409);
    },
  });
  await assert.rejects(() => client.resume(), /guest_upload_state_conflict/);
  assert.deepEqual(calls, [path + "scene"]);
  assert.equal(client.hasSavedSession(), true);
});

test("expired capability is blocked by server 410 and discarded", async () => {
  const store = storage();
  await new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken, fetchImpl: flow().fetchImpl,
  }).start(file);
  const client = new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken,
    fetchImpl: async () => response({ error: "guest_session_expired" }, 410),
  });
  await assert.rejects(() => client.resume(), (error) =>
    error.status === 410 && error.code === "guest_session_expired");
  assert.equal(store.getItem(TEMPORARY_EDIT_STORAGE_KEY), null);
});

test("source swap after refresh fails closed, invalidates private token", async () => {
  const store = storage();
  await new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken, fetchImpl: flow().fetchImpl,
  }).start(file);
  const client = new TemporaryEditBrowserSessionV0({
    storage: store, csrfToken,
    fetchImpl: async () => response({ ...opened, source_sha256: "f".repeat(64) }),
  });
  await assert.rejects(() => client.resume(), /temporary_source_changed/);
  assert.equal(store.getItem(TEMPORARY_EDIT_STORAGE_KEY), null);
});

test("malicious absolute, query or foreign session paths never receive a PUT", async () => {
  const bad = [
    { ...issue, upload_path: "https://attacker.invalid/exfil" },
    { ...issue, upload_path: path + "content?token=" + token },
    { ...issue, scene_path: path.replace(id, "tempedit:" + "f".repeat(32)) + "scene" },
    { ...issue, mode: "reader" },
    { ...issue, access_token: "secret" },
  ];
  for (const issued of bad) {
    const calls = [];
    const client = new TemporaryEditBrowserSessionV0({
      csrfToken, storage: storage(),
      fetchImpl: async (url, opts) => {
        calls.push({ url, opts });
        return response(issued);
      },
    });
    await assert.rejects(() => client.start(file), /temporary_issue_contract_invalid/);
    assert.equal(calls.length, 1);
  }
});

test("wrong session identity in upload response cannot enter open", async () => {
  const { fetchImpl, calls } = flow({
    uploaded: { ...stored, session_id: "tempedit:" + "f".repeat(32) },
  });
  const store = storage();
  await assert.rejects(
    () => new TemporaryEditBrowserSessionV0({ fetchImpl, csrfToken, storage: store }).start(file),
    /temporary_response_identity_mismatch/,
  );
  assert.equal(calls.length, 2);
  assert.equal(store.getItem(TEMPORARY_EDIT_STORAGE_KEY), null);
});

test("invalid or missing input refuses without creating a session", async () => {
  let requests = 0;
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, fetchImpl: async () => { requests++; throw new Error("must not reach"); },
  });
  await assert.rejects(() => client.start({ size: 0 }), /temporary_file_invalid/);
  await assert.rejects(() => client.start({ size: Number.MAX_SAFE_INTEGER + 1 }), /temporary_file_invalid/);
  assert.equal(requests, 0);
});

test("invalid saved identity is cleared without a network request", async () => {
  const store = storage();
  store.setItem(TEMPORARY_EDIT_STORAGE_KEY, JSON.stringify({
    schema: "chaptera.temporary-edit.browser.v0",
    session_id: "tempedit:../evil", access_token: token, expires_at_ms: 123, source_sha256: null,
  }));
  let count = 0;
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store,
    fetchImpl: async () => { count++; throw Error("unreachable"); },
  });
  assert.deepEqual(await client.resume(), { kind: "missing" });
  assert.equal(count, 0);
  assert.equal(store.getItem(TEMPORARY_EDIT_STORAGE_KEY), null);
});

test("failed open after accepted upload retains capability for recovery", async () => {
  const store = storage();
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store, fetchImpl: flow({
      open: { error: "guest_scene_worker_failed" },
    }).fetchImpl,
  });
  await assert.rejects(() => client.start(file), /temporary_response_identity_mismatch/);
  assert.equal(client.hasSavedSession(), true);
  assert.equal(JSON.parse(store.getItem(TEMPORARY_EDIT_STORAGE_KEY)).expires_at_ms, stored.expires_at_ms);
});

test("server error text cannot echo arbitrary secrets", async () => {
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, fetchImpl: async () => response({ error: token }, 500),
  });
  await assert.rejects(() => client.start(file), (error) =>
    error.code === "temporary_request_failed" && !error.message.includes(token));
});

test("one in-flight operation blocks another write and restore", async () => {
  let resolveIssue;
  const deferred = new Promise((resolve) => { resolveIssue = resolve; });
  const calls = [];
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, fetchImpl: async (url) => {
      calls.push(url);
      if (calls.length === 1) return deferred;
      return response(calls.length === 2 ? stored : opened);
    },
  });
  const first = client.start(file);
  await assert.rejects(() => client.start(file), /temporary_operation_pending/);
  await assert.rejects(() => client.resume(), /temporary_operation_pending/);
  resolveIssue(response(issue));
  await first;
  assert.equal(calls.length, 3);
});

test("delete uses only bearer headers and clears capability after physical 204 receipt", async () => {
  const store = storage();
  await new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store, fetchImpl: flow().fetchImpl,
  }).start(file);
  const calls = [];
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store, fetchImpl: async (url, opts) => {
      calls.push({url, opts});
      return new Response(null, {status: 204, headers: {"cache-control": "no-store"}});
    },
  });
  assert.equal(await client.deleteSession(), true);
  assert.equal(store.getItem(TEMPORARY_EDIT_STORAGE_KEY), null);
  assert.equal(client.hasSavedSession(), false);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "/v1/editor/temporary-sessions/" + id);
  assert.equal(calls[0].opts.method, "DELETE");
  assert.equal(calls[0].opts.headers["x-chaptera-reader-session"], token);
  assert.equal(calls[0].opts.headers["x-csrf-token"], csrfToken);
  assert.equal(calls[0].opts.cache, "no-store");
  assert.equal(calls[0].opts.credentials, "omit");
  assert.equal(calls[0].opts.redirect, "error");
  assert.ok(!calls[0].url.includes(token));
});

test("unconfirmed delete keeps the private capability for recovery, never claims purge", async () => {
  const store = storage();
  await new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store, fetchImpl: flow().fetchImpl,
  }).start(file);
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage: store,
    fetchImpl: async () => new Response(JSON.stringify({error: token}), {status: 500}),
  });
  await assert.rejects(client.deleteSession(), e =>
    e.code === "temporary_delete_not_confirmed" && e.status === 500 && !e.message.includes(token));
  assert.equal(client.hasSavedSession(), true);
  assert.equal(JSON.parse(store.getItem(TEMPORARY_EDIT_STORAGE_KEY)).access_token, token);
});

test("constructor rejects malformed CSRF token", () => {
  assert.throws(() => new TemporaryEditBrowserSessionV0({
    csrfToken: "bad", fetchImpl: async () => response(issue),
  }), /temporary_client_config_invalid/);
});
