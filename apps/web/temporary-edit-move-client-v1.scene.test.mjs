import test from "node:test";
import assert from "node:assert/strict";
import { TemporaryGuestMoveClientV1 } from "./temporary-edit-move-client-v1.mjs";

const id = "tempedit:" + "a".repeat(32);
const source = "b".repeat(64);
const bearer = "c".repeat(64);
const beforeRevision = "sha256:" + "d".repeat(64);
const afterRevision = "sha256:" + "e".repeat(64);
const canon = "f".repeat(64);
const scene = {
  protocol_version: "chaptera.reader-scene.v1",
  document_id: id, source_hash: source, revision_id: beforeRevision,
  pages: [], nodes: [],
};
const current = {
  protocol_version: "chaptera.temporary-editor-current.v1",
  document_id: id, source_hash: source, revision_id: beforeRevision,
  canonical_authoring_revision_id: canon, revision_cursor: 0,
  expires_at_ms: 4000000, project: {}, authoring_graph: { nodes: {} },
};
const exact = {
  protocol_version: "chaptera.temporary-editor-scene.v1",
  document_id: id, source_hash: source, revision_id: beforeRevision,
  expires_at_ms: current.expires_at_ms, scene,
};
function json(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status, headers: { "content-type": "application/json" },
  });
}
function harness(feed) {
  const calls = [], input = [...feed];
  const client = new TemporaryGuestMoveClientV1({
    sessionId: id, accessToken: bearer, sourceHash: source,
    csrfToken: "0".repeat(32),
    fetchImpl: async (url, opts) => {
      calls.push({ url, opts });
      return input.shift() ?? json({ error: "missing_stub" }, 500);
    },
  });
  return { client, calls };
}
test("guest exact scene is pinned to current source and revision", async () => {
  const { client, calls } = harness([json(current), json(exact)]);
  await client.refresh();
  assert.deepEqual(await client.readerScene(), scene);
  assert.deepEqual(calls.map(({url})=>url), [
    "/v1/editor/temporary-sessions/" + id + "/current",
    "/v1/editor/temporary-sessions/" + id + "/scene",
  ]);
  assert.ok(calls.every(({ url, opts }) => !url.includes(bearer)
    && opts.headers["x-chaptera-reader-session"] === bearer
    && opts.cache === "no-store"));
});
test("stale and cross-source Scene cannot be displayed or used for mutation", async () => {
  const { client } = harness([json(current), json({
    ...exact, scene: { ...scene, revision_id: afterRevision },
  })]);
  await client.refresh();
  await assert.rejects(client.readerScene(), /temporary_scene_identity_mismatch/);
  await assert.rejects(client.moveNode({ nodeId: "no", xEmu: 4, yEmu: 5 }), /temporary_move_current_required/);
});
test("edited export report is exact private revision and cannot read baseline", async () => {
  const modified = { ...current, revision_cursor: 1, revision_id: afterRevision };
  const report = {
    protocol_version: "chaptera.temporary-edited-export-report.v1",
    source_hash: source, revision_id: afterRevision,
    canonical_authoring_revision_id: canon,
    target: "idml", artifact_sha256: "1".repeat(64),
    loss_report: { counts: { blocking: 0, warning: 0 } },
  };
  const { client, calls } = harness([json(modified), json(report)]);
  await client.refresh();
  assert.equal((await client.exportEdited("idml", { reportOnly: true })).target, "idml");
  assert.ok(calls[1].url.includes("revision_id=sha256"));
  assert.ok(!calls[1].url.includes(bearer));
  const baseline = harness([json(current)]);
  await baseline.client.refresh();
  await assert.rejects(baseline.client.exportEdited("idml"), /temporary_export_current_required/);
});
test("export refuses a report for another revision", async () => {
  const modified = { ...current, revision_cursor: 1, revision_id: afterRevision };
  const { client } = harness([json(modified), json({
    protocol_version: "chaptera.temporary-edited-export-report.v1",
    source_hash: source, revision_id: beforeRevision,
    canonical_authoring_revision_id: canon, target: "odg",
    artifact_sha256: "1".repeat(64), loss_report: {},
  })]);
  await client.refresh();
  await assert.rejects(client.exportEdited("odg", { reportOnly: true }), /identity_mismatch/);
});
