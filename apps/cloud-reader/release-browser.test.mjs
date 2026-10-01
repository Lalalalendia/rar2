// Real Caddy + synthetic same-origin Chaptera + Chromium.
// Rust unit tests prove the same five files are embedded in the chaptera binary;
// this test proves the simplified edge wiring without scanner/BlobStore/TTL.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = resolve(process.env.READER_RELEASE_OUTPUT ?? join(root, "target/cloud-reader-release"));
const caddyBinary = process.env.READER_CADDY ?? "caddy";
const hash = (data) => createHash("sha256").update(data).digest("hex");
const assets = ["index.html", "reader.css", "reader-app.mjs", "reader-model.mjs", "render-v1.mjs"];
const fixtureBytes = Buffer.from("public synthetic release fixture");
const scene = {
  protocol_version: "chaptera.reader-scene.v1",
  fidelity: { state: "partial", reasons: ["text_layout_partial"] },
  pages: [{ page_id: "p", order: 0, width_emu: 3810000, height_emu: 4762500 }],
  nodes: [{ node_id: "n", page_id: "p", kind: "text", bounds: { x: 190500, y: 190500, width: 3429000, height: 952500 }, text: "Released Reader preview." }],
  stories: [{ story_id: "s", text: "Released Reader preview.", text_fidelity: "partial" }], resources: []
};
const seen = [];
let site;
const contentTypes = {
  "index.html": "text/html; charset=utf-8",
  "reader.css": "text/css; charset=utf-8",
  "reader-app.mjs": "text/javascript; charset=utf-8",
  "reader-model.mjs": "text/javascript; charset=utf-8",
  "render-v1.mjs": "text/javascript; charset=utf-8"
};
const api = createServer(async (request, response) => {
  try {
    const path = request.url.split("?", 1)[0];
    if (request.method === "GET" && site) {
      const name = path === "/" ? "index.html" : path.slice(1);
      if (assets.includes(name)) {
        const data = await readFile(join(site, name));
        response.writeHead(200, { "content-type": contentTypes[name], "cache-control": "no-cache" });
        response.end(data);
        return;
      }
    }
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    seen.push({ path: request.url, method: request.method, headers: request.headers, body });
    let payload;
    if (request.url === "/v1/reader/guest-sessions") payload = {
      protocol_version: "chaptera.reader-guest-session.v1", session_id: "guest:release", access_token: "synthetic-release-capability",
      upload_path: "/v1/reader/guest-sessions/guest:release/content", open_path: "/v1/reader/guest-sessions/guest:release/open"
    };
    else if (request.url.endsWith("/content")) payload = { protocol_version: "chaptera.reader-guest-session.v1", session_id: "guest:release", state: "uploaded" };
    else if (request.url.endsWith("/open")) payload = { protocol_version: "chaptera.reader-guest-session.v1", session_id: "guest:release", classification: "partial", scene };
    else payload = scene;
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    response.end(JSON.stringify(payload));
  } catch { response.destroy(); }
});
await new Promise((resolve) => api.listen(0, "127.0.0.1", resolve));
const portReservation = createServer();
await new Promise((resolve) => portReservation.listen(0, "127.0.0.1", resolve));
const port = portReservation.address().port;
await new Promise((resolve) => portReservation.close(resolve));
const origin = "http://127.0.0.1:" + port;
let caddy;
let browser;
let caddyLog = "";
try {
  await mkdir(output, { recursive: true });
  const commit = process.env.REPOSITORY_COMMIT_SHA ?? spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).stdout.trim();
  for (const folder of ["build-a", "build-b"]) {
    const build = spawnSync("python3", [join(root, "apps/cloud-reader/build_release.py"), "--commit", commit, "--output", join(output, folder)], { cwd: root, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr);
  }
  site = join(output, "build-a/site");
  const archive = await readFile(join(output, "build-a/cloud-reader.zip"));
  assert.deepEqual(archive, await readFile(join(output, "build-b/cloud-reader.zip")), "same committed assets must produce byte-identical releases");
  const manifest = JSON.parse(await readFile(join(site, "manifest.json"), "utf8"));
  assert.equal(manifest.source_commit, commit);
  assert.deepEqual(Object.keys(manifest.files).sort(), [...assets].sort());
  assert.deepEqual((await readdir(site)).sort(), [...assets, "manifest.json"].sort());
  for (const name of assets) {
    const data = await readFile(join(site, name));
    assert.equal(hash(data), manifest.files[name].sha256);
    assert.equal(data.length, manifest.files[name].byte_len);
  }
  const environment = { ...process.env, CHAPTERA_READER_SITE: origin,
    CHAPTERA_READER_API: "127.0.0.1:" + api.address().port };
  const config = join(root, "deploy/caddy/CloudReader.Caddyfile.example");
  const adapted = spawnSync(caddyBinary, ["adapt", "--config", config, "--adapter", "caddyfile"], { cwd: output, env: environment, encoding: "utf8" });
  assert.equal(adapted.status, 0, adapted.stderr);
  const bodyLimits = [];
  function collectLimits(value) {
    if (Array.isArray(value)) value.forEach(collectLimits);
    else if (value && typeof value === "object") {
      if (value.handler === "request_body") bodyLimits.push(value.max_size);
      Object.values(value).forEach(collectLimits);
    }
  }
  collectLimits(JSON.parse(adapted.stdout));
  assert.deepEqual(bodyLimits, [8 * 1024 * 1024], "Caddy MiB bound must match the Rust edge byte bound");
  const validation = spawnSync(caddyBinary, ["validate", "--config", config, "--adapter", "caddyfile"], { cwd: output, env: environment, encoding: "utf8" });
  assert.equal(validation.status, 0, validation.stderr);
  caddy = spawn(caddyBinary, ["run", "--config", config, "--adapter", "caddyfile"], { cwd: output, env: environment, stdio: ["ignore", "pipe", "pipe"] });
  caddy.on("error", (error) => { caddyLog += error.message; });
  for (const stream of [caddy.stdout, caddy.stderr]) stream.on("data", (data) => { caddyLog = (caddyLog + data).slice(-12000); });
  let response;
  for (let attempt = 0; attempt < 80; attempt++) {
    try { response = await fetch(origin); if (response.ok) break; } catch {}
    if (caddy.exitCode !== null) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.equal(response?.status, 200, caddyLog);
  assert.match(response.headers.get("content-security-policy"), /script-src 'self'/);
  assert.match(response.headers.get("content-security-policy"), /font-src data:/);
  assert.equal(response.headers.get("x-content-type-options"), "nosniff");
  assert.equal(response.headers.get("referrer-policy"), "no-referrer");
  assert.equal(response.headers.get("server"), null);
  for (const name of assets) {
    const result = await fetch(origin + "/" + name);
    assert.equal(result.status, 200);
    assert.equal(hash(Buffer.from(await result.arrayBuffer())), manifest.files[name].sha256);
  }
  const before = seen.length;
  const notReader = await fetch(origin + "/v1/projects/blocked");
  assert.equal(notReader.status, 404);
  assert.equal(seen.length, before);
  await fetch(origin + "/v1/reader/documents/synthetic/scene", { headers: { "x-forwarded-for": "198.51.100.10", "x-forwarded-host": "foreign.example", "x-forwarded-proto": "http" } });
  assert.equal(seen.at(-1).headers["x-forwarded-for"], "127.0.0.1");
  assert.equal(seen.at(-1).headers["x-forwarded-proto"], "https");
  assert.equal(seen.at(-1).headers["x-forwarded-host"], "127.0.0.1");
  browser = await chromium.launch({ headless: true, ...(process.env.READER_UI_BROWSER ? { executablePath: process.env.READER_UI_BROWSER } : {}) });
  const page = await browser.newPage({ viewport: { width: 1000, height: 800 } });
  const errors = [];
  const foreign = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => { if (!request.url().startsWith(origin + "/")) foreign.push(request.url()); });
  await page.addInitScript(() => {
    window.__policyViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => window.__policyViolations.push(event.violatedDirective));
  });
  await page.goto(origin);
  await page.locator("#pub-file").setInputFiles({ name: "synthetic.pub", mimeType: "application/octet-stream", buffer: fixtureBytes });
  await page.locator("#open-file").click();
  await page.waitForFunction(() => document.querySelector("#status").textContent.startsWith("Opened with display limitations"));
  assert.equal(await page.locator("#pages svg").count(), 1);
  assert.equal(await page.locator("#story-text").inputValue(), "Released Reader preview.");
  const visibleHeight = await page.locator("foreignObject").evaluate((element) => {
    const range = document.createRange(); range.selectNodeContents(element.firstElementChild); return range.getBoundingClientRect().height;
  });
  assert.ok(visibleHeight >= 8);
  assert.deepEqual(errors, []);
  assert.deepEqual(foreign, []);
  assert.deepEqual(await page.evaluate(() => window.__policyViolations), []);
  const upload = seen.find((request) => request.method === "PUT");
  assert.deepEqual(upload.body, fixtureBytes);
  assert.equal(upload.headers["x-chaptera-reader-session"], "synthetic-release-capability");
  const oversized = await fetch(origin + "/v1/reader/guest-sessions/guest:release/content", {
    method: "PUT", body: Buffer.alloc(9 * 1024 * 1024), headers: { "content-type": "application/octet-stream" }
  });
  assert.equal(oversized.status, 413, "edge must reject an oversized Reader body");
  await page.screenshot({ path: join(output, "released-reader.png"), fullPage: true });
  const version = spawnSync(caddyBinary, ["version"], { encoding: "utf8" }).stdout.trim();
  const receipt = { protocol: "chaptera.cloud-reader-one-binary-edge.v1", scope: "real Caddy/Chromium with synthetic same-origin Chaptera; Rust tests own embedded bytes; excludes TLS, live scanner/isolation/BlobStore/TTL/consent and production host acceptance",
    source_commit: commit, artifact_sha256: hash(archive), asset_count: assets.length, byte_identical_rebuild: true,
    caddy: version, browser: await browser.version(), all_assets_served_with_matching_hash: true,
    trusted_proxy_headers_collapsed: true, non_reader_api_blocked: true, reader_body_limit_bytes: bodyLimits[0], oversized_body_rejected: true,
    opened_with_csp: true, browser_errors: errors, foreign_requests: foreign };
  await writeFile(join(output, "edge-receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
  console.log(JSON.stringify({ artifact_sha256: receipt.artifact_sha256, byte_identical_rebuild: true, opened_with_csp: true }));
} finally {
  if (browser) await browser.close();
  if (caddy && caddy.exitCode === null) {
    caddy.kill("SIGTERM");
    await new Promise((resolve) => caddy.once("exit", resolve));
  }
  api.closeAllConnections();
  await new Promise((resolve) => api.close(resolve));
}
