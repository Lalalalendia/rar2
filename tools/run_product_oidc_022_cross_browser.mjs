#!/usr/bin/env node
// Real second OIDC subject; source-neutral Product 022 grant-denial proof.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const privatePath = process.env.CHAPTERA_PREVIEW_022_PRIVATE;
const summaryPath = process.env.CHAPTERA_PRODUCT_022_CROSS_SUMMARY;
if (!origin || !privatePath || !summaryPath) throw new Error("cross-principal Product022 paths required");
const original = JSON.parse(await readFile(privatePath, "utf8"));
if (!/^document:[0-9a-f]{24}$/.test(original.document_id) ||
    original.source_sha256 !== "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c") {
  throw new Error("pinned original OIDC Product022 identity invalid");
}
const browser = await chromium.launch({
  headless: true, args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({ ignoreHTTPSErrors: true });
try {
  const page = await context.newPage();
  // Separate browser, zero cookies. Provider switched to a distinct subject.
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(
    () => document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
    null, { timeout: 30000 },
  );
  const cookies = await context.cookies(origin);
  const session = cookies.find(c => c.name === "__Host-chaptera_session");
  if (!session?.secure || !session?.httpOnly || session.sameSite !== "Lax") {
    throw new Error("second principal did not complete real secured OIDC browser login");
  }
  const results = await page.evaluate(async docId => {
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
      };
    }
    return {
      foreign: await probe(docId),
      nonexistent: await probe("document:absent-" + crypto.randomUUID()),
    };
  }, original.document_id);
  if (results.foreign.status !== 403 || results.foreign.code !== "grant_missing" ||
      results.foreign.cache_control !== "no-store" ||
      results.foreign.source_disclosed || results.foreign.revision_disclosed ||
      results.foreign.resource_disclosed || results.foreign.pages_disclosed ||
      JSON.stringify(results.foreign) !== JSON.stringify(results.nonexistent)) {
    throw new Error("distinct OIDC Product022 read did not fail closed with absent-ID parity");
  }
  const summary = {
    schema: "chaptera.product-oidc-022-cross-principal.v1",
    real_second_oidc_principal: true,
    foreign_status: 403,
    missing_document_status: 403,
    deny_code: "grant_missing",
    denial_indistinguishable: true,
    image_or_source_disclosed: false,
  };
  await writeFile(summaryPath, JSON.stringify(summary, null, 2) + "\n", "utf8");
  process.stdout.write("PRODUCT_OIDC_022_CROSS " + JSON.stringify(summary) + "\n");
} finally {
  await context.close();
  await browser.close();
}
