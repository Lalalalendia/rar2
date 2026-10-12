import test from "node:test";
import assert from "node:assert/strict";
import {
  canonicalProjectRenameName, buildProjectRenameIntentV1, submitProjectRenameV1,
} from "./cloud-project-rename-v1.mjs";

const PROJECT = Object.freeze({
  project_id: "project:" + "a".repeat(24),
  lifecycle_state: "active",
  lifecycle_generation: 2,
  metadata_version: 7,
});
const REQUEST_ID = "rename-request-0001";

function fakeSession({ reply = null, failure = null } = {}) {
  const calls = [];
  return {
    calls,
    async request(path, options) {
      calls.push({ path, options });
      if (failure) throw failure;
      return reply ?? {
        protocol_version: "chaptera.project-rename-receipt.v1",
        receipt: {
          project_id: PROJECT.project_id,
          lifecycle_generation: 2,
          metadata_version: 8,
          name: "Новый выпуск",
          replayed: false,
        },
      };
    },
  };
}

test("only sends exact project/version/name/idempotency; no tenant or workspace", async () => {
  const session = fakeSession();
  const result = await submitProjectRenameV1({
    workspaceSession: session, project: PROJECT,
    name: " Новый выпуск ", clientRequestId: REQUEST_ID,
  });
  assert.equal(result.name, "Новый выпуск");
  assert.deepEqual(session.calls, [{
    path: "/v1/projects/" + encodeURIComponent(PROJECT.project_id) + "/rename",
    options: {
      method: "POST",
      json: {
        protocol_version: "chaptera.project-rename.v1",
        expected_lifecycle_generation: 2,
        expected_metadata_version: 7,
        name: "Новый выпуск",
        client_request_id: REQUEST_ID,
      },
    },
  }]);
  assert.equal("tenant_id" in session.calls[0].options.json, false);
  assert.equal("workspace_id" in session.calls[0].options.json, false);
});

test("lost ACK retry reuses the exact request id and does not invent a new version", async () => {
  const transient = Object.assign(new Error("network dropped after commit"), {
    code: "network_error", retryable: true,
  });
  const session = fakeSession();
  let first = true;
  const request = session.request;
  session.request = async (path, options) => {
    if (first) {
      first = false;
      session.calls.push({ path, options });
      throw transient;
    }
    return request.call(session, path, options);
  };
  await assert.rejects(() => submitProjectRenameV1({
    workspaceSession: session, project: PROJECT,
    name: "Новый выпуск", clientRequestId: REQUEST_ID,
  }), error => error === transient);
  const receipt = await submitProjectRenameV1({
    workspaceSession: session, project: PROJECT,
    name: "Новый выпуск", clientRequestId: REQUEST_ID,
  });
  assert.equal(receipt.metadata_version, 8);
  assert.deepEqual(
    session.calls.map(x => x.options.json.client_request_id),
    [REQUEST_ID, REQUEST_ID],
  );
  assert.deepEqual(
    session.calls.map(x => x.options.json.expected_metadata_version),
    [7, 7],
  );
});

test("accepts only source-bound exact canonical receipts, never fabricated local success", async () => {
  for (const changes of [
    { metadata_version: 9 }, { lifecycle_generation: 3 },
    { name: "Changed elsewhere" }, { project_id: "project:" + "b".repeat(24) },
    { replayed: "true" },
  ]) {
    const session = fakeSession({
      reply: {
        protocol_version: "chaptera.project-rename-receipt.v1",
        receipt: {
          project_id: PROJECT.project_id,
          lifecycle_generation: 2,
          metadata_version: 8,
          name: "Новый выпуск",
          replayed: false,
          ...changes,
        },
      },
    });
    await assert.rejects(() => submitProjectRenameV1({
      workspaceSession: session, project: PROJECT,
      name: "Новый выпуск", clientRequestId: REQUEST_ID,
    }), error => error.code === "project_rename_receipt_invalid");
  }
});

test("rejects path injection, invalid generations, controls and oversized UTF-8", () => {
  for (const project of [
    { ...PROJECT, project_id: "../../foreign" },
    { ...PROJECT, lifecycle_state: "trashed" },
    { ...PROJECT, metadata_version: Number.MAX_SAFE_INTEGER + 1 },
    { ...PROJECT, lifecycle_generation: -1 },
  ]) {
    assert.throws(() => buildProjectRenameIntentV1(project, "safe", REQUEST_ID));
  }
  for (const invalid of [" ", "x\nY", "\u0000", "界".repeat(180)]) {
    assert.throws(() => canonicalProjectRenameName(invalid));
  }
});

test("authorization denial and stale 409 propagate; no local revision/history mutation", async () => {
  for (const status of [403, 409]) {
    const original = Object.assign(new Error("not authorized or stale"), {
      status, code: status === 403 ? "capability_denied" : "stale_project_metadata_version",
      retryable: false,
    });
    const session = fakeSession({ failure: original });
    await assert.rejects(() => submitProjectRenameV1({
      workspaceSession: session, project: PROJECT,
      name: "Новый выпуск", clientRequestId: REQUEST_ID,
    }), error => error === original);
  }
});
