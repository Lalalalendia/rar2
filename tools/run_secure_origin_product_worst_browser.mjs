#!/usr/bin/env node
// Real OIDC-authenticated Product pinned PUB020/026/043 diagnostic census
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
const fixtureId = process.env.CHAPTERA_PRODUCT_MULTI_ID;
const known = {
  "020": { sha: "6c6af8064f91ad5321f76c97c00b1a6564794d92614373334de23e2759294aaa", bytes: 9351168, pages: 1 },
  "026": { sha: "5c1054b0bd7bd3ae64ab6884e51943e2c5161cdf2dd13803c8d338f652c06cf0", bytes: 4218368, pages: 1 },
  "043": { sha: "e635d3b0320073b6d08402945a90e14e13d0294c02262d854b106b9f0088904b", bytes: 980480, pages: 2 },
};
const planPath = process.env.CHAPTERA_PRODUCT_CASE_PLAN;
const pinnedPlan = planPath ? JSON.parse(await readFile(planPath, "utf8")) : null;
if (pinnedPlan && (pinnedPlan.id !== fixtureId ||
    !/^[0-9a-f]{64}$/.test(pinnedPlan.source_sha256 ?? "") ||
    !/^[0-9a-f]{64}$/.test(pinnedPlan.publisher_pdf_sha256 ?? "") ||
    !Number.isSafeInteger(pinnedPlan.source_bytes) ||
    !(pinnedPlan.source_bytes > 0 && pinnedPlan.source_bytes < 100000000) ||
    !Number.isSafeInteger(pinnedPlan.pages) ||
    !(pinnedPlan.pages > 0 && pinnedPlan.pages <= 100))) {
  throw new Error("physical Product case plan violates pinned source identity");
}
const fixture = pinnedPlan
  ? { sha: pinnedPlan.source_sha256, bytes: pinnedPlan.source_bytes, pages: pinnedPlan.pages }
  : known[fixtureId];
