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

async function verifyEditorPaint(current, stage) {
  // A successful /current read and document URL can coexist with failed
  // shell boot. Require the real Reader projection to reach the page DOM.
  const scene = await page.evaluate(async id => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin", cache: "no-store",
    });
    if (!response.ok) throw new Error("Reader scene HTTP " + response.status);
    return response.json();
  }, current.document_id);
  if (scene.protocol_version !== "chaptera.reader-scene.v1" ||
      scene.document_id !== current.document_id ||
      scene.revision_id !== current.revision_id || scene.source_hash !== hash ||
      !scene.pages?.length || !scene.nodes?.length || !scene.fidelity?.state) {
    throw new Error(stage + ": Reader scene lacks exact imported identity or content");
  }
  await page.waitForFunction(revision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") && status.textContent.includes(revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, current.revision_id, { timeout: 30000 });
  const paint = await page.evaluate(() => ({
    document_id: document.querySelector("#document")?.textContent,
    fidelity_label: document.querySelector("#fidelity")?.textContent,
    pages: [...document.querySelectorAll("#canvas svg.page[data-page-id]")].map(svg => ({
      page_id: svg.getAttribute("data-page-id"),
      visible: svg.getBoundingClientRect().width > 0 && svg.getBoundingClientRect().height > 0,
      node_ids: [...svg.querySelectorAll("g[data-node-id]")].map(node => node.getAttribute("data-node-id")),
    })),
    resolved_text_lines: document.querySelectorAll('#canvas [data-text-authority="server-shared-resolved"]').length,
    preview_text_blocks: document.querySelectorAll('#canvas [data-text-authority="browser-preview-only"]').length,
    images: document.querySelectorAll("#canvas image[data-resource-id]").length,
    missing_image_placeholders: document.querySelectorAll("#canvas [data-resource-missing]").length,
    table_cells: document.querySelectorAll("#canvas [data-table-row][data-table-column]").length,
  }));
  const expectedPages = [...scene.pages].sort((a, b) => a.order - b.order);
  if (paint.document_id !== current.document_id || paint.pages.length !== expectedPages.length) {
    throw new Error(stage + ": editor document/page paint differs from Reader scene");
  }
  for (const [index, expected] of expectedPages.entries()) {
    const actual = paint.pages[index];
    const nodeIds = scene.nodes.filter(node => node.page_id === expected.page_id &&
      node.bounds.width > 0 && node.bounds.height > 0).map(node => node.node_id).sort();
    if (!actual.visible || actual.page_id !== expected.page_id ||
        JSON.stringify([...actual.node_ids].sort()) !== JSON.stringify(nodeIds)) {
      throw new Error(stage + ": missing, stale or unordered Reader page/node paint");
    }
  }
  const reasons = scene.fidelity.reasons ?? [];
  const expectedFidelity = "Fidelity: " + scene.fidelity.state +
    (reasons.length ? " — " + reasons.join(", ") : "");
  if (paint.fidelity_label !== expectedFidelity || pageErrors.length) {
    throw new Error(stage + ": hidden fidelity disclosure or shell JavaScript failure: " +
      JSON.stringify({ paint, pageErrors }));
  }
  return { ...paint, revision_id: current.revision_id, publisher_visual_equivalence_claim: false };
}

async function currentDocumentReceipt(documentId) {
  return page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin",
      cache: "no-store",
    });
    const data = await response.json().catch(() => null);
    return {
      status: response.status,
      document_id: data?.document_id ?? null,
      revision_id: data?.revision_id ?? null,
      protocol: data?.protocol_version ?? null,
      error: data?.error ?? null,
    };
  }, documentId);
}

