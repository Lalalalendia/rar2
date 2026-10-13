import test from "node:test";
import assert from "node:assert/strict";
import {
  TemporaryGuestMoveClientV1, temporaryPlainStoryCandidates,
  temporaryStoryTextForTextarea, temporaryStoryObservedToCanonicalStory,
} from "./temporary-edit-move-client-v1.mjs";
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
  assert.equal(download.status.refresh_required, true);
  await assert.rejects(download.exportEdited("pdf"), /temporary_export_current_required/);
  await assert.rejects(download.moveNode(move), /temporary_move_current_required/);
});

test("forged edited PDF SHA fences subsequent download and MoveNode until canonical refresh", async () => {
  const pdf = new TextEncoder().encode("%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n");
  const response = new Response(pdf, { headers: {
    "content-type": "application/pdf",
    "x-chaptera-edit-revision": afterRev,
    "x-chaptera-export-sha256": "0".repeat(64),
  } });
  const seen = [];
  const client = new TemporaryGuestMoveClientV1({
    sessionId: session, accessToken: bearer, sourceHash: source, csrfToken: csrf,
    fetchImpl: async (url, options) => {
      seen.push({url, method:options.method, bearer:options.headers["x-chaptera-reader-session"]});
      if (seen.length === 1 || seen.length === 3) return reply(after);
      if (seen.length === 2) return response;
      throw Error("unplanned request");
    },
    operationIdFactory: () => "test-move-1",
  });
  await client.refresh();
  await assert.rejects(client.exportEdited("pdf"), /temporary_export_hash_mismatch/);
  assert.equal(client.status.refresh_required, true);
  await assert.rejects(client.exportEdited("pdf"), /temporary_export_current_required/);
  await assert.rejects(client.moveNode(move), /temporary_move_current_required/);
  assert.equal(seen.length, 2, "corrupted export must not retry or mutate");
  await client.refresh();
  assert.equal(client.status.refresh_required, false);
  assert.equal(client.status.revision_id, afterRev);
  assert.deepEqual(seen.map(x => x.method), ["GET", "GET", "GET"]);
  assert.ok(seen.every(x => x.bearer === bearer && !x.url.includes(bearer)));
});

test("malformed edited report JSON locks authoring until current is reread", async () => {
  let calls = 0;
  const client = new TemporaryGuestMoveClientV1({
    sessionId: session, accessToken: bearer, sourceHash: source, csrfToken: csrf,
    fetchImpl: async () => {
      calls++;
      if (calls === 1) return reply(after);
      if (calls === 2) return new Response("{malformed", {
        headers: { "content-type": "application/json" },
      });
      return reply(after);
    },
  });
  await client.refresh();
  await assert.rejects(client.exportEdited("pdf", {reportOnly:true}), /temporary_export_report_invalid/);
  assert.equal(client.status.refresh_required, true);
  await assert.rejects(client.exportEdited("pdf"), /temporary_export_current_required/);
  assert.equal(calls, 2);
  await client.refresh();
  assert.equal(client.status.refresh_required, false);
});

test("zero-byte editable output and interrupted private body read both fence the client", async () => {
  for (const bad of [
    new Response("", {headers: {
      "content-type": "application/vnd.adobe.indesign-idml-package",
      "x-chaptera-edit-revision": afterRev,
      "x-chaptera-export-sha256": "a".repeat(64),
    }}),
    {ok:true, status:200, headers: new Headers({
      "content-type": "application/vnd.adobe.indesign-idml-package",
      "x-chaptera-edit-revision": afterRev,
      "x-chaptera-export-sha256": "a".repeat(64),
    }), blob: async () => {throw Error("stream interrupted");}},
  ]) {
    let calls = 0;
    const client = new TemporaryGuestMoveClientV1({
      sessionId: session, accessToken: bearer, sourceHash: source, csrfToken: csrf,
      fetchImpl: async () => ++calls === 1 ? reply(after) : bad,
    });
    await client.refresh();
    await assert.rejects(client.exportEdited("idml"),
      /temporary_export_download_(size_invalid|read_failed)/);
    assert.equal(client.status.refresh_required, true);
    await assert.rejects(client.moveNode(move), /temporary_move_current_required/);
    assert.equal(calls, 2);
  }
});


const storyId = "01234567-89ab-4def-8abc-123456789abc";
function withPlainStory(base, text = "Initial") {
  return {
    ...base,
    authoring_graph: {
      nodes: {
        ...base.authoring_graph.nodes,
        [nodeId]: {
          ...base.authoring_graph.nodes[nodeId],
          payload: { story_frame: { story_id: storyId } },
        },
      },
      stories: { [storyId]: { text, runs: [], paragraphs: [], fields: [], hyperlinks: [] } },
    },
  };
}
function storyAck(replayed = false, {beforeText = "Initial", replacement = "Edited"} = {}) {
  return {
    ...accepted(replayed),
    protocol_version: "chaptera.temporary-editor-story-range.v1",
    canonical_operation: {
      kind: "replace_story_range", story_id: storyId, start_scalar: 0,
      end_scalar: [...beforeText].length, replacement_text: replacement,
    },
  };
}

