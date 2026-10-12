#!/usr/bin/env node
// Reuse the original genuine owner OIDC session after Chaptera server AND
// isolated worker are killed and independently restarted. Prove identical
// durable source/revision and actual host-derived image paint in Chromium.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const cases = {
  "001": { sha: "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", images: 1 },
  "023": { sha: "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", images: 7 },
};
const id = process.env.CHAPTERA_PRODUCT_MULTI_ID;
const fixture = cases[id];
const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const statePath = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const summaryPath = process.env.CHAPTERA_PRODUCT_022_RESTART_SUMMARY;
if (!fixture || !origin || !statePath || !privatePath || !summaryPath) {
  throw new Error("original Product multi-image OIDC restart scope missing");
}
const original = JSON.parse(await readFile(privatePath, "utf8"));
if (!/^document:[0-9a-f]{24}$/.test(original.document_id ?? "") ||
    typeof original.revision_id !== "string" || !original.revision_id ||
    original.source_sha256 !== fixture.sha ||
    !Array.isArray(original.resources) ||
    original.resources.length !== fixture.images) {
  throw new Error("original Product source/revision/resource receipt missing");
}
const originalIds = new Map();
for (const item of original.resources) {
  if (!item?.resource_id || originalIds.has(item.resource_id) ||
      !["image/png", "image/jpeg"].includes(item.mime)) {
    throw new Error("original Product image resource identity invalid");
  }
  originalIds.set(item.resource_id, item.mime);
}
const browser = await chromium.launch({
  headless: true, args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  storageState: statePath, ignoreHTTPSErrors: true,
  viewport: { width: 1450, height: 1100 },
});
try {
  // Do not permit silent sign-in as a fresh identity after restart.
  const cookies = await context.cookies(origin);
  const session = cookies.find(c => c.name === "__Host-chaptera_session");
  if (!session?.secure || !session?.httpOnly || session.sameSite !== "Lax") {
    throw new Error("original OIDC session missing before process restart reopen");
  }
  const page = await context.newPage();
  const external = [];
  page.on("request", request => {
    if (!request.url().startsWith(origin + "/") &&
        !request.url().startsWith("data:")) external.push("unexpected");
  });
  const docId = original.document_id;
  const matches = (response, path) =>
    response.request().method() === "GET" &&
    decodeURIComponent(new URL(response.url()).pathname) === path;
  const currentResponse = page.waitForResponse(
    response => matches(response, "/v1/documents/" + docId + "/current"),
    { timeout: 150000 },
  );
  const sceneResponse = page.waitForResponse(
    response => matches(response, "/v1/reader/documents/" + docId + "/scene"),
    { timeout: 150000 },
  );
  await page.goto(origin + "/editor/doc/" + encodeURIComponent(docId), {
    waitUntil: "domcontentloaded",
  });
  const [currentHttp, sceneHttp] = await Promise.all([currentResponse, sceneResponse]);
  const [current, scene] = await Promise.all([
    currentHttp.json().catch(() => null),
    sceneHttp.json().catch(() => null),
  ]);
  if (currentHttp.status() !== 200 || sceneHttp.status() !== 200 ||
      sceneHttp.headers()["cache-control"] !== "no-store" ||
      current?.document_id !== docId || current?.revision_id !== original.revision_id ||
      scene?.protocol_version !== "chaptera.reader-scene.v1" ||
      scene?.document_id !== docId || scene?.revision_id !== original.revision_id ||
      scene?.source_hash !== fixture.sha || scene?.pages?.length !== 1 ||
      scene?.fidelity?.state !== "partial" || external.length) {
    throw new Error("Product owner source/revision/Scene not recovered after actual restart");
  }
  const admitted = (scene.resources ?? []).filter(resource =>
    ["image/png", "image/jpeg"].includes(resource.mime) &&
    resource.availability === "inline_data_url" &&
    typeof resource.inline_data_url === "string");
  if (admitted.length !== fixture.images ||
      new Set(admitted.map(r => r.resource_id)).size !== fixture.images) {
    throw new Error("restarted Product host image resource count/identity drift");
  }
  for (const resource of admitted) {
    if (originalIds.get(resource.resource_id) !== resource.mime ||
        !resource.inline_data_url.startsWith("data:" + resource.mime + ";base64,") ||
        !(scene.nodes ?? []).some(node =>
          node.resource_id === resource.resource_id ||
          (node.decorative_border?.placements ?? []).some(p => p.resource_id === resource.resource_id))) {
      throw new Error("restarted Product image has wrong mime, identity or source-node binding");
    }
    const encoded = Buffer.from(resource.inline_data_url.split(",")[1], "base64");
    const limit = (resource.mime === "image/png" ? 4 : 5) * 1024 * 1024;
    const magic = resource.mime === "image/png" ? "89504e470d0a1a0a" : "ffd8ff";
    if (!encoded.length || encoded.length > limit ||
        encoded.subarray(0, magic.length / 2).toString("hex") !== magic) {
      throw new Error("restarted Product image bytes not host-valid PNG/JPEG");
    }
  }
  await page.waitForFunction(
    values => document.querySelector("#status")?.classList.contains("ok") &&
      document.querySelector("#status")?.textContent?.includes(values.revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length === 1 &&
      document.querySelectorAll("#canvas svg image[data-resource-id]").length >= values.count,
    { revision: original.revision_id, count: fixture.images },
    { timeout: 60000 },
  );
  const painted = await page.locator("#canvas svg image[data-resource-id]").evaluateAll(
    async nodes => Promise.all(nodes.map(async node => {
      const src = node.getAttribute("href") || node.getAttribute("xlink:href");
      if (!/^data:image\/(png|jpeg);base64,/.test(src ?? "")) {
        throw new Error("restarted browser requested non-host image bytes");
      }
      const loaded = new Image();
      loaded.src = src;
      await loaded.decode();
      const box = node.getBoundingClientRect();
      return {
        id: node.getAttribute("data-resource-id"),
        decoded: loaded.naturalWidth > 0 && loaded.naturalHeight > 0,
        visible: box.width > 0 && box.height > 0,
      };
    })),
  );
  const actualIds = new Set(
    painted.filter(p => p.decoded && p.visible).map(p => p.id));
  if (actualIds.size !== fixture.images ||
      [...originalIds.keys()].some(rid => !actualIds.has(rid)) ||
      external.length) {
    throw new Error("restarted real Chromium failed exact host-owned image paint");
  }
  const summary = {
    schema: "chaptera.product-oidc-multiimage-restart.v1",
    fixture: id,
    original_oidc_owner_session_preserved: true,
    server_and_isolated_worker_restarted: true,
    source_revision_page_and_resource_ids_stable: true,
    safely_reencoded_image_count: fixture.images,
    chromium_images_decoded_and_painted: fixture.images,
    unauthorized_external_requests: 0,
    source_image_scene_session_payloads_uploaded: false,
    deployed_host_claim: false,
  };
  await writeFile(summaryPath, JSON.stringify(summary, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_MULTI_RESTART " + JSON.stringify(summary) + "\n");
} finally {
  await context.close();
  await browser.close();
}
