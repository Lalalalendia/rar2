import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  RENDERER_KINDS,
  RENDER_SCENE_PROTOCOLS,
  isRenderableSceneProtocol,
  assertSceneSourceNeutral,
  buildOverlayPlan,
  buildRenderPlan,
  imageContentRotationGeometry,
  imagePaintGeometry,
  imageRecolorPaintPlan,
  resolvedEditorImagePlan,
  resolvedEditorTablePlan,
  unresolvedTextPreviewFrameV1,
  tableBorderPaintPlan,
  tableCellFillPaintPlan,
  tableCellPaintGeometry
} from "./render-v1.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.resolve(HERE, "../../packages/protocol/scene/v1/fixtures");
const VIEW = {
  emu_per_css_px: 9525,
  zoom: 1,
  pan_x_css_px: 0,
  pan_y_css_px: 0
};

function fixture(name) {
  return JSON.parse(fs.readFileSync(path.join(FIXTURES, name), "utf8"));
}

function deepFreeze(value) {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const child of Object.values(value)) deepFreeze(child);
  }
  return value;
}

test("preflight exposes all required renderer candidates", () => {
  assert.deepEqual(RENDERER_KINDS, ["svg", "canvas2d", "webgl2-hybrid"]);
});

test("renderer protocol discriminator keeps canonical and Editor render scenes explicit", () => {
  assert.deepEqual(RENDER_SCENE_PROTOCOLS, [
    "chaptera.scene.v1",
    "chaptera.editor-render-scene.v1",
  ]);
  assert.equal(isRenderableSceneProtocol("chaptera.scene.v1"), true);
  assert.equal(isRenderableSceneProtocol("chaptera.editor-render-scene.v1"), true);
  assert.equal(isRenderableSceneProtocol("chaptera.reader-scene.v1"), false);

  const editorScene = fixture("simple-text.json");
  editorScene.protocol_version = "chaptera.editor-render-scene.v1";
  assert.doesNotThrow(() => buildRenderPlan(editorScene, VIEW));
  editorScene.protocol_version = "chaptera.reader-scene.v1";
  assert.throws(() => buildRenderPlan(editorScene, VIEW), /EditorRenderSceneV1/);
});

test("render plan is deterministic and does not mutate a frozen scene", () => {
  const scene = deepFreeze(fixture("group-table.json"));
  const left = buildRenderPlan(scene, VIEW);
  const right = buildRenderPlan(scene, VIEW);
  assert.deepEqual(
    left.pages.map((page) => page.nodes.map((node) => node.node_id)),
    right.pages.map((page) => page.nodes.map((node) => node.node_id))
  );
  assert.equal(left.pages[0].stacking_authority, "exact");
  assert.equal(left.pages[0].nodes.length, 3);
});

test("exact stacking is back-to-front by z then paint order", () => {
  const plan = buildRenderPlan(fixture("group-table.json"), VIEW);
  assert.deepEqual(
    plan.pages[0].nodes.map((node) => node.node_id),
    [
      "23333333-3333-4333-8333-333333333330",
      "23333333-3333-4333-8333-333333333331",
      "23333333-3333-4333-8333-333333333332"
    ]
  );
});

test("negative off-page geometry survives the render plan", () => {
  const plan = buildRenderPlan(fixture("partial-unsupported.json"), VIEW);
  const node = plan.pages[0].nodes[0];
  assert.equal(node.canonical_bounds.x, -12700);
  assert.ok(node.x < plan.pages[0].x);
  assert.equal(node.diagnostics[0].code, "SCENE.NODE.UNSUPPORTED");
  assert.equal(plan.fidelity.state, "partial");
});

