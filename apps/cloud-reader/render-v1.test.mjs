import test from "node:test";
import assert from "node:assert/strict";

import {
  assertReaderSceneSourceNeutral,
  imageContentRotationGeometry,
  imagePaintGeometry,
  imageRecolorPaintPlan,
  imageResourcePaintPlan,
  presetShapePaintGeometry,
  resolvedTextLinePaintPlan,
  tableBorderPaintPlan,
  tableCellFillPaintPlan,
  tableCellPaintGeometry
} from "./render-v1.mjs";

test("Viewer-materialized OLE preview PNG uses the generic image resource paint path", () => {
  const resource = {
    resource_id: "resource:legacy-ole-preview",
    mime: "image/png",
    availability: "inline_data_url",
    inline_data_url: "data:image/png;base64,Ym91bmRlZC1vbGUtcHJldmlldy1wbmc="
  };
  const node = {
    node_id: "node:legacy-ole",
    kind: "picture_frame",
    bounds: { x: 100, y: 200, width: 300, height: 400 },
    resource_id: resource.resource_id
  };

  assert.doesNotThrow(() =>
    assertReaderSceneSourceNeutral({ nodes: [node], resources: [resource] })
  );
  assert.deepEqual(imageResourcePaintPlan(node, resource), {
    href: resource.inline_data_url,
    resource_id: resource.resource_id,
    availability: "inline_data_url",
    geometry: { x: 0, y: 0, width: 300, height: 400 },
    content_transform: null
  });
  assert.equal(
    imageResourcePaintPlan(node, { ...resource, inline_data_url: "data:image/svg+xml;base64,PHN2Zy8+" }),
    null
  );
});

test("cardinal picture content rotation keeps the destination frame fixed", () => {
  assert.deepEqual(
    imageContentRotationGeometry({ x: 100, y: 200, width: 300, height: 900 }, 270),
    {
      x: -200,
      y: 500,
      width: 900,
      height: 300,
      transform: "rotate(270 250 650)"
    }
  );
  assert.deepEqual(
    imageContentRotationGeometry({ x: 100, y: 200, width: 900, height: 300 }, 180),
    {
      x: 100,
      y: 200,
      width: 900,
      height: 300,
      transform: "rotate(180 550 350)"
    }
  );
  assert.equal(
    imageContentRotationGeometry({ x: 0, y: 0, width: 100, height: 200 }, 45),
    null
  );
});

test("unrotated cropped image keeps fractional paint geometry", () => {
  assert.deepEqual(
    imageContentRotationGeometry({ x: 10.5, y: 20.25, width: 300.75, height: 400.125 }, null),
    {
      x: 10.5,
      y: 20.25,
      width: 300.75,
      height: 400.125,
      transform: null
    }
  );
});

test("cardinal picture content rotation is a placement transform, not a frame transform", () => {
  const resource = {
    resource_id: "resource:cardinal",
    mime: "image/png",
    availability: "inline_data_url",
    inline_data_url: "data:image/png;base64,cG5n"
  };
  const node = {
    node_id: "node:cardinal",
    kind: "picture_frame",
    bounds: { x: 100, y: 200, width: 300, height: 900 },
    resource_id: resource.resource_id,
    image_content_rotation_degrees: 270
  };
  const plan = imageResourcePaintPlan(node, resource);
  assert.deepEqual(plan.geometry, { x: -300, y: 300, width: 900, height: 300 });
  assert.equal(plan.content_transform, "rotate(270 150 450)");
  assert.deepEqual(node.bounds, { x: 100, y: 200, width: 300, height: 900 });
});

test("high-offset picture frame keeps content placement frame-local", () => {
  const resource = {
    resource_id: "resource:bottom-border",
    mime: "image/png",
    availability: "inline_data_url",
    inline_data_url: "data:image/png;base64,cG5n"
  };
  const node = {
    node_id: "node:bottom-border",
    kind: "picture_frame",
    bounds: { x: 571_500, y: 9_144_000, width: 6_858_000, height: 703_977 },
    resource_id: resource.resource_id,
    image_content_rotation_degrees: 180
  };
  const plan = imageResourcePaintPlan(node, resource);
  assert.deepEqual(plan.geometry, { x: 0, y: 0, width: 6_858_000, height: 703_977 });
  assert.equal(plan.content_transform, "rotate(180 3429000 351988.5)");
});

