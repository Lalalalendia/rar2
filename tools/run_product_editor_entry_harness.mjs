import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const DOC = "10000000-0000-7000-8000-000000000001";
const PAGE = "20000000-0000-7000-8000-000000000001";
const DIRECT = "30000000-0000-7000-8000-000000000001";
const PROJECTED = "sha256:" + "d".repeat(64);
const SOURCE = "a".repeat(64);
const BASE = "sha256:" + "b".repeat(64);
const CHILD = "sha256:" + "c".repeat(64);
const CSRF = "synthetic-csrf-product-editor";
const root = resolve(".");
const mime = {
  ".html": "text/html; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
};
const csp = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'";

function scene(revisionId, x = 95250, y = 95250) {
  return {
    protocol_version: "chaptera.reader-scene.v1",
    document_id: DOC,
    source_hash: SOURCE,
    revision_id: revisionId,
    scene_authority: "synthetic_product_api_witness",
    stacking_fidelity: "partial",
    fidelity: { state: "partial", reasons: ["synthetic_product_api_witness"] },
    pages: [{ page_id: PAGE, order: 0, width_emu: 952500, height_emu: 952500 }],
    nodes: [
      {
        node_id: DIRECT,
        page_id: PAGE,
        kind: "shape",
        bounds: { x, y, width: 190500, height: 190500 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
        paint: { fill_rgb: [10, 120, 200], line: null },
      },
      {
        node_id: PROJECTED,
        origin_node_id: "30000000-0000-7000-8000-000000000002",
        page_id: PAGE,
        kind: "shape",
        bounds: { x: 571500, y: 95250, width: 190500, height: 190500 },
        transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
        paint: { fill_rgb: [180, 70, 20], line: null },
      },
    ],
    stories: [],
    resources: [],
    fonts: [],
    diagnostics: [],
  };
}

let current = scene(BASE);
const commitRequests = [];
const sessionCookies = [];
const traceClasses = [];
const staticFiles = new Map([
  ["/editor/product-editor.css", ["apps/web/product-editor.css", ".css"]],
  ["/editor/product-editor-entry-v1.mjs", ["apps/web/product-editor-entry-v1.mjs", ".mjs"]],
  ["/editor/chaptera-product-editor-service-v1.mjs", ["apps/web/chaptera-product-editor-service-v1.mjs", ".mjs"]],
  ["/editor/observability-v1.mjs", ["apps/web/observability-v1.mjs", ".mjs"]],
  ["/editor/rich-reader-editor-shell-v1.mjs", ["apps/web/rich-reader-editor-shell-v1.mjs", ".mjs"]],
  ["/editor/reader-scene-editor-interaction-v1.mjs", ["apps/web/reader-scene-editor-interaction-v1.mjs", ".mjs"]],
  ["/editor/reader-scene-editor-adapter-v1.mjs", ["apps/web/reader-scene-editor-adapter-v1.mjs", ".mjs"]],
  ["/editor/interaction-v1.mjs", ["apps/web/interaction-v1.mjs", ".mjs"]],
  ["/cloud-reader/render-v1.mjs", ["apps/cloud-reader/render-v1.mjs", ".mjs"]],
]);

function json(res, code, value) {
  res.writeHead(code, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" });
  res.end(JSON.stringify(value));
}
function isAuthenticated(req) {
  sessionCookies.push(String(req.headers.cookie ?? ""));
  return /(?:^|;\s*)chaptera-test-auth=valid(?:;|$)/.test(req.headers.cookie ?? "");
}

const server = createServer(async (req, res) => {
  try {
    const uri = new URL(req.url, "http://127.0.0.1");
    const path = uri.pathname;
    if (path === "/v1/auth/login") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end("<!doctype html><title>Test identity handoff</title><p>Login required</p>");
      return;
    }
    if (path.startsWith("/v1/")) {
      if (req.headers["x-chaptera-operation-class"]) {
        traceClasses.push(String(req.headers["x-chaptera-operation-class"]));
      }
      if (!isAuthenticated(req)) {
        json(res, 401, { error: "authentication_required" });
        return;
      }
      if (path === "/v1/session" && req.method === "GET") {
        json(res, 200, {
          principal_id: "principal:synthetic-product-test",
          csrf_token: CSRF,
          idle_expires_at_ms: Date.now() + 120000,
          absolute_expires_at_ms: Date.now() + 600000,
        });
        return;
      }
      if (path === "/v1/documents/" + DOC + "/current" && req.method === "GET") {
        json(res, 200, {
          protocol_version: "chaptera.current-document.v1",
          document_id: DOC,
          source_hash: SOURCE,
          revision_id: current.revision_id,
        });
        return;
      }
      if (path === "/v1/reader/documents/" + DOC + "/scene" && req.method === "GET") {
        json(res, 200, current);
        return;
      }
      if (path === "/v1/documents/" + DOC + "/commit" && req.method === "POST") {
        if (req.headers["x-csrf-token"] !== CSRF) {
          json(res, 403, { error: "csrf_invalid" });
          return;
        }
        let payload = "";
        for await (const part of req) {
          payload += part.toString("utf8");
          if (payload.length > 32768) throw new Error("oversized commit");
        }
        const request = JSON.parse(payload);
        if (request.protocol_version !== "chaptera.commit-request.v1" ||
            request.document_id !== DOC || request.source_hash !== SOURCE ||
            request.base_revision_id !== BASE || request.command?.node_id !== DIRECT ||
            request.command?.kind !== "move_node_to" ||
            current.revision_id !== BASE) {
          json(res, 409, { error: "stale_revision" });
          return;
        }
        commitRequests.push(request);
        current = scene(CHILD, request.command.x_emu, request.command.y_emu);
        json(res, 200, {
          protocol_version: "chaptera.commit-accepted.v1",
          document_id: DOC,
          source_hash: SOURCE,
          base_revision_id: BASE,
          revision_id: CHILD,
          state_id: "sha256:" + "e".repeat(64),
          client_operation_id: request.client_operation_id,
          canonical_operation: {
            kind: "move_node",
            node_id: DIRECT,
            before: { x: 95250, y: 95250, width: 190500, height: 190500 },
            after: {
              x: request.command.x_emu, y: request.command.y_emu,
              width: 190500, height: 190500,
            },
          },
          project_schema_version: "pub-editor-v0.11",
          canonical_revision_schema_version: "chaptera.cdm.authoring-revision.v1",
          canonical_authoring_revision_id: "f".repeat(64),
          replayed: false,
          scene_refresh: "full_snapshot",
        });
        return;
      }
      json(res, 404, { error: "unsupported_synthetic_endpoint" });
      return;
    }
    const asset = path === "/editor/doc/" + DOC
      ? ["apps/web/product-editor.html", ".html"]
      : staticFiles.get(path);
    if (!asset) {
      res.writeHead(404, { "content-type": "text/plain" });
      res.end("not found");
      return;
    }
    const bytes = await readFile(resolve(root, asset[0]));
    res.writeHead(200, {
      "content-type": mime[asset[1]],
      "content-security-policy": csp,
      "x-content-type-options": "nosniff",
      "cache-control": "no-store",
    });
    res.end(bytes);
  } catch (error) {
    res.writeHead(500, { "content-type": "text/plain" });
    res.end("synthetic harness error: " + String(error));
  }
});
await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
const origin = "http://127.0.0.1:" + server.address().port;
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 1080, height: 690 } });
  await context.addCookies([{
    name: "chaptera-test-auth", value: "valid", url: origin,
    httpOnly: true, sameSite: "Lax",
  }]);
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(err.message));
  await page.goto(origin + "/editor/doc/" + DOC);
  await page.locator('[data-node-id="' + DIRECT + '"]').waitFor();
  await page.waitForFunction((base) =>
    document.querySelector("#status")?.textContent?.includes(base), BASE);

  const projected = page.locator('[data-node-id="' + PROJECTED + '"]');
  const direct = page.locator('[data-node-id="' + DIRECT + '"]');
  const projectedBox = await projected.boundingBox();
  if (!projectedBox) throw new Error("projected visual missing");
  await page.mouse.click(projectedBox.x + projectedBox.width / 2, projectedBox.y + projectedBox.height / 2);
  if (commitRequests.length !== 0) throw new Error("projected visual edited");

  const directBox = await direct.boundingBox();
  if (!directBox) throw new Error("direct visual missing");
  const sx = directBox.x + directBox.width / 2;
  const sy = directBox.y + directBox.height / 2;
  await page.mouse.move(sx, sy);
  await page.mouse.down();
  await page.mouse.move(sx + 15, sy + 10, { steps: 5 });
  await page.mouse.up();
  await page.waitForFunction((child) =>
    document.querySelector("#status")?.textContent?.includes(child), CHILD);
  if (pageErrors.length) throw new Error("product page JavaScript error: " + pageErrors.join("; "));
  if (commitRequests.length !== 1) throw new Error("expected one canonical Product API MoveNode");
  const request = commitRequests[0];
  const editedNode = await page.locator('[data-node-id="' + DIRECT + '"] rect').first();
  const x = await editedNode.getAttribute("x");
  const y = await editedNode.getAttribute("y");
  if (x !== String(request.command.x_emu) || y !== String(request.command.y_emu)) {
    throw new Error("child Reader pixels do not match committed geometry");
  }
  const overlay = await page.locator('[data-layer="editor-transient-overlay"]').count();
  if (!overlay) throw new Error("accepted child lost the selection overlay");
  if (current.source_hash !== SOURCE) throw new Error("source identity changed");
  for (const op of ["open", "scene_read", "commit"]) {
    if (!traceClasses.includes(op)) throw new Error("missing browser trace operation: " + op);
  }
  if (!sessionCookies.length || sessionCookies.some((value) => !value.includes("chaptera-test-auth=valid"))) {
    throw new Error("browser failed cookie-backed Product API request");
  }
  const fidelity = await page.locator("#fidelity").textContent();
  if (!fidelity.includes("partial")) throw new Error("reader fidelity disclosure missing");

  const guest = await browser.newPage();
  await guest.goto(origin + "/editor/doc/" + DOC + "?from=product-smoke");
  await guest.waitForURL((url) => url.pathname === "/v1/auth/login");
  const requestedReturn = new URL(guest.url()).searchParams.get("return_path");
  if (requestedReturn !== "/editor/doc/" + DOC + "?from=product-smoke") {
    throw new Error("unauthenticated browser did not request safe login return path");
  }
  if (commitRequests.length !== 1) throw new Error("unauthenticated page committed an operation");
  const receipt = {
    schema_version: "chaptera.product-editor-entry-synthetic-browser.v1",
    status: "PASS",
    authority_scope: "synthetic same-origin HTTP Product API fixture, not real OIDC or live SourceIngress",
    exact_document_uuid_v7: DOC,
    source_hash: SOURCE,
    base_revision_id: BASE,
    child_revision_id: CHILD,
    browser_cookie_request: true,
    browser_csrf_commit: true,
    browser_observability_classes: [...new Set(traceClasses)].sort(),
    projected_visual_read_only: true,
    canonical_commit_count: 1,
    child_reader_scene_repaint: true,
    selection_overlay_restored: true,
    fidelity_disclosure_visible: true,
    missing_session_login_redirect: true,
  };
  await mkdir("target/web-editor-product", { recursive: true });
  await writeFile(
    "target/web-editor-product/chromium-product-editor-entry-smoke.json",
    JSON.stringify(receipt, null, 2) + "\n",
  );
  process.stdout.write(JSON.stringify(receipt, null, 2) + "\n");
} finally {
  await browser.close();
  await new Promise((close) => server.close(close));
}
