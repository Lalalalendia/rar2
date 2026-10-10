import test from "node:test";
import assert from "node:assert/strict";
import { ChapteraCloudSourceIngressV1 } from "./chaptera-cloud-source-ingress-v1.mjs";
import { WebFileEntryControllerV1 } from "./file-entry-v1.mjs";

const WORKSPACE = "workspace:personal:" + "a".repeat(64);
const UPLOAD = "upload:" + "b".repeat(32);
const DOC = "document:" + "c".repeat(24);
const CSRF = "verified-session-csrf";
const FILE = { name: "Newsletter.pub", size: 16, type: "application/x-mspublisher" };

function json(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}
function upload(state = "ISSUED", generation = 0) {
  return {
    upload_id: UPLOAD, state, upload_generation: generation,
    expected_byte_len: FILE.size, terminal_code: null,
  };
}

function happyApi({ reject = false, issuedTransport = true } = {}) {
  const calls = [];
  let polls = 0;
  const fetchImpl = async (path, init) => {
    calls.push({ path, init, json: init.body && typeof init.body === "string" ? JSON.parse(init.body) : null });
    if (path === "/v1/session") return json({ principal_id: "principal:a", csrf_token: CSRF });
    if (path === "/v1/workspaces/personal") return json({ workspace_id: WORKSPACE, role: "owner" });
    if (path === "/v1/uploads" && init.method === "POST") {
      return json({
        upload: upload(),
        transport: issuedTransport
          ? { kind: "streamed", path: "/v1/uploads/" + UPLOAD + "/content" }
          : null,
      });
    }
    if (path === "/v1/uploads/upload%3A" + "b".repeat(32) + "/content" &&
        init.method === "PUT") return json(upload());
    if (path.endsWith("/complete") && init.method === "POST") {
      return json(upload("VALIDATING", 1));
    }
    if (path === "/v1/uploads/upload%3A" + "b".repeat(32) && init.method === "GET") {
      polls++;
      if (reject && polls >= 2) return json({ ...upload("REJECTED", 2), terminal_code: "unsupported_source" });
      return json(upload(polls === 1 ? "VALIDATING" : "VALIDATED_DURABLE", polls === 1 ? 1 : 2));
    }
    if (path === "/v1/projects/from-upload") {
      return json({
        project_id: "project:" + "d".repeat(24),
        document_id: DOC,
        genesis_revision_id: "revision:" + "e".repeat(24),
      });
    }
    throw new Error("unexpected HTTP call " + path + " " + init.method);
  };
  return { fetchImpl, calls };
}

test("real Rust SourceIngress wire contract drives existing file-entry controller to durable project", async () => {
  const { fetchImpl, calls } = happyApi();
  const source = new ChapteraCloudSourceIngressV1({
    fetchImpl,
    pollMs: 0,
    sleep: async () => {},
  });
  const opened = [];
  const controller = new WebFileEntryControllerV1({
    sourceIngress: source,
    requestIdFactory: () => "upload-request-0001",
    openProject: async (receipt) => opened.push(receipt),
  });
  const result = await controller.openFile(FILE);
  assert.equal(result.phase, "ready");
  assert.equal(result.bytes_sent, 16);
  assert.equal(result.document_id, DOC);
  assert.equal(opened[0].document_id, DOC);
  assert.equal(calls.filter(x => x.path === "/v1/session").length, 1);
  assert.equal(calls[0].init.credentials, "same-origin");
  assert.equal(calls[0].init.redirect, "error");
  const boot = calls.find(x => x.path === "/v1/workspaces/personal");
  assert.equal(boot.init.headers["x-csrf-token"], CSRF);
  const issue = calls.find(x => x.path === "/v1/uploads");
  assert.deepEqual(issue.json, {
    workspace_id: WORKSPACE, purpose: "pub_source",
    expected_byte_len: 16, declared_content_type: FILE.type,
    idempotency_key: "upload-request-0001",
  });
  assert.equal(issue.json.tenant_id, undefined);
  const put = calls.find(x => x.init.method === "PUT");
  assert.equal(put.init.body, FILE);
  assert.equal(put.init.headers["x-csrf-token"], CSRF);
  assert.equal(put.init.headers["content-length"], undefined, "browser sets forbidden Content-Length");
  const create = calls.find(x => x.path === "/v1/projects/from-upload");
  assert.deepEqual(create.json, {
    workspace_id: WORKSPACE, upload_id: UPLOAD, expected_upload_generation: 2,
    name: "Newsletter.pub", client_idempotency_id: "upload-request-0001",
  });
  assert.equal(create.init.headers["x-csrf-token"], CSRF);
});

