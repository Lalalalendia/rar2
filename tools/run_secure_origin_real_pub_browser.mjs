#!/usr/bin/env node
// Executes the real HTTPS/OIDC and PUB ingest browser path. No mocked API.
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const fixture = process.env.CHAPTERA_SOURCE_PUB_FIXTURE;
const expectedHash = process.env.CHAPTERA_SOURCE_PUB_SHA256;
const receiptPath = process.env.CHAPTERA_SECURE_RECEIPT;
const storageStatePath = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
if (!origin || !fixture || !expectedHash || !receiptPath || !storageStatePath) {
  throw new Error("real source browser requires secure origin, fixture, SHA, receipt and storage-state paths");
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
  const initialPaint = await verifyEditorPaint(current, "initial open");

  const sourceNeutralBoundary = await page.evaluate(async id => {
    const forbiddenKeys = new Set([
      "source_bytes", "raw_source_bytes", "physical_upload_ref",
      "object_version", "object_etag", "durable_binding_id", "binding_id",
      "object_locator", "provider_key", "opaque_url", "presigned_url",
      "download_handle", "local_path", "filesystem_path",
    ]);
    const forbiddenStrings = ["SampleNewsletter.pub", "TruncatedNewsletter.pub"];
    const violations = [];
    const visit = (value, path) => {
      if (Array.isArray(value)) {
        value.forEach((item, index) => visit(item, path + "[" + index + "]"));
        return;
      }
      if (!value || typeof value !== "object") {
        if (typeof value === "string" &&
            forbiddenStrings.some(canary => value.includes(canary))) {
          violations.push(path + ":source-filename");
        }
        return;
      }
      for (const [key, nested] of Object.entries(value)) {
        const next = path ? path + "." + key : key;
        if (forbiddenKeys.has(key)) violations.push(next);
        visit(nested, next);
      }
    };
    const payloads = {};
    for (const [name, path] of [
      ["current", "/v1/documents/" + encodeURIComponent(id) + "/current"],
      ["scene", "/v1/reader/documents/" + encodeURIComponent(id) + "/scene"],
    ]) {
      const response = await fetch(path, {
        credentials: "same-origin",
        cache: "no-store",
      });
      if (!response.ok) throw new Error(name + " source-neutral probe HTTP " + response.status);
      payloads[name] = await response.json();
      visit(payloads[name], name);
    }
    return {
      violations,
      current_protocol: payloads.current?.protocol_version ?? null,
      scene_protocol: payloads.scene?.protocol_version ?? null,
    };
  }, documentId);
  if (sourceNeutralBoundary.violations.length ||
      sourceNeutralBoundary.current_protocol !== "chaptera.current-document.v1" ||
      sourceNeutralBoundary.scene_protocol !== "chaptera.reader-scene.v1") {
    throw new Error("browser payload crossed raw/provider source boundary: " +
      JSON.stringify(sourceNeutralBoundary));
  }

  // A returning user must be able to rediscover the just-created durable
  // project from the real Project Home without retaining the document URL.
  await page.goto(origin + "/editor", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(id => {
    const status = document.querySelector("#status");
    const card = document.querySelector(
      '[data-document-id="' + CSS.escape(id) + '"]'
    );
    return status?.textContent?.startsWith("Проектов:") && !!card;
  }, documentId, { timeout: 30000 });
  const homeReceipt = await page.evaluate(id => {
    const card = document.querySelector(
      '[data-document-id="' + CSS.escape(id) + '"]'
    );
    return {
      path: location.pathname,
      document_id: card?.dataset.documentId ?? null,
      project_id: card?.dataset.projectId ?? null,
      name: card?.querySelector("h2")?.textContent ?? null,
      status: document.querySelector("#status")?.textContent ?? null,
    };
  }, documentId);
  if (homeReceipt.path !== "/editor" || homeReceipt.document_id !== documentId ||
      !homeReceipt.project_id || homeReceipt.name !== "SampleNewsletter.pub") {
    // The authenticated Project Home may display the owner's uploaded filename,
    // but the retained security evidence must never copy that filename.
    throw new Error("Project Home did not preserve the authorized imported project label");
  }
  const safeHomeReceipt = {
    path: homeReceipt.path,
    document_id: homeReceipt.document_id,
    project_id: homeReceipt.project_id,
    uploaded_label_verified: true,
    status: homeReceipt.status,
  };
  // Real UI rename, not a direct API request. The result must be confirmed
  // by the exact server receipt, a fresh durable catalog and a browser reload.
  const renamedTitle = "Chaptera " + hash.slice(0, 12);
  const homeCard = page.locator(
    '[data-document-id="' + documentId.replace(/"/g, '\\"') + '"]'
  );
  await homeCard.locator('[data-action="rename-project"]').click();
  await homeCard.locator('[data-action="rename-input"]').fill(renamedTitle);
  const renameResponsePromise = page.waitForResponse(response => {
    if (response.request().method() !== "POST") return false;
    return decodeURIComponent(new URL(response.url()).pathname) ===
      "/v1/projects/" + homeReceipt.project_id + "/rename";
  }, { timeout: 30000 });
  await homeCard.locator('[data-action="save-rename"]').click();
  const renameResponse = await renameResponsePromise;
  if (renameResponse.status() !== 200) {
    // Emit only a bounded machine error code. Never log the project title or
    // any raw Product/SQLite diagnostics into retained CI browser logs.
    const denied = await renameResponse.json().catch(() => null);
    const code = typeof denied?.error === "string" &&
      /^[a-z0-9_:-]{1,96}$/.test(denied.error) ? denied.error : "unknown";
    throw new Error("real project rename HTTP failed: " + renameResponse.status() +
      " code=" + code);
  }
  if (renameResponse.headers()["cache-control"] !== "no-store") {
    throw new Error("project rename receipt response lacked private no-store policy");
  }
  const renamed = await renameResponse.json();
  if (renamed.protocol_version !== "chaptera.project-rename-receipt.v1" ||
      renamed.receipt?.project_id !== homeReceipt.project_id ||
      renamed.receipt?.name !== renamedTitle ||
      !Number.isSafeInteger(renamed.receipt?.metadata_version)) {
    throw new Error("project rename returned an unbound receipt");
  }
  await page.waitForFunction(({ id, name }) => {
    const card = document.querySelector(
      '[data-document-id="' + CSS.escape(id) + '"]'
    );
    return card?.querySelector("h2")?.textContent === name &&
      document.querySelector("#status")?.textContent?.startsWith("Проектов:");
  }, { id: documentId, name: renamedTitle }, { timeout: 30000 });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(({ id, name }) => {
    const card = document.querySelector(
      '[data-document-id="' + CSS.escape(id) + '"]'
    );
    return card?.querySelector("h2")?.textContent === name &&
      document.querySelector("#status")?.textContent?.startsWith("Проектов:");
  }, { id: documentId, name: renamedTitle }, { timeout: 30000 });

  const homeOpen = page.locator(
    '[data-document-id="' + documentId.replace(/"/g, '\\"') + '"] [data-action="open-project"]'
  );
  await Promise.all([
    page.waitForURL(url =>
      decodeURIComponent(url.pathname) === "/editor/doc/" + documentId,
    { timeout: 30000, waitUntil: "domcontentloaded" }),
    homeOpen.click(),
  ]);
  const homeOpened = await currentDocumentReceipt(documentId);
  if (homeOpened.status !== 200 || homeOpened.document_id !== documentId ||
      homeOpened.revision_id !== current.revision_id) {
    throw new Error("Project Home reopened a different durable revision: " +
      JSON.stringify({ homeOpened, expected: current.revision_id }));
  }
  const homeOpenedPaint = await verifyEditorPaint(homeOpened, "project home reopen");

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
  const reopenedPaint = await verifyEditorPaint(reopened, "page reload");

  // Drive the actual editor shell through physical pointer events. The target
  // must be direct, identity-transformed, geometrically unique at the hit
  // point, and the topmost DOM visual. No fabricated MoveNode POST is used.
  const moveTarget = await chooseRealMoveTarget(documentId);
  if (!moveTarget || moveTarget.revision_id !== reopened.revision_id) {
    throw new Error("real imported PUB exposes no stable direct MoveNode target");
  }
  // Hold the real MoveNode HTTP request at the network boundary. A user may
  // press Undo before an ACK; the browser must not send a stale history edge.
  const moveCommitPath = "/v1/documents/" + documentId + "/commit";
  const historyPath = "/v1/documents/" + documentId + "/history";
  const accidentalHistoryPosts = [];
  page.on("request", request => {
    if (request.method() === "POST" &&
        decodeURIComponent(new URL(request.url()).pathname) === historyPath) {
      accidentalHistoryPosts.push(1);
    }
  });
  let releaseMoveCommit;
  let commitIntercepted;
  const commitEnteredGate = new Promise(resolve => { commitIntercepted = resolve; });
  const commitGate = new Promise(resolve => { releaseMoveCommit = resolve; });
  let interceptedCommits = 0;
  const holdCommit = async route => {
    if (route.request().method() === "POST" &&
        decodeURIComponent(new URL(route.request().url()).pathname) === moveCommitPath) {
      interceptedCommits += 1;
      commitIntercepted();
      await commitGate;
    }
    await route.continue();
  };
  await page.route("**/v1/documents/*/commit", holdCommit);
  const commitResponsePromise = page.waitForResponse(response => {
    if (response.request().method() !== "POST") return false;
    const path = decodeURIComponent(new URL(response.url()).pathname);
    return path === "/v1/documents/" + documentId + "/commit";
  }, { timeout: 30000 });
  await page.mouse.move(moveTarget.start.x, moveTarget.start.y);
  await page.mouse.down({ button: "left" });
  await page.mouse.move(moveTarget.end.x, moveTarget.end.y, { steps: 4 });
  await page.mouse.up({ button: "left" });
  await commitEnteredGate;
  let blockedHistoryDuringSave = false;
  try {
    await page.waitForFunction(() => {
      const status = document.querySelector("#status");
      const undo = document.querySelector("#undo-action");
      const redo = document.querySelector("#redo-action");
      return status?.classList.contains("pending") &&
        undo?.disabled === true && redo?.disabled === true;
    }, null, { timeout: 15000 });
    await page.keyboard.press("Control+z");
    const held = await page.evaluate(() => ({
      status: document.querySelector("#status")?.textContent ?? null,
      pending: document.querySelector("#status")?.classList.contains("pending") ?? false,
      undo_disabled: document.querySelector("#undo-action")?.disabled ?? false,
      redo_disabled: document.querySelector("#redo-action")?.disabled ?? false,
    }));
    blockedHistoryDuringSave = held.pending &&
      held.status?.includes("Save still pending") &&
      held.undo_disabled && held.redo_disabled &&
      accidentalHistoryPosts.length === 0 && interceptedCommits === 1;
    if (!blockedHistoryDuringSave) {
      throw new Error("pending MoveNode allowed a stale Undo or falsely reported durable history");
    }
  } finally {
    releaseMoveCommit();
  }
  const commitResponse = await commitResponsePromise;
  await page.unroute("**/v1/documents/*/commit", holdCommit);
  const accepted = await commitResponse.json().catch(() => null);
  if (commitResponse.status() !== 200 ||
      accepted?.protocol_version !== "chaptera.commit-accepted.v1" ||
      accepted?.document_id !== documentId ||
      accepted?.replayed !== false ||
      !accepted?.revision_id ||
      accepted.revision_id === reopened.revision_id) {
    throw new Error("real browser MoveNode did not receive a fresh durable revision ACK: " +
      JSON.stringify({ status: commitResponse.status(), accepted, moveTarget }));
  }

  if (!blockedHistoryDuringSave || accidentalHistoryPosts.length !== 0) {
    throw new Error("pending MoveNode sent history before authoritative save reconciliation");
  }
  await page.waitForFunction(() =>
    document.querySelector("#undo-action")?.disabled === false &&
    document.querySelector("#redo-action")?.disabled === false, null,
    { timeout: 30000 });
  const movedCurrent = await currentDocumentReceipt(documentId);
  if (movedCurrent.status !== 200 || movedCurrent.document_id !== documentId ||
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

  await page.reload({ waitUntil: "domcontentloaded" });
  const edited = await currentDocumentReceipt(documentId);
  if (edited.status !== 200 || edited.document_id !== documentId ||
      edited.revision_id !== accepted.revision_id) {
    throw new Error("physical MoveNode revision did not remain current after browser reload: " +
      JSON.stringify({ edited, accepted }));
  }
  const editedPaint = await verifyEditorPaint(edited, "after pointer MoveNode reload");
  const moveCommit = {
    node_id: moveTarget.node_id,
    before: {
      x_emu: moveTarget.before_bounds.x,
      y_emu: moveTarget.before_bounds.y,
    },
    after: {
      x_emu: movedNode.bounds.x,
      y_emu: movedNode.bounds.y,
    },
    base_revision_id: reopened.revision_id,
    revision_id: accepted.revision_id,
    client_operation_id: null,
    attempts: [{ node_id: moveTarget.node_id, status: commitResponse.status(), code: null }],
  };

  // Physical hostile-source control: a truncated copy of the real Publisher
  // fixture must traverse the authenticated upload path, fail validation, and
  // never create a second Project/genesis.
  await page.goto(origin + "/editor", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.startsWith("Проектов:"),
  null, { timeout: 30000 });
  const projectsBeforeMalformed = await page.evaluate(() =>
    [...document.querySelectorAll("[data-document-id]")]
      .map(card => card.getAttribute("data-document-id"))
      .filter(Boolean)
      .sort());
  const createCallsBeforeMalformed = calls.filter(call =>
    call.method === "POST" && call.path === "/v1/projects/from-upload").length;

  const oversizedAdmission = await page.evaluate(async () => {
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin",
      cache: "no-store",
    });
    const session = await sessionResponse.json().catch(() => null);
    if (!sessionResponse.ok || !session?.csrf_token) {
      throw new Error("oversized admission probe could not obtain CSRF");
    }
    const workspaceResponse = await fetch("/v1/workspaces/personal", {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": session.csrf_token,
      },
      body: "{}",
    });
    const workspace = await workspaceResponse.json().catch(() => null);
    if (!workspaceResponse.ok || !workspace?.workspace_id) {
      throw new Error("oversized admission probe could not resolve workspace");
    }
    const response = await fetch("/v1/uploads", {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": session.csrf_token,
      },
      body: JSON.stringify({
        workspace_id: workspace.workspace_id,
        purpose: "pub_source",
        expected_byte_len: 1099511627776,
        declared_content_type: "application/x-mspublisher",
        idempotency_key: "b02-oversized-upload-admission",
      }),
    });
    const body = await response.json().catch(() => null);
    return {
      status: response.status,
      error: body?.error ?? null,
      upload_disclosed: !!body?.upload,
      transport_disclosed: !!body?.transport,
    };
  });
  if (oversizedAdmission.status !== 413 ||
      oversizedAdmission.error !== "upload_size_rejected" ||
      oversizedAdmission.upload_disclosed ||
      oversizedAdmission.transport_disclosed) {
    throw new Error("oversized upload was not rejected before object admission: " +
      JSON.stringify(oversizedAdmission));
  }

  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
  null, { timeout: 30000 });
  const truncated = raw.subarray(0, Math.min(raw.length, 4096));
  if (truncated.length >= raw.length) {
    throw new Error("real Publisher fixture is too small for truncated hostile control");
  }
  await page.locator("#pub-file").setInputFiles({
    name: "TruncatedNewsletter.pub",
    mimeType: "application/x-mspublisher",
    buffer: truncated,
  });
  await page.waitForFunction(() => {
    const code = document.querySelector("#error")?.textContent ?? "";
    return code.length > 0;
  }, null, { timeout: 120000 });
  const malformedUi = await page.evaluate(() => ({
    path: location.pathname,
    status: document.querySelector("#status")?.textContent ?? "",
    error_code: document.querySelector("#error")?.textContent ?? "",
  }));
  if (malformedUi.path !== "/editor/new" ||
      malformedUi.status !== "Needs attention" ||
      !/^[a-z0-9_:-]{1,96}$/.test(malformedUi.error_code)) {
    throw new Error("truncated PUB did not fail closed at file-entry boundary: " +
      JSON.stringify(malformedUi));
  }
  const createCallsAfterMalformed = calls.filter(call =>
    call.method === "POST" && call.path === "/v1/projects/from-upload").length;
  if (createCallsAfterMalformed !== createCallsBeforeMalformed) {
    throw new Error("truncated PUB reached durable Project creation");
  }

  await page.goto(origin + "/editor", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.startsWith("Проектов:"),
  null, { timeout: 30000 });
  const projectsAfterMalformed = await page.evaluate(() =>
    [...document.querySelectorAll("[data-document-id]")]
      .map(card => card.getAttribute("data-document-id"))
      .filter(Boolean)
      .sort());
  if (JSON.stringify(projectsAfterMalformed) !== JSON.stringify(projectsBeforeMalformed)) {
    throw new Error("truncated PUB changed durable Project catalog: " +
      JSON.stringify({ projectsBeforeMalformed, projectsAfterMalformed }));
  }
  const malformedSource = {
    claim: true,
    byte_len: truncated.length,
    error_code: malformedUi.error_code,
    project_create_calls_delta:
      createCallsAfterMalformed - createCallsBeforeMalformed,
    catalog_unchanged: true,
    raw_parser_diagnostic_disclosed: false,
  };

  // The truncated fixture above proved one real CFB rejection. Expand that
  // existing B02 browser path into deterministic hostile-header controls:
  // each nonempty source must physically reach authenticated ingress, fail
  // before Product genesis, and leave the original project/scene intact.
  const corruptCopy = (offset, bytes) => {
    const altered = Buffer.from(raw);
    for (let index = 0; index < bytes.length; index += 1) {
      altered[offset + index] = bytes[index];
    }
    return altered;
  };
  const hostileCfbFixtures = [
    { kind: "magic_only", buffer: Buffer.from(raw.subarray(0, 8)) },
    { kind: "header_prefix_128", buffer: Buffer.from(raw.subarray(0, 128)) },
    { kind: "header_prefix_511", buffer: Buffer.from(raw.subarray(0, 511)) },
    { kind: "invalid_signature", buffer: corruptCopy(0, [0x00]) },
    { kind: "invalid_byte_order", buffer: corruptCopy(28, [0x00, 0x00]) },
    { kind: "invalid_sector_shift", buffer: corruptCopy(30, [0x00, 0x00]) },
    { kind: "missing_directory_sector", buffer: corruptCopy(48, [0xfe, 0xff, 0xff, 0xff]) },
  ];
  const sourceAdmissionCount = () => calls.filter(call =>
    call.method === "POST" && call.path === "/v1/uploads").length;
  const sourceContentCount = () => calls.filter(call =>
    call.method === "PUT" && call.path.endsWith("/content")).length;
  const durableProjectCreateCount = () => calls.filter(call =>
    call.method === "POST" && call.path === "/v1/projects/from-upload").length;
  const hostileCfbResults = [];
  for (const hostile of hostileCfbFixtures) {
    if (hostile.buffer.length === 0 || hostile.buffer.length >= raw.length + 1) {
      throw new Error("hostile CFB fixture generator violated size bounds");
    }
    await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() =>
      document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
    null, { timeout: 30000 });
    const admissionBefore = sourceAdmissionCount();
    const contentBefore = sourceContentCount();
    const durableBefore = durableProjectCreateCount();
    await page.locator("#pub-file").setInputFiles({
      name: "TruncatedNewsletter.pub",
      mimeType: "application/x-mspublisher",
      buffer: hostile.buffer,
    });
    await page.waitForFunction(() =>
      (document.querySelector("#error")?.textContent ?? "").length > 0,
    null, { timeout: 45000 });
    const failed = await page.evaluate(() => ({
      path: location.pathname,
      status: document.querySelector("#status")?.textContent ?? "",
      code: document.querySelector("#error")?.textContent ?? "",
    }));
    if (failed.path !== "/editor/new" || failed.status !== "Needs attention" ||
        !/^[a-z0-9_:-]{1,96}$/.test(failed.code)) {
      throw new Error("hostile CFB did not fail with a bounded typed UI error: " + hostile.kind);
    }
    if (sourceAdmissionCount() !== admissionBefore + 1 ||
        sourceContentCount() !== contentBefore + 1 ||
        durableProjectCreateCount() !== durableBefore) {
      throw new Error("hostile CFB did not traverse exact ingress or reached Project genesis: " +
        hostile.kind);
    }
    hostileCfbResults.push({
      kind: hostile.kind,
      byte_len: hostile.buffer.length,
      error_code: failed.code,
      passed: true,
    });
  }
  await page.goto(origin + "/editor", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.startsWith("Проектов:"),
  null, { timeout: 30000 });
  const finalProjects = await page.evaluate(() =>
    [...document.querySelectorAll("[data-document-id]")]
      .map(card => card.getAttribute("data-document-id")).filter(Boolean).sort());
  if (JSON.stringify(finalProjects) !== JSON.stringify(projectsBeforeMalformed)) {
    throw new Error("hostile CFB attempts changed durable Project catalog");
  }
  const afterHostile = await currentDocumentReceipt(documentId);
  if (afterHostile.status !== 200 || afterHostile.revision_id !== edited.revision_id) {
    throw new Error("hostile CFB attempts damaged already accepted canonical revision");
  }
  const hostileCfbMatrix = {
    claim: true,
    tested_count: hostileCfbResults.length + 1,
    additional_cases: hostileCfbResults,
    same_project_catalog: true,
    original_revision_unchanged: true,
  };


  // ACK-unknown negative, physically independent of the already-accepted
  // original document. A second authenticated import of the SAME pinned real
  // PUB ensures the downstream restart/export receipts remain untouched.
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
  null, { timeout: 30000 });
  await page.locator("#pub-file").setInputFiles({
    name: "AckLossControl.pub",
    mimeType: "application/x-mspublisher",
    buffer: raw,
  });
  await page.waitForURL(url =>
    /^\/editor\/doc\/document:[0-9a-f]{24}$/.test(decodeURIComponent(url.pathname)),
  { timeout: 105000, waitUntil: "domcontentloaded" });
  const ackLossDoc = decodeURIComponent(new URL(page.url()).pathname)
    .replace("/editor/doc/", "");
  if (!ackLossDoc || ackLossDoc === documentId) {
    throw new Error("lost-ACK probe did not create an independent physical PUB project");
  }
  const ackLossBefore = await currentDocumentReceipt(ackLossDoc);
  if (ackLossBefore.status !== 200 || !ackLossBefore.revision_id) {
    throw new Error("lost-ACK control PUB lacks an authoritative baseline revision");
  }
  await verifyEditorPaint(ackLossBefore, "independent ACK-loss control open");
  const ackLossTarget = await chooseRealMoveTarget(ackLossDoc);
  if (!ackLossTarget || ackLossTarget.revision_id !== ackLossBefore.revision_id) {
    throw new Error("lost-ACK control PUB lacks a stable physical pointer target");
  }
  const ackLossCommitPath = "/v1/documents/" + ackLossDoc + "/commit";
  const ackLossHistoryPath = "/v1/documents/" + ackLossDoc + "/history";
  const countRequests = (method, path) => calls.filter(call =>
    call.method === method && decodeURIComponent(call.path) === path).length;
  const initialHistory = countRequests("POST", ackLossHistoryPath);
  const initialCommits = countRequests("POST", ackLossCommitPath);
  let authoritativeAck = null;
  let interceptedAckCount = 0;
  const dropCommittedAck = async route => {
    if (route.request().method() !== "POST" ||
        decodeURIComponent(new URL(route.request().url()).pathname) !== ackLossCommitPath) {
      await route.continue();
      return;
    }
    interceptedAckCount += 1;
    // Chromium maps edge.test -> loopback, but Playwright's Node-side
    // route.fetch does NOT inherit Chromium's host-resolver rules.
    // Preserve the original Caddy virtual host while connecting via the
    // published local HTTPS endpoint. Never print the original request or
    // upstream transport exception: it contains auth cookies and CSRF.
    const original = new URL(route.request().url());
    const upstreamAddress = new URL(original);
    upstreamAddress.hostname = "127.0.0.1";
    let upstream;
    try {
      upstream = await route.fetch({
        url: upstreamAddress.toString(),
        headers: { ...route.request().headers(), host: original.host },
        timeout: 45000,
      });
    } catch {
      await route.abort("failed").catch(() => {});
      throw new Error("ACK-loss proxy failed before service acceptance (redacted)");
    }
    const result = await upstream.json().catch(() => null);
    if (upstream.status() !== 200 ||
        result?.protocol_version !== "chaptera.commit-accepted.v1" ||
        result.document_id !== ackLossDoc ||
        result.base_revision_id !== ackLossBefore.revision_id ||
        !result.revision_id ||
        result.revision_id === ackLossBefore.revision_id ||
        !result.client_operation_id) {
      throw new Error("ACK-loss control did not first obtain a real durable MoveNode 200");
    }
    authoritativeAck = {
      revision_id: result.revision_id,
      operation_id: result.client_operation_id,
      status: upstream.status(),
    };
    // The server has fully committed and returned 200 to our proxy. The
    // browser receives NO response, exactly the ACK-unknown transport case.
    await route.abort("failed");
  };
  await page.route("**/v1/documents/*/commit", dropCommittedAck);
  await page.mouse.move(ackLossTarget.start.x, ackLossTarget.start.y);
  await page.mouse.down({ button: "left" });
  await page.mouse.move(ackLossTarget.end.x, ackLossTarget.end.y, { steps: 4 });
  await page.mouse.up({ button: "left" });
  await page.waitForFunction(() => {
    const status = document.querySelector("#status");
    const retry = document.querySelector("#reconcile-action");
    return status?.textContent?.includes("Save outcome unknown") &&
      status?.classList.contains("error") &&
      document.querySelector("#undo-action")?.disabled === true &&
      document.querySelector("#redo-action")?.disabled === true &&
      retry?.hidden === false && retry?.disabled === false;
  }, null, { timeout: 30000 });
  await page.unroute("**/v1/documents/*/commit", dropCommittedAck);
  if (!authoritativeAck || interceptedAckCount !== 1 ||
      countRequests("POST", ackLossCommitPath) !== initialCommits + 1) {
    throw new Error("ACK-loss test did not observe exactly one server-accepted MoveNode");
  }

  // Neither keyboard history nor a second pointer gesture may escape the
  // uncertain-save fence or silently use the stale pre-commit revision.
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+Shift+z");
  await page.keyboard.press("Control+y");
  await page.mouse.move(ackLossTarget.start.x, ackLossTarget.start.y);
  await page.mouse.down({ button: "left" });
  await page.mouse.move(ackLossTarget.end.x, ackLossTarget.end.y, { steps: 4 });
  await page.mouse.up({ button: "left" });
  if (countRequests("POST", ackLossHistoryPath) !== initialHistory ||
      countRequests("POST", ackLossCommitPath) !== initialCommits + 1) {
    throw new Error("unconfirmed save emitted a stale history or duplicate MoveNode POST");
  }

  // The first explicitly requested GET /current fails: no stale success,
  // no unlock and no mutation, even though the server has durably advanced.
  let abortedCurrentReads = 0;
  const denyCurrent = async route => {
    if (decodeURIComponent(new URL(route.request().url()).pathname) ===
        "/v1/documents/" + ackLossDoc + "/current") {
      abortedCurrentReads += 1;
      await route.abort("failed");
    } else {
      await route.continue();
    }
  };
  await page.route("**/v1/documents/*/current", denyCurrent);
  const failingRecheck = page.waitForRequest(request =>
    request.method() === "GET" &&
    decodeURIComponent(new URL(request.url()).pathname) ===
      "/v1/documents/" + ackLossDoc + "/current",
  { timeout: 30000 });
  await page.locator("#reconcile-action").click();
  await failingRecheck;
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Save outcome unknown") &&
    document.querySelector("#reconcile-action")?.disabled === false &&
    document.querySelector("#undo-action")?.disabled === true &&
    document.querySelector("#redo-action")?.disabled === true,
  null, { timeout: 30000 });
  await page.unroute("**/v1/documents/*/current", denyCurrent);
  if (abortedCurrentReads < 1 ||
      countRequests("POST", ackLossHistoryPath) !== initialHistory) {
    throw new Error("failed authoritative recheck unlocked or mutated an unknown save");
  }

  // Only a fresh authenticated matching /current AND Reader Scene with a
  // different revision can reconcile. The UI deliberately does not claim
  // that the lost original HTTP ACK was observed.
  await page.locator("#reconcile-action").click();
  await page.waitForFunction(() => {
    const status = document.querySelector("#status");
    return status?.textContent?.includes("Server revision synchronized") &&
      document.querySelector("#reconcile-action")?.hidden === true &&
      document.querySelector("#undo-action")?.disabled === false &&
      document.querySelector("#redo-action")?.disabled === false;
  }, null, { timeout: 30000 });
  const ackLossCurrent = await currentDocumentReceipt(ackLossDoc);
  const ackLossNode = await readerNodeReceipt(ackLossDoc, ackLossTarget.node_id);
  if (ackLossCurrent.status !== 200 ||
      ackLossCurrent.revision_id !== authoritativeAck.revision_id ||
      ackLossNode.revision_id !== authoritativeAck.revision_id ||
      !ackLossNode.bounds ||
      (ackLossNode.bounds.x === ackLossTarget.before_bounds.x &&
       ackLossNode.bounds.y === ackLossTarget.before_bounds.y) ||
      countRequests("POST", ackLossHistoryPath) !== initialHistory) {
    throw new Error("real lost-ACK reconciliation did not recover exact server revision/geometry");
  }
  const lostCommitAckEvidence = {
    claim: true,
    independent_real_pub: true,
    server_accepted_before_browser_ack_loss: true,
    accepted_commit_status: authoritativeAck.status,
    original_operation_id_preserved: true,
    history_mutations_while_unknown: 0,
    duplicate_move_mutations_while_unknown: 0,
    failed_refetch_kept_editing_locked: true,
    explicit_current_and_reader_reconciled: true,
    fabricated_original_ack: false,
  };


  // Undo/Redo ACK-unknown physical controls reuse the independent real PUB
  // already mutated by the accepted MoveNode above. The server performs each
  // durable history mutation, while Playwright suppresses ONLY the HTTP ACK.
  // No synthetic history POST or injected editor state is allowed.
  const lostHistoryAckEvidence = [];
  for (const kind of ["undo", "redo"]) {
    const beforeHistory = await currentDocumentReceipt(ackLossDoc);
    const beforeHistoryNode = await readerNodeReceipt(ackLossDoc, ackLossTarget.node_id);
    const historyPostsBefore = countRequests("POST", ackLossHistoryPath);
    const movePostsBefore = countRequests("POST", ackLossCommitPath);
    if (beforeHistory.status !== 200 ||
        beforeHistory.revision_id !== beforeHistoryNode.revision_id) {
      throw new Error(kind + " ACK-loss baseline was not a source-exact current revision");
    }

    let acceptedHistory = null;
    let proxiedHistoryPosts = 0;
    const dropDurableHistoryAck = async route => {
      if (route.request().method() !== "POST" ||
          decodeURIComponent(new URL(route.request().url()).pathname) !== ackLossHistoryPath) {
        await route.continue();
        return;
      }
      proxiedHistoryPosts += 1;
      const request = route.request().postDataJSON();
      if (request?.command?.kind !== kind ||
          request.document_id !== ackLossDoc ||
          request.source_hash !== hash ||
          request.base_revision_id !== beforeHistory.revision_id ||
          !request.client_operation_id) {
        throw new Error(kind + " ACK-loss test did not send the expected canonical intent");
      }
      // Node-side route.fetch does not inherit Chromium's edge.test resolver.
      // Preserve Caddy's virtual host and the authenticated original headers.
      const original = new URL(route.request().url());
      const upstreamAddress = new URL(original);
      upstreamAddress.hostname = "127.0.0.1";
      let upstream;
      try {
        upstream = await route.fetch({
          url: upstreamAddress.toString(),
          headers: { ...route.request().headers(), host: original.host },
          timeout: 45000,
        });
      } catch {
        await route.abort("failed").catch(() => {});
        throw new Error(kind + " ACK-loss proxy failed before authority acceptance (redacted)");
      }
      const result = await upstream.json().catch(() => null);
      if (upstream.status() !== 200 ||
          result?.protocol_version !== "chaptera.history-transition-accepted.v1" ||
          result.document_id !== ackLossDoc ||
          result.source_hash !== hash ||
          result.base_revision_id !== beforeHistory.revision_id ||
          result.transition_kind !== kind ||
          result.client_operation_id !== request.client_operation_id ||
          !result.revision_id ||
          result.revision_id === beforeHistory.revision_id) {
        throw new Error(kind + " ACK-loss control did not observe one real durable 200");
      }
      acceptedHistory = {
        revision_id: result.revision_id,
        operation_id: result.client_operation_id,
      };
      // HTTP 200 was returned to the proxy, never to the browser.
      await route.abort("failed");
    };
    await page.route("**/v1/documents/*/history", dropDurableHistoryAck);
    await page.locator("#" + kind + "-action").click();
    await page.waitForFunction(() => {
      const status = document.querySelector("#status");
      const recheck = document.querySelector("#reconcile-action");
      return status?.textContent?.includes("Undo/Redo outcome unknown") &&
        status.classList.contains("error") &&
        document.querySelector("#undo-action")?.disabled === true &&
        document.querySelector("#redo-action")?.disabled === true &&
        recheck?.hidden === false && recheck?.disabled === false;
    }, null, { timeout: 30000 });
    await page.unroute("**/v1/documents/*/history", dropDurableHistoryAck);
    if (!acceptedHistory || proxiedHistoryPosts !== 1 ||
        countRequests("POST", ackLossHistoryPath) !== historyPostsBefore + 1) {
      throw new Error(kind + " ACK-loss test produced missing/duplicate durable history");
    }

    await page.keyboard.press("Control+z");
    await page.keyboard.press("Control+Shift+z");
    await page.keyboard.press("Control+y");
    const blockedTarget = await chooseRealMoveTarget(ackLossDoc);
    if (blockedTarget) {
      await page.mouse.move(blockedTarget.start.x, blockedTarget.start.y);
      await page.mouse.down({ button: "left" });
      await page.mouse.move(blockedTarget.end.x, blockedTarget.end.y, { steps: 4 });
      await page.mouse.up({ button: "left" });
    }
    if (countRequests("POST", ackLossHistoryPath) !== historyPostsBefore + 1 ||
        countRequests("POST", ackLossCommitPath) !== movePostsBefore) {
      throw new Error(kind + " unknown outcome leaked another mutation POST");
    }

    // An unavailable /current cannot release the history-unknown fence.
    let failedCurrentRechecks = 0;
    const denyHistoryCurrent = async route => {
      if (decodeURIComponent(new URL(route.request().url()).pathname) ===
          "/v1/documents/" + ackLossDoc + "/current") {
        failedCurrentRechecks += 1;
        await route.abort("failed");
      } else {
        await route.continue();
      }
    };
    await page.route("**/v1/documents/*/current", denyHistoryCurrent);
    const attemptedRecheck = page.waitForRequest(request =>
      request.method() === "GET" &&
      decodeURIComponent(new URL(request.url()).pathname) ===
        "/v1/documents/" + ackLossDoc + "/current",
    { timeout: 30000 });
    await page.locator("#reconcile-action").click();
    await attemptedRecheck;
    await page.waitForFunction(() =>
      document.querySelector("#status")?.textContent?.includes("Undo/Redo outcome unknown") &&
      document.querySelector("#reconcile-action")?.disabled === false &&
      document.querySelector("#undo-action")?.disabled === true &&
      document.querySelector("#redo-action")?.disabled === true,
    null, { timeout: 30000 });
    await page.unroute("**/v1/documents/*/current", denyHistoryCurrent);
    if (!failedCurrentRechecks ||
        countRequests("POST", ackLossHistoryPath) !== historyPostsBefore + 1) {
      throw new Error(kind + " failed recheck was not fail-closed");
    }

    await page.locator("#reconcile-action").click();
    await page.waitForFunction(() => {
      const status = document.querySelector("#status");
      return status?.textContent?.includes("Server history synchronized") &&
        document.querySelector("#reconcile-action")?.hidden === true &&
        document.querySelector("#undo-action")?.disabled === false &&
        document.querySelector("#redo-action")?.disabled === false;
    }, null, { timeout: 30000 });
    const recoveredCurrent = await currentDocumentReceipt(ackLossDoc);
    const recoveredNode = await readerNodeReceipt(ackLossDoc, ackLossTarget.node_id);
    const expectedBounds = kind === "undo"
      ? ackLossTarget.before_bounds : ackLossNode.bounds;
    if (recoveredCurrent.status !== 200 ||
        recoveredCurrent.revision_id !== acceptedHistory.revision_id ||
        recoveredNode.revision_id !== acceptedHistory.revision_id ||
        recoveredNode.bounds?.x !== expectedBounds.x ||
        recoveredNode.bounds?.y !== expectedBounds.y ||
        (recoveredNode.bounds?.x === beforeHistoryNode.bounds?.x &&
         recoveredNode.bounds?.y === beforeHistoryNode.bounds?.y) ||
        countRequests("POST", ackLossHistoryPath) !== historyPostsBefore + 1 ||
        countRequests("POST", ackLossCommitPath) !== movePostsBefore) {
      throw new Error(kind + " lost ACK did not recover the exact new history geometry");
    }
    lostHistoryAckEvidence.push({
      kind,
      genuine_server_accepted_200_before_browser_ack_loss: true,
      exact_client_operation_preserved: true,
      no_duplicate_history_or_move_posts: true,
      failed_recheck_kept_mutations_locked: true,
      exact_current_and_reader_reconciled: true,
      original_ack_claim: false,
    });
  }


  // Selection by a physical click is transient view state: it cannot create
  // a revision or consume the Undo slot. Arrow key nudge performs the actual
  // single authenticated MoveNode against the same canonical source.
  const keyboardTarget = await chooseRealMoveTarget(ackLossDoc);
  const keyboardBase = await currentDocumentReceipt(ackLossDoc);
  if (!keyboardTarget || keyboardTarget.revision_id !== keyboardBase.revision_id) {
    throw new Error("real PUB has no admissible canonical keyboard MoveNode target");
  }
  const selectCommitsBefore = countRequests("POST", ackLossCommitPath);
  await page.mouse.move(keyboardTarget.start.x, keyboardTarget.start.y);
  await page.mouse.down({ button: "left" });
  await page.mouse.up({ button: "left" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("selected"),
  null, { timeout: 30000 });
  const afterPointerSelection = await currentDocumentReceipt(ackLossDoc);
  if (afterPointerSelection.status !== 200 ||
      afterPointerSelection.revision_id !== keyboardBase.revision_id ||
      countRequests("POST", ackLossCommitPath) !== selectCommitsBefore) {
    throw new Error("physical canvas selection must not write a blank revision");
  }
  if (await page.evaluate(() => document.activeElement?.id) !== "canvas") {
    throw new Error("physical canvas selection did not acquire keyboard focus");
  }

  let keyboardBefore = await readerNodeReceipt(ackLossDoc, keyboardTarget.node_id);
  if (keyboardBefore.revision_id !== keyboardBase.revision_id || !keyboardBefore.bounds) {
    throw new Error("selected canonical node did not match current keyboard baseline");
  }
  const commitsBeforeKeyboard = countRequests("POST", ackLossCommitPath);
  const firstKeyboardResponsePending = page.waitForResponse(response =>
    response.request().method() === "POST" &&
    decodeURIComponent(new URL(response.url()).pathname) === ackLossCommitPath,
  { timeout: 30000 });
  await page.keyboard.press("ArrowLeft");
  const firstKeyboardResponse = await firstKeyboardResponsePending;
  const firstKeyboardRequest = firstKeyboardResponse.request().postDataJSON();
  const firstKeyboardAck = await firstKeyboardResponse.json().catch(() => null);
  if (firstKeyboardResponse.status() !== 200 ||
      firstKeyboardRequest?.command?.kind !== "move_node_to" ||
      firstKeyboardRequest.command.node_id !== keyboardTarget.node_id ||
      firstKeyboardRequest.command.x_emu !== keyboardBefore.bounds.x - 12700 ||
      firstKeyboardRequest.command.y_emu !== keyboardBefore.bounds.y ||
      firstKeyboardRequest.base_revision_id !== keyboardBase.revision_id ||
      firstKeyboardRequest.source_hash !== hash ||
      firstKeyboardAck?.protocol_version !== "chaptera.commit-accepted.v1" ||
      firstKeyboardAck.revision_id === keyboardBase.revision_id) {
    throw new Error("one ArrowLeft failed exact 1pt authenticated MoveNode binding");
  }
  await page.waitForFunction(revision =>
    document.querySelector("#status")?.textContent?.includes(revision),
  firstKeyboardAck.revision_id, { timeout: 30000 });
  const afterFirstArrow = await readerNodeReceipt(ackLossDoc, keyboardTarget.node_id);
  if (afterFirstArrow.revision_id !== firstKeyboardAck.revision_id ||
      afterFirstArrow.bounds?.x !== keyboardBefore.bounds.x - 12700 ||
      afterFirstArrow.bounds?.y !== keyboardBefore.bounds.y ||
      countRequests("POST", ackLossCommitPath) !== commitsBeforeKeyboard + 1) {
    throw new Error("arrow nudge did not durably project exact 12,700 EMU geometry");
  }

  const pageMetrics = await page.evaluate(async ({ docId, pageId }) => {
    const result = await fetch("/v1/reader/documents/" + encodeURIComponent(docId) + "/scene", {
      credentials: "same-origin", cache: "no-store",
    });
    if (!result.ok) throw new Error("cannot read canonical page bounds for nudge");
    const scene = await result.json();
    const page = scene.pages?.find(candidate => candidate.page_id === pageId);
    return { width: page?.width_emu ?? null, height: page?.height_emu ?? null };
  }, { docId: ackLossDoc, pageId: keyboardTarget.page_id });
  const b = afterFirstArrow.bounds;
  const SHIFT = 127000;
  const candidateShifts = [
    { key: "Shift+ArrowLeft", dx: -SHIFT, dy: 0, fits: b.x >= SHIFT },
    { key: "Shift+ArrowRight", dx: SHIFT, dy: 0,
      fits: Number.isSafeInteger(pageMetrics.width) && b.x + b.width + SHIFT <= pageMetrics.width },
    { key: "Shift+ArrowUp", dx: 0, dy: -SHIFT, fits: b.y >= SHIFT },
    { key: "Shift+ArrowDown", dx: 0, dy: SHIFT,
      fits: Number.isSafeInteger(pageMetrics.height) && b.y + b.height + SHIFT <= pageMetrics.height },
  ];
  const shift = candidateShifts.find(option => option.fits);
  if (!shift) throw new Error("real PUB selected page-local node has no safe 10pt nudge direction");

  const shiftResponsePending = page.waitForResponse(response =>
    response.request().method() === "POST" &&
    decodeURIComponent(new URL(response.url()).pathname) === ackLossCommitPath,
  { timeout: 30000 });
  await page.keyboard.press(shift.key);
  const shiftResponse = await shiftResponsePending;
  const shiftRequest = shiftResponse.request().postDataJSON();
  const shiftAck = await shiftResponse.json().catch(() => null);
  if (shiftResponse.status() !== 200 ||
      shiftRequest?.command?.kind !== "move_node_to" ||
      shiftRequest.command.node_id !== keyboardTarget.node_id ||
      shiftRequest.command.x_emu !== b.x + shift.dx ||
      shiftRequest.command.y_emu !== b.y + shift.dy ||
      shiftRequest.base_revision_id !== firstKeyboardAck.revision_id ||
      shiftRequest.source_hash !== hash ||
      shiftAck?.protocol_version !== "chaptera.commit-accepted.v1" ||
      shiftAck.revision_id === firstKeyboardAck.revision_id) {
    throw new Error("Shift+Arrow failed exact 10pt canonical MoveNode semantics");
  }
  await page.waitForFunction(revision =>
    document.querySelector("#status")?.textContent?.includes(revision),
  shiftAck.revision_id, { timeout: 30000 });
  const afterShift = await readerNodeReceipt(ackLossDoc, keyboardTarget.node_id);
  const afterShiftCurrent = await currentDocumentReceipt(ackLossDoc);
  if (afterShift.revision_id !== shiftAck.revision_id ||
      afterShiftCurrent.revision_id !== shiftAck.revision_id ||
      afterShift.bounds?.x !== b.x + shift.dx ||
      afterShift.bounds?.y !== b.y + shift.dy ||
      countRequests("POST", ackLossCommitPath) !== commitsBeforeKeyboard + 2) {
    throw new Error("Shift+Arrow revision/geometry drift or duplicate keyboard commit");
  }

  // The browser-native keyboard belongs to focused buttons and forms, not
  // the global editor selection. Arrow keys on the Undo button do not write.
  await page.locator("#undo-action").focus();
  await page.keyboard.press("ArrowRight");
  if (countRequests("POST", ackLossCommitPath) !== commitsBeforeKeyboard + 2) {
    throw new Error("toolbar focus leaked an unwanted arrow MoveNode");
  }
  const keyboardNudgeEvidence = {
    claim: true,
    physical_pointer_selection_and_canvas_focus: true,
    selection_writes_no_revision: true,
    authenticated_real_pub: true,
    1pt_exact_emu_move: true,
    shift_10pt_exact_emu_move: true,
    separate_operation_and_revision_per_keypress: true,
    canonical_reader_geometry_reconciled: true,
    toolbar_keyboard_isolation: true,
    no_duplicate_keyboard_commit: true,
  };

  if (process.env.CHAPTERA_SECURE_SCREENSHOT) {
    await page.screenshot({ path: process.env.CHAPTERA_SECURE_SCREENSHOT, fullPage: true });
  }
  await context.storageState({ path: storageStatePath });

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
    throw new Error("browser never issued the canonical MoveNode through UI");
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
    reader_scene_reached_browser_paint: true,
    real_browser_pointer_move_node: true,
    browser_source_neutral_boundary_claim: true,
    browser_source_neutral_boundary: sourceNeutralBoundary,
    oversized_upload_admission_claim: true,
    oversized_upload_admission: oversizedAdmission,
    initial_editor_paint: initialPaint,
    project_home_catalog_claim: true,
    project_rename_claim: true,
    project_rename: { project_id: homeReceipt.project_id, metadata_version: renamed.receipt.metadata_version, catalog_verified_after_reload: true },
    project_home: safeHomeReceipt,
    project_home_editor_paint: homeOpenedPaint,
    reloaded_editor_paint: reopenedPaint,
    moved_editor_paint: movedPaint,
    edited_editor_paint: editedPaint,
    pending_move_history_fence_claim: true,
    lost_commit_ack_fence_claim: true,
    lost_commit_ack_fence: lostCommitAckEvidence,
    lost_history_ack_fence_claim: true,
    lost_history_ack_fence: lostHistoryAckEvidence,
    keyboard_nudge_claim: true,
    keyboard_nudge: keyboardNudgeEvidence,
    real_move_commit: true,
    move_commit: moveCommit,
    malformed_source_rejection_claim: true,
    malformed_source_rejection: malformedSource,
    hostile_cfb_matrix_claim: true,
    hostile_cfb_matrix: hostileCfbMatrix,
    server_restart_reopen_claim: false,
    storage_provider: "filesystem",
    s3_claim: false,
    native_pub_write_claim: false,
    move_and_export_claim: false,
    document_id: documentId,
    imported_revision_id: current.revision_id,
    current_revision_id: edited.revision_id,
    session_cookie_secure_http_only: true,
    steps: calls.map(call => call.method + " " + call.path),
    head_sha: process.env.CHAPTERA_HEAD_SHA ?? "unknown",
  };
  // Keep customer/source-derived labels out of persisted acceptance telemetry.
  // The authenticated owner can see the title, but CI receipts must not echo it.
  const retained = JSON.stringify(receipt);
  if (retained.includes("SampleNewsletter.pub") ||
      retained.includes("TruncatedNewsletter.pub")) {
    throw new Error("source filename leaked into acceptance telemetry");
  }
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt) + "\n");
} finally {
  await browser.close();
}
