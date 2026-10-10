import test from "node:test";
import assert from "node:assert/strict";

import {
  resolveRichEditorPointerTarget,
} from "./rich-reader-editor-shell-v1.mjs";

const PAGE = "20000000-0000-4000-8000-000000000001";
const A = "30000000-0000-4000-8000-000000000001";
const B = "30000000-0000-4000-8000-000000000002";

function interaction(nodes = [{
  node_id: A,
  page_id: PAGE,
  parent_node_id: null,
  kind: "shape",
  bounds: { x: 0, y: 0, width: 100, height: 100 },
  z_order: null,
  paint_order: null,
}]) {
  return {
    protocol_version: "chaptera.editor-interaction-scene.v1",
    document_id: "doc",
    source_hash: "a".repeat(64),
    revision_id: "sha256:" + "b".repeat(64),
    visual_protocol_version: "chaptera.reader-scene.v1",
    visual_scene_authority: "server_viewer_projection",
    stacking_fidelity: "unknown",
    pages: [{
      page_id: PAGE,
      order: 0,
      width_emu: 1000,
      height_emu: 1000,
    }],
    nodes,
  };
}

test("a projected visual on top never falls through to an editable origin", () => {
  const result = resolveRichEditorPointerTarget(
    interaction(),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    "sha256:" + "c".repeat(64),
  );
  assert.equal(result.kind, "read_only_visual");
});

test("blank visual area may hit one direct page-local interaction node", () => {
  assert.deepEqual(
    resolveRichEditorPointerTarget(
      interaction(),
      PAGE,
      { x_emu: 50, y_emu: 50 },
      null,
    ),
    { kind: "hit", node_id: A },
  );
});

test("overlapping direct nodes remain ambiguous without stacking authority", () => {
  const result = resolveRichEditorPointerTarget(
    interaction([
      {
        node_id: A,
        page_id: PAGE,
        parent_node_id: null,
        kind: "shape",
        bounds: { x: 0, y: 0, width: 100, height: 100 },
        z_order: null,
        paint_order: null,
      },
      {
        node_id: B,
        page_id: PAGE,
        parent_node_id: null,
        kind: "shape",
        bounds: { x: 0, y: 0, width: 100, height: 100 },
        z_order: null,
        paint_order: null,
      },
    ]),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    null,
  );
  assert.equal(result.kind, "ambiguous");
  assert.equal(result.reason, "stacking_not_authoritative");
});

test("visual target and geometry target must name the same canonical node", () => {
  const result = resolveRichEditorPointerTarget(
    interaction(),
    PAGE,
    { x_emu: 50, y_emu: 50 },
    A,
  );
  assert.deepEqual(result, { kind: "hit", node_id: A });
});
