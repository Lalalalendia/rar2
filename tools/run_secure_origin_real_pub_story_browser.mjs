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
  if (browserEngine === "chromium") {
    await context.grantPermissions(
      ["clipboard-read", "clipboard-write"],
      { origin },
    );
  }
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
  const replacement = browserEngine === "chromium"
    ? (scalars[0] === "文" ? "字" : "文")
    : (scalars[0] === "Q" ? "R" : "Q");
  const desired = replacement + scalars.slice(1).join("");
  const desiredCanonical = textareaTextToCanonicalStory(priorCanonical, desired);
  if (desired === priorText) throw new Error("test Story edit must change exactly one scalar");
  await page.evaluate(() => {
    const input = document.querySelector("#story-input");
    window.__chapteraStoryInputTrace = [];
    for (const type of ["compositionstart", "compositionupdate", "compositionend",
      "beforeinput", "input"]) {
      input.addEventListener(type, event => {
        window.__chapteraStoryInputTrace.push({
          type: event.type,
          data: typeof event.data === "string" ? event.data : null,
          inputType: typeof event.inputType === "string" ? event.inputType : null,
          isComposing: event.isComposing === true,
        });
      });
    }
  });
  const storyInput = page.locator("#story-input");
  await storyInput.focus();
  const selectedRange = await page.evaluate(firstScalarWidth => {
    const input = document.querySelector("#story-input");
    input.setSelectionRange(0, firstScalarWidth, "forward");
    return {start: input.selectionStart, end: input.selectionEnd};
  }, scalars[0].length);
  if (selectedRange.start !== 0 || selectedRange.end !== scalars[0].length) {
    throw new Error("Story browser did not preserve the requested first-scalar range");
  }
  let compositionApplyFenced = false;
  if (browserEngine === "chromium") {
    const cdp = await context.newCDPSession(page);
    await cdp.send("Input.imeSetComposition", {
      text: replacement,
      selectionStart: replacement.length,
      selectionEnd: replacement.length,
      replacementStart: 0,
      replacementEnd: scalars[0].length,
    });
    if (await storyInput.inputValue() !== desired) {
      throw new Error("active Chromium IME composition did not replace the selected scalar");
    }
    // A programmatic click does not blur/commit the active IME composition.
    // The Product handler must therefore fence the mutation while composing.
    await page.evaluate(() => document.querySelector("#story-apply-action").click());
    await page.waitForTimeout(100);
    compositionApplyFenced = calls.length === 0;
    if (!compositionApplyFenced) {
      throw new Error("Story Apply escaped the active IME composition fence");
    }
    await cdp.send("Input.insertText", {text: replacement});
  } else {
    await page.keyboard.type(replacement);
  }
  if (await storyInput.inputValue() !== desired) {
    throw new Error("physical Story input did not produce the requested scalar replacement");
  }
  const inputTrace = await page.evaluate(() => window.__chapteraStoryInputTrace);
  const compositionEventsConfirmed = browserEngine === "chromium" &&
    ["compositionstart", "compositionupdate", "compositionend"].every(type =>
      inputTrace.some(event => event.type === type)) &&
    inputTrace.some(event => event.isComposing &&
      (event.type === "beforeinput" || event.type === "input"));
  const caretRangeInputConfirmed = browserEngine === "webkit" &&
    inputTrace.some(event => event.type === "input" && event.data === replacement);
  if (browserEngine === "chromium" && !compositionEventsConfirmed) {
    throw new Error("Chromium IME did not emit a complete transient composition event sequence");
  }
  if (browserEngine === "webkit" && !caretRangeInputConfirmed) {
    throw new Error("WebKit caret/range keyboard input emitted no matching DOM input event");
  }
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

  let finalRevisionId = receipt.revision_id;
  let finalCanonical = desiredCanonical;
  let pasteInputConfirmed = false;
  let pasteRevisionId = null;
  if (browserEngine === "chromium") {
    const afterFirstScalars = [...desired];
    if (afterFirstScalars.length < 2) {
      throw new Error("selected Story has no second scalar for physical paste acceptance");
    }
    const pasteReplacement = afterFirstScalars[1] === "Ω" ? "Ж" : "Ω";
    const pastedText = afterFirstScalars[0] + pasteReplacement +
      afterFirstScalars.slice(2).join("");
    const pastedCanonical = textareaTextToCanonicalStory(desiredCanonical, pastedText);
    await page.evaluate(async ({start, end, value}) => {
      const input = document.querySelector("#story-input");
      window.__chapteraStoryPasteTrace = [];
      for (const type of ["beforeinput", "input", "paste"]) {
        input.addEventListener(type, event => {
          window.__chapteraStoryPasteTrace.push({
            type: event.type,
            inputType: typeof event.inputType === "string" ? event.inputType : null,
            clipboardText: event.clipboardData?.getData("text/plain") ?? null,
          });
        });
      }
      input.focus();
      input.setSelectionRange(start, end, "forward");
      await navigator.clipboard.writeText(value);
    }, {
      start: afterFirstScalars[0].length,
      end: afterFirstScalars[0].length + afterFirstScalars[1].length,
      value: pasteReplacement,
    });
    await page.keyboard.press("Control+V");
    if (await page.locator("#story-input").inputValue() !== pastedText) {
      throw new Error("physical Chromium clipboard paste did not replace selected Story scalar");
    }
    const pasteTrace = await page.evaluate(() => window.__chapteraStoryPasteTrace);
    pasteInputConfirmed = pasteTrace.some(event => event.type === "paste" &&
      event.clipboardText === pasteReplacement) &&
      pasteTrace.some(event => event.type === "input" &&
        event.inputType === "insertFromPaste");
    if (!pasteInputConfirmed) {
      throw new Error("Chromium paste emitted no trusted clipboard/input receipt");
    }

    const pasteResponsePromise = page.waitForResponse(response =>
      response.request().method() === "POST" &&
      decodeURIComponent(new URL(response.url()).pathname) ===
        "/v1/documents/" + documentId + "/story-commit",
    { timeout: 90000 });
    await page.locator("#story-apply-action").click();
    const pasteResponse = await pasteResponsePromise;
    const pasteSubmitted = pasteResponse.request().postDataJSON();
    const pasteReceipt = await pasteResponse.json().catch(() => null);
    if (pasteResponse.status() !== 200 ||
        pasteSubmitted?.base_revision_id !== receipt.revision_id ||
        pasteSubmitted?.command?.kind !== "replace_story_range" ||
        pasteSubmitted.command.story_id !== selected.storyId ||
        pasteSubmitted.command.start_scalar !== 1 ||
        pasteSubmitted.command.end_scalar !== 2 ||
        pasteSubmitted.command.replacement_text !== pasteReplacement ||
        pasteReceipt?.protocol_version !== "chaptera.commit-accepted.v1" ||
        pasteReceipt.base_revision_id !== receipt.revision_id ||
        pasteReceipt.revision_id === receipt.revision_id ||
        pasteReceipt.canonical_operation?.replacement_text !== pasteReplacement ||
        pasteReceipt.replayed !== false || calls.length !== 2) {
      throw new Error("physical paste did not produce one exact authorized Story commit");
    }
    await page.waitForFunction(() =>
      document.querySelector("#story-status")?.textContent?.includes(
        "Exact server revision and reopened Story text confirmed"),
    null, { timeout: 120000 });
    const pastedCurrent = await readProduct();
    if (pastedCurrent.revision_id !== pasteReceipt.revision_id ||
        pastedCurrent.authoring_graph?.stories?.[selected.storyId]?.text !== pastedCanonical) {
      throw new Error("fresh canonical current did not preserve physical paste");
    }
    const pastedScene = await readScene(pasteReceipt.revision_id);
    if (!pastedScene.stories?.some(story =>
      story.story_id === selected.storyId && story.text === pastedCanonical)) {
      throw new Error("Reader Scene did not project physical pasted Story text");
    }
    finalRevisionId = pasteReceipt.revision_id;
    finalCanonical = pastedCanonical;
    pasteRevisionId = pasteReceipt.revision_id;
  }

  const exportResult = await page.evaluate(async ({id, revisionId}) => {
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin", cache: "no-store",
    });
    const session = await sessionResponse.json();
    if (!sessionResponse.ok || typeof session.csrf_token !== "string") {
      throw new Error("authenticated browser has no current CSRF session");
    }
    const layoutEnvironmentId = "sha256:" + "b".repeat(64);
    const request = {
      protocol_version: "chaptera.export-create.v1",
      document_id: id,
      revision_id: revisionId,
      target_profile: "idml:bounded-editable",
      layout_environment_id: layoutEnvironmentId,
      client_request_id: "story-export-" + crypto.randomUUID(),
    };
    const missingCsrf = await fetch("/v1/exports", {
      method: "POST", credentials: "same-origin", cache: "no-store",
      headers: {"content-type": "application/json"},
      body: JSON.stringify({...request, client_request_id: request.client_request_id + "-denied"}),
    });
    if (missingCsrf.status !== 403) {
      throw new Error("ExportJob create without CSRF was not denied");
    }
    const create = await fetch("/v1/exports", {
      method: "POST", credentials: "same-origin", cache: "no-store",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": session.csrf_token,
      },
      body: JSON.stringify(request),
    });
    let job = await create.json().catch(() => null);
    if (!create.ok || job?.protocol_version !== "chaptera.export-job-http.v1" ||
        job.document_id !== id || job.revision_id !== revisionId ||
        job.target_profile !== request.target_profile ||
        job.layout_environment_id !== layoutEnvironmentId || !job.job_id) {
      throw new Error("browser ExportJob did not bind the accepted Story revision");
    }
    const deadline = Date.now() + 120000;
    while (job.status !== "ready" && Date.now() < deadline) {
      if (["failed", "cancelled", "expired"].includes(job.status)) {
        throw new Error("browser ExportJob reached terminal state " + job.status);
      }
      await new Promise(resolve => setTimeout(resolve, 250));
      const status = await fetch("/v1/exports/" + encodeURIComponent(job.job_id), {
        credentials: "same-origin", cache: "no-store",
      });
      job = await status.json().catch(() => null);
      if (!status.ok || job?.revision_id !== revisionId) {
        throw new Error("browser ExportJob status lost exact revision identity");
      }
    }
    if (job.status !== "ready" || !job.artifact_id || !job.loss_report_id) {
      throw new Error("browser ExportJob did not publish artifact and LossReport");
    }

    const forged = await fetch(
      "/v1/exports/" + encodeURIComponent(job.job_id) +
        "/artifacts/" + encodeURIComponent(job.artifact_id + "-forged"),
      {credentials: "same-origin", cache: "no-store"},
    );
    if (forged.status !== 409) {
      throw new Error("forged export artifact identity was not rejected");
    }
    const artifact = await fetch(
      "/v1/exports/" + encodeURIComponent(job.job_id) +
        "/artifacts/" + encodeURIComponent(job.artifact_id),
      {credentials: "same-origin", cache: "no-store"},
    );
    const artifactBytes = new Uint8Array(await artifact.arrayBuffer());
    if (!artifact.ok || artifact.headers.get("content-type") !==
        "application/vnd.adobe.indesign-idml-package" ||
        !artifact.headers.get("content-disposition")?.includes(".idml") ||
        artifact.headers.get("cache-control") !== "private, no-store" ||
        artifact.headers.get("x-content-type-options") !== "nosniff" ||
        artifactBytes.length < 2 || artifactBytes[0] !== 0x50 || artifactBytes[1] !== 0x4b) {
      throw new Error("authorized exact-revision IDML stream is invalid");
    }
    const loss = await fetch(
      "/v1/exports/" + encodeURIComponent(job.job_id) +
        "/loss-reports/" + encodeURIComponent(job.loss_report_id),
      {credentials: "same-origin", cache: "no-store"},
    );
    const lossBytes = new Uint8Array(await loss.arrayBuffer());
    let lossJson = null;
    try { lossJson = JSON.parse(new TextDecoder().decode(lossBytes)); } catch {}
    if (!loss.ok || loss.headers.get("content-type") !== "application/json" ||
        !loss.headers.get("content-disposition")?.includes("-loss-report.json") ||
        loss.headers.get("cache-control") !== "private, no-store" ||
        loss.headers.get("x-content-type-options") !== "nosniff" ||
        !lossJson || typeof lossJson !== "object" || Array.isArray(lossJson)) {
      throw new Error("authorized exact-revision LossReport stream is invalid");
    }
    const digest = async bytes => {
      const raw = await crypto.subtle.digest("SHA-256", bytes);
      return [...new Uint8Array(raw)].map(value => value.toString(16).padStart(2, "0")).join("");
    };
    return {
      job_id: job.job_id,
      target_profile: job.target_profile,
      layout_environment_id: job.layout_environment_id,
      artifact_sha256: await digest(artifactBytes),
      artifact_byte_len: artifactBytes.length,
      loss_report_sha256: await digest(lossBytes),
      loss_report_byte_len: lossBytes.length,
      csrf_negative_confirmed: true,
      forged_artifact_negative_confirmed: true,
    };
  }, {id: documentId, revisionId: finalRevisionId});
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
  const anonymousArtifact = await anonPage.goto(
    origin + "/v1/exports/" + encodeURIComponent(exportResult.job_id) +
      "/artifacts/forged",
    { waitUntil: "domcontentloaded", timeout: 30000 },
  );
  if (!anonymousArtifact ||
      (anonymousArtifact.status() !== 401 && anonymousArtifact.status() !== 403)) {
    throw new Error("anonymous browser unexpectedly reached an export artifact");
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
    committed_revision_id: finalRevisionId,
    initial_text_revision_id: receipt.revision_id,
    paste_revision_id: pasteRevisionId,
    real_dom_textarea_input: true,
    input_path: browserEngine === "chromium"
      ? "chromium_ime_composition"
      : "webkit_caret_range_keyboard",
    selected_first_scalar_range: true,
    composition_events_confirmed: compositionEventsConfirmed,
    composition_apply_fenced: compositionApplyFenced,
    caret_range_input_confirmed: caretRangeInputConfirmed,
    physical_clipboard_paste_confirmed: pasteInputConfirmed,
    authorized_story_commit_count: calls.length,
    one_authorized_story_commit: true,
    canonical_current_text_confirmed: true,
    source_neutral_reader_scene_text_refreshed: true,
    authoritative_relayout_overset_verified: false,
    browser_reload_preserves_story: true,
    anonymous_read_denied: true,
    source_bytes_in_receipt: false,
    browser_layout_is_canonical: false,
    story_undo_redo_claim: false,
    export_job_claim: true,
    export_job_id: exportResult.job_id,
    export_target_profile: exportResult.target_profile,
    export_layout_environment_id: exportResult.layout_environment_id,
    export_artifact_sha256: exportResult.artifact_sha256,
    export_artifact_byte_len: exportResult.artifact_byte_len,
    export_loss_report_sha256: exportResult.loss_report_sha256,
    export_loss_report_byte_len: exportResult.loss_report_byte_len,
    export_csrf_negative_confirmed: exportResult.csrf_negative_confirmed,
    export_forged_artifact_negative_confirmed:
      exportResult.forged_artifact_negative_confirmed,
    export_anonymous_denied: true,
    two_engine_claim: false,
  };
  const persisted = JSON.stringify(result);
  if (persisted.includes("Sample3.pub") ||
      (priorText.length >= 16 && persisted.includes(priorText)) ||
      (desired.length >= 16 && persisted.includes(desired)) ||
      (finalCanonical.length >= 16 && persisted.includes(finalCanonical))) {
    throw new Error("private fixture label or Story contents leaked into acceptance receipt");
  }
  await writeFile(receiptPath, JSON.stringify(result, null, 2) + "\n");
  process.stdout.write(persisted + "\n");
} finally {
  await browser.close();
}
