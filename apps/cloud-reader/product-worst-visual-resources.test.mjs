import assert from "node:assert/strict";
import test from "node:test";
import { imageOnlyAblation } from "./product-worst-visual-resources.mjs";

test("omitted optional resources is a legitimate empty list", () => {
  const source = { protocol_version: "chaptera.reader-scene.v1", nodes: [], pages: [] };
  const changed = imageOnlyAblation(source);
  assert.deepEqual(changed, source);
  assert.equal(Object.hasOwn(changed, "resources"), false);
});

test("explicit empty resource list remains empty", () => {
  const source = { resources: [], nodes: [] };
  assert.deepEqual(imageOnlyAblation(source), source);
});

test("only host-derived encoded image payloads are stripped", () => {
  const image = { mime: "image/jpeg", resource_id: "img", availability: "inline_data_url", inline_data_url: "data:image/jpeg;base64,AAAA" };
  const font = { mime: "font/woff2", resource_id: "font", availability: "inline_data_url", inline_data_url: "keep" };
  const source = { resources: [image, font] };
  const changed = imageOnlyAblation(source);
  assert.deepEqual(changed.resources[0], { mime: "image/jpeg", resource_id: "img", availability: "descriptor_only" });
  assert.deepEqual(changed.resources[1], font);
  assert.deepEqual(source.resources[0], image, "source Product Scene remains immutable");
});

test("invalid resources type is rejected, not silently dropped", () => {
  assert.throws(() => imageOnlyAblation({ resources: {} }), /must be an array/);
});
