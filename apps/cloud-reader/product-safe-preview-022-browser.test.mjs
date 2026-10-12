// Exact pinned 001/022/023 Product Reader image ablation in Chromium.
// Both captures use the same host-admitted isolated Product producer Scene and
// the same renderer/head/Publisher oracle. Suppress image URLs only in the
// negative control. No mock pictures, no external fetch and no PNG upload.

import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const scenePath = process.env.CHAPTERA_PREVIEW_PROBE_SCENE_OUT;
const output = process.env.CHAPTERA_PREVIEW_BROWSER_OUTPUT;
const exactHead = process.env.REPOSITORY_COMMIT_SHA;
assert.ok(scenePath && output, "pinned private scene and output paths required");
assert.match(exactHead ?? "", /^[0-9a-f]{40}$/, "exact repository head must be supplied for physical Product visual proof");
const visualCaseId = process.env.CHAPTERA_PRODUCT_VISUAL_ID ?? "022";
const visualCases = {
  "001": { sha: "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", images: 1 },
  "022": { sha: "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c", images: 1 },
  "023": { sha: "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", images: 7 },
};
assert.ok(Object.hasOwn(visualCases, visualCaseId), "unsupported Product visual case");
const pinned = visualCases[visualCaseId];
const readerScene = JSON.parse(await readFile(resolve(scenePath), "utf8"));
assert.equal(readerScene.protocol_version, "chaptera.reader-scene.v1");
assert.equal(readerScene.source_hash, pinned.sha, "Product Scene must match exact pinned case");
assert.equal(readerScene.pages.length, 1, "pinned Product visual cases require one page");
assert.ok(readerScene.resources.some((r) => r.availability === "inline_data_url"));

const noImages = structuredClone(readerScene);
const previewImageTypes = new Set(["image/png", "image/jpeg"]);
const visibleImages = (readerScene.resources ?? []).filter(
  (resource) => previewImageTypes.has(resource.mime) && resource.availability === "inline_data_url"
);
assert.equal(visibleImages.length, pinned.images, "Product positive must contain exact host-derived image resource count");
for (const resource of noImages.resources ?? []) {
  // Alter ONLY host-derived image delivery. Font URLs and nonimage
  // render inputs must remain identical in both Publisher A/B arms.
  if (previewImageTypes.has(resource.mime)) {
    delete resource.inline_data_url;
    resource.availability = "descriptor_only";
  }
}
assert.deepEqual(
  (noImages.resources ?? []).filter((resource) => !previewImageTypes.has(resource.mime)),
  (readerScene.resources ?? []).filter((resource) => !previewImageTypes.has(resource.mime)),
  "same-head image ablation must preserve all non-image resources and fonts"
);
const server = createServer(async (request, response) => {
  if (request.url === "/render-v1.mjs") {
    response.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store" });
    response.end(await readFile(join(root, "render-v1.mjs")));
  } else if (request.url === "/") {
    response.writeHead(200, { "content-type": "text/html", "cache-control": "no-store" });
    response.end('<!doctype html><html><head><title>Product isolated 022 visual witness</title></head><body style="margin:0;padding:0;background:white"><main id="pages"></main></body></html>');
  } else {
    response.writeHead(404);
    response.end();
  }
});
await mkdir(resolve(output), { recursive: true });
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const origin = "http://127.0.0.1:" + server.address().port;
let browser;
const foreign = [];
const pageErrors = [];
function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}
async function capture(page, scene, mode) {
  await page.goto(origin);
  await page.evaluate(async (data) => {
    const { renderReaderScene } = await import("/render-v1.mjs");
    await renderReaderScene(document.querySelector("#pages"), data, {
      emuPerCssPx: 914400 / 144,
      maxPageWidth: 10000,
    });
    await document.fonts.ready;
    await Promise.all([...document.querySelectorAll("#pages svg image")].map(async (image) => {
      const src = image.getAttribute("href");
      if (!src || !src.startsWith("data:image/")) {
        throw new Error("Product browser received a forbidden image URL");
      }
      const decoded = new Image();
      decoded.src = src;
      await decoded.decode();
    }));
  }, scene);
  const svgs = page.locator("#pages svg.page");
  assert.equal(await svgs.count(), 1);
  const imageCount = await page.locator("#pages svg image").count();
  const pictureFrames = scene.nodes.filter((node) => node.kind === "picture_frame").length;
  const imageStats = await page.locator("#pages svg image").evaluateAll((nodes) => nodes.map((img) => ({
    resource_id: img.getAttribute("data-resource-id"),
    visible_width: img.getBoundingClientRect().width,
    visible_height: img.getBoundingClientRect().height,
    is_data_url: img.getAttribute("href")?.startsWith("data:image/") === true,
  })));
  assert.ok(imageStats.every((s) => s.is_data_url && s.visible_width > 0 && s.visible_height > 0));
  const png = await svgs.first().screenshot({ path: join(resolve(output), mode + ".png") });
  return {
    mode,
    image_count: imageCount,
    picture_frame_count: pictureFrames,
    source_sha256: scene.source_hash,
    page_id_sha256: sha256(Buffer.from(scene.pages[0].page_id, "utf8")),
    screenshot_sha256: sha256(png),
    screenshot_bytes: png.length,
  };
}
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 2500, height: 2500 }, deviceScaleFactor: 1 });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("request", (request) => {
    if (!request.url().startsWith(origin) && !request.url().startsWith("data:")) {
      foreign.push(request.url());
    }
  });
  const before = await capture(page, noImages, "before");
  const after = await capture(page, readerScene, "after");
  assert.equal(before.image_count, 0, "source-exact JPEG must be unavailable in control");
  assert.equal(after.image_count, pinned.images, "all host-owned images must reach Chromium");
  assert.equal(before.picture_frame_count, after.picture_frame_count);
  assert.equal(before.page_id_sha256, after.page_id_sha256);
  // A real host-derived image can be hidden by page composition or be visually
  // unchanged. That is a measured zero-gain case, not a reason to discard the
  // fixed-cohort result. Retain the historical 022 improvement gate.
  if (visualCaseId === "022") {
    assert.notEqual(before.screenshot_sha256, after.screenshot_sha256);
  }
  assert.deepEqual(pageErrors, [], "Chromium must not throw during scene paint");
  assert.deepEqual(foreign, [], "source-neutral scene must not request external URLs");
  const receipt = {
    schema: visualCaseId === "022" ? "chaptera.product-safe-preview-022-browser.v1" : "chaptera.product-safe-preview-multiimage-browser.v1",
    fixture: visualCaseId,
    repository_commit_sha: exactHead,
    browser: "chromium",
    source_sha256: readerScene.source_hash,
    image_sanitizer_ablation: true,
    raster_change_observed: before.screenshot_sha256 !== after.screenshot_sha256,
    same_scene_and_source: true,
    external_url_requests: 0,
    page_errors: 0,
    before,
    after,
  };
  await writeFile(join(resolve(output), "browser-receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
  console.log("PRODUCT_PREVIEW_BROWSER_022 " + JSON.stringify(receipt));
} finally {
  if (browser) await browser.close();
  server.close();
}
