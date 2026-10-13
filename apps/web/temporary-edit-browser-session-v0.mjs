// Browser-side adapter for the temporary (no-account) PUB intake.
// This is NOT an edit/save API. Only a server-accepted revision may extend TTL.
export const TEMPORARY_EDIT_STORAGE_KEY = "chaptera.temporary-edit.v0";
export const TEMPORARY_EDIT_PROTOCOL = "chaptera.reader-guest-session.v1";
const TEMPORARY_EDIT_LOCAL_SCHEMA = "chaptera.temporary-edit.browser.v0";
const SESSION_ID = /^tempedit:[0-9a-f]{32}$/;
const SECRET = /^[0-9a-f]{64}$/;
const SOURCE_SHA = /^[0-9a-f]{64}$/;
const CLASSIFICATIONS = new Set(["supported", "partial", "salvage", "unsupported", "rejected"]);
const SAFE_SERVER_ERRORS = new Set([
  "guest_session_expired", "guest_session_not_found", "guest_session_token_required",
  "guest_upload_too_large", "guest_upload_length_mismatch", "guest_upload_state_conflict",
  "guest_upload_not_stored", "guest_open_in_progress", "guest_scene_not_ready",
  "guest_source_scan_failed", "guest_scene_worker_failed", "guest_upload_admission_failed",
]);

function fail(code, status) {
  const error = new Error(code);
  error.code = code;
  if (status !== undefined) error.status = status;
  return error;
}

function pathFor(sessionId, action) {
  if (!SESSION_ID.test(sessionId) || !["content", "open", "scene"].includes(action)) {
    throw fail("temporary_session_identity_invalid");
  }
  return "/v1/reader/guest-sessions/" + sessionId + "/" + action;
}

