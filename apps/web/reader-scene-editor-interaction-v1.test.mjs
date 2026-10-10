import test from "node:test";
import assert from "node:assert/strict";

import { MoveGestureV1, hitTestSnapshot } from "./interaction-v1.mjs";
import {
  EDITOR_INTERACTION_SCENE_V1,
  assertRichInteractionIdentity,
  projectReaderSceneToEditorInteractionScene,
} from "./reader-scene-editor-interaction-v1.mjs";

const DOC = "10000000-0000-4000-8000-000000000001";
const PAGE = "20000000-0000-4000-8000-000000000001";
const DIRECT = "30000000-0000-4000-8000-000000000001";
const NESTED = "30000000-0000-4000-8000-000000000002";
const ORIGIN = "30000000-0000-4000-8000-000000000003";
const SOURCE = "a".repeat(64);
const REV = "sha256:" + "b".repeat(64);

function scene() {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: REV,
    scene_authority: "server_viewer_projection",
    stacking_fidelity: "partial",
    fidelity: { state: "partial", reasons: [] },
    pages: [{
      page_id: PAGE,
      order: 0,
      width_emu: 10_058_400,
      height_emu: 7_772_400,
    }],
    nodes: [
      {
        node_id: DIRECT,
        page_id: PAGE,
        kind: "shape",
        bounds: { x: 1000, y: 2000, width: 3000, height: 4000 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      },
      {
        node_id: NESTED,
        page_id: PAGE,
        parent_node_id: DIRECT,
        kind: "shape",
        bounds: { x: 1500, y: 2500, width: 1000, height: 1000 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      },
      {
        node_id: "sha256:" + "c".repeat(64),
        origin_node_id: ORIGIN,
        page_id: PAGE,
        kind: "text_frame",
        bounds: { x: 6000, y: 7000, width: 3000, height: 4000 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      },
    ],
    stories: [],
    resources: [],
    fonts: [],
    diagnostics: [],
  };
}

test("interaction projection admits only direct page-local canonical nodes", () => {
  const rich = scene();
  const interaction = projectReaderSceneToEditorInteractionScene(rich);
  assert.equal(interaction.protocol_version, EDITOR_INTERACTION_SCENE_V1);
  assert.equal(interaction.revision_id, REV);
  assert.deepEqual(interaction.nodes.map((node) => node.node_id), [DIRECT]);
  assert.equal(interaction.stacking_fidelity, "unknown");
  assert.equal(assertRichInteractionIdentity(rich, interaction), true);
});

test("projected and nested visuals cannot win editor hit testing", () => {
  const interaction = projectReaderSceneToEditorInteractionScene(scene());
  assert.deepEqual(
    hitTestSnapshot(interaction, PAGE, { x_emu: 1600, y_emu: 2600 }),
    { kind: "hit", node_id: DIRECT },
  );
  assert.deepEqual(
    hitTestSnapshot(interaction, PAGE, { x_emu: 6500, y_emu: 7500 }),
    { kind: "none" },
  );
});

test("MoveGesture commits the exact Reader revision identity", () => {
  const interaction = projectReaderSceneToEditorInteractionScene(scene());
  const gesture = new MoveGestureV1(
    interaction,
    DIRECT,
    { x_emu: 1100, y_emu: 2100 },
  );
  gesture.update({ x_emu: 2100, y_emu: 4100 });
  const request = gesture.commit("move-rich-1");
  assert.equal(request.document_id, DOC);
  assert.equal(request.source_hash, SOURCE);
  assert.equal(request.base_revision_id, REV);
  assert.equal(request.command.node_id, DIRECT);
  assert.deepEqual(
    [request.command.x_emu, request.command.y_emu],
    [2000, 4000],
  );
});

test("source-private fields fail closed before interaction projection", () => {
  const rich = scene();
  rich.nodes[0].source_path = "private/source.pub";
  assert.throws(
    () => projectReaderSceneToEditorInteractionScene(rich),
    /forbidden source field source_path/,
  );
});

test("UUIDv7 authored PageId and NodeId are admitted without editing projected instances", () => {
  const rich = scene();
  const newPage = "20000000-0000-7000-8000-000000000001";
  const newDirect = "30000000-0000-7000-8000-000000000001";
  rich.pages[0].page_id = newPage;
  rich.nodes.forEach((node) => { node.page_id = newPage; });
  rich.nodes[0].node_id = newDirect;
  rich.nodes[1].parent_node_id = newDirect;
  const interaction = projectReaderSceneToEditorInteractionScene(rich);
  assert.deepEqual(interaction.nodes.map((node) => node.node_id), [newDirect]);
  assert.equal(interaction.pages[0].page_id, newPage);
  assert.equal(hitTestSnapshot(interaction, newPage, { x_emu: 1600, y_emu: 2600 }).node_id, newDirect);
});
