// Same-source Publisher A/B for authenticated host-admitted Product PUB001/023.
// The scene is captured from the REAL OIDC/HTTPS Product response by the
// existing physical acceptance, not composed or authorized here.
// Screenshots and original image bytes stay on the ephemeral runner.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const cases = {
  "001": {
    source: "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade",
    images: 1,
  },
  "023": {
    source: "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8",
    images: 7,
  },
};
const id = process.env.CHAPTERA_PRODUCT_MULTI_ID;
const expected = cases[id];
const scenePath = process.env.CHAPTERA_PREVIEW_PROBE_SCENE_OUT;
const outputPath = process.env.CHAPTERA_PREVIEW_BROWSER_OUTPUT;
const head = process.env.REPOSITORY_COMMIT_SHA;
assert.ok(expected, "only pinned Product001/023 may enter visual comparison");
assert.ok(scenePath && outputPath, "private accepted scene and private output are required");
assert.match(head ?? "", /^[0-9a-f]{40}$/, "exact repository head is required");

const scene = JSON.parse(await readFile(resolve(scenePath), "utf8"));
assert.equal(scene.protocol_version, "chaptera.reader-scene.v1");
assert.equal(scene.source_hash, expected.source);
assert.equal(scene.pages?.length, 1, "pinned Publisher pair must have one PAGE");
assert.equal(scene.fidelity?.state, "partial", "unproved color must remain Partial");
const images = new Set(["image/png", "image/jpeg"]);
const accepted = (scene.resources ?? []).filter(r =>
  images.has(r.mime) && r.availability === "inline_data_url" &&
  typeof r.inline_data_url === "string");
assert.equal(accepted.length, expected.images, "Product host-owned image count drift");
assert.equal(new Set(accepted.map(r => r.resource_id)).size, expected.images,
  "duplicate Product resource IDs");

const without = structuredClone(scene);
for (const resource of without.resources ?? []) {
  if (images.has(resource.mime)) {
    delete resource.inline_data_url;
    resource.availability = "descriptor_only";
  }
}
assert.deepEqual(
  without.resources.filter(r => !images.has(r.mime)),
  scene.resources.filter(r => !images.has(r.mime)),
  "font/nonimage resources must be byte-preserved by image-only ablation"
);
const root = dirname(fileURLToPath(import.meta.url));
const server = createServer(async (request, response) => {
  try {
    if (request.url === "/render-v1.mjs") {
      response.writeHead(200, {
        "content-type": "text/javascript", "cache-control": "no-store",
      });
      response.end(await readFile(join(root, "render-v1.mjs")));
    } else if (request.url === "/") {
      response.writeHead(200, {
        "content-type": "text/html", "cache-control": "no-store",
      });
      response.end('<!doctype html><html><body style="margin:0;background:white"><main id="pages"></main></body></html>');
    } else {
      response.writeHead(404);
      response.end();
    }
  } catch {
    response.writeHead(500);
    response.end();
  }
});
await mkdir(resolve(outputPath), { recursive: true });
await new Promise(ok => server.listen(0, "127.0.0.1", ok));
const origin = "http://127.0.0.1:" + server.address().port;
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
let browser;
const errors = [];
const external = [];

async function capture(page, data, mode) {
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await page.evaluate(async payload => {
    const { renderReaderScene } = await import("/render-v1.mjs");
    await renderReaderScene(document.querySelector("#pages"), payload, {
      emuPerCssPx: 914400 / 144, maxPageWidth: 10000,
    });
    await document.fonts.ready;
    await Promise.all([...document.querySelectorAll("#pages svg image")].map(async element => {
      const src = element.getAttribute("href") || element.getAttribute("xlink:href");
      if (!src || !/^data:image\/(png|jpeg);base64,/.test(src)) {
        throw new Error("only host-derived data:image PNG/JPEG may be painted");
      }
      const decoded = new Image();
      decoded.src = src;
      await decoded.decode();
      if (!(decoded.naturalWidth > 0 && decoded.naturalHeight > 0)) {
        throw new Error("Product image did not decode");
      }
    }));
  }, data);
  const svgs = page.locator("#pages svg.page");
  assert.equal(await svgs.count(), 1, "source PAGE membership cannot change");
  const painted = await page.locator("#pages svg image").evaluateAll(nodes => nodes.map(node => ({
    resource_id: node.getAttribute("data-resource-id"),
    width: node.getBoundingClientRect().width,
    height: node.getBoundingClientRect().height,
  })));
  assert.ok(painted.every(p => p.width > 0 && p.height > 0 && p.resource_id),
    "Product preview must occupy a real SVG frame");
  const png = await svgs.first().screenshot({ path: join(resolve(outputPath), mode + ".png") });
  assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  return {
    page_id_sha256: sha256(Buffer.from(data.pages[0].page_id, "utf8")),
    image_count: painted.length,
    unique_painted_resource_ids: new Set(painted.map(p => p.resource_id)).size,
    picture_frame_count: data.nodes.filter(n => n.kind === "picture_frame").length,
    screenshot_sha256: sha256(png),
    png_dimensions: { width: png.readUInt32BE(16), height: png.readUInt32BE(20) },
  };
}
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({
    viewport: { width: 2500, height: 2500 }, deviceScaleFactor: 1,
  });
  page.on("pageerror", error => errors.push(error.message));
  page.on("request", request => {
    if (!request.url().startsWith(origin) && !request.url().startsWith("data:")) {
      external.push("external");
    }
  });
  const before = await capture(page, without, "before");
  const after = await capture(page, scene, "after");
  assert.equal(before.image_count, 0, "negative scene must have no images");
  assert.ok(after.image_count >= expected.images, "all host-owned resources must paint");
  assert.equal(after.unique_painted_resource_ids, expected.images);
  assert.equal(before.page_id_sha256, after.page_id_sha256);
  assert.equal(before.picture_frame_count, after.picture_frame_count);
  assert.deepEqual(before.png_dimensions, after.png_dimensions);
  assert.notEqual(before.screenshot_sha256, after.screenshot_sha256,
    "host image admission must have nonzero actual raster effect");
  assert.deepEqual(errors, [], "browser render exception");
  assert.deepEqual(external, [], "external URL request");
  const receipt = {
    schema: "chaptera.product-multiimage-publisher-ab-browser.v1",
    fixture: id,
    source_sha256: expected.source,
    repository_commit_sha: head,
    same_source_scene_head_and_browser: true,
    image_only_ablation: true,
    real_chromium: true,
    external_url_request_count: 0,
    page_error_count: 0,
    host_resource_count: accepted.length,
    before, after,
  };
  await writeFile(join(resolve(outputPath), "browser-receipt.json"),
    JSON.stringify(receipt, null, 2) + "\n", "utf8");
  console.log("PRODUCT_MULTIIMAGE_PUBLISHER_AB " + JSON.stringify(receipt));
} finally {
  if (browser) await browser.close();
  await new Promise(ok => server.close(ok));
}
