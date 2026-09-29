import test from "node:test";
import assert from "node:assert/strict";

import {
  assertReaderSceneSourceNeutral,
  imagePaintGeometry,
  tableCellPaintGeometry
} from "./render-v1.mjs";

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
