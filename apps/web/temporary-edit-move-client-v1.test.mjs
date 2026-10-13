import test from "node:test";
import assert from "node:assert/strict";
import { TemporaryGuestMoveClientV1 } from "./temporary-edit-move-client-v1.mjs";
import { TEMPORARY_EDIT_STORAGE_KEY } from "./temporary-edit-browser-session-v0.mjs";

const session = "tempedit:" + "a".repeat(32);
const bearer = "b".repeat(64);
const source = "c".repeat(64);
const csrf = "e".repeat(32);
const nodeId = "01234567-89ab-4def-8abc-123456789abc";
const beforeRev = "sha256:" + "d".repeat(64);
const afterRev = "sha256:" + "f".repeat(64);
const beforeCanon = "e".repeat(64);
const afterCanon = "1".repeat(64);
const oldExpiry = 1_000_000;
const newExpiry = 4_600_000;
const before = {
  protocol_version: "chaptera.temporary-editor-current.v1",
  document_id: session, source_hash: source,
  revision_id: beforeRev, revision_cursor: 0,
  canonical_authoring_revision_id: beforeCanon, expires_at_ms: oldExpiry,
  project: { schema_version: "editor" },
  authoring_graph: { nodes: { [nodeId]: { header: { bounds: { x: 100, y: 200, width: 10, height: 20 } } } } },
};
const after = {
  ...before, revision_id: afterRev, revision_cursor: 1,
  canonical_authoring_revision_id: afterCanon, expires_at_ms: newExpiry,
  project: { schema_version: "editor", events: ["accepted"] },
  authoring_graph: { nodes: { [nodeId]: { header: { bounds: { x: 150, y: 230, width: 10, height: 20 } } } } },
};
function accepted(replayed = false) {
  return {
    protocol_version: "chaptera.temporary-editor-move-node.v1",
    document_id: session, source_hash: source, base_revision_id: beforeRev,
    client_operation_id: "test-move-1", canonical_authoring_revision_id: afterCanon,
    revision_id: afterRev, expires_at_ms: newExpiry, replayed,
    scene_refresh: "current", canonical_operation: { move_node: { node_id: nodeId } },
  };
}
function reply(body, code = 200) {
  return new Response(JSON.stringify(body), {
    status: code, headers: { "content-type": "application/json" },
  });
}
function harness(responses) {
  const requests = [];
  const feed = [...responses];
  const fetchImpl = async (url, options) => {
    requests.push({ url, options });
    const next = feed.shift();
    if (next instanceof Error) throw next;
    if (!next) throw Error("test ran out of scripted responses");
    return Array.isArray(next) ? reply(...next) : reply(next);
  };
  const client = new TemporaryGuestMoveClientV1({
    sessionId: session, accessToken: bearer, sourceHash: source, csrfToken: csrf,
    fetchImpl, operationIdFactory: () => "test-move-1",
  });
  return { client, requests };
}

const move = { nodeId, xEmu: 150, yEmu: 230 };

test("private tab capability is reused only from valid existing browser record", async () => {
  const record = {
    schema: "chaptera.temporary-edit.browser.v0", session_id: session,
    access_token: bearer, source_sha256: source, expires_at_ms: oldExpiry,
  };
  const storage = { getItem: key => key === TEMPORARY_EDIT_STORAGE_KEY ? JSON.stringify(record) : null };
  const fetchImpl = async () => reply(before);
  const client = TemporaryGuestMoveClientV1.fromTab({ storage, csrfToken: csrf, fetchImpl });
  const current = await client.refresh();
  assert.equal(current.revision_id, beforeRev);
  assert.equal(client.status.pending_reconciliation, false);
  assert.doesNotMatch(JSON.stringify(client.status), new RegExp(bearer));
  assert.throws(() => TemporaryGuestMoveClientV1.fromTab({
    storage: { getItem: () => JSON.stringify({ ...record, source_sha256: "wrong" }) },
    csrfToken: csrf, fetchImpl,
  }), /temporary_edit_capability_missing/);
});