test("zoom and pan change display coordinates without mutating scene coordinates", () => {
  const scene = fixture("exact-image.json");
  const before = structuredClone(scene);
  const base = buildRenderPlan(scene, VIEW);
  const moved = buildRenderPlan(scene, {
    ...VIEW,
    zoom: 2,
    pan_x_css_px: 80,
    pan_y_css_px: -40
  });
  assert.notEqual(base.pages[0].nodes[0].x, moved.pages[0].nodes[0].x);
  assert.equal(
    base.pages[0].nodes[0].canonical_bounds.x,
    moved.pages[0].nodes[0].canonical_bounds.x
  );
  assert.deepEqual(scene, before);
});

test("text is explicitly browser-preview-only", () => {
  const plan = buildRenderPlan(fixture("simple-text.json"), VIEW);
  const node = plan.pages[0].nodes[0];
  assert.equal(node.story.text, "Hello, Publisher");
  assert.equal(node.story.authority, "browser_preview_only");
  assert.equal(node.story.text_fidelity, "partial");
});

test("unshaped Story summary is source-frame bounded, explicitly non-authoritative", () => {
  const scene = fixture("simple-text.json");
  const original = structuredClone(scene);
  scene.stories[0].text = "Paragraph one.\r\nParagraph two.\tLong tail ".repeat(8);
  const plan = buildRenderPlan(scene, VIEW);
  const node = plan.pages[0].nodes[0];
  assert.equal(node.story.resolved_text, null);
  const preview = unresolvedTextPreviewFrameV1(node);
  assert.deepEqual(
    [preview.x, preview.y, preview.width, preview.height],
    [node.x, node.y, node.width, node.height],
  );
  assert.equal(preview.layout_verified, false);
  assert.equal(preview.authority, "browser_preview_only");
  assert.equal(preview.preview_reason, "story_text_layout_not_implemented");
  assert.ok(preview.text.includes("Paragraph one. Paragraph two."));
  assert.equal(/[\r\n\t]/u.test(preview.text), false);
  assert.ok(preview.text.length <= 120);
  assert.deepEqual(original.nodes, scene.nodes);
  assert.equal(unresolvedTextPreviewFrameV1({...node, width: 0}), null);
  assert.equal(unresolvedTextPreviewFrameV1({...node, height: -1}), null);
  assert.equal(unresolvedTextPreviewFrameV1({ ...node, story: null }), null);
});

test("resolved lines are not relabeled as verified by fallback renderer", () => {
  const scene = fixture("simple-text.json");
  const node = buildRenderPlan(scene, VIEW).pages[0].nodes[0];
  const preview = unresolvedTextPreviewFrameV1({
    ...node, story: { ...node.story, resolved_text: { lines: [] } },
  });
  assert.equal(preview.preview_reason, "backend_cannot_paint_resolved_line_layout");
  assert.equal(preview.layout_verified, false);
});

test("resource availability is carried without source bytes", () => {
  const plan = buildRenderPlan(fixture("exact-image.json"), VIEW);
  const resource = plan.pages[0].nodes[0].resource;
  assert.equal(resource.kind, "image");
  assert.equal(resource.availability, "available");
  assert.equal(resource.fetch_handle, "res_img_exact_001");
  assert.ok(!("bytes" in resource));
});

test("selection and transient preview are independent overlays", () => {
  const scene = fixture("group-table.json");
  const plan = buildRenderPlan(scene, VIEW);
  const selected = plan.pages[0].nodes[1];
  const baseBounds = structuredClone(scene.nodes[1].bounds);
  const overlay = buildOverlayPlan(plan, {
    selected_node_id: selected.node_id,
    preview_bounds: {
      x: baseBounds.x + 100000,
      y: baseBounds.y + 200000,
      width: baseBounds.width,
      height: baseBounds.height
    }
  });
  assert.equal(overlay.selected_node_id, selected.node_id);
  assert.notEqual(overlay.selected_bounds.x, selected.x);
  assert.deepEqual(scene.nodes[1].bounds, baseBounds);
});

test("renderer rejects raw/private source carrier fields recursively", () => {
  const scene = fixture("simple-text.json");
  assert.doesNotThrow(() => assertSceneSourceNeutral(scene));
  const bad = structuredClone(scene);
  bad.nodes[0].parser_record = { stream_path: "Contents/7" };
  assert.throws(() => buildRenderPlan(bad, VIEW), /forbidden source field/);
});


