import test from "node:test";
import assert from "node:assert/strict";
import { TemporaryGuestMoveClientV1 } from "./temporary-edit-move-client-v1.mjs";

const session = "tempedit:" + "a".repeat(32);
const source = "b".repeat(64);
const bearer = "c".repeat(64);
const csrf = "e".repeat(32);
const page = "01234567-89ab-4def-8abc-123456789abc";
const authored = "019a2d80-1112-7001-8001-aabbccddee01";
const oldRev = "sha256:" + "d".repeat(64);
const newRev = "sha256:" + "f".repeat(64);
const oldCanonical = "0".repeat(64);
const newCanonical = "1".repeat(64);
const oldExpiry = 1_000_000, newExpiry = 4_600_000;
const bounds = { x: 100_000, y: 200_000, width: 300_000, height: 400_000 };
const command = {
  kind: "create_shape", page_id: page,
  x_emu: bounds.x, y_emu: bounds.y,
  width_emu: bounds.width, height_emu: bounds.height,
  fill_rgb: [18, 52, 86], stroke_rgb: [170, 187, 204], stroke_width_emu: 12_700,
};
const operation = {
  kind: "create_shape", node_id: authored, page_id: page, parent_id: page,
  shape_kind: "rectangle", bounds,
  transform: { kind: "identity" },
  paint: {
    fill: { visible: true, color: { r: 18, g: 52, b: 86 } },
    stroke: { visible: true, color: { r: 170, g: 187, b: 204 }, width_emu: 12_700 },
    provenance: { kind: "author_created" },
  },
  provenance: { kind: "author_created" },
};
const current = {
  protocol_version: "chaptera.temporary-editor-current.v1",
  document_id: session, source_hash: source,
  revision_id: oldRev, revision_cursor: 0,
  canonical_authoring_revision_id: oldCanonical, expires_at_ms: oldExpiry,
  project: { operations: [] },
  authoring_graph: {
    nodes: {}, pages: { [page]: { size: { width: 3_000_000, height: 2_000_000 } } },
  },
};
const after = {
  ...current, revision_id: newRev, revision_cursor: 1,
  canonical_authoring_revision_id: newCanonical, expires_at_ms: newExpiry,
  project: { operations: [operation] },
};
const accepted = {
  protocol_version: "chaptera.temporary-editor-create-shape.v1",
  document_id: session, source_hash: source, base_revision_id: oldRev,
  client_operation_id: "shape-op-1", canonical_authoring_revision_id: newCanonical,
  revision_id: newRev, expires_at_ms: newExpiry,
  replayed: false, scene_refresh: "current", canonical_operation: operation,
};
const reply = (json, status = 200) => new Response(JSON.stringify(json), {
  status, headers: { "content-type": "application/json" },
});
function harness(replies) {
  const calls = [], messages = [...replies];
  const client = new TemporaryGuestMoveClientV1({
    sessionId: session, sourceHash: source, accessToken: bearer,
    csrfToken: csrf, operationIdFactory: () => "shape-op-1",
    fetchImpl: async (url, options) => {
      calls.push({url, options});
      const next = messages.shift();
      if (next instanceof Error) throw next;
      if (!next) throw new Error("scripted reply missing");
      return Array.isArray(next) ? reply(...next) : reply(next);
    },
  });
  return {client, calls};
}
const create = client => client.createRectangle({pageId: page, bounds});

test("CreateShape private POST reopens exact authored rectangle and never supplies NodeId", async () => {
  const {client, calls} = harness([current, accepted, after]);
  assert.equal((await client.refresh()).revision_id, oldRev);
  const result = await create(client);
  assert.equal(result.accepted.canonical_operation.node_id, authored);
  assert.equal(result.current.project.operations.length, 1);
  assert.equal(client.status.pending_reconciliation, false);
  assert.deepEqual(calls.map(call => [call.url, call.options.method]), [
    ["/v1/editor/temporary-sessions/" + session + "/current", "GET"],
    ["/v1/editor/temporary-sessions/" + session + "/create-shape", "POST"],
    ["/v1/editor/temporary-sessions/" + session + "/current", "GET"],
  ]);
  const body = JSON.parse(calls[1].options.body);
  assert.deepEqual(body.command, command);
  assert.equal(body.client_operation_id, "shape-op-1");
  assert.equal(body.base_revision_id, oldRev);
  assert.equal("node_id" in body.command, false);
  assert.equal(calls[1].options.headers["x-chaptera-reader-session"], bearer);
  assert.equal(calls[1].options.headers["x-csrf-token"], csrf);
  assert.ok(!calls[1].url.includes(bearer));
  assert.equal(calls[1].options.credentials, "omit");
});

test("CreateShape refuses unknown page, nonpositive/overflow geometry and invalid paint before HTTP", async () => {
  const {client, calls} = harness([current]);
  await client.refresh();
  const variants = [
    {pageId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", bounds},
    {pageId: page, bounds: {...bounds, width: 0}},
    {pageId: page, bounds: {...bounds, y: -1}},
    {pageId: page, bounds: {...bounds, x: 2_900_000}},
    {pageId: page, bounds: {...bounds, x: Number.MAX_SAFE_INTEGER}},
    {pageId: page, bounds, fillRgb: [18, -1, 86]},
    {pageId: page, bounds, strokeRgb: [1, 2]},
    {pageId: page, bounds, strokeWidthEmu: 0},
    {pageId: page, bounds, strokeWidthEmu: 1_000_001},
  ];
  for (const data of variants) {
    await assert.rejects(client.createRectangle(data), /temporary_create_shape_intent_invalid/);
  }
  assert.equal(calls.length, 1, "invalid intent cannot POST");
});

test("CreateShape rejects forged ACK fields and fences a second command", async () => {
  const altered = {...accepted, canonical_operation: {
    ...operation, paint: {...operation.paint,
      fill: {...operation.paint.fill, color: {r: 4, g: 5, b: 6}}},
  }};
  const {client, calls} = harness([current, altered]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_create_shape_ack_operation_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(create(client), /temporary_move_reconciliation_required/);
  assert.equal(calls.length, 2);
});

test("CreateShape rejects forged Current project operation after a valid ACK", async () => {
  const forged = {...after, project: {operations: [{...operation, node_id:
    "019a2d80-1112-7001-8001-aabbccddee99"}]}};
  const {client, calls} = harness([current, accepted, forged]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_move_reopened_state_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  assert.equal(calls.length, 3);
});

test("CreateShape lost ACK replays identical operation ID and never mints a browser NodeId", async () => {
  const {client, calls} = harness([
    current, new Error("network reset"), {...accepted, replayed: true}, after,
  ]);
  await client.refresh();
  await assert.rejects(create(client), /network reset/);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(create(client), /temporary_move_reconciliation_required/);
  const result = await client.reconcileUnknown();
  assert.equal(result.accepted.replayed, true);
  assert.equal(result.current.revision_id, newRev);
  const sent = calls.filter(call => call.options.method === "POST")
    .map(call => JSON.parse(call.options.body));
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[0], sent[1]);
  assert.equal(sent[0].client_operation_id, "shape-op-1");
  assert.ok(!("node_id" in sent[0].command));
  assert.equal(client.status.pending_reconciliation, false);
});

test("CreateShape 409 rejects the POST without replaying mutation or renewing locally", async () => {
  const {client, calls} = harness([current, [{error: "stale_revision"}, 409]]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_move_request_rejected/);
  assert.equal(client.status.pending_reconciliation, false);
  assert.equal(client.status.refresh_required, true);
  await assert.rejects(create(client), /temporary_move_current_required/);
  assert.equal(calls.length, 2);
});