test("bounded picture recolor matrix maps black to target and white to white", () => {
  const plan = imageRecolorPaintPlan({
    image_recolor: {
      target_rgb: [51, 102, 153],
      preserve_grays: false
    }
  });
  assert.ok(plan);

  const values = plan.values.split(/\s+/).map(Number);
  assert.equal(values.length, 20);
  const apply = ([r, g, b, a]) => [
    values[0] * r + values[1] * g + values[2] * b + values[3] * a + values[4],
    values[5] * r + values[6] * g + values[7] * b + values[8] * a + values[9],
    values[10] * r + values[11] * g + values[12] * b + values[13] * a + values[14],
    values[15] * r + values[16] * g + values[17] * b + values[18] * a + values[19]
  ];

  const black = apply([0, 0, 0, 1]);
  assert.deepEqual(black.map((value) => Math.round(value * 255)), [51, 102, 153, 255]);

  const white = apply([1, 1, 1, 1]);
  assert.deepEqual(white.map((value) => Math.round(value * 255)), [255, 255, 255, 255]);
});

test("picture recolor fails closed for preserve-grays or invalid target state", () => {
  assert.equal(imageRecolorPaintPlan({
    image_recolor: { target_rgb: [1, 2, 3], preserve_grays: true }
  }), null);
  assert.equal(imageRecolorPaintPlan({}), null);
  assert.throws(
    () => imageRecolorPaintPlan({
      image_recolor: { target_rgb: [1, 2, 999], preserve_grays: false }
    }),
    /byte values/
  );
});

test("image crop maps the normalized source window onto the destination frame", () => {
  const geometry = imagePaintGeometry(
    { x: 0, y: 0, width: 1000, height: 800 },
    {
      left_q16: 16_384,
      top_q16: 0,
      right_q16: 49_152,
      bottom_q16: 65_536
    }
  );
  assert.deepEqual(geometry, { x: -500, y: 0, width: 2000, height: 800 });
});

test("out-of-domain fit window preserves blank destination margins", () => {
  const geometry = imagePaintGeometry(
    { x: 0, y: 0, width: 1200, height: 600 },
    {
      left_q16: -16_384,
      top_q16: 0,
      right_q16: 81_920,
      bottom_q16: 65_536
    }
  );
  assert.equal(Math.round(geometry.x), 200);
  assert.equal(Math.round(geometry.width), 800);
});

test("invalid or non-overlapping source windows fail closed", () => {
  assert.equal(
    imagePaintGeometry(
      { x: 0, y: 0, width: 100, height: 100 },
      { left_q16: 70_000, top_q16: 0, right_q16: 80_000, bottom_q16: 65_536 }
    ),
    null
  );
});

test("renderer input rejects parser/private source carriers recursively", () => {
  assert.doesNotThrow(() => assertReaderSceneSourceNeutral({
    protocol_version: "chaptera.reader-scene.v1",
    nodes: [{ node_id: "n1", resource_id: "r1" }]
  }));
  assert.throws(
    () => assertReaderSceneSourceNeutral({ nodes: [{ parser_record: { stream_path: "Contents" } }] }),
    /forbidden source field/
  );
});


test("table cell geometry preserves authoritative page-space bounds", () => {
  assert.deepEqual(
    tableCellPaintGeometry({
      cell_id: "cell-1",
      bounds: { x: 12700, y: 25400, width: 38100, height: 50800 }
    }),
    { x: 12700, y: 25400, width: 38100, height: 50800 }
  );
  assert.equal(tableCellPaintGeometry({ bounds: null }), null);
  assert.equal(
    tableCellPaintGeometry({ bounds: { x: 0, y: 0, width: 0, height: 100 } }),
    null
  );
});


