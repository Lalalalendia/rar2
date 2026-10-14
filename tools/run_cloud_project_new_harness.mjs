// Chromium product UI smoke using synthetic backend replies. This is NOT proof
// of actual OIDC, S3 persistence, PUB validation, or Rust source materialization.
import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const DOC = "document:" + "c".repeat(24);
const UPLOAD = "upload:" + "a".repeat(32);
const CSRF = "synthetic-cloud-project-csrf";
const root = resolve(".");
const assets = new Map([
  ["/editor/new", ["apps/web/cloud-project-new.html", "text/html"]],
  ["/editor/cloud-project-new.css", ["apps/web/cloud-project-new.css", "text/css"]],
  ["/editor/cloud-project-new-v1.mjs", ["apps/web/cloud-project-new-v1.mjs", "text/javascript"]],
  ["/editor/chaptera-cloud-workspace-session-v1.mjs", ["apps/web/chaptera-cloud-workspace-session-v1.mjs", "text/javascript"]],
  ["/editor/chaptera-cloud-source-ingress-v1.mjs", ["apps/web/chaptera-cloud-source-ingress-v1.mjs", "text/javascript"]],
  ["/editor/file-entry-v1.mjs", ["apps/web/file-entry-v1.mjs", "text/javascript"]],
]);
const csp = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'";
const calls = [];
let seenBytes = 0;
let pollCount = 0;

function json(res, status, value) {
  res.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(value));
}
function status(state, generation) {
  return { upload_id: UPLOAD, purpose: "pub_source", state, upload_generation: generation,
    expected_byte_len: 15, observed_byte_len: state === "ISSUED" ? null : 15 };
}
async function bytes(req) {
  const parts = [];
  for await (const chunk of req) parts.push(chunk);
  return Buffer.concat(parts);
}

