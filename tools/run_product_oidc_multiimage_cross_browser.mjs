#!/usr/bin/env node
// Real independent OIDC subject must never see another user's Product
// PUB001/PUB023 source, revision, PAGE or host-derived preview resources.
// All document/session identities remain in ephemeral runner files.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const cases = {
  "001": { sha: "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade", images: 1 },
  "023": { sha: "6bbfbf7b4c9b240026399fcc114ee0eb78fbc954c8f258f5baf3a388d3f188c8", images: 7 },
};
const id = process.env.CHAPTERA_PRODUCT_MULTI_ID;
const fixture = cases[id];
const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const summaryPath = process.env.CHAPTERA_PRODUCT_022_CROSS_SUMMARY;
if (!fixture || !origin || !privatePath || !summaryPath) {
  throw new Error("real Product multi-image cross-principal scope missing");
}
const original = JSON.parse(await readFile(privatePath, "utf8"));
if (!/^document:[0-9a-f]{24}$/.test(original.document_id ?? "") ||
    typeof original.revision_id !== "string" || !original.revision_id ||
    original.source_sha256 !== fixture.sha ||
    !Array.isArray(original.resources) ||
    original.resources.length !== fixture.images ||
    new Set(original.resources.map(r => r.resource_id)).size !== fixture.images) {
  throw new Error("real Product multi-image original identity invalid");
}
const browser = await chromium.launch({
  headless: true, args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({ ignoreHTTPSErrors: true });
try {
  const page = await context.newPage();
  // A fresh context MUST complete an actual independent OIDC PKCE sign-in.
  // The provider subject is changed by the owning Python harness, never by
  // browser cookie fabrication or an impersonation header.
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
    null, { timeout: 30000 },
  );
  const cookies = await context.cookies(origin);
  const session = cookies.find(c => c.name === "__Host-chaptera_session");
  if (!session?.secure || !session?.httpOnly || session.sameSite !== "Lax") {
    throw new Error("second subject did not complete independent secure OIDC login");
  }
  const results = await page.evaluate(async documentId => {
    async function probe(id) {
      const response = await fetch(
        "/v1/reader/documents/" + encodeURIComponent(id) + "/scene",
        { credentials: "same-origin", cache: "no-store" },
      );
      const body = await response.json().catch(() => null);
      return {
        status: response.status,
        code: body?.error?.code ?? null,
        cache_control: response.headers.get("cache-control"),
        source_disclosed: typeof body?.source_hash === "string",
        revision_disclosed: typeof body?.revision_id === "string",
        resource_disclosed: Array.isArray(body?.resources) && body.resources.length > 0,
        pages_disclosed: Array.isArray(body?.pages) && body.pages.length > 0,
        authoring_disclosed: !!body?.project || !!body?.authoring_graph,
      };
    }
    return {
      foreign: await probe(documentId),
      nonexistent: await probe("document:absent-" + crypto.randomUUID()),
    };
  }, original.document_id);
  if (results.foreign.status !== 403 || results.foreign.code !== "grant_missing" ||
      results.foreign.cache_control !== "no-store" ||
      results.foreign.source_disclosed || results.foreign.revision_disclosed ||
      results.foreign.resource_disclosed || results.foreign.pages_disclosed ||
      results.foreign.authoring_disclosed ||
      JSON.stringify(results.foreign) !== JSON.stringify(results.nonexistent)) {
    throw new Error("second OIDC subject could inspect foreign Product image/source or detect its identity");
  }
  const summary = {
    schema: "chaptera.product-oidc-multiimage-cross-principal.v1",
    fixture: id,
    real_independent_oidc_subject: true,
    foreign_scene_http_status: 403,
    missing_scene_http_status: 403,
    denial_code: "grant_missing",
    absent_document_denial_indistinguishable: true,
    source_revision_page_image_disclosure: false,
    original_source_and_session_excluded: true,
  };
  await writeFile(summaryPath, JSON.stringify(summary, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_MULTI_CROSS " + JSON.stringify(summary) + "\n");
} finally {
  await context.close();
  await browser.close();
}
