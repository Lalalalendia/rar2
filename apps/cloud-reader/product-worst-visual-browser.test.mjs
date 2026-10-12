// Same-source Publisher A/B for authenticated host-admitted Product PUB020/026/043.
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
  "020": { source: "6c6af8064f91ad5321f76c97c00b1a6564794d92614373334de23e2759294aaa", pages: 1 },
  "026": { source: "5c1054b0bd7bd3ae64ab6884e51943e2c5161cdf2dd13803c8d338f652c06cf0", pages: 1 },
  "043": { source: "e635d3b0320073b6d08402945a90e14e13d0294c02262d854b106b9f0088904b", pages: 2 },
};
const id = process.env.CHAPTERA_PRODUCT_MULTI_ID;
const planPath = process.env.CHAPTERA_PRODUCT_CASE_PLAN;
const pinnedPlan = planPath ? JSON.parse(await readFile(planPath, "utf8")) : null;
if (pinnedPlan && (pinnedPlan.id !== id ||
    !/^[0-9a-f]{64}$/.test(pinnedPlan.source_sha256 ?? "") ||
    !Number.isSafeInteger(pinnedPlan.pages) ||
    !(pinnedPlan.pages > 0 && pinnedPlan.pages <= 100))) {
  throw new Error("Product visual case plan must be an exact pinned eligible fixture");
}
const expected = pinnedPlan
  ? {source: pinnedPlan.source_sha256, pages: pinnedPlan.pages} : cases[id];
if (pinnedPlan && cases[id] &&
    (expected.source !== cases[id].source || expected.pages !== cases[id].pages)) {
  throw new Error("Product visual case plan differs from established pilot");
}
const scenePath = process.env.CHAPTERA_PREVIEW_PROBE_SCENE_OUT;
const outputPath = process.env.CHAPTERA_PREVIEW_BROWSER_OUTPUT;
const head = process.env.REPOSITORY_COMMIT_SHA;
assert.ok(expected, "only pinned worst Product020/026/043 may enter visual comparison");
assert.ok(scenePath && outputPath, "private accepted scene and private output are required");
assert.match(head ?? "", /^[0-9a-f]{40}$/, "exact repository head is required");

const scene = JSON.parse(await readFile(resolve(scenePath), "utf8"));
assert.equal(scene.protocol_version, "chaptera.reader-scene.v1");
assert.equal(scene.source_hash, expected.source);
assert.equal(scene.pages?.length, expected.pages, "pinned Publisher PAGE count drift");
assert.ok(["supported", "partial"].includes(scene.fidelity?.state),
  "real Product fidelity disposition required");
const images = new Set(["image/png", "image/jpeg"]);
const accepted = (scene.resources ?? []).filter(r =>
  images.has(r.mime) && r.availability === "inline_data_url" &&
  typeof r.inline_data_url === "string");
assert.ok(accepted.length <= 96, "bounded host-owned image resources required");
if (accepted.length) {
  assert.equal(scene.fidelity.state, "partial",
    "unproved color on admitted host images must remain Partial");
}
assert.equal(new Set(accepted.map(r => r.resource_id)).size, accepted.length,
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
  assert.equal(await svgs.count(), expected.pages, "source PAGE membership cannot change");
  const painted = await page.locator("#pages svg image").evaluateAll(nodes => nodes.map(node => ({
    resource_id: node.getAttribute("data-resource-id"),
    width: node.getBoundingClientRect().width,
    height: node.getBoundingClientRect().height,
  })));
  assert.ok(painted.every(p => p.width > 0 && p.height > 0 && p.resource_id),
    "host preview must occupy real SVG paint bounds");
  const orderedPages = [...data.pages].sort((a,b)=>a.order-b.order);
  const captures = [];
  for (let i=0;i<orderedPages.length;i++) {
    const model=orderedPages[i], svg=svgs.nth(i);
    const pageId=await svg.getAttribute("data-page-id");
    assert.equal(pageId, model.page_id, "canonical source PAGE identity drift");
    assert.ok(model.width_emu>0 && model.height_emu>0, "positive original Publisher page geometry required");
    const vb=await svg.evaluate(el => ({w:el.viewBox.baseVal.width,h:el.viewBox.baseVal.height}));
    assert.ok(Math.abs(vb.w-model.width_emu)<1 && Math.abs(vb.h-model.height_emu)<1,
      "browser projected page extent differs from Product Scene");
    const png=await svg.screenshot({path:join(resolve(outputPath),mode + "-page-" + (i+1) + ".png")});
    assert.equal(png.subarray(0,8).toString("hex"),"89504e470d0a1a0a");
    captures.push({page:i+1,page_id_sha256:sha256(Buffer.from(model.page_id,"utf8")),
      screenshot_sha256:sha256(png),width_emu:model.width_emu,height_emu:model.height_emu,
      png_dimensions:{width:png.readUInt32BE(16),height:png.readUInt32BE(20)}});
  }
  return {
    image_count:painted.length,
    unique_painted_resource_ids:new Set(painted.map(p=>p.resource_id)).size,
    picture_frame_count:data.nodes.filter(n=>n.kind==="picture_frame").length,
    pages:captures,
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
  assert.ok(after.image_count >= accepted.length, "all host-owned resources must paint");
  assert.equal(after.unique_painted_resource_ids, accepted.length);
  assert.equal(before.picture_frame_count, after.picture_frame_count);
  assert.equal(before.pages.length, after.pages.length);
  for (let i=0;i<before.pages.length;i++) {
    assert.equal(before.pages[i].page_id_sha256, after.pages[i].page_id_sha256);
    assert.equal(before.pages[i].width_emu, after.pages[i].width_emu);
    assert.equal(before.pages[i].height_emu, after.pages[i].height_emu);
    assert.deepEqual(before.pages[i].png_dimensions, after.pages[i].png_dimensions);
  }
  assert.deepEqual(errors, [], "browser render exception");
  assert.deepEqual(external, [], "external URL request");
  const receipt = {
    schema: "chaptera.product-worst-publisher-ab-browser.v1",
    fixture: id,
    source_sha256: expected.source,
    repository_commit_sha: head,
    same_source_scene_head_and_browser: true,
    image_only_ablation: true,
    real_chromium: true,
    external_url_request_count: 0,
    page_error_count: 0,
    host_resource_count: accepted.length,
    fidelity_state: scene.fidelity.state,
    before, after,
  };
  await writeFile(join(resolve(outputPath), "browser-receipt.json"),
    JSON.stringify(receipt, null, 2) + "\n", "utf8");
  console.log("PRODUCT_WORST_PUBLISHER_AB " + JSON.stringify({fixture:id,pages:expected.pages,host_images:accepted.length,real_chromium:true}));
} finally {
  if (browser) await browser.close();
  await new Promise(ok => server.close(ok));
}