test("real guest MoveNode sends one authorized source/revision-bound POST, reopens exact changed graph", async () => {
  const { client, requests } = harness([before, accepted(), after]);
  await client.refresh();
  const receipt = await client.moveNode(move);
  assert.equal(receipt.current.authoring_graph.nodes[nodeId].header.bounds.x, 150);
  assert.equal(client.status.revision_id, afterRev);
  assert.equal(client.status.expires_at_ms, newExpiry);
  assert.deepEqual(requests.map(x => [x.options.method, x.url]), [
    ["GET", "/v1/editor/temporary-sessions/" + session + "/current"],
    ["POST", "/v1/editor/temporary-sessions/" + session + "/move-node"],
    ["GET", "/v1/editor/temporary-sessions/" + session + "/current"],
  ]);
  const intent = JSON.parse(requests[1].options.body);
  assert.deepEqual(intent, {
    protocol_version: "chaptera.commit-request.v1",
    document_id: session, source_hash: source, base_revision_id: beforeRev,
    client_operation_id: "test-move-1",
    command: { kind: "move_node_to", node_id: nodeId, x_emu: 150, y_emu: 230 },
  });
  for (const { url, options } of requests) {
    assert.equal(options.headers["x-chaptera-reader-session"], bearer);
    assert.equal(options.headers["x-csrf-token"], csrf);
    assert.equal(options.credentials, "omit");
    assert.equal(options.mode, "same-origin");
    assert.equal(options.cache, "no-store");
    assert.equal(options.redirect, "error");
    assert.equal(options.referrerPolicy, "no-referrer");
    assert.ok(!url.includes(bearer));
    assert.ok(!JSON.stringify(options.body ?? "").includes(bearer));
  }
});

test("unknown ACK locks every new mutation until exactly the same operation ID reconciles", async () => {
  const { client, requests } = harness([before, new TypeError("network disconnected"), accepted(true), after]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), /network disconnected/);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(client.moveNode({ ...move, xEmu: 160 }), /temporary_move_reconciliation_required/);
  await assert.rejects(client.refresh(), /temporary_move_reconciliation_required/);
  const restored = await client.reconcileUnknown();
  assert.equal(restored.accepted.replayed, true);
  assert.equal(client.status.pending_reconciliation, false);
  const posts = requests.filter(x => x.options.method === "POST");
  assert.equal(posts.length, 2);
  assert.equal(posts[0].options.body, posts[1].options.body);
});

test("explicit 409 rejection fences next edit until fresh authoritative current", async () => {
  const { client, requests } = harness([before, [{ error: "temporary_editor_stale_revision" }, 409], before]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), error => error.status === 409);
  assert.equal(client.status.pending_reconciliation, false);
  assert.equal(client.status.refresh_required, true);
  await assert.rejects(client.moveNode(move), /temporary_move_current_required/);
  await client.refresh();
  assert.equal(client.status.refresh_required, false);
  assert.equal(requests.length, 3);
});

test("forged source/current and ACK identities fail closed, no mutation unlocked", async () => {
  const { client: wrong, requests: onlyRead } = harness([{ ...before, source_hash: "a".repeat(64) }]);
  await assert.rejects(wrong.refresh(), /temporary_move_current_identity_invalid/);
  await assert.rejects(wrong.moveNode(move), /temporary_move_current_required/);
  assert.equal(onlyRead.length, 1);
  const { client } = harness([before, { ...accepted(), client_operation_id: "forged" }, accepted(true), after]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), /temporary_move_ack_identity_invalid/);
  assert.equal(client.status.pending_reconciliation, true);
  await client.reconcileUnknown();
  assert.equal(client.status.revision_id, afterRev);
});

