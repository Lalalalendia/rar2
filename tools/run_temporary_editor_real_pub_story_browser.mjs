// Independent physical Guest Story acceptance. Real Chromium, real PUB,
// Caddy/TLS, real Rust Reader/Editor workers and actual browser downloads.
// No synthetic fetch/POST edit, Product login, S3 or native PUB writer claim.
import assert from "node:assert/strict";
import {
  prepareStoryTextForTextarea, restoreStoryTextFromTextarea,
} from "../apps/web/story-line-endings-v1.mjs";
import { diffObservedTextToScalarReplacement } from "../apps/web/text-protocol-v1.mjs";
import { temporaryPlainStoryCandidates } from "../apps/web/temporary-edit-move-client-v1.mjs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium } from "playwright";

function arg(name) {
  const at = process.argv.indexOf("--" + name);
  if (at < 0 || !process.argv[at + 1]) throw Error("missing --" + name);
  return process.argv[at + 1];
}
const origin = arg("origin");
const fixture = resolve(arg("fixture"));
const fixtureHash = arg("fixture-sha256");
const resultFile = resolve(arg("out"));
const target = arg("target");
if (!/^https:\/\/reader\.chaptera\.test:\d+$/.test(origin) ||
    !/^[0-9a-f]{64}$/.test(fixtureHash) ||
    !["idml", "odg"].includes(target)) {
  throw Error("Guest Story physical acceptance requires pinned local TLS target");
}
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
const original = await readFile(fixture);
assert.equal(original.length, 72192, "expected immutable real Sample3 fixture length");
assert.equal(sha256(original), fixtureHash, "immutable real Publisher Story fixture hash changed");

const browser = await chromium.launch({
  headless: true, args: ["--no-proxy-server",
    "--host-resolver-rules=MAP reader.chaptera.test 127.0.0.1"],
});
const page = await browser.newPage({
  ignoreHTTPSErrors: true,
  acceptDownloads: true,
  viewport: {width: 1280, height: 900},
});
const requests = [], responses = [], errors = [], openReplyDiagnostics = [];
const transportFailures = [];
page.on("requestfailed", request => {
  if (!request.url().startsWith(origin)) return;
  const failed = request.failure()?.errorText ?? "";
  // Metadata-only diagnostic: no URLs, session identifiers, headers, bodies.
  const reason = failed.includes("ERR_CONTENT_LENGTH_MISMATCH") ? "length_mismatch"
    : failed.includes("ERR_HTTP2_PROTOCOL_ERROR") ? "http2_transport"
    : failed.includes("ERR_CONNECTION_RESET") ? "connection_reset"
    : failed.includes("ERR_ABORTED") ? "aborted"
    : failed.includes("ERR_FAILED") ? "transport_failed" : "other";
  const pathname = new URL(request.url()).pathname;
  transportFailures.push({
    action: pathname.endsWith("/open") ? "open"
      : pathname.endsWith("/scene") ? "scene" : "other",
    method: request.method(), reason,
  });
});
page.on("pageerror", error => errors.push(error.name || "pageerror"));
page.on("request", request => {
  if (request.url().startsWith(origin)) {
    requests.push({path: new URL(request.url()).pathname,
      method: request.method(), url: request.url()});
  }
});
page.on("response", response => {
  if (response.url().startsWith(origin)) {
    const path = new URL(response.url()).pathname;
    responses.push({path, status: response.status()});
    if (path.startsWith("/v1/reader/guest-sessions/") && path.endsWith("/open")) {
      // Diagnostics contain only response metadata and bounded JSON shape,
      // never raw PUB, bearer, Story text, base64 or response fragments.
      openReplyDiagnostics.push(response.body().then(bytes => {
        let parse = "invalid_json", keys = 0, classification = null;
        try {
          const value = JSON.parse(bytes.toString("utf8"));
          if (value && typeof value === "object" && !Array.isArray(value)) {
            parse = "object"; keys = Object.keys(value).length;
            classification = ["supported","partial","salvage","unsupported","rejected"]
              .includes(value.classification) ? value.classification : null;
          } else parse = typeof value;
        } catch { /* metadata only */ }
        const headers = response.headers();
        return {mime: (headers["content-type"] || "").slice(0, 60),
          encoding: (headers["content-encoding"] || "none").slice(0, 24),
          length_header: /^\d{1,12}$/.test(headers["content-length"] || "")
            ? Number(headers["content-length"]) : null,
          bytes: bytes.length, parse, keys, classification};
      }).catch(error => ({read_error: error.name || "unknown"})));
    }
  }
});
const basePath = id => "/v1/editor/temporary-sessions/" + id;
const SOURCE_MARKER = "ChapteraGuestStoryProof2026";


