#!/usr/bin/env node
// Reopen independently ingested PUB022 after real Chaptera server+worker restart.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const statePath = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const summaryPath = process.env.CHAPTERA_PRODUCT_022_RESTART_SUMMARY;
if (!origin || !statePath || !privatePath || !summaryPath) {
  throw new Error("Product022 restart requires original cookie and exact private revision");
}
const pinned = JSON.parse(await readFile(privatePath, "utf8"));
if (!pinned.document_id || !pinned.revision_id || !pinned.resource_id ||
    pinned.source_sha256 !== "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c") {
  throw new Error("original Product022 identity missing");
}
const browser = await chromium.launch({
  headless: true, args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  storageState: statePath, ignoreHTTPSErrors: true,
  viewport: { width: 1450, height: 1100 },
});
try {
  const page = await context.newPage();
  await page.goto(origin + "/editor/doc/" + encodeURIComponent(pinned.document_id), {
    waitUntil: "domcontentloaded",
  });
  const observed = await page.evaluate(async docId => {
    const currentResponse = await fetch(
      "/v1/documents/" + encodeURIComponent(docId) + "/current",
      { credentials: "same-origin", cache: "no-store" },
    );
    const current = await currentResponse.json().catch(() => null);
    const sceneResponse = await fetch(
      "/v1/reader/documents/" + encodeURIComponent(docId) + "/scene",
      { credentials: "same-origin", cache: "no-store" },
    );
    const scene = await sceneResponse.json().catch(() => null);
    return {
      current_status: currentResponse.status,
      scene_status: sceneResponse.status,
      revision: current?.revision_id,
      scene_revision: scene?.revision_id,
      source: scene?.source_hash,
      partial: scene?.fidelity?.state === "partial",
      resources: (scene?.resources ?? []).map(r => ({
        id: r.resource_id,
        mime: r.mime,
        status: r.availability,
        derived_jpeg: r.inline_data_url?.startsWith("data:image/jpeg;base64,") ?? false,
      })),
    };
  }, pinned.document_id);
  if (observed.current_status !== 200 || observed.scene_status !== 200 ||
      observed.revision !== pinned.revision_id ||
      observed.scene_revision !== pinned.revision_id ||
      observed.source !== pinned.source_sha256 ||
      !observed.partial || observed.resources.length !== 1 ||
      observed.resources[0].id !== pinned.resource_id ||
      observed.resources[0].mime !== "image/jpeg" ||
      observed.resources[0].status !== "inline_data_url" ||
      !observed.resources[0].derived_jpeg) {
    throw new Error("Product022 exact persisted scene did not recover after process restart");
  }
  await page.waitForFunction(revision =>
    document.querySelector("#status")?.classList.contains("ok") &&
    document.querySelector("#status")?.textContent?.includes(revision) &&
    document.querySelectorAll("#canvas svg image[data-resource-id]").length === 1,
    pinned.revision_id, { timeout: 60000 },
  );
  const painted = await page.locator("#canvas svg image[data-resource-id]").first()
    .evaluate(async image => {
      const href = image.getAttribute("href") || image.getAttribute("xlink:href");
      if (!href?.startsWith("data:image/jpeg;base64,")) return false;
      const decoded = new Image();
      decoded.src = href;
      await decoded.decode();
      return decoded.naturalWidth > 0 && image.getBoundingClientRect().width > 0;
    });
  if (!painted) throw new Error("restarted Product022 derived JPEG did not paint");
  const summary = {
    schema: "chaptera.product-oidc-022-restart.v1",
    original_oidc_session_preserved: true,
    server_and_worker_restarted: true,
    exact_source_revision_and_resource: true,
    host_owned_jpeg_count: 1,
    chromium_image_painted: true,
    production_deployment_claim: false,
  };
  await writeFile(summaryPath, JSON.stringify(summary, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_OIDC_022_RESTART " + JSON.stringify(summary) + "\n");
} finally {
  await context.close();
  await browser.close();
}
