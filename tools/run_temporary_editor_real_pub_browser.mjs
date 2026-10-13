// Physical guest V0 acceptance: real Chromium -> TLS/Caddy -> Rust -> isolated
// Publisher parser/replay -> browser MoveNode -> exact Scene -> edited IDML.
// No synthetic fetch, manual POST mutation, Product auth, or persistent project.
import assert from "node:assert/strict";
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
const fixture = resolve(arg("fixture"));
const fixtureHash = arg("fixture-sha256");
const receiptPath = resolve(arg("out"));
if (!/^https:\/\/reader\.chaptera\.test:\d+$/.test(origin)
    || !/^[0-9a-f]{64}$/.test(fixtureHash)) {
  throw Error("physical guest origin or pinned fixture invalid");
}
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
const original = await readFile(fixture);
assert.equal(sha256(original), fixtureHash, "pinned real source hash mismatch");
assert.ok(original.length > 100000, "not a representative pinned PUB");

const browser = await chromium.launch({
  headless: true, args: [
    "--no-proxy-server",
    "--host-resolver-rules=MAP reader.chaptera.test 127.0.0.1",
  ],
});
const page = await browser.newPage({
  acceptDownloads: true,
  ignoreHTTPSErrors: true,
  viewport: { width: 1280, height: 900 },
});
const observed = { current: [], scenes: [], accepted: [], requests: [], reports: [] };
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
  if (!response.url().startsWith(origin) || response.status() !== 200) return;
  const path = new URL(response.url()).pathname;
  let dest;
  if (/\/v1\/editor\/temporary-sessions\/[^/]+\/current$/.test(path)) {
    dest = observed.current;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) {
    dest = observed.scenes;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/move-node$/.test(path)) {
    dest = observed.accepted;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/exports\/(?:idml|odg)$/.test(path)
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
function candidatePoints(scene, current) {
  const shapes = scene.nodes.filter(n =>
    direct(n) && current.authoring_graph?.nodes?.[n.node_id]);
  const choices = [];
  for (const node of shapes) {
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
  choices.sort((a,b) => {
    // Shapes with real paint are more likely to be visually selectable.
    const ka = a.kind === "shape" ? 0 : 1, kb = b.kind === "shape" ? 0 : 1;
    return ka - kb || a.area - b.area;
  });
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
  assert.equal(result?.status(), 200, "public Caddy did not expose guest editor");
  await page.locator("#temporary-file").setInputFiles(fixture);
  await page.locator("#temporary-start").click();
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Открыта редактируемая ревизия"), null, { timeout: 90000 });
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

  const candidates = candidatePoints(sourceScene, oldHead);
  assert.ok(candidates.length, "real PUB has no unambiguous direct editable candidate");
  let selected = null;
  for (const choice of candidates.slice(0, 120)) {
    const screen = await hitOnScreen(choice);
    if (screen) {
      selected = { ...choice, screen };
      break;
    }
  }
  assert.ok(selected, "no actual browser-visible canonical node is selectable");
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
      String(state).slice(0,180));
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

  const target = "idml";
  await page.locator("#temporary-download-" + target).waitFor({state:"visible"});
  const downloadPromise = page.waitForEvent("download", { timeout: 90000 });
  await page.locator("#temporary-download-" + target).click();
  const download = await downloadPromise;
  await settleResponses();
  assert.equal(download.suggestedFilename(),"chaptera-edited.idml");
  const artifact = await readFile(await download.path());
  assert.ok(artifact.subarray(0,2).equals(Buffer.from("PK")),
    "changed IDML must be a real ZIP package");
  const report = observed.reports.at(-1);
  assert.equal(report?.revision_id, accepted.revision_id);
  assert.equal(report?.source_hash, fixtureHash);
  assert.equal(report?.target, target);
  assert.equal(sha256(artifact), report.artifact_sha256);
  assert.ok(report.loss_report?.can_serialize === true);
  assert.equal(report.loss_report?.counts?.blocking,0);

  // Reload is through the same guest page, original baseline response
  // followed by the exact changed canonical revision. No GET extends TTL.
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("#temporary-resume").click();
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Открыта редактируемая ревизия"), null,{timeout:90000});
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

  const receipt = {
    schema:"chaptera.tempedit-real-pub-chromium-https.v1",
    status:"PASS",
    source_sha256:fixtureHash,
    source_byte_len:original.length,
    real_pub:true,real_chromium:true,
    tls_caddy:true,rust_server:true,isolated_worker:true,
    browser_pointer_commits:privateMoves.length,
    exact_source_bound_revision:true,
    browser_scene_repaint:true,
    revised_scene_on_reload:true,
    read_does_not_renew_ttl:true,
    changed_idml_download_sha256:sha256(artifact),
    changed_idml_byte_len:artifact.length,
    loss_report_checked:true,
    original_pub_immutable:true,
    auth_product_routes_blocked:true,
    bearer_absent_from_urls:true,
    no_s3_or_account:true,
    v0_go:false,
  };
  await mkdir(dirname(receiptPath),{recursive:true});
  await writeFile(receiptPath,JSON.stringify(receipt,null,2)+"\n");
  console.log(JSON.stringify({schema:receipt.schema,status:receipt.status,
    real_pub:true,real_chromium:true,real_https:true,commit_count:1,
    changed_idml_sha256:receipt.changed_idml_download_sha256,
    v0_go:false}));
} finally {
  await browser.close();
}
