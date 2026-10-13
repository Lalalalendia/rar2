// Browser-only transport for the existing source-bound Guest MoveNode endpoint.
// No private PUB parsing, browser-owned revision, OIDC or editable Scene claim.
import { TEMPORARY_EDIT_STORAGE_KEY } from "./temporary-edit-browser-session-v0.mjs";

const SESSION = /^tempedit:[0-9a-f]{32}$/;
const SECRET = /^[0-9a-f]{64}$/;
const SOURCE = /^[0-9a-f]{64}$/;
const REVISION = /^sha256:[0-9a-f]{64}$/;
const NODE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const CURRENT_PROTOCOL = "chaptera.temporary-editor-current.v1";
const ACCEPTED_PROTOCOL = "chaptera.temporary-editor-move-node.v1";
const REQUEST_PROTOCOL = "chaptera.commit-request.v1";
const LOCAL_SCHEMA = "chaptera.temporary-edit.browser.v0";

function failure(code, status) {
  const error = new Error(code);
  error.code = code;
  if (status !== undefined) error.status = status;
  return error;
}
function safeUnsigned(value) {
  return Number.isSafeInteger(value) && value >= 0;
}
function safePosition(value) {
  return Number.isSafeInteger(value);
}
function object(value) {
  return value && typeof value === "object" && !Array.isArray(value);
}
function defaultStorage() {
  try { return globalThis.sessionStorage ?? null; } catch { return null; }
}
function recordFromTab(storage) {
  let raw;
  try { raw = storage?.getItem(TEMPORARY_EDIT_STORAGE_KEY); } catch { return null; }
  if (!raw || raw.length > 4096) return null;
  let record;
  try { record = JSON.parse(raw); } catch { return null; }
  return record?.schema === LOCAL_SCHEMA && SESSION.test(record.session_id ?? "")
    && SECRET.test(record.access_token ?? "") && SOURCE.test(record.source_sha256 ?? "")
    && safeUnsigned(record.expires_at_ms) && record.expires_at_ms > 0
    ? record : null;
}
function newOperationId(cryptoObject) {
  if (typeof cryptoObject?.getRandomValues !== "function") {
    throw failure("temporary_secure_randomness_required");
  }
  const bytes = cryptoObject.getRandomValues(new Uint8Array(16));
  return "temp-move-" + [...bytes].map(x => x.toString(16).padStart(2, "0")).join("");
}

export class TemporaryGuestMoveClientV1 {
  #sessionId;
  #token;
  #sourceHash;
  #csrf;
  #fetch;
  #operationIdFactory;
  #current = null;
  #pending = null;
  #busy = false;
  #refreshRequired = false;

  static fromTab({ storage = defaultStorage(), csrfToken, fetchImpl,
    cryptoObject = globalThis.crypto, operationIdFactory } = {}) {
    const record = recordFromTab(storage);
    if (!record) throw failure("temporary_edit_capability_missing");
    return new TemporaryGuestMoveClientV1({
      sessionId: record.session_id, accessToken: record.access_token,
      sourceHash: record.source_sha256, csrfToken, fetchImpl, cryptoObject,
      operationIdFactory,
    });
  }

  constructor({ sessionId, accessToken, sourceHash, csrfToken,
    fetchImpl = globalThis.fetch, cryptoObject = globalThis.crypto,
    operationIdFactory = () => newOperationId(cryptoObject) } = {}) {
    if (!SESSION.test(sessionId ?? "") || !SECRET.test(accessToken ?? "") ||
        !SOURCE.test(sourceHash ?? "") || !/^[0-9a-f]{32}$/.test(csrfToken ?? "") ||
        typeof fetchImpl !== "function" || typeof operationIdFactory !== "function") {
      throw failure("temporary_move_client_config_invalid");
    }
    this.#sessionId = sessionId;
    this.#token = accessToken;
    this.#sourceHash = sourceHash;
    this.#csrf = csrfToken;
    this.#fetch = fetchImpl;
    this.#operationIdFactory = operationIdFactory;
  }

