import { traceHeadersV1 } from "./observability-v1.mjs";
import { adaptReaderSceneToEditorScene } from "./reader-scene-editor-adapter-v1.mjs";

const COMMIT_REQUEST_V1 = "chaptera.commit-request.v1";
const COMMIT_REJECTED_V1 = "chaptera.commit-rejected.v1";
const EXPORT_CREATE_V1 = "chaptera.export-create.v1";
const MIGRATION_EDITABLE_ROUTE_REQUEST_V1 =
  "chaptera.migration-editable-route-request.v1";
const MIGRATION_EXPORT_CREATE_V1 = "chaptera.migration-export-create.v1";

const REJECTABLE_COMMIT_CODES = new Set([
  "stale_revision",
  "idempotency_conflict",
  "source_hash_mismatch",
  "node_id_invalid",
  "move_node_rejected",
  "authz_denied",
  "authz_expired",
]);

function clone(value) {
  return structuredClone(value);
}

function ident(value, label) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 256 ||
    !/^[A-Za-z0-9_.:@/-]+$/.test(value)
  ) {
    throw new TypeError(label + " must be a bounded Chaptera identity");
  }
  return value;
}

function absoluteBaseUrl(value) {
  if (typeof value !== "string" || !/^https?:\/\//.test(value)) {
    throw new TypeError("absolute HTTP(S) base URL is required");
  }
  return value.replace(/\/$/, "");
}

function responseErrorCode(value) {
  if (typeof value?.error === "string") return value.error;
  if (typeof value?.error?.code === "string") return value.error.code;
  return null;
}

function responseErrorMessage(value) {
  if (typeof value?.error?.message === "string") return value.error.message;
  const code = responseErrorCode(value);
  return code ?? "unknown_error";
}

function ensureProtocol(value, expected, label) {
  if (!value || value.protocol_version !== expected) {
    throw new Error(label + " protocol mismatch");
  }
  return value;
}

export class ChapteraProductEditorServiceV1 {
  constructor(
    baseUrl,
    {
      documentId,
      fetchImpl = globalThis.fetch,
      observability = null,
    } = {},
  ) {
    this.baseUrl = absoluteBaseUrl(baseUrl);
    this.documentId = ident(documentId, "documentId");
    if (typeof fetchImpl !== "function") throw new TypeError("fetchImpl is required");
    this.fetchImpl = fetchImpl;
    this.observability = observability;
    this.sessionCache = null;
    this.commitRequests = 0;
    this.lastRequest = null;
    this.lastTraceContext = null;
    this.lastCommitTraceContext = null;
  }