async function chooseRealMoveTarget(documentId) {
  return page.evaluate(async id => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin",
      cache: "no-store",
    });
    if (!response.ok) throw new Error("Reader scene HTTP " + response.status);
    const scene = await response.json();
    const identityTransform = transform => !transform ||
      (Number(transform.a) === 1 && Number(transform.b) === 0 &&
       Number(transform.c) === 0 && Number(transform.d) === 1 &&
       transform.tx === 0 && transform.ty === 0);
    const direct = (scene.nodes ?? []).filter(node =>
      (node.origin_node_id === undefined || node.origin_node_id === null) &&
      (node.parent_node_id === undefined || node.parent_node_id === null) &&
      node.bounds?.width > 0 && node.bounds?.height > 0 &&
      identityTransform(node.transform));
    const fractions = [
      [0.5, 0.5], [0.25, 0.25], [0.75, 0.25],
      [0.25, 0.75], [0.75, 0.75],
    ];
    const contains = (node, x, y) => x >= node.bounds.x && y >= node.bounds.y &&
      x <= node.bounds.x + node.bounds.width &&
      y <= node.bounds.y + node.bounds.height;
    for (const candidate of direct) {
      const visualGroup = [...document.querySelectorAll("#canvas g[data-node-id]")]
        .find(node => node.getAttribute("data-node-id") === candidate.node_id);
      if (!visualGroup) continue;
      visualGroup.scrollIntoView({ block: "center", inline: "center" });
      await new Promise(resolve =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const svg = [...document.querySelectorAll("#canvas svg.page[data-page-id]")]
        .find(page => page.getAttribute("data-page-id") === candidate.page_id);
      if (!svg?.getScreenCTM()) continue;
      for (const [fx, fy] of fractions) {
        const localX = Math.round(candidate.bounds.x + candidate.bounds.width * fx);
        const localY = Math.round(candidate.bounds.y + candidate.bounds.height * fy);
        const hits = direct.filter(node =>
          node.page_id === candidate.page_id && contains(node, localX, localY));
        if (hits.length !== 1 || hits[0].node_id !== candidate.node_id) continue;
        const point = svg.createSVGPoint();
        point.x = localX;
        point.y = localY;
        const screen = point.matrixTransform(svg.getScreenCTM());
        const visual = document.elementFromPoint(screen.x, screen.y)
          ?.closest?.("[data-node-id]");
        if (visual?.getAttribute("data-node-id") !== candidate.node_id) continue;

        // Four CSS-ish pixels at natural scale. The pointer path is converted
        // through the same SVG matrix the real editor uses, not fabricated
        // directly as a canonical MoveNode request.
        const endPoint = svg.createSVGPoint();
        endPoint.x = localX + 38100;
        endPoint.y = localY + 38100;
        const endScreen = endPoint.matrixTransform(svg.getScreenCTM());
        return {
          revision_id: scene.revision_id,
          node_id: candidate.node_id,
          page_id: candidate.page_id,
          before_bounds: candidate.bounds,
          start: { x: screen.x, y: screen.y },
          end: { x: endScreen.x, y: endScreen.y },
        };
      }
    }
    return null;
  }, documentId);
}

