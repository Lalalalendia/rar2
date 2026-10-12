// Real Product-host-admitted Scene -> unchanged Reader renderer -> Chromium.
// The OIDC/HTTPS SourceIngress acceptance runs first; this script NEVER grants
// access to a Source or chooses a Scene. All source and PNG bytes stay private.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const scenePath = process.env.CHAPTERA_PREVIEW_PROBE_SCENE_OUT;
const outputPath = process.env.CHAPTERA_PREVIEW_BROWSER_OUTPUT;
const sourceSha = process.env.CHAPTERA_PRODUCT_VISUAL_SOURCE_SHA256;
const expectedPages = Number(process.env.CHAPTERA_PRODUCT_VISUAL_PAGES);
const exactHead = process.env.REPOSITORY_COMMIT_SHA;
assert.ok(scenePath && outputPath, "ephemeral host Product Scene/output are required");
assert.match(sourceSha ?? "", /^[0-9a-f]{64}$/, "pinned source SHA is required");
assert.match(exactHead ?? "", /^[0-9a-f]{40}$/, "exact checked-out head is required");
assert.ok(Number.isSafeInteger(expectedPages) && expectedPages >= 1 && expectedPages <= 100,
  "bounded pinned Publisher page count required");
const scene = JSON.parse(await readFile(resolve(scenePath), "utf8"));
assert.equal(scene.protocol_version, "chaptera.reader-scene.v1");
assert.equal(scene.source_hash, sourceSha);
assert.ok(["supported", "partial"].includes(scene.fidelity?.state));
assert.equal(scene.pages.length, expectedPages, "Product PAGE count must match the frozen Publisher pair");
assert.match(scene.document_id ?? "", /^document:[0-9a-f]{24}$/, "must be an authenticated Product document");
assert.equal(typeof scene.revision_id, "string");
assert.ok(scene.revision_id.length > 0);
assert.ok(Buffer.byteLength(JSON.stringify(scene), "utf8") <= 16 * 1024 * 1024);
const sha = value => createHash("sha256").update(value).digest("hex");
const pages = [...scene.pages].sort((a, b) => a.order - b.order);
assert.equal(new Set(pages.map(p => p.page_id)).size, pages.length);
const hostImages = (scene.resources ?? []).filter(r =>
  ["image/png", "image/jpeg"].includes(r.mime) &&
  r.availability === "inline_data_url" && typeof r.inline_data_url === "string");