test("Reader-adapted inline image and preview typography survive the render plan", () => {
  const imageScene = fixture("exact-image.json");
  imageScene.resources[0].availability = "inline_data_url";
  imageScene.resources[0].inline_data_url = "data:image/png;base64,iVBORw0KGgo=";
  const imagePlan = buildRenderPlan(imageScene, VIEW);
  assert.equal(
    imagePlan.pages[0].nodes[0].resource.inline_data_url,
    "data:image/png;base64,iVBORw0KGgo=",
  );

  const textScene = fixture("simple-text.json");
  textScene.nodes[0].preview_text_style = {
    font_size_emu: 190500,
    color_rgb: [12, 34, 56],
  };
  textScene.nodes[0].visual_authority = "reader_scene";
  const textPlan = buildRenderPlan(textScene, VIEW);
  assert.equal(textPlan.pages[0].nodes[0].story.authority, "reader_scene_preview");
  assert.equal(textPlan.pages[0].nodes[0].story.style.font_size_css_px, 20);
  assert.equal(textPlan.pages[0].nodes[0].story.style.fill, "rgb(12 34 56)");
});

test("renderer strips unsafe inline image data URLs instead of handing them to DOM paint", () => {
  const scene = fixture("exact-image.json");
  scene.resources[0].inline_data_url = "javascript:alert(1)";
  const plan = buildRenderPlan(scene, VIEW);
  assert.equal(plan.pages[0].nodes[0].resource.inline_data_url, null);
});


test("Reader shared-resolved text uses server line positions and exact inline font resource", () => {
  const scene = fixture("simple-text.json");
  const fontSha = "c".repeat(64);
  const fontId = "font:reader-fallback";
  scene.nodes[0].visual_authority = "reader_scene";
  scene.nodes[0].text_bounds = {
    x: 914400,
    y: 914400,
    width: 5486400,
    height: 1371600,
  };
  scene.nodes[0].text_layout = {
    disposition: "shared_resolved",
    font_resource_id: fontId,
    font_fingerprint_sha256: fontSha,
    font_size_emu: 190500,
    line_height_emu: 228600,
    color_rgb: [12, 34, 56],
    vertical_offset_emu: 9525,
    lines: [{
      line_index: 0,
      scalar_start: 0,
      scalar_end: 16,
      consumed_scalar_end: 16,
      text: "Hello, Publisher",
      x_offset_emu: 19050,
      measured_width_emu: 952500,
      line_height_emu: 228600,
      spans: [],
    }],
  };
  scene.resources.push({
    resource_id: fontId,
    kind: "font",
    mime: "font/ttf",
    content_hash: fontSha,
    byte_len: null,
    availability: "available",
    fetch_handle: null,
    inline_data_url: "data:font/ttf;base64,AA==",
    expected_sha256: fontSha,
    family_name: "Chaptera Fallback",
  });

  const plan = buildRenderPlan(scene, VIEW);
  const text = plan.pages[0].nodes[0].story.resolved_text;
  assert.equal(text.authority, "server-shared-resolved");
  assert.equal(text.font_resource_id, fontId);
  assert.equal(text.font_family, "ChapteraEditor_" + fontSha.slice(0, 16));
  assert.equal(text.font_size_css_px, 20);
  assert.equal(text.fill, "rgb(12 34 56)");
  assert.equal(text.lines[0].x, plan.pages[0].x + (914400 + 19050) / 9525);
  assert.equal(text.lines[0].y, plan.pages[0].y + (914400 + 9525) / 9525);
  assert.equal(text.font_faces.length, 1);
});