// Privacy-safe source admission trace: only counts, never text, identifiers,
// private PUB bytes, edit request, source fragments or bearer.
function storyEligibilityCounts(graph) {
  const nodes = Object.values(graph?.nodes ?? {});
  const stories = Object.entries(graph?.stories ?? {});
  const frameOwners = new Map(), tableOwned = new Set();
  let frameNodes = 0;
  for (const node of nodes) {
    const frameId = node?.payload?.story_frame?.story_id;
    if (typeof frameId === "string") {
      frameNodes++;
      frameOwners.set(frameId, (frameOwners.get(frameId) ?? 0) + 1);
    }
    for (const owner of [node?.payload?.table_story, node?.payload?.table]) {
      if (typeof owner?.story_id === "string") tableOwned.add(owner.story_id);
    }
  }
  const rejected = {
    invalid_id: 0, frame_owner_count: 0, table_owned: 0,
    unrecognized_story_text: 0, source_line_endings: 0,
    disallowed_control: 0, too_large: 0, rich_metadata: 0,
  };
  const observed = {contains_cr: 0, contains_lf: 0, terminal_cr: 0,
    contains_runs: 0, contains_paragraphs: 0, contains_fields: 0,
    contains_hyperlinks: 0};
  let eligible = 0;
  for (const [id, story] of stories) {
    if (story?.text?.includes("\r")) observed.contains_cr++;
    if (story?.text?.includes("\n")) observed.contains_lf++;
    if (story?.text?.endsWith("\r")) observed.terminal_cr++;
    for (const field of ["runs", "paragraphs", "fields", "hyperlinks"]) {
      if (Array.isArray(story?.[field]) && story[field].length) {
        observed["contains_" + field]++;
      }
    }
    let why = null;
    if (typeof id !== "string" || !id || id.length > 128) why = "invalid_id";
    else if ((frameOwners.get(id) ?? 0) !== 1) why = "frame_owner_count";
    else if (tableOwned.has(id)) why = "table_owned";
    else if (typeof story?.text !== "string") why = "unrecognized_story_text";
    else {
      let profile;
      try { profile = prepareStoryTextForTextarea(story.text); }
      catch { why = "source_line_endings"; }
      if (!why && !profile.value.length) why = "unrecognized_story_text";
      else if (!why && [...profile.value].some(ch =>
        ch !== "\n" && /\p{Cc}/u.test(ch))) why = "disallowed_control";
      else if (!why && new TextEncoder().encode(story.text).length > 8192) why = "too_large";
      else if (!why && ["runs", "paragraphs", "fields", "hyperlinks"].some(key =>
        Array.isArray(story[key]) && story[key].length)) why = "rich_metadata";
    }
    if (why) rejected[why]++;
    else eligible++;
  }
  return {node_count: nodes.length, story_count: stories.length,
    story_frame_node_count: frameNodes, unique_frame_owner_count: frameOwners.size,
    table_story_count: tableOwned.size, eligible_count: eligible,
    first_rejection_counts: rejected, observed_source_counts: observed};
}



