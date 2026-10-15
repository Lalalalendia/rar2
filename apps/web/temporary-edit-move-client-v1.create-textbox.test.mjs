import test from "node:test";
import assert from "node:assert/strict";
import {
  TemporaryGuestMoveClientV1, temporaryPlainStoryCandidates,
  temporaryStoryTextForTextarea, temporaryStoryObservedToCanonicalStory,
} from "./temporary-edit-move-client-v1.mjs";

const session = "tempedit:" + "a".repeat(32);
const source = "b".repeat(64);
const bearer = "c".repeat(64);
const csrf = "e".repeat(32);
const page = "01234567-89ab-4def-8abc-123456789abc";
const authored = "019a2d80-1112-7001-8001-aabbccddee01";
const story = "019a2d80-1112-7001-8001-aabbccddee02";
const oldRev = "sha256:" + "d".repeat(64);
const newRev = "sha256:" + "f".repeat(64);
const oldCanonical = "0".repeat(64);
const newCanonical = "1".repeat(64);
const oldExpiry = 1_000_000, newExpiry = 4_600_000;
const bounds = { x: 100_000, y: 200_000, width: 300_000, height: 400_000 };
const command = {
  kind: "create_text_box", page_id: page,
  x_emu: bounds.x, y_emu: bounds.y,
  width_emu: bounds.width, height_emu: bounds.height,
};
const operation = {
  kind: "create_text_box", node_id: authored, story_id: story, page_id: page, bounds,
  text_preset: {
    resource_id: "chaptera.desktop.fallback-font.ubuntu-light.v1",
    font_fingerprint_sha256: "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70",
    face_index: 0, font_size_emu: 114300, line_height_emu: 142875,
  },
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
  authoring_graph: {...current.authoring_graph,
    nodes: {[authored]: {header: {parent_id: page}, payload: {story_frame: {story_id: story}}}},
    stories: {[story]: {text: ""}},
  },
};
const accepted = {
  protocol_version: "chaptera.temporary-editor-create-textbox.v1",
  document_id: session, source_hash: source, base_revision_id: oldRev,
  client_operation_id: "textbox-op-1", canonical_authoring_revision_id: newCanonical,
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
    csrfToken: csrf, operationIdFactory: () => "textbox-op-1",
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
const create = client => client.createTextBox({pageId: page, bounds});


test("server-minted empty Story is editable only with matching CreateTextBox history", () => {
  assert.deepEqual(temporaryPlainStoryCandidates(after.authoring_graph), [],
    "blank Publisher text is not implicitly admitted by its UUID");
  const candidates = temporaryPlainStoryCandidates(after.authoring_graph, after.project);
  assert.deepEqual(candidates, [{story_id:story, text:"", authored_empty:true}]);
  assert.equal(temporaryStoryTextForTextarea("", {allowEmpty:true}), "");
  const edit = temporaryStoryObservedToCanonicalStory("", "New authored text", {allowEmpty:true});
  assert.equal(edit.canonicalText, "New authored text");
  assert.deepEqual(edit.range, {
    start_scalar:0, end_scalar:0, replacement_text:"New authored text",
  });
  assert.throws(() => temporaryStoryObservedToCanonicalStory("", "New authored text"),
    /temporary_story_intent_invalid/);
  assert.throws(() => temporaryStoryObservedToCanonicalStory("", "new\nparagraph", {allowEmpty:true}),
    /temporary_story_intent_invalid/);
});

test("forged or unbound empty Story never becomes editable", () => {
  const forgedId = {...after, project:{operations:[{...operation, story_id:
    "019a2d80-1112-7001-8001-aabbccddee99"}]}};
  const forgedParent = {...after, authoring_graph:{...after.authoring_graph,
    nodes:{[authored]:{header:{parent_id:"01234567-89ab-4def-8abc-123456789000"},
      payload:{story_frame:{story_id:story}}}}}};
  const forgedFont = {...after, project:{operations:[{...operation,text_preset:{
    ...operation.text_preset,font_size_emu:1}}]}};
  const sourceOnly = {...after, project:{operations:[]}};
  for(const data of [forgedId,forgedParent,forgedFont,sourceOnly]) {
    assert.deepEqual(temporaryPlainStoryCandidates(data.authoring_graph, data.project), []);
  }
});

test("CreateTextBox private POST reopens exact authored TextFrame and never supplies NodeId", async () => {
  const {client, calls} = harness([current, accepted, after]);
  assert.equal((await client.refresh()).revision_id, oldRev);
  const result = await create(client);
  assert.equal(result.accepted.canonical_operation.node_id, authored);
  assert.equal(result.current.project.operations.length, 1);
  assert.equal(client.status.pending_reconciliation, false);
  assert.deepEqual(calls.map(call => [call.url, call.options.method]), [
    ["/v1/editor/temporary-sessions/" + session + "/current", "GET"],
    ["/v1/editor/temporary-sessions/" + session + "/create-textbox", "POST"],
    ["/v1/editor/temporary-sessions/" + session + "/current", "GET"],
  ]);
  const body = JSON.parse(calls[1].options.body);
  assert.deepEqual(body.command, command);
  assert.equal(body.client_operation_id, "textbox-op-1");
  assert.equal(body.base_revision_id, oldRev);
  assert.equal("node_id" in body.command, false);
  assert.equal("story_id" in body.command, false);
  assert.equal(calls[1].options.headers["x-chaptera-reader-session"], bearer);
  assert.equal(calls[1].options.headers["x-csrf-token"], csrf);
  assert.ok(!calls[1].url.includes(bearer));
  assert.equal(calls[1].options.credentials, "omit");
});

test("CreateTextBox refuses unknown page and nonpositive/overflow geometry before HTTP", async () => {
  const {client, calls} = harness([current]);
  await client.refresh();
  const variants = [
    {pageId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", bounds},
    {pageId: page, bounds: {...bounds, width: 0}},
    {pageId: page, bounds: {...bounds, y: -1}},
    {pageId: page, bounds: {...bounds, x: 2_900_000}},
    {pageId: page, bounds: {...bounds, x: Number.MAX_SAFE_INTEGER}},
  ];
  for (const data of variants) {
    await assert.rejects(client.createTextBox(data), /temporary_create_textbox_intent_invalid/);
  }
  assert.equal(calls.length, 1, "invalid intent cannot POST");
});

test("CreateTextBox rejects forged ACK fields and fences a second command", async () => {
  const altered = {...accepted, canonical_operation: {
    ...operation, text_preset: {...operation.text_preset, font_size_emu: 1},
  }};
  const {client, calls} = harness([current, altered]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_create_textbox_ack_operation_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  await assert.rejects(create(client), /temporary_move_reconciliation_required/);
  assert.equal(calls.length, 2);
});

test("CreateTextBox rejects forged Current project operation after a valid ACK", async () => {
  const forged = {...after, project: {operations: [{...operation, node_id:
    "019a2d80-1112-7001-8001-aabbccddee99"}]}};
  const {client, calls} = harness([current, accepted, forged]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_move_reopened_state_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  assert.equal(calls.length, 3);
});

test("CreateTextBox lost ACK replays identical operation ID and never mints a browser NodeId", async () => {
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
  assert.equal(sent[0].client_operation_id, "textbox-op-1");
  assert.ok(!("node_id" in sent[0].command));
  assert.ok(!("story_id" in sent[0].command));
  assert.equal(client.status.pending_reconciliation, false);
});

test("CreateTextBox 409 rejects the POST without replaying mutation or renewing locally", async () => {
  const {client, calls} = harness([current, [{error: "stale_revision"}, 409]]);
  await client.refresh();
  await assert.rejects(create(client), /temporary_move_request_rejected/);
  assert.equal(client.status.pending_reconciliation, false);
  assert.equal(client.status.refresh_required, true);
  await assert.rejects(create(client), /temporary_move_current_required/);
  assert.equal(calls.length, 2);
});