test("Reader shared-resolved text fails closed to preview on font fingerprint mismatch", () => {
  const scene = fixture("simple-text.json");
  const fontId = "font:reader-fallback";
  scene.nodes[0].visual_authority = "reader_scene";
  scene.nodes[0].text_layout = {
    disposition: "shared_resolved",
    font_resource_id: fontId,
    font_fingerprint_sha256: "d".repeat(64),
    font_size_emu: 114300,
    line_height_emu: 142875,
    vertical_offset_emu: 0,
    lines: [],
  };
  scene.resources.push({
    resource_id: fontId,
    kind: "font",
    mime: "font/ttf",
    content_hash: "c".repeat(64),
    byte_len: null,
    availability: "available",
    fetch_handle: null,
    inline_data_url: "data:font/ttf;base64,AA==",
    expected_sha256: "c".repeat(64),
  });
  const plan = buildRenderPlan(scene, VIEW);
  assert.equal(plan.pages[0].nodes[0].story.resolved_text, null);
  assert.equal(plan.pages[0].nodes[0].story.authority, "reader_scene_preview");
});


test("Reader picture crop maps q16 source window into fixed-frame image geometry", () => {
  assert.deepEqual(
    imagePaintGeometry(
      { x: 0, y: 0, width: 1000, height: 800 },
      { left_q16: 16384, top_q16: 0, right_q16: 49152, bottom_q16: 65536 }
    ),
    { x: -500, y: 0, width: 2000, height: 800 }
  );
});

test("Reader picture cardinal rotation stays content-local and rejects unsupported angles", () => {
  assert.deepEqual(
    imageContentRotationGeometry({ x: 100, y: 200, width: 300, height: 900 }, 270),
    { x: -200, y: 500, width: 900, height: 300, transform: "rotate(270 250 650)" }
  );
  assert.equal(
    imageContentRotationGeometry({ x: 0, y: 0, width: 100, height: 200 }, 45),
    null
  );
});

test("Reader picture recolor keeps bounded sRGB matrix semantics", () => {
  const plan = imageRecolorPaintPlan({
    image_recolor: { target_rgb: [51, 102, 153], preserve_grays: false }
  });
  assert.ok(plan);
  assert.equal(plan.values.split(/\s+/).length, 20);
  assert.equal(
    imageRecolorPaintPlan({ image_recolor: { target_rgb: [1, 2, 3], preserve_grays: true } }),
    null
  );
});

test("Reader picture render plan uses fixed-frame crop and fails closed on crop plus rotation", () => {
  const scene = fixture("exact-image.json");
  scene.nodes[0].visual_authority = "reader_scene";
  scene.nodes[0].image_source_window = {
    left_q16: 16384,
    top_q16: 0,
    right_q16: 49152,
    bottom_q16: 65536,
  };
  scene.resources[0].availability = "inline_data_url";
  scene.resources[0].inline_data_url = "data:image/png;base64,iVBORw0KGgo=";

  const plan = buildRenderPlan(scene, VIEW);
  const image = plan.pages[0].nodes[0].resource.resolved_image;
  assert.equal(image.authority, "reader-picture-content");
  assert.equal(image.viewport.width, plan.pages[0].nodes[0].width);
  assert.equal(image.geometry.x, -plan.pages[0].nodes[0].width / 2);
  assert.equal(image.geometry.width, plan.pages[0].nodes[0].width * 2);

  scene.nodes[0].image_content_rotation_degrees = 90;
  const invalid = buildRenderPlan(scene, VIEW);
  assert.equal(invalid.pages[0].nodes[0].resource.resolved_image, null);
});

