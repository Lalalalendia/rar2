// Browser-to-Rust SourceIngress adapter. Only same-origin HTTP is authoritative.
// The durable project/revision owner remains chaptera-server; this module stores no PUB bytes.
const ID_RE = /^[a-zA-Z0-9._:-]{1,160}$/;
const TERMINAL_REJECT = new Set(["REJECTED", "EXPIRED"]);
const UPLOADED = new Set(["STORED_UNVERIFIED", "VALIDATING", "VALIDATED_DURABLE", "CONSUMED"]);

function requireId(value, label) {
  if (typeof value !== "string" || !ID_RE.test(value)) {
    throw new TypeError("invalid " + label);
  }
  return value;
}

function ingressError(code, retryable = false, status = null) {
  const error = new Error(code);
  error.code = code;
  error.retryable = retryable;
  error.status = status;
  return error;
}

function abortError() {
  const error = new Error("aborted");
  error.name = "AbortError";
  return error;
}

function delay(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { reject(abortError()); return; }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", stop);
      resolve();
    }, ms);
    function stop() {
      clearTimeout(timer);
      signal?.removeEventListener("abort", stop);
      reject(abortError());
    }
    signal?.addEventListener("abort", stop, { once: true });
  });
}

export class ChapteraCloudSourceIngressV1 {
  constructor({
    fetchImpl = globalThis.fetch,
    sleep = delay,
    pollMs = 550,
    validationTimeoutMs = 120000,
  } = {}) {
    if (typeof fetchImpl !== "function" || typeof sleep !== "function") {
      throw new TypeError("fetch and sleep implementations are required");
    }
    if (!Number.isInteger(pollMs) || pollMs < 0 ||
        !Number.isInteger(validationTimeoutMs) || validationTimeoutMs <= 0) {
      throw new TypeError("valid poll interval and timeout are required");
    }
    // Browser native fetch requires its Window receiver; an unbound method
    // called as this.fetchImpl(...) fails with Illegal invocation in Chromium.
    this.fetchImpl = fetchImpl.bind(globalThis);
    this.sleep = sleep;
    this.pollMs = pollMs;
    this.validationTimeoutMs = validationTimeoutMs;
    this.workspaceId = null;
    this.csrfToken = null;
    this.preparing = null;
    this.uploads = new Map();
  }

  async prepare({ signal } = {}) {
    if (this.workspaceId && this.csrfToken) return this.workspaceId;
    if (!this.preparing) {
      this.preparing = (async () => {
        // GET /session rotates the CSRF secret; obtain it once per active flow.
        const session = await this.request("/v1/session", { signal });
        if (typeof session.csrf_token !== "string" || !session.csrf_token) {
          throw ingressError("session_csrf_missing");
        }
        this.csrfToken = session.csrf_token;
        const personal = await this.request("/v1/workspaces/personal", {
          method: "POST",
          json: {},
          signal,
        });
        this.workspaceId = requireId(personal.workspace_id, "workspace id");
        return this.workspaceId;
      })().finally(() => { this.preparing = null; });
    }
    return this.preparing;
  }

  async request(path, { method = "GET", json, body, signal } = {}) {
    if (typeof path !== "string" || !path.startsWith("/v1/") || path.startsWith("//") ||
        path.includes("?") || path.includes("#")) {
      throw new TypeError("only bounded same-origin Chaptera API paths are supported");
    }
    const write = method !== "GET";
    if (write && !this.csrfToken) throw ingressError("session_csrf_missing");
    const headers = { accept: "application/json" };
    if (write) headers["x-csrf-token"] = this.csrfToken;
    if (json !== undefined) headers["content-type"] = "application/json";
    let response;
    try {
      response = await this.fetchImpl(path, {
        method,
        credentials: "same-origin",
        redirect: "error",
        headers,
        body: json !== undefined ? JSON.stringify(json) : body,
        signal,
      });
    } catch (error) {
      if (signal?.aborted || error?.name === "AbortError") throw abortError();
      throw ingressError("network_error", true);
    }
    let result;
    try {
      result = await response.json();
    } catch {
      throw ingressError("invalid_ingress_response", false, response.status);
    }
    if (!response.ok) {
      const code = typeof result?.error === "string" ? result.error : "ingress_http_error";
      throw ingressError(code, response.status === 429 || response.status >= 500, response.status);
    }
    if (!result || typeof result !== "object" || Array.isArray(result)) {
      throw ingressError("invalid_ingress_response");
    }
    return result;
  }

