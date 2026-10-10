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
