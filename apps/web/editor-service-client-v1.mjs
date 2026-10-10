import { traceHeadersV1 } from "./observability-v1.mjs";

export class HttpEditorServiceV1 {
  constructor(baseUrl, { observability = null, principalId = "synthetic-editor" } = {}) {
    if (typeof baseUrl !== "string" || !/^https?:\/\//.test(baseUrl)) {
      throw new TypeError("absolute HTTP(S) base URL is required");
    }
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.commitRequests = 0;
    this.historyRequests = 0;
    this.lastRequest = null;
    this.lastHistoryRequest = null;
    this.observability = observability;
    if (typeof principalId !== "string" || !/^[A-Za-z0-9_.:@/-]{1,192}$/.test(principalId)) {
      throw new TypeError("valid principalId is required");
    }
    this.principalId = principalId;
    this.lastTraceContext = null;
    this.lastCommitTraceContext = null;
    this.lastHistoryTraceContext = null;
    this.lastExportPreview = null;
    this.lastExportPreviewTraceContext = null;
  }

  async currentScene() {
    const context = this.#context("open");
    return this.#json("/v1/scenes/current", {}, context, "browser.scene_current");
  }

  async commit(request) {
    this.commitRequests += 1;
    this.lastRequest = structuredClone(request);
    const context = this.#context("commit", request.client_operation_id ?? null);
    this.lastCommitTraceContext = context;
    return this.#json("/v1/commit", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }, context, "browser.commit_http");
  }

  async historyTransition(request) {
    this.historyRequests += 1;
    this.lastHistoryRequest = structuredClone(request);
    const context = this.#context("commit", request.client_operation_id ?? null);
    this.lastHistoryTraceContext = context;
    return this.#json("/v1/commit", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }, context, "browser.history_http");
  }