const server = createServer(async (req, res) => {
  try {
    const path = new URL(req.url, "http://127.0.0.1").pathname;
    if (path.startsWith("/v1/")) {
      if (path === "/v1/auth/login") {
        res.writeHead(200, { "content-type": "text/html" });
        res.end("<!doctype html><title>Synthetic login</title>");
        return;
      }
      if (!String(req.headers.cookie ?? "").includes("chaptera-test-auth=valid")) {
        json(res, 401, { error: "session_missing" });
        return;
      }
      if (req.method !== "GET" && req.headers["x-csrf-token"] !== CSRF) {
        json(res, 403, { error: "csrf_invalid" });
        return;
      }
      calls.push({ method: req.method, path });
      if (path === "/v1/session" && req.method === "GET") {
        json(res, 200, { principal_id: "principal:synthetic", csrf_token: CSRF });
      } else if (path === "/v1/workspaces/personal" && req.method === "POST") {
        json(res, 200, { workspace_id: "workspace:personal:" + "b".repeat(64), role: "owner" });
      } else if (path === "/v1/uploads" && req.method === "POST") {
        const payload = JSON.parse((await bytes(req)).toString("utf8"));
        if (payload.purpose !== "pub_source" || payload.expected_byte_len !== 15 ||
            Object.hasOwn(payload, "tenant_id")) throw new Error("invalid upload wire shape");
        json(res, 200, { upload: status("ISSUED", 0),
          transport: { kind: "streamed", path: "/v1/uploads/" + UPLOAD + "/content" } });
      } else if (decodeURIComponent(path) === "/v1/uploads/" + UPLOAD + "/content" && req.method === "PUT") {
        const data = await bytes(req);
        seenBytes += data.length;
        if (data.length !== 15 || req.headers["content-length"] !== "15") {
          throw new Error("browser fetch did not send bounded File bytes and Content-Length");
        }
        json(res, 200, status("ISSUED", 0));
      } else if (decodeURIComponent(path) === "/v1/uploads/" + UPLOAD + "/complete" && req.method === "POST") {
        json(res, 200, status("VALIDATING", 1));
      } else if (decodeURIComponent(path) === "/v1/uploads/" + UPLOAD && req.method === "GET") {
        pollCount += 1;
        json(res, 200, status(pollCount === 1 ? "VALIDATING" : "VALIDATED_DURABLE",
          pollCount === 1 ? 1 : 2));
      } else if (path === "/v1/projects/from-upload" && req.method === "POST") {
        const payload = JSON.parse((await bytes(req)).toString("utf8"));
        if (payload.expected_upload_generation !== 2 || payload.name !== "Newsletter.pub" ||
            payload.upload_id !== UPLOAD) throw new Error("invalid project creation receipt");
        json(res, 200, { project_id: "project:" + "d".repeat(24), document_id: DOC,
          genesis_revision_id: "rev:genesis" });
      } else {
        json(res, 404, { error: "unexpected_synthetic_route" });
      }
      return;
    }
    if (decodeURIComponent(path) === "/editor/doc/" + DOC) {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end("<!doctype html><title>Synthetic document landing</title>");
      return;
    }
    const item = assets.get(path);
    if (!item) { res.writeHead(404); res.end("not found"); return; }
    const contents = await readFile(resolve(root, item[0]));
    res.writeHead(200, {
      "content-type": item[1],
      "content-security-policy": csp,
      "x-content-type-options": "nosniff",
    });
    res.end(contents);
  } catch (error) {
    res.writeHead(500, { "content-type": "text/plain" });
    res.end("harness failure: " + String(error));
  }
});
await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
const origin = "http://127.0.0.1:" + server.address().port;
const browser = await chromium.launch({ headless: true });
try {
  const anonymous = await browser.newPage();
  const anonymousErrors = [];
  const anonymousResponses = [];
  anonymous.on("pageerror", (error) => anonymousErrors.push(String(error)));
  anonymous.on("response", (response) => {
    if (response.status() >= 400) {
      anonymousResponses.push(response.status() + " " + response.url());
    }
  });
  await anonymous.goto(origin + "/editor/new");
  try {
    await anonymous.waitForURL((url) => url.pathname === "/v1/auth/login", {
      timeout: 6000, waitUntil: "domcontentloaded",
    });
  } catch (error) {
    const observed = await anonymous.evaluate(() => ({
      url: window.location.href,
      status: document.querySelector("#status")?.textContent ?? null,
      error: document.querySelector("#error")?.textContent ?? null,
      scripts: [...document.querySelectorAll("script[src]")].map(x => x.src),
    }));
    throw new Error("anonymous login redirect did not occur: " + JSON.stringify({
      observed, anonymousErrors, anonymousResponses, reason: String(error),
    }));
  }
  if (new URL(anonymous.url()).searchParams.get("return_path") !== "/editor/new") {
    throw new Error("anonymous entry did not preserve safe return path");
  }

  const context = await browser.newContext({ viewport: { width: 1024, height: 740 } });
  await context.addCookies([{ name: "chaptera-test-auth", value: "valid",
    url: origin, httpOnly: true, sameSite: "Lax" }]);
  const page = await context.newPage();
  const jsErrors = [];
  page.on("pageerror", (e) => jsErrors.push(String(e)));
  await page.goto(origin + "/editor/new");
  await page.getByRole("status").waitFor();
  await page.locator("#pub-file").setInputFiles({
    name: "Newsletter.pub",
    mimeType: "application/x-mspublisher",
    buffer: Buffer.from("Publisher-blob!", "utf8"), // 15 bytes, mock ingress only
  });
  await page.waitForURL((url) => decodeURIComponent(url.pathname) === "/editor/doc/" + DOC,
    { timeout: 15000 });
  if (jsErrors.length || seenBytes !== 15 || pollCount < 2) {
    throw new Error("browser file entry did not traverse API contract: " +
      JSON.stringify({ jsErrors, seenBytes, pollCount, calls }));
  }
  const expected = [
    "GET /v1/session", "POST /v1/workspaces/personal", "POST /v1/uploads",
    "PUT /v1/uploads/upload%3A" + "a".repeat(32) + "/content",
    "POST /v1/uploads/upload%3A" + "a".repeat(32) + "/complete",
    "GET /v1/uploads/upload%3A" + "a".repeat(32),
    "POST /v1/projects/from-upload",
  ];
  const got = calls.map(x => x.method + " " + x.path);
  for (const route of expected) {
    if (!got.includes(route)) throw new Error("missing browser HTTP request " + route);
  }
  const output = {
    schema: "chaptera.web-project-new.synthetic-http.v1",
    synthetic_backend: true,
    real_pub_validated: false,
    browser_file_picker: true,
    browser_same_origin_csrf: true,
    browser_content_length_15: true,
    personal_workspace_before_upload: true,
    redirected_to_persisted_document_id: DOC,
  };
  await mkdir("target/web-editor-product", { recursive: true });
  await writeFile("target/web-editor-product/chromium-project-new-synthetic.json",
    JSON.stringify(output, null, 2) + "\n");
  process.stdout.write(JSON.stringify(output) + "\n");
} finally {
  await browser.close();
  await new Promise((done) => server.close(done));
}
