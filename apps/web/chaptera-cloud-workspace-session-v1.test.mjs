import test from "node:test";
import assert from "node:assert/strict";
import { ChapteraCloudWorkspaceSessionV1 } from "./chaptera-cloud-workspace-session-v1.mjs";

const CSRF = "csrf-session-v1";
const WORKSPACE = "workspace:personal:" + "a".repeat(64);

function json(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

test("prepare owns one same-origin session and personal workspace bootstrap", async () => {
  const calls = [];
  const client = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async (path, init) => {
      calls.push({ path, init });
      if (path === "/v1/session") return json({ csrf_token: CSRF });
      if (path === "/v1/workspaces/personal") {
        assert.equal(init.method, "POST");
        assert.equal(init.credentials, "same-origin");
        assert.equal(init.redirect, "error");
        assert.equal(init.headers["x-csrf-token"], CSRF);
        return json({ workspace_id: WORKSPACE, role: "owner" });
      }
      throw new Error("unexpected " + path);
    },
  });

  assert.equal(await client.prepare(), WORKSPACE);
  assert.equal(await client.prepare(), WORKSPACE);
  assert.equal(client.workspaceId, WORKSPACE);
  assert.equal(client.csrfToken, CSRF);
  assert.equal(calls.filter(x => x.path === "/v1/session").length, 1);
  assert.equal(calls.filter(x => x.path === "/v1/workspaces/personal").length, 1);
});

test("bounded request refuses off-origin or query-bearing paths", async () => {
  const client = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async () => { throw new Error("must not fetch"); },
  });
  for (const path of [
    "https://evil.invalid/v1/session",
    "//evil.invalid/v1/session",
    "/v1/session?x=1",
    "/v1/session#x",
    "/other",
  ]) {
    await assert.rejects(() => client.request(path), /bounded same-origin/);
  }
});

test("mutation requires CSRF while authenticated GET does not invent one", async () => {
  const calls = [];
  const client = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async (path, init) => {
      calls.push({ path, init });
      return json({ ok: true });
    },
  });
  await client.request("/v1/read");
  assert.equal(calls[0].init.headers["x-csrf-token"], undefined);
  await assert.rejects(
    () => client.request("/v1/write", { method: "POST", json: {} }),
    error => error.code === "session_csrf_missing",
  );
});

test("HTTP and network failures retain bounded retry semantics", async () => {
  const unauthorized = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async () => json({ error: "session_missing" }, 401),
  });
  await assert.rejects(
    () => unauthorized.prepare(),
    error => error.code === "session_missing" && error.status === 401 && error.retryable === false,
  );
  assert.equal(unauthorized.workspaceId, null);

  const unavailable = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async () => json({ error: "busy" }, 503),
  });
  await assert.rejects(
    () => unavailable.request("/v1/read"),
    error => error.code === "busy" && error.status === 503 && error.retryable === true,
  );

  const network = new ChapteraCloudWorkspaceSessionV1({
    fetchImpl: async () => { throw new Error("socket"); },
  });
  await assert.rejects(
    () => network.request("/v1/read"),
    error => error.code === "network_error" && error.retryable === true,
  );
});