test("reopened geometry mismatch keeps mutation fenced after a successful ACK", async () => {
  const { client } = harness([before, accepted(), { ...after, authoring_graph: before.authoring_graph }, accepted(true), after]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), /temporary_move_reopened_state_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(client.moveNode(move), /temporary_move_reconciliation_required/);
  await client.reconcileUnknown();
  assert.equal(client.status.pending_reconciliation, false);
});

test("unsupported source NodeId, no-op and unsafe coordinates never hit HTTP POST", async () => {
  const { client, requests } = harness([before]);
  await client.refresh();
  await assert.rejects(client.moveNode({ ...move, nodeId: "invalid" }), /temporary_move_intent_invalid/);
  await assert.rejects(client.moveNode({ nodeId, xEmu: 100, yEmu: 200 }), /temporary_move_intent_invalid/);
  await assert.rejects(client.moveNode({ nodeId, xEmu: Number.MAX_SAFE_INTEGER + 1, yEmu: 230 }), /temporary_move_intent_invalid/);
  assert.equal(requests.length, 1);
});

test("invalid post server body does not leak private error and locks until reconciliation", async () => {
  const { client } = harness([before, [{ error: "private PUB bytes SHOULD_NOT_EXPOSE" }, 500]]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), /temporary_move_request_rejected/);
  assert.equal(client.status.pending_reconciliation, true);
  assert.doesNotMatch(JSON.stringify(client.status), /SHOULD_NOT_EXPOSE/);
});

test("4xx from post-acceptance GET does not unlock an already committed edit", async () => {
  const { client } = harness([before, accepted(), [{ error: "guest_session_expired" }, 410]]);
  await client.refresh();
  await assert.rejects(client.moveNode(move), error => error.status === 410);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(client.refresh(), /temporary_move_reconciliation_required/);
});


test("edited PDF remains revision-bound and fails closed without server PDF support", async () => {
  const { client, requests } = harness([after, [{error: "temporary_editor_export_request_invalid"}, 400]]);
  await client.refresh();
  await assert.rejects(client.exportEdited("pdf"), error => error.status === 400);
  assert.equal(client.status.refresh_required, true);
  assert.equal(requests.length, 2);
  assert.ok(requests[1].url.includes("/exports/pdf?"));
  assert.equal(requests[1].options.headers.accept, "application/pdf");
  assert.ok(!requests[1].url.includes(bearer));
});

test("PDF report does not trust a foreign authoring revision", async () => {
  const { client } = harness([after, {
    protocol_version: "chaptera.temporary-edited-export-report.v1",
    revision_id: afterRev, canonical_authoring_revision_id: beforeCanon,
    source_hash: source, target: "pdf", artifact_sha256: "e".repeat(64),
    loss_report: {counts: {blocking: 0}},
  }]);
  await client.refresh();
  await assert.rejects(client.exportEdited("pdf", {reportOnly: true}), /temporary_export_report_identity_mismatch/);
  assert.equal(client.status.refresh_required, true);
});

test("PDF download rejects forged signature even with matching HTTP digest", async () => {
  const actual = new TextEncoder().encode("not-a-PDF");
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", actual));
  const hash = [...digest].map(b => b.toString(16).padStart(2, "0")).join("");
  const { client } = harness([after]);
  await client.refresh();
  const fakeResponse = new Response(actual, {headers: {
    "content-type": "application/pdf",
    "x-chaptera-edit-revision": afterRev,
    "x-chaptera-export-sha256": hash,
  }});
  // A separately provisioned client proves the PDF signature is checked in the download path.
  const download = new TemporaryGuestMoveClientV1({
    sessionId: session, accessToken: bearer, sourceHash: source, csrfToken: csrf,
    fetchImpl: (() => {let n = 0;return async () => ++n === 1 ? reply(after) : fakeResponse;})(),
    operationIdFactory: () => "test-move-1",
  });
  await download.refresh();
  await assert.rejects(download.exportEdited("pdf"), /temporary_export_pdf_signature_invalid/);
});
