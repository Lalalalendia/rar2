// Private guest-only canonical MoveNode transport. Never stores the bearer in a URL.
import { MoveGestureV1 } from "./interaction-v1.mjs";
import { projectReaderSceneToEditorInteractionScene } from "./reader-scene-editor-interaction-v1.mjs";

const SESSION = /^tempedit:[0-9a-f]{32}$/;
const HEX = /^[0-9a-f]{64}$/;
const REV = /^sha256:[0-9a-f]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function requireGuestCurrent(current, sessionId, sourceHash) {
  if (!SESSION.test(sessionId) || !HEX.test(sourceHash)
      || current?.protocol_version !== "chaptera.temporary-editor-current.v1"
      || current.document_id !== sessionId || current.source_hash !== sourceHash
      || !REV.test(current.revision_id) || !Number.isSafeInteger(current.expires_at_ms)) {
    throw new Error("temporary_current_identity_mismatch");
  }
  return current;
}

export function guestMoveRequest(current, scene, nodeId, point, operationId) {
  if (!UUID.test(nodeId) || !/^[a-zA-Z0-9._:-]{1,160}$/.test(operationId)) {
    throw new Error("temporary_move_identity_invalid");
  }
  if (scene?.document_id !== current.document_id || scene?.source_hash !== current.source_hash
      || scene?.revision_id !== current.revision_id) {
    throw new Error("temporary_scene_revision_mismatch");
  }
  const interaction = projectReaderSceneToEditorInteractionScene(scene);
  const node = interaction.nodes.find(n => n.node_id === nodeId);
  if (!node || node.parent_node_id !== null) throw new Error("temporary_node_unsupported");
  const gesture = new MoveGestureV1(interaction, nodeId, {
    x_emu: node.bounds.x, y_emu: node.bounds.y,
  });
  gesture.update(point);
  if (point.x_emu === node.bounds.x && point.y_emu === node.bounds.y) return null;
  return gesture.commit(operationId);
}

export class TemporaryGuestMoveTransport {
  constructor({ sessionId, token, sourceHash, csrfToken, fetchImpl = globalThis.fetch }) {
    if (!SESSION.test(sessionId) || !HEX.test(token) || !HEX.test(sourceHash)
        || !/^[0-9a-f]{32}$/.test(csrfToken) || typeof fetchImpl !== "function") {
      throw new Error("temporary_guest_client_invalid");
    }
    Object.assign(this, { sessionId, token, sourceHash, csrfToken, fetchImpl });
    this.pending = null;
  }

  async request(suffix, method = "GET", body) {
    const url = "/v1/editor/temporary-sessions/" + this.sessionId + "/" + suffix;
    const response = await this.fetchImpl(url, {
      method, body: body === undefined ? undefined : JSON.stringify(body),
      headers: {
        accept: "application/json",
        "x-chaptera-reader-session": this.token,
        "x-csrf-token": this.csrfToken,
        ...(body === undefined ? {} : { "content-type": "application/json" }),
      },
      credentials: "omit", cache: "no-store", redirect: "error",
      referrerPolicy: "no-referrer",
    });
    if (!response.ok) throw new Error("temporary_guest_request_failed_" + response.status);
    return response.json();
  }

  async current() {
    return requireGuestCurrent(await this.request("current"), this.sessionId, this.sourceHash);
  }

  async move(current, scene, nodeId, point, operationId) {
    if (this.pending) throw new Error("temporary_move_in_flight");
    const request = guestMoveRequest(
      requireGuestCurrent(current, this.sessionId, this.sourceHash),
      scene, nodeId, point, operationId,
    );
    if (!request) return null;
    this.pending = request;
    try {
      const accepted = await this.request("move-node", "POST", request);
      if (accepted?.protocol_version !== "chaptera.temporary-editor-move-node.v1"
          || accepted.document_id !== this.sessionId
          || accepted.source_hash !== this.sourceHash
          || accepted.base_revision_id !== request.base_revision_id
          || accepted.client_operation_id !== operationId
          || !REV.test(accepted.revision_id)
          || accepted.revision_id === request.base_revision_id) {
        throw new Error("temporary_move_ack_mismatch");
      }
      const fresh = await this.current();
      if (fresh.revision_id !== accepted.revision_id) {
        throw new Error("temporary_move_reconcile_required");
      }
      this.pending = null;
      return { accepted, current: fresh };
    } catch (error) {
      // Preserve the same operation ID after a lost ACK; never create a second revision.
      throw error;
    }
  }
}
