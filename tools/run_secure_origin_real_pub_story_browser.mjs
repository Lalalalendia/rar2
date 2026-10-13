#!/usr/bin/env node
// Genuine Product browser StoryRange acceptance using the existing OIDC/HTTPS
// runtime. Browser input is transient; canonical Story state lives on server.
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { chromium, webkit } from "playwright";
import {
  storyTextForTextarea,
  textareaTextToCanonicalStory,
} from "../apps/web/product-editor-entry-v1.mjs";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const fixture = process.env.CHAPTERA_STORY_PUB_FIXTURE;
const pinnedSourceSha = process.env.CHAPTERA_STORY_PUB_SHA256;
const storageState = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const receiptPath = process.env.CHAPTERA_STORY_RECEIPT;
if (!origin || !fixture || !pinnedSourceSha || !storageState || !receiptPath) {
  throw new Error("real Product Story browser requires pinned source, OIDC session and receipt path");
}
const raw = await readFile(fixture);
const sourceSha = createHash("sha256").update(raw).digest("hex");
if (sourceSha !== pinnedSourceSha || raw.length !== 72192) {
  throw new Error("real Product Story fixture bytes differ from immutable source pin");
}

const browserEngine = process.env.CHAPTERA_STORY_BROWSER ?? "chromium";
const browserType = { chromium, webkit }[browserEngine];
if (!browserType) {
  throw new Error("unsupported real Story browser engine: " + browserEngine);
}
const browser = await browserType.launch({
  headless: true,
  ...(browserEngine === "chromium"
    ? { args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"] }
    : {}),
});
try {
  const context = await browser.newContext({
    ignoreHTTPSErrors: true,
    storageState,
    viewport: { width: 1180, height: 830 },
  });
  const session = (await context.cookies(origin)).find(cookie =>
    cookie.name === "__Host-chaptera_session");
  if (!session?.secure || !session.httpOnly) {
    throw new Error("no real authenticated Product session from source ingress acceptance");
  }
  const page = await context.newPage();
  const calls = [];
  const errors = [];
  page.on("pageerror", error => errors.push(String(error)));
  page.on("request", request => {
    const path = new URL(request.url()).pathname;
    if (!path.endsWith("/story-commit") || request.method() !== "POST") return;
    calls.push({ path, csrf: !!request.headers()["x-csrf-token"],
      forgedPrincipal: !!request.headers()["x-chaptera-principal-id"] });
  });

  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
  null, { timeout: 30000 });
  // The product intentionally keeps the native file input hidden and drives it
  // through the visible drop-zone/button. Playwright's setInputFiles targets
  // that real attached input directly; visibility is not an upload-readiness
  // signal and would reject the production DOM before any source bytes move.
  await page.locator("#pub-file").waitFor({ state: "attached", timeout: 30000 });
  await page.locator("#pub-file").setInputFiles({
    name: "Sample3.pub", mimeType: "application/x-mspublisher", buffer: raw,
  });
  await page.waitForURL(url =>
    /^\/editor\/doc\/document:[0-9a-f]{24}$/.test(decodeURIComponent(url.pathname)),
  { timeout: 105000, waitUntil: "domcontentloaded" });
  const documentId = decodeURIComponent(new URL(page.url()).pathname)
    .replace("/editor/doc/", "");

  async function requirePaint(revisionId) {
    await page.waitForFunction(revision => {
      const status = document.querySelector("#status");
      return status?.classList.contains("ok") &&
        status.textContent.includes(revision) &&
        document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
    }, revisionId, { timeout: 90000 });
  }
  async function readProduct() {
    const state = await page.evaluate(async id => {
      const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
        credentials: "same-origin", cache: "no-store",
      });
      const body = await response.json().catch(() => null);
      return { status: response.status, body };
    }, documentId);
    if (state.status !== 200 ||
      state.body?.protocol_version !== "chaptera.current-document.v1" ||
      state.body.document_id !== documentId || state.body.source_hash !== sourceSha ||
      !/^sha256:[0-9a-f]{64}$/.test(state.body.revision_id)) {
      throw new Error("Story baseline did not materialize under authorized Product identity");
    }
    return state.body;
  }
  async function readScene(expectedRevision) {
    const state = await page.evaluate(async id => {
      const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
        credentials: "same-origin", cache: "no-store",
      });
      const body = await response.json().catch(() => null);
      return { status: response.status, body };
    }, documentId);
    if (state.status !== 200 ||
      state.body?.protocol_version !== "chaptera.reader-scene.v1" ||
      state.body.document_id !== documentId || state.body.source_hash !== sourceSha ||
      state.body.revision_id !== expectedRevision || !state.body.pages?.length) {
      throw new Error("Story Reader scene differs from exact authorized source/revision");
    }
    return state.body;
  }

  // Wait for real editor paint before the independent worker-backed Scene read.
  await page.waitForFunction(() =>
    document.querySelector("#status")?.classList.contains("ok") &&
    document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0,
  null, { timeout: 90000 });
  const baseline = await readProduct();
  await requirePaint(baseline.revision_id);
  const originalScene = await readScene(baseline.revision_id);

  await page.locator("#story-load-action").click();
  await page.waitForFunction(() => {
    const status = document.querySelector("#story-status")?.textContent ?? "";
    return status.includes("Canonical Story loaded") ||
      status.includes("No supported plain Story") ||
      status.includes("not confirmed");
  }, null, { timeout: 90000 });
  // The success text is published immediately before the load handler clears
  // its busy guard. Wait for that public control state too, otherwise an
  // automation-speed click can correctly be ignored while loading is active.
  await page.waitForFunction(() =>
    document.querySelector("#story-load-action")?.disabled === false,
  );
  const selected = await page.evaluate(() => ({
    status: document.querySelector("#story-status")?.textContent ?? "",
    choices: document.querySelector("#story-select")?.options.length ?? 0,
    storyId: document.querySelector("#story-select")?.value ?? "",
  }));
  if (!selected.status.includes("Canonical Story loaded") || !selected.choices ||
      !selected.storyId || await page.locator("#story-input").isDisabled()) {
    throw new Error("real pinned PUB exposes no browser-editable canonical plain Story");
  }
  const priorText = await page.locator("#story-input").inputValue();
  const priorCanonical = baseline.authoring_graph?.stories?.[selected.storyId]?.text;
  if (typeof priorCanonical !== "string" ||
      storyTextForTextarea(priorCanonical) !== priorText) {
    throw new Error("Story textarea projection differs from canonical source text");
  }
  const scalars = [...priorText];
  if (!scalars.length) throw new Error("selected Story has no text for physical range edit");
  const replacement = scalars[0] === "Q" ? "R" : "Q";
  const desired = replacement + scalars.slice(1).join("");
  const desiredCanonical = textareaTextToCanonicalStory(priorCanonical, desired);
  if (desired === priorText) throw new Error("test Story edit must change exactly one scalar");
  await page.locator("#story-input").fill(desired);
  const acceptedResponse = page.waitForResponse(response =>
    response.request().method() === "POST" &&
    decodeURIComponent(new URL(response.url()).pathname) ===
      "/v1/documents/" + documentId + "/story-commit",
  { timeout: 90000 }).then(response => ({kind: "response", response}),
    error => ({kind: "response_timeout", error}));
  const rejectedByUi = page.waitForFunction(() =>
    document.querySelector("#story-status")?.classList.contains("error") === true,
  null, { timeout: 90000 }).then(() => ({kind: "ui_error"}),
    error => ({kind: "ui_timeout", error}));
  await page.locator("#story-apply-action").click();
  const outcome = await Promise.race([acceptedResponse, rejectedByUi]);
  if (outcome.kind !== "response") {
    const status = await page.locator("#story-status").textContent();
    throw new Error("Story Apply produced no HTTP mutation (" + outcome.kind + "): " + status);
  }
  const response = outcome.response;
  const submitted = response.request().postDataJSON();
  const receipt = await response.json().catch(() => null);
  if (response.status() !== 200 ||
      submitted?.document_id !== documentId ||
      submitted.source_hash !== sourceSha ||
      submitted.base_revision_id !== baseline.revision_id ||
      submitted.command?.kind !== "replace_story_range" ||
      submitted.command.story_id !== selected.storyId ||
      submitted.command.start_scalar !== 0 ||
      submitted.command.end_scalar !== 1 ||
      submitted.command.replacement_text !== replacement ||
      !submitted.client_operation_id ||
      receipt?.client_operation_id !== submitted.client_operation_id ||
      receipt?.protocol_version !== "chaptera.commit-accepted.v1" ||
      receipt.document_id !== documentId || receipt.source_hash !== sourceSha ||
      receipt.base_revision_id !== baseline.revision_id ||
      receipt.revision_id === baseline.revision_id ||
      receipt.canonical_operation?.kind !== "replace_story_range" ||
      receipt.canonical_operation?.story_id !== selected.storyId ||
      receipt.canonical_operation?.start_scalar !== 0 ||
      receipt.canonical_operation?.end_scalar !== 1 ||
      receipt.canonical_operation?.replacement_text !== replacement ||
      receipt.replayed !== false || calls.length !== 1 ||
      !calls[0].csrf || calls[0].forgedPrincipal) {
    throw new Error("real browser Story commit did not return one authorized exact mutation");
  }
  await page.waitForFunction(() =>
    document.querySelector("#story-status")?.textContent?.includes(
      "Exact server revision and reopened Story text confirmed"),
  null, { timeout: 120000 });
  const current = await readProduct();
  if (current.revision_id !== receipt.revision_id ||
      current.authoring_graph?.stories?.[selected.storyId]?.text !== desiredCanonical) {
    throw new Error("fresh canonical current did not preserve the edited Story text");
  }
  const scene = await readScene(receipt.revision_id);
  if (!scene.stories?.some(story =>
    story.story_id === selected.storyId && story.text === desiredCanonical) ||
      !originalScene.stories?.some(story => story.story_id === selected.storyId &&
        story.text === priorCanonical)) {
    throw new Error("real Reader text projection did not follow immutable Story revision");
  }

  await page.reload({ waitUntil: "domcontentloaded" });
  await requirePaint(receipt.revision_id);
  await page.locator("#story-load-action").click();
  await page.waitForFunction(() =>
    document.querySelector("#story-status")?.textContent?.includes("Canonical Story loaded"),
  null, { timeout: 90000 });
  await page.locator("#story-select").selectOption(selected.storyId);
  if (await page.locator("#story-input").inputValue() !== desired || calls.length !== 1) {
    throw new Error("Story did not survive Product browser reload or duplicate commit detected");
  }
  const reopened = await readProduct();
  if (reopened.revision_id !== receipt.revision_id ||
      reopened.authoring_graph?.stories?.[selected.storyId]?.text !== desiredCanonical) {
    throw new Error("fresh Product reopened an inconsistent Story revision");
  }
  if (errors.length) throw new Error("real Story browser reported JavaScript errors");

  // Use the actual Chromium network stack: its test-only DNS mapping for
  // edge.test is not inherited by Node's APIRequestContext transport.
  const anonymous = await browser.newContext({ ignoreHTTPSErrors: true });
  const anonPage = await anonymous.newPage();
  const denied = await anonPage.goto(origin + "/v1/documents/" +
    encodeURIComponent(documentId) + "/current", {
      waitUntil: "domcontentloaded", timeout: 30000,
    });
  if (!denied || (denied.status() !== 401 && denied.status() !== 403)) {
    throw new Error("anonymous browser unexpectedly read durable Story");
  }
  await anonymous.close();

  const result = {
    schema: "chaptera.real-product-story-browser-acceptance.v1",
    head_sha: process.env.CHAPTERA_HEAD_SHA ?? "unknown",
    product_http_oidc_browser: true,
    product_http_oidc_chromium: browserEngine === "chromium",
    browser_engine: browserEngine,
    browser_version: browser.version(),
    source_immutable_sha256: sourceSha,
    document_id: documentId,
    story_id: selected.storyId,
    original_revision_id: baseline.revision_id,
    committed_revision_id: receipt.revision_id,
    real_dom_textarea_input: true,
    one_authorized_story_commit: true,
    canonical_current_text_confirmed: true,
    source_neutral_reader_scene_text_refreshed: true,
    authoritative_relayout_overset_verified: false,
    browser_reload_preserves_story: true,
    anonymous_read_denied: true,
    source_bytes_in_receipt: false,
    browser_layout_is_canonical: false,
    story_undo_redo_claim: false,
    export_job_claim: false,
    two_engine_claim: false,
  };
  const persisted = JSON.stringify(result);
  if (persisted.includes("Sample3.pub") ||
      (priorText.length >= 16 && persisted.includes(priorText)) ||
      (desired.length >= 16 && persisted.includes(desired))) {
    throw new Error("private fixture label or Story contents leaked into acceptance receipt");
  }
  await writeFile(receiptPath, JSON.stringify(result, null, 2) + "\n");
  process.stdout.write(persisted + "\n");
} finally {
  await browser.close();
}