test("resolved Reader picture plan preserves frame while rotating content", () => {
  const scene = fixture("exact-image.json");
  scene.nodes[0].visual_authority = "reader_scene";
  scene.nodes[0].image_content_rotation_degrees = 270;
  scene.resources[0].availability = "inline_data_url";
  scene.resources[0].inline_data_url = "data:image/png;base64,iVBORw0KGgo=";
  const resources = new Map(scene.resources.map((resource) => [resource.resource_id, resource]));
  const page = { x: 24, y: 24 };
  const image = resolvedEditorImagePlan(scene.nodes[0], resources.get(scene.nodes[0].resource_id), page, VIEW);
  assert.ok(image);
  assert.equal(image.viewport.x, page.x + scene.nodes[0].bounds.x / 9525);
  assert.match(image.content_transform, /^rotate\(270 /);
});


test("Reader picture out-of-domain fit window preserves blank destination margins", () => {
  const geometry = imagePaintGeometry(
    { x: 0, y: 0, width: 1200, height: 600 },
    { left_q16: -16384, top_q16: 0, right_q16: 81920, bottom_q16: 65536 }
  );
  assert.equal(Math.round(geometry.x), 200);
  assert.equal(Math.round(geometry.width), 800);
});

test("Reader picture invalid or non-overlapping source windows fail closed", () => {
  assert.equal(
    imagePaintGeometry(
      { x: 0, y: 0, width: 100, height: 100 },
      { left_q16: 70000, top_q16: 0, right_q16: 80000, bottom_q16: 65536 }
    ),
    null
  );
});

test("Reader table uses only server-resolved cell geometry for fills", () => {
  const cell = {
    cell_id: "cell:0:0",
    row: 0,
    column: 0,
    row_span: 1,
    column_span: 2,
    text: "Header",
    bounds: { x: 95250, y: 190500, width: 952500, height: 476250 },
    fill_rgb: [12, 34, 56],
    fill_visible: true,
  };
  assert.deepEqual(tableCellPaintGeometry(cell), cell.bounds);
  assert.deepEqual(tableCellFillPaintPlan(cell), {
    geometry: cell.bounds,
    fill: "rgb(12 34 56)",
  });
  assert.equal(tableCellPaintGeometry({ ...cell, bounds: null }), null);
  assert.equal(tableCellFillPaintPlan({ ...cell, fill_visible: false }), null);
});

test("Reader table borders require exact non-degenerate server segments", () => {
  const border = {
    x1_emu: 95250,
    y1_emu: 190500,
    x2_emu: 1047750,
    y2_emu: 190500,
    rgb: [1, 2, 3],
    width_emu: 12700,
  };
  assert.deepEqual(tableBorderPaintPlan(border), {
    x1: 95250,
    y1: 190500,
    x2: 1047750,
    y2: 190500,
    stroke: "rgb(1 2 3)",
    width: 12700,
  });
  assert.equal(tableBorderPaintPlan({ ...border, width_emu: 0 }), null);
  assert.equal(tableBorderPaintPlan({ ...border, x2_emu: border.x1_emu, y2_emu: border.y1_emu }), null);
});

test("Reader table plan maps server cell/border geometry to page CSS and keeps text preview-only", () => {
  const node = {
    visual_authority: "reader_scene",
    table: {
      story_id: "story:table",
      rows: 2,
      columns: 2,
      cells: [{
        cell_id: "cell:0:0",
        row: 0,
        column: 0,
        row_span: 1,
        column_span: 2,
        text: "Header",
        bounds: { x: 95250, y: 190500, width: 952500, height: 476250 },
        fill_rgb: [12, 34, 56],
        fill_visible: true,
      }],
      borders: [{
        x1_emu: 95250,
        y1_emu: 190500,
        x2_emu: 1047750,
        y2_emu: 190500,
        rgb: [1, 2, 3],
        width_emu: 12700,
      }],
    },
  };
  const table = resolvedEditorTablePlan(node, { x: 24, y: 30 }, VIEW);
  assert.equal(table.authority, "reader-table-resolved");
  assert.equal(table.cells[0].x, 34);
  assert.equal(table.cells[0].y, 50);
  assert.equal(table.cells[0].width, 100);
  assert.equal(table.cells[0].fill, "rgb(12 34 56)");
  assert.equal(table.cells[0].text_authority, "browser-preview-only");
  assert.equal(table.borders[0].x1, 34);
  assert.equal(table.borders[0].width_css_px, 12700 / 9525);
  assert.equal(resolvedEditorTablePlan({ ...node, visual_authority: null }, { x: 0, y: 0 }, VIEW), null);
});
