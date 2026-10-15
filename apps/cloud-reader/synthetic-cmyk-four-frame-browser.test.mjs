// Browser proof for the synthetic one-CMYK-resource/four-placement SafePreview
// receipt emitted by the real Rust worker->host functions. No user PUB or image.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const scenePath = process.env.CHAPTERA_SYNTHETIC_CMYK_SCENE_OUT;
assert.ok(scenePath, "only the synthetic host-admitted Scene path is supported");
const scene = JSON.parse(await readFile(resolve(scenePath), "utf8"));
assert.equal(scene.protocol_version, "chaptera.reader-scene.v1");
assert.equal(scene.document_id, "synthetic-four-cmyk-frames");
assert.equal(scene.source_hash, "a".repeat(64));
assert.equal(scene.resources.length, 1);
assert.equal(scene.resources[0].availability, "inline_data_url");
assert.ok(scene.resources[0].derivative_id.startsWith("safe-preview:"));
assert.equal(scene.nodes.length, 4);
assert.equal(new Set(scene.nodes.map((node) => node.node_id)).size, 4);
assert.ok(scene.nodes.every((node) => node.resource_id === "star-image"));

const noImage = structuredClone(scene);
noImage.resources[0].availability = "descriptor_only";
delete noImage.resources[0].inline_data_url;
const html = "<!doctype html><html><body style='margin:0;background:white'><main id='pages'></main></body></html>";
const server = createServer(async (request, response) => {
  if (request.url === "/render-v1.mjs") {
    response.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store" });
    response.end(await readFile(join(root, "render-v1.mjs")));
  } else if (request.url === "/") {
    response.writeHead(200, { "content-type": "text/html" });
    response.end(html);
  } else {
    response.writeHead(404);
    response.end();
  }
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const origin = "http://127.0.0.1:" + server.address().port;
const outsideRequests = [];
const pageErrors = [];
let browser;
const sha256 = (buffer) => createHash("sha256").update(buffer).digest("hex");

async function inspect(page, payload) {
  await page.goto(origin);
  await page.evaluate(async (renderScene) => {
    const { renderReaderScene } = await import("/render-v1.mjs");
    await renderReaderScene(document.querySelector("#pages"), renderScene, {
      maxPageWidth: 920,
    });
    await Promise.all([...document.querySelectorAll("svg image")].map(async (painted) => {
      const url = painted.getAttribute("href");
      if (!url?.startsWith("data:image/jpeg;base64,")) {
        throw new Error("the browser was not given a host-owned JPEG derivative");
      }
      const image = new Image();
      image.src = url;
      await image.decode();
      if (image.naturalWidth !== 2 || image.naturalHeight !== 2) {
        throw new Error("decoded SafePreview dimensions drifted");
      }
    }));
  }, payload);
  const svg = page.locator("#pages > svg.page");
  assert.equal(await svg.count(), 1);
  const painted = await page.locator("#pages svg image").evaluateAll((images) =>
    images.map((image) => {
      const bounds = image.getBoundingClientRect();
      return {
        resourceId: image.getAttribute("data-resource-id"),
        width: bounds.width,
        height: bounds.height,
        x: bounds.x,
        y: bounds.y,
      };
    }));
  const missing = await page.locator("#pages [data-resource-missing]").count();
  const screenshot = await svg.screenshot();
  return { painted, missing, screenshotHash: sha256(screenshot) };
}

try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1000, height: 1000 }, deviceScaleFactor: 1 });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("request", (request) => {
    if (!request.url().startsWith(origin) && !request.url().startsWith("data:")) {
      outsideRequests.push(request.url());
    }
  });
  const before = await inspect(page, noImage);
  const after = await inspect(page, scene);
  assert.equal(before.painted.length, 0, "unadmitted image must not reach Chromium");
  assert.equal(before.missing, 4, "negative control must preserve four missing placeholders");
  assert.equal(after.painted.length, 4, "four source picture placements must all paint");
  assert.equal(after.missing, 0, "admitted derivative must replace every missing placeholder");
  assert.ok(after.painted.every((node) => node.resourceId === "star-image"
    && node.width > 0 && node.height > 0));
  assert.equal(new Set(after.painted.map((node) => node.x + "/" + node.y)).size, 4);
  assert.notEqual(before.screenshotHash, after.screenshotHash,
    "the four real browser placements must change painted pixels");
  assert.deepEqual(outsideRequests, [], "all pictures must stay in the trusted data URL");
  assert.deepEqual(pageErrors, [], "no real Chromium page errors permitted");
  console.log("CHAPTERA_CMYK_FOUR_FRAME_CHROMIUM " + JSON.stringify({
    synthetic_fixture: true,
    one_host_owned_derivative: true,
    picture_frames: after.painted.length,
    resource_count: scene.resources.length,
    missing_placeholders_before: before.missing,
    missing_placeholders_after: after.missing,
    external_requests: 0,
    page_errors: 0,
    pixels_changed: true
  }));
} finally {
  await browser?.close();
  server.close();
}