  async beginUpload({ client_request_id, file_name, byte_length, mime, signal }) {
    await this.prepare({ signal });
    const requestId = requireId(client_request_id, "request id");
    if (!Number.isSafeInteger(byte_length) || byte_length < 1) {
      throw ingressError("upload_invalid_size");
    }
    if (typeof file_name !== "string" || !/\.pub$/i.test(file_name)) {
      throw ingressError("unsupported_file_type");
    }
    const value = await this.request("/v1/uploads", {
      method: "POST",
      json: {
        workspace_id: this.workspaceId,
        purpose: "pub_source",
        expected_byte_len: byte_length,
        declared_content_type: mime || null,
        idempotency_key: requestId,
      },
      signal,
    });
    const id = requireId(value.upload?.upload_id, "upload id");
    const transport = value.transport;
    if (transport != null && transport.kind !== "streamed") {
      // Never send a session cookie or raw PUB to a provider grant we did not
      // validate. The present S3 adapter advertises the streamed transport.
      throw ingressError("direct_upload_not_supported", false);
    }
    const expectedPath = "/v1/uploads/" + encodeURIComponent(id) + "/content";
    if (transport && transport.path !== expectedPath &&
        transport.path !== "/v1/uploads/" + id + "/content") {
      throw ingressError("upload_transport_invalid", false);
    }
    const prior = this.uploads.get(id);
    this.uploads.set(id, {
      name: file_name,
      upload: value.upload,
      transport,
      clientRequestId: requestId,
      // When create-project reached durable commit but its ACK was lost,
      // issue replay returns CONSUMED with a later generation. Preserve the
      // validated input generation for exact-request reconciliation.
      validatedGeneration: prior?.clientRequestId === requestId
        ? prior.validatedGeneration ?? null
        : null,
      projectReceipt: prior?.clientRequestId === requestId
        ? prior.projectReceipt ?? null
        : null,
    });
    return { upload_id: id };
  }

  async uploadBytes({ upload_id, file, signal, onProgress }) {
    const id = requireId(upload_id, "upload id");
    const state = this.uploads.get(id);
    if (!state) throw ingressError("upload_not_started");
    if (!file || !Number.isSafeInteger(file.size) || file.size !== state.upload.expected_byte_len) {
      throw ingressError("upload_length_mismatch");
    }
    if (TERMINAL_REJECT.has(state.upload.state)) {
      throw ingressError("source_validation_failed", false);
    }
    if (UPLOADED.has(state.upload.state)) {
      onProgress?.(file.size, file.size);
      return;
    }
    if (state.upload.state !== "ISSUED") throw ingressError("upload_state_invalid");

    if (!state.transport) {
      // Issue-replay can omit a grant. Recover a successful unknown PUT before
      // trying a create-only upload again; never trust an off-origin grant.
      try {
        state.upload = await this.completeUpload({ upload_id: id, signal });
        onProgress?.(file.size, file.size);
        return;
      } catch (error) {
        if (error.code !== "quarantine_object_missing") throw error;
      }
    }
    const contentPath = "/v1/uploads/" + encodeURIComponent(id) + "/content";
    state.upload = await this.request(contentPath, { method: "PUT", body: file, signal });
    onProgress?.(file.size, file.size);
  }

  async completeUpload({ upload_id, signal }) {
    const id = requireId(upload_id, "upload id");
    const state = this.uploads.get(id);
    if (!state) throw ingressError("upload_not_started");
    state.upload = await this.request("/v1/uploads/" + encodeURIComponent(id) + "/complete", {
      method: "POST",
      signal,
    });
    return state.upload;
  }

  async waitUntilValidated({ upload_id, signal }) {
    const id = requireId(upload_id, "upload id");
    const state = this.uploads.get(id);
    if (!state) throw ingressError("upload_not_started");
    const deadline = Date.now() + this.validationTimeoutMs;
    while (true) {
      if (signal?.aborted) throw abortError();
      const current = await this.request("/v1/uploads/" + encodeURIComponent(id), { signal });
      state.upload = current;
      if (current.state === "VALIDATED_DURABLE") {
        state.validatedGeneration = current.upload_generation;
        return { status: "validated" };
      }
      if (current.state === "CONSUMED") {
        // Only an in-memory known original generation may reconcile a lost
        // CreateProject ACK. Never invent a generation from a consumed upload.
        return state.validatedGeneration == null
          ? { status: "rejected", code: "source_already_consumed", retryable: false }
          : { status: "validated" };
      }
      if (TERMINAL_REJECT.has(current.state)) {
        return {
          status: "rejected",
          code: current.terminal_code ?? "source_validation_failed",
          retryable: false,
        };
      }
      if (!["ISSUED", "STORED_UNVERIFIED", "VALIDATING"].includes(current.state)) {
        throw ingressError("unexpected_upload_state", false);
      }
      if (Date.now() >= deadline) throw ingressError("source_validation_timeout", true);
      await this.sleep(this.pollMs, signal);
    }
  }

  async createProjectFromUpload({ upload_id, client_request_id, signal }) {
    const id = requireId(upload_id, "upload id");
    const state = this.uploads.get(id);
    if (!state || !this.workspaceId) throw ingressError("upload_not_started");
    if (state.upload.state !== "VALIDATED_DURABLE" &&
        state.upload.state !== "CONSUMED") {
      throw ingressError("source_not_validated_durable");
    }
    const requestId = requireId(client_request_id, "request id");
    if (state.projectReceipt && state.clientRequestId === requestId) {
      return structuredClone(state.projectReceipt);
    }
    const result = await this.request("/v1/projects/from-upload", {
      method: "POST",
      json: {
        workspace_id: this.workspaceId,
        upload_id: id,
        expected_upload_generation: state.validatedGeneration ?? state.upload.upload_generation,
        name: state.name,
        client_idempotency_id: requestId,
      },
      signal,
    });
    requireId(result.project_id, "project id");
    requireId(result.document_id, "document id");
    // Keep the confirmed result until the controller opens the project.
    // A failed navigation must not discard the accepted creation outcome.
    state.projectReceipt = structuredClone(result);
    return result;
  }
}