  async editorCapabilities() {
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/editor/capabilities",
      {},
      context,
      "browser.editor_capabilities_http",
    );
  }

  async fontEnvironment() {
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/editor/font-environment", {}, context, "browser.font_environment_http",
    );
  }

  async fontFormatScope() {
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/editor/font-format-scope", {}, context, "browser.font_format_scope_http",
    );
  }

  async currentPhysicalFontSpans({story_id, revision_id, snapshot_id}) {
    const uuid = /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/;
    const revision = /^sha256:[0-9a-f]{64}$/;
    if (!uuid.test(story_id) || !revision.test(revision_id) ||
        !revision.test(snapshot_id)) {
      throw new TypeError("current authoritative Story/Scene identity required for glyph projection");
    }
    const query = new URLSearchParams({story_id, revision_id, snapshot_id});
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/editor/font-glyph-spans?" + query.toString(),
      {}, context, "browser.current_physical_glyphs_http",
    );
  }

  async fontAuthoringAdmission() {
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/editor/font-authoring-admission", {}, context, "browser.font_admission_http",
    );
  }

  async exactFontResourceBytes(descriptor) {
    // This is preview-only delivery. A font URL, its name or its bytes never
    // grant authoritative Editor mutations or fixed-output substitution.
    const uuid = /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/;
    const hash = /^[0-9a-f]{64}$/;
    if (!descriptor || !uuid.test(descriptor.resource_id) ||
        !hash.test(descriptor.content_hash) || descriptor.delivery !== "deliver_exact" ||
        typeof descriptor.fetch_handle !== "string") {
      throw new TypeError("complete exact server font descriptor required");
    }
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(descriptor.fetch_handle)) {
      throw new TypeError("font delivery requires an opaque server handle");
    }
    const url = this.baseUrl + "/v1/editor/font-resource/" +
      encodeURIComponent(descriptor.fetch_handle);
    const context = this.#context("scene_read");
    const execute = async () => {
      const response = await fetch(url, {
        cache: "no-store",
        credentials: "omit",
        headers: {
          "x-chaptera-principal-id": this.principalId,
          ...(context ? traceHeadersV1(context) : {}),
        },
      });
      if (!response.ok) {
        throw new Error("exact font delivery denied: " + response.status);
      }
      if (response.headers.get("content-type") !== "font/ttf" ||
          response.headers.get("x-chaptera-font-content-sha256") !== descriptor.content_hash) {
        throw new Error("exact font delivery metadata mismatch");
      }
      const bytes = await response.arrayBuffer();
      if (bytes.byteLength === 0 || bytes.byteLength > 32 * 1024 * 1024) {
        throw new Error("exact font byte size invalid");
      }
      const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
      const actual = Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
      if (actual !== descriptor.content_hash) {
        throw new Error("exact font content SHA-256 mismatch");
      }
      return bytes;
    };
    if (this.observability && context) {
      return this.observability.measure("browser.font_exact_bytes_http", context, execute);
    }
    return execute();
  }

  async nativePubPreview() {
    const context = this.#context("export");
    return this.#json(
      "/v1/pub-save/preview",
      {},
      context,
      "browser.native_pub_preview_http",
    );
  }

  async nativePubDownload() {
    const context = this.#context("export");
    const execute = async () => {
      const response = await fetch(this.baseUrl + "/v1/pub-save/download", {
        cache: "no-store",
        credentials: "omit",
        headers: {
          "x-chaptera-principal-id": this.principalId,
          ...(context ? traceHeadersV1(context) : {}),
        },
      });
      if (!response.ok) {
        let detail = null;
        try {
          detail = await response.json();
        } catch {}
        throw new Error(
          "editor native PUB download failed: " +
          response.status + " " + JSON.stringify(detail),
        );
      }
      return {
        blob: await response.blob(),
        content_disposition: response.headers.get("content-disposition"),
      };
    };

    if (this.observability && context) {
      return this.observability.measure(
        "browser.native_pub_download_http",
        context,
        execute,
      );
    }
    return execute();
  }

  async exportPreview(target = "idml") {
    if (!["idml", "odg"].includes(target)) {
      throw new TypeError("export preview target must be idml or odg");
    }
    const context = this.#context("export");
    this.lastExportPreviewTraceContext = context;
    const preview = await this.#json(
      "/v1/export/preview?target=" + encodeURIComponent(target),
      {},
      context,
      "browser.export_preview_http",
    );
    this.lastExportPreview = structuredClone(preview);
    return preview;
  }

  async sceneForRevision(revisionId) {
    if (typeof revisionId !== "string" || !revisionId.startsWith("sha256:")) {
      throw new TypeError("revision id is required");
    }
    const context = this.#context("scene_read");
    return this.#json(
      "/v1/scenes/" + encodeURIComponent(revisionId),
      {},
      context,
      "browser.scene_revision"
    );
  }

  async harnessState() {
    return this.#json("/v1/harness/state");
  }

  async traceSummary(traceId) {
    if (typeof traceId !== "string" || traceId.length < 8) {
      throw new TypeError("trace id is required");
    }
    return this.#json("/v1/observability/traces/" + encodeURIComponent(traceId));
  }

  async metricsSnapshot() {
    return this.#json("/v1/observability/metrics");
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

  async #json(path, options = {}, context = null, browserSpanName = null) {
    const headers = {
      "x-chaptera-principal-id": this.principalId,
      ...(options.headers ?? {}),
      ...(context ? traceHeadersV1(context) : {}),
    };
    const execute = async () => {
      const response = await fetch(this.baseUrl + path, {
        cache: "no-store",
        credentials: "omit",
        ...options,
        headers,
      });
      const value = await response.json();
      if (!response.ok) {
        throw new Error("editor service request failed: " + response.status + " " + JSON.stringify(value));
      }
      return value;
    };

    if (this.observability && context && browserSpanName) {
      return this.observability.measure(browserSpanName, context, execute);
    }
    return execute();
  }
}
