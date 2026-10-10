import test from "node:test";
import assert from "node:assert/strict";

import { ChapteraProductEditorServiceV1 } from "./chaptera-product-editor-service-v1.mjs";
import { BrowserObservabilityV1 } from "./observability-v1.mjs";

const DOC = "10000000-0000-4000-8000-000000000001";
const PAGE = "20000000-0000-4000-8000-000000000001";
const NODE = "30000000-0000-4000-8000-000000000001";
const SOURCE = "a".repeat(64);
const BASE = "sha256:" + "b".repeat(64);
const CHILD = "sha256:" + "c".repeat(64);
const LAYOUT = "sha256:" + "d".repeat(64);

function json(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function session(csrf = "csrf-token-1") {
  return {
    principal_id: "principal:test",
    csrf_token: csrf,
    idle_expires_at_ms: Date.now() + 60_000,
    absolute_expires_at_ms: Date.now() + 600_000,
  };
}

function current(revisionId = BASE, x = 1000) {
  return {
    protocol_version: "chaptera.current-document.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: revisionId,
    revision_cursor: revisionId === BASE ? 0 : 1,
    canonical_revision_schema_version: "chaptera.cdm.authoring-revision.v1",
    canonical_authoring_revision_id: "e".repeat(64),
    project: {
      schema_version: "pub-editor-v0.11",
      source_hash: SOURCE,
      operations: [],
    },
    authoring_graph: {
      cdm_version: "chaptera-cdm-v0.1",
      resolver_version: "pub-resolver-v1",
      source: { source_hash: SOURCE },
      document: {
        id: DOC,
        format_origin: "pub",
        source_hash: SOURCE,
        pages: [PAGE],
        resources: [],
        styles: [],
      },
      pages: {
        [PAGE]: {
          id: PAGE,
          size: { width: 10_058_400, height: 7_772_400 },
          bleed: null,
          margins: null,
          children: [NODE],
        },
      },
      nodes: {
        [NODE]: {
          kind: "shape",
          header: {
            id: NODE,
            parent_id: PAGE,
            bounds: { x, y: 2000, width: 3000, height: 4000 },
            transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
          },
          payload: {
            contents_seq_num: 1,
            officeart_shape_type: null,
            officeart_spid: null,
            image_slot: null,
            explicit_paint: {},
            story_frame: null,
            table: null,
          },
        },
      },
      stories: {},
      paragraphs: {},
      text_runs: {},
      resources: {},
      styles: {},
      extensions: {},
    },
  };
}


function readerScene(revisionId = BASE, x = 1000) {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: revisionId,
    scene_authority: "viewer-geometry-current-revision",
    stacking_fidelity: "exact",
    fidelity: { state: "supported", reasons: [] },
    pages: [{ page_id: PAGE, order: 0, width_emu: 10_058_400, height_emu: 7_772_400 }],
    nodes: [{
      node_id: NODE,
      page_id: PAGE,
      kind: "shape",
      bounds: { x, y: 2000, width: 3000, height: 4000 },
      transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      paint: { fill_rgb: [32, 96, 192] },
    }],
    stories: [],
    resources: [],
    fonts: [],
    diagnostics: [],
  };
}

function accepted() {
  return {
    protocol_version: "chaptera.commit-accepted.v1",
    document_id: DOC,
    source_hash: SOURCE,
    base_revision_id: BASE,
    revision_id: CHILD,
    state_id: "sha256:" + "f".repeat(64),
    client_operation_id: "move-1",
    canonical_operation: {
      kind: "move_node",
      node_id: NODE,
      before: { x: 1000, y: 2000, width: 3000, height: 4000 },
      after: { x: 9525, y: 10525, width: 3000, height: 4000 },
    },
    project_schema_version: "pub-editor-v0.11",
    consequences: [],
    scene_refresh: "full_snapshot",
    replayed: false,
  };
}

function commitRequest() {
  return {
    protocol_version: "chaptera.commit-request.v1",
    document_id: DOC,
    source_hash: SOURCE,
    base_revision_id: BASE,
    client_operation_id: "move-1",
    command: {
      kind: "move_node_to",
      node_id: NODE,
      x_emu: 9525,
      y_emu: 10525,
    },
  };
}