test("table cell fill plan uses source-neutral fill on authoritative bounds", () => {
  assert.deepEqual(
    tableCellFillPaintPlan({
      cell_id: "cell-fill",
      bounds: { x: 100, y: 200, width: 300, height: 400 },
      fill_rgb: [10, 20, 30],
      fill_visible: true
    }),
    {
      geometry: { x: 100, y: 200, width: 300, height: 400 },
      fill: "rgb(10 20 30)"
    }
  );
  assert.equal(
    tableCellFillPaintPlan({
      bounds: { x: 100, y: 200, width: 300, height: 400 }
    }),
    null
  );
  assert.equal(
    tableCellFillPaintPlan({
      bounds: { x: 100, y: 200, width: 300, height: 400 },
      fill_rgb: [10, 20, 30],
      fill_visible: false
    }),
    null
  );
});

test("table border plan preserves source-backed segment geometry and width", () => {
  assert.deepEqual(
    tableBorderPaintPlan({
      x1_emu: 100,
      y1_emu: 200,
      x2_emu: 400,
      y2_emu: 200,
      rgb: [1, 2, 3],
      width_emu: 12700
    }),
    {
      x1: 100,
      y1: 200,
      x2: 400,
      y2: 200,
      stroke: "rgb(1 2 3)",
      width: 12700
    }
  );
  assert.equal(
    tableBorderPaintPlan({
      x1_emu: 100,
      y1_emu: 200,
      x2_emu: 100,
      y2_emu: 200,
      rgb: [1, 2, 3],
      width_emu: 12700
    }),
    null
  );
});

test("shared resolved text paint plan rejects negative line x offsets", () => {
  assert.equal(resolvedTextLinePaintPlan({
    bounds: { x: 100, y: 200, width: 1000, height: 600 },
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 120,
      line_height_emu: 150,
      lines: [{
        line_index: 0,
        text: "bad",
        x_offset_emu: -1,
        measured_width_emu: 300,
        line_height_emu: 150
      }]
    }
  }), null);
});

test("shared resolved text paint plan preserves server line breaks", () => {
  const plan = resolvedTextLinePaintPlan({
    bounds: { x: 100, y: 200, width: 1000, height: 600 },
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 120,
      line_height_emu: 150,
      lines: [
        {
          line_index: 1,
          text: "second",
          measured_width_emu: 450,
          line_height_emu: 150
        },
        {
          line_index: 0,
          text: "first",
          x_offset_emu: 200,
          measured_width_emu: 300,
          line_height_emu: 150
        }
      ]
    }
  });

  assert.equal(plan.font_resource_id, "font-1");
  assert.equal(plan.font_size_emu, 120);
  assert.deepEqual(
    plan.lines.map((line) => [line.line_index, line.x, line.y, line.text]),
    [[0, 300, 200, "first"], [1, 100, 350, "second"]]
  );
});

test("shared resolved text paint plan applies server vertical block offset", () => {
  const plan = resolvedTextLinePaintPlan({
    bounds: { x: 100, y: 200, width: 1000, height: 600 },
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 120,
      line_height_emu: 150,
      vertical_offset_emu: 300,
      lines: [{
        line_index: 0,
        text: "centered",
        measured_width_emu: 300,
        line_height_emu: 150
      }]
    }
  });

  assert.equal(plan.lines[0].y, 500);
});

test("shared resolved text paint plan rejects invalid vertical block offsets", () => {
  assert.equal(resolvedTextLinePaintPlan({
    bounds: { x: 0, y: 0, width: 1000, height: 600 },
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 120,
      line_height_emu: 150,
      vertical_offset_emu: 601,
      lines: []
    }
  }), null);
});

test("source-backed text bounds affect text only, not outer resource geometry", () => {
  const node = {
    node_id: "projected-carrier",
    bounds: { x: 100, y: 200, width: 1000, height: 800 },
    text_bounds: { x: 140, y: 240, width: 920, height: 720 },
    resource_id: "resource-1",
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 120,
      line_height_emu: 150,
      lines: [{
        line_index: 0,
        text: "carrier",
        measured_width_emu: 420,
        line_height_emu: 150
      }]
    }
  };
  const resource = {
    resource_id: "resource-1",
    mime: "image/png",
    availability: "inline_data_url",
    inline_data_url: "data:image/png;base64,cG5n"
  };

  assert.doesNotThrow(() => assertReaderSceneSourceNeutral({ nodes: [node], resources: [resource] }));
  assert.deepEqual(resolvedTextLinePaintPlan(node).bounds, node.text_bounds);
  assert.deepEqual(imageResourcePaintPlan(node, resource).geometry, {
    x: 0,
    y: 0,
    width: node.bounds.width,
    height: node.bounds.height
  });
});