test("missing session refuses workspace bootstrap and exposes exact HTTP 401", async () => {
  const source = new ChapteraCloudSourceIngressV1({
    fetchImpl: async (_path, init) => {
      assert.equal(init.credentials, "same-origin");
      return json({ error: "session_missing" }, 401);
    },
  });
  await assert.rejects(source.prepare(), (error) =>
    error.status === 401 && error.code === "session_missing");
  assert.equal(source.workspaceId, null);
});

test("server-side validation rejection never creates a project", async () => {
  const { fetchImpl, calls } = happyApi({ reject: true });
  const controller = new WebFileEntryControllerV1({
    sourceIngress: new ChapteraCloudSourceIngressV1({ fetchImpl, sleep: async () => {}, pollMs: 0 }),
    requestIdFactory: () => "upload-request-0002",
    openProject: async () => { throw new Error("must not open"); },
  });
  const result = await controller.openFile(FILE);
  assert.equal(result.phase, "error");
  assert.equal(result.error_code, "unsupported_source");
  assert.equal(result.retryable, false);
  assert.equal(calls.some(x => x.path === "/v1/projects/from-upload"), false);
});

test("unsigned unknown/direct transport never sends the PUB bytes anywhere", async () => {
  for (const badTransport of [
    { kind: "direct", grant: "https://example.invalid/foreign-upload" },
    { kind: "streamed", path: "https://example.invalid/foreign-upload" },
    { kind: "streamed", path: "/v1/uploads/other/content" },
  ]) {
    const paths = [];
    const source = new ChapteraCloudSourceIngressV1({
      fetchImpl: async (path) => {
        paths.push(path);
        if (path === "/v1/session") return json({ csrf_token: CSRF });
        if (path === "/v1/workspaces/personal") return json({ workspace_id: WORKSPACE });
        if (path === "/v1/uploads") return json({ upload: upload(), transport: badTransport });
        throw new Error("unauthorized upload path " + path);
      },
    });
    await assert.rejects(
      source.beginUpload({ client_request_id: "idempotent-a", file_name: FILE.name, byte_length: FILE.size }),
      (error) => error.retryable === false && [
        "direct_upload_not_supported", "upload_transport_invalid",
      ].includes(error.code),
    );
    assert.deepEqual(paths, ["/v1/session", "/v1/workspaces/personal", "/v1/uploads"]);
  }
});

test("replayed ISSUED upload without a new grant can recover stored bytes", async () => {
  const { fetchImpl, calls } = happyApi({ issuedTransport: false });
  const source = new ChapteraCloudSourceIngressV1({ fetchImpl, sleep: async () => {}, pollMs: 0 });
  await source.beginUpload({
    client_request_id: "idempotent-replay", file_name: FILE.name, byte_length: FILE.size,
  });
  await source.uploadBytes({ upload_id: UPLOAD, file: FILE });
  assert.equal(calls.filter(x => x.init.method === "PUT").length, 0);
  assert.equal(calls.filter(x => x.path.endsWith("/complete")).length, 1);
});

test("invalid project receipt is never followed as a browser URL", async () => {
  const { fetchImpl } = happyApi();
  const source = new ChapteraCloudSourceIngressV1({
    fetchImpl: async (path, init) => path === "/v1/projects/from-upload"
      ? json({ project_id: "project:okay", document_id: "//evil.invalid" })
      : fetchImpl(path, init),
    sleep: async () => {}, pollMs: 0,
  });
  await source.beginUpload({ client_request_id: "upload-123", file_name: FILE.name, byte_length: FILE.size });
  await source.uploadBytes({ upload_id: UPLOAD, file: FILE });
  await source.completeUpload({ upload_id: UPLOAD });
  await source.waitUntilValidated({ upload_id: UPLOAD });
  await assert.rejects(
    source.createProjectFromUpload({ upload_id: UPLOAD, client_request_id: "upload-123" }),
    /invalid document id/,
  );
});


