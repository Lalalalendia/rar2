import test from "node:test";
import assert from "node:assert/strict";
import { documentIdFromEditorPath, loginUrlForReturnPath, productEditorStatusView } from "./product-editor-entry-v1.mjs";

const DOC="10000000-0000-4000-8000-000000000001";

test("editor path admits one canonical document identity",()=>{
  assert.equal(documentIdFromEditorPath("/editor/doc/"+DOC),DOC);
  assert.equal(documentIdFromEditorPath("/editor/doc/"+DOC+"/"),DOC);
  assert.equal(documentIdFromEditorPath("/editor/doc/not-a-document"),null);
  assert.equal(documentIdFromEditorPath("/reader/doc/"+DOC),null);
});

test("login return stays same-origin relative",()=>{
  assert.equal(
    loginUrlForReturnPath("/editor/doc/"+DOC+"?mode=move"),
    "/v1/auth/login?return_path=%2Feditor%2Fdoc%2F"+DOC+"%3Fmode%3Dmove",
  );
  assert.throws(()=>loginUrlForReturnPath("https://evil.example/"),/local return path/);
  assert.throws(()=>loginUrlForReturnPath("//evil.example/"),/local return path/);
});

test("real UUIDv7 DocumentId is accepted by the Product Editor entry", () => {
  const v7 = "0199c4b0-e9a2-7d31-8b42-1234567890ab";
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7), v7);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7.toUpperCase()), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7 + "/extra"), null);
});

test("pending, rejected and uncertain edits are not presented as a saved revision", () => {
  const receipt = { reason: "commit_sent", revision_id: "sha256:base" };
  const scene = { fidelity: { state: "partial", reasons: ["missing_typeface"] } };
  const pending = productEditorStatusView(receipt, scene);
  assert.equal(pending.kind, "pending");
  assert.match(pending.status, /Saving change/);
  assert.equal(pending.fidelity, "Fidelity: partial — missing_typeface");
  for (const reason of ["commit_error", "commit_rejected"]) {
    const view = productEditorStatusView({ ...receipt, reason }, scene);
    assert.equal(view.kind, "error");
    assert.match(view.status, /reload/);
    assert.doesNotMatch(view.status, /Revision sha256:base/);
  }
  const undoPending = productEditorStatusView(
    { reason: "history_sent", revision_id: "sha256:child" },
    scene,
  );
  assert.equal(undoPending.kind, "pending");
  assert.match(undoPending.status, /Undoing/);
  for (const reason of ["history_error", "history_rejected"]) {
    const view = productEditorStatusView(
      { reason, revision_id: "sha256:child" },
      scene,
    );
    assert.equal(view.kind, "error");
    assert.doesNotMatch(view.status, /Revision sha256:child/);
  }

  const confirmed = productEditorStatusView({
    reason: "commit_reconciled",
    revision_id: "sha256:child",
    selected_node_id: "node-a",
  }, { fidelity: { state: "supported", reasons: [] } });
  assert.equal(confirmed.kind, "ok");
  assert.equal(confirmed.status, "Revision sha256:child · selected");
  assert.equal(confirmed.fidelity, "Fidelity: supported");
});


test("SourceIngress persisted document identity opens through the same Editor page", () => {
  const sourceDocument = "document:c54c2429d0bf699b890aab84";
  assert.equal(documentIdFromEditorPath("/editor/doc/" + sourceDocument), sourceDocument);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + encodeURIComponent(sourceDocument)), sourceDocument);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + sourceDocument.toUpperCase()), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/document:123"), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/document:%2F%2Fevil.invalid"), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/%GG"), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + sourceDocument + "/extra"), null);
});

test("Redo status distinguishes pending/rejected/uncertain from a confirmed revision", () => {
  const pending = productEditorStatusView({
    reason: "history_sent", history_command: "redo", revision_id: "sha256:undo",
  });
  assert.equal(pending.kind, "pending");
  assert.match(pending.status, /Redoing/);
  for (const reason of ["history_error", "history_rejected"]) {
    const view = productEditorStatusView({
      reason, history_command: "redo", revision_id: "sha256:undo",
    });
    assert.equal(view.kind, "error");
    assert.match(view.status, /Redo/);
    assert.doesNotMatch(view.status, /Revision sha256:undo/);
  }
});

test("Pending move save does not show a successful Undo", () => {
  const view = productEditorStatusView({reason:"history_blocked_during_save",revision_id:"sha256:old"});
  assert.equal(view.kind, "pending");
  assert.match(view.status, /Save still pending/);
  assert.doesNotMatch(view.status, /^Revision /);
});

test("ambiguous save takes precedence over stale revision success and requires manual recheck", () => {
  const oldRevision = "sha256:" + "a".repeat(64);
  const failed = productEditorStatusView({
    reason: "commit_error", revision_id: oldRevision, commit_uncertain: true,
  });
  assert.equal(failed.kind, "error");
  assert.match(failed.status, /Save outcome unknown/);
  assert.match(failed.status, /editing locked/);
  assert.doesNotMatch(failed.status, /Revision sha256/);
  const inRecovery = productEditorStatusView({
    reason: "scene_loaded", revision_id: oldRevision,
    commit_uncertain: true, commit_recovery_in_flight: true,
  });
  assert.equal(inRecovery.kind, "pending");
  assert.match(inRecovery.status, /Checking exact server revision/);
  assert.doesNotMatch(inRecovery.status, /Revision sha256/);
  const exact = productEditorStatusView({
    reason: "commit_authority_reconciled",
    revision_id: "sha256:" + "b".repeat(64),
    commit_uncertain: false, commit_recovery_in_flight: false,
  });
  assert.equal(exact.kind, "ok");
  assert.match(exact.status, /Server revision synchronized/);
  assert.doesNotMatch(exact.status, /Saved/);
});
