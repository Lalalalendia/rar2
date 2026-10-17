import assert from "node:assert/strict";

const IMAGE_MIMES = new Set(["image/png", "image/jpeg"]);

// ReaderSceneV1 legitimately omits 'resources' when its Vec is empty.
// Absence and [] both mean that no resources can be delivered to the browser.
export function imageOnlyAblation(scene) {
  const originalResources = scene.resources ?? [];
  assert.ok(Array.isArray(originalResources), "Product Scene resources must be an array if present");
  const without = structuredClone(scene);
  const ablatedResources = without.resources ?? [];
  for (const resource of ablatedResources) {
    if (IMAGE_MIMES.has(resource.mime)) {
      delete resource.inline_data_url;
      resource.availability = "descriptor_only";
    }
  }
  assert.deepEqual(
    ablatedResources.filter(r => !IMAGE_MIMES.has(r.mime)),
    originalResources.filter(r => !IMAGE_MIMES.has(r.mime)),
    "font/nonimage resources must be byte-preserved by image-only ablation",
  );
  return without;
}