  async session({ force = false } = {}) {
    const now = Date.now();
    if (
      !force &&
      this.sessionCache &&
      Number.isSafeInteger(this.sessionCache.idle_expires_at_ms) &&
      Number.isSafeInteger(this.sessionCache.absolute_expires_at_ms) &&
      now + 5_000 < this.sessionCache.idle_expires_at_ms &&
      now + 5_000 < this.sessionCache.absolute_expires_at_ms
    ) {
      return clone(this.sessionCache);
    }

    const result = await this.#fetchJson("/v1/session", {
      method: "GET",
    });
    this.#throwUnlessOk(result, "session");
    const value = result.value;
    if (
      typeof value?.principal_id !== "string" ||
      typeof value?.csrf_token !== "string" ||
      value.csrf_token.length === 0
    ) {
      throw new Error("chaptera session response is missing principal/csrf identity");
    }
    this.sessionCache = clone(value);
    return clone(value);
  }

  async currentDocument() {
    const context = this.#context("open");
    const result = await this.#fetchJson(
      "/v1/documents/" + encodeURIComponent(this.documentId) + "/current",
      { method: "GET" },
      context,
    );
    this.#throwUnlessOk(result, "current document");
    const value = ensureProtocol(
      result.value,
      "chaptera.current-document.v1",
      "current document",
    );
    if (value.document_id !== this.documentId) {
      throw new Error("current document identity mismatch");
    }
    return clone(value);
  }

  async currentScene() {
    return adaptReaderSceneToEditorScene(await this.#currentReaderScene());
  }

  async currentVisualScene() {
    const [current, visualResult] = await Promise.all([
      this.currentDocument(),
      this.#fetchJson(
        "/v1/reader/documents/" + encodeURIComponent(this.documentId) + "/scene",
        { method: "GET" },
        this.#context("visual_scene"),
      ),
    ]);
    this.#throwUnlessOk(visualResult, "current visual scene");
    const visual = ensureProtocol(
      visualResult.value,
      "chaptera.reader-scene.v1",
      "current visual scene",
    );
    if (visual.document_id !== current.document_id) {
      throw new Error("visual Scene document identity mismatch");
    }
    if (visual.source_hash !== current.source_hash) {
      throw new Error("visual Scene source identity mismatch");
    }
    if (visual.revision_id !== current.revision_id) {
      throw new Error("visual Scene revision differs from canonical current document");
    }
    return {
      current_document: clone(current),
      visual_scene: clone(visual),
    };
  }

  async sceneForRevision(revisionId) {
    ident(revisionId, "revisionId");
    const readerScene = await this.#currentReaderScene();
    if (readerScene.revision_id !== revisionId) {
      throw new Error(
        "canonical current revision advanced before exact Scene reconciliation",
      );
    }
    return adaptReaderSceneToEditorScene(readerScene);
  }

  async #currentReaderScene() {
    const context = this.#context("scene_read");
    const result = await this.#fetchJson(
      "/v1/reader/documents/" + encodeURIComponent(this.documentId) + "/scene",
      { method: "GET" },
      context,
    );
    this.#throwUnlessOk(result, "current Reader scene");
    const value = ensureProtocol(
      result.value,
      "chaptera.reader-scene.v1",
      "current Reader scene",
    );
    if (value.document_id !== this.documentId) {
      throw new Error("current Reader scene document identity mismatch");
    }
    return clone(value);
  }

  async commit(request) {
    if (!request || request.protocol_version !== COMMIT_REQUEST_V1) {
      throw new TypeError("chaptera.commit-request.v1 is required");
    }
    if (request.document_id !== this.documentId) {
      throw new Error("commit document identity differs from bound service document");
    }
    ident(request.client_operation_id, "client_operation_id");

    this.commitRequests += 1;
    this.lastRequest = clone(request);
    const context = this.#context("commit", request.client_operation_id);
    this.lastCommitTraceContext = context;

    const result = await this.#mutationJson(
      "/v1/documents/" + encodeURIComponent(this.documentId) + "/commit",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request),
      },
      context,
    );

    if (result.response.ok) {
      return clone(
        ensureProtocol(result.value, "chaptera.commit-accepted.v1", "commit accepted"),
      );
    }

    const code = responseErrorCode(result.value);
    if (!REJECTABLE_COMMIT_CODES.has(code)) {
      this.#throwUnlessOk(result, "commit");
    }

    let currentRevisionId = null;
    if (code === "stale_revision") {
      const current = await this.currentDocument();
      currentRevisionId = current.revision_id;
    }
    return {
      protocol_version: COMMIT_REJECTED_V1,
      document_id: request.document_id,
      base_revision_id: request.base_revision_id,
      current_revision_id: currentRevisionId,
      client_operation_id: request.client_operation_id,
      code,
      message_key: "revision." + code,
      retryable: code === "stale_revision",
    };
  }

  async migrationEditableRoutes(sourceSha256) {
    ident(sourceSha256, "sourceSha256");
    const context = this.#context("migration_capability");
    const result = await this.#mutationJson(
      "/v1/migration/documents/" +
        encodeURIComponent(this.documentId) +
        "/editable-routes",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          protocol_version: MIGRATION_EDITABLE_ROUTE_REQUEST_V1,
          document_id: this.documentId,
          source_sha256: sourceSha256,
        }),
      },
      context,
    );
    this.#throwUnlessOk(result, "migration editable routes");
    const value = ensureProtocol(
      result.value,
      "chaptera.migration-editable-route-response.v1",
      "migration editable routes",
    );
    if (
      value.document_id !== this.documentId ||
      value.source_sha256 !== sourceSha256
    ) {
      throw new Error("migration capability identity mismatch");
    }
    return clone(value);
  }

  async createMigrationExport({ sourceSha256, target, clientRequestId }) {
    ident(sourceSha256, "sourceSha256");
    ident(clientRequestId, "clientRequestId");
    if (!["idml", "odg"].includes(target)) {
      throw new TypeError("target must be idml or odg");
    }
    const context = this.#context("migration_export", clientRequestId);
    const result = await this.#mutationJson(
      "/v1/migration/documents/" +
        encodeURIComponent(this.documentId) +
        "/exports",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          protocol_version: MIGRATION_EXPORT_CREATE_V1,
          document_id: this.documentId,
          source_sha256: sourceSha256,
          target,
          client_request_id: clientRequestId,
        }),
      },
      context,
    );
    this.#throwUnlessOk(result, "migration export create");
    const value = ensureProtocol(
      result.value,
      "chaptera.migration-export-job.v1",
      "migration export job",
    );
    if (
      value.document_id !== this.documentId ||
      value.source_sha256 !== sourceSha256 ||
      value.target !== target
    ) {
      throw new Error("migration export identity mismatch");
    }
    return clone(value);
  }

  async createExport({
    revisionId,
    targetProfile,
    layoutEnvironmentId,
    clientRequestId,
  }) {
    ident(revisionId, "revisionId");
    ident(targetProfile, "targetProfile");
    ident(layoutEnvironmentId, "layoutEnvironmentId");
    ident(clientRequestId, "clientRequestId");
    const context = this.#context("export", clientRequestId);
    const result = await this.#mutationJson(
      "/v1/exports",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          protocol_version: EXPORT_CREATE_V1,
          document_id: this.documentId,
          revision_id: revisionId,
          target_profile: targetProfile,
          layout_environment_id: layoutEnvironmentId,
          client_request_id: clientRequestId,
        }),
      },
      context,
    );
    this.#throwUnlessOk(result, "export create");
    return clone(
      ensureProtocol(result.value, "chaptera.export-job-http.v1", "export job"),
    );
  }

  async exportStatus(jobId) {
    ident(jobId, "jobId");
    const context = this.#context("export_status");
    const result = await this.#fetchJson(
      "/v1/exports/" + encodeURIComponent(jobId),
      { method: "GET" },
      context,
    );
    this.#throwUnlessOk(result, "export status");
    return clone(
      ensureProtocol(result.value, "chaptera.export-job-http.v1", "export job"),
    );
  }

  async cancelExport(jobId) {
    ident(jobId, "jobId");
    const context = this.#context("export_cancel");
    const result = await this.#mutationJson(
      "/v1/exports/" + encodeURIComponent(jobId) + "/cancel",
      { method: "POST" },
      context,
    );
    this.#throwUnlessOk(result, "export cancel");
    return clone(
      ensureProtocol(result.value, "chaptera.export-job-http.v1", "export job"),
    );
  }

  async authorizeExportDownload(jobId, artifactId) {
    ident(jobId, "jobId");
    ident(artifactId, "artifactId");
    const context = this.#context("export_download");
    const result = await this.#mutationJson(
      "/v1/exports/" + encodeURIComponent(jobId) + "/download",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ artifact_id: artifactId }),
      },
      context,
    );
    this.#throwUnlessOk(result, "export download");
    return clone(
      ensureProtocol(
        result.value,
        "chaptera.export-download.v1",
        "export download",
      ),
    );
  }

  async authorizeLossReportDownload(jobId, lossReportId) {
    ident(jobId, "jobId");
    ident(lossReportId, "lossReportId");
    const context = this.#context("export_loss_download");
    const result = await this.#mutationJson(
      "/v1/exports/" +
        encodeURIComponent(jobId) +
        "/loss-report/download",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ loss_report_id: lossReportId }),
      },
      context,
    );
    this.#throwUnlessOk(result, "export loss report download");
    return clone(
      ensureProtocol(
        result.value,
        "chaptera.export-loss-download.v1",
        "export loss report download",
      ),
    );
  }

  #context(operationClass, clientOperationId = null) {
    if (!this.observability) return null;
    const context = this.observability.createContext({
      operationClass,
      clientOperationId,
    });
    this.lastTraceContext = context;
    return context;
  }

  async #mutationJson(path, options, context = null) {
    let session = await this.session();
    let result = await this.#fetchJson(
      path,
      {
        ...options,
        headers: {
          ...(options.headers ?? {}),
          "x-csrf-token": session.csrf_token,
        },
      },
      context,
    );

    const code = responseErrorCode(result.value);
    if (result.response.status === 403 && code === "csrf_invalid") {
      session = await this.session({ force: true });
      result = await this.#fetchJson(
        path,
        {
          ...options,
          headers: {
            ...(options.headers ?? {}),
            "x-csrf-token": session.csrf_token,
          },
        },
        context,
      );
    }
    return result;
  }

  async #fetchJson(path, options = {}, context = null) {
    const headers = {
      ...(options.headers ?? {}),
      ...(context ? traceHeadersV1(context) : {}),
    };
    const response = await this.fetchImpl(this.baseUrl + path, {
      cache: "no-store",
      credentials: "include",
      ...options,
      headers,
    });
    let value = null;
    try {
      value = await response.json();
    } catch {
      value = null;
    }
    return { response, value };
  }

  #throwUnlessOk(result, label) {
    if (result.response.ok) return;
    const code = responseErrorCode(result.value);
    const message = responseErrorMessage(result.value);
    throw new Error(
      label +
        " request failed: " +
        result.response.status +
        " " +
        (code ?? "unknown_error") +
        " " +
        message,
    );
  }
}