test("Guest story gate excludes linked, styled, table-owned and mixed-ending source", () => {
  const ready = withPlainStory(before);
  assert.deepEqual(temporaryPlainStoryCandidates(ready.authoring_graph),
    [{story_id: storyId, text: "Initial"}]);
  assert.deepEqual(temporaryPlainStoryCandidates({
    ...ready.authoring_graph,
    nodes: {
      ...ready.authoring_graph.nodes,
      extra: {payload: {story_frame: {story_id: storyId}}},
    },
  }), []);
  assert.deepEqual(temporaryPlainStoryCandidates({
    ...ready.authoring_graph,
    nodes: {
      ...ready.authoring_graph.nodes,
      table: {payload: {table: {story_id: storyId}}},
    },
  }), []);
  assert.deepEqual(temporaryPlainStoryCandidates({
    ...ready.authoring_graph,
    stories: { [storyId]: {text: "one\r\ntwo\n"} },
  }), []);
  assert.deepEqual(temporaryPlainStoryCandidates({
    ...ready.authoring_graph,
    stories: { [storyId]: {text: "Initial", runs: [{style:"rich"}]} },
  }), []);
});

test("Guest Story replacement is private, Unicode scalar-counted and reopens exact text", async () => {
  const current = withPlainStory(before, "Old \u{1f642}");
  const revised = withPlainStory(after, "New \u{1f642}");
  const { client, requests } = harness([
    current, storyAck(false, {beforeText: "Old", replacement: "New"}), revised,
  ]);
  await client.refresh();
  const receipt = await client.replaceStory({storyId, newText:"New \u{1f642}"});
  assert.equal(receipt.current.authoring_graph.stories[storyId].text, "New \u{1f642}");
  assert.equal(receipt.accepted.protocol_version, "chaptera.temporary-editor-story-range.v1");
  assert.deepEqual(requests.map(x => [x.options.method,x.url]), [
    ["GET", "/v1/editor/temporary-sessions/"+session+"/current"],
    ["POST", "/v1/editor/temporary-sessions/"+session+"/story-range"],
    ["GET", "/v1/editor/temporary-sessions/"+session+"/current"],
  ]);
  const sent = JSON.parse(requests[1].options.body);
  assert.deepEqual(sent.command, {
    kind: "replace_story_range", story_id: storyId,
    start_scalar: 0, end_scalar: 3, replacement_text: "New",
  });
  for(const r of requests) {
    assert.equal(r.options.headers["x-chaptera-reader-session"], bearer);
    assert.equal(r.options.headers["x-csrf-token"], csrf);
    assert.equal(r.options.credentials,"omit");
    assert.equal(r.options.redirect,"error");
    assert.ok(!r.url.includes(bearer));
  }
});

test("Guest Story lost ACK retries the identical operation and does not accept a different edit", async () => {
  const current = withPlainStory(before, "Initial");
  const revised = withPlainStory(after, "Edited");
  const {client,requests} = harness([current,new TypeError("drop"),storyAck(true),revised]);
  await client.refresh();
  await assert.rejects(client.replaceStory({storyId,newText:"Edited"}),/drop/);
  await assert.rejects(client.replaceStory({storyId,newText:"Other"}),/temporary_move_reconciliation_required/);
  assert.equal(client.status.pending_reconciliation,true);
  const recovered=await client.reconcileUnknown();
  assert.equal(recovered.accepted.replayed,true);
  const posts=requests.filter(x=>x.options.method==="POST");
  assert.equal(posts.length,2);
  assert.equal(posts[0].options.body,posts[1].options.body);
  assert.equal(client.status.pending_reconciliation,false);
});

test("Guest Story forged ACK/reopen, invalid text and unsupported Story fail closed", async () => {
  const current = withPlainStory(before,"Initial");
  const {client: local,requests} = harness([current]);
  await local.refresh();
  await assert.rejects(local.replaceStory({storyId,newText:"Initial"}),/temporary_story_intent_invalid/);
  await assert.rejects(local.replaceStory({storyId,newText:"a\nb"}),/temporary_story_intent_invalid/);
  await assert.rejects(local.replaceStory({storyId:"foreign",newText:"x"}),/temporary_story_intent_invalid/);
  assert.equal(requests.length,1);

  const {client: forged} = harness([current,{...storyAck(),protocol_version:"wrong"},storyAck(true),withPlainStory(after,"Edited")]);
  await forged.refresh();
  await assert.rejects(forged.replaceStory({storyId,newText:"Edited"}),/temporary_move_ack_identity_invalid/);
  assert.equal(forged.status.pending_reconciliation,true);
  await forged.reconcileUnknown();

  const {client: wrong} = harness([current,storyAck(),withPlainStory(after,"Wrong"),storyAck(true),withPlainStory(after,"Edited")]);
  await wrong.refresh();
  await assert.rejects(wrong.replaceStory({storyId,newText:"Edited"}),/temporary_move_reopened_state_mismatch/);
  assert.equal(wrong.status.pending_reconciliation,true);
  await wrong.reconcileUnknown();
  assert.equal(wrong.status.pending_reconciliation,false);
});