test("mixed shared text plan preserves server span sizes and cumulative line heights", () => {
  const plan = resolvedTextLinePaintPlan({
    bounds: { x: 100, y: 200, width: 1000, height: 600 },
    text_layout: {
      disposition: "shared_resolved",
      font_resource_id: "font-1",
      font_size_emu: 180,
      line_height_emu: 180,
      lines: [
        {
          line_index: 1,
          text: "tail",
          measured_width_emu: 240,
          line_height_emu: 90,
          spans: [
            {
              scalar_start: 3,
              scalar_end: 7,
              text: "tail",
              x_offset_emu: 0,
              measured_width_emu: 240,
              font_size_emu: 90
            }
          ]
        },
        {
          line_index: 0,
          text: "A B",
          measured_width_emu: 420,
          line_height_emu: 180,
          spans: [
            {
              scalar_start: 0,
              scalar_end: 2,
              text: "A ",
              x_offset_emu: 0,
              measured_width_emu: 180,
              font_size_emu: 100,
              font_resource_id: "font-elephant",
              font_fingerprint_sha256: "sha-elephant"
            },
            {
              scalar_start: 2,
              scalar_end: 3,
              text: "B",
              x_offset_emu: 180,
              measured_width_emu: 240,
              font_size_emu: 180,
              font_resource_id: "font-times",
              font_fingerprint_sha256: "sha-times"
            }
          ]
        }
      ]
    }
  });

  assert.deepEqual(
    plan.lines.map((line) => [line.line_index, line.y, line.line_height_emu]),
    [[0, 200, 180], [1, 380, 90]]
  );
  assert.deepEqual(
    plan.lines[0].spans.map((span) => [
      span.scalar_start,
      span.scalar_end,
      span.x_offset_emu,
      span.font_size_emu,
      span.font_resource_id,
      span.font_fingerprint_sha256,
      span.text
    ]),
    [
      [0, 2, 0, 100, "font-elephant", "sha-elephant", "A "],
      [2, 3, 180, 180, "font-times", "sha-times", "B"]
    ]
  );
});

test("shared text span font identity is all-or-nothing", () => {
  assert.equal(
    resolvedTextLinePaintPlan({
      bounds: { x: 0, y: 0, width: 1000, height: 600 },
      text_layout: {
        disposition: "shared_resolved",
        font_resource_id: "font-1",
        font_size_emu: 100,
        line_height_emu: 120,
        lines: [{
          line_index: 0,
          text: "mixed",
          measured_width_emu: 300,
          line_height_emu: 120,
          spans: [{
            scalar_start: 0,
            scalar_end: 5,
            text: "mixed",
            x_offset_emu: 0,
            measured_width_emu: 300,
            font_size_emu: 100,
            font_resource_id: "font-elephant"
          }]
        }]
      }
    }),
    null
  );
});


test("shared text plan refuses invalid frame or font metrics", () => {
  assert.equal(
    resolvedTextLinePaintPlan({
      bounds: { x: 0, y: 0, width: 0, height: 100 },
      text_layout: {
        disposition: "shared_resolved",
        font_resource_id: "font-1",
        font_size_emu: 100,
        line_height_emu: 120,
        lines: []
      }
    }),
    null
  );
});


test("default RoundRectangle preset uses bounded short-side radius", () => {
  assert.deepEqual(
    presetShapePaintGeometry({
      bounds: { x: 100, y: 200, width: 600000, height: 300000 },
      paint: { preset_shape: "round_rect" }
    }),
    {
      tag: "rect",
      attrs: { x: 100, y: 200, width: 600000, height: 300000, rx: 50001, ry: 50001 }
    }
  );
  assert.deepEqual(
    presetShapePaintGeometry({
      bounds: { x: 1, y: 2, width: 30, height: 40 },
      paint: {}
    }),
    { tag: "rect", attrs: { x: 1, y: 2, width: 30, height: 40 } }
  );
});
