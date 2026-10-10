#!/usr/bin/env node
// Executes the real HTTPS/OIDC and PUB ingest browser path. No mocked API.
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const fixture = process.env.CHAPTERA_SOURCE_PUB_FIXTURE;
const expectedHash = process.env.CHAPTERA_SOURCE_PUB_SHA256;
const receiptPath = process.env.CHAPTERA_SECURE_RECEIPT;
if (!origin || !fixture || !expectedHash || !receiptPath) {
  throw new Error("real source browser requires secure origin, fixture, SHA and receipt path");
}
const raw = await readFile(fixture);
const hash = createHash("sha256").update(raw).digest("hex");
if (hash !== expectedHash) throw new Error("real Publisher fixture SHA mismatch");

const browser = await chromium.launch({
  headless: true,
  args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 1180, height: 830 },
});
const page = await context.newPage();
const calls = [];
const failedResponses = [];
const pageErrors = [];
page.on("pageerror", error => pageErrors.push(String(error)));
page.on("request", request => {
  if (!request.url().startsWith(origin + "/v1/")) return;
  calls.push({
    method: request.method(),
    path: new URL(request.url()).pathname,
    forgedPrincipal: !!request.headers()["x-chaptera-principal-id"],
    csrf: !!request.headers()["x-csrf-token"],
  });
});
page.on("response", response => {
  if (response.url().startsWith(origin + "/v1/") && response.status() >= 400) {
    failedResponses.push(response.status() + " " + new URL(response.url()).pathname);
  }
});

try {
  // Anonymous page redirects to the real test OIDC provider, then returns
  // to the upload page with Chaptera's hardened same-origin session cookie.
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
    null, { timeout: 30000 });

  const cookies = await context.cookies(origin);
  const session = cookies.find(cookie => cookie.name === "__Host-chaptera_session");
  if (!session || !session.secure || !session.httpOnly || session.sameSite !== "Lax") {
    throw new Error("real OIDC login did not supply secure Chaptera cookie");
  }

  await page.locator("#pub-file").setInputFiles({
    name: "SampleNewsletter.pub",
    mimeType: "application/x-mspublisher",
    buffer: raw,
  });

  try {
    await page.waitForURL(url =>
      /^\/editor\/doc\/document:[0-9a-f]{24}$/.test(decodeURIComponent(url.pathname)),
    { timeout: 105000, waitUntil: "domcontentloaded" });
  } catch (failure) {
    const ui = await page.evaluate(() => ({
      url: location.href,
      status: document.querySelector("#status")?.textContent ?? "",
      error: document.querySelector("#error")?.textContent ?? "",
    }));
    throw new Error("real PUB ingress did not create a project: " +
      JSON.stringify({ ui, recentRequests: calls.slice(-20),
        failedResponses: failedResponses.slice(-10),
        pageErrors: pageErrors.slice(-10), reason: String(failure) }));
  }

  const documentId = decodeURIComponent(new URL(page.url()).pathname)
    .replace("/editor/doc/", "");
  const current = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin",
    });
    const data = await response.json().catch(() => null);
    return {
      status: response.status, document_id: data?.document_id ?? null,
      revision_id: data?.revision_id ?? null,
      protocol: data?.protocol_version ?? null,
      error: data?.error ?? null,
    };
  }, documentId);
  if (current.status !== 200 || current.document_id !== documentId ||
      !current.revision_id) {
    throw new Error("real imported PUB did not materialize in Editor: " +
      JSON.stringify({ current, calls: calls.slice(-8), failedResponses }));
  }

  // An actual page reload (no synthetic product snapshot) must retain the
  // authoritative imported service revision. Server restart is a later gate.
  await page.reload({ waitUntil: "domcontentloaded" });
  const reopened = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin",
      cache: "no-store",
    });
    const data = await response.json().catch(() => null);
    return {
      status: response.status,
      document_id: data?.document_id ?? null,
      revision_id: data?.revision_id ?? null,
    };
  }, documentId);
  if (reopened.status !== 200 || reopened.document_id !== documentId ||
      reopened.revision_id !== current.revision_id) {
    throw new Error("real project revision changed after browser reload: " +
      JSON.stringify({ reopened, expected_revision: current.revision_id }));
  }

  for (const [method, path] of [
    ["GET", "/v1/session"],
    ["POST", "/v1/workspaces/personal"],
    ["POST", "/v1/uploads"],
    ["POST", "/v1/projects/from-upload"],
  ]) {
    if (!calls.some(call => call.method === method && call.path === path)) {
      throw new Error("missing real product step " + method + " " + path);
    }
  }
  if (!calls.some(call => call.method === "PUT" && /\/content$/.test(call.path))) {
    throw new Error("browser never transmitted real PUB source bytes");
  }
  if (calls.some(call => call.forgedPrincipal || (call.method !== "GET" && !call.csrf))) {
    throw new Error("forged identity header or missing real CSRF");
  }

  const receipt = {
    schema: "chaptera.real-pub-browser-ingress.v1",
    real_oidc_and_https: true,
    real_publisher_pub: true,
    real_pub_sha256: hash,
    real_source_worker: true,
    real_project_genesis_and_page_reload: true,
    server_restart_reopen_claim: false,
    storage_provider: "filesystem",
    s3_claim: false,
    native_pub_write_claim: false,
    move_and_export_claim: false,
    document_id: documentId,
    current_revision_id: current.revision_id,
    session_cookie_secure_http_only: true,
    steps: calls.map(call => call.method + " " + call.path),
    head_sha: process.env.CHAPTERA_HEAD_SHA ?? "unknown",
  };
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt) + "\n");
} finally {
  await browser.close();
}
