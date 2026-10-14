const ID_RE = /^[a-zA-Z0-9._:-]{1,160}$/;

function requireId(value, label) {
  if (typeof value !== "string" || !ID_RE.test(value)) {
    throw new TypeError("invalid " + label);
  }
  return value;
}

function cloudHttpError(code, retryable = false, status = null) {
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

// Shared same-origin browser authority for Chaptera Cloud session + personal workspace.
// SourceIngress and Project Home consume this boundary instead of duplicating
// session/CSRF/workspace bootstrap. It owns no project state or source bytes.
export class ChapteraCloudWorkspaceSessionV1 {
  constructor({ fetchImpl = globalThis.fetch } = {}) {
    if (typeof fetchImpl !== "function") {
      throw new TypeError("fetch implementation is required");
    }
    // Native browser fetch requires its Window receiver.
    this.fetchImpl = fetchImpl.bind(globalThis);
    this.workspaceId = null;
    this.csrfToken = null;
    this.preparing = null;
  }

  async prepare({ signal } = {}) {
    if (this.workspaceId && this.csrfToken) return this.workspaceId;
    if (!this.preparing) {
      this.preparing = (async () => {
        // GET /session rotates the CSRF secret. Keep one token for this active flow.
        const session = await this.request("/v1/session", { signal });
        if (typeof session.csrf_token !== "string" || !session.csrf_token) {
          throw cloudHttpError("session_csrf_missing");
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
    if (write && !this.csrfToken) throw cloudHttpError("session_csrf_missing");
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
      throw cloudHttpError("network_error", true);
    }

    let result;
    try {
      result = await response.json();
    } catch {
      throw cloudHttpError("invalid_ingress_response", false, response.status);
    }
    if (!response.ok) {
      const code = typeof result?.error === "string" ? result.error : "ingress_http_error";
      throw cloudHttpError(code, response.status === 429 || response.status >= 500, response.status);
    }
    if (!result || typeof result !== "object" || Array.isArray(result)) {
      throw cloudHttpError("invalid_ingress_response");
    }
    return result;
  }
}