const imageIds = new Set();
assert.ok(hostImages.length <= 96, "Product image resource budget");
for (const resource of hostImages) {
  assert.ok(resource.resource_id && !imageIds.has(resource.resource_id),
    "duplicated host-image binding");
  imageIds.add(resource.resource_id);
  const prefix = "data:" + resource.mime + ";base64,";
  assert.ok(resource.inline_data_url.startsWith(prefix), "host image type and bytes");
  const encoded = Buffer.from(resource.inline_data_url.slice(prefix.length), "base64");
  const maxBytes = (resource.mime === "image/png" ? 4 : 5) * 1024 * 1024;
  assert.ok(encoded.length > 0 && encoded.length <= maxBytes);
  const expectedMagic = resource.mime === "image/png" ? "89504e470d0a1a0a" : "ffd8ff";
  assert.equal(encoded.subarray(0, expectedMagic.length / 2).toString("hex"), expectedMagic);
  assert.ok((scene.nodes ?? []).some(n => n.resource_id === resource.resource_id ||
    (n.decorative_border?.placements ?? []).some(p => p.resource_id === resource.resource_id)));
}
await mkdir(resolve(outputPath), { recursive: true });
const root = dirname(fileURLToPath(import.meta.url));
const server = createServer(async (req, res) => {
  try {
    if (req.url === "/render-v1.mjs") {
      res.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store" });
      res.end(await readFile(join(root, "render-v1.mjs")));
    } else if (req.url === "/") {
      res.writeHead(200, { "content-type": "text/html", "cache-control": "no-store" });
      res.end('<!doctype html><html><body style="margin:0;background:#fff"><main id="pages"></main></body></html>');
    } else {
      res.writeHead(404);
      res.end();
    }
  } catch {
    res.writeHead(500);
    res.end();
  }
});
await new Promise(ok => server.listen(0, "127.0.0.1", ok));
const origin = "http://127.0.0.1:" + server.address().port;
let browser;
const foreign = [];
const errors = [];
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 2600, height: 2000 }, deviceScaleFactor: 1 });
  page.on("pageerror", error => errors.push("error"));
  page.on("request", request => {
    if (!request.url().startsWith(origin + "/") && !request.url().startsWith("data:"))
      foreign.push("external");
  });
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await page.evaluate(async accepted => {
    const { renderReaderScene } = await import("/render-v1.mjs");
    await renderReaderScene(document.querySelector("#pages"), accepted, {
      emuPerCssPx: 914400 / 144,
      maxPageWidth: 10000,
    });
    await document.fonts.ready;
    await Promise.all([...document.querySelectorAll("#pages svg image")].map(async image => {
      const src = image.getAttribute("href") || image.getAttribute("xlink:href");
      if (!/^data:image\/(?:png|jpeg);base64,/.test(src ?? "")) {
        throw new Error("Product SVG fetched a non-hosted image");
      }
      const decoded = new Image();
      decoded.src = src;
      await decoded.decode();
      if (!(decoded.naturalWidth > 0 && decoded.naturalHeight > 0)) {
        throw new Error("Product SVG image decode failure");
      }
    }));
  }, scene);
  const svgs = page.locator("#pages svg.page");
  assert.equal(await svgs.count(), pages.length);
  const observedIds = new Set(await page.locator("#pages svg image[data-resource-id]")
    .evaluateAll(elements => elements.map(element => element.getAttribute("data-resource-id"))));
  assert.deepEqual([...observedIds].sort(), [...imageIds].sort(),
    "the entire admitted host image set must paint in Chromium");
  const outputPages = [];
  for (let i = 0; i < pages.length; i++) {
    const pg = pages[i];
    assert.ok(Number.isSafeInteger(pg.width_emu) && pg.width_emu > 0);
    assert.ok(Number.isSafeInteger(pg.height_emu) && pg.height_emu > 0);
    const svg = svgs.nth(i);
    assert.equal(await svg.getAttribute("data-page-id"), pg.page_id);
    const width = Math.round(pg.width_emu / (914400 / 144));
    const height = Math.round(pg.height_emu / (914400 / 144));
    assert.ok(width > 0 && height > 0 && width <= 30000 && height <= 30000);
    await svg.evaluate((element, dims) => {
      element.setAttribute("width", String(dims.width));
      element.setAttribute("height", String(dims.height));
      element.style.position = "fixed";
      element.style.left = "0";
      element.style.top = "0";
      element.style.zIndex = "2147483647";
    }, {width, height});
    const png = await svg.screenshot({ path: join(resolve(outputPath), "page-" + (i + 1) + ".png") });
    assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    assert.equal(png.readUInt32BE(16), width, "Product page raster must use 144dpi geometry");
    assert.equal(png.readUInt32BE(20), height, "Product page raster must use 144dpi geometry");
    const painted = await svg.locator("image[data-resource-id]").evaluateAll(elements =>
      elements.map(element => element.getAttribute("data-resource-id")));
    await svg.evaluate(element => {
      element.style.removeProperty("position");
      element.style.removeProperty("left");
      element.style.removeProperty("top");
      element.style.removeProperty("z-index");
    });
    outputPages.push({
      page: i + 1,
      product_page_identity_sha256: sha(Buffer.from(pg.page_id, "utf8")),
      chromium_png_sha256: sha(png),
      png_dimensions: { width, height },
      width_emu: pg.width_emu,
      height_emu: pg.height_emu,
      host_derived_resource_count: new Set(painted).size,
    });
  }
  assert.deepEqual(errors, [], "Product Scene renderer threw in Chromium");
  assert.deepEqual(foreign, [], "Product Scene fetched external resource");
  const receipt = {
    schema: "chaptera.product-host-visual-chromium.v1",
    repository_commit_sha: exactHead,
    source_sha256: sourceSha,
    revision_identity_sha256: sha(Buffer.from(
      scene.document_id + "\0" + scene.revision_id + "\0" + sourceSha, "utf8")),
    fidelity_state: scene.fidelity.state,
    authenticated_product_scene_provided_by_owner_runner: true,
    host_image_count: imageIds.size,
    external_request_count: 0,
    page_errors: 0,
    reference_raster_dpi: 144,
    pages: outputPages,
  };
  await writeFile(join(resolve(outputPath), "browser-receipt.json"),
    JSON.stringify(receipt, null, 2) + "\n", "utf8");
  console.log("PRODUCT_HOST_VISUAL_BOUNDED " + JSON.stringify({
    schema: receipt.schema, repository_commit_sha: exactHead,
    page_count: outputPages.length, host_image_count: imageIds.size,
    privacy: "scene_and_png_ephemeral",
  }));
} finally {
  if (browser) await browser.close();
  await new Promise(ok => server.close(ok));
}
