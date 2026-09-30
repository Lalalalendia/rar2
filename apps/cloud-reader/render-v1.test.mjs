import test from "node:test";
import assert from "node:assert/strict";

import {
  assertReaderSceneSourceNeutral,
  imagePaintGeometry,
  imageResourcePaintPlan,
  resolvedTextLinePaintPlan,
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
    geometry: { x: 100, y: 200, width: 300, height: 400 }
  });
  assert.equal(
    imageResourcePaintPlan(node, { ...resource, inline_data_url: "data:image/svg+xml;base64,PHN2Zy8+" }),
    null
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
          measured_width_emu: 300,
          line_height_emu: 150
        }
      ]
    }
  });

  assert.equal(plan.font_resource_id, "font-1");
  assert.equal(plan.font_size_emu, 120);
  assert.deepEqual(
    plan.lines.map((line) => [line.line_index, line.y, line.text]),
    [[0, 200, "first"], [1, 350, "second"]]
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
