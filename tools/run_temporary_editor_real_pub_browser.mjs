// Physical guest V0 acceptance: real Chromium -> TLS/Caddy -> Rust -> isolated
// Publisher parser/replay -> browser MoveNode -> exact Scene -> edited IDML.
// No synthetic fetch, manual POST mutation, Product auth, or persistent project.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium } from "playwright";

function arg(name) {
  const i = process.argv.indexOf("--" + name);
  if (i < 0 || !process.argv[i + 1]) throw Error("missing --" + name);
  return process.argv[i + 1];
}
const origin = arg("origin");
// Explicit, allowlisted public-host mode; default remains the local Caddy test.
const publicOrigin = process.argv.includes("--public-origin");
const fixture = resolve(arg("fixture"));
const fixtureHash = arg("fixture-sha256");
const receiptPath = resolve(arg("out"));
// Preserve the existing bounded IDML proof. PDF is a separate physical
// geometry consumer of the same browser workflow, selected explicitly by CI.
const exportTarget = process.argv.includes("--target") ? arg("target") : "idml";
if (exportTarget !== "idml" && exportTarget !== "pdf") {
  throw Error("guest browser acceptance target must be idml or pdf");
}
if (!(publicOrigin ? origin === "https://reader.chaptera.online" :
      /^https:\/\/reader\.chaptera\.test:\d+$/.test(origin))
    || !/^[0-9a-f]{64}$/.test(fixtureHash)) {
  throw Error("physical guest origin or pinned fixture invalid");
}
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
const original = await readFile(fixture);
assert.equal(sha256(original), fixtureHash, "pinned real source hash mismatch");
assert.ok(original.length > 100000, "not a representative pinned PUB");

