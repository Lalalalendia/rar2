import test from "node:test";
import assert from "node:assert/strict";
import {
  documentIdFromEditorPath, loginUrlForReturnPath, productEditorStatusView,
  sourcePlainStoryCandidates, oneShotStoryIntent, storyTextForTextarea,
  textareaTextToCanonicalStory,
} from "./product-editor-entry-v1.mjs";

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

test("unknown Undo/Redo outcome remains visibly locked until server history recheck", () => {
  const BASE = "sha256:" + "a".repeat(64);
  const CHILD = "sha256:" + "b".repeat(64);
  const unknown = productEditorStatusView({
    reason: "history_ack_unknown", history_command: "undo",
    history_uncertain: true, revision_id: BASE,
  });
  assert.equal(unknown.kind, "error");
  assert.match(unknown.status, /Undo\/Redo outcome unknown/);
  assert.match(unknown.status, /editing locked/);
  assert.doesNotMatch(unknown.status, /Revision sha256/);
  const checking = productEditorStatusView({
    reason: "scene_loaded", history_uncertain: true,
    history_recovery_in_flight: true, revision_id: BASE,
  });
  assert.equal(checking.kind, "pending");
  assert.match(checking.status, /Checking exact server history revision/);
  const recovered = productEditorStatusView({
    reason: "history_authority_reconciled", history_uncertain: false,
    history_recovery_in_flight: false, revision_id: CHILD,
  });
  assert.equal(recovered.kind, "ok");
  assert.match(recovered.status, /Server history synchronized/);
  assert.doesNotMatch(recovered.status, /Saved/);
});

test("Story inspector admits plain framed text with only an immutable terminal marker", () => {
  const graph = {
    nodes: {
      a: {payload: {story_frame: {story_id: "plain"}}},
      b: {payload: {story_frame: {story_id: "rich"}}},
      c: {payload: {story_frame: {story_id: "table"}}},
      d: {payload: {table: {story_id: "table"}}},
      e: {payload: {story_frame: {story_id: "multiline"}}},
      f: {payload: {story_frame: {story_id: "terminal"}}},
      g: {payload: {story_frame: {story_id: "empty"}}},
    },
    stories: {
      plain: {text: "ABC", runs: [], paragraphs: [], fields: [], hyperlinks: []},
      rich: {text: "Rich", runs: [{source: "import"}]},
      table: {text: "Cell"},
      multiline: {text: "First\r\nSecond", runs: [], paragraphs: [], fields: [], hyperlinks: []},
      terminal: {text: "Title\r", runs: [], paragraphs: [], fields: [], hyperlinks: []},
      empty: {text: "\r", runs: [], paragraphs: [], fields: [], hyperlinks: []},
      unframed: {text: "No frame"},
    },
  };
  assert.deepEqual(sourcePlainStoryCandidates(graph), [
    {story_id: "plain", text: "ABC"},
    {story_id: "terminal", text: "Title\r"},
  ]);
});

test("textarea projection preserves the source terminal paragraph marker", () => {
  assert.equal(storyTextForTextarea("Title\r"), "Title");
  assert.equal(storyTextForTextarea("Title\r\n"), "Title");
  assert.equal(textareaTextToCanonicalStory("Title\r", "Qitle"), "Qitle\r");
  assert.throws(() => storyTextForTextarea("First\rSecond"), /terminal paragraph marker/);
  assert.throws(() => textareaTextToCanonicalStory("Title\r", "New\nTitle"),
    /no new paragraphs/);
});

test("one-shot Story diff uses Unicode scalar positions and no client authoritative before", () => {
  const intent = oneShotStoryIntent({
    documentId: DOC,
    sourceHash: "a".repeat(64),
    baseRevisionId: "sha256:" + "b".repeat(64),
    storyId: "40000000-0000-4000-8000-000000000001",
    canonicalText: "A😀BC",
    observedText: "A😀YC",
    clientOperationId: "product-story-test",
  });
  assert.equal(intent.command.start_scalar, 2);
  assert.equal(intent.command.end_scalar, 3);
  assert.equal(intent.command.replacement_text, "Y");
  assert.equal("expected_before" in intent.command, false);
  assert.equal("typing_format" in intent.command, false);
  assert.equal("depends_on_client_operation_id" in intent, false);
  const terminal = oneShotStoryIntent({
    documentId: DOC,
    sourceHash: "a".repeat(64),
    baseRevisionId: "sha256:" + "b".repeat(64),
    storyId: "40000000-0000-4000-8000-000000000002",
    canonicalText: "ABC\r",
    observedText: "AYC",
    clientOperationId: "product-story-terminal-test",
  });
  assert.equal(terminal.command.start_scalar, 1);
  assert.equal(terminal.command.end_scalar, 2);
  assert.equal(terminal.command.replacement_text, "Y");
  assert.throws(() => oneShotStoryIntent({
    documentId: DOC,sourceHash:"a".repeat(64),baseRevisionId:"sha256:"+"b".repeat(64),
    storyId:"story",canonicalText:"abc",observedText:"abc",clientOperationId:"story-test",
  }), /no text change/);
  assert.throws(() => oneShotStoryIntent({
    documentId:DOC,sourceHash:"a".repeat(64),baseRevisionId:"sha256:"+"b".repeat(64),
    storyId:"story",canonicalText:"abc",observedText:"a\nbc",clientOperationId:"story-test",
  }), /no new paragraphs/);
});