test("Guest Story rejects forged canonical operation fields before acknowledging Current", async () => {
  const current = withPlainStory(before, "Initial");
  const invalid = storyAck();
  invalid.canonical_operation.replacement_text = "forged";
  const {client} = harness([current, invalid, storyAck(true), withPlainStory(after, "Edited")]);
  await client.refresh();
  await assert.rejects(client.replaceStory({storyId, newText:"Edited"}),
    /temporary_story_ack_operation_mismatch/);
  assert.equal(client.status.pending_reconciliation, true);
  const same = await client.reconcileUnknown();
  assert.equal(same.accepted.replayed, true);
  assert.equal(same.current.authoring_graph.stories[storyId].text, "Edited");
});


test("Guest Story keeps original terminal CR outside the bounded scalar edit", async () => {
  const current = withPlainStory(before, "Hello\r");
  const revised = withPlainStory(after, "Edited\r");
  assert.equal(temporaryStoryTextForTextarea("Hello\r"), "Hello");
  assert.deepEqual(temporaryPlainStoryCandidates(current.authoring_graph),
    [{story_id: storyId, text: "Hello\r"}]);
  const {client, requests} = harness([
    current, storyAck(false, {beforeText: "Hello", replacement: "Edited"}), revised,
  ]);
  await client.refresh();
  const result = await client.replaceStory({storyId, newText: "Edited"});
  assert.equal(result.current.authoring_graph.stories[storyId].text, "Edited\r");
  const submitted = JSON.parse(requests[1].options.body);
  assert.deepEqual(submitted.command, {
    kind: "replace_story_range", story_id: storyId,
    start_scalar: 0, end_scalar: 5, replacement_text: "Edited",
  });
  assert.ok(!submitted.command.replacement_text.includes("\r"));
});

test("Guest Story preserves CRLF and terminal CR while prohibiting new paragraphs", async () => {
  const original = "Hello\r\nworld\r";
  const current = withPlainStory(before, original);
  const desired = "Edited\r\nworld\r";
  assert.equal(temporaryStoryTextForTextarea(original), "Hello\nworld");
  assert.deepEqual(temporaryStoryObservedToCanonicalStory(original,"Edited\nworld"),{
    canonicalText: desired,
    range: {start_scalar: 0, end_scalar: 5, replacement_text: "Edited"},
  });
  assert.throws(() => temporaryStoryObservedToCanonicalStory(original,"Edited\nworld\nnew"),
    /temporary_story_intent_invalid/);
  assert.throws(() => temporaryStoryObservedToCanonicalStory(original,"Edited\rworld"),
    /temporary_story_intent_invalid/);
  assert.throws(() => temporaryStoryObservedToCanonicalStory(original,"Edited\tworld"),
    /temporary_story_intent_invalid/);
  assert.throws(() => temporaryStoryObservedToCanonicalStory(original,"Edited\nworld\n"),
    /temporary_story_intent_invalid/);
  assert.deepEqual(temporaryPlainStoryCandidates(current.authoring_graph),
    [{story_id: storyId, text: original}]);
  const {client,requests}=harness([
    current, storyAck(false,{beforeText:"Hello", replacement:"Edited"}),
    withPlainStory(after,desired),
  ]);
  await client.refresh();
  const result = await client.replaceStory({storyId,newText:"Edited\nworld"});
  assert.equal(result.current.authoring_graph.stories[storyId].text,desired);
  assert.equal(JSON.parse(requests[1].options.body).command.replacement_text,"Edited");
  assert.throws(() => temporaryStoryObservedToCanonicalStory("a\r\nb\n","A\nb\n"),
    /mixed Publisher Story line endings/);
});

test("Guest Story keeps original paragraph positions: reordering a separator is rejected", () => {
  const original="alpha\r\nbeta\r";
  assert.throws(() =>
    temporaryStoryObservedToCanonicalStory(original, "alphabet\n"),
    /temporary_story_intent_invalid/);
  assert.throws(() =>
    temporaryStoryObservedToCanonicalStory(original, "al\nphabeta"),
    /temporary_story_intent_invalid/);
});