function validExpiry(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function validRecord(value) {
  return value?.schema === TEMPORARY_EDIT_LOCAL_SCHEMA
    && SESSION_ID.test(value.session_id ?? "")
    && SECRET.test(value.access_token ?? "")
    && validExpiry(value.expires_at_ms)
    && (value.source_sha256 === null || SOURCE_SHA.test(value.source_sha256 ?? ""));
}

async function readJson(response) {
  let body;
  try {
    body = await response.json();
  } catch {
    throw fail("temporary_response_invalid", response.status);
  }
  if (!response.ok) {
    // Never reflect arbitrary server text, source bytes or bearer material.
    const raw = body?.error;
    const code = typeof raw === "string" && SAFE_SERVER_ERRORS.has(raw)
      ? raw : "temporary_request_failed";
    throw fail(code, response.status);
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw fail("temporary_response_invalid");
  }
  return body;
}

function checkBinding(reply, sessionId) {
  if (reply.protocol_version !== TEMPORARY_EDIT_PROTOCOL || reply.session_id !== sessionId
      || !validExpiry(reply.expires_at_ms)) {
    throw fail("temporary_response_identity_mismatch");
  }
}

function snapshot(reply) {
  if (!CLASSIFICATIONS.has(reply.classification)) {
    throw fail("temporary_classification_invalid");
  }
  if (reply.source_sha256 !== undefined && reply.source_sha256 !== null
      && !SOURCE_SHA.test(reply.source_sha256)) {
    throw fail("temporary_source_identity_invalid");
  }
  if (["supported", "partial"].includes(reply.classification)) {
    if (!SOURCE_SHA.test(reply.source_sha256 ?? "")
        || reply.scene?.protocol_version !== "chaptera.reader-scene.v1") {
      throw fail("temporary_scene_invalid");
    }
  }
  return {
    kind: "temporary_preview",
    classification: reply.classification,
    expires_at_ms: reply.expires_at_ms,
    source_sha256: reply.source_sha256 ?? null,
    scene: reply.scene ?? null,
    salvage: reply.salvage ?? null,
    compatibility_report: reply.compatibility_report ?? null,
    // No mutation/download claim until isolated temporary edit authority exists.
    can_edit: false,
    can_download_changed_file: false,
  };
}

export class TemporaryEditBrowserSessionV0 {
  #record = null;
  #busy = false;

  constructor({ fetchImpl = globalThis.fetch, storage = null, csrfToken } = {}) {
    if (typeof fetchImpl !== "function" || typeof csrfToken !== "string"
        || !/^[0-9a-f]{32}$/.test(csrfToken)) {
      throw fail("temporary_client_config_invalid");
    }
    this.fetchImpl = fetchImpl;
    this.storage = storage;
    this.csrfToken = csrfToken;
  }

  #save(record) {
    if (!validRecord(record)) throw fail("temporary_storage_record_invalid");
    this.#record = record;
    if (!this.storage) return false;
    try {
      this.storage.setItem(TEMPORARY_EDIT_STORAGE_KEY, JSON.stringify(record));
      return true;
    } catch {
      // A tab without sessionStorage can still view, but cannot resume after reload.
      return false;
    }
  }

  #restore() {
    if (this.#record) return this.#record;
    let raw;
    try {
      raw = this.storage?.getItem(TEMPORARY_EDIT_STORAGE_KEY);
    } catch {
      return null;
    }
    if (!raw) return null;
    let record;
    try { record = JSON.parse(raw); } catch { this.clear(); return null; }
    if (!validRecord(record)) { this.clear(); return null; }
    this.#record = record;
    return record;
  }

  clear() {
    this.#record = null;
    try { this.storage?.removeItem(TEMPORARY_EDIT_STORAGE_KEY); } catch { /* storage disabled */ }
    // Local forget is not server deletion; temporary bytes expire on the server.
  }

  hasSavedSession() {
    return this.#restore() !== null;
  }

  async #request(path, { method = "GET", body, token, contentType, signal } = {}) {
    const headers = {
      "accept": "application/json",
      "x-csrf-token": this.csrfToken,
    };
    if (token) headers["x-chaptera-reader-session"] = token;
    if (contentType) headers["content-type"] = contentType;
    const response = await this.fetchImpl.call(globalThis, path, {
      method, headers, body, signal,
      credentials: "omit", cache: "no-store", redirect: "error",
      referrerPolicy: "no-referrer",
    });
    return readJson(response);
  }

  async start(file, { signal } = {}) {
    if (this.#busy) throw fail("temporary_operation_pending");
    if (!file || !Number.isSafeInteger(file.size) || file.size <= 0) {
      throw fail("temporary_file_invalid");
    }
    this.#busy = true;
    let uploaded = false;
    try {
      this.clear();
      const issued = await this.#request("/v1/editor/temporary-sessions", {
        method: "POST", signal, contentType: "application/json",
        body: JSON.stringify({ expected_byte_len: file.size }),
      });
      if (issued.protocol_version !== TEMPORARY_EDIT_PROTOCOL
          || issued.mode !== "temporary_edit"
          || !SESSION_ID.test(issued.session_id ?? "")
          || !SECRET.test(issued.access_token ?? "")
          || !validExpiry(issued.expires_at_ms)
          || !Number.isSafeInteger(issued.max_file_bytes)
          || file.size > issued.max_file_bytes
          || issued.upload_path !== pathFor(issued.session_id, "content")
          || issued.open_path !== pathFor(issued.session_id, "open")
          || issued.scene_path !== pathFor(issued.session_id, "scene")) {
        throw fail("temporary_issue_contract_invalid");
      }
      const session = {
        schema: TEMPORARY_EDIT_LOCAL_SCHEMA,
        session_id: issued.session_id,
        access_token: issued.access_token,
        expires_at_ms: issued.expires_at_ms,
        source_sha256: null,
      };
      this.#save(session);
      const stored = await this.#request(pathFor(session.session_id, "content"), {
        method: "PUT", body: file, token: session.access_token,
        contentType: "application/octet-stream", signal,
      });
      checkBinding(stored, session.session_id);
      if (stored.state !== "stored") throw fail("temporary_upload_state_invalid");
      uploaded = true;
      session.expires_at_ms = stored.expires_at_ms;
      this.#save(session);
      const opened = await this.#request(pathFor(session.session_id, "open"), {
        method: "POST", body: "{}", contentType: "application/json",
        token: session.access_token, signal,
      });
      checkBinding(opened, session.session_id);
      const view = snapshot(opened);
      session.expires_at_ms = view.expires_at_ms;
      session.source_sha256 = view.source_sha256;
      const restorable = this.#save(session);
      return { ...view, restorable };
    } catch (error) {
      if (!uploaded || [401, 403, 404, 410].includes(error?.status)) this.clear();
      throw error;
    } finally {
      this.#busy = false;
    }
  }

  // Only an explicit confirmed physical DELETE receipt clears local authority.
  // A lost ACK or server 5xx must not be described as completed deletion.
  async deleteSession({ signal } = {}) {
    if (this.#busy) throw fail("temporary_operation_pending");
    const session = this.#restore();
    if (!session) throw fail("temporary_session_missing");
    this.#busy = true;
    try {
      const response = await this.fetchImpl.call(
        globalThis,
        "/v1/editor/temporary-sessions/" + session.session_id,
        {
          method: "DELETE",
          headers: {
            "x-chaptera-reader-session": session.access_token,
            "x-csrf-token": this.csrfToken,
            "accept": "application/json",
          },
          signal,
          credentials: "omit", cache: "no-store", redirect: "error",
          referrerPolicy: "no-referrer",
        },
      );
      if (response?.status !== 204 || !response.ok) {
        throw fail("temporary_delete_not_confirmed", response?.status);
      }
      this.clear();
      return true;
    } finally {
      this.#busy = false;
    }
  }

  async resume({ signal } = {}) {
    if (this.#busy) throw fail("temporary_operation_pending");
    const session = this.#restore();
    if (!session) return { kind: "missing" };
    this.#busy = true;
    try {
      // GET does not renew TTL. If upload was already accepted but the
      // scanner/open handshake was interrupted, retry only that exact open.
      // POST /open is the existing non-renewing Guest Reader transition.
      let reply;
      try {
        reply = await this.#request(pathFor(session.session_id, "scene"), {
          token: session.access_token, signal,
        });
      } catch (error) {
        if (error?.status !== 409 || error?.code !== "guest_scene_not_ready") throw error;
        reply = await this.#request(pathFor(session.session_id, "open"), {
          method: "POST", body: "{}", contentType: "application/json",
          token: session.access_token, signal,
        });
      }
      checkBinding(reply, session.session_id);
      const view = snapshot(reply);
      if (session.source_sha256 && view.source_sha256 !== session.source_sha256) {
        throw fail("temporary_source_changed");
      }
      session.expires_at_ms = view.expires_at_ms;
      session.source_sha256 = view.source_sha256;
      const restorable = this.#save(session);
      return { ...view, restorable };
    } catch (error) {
      if ([401, 403, 404, 410].includes(error?.status)
          || error?.code === "temporary_source_changed"
          || error?.code === "temporary_response_identity_mismatch") this.clear();
      throw error;
    } finally {
      this.#busy = false;
    }
  }
}