test("lost CreateProject ACK reconciles from original validated generation on retry", async () => {
  const original = happyApi();
  let committed = false;
  const projectBodies = [];
  const fetchImpl = async (path, init) => {
    if (path === "/v1/uploads" && committed) {
      return json({ upload: upload("CONSUMED", 3), transport: null });
    }
    if (path.endsWith("/complete") && committed) return json(upload("CONSUMED", 3));
    if (path === "/v1/uploads/upload%3A" + "b".repeat(32) &&
        init.method === "GET" && committed) return json(upload("CONSUMED", 3));
    if (path === "/v1/projects/from-upload") {
      const body = JSON.parse(init.body);
      projectBodies.push(body);
      if (!committed) {
        committed = true; // durable server commit occurred; client lost response
        throw new Error("connection reset after durable commit");
      }
      return json({ project_id: "project:" + "d".repeat(24), document_id: DOC });
    }
    return original.fetchImpl(path, init);
  };
  const controller = new WebFileEntryControllerV1({
    sourceIngress: new ChapteraCloudSourceIngressV1({ fetchImpl, pollMs: 0, sleep: async () => {} }),
    requestIdFactory: () => "request-lost-project-ack",
    openProject: async () => {},
  });
  const first = await controller.openFile(FILE);
  assert.equal(first.phase, "error");
  assert.equal(first.error_code, "network_error");
  assert.equal(first.retryable, true);
  const retry = await controller.retry();
  assert.equal(retry.phase, "ready");
  assert.equal(retry.document_id, DOC);
  assert.deepEqual(projectBodies.map(x => x.expected_upload_generation), [2, 2]);
  assert.deepEqual(projectBodies.map(x => x.client_idempotency_id), [
    "request-lost-project-ack", "request-lost-project-ack",
  ]);
  assert.equal(original.calls.filter(x => x.init.method === "PUT").length, 1);
});

test("already consumed source without known validated generation fails closed", async () => {
  let creationRequests = 0;
  const source = new ChapteraCloudSourceIngressV1({
    fetchImpl: async (path) => {
      if (path === "/v1/session") return json({ csrf_token: CSRF });
      if (path === "/v1/workspaces/personal") return json({ workspace_id: WORKSPACE });
      if (path === "/v1/uploads") return json({ upload: upload("CONSUMED", 4), transport: null });
      if (path.endsWith("/complete")) return json(upload("CONSUMED", 4));
      if (path === "/v1/uploads/upload%3A" + "b".repeat(32)) return json(upload("CONSUMED", 4));
      if (path === "/v1/projects/from-upload") creationRequests++;
      throw new Error("unexpected request");
    },
  });
  const controller = new WebFileEntryControllerV1({
    sourceIngress: source,
    requestIdFactory: () => "request-durable-lost",
    openProject: async () => {},
  });
  const result = await controller.openFile(FILE);
  assert.equal(result.phase, "error");
  assert.equal(result.error_code, "source_already_consumed");
  assert.equal(result.retryable, false);
  assert.equal(creationRequests, 0);
});

test("failed editor opening retries the accepted project without creating it again", async () => {
  const original = happyApi();
  let committed = false;
  let creates = 0;
  let opens = 0;
  const fetchImpl = async (path, init) => {
    if (path === "/v1/uploads" && committed) {
      return json({ upload: upload("CONSUMED", 3), transport: null });
    }
    if (path.endsWith("/complete") && committed) return json(upload("CONSUMED", 3));
    if (path === "/v1/uploads/upload%3A" + "b".repeat(32) &&
        init.method === "GET" && committed) return json(upload("CONSUMED", 3));
    if (path === "/v1/projects/from-upload") { creates++; committed = true; }
    return original.fetchImpl(path, init);
  };
  const source = new ChapteraCloudSourceIngressV1({ fetchImpl, pollMs: 0, sleep: async () => {} });
  const controller = new WebFileEntryControllerV1({
    sourceIngress: source,
    requestIdFactory: () => "request-retry-project-open",
    openProject: async value => {
      assert.equal(value.document_id, DOC);
      if (++opens === 1) {
        throw Object.assign(new Error("editor navigation failed"), { retryable: true });
      }
    },
  });
  assert.equal((await controller.openFile(FILE)).phase, "error");
  assert.equal((await controller.retry()).phase, "ready");
  assert.equal(creates, 1, "a known project receipt must not be created again");
  assert.equal(original.calls.filter(x => x.init.method === "PUT").length, 1);
  assert.equal(opens, 2);
});
