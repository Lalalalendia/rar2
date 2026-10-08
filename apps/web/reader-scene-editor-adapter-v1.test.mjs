import test from "node:test";
import assert from "node:assert/strict";

import { adaptReaderSceneToEditorScene } from "./reader-scene-editor-adapter-v1.mjs";

const DOC = "10000000-0000-4000-8000-000000000001";
const PAGE = "20000000-0000-4000-8000-000000000001";
const DIRECT = "30000000-0000-4000-8000-000000000001";
const PROJECTED = "scene-instance:projected";
const RESOURCE = "image:1";
const SOURCE = "a".repeat(64);
const REVISION = "sha256:" + "b".repeat(64);
const PIXEL = "data:image/png;base64,iVBORw0KGgo=";
const FONT_SHA = "c".repeat(64);
const FONT_ID = "font:chaptera-fallback";
const FONT_DATA = "data:font/ttf;base64,AA==";

function readerScene() {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: REVISION,
    scene_authority: "viewer-geometry-current-revision",
    stacking_fidelity: "source_back_to_front",
    fidelity: { state: "partial", reasons: ["source_font_resource_unavailable"] },
    pages: [{ page_id: PAGE, order: 0, width_emu: 1000000, height_emu: 2000000 }],
    nodes: [
      {
        node_id: DIRECT,
        page_id: PAGE,
        kind: "picture_frame",
        bounds: { x: 10, y: 20, width: 30, height: 40 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
        paint: { fill_rgb: [1, 2, 3], line: { rgb: [4, 5, 6], width_emu: 12700 } },
        resource_id: RESOURCE,
        text: "Direct text",
        text_layout: {
          disposition: "shared_resolved",
          font_resource_id: FONT_ID,
          font_fingerprint_sha256: FONT_SHA,
          font_size_emu: 114300,
          line_height_emu: 142875,
          color_rgb: [7, 8, 9],
          vertical_offset_emu: 0,
          lines: [{
            line_index: 0,
            scalar_start: 0,
            scalar_end: 11,
            consumed_scalar_end: 11,
            text: "Direct text",
            x_offset_emu: 0,
            measured_width_emu: 20,
            line_height_emu: 142875,
            spans: [],
          }],
        },
        preview_text_style: { font_size_emu: 114300, color_rgb: [7, 8, 9] },
      },
      {
        node_id: PROJECTED,
        origin_node_id: "carrier:master",
        page_id: PAGE,
        kind: "shape",
        bounds: { x: 50, y: 60, width: 70, height: 80 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
        paint: { fill_rgb: [10, 20, 30] },
      },
    ],
    stories: [],
    resources: [{ resource_id: RESOURCE, mime: "image/png", availability: "inline_data_url", inline_data_url: PIXEL }],
    fonts: [{
      resource_id: FONT_ID,
      family_name: "Chaptera Fallback",
      mime: "font/ttf",
      expected_sha256: FONT_SHA,
      availability: "inline_data_url",
      inline_data_url: FONT_DATA,
    }],
    diagnostics: [],
  };
}

test("rich Reader Scene becomes an Editor Scene without changing revision authority", async () => {
  const scene = await adaptReaderSceneToEditorScene(readerScene());
  assert.equal(scene.protocol_version, "chaptera.editor-render-scene.v1");
  assert.equal(scene.document_id, DOC);
  assert.equal(scene.revision_id, REVISION);
  assert.equal(scene.stacking_fidelity, "exact");
  assert.match(scene.snapshot_id, /^sha256:[0-9a-f]{64}$/);
  assert.equal(scene.nodes[0].editable, true);
  assert.equal(scene.nodes[1].editable, false);
  assert.equal(scene.nodes[1].origin_node_id, "carrier:master");
  assert.equal(scene.nodes[0].z_order, 0);
  assert.equal(scene.nodes[1].z_order, 1);
  assert.deepEqual(scene.paints[0].fill, { r: 1, g: 2, b: 3, a: 255 });
  assert.equal(scene.resources[0].inline_data_url, PIXEL);
  assert.equal(scene.resources[0].expected_sha256, null);
  assert.equal(scene.resources[0].family_name, null);
  const font = scene.resources.find((resource) => resource.kind === "font");
  assert.equal(font.resource_id, FONT_ID);
  assert.equal(font.expected_sha256, FONT_SHA);
  assert.equal(font.content_hash, FONT_SHA);
  assert.equal(font.inline_data_url, FONT_DATA);
  assert.equal(scene.stories[0].text, "Direct text");
  assert.equal(scene.stories[0].text_fidelity, "supported");
  assert.equal(scene.capabilities.find((item) => item.key === "render.paint").state, "supported");
});

test("unknown Reader stacking stays non-authoritative for Editor hit order", async () => {
  const payload = readerScene();
  payload.stacking_fidelity = "unknown";
  const scene = await adaptReaderSceneToEditorScene(payload);
  assert.equal(scene.stacking_fidelity, "partial");
  assert.equal(scene.nodes[0].z_order, null);
  assert.equal(scene.nodes[1].paint_order, null);
});

test("adapter rejects reader scenes with invalid source identity", async () => {
  const payload = readerScene();
  payload.source_hash = "not-a-sha";
  await assert.rejects(adaptReaderSceneToEditorScene(payload), /source_hash/);
});


test("adapter rejects image/font resource identity collisions", async () => {
  const payload = readerScene();
  payload.fonts[0].resource_id = RESOURCE;
  await assert.rejects(
    adaptReaderSceneToEditorScene(payload),
    /duplicate image\/font resource identity/,
  );
});


test("adapter preserves Reader picture crop rotation and recolor semantics verbatim", async () => {
  const payload = readerScene();
  payload.nodes[0].image_source_window = {
    left_q16: 16384,
    top_q16: 0,
    right_q16: 49152,
    bottom_q16: 65536,
  };
  payload.nodes[0].image_content_rotation_degrees = null;
  payload.nodes[0].image_recolor = {
    target_rgb: [51, 102, 153],
    preserve_grays: false,
  };

  const scene = await adaptReaderSceneToEditorScene(payload);
  assert.deepEqual(scene.nodes[0].image_source_window, payload.nodes[0].image_source_window);
  assert.equal(scene.nodes[0].image_content_rotation_degrees, null);
  assert.deepEqual(scene.nodes[0].image_recolor, payload.nodes[0].image_recolor);
});