if (pinnedPlan && known[fixtureId] &&
    (fixture.sha !== known[fixtureId].sha ||
     fixture.bytes !== known[fixtureId].bytes ||
     fixture.pages !== known[fixtureId].pages)) {
  throw new Error("physical Product case plan conflicts with established Publisher pilot");
}
const storageState = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const scenePath = process.env.CHAPTERA_PREVIEW_022_SCENE_OUT;
const summaryPath = process.env.CHAPTERA_PREVIEW_022_SUMMARY;
const responsePhasePath = process.env.CHAPTERA_PRODUCT_HTTP_RESPONSE_PHASE_FILE;
if ([origin, fixturePath, expectedSha, storageState, privatePath, scenePath, summaryPath, responsePhasePath].some(v => !v)) {
  throw new Error("Product 022 requires pinned source, saved real OIDC session and private receipt paths");
}
if (!fixture || expectedSha !== fixture.sha) {
  throw new Error("Product acceptance must use the pinned Publisher 022 source");
}
const pub = await readFile(fixturePath);
if (pub.length !== fixture.bytes ||
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
// Genuine PKCE browser login necessarily visits the test OIDC provider.
// The no-external-resource fence begins only after the authenticated owner
// is ready to select a PUB; it still covers ingress, Product and image paint.
let monitorExternalRequests = false;
page.on("pageerror", err => errors.push(String(err)));
page.on("request", request => {
  const url = request.url();
  if (monitorExternalRequests &&
      !url.startsWith(origin + "/") && !url.startsWith("data:")) {
    foreign.push(url);
  }
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
  // The UI's already completed real OIDC session/workspace preparation.
  // A second GET /v1/session would rotate its CSRF secret and invalidate
  // the browser's upload-token, causing a correct fail-closed HTTP 403.
  monitorExternalRequests = true;
  // Capture the real Editor shell's requests BEFORE the file triggers a new
  // document navigation. The shell already GETs /current and /scene. Starting
  // a second pair of Product replay jobs here would double CPU/memory use and
  // can turn a bounded single-worker result into a synthetic timeout.
  const isUiResponse = (response, kind) => {
    if (response.request().method() !== "GET") return false;
    const url = new URL(response.url());
    if (url.origin !== origin) return false;
    const route = decodeURIComponent(url.pathname);
    return kind === "current"
      ? /^\/v1\/documents\/document:[0-9a-f]{24}\/current$/.test(route)
      : /^\/v1\/reader\/documents\/document:[0-9a-f]{24}\/scene$/.test(route);
  };
  // Keep both listener promises observed even if only one response arrives.
  // The existing 150s deadline remains unchanged; this does not bypass
  // SourceIngress/worker authentication or weaken the physical acceptance.
  let currentRequested = false;
  let sceneRequested = false;
  page.on("request", request => {
    if (request.method() !== "GET") return;
    try {
      const url = new URL(request.url());
      if (url.origin !== origin) return;
      const route = decodeURIComponent(url.pathname);
      if (/^\/v1\/documents\/document:[0-9a-f]{24}\/current$/.test(route)) currentRequested = true;
      if (/^\/v1\/reader\/documents\/document:[0-9a-f]{24}\/scene$/.test(route)) sceneRequested = true;
    } catch {
      // Malformed external requests are still rejected by the existing fence.
    }
  });
  const observed = (kind) => page.waitForResponse(
    response => isUiResponse(response, kind), { timeout: 150000 }
  ).then(response => ({ response }), () => ({ response: null }));
  const currentFromUi = observed("current");
  const sceneFromUi = observed("scene");
  await page.locator("#pub-file").setInputFiles({
    name: "Pinned" + fixtureId + ".pub",
    mimeType: "application/x-mspublisher",
    buffer: pub,
  });
  await page.waitForURL(url =>
    /^\/editor\/doc\/document:[0-9a-f]{24}$/.test(decodeURIComponent(url.pathname)),
    { timeout: 150000, waitUntil: "domcontentloaded" },
  );
  const documentId = decodeURIComponent(new URL(page.url()).pathname)
    .replace("/editor/doc/", "");
  const [currentObservation, sceneObservation] = await Promise.all([
    currentFromUi, sceneFromUi,
  ]);
  if (!currentObservation.response || !sceneObservation.response) {
    let code;
    if (!currentObservation.response && !sceneObservation.response) {
      code = currentRequested === sceneRequested
        ? (currentRequested ? "http_both_unanswered" : "http_both_unrequested")
        : "http_both_mixed";
    } else if (!currentObservation.response) {
      code = currentRequested ? "http_current_unanswered" : "http_current_unrequested";
    } else {
      code = sceneRequested ? "http_scene_unanswered" : "http_scene_unrequested";
    }
    // This PRIVATE fixed-value sidecar contains no URL, ID, cookies,
    // document data, exception body, or unstable Playwright text.
    await writeFile(responsePhasePath, code + "\n", "utf8");
    throw new Error("Product owner HTTP response not observed");
  }
  const [currentResponse, sceneResponse] = [
    currentObservation.response, sceneObservation.response,
  ];
  const [current, scene] = await Promise.all([
    currentResponse.json().catch(() => null),
    sceneResponse.json().catch(() => null),
  ]);
  const result = {
    currentStatus: currentResponse.status(),
    sceneStatus: sceneResponse.status(),
    cacheControl: sceneResponse.headers()["cache-control"] ?? null,
    current, scene,
  };
  if (result.currentStatus !== 200 || result.sceneStatus !== 200) {
    const code = scene?.error?.code;
    const safeCode = typeof code === "string" && /^[a-z0-9_]{1,80}$/.test(code)
      ? code : "redacted";
    throw new Error("UI Product HTTP failed: current=" + result.currentStatus +
      " scene=" + result.sceneStatus + " fixed_code=" + safeCode);
  }
  // current and scene were captured from the actual UI HTTP responses above.
  if (result.currentStatus !== 200 || result.sceneStatus !== 200 ||
      result.cacheControl !== "no-store" || current?.document_id !== documentId ||
      !current?.revision_id || scene?.protocol_version !== "chaptera.reader-scene.v1" ||
      scene.document_id !== documentId || scene.revision_id !== current.revision_id ||
      scene.source_hash !== expectedSha || scene.pages?.length !== fixture.pages ||
      !["supported","partial"].includes(scene.fidelity?.state)) {
    throw new Error("authenticated Product HTTP did not preserve pinned source and current revision");
  }
  if (scene.project || scene.authoring_graph || scene.source_bytes) {
    throw new Error("Product Reader returned forbidden raw authoring or PUB source payload");
  }
  const resources = scene.resources ?? [];
  const admitted = resources.filter(resource =>
    (resource.mime === "image/png" || resource.mime === "image/jpeg") &&
    resource.availability === "inline_data_url" &&
    typeof resource.inline_data_url === "string");
  if (admitted.length > 96) {
    throw new Error("bounded Product admitted-image resource cap exceeded");
  }
  if (admitted.length > 0 && !(scene.fidelity.reasons ?? []).includes("safe_preview_color_untransformed")) {
    throw new Error("host preview lost explicit partial color-fidelity disposition");
  }
  const imageIds = new Set();
  for (const item of admitted) {
    const rid = item.resource_id;
    if (!rid || imageIds.has(rid)) {
      throw new Error("Product image resource identity is missing or duplicated");
    }
    imageIds.add(rid);
    const prefix = "data:" + item.mime + ";base64,";
    if (!item.inline_data_url.startsWith(prefix)) {
      throw new Error("Product host preview URL/MIME invalid");
    }
    const encoded = Buffer.from(item.inline_data_url.slice(prefix.length), "base64");
    const imageCap = (item.mime === "image/png" ? 4 : 5) * 1024 * 1024;
    const magic = item.mime === "image/png" ? "89504e470d0a1a0a" : "ffd8ff";
    if (encoded.length === 0 || encoded.length > imageCap ||
        encoded.subarray(0, magic.length / 2).toString("hex") !== magic) {
      throw new Error("Product host derivative exceeds cap or has invalid magic");
    }
    const used = (scene.nodes ?? []).some(node =>
      node.resource_id === rid ||
      (node.decorative_border?.placements ?? []).some(p => p.resource_id === rid));
    if (!used) throw new Error("Product derived image does not belong to a Scene node");
  }
  if (Buffer.byteLength(JSON.stringify(scene), "utf8") > 16 * 1024 * 1024) {
    throw new Error("Product Scene size cap violated");
  }
  await page.waitForFunction(
    props => document.querySelector("#status")?.classList.contains("ok") &&
      document.querySelector("#status")?.textContent?.includes(props.revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length === props.pages &&
      document.querySelectorAll("#canvas svg image[data-resource-id]").length >= props.images,
    { revision: current.revision_id, pages: fixture.pages, images: admitted.length }, { timeout: 45000 },
  );
  const paint = await page.locator("#canvas svg image[data-resource-id]").evaluateAll(
    async images => Promise.all(images.map(async image => {
      const href = image.getAttribute("href") || image.getAttribute("xlink:href");
      if (!href?.startsWith("data:image/png;base64,") &&
          !href?.startsWith("data:image/jpeg;base64,")) {
        throw new Error("browser displayed a non-host image URL");
      }
      const decoded = new Image();
      decoded.src = href;
      await decoded.decode();
      const box = image.getBoundingClientRect();
      return {
        resource_id: image.getAttribute("data-resource-id"),
        visible: box.width > 0 && box.height > 0,
        decoded: decoded.naturalWidth > 0 && decoded.naturalHeight > 0,
      };
    })),
  );
  const paintedIds = new Set(
    paint.filter(p => p.visible && p.decoded).map(p => p.resource_id));
  if (paintedIds.size !== admitted.length ||
      [...imageIds].some(resourceId => !paintedIds.has(resourceId)) ||
      errors.length || foreign.length) {
    throw new Error("authenticated Product source images failed Chromium paint");
  }
  const resource = admitted[0] ?? null;

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
    resource_id: resource?.resource_id ?? null,
  }), "utf8");
  const receipt = {
    schema: "chaptera.product-oidc-worst-census-browser.v1",
    fixture: fixtureId,
    repository_commit_sha: process.env.CHAPTERA_HEAD_SHA ?? "unverified",
    revision_identity_sha256: createHash("sha256").update(current.revision_id, "utf8").digest("hex"),
    real_oidc_pkce_session_reused: true,
    tls_product_source_ingress: true,
    exact_source_and_revision: true,
    actual_product_http_status: 200,
    isolated_worker_and_host_owned_image: true,
    host_image_count: admitted.length,
    document_page_count: scene.pages.length,
    node_count: (scene.nodes ?? []).length,
    node_kinds: (scene.nodes ?? []).reduce((acc, node) => {
      const kind = String(node.kind);
      if (!["text_frame","picture_frame","shape","table","group","line"].includes(kind)) {
        acc.other = (acc.other ?? 0) + 1;
      } else {
        acc[kind] = (acc[kind] ?? 0) + 1;
      }
      return acc;
    }, {}),
    descriptor_only_resource_count: resources.filter(r => r.availability !== "inline_data_url").length,
    host_image_resource_count: admitted.length,
    fidelity_reason_count: (scene.fidelity?.reasons ?? []).length,
    image_count_source_expectation_proven: false,
    scene_image_resource_count: admitted.length,
    browser_painted_resources: paintedIds.size,
    browser_svg_image_count: paint.length,
    chromium_pixels_decoded: true,
    anonymous_scene_status: 401,
    source_container_forwarded: false,
    external_image_requests: 0,
    browser_errors: 0,
    fidelity_state: scene.fidelity.state,
    production_s3_claim: false,
    deployed_host_claim: false,
  };
  await mkdir(dirname(summaryPath), { recursive: true });
  await writeFile(summaryPath, JSON.stringify(receipt, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_OIDC_WORST_CENSUS " + JSON.stringify(receipt) + "\n");
} finally {
  await context.close();
  await browser.close();
}