// This READ uses the same private browser bearer but creates no state/TTL.
// Mutations MUST occur through the user-visible textarea and Apply button.
async function readCurrent(action, documentId) {
  return page.evaluate(async ({action, documentId}) => {
    const saved = JSON.parse(sessionStorage.getItem("chaptera.temporary-edit.v0") || "null");
    if (!saved?.access_token || saved.session_id !== documentId) {
      return {status: 0, body: null};
    }
    const url = "/v1/editor/temporary-sessions/" + documentId + "/" + action;
    for (let attempt = 0; attempt < 12; attempt++) {
      const response = await fetch(url, {
        method: "GET",
        headers: {"x-chaptera-reader-session": saved.access_token,
          "accept": "application/json"},
        credentials: "omit", mode: "same-origin", cache: "no-store",
        redirect: "error", referrerPolicy: "no-referrer",
      });
      if (response.status === 429) {
        await new Promise(done => setTimeout(done, 2200));
        continue;
      }
      return {status: response.status,
        body: await response.json().catch(() => null)};
    }
    return {status: 429, body: null};
  }, {action, documentId});
}
try {
  const pageResponse = await page.goto(origin + "/editor/temporary-preview", {
    waitUntil: "domcontentloaded", timeout: 30000,
  });
  assert.equal(pageResponse?.status(), 200, "Guest Editor HTML not routed through TLS");
  await page.locator("#temporary-file").setInputFiles(fixture);
  await page.locator("#temporary-start").click();
  try {
    await page.waitForFunction(() =>
      document.querySelector("#temporary-status")?.textContent.includes(
        "Открыта редактируемая ревизия"), null, {timeout: 170000});
  } catch {
    const diagnostic = await Promise.allSettled(openReplyDiagnostics);
    throw Error("real PUB Story browser could not open: " +
      String(await page.locator("#temporary-status").textContent()).slice(0, 160) +
      " status=" + JSON.stringify(responses.slice(-12)) +
      " open_reply_metadata=" + JSON.stringify(diagnostic.map(item =>
        item.status === "fulfilled" ? item.value : {read_error: "rejected"})) +
      " transport_failures=" + JSON.stringify(transportFailures.slice(-8)));
  }
  const saved = await page.evaluate(() => {
    const item = JSON.parse(sessionStorage.getItem("chaptera.temporary-edit.v0") || "null");
    return item && {session_id: item.session_id,
      source_sha256: item.source_sha256};
  });
  assert.ok(/^tempedit:[0-9a-f]{32}$/.test(saved?.session_id || ""));
  assert.equal(saved.source_sha256, fixtureHash);
  const sessionId = saved.session_id;
  const originalHead = await readCurrent("current", sessionId);
  assert.equal(originalHead.status, 200);
  const head = originalHead.body;
  assert.equal(head.protocol_version, "chaptera.temporary-editor-current.v1");
  assert.equal(head.source_hash, fixtureHash);
  assert.equal(head.revision_cursor, 0);
  assert.ok(/^sha256:[0-9a-f]{64}$/.test(head.revision_id || ""));
  const eligibility = storyEligibilityCounts(head.authoring_graph);
  assert.ok(eligibility.eligible_count > 0,
    "Real PUB contains no admissible plain Story: " + JSON.stringify(eligibility));
  try {
    await page.locator("#temporary-story-panel").waitFor({
      state: "visible", timeout: 45000,
    });
  } catch (error) {
    const visible = await page.locator("#temporary-story-panel").isVisible();
    throw Error("Guest Story panel hidden despite candidate gate: " +
      JSON.stringify(eligibility) + " visible=" + visible +
      " wait_error=" + error.name);
  }
  const beforeScene = await readCurrent("scene", sessionId);
  assert.equal(beforeScene.status, 200);
  assert.equal(beforeScene.body.revision_id, head.revision_id);
  assert.equal(beforeScene.body.scene.source_hash, fixtureHash);

  // Physical user path: click an actually painted source-backed TextFrame.
  // The DOM hit identifies a rendered NodeId, while the canonical graph owns
  // its StoryId. A GET, programmatic select(), or invented click is insufficient.
  const graph = head.authoring_graph;
  const admittedIds = new Set(temporaryPlainStoryCandidates(graph)
    .map(candidate => candidate.story_id));
  const sourceFrames = beforeScene.body.scene.nodes.filter(node => {
    const owner = graph?.nodes?.[node.node_id];
    const bounds = node.bounds;
    return !node.origin_node_id && !node.parent_node_id &&
      graph?.pages?.[node.page_id] && owner?.header?.parent_id === node.page_id &&
      admittedIds.has(owner?.payload?.story_frame?.story_id) &&
      ["x", "y", "width", "height"].every(key => Number.isSafeInteger(bounds?.[key])) &&
      bounds.width > 0 && bounds.height > 0;
  }).sort((a, b) => a.bounds.width * a.bounds.height -
    b.bounds.width * b.bounds.height);
  assert.ok(sourceFrames.length > 0,
    "real PUB has no source-backed, page-owned visual plain TextFrame");

  let picked = null;
  for (const node of sourceFrames.slice(0, 100)) {
    const group = page.locator('#temporary-pages g[data-node-id="' + node.node_id + '"]').first();
    if (await group.count() === 0) continue;
    try { await group.scrollIntoViewIfNeeded({timeout: 2200}); } catch { continue; }
    const screen = await page.evaluate(choice => {
      const svg = [...document.querySelectorAll("#temporary-pages svg.page[data-page-id]")]
        .find(item => item.getAttribute("data-page-id") === choice.pageId);
      const group = [...(svg?.querySelectorAll("g[data-node-id]") ?? [])]
        .find(item => item.getAttribute("data-node-id") === choice.nodeId);
      if (!svg || !group || !svg.getScreenCTM()) return null;
      const visibleHit = (x, y) => {
        if (!(x >= 0 && x < innerWidth && y >= 0 && y < innerHeight)) return false;
        const target = document.elementFromPoint(x, y);
        return target?.closest?.("svg.page[data-page-id]") === svg &&
          target.closest?.("[data-node-id]")?.getAttribute("data-node-id") === choice.nodeId;
      };
      const fractions = [[.5,.5],[.3,.3],[.7,.3],[.3,.7],[.7,.7]];
      for (const element of [...group.querySelectorAll("text,tspan,rect,path,ellipse,image"), group]) {
        const rect = element.getBoundingClientRect();
        if (!(rect.width > 0 && rect.height > 0)) continue;
        for (const [fx,fy] of fractions) {
          const x = rect.left + rect.width * fx, y = rect.top + rect.height * fy;
          if (visibleHit(x, y)) return {x, y};
        }
      }
      const p = svg.createSVGPoint();
      for (const [fx,fy] of fractions) {
        p.x = choice.bounds.x + choice.bounds.width * fx;
        p.y = choice.bounds.y + choice.bounds.height * fy;
        const point = p.matrixTransform(svg.getScreenCTM());
        if (visibleHit(point.x, point.y)) return {x:point.x, y:point.y};
      }
      return null;
    }, {nodeId: node.node_id, pageId: node.page_id, bounds: node.bounds});
    if (!screen) continue;
    await page.mouse.click(screen.x, screen.y);
    const storyId = graph.nodes[node.node_id].payload.story_frame.story_id;
    if ((await page.locator("#temporary-story-select").inputValue()) === storyId &&
        (await page.locator("#temporary-status").textContent())?.includes("Текстовая рамка выбрана")) {
      picked = {storyId, nodeId: node.node_id};
      break;
    }
  }
  assert.ok(picked, "real Chromium could not select any painted canonical plain TextFrame");
  assert.equal(requests.filter(r => r.method === "POST" &&
    /(story-range|move-node|resize-node|create-shape)$/.test(r.path)).length, 0,
    "canvas text pick must not mutate document state");
  const storyId = picked.storyId;
  const priorText = head.authoring_graph?.stories?.[storyId]?.text;
  assert.ok(typeof priorText === "string" && priorText.length > 0,
    "source-backed real Publisher Story is not exposed in Guest Current");
  const sourceProfile = prepareStoryTextForTextarea(priorText);
  const observedBefore = await page.locator("#temporary-story-text").inputValue();
  assert.equal(observedBefore, sourceProfile.value);
  assert.ok(!priorText.includes(SOURCE_MARKER));
  assert.ok(new TextEncoder().encode(SOURCE_MARKER).length <= 8192);
  const firstLineBreak = observedBefore.indexOf("\n");
  const observedDesired = SOURCE_MARKER +
    (firstLineBreak === -1 ? "" : observedBefore.slice(firstLineBreak));
  const desiredCanonical = restoreStoryTextFromTextarea(observedDesired, sourceProfile);
  const expectedRange = diffObservedTextToScalarReplacement(priorText, desiredCanonical);
  assert.notEqual(priorText, desiredCanonical);
  assert.ok(!/[\p{Cc}]/u.test(expectedRange.replacement_text),
    "Story fixture change must not insert or remove existing Publisher separators");

  // Guest's 6-request admission burst is shared by page bootstrap and
  // inspection GETs; allow a bounded refill before the *UI-owned* mutation.
  await page.waitForTimeout(4500);
  // The real browser owns this mutation. No manual service POST is used.
  await page.locator("#temporary-story-text").fill(observedDesired);
  const ackPromise = page.waitForResponse(response =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname === basePath(sessionId) + "/story-range",
    {timeout: 120000});
  await page.locator("#temporary-story-apply").click();
  const ackResponse = await ackPromise;
  const submitted = ackResponse.request().postDataJSON();
  const headers = await ackResponse.request().allHeaders();
  const accepted = await ackResponse.json().catch(() => null);
  assert.equal(ackResponse.status(), 200, "real Guest Story POST rejected");
  assert.equal(submitted.document_id, sessionId);
  assert.equal(submitted.source_hash, fixtureHash);
  assert.equal(submitted.base_revision_id, head.revision_id);
  assert.equal(submitted.command.kind, "replace_story_range");
  assert.equal(submitted.command.story_id, storyId);
  assert.equal(submitted.command.start_scalar, expectedRange.start_scalar);
  assert.equal(submitted.command.end_scalar, expectedRange.end_scalar);
  assert.equal(submitted.command.replacement_text, expectedRange.replacement_text);
  assert.ok(headers["x-chaptera-reader-session"]?.length === 64);
  assert.ok(headers["x-csrf-token"]?.length === 32);
  assert.equal(accepted?.protocol_version, "chaptera.temporary-editor-story-range.v1");
  assert.equal(accepted.document_id, sessionId);
  assert.equal(accepted.source_hash, fixtureHash);
  assert.equal(accepted.base_revision_id, head.revision_id);
  assert.equal(accepted.client_operation_id, submitted.client_operation_id);
  assert.equal(accepted.replayed, false);
  assert.equal(accepted.scene_refresh, "current");
  assert.notEqual(accepted.revision_id, head.revision_id);
  assert.equal(accepted.canonical_operation?.kind, "replace_story_range");
  assert.equal(accepted.canonical_operation?.story_id, storyId);
  assert.equal(accepted.canonical_operation?.replacement_text,
    expectedRange.replacement_text);

  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Текст подтверждён сервером"), null, {timeout: 120000});
  const revised = await readCurrent("current", sessionId);
  assert.equal(revised.status, 200);
  assert.equal(revised.body.revision_id, accepted.revision_id);
  assert.equal(revised.body.canonical_authoring_revision_id,
    accepted.canonical_authoring_revision_id);
  assert.equal(revised.body.revision_cursor, 1);
  assert.equal(revised.body.expires_at_ms, accepted.expires_at_ms);
  assert.equal(revised.body.authoring_graph?.stories?.[storyId]?.text, desiredCanonical);
  const scene = await readCurrent("scene", sessionId);
  assert.equal(scene.status, 200);
  assert.equal(scene.body.revision_id, accepted.revision_id);
  assert.equal(scene.body.scene?.revision_id, accepted.revision_id);
  assert.equal(scene.body.scene?.source_hash, fixtureHash);
  assert.ok(scene.body.scene?.stories?.some(story =>
    story.story_id === storyId && story.text === desiredCanonical),
    "real Reader Story Scene did not project edited source-backed text");

  // Identical operation-ID replay is not another accepted revision/TTL renewal.
  const replay = await page.evaluate(async ({sessionId, request, headers}) => {
    for (let attempt = 0; attempt < 12; attempt++) {
      const response = await fetch("/v1/editor/temporary-sessions/" + sessionId + "/story-range", {
        method: "POST",
        headers: {"content-type": "application/json", "accept": "application/json",
          "x-chaptera-reader-session": headers["x-chaptera-reader-session"],
          "x-csrf-token": headers["x-csrf-token"]},
        body: JSON.stringify(request), credentials: "omit", mode: "same-origin",
        cache: "no-store", redirect: "error", referrerPolicy: "no-referrer",
      });
      if (response.status === 429) {
        await new Promise(done => setTimeout(done, 2200));
        continue;
      }
      return {status: response.status, body: await response.json().catch(() => null)};
    }
    return {status: 429, body: null};
  }, {sessionId, request: submitted, headers});
  assert.equal(replay.status, 200);
  assert.equal(replay.body?.replayed, true);
  assert.equal(replay.body.revision_id, accepted.revision_id);
  assert.equal(replay.body.expires_at_ms, accepted.expires_at_ms);
  const negative = await page.evaluate(async ({sessionId, request, headers}) => {
    const path = "/v1/editor/temporary-sessions/" + sessionId + "/story-range";
    const option = (payload, bearer) => ({
      method: "POST",
      headers: {"content-type": "application/json",
        "x-chaptera-reader-session": bearer,
        "x-csrf-token": headers["x-csrf-token"]},
      credentials: "omit", mode: "same-origin", cache: "no-store",
      body: JSON.stringify(payload),
    });
    // A new operation against an old head must be refused, not rebased.
    const status = async options => {
      for (let i = 0; i < 12; i++) {
        const response = await fetch(path, options);
        if (response.status !== 429) return response.status;
        await new Promise(done => setTimeout(done, 2200));
      }
      return 429;
    };
    const stale = await status(option({
      ...request, client_operation_id: request.client_operation_id + "-stale"},
      headers["x-chaptera-reader-session"]));
    const conflicting = await status(option({
      ...request, command: {...request.command, replacement_text: "forged"}},
      headers["x-chaptera-reader-session"]));
    const foreign = await status(option(request, "0".repeat(64)));
    return {stale, conflicting, foreign};
  }, {sessionId, request: submitted, headers});
  assert.equal(negative.stale, 409, "stale Guest Story base was not rejected");
  assert.equal(negative.conflicting, 409, "reused Story operation ID changed payload");
  assert.ok([401, 403, 404].includes(negative.foreign),
    "foreign Guest bearer was allowed to mutate Story");
  const reread = await readCurrent("current", sessionId);
  assert.equal(reread.body.revision_cursor, 1);
  assert.equal(reread.body.expires_at_ms, accepted.expires_at_ms);

  // Preserve the user's unmodified UI export flow: regain two Guest
  // admission credits instead of racing it with our negative network probes.
  await page.waitForTimeout(7500);
  // Download via the actual UI, after its own private source-neutral loss report.
  const reportPromise = page.waitForResponse(response =>
    response.request().method() === "GET" &&
    new URL(response.url()).pathname === basePath(sessionId) + "/exports/" + target &&
    new URL(response.url()).searchParams.get("report_only") === "true",
    {timeout: 100000});
  const downloadPromise = page.waitForEvent("download", {timeout: 120000});
  await page.locator("#temporary-download-" + target).click();
  const reportResponse = await reportPromise;
  const report = await reportResponse.json().catch(() => null);
  const download = await downloadPromise;
  assert.equal(download.suggestedFilename(), "chaptera-edited." + target);
  assert.equal(reportResponse.status(), 200);
  assert.equal(report.protocol_version, "chaptera.temporary-edited-export-report.v1");
  assert.equal(report.source_hash, fixtureHash);
  assert.equal(report.revision_id, accepted.revision_id);
  assert.equal(report.canonical_authoring_revision_id,
    accepted.canonical_authoring_revision_id);
  assert.equal(report.target, target);
  assert.equal(report.loss_report?.counts?.blocking, 0);
  assert.equal(report.loss_report?.can_serialize, true);
  const downloaded = await readFile(await download.path());
  assert.ok(downloaded.length > 100, "no real target output bytes");
  assert.equal(sha256(downloaded), report.artifact_sha256);
  const consumer = spawnSync("python3", [
    "tools/verify_temporary_guest_story_export.py",
    "--artifact", await download.path(),
    "--target", target,
    "--expected-text", SOURCE_MARKER,
    "--real-application-consumer",
  ], {encoding: "utf8", timeout: 180000, maxBuffer: 16384});
  if (consumer.status !== 0) {
    // No raw document/Story bytes in the CI error stream.
    throw Error("independent browser-download Story XML consumer rejected " +
      target + " (rc=" + consumer.status + ")");
  }
  const consumed = JSON.parse(consumer.stdout);
  assert.equal(consumed.schema,"chaptera.tempedit-independent-story-xml-consumer.v1");
  assert.equal(consumed.target, target);
  assert.equal(consumed.artifact_sha256, sha256(downloaded));
  assert.equal(consumed.edited_text_recovered, true);
  assert.equal(consumed.target_application_consumer?.reopened, true);
  assert.equal(consumed.target_application_consumer?.story_marker_retained, true);
  assert.equal(consumed.target_application_consumer?.application,
    target === "idml" ? "Scribus" : "LibreOffice Draw");

  const beforeReload = await readCurrent("current", sessionId);
  assert.equal(beforeReload.body.expires_at_ms, accepted.expires_at_ms);
  // The 6-request anonymous admission bucket is shared with independent
  // negative probes, export report/download and this exact Current check.
  // A user's explicit Resume must not race those credits. Waiting does
  // not renew the persisted 60-minute session TTL.
  await page.waitForTimeout(10000);
  await page.reload({waitUntil: "domcontentloaded"});
  await page.locator("#temporary-resume").click();
  try {
    await page.waitForFunction(() =>
      document.querySelector("#temporary-status")?.textContent.includes(
        "Открыта редактируемая ревизия"), null, {timeout: 150000});
  } catch {
    // Source-safe classification of an HTTP/TTL/rate/Scene failure. Never
    // serialize private headers, bearer, document bytes or Story contents.
    const status = await page.locator("#temporary-status").textContent();
    const safeStatus = String(status).replace(/tempedit:[0-9a-f]{32}/g, "{session}")
      .replace(/[0-9a-f]{64}/g, "{sha256}").slice(0, 210);
    const lastHttp = responses.slice(-15).map(item => ({
      route: item.path.replace(/tempedit:[0-9a-f]{32}/g, "{session}"),
      status: item.status,
    }));
    throw Error("physical Guest Story resume failed: status=" + safeStatus +
      " recent_http=" + JSON.stringify(lastHttp) +
      " page_errors=" + JSON.stringify(errors.slice(-4)) +
      " transport=" + JSON.stringify(transportFailures.slice(-4)));
  }
  assert.equal(await page.locator("#temporary-story-text").inputValue(), observedDesired);
  const reopened = await readCurrent("current", sessionId);
  assert.equal(reopened.body.revision_cursor, 1);
  assert.equal(reopened.body.revision_id, accepted.revision_id);
  assert.equal(reopened.body.expires_at_ms, accepted.expires_at_ms);
  assert.equal(reopened.body.authoring_graph.stories[storyId].text, desiredCanonical);

  const cap = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem("chaptera.temporary-edit.v0")).access_token);
  assert.ok(/^[0-9a-f]{64}$/.test(cap));
  assert.ok(requests.every(item => !item.url.includes(cap)),
    "temporary Guest bearer appeared in a request URL");
  assert.equal(errors.length, 0, "real Story browser generated JavaScript errors");
  page.once("dialog", dialog => dialog.accept());
  await page.locator("#temporary-delete").click();
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Сессия удалена с сервера"), null, {timeout: 45000});
  assert.ok(responses.some(x => x.path === basePath(sessionId) && x.status === 204));
  const denied = await page.evaluate(async ({sessionId, bearer}) => {
    const path = "/v1/editor/temporary-sessions/" + sessionId + "/current";
    for (let attempt = 0; attempt < 8; attempt++) {
      const response = await fetch(path, {
        headers: {"x-chaptera-reader-session": bearer},
        credentials: "omit", cache: "no-store",
      });
      // The same-IP admission limiter may return 429 after the full physical
      // Story journey. A 429 is NOT revocation proof: retry within a fixed
      // budget and still demand the authority's permanent 404/410 denial.
      if (response.status !== 429) return response.status;
      if (attempt < 7) await new Promise(done => setTimeout(done, 2200));
    }
    return 429;
  }, {sessionId, bearer: cap});
  assert.ok([404, 410].includes(denied),
    "revoked temporary Story capability must return 404/410, got HTTP " + denied);
  assert.equal(await page.evaluate(() =>
    sessionStorage.getItem("chaptera.temporary-edit.v0")), null);
  assert.equal(sha256(await readFile(fixture)), fixtureHash,
    "original PUB bytes were changed by temporary Story workflow");

  const receipt = {
    schema: "chaptera.tempedit-real-pub-story-consumer.v1",
    status: "PASS",
    source_sha256: fixtureHash, source_byte_len: original.length,
    real_pub: true, real_chromium: true, tls_caddy: true,
    guest_story_ui_commit: true, browser_story_commits: 1,
    canvas_story_pick_proven: true, canvas_pick_no_mutation: true,
    story_id_sha256: sha256(Buffer.from(storyId, "utf8")),
    initial_revision_id: head.revision_id,
    accepted_revision_id: accepted.revision_id,
    canonical_authoring_revision_id: accepted.canonical_authoring_revision_id,
    exact_current_story_reopened: true, exact_reader_scene_story: true,
    identical_operation_id_replay_no_second_revision: true,
    stale_story_write_denied: true,
    conflicting_operation_id_denied: true,
    foreign_bearer_mutation_denied: true,
    read_replay_export_does_not_renew_ttl: true,
    export_target: target,
    changed_artifact_sha256: sha256(downloaded),
    physical_target_xml_consumer: true,
    target_consumer_schema: consumed.schema,
    target_consumer_text_recovered: consumed.edited_text_recovered,
    target_application_consumer: consumed.target_application_consumer.application,
    target_application_reopened: consumed.target_application_consumer.reopened,
    same_browser_session_delete_proven: true,
    revoked_bearer_denied: true,
    bearer_absent_from_urls: true,
    original_pub_immutable: true,
    native_pub_writer_claim: false, public_host_proven: false,
  };
  await mkdir(dirname(resultFile), {recursive: true});
  await writeFile(resultFile, JSON.stringify(receipt, null, 2) + "\n");
  console.log(JSON.stringify({
    schema:receipt.schema, status:receipt.status, target,
    real_pub:true, real_chromium:true, changed_artifact_sha256:sha256(downloaded),
    physical_target_xml_consumer:true, original_pub_immutable:true,
  }));
} finally {
  await browser.close();
}