function recorder(handler) {
  const calls = [];
  const fetchImpl = async (url, options = {}) => {
    calls.push({
      url,
      path: new URL(url).pathname,
      options: structuredClone({
        ...options,
        body: options.body ?? null,
        headers: { ...(options.headers ?? {}) },
      }),
    });
    return handler(url, options, calls.length - 1);
  };
  return { calls, fetchImpl };
}

test("current Scene uses cookie credentials and rich current-revision Reader projection", async () => {
  const { calls, fetchImpl } = recorder((url) => {
    assert.equal(new URL(url).pathname, "/v1/reader/documents/" + DOC + "/scene");
    return json(readerScene());
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const scene = await service.currentScene();
  assert.equal(scene.protocol_version, "chaptera.editor-render-scene.v1");
  assert.equal(scene.revision_id, BASE);
  assert.equal(scene.nodes[0].bounds.x, 1000);
  assert.deepEqual(scene.paints[0].fill, { r: 32, g: 96, b: 192, a: 255 });
  assert.equal(calls[0].options.credentials, "include");
  assert.equal(calls[0].options.cache, "no-store");
  assert.ok(!("x-chaptera-principal-id" in calls[0].options.headers));
});
test("current visual Scene is bound to exact canonical document/source/revision", async () => {
  const visual = {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: BASE,
    scene_authority: "viewer",
    stacking_fidelity: "exact",
    fidelity: { state: "partial", reasons: [] },
    pages: [],
    nodes: [],
    stories: [],
    resources: [],
    fonts: [],
    diagnostics: [],
  };
  const { calls, fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/documents/" + DOC + "/current") return json(current());
    if (path === "/v1/reader/documents/" + DOC + "/scene") return json(visual);
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const result = await service.currentVisualScene();
  assert.equal(result.current_document.revision_id, BASE);
  assert.equal(result.visual_scene.protocol_version, "chaptera.reader-scene.v1");
  assert.equal(result.visual_scene.revision_id, BASE);
  assert.deepEqual(
    new Set(calls.map((call) => call.path)),
    new Set([
      "/v1/documents/" + DOC + "/current",
      "/v1/reader/documents/" + DOC + "/scene",
    ]),
  );
  for (const call of calls) {
    assert.equal(call.options.credentials, "include");
    assert.equal(call.options.cache, "no-store");
  }
});

test("current visual Scene fails closed on a stale rich visual revision", async () => {
  const { fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/documents/" + DOC + "/current") return json(current(CHILD, 9525));
    if (path === "/v1/reader/documents/" + DOC + "/scene") {
      return json({
        protocol_version: "chaptera.reader-scene.v1",
        document_id: DOC,
        source_hash: SOURCE,
        revision_id: BASE,
        scene_authority: "viewer",
        stacking_fidelity: "exact",
        fidelity: { state: "partial", reasons: [] },
        pages: [],
        nodes: [],
        stories: [],
      });
    }
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  await assert.rejects(
    service.currentVisualScene(),
    /revision differs from canonical current document/,
  );
});

test("commit obtains server CSRF and uses document-scoped canonical route", async () => {
  const { calls, fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/session") return json(session());
    if (path === "/v1/documents/" + DOC + "/commit") return json(accepted());
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const result = await service.commit(commitRequest());
  assert.equal(result.protocol_version, "chaptera.commit-accepted.v1");
  assert.equal(result.revision_id, CHILD);
  assert.deepEqual(
    calls.map((call) => call.path),
    ["/v1/session", "/v1/documents/" + DOC + "/commit"],
  );
  assert.equal(calls[1].options.credentials, "include");
  assert.equal(calls[1].options.headers["x-csrf-token"], "csrf-token-1");
  assert.equal(calls[1].options.headers["content-type"], "application/json");
  assert.ok(!("x-chaptera-principal-id" in calls[1].options.headers));
});

test("stale commit re-reads canonical current head before shell rejection", async () => {
  const { calls, fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/session") return json(session());
    if (path === "/v1/documents/" + DOC + "/commit") {
      return json(
        {
          error: {
            code: "stale_revision",
            message: "base revision is no longer current",
          },
        },
        409,
      );
    }
    if (path === "/v1/documents/" + DOC + "/current") return json(current(CHILD, 9525));
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const result = await service.commit(commitRequest());
  assert.deepEqual(result, {
    protocol_version: "chaptera.commit-rejected.v1",
    document_id: DOC,
    base_revision_id: BASE,
    current_revision_id: CHILD,
    client_operation_id: "move-1",
    code: "stale_revision",
    message_key: "revision.stale_revision",
    retryable: true,
  });
  assert.equal(calls.at(-1).path, "/v1/documents/" + DOC + "/current");
});

test("exact rich scene reconciliation refuses a silently advanced current head", async () => {
  const { calls, fetchImpl } = recorder(() => json(readerScene(CHILD, 9525)));
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  await assert.rejects(
    service.sceneForRevision(BASE),
    /advanced before exact Scene reconciliation/,
  );
  const scene = await service.sceneForRevision(CHILD);
  assert.equal(scene.revision_id, CHILD);
  assert.equal(scene.nodes[0].bounds.x, 9525);
  assert.deepEqual(
    calls.map((call) => call.path),
    [
      "/v1/reader/documents/" + DOC + "/scene",
      "/v1/reader/documents/" + DOC + "/scene",
    ],
  );
});
test("csrf_invalid refreshes /v1/session once and retries the same mutation", async () => {
  let sessionCount = 0;
  let commitCount = 0;
  const { calls, fetchImpl } = recorder((url, options) => {
    const path = new URL(url).pathname;
    if (path === "/v1/session") {
      sessionCount += 1;
      return json(session("csrf-token-" + sessionCount));
    }
    if (path === "/v1/documents/" + DOC + "/commit") {
      commitCount += 1;
      if (commitCount === 1) {
        assert.equal(options.headers["x-csrf-token"], "csrf-token-1");
        return json({ error: "csrf_invalid" }, 403);
      }
      assert.equal(options.headers["x-csrf-token"], "csrf-token-2");
      return json(accepted());
    }
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const result = await service.commit(commitRequest());
  assert.equal(result.revision_id, CHILD);
  assert.equal(sessionCount, 2);
  assert.equal(commitCount, 2);
  assert.deepEqual(
    calls.map((call) => call.path),
    [
      "/v1/session",
      "/v1/documents/" + DOC + "/commit",
      "/v1/session",
      "/v1/documents/" + DOC + "/commit",
    ],
  );
});

test("direct export API uses canonical create/status/download/cancel surfaces without preview", async () => {
  const { calls, fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/session") return json(session());
    if (path === "/v1/exports") {
      return json({
        protocol_version: "chaptera.export-job-http.v1",
        job_id: "job:1",
        document_id: DOC,
        revision_id: CHILD,
        target_profile: "idml",
        layout_environment_id: LAYOUT,
        status: "queued",
        artifact_id: null,
        loss_report_id: null,
        error_code: null,
        progress_percent: null,
      });
    }
    if (path === "/v1/exports/job%3A1") {
      return json({
        protocol_version: "chaptera.export-job-http.v1",
        job_id: "job:1",
        document_id: DOC,
        revision_id: CHILD,
        target_profile: "idml",
        layout_environment_id: LAYOUT,
        status: "ready",
        artifact_id: "binding:artifact",
        loss_report_id: "binding:loss",
        error_code: null,
        progress_percent: null,
      });
    }
    if (path === "/v1/exports/job%3A1/download") {
      return json({
        protocol_version: "chaptera.export-download.v1",
        job_id: "job:1",
        artifact_id: "binding:artifact",
        download_handle: "https://download.invalid/opaque",
        expires_at_ms: Date.now() + 60_000,
      });
    }
    if (path === "/v1/exports/job%3A1/cancel") {
      return json({
        protocol_version: "chaptera.export-job-http.v1",
        job_id: "job:1",
        document_id: DOC,
        revision_id: CHILD,
        target_profile: "idml",
        layout_environment_id: LAYOUT,
        status: "cancelled",
        artifact_id: null,
        loss_report_id: null,
        error_code: null,
        progress_percent: null,
      });
    }
    throw new Error("unexpected path " + path);
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const created = await service.createExport({
    revisionId: CHILD,
    targetProfile: "idml",
    layoutEnvironmentId: LAYOUT,
    clientRequestId: "export-request-1",
  });
  assert.equal(created.status, "queued");

  const status = await service.exportStatus("job:1");
  assert.equal(status.status, "ready");
  assert.equal(status.artifact_id, "binding:artifact");

  const download = await service.authorizeExportDownload("job:1", "binding:artifact");
  assert.equal(download.protocol_version, "chaptera.export-download.v1");

  const cancelled = await service.cancelExport("job:1");
  assert.equal(cancelled.status, "cancelled");

  assert.equal(
    calls.filter((call) => call.path.includes("preview")).length,
    0,
  );
  for (const call of calls) {
    assert.equal(call.options.credentials, "include");
    assert.ok(!("x-chaptera-principal-id" in call.options.headers));
  }
  for (const call of calls.filter((call) => call.options.method === "POST")) {
    assert.equal(call.options.headers["x-csrf-token"], "csrf-token-1");
  }
});


test("migration API binds capability, create, and loss download to exact source", async () => {
  const { calls, fetchImpl } = recorder((url, options) => {
    const path = new URL(url).pathname;
    if (path === "/v1/session") return json(session());
    if (path === "/v1/migration/documents/" + DOC + "/editable-routes") {
      const body = JSON.parse(options.body);
      assert.equal(body.protocol_version, "chaptera.migration-editable-route-request.v1");
      assert.equal(body.document_id, DOC);
      assert.equal(body.source_sha256, SOURCE);
      return json({
        protocol_version: "chaptera.migration-editable-route-response.v1",
        document_id: DOC,
        source_sha256: SOURCE,
        source_byte_len: 12345,
        open_state: "admitted",
        idml: {
          state: "available_with_declared_losses",
          reason_code: "serializable",
          declared_loss_count: 4,
          blocking_loss_count: 0,
        },
        odg: {
          state: "unavailable",
          reason_code: "blocking_losses",
          declared_loss_count: 2,
          blocking_loss_count: 1,
        },
      });
    }
    if (path === "/v1/migration/documents/" + DOC + "/exports") {
      const body = JSON.parse(options.body);
      assert.equal(body.protocol_version, "chaptera.migration-export-create.v1");
      assert.equal(body.document_id, DOC);
      assert.equal(body.source_sha256, SOURCE);
      assert.equal(body.target, "idml");
      assert.equal(body.client_request_id, "migration-request-1");
      return json({
        protocol_version: "chaptera.migration-export-job.v1",
        document_id: DOC,
        source_sha256: SOURCE,
        target: "idml",
        target_profile: "idml:bounded-editable",
        revision_id: BASE,
        job_id: "job:migration",
        status: "queued",
        declared_loss_count: 4,
        blocking_loss_count: 0,
      });
    }
    if (path === "/v1/exports/job%3Amigration/loss-report/download") {
      const body = JSON.parse(options.body);
      assert.equal(body.loss_report_id, "binding:loss");
      return json({
        protocol_version: "chaptera.export-loss-download.v1",
        job_id: "job:migration",
        loss_report_id: "binding:loss",
        download_handle: "https://download.invalid/loss",
        expires_at_ms: Date.now() + 60_000,
      });
    }
    throw new Error("unexpected path " + path);
  });

  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC,
    fetchImpl,
  });

  const capability = await service.migrationEditableRoutes(SOURCE);
  assert.equal(capability.idml.state, "available_with_declared_losses");

  const created = await service.createMigrationExport({
    sourceSha256: SOURCE,
    target: "idml",
    clientRequestId: "migration-request-1",
  });
  assert.equal(created.job_id, "job:migration");

  const loss = await service.authorizeLossReportDownload(
    "job:migration",
    "binding:loss",
  );
  assert.equal(loss.download_handle, "https://download.invalid/loss");

  for (const call of calls.filter((call) => call.options.method === "POST")) {
    assert.equal(call.options.headers["x-csrf-token"], "csrf-token-1");
    assert.equal(call.options.credentials, "include");
  }
});

test("rich editor state consumes the exact authenticated Product current visual scene", async () => {
  const paths = [];
  const fetchImpl = (url, options) => {
    const path = new URL(url).pathname;
    paths.push({ path, credentials: options.credentials });
    if (path === "/v1/documents/" + DOC + "/current") return Promise.resolve(json(current()));
    if (path === "/v1/reader/documents/" + DOC + "/scene") return Promise.resolve(json(readerScene()));
    throw new Error("unexpected path " + path);
  };
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC, fetchImpl,
  });
  const state = await service.currentRichEditorState();
  assert.equal(state.current_document.revision_id, BASE);
  assert.equal(state.reader_scene.protocol_version, "chaptera.reader-scene.v1");
  assert.equal(state.reader_scene.revision_id, BASE);
  assert.equal(state.interaction_scene.protocol_version, "chaptera.editor-interaction-scene.v1");
  assert.equal(state.interaction_scene.revision_id, BASE);
  assert.equal(state.interaction_scene.nodes.length, 1);
  assert.equal(state.interaction_scene.nodes[0].z_order, null);
  assert.deepEqual(paths.map((call) => call.path).sort(), [
    "/v1/documents/" + DOC + "/current",
    "/v1/reader/documents/" + DOC + "/scene",
  ].sort());
  assert.ok(paths.every((call) => call.credentials === "include"));
});

test("rich editor state denies a mismatched document/Reader revision", async () => {
  const fetchImpl = (url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/documents/" + DOC + "/current") return Promise.resolve(json(current(BASE)));
    if (path === "/v1/reader/documents/" + DOC + "/scene") return Promise.resolve(json(readerScene(CHILD)));
    throw new Error("unexpected path " + path);
  };
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC, fetchImpl,
  });
  await assert.rejects(
    service.currentRichEditorState(),
    /visual Scene revision differs from canonical current document/,
  );
});

test("rich Reader scene re-fetch rejects unexpected advanced child revision", async () => {
  const fetchImpl = (url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/documents/" + DOC + "/current") return Promise.resolve(json(current(CHILD)));
    if (path === "/v1/reader/documents/" + DOC + "/scene") return Promise.resolve(json(readerScene(CHILD)));
    throw new Error("unexpected path " + path);
  };
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC, fetchImpl,
  });
  await assert.rejects(
    service.readerSceneForRevision(BASE),
    /canonical current revision advanced before exact rich Scene reconciliation/,
  );
  const reader = await service.readerSceneForRevision(CHILD);
  assert.equal(reader.revision_id, CHILD);
});

test("rich scene read supports production browser observability", async () => {
  const { calls, fetchImpl } = recorder((url) => {
    const path = new URL(url).pathname;
    if (path === "/v1/documents/" + DOC + "/current") return json(current());
    if (path === "/v1/reader/documents/" + DOC + "/scene") return json(readerScene());
    throw new Error("unexpected path " + path);
  });
  let serial = 0;
  const observability = new BrowserObservabilityV1({
    sessionIncarnation: "session:product-editor-test",
    browserFamily: "chromium",
    idFactory: (prefix) => prefix + ":product-test-" + String(++serial).padStart(6, "0"),
  });
  const service = new ChapteraProductEditorServiceV1("https://chaptera.test", {
    documentId: DOC, fetchImpl, observability,
  });
  const state = await service.currentRichEditorState();
  assert.equal(state.current_document.revision_id, BASE);
  assert.equal(state.reader_scene.revision_id, BASE);
  assert.equal(state.interaction_scene.revision_id, BASE);
  assert.deepEqual(
    new Set(calls.map((call) => call.options.headers["x-chaptera-operation-class"])),
    new Set(["open", "scene_read"]),
  );
  assert.ok(calls.every((call) => call.options.credentials === "include"));
});