const browser = await chromium.launch({
  headless: true, args: publicOrigin ? [] : [
    "--no-proxy-server",
    "--host-resolver-rules=MAP reader.chaptera.test 127.0.0.1",
  ],
});
const page = await browser.newPage({
  acceptDownloads: true,
  // Real public TLS must validate the trusted CA and expected hostname.
  ignoreHTTPSErrors: !publicOrigin,
  viewport: { width: 1280, height: 900 },
});
const observed = { current: [], scenes: [], accepted: [], requests: [], reports: [], responses: [] };
const pendingResponseReads = [];
const browserErrors = [];
page.on("pageerror", error => browserErrors.push(error.name || "PageError"));
page.on("request", request => {
  if (request.url().startsWith(origin)) {
    observed.requests.push({ path: new URL(request.url()).pathname,
      url: request.url(), method: request.method() });
  }
});
page.on("response", response => {
  if (!response.url().startsWith(origin)) return;
  const path = new URL(response.url()).pathname;
  observed.responses.push({path:path.replace(/tempedit:[0-9a-f]{32}/g, "{session}"),
    status: response.status(), mime: response.headers()["content-type"] || null});
  if (response.status() !== 200) return;
  let dest;
  if (/\/v1\/editor\/temporary-sessions\/[^/]+\/current$/.test(path)) {
    dest = observed.current;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) {
    dest = observed.scenes;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/move-node$/.test(path)) {
    dest = observed.accepted;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/exports\/(?:idml|odg|pdf)$/.test(path)
      && new URL(response.url()).searchParams.get("report_only") === "true") {
    dest = observed.reports;
  }
  if (dest) {
    pendingResponseReads.push(response.json().then(data => dest.push(data)));
  }
});
async function settleResponses() {
  const settled = await Promise.allSettled(pendingResponseReads.splice(0));
  for (const result of settled) {
    if (result.status !== "fulfilled") {
      throw Error("physical guest HTTP reply was not JSON");
    }
  }
}
function direct(node) {
  return node && !node.origin_node_id && !node.parent_node_id &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(node.node_id);
}
function contains(node, x, y) {
  const b = node.bounds;
  return Number.isSafeInteger(b?.x) && Number.isSafeInteger(b?.y) &&
    Number.isSafeInteger(b?.width) && Number.isSafeInteger(b?.height) &&
    b.width > 0 && b.height > 0 &&
    x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height;
}
function candidatePoints(scene, current, target) {
  const shapes = scene.nodes.filter(n =>
    direct(n) && current.authoring_graph?.nodes?.[n.node_id]);
  // The actual IDML exporter uses pub-editor::frame_from_payload:
  // payload.story_frame.story_id, NOT node.kind (imported publisher text
  // frames may retain NodeKind::Shape). Do not infer IDML admission from the
  // visual Reader kind or the authored NodeKind.
  const graph = current.authoring_graph;
  const textFrames = shapes.filter(n => {
    const frame = graph.nodes[n.node_id]?.payload?.story_frame;
    return typeof frame?.story_id === "string" &&
      Object.hasOwn(graph.stories ?? {}, frame.story_id);
  });
  const paintedRects = shapes.filter(n =>
    (Array.isArray(n.paint?.fill_rgb) && n.paint.fill_rgb.length === 3)
    || Number(n.paint?.line?.width_emu) > 0);
  const choices = [];
  for (const node of (target === "pdf" ? paintedRects : textFrames)) {
    // Require an unambiguous page-local hit; the browser must never edit
    // projected, master, group-child or nonauthoritative stacking candidates.
    for (const fraction of [[.5,.5],[.25,.25],[.75,.25],[.25,.75],[.75,.75]]) {
      const x = Math.round(node.bounds.x + node.bounds.width * fraction[0]);
      const y = Math.round(node.bounds.y + node.bounds.height * fraction[1]);
      if (!contains(node, x, y)) continue;
      const hits = shapes.filter(other =>
        other.page_id === node.page_id && contains(other,x,y));
      if (hits.length === 1 && hits[0].node_id === node.node_id) {
        choices.push({ nodeId: node.node_id, pageId: node.page_id,
          xEmu: x, yEmu: y, area: node.bounds.width * node.bounds.height,
          kind: node.kind });
      }
    }
  }
  choices.sort((a,b) => a.area - b.area || a.nodeId.localeCompare(b.nodeId));
  return choices;
}
async function hitOnScreen(choice) {
  const locator = page.locator('[data-node-id="' + choice.nodeId + '"]').first();
  if (await locator.count() === 0) return null;
  try { await locator.scrollIntoViewIfNeeded({ timeout: 1800 }); } catch { return null; }
  return page.evaluate(c => {
    const svg = [...document.querySelectorAll("#temporary-pages svg.page[data-page-id]")]
      .find(el => el.getAttribute("data-page-id") === c.pageId);
    if (!svg || !svg.getScreenCTM()) return null;
    const p = svg.createSVGPoint();
    p.x = c.xEmu; p.y = c.yEmu;
    const screen = p.matrixTransform(svg.getScreenCTM());
    if (!(screen.x > 0 && screen.x < innerWidth
        && screen.y > 0 && screen.y < innerHeight)) return null;
    const target = document.elementFromPoint(screen.x, screen.y);
    if (target?.closest?.("svg.page[data-page-id]") !== svg) return null;
    const id = target.closest?.("[data-node-id]")?.getAttribute("data-node-id");
    if (id && id !== c.nodeId) return null;
    return { x: screen.x, y: screen.y, nodeId: c.nodeId };
  }, choice);
}
function privatePath(action, sid) {
  return "/v1/editor/temporary-sessions/" + sid + "/" + action;
}
try {
  const result = await page.goto(origin + "/editor/temporary-preview", {
    waitUntil: "domcontentloaded", timeout: 30000,
  });
  assert.equal(result?.status(), 200, "trusted editor edge did not expose guest editor");
  await page.locator("#temporary-file").setInputFiles(fixture);
  await page.locator("#temporary-start").click();
  try {
    await page.waitForFunction(() =>
      /(?:PUB открыт|Открыта редактируемая ревизия)/.test(
        document.querySelector("#temporary-status")?.textContent ?? ""), null, { timeout: 170000 });
  } catch {
    const ui = await page.locator("#temporary-status").textContent();
    throw Error("real PUB editor did not open; visible_status=" +
      String(ui).slice(0,220) + " http=" +
      JSON.stringify(observed.responses.slice(-18)) +
      " page_errors=" + JSON.stringify(browserErrors));
  }
  await settleResponses();
  const oldHead = observed.current.at(-1);
  const sourceScene = observed.scenes.at(-1)?.scene;
  assert.equal(oldHead?.protocol_version, "chaptera.temporary-editor-current.v1");
  assert.equal(oldHead?.source_hash, fixtureHash);
  assert.equal(oldHead?.revision_cursor, 0);
  assert.equal(sourceScene?.protocol_version,"chaptera.reader-scene.v1");
  assert.equal(sourceScene?.source_hash, fixtureHash);
  assert.equal(sourceScene?.revision_id, oldHead.revision_id);
  assert.ok(Array.isArray(sourceScene?.nodes) && sourceScene.nodes.length > 0,
    "real isolated Publisher scene did not render");

  // Download an actual pristine PDF before any pointer or authoring event.
  // Compare the browser's downloaded bytes against the canonical private
  // report and independently reopen the physical PDF xref and page tree.
  let pristinePdfProof = null;
  if (exportTarget === "pdf") {
    const untouched = page.locator("#temporary-download-pdf");
    await untouched.waitFor({ state: "visible" });
    assert.equal(await untouched.isEnabled(), true,
      "unedited supported PUB must allow baseline PDF");
    assert.equal((await untouched.textContent())?.trim(), "Скачать PDF");
    const baselinePromise = page.waitForEvent("download", { timeout: 90000 });
    await untouched.click();
    const baselineDownload = await baselinePromise;
    await settleResponses();
    assert.equal(baselineDownload.suggestedFilename(), "chaptera-original.pdf");
    const downloaded = await readFile(await baselineDownload.path());
    assert.ok(downloaded.length > 40 && downloaded.length <= 16 * 1024 * 1024,
      "baseline PDF byte length must be bounded");
    assert.ok(downloaded.subarray(0, 5).equals(Buffer.from("%PDF-")),
      "baseline download has no PDF signature");
    const pdfText = downloaded.toString("latin1");
    const xrefMatch = pdfText.match(/startxref\s+(\d+)\s+%%EOF\s*$/);
    assert.ok(xrefMatch, "baseline PDF must end with startxref and EOF");
    const xrefOffset = Number(xrefMatch[1]);
    assert.ok(Number.isSafeInteger(xrefOffset) && xrefOffset > 0 &&
      xrefOffset < downloaded.length &&
      pdfText.slice(xrefOffset).startsWith("xref\n"),
      "baseline physical PDF cross-reference cannot be reopened");
    const pageCount = [...pdfText.matchAll(/\/Type\s*\/Page\s*\/Parent/g)].length;
    assert.ok(pageCount > 0 && pageCount <= 1000,
      "baseline PDF contains no physical pages");
    const baselineReport = observed.reports.at(-1);
    const baselineSha = sha256(downloaded);
    assert.equal(baselineReport?.protocol_version,
      "chaptera.temporary-edited-export-report.v1");
    assert.equal(baselineReport?.target, "pdf");
    assert.equal(baselineReport?.is_baseline, true);
    assert.equal(baselineReport?.source_hash, fixtureHash);
    assert.equal(baselineReport?.revision_id, oldHead.revision_id);
    assert.equal(baselineReport?.canonical_authoring_revision_id,
      oldHead.canonical_authoring_revision_id);
    assert.equal(baselineReport?.artifact_sha256, baselineSha);
    assert.equal(baselineReport?.loss_report?.counts?.blocking, 0);
    assert.equal(baselineReport?.loss_report?.counts?.moved_nodes, 0);
    assert.ok(observed.responses.some(r => r.path.endsWith("/exports/pdf") &&
      r.status === 200 && r.mime === "application/pdf"),
      "baseline PDF lacked an actual HTTPS application/pdf response");
    assert.equal(observed.requests.filter(r => r.method === "POST" &&
      r.path.endsWith("/move-node")).length, 0,
      "PDF without an edit must not create an artificial MoveNode");
    assert.equal(observed.current.at(-1)?.revision_cursor, 0);
    assert.equal(sha256(await readFile(fixture)), fixtureHash,
      "pristine export altered original PUB");
    pristinePdfProof = { sha256: baselineSha, pageCount };
  }

  const candidates = candidatePoints(sourceScene, oldHead, exportTarget);
  assert.ok(candidates.length,
    exportTarget === "pdf"
      ? "real PUB has no unambiguous direct painted rectangle for PDF consumer"
      : "real PUB has no unambiguous direct canonical StoryFrame for IDML consumer");
  let selected = null;
  for (const choice of candidates.slice(0, 120)) {
    const screen = await hitOnScreen(choice);
    if (screen) {
      selected = { ...choice, screen };
      break;
    }
  }
  assert.ok(selected, exportTarget === "pdf"
    ? "no actual browser-visible painted rectangle is selectable"
    : "no actual browser-visible canonical StoryFrame is selectable");
  if (exportTarget === "idml") {
    const selectedNode = oldHead.authoring_graph.nodes[selected.nodeId];
    const storyId = selectedNode.payload?.story_frame?.story_id;
    assert.ok(typeof storyId === "string" &&
        Object.hasOwn(oldHead.authoring_graph.stories ?? {}, storyId),
      "chosen node lacks an exportable canonical StoryFrame");
  } else {
    const paint = sourceScene.nodes.find(n => n.node_id === selected.nodeId)?.paint;
    assert.ok(Array.isArray(paint?.fill_rgb) || Number(paint?.line?.width_emu) > 0,
      "selected browser node is not a painted PDF rectangle");
  }
  const originalBounds = oldHead.authoring_graph.nodes[selected.nodeId].header.bounds;

  // This is a real pointer-driven Chromium event. The test does not call POST
  // /move-node directly and cannot create an edit outside the browser UI.
  await page.mouse.move(selected.screen.x, selected.screen.y);
  await page.mouse.down();
  await page.mouse.move(selected.screen.x + 20, selected.screen.y + 15,
    { steps: 5 });
  await page.mouse.up();
  try {
    await page.waitForFunction(() =>
      document.querySelector("#temporary-status")?.textContent.includes(
        "Правка сохранена в ревизии"), null, { timeout: 60000 });
  } catch {
    const state = await page.locator("#temporary-status").textContent();
    throw Error("physical browser MoveNode failed with UI status: " +
      String(state).slice(0,180) + " http=" +
      JSON.stringify(observed.responses.slice(-18)) +
      " page_errors=" + JSON.stringify(browserErrors));
  }
  await settleResponses();
  const nextHead = observed.current.at(-1);
  const accepted = observed.accepted.at(-1);
  const revised = observed.scenes.at(-1);
  assert.equal(accepted?.protocol_version,
    "chaptera.temporary-editor-move-node.v1");
  assert.equal(accepted.document_id, oldHead.document_id);
  assert.equal(accepted.source_hash, fixtureHash);
  assert.equal(accepted.base_revision_id, oldHead.revision_id);
  assert.equal(accepted.replayed,false);
  assert.notEqual(accepted.revision_id, oldHead.revision_id);
  assert.equal(accepted.revision_id, nextHead?.revision_id);
  assert.equal(nextHead.revision_cursor, 1);
  assert.equal(nextHead.expires_at_ms, accepted.expires_at_ms);
  assert.equal(revised?.revision_id, accepted.revision_id);
  assert.equal(revised?.scene?.revision_id, accepted.revision_id);
  assert.equal(revised?.scene?.source_hash, fixtureHash);
  const geometry = revised.scene.nodes.find(n => n.node_id === selected.nodeId)?.bounds;
  const authoring = nextHead.authoring_graph.nodes[selected.nodeId]?.header?.bounds;
  assert.ok(geometry && authoring, "edited node vanished from the revised scene");
  assert.equal(geometry.x, authoring.x);
  assert.equal(geometry.y, authoring.y);
  assert.ok(geometry.x !== originalBounds.x || geometry.y !== originalBounds.y,
    "real browser gesture left canonical geometry unchanged");

  const target = exportTarget;
  await page.locator("#temporary-download-" + target).waitFor({state:"visible"});
  const downloadPromise = page.waitForEvent("download", { timeout: 90000 });
  await page.locator("#temporary-download-" + target).click();
  const download = await downloadPromise;
  await settleResponses();
  assert.equal(download.suggestedFilename(),"chaptera-edited." + target);
  const artifactPath = await download.path();
  const artifact = await readFile(artifactPath);
  assert.ok(artifact.subarray(0, target === "pdf" ? 5 : 2).equals(
    Buffer.from(target === "pdf" ? "%PDF-" : "PK")),
    "changed browser download lacks the required physical target signature");
  const report = observed.reports.at(-1);
  assert.equal(report?.revision_id, accepted.revision_id);
  assert.equal(report?.canonical_authoring_revision_id,
    accepted.canonical_authoring_revision_id);
  assert.equal(report?.source_hash, fixtureHash);
  assert.equal(report?.target, target);
  assert.equal(sha256(artifact), report.artifact_sha256);
  assert.ok(report.loss_report?.can_serialize === true);
  assert.equal(report.loss_report?.counts?.blocking,0);
  if (target === "pdf") {
    assert.equal(report.loss_report?.schema_version,
      "chaptera.temporary-edited-fixed-pdf-loss.v1");
    assert.equal(report.loss_report?.current_editor_state,true);
    assert.equal(report.loss_report?.source_hash,fixtureHash);
    assert.ok(report.loss_report?.counts?.moved_nodes > 0,
      "PDF LossReport did not consume the canonical MoveNode history");
    assert.ok(observed.responses.some(r =>
      r.path.endsWith("/exports/pdf") && r.status === 200
      && r.mime === "application/pdf"),
      "real HTTPS response did not provide physical PDF MIME");
  }

  // Independent consumer: inspect the exact artifact saved by Playwright.
  // A ZIP/PDF signature, server hash or Reader Scene is not fidelity evidence.
  const rectKeys = ["x", "y", "width", "height"];
  assert.ok(rectKeys.every(key => Number.isSafeInteger(geometry[key])),
    "current Reader Scene produced non-integer edited geometry");
  assert.ok(geometry.width > 0 && geometry.height > 0,
    "current Reader Scene produced an empty edited rectangle");
  for (const key of rectKeys) {
    assert.equal(geometry[key], authoring[key],
      "current Scene and canonical authoring rectangle diverged");
  }
  // A PDF rectangle drawn at a source-existing position is not an edit proof.
  if (target === "pdf") {
    assert.ok(!sourceScene.nodes.some(n =>
      n.node_id !== selected.nodeId && n.bounds
      && rectKeys.every(key => n.bounds[key] === geometry[key])),
      "current PDF rectangle coincides with unrelated source geometry");
  }
  const verifier = spawnSync("python3", target === "pdf" ? [
    "tools/verify_temporary_guest_pdf_geometry.py",
    "--artifact", artifactPath,
    "--x-emu", String(geometry.x),
    "--y-emu", String(geometry.y),
    "--width-emu", String(geometry.width),
    "--height-emu", String(geometry.height),
    "--before-x-emu", String(originalBounds.x),
    "--before-y-emu", String(originalBounds.y),
  ] : [
    "tools/verify_editable_export_geometry.py",
    "--target", target,
    "--artifact", artifactPath,
    "--node-id", selected.nodeId,
    "--x-emu", String(geometry.x),
    "--y-emu", String(geometry.y),
    "--width-emu", String(geometry.width),
    "--height-emu", String(geometry.height),
  ], { encoding: "utf8", timeout: 60000, maxBuffer: 65536 });
  if (verifier.status !== 0) {
    // Never print temporary browser paths, source payloads or private XML.
    const stderr = verifier.stderr || "";
    const reason = /cross-reference pointer/.test(stderr)
      ? "pdf_xref_invalid"
      : /does not paint the accepted MoveNode/.test(stderr)
        ? "pdf_edited_geometry_missing"
        : /expected exactly one IDML TextFrame/.test(stderr)
          ? "idml_frame_identity_missing"
          : /IDML geometry mismatch/.test(stderr)
            ? "idml_frame_geometry_mismatch"
            : /BadZipFile|CRC failed|corrupt zip/i.test(stderr)
              ? "idml_package_corrupt" : target + "_consumer_error";
    throw Error("browser-downloaded " + target +
      " independent proof failed: " + reason);
  }
  let consumer;
  try {
    consumer = JSON.parse(verifier.stdout);
  } catch {
    throw Error("browser-downloaded " + target +
      " consumer did not emit a valid proof");
  }
  assert.equal(consumer.receipt_kind,target === "pdf"
    ? "chaptera.tempedit-guest-pdf-physical-geometry.v1"
    : "chaptera.local-editable-export-geometry-proof.v1");
  assert.equal(consumer.target, target);
  if (target === "idml") {
    assert.equal(consumer.node_id, selected.nodeId);
  } else {
    assert.ok(consumer.pages >= 1,"downloaded PDF has no reopened pages");
    assert.equal(consumer.artifact_byte_len,artifact.length);
    assert.equal(consumer.source_position_changed,true);
  }
  assert.equal(consumer.artifact_sha256, sha256(artifact));
  assert.deepEqual(consumer.expected_rect_emu, {
    x: geometry.x, y: geometry.y, width: geometry.width,
    height: geometry.height,
  });
  assert.equal(consumer.geometry_form,
    target === "pdf" ? "pdf_rectangle_stream" : "idml_path_geometry");
  assert.equal(consumer.geometry_matches_edit,true);

  // Reload is through the same guest page, original baseline response
  // followed by the exact changed canonical revision. No GET extends TTL.
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("#temporary-resume").click();
  try {
    await page.waitForFunction(() =>
      /(?:PUB открыт|Открыта редактируемая ревизия)/.test(
        document.querySelector("#temporary-status")?.textContent ?? ""), null,{timeout:170000});
  } catch {
    const state = await page.locator("#temporary-status").textContent();
    throw Error("physical browser resume failed with UI status: " +
      String(state).slice(0,180) + " http=" +
      JSON.stringify(observed.responses.slice(-18)) +
      " page_errors=" + JSON.stringify(browserErrors));
  }
  await settleResponses();
  const reopened = observed.scenes.at(-1);
  assert.equal(reopened.revision_id,accepted.revision_id);
  assert.equal(observed.current.at(-1).expires_at_ms, accepted.expires_at_ms);
  const reopenedNode = reopened.scene.nodes.find(n => n.node_id === selected.nodeId);
  assert.equal(reopenedNode?.bounds?.x,geometry.x);
  assert.equal(reopenedNode?.bounds?.y,geometry.y);
  assert.equal(sha256(await readFile(fixture)),fixtureHash,
    "guest editing modified original PUB on disk");

  // Caddy must not leak Product/auth surfaces on the guest origin.
  for (const restricted of [
    "/editor/product-editor-entry-v1.mjs",
    "/editor/chaptera-product-editor-service-v1.mjs",
    "/v1/editor/projects",
    "/v1/auth/session",
  ]) {
    const denied = await page.evaluate(async path => {
      const response = await fetch(path, {
        method: "GET", credentials: "omit", cache: "no-store"
      });
      return response.status;
    }, restricted);
    assert.equal(denied,404,
      "public edge exposed authenticated route " + restricted);
  }
  const privateMoves = observed.requests.filter(x =>
    x.path === privatePath("move-node", oldHead.document_id) && x.method === "POST");
  assert.equal(privateMoves.length,1,
    "pointer must cause exactly one canonical MoveNode POST");
  // No bearer may appear in any navigation or request URL. Read capability
  // privately inside browser process and never put it in the receipt or logs.
  const cap = await page.evaluate(() => {
    const item = sessionStorage.getItem("chaptera.temporary-edit.v0");
    return item ? JSON.parse(item).access_token : null;
  });
  assert.ok(typeof cap === "string" && /^[0-9a-f]{64}$/.test(cap));
  assert.ok(observed.requests.every(x=>!x.url.includes(cap)),
    "guest capability leaked into HTTP URL");
  assert.equal(browserErrors.length,0,"real guest browser emitted page errors");

  // Close the full real-PUB gesture/export/reload journey in the SAME browser
  // session by clicking the actual user Delete button. 204 is not simulated.
  page.once("dialog", dialog => dialog.accept());
  await page.locator("#temporary-delete").click();
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Сессия удалена с сервера"), null, {timeout:45000});
  const deletedBase = "/v1/editor/temporary-sessions/" + oldHead.document_id;
  assert.equal(observed.requests.filter(x =>
    x.path === deletedBase && x.method === "DELETE").length,1,
    "user deletion did not issue exactly one private DELETE");
  assert.ok(observed.responses.some(x =>
    x.path === "/v1/editor/temporary-sessions/{session}" && x.status === 204),
    "physical server did not acknowledge the guest DELETE with 204");
  const editedQuery = "?revision_id=" + encodeURIComponent(accepted.revision_id) +
    "&canonical_authoring_revision_id=" +
    encodeURIComponent(accepted.canonical_authoring_revision_id);
  const revokedPaths = [
    deletedBase + "/current", deletedBase + "/scene",
    deletedBase + "/exports/" + target + editedQuery,
  ];
  const revokedCodes = await page.evaluate(async ({paths,token}) => {
    const result=[];
    for(const path of paths){
      let status=429;
      for(let i=0;i<8;i++){
        const response=await fetch(path,{
          method:"GET",credentials:"omit",cache:"no-store",
          headers:{"x-chaptera-reader-session":token},
        });
        status=response.status;
        if(status!==429)break;
        await new Promise(done=>setTimeout(done,2200));
      }
      result.push(status);
    }
    return result;
  },{paths:revokedPaths,token:cap});
  assert.ok(revokedCodes.every(code=>code===404||code===410),
    "deleted browser bearer still accessed private current/scene/export");
  await page.reload({waitUntil:"domcontentloaded"});
  assert.equal(await page.evaluate(() =>
    sessionStorage.getItem("chaptera.temporary-edit.v0")),null,
    "guest tab kept the private bearer after physical deletion");
  assert.equal(await page.locator("#temporary-resume").isHidden(),true,
    "browser offered to resume a deleted guest session");

  const receipt = {
    schema:"chaptera.tempedit-real-pub-chromium-https.v1",
    status:"PASS",
    source_sha256:fixtureHash,
    source_byte_len:original.length,
    real_pub:true,real_chromium:true,
    deployment_mode:publicOrigin ? "public_trusted_origin" : "local_caddy",
    // A remote browser proves public TLS and behavior, not worker topology.
    tls_caddy:!publicOrigin,
    public_trusted_tls:publicOrigin,
    rust_server:!publicOrigin,
    isolated_worker:!publicOrigin,
    browser_pointer_commits:privateMoves.length,
    exact_source_bound_revision:true,
    browser_scene_repaint:true,
    revised_scene_on_reload:true,
    read_does_not_renew_ttl:true,
    export_target:target,
    ...(pristinePdfProof ? {
      pristine_pdf_without_edit_proven:true,
      pristine_pdf_sha256:pristinePdfProof.sha256,
      pristine_pdf_pages:pristinePdfProof.pageCount,
      pristine_pdf_unchanged_revision_proven:true,
      pristine_pdf_private_report_proven:true,
    } : {}),
    ["changed_" + target + "_download_sha256"]:sha256(artifact),
    ["changed_" + target + "_byte_len"]:artifact.length,
    ["changed_" + target + "_geometry_proven"]:true,
    ["changed_" + target + "_identity_proven"]:true,
    ["changed_" + target + "_geometry_form"]:consumer.geometry_form,
    same_browser_session_delete_proven:true,
    revoked_old_current_scene_export_denied:true,
    temporary_bearer_cleared_after_delete:true,
    loss_report_checked:true,
    original_pub_immutable:true,
    auth_product_routes_blocked:true,
    bearer_absent_from_urls:true,
    no_s3_or_account:!publicOrigin,
    v0_go:false,
  };
  await mkdir(dirname(receiptPath),{recursive:true});
  await writeFile(receiptPath,JSON.stringify(receipt,null,2)+"\n");
  console.log(JSON.stringify({schema:receipt.schema,status:receipt.status,
    real_pub:true,real_chromium:true,real_https:true,commit_count:1,
    target,
    changed_download_sha256:sha256(artifact),
    changed_download_geometry_proven:true,
    v0_go:false}));
} finally {
  await browser.close();
}
