import test from "node:test";
import assert from "node:assert/strict";
import { TemporaryGuestMoveTransport, guestMoveRequest, requireGuestCurrent } from "./temporary-guest-move-v1.mjs";

const id = "tempedit:" + "a".repeat(32);
const source = "b".repeat(64);
const token = "c".repeat(64);
const revision = "sha256:" + "d".repeat(64);
const nextRevision = "sha256:" + "e".repeat(64);
const node = "10000000-0000-7000-8000-000000000001";
const page = "20000000-0000-7000-8000-000000000002";
const current = { protocol_version: "chaptera.temporary-editor-current.v1", document_id: id, source_hash: source, revision_id: revision, expires_at_ms: 10_000 };
const scene = {
  protocol_version: "chaptera.reader-scene.v1", document_id: id,
  source_hash: source, revision_id: revision,
  pages: [{ page_id: page, order: 0, width_emu: 10000, height_emu: 10000 }],
  nodes: [{ node_id: node, page_id: page, kind: "shape", bounds: { x: 100, y: 200, width: 300, height: 400 } }],
};
test("guest source and revision mismatch fail closed", () => {
  assert.throws(() => requireGuestCurrent({ ...current, source_hash: "f".repeat(64) }, id, source), /identity/);
  assert.throws(() => guestMoveRequest(current, { ...scene, revision_id: nextRevision }, node, { x_emu: 500, y_emu: 600 }, "move-1"), /revision/);
});
test("only direct canonical node creates correct single MoveNode intent", () => {
  const cmd = guestMoveRequest(current, scene, node, { x_emu: 500, y_emu: 600 }, "move-1");
  assert.equal(cmd.command.kind, "move_node_to");
  assert.equal(cmd.command.x_emu, 500);
  assert.equal(cmd.document_id, id);
  assert.equal(cmd.base_revision_id, revision);
  assert.equal(guestMoveRequest(current, scene, node, { x_emu: 100, y_emu: 200 }, "move-2"), null);
  assert.throws(() => guestMoveRequest(current, { ...scene, nodes: [{ ...scene.nodes[0], origin_node_id: node }] }, node, { x_emu: 500, y_emu: 600 }, "move-2"), /unsupported/);
});
test("private transport posts one bounded move; current read never renews TTL", async () => {
  const calls = [];
  const client = new TemporaryGuestMoveTransport({
    sessionId: id, token, sourceHash: source, csrfToken: "f".repeat(32),
    fetchImpl: async (url, options) => {
      calls.push({ url, options });
      const payload = calls.length === 1
        ? { protocol_version: "chaptera.temporary-editor-move-node.v1", document_id: id,
          source_hash: source, base_revision_id: revision, client_operation_id: "move-1",
          revision_id: nextRevision, expires_at_ms: 20_000 }
        : { ...current, revision_id: nextRevision, expires_at_ms: 20_000 };
      return { ok: true, json: async () => payload };
    },
  });
  const result = await client.move(current, scene, node, { x_emu: 500, y_emu: 600 }, "move-1");
  assert.equal(result.current.revision_id, nextRevision);
  assert.deepEqual(calls.map(x => x.options.method), ["POST", "GET"]);
  assert.equal(calls[0].options.headers["x-chaptera-reader-session"], token);
  assert.ok(calls.every(x => !x.url.includes(token)));
  assert.ok(calls.every(x => x.options.cache === "no-store"));
  assert.equal(client.pending, null);
});
test("lost ACK retains the same pending request and prevents another operation", async () => {
  const client = new TemporaryGuestMoveTransport({
    sessionId: id, token, sourceHash: source, csrfToken: "f".repeat(32),
    fetchImpl: async () => { throw new Error("lost"); },
  });
  await assert.rejects(() => client.move(current, scene, node, { x_emu: 500, y_emu: 600 }, "move-1"), /lost/);
  assert.equal(client.pending.client_operation_id, "move-1");
  await assert.rejects(() => client.move(current, scene, node, { x_emu: 600, y_emu: 600 }, "move-2"), /in_flight/);
});