  // No bearer or raw PUB contents are ever exposed by status introspection.
  get status() {
    return {
      document_id: this.#sessionId,
      source_hash: this.#sourceHash,
      revision_id: this.#current?.revision_id ?? null,
      expires_at_ms: this.#current?.expires_at_ms ?? null,
      pending_reconciliation: this.#pending !== null,
      refresh_required: this.#refreshRequired,
      busy: this.#busy,
    };
  }

  #path(action) {
    return "/v1/editor/temporary-sessions/" + this.#sessionId + "/" + action;
  }

  async #request(action, method = "GET", body = null) {
    const response = await this.#fetch(this.#path(action), {
      method,
      headers: {
        "accept": "application/json",
        "x-chaptera-reader-session": this.#token,
        "x-csrf-token": this.#csrf,
        ...(body ? { "content-type": "application/json" } : {}),
      },
      ...(body ? { body: JSON.stringify(body) } : {}),
      credentials: "omit", mode: "same-origin", cache: "no-store",
      redirect: "error", referrerPolicy: "no-referrer",
    });
    if (!response?.ok) {
      const status = Number.isSafeInteger(response?.status) ? response.status : undefined;
      throw failure("temporary_move_request_rejected", status);
    }
    let data;
    try { data = await response.json(); } catch {
      throw failure("temporary_move_response_invalid");
    }
    if (!object(data)) throw failure("temporary_move_response_invalid");
    return data;
  }

  #checkedCurrent(value) {
    if (value.protocol_version !== CURRENT_PROTOCOL ||
        value.document_id !== this.#sessionId || value.source_hash !== this.#sourceHash ||
        !REVISION.test(value.revision_id ?? "") ||
        !SOURCE.test(value.canonical_authoring_revision_id ?? "") ||
        !safeUnsigned(value.revision_cursor) ||
        !safeUnsigned(value.expires_at_ms) || value.expires_at_ms === 0 ||
        !object(value.project) || !object(value.authoring_graph) ||
        !object(value.authoring_graph.nodes)) {
      throw failure("temporary_move_current_identity_invalid");
    }
    return value;
  }

  async refresh() {
    if (this.#pending) throw failure("temporary_move_reconciliation_required");
    if (this.#busy) throw failure("temporary_move_busy");
    this.#busy = true;
    try {
      const current = this.#checkedCurrent(await this.#request("current"));
      this.#current = current;
      this.#refreshRequired = false;
      return current;
    } catch (error) {
      this.#refreshRequired = true;
      throw error;
    } finally { this.#busy = false; }
  }

  #checkedAck(ack, request, previous) {
    if (ack.protocol_version !== ACCEPTED_PROTOCOL ||
        ack.document_id !== this.#sessionId || ack.source_hash !== this.#sourceHash ||
        ack.base_revision_id !== request.base_revision_id ||
        ack.client_operation_id !== request.client_operation_id ||
        !REVISION.test(ack.revision_id ?? "") ||
        ack.revision_id === request.base_revision_id ||
        !SOURCE.test(ack.canonical_authoring_revision_id ?? "") ||
        !safeUnsigned(ack.expires_at_ms) || ack.expires_at_ms < previous.expires_at_ms ||
        typeof ack.replayed !== "boolean" || ack.scene_refresh !== "current" ||
        !object(ack.canonical_operation)) {
      throw failure("temporary_move_ack_identity_invalid");
    }
    return ack;
  }

  async #finishPending() {
    const pending = this.#pending;
    if (!pending) throw failure("temporary_move_nothing_to_reconcile");
    let rawAck;
    try { rawAck = await this.#request("move-node", "POST", pending.request); }
    catch (error) {
      // Only this POST rejection is conclusive. A later GET 4xx after a
      // successful POST does NOT prove that the edit failed.
      if (Number.isInteger(error?.status) && error.status >= 400 && error.status < 500) {
        pending.postRejected = true;
      }
      throw error;
    }
    const ack = this.#checkedAck(rawAck, pending.request, pending.before);
    if (pending.ack && (pending.ack.revision_id !== ack.revision_id ||
        pending.ack.canonical_authoring_revision_id !== ack.canonical_authoring_revision_id ||
        pending.ack.expires_at_ms !== ack.expires_at_ms)) {
      throw failure("temporary_move_ack_changed_on_replay");
    }
    pending.ack = ack;
    const current = this.#checkedCurrent(await this.#request("current"));
    const node = current.authoring_graph.nodes[pending.request.command.node_id];
    if (current.revision_id !== ack.revision_id ||
        current.canonical_authoring_revision_id !== ack.canonical_authoring_revision_id ||
        current.revision_cursor !== pending.before.revision_cursor + 1 ||
        current.expires_at_ms !== ack.expires_at_ms ||
        !object(node?.header?.bounds) ||
        node.header.bounds.x !== pending.request.command.x_emu ||
        node.header.bounds.y !== pending.request.command.y_emu) {
      throw failure("temporary_move_reopened_state_mismatch");
    }
    this.#current = current;
    this.#pending = null;
    this.#refreshRequired = false;
    return { accepted: ack, current };
  }

  // Exact revised Reader Scene; never substitute the original Guest Reader
  // upload scene after a mutation or infer geometry in the browser.
  async readerScene() {
    if (this.#busy || this.#pending || this.#refreshRequired || !this.#current) {
      throw failure("temporary_scene_current_required");
    }
    const head = this.#current;
    let response;
    try { response = await this.#request("scene"); }
    catch (error) {
      this.#refreshRequired = true;
      throw error;
    }
    if (response.protocol_version !== "chaptera.temporary-editor-scene.v1" ||
        response.document_id !== this.#sessionId ||
        response.source_hash !== this.#sourceHash ||
        response.revision_id !== head.revision_id ||
        response.expires_at_ms !== head.expires_at_ms ||
        response.scene?.protocol_version !== "chaptera.reader-scene.v1" ||
        response.scene.document_id !== this.#sessionId ||
        response.scene.source_hash !== this.#sourceHash ||
        response.scene.revision_id !== head.revision_id ||
        !Array.isArray(response.scene.pages) ||
        !Array.isArray(response.scene.nodes)) {
      this.#refreshRequired = true;
      throw failure("temporary_scene_identity_mismatch");
    }
    return response.scene;
  }

  // A private edited-revision report or physical download, never original PUB.
  // Only query-bound public revision identities appear in the URL, never bearer.
  async exportEdited(target, { reportOnly = false } = {}) {
    if (!["idml", "odg"].includes(target)) {
      throw failure("temporary_export_target_invalid");
    }
    if (this.#busy || this.#pending || this.#refreshRequired ||
        !this.#current || this.#current.revision_cursor === 0) {
      throw failure("temporary_export_current_required");
    }
    const head = this.#current;
    const query = new URLSearchParams({
      revision_id: head.revision_id,
      canonical_authoring_revision_id: head.canonical_authoring_revision_id,
      report_only: String(reportOnly),
    });
    const response = await this.#fetch(
      this.#path("exports/" + target) + "?" + query.toString(),
      {
        method: "GET",
        headers: {
          accept: reportOnly ? "application/json" : "application/octet-stream",
          "x-chaptera-reader-session": this.#token,
          "x-csrf-token": this.#csrf,
        },
        credentials: "omit", mode: "same-origin", cache: "no-store",
        redirect: "error", referrerPolicy: "no-referrer",
      },
    );
    if (!response?.ok) {
      this.#refreshRequired = true;
      throw failure("temporary_export_request_rejected", response?.status);
    }
    if (reportOnly) {
      let report;
      try { report = await response.json(); }
      catch { throw failure("temporary_export_report_invalid"); }
      if (report?.protocol_version !== "chaptera.temporary-edited-export-report.v1" ||
          report.revision_id !== head.revision_id ||
          report.canonical_authoring_revision_id !== head.canonical_authoring_revision_id ||
          report.source_hash !== this.#sourceHash ||
          report.target !== target || !SECRET.test(report.artifact_sha256 ?? "") ||
          !object(report.loss_report)) {
        this.#refreshRequired = true;
        throw failure("temporary_export_report_identity_mismatch");
      }
      return report;
    }
    const expectedType = target === "idml"
      ? "application/vnd.adobe.indesign-idml-package"
      : "application/vnd.oasis.opendocument.graphics";
    const expectedHash = response.headers?.get("x-chaptera-export-sha256");
    if (response.headers?.get("content-type") !== expectedType ||
        response.headers?.get("x-chaptera-edit-revision") !== head.revision_id ||
        !SECRET.test(expectedHash ?? "")) {
      this.#refreshRequired = true;
      throw failure("temporary_export_download_identity_mismatch");
    }
    const blob = await response.blob();
    if (blob.size <= 0) {
      throw failure("temporary_export_download_empty");
    }
    const subtle = globalThis.crypto?.subtle;
    if (!subtle || typeof subtle.digest !== "function") {
      throw failure("temporary_export_crypto_unavailable");
    }
    const digest = new Uint8Array(await subtle.digest("SHA-256", await blob.arrayBuffer()));
    const actualHash = [...digest].map(b => b.toString(16).padStart(2, "0")).join("");
    if (actualHash !== expectedHash) {
      throw failure("temporary_export_hash_mismatch");
    }
    return { blob, target, sha256: actualHash, revision_id: head.revision_id };
  }

  async moveNode({ nodeId, xEmu, yEmu } = {}) {
    if (this.#busy) throw failure("temporary_move_busy");
    if (this.#pending) throw failure("temporary_move_reconciliation_required");
    if (this.#refreshRequired || !this.#current) throw failure("temporary_move_current_required");
    const node = this.#current.authoring_graph.nodes[nodeId];
    if (!NODE.test(nodeId ?? "") || !object(node?.header?.bounds) ||
        !safePosition(node.header.bounds.x) || !safePosition(node.header.bounds.y) ||
        !safePosition(xEmu) || !safePosition(yEmu) ||
        (xEmu === node.header.bounds.x && yEmu === node.header.bounds.y)) {
      throw failure("temporary_move_intent_invalid");
    }
    const clientOperationId = this.#operationIdFactory();
    if (typeof clientOperationId !== "string" ||
        !/^[a-zA-Z0-9._:-]{1,160}$/.test(clientOperationId)) {
      throw failure("temporary_move_operation_id_invalid");
    }
    this.#pending = {
      before: this.#current,
      request: {
        protocol_version: REQUEST_PROTOCOL,
        document_id: this.#sessionId,
        source_hash: this.#sourceHash,
        base_revision_id: this.#current.revision_id,
        client_operation_id: clientOperationId,
        command: { kind: "move_node_to", node_id: nodeId, x_emu: xEmu, y_emu: yEmu },
      },
      ack: null,
      postRejected: false,
    };
    this.#busy = true;
    try {
      return await this.#finishPending();
    } catch (error) {
      // A 4xx is an explicit rejection, not a lost ACK. Require an exact
      // new Current before the caller can submit a different operation.
      if (this.#pending?.postRejected === true) {
        this.#pending = null;
        this.#refreshRequired = true;
      }
      throw error;
    } finally { this.#busy = false; }
  }

  async reconcileUnknown() {
    if (!this.#pending) throw failure("temporary_move_nothing_to_reconcile");
    if (this.#busy) throw failure("temporary_move_busy");
    this.#busy = true;
    try { return await this.#finishPending(); }
    finally { this.#busy = false; }
  }
}