async function readerNodeReceipt(documentId, nodeId) {
  return page.evaluate(async ({ id, nodeId }) => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin",
      cache: "no-store",
    });
    if (!response.ok) throw new Error("Reader scene HTTP " + response.status);
    const scene = await response.json();
    const node = (scene.nodes ?? []).find(candidate => candidate.node_id === nodeId);
    return {
      revision_id: scene.revision_id,
      bounds: node?.bounds ?? null,
    };
  }, { id: documentId, nodeId });
}

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
  const current = await currentDocumentReceipt(documentId);
  if (current.status !== 200 || current.document_id !== documentId ||
      !current.revision_id) {
    throw new Error("real imported PUB did not materialize in Editor: " +
      JSON.stringify({ current, calls: calls.slice(-8), failedResponses }));
  }
  const initialPaint = await verifyEditorPaint(current, "initial open");

  // Drive the actual product shell with physical pointer events. Choose an
  // untransformed, direct page-local node at a point that is geometrically
  // unique and is the topmost visual DOM target. This mirrors the server's
  // bounded MoveNode admission instead of posting a fabricated command.
  const moveTarget = await chooseRealMoveTarget(documentId);
  if (!moveTarget || moveTarget.revision_id !== current.revision_id) {
    throw new Error("real imported PUB exposes no stable direct MoveNode target");
  }
  const commitResponsePromise = page.waitForResponse(response => {
    if (response.request().method() !== "POST") return false;
    const path = decodeURIComponent(new URL(response.url()).pathname);
    return path === "/v1/documents/" + documentId + "/commit";
  }, { timeout: 30000 });
  await page.mouse.move(moveTarget.start.x, moveTarget.start.y);
  await page.mouse.down({ button: "left" });
  await page.mouse.move(moveTarget.end.x, moveTarget.end.y, { steps: 4 });
  await page.mouse.up({ button: "left" });
  const commitResponse = await commitResponsePromise;
  const accepted = await commitResponse.json().catch(() => null);
  if (commitResponse.status() !== 200 ||
      accepted?.protocol_version !== "chaptera.commit-accepted.v1" ||
      accepted?.document_id !== documentId ||
      accepted?.replayed !== false ||
      !accepted?.revision_id ||
      accepted.revision_id === current.revision_id) {
    throw new Error("real browser MoveNode did not receive a fresh durable revision ACK: " +
      JSON.stringify({ status: commitResponse.status(), accepted, moveTarget }));
  }

  const movedCurrent = await currentDocumentReceipt(documentId);
  if (movedCurrent.status !== 200 ||
      movedCurrent.revision_id !== accepted.revision_id) {
    throw new Error("accepted browser MoveNode is not canonical current state: " +
      JSON.stringify({ movedCurrent, accepted }));
  }
  const movedNode = await readerNodeReceipt(documentId, moveTarget.node_id);
  if (movedNode.revision_id !== movedCurrent.revision_id || !movedNode.bounds ||
      (movedNode.bounds.x === moveTarget.before_bounds.x &&
       movedNode.bounds.y === moveTarget.before_bounds.y)) {
    throw new Error("Reader scene did not project the durable browser MoveNode: " +
      JSON.stringify({ moveTarget, movedNode, movedCurrent }));
  }
  const movedPaint = await verifyEditorPaint(movedCurrent, "after real pointer MoveNode");

  // An actual page reload (no synthetic product snapshot) must retain the
  // authoritative edited service revision. Server-process restart is a later gate.
  await page.reload({ waitUntil: "domcontentloaded" });
  const reopened = await currentDocumentReceipt(documentId);
  if (reopened.status !== 200 || reopened.document_id !== documentId ||
      reopened.revision_id !== movedCurrent.revision_id) {
    throw new Error("durable browser MoveNode changed after page reload: " +
      JSON.stringify({ reopened, expected_revision: movedCurrent.revision_id }));
  }
  const reopenedPaint = await verifyEditorPaint(reopened, "page reload");
  if (process.env.CHAPTERA_SECURE_SCREENSHOT) {
    await page.screenshot({ path: process.env.CHAPTERA_SECURE_SCREENSHOT, fullPage: true });
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
  if (!calls.some(call => call.method === "POST" &&
      decodeURIComponent(call.path) === "/v1/documents/" + documentId + "/commit")) {
    throw new Error("browser never issued the real canonical MoveNode commit");
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
    real_browser_pointer_move_node: true,
    reader_scene_reached_browser_paint: true,
    initial_editor_paint: initialPaint,
    moved_editor_paint: movedPaint,
    reloaded_editor_paint: reopenedPaint,
    moved_node_id: moveTarget.node_id,
    moved_node_before_bounds: moveTarget.before_bounds,
    moved_node_after_bounds: movedNode.bounds,
    baseline_revision_id: current.revision_id,
    edited_revision_id: movedCurrent.revision_id,
    server_restart_reopen_claim: false,
    storage_provider: "filesystem",
    s3_claim: false,
    native_pub_write_claim: false,
    move_and_export_claim: false,
    document_id: documentId,
    current_revision_id: movedCurrent.revision_id,
    session_cookie_secure_http_only: true,
    steps: calls.map(call => call.method + " " + call.path),
    head_sha: process.env.CHAPTERA_HEAD_SHA ?? "unknown",
  };
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt) + "\n");
} finally {
  await browser.close();
}
