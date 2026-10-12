#!/usr/bin/env node
// Real OIDC-authenticated Product PUB022: SourceIngress -> isolated reader
// -> host re-encode -> authenticated HTTP -> Chromium SVG. No mocked routes.
// Scene, PUB, cookies, and image bytes remain on the ephemeral CI runner.
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname } from "node:path";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const fixturePath = process.env.CHAPTERA_PREVIEW_022_FIXTURE;
const expectedSha = process.env.CHAPTERA_PREVIEW_022_SHA256;
const storageState = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const scenePath = process.env.CHAPTERA_PREVIEW_022_SCENE_OUT;
const summaryPath = process.env.CHAPTERA_PREVIEW_022_SUMMARY;
if ([origin, fixturePath, expectedSha, storageState, privatePath, scenePath, summaryPath].some(v => !v)) {
  throw new Error("Product 022 requires pinned source, saved real OIDC session and private receipt paths");
}
if (expectedSha !== "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c") {
  throw new Error("Product acceptance must use the pinned Publisher 022 source");
}
const pub = await readFile(fixturePath);
if (pub.length !== 5012480 ||
    createHash("sha256").update(pub).digest("hex") !== expectedSha) {
  throw new Error("real Publisher 022 source identity mismatch");
}
const browser = await chromium.launch({
  headless: true,
  args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  ...(existsSync(storageState) ? { storageState } : {}),
  viewport: { width: 1450, height: 1100 },
});
const page = await context.newPage();
const errors = [];
const foreign = [];
page.on("pageerror", err => errors.push(String(err)));
page.on("request", request => {
  const url = request.url();
  if (!url.startsWith(origin + "/") && !url.startsWith("data:")) foreign.push(url);
});
try {
  // This cookie was obtained by the real authorization-code/PKCE browser
  // journey, NOT assembled or injected by the test.
  let cookies = await context.cookies(origin);
  if (!cookies.some(c => c.name === "__Host-chaptera_session")) {
    // Fresh browser must complete the actual OIDC authorization-code / PKCE
    // redirect; never synthesize or inject an authentication cookie.
    await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
    await page.waitForFunction(
      () => document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
      null, { timeout: 30000 },
    );
    await context.storageState({ path: storageState });
    cookies = await context.cookies(origin);
  }
  const session = cookies.find(c => c.name === "__Host-chaptera_session");
  if (!session || !session.secure || !session.httpOnly ||
      session.sameSite !== "Lax" || session.domain !== "edge.test") {
    throw new Error("real OIDC browser session not preserved");
  }
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
    null, { timeout: 30000 },
  );
  const sessionStatus = await page.evaluate(async () => {
    const response = await fetch("/v1/session", {
      credentials: "same-origin", cache: "no-store",
    });
    const json = await response.json().catch(() => null);
    return { status: response.status, principal: Boolean(json?.principal_id) };
  });
  if (sessionStatus.status !== 200 || !sessionStatus.principal) {
    throw new Error("Product 022 requires a genuine authenticated OIDC principal");
  }

  await page.locator("#pub-file").setInputFiles({
    name: "Pinned022.pub",
    mimeType: "application/x-mspublisher",
    buffer: pub,
  });
  await page.waitForURL(url =>
    /^\/editor\/doc\/document:[0-9a-f]{24}$/.test(decodeURIComponent(url.pathname)),
    { timeout: 150000, waitUntil: "domcontentloaded" },
  );
  const documentId = decodeURIComponent(new URL(page.url()).pathname)
    .replace("/editor/doc/", "");
  const result = await page.evaluate(async id => {
    const currentResponse = await fetch(
      "/v1/documents/" + encodeURIComponent(id) + "/current",
      { credentials: "same-origin", cache: "no-store" },
    );
    const current = await currentResponse.json().catch(() => null);
    const response = await fetch(
      "/v1/reader/documents/" + encodeURIComponent(id) + "/scene",
      { credentials: "same-origin", cache: "no-store" },
    );
    const scene = await response.json().catch(() => null);
    return {
      currentStatus: currentResponse.status,
      sceneStatus: response.status,
      cacheControl: response.headers.get("cache-control"),
      current, scene,
    };
  }, documentId);
  const { scene, current } = result;
  if (result.currentStatus !== 200 || result.sceneStatus !== 200 ||
      result.cacheControl !== "no-store" || current?.document_id !== documentId ||
      !current?.revision_id || scene?.protocol_version !== "chaptera.reader-scene.v1" ||
      scene.document_id !== documentId || scene.revision_id !== current.revision_id ||
      scene.source_hash !== expectedSha || scene.pages?.length !== 1 ||
      scene.fidelity?.state !== "partial") {
    throw new Error("authenticated Product HTTP did not preserve pinned source and current revision");
  }
  if (!(scene.fidelity.reasons ?? []).includes("safe_preview_color_untransformed")) {
    throw new Error("host preview lost its explicit partial color-fidelity disposition");
  }
  if (scene.project || scene.authoring_graph || scene.source_bytes) {
    throw new Error("Product Reader returned forbidden raw authoring or PUB source payload");
  }
  const resources = scene.resources ?? [];
  const admitted = resources.filter(resource =>
    resource.mime === "image/jpeg" &&
    resource.availability === "inline_data_url" &&
    typeof resource.inline_data_url === "string");
  if (resources.length !== 1 || admitted.length !== 1) {
    throw new Error("pinned Product 022 did not admit exactly one derived JPEG");
  }
  const resource = admitted[0];
  if (!resource.inline_data_url.startsWith("data:image/jpeg;base64,") ||
      resource.inline_data_url.length > 7 * 1024 * 1024 ||
      !(scene.nodes ?? []).some(node => node.resource_id === resource.resource_id)) {
    throw new Error("host-derived JPEG lacked an authorized scene image-frame binding");
  }
  const encoded = Buffer.from(resource.inline_data_url.split(",")[1], "base64");
  if (encoded.length < 100 || encoded.length === 4869956 ||
      encoded.slice(0, 3).toString("hex") !== "ffd8ff") {
    throw new Error("Product route forwarded original JPEG or non-JPEG bytes");
  }

  await page.waitForFunction(
    revision => document.querySelector("#status")?.classList.contains("ok") &&
      document.querySelector("#status")?.textContent?.includes(revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length === 1 &&
      document.querySelectorAll("#canvas svg image[data-resource-id]").length > 0,
    current.revision_id, { timeout: 45000 },
  );
  const paint = await page.locator("#canvas svg image[data-resource-id]").evaluateAll(
    async images => {
      return Promise.all(images.map(async image => {
        const href = image.getAttribute("href") || image.getAttribute("xlink:href");
        if (!href?.startsWith("data:image/jpeg;base64,")) {
          throw new Error("browser displayed an untrusted image URL");
        }
        const decoded = new Image();
        decoded.src = href;
        await decoded.decode();
        const rect = image.getBoundingClientRect();
        return {
          resource_id: image.getAttribute("data-resource-id"),
          visible: rect.width > 0 && rect.height > 0,
          decoded: decoded.naturalWidth > 0 && decoded.naturalHeight > 0,
        };
      }));
    },
  );
  if (paint.length !== 1 || paint[0].resource_id !== resource.resource_id ||
      !paint[0].visible || !paint[0].decoded || errors.length || foreign.length) {
    throw new Error("authenticated Product JPEG failed Chromium decode/paint or fetched an external URL");
  }

  // Fresh, cookie-free browser context must not receive this Scene.
  const anon = await browser.newContext({ ignoreHTTPSErrors: true });
  try {
    const anonymousPage = await anon.newPage();
    await anonymousPage.goto(origin + "/live", { waitUntil: "domcontentloaded" });
    const anonymousStatus = await anonymousPage.evaluate(async id => {
      const response = await fetch("/v1/reader/documents/" +
        encodeURIComponent(id) + "/scene", { credentials: "same-origin", cache: "no-store" });
      return response.status;
    }, documentId);
    if (anonymousStatus !== 401) {
      throw new Error("anonymous Product Reader request did not return 401");
    }
  } finally {
    await anon.close();
  }

  // PRIVATE files used only for image-on/off Publisher A/B and cross-principal/
  // restart controls in the same ephemeral job. Never upload these paths.
  await mkdir(dirname(scenePath), { recursive: true });
  await writeFile(scenePath, JSON.stringify(scene), "utf8");
  await writeFile(privatePath, JSON.stringify({
    document_id: documentId,
    revision_id: current.revision_id,
    source_sha256: expectedSha,
    resource_id: resource.resource_id,
  }), "utf8");
  const receipt = {
    schema: "chaptera.product-oidc-022-safe-preview.v1",
    repository_commit_sha: process.env.CHAPTERA_HEAD_SHA ?? "unverified",
    real_oidc_pkce_session_reused: true,
    tls_product_source_ingress: true,
    exact_source_and_revision: true,
    actual_product_http_status: 200,
    isolated_worker_and_host_owned_image: true,
    derived_jpeg_count: 1,
    scene_jpeg_count: resources.length,
    browser_svg_image_count: paint.length,
    chromium_pixels_decoded: true,
    anonymous_scene_status: 401,
    source_container_forwarded: false,
    external_image_requests: 0,
    browser_errors: 0,
    fidelity_state: "partial",
    production_s3_claim: false,
    deployed_host_claim: false,
  };
  await mkdir(dirname(summaryPath), { recursive: true });
  await writeFile(summaryPath, JSON.stringify(receipt, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_OIDC_022 " + JSON.stringify(receipt) + "\n");
} finally {
  await context.close();
  await browser.close();
}
